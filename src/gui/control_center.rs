//! # Kontrol Merkezi
//!
//! macOS tarzı hızlı ayarlar paneli.
//! Wi-Fi, Bluetooth, AirDrop, Ekran, Ses vb. ayarları kolay erişimle sunar.
//!
//! ## Mimari
//! - `ControlTile`: Tanımlayıcı, ad, simge, tür, aktiflik durumu ve değer içeren ayar karesi
//! - `TileType`: Toggle (açma/kapama), Slider (kaydırıcı), Button (buton), Menu (menü)
//! - `ControlGroup`: İlgili karelerin düzen bilgisiyle birleştirilmesi
//! - `GroupLayout`: Row2 (2'li satır), Row3 (3'lü satır), Grid (2×2 ızgara), Column (dikey yığın), Large (büyük kare)
//! - `ControlCenter`: Paneli, animasyonu ve tıklama/sürükleme olaylarını yöneten yapı
//!
//! ## Çizim Algoritması
//! Panel sağ üst köşeden kayarak girer; `animation_progress` 0→1 arası artar.
//! Kaydırıcı karelerinde alt kenarda doluluk çubuğu çizilir.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};

// ============================================================================
// KONTROL MERKEZİ SABİTLERİ
// ============================================================================

/// Kontrol merkezi genişliği (piksel)
pub const CC_WIDTH: usize = 320;

/// Kare boyutu (piksel)
pub const TILE_SIZE: usize = 64;

/// Kareler arası boşluk (piksel)
pub const TILE_SPACING: usize = 8;

// ============================================================================
// KONTROL KARESİ
// ============================================================================

/// Bir kontrol karesi (buton/geçiş anahtarı)
pub struct ControlTile {
    /// Kare kimliği
    pub id: u32,
    /// Görüntü adı
    pub name: String,
    /// Simge
    pub icon: String,
    /// Kare türü
    pub tile_type: TileType,
    /// Aktif/etkin mi
    pub active: bool,
    /// Geçerli değer (kaydırıcılar için)
    pub value: f32,
    /// Alt başlık (durum metni)
    pub subtitle: String,
    /// Aktifken kare rengi
    pub active_color: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileType {
    Toggle,
    Slider,
    Button,
    Menu,
}

impl ControlTile {
    pub fn toggle(id: u32, name: &str, icon: &str) -> Self {
        ControlTile {
            id,
            name: String::from(name),
            icon: String::from(icon),
            tile_type: TileType::Toggle,
            active: false,
            value: 0.0,
            subtitle: String::from("Off"),
            active_color: Theme::ACCENT_PRIMARY.to_u32(),
        }
    }

    pub fn slider(id: u32, name: &str, icon: &str, value: f32) -> Self {
        ControlTile {
            id,
            name: String::from(name),
            icon: String::from(icon),
            tile_type: TileType::Slider,
            active: value > 0.0,
            value,
            subtitle: format!("{}%", (value * 100.0) as i32),
            active_color: Theme::ACCENT_PRIMARY.to_u32(),
        }
    }

    pub fn button(id: u32, name: &str, icon: &str) -> Self {
        ControlTile {
            id,
            name: String::from(name),
            icon: String::from(icon),
            tile_type: TileType::Button,
            active: false,
            value: 0.0,
            subtitle: String::new(),
            active_color: Theme::ACCENT_PRIMARY.to_u32(),
        }
    }

    pub fn menu(id: u32, name: &str, icon: &str, subtitle: &str) -> Self {
        ControlTile {
            id,
            name: String::from(name),
            icon: String::from(icon),
            tile_type: TileType::Menu,
            active: false,
            value: 0.0,
            subtitle: String::from(subtitle),
            active_color: Theme::ACCENT_PRIMARY.to_u32(),
        }
    }

    /// Aktif durumu değiştir
    pub fn toggle_active(&mut self) {
        if self.tile_type == TileType::Toggle {
            self.active = !self.active;
            self.subtitle = if self.active { String::from("On") } else { String::from("Off") };
        }
    }

    /// Değer ayarla
    pub fn set_value(&mut self, value: f32) {
        self.value = value.max(0.0).min(1.0);
        self.active = self.value > 0.0;
        self.subtitle = format!("{}%", (self.value * 100.0) as i32);
    }

