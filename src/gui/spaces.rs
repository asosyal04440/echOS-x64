//! # Sanal Masaüstleri (Spaces)
//!
//! Pürüzsüz geçiş animasyonuyla çoklu sanal masaüstü desteği.
//! Her Space, kendine ait pencereler, duvar kağıdı ve ayarlara sahiptir.
//!
//! ## Mimari Genel Bakış
//!
//! ```text
//!  ┌─────────────────────────────────────────────────────┐
//!  │               SpacesManager                         │
//!  │  ┌──────────┐  ┌──────────┐  ┌──────────┐          │
//!  │  │ Space 0  │  │ Space 1  │  │ Space 2  │  ...     │
//!  │  │ (aktif)  │  │          │  │          │          │
//!  │  │ ┌──────┐ │  │ ┌──────┐ │  │ ┌──────┐ │          │
//!  │  │ │ Win1 │ │  │ │ Win3 │ │  │ │ Win5 │ │          │
//!  │  │ │ Win2 │ │  │ │ Win4 │ │  │ └──────┘ │          │
//!  │  │ └──────┘ │  │ └──────┘ │  │          │          │
//!  │  └──────────┘  └──────────┘  └──────────┘          │
//!  └─────────────────────────────────────────────────────┘
//! ```
//!
//! ## Geçiş Animasyonu (Slide)
//!
//! Space 0'dan Space 1'e geçiş (sağa kayma, transition_direction = +1):
//!
//! ```text
//!  t=0.0            t=0.5               t=1.0
//!  ┌────────┐       ┌────┬────┐         ┌────────┐
//!  │Space 0 │  →    │ S0 │ S1 │  →      │Space 1 │
//!  └────────┘       └────┴────┘         └────────┘
//!
//!  offset = screen_width * (1.0 - progress) * direction
//!  Space 0: offset - screen_width  (soldan çıkar)
//!  Space 1: offset                 (sağdan girer)
//! ```

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use alloc::collections::BTreeMap;
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};

// ============================================================================
// SPACE SABİTLERİ
// ============================================================================

/// Maksimum sanal masaüstü (space) sayısı
pub const MAX_SPACES: usize = 16;

/// Geçiş animasyonu süresi (saniye cinsinden)
pub const TRANSITION_DURATION: f32 = 0.3;

// ============================================================================
// SPACE PENCERESİ
// ============================================================================

/// Bir space içindeki pencere bilgisi.
/// Her pencere, hangi space'e ait olduğunu ve o space içindeki
/// konumunu/durumunu bu yapı ile saklar.
#[derive(Clone, Debug)]
pub struct SpaceWindow {
    /// Pencere kimliği (pencere yöneticisiyle eşleşir)
    pub window_id: u32,
    /// Bu space içindeki X konumu
    pub x: i32,
    /// Bu space içindeki Y konumu
    pub y: i32,
    /// Pencere genişliği
    pub width: usize,
    /// Pencere yüksekliği
    pub height: usize,
    /// Simge durumuna küçültülmüş mü
    pub minimized: bool,
    /// Tam ekrana büyütülmüş mü (maksimize)
    pub maximized: bool,
    /// Gerçek tam ekran modunda mı
    pub fullscreen: bool,
    /// Z sırası (yüksek = öne yakın; üst pencereye odaklanma için)
    pub z_order: u32,
    /// Uygulamanın kimliği (örn. "terminal", "browser")
    pub app_id: String,
}

impl SpaceWindow {
    pub fn new(window_id: u32, app_id: &str, x: i32, y: i32, width: usize, height: usize) -> Self {
        SpaceWindow {
            window_id,
            x,
            y,
            width,
            height,
            minimized: false,
            maximized: false,
            fullscreen: false,
            z_order: 0,
            app_id: String::from(app_id),
        }
    }
}

// ============================================================================
// SPACE (SANAL MASAÜSTÜ)
// ============================================================================

