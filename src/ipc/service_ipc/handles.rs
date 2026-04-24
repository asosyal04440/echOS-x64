use super::*;

impl ServiceIpcManager {
    pub fn open_service_handle(
        &self,
        pid: u32,
        service: ServiceId,
        rights: CapabilityRights,
    ) -> Result<ServiceHandleDescriptor, ServiceError> {
        if !matches!(
            service,
            ServiceId::Directory
                | ServiceId::NetworkBroker
                | ServiceId::PackageRegistry
                | ServiceId::ProcessBroker
                | ServiceId::UpdateInstaller
        ) && self.ensure_runtime_endpoint(service).is_none()
        {
            return Err(ServiceError::ServiceUnavailable);
        }
        let endpoint_generation = self.endpoint_generation(service);
        if endpoint_generation == 0 {
            return Err(ServiceError::ServiceUnavailable);
        }
        let handle = capability::open_service_handle(
            pid as u64,
            service as u64,
            rights,
            endpoint_generation,
            self.current_service_context().map(|service| service as u64),
        )
        .map_err(map_capability_error)?;
        Ok(ServiceHandleDescriptor {
            handle,
            service_id: service,
            endpoint_generation,
            rights,
        })
    }

    pub fn grant_shared_region_handle(
        &self,
        pid: u32,
        region_id: u64,
    ) -> Result<SharedRegionDescriptor, ServiceError> {
        let lease = shared_region::snapshot_ipc_region(region_id)
            .ok_or(ServiceError::SharedRegionUnavailable)?;
        let handle = capability::grant_shared_region_handle(
            pid as u64,
            lease.id,
            lease.generation,
            lease.len,
            lease.writable,
        )
        .map_err(map_capability_error)?;
        Ok(SharedRegionDescriptor {
            handle,
            region_id: lease.id,
            generation: lease.generation,
            len: lease.len,
            writable: lease.writable,
        })
    }

    pub fn map_shared_region(
        &self,
        pid: u32,
        region_handle: UserHandle,
    ) -> Result<UserMapping, ServiceError> {
        let descriptor =
            capability::resolve_shared_region_handle(pid as u64, region_handle, CapRights::READ)
                .map_err(map_capability_error)?;
        if !shared_region::region_generation_matches(
            descriptor.region_id,
            descriptor.region_generation,
        ) {
            return Err(ServiceError::StaleGeneration);
        }
        if let Some(space) =
            crate::runtime_layer::capability_contract::runtime_address_space_for_pid(pid as u64)
        {
            shared_region::map_ipc_region_into_space(pid as u64, descriptor.region_id, &space)
                .ok_or(ServiceError::SharedRegionUnavailable)
        } else {
            shared_region::map_ipc_region(pid as u64, descriptor.region_id)
                .ok_or(ServiceError::SharedRegionUnavailable)
        }
    }

    pub fn revoke_handle(&self, pid: u32, handle: RawUserHandle) -> Result<(), ServiceError> {
        capability::revoke_handle(pid as u64, handle).map_err(map_capability_error)
    }

    pub fn unmap_shared_region(
        &self,
        pid: u32,
        region_handle: UserHandle,
    ) -> Result<(), ServiceError> {
        let descriptor =
            capability::resolve_shared_region_handle(pid as u64, region_handle, CapRights::READ)
                .map_err(map_capability_error)?;
        if !shared_region::region_generation_matches(
            descriptor.region_id,
            descriptor.region_generation,
        ) {
            return Err(ServiceError::StaleGeneration);
        }
        if shared_region::unmap_ipc_region(pid as u64, descriptor.region_id) {
            Ok(())
        } else {
            Err(ServiceError::SharedRegionUnavailable)
        }
    }
}

pub(in crate::ipc::service_ipc) fn map_capability_error(error: CapabilityError) -> ServiceError {
    match error {
        CapabilityError::ProcessNotInitialized | CapabilityError::InvalidHandle => {
            ServiceError::InvalidHandle
        }
        CapabilityError::Revoked => ServiceError::Revoked,
        CapabilityError::RightsDenied => ServiceError::RightsDenied,
        CapabilityError::WrongKind => ServiceError::WrongService,
        CapabilityError::StaleGeneration => ServiceError::StaleGeneration,
    }
}

pub(in crate::ipc::service_ipc) fn default_rights_for(service: ServiceId) -> CapabilityRights {
    match service {
        ServiceId::Directory
        | ServiceId::NetworkBroker
        | ServiceId::PackageRegistry
        | ServiceId::ProcessBroker
        | ServiceId::UpdateInstaller => CapRights::READ_WRITE,
        _ => CapRights::READ_WRITE,
    }
}

pub(in crate::ipc::service_ipc) fn service_id_from_u64(value: u64) -> Option<ServiceId> {
    match value {
        0 => Some(ServiceId::Directory),
        13 => Some(ServiceId::NetworkBroker),
        1 => Some(ServiceId::EchDisplay),
        2 => Some(ServiceId::EchInput),
        3 => Some(ServiceId::EchAudio),
        4 => Some(ServiceId::EchStore),
        5 => Some(ServiceId::EchShell),
        6 => Some(ServiceId::EchNotifications),
        7 => Some(ServiceId::EchClipboard),
        8 => Some(ServiceId::EchDialogs),
        9 => Some(ServiceId::EchCapture),
        10 => Some(ServiceId::PackageRegistry),
        11 => Some(ServiceId::ProcessBroker),
        12 => Some(ServiceId::UpdateInstaller),
        _ => None,
    }
}
