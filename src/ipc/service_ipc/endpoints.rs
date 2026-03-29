use super::*;

#[derive(Clone)]
pub(super) enum BoundServiceEndpoint {
    Display(Arc<EchDisplay>),
    Input(Arc<EchInput>),
    Audio(Arc<EchAudio>),
    Store(Arc<EchStore>),
    Shell(Arc<EchShell>),
    Notifications(Arc<EchNotifications>),
    Clipboard(Arc<EchClipboard>),
    Dialogs(Arc<EchDialogs>),
    Capture(Arc<EchCapture>),
}

#[derive(Clone)]
pub enum ServiceEndpointRegistration {
    Display(Arc<EchDisplay>),
    Input(Arc<EchInput>),
    Audio(Arc<EchAudio>),
    Store(Arc<EchStore>),
    Shell(Arc<EchShell>),
    Notifications(Arc<EchNotifications>),
    Clipboard(Arc<EchClipboard>),
    Dialogs(Arc<EchDialogs>),
    Capture(Arc<EchCapture>),
}

impl ServiceEndpointRegistration {
    pub(super) fn into_bound(self) -> BoundServiceEndpoint {
        match self {
            Self::Display(endpoint) => BoundServiceEndpoint::Display(endpoint),
            Self::Input(endpoint) => BoundServiceEndpoint::Input(endpoint),
            Self::Audio(endpoint) => BoundServiceEndpoint::Audio(endpoint),
            Self::Store(endpoint) => BoundServiceEndpoint::Store(endpoint),
            Self::Shell(endpoint) => BoundServiceEndpoint::Shell(endpoint),
            Self::Notifications(endpoint) => BoundServiceEndpoint::Notifications(endpoint),
            Self::Clipboard(endpoint) => BoundServiceEndpoint::Clipboard(endpoint),
            Self::Dialogs(endpoint) => BoundServiceEndpoint::Dialogs(endpoint),
            Self::Capture(endpoint) => BoundServiceEndpoint::Capture(endpoint),
        }
    }
}

impl BoundServiceEndpoint {
    pub(super) fn dispatch_sync(
        &self,
        message: ServiceMessage,
    ) -> Result<ServiceResponse, ServiceError> {
        match self {
            Self::Display(display) => match message {
                ServiceMessage::DisplayCommand(cmd) => Ok(ServiceResponse::DisplayResponse(
                    display.process_command(cmd),
                )),
                _ => Err(ServiceError::WrongService),
            },
            Self::Input(input) => match message {
                ServiceMessage::InputCommand(cmd) => {
                    Ok(ServiceResponse::InputResponse(input.process_command(cmd)))
                }
                _ => Err(ServiceError::WrongService),
            },
            Self::Audio(audio) => match message {
                ServiceMessage::AudioCommand(cmd) => {
                    Ok(ServiceResponse::AudioResponse(audio.process_command(cmd)))
                }
                _ => Err(ServiceError::WrongService),
            },
            Self::Store(store) => match message {
                ServiceMessage::StoreCommand(cmd) => {
                    Ok(ServiceResponse::StoreResponse(store.process_command(cmd)))
                }
                _ => Err(ServiceError::WrongService),
            },
            Self::Shell(shell) => match message {
                ServiceMessage::ShellCommand(cmd) => {
                    Ok(ServiceResponse::ShellResponse(shell.process_command(cmd)))
                }
                _ => Err(ServiceError::WrongService),
            },
            Self::Notifications(notifications) => match message {
                ServiceMessage::NotificationCommand(cmd) => Ok(
                    ServiceResponse::NotificationResponse(notifications.process_command(cmd)),
                ),
                _ => Err(ServiceError::WrongService),
            },
            Self::Clipboard(clipboard) => match message {
                ServiceMessage::ClipboardCommand(cmd) => Ok(ServiceResponse::ClipboardResponse(
                    clipboard.process_command(cmd),
                )),
                _ => Err(ServiceError::WrongService),
            },
            Self::Dialogs(dialogs) => match message {
                ServiceMessage::DialogCommand(cmd) => Ok(ServiceResponse::DialogResponse(
                    dialogs.process_command(cmd),
                )),
                _ => Err(ServiceError::WrongService),
            },
            Self::Capture(capture) => match message {
                ServiceMessage::CaptureCommand(cmd) => Ok(ServiceResponse::CaptureResponse(
                    capture.process_command(cmd),
                )),
                _ => Err(ServiceError::WrongService),
            },
        }
    }