/// Tek bir sanal masaüstünü (Space) temsil eder.
///
/// Her Space bağımsız olarak:
/// - Kendi pencere listesini yönetir (z_order ile sıralı)
/// - Kendi duvar kağıdına sahiptir (renk, degrade veya görüntü)
/// - Geçiş sırasında `transition_offset` ile kayma animasyonu yapar
#[derive(Clone, Debug)]
pub struct Space {
    /// Space kimliği (benzersiz, değişmez)
    pub id: u32,
    /// Görüntü adı (örn. "Desktop 1")
    pub name: String,
    /// Bu space'teki pencereler
    pub windows: Vec<SpaceWindow>,
    /// Duvar kağıdı türü
    pub wallpaper: Wallpaper,
    /// Şu an aktif olan (görünen) space mi
    pub is_current: bool,
    /// Geçiş animasyonu kayma miktarı (-1.0 ile 1.0 arası, ekran genişliğine göre normalize)
    pub transition_offset: f32,
    /// Space'in sıra indeksi (0 tabanlı)
    pub index: usize,
}

/// Space duvar kağıdı türleri
#[derive(Clone, Debug)]
pub enum Wallpaper {
    /// Düz renk (0xRRGGBB)
    Color(u32),
    /// İki renkli dikey degrade (üst renk, alt renk)
    Gradient(u32, u32),
    /// Yoldan görüntü
    Image(String),
}

impl Space {
    pub fn new(id: u32, name: &str, index: usize) -> Self {
        Space {
            id,
            name: String::from(name),
            windows: Vec::new(),
            wallpaper: Wallpaper::Color(Theme::DESKTOP_BG.to_u32()),
            is_current: false,
            transition_offset: 0.0,
            index,
        }
    }

    /// Space'e pencere ekle. Yeni pencere otomatik olarak en üste (en yüksek z_order) yerleştirilir.
    pub fn add_window(&mut self, window: SpaceWindow) {
        // Mevcut en yüksek z sırasını bul ve bir üstüne ayarla
        let max_z = self.windows.iter().map(|w| w.z_order).max().unwrap_or(0);
        let mut window = window;
        window.z_order = max_z + 1;

        self.windows.push(window);
    }

    /// Space'ten pencereyi kaldır
    pub fn remove_window(&mut self, window_id: u32) {
        self.windows.retain(|w| w.window_id != window_id);
    }

    /// Kimliğe göre pencereyi döndür
    pub fn get_window(&self, window_id: u32) -> Option<&SpaceWindow> {
        self.windows.iter().find(|w| w.window_id == window_id)
    }

    /// Kimliğe göre pencereyi değiştirilebilir döndür
    pub fn get_window_mut(&mut self, window_id: u32) -> Option<&mut SpaceWindow> {
        self.windows.iter_mut().find(|w| w.window_id == window_id)
    }

    /// Pencereyi en öne getir (z_order'ı en yüksek yap)
    pub fn bring_to_front(&mut self, window_id: u32) {
        let max_z = self.windows.iter().map(|w| w.z_order).max().unwrap_or(0);
        if let Some(window) = self.get_window_mut(window_id) {
            window.z_order = max_z + 1;
        }
    }

    /// Pencereleri z_order'a göre sıralı döndür (arkadan öne: düşük → yüksek)
    pub fn windows_sorted(&self) -> Vec<&SpaceWindow> {
        let mut windows: Vec<_> = self.windows.iter().collect();
        windows.sort_by_key(|w| w.z_order);
        windows
    }

    /// Duvar kağıdını çiz. Geçiş animasyonu sırasında `offset_x` ile kaymış konumda çizer.
    ///
    /// ```text
    /// offset_x = 0:        normal çizim (tam ekran)
    /// offset_x > 0:        sağdan giriyor (start_x = offset_x)
    /// offset_x < 0:        sola çıkıyor   (end_x = width + offset_x)
    /// ```
    pub fn draw_wallpaper(&self, fb: &mut Framebuffer, offset_x: i32) {
        match &self.wallpaper {
            Wallpaper::Color(color) => {
                if offset_x == 0 {
                    // Basit tam ekran doldurma
                    for y in 0..fb.height {
                        for x in 0..fb.width {
                            fb.plot_pixel(x, y, *color);
                        }
                    }
                } else {
                    // Offset ile çiz (geçiş animasyonu için)
                    let start_x = if offset_x > 0 { offset_x as usize } else { 0 };
                    let end_x = if offset_x > 0 { fb.width } else { (fb.width as i32 + offset_x) as usize };

                    for y in 0..fb.height {
                        for x in start_x..end_x {
                            fb.plot_pixel(x, y, *color);
                        }
                    }
                }
            }
            Wallpaper::Gradient(color1, color2) => {
                for y in 0..fb.height {
                    let t = y as f32 / fb.height as f32;
                    let color = Self::lerp_color(*color1, *color2, t);

                    let start_x = if offset_x > 0 { offset_x as usize } else { 0 };
                    let end_x = if offset_x > 0 { fb.width } else { (fb.width as i32 + offset_x) as usize };

                    for x in start_x..end_x {
                        fb.plot_pixel(x, y, color);
                    }
                }
            }
            Wallpaper::Image(_path) => {
                // Görüntü yüklenip çizilecek — şimdilik solidle geri düş
                self.draw_wallpaper(fb, offset_x);
            }
        }
    }

