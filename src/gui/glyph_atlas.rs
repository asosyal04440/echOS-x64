//! # Alt Piksel Kenar Yumuşatmalı Glyph Atlası
//!
//! Önbelleğe alınmış glyph bitmap'leri kullanarak verimli metin render
//! Keskin metin için alt piksel render destekler (ClearType benzeri)

use alloc::collections::BTreeMap;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use alloc::vec;
use alloc::boxed::Box;
use core::cmp::{min, max};
use spin::Mutex;
use libm::ceilf;

// ============================================================================
// GLYPH ATLASI SABİTLERİ
// ============================================================================

/// Piksel cinsinden atlas genişliği
pub const ATLAS_WIDTH: u16 = 1024;

/// Piksel cinsinden atlas yüksekliği
pub const ATLAS_HEIGHT: u16 = 1024;

/// Maksimum atlas sayısı
pub const MAX_ATLASES: usize = 4;

/// Maksimum glyph önbellek girişi
pub const MAX_GLYPH_CACHE: usize = 4096;

/// Glyph'ler etrafındaki dolgu (piksel)
pub const GLYPH_PADDING: u8 = 1;

// ============================================================================
// YAZI TİPİ STİLİ
// ============================================================================

/// Yazı tipi stil bayrakları
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FontStyle {
    pub weight: FontWeight,
    pub style: FontStyleType,
    pub size: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontWeight {
    Thin = 100,
    ExtraLight = 200,
    Light = 300,
    Regular = 400,
    Medium = 500,
    SemiBold = 600,
    Bold = 700,
    ExtraBold = 800,
    Black = 900,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontStyleType {
    Normal,
    Italic,
    Oblique,
}

impl FontStyle {
    pub fn regular(size: u16) -> Self {
        FontStyle {
            weight: FontWeight::Regular,
            style: FontStyleType::Normal,
            size,
        }
    }
    
    pub fn bold(size: u16) -> Self {
        FontStyle {
            weight: FontWeight::Bold,
            style: FontStyleType::Normal,
            size,
        }
    }
    
    pub fn italic(size: u16) -> Self {
        FontStyle {
            weight: FontWeight::Regular,
            style: FontStyleType::Italic,
            size,
        }
    }
}

// ============================================================================
// GLYPH ANAHTARI
// ============================================================================

/// Glyph önbellek araması için anahtar
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GlyphKey {
    /// Unicode kod noktası
    pub codepoint: u32,
    /// Yazı tipi boyutu
    pub size: u16,
    /// Yazı tipi ağırlığı (0-9 arası 100-900'e eşlenir)
    pub weight: u8,
    /// Stil bayrakları
    pub style_flags: u8,
}

impl GlyphKey {
    pub fn new(codepoint: char, style: &FontStyle) -> Self {
        GlyphKey {
            codepoint: codepoint as u32,
            size: style.size,
            weight: (style.weight as u16 / 100) as u8,
            style_flags: match style.style {
                FontStyleType::Normal => 0,
                FontStyleType::Italic => 1,
                FontStyleType::Oblique => 2,
            },
        }
    }
}

// ============================================================================
// GLYPH BİLGİSİ
// ============================================================================

/// Önbelleğe alınmış glyph bilgisi
#[derive(Clone, Copy, Debug)]
pub struct GlyphInfo {
    /// Atlas içindeki konum (x, y)
    pub atlas_x: u16,
    pub atlas_y: u16,
    /// Atlas indeksi (çoklu atlas için)
    pub atlas_index: u8,
    /// Piksel cinsinden glyph genişliği
    pub width: u16,
    /// Piksel cinsinden glyph yüksekliği
    pub height: u16,
    /// Yatay ilerleme (piksel, 16.16 sabit nokta)
    pub advance: i32,
    /// Sol taşma (piksel, 16.16 sabit nokta)
    pub bearing_x: i32,
    /// Üst taşma (piksel, 16.16 sabit nokta)
    pub bearing_y: i32,
    /// Bu renkli bir glyph mi (emoji)
    pub is_colored: bool,
    /// LRU zaman damgası
    pub last_used: u64,
}

impl GlyphInfo {
    pub fn new() -> Self {
        GlyphInfo {
            atlas_x: 0,
            atlas_y: 0,
            atlas_index: 0,
            width: 0,
            height: 0,
            advance: 0,
            bearing_x: 0,
            bearing_y: 0,
            is_colored: false,
            last_used: 0,
        }
    }
    
    /// Render için sınırlayıcı kutuyu al
    pub fn bounds(&self, x: i32, y: i32) -> (i32, i32, i32, i32) {
        let min_x = x + (self.bearing_x >> 16);
        let min_y = y - (self.bearing_y >> 16);
        let max_x = min_x + self.width as i32;
        let max_y = min_y + self.height as i32;
        (min_x, min_y, max_x, max_y)
    }
}

// ============================================================================
// GLYPH BİTMAP
// ============================================================================

/// Render edilmiş glyph bitmap
#[derive(Clone, Debug)]
pub struct GlyphBitmap {
    /// Piksel cinsinden genişlik
    pub width: u16,
    /// Piksel cinsinden yükseklik
    pub height: u16,
    /// Adım (satır başına bayt)
    pub pitch: i32,
    /// Piksel verisi (gri tonlama veya BGRA)
    pub data: Vec<u8>,
    /// Renkli mi (BGRA formatı)
    pub is_colored: bool,
}

impl GlyphBitmap {
    pub fn new(width: u16, height: u16, is_colored: bool) -> Self {
        let pitch = if is_colored { width as i32 * 4 } else { width as i32 };
        GlyphBitmap {
            width,
            height,
            pitch,
            data: vec![0; pitch as usize * height as usize],
            is_colored,
        }
    }
    
    /// Konumdaki gri tonlama değerini al
    pub fn get_grayscale(&self, x: u16, y: u16) -> u8 {
        if x >= self.width || y >= self.height || self.is_colored {
            return 0;
        }
        self.data[y as usize * self.pitch as usize + x as usize]
    }
    
    /// Konumdaki BGRA değerini al
    pub fn get_bgra(&self, x: u16, y: u16) -> (u8, u8, u8, u8) {
        if x >= self.width || y >= self.height || !self.is_colored {
            return (0, 0, 0, 0);
        }
        let offset = y as usize * self.pitch as usize + x as usize * 4;
        (self.data[offset], self.data[offset + 1], self.data[offset + 2], self.data[offset + 3])
    }
    
    /// Konumdaki alt piksel kapsamını al (3x yatay çözünürlük)
    pub fn get_subpixel(&self, x: u16, y: u16, subpixel: u8) -> u8 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        
        // Alt piksel render için 3x yatay çözünürlük gerekir
        // Bu genellikle yazı tipi rasterleştirici tarafından sağlanır
        // Burada örnekleme ile yaklaşıyoruz
        let base = self.get_grayscale(x, y);
        
        // Basit alt piksel yaklaşımı
        match subpixel {
            0 => base.saturating_mul(3) / 4, // Sol alt piksel
            1 => base,                        // Merkez alt piksel
            2 => base.saturating_mul(3) / 4, // Sağ alt piksel
            _ => base,
        }
    }
}

// ============================================================================
// ALT PİKSEL DÜZENİ
// ============================================================================

/// LCD alt piksel düzeni
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubpixelLayout {
    /// Standart RGB yatay
    RgbHorizontal,
    /// BGR yatay
    BgrHorizontal,
    /// RGB dikey
    RgbVertical,
    /// BGR dikey
    BgrVertical,
    /// Alt piksel yok (OLED, bilinmiyor)
    None,
}

impl SubpixelLayout {
    /// Varsayılan düzeni al
    pub fn default_layout() -> Self {
        SubpixelLayout::RgbHorizontal
    }
    
    /// Konum için alt piksel rengini al
    pub fn subpixel_color(&self, x: i32) -> (u8, u8, u8) {
        match self {
            SubpixelLayout::RgbHorizontal => {
                match x % 3 {
                    0 => (255, 0, 0),   // Kırmızı
                    1 => (0, 255, 0),   // Yeşil
                    2 => (0, 0, 255),   // Mavi
                    _ => (255, 255, 255),
                }
            }
            SubpixelLayout::BgrHorizontal => {
                match x % 3 {
                    0 => (0, 0, 255),   // Mavi
                    1 => (0, 255, 0),   // Yeşil
                    2 => (255, 0, 0),   // Kırmızı
                    _ => (255, 255, 255),
                }
            }
            SubpixelLayout::RgbVertical => {
                match x % 3 {
                    0 => (255, 0, 0),
                    1 => (0, 255, 0),
                    2 => (0, 0, 255),
                    _ => (255, 255, 255),
                }
            }
            SubpixelLayout::BgrVertical => {
                match x % 3 {
                    0 => (0, 0, 255),
                    1 => (0, 255, 0),
                    2 => (255, 0, 0),
                    _ => (255, 255, 255),
                }
            }
            SubpixelLayout::None => (255, 255, 255),
        }
    }
}

// ============================================================================
// GLYPH ATLASI
// ============================================================================

/// Tek glyph atlas dokusu
pub struct GlyphAtlas {
    /// Atlas doku verisi (RGBA)
    texture: Vec<u32>,
    /// Piksel cinsinden genişlik
    width: u16,
    /// Piksel cinsinden yükseklik
    height: u16,
    /// Sonraki kullanılabilir X konumu
    next_x: u16,
    /// Sonraki kullanılabilir Y konumu
    next_y: u16,
    /// Mevcut satır yüksekliği
    row_height: u16,
    /// Depolanan glyph sayısı
    glyph_count: usize,
}

impl GlyphAtlas {
    pub fn new() -> Self {
        GlyphAtlas {
            texture: vec![0; ATLAS_WIDTH as usize * ATLAS_HEIGHT as usize],
            width: ATLAS_WIDTH,
            height: ATLAS_HEIGHT,
            next_x: 0,
            next_y: 0,
            row_height: 0,
            glyph_count: 0,
        }
    }
    
    /// Atlas'ta glyph için alan var mı kontrol et
    pub fn has_space(&self, width: u16, height: u16) -> bool {
        let padded_w = width + GLYPH_PADDING as u16 * 2;
        let padded_h = height + GLYPH_PADDING as u16 * 2;
        
        // Mevcut satırı kontrol et
        if self.next_x + padded_w <= self.width {
            return true;
        }
        
        // Yeni satırı kontrol et
        if self.next_y + self.row_height + padded_h <= self.height {
            return true;
        }
        
        false
    }
    
    /// Glyph için alan tahsis et
    pub fn allocate(&mut self, width: u16, height: u16) -> Option<(u16, u16)> {
        let padded_w = width + GLYPH_PADDING as u16 * 2;
        let padded_h = height + GLYPH_PADDING as u16 * 2;
        
        // Mevcut satırı dene
        if self.next_x + padded_w <= self.width {
            let x = self.next_x + GLYPH_PADDING as u16;
            let y = self.next_y + self.row_height + GLYPH_PADDING as u16;
            
            self.next_x += padded_w;
            self.row_height = max(self.row_height, padded_h);
            self.glyph_count += 1;
            
            return Some((x, y));
        }
        
        // Yeni satır başlat
        if self.next_y + self.row_height + padded_h <= self.height {
            self.next_y += self.row_height;
            self.next_x = padded_w;
            self.row_height = padded_h;
            self.glyph_count += 1;
            
            let x = GLYPH_PADDING as u16;
            let y = self.next_y + GLYPH_PADDING as u16;
            
            return Some((x, y));
        }
        
        None
    }
    
    /// Glyph bitmap'ini atlasa kopyala
    pub fn copy_glyph(&mut self, x: u16, y: u16, bitmap: &GlyphBitmap) {
        for row in 0..bitmap.height {
            let dst_y = y + row;
            if dst_y >= self.height {
                break;
            }
            
            for col in 0..bitmap.width {
                let dst_x = x + col;
                if dst_x >= self.width {
                    break;
                }
                
                let pixel = if bitmap.is_colored {
                    let (b, g, r, a) = bitmap.get_bgra(col, row);
                    // 0xAABBGGRR formatına dönüştür
                    ((a as u32) << 24) | ((b as u32) << 16) | ((g as u32) << 8) | (r as u32)
                } else {
                    let alpha = bitmap.get_grayscale(col, row);
                    // Alfa kanallı beyaz glyph
                    ((alpha as u32) << 24) | 0x00FFFFFF
                };
                
                let idx = dst_y as usize * self.width as usize + dst_x as usize;
                self.texture[idx] = pixel;
            }
        }
    }
    
    /// Atlas'tan piksel al
    pub fn get_pixel(&self, x: u16, y: u16) -> u32 {
        if x >= self.width || y >= self.height {
            return 0;
        }
        self.texture[y as usize * self.width as usize + x as usize]
    }
    
    /// Doku verisini al
    pub fn texture(&self) -> &[u32] {
        &self.texture
    }
    
    /// Boyutları al
    pub fn dimensions(&self) -> (u16, u16) {
        (self.width, self.height)
    }
    
    /// Glyph sayısını al
    pub fn count(&self) -> usize {
        self.glyph_count
    }
    
    /// Atlas boş mu kontrol et
    pub fn is_empty(&self) -> bool {
        self.glyph_count == 0
    }
    
    /// Atlası temizle
    pub fn clear(&mut self) {
        self.texture.fill(0);
        self.next_x = 0;
        self.next_y = 0;
        self.row_height = 0;
        self.glyph_count = 0;
    }
}

// ============================================================================
// GLYPH ATLASI YÖNETİCİSİ
// ============================================================================

/// LRU çıkarmalı çoklu glyph atlaslarını yönetir
pub struct GlyphAtlasManager {
    /// Tüm atlaslar
    atlases: Vec<GlyphAtlas>,
    /// Mevcut atlas indeksi
    current_atlas: usize,
    /// Glyph önbelleği (anahtar -> bilgi)
    cache: BTreeMap<GlyphKey, GlyphInfo>,
    /// LRU erişim sırası
    lru: VecDeque<GlyphKey>,
    /// LRU için kare sayıcı
    frame: u64,
    /// Alt piksel düzeni
    subpixel_layout: SubpixelLayout,
    /// Alt piksel renderi etkinleştir
    subpixel_enabled: bool,
    /// Önbellek isabetleri
    hits: u64,
    /// Önbellek ıskaları
    misses: u64,
}

impl GlyphAtlasManager {
    pub fn new() -> Self {
        let mut manager = GlyphAtlasManager {
            atlases: Vec::new(),
            current_atlas: 0,
            cache: BTreeMap::new(),
            lru: VecDeque::new(),
            frame: 0,
            subpixel_layout: SubpixelLayout::default_layout(),
            subpixel_enabled: true,
            hits: 0,
            misses: 0,
        };
        
        // İlk atlası oluştur
        manager.atlases.push(GlyphAtlas::new());
        
        manager
    }
    
    /// Alt piksel renderi etkinleştir/devre dışı bırak
    pub fn set_subpixel(&mut self, enabled: bool) {
        self.subpixel_enabled = enabled;
    }
    
    /// Alt piksel düzenini ayarla
    pub fn set_subpixel_layout(&mut self, layout: SubpixelLayout) {
        self.subpixel_layout = layout;
    }
    
    /// Glyph al veya render et
    pub fn get_glyph(&mut self, key: GlyphKey, rasterizer: &mut dyn GlyphRasterizer) -> Option<GlyphInfo> {
        // Önbelleği kontrol et
        if let Some(mut info) = self.cache.get(&key).copied() {
            // Önbellek isabeti
            self.hits += 1;
            info.last_used = self.frame;
            self.cache.insert(key, info);
            self.touch_lru(key);
            return Some(info);
        }
        
        // Önbellek ıskandı
        self.misses += 1;
        
        // Glyph'i render et
        let bitmap = rasterizer.render(key.codepoint, key.size, key.weight, key.style_flags)?;
        
        // Mevcut atlasta alan bul
        let mut atlas_idx = self.current_atlas;
        let mut pos = None;
        
        if let Some(atlas) = self.atlases.get_mut(atlas_idx) {
            pos = atlas.allocate(bitmap.width, bitmap.height);
        }
        
        // Alan yoksa, diğer atlasları dene ya da yeni oluştur
        if pos.is_none() {
            for (i, atlas) in self.atlases.iter_mut().enumerate() {
                if i != self.current_atlas {
                    pos = atlas.allocate(bitmap.width, bitmap.height);
                    if pos.is_some() {
                        atlas_idx = i;
                        break;
                    }
                }
            }
        }
        
        // Gerekirse yeni atlas oluştur
        if pos.is_none() && self.atlases.len() < MAX_ATLASES {
            let mut new_atlas = GlyphAtlas::new();
            pos = new_atlas.allocate(bitmap.width, bitmap.height);
            self.atlases.push(new_atlas);
            atlas_idx = self.atlases.len() - 1;
        }
        
        // Hâlâ alan yoksa LRU'yu çıkar
        if pos.is_none() {
            self.evict_lru();
            if let Some(atlas) = self.atlases.get_mut(atlas_idx) {
                pos = atlas.allocate(bitmap.width, bitmap.height);
            }
        }
        
        // Konumu al
        let (x, y) = pos?;
        
        // Atlasa kopyala
        if let Some(atlas) = self.atlases.get_mut(atlas_idx) {
            atlas.copy_glyph(x, y, &bitmap);
        }
        
        // Bilgi oluştur
        let info = GlyphInfo {
            atlas_x: x,
            atlas_y: y,
            atlas_index: atlas_idx as u8,
            width: bitmap.width,
            height: bitmap.height,
            advance: rasterizer.get_advance(key.codepoint, key.size),
            bearing_x: rasterizer.get_bearing_x(key.codepoint, key.size),
            bearing_y: rasterizer.get_bearing_y(key.codepoint, key.size),
            is_colored: bitmap.is_colored,
            last_used: self.frame,
        };
        
        // Önbelleğe al
        self.cache.insert(key, info);
        self.lru.push_back(key);
        
        // Önbellek boyutunu kontrol et
        if self.cache.len() > MAX_GLYPH_CACHE {
            self.evict_lru();
        }
        
        Some(info)
    }
    
    /// LRU girdisini güncelle
    fn touch_lru(&mut self, key: GlyphKey) {
        self.lru.retain(|&k| k != key);
        self.lru.push_back(key);
    }
    
    /// En az kullanılan glyph'leri çıkar
    fn evict_lru(&mut self) {
        if let Some(key) = self.lru.pop_front() {
            if let Some(info) = self.cache.remove(&key) {
                // Atlas bölgesini serbest olarak işaretle (basitleştirilmiş - sadece glyph'i temizle)
                // Gerçek bir uygulamada serbest bölgeleri takip ederiz
                self.cache.remove(&key);
            }
        }
    }
    
    /// Önbelleğe alınmış glyph'leri kullanarak metin render et
    pub fn render_text(
        &mut self,
        text: &str,
        style: &FontStyle,
        mut x: i32,
        y: i32,
        color: u32,
        fb: &mut [u32],
        fb_width: usize,
        fb_height: usize,
        rasterizer: &mut dyn GlyphRasterizer,
    ) -> i32 {
        self.frame += 1;
        
        let mut max_x = x;
        
        for c in text.chars() {
            let key = GlyphKey::new(c, style);
            
            if let Some(info) = self.get_glyph(key, rasterizer) {
                // Render glyph
                self.render_glyph(&info, x, y, color, fb, fb_width, fb_height);
                
                // İlerlet
                x += info.advance >> 16;
                max_x = max(max_x, x);
            }
        }
        
        max_x
    }
    
    /// Tek bir glyph render et
    pub fn render_glyph(
        &self,
        info: &GlyphInfo,
        x: i32,
        y: i32,
        color: u32,
        fb: &mut [u32],
        fb_width: usize,
        fb_height: usize,
    ) {
        let atlas = match self.atlases.get(info.atlas_index as usize) {
            Some(a) => a,
            None => return,
        };
        
        // Konumu hesapla
        let draw_x = x + (info.bearing_x >> 16);
        let draw_y = y - (info.bearing_y >> 16);
        
        // Renk bileşenlerini çıkar
        let cr = ((color >> 16) & 0xFF) as u8;
        let cg = ((color >> 8) & 0xFF) as u8;
        let cb = (color & 0xFF) as u8;
        
        for gy in 0..info.height {
            let fb_y = draw_y + gy as i32;
            if fb_y < 0 || fb_y >= fb_height as i32 {
                continue;
            }
            
            for gx in 0..info.width {
                let fb_x = draw_x + gx as i32;
                if fb_x < 0 || fb_x >= fb_width as i32 {
                    continue;
                }
                
                let atlas_pixel = atlas.get_pixel(info.atlas_x + gx, info.atlas_y + gy);
                let alpha = ((atlas_pixel >> 24) & 0xFF) as u8;
                
                if alpha == 0 {
                    continue;
                }
                
                let fb_idx = fb_y as usize * fb_width + fb_x as usize;
                let bg = fb[fb_idx];
                
                // Arka planı çıkar
                let br = ((bg >> 16) & 0xFF) as u8;
                let bg_ = ((bg >> 8) & 0xFF) as u8;
                let bb = (bg & 0xFF) as u8;
                
                // Alfa harmanla
                let a = alpha as u32;
                let inv_a = 255 - a;
                
                let r = ((cr as u32 * a + br as u32 * inv_a) / 255) as u8;
                let g = ((cg as u32 * a + bg_ as u32 * inv_a) / 255) as u8;
                let b = ((cb as u32 * a + bb as u32 * inv_a) / 255) as u8;
                
                fb[fb_idx] = ((r as u32) << 16) | ((g as u32) << 8) | (b as u32);
            }
        }
    }
    
    /// Alt piksel kenar yumuşatma ile render et
    pub fn render_glyph_subpixel(
        &self,
        info: &GlyphInfo,
        x: i32,
        y: i32,
        color: u32,
        fb: &mut [u32],
        fb_width: usize,
        fb_height: usize,
    ) {
        if !self.subpixel_enabled {
            return self.render_glyph(info, x, y, color, fb, fb_width, fb_height);
        }
        
        let atlas = match self.atlases.get(info.atlas_index as usize) {
            Some(a) => a,
            None => return,
        };
        
        let draw_x = x + (info.bearing_x >> 16);
        let draw_y = y - (info.bearing_y >> 16);
        
        // Renk bileşenlerini çıkar
        let cr = ((color >> 16) & 0xFF) as f32;
        let cg = ((color >> 8) & 0xFF) as f32;
        let cb = (color & 0xFF) as f32;
        
        for gy in 0..info.height {
            let fb_y = draw_y + gy as i32;
            if fb_y < 0 || fb_y >= fb_height as i32 {
                continue;
            }
            
            for gx in 0..info.width {
                let fb_x = draw_x + gx as i32;
                if fb_x < 0 || fb_x >= fb_width as i32 {
                    continue;
                }
                
                let atlas_pixel = atlas.get_pixel(info.atlas_x + gx, info.atlas_y + gy);
                let alpha = ((atlas_pixel >> 24) & 0xFF) as f32 / 255.0;
                
                if alpha < 0.01 {
                    continue;
                }
                
                let fb_idx = fb_y as usize * fb_width + fb_x as usize;
                let bg = fb[fb_idx];
                
                // Arka planı çıkar
                let br = ((bg >> 16) & 0xFF) as f32;
                let bg_ = ((bg >> 8) & 0xFF) as f32;
                let bb = (bg & 0xFF) as f32;
                
                // Konuma göre alt piksel ağırlıklarını al
                let (sr, sg, sb) = self.subpixel_layout.subpixel_color(fb_x);
                
                // Alt piksel harmanlamayı uygula
                let r = br * (1.0 - alpha * sr as f32 / 255.0) + cr * alpha * sr as f32 / 255.0;
                let g = bg_ * (1.0 - alpha * sg as f32 / 255.0) + cg * alpha * sg as f32 / 255.0;
                let b = bb * (1.0 - alpha * sb as f32 / 255.0) + cb * alpha * sb as f32 / 255.0;
                
                fb[fb_idx] = ((r as u32).min(255) << 16) | ((g as u32).min(255) << 8) | (b as u32).min(255);
            }
        }
    }
    
    /// Önbellek istatistiklerini al
    pub fn stats(&self) -> (u64, u64, f32) {
        let total = self.hits + self.misses;
        let hit_rate = if total > 0 { self.hits as f32 / total as f32 } else { 0.0 };
        (self.hits, self.misses, hit_rate)
    }
    
    /// Önbellek boyutunu al
    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }
    
    /// Atlas sayısını al
    pub fn atlas_count(&self) -> usize {
        self.atlases.len()
    }
    
    /// Tüm önbellekleri temizle
    pub fn clear(&mut self) {
        for atlas in &mut self.atlases {
            atlas.clear();
        }
        self.cache.clear();
        self.lru.clear();
        self.current_atlas = 0;
        self.hits = 0;
        self.misses = 0;
    }
}

