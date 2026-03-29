#![no_std]

use core::arch::asm;

pub const ENOSYS: isize = -38;
pub const EINVAL: isize = -22;
pub const EACCES: isize = -13;
pub const EFBIG: isize = -27;

pub const SYS_ECHOS_WIN_CREATE: usize = 451;
pub const SYS_ECHOS_WIN_DESTROY: usize = 452;
pub const SYS_ECHOS_SCENE_COMMIT: usize = 456;
pub const SYS_ECHOS_NOTIFICATION_POST: usize = 457;
pub const SYS_ECHOS_CLIPBOARD_SET_TEXT: usize = 458;
pub const SYS_ECHOS_CLIPBOARD_GET_TEXT: usize = 459;
pub const SYS_ECHOS_EVENT_POLL: usize = 460;
pub const SYS_ECHOS_SERVICE_BOOTSTRAP_CLAIM: usize = 461;
pub const SYS_ECHOS_SERVICE_STATUS: usize = 462;
pub const SYS_ECHOS_SERVICE_PARITY_STATUS: usize = 463;
pub const SYS_ECHOS_SERVICE_REGION_MAP: usize = 464;
pub const SYS_ECHOS_SERVICE_ENDPOINT_PUBLISH: usize = 465;
pub const SYS_ECHOS_SERVICE_HEARTBEAT: usize = 466;
pub const SYS_ECHOS_NOTIFICATION_SERVICE_RECV: usize = 467;
pub const SYS_ECHOS_NOTIFICATION_SERVICE_RESPOND: usize = 468;