    /// Kareyi çiz
    pub fn draw(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize) {
        let bg_color = if self.active {
            self.active_color
        } else {
            Theme::SIDEBAR_BG.to_u32()
        };

        // Arka plan
        fb.draw_rect(x, y, width, height, bg_color);

        // Simge
        let icon_color = if self.active { 0xFFFFFFFF } else { Theme::TEXT_PRIMARY.to_u32() };
        fb.draw_string(x + 8, y + 8, &self.icon, icon_color);

        // Ad
        let text_color = if self.active { 0xFFFFFFFF } else { Theme::TEXT_PRIMARY.to_u32() };
        fb.draw_string(x + 8, y + 32, &self.name, text_color);

        // Alt başlık/durum
        let sub_color = if self.active { 0xFFCCCCCC } else { Theme::TEXT_SECONDARY.to_u32() };
        if !self.subtitle.is_empty() {
            fb.draw_string(x + 8, y + 48, &self.subtitle, sub_color);
        }

        // Kaydırıcı göstergesi
        if self.tile_type == TileType::Slider && self.value > 0.0 {
            let slider_y = y + height - 4;
            let slider_width = (width as f32 * self.value) as usize;
            fb.draw_rect(x, slider_y, slider_width, 4, 0xFFFFFFFF);
        }
    }
}

// ============================================================================
// KONTROL GRUBU
// ============================================================================

/// İlgili kontrollerin grubu
pub struct ControlGroup {
    /// Grup adı
    pub name: String,
    /// Gruptaki kareler
    pub tiles: Vec<ControlTile>,
    /// Grup düzeni
    pub layout: GroupLayout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupLayout {
    Row2,    // 2 kare yan yana
    Row3,    // 3 kare yan yana
    Grid,    // 2×2 ızgara
    Column,  // Dikey yığın
    Large,   // Tek büyük kare
}

impl ControlGroup {
    pub fn new(name: &str, layout: GroupLayout) -> Self {
        ControlGroup {
            name: String::from(name),
            tiles: Vec::new(),
            layout,
        }
    }

    pub fn add_tile(&mut self, tile: ControlTile) {
        self.tiles.push(tile);
    }

    /// Grup yüksekliğini hesapla
    pub fn height(&self) -> usize {
        match self.layout {
            GroupLayout::Row2 | GroupLayout::Row3 => TILE_SIZE + 16,
            GroupLayout::Grid => TILE_SIZE * 2 + TILE_SPACING + 16,
            GroupLayout::Column => self.tiles.len() * 44 + 16,
            GroupLayout::Large => 80,
        }
    }

    /// Grubu çiz
    pub fn draw(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize) {
        let group_height = self.height();

        // Grup arka planı
        fb.draw_rect(x, y, width, group_height, Theme::SIDEBAR_BG.to_u32());

        match self.layout {
            GroupLayout::Row2 => {
                let tile_width = (width - TILE_SPACING * 3) / 2;
                for (i, tile) in self.tiles.iter().enumerate() {
                    let tile_x = x + TILE_SPACING + i * (tile_width + TILE_SPACING);
                    tile.draw(fb, tile_x, y + 8, tile_width, TILE_SIZE);
                }
            }
            GroupLayout::Row3 => {
                let tile_width = (width - TILE_SPACING * 4) / 3;
                for (i, tile) in self.tiles.iter().enumerate() {
                    let tile_x = x + TILE_SPACING + i * (tile_width + TILE_SPACING);
                    tile.draw(fb, tile_x, y + 8, tile_width, TILE_SIZE);
                }
            }
            GroupLayout::Grid => {
                let tile_width = (width - TILE_SPACING * 3) / 2;
                for (i, tile) in self.tiles.iter().enumerate() {
                    let col = i % 2;
                    let row = i / 2;
                    let tile_x = x + TILE_SPACING + col * (tile_width + TILE_SPACING);
                    let tile_y = y + 8 + row * (TILE_SIZE + TILE_SPACING);
                    tile.draw(fb, tile_x, tile_y, tile_width, TILE_SIZE);
                }
            }
            GroupLayout::Column => {
                let item_height = 44;
                for (i, tile) in self.tiles.iter().enumerate() {
                    let tile_y = y + 8 + i * item_height;
                    tile.draw(fb, x + 8, tile_y, width - 16, item_height - 4);
                }
            }
            GroupLayout::Large => {
                if let Some(tile) = self.tiles.first() {
                    tile.draw(fb, x + 8, y + 8, width - 16, 64);
                }
            }
        }
    }

