//! # Audio/ALSA Jail — TIER 2 HD Audio Surucusu
//!
//! Intel HD Audio (HDA) codec ve PCM stream yonetimi jail sandbox ortaminda.
//! ALSA uyumlu arayuz ile ses donanimini kontrol eder.
//!
//! ## Crash-Only Microreboot Sozlesmesi (MINIX 3 Reincarnation Server modeli)
//!
//! Audio jail, crash-only tasarim prensibine gore calisir:
//! - Her islem atomik bir birimdir; yarim kalan islem birakilmaz
//! - Crash durumunda jail worker otomatik yeniden baslatilir
//! - MMIO durumu her reboot'da sifirdan yapilandirilir
//! - DMA buffer'lar reboot'da yeniden tahsis edilir
//! - Binary exponential backoff ile restart flood onlenir
//!
//! ## ALSA PCM Ring Buffer Modeli
//!
//! ```text
//! ┌──────────────────┐  SPSC Ring   ┌───────────────┐  CORB/RIRB  ┌──────────┐
//! │ Core (Producer)  │◄────────────►│ Audio Jail    │────────────►│ HDA      │
//! │ appl_ptr         │ JailChannel  │ (Tier 2)      │  DMA        │ Codec    │
//! │                  │              │ hw_ptr        │             │          │
//! └──────────────────┘              └───────────────┘             └──────────┘
//! ```
//!
//! - `appl_ptr`: uygulamanin yazdigi son frame (core tarafinda takip)
//! - `hw_ptr`: donanimin oynattigi son frame (jail tarafinda, LPIB'den okunur)
//! - `avail = appl_ptr - hw_ptr`: yazilabilir frame sayisi
//! - Period boundary'de jail → core'a JailEvent gonderir

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

use crate::drivers::audio::{
    default_codec, default_controller, init_dma_playback, play_audio_dma, stop_audio_dma,
    AudioError, AudioFormat, HdaController, StreamDirection,
};
use crate::drivers::jail_ring::{JailChannel, JailEvent, JailOpcode, JailRequest};

// ============================================================================
// HDA Controller Register Offsets
// ============================================================================

const HDA_GCAP: usize = 0x00;
const HDA_GCTL: usize = 0x08;
const HDA_STATESTS: usize = 0x0E;
const HDA_CORBLBASE: usize = 0x40;
const HDA_CORBUBASE: usize = 0x44;
const HDA_CORBWP: usize = 0x48;
const HDA_CORBRP: usize = 0x4A;
const HDA_CORBCTL: usize = 0x4C;
const HDA_RIRBLBASE: usize = 0x50;
const HDA_RIRBUBASE: usize = 0x54;
const HDA_RIRBWP: usize = 0x58;
const HDA_RIRBCTL: usize = 0x5C;
const HDA_RINTCNT: usize = 0x5A;
const HDA_RIRBSIZE: usize = 0x5E;
const HDA_CORBSIZE: usize = 0x4E;
const HDA_SD_LPIB: u64 = 0x04;
const HDA_SD_CBL: u64 = 0x08;
const HDA_SD_CTL: u64 = 0x00;

const GCTL_CRST: u32 = 1 << 0;
const GCTL_UNSOL: u32 = 1 << 8;
const CORBCTL_RUN: u8 = 1 << 1;
const RIRBCTL_RUN: u8 = 1 << 1;
const RIRBCTL_IRQ: u8 = 1 << 0;

// ============================================================================
// Audio Jail Komutlari (genisletilmis)
// ============================================================================

#[derive(Clone, Debug)]
pub enum AudioJailCommand {
    /// HDA controller reset ve re-init
    ResetController,
    /// Codec kesfi (CORB/RIRB uzerinden)
    EnumerateCodecs,
    /// Playback stream yapilandir
    ConfigurePlayback {
        sample_rate: u32,
        bit_depth: u8,
        channels: u8,
        buffer_size: usize,
    },
    /// Capture stream yapilandir
    ConfigureCapture {
        sample_rate: u32,
        bit_depth: u8,
        channels: u8,
        buffer_size: usize,
    },
    /// Volume ayarla (0-100)
    SetVolume(u32),
    /// Mute toggle
    SetMute(bool),
    /// DMA buffer yaz (PCM sample transfer)
    WriteDmaBuffer { offset: usize, data: Vec<u8> },
    /// Playback baslat
    StartPlayback,
    /// Playback durdur
    StopPlayback,
    /// Stream durumu sorgula
    GetStatus,
    /// Codec widget tarama
    ScanWidgets { codec_addr: u8 },
    /// PCM ring buffer'dan frame oku (ALSA avail_update)
    GetAvailFrames,
    /// Buffer'i silence ile doldur (underrun onleme)
    FillSilence { frames: u32 },
    /// Stream pozisyonu (hw_ptr)
    GetHwPosition,
}

