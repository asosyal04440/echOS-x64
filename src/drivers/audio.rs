//! # echOS Intel HD Audio (HDA) Sürücüsü
//!
//! Intel High Definition Audio Specification implementasyonu.
//! CORB/RIRB üzerinden codec keşfi, widget enum, DMA playback.
//!
//! ## Referanslar
//! - Intel HDA Specification §3 (controller registers), §4 (codec discovery)
//! - Linux snd_hda_intel: hdac_controller.c, hda_codec.c
//! - OSDev Intel HD Audio wiki

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

// ============================================================================
// PCI Identification
// ============================================================================

const PCI_CLASS_MULTIMEDIA: u8 = 0x04;
const PCI_SUBCLASS_HDA: u8 = 0x03;

// ============================================================================
// Controller Register Offsets (Intel HDA Spec §3)
// ============================================================================

const HDA_GCAP: u64 = 0x00;
const HDA_GCTL: u64 = 0x08;
const HDA_STATESTS: u64 = 0x0E;
const HDA_INTCTL: u64 = 0x20;
const HDA_INTSTS: u64 = 0x24;

// CORB registers (§3.3.18-24)
const HDA_CORBLBASE: u64 = 0x40;
const HDA_CORBUBASE: u64 = 0x44;
const HDA_CORBWP: u64 = 0x48;
const HDA_CORBRP: u64 = 0x4A;
const HDA_CORBCTL: u64 = 0x4C;
const HDA_CORBSTS: u64 = 0x4D;
const HDA_CORBSIZE: u64 = 0x4E;

// RIRB registers (§3.3.25-31)
const HDA_RIRBLBASE: u64 = 0x50;
const HDA_RIRBUBASE: u64 = 0x54;
const HDA_RIRBWP: u64 = 0x58;
const HDA_RINTCNT: u64 = 0x5A;
const HDA_RIRBCTL: u64 = 0x5C;
const HDA_RIRBSTS: u64 = 0x5D;
const HDA_RIRBSIZE: u64 = 0x5E;

// Immediate Command Interface (§3.3.32-34)
const HDA_ICOI: u64 = 0x60;
const HDA_ICII: u64 = 0x64;
const HDA_ICIS: u64 = 0x68;

// DMA Position Buffer (§3.3.35-36)
const HDA_DPIBASE: u64 = 0x74;

// Stream Descriptor base and interval (§4.5)
const HDA_STREAM_BASE: u64 = 0x80;
const HDA_STREAM_INTERVAL: u64 = 0x20;

// Stream Descriptor sub-offsets
const HDA_SD_CTL: u64 = 0x00;
const HDA_SD_STS: u64 = 0x03;
const HDA_SD_LPIB: u64 = 0x04;
const HDA_SD_CBL: u64 = 0x08;
const HDA_SD_LVI: u64 = 0x0C;
const HDA_SD_FIFOS: u64 = 0x10;
const HDA_SD_FMT: u64 = 0x12;
const HDA_SD_BDPL: u64 = 0x18;
const HDA_SD_BDPU: u64 = 0x1C;

// ============================================================================
// Register Bit Definitions
// ============================================================================

// GCTL
const GCTL_CRST: u32 = 1 << 0;
const GCTL_UNSOL: u32 = 1 << 8;

// CORBCTL
const CORBCTL_RUN: u8 = 1 << 1;
const CORBCTL_MEIE: u8 = 1 << 0;

// CORBRP
const CORBRP_RST: u16 = 1 << 15;

// RIRBCTL (Intel HDA Spec §3.3.22)
// bit 0 = RIRBOIC (Response Overrun Interrupt Control)
// bit 1 = RIRB_DMA_EN (DMA enable)
// bit 2 = RIRB_INT_CTL (Response Interrupt Control — generate IRQ per response)
const RIRBCTL_DMA_EN: u8 = 1 << 1;
const RIRBCTL_OVERRUN_IE: u8 = 1 << 0;
const RIRBCTL_INT_CTL: u8 = 1 << 2;

// RIRBWP
const RIRBWP_RST: u16 = 1 << 15;

// RIRBSTS
const RIRBSTS_RIRBOIS: u8 = 1 << 2;

// CORBSIZE / RIRBSIZE values (spec: 0x02 = 256 entries)
const RB_SIZE_256: u8 = 0x02;
const RB_SIZE_16: u8 = 0x01;
const RB_SIZE_2: u8 = 0x00;

// ICIS (Immediate Command Status)
const ICIS_BUSY: u16 = 1 << 0;
const ICIS_VALID: u16 = 1 << 1;

// SD CTL bits (Intel HDA Spec §3.4)
const SD_CTL_RUN: u8 = 1 << 1;
const SD_CTL_SRST: u8 = 1 << 0;
const SD_CTL_IOCE: u8 = 1 << 2; // Interrupt on Completion Enable
const SD_CTL_FIFO_ERROR_IE: u8 = 1 << 3; // FIFO Error Interrupt Enable
const SD_CTL_DESC_ERROR_IE: u8 = 1 << 4; // Descriptor Error Interrupt Enable

// SD FMT encoding (Intel HDA Stream Descriptor Format register)
const FMT_RATE_48K: u16 = 0x0000;
const FMT_RATE_44_1K: u16 = 0x4000;
const FMT_RATE_96K: u16 = 0x0800;
const FMT_RATE_192K: u16 = 0x1800;
const FMT_BITS_8: u16 = 0x00;
const FMT_BITS_16: u16 = 0x10;
const FMT_BITS_20: u16 = 0x20;
const FMT_BITS_24: u16 = 0x30;
const FMT_BITS_32: u16 = 0x40;

// BDL flags
const BDL_IOC: u32 = 1 << 0;

// ============================================================================
// HDA Codec Verbs (Intel HDA Spec §7.3, Linux hda_verbs.h)
// ============================================================================

const VERB_GET_PARAMETER: u16 = 0x0F00;
const VERB_GET_CONNECT_LIST: u16 = 0x0F02;
const VERB_GET_PIN_SENSE: u16 = 0x0F09;
const VERB_GET_CONFIG_DEFAULT: u16 = 0x0F1C;
const VERB_GET_SUBSYSTEM_ID: u16 = 0x0F20;
const VERB_GET_AMP_GAIN_MUTE: u16 = 0x0B00;
const VERB_SET_POWER_STATE: u16 = 0x0705;
const VERB_SET_PIN_WIDGET_CONTROL: u16 = 0x0707;
const VERB_SET_AMP_GAIN_MUTE: u16 = 0x0300;
const VERB_SET_CONVERTER_FORMAT: u16 = 0x0200;
const VERB_SET_STREAM_CHANNEL: u16 = 0x0706;
const VERB_SET_EAPD_BTLENABLE: u16 = 0x070C;
const VERB_SET_CHANNEL_COUNT: u16 = 0x072D;

// Parameter IDs
const PAR_VENDOR_ID: u8 = 0x00;
const PAR_REVISION_ID: u8 = 0x02;
const PAR_NODE_COUNT: u8 = 0x04;
const PAR_FUNCTION_TYPE: u8 = 0x05;
const PAR_AUDIO_FG_CAP: u8 = 0x08;
const PAR_AUDIO_WIDGET_CAP: u8 = 0x09;
const PAR_CONN_LIST_LEN: u8 = 0x0E;
const PAR_SUBSYSTEM_ID: u8 = 0x20;

// Node IDs
const NID_ROOT: u8 = 0x00;

// Widget types (AC_WID_*)
const WID_AUD_OUT: u8 = 0x0;
const WID_AUD_IN: u8 = 0x1;
const WID_AUD_MIX: u8 = 0x3;
const WID_PIN: u8 = 0x4;
const WID_POWER: u8 = 0x7;
const WID_VOL_KNB: u8 = 0x8;
const WID_BEEP: u8 = 0x9;

// Widget capability bits
const WCAP_STEREO: u32 = 1 << 0;
const WCAP_IN_AMP: u32 = 1 << 1;
const WCAP_OUT_AMP: u32 = 1 << 2;
const WCAP_CONN_LIST: u32 = 1 << 8;
const WCAP_TYPE_SHIFT: u32 = 20;

// Pin widget control bits
const PIN_CTL_OUT_EN: u8 = 1 << 0;
const PIN_CTL_IN_EN: u8 = 1 << 5;
const PIN_CTL_HP_EN: u8 = 1 << 6;

// RIRB entry valid bit (bit 63)
const RIRB_VALID: u64 = 1 << 63;
const RIRB_UNSOL: u64 = 1 << 36;

// ============================================================================
// HDA Controller
// ============================================================================

#[derive(Clone, Debug)]
pub struct HdaController {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub mmio_base: u64,
    pub mmio_size: u64,
    pub vendor_id: u16,
    pub device_id: u16,
    pub output_streams: u8,
    pub input_streams: u8,
    pub bidir_streams: u8,
    pub addr64: bool,
    pub corb_size_cap: u16,
    pub rirb_size_cap: u16,
    pub codecs: Vec<HdaCodec>,
    corb_phys: u64,
    corb_virt: usize,
    rirb_phys: u64,
    rirb_virt: usize,
    corb_wp: u16,
    rirb_rp: u16,
    pending_cmds: [u8; 16],
    last_responses: [u32; 16],
}

