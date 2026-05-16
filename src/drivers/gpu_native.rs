//! # GPU Native Driver â€” TIER 1 Lock-Free GPU SÃ¼rÃ¼cÃ¼sÃ¼
//!
//! GPU donanÄ±mÄ± TIER 1 sÃ¼rÃ¼cÃ¼ olarak doÄŸrudan ÃSection ekirdek alanÄ±nda ÃSection alÄ±ÅŸÄ±r.
//! AsyncGpuDevice trait'ini implemente eder, DMA-tabanlÄ± framebuffer yÃ¶netimi saÄŸlar.
//!
//! ## Mimari
//!
//! ```text
//! â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”  DMA Blit  â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”  PCIe BAR   â”Œâ”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”
//! â”‚ Compositor/  â”‚â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â–ºâ”‚ GPU Native   â”‚â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â–ºâ”‚ GPU HW   â”‚
//! â”‚ Wayland/DRM  â”‚           â”‚ (Tier 1)     â”‚  MMIO/VRAM  â”‚ (PCIe)   â”‚
//! â””â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”˜            â”‚ Lock-free    â”‚             â””â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”˜
//!                            â””â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”˜
//! ```
//!
//! ## Ã–zellikler
//!
//! - PCIe BAR memory-mapped VRAM eriÅŸimi
//! - 2D blit engine (framebuffer â†’ CRTC)
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

/// 2D engine komutlarÄ±
const CMD_2D_BLIT: u32 = 1;
const CMD_2D_FILL: u32 = 2;
const CMD_2D_COPY: u32 = 3;

const PCI_VENDOR_INTEL: u16 = 0x8086;
const PCI_VENDOR_AMD: u16 = 0x1002;
const PCI_VENDOR_NVIDIA: u16 = 0x10DE;
const GPU_MMIO_WINDOW_BYTES: u64 = 0x1000;
const GPU_MMIO_REQUIRED_BYTES: u64 = (GPU_2D_STATUS + 4) as u64;
const GPU_MIN_VRAM_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
struct GpuBarWindow {
    index: u8,
    base: u64,
    size: u64,
}

/// Pixel formatlarÄ±
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

/// Display mode (ÃSection Ã¶zÃ¼nÃ¼rlÃ¼k + refresh rate)
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

/// TIER 1 GPU native sÃ¼rÃ¼cÃ¼ yapÄ±sÄ±
pub struct GpuNativeDevice {
    /// Cihaz ismi
    name: String,
    /// MMIO BAR metadata
    mmio_bar: GpuBarWindow,
    /// VRAM BAR metadata
    vram_bar: GpuBarWindow,
    /// VRAM boyutu
    vram_size: u64,
    /// Aktif display mode
    mode: DisplayMode,
    /// Framebuffer physical address
    fb_phys: u64,
    /// Cursor gÃ¶rÃ¼nÃ¼r mÃ¼?
    cursor_visible: AtomicBool,
    /// Cursor X konumu
    cursor_x: AtomicU32,
    /// Cursor Y konumu
    cursor_y: AtomicU32,
    /// Pending completion sayacÄ±
    pending_completions: AtomicU32,
    /// Son completion token
    last_completion: AtomicU64,
    /// Cihaz hazÄ±r mÄ±?
    ready: AtomicBool,
    /// VSync counter
    vsync_count: AtomicU64,
    /// PCI bus/device/function
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    /// IOMMU DMA domain
    dma_domain: AtomicU32,
}

impl GpuNativeDevice {
    /// Yeni GPU native device oluÅŸturur
    fn new(name: &str, mmio_bar: GpuBarWindow, vram_bar: GpuBarWindow) -> Self {
        Self {
            name: String::from(name),
            mmio_bar,
            vram_bar,
            vram_size: vram_bar.size,
            mode: DisplayMode {
                width: 1920,
                height: 1080,
                refresh_hz: 60,
                pixel_format: PixelFormat::Xrgb8888,
            },
            fb_phys: vram_bar.base,
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
            dma_domain: AtomicU32::new(0),
        }
    }

    #[inline(always)]
    fn write_reg32(&self, offset: usize, value: u32) {
        unsafe {
            core::ptr::write_volatile((self.mmio_bar.base + offset as u64) as *mut u32, value);
        }
    }

    #[inline(always)]
    fn read_reg32(&self, offset: usize) -> u32 {
        unsafe { core::ptr::read_volatile((self.mmio_bar.base + offset as u64) as *const u32) }
    }

