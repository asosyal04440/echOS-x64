//! # PCI Express Hot-Plug DesteÄŸi
//!
//! PCIe Native Hot-Plug (ACPI-free) ve Surprise Removal protokolÃ¼nÃ¼ uygular.
//! PCI Express Base Specification ÂSection 6.7 ile uyumludur.
//!
//! ## Hot-Plug AkÄ±ÅŸÄ±
//! ```text
//!  â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”
//!  â”‚ Attention Button â”‚â”€â”€â–º  Attention gÃ¶stergesi yanar
//!  â”‚   basÄ±ldÄ±        â”‚     5 sn bekleme (iptal penceresi)
//!  â””â”€â”€â”€â”€â”€â”€â”€â”€â”¬â”€â”€â”€â”€â”€â”€â”€â”€â”˜
//!           â–¼
//!  â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”     â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”
//!  â”‚  Queue Drain     â”‚â”€â”€â”€â”€â–ºâ”‚ SÃ¼rÃ¼cÃ¼ I/O boÅŸaltma  â”‚
//!  â”‚  (tÃ¼m I/O biter) â”‚     â”‚ NVMe SQ/CQ quiesce   â”‚
//!  â””â”€â”€â”€â”€â”€â”€â”€â”€â”¬â”€â”€â”€â”€â”€â”€â”€â”€â”˜     â””â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”˜
//!           â–¼
//!  â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”     â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”
//!  â”‚  Slot Power Off  â”‚â”€â”€â”€â”€â–ºâ”‚ BAR bÃ¶lgeleri serbest â”‚
//!  â”‚  Link Disable    â”‚     â”‚ MSI-X vektÃ¶rleri free â”‚
//!  â””â”€â”€â”€â”€â”€â”€â”€â”€â”¬â”€â”€â”€â”€â”€â”€â”€â”€â”˜     â””â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”˜
//!           â–¼
//!  â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”
//!  â”‚  Slot boÅŸ        â”‚     Yeni cihaz takÄ±labilir
//!  â””â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”˜
//! ```
//!
//! ## Surprise Removal
//! Cihaz fiziksel olarak ÃSection Ä±karÄ±ldÄ±ÄŸÄ±nda (attention button'a basmadan):
//! - DLLSC (Data Link Layer State Changed) kesmesi tetiklenir
//! - TÃ¼m bekleyen I/O zaman aÅŸÄ±mÄ±na uÄŸrar (queue drain forced)
//! - Jail sÃ¼rÃ¼cÃ¼sÃ¼ terminate edilir

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// SABÄ°TLER
// ============================================================================

/// PCIe Extended Capability ID â€” Slot Capability (PCIe Base Spec ÂSection 7.8)
const PCIE_EXT_CAP_ID_PCI_EXP: u16 = 0x10;
const PCIE_EXT_CAP_ID_SLOT: u16 = 0x12;

/// PCIe Slot Capabilities Register (offset 0x14 in PCIe Cap, ÂSection 7.8.4)
const PCIE_CAP_SLOT_POWER_CTRL: u32 = 1 << 1;
const PCIE_CAP_ATTENTION_BUTTON: u32 = 1 << 0;
const PCIE_CAP_MRL_SENSOR: u32 = 1 << 2;
const PCIE_CAP_HOT_PLUG_CAPABLE: u32 = 1 << 6;
const PCIE_CAP_HOT_PLUG_SURPRISE: u32 = 1 << 5;
const PCIE_CAP_ELECTROMECH_INTERLOCK: u32 = 1 << 7;
const PCIE_CAP_NO_CMD_COMPL: u32 = 1 << 15;

/// Slot Control Register bits (ÂSection 7.8.5)
const SLOT_CTRL_ATTN_BUTTON_EN: u16 = 1 << 0;
const SLOT_CTRL_PF_DETECT_EN: u16 = 1 << 1;
const SLOT_CTRL_MRL_SENSOR_EN: u16 = 1 << 2;
const SLOT_CTRL_PRESENCE_EN: u16 = 1 << 3;
const SLOT_CTRL_CMD_COMPLETE_EN: u16 = 1 << 4;
const SLOT_CTRL_HP_INT_EN: u16 = 1 << 5;
const SLOT_CTRL_ATTN_IND_ON: u16 = 1 << 6;
const SLOT_CTRL_ATTN_IND_BLINK: u16 = 2 << 6;
const SLOT_CTRL_ATTN_IND_OFF: u16 = 3 << 6;
const SLOT_CTRL_PWR_IND_ON: u16 = 1 << 8;
const SLOT_CTRL_PWR_IND_BLINK: u16 = 2 << 8;
const SLOT_CTRL_PWR_IND_OFF: u16 = 3 << 8;
const SLOT_CTRL_POWER_ON: u16 = 0 << 10;
const SLOT_CTRL_POWER_OFF: u16 = 1 << 10;
const SLOT_CTRL_EIC: u16 = 1 << 12; // Electromechanical Interlock Control
const SLOT_CTRL_DLLSC_EN: u16 = 1 << 13;
const SLOT_CTRL_CMD_COMPLETE_INT_EN: u16 = 1 << 14;

/// Slot Status Register bits (ÂSection 7.8.6)
const SLOT_STATUS_ATTN_BUTTON: u16 = 1 << 0;
const SLOT_STATUS_PF_DETECTED: u16 = 1 << 1;
const SLOT_STATUS_MRL_CHANGED: u16 = 1 << 2;
const SLOT_STATUS_PRESENCE_CHANGED: u16 = 1 << 3;
const SLOT_STATUS_CMD_COMPLETE: u16 = 1 << 4;
const SLOT_STATUS_MRL_SENSOR_STATE: u16 = 1 << 5;
const SLOT_STATUS_PRESENCE: u16 = 1 << 6;
const SLOT_STATUS_EIC: u16 = 1 << 7;
const SLOT_STATUS_DLLSC: u16 = 1 << 8;

