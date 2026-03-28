#![no_std]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
pub use echos_rt::{validate_runtime_state, ResumeReason, RuntimeState};
use echos_sdk_sys::{
    syscall1, syscall2, NativeClipboardGetTextResponse, NativeClipboardSetTextRequest,
    NativeEventKind, NativeInputEvent, NativeNotificationRequest, NativeSceneOp,
    NativeSceneSubmitRequest, NativeServiceBootstrap, NativeServiceEndpointPublishRequest,
    NativeServiceEndpointState, NativeServiceParityStatus, NativeServiceRegionMapping,
    NativeServiceStatus, NativeWindowCreateRequest, NativeWindowHandle, EACCES, EFBIG, EINVAL,
    ENOSYS, MAX_INLINE_TEXT, MAX_POLLED_EVENTS, SYS_ECHOS_CLIPBOARD_GET_TEXT,
    SYS_ECHOS_CLIPBOARD_SET_TEXT, SYS_ECHOS_EVENT_POLL, SYS_ECHOS_NOTIFICATION_POST,
    SYS_ECHOS_SCENE_COMMIT, SYS_ECHOS_SERVICE_BOOTSTRAP_CLAIM, SYS_ECHOS_SERVICE_ENDPOINT_PUBLISH,
    SYS_ECHOS_SERVICE_HEARTBEAT, SYS_ECHOS_SERVICE_PARITY_STATUS, SYS_ECHOS_SERVICE_REGION_MAP,
    SYS_ECHOS_SERVICE_STATUS, SYS_ECHOS_WIN_CREATE, SYS_ECHOS_WIN_DESTROY,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    AccessDenied,
    InvalidInput,
    Unsupported,
    StateTooLarge,
    SyscallFailed(isize),
    Utf8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowOptions {
    pub title: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl WindowOptions {
    pub fn new(title: &str, width: u32, height: u32) -> Self {
        Self {
            title: title.to_string(),
            x: 120,
            y: 96,
            width,
            height,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotificationLevel {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    None,
    Key {
        window_id: u64,
        key_code: u32,
        modifiers: u8,
        pressed: bool,
    },
    PointerMove {
        window_id: u64,
        x: i32,
        y: i32,
        delta_x: i32,
        delta_y: i32,
    },
    PointerButton {
        window_id: u64,
        x: i32,
        y: i32,
        button: u8,
        pressed: bool,
    },
    CloseRequested {
        window_id: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SceneOp {
    SolidRect {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        color: u32,
        radius: u16,
        z_index: u32,
        opacity: u8,
    },
    Text {
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        color: u32,
        z_index: u32,
        opacity: u8,
        monospace: bool,
        text: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scene {
    pub revision: u64,
    pub ops: Vec<SceneOp>,
}

impl Scene {
    pub fn new() -> Self {
        Self {
            revision: 1,
            ops: Vec::new(),
        }
    }

    pub fn push(&mut self, op: SceneOp) {
        self.ops.push(op);
    }
}

pub struct NotificationClient;
pub struct ClipboardClient;

pub struct AppContext {
    notifications: NotificationClient,
    clipboard: ClipboardClient,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceId {
    Directory = 0,
    EchDisplay = 1,
    EchInput = 2,
    EchAudio = 3,
    EchStore = 4,
    EchShell = 5,
    EchNotifications = 6,
    EchClipboard = 7,
    EchDialogs = 8,
    EchCapture = 9,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceIsolation {
    Unknown,
    KernelTask,
    UserProcess,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceBootstrap {
    pub service_id: ServiceId,
    pub runtime_app_id: u32,
    pub service_handle: u32,
    pub request_region_handle: u32,
    pub response_region_handle: u32,
    pub endpoint_generation: u32,
    pub rights_bits: u32,
    pub isolation: ServiceIsolation,
    pub runtime_task_id: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceStatus {
    pub service_id: ServiceId,
    pub openable_rights_bits: u32,
    pub endpoint_generation: u32,
    pub control_plane: bool,
    pub bulk_data_out_of_band: bool,
    pub service_process_available: bool,
    pub user_published_endpoint: bool,
    pub runtime_isolation: ServiceIsolation,
    pub runtime_task_id: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceParityStatusView {
    pub required_services: u32,
    pub packaged_service_slots: u32,
    pub live_user_process_slots: u32,
    pub published_user_process_slots: u32,
    pub strict_mode_enabled: bool,
    pub full_parity_ready: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceRegionMapping {
    pub region_handle: u32,
    pub writable: bool,
    pub region_id: u64,
    pub generation: u64,
    pub base: u64,
    pub len: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServiceEndpointState {
    pub service_id: ServiceId,
    pub request_region_id: u64,
    pub request_generation: u64,
    pub response_region_id: u64,
    pub response_generation: u64,
    pub heartbeat_epoch: u64,
}

pub struct ServiceContext {
    bootstrap: ServiceBootstrap,
    request_region: ServiceRegionMapping,
    response_region: ServiceRegionMapping,
}

impl ServiceContext {
    pub fn bootstrap(&self) -> &ServiceBootstrap {
        &self.bootstrap
    }

    pub fn request_region(&self) -> &ServiceRegionMapping {
        &self.request_region
    }

    pub fn response_region(&self) -> &ServiceRegionMapping {
        &self.response_region
    }

    pub fn heartbeat(&self) -> Result<ServiceEndpointState, Error> {
        service_heartbeat(self.bootstrap.service_id)
    }
}

pub trait ServiceApplication {
    fn bootstrap(&mut self, _ctx: &mut ServiceContext) -> Result<(), Error> {
        Ok(())
    }

    fn tick(&mut self, _ctx: &mut ServiceContext) -> Result<(), Error> {
        Ok(())
    }
}

impl AppContext {
    pub fn notifications(&self) -> &NotificationClient {
        &self.notifications
    }

    pub fn clipboard(&self) -> &ClipboardClient {
        &self.clipboard
    }

    pub fn validate_state(&self, state: &RuntimeState) -> Result<(), Error> {
        validate_runtime_state(state).map_err(|err| match err {
            echos_rt::RuntimeError::StateTooLarge => Error::StateTooLarge,
            echos_rt::RuntimeError::InvalidResumeRef => Error::InvalidInput,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    handle: NativeWindowHandle,
}

impl Window {
    pub fn id(&self) -> u64 {
        self.handle.window_id
    }

    pub fn surface_id(&self) -> u64 {
        self.handle.surface_id
    }

    pub fn size(&self) -> (u32, u32) {
        (self.handle.content_width, self.handle.content_height)
    }

    pub fn submit(&mut self, scene: &Scene) -> Result<(), Error> {
        let mut raw_ops = Vec::with_capacity(scene.ops.len());
        for op in &scene.ops {
            raw_ops.push(scene_op_to_raw(op)?);
        }
        let request = NativeSceneSubmitRequest {
            window_id: self.id(),
            revision: scene.revision,
            op_count: raw_ops.len() as u32,
            ops_ptr: raw_ops.as_ptr() as u64,
        };
        let rc = unsafe {
            syscall1(
                SYS_ECHOS_SCENE_COMMIT,
                (&request as *const NativeSceneSubmitRequest) as usize,
            )
        };
        if rc < 0 {
            Err(map_error(rc))
        } else {
            Ok(())
        }
    }

    pub fn close(self) -> Result<(), Error> {
        let rc = unsafe { syscall1(SYS_ECHOS_WIN_DESTROY, self.id() as usize) };
        if rc < 0 {
            Err(map_error(rc))
        } else {
            Ok(())
        }
    }
}

pub trait Application {
    fn configure(&mut self, _ctx: &mut AppContext) -> Result<WindowOptions, Error>;
    fn initial_scene(&mut self, _ctx: &mut AppContext) -> Result<Scene, Error>;
    fn on_event(
        &mut self,
        _ctx: &mut AppContext,
        _window: &mut Window,
        _event: Event,
    ) -> Result<Option<Scene>, Error> {
        Ok(None)
    }
    fn export_state(&mut self, _ctx: &mut AppContext) -> Result<Option<RuntimeState>, Error> {
        Ok(None)
    }
    fn import_state(&mut self, _ctx: &mut AppContext, _state: &[u8]) -> Result<(), Error> {
        Ok(())
    }
    fn resume(&mut self, _ctx: &mut AppContext, _reason: ResumeReason) -> Result<(), Error> {
        Ok(())
    }
}

pub fn run<A: Application>(mut app: A) -> ! {
    let mut ctx = AppContext {
        notifications: NotificationClient,
        clipboard: ClipboardClient,
    };
    let options = app
        .configure(&mut ctx)
        .unwrap_or_else(|_| WindowOptions::new("echOS App", 800, 520));
    let mut window = create_window(&options).unwrap_or_else(|_| panic_loop());
    if let Ok(scene) = app.initial_scene(&mut ctx) {
        let _ = window.submit(&scene);
    }
    let _ = app.resume(&mut ctx, ResumeReason::ColdStart);
    loop {
        match poll_event() {
            Ok(Event::None) => core::hint::spin_loop(),
            Ok(event) => {
                if let Ok(Some(scene)) = app.on_event(&mut ctx, &mut window, event) {
                    let _ = window.submit(&scene);
                }
            }
            Err(_) => core::hint::spin_loop(),
        }
    }
}

pub fn claim_service_bootstrap() -> Result<ServiceBootstrap, Error> {
    let mut raw = NativeServiceBootstrap {
        abi_version: 0,
        service_id: 0,
        runtime_app_id: 0,
        service_handle: 0,
        request_region_handle: 0,
        response_region_handle: 0,
        endpoint_generation: 0,
        rights_bits: 0,
        isolation_domain: 0,
        runtime_task_id: 0,
    };
    let rc = unsafe {
        syscall1(
            SYS_ECHOS_SERVICE_BOOTSTRAP_CLAIM,
            (&mut raw as *mut NativeServiceBootstrap) as usize,
        )
    };
    if rc < 0 {
        return Err(map_error(rc));
    }
    Ok(ServiceBootstrap {
        service_id: decode_service_id(raw.service_id)?,
        runtime_app_id: raw.runtime_app_id,
        service_handle: raw.service_handle,
        request_region_handle: raw.request_region_handle,
        response_region_handle: raw.response_region_handle,
        endpoint_generation: raw.endpoint_generation,
        rights_bits: raw.rights_bits,
        isolation: decode_service_isolation(raw.isolation_domain),
        runtime_task_id: raw.runtime_task_id,
    })
}

pub fn query_service_status(service_id: ServiceId) -> Result<ServiceStatus, Error> {
    let mut raw = NativeServiceStatus {
        abi_version: 0,
        service_id: service_id as u32,
        openable_rights_bits: 0,
        endpoint_generation: 0,
        control_plane: 0,
        bulk_data_out_of_band: 0,
        service_process_available: 0,
        user_published_endpoint: 0,
        runtime_isolation: 0,
        runtime_task_id: 0,
    };
    let rc = unsafe {
        syscall2(
            SYS_ECHOS_SERVICE_STATUS,
            service_id as usize,
            (&mut raw as *mut NativeServiceStatus) as usize,
        )
    };
    if rc < 0 {
        return Err(map_error(rc));
    }
    Ok(ServiceStatus {
        service_id: decode_service_id(raw.service_id)?,
        openable_rights_bits: raw.openable_rights_bits,
        endpoint_generation: raw.endpoint_generation,
        control_plane: raw.control_plane != 0,
        bulk_data_out_of_band: raw.bulk_data_out_of_band != 0,
        service_process_available: raw.service_process_available != 0,
        user_published_endpoint: raw.user_published_endpoint != 0,
        runtime_isolation: decode_service_isolation(raw.runtime_isolation as u32),
        runtime_task_id: raw.runtime_task_id,
    })
}

pub fn query_service_parity_status() -> Result<ServiceParityStatusView, Error> {
    let mut raw = NativeServiceParityStatus {
        abi_version: 0,
        required_services: 0,
        packaged_service_slots: 0,
        live_user_process_slots: 0,
        published_user_process_slots: 0,
        strict_mode_enabled: 0,
        full_parity_ready: 0,
        reserved: [0; 6],
    };
    let rc = unsafe {
        syscall1(
            SYS_ECHOS_SERVICE_PARITY_STATUS,
            (&mut raw as *mut NativeServiceParityStatus) as usize,
        )
    };
    if rc < 0 {
        return Err(map_error(rc));
    }
    Ok(ServiceParityStatusView {
        required_services: raw.required_services,
        packaged_service_slots: raw.packaged_service_slots,
        live_user_process_slots: raw.live_user_process_slots,
        published_user_process_slots: raw.published_user_process_slots,
        strict_mode_enabled: raw.strict_mode_enabled != 0,
        full_parity_ready: raw.full_parity_ready != 0,
    })
}

pub fn map_service_region(
    region_handle: u32,
    writable: bool,
) -> Result<ServiceRegionMapping, Error> {
    let mut raw = NativeServiceRegionMapping {
        abi_version: 0,
        region_handle,
        writable: writable as u32,
        region_id: 0,
        generation: 0,
        base: 0,
        len: 0,
    };
    let rc = unsafe {
        syscall1(
            SYS_ECHOS_SERVICE_REGION_MAP,
            (&mut raw as *mut NativeServiceRegionMapping) as usize,
        )
    };
    if rc < 0 {
        return Err(map_error(rc));
    }
    Ok(ServiceRegionMapping {
        region_handle: raw.region_handle,
        writable: raw.writable != 0,
        region_id: raw.region_id,
        generation: raw.generation,
        base: raw.base,
        len: raw.len,
    })
}

pub fn publish_service_endpoint(
    service_id: ServiceId,
    request_region_handle: u32,
    response_region_handle: u32,
) -> Result<ServiceEndpointState, Error> {
    let mut raw = NativeServiceEndpointPublishRequest {
        abi_version: 1,
        service_id: service_id as u32,
        request_region_handle,
        response_region_handle,
    };
    let rc = unsafe {
        syscall1(
            SYS_ECHOS_SERVICE_ENDPOINT_PUBLISH,
            (&mut raw as *mut NativeServiceEndpointPublishRequest) as usize,
        )
    };
    if rc < 0 {
        return Err(map_error(rc));
    }
    service_heartbeat(service_id)
}

pub fn service_heartbeat(service_id: ServiceId) -> Result<ServiceEndpointState, Error> {
    let mut raw = NativeServiceEndpointState {
        abi_version: 0,
        service_id: service_id as u32,
        request_region_id: 0,
        request_generation: 0,
        response_region_id: 0,
        response_generation: 0,
        heartbeat_epoch: 0,
    };
    let rc = unsafe {
        syscall2(
            SYS_ECHOS_SERVICE_HEARTBEAT,
            service_id as usize,
            (&mut raw as *mut NativeServiceEndpointState) as usize,
        )
    };
    if rc < 0 {
        return Err(map_error(rc));
    }
    Ok(ServiceEndpointState {
        service_id: decode_service_id(raw.service_id)?,
        request_region_id: raw.request_region_id,
        request_generation: raw.request_generation,
        response_region_id: raw.response_region_id,
        response_generation: raw.response_generation,
        heartbeat_epoch: raw.heartbeat_epoch,
    })
}

pub fn run_service<A: ServiceApplication>(mut app: A) -> ! {
    let bootstrap = claim_service_bootstrap().unwrap_or_else(|_| panic_loop());
    let request_region =
        map_service_region(bootstrap.request_region_handle, true).unwrap_or_else(|_| panic_loop());
    let response_region =
        map_service_region(bootstrap.response_region_handle, true).unwrap_or_else(|_| panic_loop());
    publish_service_endpoint(
        bootstrap.service_id,
        bootstrap.request_region_handle,
        bootstrap.response_region_handle,
    )
    .unwrap_or_else(|_| panic_loop());
    let mut ctx = ServiceContext {
        bootstrap,
        request_region,
        response_region,
    };
    let _ = app.bootstrap(&mut ctx);
    loop {
        let _ = app.tick(&mut ctx);
        let _ = ctx.heartbeat();
        core::hint::spin_loop();
    }
}

impl NotificationClient {
    pub fn post(&self, level: NotificationLevel, title: &str, message: &str) -> Result<(), Error> {
        let (title_len, title_buf) = inline_text(title)?;
        let (message_len, message_buf) = inline_text(message)?;
        let request = NativeNotificationRequest {
            level: notification_level(level),
            title_len,
            message_len,
            title: title_buf,
            message: message_buf,
        };
        let rc = unsafe {
            syscall1(
                SYS_ECHOS_NOTIFICATION_POST,
                (&request as *const NativeNotificationRequest) as usize,
            )
        };
        if rc < 0 {
            Err(map_error(rc))
        } else {
            Ok(())
        }
    }
}

impl ClipboardClient {
    pub fn set_text(&self, value: &str) -> Result<(), Error> {
        let (text_len, text) = inline_text(value)?;
        let request = NativeClipboardSetTextRequest { text_len, text };
        let rc = unsafe {
            syscall1(
                SYS_ECHOS_CLIPBOARD_SET_TEXT,
                (&request as *const NativeClipboardSetTextRequest) as usize,
            )
        };
        if rc < 0 {
            Err(map_error(rc))
        } else {
            Ok(())
        }
    }

    pub fn get_text(&self) -> Result<String, Error> {
        let mut response = NativeClipboardGetTextResponse {
            text_len: 0,
            text: [0u8; MAX_INLINE_TEXT],
        };
        let rc = unsafe {
            syscall1(
                SYS_ECHOS_CLIPBOARD_GET_TEXT,
                (&mut response as *mut NativeClipboardGetTextResponse) as usize,
            )
        };
        if rc < 0 {
            return Err(map_error(rc));
        }
        core::str::from_utf8(&response.text[..response.text_len as usize])
            .map(|text| text.to_string())
            .map_err(|_| Error::Utf8)
    }
}

pub fn create_window(options: &WindowOptions) -> Result<Window, Error> {
    let (title_len, title) = inline_text(&options.title)?;
    let request = NativeWindowCreateRequest {
        x: options.x,
        y: options.y,
        width: options.width,
        height: options.height,
        title_len,
        title,
    };
    let mut handle = NativeWindowHandle {
        window_id: 0,
        surface_id: 0,
        content_width: 0,
        content_height: 0,
    };
    let rc = unsafe {
        syscall2(
            SYS_ECHOS_WIN_CREATE,
            (&request as *const NativeWindowCreateRequest) as usize,
            (&mut handle as *mut NativeWindowHandle) as usize,
        )
    };
    if rc < 0 {
        Err(map_error(rc))
    } else {
        Ok(Window { handle })
    }
}

pub fn poll_event() -> Result<Event, Error> {
    let mut events = [NativeInputEvent {
        kind: NativeEventKind::None as u32,
        window_id: 0,
        x: 0,
        y: 0,
        delta_x: 0,
        delta_y: 0,
        key_code: 0,
        modifiers: 0,
        state: 0,
        button: 0,
        reserved: 0,
    }; MAX_POLLED_EVENTS];
    let rc = unsafe { syscall2(SYS_ECHOS_EVENT_POLL, events.as_mut_ptr() as usize, 1) };
    if rc < 0 {
        return Err(map_error(rc));
    }
    if rc == 0 {
        return Ok(Event::None);
    }
    Ok(map_event(events[0]))
}

fn scene_op_to_raw(op: &SceneOp) -> Result<NativeSceneOp, Error> {
    match op {
        SceneOp::SolidRect {
            x,
            y,
            width,
            height,
            color,
            radius,
            z_index,
            opacity,
        } => Ok(NativeSceneOp {
            kind: echos_sdk_sys::NativeSceneOpKind::SolidRect as u32,
            x: *x,
            y: *y,
            width: *width,
            height: *height,
            color: *color,
            z_index: *z_index,
            opacity: *opacity,
            corner_radius: *radius,
            style_flags: 0,
            text_len: 0,
            text: [0u8; MAX_INLINE_TEXT],
        }),
        SceneOp::Text {
            x,
            y,
            width,
            height,
            color,
            z_index,
            opacity,
            monospace,
            text,
        } => {
            let (text_len, inline) = inline_text(text)?;
            Ok(NativeSceneOp {
                kind: echos_sdk_sys::NativeSceneOpKind::Text as u32,
                x: *x,
                y: *y,
                width: *width,
                height: *height,
                color: *color,
                z_index: *z_index,
                opacity: *opacity,
                corner_radius: 0,
                style_flags: if *monospace { 1 } else { 0 },
                text_len,
                text: inline,
            })
        }
    }
}

fn inline_text(value: &str) -> Result<(u16, [u8; MAX_INLINE_TEXT]), Error> {
    if value.len() > MAX_INLINE_TEXT {
        return Err(Error::InvalidInput);
    }
    let mut out = [0u8; MAX_INLINE_TEXT];
    out[..value.len()].copy_from_slice(value.as_bytes());
    Ok((value.len() as u16, out))
}

fn notification_level(level: NotificationLevel) -> u32 {
    match level {
        NotificationLevel::Info => 0,
        NotificationLevel::Success => 1,
        NotificationLevel::Warning => 2,
        NotificationLevel::Error => 3,
    }
}

fn decode_service_id(raw: u32) -> Result<ServiceId, Error> {
    match raw {
        0 => Ok(ServiceId::Directory),
        1 => Ok(ServiceId::EchDisplay),
        2 => Ok(ServiceId::EchInput),
        3 => Ok(ServiceId::EchAudio),
        4 => Ok(ServiceId::EchStore),
        5 => Ok(ServiceId::EchShell),
        6 => Ok(ServiceId::EchNotifications),
        7 => Ok(ServiceId::EchClipboard),
        8 => Ok(ServiceId::EchDialogs),
        9 => Ok(ServiceId::EchCapture),
        _ => Err(Error::InvalidInput),
    }
}

fn decode_service_isolation(raw: u32) -> ServiceIsolation {
    match raw {
        1 => ServiceIsolation::KernelTask,
        2 => ServiceIsolation::UserProcess,
        _ => ServiceIsolation::Unknown,
    }
}

fn map_event(event: NativeInputEvent) -> Event {
    match event.kind {
        value if value == NativeEventKind::Key as u32 => Event::Key {
            window_id: event.window_id,
            key_code: event.key_code,
            modifiers: event.modifiers,
            pressed: event.state != 0,
        },
        value if value == NativeEventKind::PointerMove as u32 => Event::PointerMove {
            window_id: event.window_id,
            x: event.x,
            y: event.y,
            delta_x: event.delta_x,
            delta_y: event.delta_y,
        },
        value if value == NativeEventKind::PointerButton as u32 => Event::PointerButton {
            window_id: event.window_id,
            x: event.x,
            y: event.y,
            button: event.button,
            pressed: event.state != 0,
        },
        value if value == NativeEventKind::CloseRequested as u32 => Event::CloseRequested {
            window_id: event.window_id,
        },
        _ => Event::None,
    }
}

fn map_error(code: isize) -> Error {
    match code {
        EACCES => Error::AccessDenied,
        EINVAL => Error::InvalidInput,
        ENOSYS => Error::Unsupported,
        EFBIG => Error::StateTooLarge,
        other => Error::SyscallFailed(other),
    }
}

fn panic_loop() -> ! {
    loop {
        core::hint::spin_loop();
    }
}