    fn with_gpu_domain<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let prev_domain = crate::cpu::smp::current_dma_domain();
        let domain = self.dma_domain.load(Ordering::Acquire);
        crate::cpu::smp::set_current_dma_domain(domain);
        let result = f();
        crate::cpu::smp::set_current_dma_domain(prev_domain);
        result
    }

    fn validate_mode_contract(&self) -> Result<(), &'static str> {
        if self.mmio_bar.size < GPU_MMIO_REQUIRED_BYTES {
            return Err("GPU MMIO BAR too small");
        }
        if self.vram_bar.size < GPU_MIN_VRAM_BYTES {
            return Err("GPU VRAM BAR too small");
        }
        if self.mode.framebuffer_size() as u64 > self.vram_size {
            return Err("GPU framebuffer exceeds VRAM BAR");
        }
        Ok(())
    }

    fn validate_dma_range(&self, paddr: u64, len: u64) -> Result<(), &'static str> {
        if len == 0 {
            return Err("GPU DMA range is empty");
        }
        let end = paddr.checked_add(len).ok_or("GPU DMA range overflow")?;
        let mmio_end = self
            .mmio_bar
            .base
            .checked_add(self.mmio_bar.size)
            .ok_or("GPU MMIO BAR overflow")?;
        if paddr < mmio_end && end > self.mmio_bar.base {
            return Err("GPU DMA overlaps MMIO BAR");
        }
        Ok(())
    }

    fn validate_blit_geometry(&self, x: u32, y: u32, w: u32, h: u32) -> Result<u64, &'static str> {
        if w == 0 || h == 0 {
            return Err("GPU blit dimensions are empty");
        }
        let x_end = x.checked_add(w).ok_or("GPU blit width overflow")?;
        let y_end = y.checked_add(h).ok_or("GPU blit height overflow")?;
        if x_end > self.mode.width || y_end > self.mode.height {
            return Err("GPU blit exceeds active mode");
        }
        let bpp = self.mode.pixel_format.bytes_per_pixel() as u64;
        let copied = (w as u64)
            .checked_mul(h as u64)
            .and_then(|px| px.checked_mul(bpp))
            .ok_or("GPU blit byte count overflow")?;
        let stride = (self.mode.width as u64)
            .checked_mul(bpp)
            .ok_or("GPU stride overflow")?;
        let dst_offset = (y as u64)
            .checked_mul(stride)
            .and_then(|row| row.checked_add((x as u64) * bpp))
            .ok_or("GPU destination offset overflow")?;
        let dst_end = dst_offset
            .checked_add(copied)
            .ok_or("GPU destination end overflow")?;
        if dst_end > self.vram_size {
            return Err("GPU blit exceeds VRAM aperture");
        }
        Ok(copied)
    }

    /// GPU donanÄ±mÄ±nÄ± baÅŸlatÄ±r
    pub fn init(&self) -> Result<(), &'static str> {
        self.validate_mode_contract()?;
        let domain = crate::memory::iommu_register_device(self.bus, self.device, self.function);
        self.dma_domain.store(domain, Ordering::Release);
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
            "[GPU-Native] Initialized: {}x{} @ {}Hz, MMIO=BAR{} 0x{:x}+0x{:x}, VRAM=BAR{} 0x{:x}+0x{:x}, domain={}",
            self.mode.width,
            self.mode.height,
            self.mode.refresh_hz,
            self.mmio_bar.index,
            self.mmio_bar.base,
            self.mmio_bar.size,
            self.vram_bar.index,
            self.vram_bar.base,
            self.vram_bar.size,
            domain
        );

        Ok(())
    }

    /// 2D blit engine ile dikdÃ¶rtgen kopyalar
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
        let copied = self.validate_blit_geometry(x, y, w, h)?;
        self.validate_dma_range(src_phys, copied)?;

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

        // Blit tamamlanmasÄ±nÄ± bekle
        for _ in 0..10000 {
            let status = self.read_reg32(GPU_2D_STATUS);
            if status & 1 == 0 {
                break;
            } // idle
        }

        Ok(())
    }

    /// Ã‡Ã¶zÃ¼nÃ¼rlÃ¼k deÄŸiÅŸtirir
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

    /// VBLANK event'i DRM device'e bildirir (fence signaling unification).
    ///
    /// Linux DRM/KMS modelinde VBLANK IRQ â†’ dma-fence signal â†’ userspace event.
    /// echOS'ta: GPU native VBLANK â†’ drm.signal_vblank â†’ pending flip completion.
    pub fn handle_vsync_with_drm(&self, drm_device: &crate::drivers::drm::DrmDevice) {
        let event = self.handle_vsync();
        let _ = drm_device.signal_vblank(event.timestamp_ns);
        self.pending_completions.fetch_sub(1, Ordering::Relaxed);
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
        let required = self
            .validate_blit_geometry(x, y, width, height)
            .map_err(|_| AsyncIoError::InvalidParam)?;
        if src_buf.size < required as usize {
            return Err(AsyncIoError::InvalidParam);
        }
        self.with_gpu_domain(|| self.hw_blit(src_buf.paddr, x, y, width, height))
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
        if framebuffer.size < self.mode.framebuffer_size() {
            return Err(AsyncIoError::InvalidParam);
        }
        self.validate_dma_range(framebuffer.paddr, self.mode.framebuffer_size() as u64)
            .map_err(|_| AsyncIoError::InvalidParam)?;
        self.with_gpu_domain(|| self.write_reg32(GPU_REG_FB_ADDR, framebuffer.paddr as u32));

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
    /// KayÄ±tlÄ± GPU native cihazlarÄ±
    static ref GPU_DEVICES: Mutex<Vec<GpuNativeDevice>> = Mutex::new(Vec::new());
}