    /// İsabet testi
    pub fn hit_test(&self, mx: i32, my: i32, x: usize, y: usize, width: usize) -> Option<usize> {
        let group_height = self.height() as i32;

        if mx < x as i32 || mx >= (x + width) as i32 || my < y as i32 || my >= (y as i32 + group_height) {
            return None;
        }

        match self.layout {
            GroupLayout::Row2 => {
                let tile_width = (width - TILE_SPACING * 3) / 2;
                for i in 0..self.tiles.len().min(2) {
                    let tile_x = x as i32 + TILE_SPACING as i32 + i as i32 * (tile_width + TILE_SPACING) as i32;
                    if mx >= tile_x && mx < tile_x + tile_width as i32 {
                        return Some(i);
                    }
                }
            }
            GroupLayout::Row3 => {
                let tile_width = (width - TILE_SPACING * 4) / 3;
                for i in 0..self.tiles.len().min(3) {
                    let tile_x = x as i32 + TILE_SPACING as i32 + i as i32 * (tile_width + TILE_SPACING) as i32;
                    if mx >= tile_x && mx < tile_x + tile_width as i32 {
                        return Some(i);
                    }
                }
            }
            GroupLayout::Grid => {
                let tile_width = (width - TILE_SPACING * 3) / 2;
                for i in 0..self.tiles.len().min(4) {
                    let col = i % 2;
                    let row = i / 2;
                    let tile_x = x as i32 + TILE_SPACING as i32 + col as i32 * (tile_width + TILE_SPACING) as i32;
                    let tile_y = y as i32 + 8 + row as i32 * (TILE_SIZE + TILE_SPACING) as i32;
                    if mx >= tile_x && mx < tile_x + tile_width as i32 && my >= tile_y && my < tile_y + TILE_SIZE as i32 {
                        return Some(i);
                    }
                }
            }
            GroupLayout::Column => {
                let item_height = 44;
                let idx = ((my - y as i32 - 8) / item_height) as usize;
                if idx < self.tiles.len() {
                    return Some(idx);
                }
            }
            GroupLayout::Large => {
                return Some(0);
            }
        }

        None
    }
}

// ============================================================================
// KONTROL MERKEZİ
// ============================================================================

/// Kontrol Merkezi paneli
pub struct ControlCenter {
    /// Görünür mü
    pub visible: bool,
    /// Kontrol grupları
    pub groups: Vec<ControlGroup>,
    /// Animasyon ilerlemesi
    pub animation_progress: f32,
    /// Ekran genişliği
    pub screen_width: usize,
    /// Ekran yüksekliği
    pub screen_height: usize,
    /// Panel genişliği
    pub panel_width: usize,
    /// Sonraki kare kimliği
    pub next_tile_id: u32,
}

impl ControlCenter {
    pub fn new(screen_width: usize, screen_height: usize) -> Self {
        let mut cc = ControlCenter {
            visible: false,
            groups: Vec::new(),
            animation_progress: 0.0,
            screen_width,
            screen_height,
            panel_width: CC_WIDTH,
            next_tile_id: 1,
        };

        cc.add_default_groups();
        cc
    }

