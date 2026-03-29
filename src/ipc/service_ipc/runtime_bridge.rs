use super::handles::map_capability_error;
use super::*;

use crate::kernel::tasking;

impl ServiceIpcManager {
    pub fn register_service_runtime_task(&self, service: ServiceId, task_id: u64) {
        self.service_runtime_tasks.lock().insert(task_id, service);
    }

    pub fn bind_runtime_endpoints(&self) {}

    pub fn grant_service_mailbox_regions(
        &self,
        pid: u32,
        service: ServiceId,
    ) -> Result<ServiceMailboxLease, ServiceError> {
        let service_pid = pid as u64;
        let (request_region_id, response_region_id) = {
            let mut regions = self.mailbox_regions.lock();
            if let Some(existing) = regions.get(&service).copied() {
                existing
            } else {
                let request = shared_region::create_ipc_region(
                    service_pid,
                    service_mailbox_region_name(service, "request").as_str(),
                    64 * 1024,
                    true,
                );
                let response = shared_region::create_ipc_region(
                    service_pid,
                    service_mailbox_region_name(service, "response").as_str(),
                    64 * 1024,
                    true,
                );
                let ids = (request.id, response.id);
                regions.insert(service, ids);
                ids
            }
        };
        let request_region = self.grant_shared_region_handle(pid, request_region_id)?;
        let response_region = self.grant_shared_region_handle(pid, response_region_id)?;
        Ok(ServiceMailboxLease {
            request_region,
            response_region,
        })
    }

    pub fn publish_user_endpoint(
        &self,
        pid: u32,
        service: ServiceId,
        request_region_handle: UserHandle,
        response_region_handle: UserHandle,
    ) -> Result<UserPublishedEndpointDescriptor, ServiceError> {
        let task_id = tasking::scheduler::current_task_id() as u64;
        let current_service = self
            .current_service_context()
            .ok_or(ServiceError::RightsDenied)?;
        if current_service != service {
            return Err(ServiceError::RightsDenied);
        }
        let request_region = capability::resolve_shared_region_handle(
            pid as u64,
            request_region_handle,
            CapRights::READ_WRITE,
        )
        .map_err(map_capability_error)?;
        let response_region = capability::resolve_shared_region_handle(
            pid as u64,
            response_region_handle,
            CapRights::READ_WRITE,
        )
        .map_err(map_capability_error)?;
        if !shared_region::region_generation_matches(
            request_region.region_id,
            request_region.region_generation,
        ) || !shared_region::region_generation_matches(
            response_region.region_id,
            response_region.region_generation,
        ) {
            return Err(ServiceError::StaleGeneration);
        }
        let descriptor = UserPublishedEndpointDescriptor {
            owner_pid: pid as u64,
            task_id,
            request_region_id: request_region.region_id,
            request_generation: request_region.region_generation,
            response_region_id: response_region.region_id,
            response_generation: response_region.region_generation,
            heartbeat_epoch: HEARTBEAT_EPOCH.fetch_add(1, Ordering::Relaxed),
        };
        self.published_user_endpoints
            .lock()
            .insert(service, descriptor);
        Ok(descriptor)
    }

    pub fn heartbeat_user_endpoint(
        &self,
        pid: u32,
        service: ServiceId,
    ) -> Result<UserPublishedEndpointDescriptor, ServiceError> {
        let current_service = self
            .current_service_context()
            .ok_or(ServiceError::RightsDenied)?;
        if current_service != service {
            return Err(ServiceError::RightsDenied);
        }
        let mut endpoints = self.published_user_endpoints.lock();
        let descriptor = endpoints
            .get_mut(&service)
            .ok_or(ServiceError::ServiceUnavailable)?;
        if descriptor.owner_pid != pid as u64 {
            return Err(ServiceError::RightsDenied);
        }
        descriptor.heartbeat_epoch = HEARTBEAT_EPOCH.fetch_add(1, Ordering::Relaxed);
        Ok(*descriptor)
    }

    pub fn published_user_endpoint(
        &self,
        service: ServiceId,
    ) -> Option<UserPublishedEndpointDescriptor> {
        self.published_user_endpoints.lock().get(&service).copied()
    }

    pub(in crate::ipc::service_ipc) fn route_to_user_published_endpoint(
        &self,
        service: ServiceId,
    ) -> bool {
        matches!(service, ServiceId::EchNotifications)
            && self.service_has_runtime_task(service)
            && self.published_user_endpoint(service).is_some()
    }

    pub(in crate::ipc::service_ipc) fn published_endpoint_owned_by(
        &self,
        pid: u32,
        service: ServiceId,
    ) -> Result<UserPublishedEndpointDescriptor, ServiceError> {
        let descriptor = self
            .published_user_endpoint(service)
            .ok_or(ServiceError::ServiceUnavailable)?;
        if descriptor.owner_pid != pid as u64 {
            return Err(ServiceError::RightsDenied);
        }
        if !shared_region::region_generation_matches(
            descriptor.request_region_id,
            descriptor.request_generation,
        ) || !shared_region::region_generation_matches(
            descriptor.response_region_id,
            descriptor.response_generation,
        ) {
            return Err(ServiceError::StaleGeneration);
        }
        Ok(descriptor)
    }

    pub(in crate::ipc::service_ipc) fn ensure_runtime_endpoint(
        &self,
        service: ServiceId,
    ) -> Option<BoundServiceEndpoint> {
        if matches!(
            service,
            ServiceId::Directory | ServiceId::PackageRegistry | ServiceId::ProcessBroker
        ) {
            return None;
        }
        self.endpoints.lock().get(&service).cloned()
    }

