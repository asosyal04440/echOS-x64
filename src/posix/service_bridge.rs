use alloc::vec::Vec;

use super::super::gui::protocol::{NotificationEntry, NotificationLevel};
use super::super::runtime_layer::launch_contract::IsolationDomain;
use super::super::runtime_layer::native_scene_contract::{self, RuntimeHandle};
use super::super::runtime_layer::service_endpoint_contract;
use super::super::runtime_layer::service_parity_contract;
use super::super::services::{NotificationCommand, NotificationResponse};
use super::super::task;

pub(super) fn sys_service_bootstrap_claim(out_ptr: usize) -> usize {
    if let Err(err) = super::validate_user_range(
        out_ptr,
        core::mem::size_of::<super::NativeServiceBootstrap>(),
    ) {
        return err;
    }
    let runtime = match current_service_runtime() {
        Ok(runtime) => runtime,
        Err(err) => return err,
    };
    let Some(service_id) = runtime.service_id else {
        return super::errno(super::EACCES);
    };
    let endpoint_generation =
        service_endpoint_contract::endpoint_generation_for_service(service_id);
    let mailbox_lease = match service_endpoint_contract::grant_service_mailbox_regions(
        runtime.identity.app_id,
        service_id,
    ) {
        Ok(lease) => lease,
        Err(_) => return super::errno(super::EIO),
    };
    let (service_handle, rights_bits) =
        match service_endpoint_contract::describe_service(service_id) {
            Some(descriptor) => {
                let handle = service_endpoint_contract::open_service_handle(
                    runtime.identity.app_id,
                    service_id,
                    descriptor.openable_rights,
                )
                .ok();
                (
                    handle.as_ref().map(|handle| handle.handle).unwrap_or(0),
                    encode_service_rights_bits(descriptor.openable_rights),
                )
            }
            None => return super::errno(super::ENOENT),
        };
    let response = super::NativeServiceBootstrap {
        abi_version: 1,
        service_id: service_id as u32,
        runtime_app_id: runtime.identity.app_id,
        service_handle,
        request_region_handle: mailbox_lease.request_region.handle,
        response_region_handle: mailbox_lease.response_region.handle,
        endpoint_generation,
        rights_bits,
        isolation_domain: encode_isolation_domain(runtime.isolation_domain),
        runtime_task_id: runtime
            .task_id
            .unwrap_or(task::scheduler::current_task_id() as u64),
    };
    if let Err(err) = super::write_user(out_ptr, response) {
        return err;
    }
    0
}

pub(super) fn sys_service_status(service_id: usize, out_ptr: usize) -> usize {
    if let Err(err) =
        super::validate_user_range(out_ptr, core::mem::size_of::<super::NativeServiceStatus>())
    {
        return err;
    }
    let Some(service_id) = decode_service_id(service_id as u32) else {
        return super::errno(super::EINVAL);
    };
    let Some(descriptor) = service_endpoint_contract::describe_service(service_id) else {
        return super::errno(super::ENOENT);
    };
    let response = super::NativeServiceStatus {
        abi_version: 1,
        service_id: service_id as u32,
        openable_rights_bits: encode_service_rights_bits(descriptor.openable_rights),
        endpoint_generation: service_endpoint_contract::endpoint_generation_for_service(service_id),
        control_plane: descriptor.control_plane as u8,
        bulk_data_out_of_band: descriptor.bulk_data_out_of_band as u8,
        service_process_available: descriptor.service_process_available as u8,
        user_published_endpoint: descriptor.user_published_endpoint as u8,
        runtime_isolation: descriptor
            .runtime_isolation
            .map(encode_isolation_domain)
            .unwrap_or(0) as u8,
        runtime_task_id: descriptor.runtime_task_id.unwrap_or(0),
    };
    if let Err(err) = super::write_user(out_ptr, response) {
        return err;
    }
    0
}

pub(super) fn sys_service_parity_status(out_ptr: usize) -> usize {
    if let Err(err) = super::validate_user_range(
        out_ptr,
        core::mem::size_of::<super::NativeServiceParityStatus>(),
    ) {
        return err;
    }
    let status = service_parity_contract::service_parity_status();
    let response = super::NativeServiceParityStatus {
        abi_version: 1,
        required_services: status.required_services,
        packaged_service_slots: status.packaged_service_slots,
        live_user_process_slots: status.live_user_process_slots,
        published_user_process_slots: status.published_user_process_slots,
        strict_mode_enabled: status.strict_mode_enabled as u8,
        full_parity_ready: status.full_parity_ready as u8,
        reserved: [0; 6],
    };
    if let Err(err) = super::write_user(out_ptr, response) {
        return err;
    }
    0
}

