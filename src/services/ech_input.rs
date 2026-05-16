//! # EchInput - Week-2 Input Service
//!
//! Sorumluluk:
//! - Donanimdan gelen input eventlerini toplamak
//! - Focus manager ile aktif uygulamayi belirlemek
//! - Eventleri pencere hit-test sonucuna gore app kuyruklarina yonlendirmek

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use pc_keyboard::DecodedKey;
use spin::Mutex;

use crate::drivers::input::{InputEvent as RawInputEvent, MousePacket};
use crate::gui::focus::FocusManager;
use crate::gui::input_pipeline::InputPipeline;
use crate::gui::protocol::{
    AccessibilityProfile, AppId, InputEvent, KeyState, Point, PointerButton, ShellShortcut,
    WindowInputEvent, MOD_ALT, MOD_CTRL, MOD_SUPER,
};
use crate::gui::shared_ring::SharedRing;
use crate::ipc::request_display_sync;
use crate::services::display_atomic::MailboxRing;
use crate::services::ech_display::{DisplayCommand, DisplayResponse, InputRouting};
use crate::services::ech_shell::{get_shell_service, ShellCommand, ShellResponse};

const MAX_EVENTS_PER_APP: usize = 1024;
const MOUSE_BUTTON_DEBOUNCE_NS: u64 = 8_000_000;
const ACCESSIBILITY_PROFILE_REFRESH_NS: u64 = 50_000_000;
const SLOW_KEYS_DELAY_NS: u64 = 250_000_000;
const INPUT_COMMAND_QUEUE_CAPACITY: usize = 256;
const INPUT_RESPONSE_QUEUE_CAPACITY: usize = 256;

/// Input servisi komutlari.
#[derive(Clone, Debug)]
pub enum InputCommand {
    RegisterApp { app_id: AppId },
    RegisterShortcutSink { app_id: AppId },
    UnregisterApp { app_id: AppId },
    RequestFocus { app_id: AppId },
    ReleaseFocus { app_id: AppId },
    InjectEvent { event: InputEvent },
    PollEvents { app_id: AppId, max_events: usize },
    PollShortcuts { app_id: AppId, max_events: usize },
}

/// Input servisi yanitlari.
#[derive(Clone, Debug)]
pub enum InputResponse {
    Ack,
    FocusChanged {
        focused_app: Option<AppId>,
    },
    Events {
        app_id: AppId,
        events: Vec<WindowInputEvent>,
    },
    Shortcuts(Vec<ShellShortcut>),
    Error(String),
}

#[derive(Clone, Copy, Default)]
struct ButtonState {
    left: bool,
    right: bool,
    middle: bool,
}

#[derive(Clone, Copy, Default)]
struct ButtonDebounce {
    left_ns: u64,
    right_ns: u64,
    middle_ns: u64,
}

#[derive(Clone)]
struct PendingSlowKey {
    event: InputEvent,
    ready_ns: u64,
}

pub struct EchInput {
    running: AtomicBool,
    focus: Mutex<FocusManager>,
    app_queues: Mutex<BTreeMap<AppId, SharedRing<WindowInputEvent>>>,
    shortcut_sink: Mutex<Option<AppId>>,
    shortcut_queue: Mutex<SharedRing<ShellShortcut>>,
    command_queue: MailboxRing<InputCommand>,
    response_queue: MailboxRing<InputResponse>,
    last_cursor: Mutex<Point>,
    last_buttons: Mutex<ButtonState>,
    last_button_change_ns: Mutex<ButtonDebounce>,
    pipeline: Mutex<InputPipeline>,
    last_scroll_poll_ns: Mutex<u64>,
    last_accessibility_refresh_ns: Mutex<u64>,
    accessibility_profile: Mutex<AccessibilityProfile>,
    latched_modifiers: Mutex<u8>,
    pending_slow_keys: Mutex<BTreeMap<u16, PendingSlowKey>>,
    alt_switch_armed: AtomicBool,
}

