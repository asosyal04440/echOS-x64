//! # echOS Grafik Motoru
//!
//! Doğrusal çerçeve tampon (linear framebuffer) render motoru.
//! SIMD optimizasyonları, tile-based rendering ve compositor içerir.
//!
//! ## Grafik Altyapısı Genel Görünümü
//!
//! ```text
//!  Uygulama Katmanı
//!       │
//!       ▼
//!  ┌────────────────┐    ┌──────────────────┐
//!  │  compositor    │    │  gal (GAL trait) │  ← GPU Soyutlama Katmanı
//!  │  (pencere WM)  │    │  SoftwareGal     │  ← CPU geri dönüş
//!  └───────┬────────┘    └────────┬─────────┘
//!          │                      │
//!          ▼                      ▼
//!  ┌────────────────────────────────────────┐
//!  │           Surface / SwapChain          │  ← Piksel tamponu
//!  │  tile_renderer  (kirli döşeme takibi)  │  ← tile-based render
//!  └─────────────────┬──────────────────────┘
//!                    │
//!                    ▼
//!  ┌────────────────────────────────────────┐
//!  │  simd (AVX2/SSE bellek kopyalama)      │  ← Donanım hızlandırma
//!  └─────────────────┬──────────────────────┘
//!                    │
//!                    ▼
//!  ┌────────────────────────────────────────┐
//!  │  GPU Compute (OpenCL/DirectCompute)    │  ← Paralel hesaplama
//!  └─────────────────┬──────────────────────┘
//!                    │
//!                    ▼
//!  ┌────────────────────────────────────────┐
//!  │  GOP Framebuffer (fiziksel bellek)     │  ← Ekrana doğrudan yazma
//!  └────────────────────────────────────────┘
//! ```

use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicPtr, Ordering};

/// SIMD (AVX2/SSE) grafik operasyonları — donanım hızlandırmalı bellek kopyalama
pub mod simd;

/// Tile (döşeme) render altyapısı — temel Tile ve TileIterator yapıları
pub mod tile;

/// Tile tabanlı render sistemi — TileRenderer, TileCache, HierarchicalTileCache
pub mod tile_renderer;

/// GPU Soyutlama Katmanı (GAL) — SoftwareGal ve Gal trait'i
pub mod gal;

/// GPU Compute Shaders (OpenCL/DirectCompute)
pub mod gpu_compute;

/// DPI Scaling System — resolution-aware scaling for high-DPI displays
pub mod scaling;

/// Animasyonlu duvar kağıdı motoru
pub mod wallpaper;

/// Blur ve gölge efektleri
pub mod blur;

/// Scene-backed shell surface helpers
pub mod shell_scene;

/// Shell frame invalidation and publication planning
pub mod shell_invalidation;

/// Velvet Glove Compositor - echOS native desktop runtime
pub mod velvet_glove;

pub struct Surface {
    pub width: usize,
    pub height: usize,
    pub stride: usize,
    pub buffer: Vec<u32>,
}

impl Surface {
    /// Her pikseli sabit renkle doldurur.
    pub fn fill(&mut self, color: u32) {
        for p in self.buffer.iter_mut() {
            *p = color;
        }
    }