// ============================================================================
// GLYPH RASTERLEŞTİRİCİ TRAIT
// ============================================================================

/// Glyph rasterleştirme için trait
pub trait GlyphRasterizer: Send + Sync {
    /// Glyph'i bitmap'e render et
    fn render(&mut self, codepoint: u32, size: u16, weight: u8, style: u8) -> Option<GlyphBitmap>;
    
    /// Yatay ilerleme al (16.16 sabit nokta)
    fn get_advance(&self, codepoint: u32, size: u16) -> i32;
    
    /// Sol taşmayı al (16.16 sabit nokta)
    fn get_bearing_x(&self, codepoint: u32, size: u16) -> i32;
    
    /// Üst taşmayı al (16.16 sabit nokta)
    fn get_bearing_y(&self, codepoint: u32, size: u16) -> i32;
    
    /// Satır yüksekliğini al
    fn line_height(&self, size: u16) -> i32;
}

// ============================================================================
// GÖMÜLÜ RASTERLEŞTİRİCİ (VGA Yazı Tipi)
// ============================================================================

/// Basit VGA yazı tipi rasterleştiricisi
pub struct VgaFontRasterizer;

impl VgaFontRasterizer {
    pub fn new() -> Self {
        VgaFontRasterizer
    }
    
    /// Karakter için VGA yazı tipi verisi al
    fn get_font_data(c: char) -> [u8; 16] {
        // Crate'in VGA yazı tipini kullan
        crate::font::vga_font::get_font_data(c)
    }
}