    fn add_default_groups(&mut self) {
        // Ağ grubu (2'li satır)
        let mut network = ControlGroup::new("Network", GroupLayout::Row2);
        let mut wifi = ControlTile::toggle(self.next_tile_id, "Wi-Fi", "📶");
        wifi.active = true;
        wifi.subtitle = String::from("echOS-WiFi");
        network.add_tile(wifi);
        self.next_tile_id += 1;

        let mut bluetooth = ControlTile::toggle(self.next_tile_id, "Bluetooth", "🔵");
        bluetooth.active = true;
        bluetooth.subtitle = String::from("On");
        network.add_tile(bluetooth);
        self.next_tile_id += 1;

        self.groups.push(network);

        // Medya grubu (3'lü satır)
        let mut media = ControlGroup::new("Media", GroupLayout::Row3);

        let mut airdrop = ControlTile::toggle(self.next_tile_id, "AirDrop", "📡");
        airdrop.subtitle = String::from("Contacts Only");
        media.add_tile(airdrop);
        self.next_tile_id += 1;

        let mut focus = ControlTile::toggle(self.next_tile_id, "Focus", "🌙");
        focus.active = false;
        focus.subtitle = String::from("Off");
        media.add_tile(focus);
        self.next_tile_id += 1;

        let mut airplane = ControlTile::toggle(self.next_tile_id, "Airplane", "✈");
        airplane.active = false;
        airplane.subtitle = String::from("Off");
        media.add_tile(airplane);
        self.next_tile_id += 1;

        self.groups.push(media);

        // Ekran grubu (ızgara)
        let mut display = ControlGroup::new("Display", GroupLayout::Grid);

        let brightness = ControlTile::slider(self.next_tile_id, "Brightness", "☀", 0.8);
        display.add_tile(brightness);
        self.next_tile_id += 1;

        let night = ControlTile::toggle(self.next_tile_id, "Night Shift", "🌙");
        display.add_tile(night);
        self.next_tile_id += 1;

        let mut external = ControlTile::menu(self.next_tile_id, "Displays", "🖥", "Built-in");
        display.add_tile(external);
        self.next_tile_id += 1;

        let mut hdr = ControlTile::toggle(self.next_tile_id, "HDR", "🎨");
        hdr.active = true;
        display.add_tile(hdr);
        self.next_tile_id += 1;

        self.groups.push(display);

        // Ses grubu (2'li satır)
        let mut sound = ControlGroup::new("Sound", GroupLayout::Row2);

        let volume = ControlTile::slider(self.next_tile_id, "Volume", "🔊", 0.7);
        sound.add_tile(volume);
        self.next_tile_id += 1;

        let mut output = ControlTile::menu(self.next_tile_id, "Output", "🎧", "Speakers");
        sound.add_tile(output);
        self.next_tile_id += 1;

        self.groups.push(sound);

        // Şu an çalınan (büyük kare)
        let mut now_playing = ControlGroup::new("Now Playing", GroupLayout::Large);

        let mut music = ControlTile::button(self.next_tile_id, "Music", "🎵");
        music.subtitle = String::from("Not Playing");
        now_playing.add_tile(music);
        self.next_tile_id += 1;

        self.groups.push(now_playing);

        // Hızlı eylemler (dikey yığın)
        let mut quick = ControlGroup::new("Quick Actions", GroupLayout::Column);

        let mut dnd = ControlTile::toggle(self.next_tile_id, "Do Not Disturb", "🔕");
        quick.add_tile(dnd);
        self.next_tile_id += 1;

        let mut screen = ControlTile::button(self.next_tile_id, "Screen Mirroring", "📺");
        quick.add_tile(screen);
        self.next_tile_id += 1;

        let mut sleep = ControlTile::button(self.next_tile_id, "Sleep Display", "💤");
        quick.add_tile(sleep);
        self.next_tile_id += 1;

        let mut lock = ControlTile::button(self.next_tile_id, "Lock Screen", "🔒");
        quick.add_tile(lock);
        self.next_tile_id += 1;

        self.groups.push(quick);
    }

    /// Kontrol merkezini göster
    pub fn show(&mut self) {
        self.visible = true;
        self.animation_progress = 0.0;
    }

    /// Kontrol merkezini gizle
    pub fn hide(&mut self) {
        self.visible = false;
    }

    /// Görünürlüğü değiştir
    pub fn toggle(&mut self) {
        if self.visible {
            self.hide();
        } else {
            self.show();
        }
    }

    /// Animasyonu güncelle
    pub fn update(&mut self, dt: f32) {
        if self.visible && self.animation_progress < 1.0 {
            self.animation_progress = (self.animation_progress + dt * 8.0).min(1.0);
        } else if !self.visible && self.animation_progress > 0.0 {
            self.animation_progress = (self.animation_progress - dt * 8.0).max(0.0);
        }
    }

    /// Kontrol merkezini çiz
    pub fn draw(&self, fb: &mut Framebuffer) {
        if self.animation_progress <= 0.0 {
            return;
        }

        let progress = self.animation_progress;

        // Konumu hesapla (sağ üst, üstten kayarak)
        let panel_x = self.screen_width - self.panel_width;
        let panel_y = (40.0 * (1.0 - progress)) as usize; // Üstten kaydır

        // Arka planı karart
        let bg_alpha = 0.2 * progress;
        for y in 0..self.screen_height {
            for x in 0..panel_x {
                let ptr = unsafe { (fb.base_addr as *mut u32).add(y * fb.pixels_per_scan_line + x) };
                let bg = unsafe { *ptr };
                let dimmed = Self::blend_color(bg, 0x000000, bg_alpha as f32);
                unsafe { *ptr = dimmed; }
            }
        }

        // Panel arka planı
        fb.draw_rect(panel_x, panel_y, self.panel_width, self.screen_height - panel_y, Theme::WINDOW_BG.to_u32());

        // Grupları çiz
        let mut y = panel_y + 16;
        let content_width = self.panel_width - 32;
        let x = panel_x + 16;

        for group in &self.groups {
            group.draw(fb, x, y, content_width);
            y += group.height() + TILE_SPACING;
        }
    }