impl HdaController {
    pub fn new(bus: u8, device: u8, function: u8) -> Self {
        HdaController {
            bus,
            device,
            function,
            mmio_base: 0,
            mmio_size: 0,
            vendor_id: 0,
            device_id: 0,
            output_streams: 0,
            input_streams: 0,
            bidir_streams: 0,
            addr64: false,
            corb_size_cap: 256,
            rirb_size_cap: 256,
            codecs: Vec::new(),
            corb_phys: 0,
            corb_virt: 0,
            rirb_phys: 0,
            rirb_virt: 0,
            corb_wp: 0,
            rirb_rp: 0,
            pending_cmds: [0; 16],
            last_responses: [0; 16],
        }
    }

    pub fn init(&mut self) -> Result<(), AudioError> {
        self.reset()?;
        self.read_capabilities();
        self.allocate_corb_rirb()?;
        self.init_corb_rirb()?;
        self.detect_codecs()?;
        crate::serial_println!(
            "[HDA] Controller initialized: {} out, {} in, {} bidir, {} codecs",
            self.output_streams,
            self.input_streams,
            self.bidir_streams,
            self.codecs.len()
        );
        Ok(())
    }

    // --- Controller Reset (Intel HDA Spec §3.3.3, Linux: snd_hdac_bus_init_chip) ---

    fn reset(&mut self) -> Result<(), AudioError> {
        unsafe {
            let gctl = (self.mmio_base + HDA_GCTL) as *mut u32;

            // Assert reset: CRST = 0
            let v = read_volatile(gctl);
            write_volatile(gctl, v & !GCTL_CRST);

            // Wait for CRST to clear
            let mut t = 100_000;
            while (read_volatile(gctl) & GCTL_CRST) != 0 && t > 0 {
                core::hint::spin_loop();
                t -= 1;
            }
            if t == 0 {
                crate::serial_println!("[HDA] Reset assert timeout");
                return Err(AudioError::Timeout);
            }

            // Deassert reset: CRST = 1
            write_volatile(gctl, v | GCTL_CRST);

            // Wait for CRST to set
            t = 100_000;
            while (read_volatile(gctl) & GCTL_CRST) == 0 && t > 0 {
                core::hint::spin_loop();
                t -= 1;
            }
            if t == 0 {
                crate::serial_println!("[HDA] Reset deassert timeout");
                return Err(AudioError::Timeout);
            }

            // Wait for codecs to initialize (spec: >= 540us)
            for _ in 0..10_000 {
                core::hint::spin_loop();
            }

            // Enable unsolicited responses
            write_volatile(gctl, read_volatile(gctl) | GCTL_UNSOL);
        }
        Ok(())
    }

    // --- Capability Reading (Intel HDA Spec §3.3.1) ---

    fn read_capabilities(&mut self) {
        unsafe {
            let gcap = read_volatile((self.mmio_base + HDA_GCAP) as *const u16);
            self.output_streams = ((gcap >> 12) & 0xF) as u8;
            self.input_streams = ((gcap >> 8) & 0xF) as u8;
            self.bidir_streams = ((gcap >> 3) & 0x1F) as u8;
            self.addr64 = (gcap & 1) != 0;

            // CORB size capability
            let cs = read_volatile((self.mmio_base + HDA_CORBSIZE) as *const u8);
            self.corb_size_cap = if (cs & 0x40) != 0 {
                256
            } else if (cs & 0x20) != 0 {
                16
            } else {
                2
            };

            // RIRB size capability
            let rs = read_volatile((self.mmio_base + HDA_RIRBSIZE) as *const u8);
            self.rirb_size_cap = if (rs & 0x40) != 0 {
                256
            } else if (rs & 0x20) != 0 {
                16
            } else {
                2
            };
        }
    }

    // --- CORB/RIRB DMA Buffer Allocation ---

    fn allocate_corb_rirb(&mut self) -> Result<(), AudioError> {
        // CORB: 256 entries × 4 bytes = 1024 bytes (1 page)
        // RIRB: 256 entries × 8 bytes = 2048 bytes (1 page)
        // Total: 3072 bytes → allocate 2 pages for safety

        #[cfg(not(any(test, target_os = "windows")))]
        {
            use crate::memory::{dma_alloc, PAGE_SIZE};

            let pages = 2;
            let (phys, virt) = dma_alloc(pages).ok_or(AudioError::BufferError)?;

            self.corb_phys = phys as u64;
            self.corb_virt = virt.as_ptr() as usize;
            self.rirb_phys = (phys + PAGE_SIZE) as u64;
            self.rirb_virt = unsafe { virt.as_ptr().add(PAGE_SIZE) } as usize;
        }

        #[cfg(any(test, target_os = "windows"))]
        {
            // Host/test: use heap-allocated buffers
            let corb_box = alloc::boxed::Box::new([0u32; 256]);
            let rirb_box = alloc::boxed::Box::new([0u64; 256]);
            self.corb_phys = alloc::boxed::Box::into_raw(corb_box) as u64;
            self.corb_virt = self.corb_phys as usize;
            self.rirb_phys = alloc::boxed::Box::into_raw(rirb_box) as u64;
            self.rirb_virt = self.rirb_phys as usize;
        }

        Ok(())
    }

    // --- CORB/RIRB Initialization (Linux: snd_hdac_bus_init_cmd_io) ---

    fn init_corb_rirb(&mut self) -> Result<(), AudioError> {
        unsafe {
            // === CORB Setup ===

            // Stop CORB DMA
            write_volatile((self.mmio_base + HDA_CORBCTL) as *mut u8, 0);

            // Set CORB base address
            write_volatile(
                (self.mmio_base + HDA_CORBLBASE) as *mut u32,
                (self.corb_phys & 0xFFFFFFFF) as u32,
            );
            write_volatile(
                (self.mmio_base + HDA_CORBUBASE) as *mut u32,
                (self.corb_phys >> 32) as u32,
            );

            // Set CORB size to max (256 entries = 0x02)
            write_volatile((self.mmio_base + HDA_CORBSIZE) as *mut u8, RB_SIZE_256);

            // Reset CORB read pointer
            write_volatile((self.mmio_base + HDA_CORBRP) as *mut u16, CORBRP_RST);

            // Wait for reset to complete (some controllers self-clear)
            let mut t = 10_000;
            loop {
                let rp = read_volatile((self.mmio_base + HDA_CORBRP) as *const u16);
                if (rp & CORBRP_RST) == 0 {
                    break;
                }
                t -= 1;
                if t == 0 {
                    break;
                }
                core::hint::spin_loop();
            }

            // Set write pointer to 0
            write_volatile((self.mmio_base + HDA_CORBWP) as *mut u16, 0);
            self.corb_wp = 0;

            // Start CORB DMA
            write_volatile((self.mmio_base + HDA_CORBCTL) as *mut u8, CORBCTL_RUN);

            // === RIRB Setup ===

            // Stop RIRB DMA
            write_volatile((self.mmio_base + HDA_RIRBCTL) as *mut u8, 0);

            // Set RIRB base address
            write_volatile(
                (self.mmio_base + HDA_RIRBLBASE) as *mut u32,
                (self.rirb_phys & 0xFFFFFFFF) as u32,
            );
            write_volatile(
                (self.mmio_base + HDA_RIRBUBASE) as *mut u32,
                (self.rirb_phys >> 32) as u32,
            );

            // Set RIRB size to max (256 entries = 0x02)
            write_volatile((self.mmio_base + HDA_RIRBSIZE) as *mut u8, RB_SIZE_256);

            // Reset RIRB write pointer
            write_volatile((self.mmio_base + HDA_RIRBWP) as *mut u16, RIRBWP_RST);

            // Set response interrupt count to 1 (interrupt per response)
            write_volatile((self.mmio_base + HDA_RINTCNT) as *mut u16, 1);

            // Start RIRB DMA + Response Interrupt Control (bit 2) + Overrun IE (bit 0)
            // Per Intel HDA spec §3.3.22: bit 2 enables per-response interrupt generation
            write_volatile(
                (self.mmio_base + HDA_RIRBCTL) as *mut u8,
                RIRBCTL_DMA_EN | RIRBCTL_INT_CTL | RIRBCTL_OVERRUN_IE,
            );

            self.rirb_rp = 0;
        }

        Ok(())
    }

    // --- Codec Discovery (Intel HDA Spec §4.3, Linux: probe_codec) ---

    fn detect_codecs(&mut self) -> Result<(), AudioError> {
        unsafe {
            // Read STATESTS to find which codec addresses are present
            let statests = read_volatile((self.mmio_base + HDA_STATESTS) as *const u16);

            // Clear STATESTS (write-1-to-clear)
            write_volatile((self.mmio_base + HDA_STATESTS) as *mut u16, 0x7FFF);

            for addr in 0..15u8 {
                if (statests & (1 << addr)) != 0 {
                    // Probe codec by reading vendor ID from root node (NID 0)
                    let verb = encode_verb(addr, NID_ROOT, VERB_GET_PARAMETER, PAR_VENDOR_ID);
                    match self.send_command_raw(verb) {
                        Ok(response) => {
                            if response != 0xFFFFFFFF && response != 0 {
                                let vendor_id = (response >> 16) as u16;
                                let device_id = (response & 0xFFFF) as u16;

                                crate::serial_println!(
                                    "[HDA] Codec at addr {}: vendor={:04x} device={:04x}",
                                    addr,
                                    vendor_id,
                                    device_id
                                );

                                let mut codec = HdaCodec::new(addr, vendor_id, device_id);
                                codec.scan_widgets(self)?;
                                self.codecs.push(codec);
                            }
                        }
                        Err(e) => {
                            crate::serial_println!(
                                "[HDA] Codec probe failed at addr {}: {:?}",
                                addr,
                                e
                            );
                        }
                    }
                }
            }
        }

        if self.codecs.is_empty() {
            crate::serial_println!("[HDA] No codecs detected");
            Err(AudioError::NoCodec)
        } else {
            Ok(())
        }
    }