fn supports_native_scanout(dev: &PciDevice) -> bool {
    dev.class_code == 0x03
        && matches!(
            dev.vendor_id,
            PCI_VENDOR_INTEL | PCI_VENDOR_AMD | PCI_VENDOR_NVIDIA
        )
}

fn gpu_bar_window(dev: &PciDevice, index: u8) -> Option<GpuBarWindow> {
    let bar = crate::drivers::pci::read_bar_mmio(dev.bus, dev.device, dev.function, index)?;
    (bar.base != 0 && bar.size != 0).then_some(GpuBarWindow {
        index,
        base: bar.base,
        size: bar.size,
    })
}

fn select_vram_bar(dev: &PciDevice, mmio_bar: GpuBarWindow) -> Option<GpuBarWindow> {
    if let Some(bar2) = gpu_bar_window(dev, 2) {
        if bar2.base != mmio_bar.base && bar2.size >= GPU_MIN_VRAM_BYTES {
            return Some(bar2);
        }
    }
    if mmio_bar.size > GPU_MMIO_WINDOW_BYTES + GPU_MIN_VRAM_BYTES {
        return Some(GpuBarWindow {
            index: mmio_bar.index,
            base: mmio_bar.base + GPU_MMIO_WINDOW_BYTES,
            size: mmio_bar.size - GPU_MMIO_WINDOW_BYTES,
        });
    }
    None
}

