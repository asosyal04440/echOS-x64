//! # GPU Native Driver — TIER 1 Lock-Free GPU Sürücüsü
//!
//! GPU donanımı TIER 1 sürücü olarak doğrudan çekirdek alanında çalışır.
//! AsyncGpuDevice trait'ini implemente eder, DMA-tabanlı framebuffer yönetimi sağlar.
//!
//! ## Mimari
//!
//! ```text
//! ┌─────────────┐  DMA Blit  ┌──────────────┐  PCIe BAR   ┌──────────┐
//! │ Compositor/  │──────────►│ GPU Native   │────────────►│ GPU HW   │
//! │ Wayland/DRM  │           │ (Tier 1)     │  MMIO/VRAM  │ (PCIe)   │
//! └─────────────┘            │ Lock-free    │             └──────────┘
//!                            └──────────────┘
//! ```
//!
//! ## Özellikler
//!
//! - PCIe BAR memory-mapped VRAM erişimi
//! - 2D blit engine (framebuffer → CRTC)
//! - Cursor overlay
//! - Page flip (vsync-aligned)
//! - Resolution/mode setting
//! - DMA buffer management
//! - AsyncGpuDevice trait implementasyonu

use crate::drivers::async_traits::{
    AsyncGpuDevice, AsyncIoError, CompletionEvent, DmaBuffer, SubmissionToken,
};
use crate::drivers::drm::VBlankEvent;
use crate::drivers::pci::PciDevice;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// GPU Register Offsets (Generic VGA/GPU)
// ============================================================================

/// Display control registers
const GPU_REG_STATUS: usize = 0x00;
const GPU_REG_CONTROL: usize = 0x04;
const GPU_REG_WIDTH: usize = 0x08;
const GPU_REG_HEIGHT: usize = 0x0C;
const GPU_REG_STRIDE: usize = 0x10;
const GPU_REG_FORMAT: usize = 0x14;
const GPU_REG_FB_ADDR: usize = 0x18;
const GPU_REG_CURSOR_X: usize = 0x20;
const GPU_REG_CURSOR_Y: usize = 0x24;
const GPU_REG_CURSOR_CTRL: usize = 0x28;
const GPU_REG_VBLANK_SEQ: usize = 0x2C;

const GPU_STATUS_VBLANK_PENDING: u32 = 1 << 0;
const GPU_CONTROL_VBLANK_ENABLE: u32 = 1 << 8;

/// 2D Engine registers
const GPU_2D_CMD: usize = 0x100;
const GPU_2D_SRC_ADDR: usize = 0x104;
const GPU_2D_DST_ADDR: usize = 0x108;
const GPU_2D_SRC_STRIDE: usize = 0x10C;
const GPU_2D_DST_STRIDE: usize = 0x110;
const GPU_2D_WIDTH: usize = 0x114;
const GPU_2D_HEIGHT: usize = 0x118;
const GPU_2D_STATUS: usize = 0x11C;

/// 2D engine komutları
const CMD_2D_BLIT: u32 = 1;
const CMD_2D_FILL: u32 = 2;
const CMD_2D_COPY: u32 = 3;

const PCI_VENDOR_INTEL: u16 = 0x8086;
const PCI_VENDOR_AMD: u16 = 0x1002;
const PCI_VENDOR_NVIDIA: u16 = 0x10DE;

/// Pixel formatları
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Argb8888,
    Xrgb8888,
    Rgb565,
    Rgb888,
}

impl PixelFormat {
    pub fn bytes_per_pixel(&self) -> u32 {
        match self {
            Self::Argb8888 | Self::Xrgb8888 => 4,
            Self::Rgb888 => 3,
            Self::Rgb565 => 2,
        }
    }

    pub fn to_hw_format(&self) -> u32 {
        match self {
            Self::Argb8888 => 0,
            Self::Xrgb8888 => 1,
            Self::Rgb565 => 2,
            Self::Rgb888 => 3,
        }
    }
}

/// Display mode (çözünürlük + refresh rate)
#[derive(Clone, Debug)]
pub struct DisplayMode {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
    pub pixel_format: PixelFormat,
}

