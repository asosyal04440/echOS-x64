use crate::gfx::gal::{Gal, SoftwareGal};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::protocol::{Rect, RenderObject, RenderObjectKind, SceneUpdate};
use crate::gui::text::{TextStyle, TextSystem};
use alloc::vec::Vec;
use core::ops::Range;
use core::ptr::NonNull;

pub trait Renderer {
    fn render_scene(
        &mut self,
        framebuffer: &mut Framebuffer,
        damage: Rect,
        origin_x: i32,
        origin_y: i32,
        scene: &SceneUpdate,
        text_system: &mut TextSystem,
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderBatchClass {
    SolidRect,
    Raster,
    TextRun,
    GlyphRun,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderBatchKey {
    pub lane: crate::gui::protocol::DamageLane,
    pub clip: Option<Rect>,
    pub opacity: u8,
    pub class: RenderBatchClass,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderBatch {
    pub key: RenderBatchKey,
    pub range: Range<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderFrame {
    pub batches: Vec<RenderBatch>,
}

#[derive(Default)]
pub struct CpuRenderer;

pub struct GpuRenderer {
    backend: GpuBackend,
}

enum GpuBackend {
    Software(SoftwareGal),
    Native(NativePresentBackend),
}

struct NativePresentBackend {
    width: u32,
    height: u32,
    paddr: usize,
    vaddr: NonNull<u32>,
    pages: usize,
}

trait GpuPixelBackend {
    fn plot_pixel(&mut self, x: u32, y: u32, color: u32);
    fn pixel(&self, x: u32, y: u32) -> Option<u32>;
}

impl CpuRenderer {
    pub fn new() -> Self {
        Self
    }
}

impl GpuRenderer {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            backend: GpuBackend::new(width, height),
        }
    }
}

impl GpuBackend {
    fn new(width: u32, height: u32) -> Self {
        NativePresentBackend::new(width, height)
            .map(Self::Native)
            .unwrap_or_else(|| Self::Software(SoftwareGal::new(width, height)))
    }

    fn pixel_len(&self) -> usize {
        match self {
            Self::Software(backend) => backend.pixels().len(),
            Self::Native(backend) => backend.pixel_len(),
        }
    }

    fn resize(&mut self, width: u32, height: u32) {
        match self {
            Self::Software(backend) => backend.resize(width, height),
            Self::Native(backend) => backend.resize(width, height),
        }
    }

    fn clear_rgba(&mut self, color: u32) {
        match self {
            Self::Software(backend) => backend.clear_rgba(color),
            Self::Native(backend) => backend.clear_rgba(color),
        }
    }

    fn present_damage_to_framebuffer(&self, framebuffer: &mut Framebuffer, damage: Rect) {
        match self {
            Self::Software(backend) => {
                for y in damage.y.max(0) as usize..damage.bottom().max(0) as usize {
                    if y >= framebuffer.height {
                        break;
                    }
                    for x in damage.x.max(0) as usize..damage.right().max(0) as usize {
                        if x >= framebuffer.width {
                            break;
                        }
                        if let Some(color) = backend.pixel(x as u32, y as u32) {
                            if color != 0 {
                                framebuffer.plot_pixel(x, y, color);
                            }
                        }
                    }
                }
            }
            Self::Native(backend) => backend.present_damage_to_framebuffer(framebuffer, damage),
        }
    }
}

impl GpuPixelBackend for GpuBackend {
    fn plot_pixel(&mut self, x: u32, y: u32, color: u32) {
        match self {
            Self::Software(backend) => backend.plot_pixel(x, y, color),
            Self::Native(backend) => backend.plot_pixel(x, y, color),
        }
    }

    fn pixel(&self, x: u32, y: u32) -> Option<u32> {
        match self {
            Self::Software(backend) => backend.pixel(x, y),
            Self::Native(backend) => backend.pixel(x, y),
        }
    }
}

impl NativePresentBackend {
    fn new(width: u32, height: u32) -> Option<Self> {
        if crate::drivers::gpu_native::device_count() == 0 {
            return None;
        }

        let bytes = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(core::mem::size_of::<u32>());
        let pages = ((bytes.saturating_add(4095)) / 4096).max(1);
        let (paddr, vaddr) = crate::memory::dma_alloc(pages)?;
        let vaddr = NonNull::new(vaddr.as_ptr() as *mut u32)?;
        Some(Self {
            width,
            height,
            paddr,
            vaddr,
            pages,
        })
    }

    fn pixel_len(&self) -> usize {
        self.width as usize * self.height as usize
    }

    fn pixels(&self) -> &[u32] {
        unsafe { core::slice::from_raw_parts(self.vaddr.as_ptr(), self.pixel_len()) }
    }

    fn pixels_mut(&mut self) -> &mut [u32] {
        unsafe { core::slice::from_raw_parts_mut(self.vaddr.as_ptr(), self.pixel_len()) }
    }

    fn clear_rgba(&mut self, color: u32) {
        for pixel in self.pixels_mut().iter_mut() {
            *pixel = color;
        }
    }

    fn plot_pixel(&mut self, x: u32, y: u32, color: u32) {
        if x >= self.width || y >= self.height {
            return;
        }
        let index = y as usize * self.width as usize + x as usize;
        if let Some(pixel) = self.pixels_mut().get_mut(index) {
            *pixel = color;
        }
    }

    fn pixel(&self, x: u32, y: u32) -> Option<u32> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.pixels()
            .get(y as usize * self.width as usize + x as usize)
            .copied()
    }

    fn resize(&mut self, width: u32, height: u32) {
        let bytes = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(core::mem::size_of::<u32>());
        let pages = ((bytes.saturating_add(4095)) / 4096).max(1);
        if pages == self.pages {
            self.width = width;
            self.height = height;
            self.clear_rgba(0);
            return;
        }

        if let Some((paddr, vaddr)) = crate::memory::dma_alloc(pages) {
            if let Some(vaddr) = NonNull::new(vaddr.as_ptr() as *mut u32) {
                crate::memory::dma_dealloc(self.paddr, self.pages);
                self.width = width;
                self.height = height;
                self.paddr = paddr;
                self.vaddr = vaddr;
                self.pages = pages;
                self.clear_rgba(0);
                return;
            }
            crate::memory::dma_dealloc(paddr, pages);
        }
        self.clear_rgba(0);
    }

    fn present_damage_to_framebuffer(&self, framebuffer: &mut Framebuffer, damage: Rect) {
        for y in damage.y.max(0) as usize..damage.bottom().max(0) as usize {
            if y >= framebuffer.height || y >= self.height as usize {
                break;
            }
            for x in damage.x.max(0) as usize..damage.right().max(0) as usize {
                if x >= framebuffer.width || x >= self.width as usize {
                    break;
                }
                if let Some(color) = self.pixel(x as u32, y as u32) {
                    if color != 0 {
                        framebuffer.plot_pixel(x, y, color);
                    }
                }
            }
        }

        let _ = crate::drivers::gpu_native::blit_primary_region(
            self.paddr as u64,
            damage.x.max(0) as u32,
            damage.y.max(0) as u32,
            damage.width,
            damage.height,
        );
    }
}

impl Drop for NativePresentBackend {
    fn drop(&mut self) {
        crate::memory::dma_dealloc(self.paddr, self.pages);
    }
}

impl RenderBatchKey {
    fn for_object(object: &RenderObject) -> Self {
        Self {
            lane: object.lane,
            clip: object.clip,
            opacity: object.opacity,
            class: match object.kind {
                RenderObjectKind::SolidRect { .. } => RenderBatchClass::SolidRect,
                RenderObjectKind::Raster { .. } => RenderBatchClass::Raster,
                RenderObjectKind::TextRun { .. } => RenderBatchClass::TextRun,
                RenderObjectKind::GlyphRun { .. } => RenderBatchClass::GlyphRun,
            },
        }
    }
}

pub fn compile_render_frame(objects: &[RenderObject]) -> RenderFrame {
    let mut batches = Vec::new();
    if objects.is_empty() {
        return RenderFrame { batches };
    }

    let mut start = 0usize;
    let mut current_key = RenderBatchKey::for_object(&objects[0]);
    for (index, object) in objects.iter().enumerate().skip(1) {
        let next_key = RenderBatchKey::for_object(object);
        if next_key != current_key {
            batches.push(RenderBatch {
                key: current_key,
                range: start..index,
            });
            start = index;
            current_key = next_key;
        }
    }
    batches.push(RenderBatch {
        key: current_key,
        range: start..objects.len(),
    });

    RenderFrame { batches }
}

impl Renderer for CpuRenderer {
    fn render_scene(
        &mut self,
        framebuffer: &mut Framebuffer,
        damage: Rect,
        origin_x: i32,
        origin_y: i32,
        scene: &SceneUpdate,
        text_system: &mut TextSystem,
    ) {
        let frame = compile_render_frame(&scene.render_objects);
        render_compiled_frame_cpu(
            framebuffer,
            damage,
            origin_x,
            origin_y,
            &scene.render_objects,
            &frame,
            text_system,
        );
    }
}

impl Renderer for GpuRenderer {
    fn render_scene(
        &mut self,
        framebuffer: &mut Framebuffer,
        damage: Rect,
        origin_x: i32,
        origin_y: i32,
        scene: &SceneUpdate,
        text_system: &mut TextSystem,
    ) {
        if self.backend.pixel_len() != framebuffer.width.saturating_mul(framebuffer.height) {
            self.backend
                .resize(framebuffer.width as u32, framebuffer.height as u32);
        }
        self.backend.clear_rgba(0);
        let frame = compile_render_frame(&scene.render_objects);
        render_compiled_frame_gpu(
            &mut self.backend,
            damage,
            origin_x,
            origin_y,
            &scene.render_objects,
            &frame,
            text_system,
        );
        self.backend
            .present_damage_to_framebuffer(framebuffer, damage);
    }
}

pub fn render_object_list(
    framebuffer: &mut Framebuffer,
    damage: Rect,
    origin_x: i32,
    origin_y: i32,
    objects: &[RenderObject],
    text_system: &mut TextSystem,
) {
    let frame = compile_render_frame(objects);
    render_compiled_frame_cpu(
        framebuffer,
        damage,
        origin_x,
        origin_y,
        objects,
        &frame,
        text_system,
    );
}

fn render_compiled_frame_cpu(
    framebuffer: &mut Framebuffer,
    damage: Rect,
    origin_x: i32,
    origin_y: i32,
    objects: &[RenderObject],
    frame: &RenderFrame,
    text_system: &mut TextSystem,
) {
    for batch in frame.batches.iter() {
        for object in objects[batch.range.clone()].iter() {
            draw_render_object(framebuffer, damage, origin_x, origin_y, object, text_system);
        }
    }
}

fn render_compiled_frame_gpu(
    backend: &mut impl GpuPixelBackend,
    damage: Rect,
    origin_x: i32,
    origin_y: i32,
    objects: &[RenderObject],
    frame: &RenderFrame,
    text_system: &mut TextSystem,
) {
    for batch in frame.batches.iter() {
        for object in objects[batch.range.clone()].iter() {
            draw_render_object_gpu(backend, damage, origin_x, origin_y, object, text_system);
        }
    }
}

fn draw_render_object(
    framebuffer: &mut Framebuffer,
    damage: Rect,
    origin_x: i32,
    origin_y: i32,
    object: &RenderObject,
    text_system: &mut TextSystem,
) {
    let translated = Rect::new(
        object.bounds.x.saturating_add(origin_x),
        object.bounds.y.saturating_add(origin_y),
        object.bounds.width,
        object.bounds.height,
    );
    let clip = object
        .clip
        .map(|clip| {
            Rect::new(
                clip.x.saturating_add(origin_x),
                clip.y.saturating_add(origin_y),
                clip.width,
                clip.height,
            )
        })
        .unwrap_or(translated);
    let Some(render_rect) = translated
        .intersection(&damage)
        .and_then(|rect| rect.intersection(&clip))
    else {
        return;
    };

    match &object.kind {
        RenderObjectKind::SolidRect {
            color,
            corner_radius,
        } => {
            if *corner_radius == 0 {
                fill_rect(framebuffer, render_rect, *color);
            } else {
                fill_rounded_rect(
                    framebuffer,
                    translated,
                    render_rect,
                    *color,
                    *corner_radius as u32,
                );
            }
        }
        RenderObjectKind::Raster {
            width,
            height,
            pixels,
        } => draw_raster(
            framebuffer,
            translated,
            render_rect,
            *width as usize,
            *height as usize,
            pixels,
            object.opacity,
        ),
        RenderObjectKind::TextRun {
            text,
            color,
            style,
            max_width,
            ..
        } => draw_text_run(
            framebuffer,
            translated,
            render_rect,
            text,
            *color,
            object.opacity,
            *style,
            *max_width,
            text_system,
        ),
        RenderObjectKind::GlyphRun {
            width,
            height,
            pixels,
            ..
        } => draw_raster(
            framebuffer,
            translated,
            render_rect,
            *width as usize,
            *height as usize,
            pixels,
            object.opacity,
        ),
    }
}

fn fill_rect(framebuffer: &mut Framebuffer, rect: Rect, color: u32) {
    for y in rect.y.max(0) as usize..rect.bottom().max(0) as usize {
        if y >= framebuffer.height {
            break;
        }
        for x in rect.x.max(0) as usize..rect.right().max(0) as usize {
            if x >= framebuffer.width {
                break;
            }
            framebuffer.plot_pixel(x, y, color);
        }
    }
}

trait PixelSink {
    fn plot_pixel(&mut self, x: u32, y: u32, color: u32);
}

impl PixelSink for Framebuffer {
    fn plot_pixel(&mut self, x: u32, y: u32, color: u32) {
        Framebuffer::plot_pixel(self, x as usize, y as usize, color);
    }
}

impl<T: GpuPixelBackend> PixelSink for T {
    fn plot_pixel(&mut self, x: u32, y: u32, color: u32) {
        GpuPixelBackend::plot_pixel(self, x, y, color);
    }
}

fn fill_rect_sink<S: PixelSink>(sink: &mut S, rect: Rect, color: u32) {
    for y in rect.y.max(0) as u32..rect.bottom().max(0) as u32 {
        for x in rect.x.max(0) as u32..rect.right().max(0) as u32 {
            sink.plot_pixel(x, y, color);
        }
    }
}

fn fill_rounded_rect(
    framebuffer: &mut Framebuffer,
    geometry_bounds: Rect,
    render_rect: Rect,
    color: u32,
    radius: u32,
) {
    fill_rounded_rect_clipped(framebuffer, geometry_bounds, render_rect, color, radius);
}

fn fill_rounded_rect_clipped<S: PixelSink>(
    sink: &mut S,
    geometry_bounds: Rect,
    render_rect: Rect,
    color: u32,
    radius: u32,
) {
    if geometry_bounds.width == 0
        || geometry_bounds.height == 0
        || render_rect.width == 0
        || render_rect.height == 0
    {
        return;
    }
    let radius = radius
        .min(geometry_bounds.width / 2)
        .min(geometry_bounds.height / 2);
    if radius <= 1 {
        fill_rect_sink(sink, render_rect, color);
        return;
    }

    let r = radius as i32;
    let left = geometry_bounds.x;
    let top = geometry_bounds.y;
    let right = geometry_bounds.right().saturating_sub(1);
    let bottom = geometry_bounds.bottom().saturating_sub(1);
    let inner_left = left + r;
    let inner_right = right - r;
    let inner_top = top + r;
    let inner_bottom = bottom - r;
    let r_sq = (r * r) as u32;

    for y in render_rect.y.max(0) as u32..render_rect.bottom().max(0) as u32 {
        let yi = y as i32;
        let draw_span = if yi < inner_top {
            let dy = (inner_top - yi).unsigned_abs();
            rounded_span(inner_left, inner_right, r_sq, dy)
        } else if yi > inner_bottom {
            let dy = (yi - inner_bottom).unsigned_abs();
            rounded_span(inner_left, inner_right, r_sq, dy)
        } else {
            Some((left, right))
        };
        let Some((span_left, span_right)) = draw_span else {
            continue;
        };
        let clipped_left = span_left.max(render_rect.x).max(0) as u32;
        let clipped_right = (span_right + 1).min(render_rect.right()).max(0) as u32;
        if clipped_left >= clipped_right {
            continue;
        }
        for x in clipped_left..clipped_right {
            sink.plot_pixel(x, y, color);
        }
    }
}

fn rounded_span(inner_left: i32, inner_right: i32, radius_sq: u32, dy: u32) -> Option<(i32, i32)> {
    if dy.saturating_mul(dy) > radius_sq {
        return None;
    }
    let dx = isqrt_u32(radius_sq.saturating_sub(dy.saturating_mul(dy))) as i32;
    Some((inner_left - dx, inner_right + dx))
}

fn isqrt_u32(value: u32) -> u32 {
    if value <= 1 {
        return value;
    }
    let mut x = value;
    let mut y = x.div_ceil(2);
    while y < x {
        x = y;
        y = (x + value / x) / 2;
    }
    x
}

fn draw_raster(
    framebuffer: &mut Framebuffer,
    translated: Rect,
    render_rect: Rect,
    width: usize,
    height: usize,
    pixels: &[u32],
    opacity: u8,
) {
    if width == 0 || height == 0 {
        return;
    }

    let offset_x = (render_rect.x - translated.x) as usize;
    let offset_y = (render_rect.y - translated.y) as usize;
    for row in 0..render_rect.height as usize {
        let src_row = (offset_y + row) * width;
        let dst_y = render_rect.y.max(0) as usize + row;
        if dst_y >= framebuffer.height {
            break;
        }
        for col in 0..render_rect.width as usize {
            let src_idx = src_row + offset_x + col;
            if src_idx >= pixels.len() {
                break;
            }
            let dst_x = render_rect.x.max(0) as usize + col;
            if dst_x >= framebuffer.width {
                break;
            }
            let source = pixels[src_idx];
            if let Some(color) = raster_pixel(framebuffer.get_pixel(dst_x, dst_y), source, opacity)
            {
                framebuffer.plot_pixel(dst_x, dst_y, color);
            }
        }
    }
}

fn draw_text_run(
    framebuffer: &mut Framebuffer,
    translated: Rect,
    render_rect: Rect,
    text: &str,
    color: u32,
    opacity: u8,
    style: crate::gui::protocol::TextRunStyle,
    max_width: u32,
    text_system: &mut TextSystem,
) {
    let blob = text_system.layout_text_with_style(
        text,
        max_width.max(translated.width.max(1)),
        match style {
            crate::gui::protocol::TextRunStyle::Ui => TextStyle::ui(),
            crate::gui::protocol::TextRunStyle::Mono => TextStyle::mono(),
        },
        color,
    );
    draw_raster(
        framebuffer,
        Rect::new(
            translated.x,
            translated.y,
            blob.width_px.max(1),
            blob.height_px.max(1),
        ),
        render_rect,
        blob.width_px.max(1) as usize,
        blob.height_px.max(1) as usize,
        &blob.pixels,
        opacity,
    );
}

fn blend_pixel(background: u32, foreground: u32, opacity: u8) -> u32 {
    let alpha = opacity as u32;
    let inv_alpha = 255u32.saturating_sub(alpha);
    let br = (background >> 16) & 0xFF;
    let bg = (background >> 8) & 0xFF;
    let bb = background & 0xFF;
    let fr = (foreground >> 16) & 0xFF;
    let fg = (foreground >> 8) & 0xFF;
    let fb = foreground & 0xFF;

    let r = (fr * alpha + br * inv_alpha) / 255;
    let g = (fg * alpha + bg * inv_alpha) / 255;
    let b = (fb * alpha + bb * inv_alpha) / 255;
    0xFF00_0000 | (r << 16) | (g << 8) | b
}

fn raster_pixel(background: u32, source: u32, opacity: u8) -> Option<u32> {
    let source_alpha = ((source >> 24) & 0xFF) as u32;
    if source_alpha == 0 || opacity == 0 {
        return None;
    }
    let effective_alpha = (source_alpha * opacity as u32) / 255;
    if effective_alpha == 0 {
        return None;
    }
    if effective_alpha == 255 {
        return Some(0xFF00_0000 | (source & 0x00FF_FFFF));
    }
    Some(blend_pixel(
        background,
        0xFF00_0000 | (source & 0x00FF_FFFF),
        effective_alpha as u8,
    ))
}

fn draw_render_object_gpu(
    backend: &mut impl GpuPixelBackend,
    damage: Rect,
    origin_x: i32,
    origin_y: i32,
    object: &RenderObject,
    text_system: &mut TextSystem,
) {
    let translated = Rect::new(
        object.bounds.x.saturating_add(origin_x),
        object.bounds.y.saturating_add(origin_y),
        object.bounds.width,
        object.bounds.height,
    );
    let clip = object
        .clip
        .map(|clip| {
            Rect::new(
                clip.x.saturating_add(origin_x),
                clip.y.saturating_add(origin_y),
                clip.width,
                clip.height,
            )
        })
        .unwrap_or(translated);
    let Some(render_rect) = translated
        .intersection(&damage)
        .and_then(|rect| rect.intersection(&clip))
    else {
        return;
    };

    match &object.kind {
        RenderObjectKind::SolidRect {
            color,
            corner_radius,
        } => fill_rounded_rect_gpu(
            backend,
            translated,
            render_rect,
            *color,
            *corner_radius as u32,
        ),
        RenderObjectKind::Raster {
            width,
            height,
            pixels,
        } => draw_raster_gpu(
            backend,
            translated,
            render_rect,
            *width as usize,
            *height as usize,
            pixels,
            object.opacity,
        ),
        RenderObjectKind::TextRun {
            text,
            color,
            style,
            max_width,
            ..
        } => {
            let blob = text_system.layout_text_with_style(
                text,
                (*max_width).max(translated.width.max(1)),
                match style {
                    crate::gui::protocol::TextRunStyle::Ui => TextStyle::ui(),
                    crate::gui::protocol::TextRunStyle::Mono => TextStyle::mono(),
                },
                *color,
            );
            draw_raster_gpu(
                backend,
                Rect::new(
                    translated.x,
                    translated.y,
                    blob.width_px.max(1),
                    blob.height_px.max(1),
                ),
                render_rect,
                blob.width_px.max(1) as usize,
                blob.height_px.max(1) as usize,
                &blob.pixels,
                object.opacity,
            );
        }
        RenderObjectKind::GlyphRun {
            width,
            height,
            pixels,
            ..
        } => draw_raster_gpu(
            backend,
            translated,
            render_rect,
            *width as usize,
            *height as usize,
            pixels,
            object.opacity,
        ),
    }
}

fn fill_rounded_rect_gpu(
    backend: &mut impl GpuPixelBackend,
    translated: Rect,
    render_rect: Rect,
    color: u32,
    radius: u32,
) {
    fill_rounded_rect_clipped(backend, translated, render_rect, color, radius);
}

fn draw_raster_gpu(
    backend: &mut impl GpuPixelBackend,
    translated: Rect,
    render_rect: Rect,
    width: usize,
    height: usize,
    pixels: &[u32],
    opacity: u8,
) {
    let offset_x = (render_rect.x - translated.x) as usize;
    let offset_y = (render_rect.y - translated.y) as usize;
    for row in 0..render_rect.height as usize {
        let src_row = (offset_y + row) * width;
        let dst_y = render_rect.y.max(0) as usize + row;
        for col in 0..render_rect.width as usize {
            let src_idx = src_row + offset_x + col;
            if src_idx >= pixels.len() {
                break;
            }
            let dst_x = render_rect.x.max(0) as usize + col;
            let source = pixels[src_idx];
            if let Some(color) = raster_pixel(
                backend.pixel(dst_x as u32, dst_y as u32).unwrap_or(0),
                source,
                opacity,
            ) {
                backend.plot_pixel(dst_x as u32, dst_y as u32, color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::protocol::{
        DamageLane, Rect, RenderObject, RenderObjectKind, SceneUpdate, TextRunStyle,
    };
    use alloc::vec;

    fn sample_scene() -> SceneUpdate {
        SceneUpdate {
            root_id: 1,
            revision: 1,
            render_objects: vec![
                RenderObject {
                    object_id: 1,
                    bounds: Rect::new(0, 0, 32, 24),
                    clip: None,
                    z_index: 0,
                    opacity: u8::MAX,
                    lane: DamageLane::Window,
                    kind: RenderObjectKind::SolidRect {
                        color: 0xFF10_2030,
                        corner_radius: 0,
                    },
                },
                RenderObject {
                    object_id: 2,
                    bounds: Rect::new(4, 4, 24, 18),
                    clip: None,
                    z_index: 1,
                    opacity: u8::MAX,
                    lane: DamageLane::Text,
                    kind: RenderObjectKind::TextRun {
                        blob_id: 0,
                        text: alloc::string::String::from("GPU"),
                        color: 0xFFFF_FFFF,
                        style: TextRunStyle::Ui,
                        max_width: 24,
                    },
                },
            ],
            damage_hint: vec![Rect::new(0, 0, 32, 24)],
            semantic_root: None,
        }
    }

    #[test]
    fn cpu_and_gpu_renderers_match_for_text_and_rects() {
        let scene = sample_scene();
        let damage = Rect::new(0, 0, 32, 24);

        let mut cpu_fb = Framebuffer::new_for_test(32, 24);
        let mut gpu_fb = Framebuffer::new_for_test(32, 24);
        let mut cpu_renderer = CpuRenderer::new();
        let mut gpu_renderer = GpuRenderer::new(32, 24);
        let mut cpu_text = TextSystem::new();
        let mut gpu_text = TextSystem::new();

        cpu_renderer.render_scene(&mut cpu_fb, damage, 0, 0, &scene, &mut cpu_text);
        gpu_renderer.render_scene(&mut gpu_fb, damage, 0, 0, &scene, &mut gpu_text);

        assert_eq!(cpu_fb.front_buffer(), gpu_fb.front_buffer());
    }

    #[test]
    fn compile_render_frame_coalesces_adjacent_objects_with_same_state() {
        let scene = sample_scene();
        let frame = compile_render_frame(&scene.render_objects);

        assert_eq!(frame.batches.len(), 2);
        assert_eq!(frame.batches[0].range, 0..1);
        assert_eq!(frame.batches[1].range, 1..2);

        let mut duplicated = scene.render_objects.clone();
        duplicated.push(RenderObject {
            object_id: 3,
            bounds: Rect::new(8, 8, 12, 12),
            clip: None,
            z_index: 2,
            opacity: u8::MAX,
            lane: DamageLane::Text,
            kind: RenderObjectKind::TextRun {
                blob_id: 1,
                text: alloc::string::String::from("UI"),
                color: 0xFFFF_FFFF,
                style: TextRunStyle::Ui,
                max_width: 12,
            },
        });
        let frame = compile_render_frame(&duplicated);
        assert_eq!(frame.batches.len(), 2);
        assert_eq!(frame.batches[1].range, 1..3);
    }

    fn rounded_rect_scene() -> SceneUpdate {
        SceneUpdate {
            root_id: 7,
            revision: 1,
            render_objects: vec![RenderObject {
                object_id: 99,
                bounds: Rect::new(4, 4, 56, 28),
                clip: None,
                z_index: 0,
                opacity: u8::MAX,
                lane: DamageLane::Shell,
                kind: RenderObjectKind::SolidRect {
                    color: 0xFF102438,
                    corner_radius: 14,
                },
            }],
            damage_hint: vec![Rect::new(0, 0, 64, 40)],
            semantic_root: None,
        }
    }

    #[test]
    fn clipped_rounded_rect_matches_full_render_cpu() {
        let scene = rounded_rect_scene();
        let mut full_fb = Framebuffer::new_for_test(64, 40);
        let mut partial_fb = Framebuffer::new_for_test(64, 40);
        let mut full_text = TextSystem::new();
        let mut partial_text = TextSystem::new();
        let mut renderer = CpuRenderer::new();

        renderer.render_scene(
            &mut full_fb,
            Rect::new(0, 0, 64, 40),
            0,
            0,
            &scene,
            &mut full_text,
        );

        let quadrants = [
            Rect::new(0, 0, 32, 20),
            Rect::new(32, 0, 32, 20),
            Rect::new(0, 20, 32, 20),
            Rect::new(32, 20, 32, 20),
        ];
        for damage in quadrants {
            renderer.render_scene(&mut partial_fb, damage, 0, 0, &scene, &mut partial_text);
        }

        assert_eq!(full_fb.front_buffer(), partial_fb.front_buffer());
    }

    #[test]
    fn clipped_rounded_rect_matches_full_render_gpu() {
        let scene = rounded_rect_scene();
        let mut full_fb = Framebuffer::new_for_test(64, 40);
        let mut partial_fb = Framebuffer::new_for_test(64, 40);
        let mut full_text = TextSystem::new();
        let mut partial_text = TextSystem::new();
        let mut full_renderer = GpuRenderer::new(64, 40);
        let mut partial_renderer = GpuRenderer::new(64, 40);

        full_renderer.render_scene(
            &mut full_fb,
            Rect::new(0, 0, 64, 40),
            0,
            0,
            &scene,
            &mut full_text,
        );

        let quadrants = [
            Rect::new(0, 0, 32, 20),
            Rect::new(32, 0, 32, 20),
            Rect::new(0, 20, 32, 20),
            Rect::new(32, 20, 32, 20),
        ];
        for damage in quadrants {
            partial_renderer.render_scene(&mut partial_fb, damage, 0, 0, &scene, &mut partial_text);
        }

        assert_eq!(full_fb.front_buffer(), partial_fb.front_buffer());
    }

    #[test]
    fn gpu_raster_draws_translated_small_blobs_below_origin() {
        let mut backend = GpuBackend::Software(SoftwareGal::new(64, 64));
        let mut text_system = TextSystem::new();
        let object = RenderObject {
            object_id: 99,
            bounds: Rect::new(20, 20, 2, 2),
            clip: None,
            z_index: 0,
            opacity: u8::MAX,
            lane: DamageLane::Text,
            kind: RenderObjectKind::Raster {
                width: 2,
                height: 2,
                pixels: vec![0xFFFF_0000; 4],
            },
        };

        draw_render_object_gpu(
            &mut backend,
            Rect::new(0, 0, 64, 64),
            0,
            0,
            &object,
            &mut text_system,
        );

        assert_eq!(GpuPixelBackend::pixel(&backend, 20, 20), Some(0xFFFF_0000));
        assert_eq!(GpuPixelBackend::pixel(&backend, 21, 21), Some(0xFFFF_0000));
    }
}
