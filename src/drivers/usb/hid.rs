//! # echOS USB HID Driver
//!
//! Human Interface Device driver for keyboard, mouse, and gamepad support.
//! Implements USB HID specification 1.11 with boot protocol support.

use alloc::vec::Vec;
use alloc::boxed::Box;
use alloc::sync::Arc;
use spin::Mutex;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use super::{UsbDevice, UsbError, UsbSetupPacket, UsbEndpoint, UsbDirection, UsbTransferType};

// ============================================================================
// HID CLASS REQUESTS
// ============================================================================

const HID_GET_REPORT: u8 = 0x01;
const HID_GET_IDLE: u8 = 0x02;
const HID_GET_PROTOCOL: u8 = 0x03;
const HID_SET_REPORT: u8 = 0x09;
const HID_SET_IDLE: u8 = 0x0A;
const HID_SET_PROTOCOL: u8 = 0x0B;

// HID Protocol modes
const HID_PROTOCOL_BOOT: u8 = 0x00;
const HID_PROTOCOL_REPORT: u8 = 0x01;

// HID Report Types
const HID_REPORT_INPUT: u8 = 0x01;
const HID_REPORT_OUTPUT: u8 = 0x02;
const HID_REPORT_FEATURE: u8 = 0x03;

// ============================================================================
// HID BOOT PROTOCOL REPORTS
// ============================================================================

/// Standard keyboard boot protocol report (8 bytes)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KeyboardBootReport {
    /// Modifier keys (Ctrl, Shift, Alt, GUI)
    pub modifiers: u8,
    /// Reserved
    pub reserved: u8,
    /// Key codes (up to 6 simultaneous keys)
    pub keys: [u8; 6],
}

impl KeyboardBootReport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if a modifier is pressed
    pub fn modifier_pressed(&self, modifier: KeyboardModifier) -> bool {
        (self.modifiers & modifier as u8) != 0
    }

    /// Get all pressed keys
    pub fn pressed_keys(&self) -> Vec<u8> {
        self.keys.iter().filter(|&&k| k != 0).copied().collect()
    }

    /// Check if a specific key is pressed
    pub fn key_pressed(&self, key_code: u8) -> bool {
        self.keys.contains(&key_code)
    }
}

/// Keyboard modifier keys
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum KeyboardModifier {
    LeftCtrl = 0x01,
    LeftShift = 0x02,
    LeftAlt = 0x04,
    LeftGUI = 0x08,
    RightCtrl = 0x10,
    RightShift = 0x20,
    RightAlt = 0x40,
    RightGUI = 0x80,
}

/// Standard mouse boot protocol report (3-4 bytes)
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MouseBootReport {
    /// Button states
    pub buttons: u8,
    /// X displacement
    pub x: i8,
    /// Y displacement
    pub y: i8,
}

impl MouseBootReport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if left button is pressed
    pub fn left_button(&self) -> bool {
        (self.buttons & 0x01) != 0
    }

    /// Check if right button is pressed
    pub fn right_button(&self) -> bool {
        (self.buttons & 0x02) != 0
    }

    /// Check if middle button is pressed
    pub fn middle_button(&self) -> bool {
        (self.buttons & 0x04) != 0
    }
}

// ============================================================================
// HID USAGE TABLES
// ============================================================================

/// HID Usage Pages
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum UsagePage {
    Undefined = 0x00,
    GenericDesktop = 0x01,
    Simulation = 0x02,
    VR = 0x03,
    Sport = 0x04,
    Game = 0x05,
    Keyboard = 0x07,
    LED = 0x08,
    Button = 0x09,
    Ordinal = 0x0A,
    Telephony = 0x0B,
    Consumer = 0x0C,
    Digitizer = 0x0D,
    PID = 0x0F,
    Unicode = 0x10,
    AlphaNumeric = 0x14,
    Medical = 0x40,
    Monitor = 0x80,
    Power = 0x84,
    VendorDefined = 0xFF00,
}

/// HID Generic Desktop Usages
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum GenericDesktopUsage {
    Undefined = 0x00,
    Pointer = 0x01,
    Mouse = 0x02,
    Joystick = 0x04,
    Gamepad = 0x05,
    Keyboard = 0x06,
    Keypad = 0x07,
    X = 0x30,
    Y = 0x31,
    Z = 0x32,
    Rx = 0x33,
    Ry = 0x34,
    Rz = 0x35,
    Slider = 0x36,
    Dial = 0x37,
    Wheel = 0x38,
    HatSwitch = 0x39,
    MotionWakeup = 0x46,
    Start = 0x47,
    Select = 0x48,
}