impl EchInput {
    pub fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            focus: Mutex::new(FocusManager::new()),
            app_queues: Mutex::new(BTreeMap::new()),
            shortcut_sink: Mutex::new(None),
            shortcut_queue: Mutex::new(SharedRing::with_capacity_pow2(MAX_EVENTS_PER_APP)),
            command_queue: MailboxRing::with_capacity_pow2(INPUT_COMMAND_QUEUE_CAPACITY),
            response_queue: MailboxRing::with_capacity_pow2(INPUT_RESPONSE_QUEUE_CAPACITY),
            last_cursor: Mutex::new(Point::new(640, 400)),
            last_buttons: Mutex::new(ButtonState::default()),
            last_button_change_ns: Mutex::new(ButtonDebounce::default()),
            pipeline: Mutex::new(InputPipeline::new()),
            last_scroll_poll_ns: Mutex::new(crate::cpu::tsc::read_ns()),
            last_accessibility_refresh_ns: Mutex::new(0),
            accessibility_profile: Mutex::new(AccessibilityProfile::default()),
            latched_modifiers: Mutex::new(0),
            pending_slow_keys: Mutex::new(BTreeMap::new()),
            alt_switch_armed: AtomicBool::new(false),
        }
    }

    pub fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
        crate::serial_println!("[ECHINPUT] Week-2 input service started");
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        crate::serial_println!("[ECHINPUT] Week-2 input service stopped");
    }

    pub fn send_command(&self, command: InputCommand) -> bool {
        self.command_queue.try_push(command).is_ok()
    }

    pub fn receive_response(&self) -> Option<InputResponse> {
        self.response_queue.pop()
    }

    pub fn process_command(&self, command: InputCommand) -> InputResponse {
        self.refresh_accessibility_profile(false);
        // Clients poll this service synchronously; drain the raw driver queue here too so
        // pointer/keyboard interaction does not depend solely on the background service task.
        self.drain_raw_input();

        match command {
            InputCommand::RegisterApp { app_id } => {
                self.focus.lock().register_app(app_id);
                self.app_queues
                    .lock()
                    .entry(app_id)
                    .or_insert_with(|| SharedRing::with_capacity_pow2(MAX_EVENTS_PER_APP));
                InputResponse::FocusChanged {
                    focused_app: self.focus.lock().focused_app(),
                }
            }
            InputCommand::RegisterShortcutSink { app_id } => {
                self.focus.lock().register_app(app_id);
                self.app_queues
                    .lock()
                    .entry(app_id)
                    .or_insert_with(|| SharedRing::with_capacity_pow2(MAX_EVENTS_PER_APP));
                *self.shortcut_sink.lock() = Some(app_id);
                InputResponse::Ack
            }
            InputCommand::UnregisterApp { app_id } => {
                self.focus.lock().unregister_app(app_id);
                self.app_queues.lock().remove(&app_id);
                if self.shortcut_sink.lock().as_ref() == Some(&app_id) {
                    *self.shortcut_sink.lock() = None;
                    self.shortcut_queue.lock().clear();
                }
                InputResponse::FocusChanged {
                    focused_app: self.focus.lock().focused_app(),
                }
            }
            InputCommand::RequestFocus { app_id } => {
                if self.focus.lock().request_focus(app_id) {
                    InputResponse::FocusChanged {
                        focused_app: Some(app_id),
                    }
                } else {
                    InputResponse::Error(String::from("app not registered"))
                }
            }
            InputCommand::ReleaseFocus { app_id } => {
                self.focus.lock().release_focus(app_id);
                InputResponse::FocusChanged {
                    focused_app: self.focus.lock().focused_app(),
                }
            }
            InputCommand::InjectEvent { event } => {
                self.route_event(event);
                InputResponse::Ack
            }
            InputCommand::PollEvents { app_id, max_events } => {
                let max_events = max_events.max(1);
                let mut queues = self.app_queues.lock();
                let queue = queues
                    .entry(app_id)
                    .or_insert_with(|| SharedRing::with_capacity_pow2(MAX_EVENTS_PER_APP));
                let events = queue.drain(max_events);
                InputResponse::Events { app_id, events }
            }
            InputCommand::PollShortcuts { app_id, max_events } => {
                if self.shortcut_sink.lock().as_ref() != Some(&app_id) {
                    return InputResponse::Shortcuts(Vec::new());
                }
                let max_events = max_events.max(1);
                let mut queue = self.shortcut_queue.lock();
                let shortcuts = queue.drain(max_events);
                InputResponse::Shortcuts(shortcuts)
            }
        }
    }

    pub fn run_service(&self) {
        while self.running.load(Ordering::SeqCst) {
            self.refresh_accessibility_profile(false);
            self.drain_raw_input();
            self.flush_pending_slow_keys();

            while let Some(command) = self.command_queue.pop() {
                let response = self.process_command(command);
                let _ = self.response_queue.push_overwrite(response);
            }

            for _ in 0..1000 {
                core::hint::spin_loop();
            }
        }
    }

    fn drain_raw_input(&self) {
        let now_ns = crate::cpu::tsc::read_ns();
        let dt_sec = {
            let mut last = self.last_scroll_poll_ns.lock();
            let elapsed = now_ns.saturating_sub(*last);
            *last = now_ns;
            (elapsed as f32 / 1_000_000_000.0).clamp(1.0 / 1000.0, 1.0 / 30.0)
        };

        // QEMU/UEFI GUI yolunda IRQ12 teslimi kaçsa bile AUX-buffer'daki fare verisini
        // klavye scancode'larına dokunmadan toparla.
        let aux_bytes = crate::drivers::mouse::poll_aux_burst(64);
        if aux_bytes == 0 {
            let _ = crate::drivers::mouse::poll_vmware_compat_burst(64);
        }

        while let Some(raw) = crate::drivers::input::pop_event() {
            let events = self.translate_raw_event(raw);
            for event in events {
                self.route_event(event);
            }
        }

        if let Some(delta) = self.pipeline.lock().poll_kinetic_scroll(dt_sec) {
            let position = *self.last_cursor.lock();
            self.route_event(InputEvent::Scroll { delta, position });
        }
    }

    fn translate_raw_event(&self, raw: RawInputEvent) -> Vec<InputEvent> {
        match raw {
            RawInputEvent::Keyboard {
                decoded,
                scan_code,
                modifiers,
                state,
            } => {
                let event = match decoded {
                    Some(DecodedKey::Unicode(ch)) => InputEvent::Key {
                        unicode: Some(ch),
                        scan_code,
                        modifiers,
                        state: translate_key_state(state),
                    },
                    _ => InputEvent::Key {
                        unicode: None,
                        scan_code,
                        modifiers,
                        state: translate_key_state(state),
                    },
                };
                alloc::vec![event]
            }
            RawInputEvent::Mouse(packet) => self.translate_mouse_packet(packet),
            RawInputEvent::MouseByte(byte) => {
                // crate::serial_println!("[ECHINPUT] Mouse byte: {:02X}", byte);
                crate::drivers::mouse::handle_packet(byte);
                Vec::new()
            }
            RawInputEvent::Gesture(_) => Vec::new(),
        }
    }

    fn translate_mouse_packet(&self, packet: MousePacket) -> Vec<InputEvent> {
        let (x, y) = crate::drivers::mouse::get_position();
        let position = Point::new(x, y);

        let mut out = Vec::new();
        let mut last_cursor = self.last_cursor.lock();
        let previous = *last_cursor;
        let raw_delta = Point::new(position.x - previous.x, position.y - previous.y);
        *last_cursor = position;
        drop(last_cursor);

        let filtered_delta = self.pipeline.lock().filter_pointer_delta(raw_delta);
        let motion_delta = if filtered_delta.x != 0 || filtered_delta.y != 0 {
            filtered_delta
        } else {
            raw_delta
        };
        if motion_delta.x != 0 || motion_delta.y != 0 {
            // Small raw motion can be quantized to zero by the filter; keep absolute cursor
            // tracking responsive by emitting raw delta when position actually changed.
            out.push(InputEvent::PointerMove {
                position,
                delta: motion_delta,
            });
        }

        if let MousePacket::Intelli { z, .. } = packet {
            let scroll_delta = self.pipeline.lock().feed_scroll_notch(z);
            if scroll_delta != 0 {
                out.push(InputEvent::Scroll {
                    delta: Point::new(0, scroll_delta),
                    position,
                });
            }
        }

        self.push_changed_button_events(position, &mut out);
        out
    }

    fn push_changed_button_events(&self, position: Point, out: &mut Vec<InputEvent>) {
        let buttons = crate::drivers::mouse::get_buttons();
        let now_ns = crate::cpu::tsc::read_ns();
        let mut last = self.last_buttons.lock();
        let mut debounce = self.last_button_change_ns.lock();

        if buttons.left != last.left {
            if now_ns.saturating_sub(debounce.left_ns) >= MOUSE_BUTTON_DEBOUNCE_NS {
                out.push(InputEvent::PointerButton {
                    button: PointerButton::Left,
                    state: if buttons.left {
                        KeyState::Pressed
                    } else {
                        KeyState::Released
                    },
                    position,
                });
                debounce.left_ns = now_ns;
            }
            last.left = buttons.left;
        }

        if buttons.right != last.right {
            if now_ns.saturating_sub(debounce.right_ns) >= MOUSE_BUTTON_DEBOUNCE_NS {
                out.push(InputEvent::PointerButton {
                    button: PointerButton::Right,
                    state: if buttons.right {
                        KeyState::Pressed
                    } else {
                        KeyState::Released
                    },
                    position,
                });
                debounce.right_ns = now_ns;
            }
            last.right = buttons.right;
        }

        if buttons.middle != last.middle {
            if now_ns.saturating_sub(debounce.middle_ns) >= MOUSE_BUTTON_DEBOUNCE_NS {
                out.push(InputEvent::PointerButton {
                    button: PointerButton::Middle,
                    state: if buttons.middle {
                        KeyState::Pressed
                    } else {
                        KeyState::Released
                    },
                    position,
                });
                debounce.middle_ns = now_ns;
            }
            last.middle = buttons.middle;
        }
    }

    fn route_event(&self, event: InputEvent) {
        let Some(event) = self.apply_accessibility_filters(event) else {
            return;
        };

        if self.intercept_shortcut(&event) {
            return;
        }

        self.dispatch_event(event);
    }

    fn dispatch_event(&self, event: InputEvent) {
        if matches!(event, InputEvent::Key { .. }) {
            self.flush_pending_slow_keys();
        }

        let routing = match request_display_sync(
            0,
            DisplayCommand::RouteInputEvent {
                event: event.clone(),
            },
        ) {
            Some(DisplayResponse::InputRoute(routing)) => routing,
            _ => self
                .focus
                .lock()
                .focused_app()
                .map(InputRouting::FocusOnly)
                .unwrap_or(InputRouting::None),
        };

        match routing {
            InputRouting::None => {}
            InputRouting::FocusOnly(app_id) => {
                self.sync_focus(app_id);
            }
            InputRouting::DeliverTo {
                app_id,
                window_id,
                global_position,
                local_position,
                captured,
            } => {
                self.sync_focus(app_id);
                self.enqueue_event(
                    app_id,
                    WindowInputEvent {
                        app_id,
                        window_id,
                        global_position,
                        local_position,
                        captured,
                        event,
                    },
                );
            }
        }
    }

    fn refresh_accessibility_profile(&self, force: bool) {
        let now_ns = crate::cpu::tsc::read_ns();
        {
            let mut last = self.last_accessibility_refresh_ns.lock();
            if !force && now_ns.saturating_sub(*last) < ACCESSIBILITY_PROFILE_REFRESH_NS {
                return;
            }
            *last = now_ns;
        }

        if let ShellResponse::AccessibilityProfile(profile) =
            get_shell_service().process_command(ShellCommand::GetAccessibilityProfile)
        {
            *self.accessibility_profile.lock() = profile;
            if !profile.sticky_keys {
                *self.latched_modifiers.lock() = 0;
            }
            if !profile.slow_keys {
                self.pending_slow_keys.lock().clear();
            }
        }
    }

    fn apply_accessibility_filters(&self, event: InputEvent) -> Option<InputEvent> {
        let profile = *self.accessibility_profile.lock();
        let InputEvent::Key {
            unicode,
            scan_code,
            modifiers,
            state,
        } = event
        else {
            return Some(event);
        };

        let modifier_mask = sticky_modifier_mask(scan_code);
        if profile.sticky_keys {
            if modifier_mask != 0 {
                if matches!(state, KeyState::Pressed) {
                    let mut latched = self.latched_modifiers.lock();
                    *latched |= modifier_mask;
                }
                return None;
            }
        } else {
            *self.latched_modifiers.lock() = 0;
        }

        let effective_modifiers = if profile.sticky_keys && matches!(state, KeyState::Pressed) {
            let mut latched = self.latched_modifiers.lock();
            let combined = modifiers | *latched;
            *latched = 0;
            combined
        } else {
            modifiers
        };

        let filtered = InputEvent::Key {
            unicode,
            scan_code,
            modifiers: effective_modifiers,
            state,
        };

        if profile.slow_keys && modifier_mask == 0 {
            match state {
                KeyState::Pressed => {
                    self.pending_slow_keys.lock().insert(
                        scan_code,
                        PendingSlowKey {
                            event: filtered,
                            ready_ns: crate::cpu::tsc::read_ns().saturating_add(SLOW_KEYS_DELAY_NS),
                        },
                    );
                    return None;
                }
                KeyState::Released => {
                    if self.pending_slow_keys.lock().remove(&scan_code).is_some() {
                        return None;
                    }
                }
            }
        }

        Some(filtered)
    }

    fn flush_pending_slow_keys(&self) {
        let now_ns = crate::cpu::tsc::read_ns();
        let mut ready = Vec::new();
        {
            let pending = self.pending_slow_keys.lock();
            for (&scan_code, entry) in pending.iter() {
                if now_ns >= entry.ready_ns {
                    ready.push(scan_code);
                }
            }
        }
        if ready.is_empty() {
            return;
        }

        let mut pending = self.pending_slow_keys.lock();
        let mut events = Vec::with_capacity(ready.len());
        for scan_code in ready {
            if let Some(entry) = pending.remove(&scan_code) {
                events.push(entry.event);
            }
        }
        drop(pending);

        for event in events {
            if self.intercept_shortcut(&event) {
                continue;
            }
            self.dispatch_event(event);
        }
    }

    fn sync_focus(&self, app_id: AppId) {
        let _ = self.focus.lock().request_focus(app_id);
    }

    fn enqueue_event(&self, app_id: AppId, event: WindowInputEvent) {
        let mut queues = self.app_queues.lock();
        let queue = queues
            .entry(app_id)
            .or_insert_with(|| SharedRing::with_capacity_pow2(MAX_EVENTS_PER_APP));
        if queue.push(event.clone()).is_err() {
            let _ = queue.pop();
            let _ = queue.push(event);
        }
    }

    fn intercept_shortcut(&self, event: &InputEvent) -> bool {
        let InputEvent::Key {
            unicode,
            scan_code,
            modifiers,
            state,
        } = *event
        else {
            return false;
        };

        if !matches!(state, KeyState::Pressed | KeyState::Released) {
            return false;
        }

        let alt_down = modifiers & MOD_ALT != 0;
        let super_down = modifiers & MOD_SUPER != 0;
        let is_tab = unicode == Some('\t') || scan_code == 0x0F;
        let is_escape = unicode == Some('\u{1b}') || scan_code == 0x01;
        let is_space = unicode == Some(' ') || scan_code == 0x39;
        let is_comma = unicode == Some(',') || scan_code == 0x33;
        let is_grave = unicode == Some('`') || scan_code == 0x29;
        let is_s = unicode == Some('s') || unicode == Some('S') || scan_code == 0x1F;
        let is_enter = unicode == Some('\n') || scan_code == 0x1C;
        let workspace = match scan_code {
            0x02 => Some(0),
            0x03 => Some(1),
            0x04 => Some(2),
            0x05 => Some(3),
            0x06 => Some(4),
            0x07 => Some(5),
            0x08 => Some(6),
            0x09 => Some(7),
            _ => None,
        };

        if matches!(state, KeyState::Pressed) && alt_down && is_tab {
            self.alt_switch_armed.store(true, Ordering::SeqCst);
            self.enqueue_shortcut(ShellShortcut::AppSwitcherNext);
            return true;
        }

        if matches!(state, KeyState::Pressed) && super_down && is_space {
            self.enqueue_shortcut(ShellShortcut::ToggleCommandPalette);
            return true;
        }

        if matches!(state, KeyState::Pressed) && super_down && is_comma {
            self.enqueue_shortcut(ShellShortcut::ToggleQuickSettings);
            return true;
        }

        if matches!(state, KeyState::Pressed) && super_down && is_grave {
            self.enqueue_shortcut(ShellShortcut::ToggleOverview);
            return true;
        }

        if matches!(state, KeyState::Pressed) && super_down && is_s {
            self.enqueue_shortcut(ShellShortcut::ToggleScratchpad);
            return true;
        }

        if matches!(state, KeyState::Pressed) && super_down && is_enter {
            self.enqueue_shortcut(ShellShortcut::LaunchTerminal);
            return true;
        }

        if matches!(state, KeyState::Pressed) && super_down {
            if let Some(workspace_id) = workspace {
                self.enqueue_shortcut(ShellShortcut::Workspace(workspace_id));
                return true;
            }
        }

        if matches!(state, KeyState::Released)
            && !alt_down
            && self.alt_switch_armed.load(Ordering::SeqCst)
        {
            self.alt_switch_armed.store(false, Ordering::SeqCst);
            self.enqueue_shortcut(ShellShortcut::AppSwitcherConfirm);
            return true;
        }

        if matches!(state, KeyState::Pressed)
            && self.alt_switch_armed.load(Ordering::SeqCst)
            && is_escape
        {
            self.alt_switch_armed.store(false, Ordering::SeqCst);
            self.enqueue_shortcut(ShellShortcut::AppSwitcherCancel);
            return true;
        }

        false
    }

    fn enqueue_shortcut(&self, shortcut: ShellShortcut) {
        if self.shortcut_sink.lock().is_none() {
            return;
        }

        let mut queue = self.shortcut_queue.lock();
        if queue.push(shortcut).is_err() {
            let _ = queue.pop();
            let _ = queue.push(shortcut);
        }
    }
}