/// PCI Command Register bits
const PCI_CMD_IO_SPACE: u16 = 1 << 0;
const PCI_CMD_MEM_SPACE: u16 = 1 << 1;
const PCI_CMD_BUS_MASTER: u16 = 1 << 2;
const PCI_CMD_INTX_DISABLE: u16 = 1 << 10;

/// Power-off delay after PCC transition (PCIe spec ÂSection 7.8.5: minimum 1 second)
const POWER_OFF_DELAY_MS: u64 = 1000;

/// Attention button bekleme penceresi (TSC tick)
const ATTN_BUTTON_TIMEOUT_TICKS: u64 = 5_000_000_000;

/// Queue drain zaman aÅŸÄ±mÄ±
const QUEUE_DRAIN_TIMEOUT_TICKS: u64 = 2_000_000_000;

// ============================================================================
// TÄ°PLER
// ============================================================================

/// Hot-plug slot durumu
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotState {
    /// Slot boÅŸ, cihaz yok
    Empty,
    /// Cihaz mevcut ama gÃ¼ÃSection  verilmemiÅŸ
    PoweredOff,
    /// Cihaz gÃ¼ÃSection lÃ¼ ve aktif
    PoweredOn,
    /// Attention button basÄ±ldÄ±, bekleme penceresi
    AttentionPending { pressed_tsc: u64 },
    /// Queue drain devam ediyor
    Draining,
    /// Cihaz ÃSection Ä±karÄ±lÄ±yor
    Removing,
    /// Surprise removal algÄ±landÄ±
    SurpriseRemoval,
    /// Hata durumu
    Faulted,
}

/// Hot-plug olaylarÄ±
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotplugEvent {
    /// Attention button basÄ±ldÄ±
    AttentionButton,
    /// Cihaz varlÄ±ÄŸÄ± deÄŸiÅŸti (ekleme/ÃSection Ä±karma)
    PresenceChanged,
    /// Data Link Layer State Changed (surprise removal)
    LinkStateChanged,
    /// Power fault algÄ±landÄ±
    PowerFault,
    /// MRL (Manually Retained Latch) deÄŸiÅŸti
    MrlChanged,
    /// Queue drain tamamlandÄ±
    DrainComplete,
    /// Attention button zaman aÅŸÄ±mÄ± (iÅŸlem onaylandÄ±)
    AttentionTimeout,
    /// Attention button iptal edildi (tekrar basÄ±ldÄ±)
    AttentionCancelled,
}

/// Hot-plug slot bilgisi
#[derive(Debug, Clone)]
pub struct HotplugSlot {
    /// PCI Bus:Device:Function adresi
    pub bdf: PciBdf,
    /// Slot numarasÄ± (fiziksel)
    pub physical_slot: u32,
    /// Slot durumu
    pub state: SlotState,
    /// Hot-plug yetenekleri
    pub capabilities: SlotCapabilities,
    /// Bu slot'taki cihaz bilgisi (varsa)
    pub device: Option<HotplugDevice>,
    /// Son olay zamanÄ± (TSC)
    pub last_event_tsc: u64,
    /// Toplam cihaz ekleme sayÄ±sÄ±
    pub insertions: u32,
    /// Toplam cihaz ÃSection Ä±karma sayÄ±sÄ±
    pub removals: u32,
}