    /// Diğer surface'i bu surface'e verilen (x, y) konumundan alpha-blend ile çizer.
    /// `opacity` 0..=255 arası ek saydamlık katsayısı.
    pub fn blend_from(&mut self, src: &Surface, dst_x: i32, dst_y: i32, opacity: u8) {
        let sw = src.width as i32;
        let sh = src.height as i32;
        let dw = self.width as i32;
        let dh = self.height as i32;

        let clip_x0 = dst_x.max(0);
        let clip_y0 = dst_y.max(0);
        let clip_x1 = (dst_x + sw).min(dw);
        let clip_y1 = (dst_y + sh).min(dh);

        if clip_x0 >= clip_x1 || clip_y0 >= clip_y1 {
            return;
        }

        let a = opacity as u32;
        let inv_a = 255 - a;

        for dy in clip_y0..clip_y1 {
            let sy = dy - dst_y;
            let src_row = sy as usize * src.stride;
            let dst_row = dy as usize * self.stride;
            for dx in clip_x0..clip_x1 {
                let sx = dx - dst_x;
                let src_px = src.buffer[src_row + sx as usize];
                let dst_px = self.buffer[dst_row + dx as usize];
                // Kaynak pikselin kendi alpha değeri varsa (ARGB formatı bit 31..24)
                // etkin alpha = kaynak_alpha × opacity / 255 şeklinde hesaplanır
                let src_a = ((src_px >> 24) & 0xFF) as u32;
                let eff_a = (src_a * a) / 255;
                let eff_inv = 255 - eff_a;
                let r = (((src_px >> 16) & 0xFF) * eff_a + ((dst_px >> 16) & 0xFF) * eff_inv) / 255;
                let g = (((src_px >> 8) & 0xFF) * eff_a + ((dst_px >> 8) & 0xFF) * eff_inv) / 255;
                let b = ((src_px & 0xFF) * eff_a + (dst_px & 0xFF) * eff_inv) / 255;
                self.buffer[dst_row + dx as usize] = 0xFF000000 | (r << 16) | (g << 8) | b;
            }
        }
    }

    /// Tek renk dikdörtgen çizer (opak)
    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        let x0 = x.max(0) as usize;
        let y0 = y.max(0) as usize;
        let x1 = (x + w).min(self.width as i32).max(0) as usize;
        let y1 = (y + h).min(self.height as i32).max(0) as usize;
        for row in y0..y1 {
            let start = row * self.stride + x0;
            let end = row * self.stride + x1;
            for p in &mut self.buffer[start..end] {
                *p = color;
            }
        }
    }

    /// Dikdörtgen kenarlık çizer.
    pub fn draw_rect_outline(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        self.fill_rect(x, y, w, 1, color);
        self.fill_rect(x, y + h - 1, w, 1, color);
        self.fill_rect(x, y, 1, h, color);
        self.fill_rect(x + w - 1, y, 1, h, color);
    }

    pub fn new(width: usize, height: usize, stride: usize) -> Self {
        let len = stride.saturating_mul(height);
        Self {
            width,
            height,
            stride,
            buffer: vec![0u32; len],
        }
    }

    pub fn buffer_mut(&mut self) -> &mut [u32] {
        &mut self.buffer
    }

    pub unsafe fn blit_rect_to(
        &self,
        dst: *mut u32,
        dst_stride: usize,
        x: usize,
        y: usize,
        width: usize,
        height: usize,
    ) {
        let src_stride = self.stride;
        for row in 0..height {
            let src_ptr = self.buffer.as_ptr().add((y + row) * src_stride + x) as *const u8;
            let dst_ptr = dst.add((y + row) * dst_stride + x) as *mut u8;
            let byte_len = width * 4;
            crate::gfx::simd::stream_copy(src_ptr, dst_ptr, byte_len);
        }
    }
}

pub struct SwapChain {
    pub front: Surface,
    pub back: Surface,
    front_ptr: AtomicPtr<u32>,
}

impl SwapChain {
    pub fn new(width: usize, height: usize, stride: usize) -> Self {
        let mut front = Surface::new(width, height, stride);
        let back = Surface::new(width, height, stride);
        let front_ptr = AtomicPtr::new(front.buffer.as_mut_ptr());
        Self {
            front,
            back,
            front_ptr,
        }
    }

    pub fn front_ptr(&self) -> *mut u32 {
        self.front_ptr.load(Ordering::SeqCst)
    }

    pub fn swap(&mut self) {
        core::mem::swap(&mut self.front, &mut self.back);
        self.front_ptr
            .store(self.front.buffer.as_mut_ptr(), Ordering::SeqCst);
    }
}