pub(super) fn sys_service_region_map(mapping_ptr: usize) -> usize {
    let mut request = match super::read_user::<super::NativeServiceRegionMapping>(mapping_ptr) {
        Ok(value) => value,
        Err(err) => return err,
    };
    let pid = current_runtime_pid();
    let mapping =
        match service_endpoint_contract::map_shared_region(pid as u32, request.region_handle) {
            Ok(mapping) => mapping,
            Err(service_endpoint_contract::ServiceError::RightsDenied) => {
                return super::errno(super::EACCES);
            }
            Err(service_endpoint_contract::ServiceError::StaleGeneration) => {
                return super::errno(super::EIO);
            }
            Err(_) => return super::errno(super::EINVAL),
        };
    request.abi_version = 1;
    request.region_id = mapping.region_id;
    request.generation = mapping.generation;
    request.base = mapping.base;
    request.len = mapping.len;
    request.writable = mapping.writable as u32;
    if let Err(err) = super::write_user(mapping_ptr, request) {
        return err;
    }
    0
}

pub(super) fn sys_service_endpoint_publish(request_ptr: usize) -> usize {
    let request = match super::read_user::<super::NativeServiceEndpointPublishRequest>(request_ptr)
    {
        Ok(value) => value,
        Err(err) => return err,
    };
    let Some(service_id) = decode_service_id(request.service_id) else {
        return super::errno(super::EINVAL);
    };
    let pid = current_runtime_pid();
    match service_endpoint_contract::publish_user_service_endpoint(
        pid as u32,
        service_id,
        request.request_region_handle,
        request.response_region_handle,
    ) {
        Ok(_) => 0,
        Err(service_endpoint_contract::ServiceError::RightsDenied) => super::errno(super::EACCES),
        Err(service_endpoint_contract::ServiceError::StaleGeneration) => super::errno(super::EIO),
        Err(_) => super::errno(super::EINVAL),
    }
}

pub(super) fn sys_service_heartbeat(service_id: usize, out_ptr: usize) -> usize {
    if let Err(err) = super::validate_user_range(
        out_ptr,
        core::mem::size_of::<super::NativeServiceEndpointState>(),
    ) {
        return err;
    }
    let Some(service_id) = decode_service_id(service_id as u32) else {
        return super::errno(super::EINVAL);
    };
    let pid = current_runtime_pid();
    let state =
        match service_endpoint_contract::heartbeat_user_service_endpoint(pid as u32, service_id) {
            Ok(state) => state,
            Err(service_endpoint_contract::ServiceError::RightsDenied) => {
                return super::errno(super::EACCES);
            }
            Err(service_endpoint_contract::ServiceError::ServiceUnavailable) => {
                return super::errno(super::ENOENT);
            }
            Err(_) => return super::errno(super::EINVAL),
        };
    let response = super::NativeServiceEndpointState {
        abi_version: 1,
        service_id: service_id as u32,
        request_region_id: state.request_region_id,
        request_generation: state.request_generation,
        response_region_id: state.response_region_id,
        response_generation: state.response_generation,
        heartbeat_epoch: state.heartbeat_epoch,
    };
    if let Err(err) = super::write_user(out_ptr, response) {
        return err;
    }
    0
}

