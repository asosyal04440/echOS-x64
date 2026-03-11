//! # Service IPC (Faz 3)
//!
//! Uygulamalar ve sistem servisleri arasındaki IPC iletişimi.
//! EchDisplay, EchInput, EchAudio, EchStore servisleri ile uygulama iletişimi.
//!
//! ## Mimari
//!
//! ```text
//!  Uygulama                    Servis
//! ┌─────────────────┐        ┌─────────────────┐
//! │ send_to_service │ ──►    │ service mailbox │
//! │                 │        │                 │
//! │ receive_from_  │ ◄───── │ response queue  │
//! │   service()     │        │                 │
//! └─────────────────┘        └─────────────────┘
//! ```
//!
//! ## Servis Kimlikleri
//!
//! - **EchDisplay**: 1
//! - **EchInput**: 2
//! - **EchAudio**: 3
//! - **EchStore**: 4

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use lazy_static::lazy_static;

use crate::services::ech_audio::get_audio;
use crate::services::ech_display::get_display;
use crate::services::ech_input::get_input;
use crate::services::ech_store::get_store;

/// Servis kimlikleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceId {
    EchDisplay = 1,
    EchInput = 2,
    EchAudio = 3,
    EchStore = 4,
}

/// IPC mesajı
#[derive(Clone, Debug)]
pub enum ServiceMessage {
    /// EchDisplay komutları
    DisplayCommand(crate::services::DisplayCommand),
    /// EchInput komutları
    InputCommand(crate::services::InputCommand),
    /// EchAudio komutları
    AudioCommand(crate::services::AudioCommand),
    /// EchStore komutları
    StoreCommand(crate::services::StoreCommand),
}

/// IPC yanıtı
#[derive(Clone, Debug)]
pub enum ServiceResponse {
    /// EchDisplay yanıtı
    DisplayResponse(crate::services::DisplayResponse),
    /// EchInput yanıtı
    InputResponse(crate::services::InputResponse),
    /// EchAudio yanıtı
    AudioResponse(crate::services::AudioResponse),
    /// EchStore yanıtı
    StoreResponse(crate::services::StoreResponse),
}

/// Mesaj kimliği (benzersiz)
static MESSAGE_ID: AtomicU64 = AtomicU64::new(0);

/// Mesaj zarfı
#[derive(Clone, Debug)]
pub struct MessageEnvelope {
    pub id: u64,
    pub from_app: u32, // Uygulama ID'si
    pub to_service: ServiceId,
    pub message: ServiceMessage,
}

/// Yanıt zarfı
#[derive(Clone, Debug)]
pub struct ResponseEnvelope {
    pub message_id: u64,
    pub response: ServiceResponse,
}

/// Servis IPC yöneticisi
pub struct ServiceIpcManager {
    /// Giden mesaj kuyruğu
    outgoing: Mutex<Vec<MessageEnvelope>>,
    /// Gelen yanıt kuyruğu
    incoming: Mutex<Vec<ResponseEnvelope>>,
}

impl ServiceIpcManager {
    /// Yeni IPC yöneticisi oluştur
    pub fn new() -> Self {
        Self {
            outgoing: Mutex::new(Vec::new()),
            incoming: Mutex::new(Vec::new()),
        }
    }

    /// Servise mesaj gönder
    pub fn send_to_service(&self, app_id: u32, service: ServiceId, message: ServiceMessage) -> u64 {
        let message_id = MESSAGE_ID.fetch_add(1, Ordering::SeqCst);

        let envelope = MessageEnvelope {
            id: message_id,
            from_app: app_id,
            to_service: service,
            message,
        };

        self.outgoing.lock().push(envelope);
        message_id
    }

    /// Servisten yanıt al (non-blocking)
    pub fn receive_from_service(&self) -> Option<ResponseEnvelope> {
        self.incoming.lock().pop()
    }

    /// Servisten yanıt al (blocking, timeout ile)
    pub fn receive_from_service_timeout(&self, _timeout_ms: u32) -> Option<ResponseEnvelope> {
        // Basit implementasyon - gerçekte timeout mekanizması eklenir
        self.receive_from_service()
    }

    pub fn take_response_for(&self, message_id: u64) -> Option<ResponseEnvelope> {
        let mut incoming = self.incoming.lock();
        if let Some(pos) = incoming.iter().position(|r| r.message_id == message_id) {
            Some(incoming.remove(pos))
        } else {
            None
        }
    }

    /// Bekleyen mesajları işle
    pub fn process_pending_messages(&self) {
        let messages = {
            let mut outgoing = self.outgoing.lock();
            core::mem::take(&mut *outgoing)
        };

        for envelope in messages {
            let response = self.dispatch_to_service(envelope.clone());

            let response_envelope = ResponseEnvelope {
                message_id: envelope.id,
                response,
            };

            self.incoming.lock().push(response_envelope);
        }
    }