    fn blend_color(bg: u32, fg: u32, alpha: f32) -> u32 {
        let br = ((bg >> 16) & 0xFF) as f32;
        let bg_ = ((bg >> 8) & 0xFF) as f32;
        let bb = (bg & 0xFF) as f32;

        let fr = ((fg >> 16) & 0xFF) as f32;
        let fg_ = ((fg >> 8) & 0xFF) as f32;
        let fb = (fg & 0xFF) as f32;

        let r = (br * (1.0 - alpha) + fr * alpha) as u32;
        let g = (bg_ * (1.0 - alpha) + fg_ * alpha) as u32;
        let b = (bb * (1.0 - alpha) + fb * alpha) as u32;

        (r << 16) | (g << 8) | b
    }

    /// Tıklama olayını işle
    pub fn on_click(&mut self, mx: i32, my: i32) -> ControlCenterEvent {
        let panel_x = self.screen_width - self.panel_width;

        // Panel dışını kontrol et
        if mx < panel_x as i32 {
            self.hide();
            return ControlCenterEvent::Cancelled;
        }

        // Grupları kontrol et
        let mut y = 16;
        let content_width = self.panel_width - 32;
        let x = panel_x + 16;

        for (group_idx, group) in self.groups.iter_mut().enumerate() {
            if let Some(tile_idx) = group.hit_test(mx, my, x, y, content_width) {
                if tile_idx < group.tiles.len() {
                    let tile = &mut group.tiles[tile_idx];
                    let tile_id = tile.id;

                    match tile.tile_type {
                        TileType::Toggle => {
                            tile.toggle_active();
                            return ControlCenterEvent::Toggled(tile_id, tile.active);
                        }
                        TileType::Button => {
                            return ControlCenterEvent::ButtonPressed(tile_id, tile.name.clone());
                        }
                        TileType::Menu => {
                            return ControlCenterEvent::MenuRequested(tile_id, tile.name.clone());
                        }
                        TileType::Slider => {
                            // Kaydırıcı detay görünümü açılacak
                            return ControlCenterEvent::SliderAdjusted(tile_id, tile.value);
                        }
                    }
                }
            }

            y += group.height() + TILE_SPACING;
        }

        ControlCenterEvent::None
    }

    /// Sürükleme olayını işle (kaydırıcılar için)
    pub fn on_drag(&mut self, mx: i32, my: i32, start_x: i32, start_y: i32) -> ControlCenterEvent {
        let panel_x = self.screen_width - self.panel_width;

        // Sürüklenen kaydırıcıyı bul
        let mut y = 16;
        let content_width = self.panel_width - 32;
        let x = panel_x + 16;

        for group in &mut self.groups {
            if let Some(tile_idx) = group.hit_test(start_x, start_y, x, y, content_width) {
                if tile_idx < group.tiles.len() {
                    let tile = &mut group.tiles[tile_idx];
                    if tile.tile_type == TileType::Slider {
                        // Sürükleme konumuna göre yeni değeri hesapla
                        let tile_width = (content_width - TILE_SPACING * 3) / 2;
                        let tile_x = x + TILE_SPACING + (tile_idx % 2) * (tile_width + TILE_SPACING);

                        let new_value = ((mx - tile_x as i32) as f32 / tile_width as f32).max(0.0).min(1.0);
                        tile.set_value(new_value);

                        return ControlCenterEvent::SliderAdjusted(tile.id, tile.value);
                    }
                }
            }

            y += group.height() + TILE_SPACING;
        }

        ControlCenterEvent::None
    }

    /// Yeniden boyutlandır
    pub fn resize(&mut self, width: usize, height: usize) {
        self.screen_width = width;
        self.screen_height = height;
    }

    /// Kimliğe göre kare al
    pub fn get_tile_mut(&mut self, id: u32) -> Option<&mut ControlTile> {
        for group in &mut self.groups {
            for tile in &mut group.tiles {
                if tile.id == id {
                    return Some(tile);
                }
            }
        }
        None
    }
}

/// Kontrol merkezi olayları
#[derive(Clone, Debug)]
pub enum ControlCenterEvent {
    None,
    Toggled(u32, bool),
    ButtonPressed(u32, String),
    MenuRequested(u32, String),
    SliderAdjusted(u32, f32),
    Cancelled,
}

// ============================================================================
// GLOBAL KONTROL MERKEZİ
// ============================================================================

lazy_static::lazy_static! {
    static ref CONTROL_CENTER: Mutex<ControlCenter> = Mutex::new(ControlCenter::new(1920, 1080));
}

/// Kontrol merkezini başlat
pub fn init(width: usize, height: usize) {
    let mut cc = CONTROL_CENTER.lock();
    cc.resize(width, height);
    crate::serial_println!("[GUI] Control Center initialized");
}

/// Kontrol merkezine erişim sağla
pub fn get_control_center() -> &'static Mutex<ControlCenter> {
    &CONTROL_CENTER
}