impl GlyphRasterizer for VgaFontRasterizer {
    fn render(&mut self, codepoint: u32, size: u16, _weight: u8, _style: u8) -> Option<GlyphBitmap> {
        let c = char::from_u32(codepoint)?;
        
        // VGA yazı tipi 8x16, gerekirse ölekekle
        let scale = size as f32 / 16.0;
        let width = ceilf(8.0 * scale) as u16;
        let height = ceilf(16.0 * scale) as u16;
        
        let mut bitmap = GlyphBitmap::new(width.max(8), height.max(16), false);
        
        let font_data = Self::get_font_data(c);
        
        // Basit en yakın komşu ölekleme
        for row in 0..height {
            let src_row = (row as f32 / scale).min(15.0) as usize;
            let byte = font_data[src_row];
            
            for col in 0..width {
                let src_col = (col as f32 / scale).min(7.0) as usize;
                let bit = (byte >> (7 - src_col)) & 1;
                
                if bit == 1 {
                    let idx = row as usize * bitmap.pitch as usize + col as usize;
                    if idx < bitmap.data.len() {
                        bitmap.data[idx] = 255;
                    }
                }
            }
        }
        
        Some(bitmap)
    }
    
    fn get_advance(&self, _codepoint: u32, size: u16) -> i32 {
        // VGA yazı tipi tek aralıklıdır
        (size as i32 * 8 / 16) << 16 // 8 piksel öleklenmiş
    }
    