    // --- CORB Command Send (Linux: snd_hdac_bus_send_cmd) ---

    fn send_command_raw(&mut self, verb: u32) -> Result<u32, AudioError> {
        let codec_addr = ((verb >> 28) & 0xF) as u8;

        unsafe {
            // Read current write pointer
            let wp = read_volatile((self.mmio_base + HDA_CORBWP) as *const u16);
            if wp == 0xFFFF {
                return Err(AudioError::ControllerError);
            }

            let next_wp = ((wp as u16 + 1) % self.corb_size_cap) as u16;

            // Check if CORB is full
            let rp = read_volatile((self.mmio_base + HDA_CORBRP) as *const u16);
            if next_wp == (rp & 0x7FFF) {
                return Err(AudioError::BufferError);
            }

            // Write command to CORB buffer
            self.pending_cmds[codec_addr as usize] += 1;
            *(self.corb_virt as *mut u32).add(wp as usize) = verb;

            // Update write pointer
            write_volatile((self.mmio_base + HDA_CORBWP) as *mut u16, next_wp);
            self.corb_wp = next_wp;

            // Wait for response with timeout
            let mut timeout = 100_000;
            while self.pending_cmds[codec_addr as usize] > 0 && timeout > 0 {
                self.poll_rirb();
                if self.pending_cmds[codec_addr as usize] == 0 {
                    return Ok(self.last_responses[codec_addr as usize]);
                }
                core::hint::spin_loop();
                timeout -= 1;
            }

            if timeout == 0 {
                crate::serial_println!("[HDA] Command timeout for codec {}", codec_addr);
                self.pending_cmds[codec_addr as usize] = 0;
                return Err(AudioError::Timeout);
            }

            Ok(self.last_responses[codec_addr as usize])
        }
    }

    // --- RIRB Response Polling (Linux: snd_hdac_bus_update_rirb) ---

    fn poll_rirb(&mut self) {
        unsafe {
            let wp = read_volatile((self.mmio_base + HDA_RIRBWP) as *const u16);
            if wp == 0xFFFF {
                return;
            }

            let wp_val = (wp & 0x7FFF) as u16;

            while self.rirb_rp != wp_val {
                self.rirb_rp = (self.rirb_rp + 1) % self.rirb_size_cap;

                // Read RIRB entry (8 bytes: response + metadata)
                let entry = *(self.rirb_virt as *const u64).add(self.rirb_rp as usize);

                // Check valid bit
                if (entry & RIRB_VALID) == 0 {
                    continue;
                }

                // Check if unsolicited response (skip for command responses)
                if (entry & RIRB_UNSOL) != 0 {
                    continue;
                }

                // Extract response (lower 32 bits) and codec address
                let response = (entry & 0xFFFFFFFF) as u32;
                let sdi = ((entry >> 32) & 0xF) as u8;

                if sdi < 16 && self.pending_cmds[sdi as usize] > 0 {
                    self.pending_cmds[sdi as usize] -= 1;
                    self.last_responses[sdi as usize] = response;
                }
            }

            // Clear overrun status if set
            let sts = read_volatile((self.mmio_base + HDA_RIRBSTS) as *const u8);
            if (sts & RIRBSTS_RIRBOIS) != 0 {
                write_volatile((self.mmio_base + HDA_RIRBSTS) as *mut u8, RIRBSTS_RIRBOIS);
            }
        }
    }

    // --- Public Command Interface ---

    pub fn send_command(
        &mut self,
        codec_addr: u8,
        nid: u8,
        verb: u16,
        param: u8,
    ) -> Result<u32, AudioError> {
        let encoded = encode_verb(codec_addr, nid, verb, param);
        self.send_command_raw(encoded)
    }

    pub fn send_verb(&mut self, codec_addr: u8, nid: u8, verb: u32) -> Result<u32, AudioError> {
        self.send_command_raw(verb)
    }

    pub fn get_playback_stream(&self) -> Option<u8> {
        if self.output_streams > 0 {
            Some(0)
        } else {
            None
        }
    }
}

// ============================================================================
// Verb Encoding (Intel HDA Spec §3.7)
// ============================================================================

/// Encode a HDA verb into a 32-bit CORB entry.
/// Format: [31:28] LinkID | [27] IndirectNID | [26:20] NID | [19:8] Verb | [7:0] Parameter
fn encode_verb(codec_addr: u8, nid: u8, verb: u16, param: u8) -> u32 {
    ((codec_addr as u32) << 28) | ((nid as u32) << 20) | ((verb as u32) << 8) | (param as u32)
}

// ============================================================================
// HDA Codec
// ============================================================================

#[derive(Clone, Debug)]
pub struct HdaCodec {
    pub address: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub revision_id: u8,
    pub subsystem_id: u32,
    pub widgets: Vec<AudioWidget>,
    pub root_nid: u8,
    pub audio_func_group: u8,
    pub afg_start_nid: u8,
    pub afg_node_count: u8,
}

impl HdaCodec {
    pub fn new(address: u8, vendor_id: u16, device_id: u16) -> Self {
        HdaCodec {
            address,
            vendor_id,
            device_id,
            revision_id: 0,
            subsystem_id: 0,
            widgets: Vec::new(),
            root_nid: 0,
            audio_func_group: 0,
            afg_start_nid: 0,
            afg_node_count: 0,
        }
    }

    /// Scan widgets by querying the codec via CORB/RIRB.
    /// Follows Intel HDA Spec §4.3 codec discovery flow.
    pub fn scan_widgets(&mut self, ctrl: &mut HdaController) -> Result<(), AudioError> {
        // Step 1: Get revision ID from root node
        let rev = ctrl.send_command(self.address, NID_ROOT, VERB_GET_PARAMETER, PAR_REVISION_ID)?;
        self.revision_id = (rev & 0xFF) as u8;

        // Step 2: Get subsystem ID
        let subsys = ctrl.send_command(self.address, NID_ROOT, VERB_GET_SUBSYSTEM_ID, 0)?;
        self.subsystem_id = subsys;

        // Step 3: Get node count from root node
        let node_info =
            ctrl.send_command(self.address, NID_ROOT, VERB_GET_PARAMETER, PAR_NODE_COUNT)?;
        let start_nid = (node_info >> 16 & 0x7F) as u8;
        let node_count = (node_info & 0x7F) as u8;

        self.afg_start_nid = start_nid;
        self.afg_node_count = node_count;

        crate::serial_println!(
            "[HDA] Codec {}: nodes {}-{} (count={})",
            self.address,
            start_nid,
            start_nid + node_count - 1,
            node_count
        );

        // Step 4: Find the Audio Function Group (AFG)
        // First check if start_nid itself is the AFG
        let fg_type = ctrl.send_command(
            self.address,
            start_nid,
            VERB_GET_PARAMETER,
            PAR_FUNCTION_TYPE,
        )?;
        let fg_type_val = (fg_type & 0xFF) as u8;

        if fg_type_val == 1 {
            // It's an AFG
            self.audio_func_group = start_nid;
            self.root_nid = start_nid;
        } else {
            // Search for AFG among nodes
            for nid in start_nid..start_nid + node_count {
                let ft =
                    ctrl.send_command(self.address, nid, VERB_GET_PARAMETER, PAR_FUNCTION_TYPE)?;
                if (ft & 0xFF) == 1 {
                    self.audio_func_group = nid;
                    self.root_nid = nid;
                    break;
                }
            }
        }

        // Step 5: Enumerate all widgets in the AFG
        for nid in self.afg_start_nid..self.afg_start_nid + self.afg_node_count {
            let wcaps =
                ctrl.send_command(self.address, nid, VERB_GET_PARAMETER, PAR_AUDIO_WIDGET_CAP)?;
            let widget_type = ((wcaps >> WCAP_TYPE_SHIFT) & 0xF) as u8;

            let name = match widget_type {
                WID_AUD_OUT => format!("DAC{}", nid),
                WID_AUD_IN => format!("ADC{}", nid),
                WID_AUD_MIX => format!("Mixer{}", nid),
                WID_PIN => format!("Pin{}", nid),
                WID_POWER => format!("Pwr{}", nid),
                WID_VOL_KNB => format!("VolKnb{}", nid),
                WID_BEEP => format!("Beep{}", nid),
                _ => format!("Vendor{}", nid),
            };

            let mut caps = WidgetCaps(0);
            if (wcaps & WCAP_STEREO) != 0 {
                caps.insert(WidgetCaps::STEREO);
            }
            if (wcaps & WCAP_IN_AMP) != 0 {
                caps.insert(WidgetCaps::INPUT_AMP);
            }
            if (wcaps & WCAP_OUT_AMP) != 0 {
                caps.insert(WidgetCaps::OUTPUT_AMP);
            }

            self.widgets.push(AudioWidget {
                nid,
                widget_type: HdaWidgetType::from_u8(widget_type),
                name,
                capabilities: caps,
                default_gain: 0,
                muted: false,
            });
        }

        crate::serial_println!(
            "[HDA] Codec {}: {} widgets enumerated",
            self.address,
            self.widgets.len()
        );

        Ok(())
    }