impl DisplayMode {
    pub fn framebuffer_size(&self) -> usize {
        (self.width * self.height * self.pixel_format.bytes_per_pixel()) as usize
    }
}

// ============================================================================
// GPU Native Controller
// ============================================================================

/// TIER 1 GPU native sürücü yapısı
pub struct GpuNativeDevice {
    /// Cihaz ismi
    name: String,
    /// MMIO base address (PCIe BAR0)
    mmio_base: u64,
    /// VRAM base address (PCIe BAR2 veya BAR0 offset)
    vram_base: u64,
    /// VRAM boyutu
    vram_size: u64,
    /// Aktif display mode
    mode: DisplayMode,
    /// Framebuffer physical address
    fb_phys: u64,
    /// Cursor görünür mü?
    cursor_visible: AtomicBool,
    /// Cursor X konumu
    cursor_x: AtomicU32,
    /// Cursor Y konumu
    cursor_y: AtomicU32,
    /// Pending completion sayacı
    pending_completions: AtomicU32,
    /// Son completion token
    last_completion: AtomicU64,
    /// Cihaz hazır mı?
    ready: AtomicBool,
    /// VSync counter
    vsync_count: AtomicU64,
    /// PCI bus/device/function
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

impl GpuNativeDevice {
    /// Yeni GPU native device oluşturur
    pub fn new(name: &str, mmio_base: u64, vram_base: u64, vram_size: u64) -> Self {
        Self {
            name: String::from(name),
            mmio_base,
            vram_base,
            vram_size,
            mode: DisplayMode {
                width: 1920,
                height: 1080,
                refresh_hz: 60,
                pixel_format: PixelFormat::Xrgb8888,
            },
            fb_phys: vram_base,
            cursor_visible: AtomicBool::new(false),
            cursor_x: AtomicU32::new(0),
            cursor_y: AtomicU32::new(0),
            pending_completions: AtomicU32::new(0),
            last_completion: AtomicU64::new(0),
            ready: AtomicBool::new(false),
            vsync_count: AtomicU64::new(0),
            bus: 0,
            device: 0,
            function: 0,
        }
    }

    #[inline(always)]
    fn write_reg32(&self, offset: usize, value: u32) {
        unsafe {
            core::ptr::write_volatile((self.mmio_base + offset as u64) as *mut u32, value);
        }
    }

    #[inline(always)]
    fn read_reg32(&self, offset: usize) -> u32 {
        unsafe { core::ptr::read_volatile((self.mmio_base + offset as u64) as *const u32) }
    }

    /// GPU donanımını başlatır
    pub fn init(&self) -> Result<(), &'static str> {
        self.write_reg32(GPU_REG_WIDTH, self.mode.width);
        self.write_reg32(GPU_REG_HEIGHT, self.mode.height);
        self.write_reg32(
            GPU_REG_STRIDE,
            self.mode.width * self.mode.pixel_format.bytes_per_pixel(),
        );
        self.write_reg32(GPU_REG_FORMAT, self.mode.pixel_format.to_hw_format());
        self.write_reg32(GPU_REG_FB_ADDR, self.fb_phys as u32);
        self.write_reg32(GPU_REG_CONTROL, 1);

        self.ready.store(true, Ordering::Release);

        crate::serial_println!(
            "[GPU-Native] Initialized: {}x{} @ {}Hz, VRAM={}MB",
            self.mode.width,
            self.mode.height,
            self.mode.refresh_hz,
            self.vram_size / (1024 * 1024)
        );

        Ok(())
    }

    /// 2D blit engine ile dikdörtgen kopyalar
    pub fn hw_blit(
        &self,
        src_phys: u64,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
    ) -> Result<(), &'static str> {
        if !self.ready.load(Ordering::Acquire) {
            return Err("GPU not ready");
        }

        let stride = self.mode.width * self.mode.pixel_format.bytes_per_pixel();
        let dst_offset = (y * stride + x * self.mode.pixel_format.bytes_per_pixel()) as u64;
        let dst_phys = self.fb_phys + dst_offset;

