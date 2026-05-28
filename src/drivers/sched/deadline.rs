use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::cmp::Ordering;
use core::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use crate::drivers::block::{BlockDevice, BlockDeviceError};

pub type BioId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BioOp {
    Read,
    Write,
    Flush,
    Discard,
}

#[derive(Debug, Clone)]
pub struct DeadlineBio {
    pub id: BioId,
    pub op: BioOp,
    pub lba: u64,
    pub count: u32,
    pub data: Option<Vec<u8>>,
    pub deadline: u64,
    pub submit_ticks: u64,
    pub completed: bool,
    pub error: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchDir {
    Reads,
    Writes,
}

pub struct DeadlineQueue {
    read_rb: Vec<BioId>,
    write_rb: Vec<BioId>,
    read_fifo: VecDeque<BioId>,
    write_fifo: VecDeque<BioId>,
    bio_map: BTreeMap<BioId, DeadlineBio>,

    read_expire: u64,
    write_expire: u64,
    writes_starved: u32,
    fifo_batch: u32,
    front_merges: bool,

    starved_count: u32,
    dispatch_dir: DispatchDir,
    batch_count: u32,
    last_lba: u64,

    next_id: AtomicU64,
    ticks: u64,
}

impl DeadlineQueue {
    pub fn new(read_expire: u64, write_expire: u64, writes_starved: u32, fifo_batch: u32) -> Self {
        Self {
            read_rb: Vec::new(),
            write_rb: Vec::new(),
            read_fifo: VecDeque::new(),
            write_fifo: VecDeque::new(),
            bio_map: BTreeMap::new(),
            read_expire,
            write_expire,
            writes_starved,
            fifo_batch,
            front_merges: true,
            starved_count: 0,
            dispatch_dir: DispatchDir::Reads,
            batch_count: 0,
            last_lba: 0,
            next_id: AtomicU64::new(1),
            ticks: 0,
        }
    }

    pub fn next_id(&self) -> BioId {
        self.next_id.fetch_add(1, AtomicOrdering::Relaxed)
    }

    fn lba_cmp(a: &DeadlineBio, b: &DeadlineBio) -> Ordering {
        a.lba.cmp(&b.lba)
    }

    fn insert_sorted(rb: &mut Vec<BioId>, id: BioId, bio_map: &BTreeMap<BioId, DeadlineBio>) {
        let bio = &bio_map[&id];
        let pos = rb.binary_search_by(|&candidate_id| {
            let candidate = &bio_map[&candidate_id];
            Self::lba_cmp(bio, candidate)
        });
        match pos {
            Ok(p) | Err(p) => rb.insert(p, id),
        }
    }

    fn remove_sorted(rb: &mut Vec<BioId>, id: BioId) {
        if let Some(pos) = rb.iter().position(|&x| x == id) {
            rb.remove(pos);
        }
    }

    pub fn add_request(&mut self, op: BioOp, lba: u64, count: u32, data: Option<Vec<u8>>) -> BioId {
        if self.front_merges {
            let end = lba + count as u64;
            if let Some(&last) = self.read_rb.last() {
                if let Some(bio) = self.bio_map.get(&last) {
                    let bio_end = bio.lba + bio.count as u64;
                    if bio_end == lba {
                    }
                }
            }
            if let Some(&last) = self.write_rb.last() {
                if let Some(bio) = self.bio_map.get(&last) {
                    let bio_end = bio.lba + bio.count as u64;
                    if bio_end == lba {
                    }
                }
            }
        }

        let id = self.next_id();
        let deadline = self.ticks + match op {
            BioOp::Read => self.read_expire,
            BioOp::Write => self.write_expire,
            BioOp::Flush => 0,
            BioOp::Discard => self.write_expire,
        };

        let bio = DeadlineBio {
            id,
            op,
            lba,
            count,
            data,
            deadline,
            submit_ticks: self.ticks,
            completed: false,
            error: false,
        };

        match op {
            BioOp::Read => {
                Self::insert_sorted(&mut self.read_rb, id, &self.bio_map);
                self.read_fifo.push_back(id);
            }
            BioOp::Write | BioOp::Discard => {
                Self::insert_sorted(&mut self.write_rb, id, &self.bio_map);
                self.write_fifo.push_back(id);
            }
            BioOp::Flush => {
                self.read_fifo.push_back(id);
                self.write_fifo.push_back(id);
            }
        }

        self.bio_map.insert(id, bio);
        id
    }