    pub fn find_widget(&self, nid: u8) -> Option<&AudioWidget> {
        self.widgets.iter().find(|w| w.nid == nid)
    }

    pub fn find_widget_by_type(&self, t: HdaWidgetType) -> Option<&AudioWidget> {
        self.widgets.iter().find(|w| w.widget_type == t)
    }

    pub fn find_output_dac(&self) -> Option<&AudioWidget> {
        self.find_widget_by_type(HdaWidgetType::OutputDac)
    }

    pub fn find_input_adc(&self) -> Option<&AudioWidget> {
        self.find_widget_by_type(HdaWidgetType::InputAdc)
    }
}

// ============================================================================
// Widget Types and Capabilities
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HdaWidgetType {
    OutputDac,
    InputAdc,
    Mixer,
    Selector,
    Pin,
    Power,
    VolumeKnob,
    Beep,
    Unknown,
}

impl HdaWidgetType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0x0 => Self::OutputDac,
            0x1 => Self::InputAdc,
            0x3 => Self::Mixer,
            0x4 => Self::Pin,
            0x7 => Self::Power,
            0x8 => Self::VolumeKnob,
            0x9 => Self::Beep,
            _ => Self::Unknown,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WidgetCaps(pub u32);

impl WidgetCaps {
    pub const STEREO: Self = Self(1 << 0);
    pub const INPUT_AMP: Self = Self(1 << 1);
    pub const OUTPUT_AMP: Self = Self(1 << 2);
    pub const PIN_SENSE: Self = Self(1 << 13);

    pub fn contains(&self, o: Self) -> bool {
        (self.0 & o.0) != 0
    }

    pub fn insert(&mut self, o: Self) {
        self.0 |= o.0;
    }
}

#[derive(Clone, Debug)]
pub struct AudioWidget {
    pub nid: u8,
    pub widget_type: HdaWidgetType,
    pub name: String,
    pub capabilities: WidgetCaps,
    pub default_gain: i16,
    pub muted: bool,
}

impl AudioWidget {
    pub fn set_volume(&mut self, v: u8) {
        self.default_gain = (v as i16) - 100;
    }

    pub fn set_mute(&mut self, m: bool) {
        self.muted = m;
    }
}

// ============================================================================
// Audio Format
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub bits_per_sample: u8,
    pub channels: u8,
}

impl AudioFormat {
    pub fn new(sr: u32, bps: u8, ch: u8) -> Self {
        Self {
            sample_rate: sr,
            bits_per_sample: bps,
            channels: ch,
        }
    }

    pub fn cd_quality() -> Self {
        Self::new(44100, 16, 2)
    }

    pub fn dvd_quality() -> Self {
        Self::new(48000, 16, 2)
    }

    pub fn high_quality() -> Self {
        Self::new(96000, 24, 2)
    }

    /// Convert to HDA Stream Format register value (Intel HDA Spec Table 53)
    pub fn to_hda_format(&self) -> u16 {
        let base = match self.sample_rate {
            44100 => FMT_RATE_44_1K,
            48000 => FMT_RATE_48K,
            96000 => FMT_RATE_96K,
            192000 => FMT_RATE_192K,
            _ => FMT_RATE_48K,
        };
        let bits = match self.bits_per_sample {
            8 => FMT_BITS_8,
            16 => FMT_BITS_16,
            20 => FMT_BITS_20,
            24 => FMT_BITS_24,
            32 => FMT_BITS_32,
            _ => FMT_BITS_16,
        };
        let channels = self.channels.saturating_sub(1).min(15) as u16;
        base | bits | channels
    }
}

// ============================================================================
// Stream Direction
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamDirection {
    Playback,
    Capture,
}

// ============================================================================
// Audio Stream
// ============================================================================

#[derive(Clone, Debug)]
pub struct AudioStream {
    pub stream_id: u8,
    pub direction: StreamDirection,
    pub format: AudioFormat,
    pub buffer: Vec<u8>,
    pub buffer_size: usize,
    pub position: usize,
    pub playing: bool,
    pub loop_enabled: bool,
}

impl AudioStream {
    pub fn new(sid: u8, dir: StreamDirection, fmt: AudioFormat) -> Self {
        Self {
            stream_id: sid,
            direction: dir,
            format: fmt,
            buffer: Vec::new(),
            buffer_size: 0,
            position: 0,
            playing: false,
            loop_enabled: false,
        }
    }

    pub fn set_buffer(&mut self, d: Vec<u8>) {
        self.buffer_size = d.len();
        self.buffer = d;
        self.position = 0;
    }

    pub fn start(&mut self) {
        self.playing = true;
        self.position = 0;
    }

    pub fn stop(&mut self) {
        self.playing = false;
    }

    pub fn pause(&mut self) {
        self.playing = false;
    }

    pub fn resume(&mut self) {
        self.playing = true;
    }

    pub fn consume(&mut self, bytes: usize) -> bool {
        if !self.playing {
            return false;
        }
        self.position += bytes;
        if self.position >= self.buffer_size {
            if self.loop_enabled {
                self.position = 0;
            } else {
                self.playing = false;
                self.position = 0;
                return false;
            }
        }
        true
    }
}

// ============================================================================
// Buffer Descriptor List (BDL) Entry
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BufferDescriptorEntry {
    pub address_low: u32,
    pub address_high: u32,
    pub length: u32,
    pub flags: u32,
}

impl BufferDescriptorEntry {
    pub fn new(addr: u64, len: u32, ioc: bool) -> Self {
        Self {
            address_low: addr as u32,
            address_high: (addr >> 32) as u32,
            length: len,
            flags: if ioc { BDL_IOC } else { 0 },
        }
    }
}

// ============================================================================
// Audio Error
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioError {
    NoController,
    NoCodec,
    NoStream,
    BufferError,
    FormatNotSupported,
    CodecError,
    ControllerError,
    Timeout,
}

// ============================================================================
// Global State
// ============================================================================

static HDA_CONTROLLERS: Mutex<Vec<HdaController>> = Mutex::new(Vec::new());
static AUDIO_STREAMS: Mutex<BTreeMap<u8, AudioStream>> = Mutex::new(BTreeMap::new());
static NEXT_STREAM_ID: AtomicU32 = AtomicU32::new(1);
static AUDIO_INITIALIZED: AtomicBool = AtomicBool::new(false);

// ============================================================================
// Public API
// ============================================================================

pub fn init() {
    if AUDIO_INITIALIZED.swap(true, Ordering::SeqCst) {
        return;
    }
    crate::serial_println!("[AUDIO] HDA baslatiliyor...");
    let ctrls = discover_hda_controllers();
    let mut list = HDA_CONTROLLERS.lock();
    for mut c in ctrls {
        if c.init().is_ok() {
            list.push(c);
        }
    }
    crate::serial_println!("[AUDIO] Found {} HDA controllers", list.len());
}

pub fn discover_hda_controllers() -> Vec<HdaController> {
    let mut v = Vec::new();
    for d in crate::drivers::pci::scan() {
        if d.class_code == PCI_CLASS_MULTIMEDIA && d.subclass == PCI_SUBCLASS_HDA {
            v.push(HdaController::new(d.bus, d.device, d.function));
        }
    }
    v
}

pub fn default_controller() -> Option<HdaController> {
    HDA_CONTROLLERS.lock().first().cloned()
}

pub fn default_codec() -> Option<HdaCodec> {
    HDA_CONTROLLERS
        .lock()
        .first()
        .and_then(|c| c.codecs.first().cloned())
}

pub fn open_playback_stream(fmt: AudioFormat) -> Result<u8, AudioError> {
    let id = NEXT_STREAM_ID.fetch_add(1, Ordering::SeqCst) as u8;
    AUDIO_STREAMS
        .lock()
        .insert(id, AudioStream::new(id, StreamDirection::Playback, fmt));
    crate::serial_println!(
        "[AUDIO] Opened stream {} ({}Hz {}bit {}ch)",
        id,
        fmt.sample_rate,
        fmt.bits_per_sample,
        fmt.channels
    );
    Ok(id)
}

pub fn close_stream(id: u8) -> Result<(), AudioError> {
    if AUDIO_STREAMS.lock().remove(&id).is_some() {
        Ok(())
    } else {
        Err(AudioError::NoStream)
    }
}

pub fn write_stream(id: u8, data: &[u8]) -> Result<usize, AudioError> {
    let mut m = AUDIO_STREAMS.lock();
    let s = m.get_mut(&id).ok_or(AudioError::NoStream)?;
    s.set_buffer(data.to_vec());
    Ok(data.len())
}

pub fn start_stream(id: u8) -> Result<(), AudioError> {
    let mut m = AUDIO_STREAMS.lock();
    m.get_mut(&id).ok_or(AudioError::NoStream)?.start();
    Ok(())
}