        self.write_reg32(GPU_2D_SRC_ADDR, src_phys as u32);
        self.write_reg32(GPU_2D_DST_ADDR, dst_phys as u32);
        self.write_reg32(
            GPU_2D_SRC_STRIDE,
            w * self.mode.pixel_format.bytes_per_pixel(),
        );
        self.write_reg32(GPU_2D_DST_STRIDE, stride);
        self.write_reg32(GPU_2D_WIDTH, w);
        self.write_reg32(GPU_2D_HEIGHT, h);
        self.write_reg32(GPU_2D_CMD, CMD_2D_BLIT);

        // Blit tamamlanmasını bekle
        for _ in 0..10000 {
            let status = self.read_reg32(GPU_2D_STATUS);
            if status & 1 == 0 {
                break;
            } // idle
        }

        Ok(())
    }

    /// Çözünürlük değiştirir
    pub fn set_mode(&mut self, width: u32, height: u32, refresh: u32) {
        self.mode = DisplayMode {
            width,
            height,
            refresh_hz: refresh,
            pixel_format: PixelFormat::Xrgb8888,
        };

        crate::serial_println!(
            "[GPU-Native] Mode changed: {}x{} @ {}Hz",
            width,
            height,
            refresh
        );
    }

    pub fn enable_vblank_irq(&self, enabled: bool) {
        let mut control = self.read_reg32(GPU_REG_CONTROL);
        if enabled {
            control |= GPU_CONTROL_VBLANK_ENABLE;
        } else {
            control &= !GPU_CONTROL_VBLANK_ENABLE;
        }
        self.write_reg32(GPU_REG_CONTROL, control);
    }

    fn ack_vblank_irq(&self) {
        let status = self.read_reg32(GPU_REG_STATUS);
        self.write_reg32(GPU_REG_STATUS, status & !GPU_STATUS_VBLANK_PENDING);
    }

    /// VSync interrupt handler
    pub fn handle_vsync(&self) -> VBlankEvent {
        let seq = self.vsync_count.fetch_add(1, Ordering::AcqRel) + 1;
        let timestamp_ns = crate::cpu::tsc::read_ns();
        self.write_reg32(GPU_REG_VBLANK_SEQ, seq as u32);
        VBlankEvent {
            seq,
            timestamp_ns,
            crtc_id: 0,
        }
    }

    /// Minimal ISR yolu: ack + event capture.
    pub fn handle_vblank_irq_minimal(&self) -> Option<VBlankEvent> {
        let status = self.read_reg32(GPU_REG_STATUS);
        if status & GPU_STATUS_VBLANK_PENDING == 0 {
            return None;
        }
        self.ack_vblank_irq();
        Some(self.handle_vsync())
    }

    /// VSync counter
    pub fn vsync_count(&self) -> u64 {
        self.vsync_count.load(Ordering::Relaxed)
    }
}

// ============================================================================
// AsyncGpuDevice Trait Implementation
// ============================================================================

impl AsyncGpuDevice for GpuNativeDevice {
    fn name(&self) -> &str {
        &self.name
    }

    fn vram_size(&self) -> u64 {
        self.vram_size
    }

    fn resolution(&self) -> (u32, u32) {
        (self.mode.width, self.mode.height)
    }

    fn submit_blit(
        &self,
        src_buf: &DmaBuffer,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    ) -> Result<SubmissionToken, AsyncIoError> {
        self.hw_blit(src_buf.paddr, x, y, width, height)
            .map_err(|_| AsyncIoError::DeviceError)?;

        let token = SubmissionToken::next();
        self.pending_completions.fetch_add(1, Ordering::Relaxed);
        self.last_completion.store(token.0, Ordering::Release);
        Ok(token)
    }

    fn submit_cursor_update(
        &self,
        x: u32,
        y: u32,
        visible: bool,
    ) -> Result<SubmissionToken, AsyncIoError> {
        self.cursor_x.store(x, Ordering::Relaxed);
        self.cursor_y.store(y, Ordering::Relaxed);
        self.cursor_visible.store(visible, Ordering::Relaxed);

        self.write_reg32(GPU_REG_CURSOR_X, x);
        self.write_reg32(GPU_REG_CURSOR_Y, y);
        self.write_reg32(GPU_REG_CURSOR_CTRL, if visible { 1 } else { 0 });

        let token = SubmissionToken::next();
        Ok(token)
    }

