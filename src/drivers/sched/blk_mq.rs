use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::drivers::block::{BlockDevice, BlockDeviceError};
use crate::drivers::sched::deadline::{BioOp, DeadlineQueue};

pub type BioId = u64;

#[derive(Debug, Clone)]
pub struct Bio {
    pub id: BioId,
    pub op: BioOp,
    pub lba: u64,
    pub count: u32,
    pub data: Option<Vec<u8>>,
    pub error: bool,
    pub completed: bool,
}

#[derive(Debug)]
pub struct BlkMqSwQueue {
    pending: Vec<BioId>,
    lock: Mutex<()>,
}

#[derive(Debug)]
pub struct BlkMqHwQueue {
    in_flight: BTreeMap<BioId, Bio>,
    dispatch: VecDeque<BioId>,
    lock: Mutex<()>,
}

pub struct BlkMq {
    num_queues: u32,
    queue_depth: u32,
    software_queues: Vec<BlkMqSwQueue>,
    hardware_queues: Vec<BlkMqHwQueue>,
    scheduler: DeadlineQueue,
    next_id: AtomicU64,
    device: Option<Mutex<Box<dyn BlockDevice>>>,
}

impl BlkMq {
    pub fn new(
        num_queues: u32,
        queue_depth: u32,
        read_expire: u64,
        write_expire: u64,
        device: Option<Box<dyn BlockDevice>>,
    ) -> Self {
        let sw_queues = (0..num_queues)
            .map(|_| BlkMqSwQueue {
                pending: Vec::new(),
                lock: Mutex::new(()),
            })
            .collect();

        let hw_queues = (0..num_queues)
            .map(|_| BlkMqHwQueue {
                in_flight: BTreeMap::new(),
                dispatch: VecDeque::new(),
                lock: Mutex::new(()),
            })
            .collect();

        Self {
            num_queues,
            queue_depth,
            software_queues: sw_queues,
            hardware_queues: hw_queues,
            scheduler: DeadlineQueue::new(read_expire, write_expire, 3, 16),
            next_id: AtomicU64::new(1),
            device: device.map(|d| Mutex::new(d)),
        }
    }

    fn next_id(&self) -> BioId {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    fn sw_hash(&self, lba: u64) -> usize {
        (lba as usize) % self.num_queues as usize
    }

    fn hw_hash(&self, lba: u64) -> usize {
        (lba as usize) % self.num_queues as usize
    }

    pub fn submit_bio(&mut self, op: BioOp, lba: u64, count: u32, data: Option<Vec<u8>>) -> BioId {
        let sw_idx = self.sw_hash(lba);

        let scheduler_id = self.scheduler.add_request(op, lba, count, data);

        let bio = Bio {
            id: scheduler_id,
            op,
            lba,
            count,
            data: None,
            error: false,
            completed: false,
        };

        let mut sw_q = &mut self.software_queues[sw_idx];
        let _guard = sw_q.lock.lock();
        sw_q.pending.push(scheduler_id);
        drop(_guard);

        let hw_idx = self.hw_hash(lba);
        let mut hw_q = &mut self.hardware_queues[hw_idx];
        let _guard = hw_q.lock.lock();
        hw_q.in_flight.insert(scheduler_id, bio);
        drop(_guard);

        scheduler_id
    }

    pub fn dispatch(&mut self) -> Vec<BioId> {
        for sw_q in &mut self.software_queues {
            let _guard = sw_q.lock.lock();
            while let Some(id) = sw_q.pending.pop() {
            }
            drop(_guard);
        }

        self.scheduler.dispatch(16)
    }

    pub fn complete_bio(&mut self, id: BioId, error: bool) {
        self.scheduler.complete(id, error);
        for hw_q in &mut self.hardware_queues {
            let _guard = hw_q.lock.lock();
            if let Some(bio) = hw_q.in_flight.get_mut(&id) {
                bio.completed = true;
                bio.error = error;
            }
            drop(_guard);
        }
        self.scheduler.remove_bio(id);
    }

    pub fn execute_bio(&mut self, id: BioId) -> Result<(), BlockDeviceError> {
        if self.device.is_none() {
            return Err(BlockDeviceError::DeviceBusy);
        }
        let mut dev = self.device.as_ref().unwrap().lock();
        self.scheduler.execute_io(&mut **dev, id)
    }

    pub fn tick(&mut self) {
        self.scheduler.tick();
    }

    pub fn pending(&self) -> usize {
        self.scheduler.pending()
    }

    pub fn set_device(&mut self, device: Box<dyn BlockDevice>) {
        self.device = Some(Mutex::new(device));
    }
}