    pub(in crate::ipc::service_ipc) fn endpoint_generation(
        &self,
        service: ServiceId,
    ) -> EndpointGeneration {
        if matches!(
            service,
            ServiceId::Directory | ServiceId::PackageRegistry | ServiceId::ProcessBroker
        ) {
            return 1;
        }
        self.endpoint_generations
            .lock()
            .get(&service)
            .copied()
            .unwrap_or(0)
    }

    pub(in crate::ipc::service_ipc) fn current_service_context(&self) -> Option<ServiceId> {
        if cfg!(test) {
            return None;
        }
        let task_id = tasking::scheduler::current_task_id() as u64;
        self.service_runtime_tasks.lock().get(&task_id).copied()
    }

    pub(in crate::ipc::service_ipc) fn current_active_request(
        &self,
    ) -> Option<ActiveServiceRequest> {
        let service = self.current_service_context()?;
        self.active_requests.lock().get(&service).copied()
    }

    pub(in crate::ipc::service_ipc) fn service_has_runtime_task(&self, service: ServiceId) -> bool {
        self.service_runtime_tasks
            .lock()
            .values()
            .any(|registered| *registered == service)
    }

    pub(in crate::ipc::service_ipc) fn submit_to_runtime_queue(
        &self,
        endpoint: &BoundServiceEndpoint,
        envelope: MessageEnvelope,
    ) -> Result<(), ServiceError> {
        self.active_requests.lock().insert(
            envelope.to_service,
            ActiveServiceRequest {
                request_token: envelope.request_token,
                origin_endpoint: envelope.origin_endpoint.or(Some(envelope.to_service)),
            },
        );
        if let Err(error) = endpoint.enqueue(envelope.message.clone()) {
            self.active_requests.lock().remove(&envelope.to_service);
            return Err(error);
        }
        self.inflight_requests
            .lock()
            .entry(envelope.to_service)
            .or_default()
            .push_back(envelope);
        Ok(())
    }

    pub(in crate::ipc::service_ipc) fn submit_to_user_runtime_queue(
        &self,
        envelope: MessageEnvelope,
    ) -> Result<(), ServiceError> {
        let command = match envelope.message.clone() {
            ServiceMessage::NotificationCommand(command) => command,
            _ => return Err(ServiceError::WrongService),
        };
        let descriptor =
            self.published_endpoint_owned_by(envelope.from_app, envelope.to_service)?;
        self.active_requests.lock().insert(
            envelope.to_service,
            ActiveServiceRequest {
                request_token: envelope.request_token,
                origin_endpoint: envelope.origin_endpoint.or(Some(envelope.to_service)),
            },
        );
        if self
            .notification_user_requests
            .try_push(NotificationUserRequest {
                request_id: envelope.id,
                command,
            })
            .is_err()
        {
            self.active_requests.lock().remove(&envelope.to_service);
            return Err(ServiceError::QueueFull);
        }
        if !shared_region::region_generation_matches(
            descriptor.response_region_id,
            descriptor.response_generation,
        ) {
            self.active_requests.lock().remove(&envelope.to_service);
            return Err(ServiceError::StaleGeneration);
        }
        self.inflight_requests
            .lock()
            .entry(envelope.to_service)
            .or_default()
            .push_back(envelope);
        Ok(())
    }

    pub(in crate::ipc::service_ipc) fn complete_immediate_response(
        &self,
        envelope: &MessageEnvelope,
        response: ServiceResponse,
    ) {
        {
            let mut pending = self.pending.lock();
            if let Some(entry) = pending.get_mut(&envelope.id) {
                entry.response = Some(response.clone());
            }
        }
        let _ = self.incoming.try_push(ResponseEnvelope {
            message_id: envelope.id,
            request_token: envelope.request_token,
            response,
        });
    }

    pub(in crate::ipc::service_ipc) fn poll_runtime_responses(&self) {
        let services = self
            .inflight_requests
            .lock()
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for service in services {
            if self.route_to_user_published_endpoint(service) {
                while let Some(user_response) = self.notification_user_responses.pop() {
                    let envelope = {
                        let mut inflight = self.inflight_requests.lock();
                        let Some(queue) = inflight.get_mut(&service) else {
                            break;
                        };
                        let position = queue
                            .iter()
                            .position(|entry| entry.id == user_response.request_id);
                        let envelope = position.and_then(|index| queue.remove(index));
                        if queue.is_empty() {
                            inflight.remove(&service);
                        }
                        envelope
                    };
                    let Some(envelope) = envelope else {
                        continue;
                    };
                    self.active_requests.lock().remove(&service);
                    self.complete_immediate_response(
                        &envelope,
                        ServiceResponse::NotificationResponse(user_response.response),
                    );
                }
                continue;
            }
            let Some(endpoint) = self.ensure_runtime_endpoint(service) else {
                continue;
            };
            while let Some(response) = endpoint.try_receive() {
                let envelope = {
                    let mut inflight = self.inflight_requests.lock();
                    let Some(queue) = inflight.get_mut(&service) else {
                        break;
                    };
                    let envelope = queue.pop_front();
                    if queue.is_empty() {
                        inflight.remove(&service);
                    }
                    envelope
                };
                let Some(envelope) = envelope else {
                    break;
                };
                self.active_requests.lock().remove(&service);
                self.complete_immediate_response(&envelope, response);
            }
        }
    }
}
