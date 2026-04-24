use super::super::super::{preempt, serial_println};
use super::handles::{default_rights_for, map_capability_error, service_id_from_u64};
use super::*;

impl ServiceIpcManager {
    pub fn send_request(
        &self,
        pid: u32,
        handle: UserHandle,
        message: ServiceMessage,
    ) -> Result<RequestTokenDescriptor, ServiceError> {
        self.enqueue_request(pid, handle, message, BlockingMode::Async)
    }

    fn enqueue_request(
        &self,
        pid: u32,
        handle: UserHandle,
        message: ServiceMessage,
        blocking_mode: BlockingMode,
    ) -> Result<RequestTokenDescriptor, ServiceError> {
        let record = capability::resolve_service_handle(pid as u64, handle, CapRights::WRITE)
            .map_err(map_capability_error)?;
        let service = service_id_from_u64(record.service_id).ok_or(ServiceError::WrongService)?;
        let current_generation = self.endpoint_generation(service);
        if current_generation == 0 {
            return Err(ServiceError::ServiceUnavailable);
        }
        if current_generation != record.endpoint_generation {
            return Err(ServiceError::EndpointRestarted);
        }
        let request_id = REQUEST_ID.fetch_add(1, Ordering::SeqCst);
        let token_handle = capability::grant_request_handle(
            pid as u64,
            request_id,
            record.service_id,
            record.endpoint_generation,
            record.owner_endpoint,
        )
        .map_err(map_capability_error)?;
        let current_service = self.current_service_context();
        let active_request = self.current_active_request();
        let descriptor = RequestTokenDescriptor {
            token: RequestToken(token_handle),
            request_id,
            service_id: service,
            endpoint_generation: record.endpoint_generation,
        };
        self.pending.lock().insert(
            request_id,
            PendingRequest {
                owner_pid: pid as u64,
                request_token: descriptor.token,
                response: None,
            },
        );
        let envelope = MessageEnvelope {
            id: request_id,
            owner_pid: pid as u64,
            from_app: pid,
            request_token: descriptor.token,
            to_service: service,
            endpoint_generation: record.endpoint_generation,
            origin_endpoint: active_request
                .and_then(|request| request.origin_endpoint)
                .or(current_service),
            blocking_mode,
            causal_parent_token: active_request.map(|request| request.request_token),
            message,
        };
        if self.outgoing.try_push(envelope).is_err() {
            let _ = self.pending.lock().remove(&request_id);
            let _ = capability::revoke_handle(pid as u64, token_handle);
            return Err(ServiceError::QueueFull);
        }
        Ok(descriptor)
    }

    pub fn take_response(
        &self,
        pid: u32,
        token: RequestToken,
    ) -> Result<Option<ServiceResponse>, ServiceError> {
        let request = capability::resolve_request_handle(pid as u64, token.0)
            .map_err(map_capability_error)?;
        let response = self
            .pending
            .lock()
            .get(&request.request_id)
            .and_then(|pending| pending.response.clone());
        if let Some(response) = response {
            self.discard_buffered_response(request.request_id);
            self.complete_request(request.request_id);
            Ok(Some(response))
        } else {
            Ok(None)
        }
    }

    pub fn request_sync(
        &self,
        pid: u32,
        handle: UserHandle,
        message: ServiceMessage,
    ) -> Result<ServiceResponse, ServiceError> {
        let service = capability::resolve_service_handle(pid as u64, handle, CapRights::WRITE)
            .map_err(map_capability_error)?
            .service_id;
        let service = service_id_from_u64(service).ok_or(ServiceError::WrongService)?;
        if self.sync_cycle_risk(service) {
            return Err(ServiceError::SyncCycleRisk);
        }
        let descriptor = self.enqueue_request(pid, handle, message, BlockingMode::Sync)?;
        for spin in 0..SERVICE_RESPONSE_SPINS {
            self.process_pending_messages();
            if let Some(response) = self.take_response(pid, descriptor.token)? {
                return Ok(response);
            }
            if (spin + 1) % SERVICE_RESPONSE_SCHEDULE_INTERVAL == 0 {
                preempt::preemptible_schedule();
            } else {
                core::hint::spin_loop();
            }
        }
        serial_println!(
            "[SERVICE_IPC][SYNC_WAIT_TIMEOUT] pid={} service={:?} request_id={} spins={}",
            pid,
            service,
            descriptor.request_id,
            SERVICE_RESPONSE_SPINS
        );
        Err(ServiceError::ServiceUnavailable)
    }

    pub fn send_to_service(&self, app_id: u32, service: ServiceId, message: ServiceMessage) -> u64 {
        let Ok(handle) = self.open_service_handle(app_id, service, default_rights_for(service))
        else {
            return 0;
        };
        match self.send_request(app_id, handle.handle, message) {
            Ok(descriptor) => descriptor.request_id,
            Err(_) => {
                let _ = self.revoke_handle(app_id, handle.handle);
                0
            }
        }
    }

    pub fn receive_from_service(&self) -> Option<ResponseEnvelope> {
        let response = self.incoming.pop()?;
        self.complete_request(response.message_id);
        Some(response)
    }

