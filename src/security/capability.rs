//! Capability-based security primitives plus the user-visible handle model used by IPC v2.

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

use lazy_static::lazy_static;
use spin::Mutex;

pub type CapId = u64;
pub type UserHandle = u32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CapabilityError {
    ProcessNotInitialized,
    InvalidHandle,
    Revoked,
    RightsDenied,
    WrongKind,
    StaleGeneration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapRights {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
    pub share: bool,
    pub transfer: bool,
}

impl CapRights {
    pub const NONE: Self = CapRights {
        read: false,
        write: false,
        execute: false,
        share: false,
        transfer: false,
    };
    pub const READ: Self = CapRights {
        read: true,
        write: false,
        execute: false,
        share: false,
        transfer: false,
    };
    pub const WRITE: Self = CapRights {
        read: false,
        write: true,
        execute: false,
        share: false,
        transfer: false,
    };
    pub const READ_WRITE: Self = CapRights {
        read: true,
        write: true,
        execute: false,
        share: false,
        transfer: false,
    };
    pub const ALL: Self = CapRights {
        read: true,
        write: true,
        execute: true,
        share: true,
        transfer: true,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceType {
    File,
    Directory,
    Socket,
    Device,
    Memory,
    Process,
    Thread,
    Port,
    Key,
    Service,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceCapabilityRecord {
    pub service_id: u64,
    pub endpoint_generation: u32,
    pub owner_endpoint: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedRegionCapabilityRecord {
    pub region_id: u64,
    pub region_generation: u64,
    pub len: u64,
    pub writable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestCapabilityRecord {
    pub request_id: u64,
    pub service_id: u64,
    pub endpoint_generation: u32,
    pub owner_endpoint: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CapabilityKind {
    Generic,
    Service(ServiceCapabilityRecord),
    SharedRegion(SharedRegionCapabilityRecord),
    Request(RequestCapabilityRecord),
}

#[derive(Clone, Debug)]
pub struct Capability {
    pub id: CapId,
    pub resource_type: ResourceType,
    pub resource_id: u64,
    pub rights: CapRights,
    pub owner: u64,
    pub generation: u32,
    pub user_handle: UserHandle,
    pub handle_generation: u32,
    pub kind: CapabilityKind,
    pub children: Vec<CapId>,
}

#[derive(Clone, Debug)]
pub struct CapabilityTable {
    pub process_id: u64,
    pub capabilities: BTreeMap<CapId, Capability>,
    pub handles: BTreeMap<UserHandle, CapId>,
    pub next_cap_id: CapId,
    pub next_handle: UserHandle,
}

impl CapabilityTable {
    pub fn new(process_id: u64) -> Self {
        CapabilityTable {
            process_id,
            capabilities: BTreeMap::new(),
            handles: BTreeMap::new(),
            next_cap_id: 1,
            next_handle: 1,
        }
    }

    fn allocate_handle(&mut self) -> UserHandle {
        let handle = self.next_handle.max(1);
        self.next_handle = self.next_handle.saturating_add(1).max(1);
        handle
    }

    pub fn create(
        &mut self,
        resource_type: ResourceType,
        resource_id: u64,
        rights: CapRights,
    ) -> CapId {
        self.create_with_kind(resource_type, resource_id, rights, CapabilityKind::Generic)
    }

    pub fn create_with_kind(
        &mut self,
        resource_type: ResourceType,
        resource_id: u64,
        rights: CapRights,
        kind: CapabilityKind,
    ) -> CapId {
        let id = self.next_cap_id;
        self.next_cap_id += 1;
        let handle = self.allocate_handle();

        let cap = Capability {
            id,
            resource_type,
            resource_id,
            rights,
            owner: self.process_id,
            generation: 0,
            user_handle: handle,
            handle_generation: 1,
            kind,
            children: Vec::new(),
        };

        self.handles.insert(handle, id);
        self.capabilities.insert(id, cap);
        id
    }

    pub fn get(&self, id: CapId) -> Option<&Capability> {
        self.capabilities.get(&id)
    }

    pub fn get_by_handle(&self, handle: UserHandle) -> Option<&Capability> {
        let cap_id = self.handles.get(&handle)?;
        self.capabilities.get(cap_id)
    }

    pub fn check(&self, id: CapId, required: CapRights) -> bool {
        if let Some(cap) = self.capabilities.get(&id) {
            let r = cap.rights;
            (!required.read || r.read)
                && (!required.write || r.write)
                && (!required.execute || r.execute)
                && (!required.share || r.share)
                && (!required.transfer || r.transfer)
        } else {
            false
        }
    }

    pub fn derive(&mut self, parent_id: CapId, subset_rights: CapRights) -> Option<CapId> {
        let parent = self.capabilities.get(&parent_id)?.clone();
        if subset_rights.read && !parent.rights.read {
            return None;
        }
        if subset_rights.write && !parent.rights.write {
            return None;
        }
        if subset_rights.execute && !parent.rights.execute {
            return None;
        }
        if subset_rights.share && !parent.rights.share {
            return None;
        }
        if subset_rights.transfer && !parent.rights.transfer {
            return None;
        }

        let child_id = self.next_cap_id;
        self.next_cap_id += 1;
        let handle = self.allocate_handle();
        let child = Capability {
            id: child_id,
            resource_type: parent.resource_type,
            resource_id: parent.resource_id,
            rights: subset_rights,
            owner: self.process_id,
            generation: parent.generation + 1,
            user_handle: handle,
            handle_generation: parent.handle_generation.saturating_add(1).max(1),
            kind: parent.kind.clone(),
            children: Vec::new(),
        };

        self.capabilities
            .get_mut(&parent_id)?
            .children
            .push(child_id);
        self.handles.insert(handle, child_id);
        self.capabilities.insert(child_id, child);
        Some(child_id)
    }

    pub fn revoke(&mut self, id: CapId) -> bool {
        if let Some(cap) = self.capabilities.remove(&id) {
            self.handles.remove(&cap.user_handle);
            for child_id in cap.children {
                self.revoke(child_id);
            }
            true
        } else {
            false
        }
    }

    pub fn revoke_handle(&mut self, handle: UserHandle) -> bool {
        let Some(cap_id) = self.handles.get(&handle).copied() else {
            return false;
        };
        self.revoke(cap_id)
    }

    pub fn transfer(&mut self, id: CapId, target_pid: u64) -> Option<Capability> {
        let cap = self.capabilities.remove(&id)?;
        self.handles.remove(&cap.user_handle);
        if !cap.rights.transfer {
            self.handles.insert(cap.user_handle, id);
            self.capabilities.insert(id, cap);
            return None;
        }

        let mut transferred = cap.clone();
        transferred.owner = target_pid;
        transferred.generation = transferred.generation.saturating_add(1);
        Some(transferred)
    }

    pub fn insert_transferred_capability(&mut self, mut cap: Capability) -> UserHandle {
        let handle = self.allocate_handle();
        cap.owner = self.process_id;
        cap.user_handle = handle;
        cap.handle_generation = cap.handle_generation.saturating_add(1).max(1);
        self.handles.insert(handle, cap.id);
        self.capabilities.insert(cap.id, cap);
        handle
    }
}

lazy_static! {
    static ref CAP_TABLES: Mutex<BTreeMap<u64, CapabilityTable>> = Mutex::new(BTreeMap::new());
}

fn ensure_process_inner(
    tables: &mut BTreeMap<u64, CapabilityTable>,
    pid: u64,
) -> &mut CapabilityTable {
    tables
        .entry(pid)
        .or_insert_with(|| CapabilityTable::new(pid))
}

pub fn init_process(pid: u64) {
    let mut tables = CAP_TABLES.lock();
    let _ = ensure_process_inner(&mut tables, pid);
}

pub fn get_table(pid: u64) -> Option<CapabilityTable> {
    CAP_TABLES.lock().get(&pid).cloned()
}

pub fn create_capability(
    pid: u64,
    resource_type: ResourceType,
    resource_id: u64,
    rights: CapRights,
) -> Option<CapId> {
    let mut tables = CAP_TABLES.lock();
    let table = tables.get_mut(&pid)?;
    Some(table.create(resource_type, resource_id, rights))
}

pub fn check_capability(pid: u64, cap_id: CapId, rights: CapRights) -> bool {
    let tables = CAP_TABLES.lock();
    tables
        .get(&pid)
        .map(|table| table.check(cap_id, rights))
        .unwrap_or(false)
}

pub fn derive_capability(pid: u64, parent_id: CapId, subset_rights: CapRights) -> Option<CapId> {
    let mut tables = CAP_TABLES.lock();
    let table = tables.get_mut(&pid)?;
    table.derive(parent_id, subset_rights)
}

pub fn revoke_capability(pid: u64, cap_id: CapId) -> bool {
    let mut tables = CAP_TABLES.lock();
    tables
        .get_mut(&pid)
        .map(|table| table.revoke(cap_id))
        .unwrap_or(false)
}

pub fn transfer_capability(from_pid: u64, cap_id: CapId, to_pid: u64) -> bool {
    let mut tables = CAP_TABLES.lock();
    let transferred = {
        let Some(table) = tables.get_mut(&from_pid) else {
            return false;
        };
        table.transfer(cap_id, to_pid)
    };

    if let Some(cap) = transferred {
        let target = ensure_process_inner(&mut tables, to_pid);
        let _ = target.insert_transferred_capability(cap);
        return true;
    }
    false
}

pub fn cleanup_process(pid: u64) {
    CAP_TABLES.lock().remove(&pid);
}

pub fn seal_capability(pid: u64, cap_id: CapId) -> bool {
    let tables = CAP_TABLES.lock();
    tables
        .get(&pid)
        .and_then(|table| table.capabilities.get(&cap_id))
        .map(|cap| !cap.rights.transfer)
        .unwrap_or(false)
}

pub fn open_service_handle(
    pid: u64,
    service_id: u64,
    rights: CapRights,
    endpoint_generation: u32,
    owner_endpoint: Option<u64>,
) -> Result<UserHandle, CapabilityError> {
    let mut tables = CAP_TABLES.lock();
    let table = ensure_process_inner(&mut tables, pid);
    let cap_id = table.create_with_kind(
        ResourceType::Service,
        service_id,
        rights,
        CapabilityKind::Service(ServiceCapabilityRecord {
            service_id,
            endpoint_generation,
            owner_endpoint,
        }),
    );
    Ok(table
        .get(cap_id)
        .map(|cap| cap.user_handle)
        .expect("service handle capability must exist"))
}

pub fn grant_request_handle(
    pid: u64,
    request_id: u64,
    service_id: u64,
    endpoint_generation: u32,
    owner_endpoint: Option<u64>,
) -> Result<UserHandle, CapabilityError> {
    let mut tables = CAP_TABLES.lock();
    let table = ensure_process_inner(&mut tables, pid);
    let cap_id = table.create_with_kind(
        ResourceType::Service,
        service_id,
        CapRights::READ_WRITE,
        CapabilityKind::Request(RequestCapabilityRecord {
            request_id,
            service_id,
            endpoint_generation,
            owner_endpoint,
        }),
    );
    Ok(table
        .get(cap_id)
        .map(|cap| cap.user_handle)
        .expect("request handle capability must exist"))
}

pub fn grant_shared_region_handle(
    pid: u64,
    region_id: u64,
    region_generation: u64,
    len: u64,
    writable: bool,
) -> Result<UserHandle, CapabilityError> {
    let mut tables = CAP_TABLES.lock();
    let table = ensure_process_inner(&mut tables, pid);
    let rights = if writable {
        CapRights::READ_WRITE
    } else {
        CapRights::READ
    };
    let cap_id = table.create_with_kind(
        ResourceType::Memory,
        region_id,
        rights,
        CapabilityKind::SharedRegion(SharedRegionCapabilityRecord {
            region_id,
            region_generation,
            len,
            writable,
        }),
    );
    Ok(table
        .get(cap_id)
        .map(|cap| cap.user_handle)
        .expect("shared-region capability must exist"))
}

pub fn revoke_handle(pid: u64, handle: UserHandle) -> Result<(), CapabilityError> {
    let mut tables = CAP_TABLES.lock();
    let table = tables
        .get_mut(&pid)
        .ok_or(CapabilityError::ProcessNotInitialized)?;
    if table.revoke_handle(handle) {
        Ok(())
    } else {
        Err(CapabilityError::InvalidHandle)
    }
}

fn require_rights(cap: &Capability, rights: CapRights) -> Result<(), CapabilityError> {
    let granted = cap.rights;
    if (rights.read && !granted.read)
        || (rights.write && !granted.write)
        || (rights.execute && !granted.execute)
        || (rights.share && !granted.share)
        || (rights.transfer && !granted.transfer)
    {
        Err(CapabilityError::RightsDenied)
    } else {
        Ok(())
    }
}

pub fn resolve_service_handle(
    pid: u64,
    handle: UserHandle,
    rights: CapRights,
) -> Result<ServiceCapabilityRecord, CapabilityError> {
    let tables = CAP_TABLES.lock();
    let table = tables
        .get(&pid)
        .ok_or(CapabilityError::ProcessNotInitialized)?;
    let cap = table
        .get_by_handle(handle)
        .ok_or(CapabilityError::InvalidHandle)?;
    require_rights(cap, rights)?;
    match &cap.kind {
        CapabilityKind::Service(record) => Ok(record.clone()),
        _ => Err(CapabilityError::WrongKind),
    }
}

pub fn resolve_request_handle(
    pid: u64,
    handle: UserHandle,
) -> Result<RequestCapabilityRecord, CapabilityError> {
    let tables = CAP_TABLES.lock();
    let table = tables
        .get(&pid)
        .ok_or(CapabilityError::ProcessNotInitialized)?;
    let cap = table
        .get_by_handle(handle)
        .ok_or(CapabilityError::InvalidHandle)?;
    match &cap.kind {
        CapabilityKind::Request(record) => Ok(record.clone()),
        _ => Err(CapabilityError::WrongKind),
    }
}

pub fn resolve_shared_region_handle(
    pid: u64,
    handle: UserHandle,
    rights: CapRights,
) -> Result<SharedRegionCapabilityRecord, CapabilityError> {
    let tables = CAP_TABLES.lock();
    let table = tables
        .get(&pid)
        .ok_or(CapabilityError::ProcessNotInitialized)?;
    let cap = table
        .get_by_handle(handle)
        .ok_or(CapabilityError::InvalidHandle)?;
    require_rights(cap, rights)?;
    match &cap.kind {
        CapabilityKind::SharedRegion(record) => Ok(record.clone()),
        _ => Err(CapabilityError::WrongKind),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_handle_resolution_is_pid_local() {
        init_process(7);
        init_process(8);
        let handle = open_service_handle(7, 3, CapRights::READ_WRITE, 1, None).expect("handle");
        assert!(resolve_service_handle(7, handle, CapRights::READ).is_ok());
        assert_eq!(
            resolve_service_handle(8, handle, CapRights::READ).expect_err("foreign pid must fail"),
            CapabilityError::InvalidHandle
        );
    }

    #[test]
    fn revoke_handle_removes_user_visible_slot() {
        init_process(11);
        let handle = open_service_handle(11, 5, CapRights::READ_WRITE, 1, None).expect("handle");
        revoke_handle(11, handle).expect("revoke");
        assert_eq!(
            resolve_service_handle(11, handle, CapRights::READ)
                .expect_err("revoked handle must fail"),
            CapabilityError::InvalidHandle
        );
    }
}