pub fn stop_stream(id: u8) -> Result<(), AudioError> {
    let mut m = AUDIO_STREAMS.lock();
    m.get_mut(&id).ok_or(AudioError::NoStream)?.stop();
    Ok(())
}

pub fn set_volume(v: u8) -> Result<(), AudioError> {
    let mut c = HDA_CONTROLLERS.lock();
    let _ = c.first_mut().ok_or(AudioError::NoController)?;
    crate::serial_println!("[AUDIO] Volume {}%", v);
    Ok(())
}

pub fn set_mute(m: bool) -> Result<(), AudioError> {
    let mut c = HDA_CONTROLLERS.lock();
    let _ = c.first_mut().ok_or(AudioError::NoController)?;
    crate::serial_println!("[AUDIO] Mute: {}", m);
    Ok(())
}

pub fn get_stream_position(id: u8) -> Result<usize, AudioError> {
    let m = AUDIO_STREAMS.lock();
    Ok(m.get(&id).ok_or(AudioError::NoStream)?.position)
}

pub fn is_stream_playing(id: u8) -> Result<bool, AudioError> {
    let m = AUDIO_STREAMS.lock();
    Ok(m.get(&id).ok_or(AudioError::NoStream)?.playing)
}

#[derive(Clone, Copy, Debug)]
pub struct AudioCapabilities {
    pub max_channels: u8,
    pub max_sample_rate: u32,
    pub max_bits_per_sample: u8,
    pub output_streams: u8,
    pub input_streams: u8,
}

pub fn get_capabilities() -> Option<AudioCapabilities> {
    let cl = HDA_CONTROLLERS.lock();
    let c = cl.first()?;
    Some(AudioCapabilities {
        max_channels: 8,
        max_sample_rate: 192000,
        max_bits_per_sample: 32,
        output_streams: c.output_streams,
        input_streams: c.input_streams,
    })
}

// ============================================================================
// Stream Descriptor (Intel HDA Spec §4.5)
// ============================================================================

#[derive(Debug, Clone)]
pub struct StreamDescriptor {
    pub index: u8,
    pub direction: StreamDirection,
    pub format: AudioFormat,
    pub bdl_base: u64,
    pub bdl: Vec<BufferDescriptorEntry>,
    pub buffer_length: u32,
    pub last_valid_index: u8,
    pub running: bool,
}

impl StreamDescriptor {
    pub fn new(idx: u8, dir: StreamDirection) -> Self {
        Self {
            index: idx,
            direction: dir,
            format: AudioFormat::dvd_quality(),
            bdl_base: 0,
            bdl: Vec::new(),
            buffer_length: 0,
            last_valid_index: 0,
            running: false,
        }
    }

    /// Calculate HDA Stream Format register value
    pub fn calculate_format_value(&self) -> u16 {
        self.format.to_hda_format()
    }

    /// Program the BDL (Buffer Descriptor List)
    pub fn program_bdl(&mut self, bp: u64, bs: usize, fc: usize) {
        let fs = bs / fc;
        self.bdl.clear();
        for i in 0..fc {
            let is_last = i == fc - 1;
            // IOC on first and last entry for interrupt-driven completion
            let ioc = i == 0 || is_last;
            self.bdl.push(BufferDescriptorEntry::new(
                bp + (i * fs) as u64,
                fs as u32,
                ioc,
            ));
        }
        self.buffer_length = bs as u32;
        self.last_valid_index = (fc - 1) as u8;
    }

    /// Program stream descriptor MMIO registers (Intel HDA Spec §4.5.2)
    pub fn program_registers(&self, mmio: u64, stream_base: usize) {
        let sd = mmio + stream_base as u64;
        unsafe {
            // 1. Stop the stream (clear RUN bit)
            let ctl = read_volatile((sd + HDA_SD_CTL) as *const u8);
            write_volatile((sd + HDA_SD_CTL) as *mut u8, ctl & !SD_CTL_RUN);

            // 2. Reset the stream (set SRST), then wait for self-clear
            write_volatile((sd + HDA_SD_CTL) as *mut u8, SD_CTL_SRST);
            for _ in 0..1000 {
                let status = read_volatile((sd + HDA_SD_CTL) as *const u8);
                if (status & SD_CTL_SRST) == 0 {
                    break;
                }
                core::hint::spin_loop();
            }

            // 3. Program format register
            let fmt = self.calculate_format_value();
            write_volatile((sd + HDA_SD_FMT) as *mut u16, fmt);

            // 4. Program buffer length (CBL)
            write_volatile((sd + HDA_SD_CBL) as *mut u32, self.buffer_length);

            // 5. Program last valid index (LVI)
            write_volatile((sd + HDA_SD_LVI) as *mut u8, self.last_valid_index);

            // 6. Program BDL base address (BDPL/BDPU)
            write_volatile(
                (sd + HDA_SD_BDPL) as *mut u32,
                (self.bdl_base & 0xFFFFFFFF) as u32,
            );
            write_volatile((sd + HDA_SD_BDPU) as *mut u32, (self.bdl_base >> 32) as u32);

            // 7. Enable Interrupt on Completion (IOCE), stream stopped until explicitly started
            write_volatile((sd + HDA_SD_CTL) as *mut u8, SD_CTL_IOCE);
        }
    }

    /// Start the stream (set RUN bit)
    pub fn start(&mut self, mmio: u64, stream_base: usize) {
        let sd = mmio + stream_base as u64;
        unsafe {
            let ctl = read_volatile((sd + HDA_SD_CTL) as *const u8);
            write_volatile((sd + HDA_SD_CTL) as *mut u8, ctl | SD_CTL_RUN);
        }
        self.running = true;
    }

    /// Stop the stream (clear RUN bit)
    pub fn stop(&mut self, mmio: u64, stream_base: usize) {
        let sd = mmio + stream_base as u64;
        unsafe {
            let ctl = read_volatile((sd + HDA_SD_CTL) as *const u8);
            write_volatile((sd + HDA_SD_CTL) as *mut u8, ctl & !SD_CTL_RUN);
        }
        self.running = false;
    }

    /// Reset the stream
    pub fn reset(&mut self, mmio: u64, stream_base: usize) {
        let sd = mmio + stream_base as u64;
        unsafe {
            write_volatile((sd + HDA_SD_CTL) as *mut u8, SD_CTL_SRST);
            // Wait for reset to complete
            let mut t = 10_000;
            while t > 0 {
                let ctl = read_volatile((sd + HDA_SD_CTL) as *const u8);
                if (ctl & SD_CTL_SRST) == 0 {
                    break;
                }
                core::hint::spin_loop();
                t -= 1;
            }
            write_volatile((sd + HDA_SD_CTL) as *mut u8, 0);
        }
        self.running = false;
    }
}

// ============================================================================
// Audio Path (Pin → DAC routing)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinType {
    Speaker,
    Headphone,
    LineOut,
    LineIn,
    Mic,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct AudioPath {
    pub pin_nid: u8,
    pub dac_nid: u8,
    pub path: Vec<u8>,
    pub pin_config: u32,
    pub pin_type: PinType,
}

impl AudioPath {
    pub fn discover_paths(codec: &HdaCodec) -> Vec<Self> {
        let mut paths = Vec::new();
        for w in &codec.widgets {
            if w.widget_type == HdaWidgetType::Pin {
                if let Some(d) = codec.find_output_dac() {
                    paths.push(Self {
                        pin_nid: w.nid,
                        dac_nid: d.nid,
                        path: vec![w.nid, d.nid],
                        pin_config: 0,
                        pin_type: PinType::Speaker,
                    });
                }
            }
        }
        paths
    }

    /// Configure the audio path via codec verbs.
    /// Sets pin widget control, power state, and amplifier settings.
    pub fn configure(
        &self,
        ctrl: &mut HdaController,
        codec_addr: u8,
        fmt: u16,
    ) -> Result<(), AudioError> {
        // Set power state to D0 (full power) for the pin
        ctrl.send_command(codec_addr, self.pin_nid, VERB_SET_POWER_STATE, 0x00)?;

        // Set pin widget control: enable output
        ctrl.send_command(
            codec_addr,
            self.pin_nid,
            VERB_SET_PIN_WIDGET_CONTROL,
            PIN_CTL_OUT_EN,
        )?;

        // Set converter format for the DAC
        ctrl.send_command(
            codec_addr,
            self.dac_nid,
            VERB_SET_CONVERTER_FORMAT,
            (fmt & 0xFF) as u8,
        )?;

        // Set stream/channel for the DAC
        ctrl.send_command(codec_addr, self.dac_nid, VERB_SET_STREAM_CHANNEL, 0x00)?;

        // Set output amplifier gain (unmute, 0dB)
        // Verb format: 0x3XX where XX = gain/mute
        // 0x80 = unmute, 0x00 = 0dB offset
        ctrl.send_command(codec_addr, self.dac_nid, VERB_SET_AMP_GAIN_MUTE, 0x80)?;

        Ok(())
    }
}

// ============================================================================
// DMA Playback Engine
// ============================================================================