/// HID Keyboard Usages (subset)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum KeyboardUsage {
    NoEvent = 0x00,
    ErrorRollOver = 0x01,
    A = 0x04,
    B = 0x05,
    C = 0x06,
    D = 0x07,
    E = 0x08,
    F = 0x09,
    G = 0x0A,
    H = 0x0B,
    I = 0x0C,
    J = 0x0D,
    K = 0x0E,
    L = 0x0F,
    M = 0x10,
    N = 0x11,
    O = 0x12,
    P = 0x13,
    Q = 0x14,
    R = 0x15,
    S = 0x16,
    T = 0x17,
    U = 0x18,
    V = 0x19,
    W = 0x1A,
    X = 0x1B,
    Y = 0x1C,
    Z = 0x1D,
    Digit1 = 0x1E,
    Digit2 = 0x1F,
    Digit3 = 0x20,
    Digit4 = 0x21,
    Digit5 = 0x22,
    Digit6 = 0x23,
    Digit7 = 0x24,
    Digit8 = 0x25,
    Digit9 = 0x26,
    Digit0 = 0x27,
    Enter = 0x28,
    Escape = 0x29,
    Backspace = 0x2A,
    Tab = 0x2B,
    Space = 0x2C,
    Minus = 0x2D,
    Equal = 0x2E,
    LeftBrace = 0x2F,
    RightBrace = 0x30,
    Backslash = 0x31,
    Semicolon = 0x33,
    Quote = 0x34,
    Grave = 0x35,
    Comma = 0x36,
    Period = 0x37,
    Slash = 0x38,
    CapsLock = 0x39,
    F1 = 0x3A,
    F2 = 0x3B,
    F3 = 0x3C,
    F4 = 0x3D,
    F5 = 0x3E,
    F6 = 0x3F,
    F7 = 0x40,
    F8 = 0x41,
    F9 = 0x42,
    F10 = 0x43,
    F11 = 0x44,
    F12 = 0x45,
    PrintScreen = 0x46,
    ScrollLock = 0x47,
    Pause = 0x48,
    Insert = 0x49,
    Home = 0x4A,
    PageUp = 0x4B,
    Delete = 0x4C,
    End = 0x4D,
    PageDown = 0x4E,
    RightArrow = 0x4F,
    LeftArrow = 0x50,
    DownArrow = 0x51,
    UpArrow = 0x52,
    NumLock = 0x53,
    KeypadSlash = 0x54,
    KeypadAsterisk = 0x55,
    KeypadMinus = 0x56,
    KeypadPlus = 0x57,
    KeypadEnter = 0x58,
    Keypad1 = 0x59,
    Keypad2 = 0x5A,
    Keypad3 = 0x5B,
    Keypad4 = 0x5C,
    Keypad5 = 0x5D,
    Keypad6 = 0x5E,
    Keypad7 = 0x5F,
    Keypad8 = 0x60,
    Keypad9 = 0x61,
    Keypad0 = 0x62,
    KeypadPeriod = 0x63,
    // Modifier keys are reported separately in boot protocol
}