    fn get_bearing_x(&self, _codepoint: u32, _size: u16) -> i32 {
        0
    }
    
    fn get_bearing_y(&self, _codepoint: u32, size: u16) -> i32 {
        (size as i32) << 16 // Karakterin üstü
    }
    
    fn line_height(&self, size: u16) -> i32 {
        ((size as f32 * 1.2) as i32) << 16
    }
}

// ============================================================================
// GLOBAL GLYPH ATLASI
// ============================================================================

lazy_static::lazy_static! {
    static ref GLYPH_ATLAS: Mutex<GlyphAtlasManager> = Mutex::new(GlyphAtlasManager::new());
}

/// Glyph atlasını başlat
pub fn init() {
    let mut atlas = GLYPH_ATLAS.lock();
    atlas.set_subpixel(true);
    crate::serial_println!("[FONT] Glyph atlas initialized ({}x{}, subpixel: {:?})", 
        ATLAS_WIDTH, ATLAS_HEIGHT, atlas.subpixel_layout);
}

/// Glyph atlas yöneticisini al
pub fn get_atlas() -> &'static Mutex<GlyphAtlasManager> {
    &GLYPH_ATLAS
}

/// Global atlası kullanarak metin render et
pub fn render_text(
    text: &str,
    style: &FontStyle,
    x: i32,
    y: i32,
    color: u32,
    fb: &mut [u32],
    fb_width: usize,
    fb_height: usize,
) -> i32 {
    let mut atlas = GLYPH_ATLAS.lock();
    let mut rasterizer = VgaFontRasterizer::new();
    atlas.render_text(text, style, x, y, color, fb, fb_width, fb_height, &mut rasterizer)
}
