use super::handles::default_rights_for;
use super::*;

impl ServiceIpcManager {
    fn record_legacy_sync(&self, service: ServiceId, callsite: &'static str) {
        *self
            .legacy_sync_by_service
            .lock()
            .entry(service)
            .or_insert(0) += 1;
        *self
            .legacy_sync_by_callsite
            .lock()
            .entry(callsite)
            .or_insert(0) += 1;
    }

    fn migrated_legacy_services() -> &'static [ServiceId] {
        &[
            ServiceId::EchDisplay,
            ServiceId::EchInput,
            ServiceId::EchShell,
        ]
    }

    fn strict_legacy_sync_violation(&self, service: ServiceId) -> bool {
        strict_full_parity_mode_enabled() && Self::migrated_legacy_services().contains(&service)
    }

    fn record_legacy_sync_strict_violation(&self, service: ServiceId, callsite: &'static str) {
        *self
            .legacy_sync_strict_violation_by_service
            .lock()
            .entry(service)
            .or_insert(0) += 1;
        *self
            .legacy_sync_strict_violation_by_callsite
            .lock()
            .entry(callsite)
            .or_insert(0) += 1;
    }

    pub fn migrated_legacy_sync_clear(&self) -> bool {
        let counts = self.legacy_sync_by_service.lock().clone();
        Self::migrated_legacy_services()
            .iter()
            .all(|service| counts.get(service).copied().unwrap_or(0) == 0)
    }

    pub fn legacy_sync_metrics(&self) -> LegacySyncMetrics {
        let by_service = {
            let guard = self.legacy_sync_by_service.lock();
            guard.clone()
        };
        let by_callsite = {
            let guard = self.legacy_sync_by_callsite.lock();
            guard.clone()
        };
        let strict_violation_by_service = {
            let guard = self.legacy_sync_strict_violation_by_service.lock();
            guard.clone()
        };
        let strict_violation_by_callsite = {
            let guard = self.legacy_sync_strict_violation_by_callsite.lock();
            guard.clone()
        };
        LegacySyncMetrics {
            by_service,
            by_callsite,
            strict_violation_by_service,
            strict_violation_by_callsite,
            migrated_services_clear: self.migrated_legacy_sync_clear(),
        }
    }

    pub fn request_sync_legacy(
        &self,
        app_id: u32,
        service: ServiceId,
        message: ServiceMessage,
    ) -> ServiceResponse {
        self.request_sync_compat(app_id, service, message, "request_sync_legacy")
    }

    pub fn request_sync_compat(
        &self,
        app_id: u32,
        service: ServiceId,
        message: ServiceMessage,
        callsite: &'static str,
    ) -> ServiceResponse {
        self.record_legacy_sync_probe(service, callsite);
        if let Some(response) = self.dispatch_sync_compat_endpoint(service, &message) {
            return response;
        }
        let Ok(handle) = self.open_service_handle(app_id, service, default_rights_for(service))
        else {
            return service_unavailable_response(service);
        };
        let response = self
            .request_sync(app_id, handle.handle, message)
            .unwrap_or_else(|_| service_unavailable_response(service));
        let _ = self.revoke_handle(app_id, handle.handle);
        response
    }

    pub(crate) fn record_legacy_sync_probe(
        &self,
        service: ServiceId,
        callsite: &'static str,
    ) -> bool {
        self.record_legacy_sync(service, callsite);
        let strict_violation = self.strict_legacy_sync_violation(service);
        if strict_violation {
            self.record_legacy_sync_strict_violation(service, callsite);
            if !cfg!(test) {
                crate::serial_println!(
                    "[SERVICE_IPC][LEGACY_SYNC][STRICT] service={:?} callsite={}",
                    service,
                    callsite
                );
            }
        }
        strict_violation
    }

    fn dispatch_sync_compat_endpoint(
        &self,
        service: ServiceId,
        message: &ServiceMessage,
    ) -> Option<ServiceResponse> {
        if self.route_to_user_published_endpoint(service) {
            return None;
        }
        let endpoint = self.endpoints.lock().get(&service).cloned()?;
        endpoint.dispatch_sync(message.clone()).ok()
    }
}