/// Convert HID usage code to ASCII character (US keyboard layout)
pub fn hid_to_ascii(usage: u8, shift: bool) -> Option<char> {
    match usage {
        0x04 => Some(if shift { 'A' } else { 'a' }),
        0x05 => Some(if shift { 'B' } else { 'b' }),
        0x06 => Some(if shift { 'C' } else { 'c' }),
        0x07 => Some(if shift { 'D' } else { 'd' }),
        0x08 => Some(if shift { 'E' } else { 'e' }),
        0x09 => Some(if shift { 'F' } else { 'f' }),
        0x0A => Some(if shift { 'G' } else { 'g' }),
        0x0B => Some(if shift { 'H' } else { 'h' }),
        0x0C => Some(if shift { 'I' } else { 'i' }),
        0x0D => Some(if shift { 'J' } else { 'j' }),
        0x0E => Some(if shift { 'K' } else { 'k' }),
        0x0F => Some(if shift { 'L' } else { 'l' }),
        0x10 => Some(if shift { 'M' } else { 'm' }),
        0x11 => Some(if shift { 'N' } else { 'n' }),
        0x12 => Some(if shift { 'O' } else { 'o' }),
        0x13 => Some(if shift { 'P' } else { 'p' }),
        0x14 => Some(if shift { 'Q' } else { 'q' }),
        0x15 => Some(if shift { 'R' } else { 'r' }),
        0x16 => Some(if shift { 'S' } else { 's' }),
        0x17 => Some(if shift { 'T' } else { 't' }),
        0x18 => Some(if shift { 'U' } else { 'u' }),
        0x19 => Some(if shift { 'V' } else { 'v' }),
        0x1A => Some(if shift { 'W' } else { 'w' }),
        0x1B => Some(if shift { 'X' } else { 'x' }),
        0x1C => Some(if shift { 'Y' } else { 'y' }),
        0x1D => Some(if shift { 'Z' } else { 'z' }),
        0x1E => Some(if shift { '!' } else { '1' }),
        0x1F => Some(if shift { '@' } else { '2' }),
        0x20 => Some(if shift { '#' } else { '3' }),
        0x21 => Some(if shift { '$' } else { '4' }),
        0x22 => Some(if shift { '%' } else { '5' }),
        0x23 => Some(if shift { '^' } else { '6' }),
        0x24 => Some(if shift { '&' } else { '7' }),
        0x25 => Some(if shift { '*' } else { '8' }),
        0x26 => Some(if shift { '(' } else { '9' }),
        0x27 => Some(if shift { ')' } else { '0' }),
        0x28 => Some('\n'), // Enter
        0x29 => Some('\x1B'), // Escape
        0x2A => Some('\x08'), // Backspace
        0x2B => Some('\t'), // Tab
        0x2C => Some(' '), // Space
        0x2D => Some(if shift { '_' } else { '-' }),
        0x2E => Some(if shift { '+' } else { '=' }),
        0x2F => Some(if shift { '{' } else { '[' }),
        0x30 => Some(if shift { '}' } else { ']' }),
        0x31 => Some(if shift { '|' } else { '\\' }),
        0x33 => Some(if shift { ':' } else { ';' }),
        0x34 => Some(if shift { '"' } else { '\'' }),
        0x35 => Some(if shift { '~' } else { '`' }),
        0x36 => Some(if shift { '<' } else { ',' }),
        0x37 => Some(if shift { '>' } else { '.' }),
        0x38 => Some(if shift { '?' } else { '/' }),
        _ => None,
    }
}

// ============================================================================
// HID DEVICE STATE
// ============================================================================

/// HID device type
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HidDeviceType {
    Keyboard,
    Mouse,
    Gamepad,
    Generic,
    Unknown,
}

/// HID device state
#[derive(Clone, Debug)]
pub struct HidDeviceState {
    /// Device type
    pub device_type: HidDeviceType,
    /// Current keyboard report
    pub keyboard: KeyboardBootReport,
    /// Previous keyboard report (for change detection)
    pub prev_keyboard: KeyboardBootReport,
    /// Current mouse report
    pub mouse: MouseBootReport,
    /// Mouse accumulator (for absolute positioning)
    pub mouse_x: i32,
    pub mouse_y: i32,
    /// LED state (NumLock, CapsLock, ScrollLock)
    pub leds: u8,
    /// Report interval in milliseconds
    pub poll_interval_ms: u8,
    /// Boot protocol enabled
    pub boot_protocol: bool,
}

impl HidDeviceState {
    pub fn new(device_type: HidDeviceType) -> Self {
        Self {
            device_type,
            keyboard: KeyboardBootReport::new(),
            prev_keyboard: KeyboardBootReport::new(),
            mouse: MouseBootReport::new(),
            mouse_x: 0,
            mouse_y: 0,
            leds: 0,
            poll_interval_ms: 10,
            boot_protocol: false,
        }
    }

    /// Update keyboard state from report
    pub fn update_keyboard(&mut self, report: &[u8]) {
        if report.len() >= 8 {
            self.prev_keyboard = self.keyboard;
            self.keyboard.modifiers = report[0];
            self.keyboard.keys.copy_from_slice(&report[2..8]);
        }
    }

    /// Update mouse state from report
    pub fn update_mouse(&mut self, report: &[u8]) {
        if report.len() >= 3 {
            self.mouse.buttons = report[0];
            self.mouse.x = report[1] as i8;
            self.mouse.y = report[2] as i8;
            self.mouse_x += self.mouse.x as i32;
            self.mouse_y += self.mouse.y as i32;
        }
    }

    /// Get newly pressed keys
    pub fn new_keys(&self) -> Vec<u8> {
        let mut new = Vec::new();
        for key in self.keyboard.keys.iter() {
            if *key != 0 && !self.prev_keyboard.keys.contains(key) {
                new.push(*key);
            }
        }
        new
    }

    /// Get newly released keys
    pub fn released_keys(&self) -> Vec<u8> {
        let mut released = Vec::new();
        for key in self.prev_keyboard.keys.iter() {
            if *key != 0 && !self.keyboard.keys.contains(key) {
                released.push(*key);
            }
        }
        released
    }
}