#[derive(Clone, Copy, Debug)]
pub struct OpenGlVertex {
    pub x: f32,
    pub y: f32,
    pub color: u32,
}

pub struct OpenGlContext {
    pub surface: Surface,
    clear_color: u32,
}

impl OpenGlContext {
    pub fn new(width: usize, height: usize, stride: usize) -> Self {
        Self {
            surface: Surface::new(width, height, stride),
            clear_color: 0,
        }
    }

    pub fn surface(&self) -> &Surface {
        &self.surface
    }

    pub fn surface_mut(&mut self) -> &mut Surface {
        &mut self.surface
    }

    pub fn set_clear_color(&mut self, color: u32) {
        self.clear_color = color;
    }

    pub fn clear(&mut self) {
        for pixel in self.surface.buffer.iter_mut() {
            *pixel = self.clear_color;
        }
    }

    pub fn draw_triangle(&mut self, v0: OpenGlVertex, v1: OpenGlVertex, v2: OpenGlVertex) {
        let min_x = floor_to_i32(v0.x.min(v1.x.min(v2.x)));
        let max_x = ceil_to_i32(v0.x.max(v1.x.max(v2.x)));
        let min_y = floor_to_i32(v0.y.min(v1.y.min(v2.y)));
        let max_y = ceil_to_i32(v0.y.max(v1.y.max(v2.y)));

        let width = self.surface.width as i32;
        let height = self.surface.height as i32;

        let start_x = min_x.clamp(0, width.saturating_sub(1));
        let end_x = max_x.clamp(0, width.saturating_sub(1));
        let start_y = min_y.clamp(0, height.saturating_sub(1));
        let end_y = max_y.clamp(0, height.saturating_sub(1));

        let area = edge_function(v0.x, v0.y, v1.x, v1.y, v2.x, v2.y);
        if area == 0.0 {
            return;
        }
        let inv_area = 1.0 / area;

        let (r0, g0, b0) = unpack_color(v0.color);
        let (r1, g1, b1) = unpack_color(v1.color);
        let (r2, g2, b2) = unpack_color(v2.color);

        for y in start_y..=end_y {
            for x in start_x..=end_x {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let w0 = edge_function(v1.x, v1.y, v2.x, v2.y, px, py);
                let w1 = edge_function(v2.x, v2.y, v0.x, v0.y, px, py);
                let w2 = edge_function(v0.x, v0.y, v1.x, v1.y, px, py);

                if (w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0) || (w0 <= 0.0 && w1 <= 0.0 && w2 <= 0.0) {
                    let a0 = w0 * inv_area;
                    let a1 = w1 * inv_area;
                    let a2 = w2 * inv_area;
                    let r = r0 * a0 + r1 * a1 + r2 * a2;
                    let g = g0 * a0 + g1 * a1 + g2 * a2;
                    let b = b0 * a0 + b1 * a1 + b2 * a2;
                    let color = pack_color(r, g, b);
                    let idx = (y as usize) * self.surface.stride + (x as usize);
                    if idx < self.surface.buffer.len() {
                        self.surface.buffer[idx] = color;
                    }
                }
            }
        }
    }
}

fn edge_function(ax: f32, ay: f32, bx: f32, by: f32, cx: f32, cy: f32) -> f32 {
    (cx - ax) * (by - ay) - (cy - ay) * (bx - ax)
}

fn floor_to_i32(value: f32) -> i32 {
    let truncated = value as i32;
    if value < 0.0 && (truncated as f32) != value {
        truncated - 1
    } else {
        truncated
    }
}

fn ceil_to_i32(value: f32) -> i32 {
    let truncated = value as i32;
    if value > 0.0 && (truncated as f32) != value {
        truncated + 1
    } else {
        truncated
    }
}

fn unpack_color(color: u32) -> (f32, f32, f32) {
    let r = ((color >> 16) & 0xFF) as f32;
    let g = ((color >> 8) & 0xFF) as f32;
    let b = (color & 0xFF) as f32;
    (r, g, b)
}