#[derive(Debug, Clone)]
pub struct DmaPlaybackEngine {
    pub stream: StreamDescriptor,
    pub paths: Vec<AudioPath>,
    pub buffer_phys: u64,
    pub buffer_virt: usize,
    pub buffer_size: usize,
    pub mmio_base: u64,
    pub stream_base: usize,
    pub initialized: bool,
}

impl DmaPlaybackEngine {
    pub fn new() -> Self {
        Self {
            stream: StreamDescriptor::new(0, StreamDirection::Playback),
            paths: Vec::new(),
            buffer_phys: 0,
            buffer_virt: 0,
            buffer_size: 0,
            mmio_base: 0,
            stream_base: 0x80,
            initialized: false,
        }
    }

    pub fn init(&mut self, ctrl: &mut HdaController) -> Result<(), AudioError> {
        if self.initialized {
            return Ok(());
        }

        // Discover audio paths from codecs
        for c in &ctrl.codecs {
            self.paths.extend(AudioPath::discover_paths(c));
        }

        if self.paths.is_empty() {
            return Err(AudioError::NoCodec);
        }

        // Configure the first audio path
        let fmt = self.stream.calculate_format_value();
        if let Some(p) = self.paths.first() {
            if let Some(c) = ctrl.codecs.first() {
                p.configure(ctrl, c.address, fmt)?;
            }
        }

        self.initialized = true;
        Ok(())
    }

    /// Allocate a DMA-capable buffer for audio playback.
    pub fn allocate_buffer(&mut self, sz: usize) -> Result<(), AudioError> {
        #[cfg(not(any(test, target_os = "windows")))]
        {
            use crate::memory::{dma_alloc, PAGE_SIZE};
            let pages = (sz + PAGE_SIZE - 1) / PAGE_SIZE;
            let (phys, virt) = dma_alloc(pages.max(1)).ok_or(AudioError::BufferError)?;
            self.buffer_phys = phys as u64;
            self.buffer_virt = virt.as_ptr() as usize;
            self.buffer_size = pages * PAGE_SIZE;
        }

        #[cfg(any(test, target_os = "windows"))]
        {
            let buf = alloc::boxed::Box::new([0u8; 65536]);
            self.buffer_phys = alloc::boxed::Box::into_raw(buf) as u64;
            self.buffer_virt = self.buffer_phys as usize;
            self.buffer_size = 65536.min(sz);
        }

        Ok(())
    }

    /// Write audio data to the DMA buffer.
    pub fn write_buffer(&mut self, data: &[u8]) -> Result<usize, AudioError> {
        if self.buffer_virt == 0 || self.buffer_size == 0 {
            return Err(AudioError::BufferError);
        }
        let copy_len = data.len().min(self.buffer_size);
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), self.buffer_virt as *mut u8, copy_len);
        }
        Ok(copy_len)
    }

    /// Start DMA playback.
    pub fn start_playback(&mut self) -> Result<(), AudioError> {
        if !self.initialized {
            return Err(AudioError::ControllerError);
        }

        // Program BDL with the allocated buffer
        self.stream
            .program_bdl(self.buffer_phys, self.buffer_size, 2);

        // Set BDL base on stream descriptor
        self.stream.bdl_base = self.buffer_phys;

        // Program stream descriptor registers
        self.stream
            .program_registers(self.mmio_base, self.stream_base);

        // Start the stream
        self.stream.start(self.mmio_base, self.stream_base);

        Ok(())
    }

    /// Stop DMA playback.
    pub fn stop_playback(&mut self) -> Result<(), AudioError> {
        self.stream.stop(self.mmio_base, self.stream_base);
        Ok(())
    }

    /// Handle buffer complete interrupt.
    pub fn handle_buffer_complete(&mut self) -> Result<(), AudioError> {
        // Read LPIB (Link Position in Buffer) to get current DMA position
        let sd = self.mmio_base + self.stream_base as u64;
        let _lpib = unsafe { read_volatile((sd + HDA_SD_LPIB) as *const u32) };
        // In a full implementation, this would trigger buffer refill
        Ok(())
    }
}

static DMA_ENGINE: Mutex<Option<DmaPlaybackEngine>> = Mutex::new(None);

pub fn init_dma_playback() -> Result<(), AudioError> {
    let mut cl = HDA_CONTROLLERS.lock();
    let c = cl.first_mut().ok_or(AudioError::NoController)?;
    let mut e = DmaPlaybackEngine::new();
    e.mmio_base = c.mmio_base;
    e.init(c)?;
    *DMA_ENGINE.lock() = Some(e);
    Ok(())
}

pub fn play_audio_dma(data: &[u8], fmt: AudioFormat) -> Result<(), AudioError> {
    let mut el = DMA_ENGINE.lock();
    let e = el.as_mut().ok_or(AudioError::NoController)?;
    e.stream.format = fmt;
    e.allocate_buffer(data.len())?;
    e.write_buffer(data)?;
    e.start_playback()
}

pub fn stop_audio_dma() -> Result<(), AudioError> {
    let mut el = DMA_ENGINE.lock();
    let e = el.as_mut().ok_or(AudioError::NoController)?;
    e.stop_playback()
}

// ============================================================================
// Audio Mixer
// ============================================================================

#[derive(Clone, Debug)]
pub struct MixerChannel {
    pub id: u8,
    pub name: String,
    pub volume: u8,
    pub pan: i8,
    pub muted: bool,
    pub solo: bool,
    pub input_stream: Option<u8>,
}

impl MixerChannel {
    pub fn new(id: u8, name: &str) -> Self {
        Self {
            id,
            name: name.into(),
            volume: 100,
            pan: 0,
            muted: false,
            solo: false,
            input_stream: None,
        }
    }

    pub fn apply_to_sample(&self, l: i16, r: i16) -> (i16, i16) {
        if self.muted {
            return (0, 0);
        }
        let v = self.volume as i32;
        let lv = l as i32 * v / 100;
        let rv = r as i32 * v / 100;
        let p = self.pan as i32;
        let lp = if p > 0 { (100 - p) * lv / 100 } else { lv };
        let rp = if p < 0 { (100 + p) * rv / 100 } else { rv };
        (
            lp.clamp(-32768, 32767) as i16,
            rp.clamp(-32768, 32767) as i16,
        )
    }
}

#[derive(Clone, Debug)]
pub struct AudioMixer {
    pub channels: Vec<MixerChannel>,
    pub master_volume: u8,
    pub master_muted: bool,
    pub sample_rate: u32,
    pub buffer_size: usize,
}

impl AudioMixer {
    pub fn new(sr: u32, bs: usize) -> Self {
        Self {
            channels: Vec::new(),
            master_volume: 100,
            master_muted: false,
            sample_rate: sr,
            buffer_size: bs,
        }
    }

    pub fn add_channel(&mut self, name: &str) -> u8 {
        let id = self.channels.len() as u8;
        self.channels.push(MixerChannel::new(id, name));
        id
    }

    pub fn remove_channel(&mut self, id: u8) {
        self.channels.retain(|c| c.id != id);
    }

    pub fn get_channel(&self, id: u8) -> Option<&MixerChannel> {
        self.channels.iter().find(|c| c.id == id)
    }

    pub fn get_channel_mut(&mut self, id: u8) -> Option<&mut MixerChannel> {
        self.channels.iter_mut().find(|c| c.id == id)
    }

    pub fn mix_to_buffer(&self, streams: &BTreeMap<u8, AudioStream>) -> Vec<u8> {
        let samples = self.buffer_size / 4;
        let mut output = vec![0i32; samples * 2];
        let any_solo = self.channels.iter().any(|c| c.solo);
        for i in 0..self.channels.len() {
            let ch = &self.channels[i];
            if ch.muted || (any_solo && !ch.solo) {
                continue;
            }
            if let Some(sid) = ch.input_stream {
                if let Some(s) = streams.get(&sid) {
                    if s.playing && s.format.channels == 2 && s.format.bits_per_sample == 16 {
                        for j in 0..samples {
                            let off = (s.position + j * 4).min(s.buffer.len().saturating_sub(4));
                            if off + 4 <= s.buffer.len() {
                                let l = i16::from_le_bytes([s.buffer[off], s.buffer[off + 1]]);
                                let r = i16::from_le_bytes([s.buffer[off + 2], s.buffer[off + 3]]);
                                let (lo, ro) = ch.apply_to_sample(l, r);
                                output[j * 2] += lo as i32;
                                output[j * 2 + 1] += ro as i32;
                            }
                        }
                    }
                }
            }
        }
        let mut ob = Vec::with_capacity(self.buffer_size);
        for i in 0..samples * 2 {
            let s = if self.master_muted {
                0
            } else {
                (output[i] * self.master_volume as i32 / 100).clamp(-32768, 32767)
            };
            ob.extend_from_slice(&(s as i16).to_le_bytes());
        }
        ob
    }

    pub fn set_master_volume(&mut self, v: u8) {
        self.master_volume = v.min(100);
    }

    pub fn set_master_mute(&mut self, m: bool) {
        self.master_muted = m;
    }
}

static AUDIO_MIXER: Mutex<Option<AudioMixer>> = Mutex::new(None);

pub fn init_mixer(sr: u32, bs: usize) {
    *AUDIO_MIXER.lock() = Some(AudioMixer::new(sr, bs));
}

pub fn get_mixer() -> Option<AudioMixer> {
    AUDIO_MIXER.lock().clone()
}

