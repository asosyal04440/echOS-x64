//! IronShim app personality bridge for packaged PE/ELF runtimes.

use super::ecosystem_exactness;
use super::gui::launch_pipeline::AbiPersonality;
use super::runtime_layer::process_broker_contract::{
    BrokeredLaunch, CapabilityTokenId, ProcessBroker, ProcessBrokerTicket,
};
use alloc::vec::Vec;
use ironshim_rs::{enforce_syscall, AuditEvent, AuditSink, Error, SyscallPolicy, SyscallRequest};

pub const ABI_REQUEST_CAPABILITY: u32 = 0x1000;
pub const ABI_OPEN_FILE_GRANT: u32 = 0x1001;
pub const ABI_OPEN_DIALOG: u32 = 0x1002;
pub const ABI_POST_NOTIFICATION: u32 = 0x1003;
pub const ABI_CREATE_WINDOW: u32 = 0x1004;
pub const ABI_COMMIT_SCENE: u32 = 0x1005;
pub const ABI_POLL_EVENTS: u32 = 0x1006;
pub const ABI_EXPORT_LIFECYCLE_STATE: u32 = 0x1007;
pub const ABI_ATTACH_SESSION: u32 = 0x1008;
pub const ABI_UNSUPPORTED: u32 = 0x1fff;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdapterKind {
    Native,
    Win32,
    Posix,
}