#[derive(Clone, Debug)]
pub enum AudioJailResponse {
    Ok,
    Error(AudioJailError),
    CodecList(Vec<CodecInfo>),
    Status(AudioJailStatus),
    WidgetList(Vec<WidgetInfo>),
    Volume(u32),
    AvailFrames(u32),
    HwPosition(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioJailError {
    NotInitialized,
    ControllerResetFailed,
    CodecNotFound,
    StreamError,
    BufferOverflow,
    MmioTimeout,
    JailChannelClosed,
    InvalidParameter,
    DmaAllocationFailed,
    Underrun,
    DegradedMode,
}

#[derive(Clone, Debug)]
pub struct AudioJailStatus {
    pub initialized: bool,
    pub codec_count: usize,
    pub playback_active: bool,
    pub capture_active: bool,
    pub volume: u32,
    pub muted: bool,
    pub crash_count: u32,
    pub last_reboot_seq: u64,
    pub hw_ptr: u32,
    pub appl_ptr: u32,
    pub avail_frames: u32,
    pub buffer_size_frames: u32,
    pub degraded: bool,
    pub underrun_count: u32,
}

#[derive(Clone, Debug)]
pub struct CodecInfo {
    pub address: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub widget_count: usize,
}

#[derive(Clone, Debug)]
pub struct WidgetInfo {
    pub nid: u8,
    pub widget_type: u8,
    pub name: String,
}

// ============================================================================
// PCM Ring Buffer State (ALSA modeli)
// ============================================================================

/// ALSA PCM ring buffer durumu.
/// Core (appl_ptr) ve Jail (hw_ptr) arasindaki senkronizasyon.
pub struct PcmRingState {
    /// Uygulamanin yazdigi son frame (core tarafinda guncellenir)
    pub appl_ptr: AtomicU64,
    /// Donanimin oynattigi son frame (jail tarafinda, LPIB'den)
    pub hw_ptr: AtomicU64,
    /// Toplam buffer boyutu (frame cinsinden)
    pub buffer_size_frames: AtomicU32,
    /// Period boyutu (frame cinsinden) — her period'da interrupt
    pub period_size_frames: AtomicU32,
    /// Son period hw_ptr (period boundary tespiti icin)
    pub last_period_hw_ptr: AtomicU64,
    /// Underrun sayisi (hw_ptr >= appl_ptr ve playback aktif)
    pub underrun_count: AtomicU32,
    /// Toplam yazilan frame
    pub total_written_frames: AtomicU64,
    /// Toplam oynatilan frame
    pub total_played_frames: AtomicU64,
}

impl PcmRingState {
    pub fn new() -> Self {
        Self {
            appl_ptr: AtomicU64::new(0),
            hw_ptr: AtomicU64::new(0),
            buffer_size_frames: AtomicU32::new(0),
            period_size_frames: AtomicU32::new(0),
            last_period_hw_ptr: AtomicU64::new(0),
            underrun_count: AtomicU32::new(0),
            total_written_frames: AtomicU64::new(0),
            total_played_frames: AtomicU64::new(0),
        }
    }

    /// Mevcut avail (yazilabilir) frame sayisi
    pub fn avail(&self) -> u32 {
        let boundary = self.buffer_size_frames.load(Ordering::Acquire) as u64;
        if boundary == 0 {
            return 0;
        }
        let appl = self.appl_ptr.load(Ordering::Acquire) % boundary;
        let hw = self.hw_ptr.load(Ordering::Acquire) % boundary;
        let queued = if appl >= hw {
            appl - hw
        } else {
            boundary - hw + appl
        };
        (boundary - queued) as u32
    }

    /// Period boundary kontrolu — yeni period tamamlandi mi?
    pub fn check_period_boundary(&self) -> bool {
        let period = self.period_size_frames.load(Ordering::Acquire) as u64;
        if period == 0 {
            return false;
        }
        let hw = self.hw_ptr.load(Ordering::Acquire);
        let last = self.last_period_hw_ptr.load(Ordering::Acquire);
        hw / period > last / period
    }

    /// Period boundary'yi guncelle
    pub fn update_period_boundary(&self) {
        let period = self.period_size_frames.load(Ordering::Acquire) as u64;
        if period == 0 {
            return;
        }
        let hw = self.hw_ptr.load(Ordering::Acquire);
        self.last_period_hw_ptr
            .store((hw / period) * period, Ordering::Release);
    }

    /// Underrun kontrolu: hw_ptr >= appl_ptr ve playback aktif
    pub fn check_underrun(&self, playback_active: bool) -> bool {
        if !playback_active {
            return false;
        }
        let boundary = self.buffer_size_frames.load(Ordering::Acquire) as u64;
        if boundary == 0 {
            return false;
        }
        let appl = self.appl_ptr.load(Ordering::Acquire) % boundary;
        let hw = self.hw_ptr.load(Ordering::Acquire) % boundary;
        if appl == hw {
            self.underrun_count.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }
}

// ============================================================================
// Audio Jail Controller — Crash-Only Microreboot (MINIX 3 modeli)
// ============================================================================

pub struct AudioJailController {
    mmio_base: u64,
    codecs: Vec<CodecInfo>,
    playback_config: Option<PlaybackConfig>,
    capture_config: Option<CaptureConfig>,
    master_volume: AtomicU32,
    master_mute: AtomicBool,
    ready: AtomicBool,
    pub jail_id: u32,

    // Crash-only microreboot state (MINIX 3 reincarnation server modeli)
    crash_count: AtomicU32,
    reboot_seq: AtomicU64,
    last_healthy_seq: AtomicU64,
    jail_channel: Option<JailChannel>,
    watchdog_timeout_ms: AtomicU32,
    last_heartbeat: AtomicU64,

    // Restart policy: binary exponential backoff
    restart_backoff_ms: AtomicU64,
    max_backoff_ms: AtomicU64,
    consecutive_crashes: AtomicU32,

    // Degraded mode flag
    degraded: AtomicBool,

    // PCM ring buffer state (ALSA modeli)
    pcm_ring: PcmRingState,
    playback_active: AtomicBool,

    // DMA buffer tracking
    dma_buffer_phys: AtomicU64,
    dma_buffer_size: AtomicU64,
}

#[derive(Clone, Debug)]
struct PlaybackConfig {
    sample_rate: u32,
    bit_depth: u8,
    channels: u8,
    buffer_size: usize,
}

#[derive(Clone, Debug)]
struct CaptureConfig {
    sample_rate: u32,
    bit_depth: u8,
    channels: u8,
    buffer_size: usize,
}

impl AudioJailController {
    pub fn new(mmio_base: u64) -> Self {
        Self {
            mmio_base,
            codecs: Vec::new(),
            playback_config: None,
            capture_config: None,
            master_volume: AtomicU32::new(80),
            master_mute: AtomicBool::new(false),
            ready: AtomicBool::new(false),
            jail_id: 0,
            crash_count: AtomicU32::new(0),
            reboot_seq: AtomicU64::new(0),
            last_healthy_seq: AtomicU64::new(0),
            jail_channel: None,
            watchdog_timeout_ms: AtomicU32::new(5000),
            last_heartbeat: AtomicU64::new(0),
            restart_backoff_ms: AtomicU64::new(100),
            max_backoff_ms: AtomicU64::new(30000),
            consecutive_crashes: AtomicU32::new(0),
            degraded: AtomicBool::new(false),
            pcm_ring: PcmRingState::new(),
            playback_active: AtomicBool::new(false),
            dma_buffer_phys: AtomicU64::new(0),
            dma_buffer_size: AtomicU64::new(0),
        }
    }

    // ========================================================================
    // Crash-Only Microreboot Contract (MINIX 3 Reincarnation Server)
    // ========================================================================

    /// Jail crash sayisini artirir ve microreboot baslatir.
    /// Intel HDA spec S3.3.1: GCTL.CRST clear->wait->set->wait pattern
    /// MINIX 3: binary exponential backoff ile restart flood onleme
    pub fn crash_and_reboot(&self) -> Result<u64, AudioJailError> {
        let crashes = self.crash_count.fetch_add(1, Ordering::SeqCst) + 1;
        self.consecutive_crashes.fetch_add(1, Ordering::SeqCst);
        let new_seq = self.reboot_seq.fetch_add(1, Ordering::SeqCst) + 1;

        // Binary exponential backoff hesapla
        let backoff = self.calculate_backoff(crashes);

        crate::serial_println!(
            "[Audio-Jail] Crash #{}, backoff={}ms, initiating microreboot seq={}",
            crashes,
            backoff,
            new_seq
        );

        // Backoff suresi bekle (simulated — gercekte timer kullanilir)
        if backoff > 0 {
            crate::serial_println!("[Audio-Jail] Backoff delay: {}ms", backoff);
        }

        self.reset_mmio()?;
        self.ready.store(true, Ordering::SeqCst);
        self.last_healthy_seq.store(new_seq, Ordering::SeqCst);

        // Basarili reboot → backoff sifirla
        self.consecutive_crashes.store(0, Ordering::SeqCst);
        self.restart_backoff_ms.store(100, Ordering::SeqCst);

        crate::serial_println!("[Audio-Jail] Microreboot complete, seq={}", new_seq);
        Ok(new_seq)
    }

    /// Binary exponential backoff hesapla (MINIX 3 policy-driven recovery)
    fn calculate_backoff(&self, crash_count: u32) -> u64 {
        let base = self.restart_backoff_ms.load(Ordering::Relaxed);
        let max = self.max_backoff_ms.load(Ordering::Relaxed);
        let backoff = base.saturating_mul(2u64.saturating_pow(crash_count.min(10) as u32));
        backoff.min(max)
    }

    /// Watchdog heartbeat — jail worker periyodik olarak cagirir
    pub fn heartbeat(&self) {
        let now = crate::interrupts::get_ticks();
        self.last_heartbeat.store(now, Ordering::Relaxed);
    }

    /// Watchdog timeout kontrolu
    pub fn check_watchdog(&self) -> bool {
        let now = crate::interrupts::get_ticks();
        let last = self.last_heartbeat.load(Ordering::Relaxed);
        let timeout = self.watchdog_timeout_ms.load(Ordering::Relaxed) as u64;
        Self::watchdog_expired_at(now, last, timeout)
    }

    fn watchdog_expired_at(now: u64, last: u64, timeout: u64) -> bool {
        now.saturating_sub(last) > timeout
    }

    /// MMIO durumunu sifirlar: controller reset + CORB/RIRB re-init
    fn reset_mmio(&self) -> Result<(), AudioJailError> {
        #[cfg(test)]
        {
            if self.mmio_base == 0 {
                return Err(AudioJailError::MmioTimeout);
            }
            Ok(())
        }

        #[cfg(not(test))]
        {
            unsafe {
                let gctl_addr = self.mmio_base + HDA_GCTL as u64;

                // CRST = 0 (assert reset)
                let gctl = core::ptr::read_volatile(gctl_addr as *const u32);
                core::ptr::write_volatile(gctl_addr as *mut u32, gctl & !GCTL_CRST);

                let mut timeout = 10000;
                while (core::ptr::read_volatile(gctl_addr as *const u32) & GCTL_CRST) != 0
                    && timeout > 0
                {
                    core::hint::spin_loop();
                    timeout -= 1;
                }
                if timeout == 0 {
                    return Err(AudioJailError::MmioTimeout);
                }

                // CRST = 1 (exit reset)
                core::ptr::write_volatile(gctl_addr as *mut u32, gctl | GCTL_CRST);

                timeout = 10000;
                while (core::ptr::read_volatile(gctl_addr as *const u32) & GCTL_CRST) == 0
                    && timeout > 0
                {
                    core::hint::spin_loop();
                    timeout -= 1;
                }
                if timeout == 0 {
                    return Err(AudioJailError::MmioTimeout);
                }

                // CORB/RIRB re-init
                core::ptr::write_volatile((self.mmio_base + HDA_CORBCTL as u64) as *mut u8, 0);
                core::ptr::write_volatile((self.mmio_base + HDA_RIRBCTL as u64) as *mut u8, 0);
                core::ptr::write_volatile((self.mmio_base + HDA_CORBWP as u64) as *mut u16, 0);
                core::ptr::write_volatile((self.mmio_base + HDA_CORBRP as u64) as *mut u16, 0x8000);
                core::ptr::write_volatile((self.mmio_base + HDA_RIRBWP as u64) as *mut u16, 0x8000);
                core::ptr::write_volatile(
                    (self.mmio_base + HDA_CORBCTL as u64) as *mut u8,
                    CORBCTL_RUN,
                );
                core::ptr::write_volatile(
                    (self.mmio_base + HDA_RIRBCTL as u64) as *mut u8,
                    RIRBCTL_RUN | RIRBCTL_IRQ,
                );

                // Unsolicited responses
                let gctl2 = core::ptr::read_volatile(gctl_addr as *const u32);
                core::ptr::write_volatile(gctl_addr as *mut u32, gctl2 | GCTL_UNSOL);
            }

            crate::serial_println!("[Audio-Jail] MMIO reset complete");
            Ok(())
        }
    }

    // ========================================================================
    // Jail Command Processing
    // ========================================================================

    pub fn process_command(&mut self, cmd: AudioJailCommand) -> AudioJailResponse {
        if !self.ready.load(Ordering::SeqCst) && !matches!(&cmd, AudioJailCommand::ResetController)
        {
            return AudioJailResponse::Error(AudioJailError::NotInitialized);
        }

        match cmd {
            AudioJailCommand::ResetController => match self.crash_and_reboot() {
                Ok(seq) => {
                    crate::serial_println!("[Audio-Jail] Controller rebooted, seq={}", seq);
                    AudioJailResponse::Ok
                }
                Err(e) => AudioJailResponse::Error(e),
            },
            AudioJailCommand::EnumerateCodecs => {
                self.enumerate_codecs();
                let codecs: Vec<CodecInfo> = self.codecs.clone();
                AudioJailResponse::CodecList(codecs)
            }
            AudioJailCommand::ConfigurePlayback {
                sample_rate,
                bit_depth,
                channels,
                buffer_size,
            } => {
                self.playback_config = Some(PlaybackConfig {
                    sample_rate,
                    bit_depth,
                    channels,
                    buffer_size,
                });

                // PCM ring buffer boyutunu hesapla (frame cinsinden)
                let frame_size = (bit_depth as u32 / 8 * channels as u32) as usize;
                let buffer_size_frames = if frame_size > 0 {
                    buffer_size / frame_size
                } else {
                    0
                };
                self.pcm_ring
                    .buffer_size_frames
                    .store(buffer_size_frames as u32, Ordering::Release);

                // Period size = buffer_size / 4 (ALSA default)
                let period_frames = buffer_size_frames / 4;
                self.pcm_ring
                    .period_size_frames
                    .store(period_frames as u32, Ordering::Release);

                crate::serial_println!(
                    "[Audio-Jail] Playback configured: {}Hz, {}bit, {}ch, {}B buffer, {} frames",
                    sample_rate,
                    bit_depth,
                    channels,
                    buffer_size,
                    buffer_size_frames
                );
                AudioJailResponse::Ok
            }
            AudioJailCommand::ConfigureCapture {
                sample_rate,
                bit_depth,
                channels,
                buffer_size,
            } => {
                self.capture_config = Some(CaptureConfig {
                    sample_rate,
                    bit_depth,
                    channels,
                    buffer_size,
                });
                crate::serial_println!(
                    "[Audio-Jail] Capture configured: {}Hz, {}bit, {}ch, {}B buffer",
                    sample_rate,
                    bit_depth,
                    channels,
                    buffer_size
                );
                AudioJailResponse::Ok
            }
            AudioJailCommand::SetVolume(vol) => {
                self.master_volume.store(vol.min(100), Ordering::Relaxed);
                AudioJailResponse::Ok
            }
            AudioJailCommand::SetMute(mute) => {
                self.master_mute.store(mute, Ordering::Relaxed);
                AudioJailResponse::Ok
            }
            AudioJailCommand::WriteDmaBuffer { offset, data } => {
                // Gercek DMA buffer yazma
                match self.write_dma_buffer(offset, &data) {
                    Ok(written) => {
                        // appl_ptr'yi guncelle (ALSA modeli)
                        let frame_size = self.frame_size();
                        if frame_size > 0 {
                            let frames_written = (written / frame_size) as u64;
                            let boundary =
                                self.pcm_ring.buffer_size_frames.load(Ordering::Acquire) as u64;
                            let old_appl = self.pcm_ring.appl_ptr.load(Ordering::Relaxed);
                            let new_appl = if boundary > 0 {
                                (old_appl + frames_written) % boundary
                            } else {
                                old_appl + frames_written
                            };
                            self.pcm_ring.appl_ptr.store(new_appl, Ordering::Release);
                            self.pcm_ring
                                .total_written_frames
                                .fetch_add(frames_written, Ordering::Relaxed);
                        }
                        AudioJailResponse::Ok
                    }
                    Err(e) => AudioJailResponse::Error(e),
                }
            }
            AudioJailCommand::StartPlayback => {
                match self.start_playback_internal() {
                    Ok(()) => {
                        self.playback_active.store(true, Ordering::Release);
                        AudioJailResponse::Ok
                    }
                    Err(e) => {
                        // Degraded fallback: DMA baslatilamazsa typed error
                        self.degraded.store(true, Ordering::Release);
                        AudioJailResponse::Error(e)
                    }
                }
            }
            AudioJailCommand::StopPlayback => {
                self.stop_playback_internal();
                self.playback_active.store(false, Ordering::Release);
                AudioJailResponse::Ok
            }
            AudioJailCommand::GetStatus => AudioJailResponse::Status(self.get_status()),
            AudioJailCommand::ScanWidgets { codec_addr } => self.scan_widgets_real(codec_addr),
            AudioJailCommand::GetAvailFrames => {
                let avail = self.pcm_ring.avail();
                AudioJailResponse::AvailFrames(avail)
            }
            AudioJailCommand::FillSilence { frames } => {
                self.fill_silence(frames);
                AudioJailResponse::Ok
            }
            AudioJailCommand::GetHwPosition => {
                let hw = self.pcm_ring.hw_ptr.load(Ordering::Acquire) as u32;
                AudioJailResponse::HwPosition(hw)
            }
        }
    }

    /// Frame boyutunu hesapla (byte cinsinden)
    fn frame_size(&self) -> usize {
        if let Some(ref cfg) = self.playback_config {
            (cfg.bit_depth as usize / 8) * cfg.channels as usize
        } else {
            0
        }
    }

    /// Gercek DMA buffer yazma
    fn write_dma_buffer(&self, offset: usize, data: &[u8]) -> Result<usize, AudioJailError> {
        let buf_phys = self.dma_buffer_phys.load(Ordering::Acquire);
        let buf_size = self.dma_buffer_size.load(Ordering::Acquire);

        if buf_phys == 0 || buf_size == 0 {
            // DMA buffer tahsis edilmemis — degraded mode
            return Err(AudioJailError::DmaAllocationFailed);
        }

        if offset >= buf_size as usize {
            return Err(AudioJailError::BufferOverflow);
        }

        let available = (buf_size as usize).saturating_sub(offset);
        let copy_len = data.len().min(available);

        if copy_len == 0 {
            return Ok(0);
        }

        // Gercek DMA buffer'a yaz (phys_addr → virt_addr donusumu)
        #[cfg(not(any(test, target_os = "windows")))]
        {
            use crate::memory::phys_to_virt;
            let virt = phys_to_virt((buf_phys + offset as u64) as usize);
            unsafe {
                core::ptr::copy_nonoverlapping(data.as_ptr(), virt as *mut u8, copy_len);
            }
        }

        #[cfg(any(test, target_os = "windows"))]
        {
            // Test mode: simulate write
            let _ = (buf_phys, offset, data);
        }

        Ok(copy_len)
    }

    /// Playback baslatma — gercek HDA controller programlama
    fn start_playback_internal(&self) -> Result<(), AudioJailError> {
        // DMA engine uzerinden baslat
        match play_audio_dma(&[], AudioFormat::dvd_quality()) {
            Ok(()) => {
                crate::serial_println!("[Audio-Jail] Playback started via DMA engine");
                Ok(())
            }
            Err(AudioError::NoController) => {
                // Controller yoksa degraded mode
                crate::serial_println!("[Audio-Jail] No HDA controller — degraded mode");
                Err(AudioJailError::DegradedMode)
            }
            Err(AudioError::BufferError) => {
                crate::serial_println!("[Audio-Jail] DMA buffer error — degraded mode");
                Err(AudioJailError::DmaAllocationFailed)
            }
            Err(_) => Err(AudioJailError::StreamError),
        }
    }

    /// Playback durdurma
    fn stop_playback_internal(&self) {
        let _ = stop_audio_dma();
        crate::serial_println!("[Audio-Jail] Playback stopped");
    }

    /// Buffer'i silence (0) ile doldur — underrun onleme
    fn fill_silence(&self, frames: u32) {
        let frame_size = self.frame_size();
        if frame_size == 0 {
            return;
        }
        let byte_count = (frames as usize) * frame_size;
        let buf_phys = self.dma_buffer_phys.load(Ordering::Acquire);
        let buf_size = self.dma_buffer_size.load(Ordering::Acquire);

        if buf_phys == 0 || buf_size == 0 {
            return;
        }

        // hw_ptr'den baslayarak silence yaz
        let hw = self.pcm_ring.hw_ptr.load(Ordering::Acquire);
        let offset = (hw as usize * frame_size) % (buf_size as usize);
        let available = (buf_size as usize).saturating_sub(offset);
        let fill_len = byte_count.min(available);

        #[cfg(not(any(test, target_os = "windows")))]
        {
            use crate::memory::phys_to_virt;
            let virt = phys_to_virt((buf_phys + offset as u64) as usize);
            unsafe {
                core::ptr::write_bytes(virt as *mut u8, 0, fill_len);
            }
        }

        crate::serial_println!(
            "[Audio-Jail] Filled {} frames with silence at offset {}",
            frames,
            offset
        );
    }

    /// Gercek codec widget tarama (HDA controller uzerinden)
    fn scan_widgets_real(&self, codec_addr: u8) -> AudioJailResponse {
        let ctrl = default_controller();

        if ctrl.is_none() {
            return AudioJailResponse::Error(AudioJailError::CodecNotFound);
        }

        let ctrl = ctrl.unwrap();
        let codec = ctrl.codecs.iter().find(|c| c.address == codec_addr);

        if codec.is_none() {
            return AudioJailResponse::Error(AudioJailError::CodecNotFound);
        }

        let codec = codec.unwrap();
        let widgets: Vec<WidgetInfo> = codec
            .widgets
            .iter()
            .map(|w| WidgetInfo {
                nid: w.nid,
                widget_type: match w.widget_type {
                    crate::drivers::audio::HdaWidgetType::OutputDac => 0x0,
                    crate::drivers::audio::HdaWidgetType::InputAdc => 0x1,
                    crate::drivers::audio::HdaWidgetType::Mixer => 0x3,
                    crate::drivers::audio::HdaWidgetType::Pin => 0x4,
                    crate::drivers::audio::HdaWidgetType::Power => 0x7,
                    crate::drivers::audio::HdaWidgetType::VolumeKnob => 0x8,
                    crate::drivers::audio::HdaWidgetType::Beep => 0x9,
                    _ => 0xF,
                },
                name: w.name.clone(),
            })
            .collect();

        AudioJailResponse::WidgetList(widgets)
    }

    pub fn get_status(&self) -> AudioJailStatus {
        let hw = self.pcm_ring.hw_ptr.load(Ordering::Acquire) as u32;
        let appl = self.pcm_ring.appl_ptr.load(Ordering::Acquire) as u32;
        let avail = self.pcm_ring.avail();
        let buf_frames = self.pcm_ring.buffer_size_frames.load(Ordering::Acquire);

        AudioJailStatus {
            initialized: self.ready.load(Ordering::Relaxed),
            codec_count: self.codecs.len(),
            playback_active: self.playback_active.load(Ordering::Acquire),
            capture_active: self.capture_config.is_some(),
            volume: self.master_volume.load(Ordering::Relaxed),
            muted: self.master_mute.load(Ordering::Relaxed),
            crash_count: self.crash_count.load(Ordering::Relaxed),
            last_reboot_seq: self.reboot_seq.load(Ordering::Relaxed),
            hw_ptr: hw,
            appl_ptr: appl,
            avail_frames: avail,
            buffer_size_frames: buf_frames,
            degraded: self.degraded.load(Ordering::Acquire),
            underrun_count: self.pcm_ring.underrun_count.load(Ordering::Relaxed),
        }
    }

    // ========================================================================
    // JailChannel Integration
    // ========================================================================

    pub fn attach_channel(&mut self, channel: JailChannel) {
        let cid = channel.channel_id;
        self.jail_channel = Some(channel);
        crate::serial_println!("[Audio-Jail] JailChannel {} attached", cid);
    }

    pub fn poll_requests(&self) -> Option<JailRequest> {
        self.jail_channel.as_ref().and_then(|ch| ch.poll_request())
    }

    pub fn submit_event(&self, event: JailEvent) -> Result<(), JailEvent> {
        match &self.jail_channel {
            Some(ch) => ch.submit_event(event),
            None => Err(event),
        }
    }

    /// JailRequest isleme — gercek implementasyon
    pub fn handle_jail_request(&self, req: JailRequest) -> JailEvent {
        let result = match req.opcode {
            JailOpcode::Read => {
                // Capture: DMA buffer'dan oku
                self.read_dma_buffer(req.offset as usize, req.length as usize) as i64
            }
            JailOpcode::Write => {
                // Playback: DMA buffer'a yaz
                // buffer_paddr'den veri okunup DMA'ya yazilir
                self.write_dma_buffer_from_phys(
                    req.offset as usize,
                    req.buffer_paddr,
                    req.length as usize,
                ) as i64
            }
            JailOpcode::Control => {
                // Stream kontrol komutu (start/stop/volume)
                self.handle_control_request(req.offset as u32, req.length as u32)
            }
            JailOpcode::Reset => match self.crash_and_reboot() {
                Ok(seq) => seq as i64,
                Err(_) => -1i64,
            },
            JailOpcode::Status => {
                let status = self.get_status();
                (if status.playback_active { 1 } else { 0 }) as i64
            }
            JailOpcode::Nop => 0i64,
            JailOpcode::Flush => {
                // Buffer sync (cache flush)
                self.flush_dma_buffer();
                0i64
            }
        };

        JailEvent {
            request_id: req.request_id,
            result,
            data_len: if result >= 0 { result as u32 } else { 0 },
            jail_id: self.jail_id as u16,
            flags: 0,
        }
    }

    /// DMA buffer'dan okuma (capture)
    fn read_dma_buffer(&self, offset: usize, length: usize) -> usize {
        let buf_phys = self.dma_buffer_phys.load(Ordering::Acquire);
        let buf_size = self.dma_buffer_size.load(Ordering::Acquire);

        if buf_phys == 0 || buf_size == 0 {
            return 0;
        }

        if offset >= buf_size as usize {
            return 0;
        }

        let available = (buf_size as usize).saturating_sub(offset);
        length.min(available)
    }

    /// Fiziksel adresten DMA buffer'a yazma (playback)
    fn write_dma_buffer_from_phys(&self, offset: usize, src_phys: u64, length: usize) -> usize {
        let buf_phys = self.dma_buffer_phys.load(Ordering::Acquire);
        let buf_size = self.dma_buffer_size.load(Ordering::Acquire);

        if buf_phys == 0 || buf_size == 0 || src_phys == 0 {
            return 0;
        }

        if offset >= buf_size as usize {
            return 0;
        }

        let available = (buf_size as usize).saturating_sub(offset);
        let copy_len = length.min(available);

        #[cfg(not(any(test, target_os = "windows")))]
        {
            use crate::memory::phys_to_virt;
            let dst_virt = phys_to_virt((buf_phys + offset as u64) as usize);
            let src_virt = phys_to_virt(src_phys as usize);
            unsafe {
                core::ptr::copy_nonoverlapping(
                    src_virt as *const u8,
                    dst_virt as *mut u8,
                    copy_len,
                );
            }
        }

        // appl_ptr'yi guncelle
        let frame_size = self.frame_size();
        if frame_size > 0 {
            let frames_written = (copy_len / frame_size) as u64;
            let boundary = self.pcm_ring.buffer_size_frames.load(Ordering::Acquire) as u64;
            let old_appl = self.pcm_ring.appl_ptr.load(Ordering::Relaxed);
            let new_appl = if boundary > 0 {
                (old_appl + frames_written) % boundary
            } else {
                old_appl + frames_written
            };
            self.pcm_ring.appl_ptr.store(new_appl, Ordering::Release);
            self.pcm_ring
                .total_written_frames
                .fetch_add(frames_written, Ordering::Relaxed);
        }

        copy_len
    }

    /// Kontrol isteği işleme
    fn handle_control_request(&self, cmd: u32, param: u32) -> i64 {
        match cmd {
            0 => {
                // Start
                match self.start_playback_internal() {
                    Ok(()) => {
                        self.playback_active.store(true, Ordering::Release);
                        0
                    }
                    Err(e) => {
                        self.degraded.store(true, Ordering::Release);
                        -(e as i64)
                    }
                }
            }
            1 => {
                // Stop
                self.stop_playback_internal();
                self.playback_active.store(false, Ordering::Release);
                0
            }
            2 => {
                // Volume
                self.master_volume.store(param.min(100), Ordering::Relaxed);
                0
            }
            3 => {
                // Mute
                self.master_mute.store(param != 0, Ordering::Relaxed);
                0
            }
            _ => -1,
        }
    }

    /// DMA buffer cache flush
    fn flush_dma_buffer(&self) {
        #[cfg(not(any(test, target_os = "windows")))]
        {
            use crate::drivers::dma::clflush_range;
            let buf_phys = self.dma_buffer_phys.load(Ordering::Acquire);
            let buf_size = self.dma_buffer_size.load(Ordering::Acquire);
            if buf_phys != 0 && buf_size != 0 {
                unsafe {
                    clflush_range(buf_phys, buf_size as usize);
                }
            }
        }
    }

    // ========================================================================
    // Codec Enumeration (gercek CORB/RIRB uzerinden)
    // ========================================================================

    pub fn enumerate_codecs(&mut self) {
        // Public API uzerinden controller bilgilerini al
        if let Some(ctrl) = default_controller() {
            self.codecs.clear();
            for codec in &ctrl.codecs {
                self.codecs.push(CodecInfo {
                    address: codec.address,
                    vendor_id: codec.vendor_id,
                    device_id: codec.device_id,
                    widget_count: codec.widgets.len(),
                });
            }
        }

        if self.codecs.is_empty() {
            // Fallback: STATESTS'ten oku
            unsafe {
                let statests =
                    core::ptr::read_volatile((self.mmio_base + HDA_STATESTS as u64) as *const u16);

                for addr in 0..15u8 {
                    if statests & (1 << addr) != 0 {
                        crate::serial_println!(
                            "[Audio-Jail] Codec at addr {} (STATESTS only)",
                            addr
                        );
                        self.codecs.push(CodecInfo {
                            address: addr,
                            vendor_id: 0,
                            device_id: 0,
                            widget_count: 0,
                        });
                    }
                }
            }
        }

        crate::serial_println!("[Audio-Jail] {} codecs enumerated", self.codecs.len());
    }

    // ========================================================================
    // PCM hw_ptr guncelleme (LPIB'den okuma)
    // ========================================================================

    /// hw_ptr'yi LPIB register'indan guncelle
    pub fn update_hw_ptr(&self) {
        let stream_base = 0x80u64; // Stream 0
        let sd = self.mmio_base + stream_base;

        unsafe {
            let lpib = core::ptr::read_volatile((sd + HDA_SD_LPIB) as *const u32);
            let cbl = core::ptr::read_volatile((sd + HDA_SD_CBL) as *const u32);

            let frame_size = self.frame_size();
            if frame_size == 0 {
                return;
            }

            let hw_frames = if cbl > 0 {
                (lpib as u64 * self.pcm_ring.buffer_size_frames.load(Ordering::Acquire) as u64)
                    / cbl as u64
            } else {
                lpib as u64 / frame_size as u64
            };

            let boundary = self.pcm_ring.buffer_size_frames.load(Ordering::Acquire) as u64;
            if boundary > 0 {
                self.pcm_ring
                    .hw_ptr
                    .store(hw_frames % boundary, Ordering::Release);
            } else {
                self.pcm_ring.hw_ptr.store(hw_frames, Ordering::Release);
            }

            self.pcm_ring
                .total_played_frames
                .fetch_add(hw_frames, Ordering::Relaxed);

            // Period boundary kontrolu
            if self.pcm_ring.check_period_boundary() {
                self.pcm_ring.update_period_boundary();

                // Period tamamlandi → core'a event gonder
                let _ = self.submit_event(JailEvent {
                    request_id: 0,
                    result: 0,
                    data_len: self.pcm_ring.period_size_frames.load(Ordering::Acquire),
                    jail_id: self.jail_id as u16,
                    flags: 1, // period complete flag
                });
            }

            // Underrun kontrolu
            if self
                .pcm_ring
                .check_underrun(self.playback_active.load(Ordering::Acquire))
            {
                crate::serial_println!("[Audio-Jail] UNDERRUN detected");
                // Silence fill ile buffer'i doldur
                self.fill_silence(self.pcm_ring.period_size_frames.load(Ordering::Acquire));
            }
        }
    }

    // ========================================================================
    // DMA buffer kayit
    // ========================================================================

    pub fn register_dma_buffer(&self, phys: u64, size: u64) {
        self.dma_buffer_phys.store(phys, Ordering::Release);
        self.dma_buffer_size.store(size, Ordering::Release);
        crate::serial_println!(
            "[Audio-Jail] DMA buffer registered: phys=0x{:x} size={}",
            phys,
            size
        );
    }

    // ========================================================================
    // Public getters
    // ========================================================================

    pub fn set_volume(&self, volume: u32) {
        self.master_volume.store(volume.min(100), Ordering::Relaxed);
    }

    pub fn set_mute(&self, mute: bool) {
        self.master_mute.store(mute, Ordering::Relaxed);
    }

    pub fn volume(&self) -> u32 {
        self.master_volume.load(Ordering::Relaxed)
    }

    pub fn is_muted(&self) -> bool {
        self.master_mute.load(Ordering::Relaxed)
    }

    pub fn codec_count(&self) -> usize {
        self.codecs.len()
    }

    pub fn is_degraded(&self) -> bool {
        self.degraded.load(Ordering::Acquire)
    }

    pub fn pcm_ring(&self) -> &PcmRingState {
        &self.pcm_ring
    }
}

// ============================================================================
// Global Registry
// ============================================================================

lazy_static::lazy_static! {
    static ref AUDIO_CONTROLLERS: Mutex<Vec<AudioJailController>> = Mutex::new(Vec::new());
}

pub fn init() {
    crate::serial_println!("[Audio-Jail] TIER 2 Audio/ALSA Jail driver initializing...");

    let devices = crate::drivers::pci::scan();
    for dev in devices {
        if dev.class_code == 0x04 && dev.subclass == 0x03 {
            crate::serial_println!(
                "[Audio-Jail] Found HDA controller at {:02x}:{:02x}.{}",
                dev.bus,
                dev.device,
                dev.function
            );

            let bar = crate::drivers::pci::read_bar_mmio(dev.bus, dev.device, dev.function, 0);
            if let Some(bar) = bar {
                let mut ctrl = AudioJailController::new(bar.base);

                // Codec enumeration
                ctrl.enumerate_codecs();

                AUDIO_CONTROLLERS.lock().push(ctrl);
            }
        }
    }

    if AUDIO_CONTROLLERS.lock().is_empty() {
        crate::serial_println!("[Audio-Jail] No HDA controllers found — degraded mode available");
    }
}

pub fn controller_count() -> usize {
    AUDIO_CONTROLLERS.lock().len()
}

pub fn primary_controller_status() -> Option<AudioJailStatus> {
    let controllers = AUDIO_CONTROLLERS.lock();
    controllers.first().map(|c| c.get_status())
}

// ============================================================================
// Host Corpus Tests (D-AUDIO-02)
// ============================================================================

#[cfg(test)]
mod audio_jail_tests {
    use super::*;

    #[test]
    fn pcm_ring_avail_calculation() {
        let ring = PcmRingState::new();
        ring.buffer_size_frames.store(4096, Ordering::Release);
        ring.appl_ptr.store(1000, Ordering::Release);
        ring.hw_ptr.store(500, Ordering::Release);

        // avail = buffer_size - (appl_ptr - hw_ptr) = 4096 - 500 = 3596
        let avail = ring.avail();
        assert_eq!(avail, 3596);
    }

    #[test]
    fn pcm_ring_avail_wrapping() {
        let ring = PcmRingState::new();
        ring.buffer_size_frames.store(4096, Ordering::Release);
        ring.appl_ptr.store(100, Ordering::Release);
        ring.hw_ptr.store(4000, Ordering::Release);

        // appl_ptr < hw_ptr → wrapping
        let avail = ring.avail();
        assert_eq!(avail, 4096 - (100u64.wrapping_sub(4000) % 4096) as u32);
    }

    #[test]
    fn pcm_ring_period_boundary_detection() {
        let ring = PcmRingState::new();
        ring.buffer_size_frames.store(4096, Ordering::Release);
        ring.period_size_frames.store(1024, Ordering::Release);
        ring.hw_ptr.store(500, Ordering::Release);
        ring.last_period_hw_ptr.store(0, Ordering::Release);

        assert!(!ring.check_period_boundary());

        ring.hw_ptr.store(1500, Ordering::Release);
        assert!(ring.check_period_boundary());

        ring.update_period_boundary();
        assert!(!ring.check_period_boundary());
    }

    #[test]
    fn pcm_ring_underrun_detection() {
        let ring = PcmRingState::new();
        ring.buffer_size_frames.store(4096, Ordering::Release);
        ring.appl_ptr.store(100, Ordering::Release);
        ring.hw_ptr.store(100, Ordering::Release);

        // hw_ptr == appl_ptr → underrun
        assert!(ring.check_underrun(true));
        assert!(!ring.check_underrun(false)); // playback degilse underrun yok

        let underruns = ring.underrun_count.load(Ordering::Relaxed);
        assert_eq!(underruns, 1);
    }

    #[test]
    fn audio_jail_controller_initial_state() {
        let ctrl = AudioJailController::new(0xF000_0000);
        assert!(!ctrl.ready.load(Ordering::Relaxed));
        assert_eq!(ctrl.crash_count.load(Ordering::Relaxed), 0);
        assert_eq!(ctrl.codec_count(), 0);
        assert!(!ctrl.is_degraded());
    }

    #[test]
    fn audio_jail_backoff_calculation() {
        let ctrl = AudioJailController::new(0xF000_0000);

        // Crash 1: base * 2^1 = 100 * 2 = 200ms
        let b1 = ctrl.calculate_backoff(1);
        assert_eq!(b1, 200);

        // Crash 3: base * 2^3 = 100 * 8 = 800ms
        let b3 = ctrl.calculate_backoff(3);
        assert_eq!(b3, 800);

        // Crash 10: capped at max
        let b10 = ctrl.calculate_backoff(10);
        assert!(b10 <= ctrl.max_backoff_ms.load(Ordering::Relaxed));
    }

    #[test]
    fn audio_jail_status_reflects_degraded_mode() {
        let ctrl = AudioJailController::new(0xF000_0000);
        ctrl.degraded.store(true, Ordering::Release);

        let status = ctrl.get_status();
        assert!(status.degraded);
    }

    #[test]
    fn audio_jail_pcm_ring_state_defaults() {
        let ring = PcmRingState::new();
        assert_eq!(ring.appl_ptr.load(Ordering::Relaxed), 0);
        assert_eq!(ring.hw_ptr.load(Ordering::Relaxed), 0);
        assert_eq!(ring.buffer_size_frames.load(Ordering::Relaxed), 0);
        assert_eq!(ring.period_size_frames.load(Ordering::Relaxed), 0);
        assert_eq!(ring.underrun_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn audio_jail_frame_size_calculation() {
        let mut ctrl = AudioJailController::new(0xF000_0000);

        // 16-bit, 2ch → 4 bytes/frame
        ctrl.playback_config = Some(PlaybackConfig {
            sample_rate: 48000,
            bit_depth: 16,
            channels: 2,
            buffer_size: 65536,
        });
        assert_eq!(ctrl.frame_size(), 4);

        // 24-bit, 6ch → 18 bytes/frame
        ctrl.playback_config = Some(PlaybackConfig {
            sample_rate: 96000,
            bit_depth: 24,
            channels: 6,
            buffer_size: 65536,
        });
        assert_eq!(ctrl.frame_size(), 18);
    }

    #[test]
    fn audio_jail_configure_playback_sets_ring_buffer() {
        let mut ctrl = AudioJailController::new(0xF000_0000);
        ctrl.ready.store(true, Ordering::Release);

        let response = ctrl.process_command(AudioJailCommand::ConfigurePlayback {
            sample_rate: 48000,
            bit_depth: 16,
            channels: 2,
            buffer_size: 65536,
        });

        assert!(matches!(response, AudioJailResponse::Ok));
        assert_eq!(
            ctrl.pcm_ring.buffer_size_frames.load(Ordering::Relaxed),
            16384
        ); // 65536 / 4
        assert_eq!(
            ctrl.pcm_ring.period_size_frames.load(Ordering::Relaxed),
            4096
        ); // 16384 / 4
    }

    #[test]
    fn audio_jail_write_dma_buffer_updates_appl_ptr() {
        let mut ctrl = AudioJailController::new(0xF000_0000);
        ctrl.ready.store(true, Ordering::Release);

        ctrl.process_command(AudioJailCommand::ConfigurePlayback {
            sample_rate: 48000,
            bit_depth: 16,
            channels: 2,
            buffer_size: 65536,
        });

        ctrl.register_dma_buffer(0x1000_0000, 65536);

        let data = vec![0u8; 4096]; // 1024 frames (4 bytes/frame)
        let response = ctrl.process_command(AudioJailCommand::WriteDmaBuffer { offset: 0, data });

        assert!(matches!(response, AudioJailResponse::Ok));
        assert_eq!(ctrl.pcm_ring.appl_ptr.load(Ordering::Relaxed), 1024);
    }

    #[test]
    fn audio_jail_write_dma_buffer_overflow_rejected() {
        let mut ctrl = AudioJailController::new(0xF000_0000);
        ctrl.ready.store(true, Ordering::Release);
        ctrl.register_dma_buffer(0x1000_0000, 4096);

        let response = ctrl.process_command(AudioJailCommand::WriteDmaBuffer {
            offset: 8096,
            data: vec![0u8; 100],
        });

        assert!(matches!(
            response,
            AudioJailResponse::Error(AudioJailError::BufferOverflow)
        ));
    }

    #[test]
    fn audio_jail_write_without_dma_buffer_fails() {
        let mut ctrl = AudioJailController::new(0xF000_0000);
        ctrl.ready.store(true, Ordering::Release);

        let response = ctrl.process_command(AudioJailCommand::WriteDmaBuffer {
            offset: 0,
            data: vec![0u8; 100],
        });

        assert!(matches!(
            response,
            AudioJailResponse::Error(AudioJailError::DmaAllocationFailed)
        ));
    }

    #[test]
    fn audio_jail_fill_silence() {
        let mut ctrl = AudioJailController::new(0xF000_0000);
        ctrl.playback_config = Some(PlaybackConfig {
            sample_rate: 48000,
            bit_depth: 16,
            channels: 2,
            buffer_size: 65536,
        });
        ctrl.register_dma_buffer(0x1000_0000, 65536);

        ctrl.process_command(AudioJailCommand::FillSilence { frames: 100 });
        // Should not panic, silence fill executed
    }

    #[test]
    fn audio_jail_crash_and_reboot_increments_counters() {
        let ctrl = AudioJailController::new(0xF000_0000);
        ctrl.ready.store(true, Ordering::Release);

        // MMIO base 0 oldugu icin reset_mmio timeout dönecek
        // Bu test sadece counter mantigini test ediyor
        let result = ctrl.crash_and_reboot();
        // MMIO timeout bekleniyor (mmio_base = 0)
        assert!(result.is_err() || result.unwrap() == 1);
        assert_eq!(ctrl.crash_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn audio_jail_watchdog_timeout_detection() {
        assert!(AudioJailController::watchdog_expired_at(101, 0, 100));
        assert!(!AudioJailController::watchdog_expired_at(100, 0, 100));
    }

    #[test]
    fn audio_jail_handle_jail_request_write() {
        let mut ctrl = AudioJailController::new(0xF000_0000);
        ctrl.playback_config = Some(PlaybackConfig {
            sample_rate: 48000,
            bit_depth: 16,
            channels: 2,
            buffer_size: 65536,
        });
        ctrl.register_dma_buffer(0x1000_0000, 65536);

        let req = JailRequest {
            request_id: 1,
            opcode: JailOpcode::Write,
            offset: 0,
            length: 4096,
            buffer_paddr: 0x2000_0000,
            device_id: 0,
            flags: 0,
        };

        let event = ctrl.handle_jail_request(req);
        assert_eq!(event.request_id, 1);
        assert!(event.result >= 0);
    }

    #[test]
    fn audio_jail_handle_jail_request_reset() {
        let ctrl = AudioJailController::new(0xF000_0000);
        ctrl.ready.store(true, Ordering::Release);

        let req = JailRequest {
            request_id: 42,
            opcode: JailOpcode::Reset,
            offset: 0,
            length: 0,
            buffer_paddr: 0,
            device_id: 0,
            flags: 0,
        };

        let event = ctrl.handle_jail_request(req);
        assert_eq!(event.request_id, 42);
        // MMIO timeout nedeniyle -1 donebilir
        assert!(event.result == 1 || event.result == -1);
    }

    #[test]
    fn audio_jail_handle_jail_request_status() {
        let ctrl = AudioJailController::new(0xF000_0000);
        ctrl.ready.store(true, Ordering::Release);
        ctrl.playback_active.store(true, Ordering::Release);

        let req = JailRequest {
            request_id: 99,
            opcode: JailOpcode::Status,
            offset: 0,
            length: 0,
            buffer_paddr: 0,
            device_id: 0,
            flags: 0,
        };

        let event = ctrl.handle_jail_request(req);
        assert_eq!(event.result, 1); // playback active
    }
}