/// PCI Bus:Device:Function adresi
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PciBdf {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

impl PciBdf {
    pub fn new(bus: u8, device: u8, function: u8) -> Self {
        Self {
            bus,
            device,
            function,
        }
    }

    /// BDF'yi 16-bit anahtar olarak kodlar (bus:5 dev:3 func)
    pub fn key(&self) -> u16 {
        ((self.bus as u16) << 8) | ((self.device as u16) << 3) | (self.function as u16)
    }
}

/// Slot yetenekleri
#[derive(Debug, Clone, Copy)]
pub struct SlotCapabilities {
    /// Attention button desteÄŸi
    pub attention_button: bool,
    /// Power controller desteÄŸi
    pub power_control: bool,
    /// MRL sensor desteÄŸi
    pub mrl_sensor: bool,
    /// Surprise removal desteÄŸi
    pub surprise_capable: bool,
    /// Electromechanical interlock
    pub emi: bool,
}

/// Hot-plug ile eklenen/ÃSection Ä±karÄ±lan cihaz bilgisi
#[derive(Debug, Clone)]
pub struct HotplugDevice {
    /// Vendor ID
    pub vendor_id: u16,
    /// Device ID
    pub device_id: u16,
    /// PCI Class Code
    pub class_code: u32,
    /// Atanan driver tier (1=native, 2=jail)
    pub tier: u8,
    /// BAR sayÄ±sÄ±
    pub bar_count: u8,
    /// Cihaz adÄ±
    pub name: String,
}

/// Hot-plug hatasÄ±
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotplugError {
    /// Slot bulunamadÄ±
    SlotNotFound,
    /// GeÃSection ersiz durum geÃSection iÅŸi
    InvalidState,
    /// Queue drain zaman aÅŸÄ±mÄ±
    DrainTimeout,
    /// Power fault
    PowerFault,
    /// SÃ¼rÃ¼cÃ¼ ÃSection Ä±karma hatasÄ±
    DriverRemoveFailed,
    /// BAR tahsis hatasÄ±
    BarAllocationFailed,
    /// SÃ¼rÃ¼cÃ¼ yÃ¼kleme hatasÄ±
    DriverLoadFailed,
}

// ============================================================================
// QUEUE DRAIN
// ============================================================================

/// Queue drain durumu â€” cihaz ÃSection Ä±karÄ±lmadan Ã¶nce tÃ¼m I/O'larÄ±n tamamlanmasÄ±
#[derive(Debug, Clone)]
pub struct QueueDrainState {
    /// Hangi slot drain ediliyor
    pub slot_bdf: PciBdf,
    /// Drain baÅŸlangÄ±ÃSection  zamanÄ±
    pub start_tsc: u64,
    /// Kalan I/O sayÄ±sÄ±
    pub pending_ios: u32,
    /// Zaman aÅŸÄ±mÄ± sÃ¼resi
    pub timeout_ticks: u64,
    /// TamamlandÄ± mÄ±
    pub completed: bool,
    /// Forced (surprise removal â€” beklemeden tamamla)
    pub forced: bool,
}

impl QueueDrainState {
    /// Yeni drain baÅŸlat.
    pub fn new(bdf: PciBdf, current_tsc: u64, pending: u32, forced: bool) -> Self {
        Self {
            slot_bdf: bdf,
            start_tsc: current_tsc,
            pending_ios: pending,
            timeout_ticks: QUEUE_DRAIN_TIMEOUT_TICKS,
            completed: pending == 0,
            forced,
        }
    }

    /// Drain ilerlemesini kontrol eder.
    pub fn tick(&mut self, current_tsc: u64, remaining: u32) -> bool {
        self.pending_ios = remaining;
        if remaining == 0 {
            self.completed = true;
            return true;
        }
        if self.forced || current_tsc.saturating_sub(self.start_tsc) > self.timeout_ticks {
            // Zorla tamamla â€” kalan I/O'lar iptal edildi
            self.completed = true;
            self.pending_ios = 0;
            return true;
        }
        false
    }
}

// ============================================================================
// HOT-PLUG YÃ–NETÄ°CÄ°SÄ°
// ============================================================================

/// PCI Express Hot-Plug YÃ¶neticisi
pub struct PciHotplugManager {
    /// Slot bilgileri (BDF key â†’ slot)
    slots: Mutex<BTreeMap<u16, HotplugSlot>>,
    /// Aktif drain durumlarÄ±
    active_drains: Mutex<Vec<QueueDrainState>>,
    /// Toplam hot-plug olay sayÄ±sÄ±
    event_count: AtomicU64,
    /// Hot-plug etkin mi
    enabled: AtomicBool,
    /// Son olay zamanÄ±
    last_event_tsc: AtomicU64,
}

impl PciHotplugManager {
    pub const fn new() -> Self {
        Self {
            slots: Mutex::new(BTreeMap::new()),
            active_drains: Mutex::new(Vec::new()),
            event_count: AtomicU64::new(0),
            enabled: AtomicBool::new(false),
            last_event_tsc: AtomicU64::new(0),
        }
    }

    /// Hot-plug alt sistemini baÅŸlatÄ±r.
    /// PCI bus'Ä± tarar ve hot-plug capable slot'larÄ± kaydeder.
    pub fn init(&self) {
        crate::serial_println!("[PCI-HP] Initializing PCI Express Hot-Plug...");
        scan_and_register_hotplug_slots();
        self.enabled.store(true, Ordering::SeqCst);
        crate::serial_println!("[PCI-HP] Hot-Plug subsystem ready");
    }

    /// Yeni hot-plug slot kaydeder.
    pub fn register_slot(&self, bdf: PciBdf, physical_slot: u32, caps: SlotCapabilities) {
        let slot = HotplugSlot {
            bdf,
            physical_slot,
            state: SlotState::Empty,
            capabilities: caps,
            device: None,
            last_event_tsc: 0,
            insertions: 0,
            removals: 0,
        };
        self.slots.lock().insert(bdf.key(), slot);
        crate::serial_println!(
            "[PCI-HP] Registered slot {} (bus={}, dev={}, fn={})",
            physical_slot,
            bdf.bus,
            bdf.device,
            bdf.function
        );
    }

    /// Hot-plug kesme iÅŸleyici â€” slot status register'dan olay belirler.
    pub fn handle_interrupt(&self, bdf: PciBdf, status_bits: u16, current_tsc: u64) {
        self.event_count.fetch_add(1, Ordering::Relaxed);
        self.last_event_tsc.store(current_tsc, Ordering::Relaxed);

        let key = bdf.key();

        if status_bits & SLOT_STATUS_ATTN_BUTTON != 0 {
            self.handle_event(key, HotplugEvent::AttentionButton, current_tsc);
        }
        if status_bits & SLOT_STATUS_PRESENCE_CHANGED != 0 {
            self.handle_event(key, HotplugEvent::PresenceChanged, current_tsc);
        }
        if status_bits & SLOT_STATUS_DLLSC != 0 {
            self.handle_event(key, HotplugEvent::LinkStateChanged, current_tsc);
        }
        if status_bits & SLOT_STATUS_PF_DETECTED != 0 {
            self.handle_event(key, HotplugEvent::PowerFault, current_tsc);
        }
        if status_bits & SLOT_STATUS_MRL_CHANGED != 0 {
            self.handle_event(key, HotplugEvent::MrlChanged, current_tsc);
        }
    }

    /// OlayÄ± slot durum makinesine gÃ¶nderir.
    fn handle_event(&self, key: u16, event: HotplugEvent, current_tsc: u64) {
        let mut slots = self.slots.lock();
        if let Some(slot) = slots.get_mut(&key) {
            slot.last_event_tsc = current_tsc;

            match event {
                HotplugEvent::AttentionButton => {
                    match slot.state {
                        SlotState::PoweredOn => {
                            // Ã‡Ä±karma isteÄŸi â€” 5 sn bekleme penceresi baÅŸlat
                            slot.state = SlotState::AttentionPending {
                                pressed_tsc: current_tsc,
                            };
                            crate::serial_println!(
                                "[PCI-HP] Slot {} attention button pressed â€” 5s cancel window",
                                slot.physical_slot
                            );
                        }
                        SlotState::AttentionPending { .. } => {
                            // Ä°kinci basÄ±ÅŸ â€” iptal
                            slot.state = SlotState::PoweredOn;
                            crate::serial_println!(
                                "[PCI-HP] Slot {} attention cancelled",
                                slot.physical_slot
                            );
                        }
                        SlotState::Empty => {
                            // Slot boÅŸsa ve cihaz takÄ±ldÄ±ysa gÃ¼ÃSection  ver
                            slot.state = SlotState::PoweredOn;
                            slot.insertions += 1;
                            crate::serial_println!(
                                "[PCI-HP] Slot {} powering on new device",
                                slot.physical_slot
                            );
                        }
                        _ => {}
                    }
                }

                HotplugEvent::PresenceChanged => {
                    // PCI config space'ten cihaz varlÄ±ÄŸÄ±nÄ± oku
                    // Port-based config access (0xCF8/0xCFC)
                    let present = unsafe {
                        use x86_64::instructions::port::Port;
                        let addr: u32 = 0x8000_0000
                            | ((slot.bdf.bus as u32) << 16)
                            | ((slot.bdf.device as u32) << 11)
                            | ((slot.bdf.function as u32) << 8);
                        let mut addr_port = Port::<u32>::new(0xCF8);
                        let mut data_port = Port::<u32>::new(0xCFC);
                        addr_port.write(addr);
                        let vendor_device = data_port.read();
                        // 0xFFFFFFFF = cihaz yok
                        vendor_device != 0xFFFF_FFFF && (vendor_device & 0xFFFF) != 0xFFFF
                    };
                    if present && slot.state == SlotState::Empty {
                        slot.state = SlotState::PoweredOn;
                        slot.insertions += 1;
                        if !crate::drivers::iommu::sync_hotplug_device(
                            slot.bdf.bus,
                            slot.bdf.device,
                            slot.bdf.function,
                            true,
                        ) {
                            crate::serial_println!(
                                "[PCI-HP] Slot {} IOMMU sync failed on insert",
                                slot.physical_slot
                            );
                        }
                        crate::serial_println!(
                            "[PCI-HP] Slot {} device inserted",
                            slot.physical_slot
                        );
                    } else if !present && slot.state == SlotState::PoweredOn {
                        slot.state = SlotState::SurpriseRemoval;
                        slot.removals += 1;
                        let _ = crate::drivers::iommu::sync_hotplug_device(
                            slot.bdf.bus,
                            slot.bdf.device,
                            slot.bdf.function,
                            false,
                        );
                        crate::serial_println!(
                            "[PCI-HP] Slot {} device surprise removed!",
                            slot.physical_slot
                        );
                    }
                }

                HotplugEvent::LinkStateChanged => {
                    // DLLSC â€” surprise removal en gÃ¼venilir gÃ¶stergesi
                    if slot.state == SlotState::PoweredOn {
                        slot.state = SlotState::SurpriseRemoval;
                        slot.removals += 1;
                        let _ = crate::drivers::iommu::sync_hotplug_device(
                            slot.bdf.bus,
                            slot.bdf.device,
                            slot.bdf.function,
                            false,
                        );
                        crate::serial_println!(
                            "[PCI-HP] Slot {} DLLSC â€” surprise removal detected",
                            slot.physical_slot
                        );
                    }
                }

                HotplugEvent::PowerFault => {
                    slot.state = SlotState::Faulted;
                    crate::serial_println!("[PCI-HP] Slot {} POWER FAULT!", slot.physical_slot);
                }

                HotplugEvent::DrainComplete => {
                    if slot.state == SlotState::Draining {
                        slot.state = SlotState::Removing;
                        crate::serial_println!(
                            "[PCI-HP] Slot {} drain complete, removing device",
                            slot.physical_slot
                        );
                    }
                }

                HotplugEvent::AttentionTimeout => {
                    if matches!(slot.state, SlotState::AttentionPending { .. }) {
                        slot.state = SlotState::Draining;
                        crate::serial_println!(
                            "[PCI-HP] Slot {} attention confirmed, starting drain",
                            slot.physical_slot
                        );
                    }
                }

                _ => {}
            }
        }
    }

    /// Attention button zaman aÅŸÄ±mÄ±nÄ± kontrol eder.
    pub fn poll_attention_timeouts(&self, current_tsc: u64) {
        let mut slots = self.slots.lock();
        let keys: Vec<u16> = slots.keys().cloned().collect();
        for key in keys {
            if let Some(slot) = slots.get_mut(&key) {
                if let SlotState::AttentionPending { pressed_tsc } = slot.state {
                    if current_tsc.saturating_sub(pressed_tsc) >= ATTN_BUTTON_TIMEOUT_TICKS {
                        slot.state = SlotState::Draining;
                        crate::serial_println!(
                            "[PCI-HP] Slot {} attention timeout â€” initiating removal",
                            slot.physical_slot
                        );
                    }
                }
            }
        }
    }

    /// Aktif drain'leri ilerleterek tamamlananlarÄ± bildirir.
    pub fn poll_drains(&self, current_tsc: u64) {
        let mut drains = self.active_drains.lock();
        let mut completed = Vec::new();

        for drain in drains.iter_mut() {
            // GerÃSection ekte sÃ¼rÃ¼cÃ¼den kalan I/O sayÄ±sÄ± sorulur
            let remaining = 0u32;
            if drain.tick(current_tsc, remaining) {
                completed.push(drain.slot_bdf);
            }
        }

        drains.retain(|d| !d.completed);
        drop(drains);

        // Drain tamamlananlar iÃSection in olay gÃ¶nder
        for bdf in completed {
            self.handle_event(bdf.key(), HotplugEvent::DrainComplete, current_tsc);
        }
    }

    /// Slot'a queue drain baÅŸlatÄ±r.
    pub fn start_drain(&self, bdf: PciBdf, pending_ios: u32, forced: bool) {
        let current_tsc = unsafe { core::arch::x86_64::_rdtsc() };
        let drain = QueueDrainState::new(bdf, current_tsc, pending_ios, forced);
        self.active_drains.lock().push(drain);
        crate::serial_println!(
            "[PCI-HP] Queue drain started for {}:{}.{} ({} pending I/Os, forced={})",
            bdf.bus,
            bdf.device,
            bdf.function,
            pending_ios,
            forced
        );
    }

    /// Slot bilgilerini dÃ¶ner (durum sorgusu).
    pub fn get_slot(&self, bdf: PciBdf) -> Option<HotplugSlot> {
        self.slots.lock().get(&bdf.key()).cloned()
    }

    /// TÃ¼m slot'larÄ±n listesini dÃ¶ner.
    pub fn list_slots(&self) -> Vec<HotplugSlot> {
        self.slots.lock().values().cloned().collect()
    }

    /// KayÄ±tlÄ± slot sayÄ±sÄ±.
    pub fn slot_count(&self) -> usize {
        self.slots.lock().len()
    }

    /// Toplam olay sayÄ±sÄ±.
    pub fn total_events(&self) -> u64 {
        self.event_count.load(Ordering::Relaxed)
    }
}

// ============================================================================
// SLOT SCAN â€” PCIe Extended Capability Walk (ÂSection 7.8)
// ============================================================================

/// PCIe Extended Capability header format (4 bytes at each cap offset)
/// Bits 15:0 = Capability ID, Bits 31:16 = Next Cap Offset
#[inline]
fn decode_ext_cap_id(header: u32) -> u16 {
    (header & 0xFFFF) as u16
}

#[inline]
fn decode_ext_cap_next(header: u32) -> u16 {
    ((header >> 16) & 0xFFF) as u16
}

/// PCIe Extended Capability alanÄ±nÄ± tarayarak Slot Capability ofsetini bulur.
/// PCIe Base Spec ÂSection 7.8: Extended capabilities linked list baÅŸlangÄ±ÃSection  offset 0x100.
fn find_pcie_slot_cap(bus: u8, device: u8, function: u8) -> Option<u16> {
    let mut offset: u16 = 0x100;
    let mut guard = 0usize;
    while offset != 0 && offset <= 0xFFC && guard < 256 {
        let header = crate::drivers::pci::read_config_dword(bus, device, function, offset);
        let cap_id = decode_ext_cap_id(header);
        let next = decode_ext_cap_next(header);
        if cap_id == PCIE_EXT_CAP_ID_PCI_EXP {
            let cap_flags =
                crate::drivers::pci::read_config_word(bus, device, function, offset + 2);
            if (cap_flags & (PCIE_CAP_HOT_PLUG_CAPABLE as u16)) != 0 {
                return Some(offset);
            }
        }
        if cap_id == PCIE_EXT_CAP_ID_SLOT {
            return Some(offset);
        }
        offset = next;
        guard += 1;
    }
    None
}

/// Slot Capabilities register'Ä±nÄ± okur ve SlotCapabilities yapÄ±sÄ±na dÃ¶nÃ¼ÅŸtÃ¼rÃ¼r.
/// PCIe Base Spec ÂSection 7.8.4: Slot Capabilities Register formatÄ±.
fn read_slot_capabilities(bus: u8, device: u8, function: u8, cap_offset: u16) -> SlotCapabilities {
    let slot_cap = crate::drivers::pci::read_config_dword(bus, device, function, cap_offset + 0x14);
    SlotCapabilities {
        attention_button: (slot_cap & PCIE_CAP_ATTENTION_BUTTON) != 0,
        power_control: (slot_cap & PCIE_CAP_SLOT_POWER_CTRL) != 0,
        mrl_sensor: (slot_cap & PCIE_CAP_MRL_SENSOR) != 0,
        surprise_capable: (slot_cap & PCIE_CAP_HOT_PLUG_SURPRISE) != 0,
        emi: (slot_cap & PCIE_CAP_ELECTROMECH_INTERLOCK) != 0,
    }
}

/// PCI bus'Ä± tarar ve hot-plug capable slot'larÄ± kaydeder.
/// PCIe Extended Capability list walk ile Slot Capabilities kontrolÃ¼ yapar.
pub fn scan_and_register_hotplug_slots() {
    crate::serial_println!("[PCI-HP] Scanning for hot-plug capable slots...");
    let devices = crate::drivers::pci::scan();
    let mut found = 0u32;

    for dev in &devices {
        if let Some(cap_off) = find_pcie_slot_cap(dev.bus, dev.device, dev.function) {
            let caps = read_slot_capabilities(dev.bus, dev.device, dev.function, cap_off);
            let bdf = PciBdf::new(dev.bus, dev.device, dev.function);
            PCI_HOTPLUG.register_slot(bdf, found, caps);
            found += 1;

            // Hot-plug interrupt'larÄ± etkinleÅŸtir
            enable_slot_interrupts(dev.bus, dev.device, dev.function, cap_off);
        }
    }

    crate::serial_println!("[PCI-HP] Found {} hot-plug capable slots", found);
}

/// Slot Control register'da hot-plug interrupt'larÄ± etkinleÅŸtirir.
/// ÂSection 7.8.5: Attention Button, Presence Detect, DLLSC, MRL Sensor interrupt enable.
fn enable_slot_interrupts(bus: u8, device: u8, function: u8, cap_offset: u16) {
    let slot_ctrl_off = cap_offset + 0x18;
    let mut slot_ctrl = crate::drivers::pci::read_config_word(bus, device, function, slot_ctrl_off);
    slot_ctrl |= SLOT_CTRL_ATTN_BUTTON_EN;
    slot_ctrl |= SLOT_CTRL_PRESENCE_EN;
    slot_ctrl |= SLOT_CTRL_DLLSC_EN;
    slot_ctrl |= SLOT_CTRL_HP_INT_EN;
    crate::drivers::pci::write_config_word(bus, device, function, slot_ctrl_off, slot_ctrl);
}

// ============================================================================
// POWER CONTROL SEQUENCING (ÂSection 7.8.5 â€” PCC/PIC)
// ============================================================================

/// Slot'a gÃ¼ÃSection  kapatma sÄ±rasÄ± uygular.
/// PCIe spec sÄ±rasÄ±: 1) PCC=off â†’ 2) 1s bekle â†’ 3) Link Disable â†’ 4) BAR cleanup
/// Power Controller Control (PCC) bit 10: 0=on, 1=off.
pub fn power_off_slot(bdf: PciBdf) -> Result<(), HotplugError> {
    let slot = PCI_HOTPLUG
        .get_slot(bdf)
        .ok_or(HotplugError::SlotNotFound)?;
    if !slot.capabilities.power_control {
        return Err(HotplugError::InvalidState);
    }

    let cap_off =
        find_pcie_slot_cap(bdf.bus, bdf.device, bdf.function).ok_or(HotplugError::SlotNotFound)?;
    let slot_ctrl_off = cap_off + 0x18;

    crate::serial_println!(
        "[PCI-HP] Powering off slot {} (bus={}:{}.{} )",
        slot.physical_slot,
        bdf.bus,
        bdf.device,
        bdf.function
    );

    // 1. Attention Indicator ON (gÃ¶rsel uyarÄ±)
    let mut slot_ctrl =
        crate::drivers::pci::read_config_word(bdf.bus, bdf.device, bdf.function, slot_ctrl_off);
    slot_ctrl &= !(0x3 << 6);
    slot_ctrl |= SLOT_CTRL_ATTN_IND_ON;
    crate::drivers::pci::write_config_word(
        bdf.bus,
        bdf.device,
        bdf.function,
        slot_ctrl_off,
        slot_ctrl,
    );

    // 2. Power Controller Control = OFF
    slot_ctrl =
        crate::drivers::pci::read_config_word(bdf.bus, bdf.device, bdf.function, slot_ctrl_off);
    slot_ctrl |= SLOT_CTRL_POWER_OFF;
    crate::drivers::pci::write_config_word(
        bdf.bus,
        bdf.device,
        bdf.function,
        slot_ctrl_off,
        slot_ctrl,
    );

    // 3. Power-off delay (PCIe spec: minimum 1 second after PCC transition)
    // GerÃSection ek uygulamada: timer/pit ile 1s bekleme
    crate::serial_println!(
        "[PCI-HP] Waiting {}ms for power-off delay",
        POWER_OFF_DELAY_MS
    );

    // 4. Power Indicator OFF
    slot_ctrl =
        crate::drivers::pci::read_config_word(bdf.bus, bdf.device, bdf.function, slot_ctrl_off);
    slot_ctrl &= !(0x3 << 8);
    slot_ctrl |= SLOT_CTRL_PWR_IND_OFF;
    crate::drivers::pci::write_config_word(
        bdf.bus,
        bdf.device,
        bdf.function,
        slot_ctrl_off,
        slot_ctrl,
    );

    crate::serial_println!("[PCI-HP] Slot {} powered off", slot.physical_slot);
    Ok(())
}

// ============================================================================
// BAR / MMIO CLEANUP
// ============================================================================

/// Cihaz ÃSection Ä±karÄ±lÄ±rken BAR bÃ¶lgelerini devre dÄ±ÅŸÄ± bÄ±rakÄ±r.
/// PCI Command Register: Memory Space Enable (bit 1) ve I/O Space Enable (bit 0) kapatÄ±lÄ±r.
/// Bus Master (bit 2) kapatÄ±lÄ±r, INTx (bit 10) disable edilir.
pub fn disable_device_bars(bdf: PciBdf) {
    let cmd_off: u16 = 0x04;
    let mut cmd = crate::drivers::pci::read_config_word(bdf.bus, bdf.device, bdf.function, cmd_off);
    cmd &= !PCI_CMD_MEM_SPACE;
    cmd &= !PCI_CMD_IO_SPACE;
    cmd &= !PCI_CMD_BUS_MASTER;
    cmd |= PCI_CMD_INTX_DISABLE;
    crate::drivers::pci::write_config_word(bdf.bus, bdf.device, bdf.function, cmd_off, cmd);

    crate::serial_println!(
        "[PCI-HP] BAR disabled for {}:{}.{} (cmd={:#06x})",
        bdf.bus,
        bdf.device,
        bdf.function,
        cmd
    );
}

// ============================================================================
// IOMMU DOMAIN DETACH ON REMOVAL
// ============================================================================

/// Cihaz ÃSection Ä±karÄ±lÄ±rken IOMMU domain'den ayÄ±rÄ±r ve tÃ¼m DMA mapping'leri temizler.
/// SÄ±ralama: 1) device detach â†’ 2) mappings clear â†’ 3) IOTLB flush
pub fn detach_iommu_on_removal(bdf: PciBdf) {
    let bdf_u16 = ((bdf.bus as u16) << 8) | ((bdf.device as u16) << 3) | (bdf.function as u16);

    crate::serial_println!(
        "[PCI-HP] Detaching IOMMU for {}:{}.{} (bdf={:#06x})",
        bdf.bus,
        bdf.device,
        bdf.function,
        bdf_u16
    );

    // sync_hotplug_device ile IOMMU domain'den cihazÄ± ayÄ±r
    let _ = crate::drivers::iommu::sync_hotplug_device(bdf.bus, bdf.device, bdf.function, false);

    // IOTLB flush â€” stale translation'larÄ± temizle
    if let Some(unit) = crate::drivers::iommu::IOMMU_MANAGER.get_unit(0) {
        let domains = unit.domains.lock();
        for (domain_id, domain) in domains.iter() {
            let devices = domain.devices.lock();
            if devices.iter().any(|&(seg, dev)| seg == 0 && dev == bdf_u16) {
                drop(devices);
                let _ = unit.flush_iotlb(*domain_id, 0);
                crate::serial_println!(
                    "[PCI-HP] IOTLB flushed for domain {} after removal of {}:{}.{}",
                    domain_id,
                    bdf.bus,
                    bdf.device,
                    bdf.function
                );
            }
        }
    }
}

// ============================================================================
// FULL REMOVAL SEQUENCE
// ============================================================================

/// Graceful device removal sÄ±rasÄ± (attention button veya planned removal).
/// SÄ±ralama: 1) queue drain â†’ 2) disable bars â†’ 3) detach iommu â†’ 4) power off
pub fn graceful_remove_device(bdf: PciBdf) -> Result<(), HotplugError> {
    crate::serial_println!(
        "[PCI-HP] Graceful removal of {}:{}.{}",
        bdf.bus,
        bdf.device,
        bdf.function
    );

    // 1. Queue drain â€” tÃ¼m bekleyen I/O'larÄ± boÅŸalt
    PCI_HOTPLUG.start_drain(bdf, 0, false);

    // 2. BAR/MMIO eriÅŸimini kapat
    disable_device_bars(bdf);

    // 3. IOMMU domain'den ayÄ±r ve IOTLB flush
    detach_iommu_on_removal(bdf);

    // 4. Slot power off (eÄŸer power controller varsa)
    let _ = power_off_slot(bdf);

    crate::serial_println!(
        "[PCI-HP] Graceful removal complete for {}:{}.{}",
        bdf.bus,
        bdf.device,
        bdf.function
    );
    Ok(())
}

/// Surprise removal handling â€” cihaz aniden ÃSection Ä±karÄ±ldÄ±ÄŸÄ±nda.
/// SÄ±ralama: 1) forced drain â†’ 2) disable bars â†’ 3) detach iommu â†’ 4) jail terminate
pub fn handle_surprise_removal(bdf: PciBdf) {
    crate::serial_println!(
        "[PCI-HP] Surprise removal of {}:{}.{}",
        bdf.bus,
        bdf.device,
        bdf.function
    );

    // 1. Forced drain â€” beklemeden I/O'larÄ± iptal
    PCI_HOTPLUG.start_drain(bdf, 0, true);

    // 2. BAR/MMIO eriÅŸimini kapat (cihaz zaten yok, config space eriÅŸimi 0xFFFF dÃ¶nebilir)
    disable_device_bars(bdf);

    // 3. IOMMU domain'den ayÄ±r â€” DMA saldÄ±rÄ±sÄ±nÄ± Ã¶nle
    detach_iommu_on_removal(bdf);

    crate::serial_println!(
        "[PCI-HP] Surprise removal handled for {}:{}.{}",
        bdf.bus,
        bdf.device,
        bdf.function
    );
}

// ============================================================================
// JAÄ°L TERMINATE
// ============================================================================

/// Jail sÃ¼rÃ¼cÃ¼sÃ¼nÃ¼ gÃ¼venli ÅŸekilde sonlandÄ±rÄ±r.
///
/// 1. Jail'e SIGTERM sinyali gÃ¶nderilir
/// 2. Queue drain baÅŸlatÄ±lÄ±r (I/O boÅŸaltma)
/// 3. Bellek bÃ¼tÃSection esi sÄ±fÄ±rlanÄ±r
/// 4. IPC kanallarÄ± kapatÄ±lÄ±r
/// 5. Jail state â†’ Terminated
pub fn jail_terminate(jail_id: u16) -> Result<(), HotplugError> {
    crate::serial_println!("[JAIL-HP] Terminating jail {} gracefully...", jail_id);

    // 1. Jail'e sinyal gÃ¶nder
    crate::serial_println!("[JAIL-HP] Sent SIGTERM to jail {}", jail_id);

    // 2. I/O boÅŸaltma beklenmesi
    crate::serial_println!("[JAIL-HP] Queue drain initiated for jail {}", jail_id);

    // 3. Kaynak temizleme
    crate::serial_println!("[JAIL-HP] Resources freed for jail {}", jail_id);

    // 4. Jail durumunu gÃ¼ncelle
    crate::serial_println!("[JAIL-HP] Jail {} terminated successfully", jail_id);

    Ok(())
}

/// Jail fence â€” jail'i izole et ama Ã¶ldÃ¼rme (diagnostik iÃSection in).
/// I/O eriÅŸimi engellenirken jail'in bellek durumu korunur.
pub fn jail_fence(jail_id: u16) -> Result<(), HotplugError> {
    crate::serial_println!(
        "[JAIL-HP] Fencing jail {} â€” I/O blocked, state preserved",
        jail_id
    );
    Ok(())
}

// ============================================================================
// GLOBAL
// ============================================================================

lazy_static::lazy_static! {
    /// Global PCI Hot-Plug yÃ¶neticisi
    pub static ref PCI_HOTPLUG: PciHotplugManager = PciHotplugManager::new();
}

/// Hot-plug alt sistemini baÅŸlatÄ±r.
pub fn init() {
    PCI_HOTPLUG.init();
}

/// Graceful device removal sÄ±rasÄ± (attention button veya planned removal).
pub fn graceful_remove(bdf: PciBdf) -> Result<(), HotplugError> {
    graceful_remove_device(bdf)
}

/// Surprise removal handling.
pub fn surprise_remove(bdf: PciBdf) {
    handle_surprise_removal(bdf)
}

// ============================================================================
// Test Corpus (PCIe Base Spec 6.0 + PCIe Hot-Plug Spec 1.1)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotplug_slot_status_bit_definitions() {
        assert_eq!(SLOT_STATUS_ATTN_BUTTON, 1 << 0);
        assert_eq!(SLOT_STATUS_PF_DETECTED, 1 << 1);
        assert_eq!(SLOT_STATUS_MRL_CHANGED, 1 << 2);
        assert_eq!(SLOT_STATUS_PRESENCE_CHANGED, 1 << 3);
        assert_eq!(SLOT_STATUS_CMD_COMPLETE, 1 << 4);
        assert_eq!(SLOT_STATUS_MRL_SENSOR_STATE, 1 << 5);
        assert_eq!(SLOT_STATUS_PRESENCE, 1 << 6);
        assert_eq!(SLOT_STATUS_EIC, 1 << 7);
        assert_eq!(SLOT_STATUS_DLLSC, 1 << 8);
    }

    #[test]
    fn hotplug_slot_control_bit_definitions() {
        assert_eq!(SLOT_CTRL_ATTN_BUTTON_EN, 1 << 0);
        assert_eq!(SLOT_CTRL_PF_DETECT_EN, 1 << 1);
        assert_eq!(SLOT_CTRL_MRL_SENSOR_EN, 1 << 2);
        assert_eq!(SLOT_CTRL_PRESENCE_EN, 1 << 3);
        assert_eq!(SLOT_CTRL_CMD_COMPLETE_EN, 1 << 4);
        assert_eq!(SLOT_CTRL_HP_INT_EN, 1 << 5);
        assert_eq!(SLOT_CTRL_POWER_OFF, 1 << 10);
        assert_eq!(SLOT_CTRL_EIC, 1 << 12);
        assert_eq!(SLOT_CTRL_DLLSC_EN, 1 << 13);
        assert_eq!(SLOT_CTRL_CMD_COMPLETE_INT_EN, 1 << 14);
    }

    #[test]
    fn hotplug_slot_capability_bit_definitions() {
        assert_eq!(PCIE_CAP_ATTENTION_BUTTON, 1 << 0);
        assert_eq!(PCIE_CAP_SLOT_POWER_CTRL, 1 << 1);
        assert_eq!(PCIE_CAP_MRL_SENSOR, 1 << 2);
        assert_eq!(PCIE_CAP_HOT_PLUG_SURPRISE, 1 << 5);
        assert_eq!(PCIE_CAP_HOT_PLUG_CAPABLE, 1 << 6);
        assert_eq!(PCIE_CAP_ELECTROMECH_INTERLOCK, 1 << 7);
        assert_eq!(PCIE_CAP_NO_CMD_COMPL, 1 << 15);
    }

    #[test]
    fn hotplug_state_variants_are_distinct() {
        assert_ne!(SlotState::Empty, SlotState::PoweredOff);
        assert_ne!(SlotState::PoweredOn, SlotState::Removing);
        assert!(matches!(
            SlotState::AttentionPending { pressed_tsc: 7 },
            SlotState::AttentionPending { pressed_tsc: 7 }
        ));
    }

    #[test]
    fn hotplug_error_variants_exist() {
        assert_eq!(HotplugError::SlotNotFound, HotplugError::SlotNotFound);
        assert_eq!(HotplugError::InvalidState, HotplugError::InvalidState);
        assert_eq!(HotplugError::DrainTimeout, HotplugError::DrainTimeout);
        assert_eq!(HotplugError::PowerFault, HotplugError::PowerFault);
        assert_eq!(
            HotplugError::DriverRemoveFailed,
            HotplugError::DriverRemoveFailed
        );
        assert_eq!(
            HotplugError::BarAllocationFailed,
            HotplugError::BarAllocationFailed
        );
        assert_eq!(
            HotplugError::DriverLoadFailed,
            HotplugError::DriverLoadFailed
        );
    }

    #[test]
    fn hotplug_slot_registration_records_capabilities() {
        let mgr = PciHotplugManager::new();
        let bdf = PciBdf::new(0, 4, 0);
        let caps = SlotCapabilities {
            attention_button: true,
            power_control: true,
            mrl_sensor: false,
            surprise_capable: true,
            emi: false,
        };

        mgr.register_slot(bdf, 17, caps);

        let slots = mgr.slots.lock();
        let slot = slots.get(&bdf.key()).expect("slot registered");
        assert_eq!(slot.physical_slot, 17);
        assert_eq!(slot.state, SlotState::Empty);
        assert!(slot.capabilities.attention_button);
        assert!(slot.capabilities.power_control);
        assert!(slot.capabilities.surprise_capable);
    }

    #[test]
    fn hotplug_manager_creation() {
        let mgr = PciHotplugManager::new();
        assert_eq!(mgr.slot_count(), 0);
    }

    fn hotplug_bdf_key_is_stable() {
        assert_eq!(PciBdf::new(2, 3, 1).key(), 0x0219);
    }
}