    /// İki rengi `t` parametresine göre doğrusal interpolasyonla karıştırır.
    /// `t = 0.0` → c1, `t = 1.0` → c2
    fn lerp_color(c1: u32, c2: u32, t: f32) -> u32 {
        let r1 = ((c1 >> 16) & 0xFF) as f32;
        let g1 = ((c1 >> 8) & 0xFF) as f32;
        let b1 = (c1 & 0xFF) as f32;

        let r2 = ((c2 >> 16) & 0xFF) as f32;
        let g2 = ((c2 >> 8) & 0xFF) as f32;
        let b2 = (c2 & 0xFF) as f32;

        let r = (r1 + (r2 - r1) * t) as u32;
        let g = (g1 + (g2 - g1) * t) as u32;
        let b = (b1 + (b2 - b1) * t) as u32;

        (r << 16) | (g << 8) | b
    }
}

// ============================================================================
// SPACES YÖNETİCİSİ
// ============================================================================

/// Tüm sanal masaüstlerini (Space) yöneten merkezi yapı.
///
/// ## Geçiş Animasyonu Mantığı
/// ```text
/// switch_to_space(new_idx) çağrıldığında:
///   1. previous_space = current_space (eski masaüstü kaydedilir)
///   2. current_space  = new_idx       (yeni masaüstüne geçilir)
///   3. transitioning  = true
///   4. transition_progress = 0.0
///
/// update(dt) her kare çağrıldığında:
///   progress += dt / TRANSITION_DURATION  (0.0 → 1.0)
///
/// draw() sırasında:
///   offset = width * (1.0 - progress) * direction
///   eski space → offset - direction*width konumunda çizilir
///   yeni space → offset konumunda çizilir
/// ```
pub struct SpacesManager {
    /// Tüm sanal masaüstleri (spaces)
    pub spaces: Vec<Space>,
    /// Şu anki aktif space'in indeksi
    pub current_space: usize,
    /// Ekran genişliği
    pub screen_width: usize,
    /// Ekran yüksekliği
    pub screen_height: usize,
    /// Geçiş animasyonu ilerlemesi (0.0 = başlangıç, 1.0 = tamamlandı)
    pub transition_progress: f32,
    /// Geçiş yönü: +1 = sağa (ileri space), -1 = sola (geri space)
    pub transition_direction: i32,
    /// Geçiş animasyonu aktif mi
    pub transitioning: bool,
    /// Geçişten önceki space indeksi (animasyon için)
    pub previous_space: usize,
    /// Space değiştirildiğinde çağrılacak geri çağırım (callback)
    pub on_space_switch: Option<fn(u32)>,
    /// Pencere başka bir space'e taşındığında çağrılır (window_id, hedef_space_id)
    pub on_window_move: Option<fn(u32, u32)>,
}

impl SpacesManager {
    pub fn new(screen_width: usize, screen_height: usize) -> Self {
        let mut manager = SpacesManager {
            spaces: Vec::new(),
            current_space: 0,
            screen_width,
            screen_height,
            transition_progress: 0.0,
            transition_direction: 0,
            transitioning: false,
            previous_space: 0,
            on_space_switch: None,
            on_window_move: None,
        };

        // Varsayılan bir space oluştur
        manager.create_space("Desktop 1");
        manager.spaces[0].is_current = true;

        manager
    }