pub fn add_mixer_channel(name: &str) -> Option<u8> {
    AUDIO_MIXER.lock().as_mut().map(|m| m.add_channel(name))
}

pub fn set_channel_volume(cid: u8, v: u8) -> Result<(), AudioError> {
    let mut m = AUDIO_MIXER.lock();
    let mx = m.as_mut().ok_or(AudioError::NoController)?;
    let ch = mx.get_channel_mut(cid).ok_or(AudioError::NoStream)?;
    ch.volume = v.min(100);
    Ok(())
}

pub fn set_channel_pan(cid: u8, p: i8) -> Result<(), AudioError> {
    let mut m = AUDIO_MIXER.lock();
    let mx = m.as_mut().ok_or(AudioError::NoController)?;
    let ch = mx.get_channel_mut(cid).ok_or(AudioError::NoStream)?;
    ch.pan = p.clamp(-100, 100);
    Ok(())
}

pub fn set_channel_mute(cid: u8, mt: bool) -> Result<(), AudioError> {
    let mut m = AUDIO_MIXER.lock();
    let mx = m.as_mut().ok_or(AudioError::NoController)?;
    let ch = mx.get_channel_mut(cid).ok_or(AudioError::NoStream)?;
    ch.muted = mt;
    Ok(())
}

pub fn set_channel_solo(cid: u8, s: bool) -> Result<(), AudioError> {
    let mut m = AUDIO_MIXER.lock();
    let mx = m.as_mut().ok_or(AudioError::NoController)?;
    let ch = mx.get_channel_mut(cid).ok_or(AudioError::NoStream)?;
    ch.solo = s;
    Ok(())
}

pub fn link_stream_to_channel(cid: u8, sid: u8) -> Result<(), AudioError> {
    let mut m = AUDIO_MIXER.lock();
    let mx = m.as_mut().ok_or(AudioError::NoController)?;
    let ch = mx.get_channel_mut(cid).ok_or(AudioError::NoStream)?;
    ch.input_stream = Some(sid);
    Ok(())
}

pub fn mix_streams() -> Option<Vec<u8>> {
    let m = AUDIO_MIXER.lock();
    let mx = m.as_ref()?;
    let s = AUDIO_STREAMS.lock();
    Some(mx.mix_to_buffer(&s))
}

pub fn set_master_volume_global(v: f32) {
    let vol = (v.clamp(0.0, 1.0) * 100.0) as u8;
    let mut m = AUDIO_MIXER.lock();
    if let Some(ref mut mx) = *m {
        mx.set_master_volume(vol);
    }
}

pub fn set_master_mute_global(mt: bool) {
    let mut m = AUDIO_MIXER.lock();
    if let Some(ref mut mx) = *m {
        mx.set_master_mute(mt);
    }
}

// ============================================================================
// Audio Backend (high-level abstraction)
// ============================================================================

pub struct AudioBackend {
    pub volume: f32,
    pub playing: bool,
    pub position: f32,
}

impl AudioBackend {
    pub fn new() -> Self {
        Self {
            volume: 1.0,
            playing: false,
            position: 0.0,
        }
    }

    pub fn play(&mut self, _p: &str) {
        self.playing = true;
    }

    pub fn pause(&mut self) {
        self.playing = false;
    }

    pub fn stop(&mut self) {
        self.playing = false;
        self.position = 0.0;
    }

    pub fn set_volume(&mut self, v: f32) {
        self.volume = v;
    }

    pub fn seek(&mut self, p: f32) {
        self.position = p;
    }
}

lazy_static::lazy_static! {
    static ref AUDIO_BACKEND: Mutex<AudioBackend> = Mutex::new(AudioBackend::new());
}

pub fn get_audio() -> Option<&'static Mutex<AudioBackend>> {
    Some(&AUDIO_BACKEND)
}

// ============================================================================
// PCM Format
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PcmFormat {
    pub sample_rate: u32,
    pub bits_per_sample: u8,
    pub channels: u8,
    pub is_float: bool,
    pub is_big_endian: bool,
}

impl PcmFormat {
    pub fn new(sr: u32, bps: u8, ch: u8) -> Self {
        Self {
            sample_rate: sr,
            bits_per_sample: bps,
            channels: ch,
            is_float: false,
            is_big_endian: false,
        }
    }

    pub fn cd_quality() -> Self {
        Self::new(44100, 16, 2)
    }

    pub fn dvd_quality() -> Self {
        Self::new(48000, 16, 2)
    }

    pub fn bluray_quality() -> Self {
        Self::new(96000, 24, 6)
    }

    pub fn bytes_per_sample(&self) -> usize {
        (self.bits_per_sample as usize + 7) / 8
    }

    pub fn frame_size(&self) -> usize {
        self.bytes_per_sample() * self.channels as usize
    }

    pub fn byte_rate(&self) -> u32 {
        self.sample_rate * self.frame_size() as u32
    }
}

// ============================================================================
// Audio Codec Trait
// ============================================================================

pub trait AudioCodec {
    fn decode(&self, input: &[u8], output: &mut Vec<u8>) -> Result<usize, AudioError>;
    fn encode(&self, input: &[u8], output: &mut Vec<u8>) -> Result<usize, AudioError>;
    fn name(&self) -> &str;
    fn output_format(&self) -> PcmFormat;
}

// ============================================================================
// Sine Wave Generator (test tone)
// ============================================================================

fn sin_approx(x: f32) -> f32 {
    let mut x = x;
    let pi = core::f32::consts::PI;
    let tp = 2.0 * pi;
    while x > pi {
        x -= tp;
    }
    while x < -pi {
        x += tp;
    }
    let x2 = x * x;
    x - x2 * x / 6.0 + x2 * x2 * x / 120.0
}

pub struct SineWaveCodec {
    pub frequency: f32,
    pub sample_rate: u32,
    pub amplitude: f32,
    pub phase: f32,
}

impl SineWaveCodec {
    pub fn new(f: f32, sr: u32) -> Self {
        Self {
            frequency: f,
            sample_rate: sr,
            amplitude: 0.5,
            phase: 0.0,
        }
    }

    pub fn generate(&mut self, samples: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(samples * 2);
        let step = 2.0 * core::f32::consts::PI * self.frequency / self.sample_rate as f32;
        for _ in 0..samples {
            let s = self.amplitude * sin_approx(self.phase);
            out.extend_from_slice(&((s * 32767.0) as i16).to_le_bytes());
            self.phase += step;
            if self.phase > 2.0 * core::f32::consts::PI {
                self.phase -= 2.0 * core::f32::consts::PI;
            }
        }
        out
    }
}

impl AudioCodec for SineWaveCodec {
    fn decode(&self, _i: &[u8], o: &mut Vec<u8>) -> Result<usize, AudioError> {
        let mut c = self.clone();
        let d = c.generate(1024);
        o.extend_from_slice(&d);
        Ok(d.len())
    }

    fn encode(&self, _i: &[u8], _o: &mut Vec<u8>) -> Result<usize, AudioError> {
        Err(AudioError::FormatNotSupported)
    }

    fn name(&self) -> &str {
        "SineWave"
    }

    fn output_format(&self) -> PcmFormat {
        PcmFormat::new(self.sample_rate, 16, 1)
    }
}

impl Clone for SineWaveCodec {
    fn clone(&self) -> Self {
        Self {
            frequency: self.frequency,
            sample_rate: self.sample_rate,
            amplitude: self.amplitude,
            phase: self.phase,
        }
    }
}

// ============================================================================
// Mu-Law Codec
// ============================================================================

pub struct MuLawCodec {
    pub sample_rate: u32,
}

impl MuLawCodec {
    pub fn new(sr: u32) -> Self {
        Self { sample_rate: sr }
    }

    pub fn decode_sample(s: u8) -> i16 {
        let s = s ^ 0xFF;
        let sign = if s & 0x80 != 0 { -1 } else { 1 };
        let exp = (s >> 4) & 0x07;
        let mant = s & 0x0F;
        ((33 * (2 * mant as i32 + 33) * (1 << exp) - 33) * sign).clamp(-32768, 32767) as i16
    }

    pub fn encode_sample(s: i16) -> u8 {
        let mut best_code = 0u8;
        let mut best_diff = i32::MAX;
        for code in 0u16..=u8::MAX as u16 {
            let decoded = Self::decode_sample(code as u8);
            let diff = (s as i32 - decoded as i32).abs();
            if diff < best_diff {
                best_diff = diff;
                best_code = code as u8;
            }
        }
        best_code
    }
}

impl AudioCodec for MuLawCodec {
    fn decode(&self, i: &[u8], o: &mut Vec<u8>) -> Result<usize, AudioError> {
        o.reserve(i.len() * 2);
        for s in i {
            o.extend_from_slice(&Self::decode_sample(*s).to_le_bytes());
        }
        Ok(i.len() * 2)
    }

    fn encode(&self, i: &[u8], o: &mut Vec<u8>) -> Result<usize, AudioError> {
        if i.len() % 2 != 0 {
            return Err(AudioError::BufferError);
        }
        for c in i.chunks(2) {
            o.push(Self::encode_sample(i16::from_le_bytes([c[0], c[1]])));
        }
        Ok(i.len() / 2)
    }