    pub fn dispatch(&mut self, budget: u32) -> Vec<BioId> {
        let mut dispatched = Vec::new();
        let mut remaining = budget;

        while remaining > 0 {
            let dir = self.choose_dir();
            let batch = core::cmp::min(self.fifo_batch, remaining);

            let (rb, fifo) = match dir {
                DispatchDir::Reads => (&mut self.read_rb, &mut self.read_fifo),
                DispatchDir::Writes => (&mut self.write_rb, &mut self.write_fifo),
            };

            let mut taken = 0u32;
            while taken < batch && !rb.is_empty() && !fifo.is_empty() {
                let id = match dir {
                    DispatchDir::Reads => rb.remove(0),
                    DispatchDir::Writes => {
                        let id = rb.remove(0);
                        id
                    }
                };
                if let Some(fifo_pos) = fifo.iter().position(|&x| x == id) {
                    fifo.remove(fifo_pos);
                }
                dispatched.push(id);
                taken += 1;
                remaining -= 1;
            }

            if taken > 0 {
                self.batch_count = taken;
                self.dispatch_dir = match dir {
                    DispatchDir::Reads => DispatchDir::Writes,
                    DispatchDir::Writes => DispatchDir::Reads,
                };
                if dir == DispatchDir::Writes {
                    self.starved_count = 0;
                }
            } else {
                self.batch_count = 0;
                break;
            }
        }

        dispatched
    }

    fn choose_dir(&self) -> DispatchDir {
        if self.has_expired_reads() {
            return DispatchDir::Reads;
        }

        match self.dispatch_dir {
            DispatchDir::Reads => {
                if self.batch_count >= self.fifo_batch || self.starved_count < self.writes_starved {
                    DispatchDir::Writes
                } else {
                    DispatchDir::Reads
                }
            }
            DispatchDir::Writes => {
                if self.batch_count >= self.fifo_batch || self.starved_count >= self.writes_starved {
                    DispatchDir::Reads
                } else {
                    DispatchDir::Writes
                }
            }
        }
    }

    fn has_expired_reads(&self) -> bool {
        self.read_fifo.front().map_or(false, |id| {
            self.bio_map.get(id).map_or(false, |bio| bio.deadline <= self.ticks)
        })
    }

    pub fn complete(&mut self, id: BioId, error: bool) {
        if let Some(bio) = self.bio_map.get_mut(&id) {
            bio.completed = true;
            bio.error = error;
        }
    }

    pub fn remove_bio(&mut self, id: BioId) {
        Self::remove_sorted(&mut self.read_rb, id);
        Self::remove_sorted(&mut self.write_rb, id);
        if let Some(pos) = self.read_fifo.iter().position(|&x| x == id) {
            self.read_fifo.remove(pos);
        }
        if let Some(pos) = self.write_fifo.iter().position(|&x| x == id) {
            self.write_fifo.remove(pos);
        }
        self.bio_map.remove(&id);
    }

    pub fn tick(&mut self) {
        self.ticks = self.ticks.wrapping_add(1);
    }

    pub fn pending(&self) -> usize {
        self.bio_map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bio_map.is_empty()
    }

    pub fn execute_io(&mut self, device: &mut dyn BlockDevice, id: BioId) -> Result<(), BlockDeviceError> {
        let bio = match self.bio_map.get(&id) {
            Some(b) => b.clone(),
            None => return Ok(()),
        };

        let result = match bio.op {
            BioOp::Read => {
                let buf_len = (bio.count as u64 * device.block_size() as u64) as usize;
                let mut buf = alloc::vec::from_elem(0u8, buf_len);
                device.read_block(bio.lba, &mut buf)?;
                if let Some(b) = self.bio_map.get_mut(&id) {
                    b.data = Some(buf);
                }
                Ok(())
            }
            BioOp::Write => {
                if let Some(ref data) = bio.data {
                    device.write_block(bio.lba, data)?;
                }
                Ok(())
            }
            BioOp::Flush => device.flush(),
            BioOp::Discard => Ok(()),
        };

        if let Some(b) = self.bio_map.get_mut(&id) {
            b.completed = true;
            b.error = result.is_err();
        }

        result
    }
}