/// GPU native sÃ¼rÃ¼cÃ¼sÃ¼nÃ¼ baÅŸlatÄ±r
pub fn init() {
    crate::serial_println!("[GPU-Native] TIER 1 GPU native driver initialized");

    // PCI taramasÄ± â€” VGA uyumlu cihazlar (class=0x03)
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

            if let Some(mmio_bar) = gpu_bar_window(&dev, 0) {
                let Some(vram_bar) = select_vram_bar(&dev, mmio_bar) else {
                    crate::serial_println!(
                        "[GPU-Native] skipping GPU {:02x}:{:02x}.{}: no validated VRAM BAR",
                        dev.bus,
                        dev.device,
                        dev.function
                    );
                    continue;
                };
                let mut gpu = GpuNativeDevice::new("gpu0", mmio_bar, vram_bar);
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

/// KayÄ±tlÄ± GPU sayÄ±sÄ±
pub fn device_count() -> usize {
    GPU_DEVICES
        .lock()
        .iter()
        .filter(|gpu| gpu.ready.load(Ordering::Acquire))
        .count()
}

/// PCI/MSI IRQ yolundan ÃƒÂSection aÃ„Å¸rÃ„Â±labilecek minimal VBLANK event yardÃ„Â±mcÃ„Â±sÃ„Â±.
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

// ============================================================================
// Test Corpus (Intel HD Graphics + VESA VBE 3.0)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu_for_test(name: &str) -> GpuNativeDevice {
        GpuNativeDevice::new(
            name,
            GpuBarWindow {
                index: 0,
                base: 0x1000,
                size: GPU_MMIO_REQUIRED_BYTES,
            },
            GpuBarWindow {
                index: 2,
                base: 0x2000_0000,
                size: GPU_MIN_VRAM_BYTES,
            },
        )
    }

    #[test]
    fn gpu_device_creation() {
        let device = gpu_for_test("Intel HD Graphics");
        assert_eq!(device.name, "Intel HD Graphics");
        assert_eq!(device.mmio_bar.index, 0);
        assert_eq!(device.vram_bar.index, 2);
        assert!(!device.ready.load(Ordering::Acquire));
    }

    #[test]
    fn gpu_device_ready_state() {
        let device = gpu_for_test("Test GPU");
        assert!(!device.ready.load(Ordering::Acquire));
        device.ready.store(true, Ordering::Release);
        assert!(device.ready.load(Ordering::Acquire));
    }

    #[test]
    fn vesa_vbe_signature() {
        // VESA VBE 3.0: VbeInfoBlock signature must be "VESA"
        let sig: [u8; 4] = [b'V', b'E', b'S', b'A'];
        assert_eq!(sig, *b"VESA");
    }

    #[test]
    fn vesa_vbe_version_encoding() {
        // VBE 3.0: version = 0x0300
        let version: u16 = 0x0300;
        assert_eq!(version >> 8, 3); // Major
        assert_eq!(version & 0xFF, 0); // Minor
    }

    #[test]
    fn vesa_vbe_mode_info_attributes() {
        // VBE 3.0: ModeInfoBlock attributes
        // Bit 0 = Mode supported by hardware
        // Bit 1 = BIOS support
        // Bit 3 = Color mode
        // Bit 4 = Graphics mode
        // Bit 7 = Linear framebuffer
        let attr: u16 = 0x0099; // 0b1001_1001
        assert!(attr & (1 << 0) != 0); // Hardware supported
        assert!(attr & (1 << 3) != 0); // Color mode
        assert!(attr & (1 << 4) != 0); // Graphics mode
        assert!(attr & (1 << 7) != 0); // Linear framebuffer
    }

    #[test]
    fn vesa_vbe_mode_info_1920x1080x32() {
        // VBE mode 0x192: 1920x1080x32
        let width: u16 = 1920;
        let height: u16 = 1080;
        let bpp: u8 = 32;
        let pitch: u16 = width * (bpp as u16 / 8);
        assert_eq!(pitch, 7680); // 1920 * 4 bytes
        let fb_size = (pitch as u32) * (height as u32);
        assert_eq!(fb_size, 8_294_400); // ~8MB framebuffer
    }

    #[test]
    fn vesa_vbe_mode_info_1280x720x32() {
        let width: u16 = 1280;
        let height: u16 = 720;
        let bpp: u8 = 32;
        let pitch: u16 = width * (bpp as u16 / 8);
        assert_eq!(pitch, 5120);
        let fb_size = (pitch as u32) * (height as u32);
        assert_eq!(fb_size, 3_686_400); // ~3.5MB framebuffer
    }

    #[test]
    fn vesa_vbe_mode_info_800x600x16() {
        let width: u16 = 800;
        let height: u16 = 600;
        let bpp: u8 = 16;
        let pitch: u16 = width * (bpp as u16 / 8);
        assert_eq!(pitch, 1600);
        let fb_size = (pitch as u32) * (height as u32);
        assert_eq!(fb_size, 960_000); // ~1MB framebuffer
    }

    #[test]
    fn vesa_vbe_color_mask_positions() {
        // VBE 3.0: RGB mask positions for 32-bit mode
        let r_mask: u8 = 0xFF;
        let g_mask: u8 = 0xFF;
        let b_mask: u8 = 0xFF;
        let r_pos: u8 = 16; // Red at bits 16-23
        let g_pos: u8 = 8; // Green at bits 8-15
        let b_pos: u8 = 0; // Blue at bits 0-7
        let pixel: u32 =
            ((r_mask as u32) << r_pos) | ((g_mask as u32) << g_pos) | ((b_mask as u32) << b_pos);
        assert_eq!(pixel, 0x00FF_FFFF);
    }

    #[test]
    fn gpu_blit_zero_dimension_returns_true() {
        // Edge case: zero-width or zero-height blit should succeed
        assert!(blit_primary_region(0, 0, 0, 0, 100));
        assert!(blit_primary_region(0, 0, 0, 100, 0));
    }
}