// ============================================================================
// HID DRIVER
// ============================================================================

/// HID Driver instance
pub struct HidDriver {
    /// USB device reference
    pub device: UsbDevice,
    /// Interface number
    pub interface: u8,
    /// Interrupt IN endpoint
    pub interrupt_in: Option<UsbEndpoint>,
    /// Interrupt OUT endpoint (optional, for LEDs)
    pub interrupt_out: Option<UsbEndpoint>,
    /// Device state
    pub state: Mutex<HidDeviceState>,
    /// Initialized flag
    pub initialized: AtomicBool,
}

impl HidDriver {
    /// Create new HID driver
    pub fn new(device: UsbDevice, interface: u8) -> Self {
        Self {
            device,
            interface,
            interrupt_in: None,
            interrupt_out: None,
            state: Mutex::new(HidDeviceState::new(HidDeviceType::Unknown)),
            initialized: AtomicBool::new(false),
        }
    }

    /// Initialize HID device
    pub fn init(&mut self) -> Result<(), UsbError> {
        // Find interrupt endpoints
        for iface in &self.device.interfaces {
            if iface.interface_number == self.interface {
                // Determine device type from usage page
                // For now, assume keyboard if HID class
                
                for ep in &iface.endpoints {
                    if ep.transfer_type == UsbTransferType::Interrupt {
                        if ep.direction == UsbDirection::In {
                            self.interrupt_in = Some(*ep);
                        } else {
                            self.interrupt_out = Some(*ep);
                        }
                    }
                }
                break;
            }
        }

        // Set boot protocol for keyboard/mouse
        self.set_boot_protocol(true)?;

        // Set idle rate (how often device sends reports)
        self.set_idle(0, 10)?; // 10ms

        self.initialized.store(true, Ordering::SeqCst);
        crate::serial_println!(
            "[HID] Device initialized on interface {} (type: {:?})",
            self.interface,
            self.state.lock().device_type
        );

        Ok(())
    }

    /// Set boot protocol or report protocol
    pub fn set_boot_protocol(&self, boot: bool) -> Result<(), UsbError> {
        let protocol = if boot { HID_PROTOCOL_BOOT } else { HID_PROTOCOL_REPORT };
        
        let setup = UsbSetupPacket {
            request_type: 0x21, // Host-to-device, class, interface
            request: HID_SET_PROTOCOL,
            value: protocol as u16,
            index: self.interface as u16,
            length: 0,
        };

        // Send control transfer
        let _ = setup; // Placeholder
        
        self.state.lock().boot_protocol = boot;
        Ok(())
    }

    /// Set idle rate (report interval)
    pub fn set_idle(&self, report_id: u8, duration_ms: u8) -> Result<(), UsbError> {
        let setup = UsbSetupPacket {
            request_type: 0x21,
            request: HID_SET_IDLE,
            value: ((duration_ms as u16) << 8) | (report_id as u16),
            index: self.interface as u16,
            length: 0,
        };

        let _ = setup;
        Ok(())
    }

    /// Set LED state (for keyboards)
    pub fn set_leds(&self, leds: u8) -> Result<(), UsbError> {
        let mut state = self.state.lock();
        state.leds = leds;
        
        // Send output report
        let setup = UsbSetupPacket {
            request_type: 0x21,
            request: HID_SET_REPORT,
            value: ((HID_REPORT_OUTPUT as u16) << 8) | 0, // Report ID 0
            index: self.interface as u16,
            length: 1,
        };

        let _ = setup;
        drop(state);

        // If we have interrupt OUT endpoint, use that instead
        // self.send_output_report(&[leds])?;

        Ok(())
    }

    /// Toggle NumLock
    pub fn toggle_num_lock(&self) -> Result<(), UsbError> {
        let mut state = self.state.lock();
        state.leds ^= 0x01;
        let leds = state.leds;
        drop(state);
        self.set_leds(leds)
    }

    /// Toggle CapsLock
    pub fn toggle_caps_lock(&self) -> Result<(), UsbError> {
        let mut state = self.state.lock();
        state.leds ^= 0x02;
        let leds = state.leds;
        drop(state);
        self.set_leds(leds)
    }

    /// Toggle ScrollLock
    pub fn toggle_scroll_lock(&self) -> Result<(), UsbError> {
        let mut state = self.state.lock();
        state.leds ^= 0x04;
        let leds = state.leds;
        drop(state);
        self.set_leds(leds)
    }

    /// Poll device for input (call periodically)
    pub fn poll(&self) -> Result<HidEvent, UsbError> {
        // In real implementation, read from interrupt IN endpoint
        // For now, return no event
        Ok(HidEvent::None)
    }