pub const MAX_INLINE_TEXT: usize = 96;
pub const MAX_SCENE_OPS: usize = 64;
pub const MAX_POLLED_EVENTS: usize = 32;
pub const MAX_SERVICE_NOTIFICATION_ITEMS: usize = 16;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeWindowCreateRequest {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub title_len: u16,
    pub title: [u8; MAX_INLINE_TEXT],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeWindowHandle {
    pub window_id: u64,
    pub surface_id: u64,
    pub content_width: u32,
    pub content_height: u32,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeSceneOpKind {
    SolidRect = 1,
    Text = 2,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeSceneOp {
    pub kind: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub color: u32,
    pub z_index: u32,
    pub opacity: u8,
    pub corner_radius: u16,
    pub style_flags: u8,
    pub text_len: u16,
    pub text: [u8; MAX_INLINE_TEXT],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeSceneSubmitRequest {
    pub window_id: u64,
    pub revision: u64,
    pub op_count: u32,
    pub ops_ptr: u64,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeEventKind {
    None = 0,
    Key = 1,
    PointerMove = 2,
    PointerButton = 3,
    CloseRequested = 4,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeInputEvent {
    pub kind: u32,
    pub window_id: u64,
    pub x: i32,
    pub y: i32,
    pub delta_x: i32,
    pub delta_y: i32,
    pub key_code: u32,
    pub modifiers: u8,
    pub state: u8,
    pub button: u8,
    pub reserved: u8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeNotificationRequest {
    pub level: u32,
    pub title_len: u16,
    pub message_len: u16,
    pub title: [u8; MAX_INLINE_TEXT],
    pub message: [u8; MAX_INLINE_TEXT],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeClipboardSetTextRequest {
    pub text_len: u16,
    pub text: [u8; MAX_INLINE_TEXT],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeClipboardGetTextResponse {
    pub text_len: u16,
    pub text: [u8; MAX_INLINE_TEXT],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeServiceBootstrap {
    pub abi_version: u32,
    pub service_id: u32,
    pub runtime_app_id: u32,
    pub service_handle: u32,
    pub request_region_handle: u32,
    pub response_region_handle: u32,
    pub endpoint_generation: u32,
    pub rights_bits: u32,
    pub isolation_domain: u32,
    pub runtime_task_id: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeServiceStatus {
    pub abi_version: u32,
    pub service_id: u32,
    pub openable_rights_bits: u32,
    pub endpoint_generation: u32,
    pub control_plane: u8,
    pub bulk_data_out_of_band: u8,
    pub service_process_available: u8,
    pub user_published_endpoint: u8,
    pub runtime_isolation: u8,
    pub runtime_task_id: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeServiceParityStatus {
    pub abi_version: u32,
    pub required_services: u32,
    pub packaged_service_slots: u32,
    pub live_user_process_slots: u32,
    pub published_user_process_slots: u32,
    pub strict_mode_enabled: u8,
    pub full_parity_ready: u8,
    pub reserved: [u8; 6],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeServiceRegionMapping {
    pub abi_version: u32,
    pub region_handle: u32,
    pub writable: u32,
    pub region_id: u64,
    pub generation: u64,
    pub base: u64,
    pub len: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeServiceEndpointPublishRequest {
    pub abi_version: u32,
    pub service_id: u32,
    pub request_region_handle: u32,
    pub response_region_handle: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeServiceEndpointState {
    pub abi_version: u32,
    pub service_id: u32,
    pub request_region_id: u64,
    pub request_generation: u64,
    pub response_region_id: u64,
    pub response_generation: u64,
    pub heartbeat_epoch: u64,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeServiceNotificationCommandKind {
    None = 0,
    Push = 1,
    List = 2,
    MarkRead = 3,
    Clear = 4,
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeServiceNotificationResponseKind {
    None = 0,
    Ack = 1,
    NotificationId = 2,
    Notifications = 3,
    Error = 4,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeServiceNotificationEntry {
    pub id: u64,
    pub app_id: u32,
    pub level: u32,
    pub read: u8,
    pub reserved: [u8; 3],
    pub timestamp_ticks: u64,
    pub source_name_len: u16,
    pub title_len: u16,
    pub message_len: u16,
    pub action_label_len: u16,
    pub source_name: [u8; MAX_INLINE_TEXT],
    pub title: [u8; MAX_INLINE_TEXT],
    pub message: [u8; MAX_INLINE_TEXT],
    pub action_label: [u8; MAX_INLINE_TEXT],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeServiceNotificationRequest {
    pub abi_version: u32,
    pub request_id: u64,
    pub kind: u32,
    pub app_id: u32,
    pub include_read: u32,
    pub max_items: u32,
    pub notification_id: u64,
    pub level: u32,
    pub title_len: u16,
    pub message_len: u16,
    pub action_label_len: u16,
    pub reserved: u16,
    pub title: [u8; MAX_INLINE_TEXT],
    pub message: [u8; MAX_INLINE_TEXT],
    pub action_label: [u8; MAX_INLINE_TEXT],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeServiceNotificationResponse {
    pub abi_version: u32,
    pub request_id: u64,
    pub kind: u32,
    pub notification_id: u64,
    pub entry_count: u32,
    pub error_len: u16,
    pub reserved: u16,
    pub error: [u8; MAX_INLINE_TEXT],
    pub entries: [NativeServiceNotificationEntry; MAX_SERVICE_NOTIFICATION_ITEMS],
}

#[inline]
pub unsafe fn syscall0(number: usize) -> isize {
    syscall6(number, 0, 0, 0, 0, 0, 0)
}

#[inline]
pub unsafe fn syscall1(number: usize, a1: usize) -> isize {
    syscall6(number, a1, 0, 0, 0, 0, 0)
}

#[inline]
pub unsafe fn syscall2(number: usize, a1: usize, a2: usize) -> isize {
    syscall6(number, a1, a2, 0, 0, 0, 0)
}

#[inline]
pub unsafe fn syscall3(number: usize, a1: usize, a2: usize, a3: usize) -> isize {
    syscall6(number, a1, a2, a3, 0, 0, 0)
}

#[inline]
pub unsafe fn syscall6(
    number: usize,
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
) -> isize {
    #[cfg(target_arch = "x86_64")]
    {
        let ret: isize;
        asm!(
            "syscall",
            inlateout("rax") number as isize => ret,
            in("rdi") a1 as isize,
            in("rsi") a2 as isize,
            in("rdx") a3 as isize,
            in("r10") a4 as isize,
            in("r8") a5 as isize,
            in("r9") a6 as isize,
            lateout("rcx") _,
            lateout("r11") _,
        );
        ret
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = (number, a1, a2, a3, a4, a5, a6);
        ENOSYS
    }
}
