use crate::gui::protocol::{
    AtlasId, DamageLane, Rect, RenderObject, RenderObjectKind, TextBlobId, TextRunStyle,
};
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use cosmic_text::{
    fontdb::Database, Attrs, Buffer, CacheKey, CacheKeyFlags, Color, Family,
    FontSystem as CosmicFontSystem, Metrics, Shaping, Wrap,
};
use libm::{ceilf, roundf};
use swash::scale::image::{Content as SwashContent, Image as SwashImage};
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::{Format, Vector};

const UI_FONT_SIZE: f32 = 15.0;
const UI_LINE_HEIGHT: f32 = 19.0;
const MONO_FONT_SIZE: f32 = 14.0;
const MONO_LINE_HEIGHT: f32 = 18.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TextFamily {
    Ui,
    Mono,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextStyle {
    family: TextFamily,
    font_size_bits: u32,
    line_height_bits: u32,
}

impl TextStyle {
    pub const fn ui() -> Self {
        Self {
            family: TextFamily::Ui,
            font_size_bits: UI_FONT_SIZE.to_bits(),
            line_height_bits: UI_LINE_HEIGHT.to_bits(),
        }
    }

    pub const fn mono() -> Self {
        Self {
            family: TextFamily::Mono,
            font_size_bits: MONO_FONT_SIZE.to_bits(),
            line_height_bits: MONO_LINE_HEIGHT.to_bits(),
        }
    }

    pub const fn family(self) -> TextFamily {
        self.family
    }

    pub fn font_size(self) -> f32 {
        f32::from_bits(self.font_size_bits)
    }

    pub fn line_height(self) -> f32 {
        f32::from_bits(self.line_height_bits)
    }
}

impl From<TextRunStyle> for TextStyle {
    fn from(value: TextRunStyle) -> Self {
        match value {
            TextRunStyle::Ui => TextStyle::ui(),
            TextRunStyle::Mono => TextStyle::mono(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextBlob {
    pub id: TextBlobId,
    pub atlas_id: AtlasId,
    pub text: String,
    pub width_px: u32,
    pub height_px: u32,
    pub pixels: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct TextCacheKey {
    style: TextStyle,
    color: u32,
    max_width: u32,
    text: String,
}

pub struct FontRegistry {
    font_system: CosmicFontSystem,
}

impl FontRegistry {
    pub fn new() -> Self {
        let mut db = Database::new();
        db.load_font_data(include_bytes!("../../assets/fonts/Roboto.ttf").to_vec());
        db.load_font_data(include_bytes!("../../assets/fonts/RobotoMono.ttf").to_vec());
        db.set_sans_serif_family("Roboto");
        db.set_serif_family("Roboto");
        db.set_monospace_family("Roboto Mono");
        Self {
            font_system: CosmicFontSystem::new_with_locale_and_db(String::from("en-US"), db),
        }
    }

    pub fn system(&mut self) -> &mut CosmicFontSystem {
        &mut self.font_system
    }

    pub fn loaded_faces(&self) -> usize {
        self.font_system.db().faces().count()
    }
}

impl Default for FontRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub struct GlyphAtlas {
    atlas_id: AtlasId,
    next_blob_id: TextBlobId,
    entries: BTreeMap<TextCacheKey, TextBlob>,
}

impl GlyphAtlas {
    pub fn new(atlas_id: AtlasId) -> Self {
        Self {
            atlas_id,
            next_blob_id: 1,
            entries: BTreeMap::new(),
        }
    }

    fn lookup(&self, key: &TextCacheKey) -> Option<TextBlob> {
        self.entries.get(key).cloned()
    }

    fn store(&mut self, key: TextCacheKey, mut blob: TextBlob) -> TextBlob {
        blob.id = self.next_blob_id;
        blob.atlas_id = self.atlas_id;
        self.next_blob_id = self.next_blob_id.saturating_add(1);
        self.entries.insert(key, blob.clone());
        blob
    }
}

pub struct TextSystem {
    registry: FontRegistry,
    swash_cache: SwashRasterizer,
    atlas: GlyphAtlas,
}

impl TextSystem {
    pub fn new() -> Self {
        Self {
            registry: FontRegistry::new(),
            swash_cache: SwashRasterizer::new(),
            atlas: GlyphAtlas::new(1),
        }
    }

    pub fn loaded_faces(&self) -> usize {
        self.registry.loaded_faces()
    }

    pub fn layout_text(&mut self, text: &str, max_width: u32) -> TextBlob {
        self.layout_text_with_style(text, max_width, TextStyle::ui(), 0xFFFF_FFFF)
    }

    pub fn layout_text_with_style(
        &mut self,
        text: &str,
        max_width: u32,
        style: TextStyle,
        color: u32,
    ) -> TextBlob {
        let key = TextCacheKey {
            style,
            color,
            max_width,
            text: text.to_string(),
        };
        if let Some(blob) = self.atlas.lookup(&key) {
            return blob;
        }

        let metrics = Metrics::new(style.font_size(), style.line_height());
        let mut buffer = Buffer::new(self.registry.system(), metrics);
        {
            let mut buffer = buffer.borrow_with(self.registry.system());
            buffer.set_wrap(Wrap::None);
            if max_width > 0 {
                buffer.set_size(Some(max_width as f32), None);
            }
            let attrs = Attrs::new().family(match style.family() {
                TextFamily::Ui => Family::SansSerif,
                TextFamily::Mono => Family::Monospace,
            });
            buffer.set_text(text, &attrs, Shaping::Advanced, None);
            buffer.shape_until_scroll(true);
        }

        let mut width_px = 1u32;
        let mut height_px = ceilf(style.line_height()) as u32;
        for run in buffer.layout_runs() {
            width_px = width_px.max(ceilf(run.line_w) as u32 + 4);
            height_px = height_px.max(ceilf(run.line_top + run.line_height) as u32 + 4);
        }
        if max_width > 0 {
            width_px = width_px.min(max_width.max(1));
        }

        let mut pixels = vec![0u32; width_px.saturating_mul(height_px) as usize];
        for run in buffer.layout_runs() {
            for glyph in run.glyphs.iter() {
                let physical = glyph.physical((0.0, run.line_y), 1.0);
                let glyph_color = glyph.color_opt.unwrap_or(Color(color));
                self.swash_cache.with_pixels(
                    self.registry.system(),
                    physical.cache_key,
                    glyph_color,
                    |x, y, source| {
                        let px = physical.x + x;
                        let py = physical.y + y;
                        if px < 0 || py < 0 || px >= width_px as i32 || py >= height_px as i32 {
                            return;
                        }
                        let index = py as usize * width_px as usize + px as usize;
                        pixels[index] = blend_source_over(pixels[index], source.0);
                    },
                );
            }
        }

        self.atlas.store(
            key,
            TextBlob {
                id: 0,
                atlas_id: 0,
                text: text.to_string(),
                width_px,
                height_px,
                pixels,
            },
        )
    }

    pub fn text_object(
        &mut self,
        object_id: u64,
        bounds: Rect,
        z_index: u32,
        text: &str,
        color: u32,
    ) -> RenderObject {
        self.text_object_with_style(
            object_id,
            bounds,
            z_index,
            text,
            color,
            TextRunStyle::Ui,
        )
    }

    pub fn text_object_with_style(
        &mut self,
        object_id: u64,
        bounds: Rect,
        z_index: u32,
        text: &str,
        color: u32,
        style: TextRunStyle,
    ) -> RenderObject {
        let blob =
            self.layout_text_with_style(text, bounds.width.max(1), TextStyle::from(style), color);
        RenderObject {
            object_id,
            bounds: Rect::new(
                bounds.x,
                bounds.y,
                blob.width_px.max(1),
                blob.height_px.max(1),
            ),
            clip: None,
            z_index,
            opacity: u8::MAX,
            lane: DamageLane::Text,
            kind: RenderObjectKind::GlyphRun {
                blob_id: blob.id,
                atlas_id: blob.atlas_id,
                width: blob.width_px.max(1),
                height: blob.height_px.max(1),
                pixels: blob.pixels,
                color,
            },
        }
    }
}

impl Default for TextSystem {
    fn default() -> Self {
        Self::new()
    }
}

fn blend_source_over(background: u32, source: u32) -> u32 {
    let alpha = ((source >> 24) & 0xFF) as u32;
    if alpha == 0 {
        return background;
    }
    if alpha == 255 {
        return source;
    }

    let inv_alpha = 255u32.saturating_sub(alpha);
    let br = (background >> 16) & 0xFF;
    let bg = (background >> 8) & 0xFF;
    let bb = background & 0xFF;
    let fr = (source >> 16) & 0xFF;
    let fg = (source >> 8) & 0xFF;
    let fb = source & 0xFF;

    let r = (fr * alpha + br * inv_alpha) / 255;
    let g = (fg * alpha + bg * inv_alpha) / 255;
    let b = (fb * alpha + bb * inv_alpha) / 255;
    0xFF00_0000 | (r << 16) | (g << 8) | b
}

struct SwashRasterizer {
    context: ScaleContext,
    image_cache: BTreeMap<CacheKey, Option<SwashImage>>,
}

impl SwashRasterizer {
    fn new() -> Self {
        Self {
            context: ScaleContext::new(),
            image_cache: BTreeMap::new(),
        }
    }

    fn with_pixels<F: FnMut(i32, i32, Color)>(
        &mut self,
        font_system: &mut CosmicFontSystem,
        cache_key: CacheKey,
        base: Color,
        mut callback: F,
    ) {
        if let Some(image) = self.get_image(font_system, cache_key) {
            let x = image.placement.left;
            let y = -image.placement.top;
            match image.content {
                SwashContent::Mask => {
                    let mut index = 0usize;
                    for off_y in 0..image.placement.height as i32 {
                        for off_x in 0..image.placement.width as i32 {
                            callback(
                                x + off_x,
                                y + off_y,
                                Color(
                                    (u32::from(image.data[index]) << 24) | (base.0 & 0x00FF_FFFF),
                                ),
                            );
                            index += 1;
                        }
                    }
                }
                SwashContent::Color => {
                    let mut index = 0usize;
                    for off_y in 0..image.placement.height as i32 {
                        for off_x in 0..image.placement.width as i32 {
                            callback(
                                x + off_x,
                                y + off_y,
                                Color::rgba(
                                    image.data[index],
                                    image.data[index + 1],
                                    image.data[index + 2],
                                    image.data[index + 3],
                                ),
                            );
                            index += 4;
                        }
                    }
                }
                SwashContent::SubpixelMask => {}
            }
        }
    }

    fn get_image(
        &mut self,
        font_system: &mut CosmicFontSystem,
        cache_key: CacheKey,
    ) -> Option<&SwashImage> {
        self.image_cache
            .entry(cache_key)
            .or_insert_with(|| swash_image(font_system, &mut self.context, cache_key))
            .as_ref()
    }
}

fn swash_image(
    font_system: &mut CosmicFontSystem,
    context: &mut ScaleContext,
    cache_key: CacheKey,
) -> Option<SwashImage> {
    let font = font_system.get_font(cache_key.font_id, cache_key.font_weight)?;
    let swash_font = swash::FontRef::from_index(font.data(), 0)?;
    let variable_weight = swash_font
        .variations()
        .find_by_tag(swash::Tag::from_be_bytes(*b"wght"));

    let mut scaler = context
        .builder(swash_font)
        .size(f32::from_bits(cache_key.font_size_bits))
        .hint(!cache_key.flags.contains(CacheKeyFlags::DISABLE_HINTING));
    if let Some(variation) = variable_weight {
        scaler = scaler.variations(core::iter::once(swash::Setting {
            tag: swash::Tag::from_be_bytes(*b"wght"),
            value: f32::from(cache_key.font_weight.0)
                .clamp(variation.min_value(), variation.max_value()),
        }));
    }
    let mut scaler = scaler.build();

    let offset = if cache_key.flags.contains(CacheKeyFlags::PIXEL_FONT) {
        Vector::new(
            roundf(cache_key.x_bin.as_float()) + 1.0,
            roundf(cache_key.y_bin.as_float()),
        )
    } else {
        Vector::new(cache_key.x_bin.as_float(), cache_key.y_bin.as_float())
    };

    Render::new(&[
        Source::ColorOutline(0),
        Source::ColorBitmap(StrikeWith::BestFit),
        Source::Outline,
    ])
    .format(Format::Alpha)
    .offset(offset)
    .transform(if cache_key.flags.contains(CacheKeyFlags::FAKE_ITALIC) {
        Some(swash::zeno::Transform::skew(
            swash::zeno::Angle::from_degrees(14.0),
            swash::zeno::Angle::from_degrees(0.0),
        ))
    } else {
        None
    })
    .render(&mut scaler, cache_key.glyph_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_reuses_existing_blob_for_same_text() {
        let mut system = TextSystem::new();
        let a = system.layout_text("echOS", 256);
        let b = system.layout_text("echOS", 256);
        assert_eq!(a.id, b.id);
        assert!(a.pixels.iter().any(|pixel| (*pixel >> 24) != 0));
    }

    #[test]
    fn text_object_is_rasterized_with_cosmic_text() {
        let mut system = TextSystem::new();
        let object = system.text_object(7, Rect::new(10, 20, 196, 24), 3, "Panel Ş", 0xFFFFFFFF);
        assert_eq!(object.object_id, 7);
        assert_eq!(object.z_index, 3);
        match object.kind {
            RenderObjectKind::GlyphRun {
                width,
                height,
                pixels,
                ..
            } => {
                assert!(width > 0);
                assert!(height > 0);
                assert!(pixels.iter().any(|pixel| (*pixel >> 24) != 0));
            }
            other => panic!("unexpected render object: {other:?}"),
        }
    }

    #[test]
    fn bundled_font_database_exposes_multiple_faces_for_fallback() {
        let system = TextSystem::new();
        assert!(system.loaded_faces() >= 2);
    }
}