impl AdapterKind {
    pub const fn from_personality(personality: AbiPersonality) -> Self {
        match personality {
            AbiPersonality::Native => Self::Native,
            AbiPersonality::Win32 => Self::Win32,
            AbiPersonality::Posix => Self::Posix,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IronShimLaunchEnvelope {
    pub ticket: ProcessBrokerTicket,
    pub capability_token: CapabilityTokenId,
    pub address_space_handle: u64,
    pub session_contract: u64,
    pub resume_token: Option<u64>,
    pub adapter: AdapterKind,
}

pub fn prepare_packaged_bridge(
    launch: &BrokeredLaunch,
    personality: AbiPersonality,
) -> IronShimLaunchEnvelope {
    IronShimLaunchEnvelope {
        ticket: launch.ticket,
        capability_token: launch.token.id,
        address_space_handle: launch.address_space_handle,
        session_contract: launch.session_contract,
        resume_token: launch.resume_token,
        adapter: AdapterKind::from_personality(personality),
    }
}

pub fn translate_win32_request(request_number: u32, arg0: usize) -> SyscallRequest {
    match request_number {
        1 => SyscallRequest {
            number: ABI_CREATE_WINDOW,
            args: [arg0, 0, 0, 0, 0, 0],
        },
        2 => SyscallRequest {
            number: ABI_COMMIT_SCENE,
            args: [arg0, 0, 0, 0, 0, 0],
        },
        3 => SyscallRequest {
            number: ABI_OPEN_DIALOG,
            args: [arg0, 0, 0, 0, 0, 0],
        },
        4 => SyscallRequest {
            number: ABI_POST_NOTIFICATION,
            args: [arg0, 0, 0, 0, 0, 0],
        },
        _ => SyscallRequest {
            number: ABI_UNSUPPORTED,
            args: [request_number as usize, arg0, 0, 0, 0, 0],
        },
    }
}

pub fn translate_posix_request(syscall_number: u32, arg0: usize) -> SyscallRequest {
    match syscall_number {
        56 => SyscallRequest {
            number: ABI_ATTACH_SESSION,
            args: [arg0, 0, 0, 0, 0, 0],
        },
        257 => SyscallRequest {
            number: ABI_OPEN_FILE_GRANT,
            args: [arg0, 0, 0, 0, 0, 0],
        },
        1 => SyscallRequest {
            number: ABI_COMMIT_SCENE,
            args: [arg0, 0, 0, 0, 0, 0],
        },
        _ => SyscallRequest {
            number: ABI_UNSUPPORTED,
            args: [syscall_number as usize, arg0, 0, 0, 0, 0],
        },
    }
}

pub fn enforce_bridge_request(
    launch: &BrokeredLaunch,
    request: &SyscallRequest,
) -> Result<(), Error> {
    let policy = AppBridgePolicy {
        launch: launch.clone(),
    };
    let audit = AuditRecorder::default();
    let result = enforce_syscall(&policy, &audit, request);
    if matches!(result, Err(Error::Unsupported)) {
        ecosystem_exactness::record_ironshim_unsupported(alloc::format!(
            "{:?}:req={}",
            launch.process_class,
            request.args[0]
        ));
    }
    result
}

#[derive(Clone)]
struct AppBridgePolicy {
    launch: BrokeredLaunch,
}

impl SyscallPolicy for AppBridgePolicy {
    fn check(&self, request: &SyscallRequest) -> Result<(), Error> {
        match request.number {
            ABI_REQUEST_CAPABILITY
            | ABI_ATTACH_SESSION
            | ABI_POLL_EVENTS
            | ABI_CREATE_WINDOW
            | ABI_COMMIT_SCENE => Ok(()),
            ABI_OPEN_FILE_GRANT => {
                if self.launch.token.capabilities.file_system {
                    Ok(())
                } else {
                    Err(Error::AccessDenied)
                }
            }
            ABI_OPEN_DIALOG => {
                if self.launch.token.capabilities.file_dialogs {
                    Ok(())
                } else {
                    Err(Error::AccessDenied)
                }
            }
            ABI_POST_NOTIFICATION => {
                if self.launch.token.capabilities.notifications {
                    Ok(())
                } else {
                    Err(Error::AccessDenied)
                }
            }
            ABI_EXPORT_LIFECYCLE_STATE => Ok(()),
            _ => Err(Error::Unsupported),
        }
    }
}

#[derive(Default)]
struct AuditRecorder {
    events: Vec<AuditEvent>,
}

impl AuditSink for AuditRecorder {
    fn record(&self, _event: AuditEvent) {}
}

#[cfg(test)]
mod tests {
    use super::super::ecosystem_exactness::{
        reset_runtime_counters, snapshot, ExactnessSurfaceKind,
    };
    use super::super::gui::launch_pipeline::{
        AbiPersonality, AppDescriptor, AppPresentation, CapabilityProfile, ExecutionContext,
        LaunchIntent, LaunchSource, LoaderDispatch,
    };
    use super::super::runtime_layer::launch_contract::IsolationDomain;
    use super::super::runtime_layer::process_broker_contract::{BrokeredLaunch, ProcessBroker};
    use super::{
        enforce_bridge_request, prepare_packaged_bridge, translate_posix_request,
        translate_win32_request, ABI_OPEN_DIALOG, ABI_OPEN_FILE_GRANT, ABI_UNSUPPORTED,
    };

    fn brokered_launch(profile: CapabilityProfile) -> BrokeredLaunch {
        let descriptor = AppDescriptor::new(
            19,
            "demo",
            "Demo",
            LoaderDispatch::Pe,
            AbiPersonality::Win32,
            AppPresentation::ShellOwned,
            profile,
        );
        let session = LaunchIntent::new(
            descriptor,
            ExecutionContext::new(LaunchSource::Launcher, 1, "launcher"),
        )
        .canonical_session();
        ProcessBroker::new().authorize_launch(
            session,
            Some("/apps/demo/demo.exe"),
            IsolationDomain::UserProcess,
        )
    }

    #[test]
    fn adapters_map_to_shared_broker_abi() {
        let win32 = translate_win32_request(3, 7);
        let posix = translate_posix_request(257, 7);
        assert_eq!(win32.number, ABI_OPEN_DIALOG);
        assert_eq!(posix.number, ABI_OPEN_FILE_GRANT);
    }

    #[test]
    fn bridge_policy_reuses_capability_profile() {
        let launch = brokered_launch(CapabilityProfile {
            file_system: false,
            file_dialogs: false,
            notifications: true,
        });
        let denied = enforce_bridge_request(&launch, &translate_posix_request(257, 0));
        assert!(denied.is_err());

        let allowed = enforce_bridge_request(&launch, &translate_win32_request(4, 0));
        assert!(allowed.is_ok());
    }

    #[test]
    fn bridge_envelope_reuses_broker_grant_ids() {
        let launch = brokered_launch(CapabilityProfile::file_worker());
        let envelope = prepare_packaged_bridge(&launch, AbiPersonality::Win32);
        assert_eq!(envelope.ticket, launch.ticket);
        assert_eq!(envelope.capability_token, launch.token.id);
    }

    #[test]
    fn unknown_translation_surfaces_are_not_silently_reinterpreted() {
        reset_runtime_counters();
        let launch = brokered_launch(CapabilityProfile::file_worker());
        let request = translate_win32_request(99, 7);
        assert_eq!(request.number, ABI_UNSUPPORTED);
        let result = enforce_bridge_request(&launch, &request);
        assert!(result.is_err());
        let snapshot = snapshot();
        assert!(snapshot.runtime_counters.iter().any(|entry| {
            entry.kind == ExactnessSurfaceKind::IronShimUnsupported
                && entry.subject.contains("req=99")
        }));
    }
}