    /// Yeni bir sanal masaüstü oluştur. MAX_SPACES sınırına ulaşılırsa son space döner.
    pub fn create_space(&mut self, name: &str) -> u32 {
        if self.spaces.len() >= MAX_SPACES {
            return self.spaces.last().map(|s| s.id).unwrap_or(0);
        }

        let id = self.spaces.len() as u32;
        let space = Space::new(id, name, self.spaces.len());
        self.spaces.push(space);

        id
    }

    /// Space'i sil. Space'teki pencereler komşu space'e taşınır.
    /// Son kalan space silinemez (false döner).
    pub fn delete_space(&mut self, space_id: u32) -> bool {
        if self.spaces.len() <= 1 {
            return false; // En az bir space kalmalı
        }

        let idx = self.spaces.iter().position(|s| s.id == space_id);
        if let Some(idx) = idx {
            // Pencereleri komşu space'e taşı
            let target_idx = if idx < self.spaces.len() - 1 { idx + 1 } else { idx - 1 };
            let windows: Vec<_> = self.spaces[idx].windows.clone();

            for window in windows {
                self.spaces[target_idx].add_window(window);
            }

            self.spaces.remove(idx);

            // Tüm space'lerin indekslerini güncelle
            for (i, space) in self.spaces.iter_mut().enumerate() {
                space.index = i;
            }

            // Aktif space sınır dışında kalmışsa düzelt
            if self.current_space >= self.spaces.len() {
                self.current_space = self.spaces.len() - 1;
            }

            return true;
        }

        false
    }

    /// Belirtilen indeksteki space'e geç ve kayma animasyonunu başlat.
    pub fn switch_to_space(&mut self, space_index: usize) {
        if space_index >= self.spaces.len() || space_index == self.current_space {
            return;
        }

        self.previous_space = self.current_space;
        self.current_space = space_index;
        // Yön belirleme: yeni indeks daha büyükse sağa, küçükse sola kayma
        self.transition_direction = if space_index > self.previous_space { 1 } else { -1 };
        self.transition_progress = 0.0;
        self.transitioning = true;

        // is_current bayraklarını güncelle
        for space in &mut self.spaces {
            space.is_current = false;
        }
        self.spaces[self.current_space].is_current = true;

        // Space değişim geri çağırımını tetikle
        if let Some(callback) = self.on_space_switch {
            callback(self.spaces[self.current_space].id);
        }
    }

    /// Sonraki space'e geç (dairesel: son → ilk)
    pub fn next_space(&mut self) {
        if self.current_space < self.spaces.len() - 1 {
            self.switch_to_space(self.current_space + 1);
        } else if !self.spaces.is_empty() {
            // Sona gelince başa dön
            self.switch_to_space(0);
        }
    }

    /// Önceki space'e geç (dairesel: ilk → son)
    pub fn prev_space(&mut self) {
        if self.current_space > 0 {
            self.switch_to_space(self.current_space - 1);
        } else if !self.spaces.is_empty() {
            // Başa gelince sona dön
            self.switch_to_space(self.spaces.len() - 1);
        }
    }

    /// Mevcut aktif space'e salt okunur referans döndür
    pub fn current_space_ref(&self) -> &Space {
        &self.spaces[self.current_space]
    }

    /// Mevcut aktif space'e değiştirilebilir referans döndür
    pub fn current_space_mut(&mut self) -> &mut Space {
        &mut self.spaces[self.current_space]
    }

    /// İndekse göre space döndür
    pub fn get_space(&self, index: usize) -> Option<&Space> {
        self.spaces.get(index)
    }

    /// Kimliğe göre space döndür
    pub fn get_space_by_id(&self, id: u32) -> Option<&Space> {
        self.spaces.iter().find(|s| s.id == id)
    }

    /// Mevcut aktif space'e pencere ekle
    pub fn add_window_to_current(&mut self, window: SpaceWindow) {
        self.spaces[self.current_space].add_window(window);
    }

    /// Pencereyi başka bir space'e taşı.
    /// Hedef, mevcut space ile aynıysa false döner.
    pub fn move_window_to_space(&mut self, window_id: u32, target_space_index: usize) -> bool {
        if target_space_index >= self.spaces.len() || target_space_index == self.current_space {
            return false;
        }

        // Pencereyi mevcut space'ten bul ve kaldır
        if let Some(window) = self.spaces[self.current_space].windows.iter()
            .find(|w| w.window_id == window_id).cloned() {

            self.spaces[self.current_space].remove_window(window_id);
            self.spaces[target_space_index].add_window(window);

            // Taşıma geri çağırımını tetikle
            if let Some(callback) = self.on_window_move {
                callback(window_id, self.spaces[target_space_index].id);
            }

            return true;
        }

        false
    }