fn translate_key_state(state: pc_keyboard::KeyState) -> KeyState {
    match state {
        pc_keyboard::KeyState::Down | pc_keyboard::KeyState::SingleShot => KeyState::Pressed,
        pc_keyboard::KeyState::Up => KeyState::Released,
    }
}

fn sticky_modifier_mask(scan_code: u16) -> u8 {
    match scan_code {
        0x1D => MOD_CTRL,
        0x38 => MOD_ALT,
        0x5B | 0x5C => MOD_SUPER,
        _ => 0,
    }
}

lazy_static::lazy_static! {
    static ref ECH_INPUT: Arc<EchInput> = Arc::new(EchInput::new());
}

pub fn init() {
    ECH_INPUT.start();
    crate::serial_println!("[ECHINPUT] Week-2 initialized");
}

pub fn get_input() -> Arc<EchInput> {
    Arc::clone(&ECH_INPUT)
}

pub fn service_task() -> ! {
    let svc = get_input();
    svc.run_service();
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sticky_keys_latch_modifier_into_next_key_press() {
        let input = EchInput::new();
        *input.accessibility_profile.lock() = AccessibilityProfile {
            sticky_keys: true,
            ..AccessibilityProfile::default()
        };

        let modifier = input.apply_accessibility_filters(InputEvent::Key {
            unicode: None,
            scan_code: 0x1D,
            modifiers: 0,
            state: KeyState::Pressed,
        });
        assert!(modifier.is_none());

        let forwarded = input.apply_accessibility_filters(InputEvent::Key {
            unicode: Some('c'),
            scan_code: 0x2E,
            modifiers: 0,
            state: KeyState::Pressed,
        });
        assert!(matches!(
            forwarded,
            Some(InputEvent::Key { modifiers, .. }) if modifiers & MOD_CTRL != 0
        ));
    }

    #[test]
    fn slow_keys_cancel_press_when_key_released_early() {
        let input = EchInput::new();
        *input.accessibility_profile.lock() = AccessibilityProfile {
            slow_keys: true,
            ..AccessibilityProfile::default()
        };

        let press = input.apply_accessibility_filters(InputEvent::Key {
            unicode: Some('a'),
            scan_code: 0x1E,
            modifiers: 0,
            state: KeyState::Pressed,
        });
        assert!(press.is_none());
        assert!(input.pending_slow_keys.lock().contains_key(&0x1E));

        let release = input.apply_accessibility_filters(InputEvent::Key {
            unicode: Some('a'),
            scan_code: 0x1E,
            modifiers: 0,
            state: KeyState::Released,
        });
        assert!(release.is_none());
        assert!(!input.pending_slow_keys.lock().contains_key(&0x1E));
    }
}