    /// Mesajı ilgili servise yönlendir
    fn dispatch_to_service(&self, envelope: MessageEnvelope) -> ServiceResponse {
        match envelope.to_service {
            ServiceId::EchDisplay => {
                if let ServiceMessage::DisplayCommand(cmd) = envelope.message {
                    let display = crate::services::ech_display::get_display().lock().clone();
                    if let Some(display) = display {
                        ServiceResponse::DisplayResponse(display.process_command(cmd))
                    } else {
                        ServiceResponse::DisplayResponse(crate::services::DisplayResponse::Error(
                            String::from("EchDisplay not initialized"),
                        ))
                    }
                } else {
                    ServiceResponse::DisplayResponse(crate::services::DisplayResponse::Error(
                        String::from("Invalid message type for EchDisplay"),
                    ))
                }
            }
            ServiceId::EchInput => {
                if let ServiceMessage::InputCommand(cmd) = envelope.message {
                    let input = crate::services::ech_input::get_input();
                    input.send_command(cmd);
                    // For now, return success - real implementation would wait for response
                    ServiceResponse::InputResponse(crate::services::InputResponse::Ack)
                } else {
                    ServiceResponse::InputResponse(crate::services::InputResponse::Error(
                        String::from("Invalid message type for EchInput"),
                    ))
                }
            }
            ServiceId::EchAudio => {
                if let ServiceMessage::AudioCommand(cmd) = envelope.message {
                    let audio = crate::services::ech_audio::get_audio();
                    audio.send_command(cmd);
                    // For now, return success
                    ServiceResponse::AudioResponse(crate::services::AudioResponse::Success)
                } else {
                    ServiceResponse::AudioResponse(crate::services::AudioResponse::Error(
                        String::from("Invalid message type for EchAudio"),
                    ))
                }
            }
            ServiceId::EchStore => {
                if let ServiceMessage::StoreCommand(cmd) = envelope.message {
                    let store = crate::services::ech_store::get_store();
                    store.send_command(cmd);
                    // For now, return success
                    ServiceResponse::StoreResponse(crate::services::StoreResponse::Success)
                } else {
                    ServiceResponse::StoreResponse(crate::services::StoreResponse::Error(
                        String::from("Invalid message type for EchStore"),
                    ))
                }
            }
        }
    }
}

/// Global Service IPC yöneticisi
lazy_static::lazy_static! {
    static ref SERVICE_IPC: Mutex<ServiceIpcManager> = Mutex::new(ServiceIpcManager::new());
}

/// Service IPC'yi başlat
pub fn init() {
    crate::serial_println!("[SERVICE_IPC] Initialized");
}

/// Global Service IPC yöneticisini al
pub fn get_service_ipc() -> &'static Mutex<ServiceIpcManager> {
    &SERVICE_IPC
}

/// Uygulamadan servise kolay mesaj gönderme fonksiyonu
pub fn send_to_display(app_id: u32, command: crate::services::DisplayCommand) -> u64 {
    let ipc = get_service_ipc();
    ipc.lock().send_to_service(
        app_id,
        ServiceId::EchDisplay,
        ServiceMessage::DisplayCommand(command),
    )
}

pub fn send_to_input(app_id: u32, command: crate::services::InputCommand) -> u64 {
    let ipc = get_service_ipc();
    ipc.lock().send_to_service(
        app_id,
        ServiceId::EchInput,
        ServiceMessage::InputCommand(command),
    )
}

pub fn send_to_audio(app_id: u32, command: crate::services::AudioCommand) -> u64 {
    let ipc = get_service_ipc();
    ipc.lock().send_to_service(
        app_id,
        ServiceId::EchAudio,
        ServiceMessage::AudioCommand(command),
    )
}

pub fn send_to_store(app_id: u32, command: crate::services::StoreCommand) -> u64 {
    let ipc = get_service_ipc();
    ipc.lock().send_to_service(
        app_id,
        ServiceId::EchStore,
        ServiceMessage::StoreCommand(command),
    )
}

pub fn request_store_sync(
    app_id: u32,
    command: crate::services::StoreCommand,
) -> Option<crate::services::StoreResponse> {
    let message_id = send_to_store(app_id, command);
    for _ in 0..10000 {
        process_messages();
        if let Some(resp) = get_service_ipc().lock().take_response_for(message_id) {
            if let ServiceResponse::StoreResponse(r) = resp.response {
                return Some(r);
            }
        }
        core::hint::spin_loop();
    }
    None
}

pub fn request_audio_sync(
    app_id: u32,
    command: crate::services::AudioCommand,
) -> Option<crate::services::AudioResponse> {
    let message_id = send_to_audio(app_id, command);
    for _ in 0..10000 {
        process_messages();
        if let Some(resp) = get_service_ipc().lock().take_response_for(message_id) {
            if let ServiceResponse::AudioResponse(r) = resp.response {
                return Some(r);
            }
        }
        core::hint::spin_loop();
    }
    None
}

/// Servisten yanıt alma fonksiyonu
pub fn receive_response() -> Option<ResponseEnvelope> {
    let ipc = get_service_ipc();
    ipc.lock().receive_from_service()
}

/// Bekleyen mesajları işle (kernel task'tan çağrılır)
pub fn process_messages() {
    let ipc = get_service_ipc();
    ipc.lock().process_pending_messages();
}

pub fn service_task() -> ! {
    loop {
        process_messages();
        for _ in 0..1000 {
            core::hint::spin_loop();
        }
    }
}

pub fn spawn_task() {
    crate::task::scheduler::spawn_with_priority(
        service_task,
        crate::task::task::Priority::Low,
        "service_ipc",
    );
}