    /// Process received report data
    pub fn process_report(&self, data: &[u8]) -> HidEvent {
        let mut state = self.state.lock();
        
        match state.device_type {
            HidDeviceType::Keyboard => {
                state.update_keyboard(data);
                let new_keys = state.new_keys();
                let released = state.released_keys();
                
                if !new_keys.is_empty() {
                    HidEvent::KeyPress(new_keys)
                } else if !released.is_empty() {
                    HidEvent::KeyRelease(released)
                } else {
                    HidEvent::None
                }
            }
            HidDeviceType::Mouse => {
                state.update_mouse(data);
                HidEvent::MouseMove {
                    dx: state.mouse.x as i32,
                    dy: state.mouse.y as i32,
                    buttons: state.mouse.buttons,
                }
            }
            _ => HidEvent::None,
        }
    }
}

// ============================================================================
// HID EVENT
// ============================================================================

/// HID event type
#[derive(Clone, Debug)]
pub enum HidEvent {
    /// No event
    None,
    /// Key press event
    KeyPress(Vec<u8>),
    /// Key release event
    KeyRelease(Vec<u8>),
    /// Mouse movement event
    MouseMove {
        dx: i32,
        dy: i32,
        buttons: u8,
    },
    /// Gamepad event
    Gamepad {
        buttons: u16,
        axes: [i16; 6],
    },
}

// ============================================================================
// GLOBAL HID DEVICE REGISTRY
// ============================================================================

use alloc::collections::BTreeMap;

lazy_static::lazy_static! {
    static ref HID_DRIVERS: Mutex<BTreeMap<u8, Arc<Mutex<HidDriver>>>> = Mutex::new(BTreeMap::new());
}

/// Register HID driver
pub fn register_hid_driver(device: UsbDevice, interface: u8) -> Result<u8, UsbError> {
    let driver = HidDriver::new(device, interface);
    let id = interface; // Use interface as ID for now
    
    HID_DRIVERS.lock().insert(id, Arc::new(Mutex::new(driver)));
    Ok(id)
}

/// Get HID driver by ID
pub fn get_hid_driver(id: u8) -> Option<Arc<Mutex<HidDriver>>> {
    HID_DRIVERS.lock().get(&id).cloned()
}

/// Poll all HID devices
pub fn poll_all_hid() -> Vec<(u8, HidEvent)> {
    let mut events = Vec::new();
    let drivers = HID_DRIVERS.lock();
    
    for (id, driver) in drivers.iter() {
        if let Ok(event) = driver.lock().poll() {
            if !matches!(event, HidEvent::None) {
                events.push((*id, event));
            }
        }
    }
    
    events
}

/// Initialize all registered HID devices
pub fn init_all_hid() {
    let drivers = HID_DRIVERS.lock();
    for (id, driver) in drivers.iter() {
        if let Err(e) = driver.lock().init() {
            crate::serial_println!("[HID] Failed to init device {}: {:?}", id, e);
        }
    }
}

// ============================================================================
// KEYBOARD INPUT QUEUE
// ============================================================================

use alloc::collections::VecDeque;

/// Keyboard input queue for buffering key events
pub struct KeyboardQueue {
    queue: Mutex<VecDeque<u8>>,
}

impl KeyboardQueue {
    pub const fn new() -> Self {
        Self {
            queue: Mutex::new(VecDeque::new()),
        }
    }

    /// Push key to queue
    pub fn push(&self, key: u8) {
        self.queue.lock().push_back(key);
    }

    /// Pop key from queue
    pub fn pop(&self) -> Option<u8> {
        self.queue.lock().pop_front()
    }

    /// Check if queue is empty
    pub fn is_empty(&self) -> bool {
        self.queue.lock().is_empty()
    }

    /// Get queue length
    pub fn len(&self) -> usize {
        self.queue.lock().len()
    }
}

/// Global keyboard queue
pub static KEYBOARD_QUEUE: KeyboardQueue = KeyboardQueue::new();

/// Read key from keyboard queue (blocking)
pub fn read_key() -> u8 {
    loop {
        if let Some(key) = KEYBOARD_QUEUE.pop() {
            return key;
        }
        // Yield to scheduler
        // In real implementation, would block current task
        core::hint::spin_loop();
    }
}

/// Check if key is available
pub fn has_key() -> bool {
    !KEYBOARD_QUEUE.is_empty()
}

/// Get key without blocking
pub fn try_read_key() -> Option<u8> {
    KEYBOARD_QUEUE.pop()
}