    fn name(&self) -> &str {
        "MuLaw"
    }

    fn output_format(&self) -> PcmFormat {
        PcmFormat::new(self.sample_rate, 16, 1)
    }
}

// ============================================================================
// A-Law Codec
// ============================================================================

pub struct ALawCodec {
    pub sample_rate: u32,
}

impl ALawCodec {
    pub fn new(sr: u32) -> Self {
        Self { sample_rate: sr }
    }

    pub fn decode_sample(s: u8) -> i16 {
        let s = s ^ 0x55;
        let sign = if s & 0x80 != 0 { -1 } else { 1 };
        let exp = (s >> 4) & 0x07;
        let mant = s & 0x0F;
        let d = if exp == 0 {
            (mant as i32 * 2 + 1) * 16 * sign
        } else {
            ((1 << exp) * (mant as i32 * 2 + 33) - 32) * sign
        };
        d.clamp(-32768, 32767) as i16
    }

    pub fn encode_sample(s: i16) -> u8 {
        let sign = if s < 0 { 0x80 } else { 0 };
        let s = s.abs() as i32;
        let (exp, mant) = if s > 0x0F {
            let mut e = 7;
            while e > 0 && s <= (0x10 << e) {
                e -= 1;
            }
            (e, (s >> (e + 3)) & 0x0F)
        } else {
            (0, s >> 1)
        };
        ((sign | (exp << 4) | mant) ^ 0x55) as u8
    }
}

impl AudioCodec for ALawCodec {
    fn decode(&self, i: &[u8], o: &mut Vec<u8>) -> Result<usize, AudioError> {
        o.reserve(i.len() * 2);
        for s in i {
            o.extend_from_slice(&Self::decode_sample(*s).to_le_bytes());
        }
        Ok(i.len() * 2)
    }

    fn encode(&self, i: &[u8], o: &mut Vec<u8>) -> Result<usize, AudioError> {
        if i.len() % 2 != 0 {
            return Err(AudioError::BufferError);
        }
        for c in i.chunks(2) {
            o.push(Self::encode_sample(i16::from_le_bytes([c[0], c[1]])));
        }
        Ok(i.len() / 2)
    }

    fn name(&self) -> &str {
        "ALaw"
    }

    fn output_format(&self) -> PcmFormat {
        PcmFormat::new(self.sample_rate, 16, 1)
    }
}

// ============================================================================
// DMA Audio Transfer
// ============================================================================

#[derive(Clone, Debug)]
pub struct DmaAudioTransfer {
    pub buffer_addr: u64,
    pub buffer_size: usize,
    pub position: usize,
    pub active: bool,
    pub callback: Option<fn()>,
}

impl DmaAudioTransfer {
    pub fn new(ba: u64, bs: usize) -> Self {
        Self {
            buffer_addr: ba,
            buffer_size: bs,
            position: 0,
            active: false,
            callback: None,
        }
    }

    pub fn start(&mut self) {
        self.active = true;
        self.position = 0;
    }

    pub fn stop(&mut self) {
        self.active = false;
    }

    pub fn set_callback(&mut self, cb: fn()) {
        self.callback = Some(cb);
    }

    pub fn get_next_fragment(&mut self, fs: usize) -> Option<(u64, usize)> {
        if !self.active {
            return None;
        }
        if self.position >= self.buffer_size {
            if let Some(cb) = self.callback {
                cb();
            }
            return None;
        }
        let rem = self.buffer_size - self.position;
        let sz = fs.min(rem);
        let addr = self.buffer_addr + self.position as u64;
        self.position += sz;
        Some((addr, sz))
    }
}

// ============================================================================
// Host Corpus Tests (D-AUDIO-01)
// ============================================================================

#[cfg(test)]
mod hda_tests {
    use super::*;

    #[test]
    fn verb_encoding_matches_spec() {
        // Intel HDA Spec §3.7: CORB entry format
        // [31:28] LinkID | [27] IndirectNID | [26:20] NID | [19:8] Verb | [7:0] Parameter
        let verb = encode_verb(3, 0x02, VERB_GET_PARAMETER, PAR_VENDOR_ID);
        assert_eq!(verb, (3 << 28) | (0x02 << 20) | (0x0F00 << 8) | 0x00);
    }

    #[test]
    fn audio_format_to_hda_register() {
        let fmt = AudioFormat::dvd_quality(); // 48000Hz, 16bit, 2ch
        let reg = fmt.to_hda_format();
        // 48K base = 0x0000, 16bit = 0x10, 2ch = channels-1 = 0x1
        assert_eq!(reg, 0x0011);

        let fmt = AudioFormat::cd_quality(); // 44100Hz, 16bit, 2ch
        let reg = fmt.to_hda_format();
        // 44.1K base = 0x4000, 16bit = 0x10, 2ch = 0x1
        assert_eq!(reg, 0x4011);
    }

    #[test]
    fn bdl_entry_format() {
        let entry = BufferDescriptorEntry::new(0x1234_5678_9ABC_DEF0, 4096, true);
        assert_eq!(entry.address_low, 0x9ABC_DEF0);
        assert_eq!(entry.address_high, 0x1234_5678);
        assert_eq!(entry.length, 4096);
        assert_eq!(entry.flags, BDL_IOC);
    }

    #[test]
    fn widget_type_from_u8() {
        assert_eq!(HdaWidgetType::from_u8(0x0), HdaWidgetType::OutputDac);
        assert_eq!(HdaWidgetType::from_u8(0x1), HdaWidgetType::InputAdc);
        assert_eq!(HdaWidgetType::from_u8(0x4), HdaWidgetType::Pin);
        assert_eq!(HdaWidgetType::from_u8(0xFF), HdaWidgetType::Unknown);
    }

    #[test]
    fn codec_vendor_device_id_extraction() {
        // GET_PARAMETER(PAR_VENDOR_ID) returns: [31:16] Vendor ID, [15:0] Device ID
        let response = (0x8086 << 16) | 0x2807; // Intel HDA codec
        let vendor_id = (response >> 16) as u16;
        let device_id = (response & 0xFFFF) as u16;
        assert_eq!(vendor_id, 0x8086);
        assert_eq!(device_id, 0x2807);
    }

    #[test]
    fn stream_descriptor_format_calculation() {
        let sd = StreamDescriptor::new(0, StreamDirection::Playback);
        let fmt = AudioFormat::new(48000, 16, 2);
        let sd_with_fmt = StreamDescriptor { format: fmt, ..sd };
        let reg = sd_with_fmt.calculate_format_value();
        assert_eq!(reg, 0x0011);
    }

    #[test]
    fn corb_rirb_size_encoding() {
        // Intel HDA Spec: CORBSIZE/RIRBSIZE values
        assert_eq!(RB_SIZE_256, 0x02);
        assert_eq!(RB_SIZE_16, 0x01);
        assert_eq!(RB_SIZE_2, 0x00);
    }

    #[test]
    fn pin_widget_control_bits() {
        assert_eq!(PIN_CTL_OUT_EN, 0x01);
        assert_eq!(PIN_CTL_IN_EN, 0x20);
        assert_eq!(PIN_CTL_HP_EN, 0x40);
    }

    #[test]
    fn controller_reset_sequence_valid() {
        // Verify GCTL bit definitions match spec
        assert_eq!(GCTL_CRST, 0x01);
        assert_eq!(GCTL_UNSOL, 0x100);
    }

    #[test]
    fn dma_playback_engine_initial_state() {
        let engine = DmaPlaybackEngine::new();
        assert!(!engine.initialized);
        assert_eq!(engine.buffer_phys, 0);
        assert_eq!(engine.buffer_virt, 0);
        assert_eq!(engine.stream_base, 0x80);
    }

    #[test]
    fn audio_stream_lifecycle() {
        let fmt = AudioFormat::cd_quality();
        let mut stream = AudioStream::new(1, StreamDirection::Playback, fmt);
        assert!(!stream.playing);
        stream.start();
        assert!(stream.playing);
        stream.stop();
        assert!(!stream.playing);
    }

    #[test]
    fn mu_law_encode_decode_roundtrip() {
        let original: i16 = 12345;
        let encoded = MuLawCodec::encode_sample(original);
        let decoded = MuLawCodec::decode_sample(encoded);
        // Mu-law is lossy, but should be within reasonable range
        let diff = (original as i32 - decoded as i32).abs();
        assert!(diff < 1000);
    }

    #[test]
    fn a_law_encode_decode_roundtrip() {
        let original: i16 = -5000;
        let encoded = ALawCodec::encode_sample(original);
        let decoded = ALawCodec::decode_sample(encoded);
        let diff = (original as i32 - decoded as i32).abs();
        assert!(diff < 1000);
    }

    #[test]
    fn sine_wave_generator_produces_output() {
        let mut gen = SineWaveCodec::new(440.0, 48000);
        let output = gen.generate(100);
        assert_eq!(output.len(), 200); // 100 samples × 2 bytes
    }

    #[test]
    fn pcm_format_calculations() {
        let fmt = PcmFormat::cd_quality();
        assert_eq!(fmt.bytes_per_sample(), 2);
        assert_eq!(fmt.frame_size(), 4); // 2 bytes × 2 channels
        assert_eq!(fmt.byte_rate(), 176400); // 44100 × 4
    }
}