fn pack_color(r: f32, g: f32, b: f32) -> u32 {
    let r = r.max(0.0).min(255.0) as u32;
    let g = g.max(0.0).min(255.0) as u32;
    let b = b.max(0.0).min(255.0) as u32;
    (r << 16) | (g << 8) | b
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphicsBackend {
    Software,
    OpenGl,
    Vulkan,
}

#[derive(Clone, Debug)]
pub struct GraphicsDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub mmio_bars: Vec<crate::drivers::pci::PciBar>,
}

#[derive(Clone, Debug)]
pub struct GraphicsCapabilities {
    pub backend: GraphicsBackend,
    pub gpu_devices: usize,
    pub has_opengl: bool,
    pub has_vulkan: bool,
}

#[derive(Clone, Debug)]
pub struct GraphicsBackendState {
    pub backend: GraphicsBackend,
    pub devices: Vec<GraphicsDevice>,
    pub opengl_ready: bool,
    pub vulkan_ready: bool,
}

pub fn detect_graphics_capabilities() -> GraphicsCapabilities {
    let gpu_list = list_gpu_devices();
    let (has_opengl, has_vulkan) = detect_api_support(&gpu_list);
    let backend = select_backend(has_opengl, has_vulkan);
    GraphicsCapabilities {
        backend,
        gpu_devices: gpu_list.len(),
        has_opengl,
        has_vulkan,
    }
}

pub fn list_gpu_devices() -> Vec<GraphicsDevice> {
    let mut gpus = Vec::new();
    for dev in crate::drivers::pci::scan() {
        if dev.class_code == 0x03 {
            let mmio_bars = read_gpu_mmio_bars(dev.bus, dev.device, dev.function);
            gpus.push(GraphicsDevice {
                bus: dev.bus,
                device: dev.device,
                function: dev.function,
                vendor_id: dev.vendor_id,
                device_id: dev.device_id,
                class_code: dev.class_code,
                subclass: dev.subclass,
                prog_if: dev.prog_if,
                mmio_bars,
            });
        }
    }
    gpus
}

pub fn init_graphics_backend() -> GraphicsBackendState {
    let devices = list_gpu_devices();
    let (has_opengl, has_vulkan) = detect_api_support(&devices);
    let backend = select_backend(has_opengl, has_vulkan);
    GraphicsBackendState {
        backend,
        devices,
        opengl_ready: has_opengl,
        vulkan_ready: has_vulkan,
    }
}

fn detect_api_support(devices: &[GraphicsDevice]) -> (bool, bool) {
    if devices.is_empty() {
        return (false, false);
    }
    let mut has_opengl = false;
    let mut has_vulkan = false;
    for dev in devices {
        let has_mmio = !dev.mmio_bars.is_empty();
        if dev.subclass == 0x00 && has_mmio {
            has_opengl = true;
        }
        if dev.subclass == 0x02 && has_mmio {
            has_vulkan = true;
        }
    }
    (has_opengl, has_vulkan)
}

fn read_gpu_mmio_bars(bus: u8, device: u8, function: u8) -> Vec<crate::drivers::pci::PciBar> {
    let mut bars = Vec::new();
    let mut bar_index = 0u8;
    while bar_index < 6 {
        if let Some(bar) = crate::drivers::pci::read_bar_mmio(bus, device, function, bar_index) {
            let is_64 = bar.is_64;
            bars.push(bar);
            if is_64 {
                bar_index = bar_index.saturating_add(2);
                continue;
            }
        }
        bar_index = bar_index.saturating_add(1);
    }
    bars
}

fn select_backend(has_opengl: bool, has_vulkan: bool) -> GraphicsBackend {
    if has_vulkan {
        GraphicsBackend::Vulkan
    } else if has_opengl {
        GraphicsBackend::OpenGl
    } else {
        GraphicsBackend::Software
    }
}