    pub(super) fn enqueue(&self, message: ServiceMessage) -> Result<(), ServiceError> {
        match self {
            Self::Display(display) => match message {
                ServiceMessage::DisplayCommand(cmd) => display
                    .send_command(cmd)
                    .then_some(())
                    .ok_or(ServiceError::QueueFull),
                _ => Err(ServiceError::WrongService),
            },
            Self::Input(input) => match message {
                ServiceMessage::InputCommand(cmd) => input
                    .send_command(cmd)
                    .then_some(())
                    .ok_or(ServiceError::QueueFull),
                _ => Err(ServiceError::WrongService),
            },
            Self::Audio(audio) => match message {
                ServiceMessage::AudioCommand(cmd) => audio
                    .send_command(cmd)
                    .then_some(())
                    .ok_or(ServiceError::QueueFull),
                _ => Err(ServiceError::WrongService),
            },
            Self::Store(store) => match message {
                ServiceMessage::StoreCommand(cmd) => store
                    .send_command(cmd)
                    .then_some(())
                    .ok_or(ServiceError::QueueFull),
                _ => Err(ServiceError::WrongService),
            },
            Self::Shell(shell) => match message {
                ServiceMessage::ShellCommand(cmd) => shell
                    .send_command(cmd)
                    .then_some(())
                    .ok_or(ServiceError::QueueFull),
                _ => Err(ServiceError::WrongService),
            },
            Self::Notifications(notifications) => match message {
                ServiceMessage::NotificationCommand(cmd) => notifications
                    .send_command(cmd)
                    .then_some(())
                    .ok_or(ServiceError::QueueFull),
                _ => Err(ServiceError::WrongService),
            },
            Self::Clipboard(clipboard) => match message {
                ServiceMessage::ClipboardCommand(cmd) => clipboard
                    .send_command(cmd)
                    .then_some(())
                    .ok_or(ServiceError::QueueFull),
                _ => Err(ServiceError::WrongService),
            },
            Self::Dialogs(dialogs) => match message {
                ServiceMessage::DialogCommand(cmd) => dialogs
                    .send_command(cmd)
                    .then_some(())
                    .ok_or(ServiceError::QueueFull),
                _ => Err(ServiceError::WrongService),
            },
            Self::Capture(capture) => match message {
                ServiceMessage::CaptureCommand(cmd) => capture
                    .send_command(cmd)
                    .then_some(())
                    .ok_or(ServiceError::QueueFull),
                _ => Err(ServiceError::WrongService),
            },
        }
    }

    pub(super) fn try_receive(&self) -> Option<ServiceResponse> {
        match self {
            Self::Display(display) => display
                .receive_response()
                .map(ServiceResponse::DisplayResponse),
            Self::Input(input) => input.receive_response().map(ServiceResponse::InputResponse),
            Self::Audio(audio) => audio.receive_response().map(ServiceResponse::AudioResponse),
            Self::Store(store) => store.receive_response().map(ServiceResponse::StoreResponse),
            Self::Shell(shell) => shell.receive_response().map(ServiceResponse::ShellResponse),
            Self::Notifications(notifications) => notifications
                .receive_response()
                .map(ServiceResponse::NotificationResponse),
            Self::Clipboard(clipboard) => clipboard
                .receive_response()
                .map(ServiceResponse::ClipboardResponse),
            Self::Dialogs(dialogs) => dialogs
                .receive_response()
                .map(ServiceResponse::DialogResponse),
            Self::Capture(capture) => capture
                .receive_response()
                .map(ServiceResponse::CaptureResponse),
        }
    }
}

impl ServiceIpcManager {
    pub fn register_endpoint(&self, service: ServiceId, endpoint: ServiceEndpointRegistration) {
        let mut endpoints = self.endpoints.lock();
        let mut generations = self.endpoint_generations.lock();
        publish_endpoint(
            &mut endpoints,
            &mut generations,
            service,
            endpoint.into_bound(),
        );
    }
}

pub(super) fn publish_endpoint(
    endpoints: &mut BTreeMap<ServiceId, BoundServiceEndpoint>,
    generations: &mut BTreeMap<ServiceId, EndpointGeneration>,
    service: ServiceId,
    endpoint: BoundServiceEndpoint,
) {
    endpoints.insert(service, endpoint);
    let generation = generations.entry(service).or_insert(0);
    *generation = generation.saturating_add(1).max(1);
}

pub(super) fn blocking_sync_allowed(current: ServiceId, target: ServiceId) -> bool {
    matches!(
        (current, target),
        (ServiceId::EchInput, ServiceId::EchDisplay)
            | (ServiceId::EchCapture, ServiceId::EchDisplay)
            | (ServiceId::EchCapture, ServiceId::EchShell)
            | (ServiceId::EchClipboard, ServiceId::EchShell)
            | (ServiceId::EchDialogs, ServiceId::EchShell)
            | (ServiceId::EchNotifications, ServiceId::EchShell)
    )
}