pub(super) fn sys_notification_service_recv(out_ptr: usize) -> usize {
    if let Err(err) = super::validate_user_range(
        out_ptr,
        core::mem::size_of::<super::NativeServiceNotificationRequest>(),
    ) {
        return err;
    }
    let runtime = match current_service_runtime() {
        Ok(runtime) => runtime,
        Err(err) => return err,
    };
    if runtime.service_id != Some(service_endpoint_contract::ServiceId::EchNotifications) {
        return super::errno(super::EACCES);
    }
    let pid = current_runtime_pid() as u32;
    let mut response = super::NativeServiceNotificationRequest {
        abi_version: 1,
        request_id: 0,
        kind: super::NativeServiceNotificationCommandKind::None as u32,
        app_id: 0,
        include_read: 0,
        max_items: 0,
        notification_id: 0,
        level: 0,
        title_len: 0,
        message_len: 0,
        action_label_len: 0,
        reserved: 0,
        title: [0; super::MAX_INLINE_TEXT],
        message: [0; super::MAX_INLINE_TEXT],
        action_label: [0; super::MAX_INLINE_TEXT],
    };
    match service_endpoint_contract::receive_notification_user_request(pid) {
        Ok(Some((request_id, command))) => {
            response.request_id = request_id;
            encode_notification_service_request(command, &mut response);
        }
        Ok(None) => {}
        Err(service_endpoint_contract::ServiceError::RightsDenied) => {
            return super::errno(super::EACCES);
        }
        Err(service_endpoint_contract::ServiceError::StaleGeneration) => {
            return super::errno(super::EIO);
        }
        Err(service_endpoint_contract::ServiceError::ServiceUnavailable) => {
            return super::errno(super::ENOENT);
        }
        Err(_) => return super::errno(super::EINVAL),
    }
    if let Err(err) = super::write_user(out_ptr, response) {
        return err;
    }
    0
}

pub(super) fn sys_notification_service_respond(request_ptr: usize) -> usize {
    let request = match super::read_user::<super::NativeServiceNotificationResponse>(request_ptr) {
        Ok(value) => value,
        Err(err) => return err,
    };
    let runtime = match current_service_runtime() {
        Ok(runtime) => runtime,
        Err(err) => return err,
    };
    if runtime.service_id != Some(service_endpoint_contract::ServiceId::EchNotifications) {
        return super::errno(super::EACCES);
    }
    let response = match decode_notification_service_response(&request) {
        Ok(response) => response,
        Err(err) => return err,
    };
    match service_endpoint_contract::send_notification_user_response(
        current_runtime_pid() as u32,
        request.request_id,
        response,
    ) {
        Ok(()) => 0,
        Err(service_endpoint_contract::ServiceError::RightsDenied) => super::errno(super::EACCES),
        Err(service_endpoint_contract::ServiceError::StaleGeneration) => super::errno(super::EIO),
        Err(service_endpoint_contract::ServiceError::QueueFull) => super::errno(super::EAGAIN),
        Err(service_endpoint_contract::ServiceError::ServiceUnavailable) => {
            super::errno(super::ENOENT)
        }
        Err(_) => super::errno(super::EINVAL),
    }
}

fn encode_notification_service_request(
    command: NotificationCommand,
    raw: &mut super::NativeServiceNotificationRequest,
) {
    match command {
        NotificationCommand::Push(request) => {
            raw.kind = super::NativeServiceNotificationCommandKind::Push as u32;
            raw.app_id = request.app_id;
            raw.level = encode_notification_level(request.level);
            let (title_len, title) = super::inline_text_buffer(request.title.as_str());
            raw.title_len = title_len;
            raw.title = title;
            let (message_len, message) = super::inline_text_buffer(request.message.as_str());
            raw.message_len = message_len;
            raw.message = message;
            let (action_label_len, action_label) =
                super::optional_inline_text_buffer(request.action_label.as_deref());
            raw.action_label_len = action_label_len;
            raw.action_label = action_label;
        }
        NotificationCommand::List {
            app_id,
            include_read,
            max_items,
        } => {
            raw.kind = super::NativeServiceNotificationCommandKind::List as u32;
            raw.app_id = app_id;
            raw.include_read = include_read as u32;
            raw.max_items = max_items.min(u32::MAX as usize) as u32;
        }
        NotificationCommand::MarkRead { app_id, id } => {
            raw.kind = super::NativeServiceNotificationCommandKind::MarkRead as u32;
            raw.app_id = app_id;
            raw.notification_id = id;
        }
        NotificationCommand::Clear { app_id } => {
            raw.kind = super::NativeServiceNotificationCommandKind::Clear as u32;
            raw.app_id = app_id;
        }
    }
}

