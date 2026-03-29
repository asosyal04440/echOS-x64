use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use lazy_static::lazy_static;
use spin::Mutex;
use x86_64::structures::paging::PageTableFlags;

use super::{
    address_space_id, allocate_user_mmap_in, register_shared_anon_region_in, AddressSpace,
};

pub type SharedRegionId = u64;
pub type SharedRegionGeneration = u64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedRegionLease {
    pub id: SharedRegionId,
    pub owner_pid: u64,
    pub owner_space_id: Option<u64>,
    pub name: String,
    pub len: u64,
    pub generation: SharedRegionGeneration,
    pub backing_shared_id: u64,
    pub writable: bool,
    pub mapped_pids: Vec<u64>,
    pub mapped_vaddrs: BTreeMap<u64, u64>,
    pub revoked: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserMapping {
    pub region_id: SharedRegionId,
    pub base: u64,
    pub generation: SharedRegionGeneration,
    pub len: u64,
    pub writable: bool,
}

static NEXT_SHARED_REGION_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_SHARED_BACKING_ID: AtomicU64 = AtomicU64::new(1);

lazy_static! {
    static ref SHARED_REGIONS: Mutex<BTreeMap<SharedRegionId, SharedRegionLease>> =
        Mutex::new(BTreeMap::new());
}

pub fn create_ipc_region(
    owner_pid: u64,
    name: &str,
    len: u64,
    writable: bool,
) -> SharedRegionLease {
    let region = SharedRegionLease {
        id: NEXT_SHARED_REGION_ID.fetch_add(1, Ordering::Relaxed),
        owner_pid,
        owner_space_id: None,
        name: String::from(name),
        len,
        generation: 1,
        backing_shared_id: NEXT_SHARED_BACKING_ID.fetch_add(1, Ordering::Relaxed),
        writable,
        mapped_pids: Vec::new(),
        mapped_vaddrs: BTreeMap::new(),
        revoked: false,
    };
    SHARED_REGIONS.lock().insert(region.id, region.clone());
    region
}

pub fn snapshot_ipc_region(region_id: SharedRegionId) -> Option<SharedRegionLease> {
    SHARED_REGIONS.lock().get(&region_id).cloned()
}

pub fn map_ipc_region(pid: u64, region_id: SharedRegionId) -> Option<UserMapping> {
    let mut regions = SHARED_REGIONS.lock();
    let region = regions.get_mut(&region_id)?;
    if region.revoked {
        return None;
    }
    if !region.mapped_pids.iter().any(|mapped| *mapped == pid) {
        region.mapped_pids.push(pid);
    }
    Some(UserMapping {
        region_id,
        base: region.mapped_vaddrs.get(&pid).copied().unwrap_or(0),
        generation: region.generation,
        len: region.len,
        writable: region.writable,
    })
}

pub fn map_ipc_region_into_space(
    pid: u64,
    region_id: SharedRegionId,
    space: &Arc<Mutex<AddressSpace>>,
) -> Option<UserMapping> {
    let (len, writable, generation, backing_shared_id, existing_base) = {
        let regions = SHARED_REGIONS.lock();
        let region = regions.get(&region_id)?;
        if region.revoked {
            return None;
        }
        (
            region.len,
            region.writable,
            region.generation,
            region.backing_shared_id,
            region.mapped_vaddrs.get(&pid).copied(),
        )
    };

    let base = if let Some(base) = existing_base {
        base
    } else {
        let flags = if writable {
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE
        } else {
            PageTableFlags::PRESENT
        };
        let base = allocate_user_mmap_in(space, len)?;
        register_shared_anon_region_in(space, base, len, flags, Some(backing_shared_id))?;
        let mut regions = SHARED_REGIONS.lock();
        let region = regions.get_mut(&region_id)?;
        if region.revoked || region.generation != generation {
            return None;
        }
        if !region.mapped_pids.iter().any(|mapped| *mapped == pid) {
            region.mapped_pids.push(pid);
        }
        region.mapped_vaddrs.insert(pid, base);
        if pid == region.owner_pid && region.owner_space_id.is_none() {
            region.owner_space_id = Some(address_space_id(space));
        }
        base
    };

    Some(UserMapping {
        region_id,
        base,
        generation,
        len,
        writable,
    })
}

pub fn unmap_ipc_region(pid: u64, region_id: SharedRegionId) -> bool {
    let mut regions = SHARED_REGIONS.lock();
    let Some(region) = regions.get_mut(&region_id) else {
        return false;
    };
    if region.revoked {
        return false;
    }
    region
        .mapped_pids
        .retain(|mapped| *mapped != pid || *mapped == region.owner_pid);
    if pid != region.owner_pid {
        region.mapped_vaddrs.remove(&pid);
    }
    true
}

pub fn bump_ipc_region_generation(region_id: SharedRegionId) -> Option<SharedRegionLease> {
    let mut regions = SHARED_REGIONS.lock();
    let region = regions.get_mut(&region_id)?;
    region.generation = region.generation.saturating_add(1).max(1);
    Some(region.clone())
}

pub fn revoke_ipc_region(region_id: SharedRegionId) -> bool {
    let mut regions = SHARED_REGIONS.lock();
    let Some(region) = regions.get_mut(&region_id) else {
        return false;
    };
    region.revoked = true;
    region.generation = region.generation.saturating_add(1).max(1);
    region.mapped_pids.clear();
    region.mapped_vaddrs.clear();
    true
}

pub fn region_generation_matches(
    region_id: SharedRegionId,
    generation: SharedRegionGeneration,
) -> bool {
    SHARED_REGIONS
        .lock()
        .get(&region_id)
        .map(|region| !region.revoked && region.generation == generation)
        .unwrap_or(false)
}