    pub fn receive_from_service_timeout(&self, _timeout_ms: u32) -> Option<ResponseEnvelope> {
        self.receive_from_service()
    }

    pub fn take_response_for(&self, message_id: u64) -> Option<ResponseEnvelope> {
        let request_token = self
            .pending
            .lock()
            .get(&message_id)
            .map(|pending| pending.request_token)?;
        let response = self
            .pending
            .lock()
            .get(&message_id)
            .and_then(|pending| pending.response.clone())?;
        self.discard_buffered_response(message_id);
        self.complete_request(message_id);
        Some(ResponseEnvelope {
            message_id,
            request_token,
            response,
        })
    }

    pub fn process_pending_messages(&self) {
        while let Some(envelope) = self.outgoing.pop() {
            match envelope.to_service {
                ServiceId::Directory => {
                    self.complete_immediate_response(
                        &envelope,
                        dispatch_directory_command(envelope.message.clone()),
                    );
                    continue;
                }
                ServiceId::NetworkBroker => {
                    self.complete_immediate_response(
                        &envelope,
                        dispatch_network_broker_command(envelope.message.clone()),
                    );
                    continue;
                }
                ServiceId::PackageRegistry => {
                    self.complete_immediate_response(
                        &envelope,
                        dispatch_package_registry_command(envelope.message.clone()),
                    );
                    continue;
                }
                ServiceId::ProcessBroker => {
                    self.complete_immediate_response(
                        &envelope,
                        dispatch_process_broker_command(envelope.message.clone()),
                    );
                    continue;
                }
                ServiceId::UpdateInstaller => {
                    self.complete_immediate_response(
                        &envelope,
                        dispatch_update_installer_command(envelope.message.clone()),
                    );
                    continue;
                }
                _ => {}
            }
            let current_generation = self.endpoint_generation(envelope.to_service);
            if current_generation == 0 {
                self.complete_immediate_response(
                    &envelope,
                    service_unavailable_response(envelope.to_service),
                );
                continue;
            }
            if current_generation != envelope.endpoint_generation {
                self.complete_immediate_response(
                    &envelope,
                    error_response(envelope.to_service, ServiceError::EndpointRestarted),
                );
                continue;
            }
            let Some(endpoint) = self.ensure_runtime_endpoint(envelope.to_service) else {
                self.complete_immediate_response(
                    &envelope,
                    service_unavailable_response(envelope.to_service),
                );
                continue;
            };
            if self.service_has_runtime_task(envelope.to_service) {
                let response = if self.route_to_user_published_endpoint(envelope.to_service) {
                    self.submit_to_user_runtime_queue(envelope.clone())
                        .err()
                        .map(|error| error_response(envelope.to_service, error))
                } else {
                    self.submit_to_runtime_queue(&endpoint, envelope.clone())
                        .err()
                        .map(|error| error_response(envelope.to_service, error))
                };
                if let Some(response) = response {
                    self.complete_immediate_response(&envelope, response);
                }
            } else {
                let response = self.dispatch_to_service(&envelope);
                self.complete_immediate_response(&envelope, response);
            }
        }
        self.poll_runtime_responses();
    }

    fn dispatch_to_service(&self, envelope: &MessageEnvelope) -> ServiceResponse {
        if envelope.to_service == ServiceId::Directory {
            return dispatch_directory_command(envelope.message.clone());
        }
        if envelope.to_service == ServiceId::NetworkBroker {
            return dispatch_network_broker_command(envelope.message.clone());
        }
        if envelope.to_service == ServiceId::UpdateInstaller {
            return dispatch_update_installer_command(envelope.message.clone());
        }
        let current_generation = self.endpoint_generation(envelope.to_service);
        if current_generation == 0 {
            return service_unavailable_response(envelope.to_service);
        }
        if current_generation != envelope.endpoint_generation {
            return error_response(envelope.to_service, ServiceError::EndpointRestarted);
        }
        let Some(endpoint) = self.ensure_runtime_endpoint(envelope.to_service) else {
            return service_unavailable_response(envelope.to_service);
        };
        self.active_requests.lock().insert(
            envelope.to_service,
            ActiveServiceRequest {
                request_token: envelope.request_token,
                origin_endpoint: envelope.origin_endpoint.or(Some(envelope.to_service)),
            },
        );
        let response = endpoint
            .dispatch_sync(envelope.message.clone())
            .unwrap_or_else(|error| error_response(envelope.to_service, error));
        self.active_requests.lock().remove(&envelope.to_service);
        response
    }

    fn discard_buffered_response(&self, message_id: u64) {
        let mut buffered = Vec::new();
        while let Some(response) = self.incoming.pop() {
            if response.message_id != message_id {
                buffered.push(response);
            }
        }
        for response in buffered {
            let _ = self.incoming.try_push(response);
        }
    }

    fn complete_request(&self, message_id: u64) {
        let pending = self.pending.lock().remove(&message_id);
        if let Some(pending) = pending {
            let _ = capability::revoke_handle(pending.owner_pid, pending.request_token.0);
        }
    }
}