fn decode_notification_service_response(
    raw: &super::NativeServiceNotificationResponse,
) -> Result<NotificationResponse, usize> {
    match raw.kind {
        kind if kind == super::NativeServiceNotificationResponseKind::Ack as u32 => {
            Ok(NotificationResponse::Ack)
        }
        kind if kind == super::NativeServiceNotificationResponseKind::NotificationId as u32 => {
            Ok(NotificationResponse::NotificationId(raw.notification_id))
        }
        kind if kind == super::NativeServiceNotificationResponseKind::Notifications as u32 => {
            let count = raw.entry_count as usize;
            if count > super::MAX_SERVICE_NOTIFICATION_ITEMS {
                return Err(super::errno(super::EINVAL));
            }
            let mut entries = Vec::with_capacity(count);
            for entry in raw.entries.iter().take(count) {
                entries.push(NotificationEntry {
                    id: entry.id,
                    app_id: entry.app_id,
                    source_name: super::decode_inline_text(
                        &entry.source_name,
                        entry.source_name_len,
                    )?,
                    title: super::decode_inline_text(&entry.title, entry.title_len)?,
                    message: super::decode_inline_text(&entry.message, entry.message_len)?,
                    level: decode_notification_level(entry.level)?,
                    read: entry.read != 0,
                    timestamp_ticks: entry.timestamp_ticks,
                    action_label: super::decode_optional_inline_text(
                        &entry.action_label,
                        entry.action_label_len,
                    )?,
                });
            }
            Ok(NotificationResponse::Notifications(entries))
        }
        kind if kind == super::NativeServiceNotificationResponseKind::Error as u32 => Ok(
            NotificationResponse::Error(super::decode_inline_text(&raw.error, raw.error_len)?),
        ),
        _ => Err(super::errno(super::EINVAL)),
    }
}

fn current_service_runtime() -> Result<RuntimeHandle, usize> {
    let task_id = task::scheduler::current_task_id() as u64;
    let Some(runtime) = native_scene_contract::runtime_handle_for_task(task_id) else {
        return Err(super::errno(super::EACCES));
    };
    if runtime.service_id.is_none() {
        return Err(super::errno(super::EACCES));
    }
    Ok(runtime)
}

fn current_runtime_pid() -> u64 {
    let task_id = task::scheduler::current_task_id() as u64;
    native_scene_contract::runtime_handle_for_task(task_id)
        .map(|runtime| runtime.identity.app_id as u64)
        .unwrap_or(task_id)
}

fn decode_service_id(service_id: u32) -> Option<service_endpoint_contract::ServiceId> {
    match service_id {
        0 => Some(service_endpoint_contract::ServiceId::Directory),
        13 => Some(service_endpoint_contract::ServiceId::NetworkBroker),
        1 => Some(service_endpoint_contract::ServiceId::EchDisplay),
        2 => Some(service_endpoint_contract::ServiceId::EchInput),
        3 => Some(service_endpoint_contract::ServiceId::EchAudio),
        4 => Some(service_endpoint_contract::ServiceId::EchStore),
        5 => Some(service_endpoint_contract::ServiceId::EchShell),
        6 => Some(service_endpoint_contract::ServiceId::EchNotifications),
        7 => Some(service_endpoint_contract::ServiceId::EchClipboard),
        8 => Some(service_endpoint_contract::ServiceId::EchDialogs),
        9 => Some(service_endpoint_contract::ServiceId::EchCapture),
        10 => Some(service_endpoint_contract::ServiceId::PackageRegistry),
        11 => Some(service_endpoint_contract::ServiceId::ProcessBroker),
        12 => Some(service_endpoint_contract::ServiceId::UpdateInstaller),
        _ => None,
    }
}

fn encode_isolation_domain(domain: IsolationDomain) -> u32 {
    match domain {
        IsolationDomain::KernelTask => 1,
        IsolationDomain::UserProcess => 2,
    }
}

fn encode_service_rights_bits(rights: service_endpoint_contract::CapabilityRights) -> u32 {
    (rights.read as u32)
        | ((rights.write as u32) << 1)
        | ((rights.execute as u32) << 2)
        | ((rights.share as u32) << 3)
        | ((rights.transfer as u32) << 4)
}

fn encode_notification_level(level: NotificationLevel) -> u32 {
    match level {
        NotificationLevel::Info => 0,
        NotificationLevel::Success => 1,
        NotificationLevel::Warning => 2,
        NotificationLevel::Error => 3,
    }
}

fn decode_notification_level(raw: u32) -> Result<NotificationLevel, usize> {
    match raw {
        0 => Ok(NotificationLevel::Info),
        1 => Ok(NotificationLevel::Success),
        2 => Ok(NotificationLevel::Warning),
        3 => Ok(NotificationLevel::Error),
        _ => Err(super::errno(super::EINVAL)),
    }
}