    /// Geçiş animasyonunu güncelle. Her kare `dt` saniyesiyle çağrılmalı.
    pub fn update(&mut self, dt: f32) {
        if self.transitioning {
            self.transition_progress += dt / TRANSITION_DURATION;

            if self.transition_progress >= 1.0 {
                self.transition_progress = 1.0;
                self.transitioning = false;
                self.transition_direction = 0;
            }
        }
    }

    /// Space'leri (geçiş animasyonuyla birlikte) çiz.
    ///
    /// Animasyon aktifse hem eski hem yeni space aynı anda çizilir;
    /// her biri `offset` piksel kaydırılmış konumdadır.
    pub fn draw(&self, fb: &mut Framebuffer) {
        if self.transitioning {
            // Her iki space'i birden çiz (biri çıkarken diğeri giriyor)
            let offset = self.screen_width as f32 * (1.0 - self.transition_progress) * self.transition_direction as f32;

            // Önceki space (ekrandan çıkıyor)
            let prev_offset = offset as i32 - self.transition_direction * self.screen_width as i32;
            self.spaces[self.previous_space].draw_wallpaper(fb, prev_offset);

            // Yeni space (ekrana giriyor)
            self.spaces[self.current_space].draw_wallpaper(fb, offset as i32);
        } else {
            // Sadece aktif space'i çiz
            self.spaces[self.current_space].draw_wallpaper(fb, 0);
        }
    }

    /// Toplam space sayısını döndür
    pub fn space_count(&self) -> usize {
        self.spaces.len()
    }

    /// Space'i yeniden adlandır
    pub fn rename_space(&mut self, space_index: usize, new_name: &str) -> bool {
        if let Some(space) = self.spaces.get_mut(space_index) {
            space.name = String::from(new_name);
            return true;
        }
        false
    }

    /// Belirtilen space'in duvar kağıdını değiştir
    pub fn set_wallpaper(&mut self, space_index: usize, wallpaper: Wallpaper) -> bool {
        if let Some(space) = self.spaces.get_mut(space_index) {
            space.wallpaper = wallpaper;
            return true;
        }
        false
    }

    /// Ekran yeniden boyutlandırıldığında boyutları güncelle
    pub fn resize(&mut self, width: usize, height: usize) {
        self.screen_width = width;
        self.screen_height = height;
    }

    /// Mission Control (görev görünümü) için tüm space'lerin özet bilgisini döndür
    pub fn get_space_info(&self) -> Vec<SpaceInfo> {
        self.spaces.iter().map(|s| SpaceInfo {
            id: s.id,
            name: s.name.clone(),
            window_count: s.windows.len(),
            is_current: s.is_current,
        }).collect()
    }
}

/// Dışarıya sunulan space özet bilgisi (Mission Control ve görev çubuğu için)
#[derive(Clone, Debug)]
pub struct SpaceInfo {
    /// Space kimliği
    pub id: u32,
    /// Space adı
    pub name: String,
    /// Bu space'teki pencere sayısı
    pub window_count: usize,
    /// Şu an aktif space mi
    pub is_current: bool,
}

// ============================================================================
// GLOBAL SPACES YÖNETİCİSİ (Spin Mutex Singleton)
// ============================================================================

lazy_static::lazy_static! {
    /// `spin::Mutex` ile korunan global spaces yöneticisi.
    /// Çekirdek modunda `std::sync` kullanılamadığı için döngüsel beklemeli
    /// (spinlock) mutex tercih edilir.
    static ref SPACES: Mutex<SpacesManager> = Mutex::new(SpacesManager::new(1920, 1080));
}

/// Spaces yöneticisini başlat (ekran boyutunu ayarla)
pub fn init(width: usize, height: usize) {
    let mut spaces = SPACES.lock();
    spaces.resize(width, height);
    crate::serial_println!("[GUI] Spaces manager initialized");
}

/// Global spaces yöneticisine erişim sağla
pub fn get_spaces() -> &'static Mutex<SpacesManager> {
    &SPACES
}