    fn submit_page_flip(&self, framebuffer: &DmaBuffer) -> Result<SubmissionToken, AsyncIoError> {
        self.write_reg32(GPU_REG_FB_ADDR, framebuffer.paddr as u32);

        let token = SubmissionToken::next();
        self.pending_completions.fetch_add(1, Ordering::Relaxed);
        self.last_completion.store(token.0, Ordering::Release);
        Ok(token)
    }

    fn poll_completion(&self) -> Option<CompletionEvent> {
        let pending = self.pending_completions.load(Ordering::Relaxed);
        if pending > 0 {
            self.pending_completions.fetch_sub(1, Ordering::Relaxed);
            let token = self.last_completion.load(Ordering::Acquire);
            Some(CompletionEvent {
                token: SubmissionToken(token),
                result: 0,
                data_len: 0,
                flags: 0,
            })
        } else {
            None
        }
    }
}

// ============================================================================
// Global GPU Registry
// ============================================================================

lazy_static::lazy_static! {
    /// Kayıtlı GPU native cihazları
    static ref GPU_DEVICES: Mutex<Vec<GpuNativeDevice>> = Mutex::new(Vec::new());
}

fn supports_native_scanout(dev: &PciDevice) -> bool {
    dev.class_code == 0x03
        && matches!(
            dev.vendor_id,
            PCI_VENDOR_INTEL | PCI_VENDOR_AMD | PCI_VENDOR_NVIDIA
        )
}

/// GPU native sürücüsünü başlatır
pub fn init() {
    crate::serial_println!("[GPU-Native] TIER 1 GPU native driver initialized");

    // PCI taraması — VGA uyumlu cihazlar (class=0x03)
    let devices = crate::drivers::pci::scan();
    for dev in devices {
        if dev.class_code == 0x03 {
            crate::serial_println!(
                "[GPU-Native] Found GPU at {:02x}:{:02x}.{} vendor={:04x} device={:04x} subclass=0x{:02x}",
                dev.bus,
                dev.device,
                dev.function,
                dev.vendor_id,
                dev.device_id,
                dev.subclass
            );

            if !supports_native_scanout(&dev) {
                crate::serial_println!(
                    "[GPU-Native] skipping native scanout for unsupported GPU family {:04x}:{:04x}",
                    dev.vendor_id,
                    dev.device_id
                );
                continue;
            }

            let bar = crate::drivers::pci::read_bar_mmio(dev.bus, dev.device, dev.function, 0);
            if let Some(bar) = bar {
                let mut gpu = GpuNativeDevice::new(
                    "gpu0", bar.base, bar.base, // VRAM often at BAR0 or BAR2
                    bar.size,
                );
                gpu.bus = dev.bus;
                gpu.device = dev.device;
                gpu.function = dev.function;
                if gpu.init().is_ok() {
                    gpu.enable_vblank_irq(true);
                    GPU_DEVICES.lock().push(gpu);
                } else {
                    crate::serial_println!("[GPU-Native] init failed for detected GPU");
                }
            }
        }
    }
}

/// Kayıtlı GPU sayısı
pub fn device_count() -> usize {
    GPU_DEVICES
        .lock()
        .iter()
        .filter(|gpu| gpu.ready.load(Ordering::Acquire))
        .count()
}

/// PCI/MSI IRQ yolundan Ã§aÄŸrÄ±labilecek minimal VBLANK event yardÄ±mcÄ±sÄ±.
pub fn blit_primary_region(src_paddr: u64, x: u32, y: u32, width: u32, height: u32) -> bool {
    if width == 0 || height == 0 {
        return true;
    }

    let devices = GPU_DEVICES.lock();
    let Some(device) = devices.iter().find(|gpu| gpu.ready.load(Ordering::Acquire)) else {
        return false;
    };

    device.hw_blit(src_paddr, x, y, width, height).is_ok()
}

pub fn dispatch_vblank_irq(device_index: usize) -> Option<VBlankEvent> {
    let devices = GPU_DEVICES.lock();
    let device = devices.get(device_index)?;
    device.handle_vblank_irq_minimal()
}
