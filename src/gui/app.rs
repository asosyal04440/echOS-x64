//! # echOS Uygulama Trait'i (App Trait)
//!
//! Tüm echOS uygulamaları için ortak arayüz. `enum AppWindow` yaklaşımından
//! trait-based architecture'a geçiş sağlar.
//!
//! ## Avantajlar
//!
//! - **Genişletilebilirlik:** Yeni uygulama eklemek için sadece `App` trait'ini
//!   impl etmek yeterli, `match` arm eklemeye gerek yok
//! - **Dinamik Dağıtım:** `Box<dyn App>` ile runtime'da farklı uygulama türleri
//! - **Modülerlik:** Her uygulama kendi modülünde tamamen izole
//!
//! ## Örnek
//!
//! ```rust
//! pub struct MyApplication {
//!     title: String,
//!     rect: Rect,
//! }
//!
//! impl App for MyApplication {
//!     fn title(&self) -> &str { &self.title }
//!     fn bounds(&self) -> Rect { self.rect }
//!     fn draw(&mut self, fb: &mut Framebuffer) {
//!         // Uygulama çizim mantığı
//!     }
//!     // ... diğer metodlar
//! }
//! ```

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::widgets::Rect;
use crate::gui::echos_wm::InputAction;

// ============================================================================
// UYGULAMA TRAIT'I
// ============================================================================

/// Tüm echOS uygulamaları için ortak arayüz.
///
/// Bu trait, bir uygulamanın temel yaşam döngüsünü ve etkileşim metodlarını tanımlar.
/// Her uygulama bu trait'i implement ederek masaüstü ile entegre olur.
pub trait App {
    // ========================================================================
    // Zorunlu Metodlar
    // ========================================================================

    /// Uygulama başlığı (pencere başlık çubuğunda gösterilir)
    fn title(&self) -> &str;

    /// Uygulamanın geçerli sınırları (pencere pozisyonu ve boyutu)
    fn bounds(&self) -> Rect;

    /// Uygulamayı framebuffer'a çiz
    ///
    /// Bu metod her frame'de çağrılır. Uygulama kendi içeriğini
    /// `bounds()` ile belirtilen alana çizmelidir.
    fn draw(&mut self, fb: &mut Framebuffer);

    // ========================================================================
    // Olay İşleme (Event Handling)
    // ========================================================================

    /// Klavye olayı işle
    ///
    /// # Parametreler
    /// - `key`: Basılan tuş karakteri
    /// - `scancode`: Tuş scancode'u
    /// - `modifiers`: Değiştirici tuşlar (Ctrl, Alt, Shift, Super)
    ///
    /// # Dönüş
    /// - `true`: Olay işlendi, başka handler'a geçirilmemeli
    /// - `false`: Olay işlenmedi, parent handler'a geçirilmeli
    fn on_key(&mut self, _key: char, _scancode: u8, _modifiers: u8) -> bool {
        false
    }

    /// Fare olayı işle
    ///
    /// # Parametreler
    /// - `action`: Fare eylemi (tıklama, sürükleme, hareket)
    ///
    /// # Dönüş
    /// - `true`: Olay işlendi
    /// - `false`: Olay işlenmedi
    fn on_mouse(&mut self, _action: &InputAction) -> bool {
        false
    }

    /// Pencere boyutu değiştiğinde çağrılır
    ///
    /// # Parametreler
    /// - `new_bounds`: Yeni pencere sınırları
    fn on_resize(&mut self, _new_bounds: Rect) {}

    /// Pencere odak kazandığında çağrılır
    fn on_focus(&mut self) {}

    /// Pencere odak kaybettiğinde çağrılır
    fn on_blur(&mut self) {}

    // ========================================================================
    // Güncelleme (Update)
    // ========================================================================

    /// Frame güncellemesi
    ///
    /// Her frame'de çizim öncesinde çağrılır. Animasyonlar, zamanlayıcılar
    /// ve periyodik güncellemeler burada yapılmalıdır.
    ///
    /// # Parametreler
    /// - `dt`: Son frame'den beri geçen süre (saniye)
    fn update(&mut self, _dt: f32) {}

    // ========================================================================
    // Pencere Yönetimi
    // ========================================================================

    /// Pencere pozisyonunu ayarla
    fn set_position(&mut self, x: i32, y: i32);

    /// Pencere boyutunu ayarla
    fn set_size(&mut self, width: i32, height: i32);

    /// Pencereyi simge durumuna küçült
    fn minimize(&mut self) {}

    
    /// Pencereyi büyüt (maximize)
    fn maximize(&mut self) {}

    /// Pencereyi eski haline getir (restore)
    fn restore(&mut self) {}

    /// Pencere kapatılmak isteniyor
    ///
    /// # Dönüş
    /// - `true`: Pencere kapatılabilir
    /// - `false`: Pencere kapatılamaz (örn. kaydedilmemiş dosya var)
    fn can_close(&self) -> bool {
        true
    }

    /// Pencere kapatıldığında çağrılır (temizlik için)
    fn on_close(&mut self) {}

    // ========================================================================
    // Görsel Efektler
    // ========================================================================

    /// Arka planda blur efekti istiyor mu?
    ///
    /// `true` dönerse, compositor pencere arkasında frosted-glass efekti uygular
    fn wants_blur_behind(&self) -> bool {
        false
    }

    /// Pencere saydamlığı (0.0 = tam saydam, 1.0 = opak)
    fn opacity(&self) -> f32 {
        1.0
    }

    /// Pencere gölgesi istiyor mu?
    fn wants_shadow(&self) -> bool {
        true
    }

    // ========================================================================
    // Durum (State)
    // ========================================================================

    /// Uygulama tipik tanımlayıcısı (örn. "finder", "terminal", "browser")
    fn app_id(&self) -> &str;

    /// Kaydedilmemiş değişiklik var mı?
    fn is_dirty(&self) -> bool {
        false
    }

    /// Uygulama duraklatma (suspend) - oturum kaydetme için
    ///
    /// # Dönüş
    /// Uygulama durumunu serialize eden byte dizisi
    fn suspend(&self) -> Option<Vec<u8>> {
        None
    }

    /// Uygulama devam ettirme (resume) - oturum geri yükleme için
    ///
    /// # Parametreler
    /// - `state`: `suspend()` ile kaydedilen durum
    fn resume(&mut self, _state: &[u8]) {}
}

// ============================================================================
// UYGULAMA YÖNETİCİSİ (App Manager)
// ============================================================================

/// Uygulama pencerelerini yöneten koleksiyon.
///
/// `Vec<Box<dyn App>>` etrafında kullanışlı metodlar sağlar.
pub struct AppManager {
    apps: Vec<Box<dyn App>>,
    focused_idx: Option<usize>,
}

impl AppManager {
    /// Yeni uygulama yöneticisi oluştur
    pub fn new() -> Self {
        AppManager {
            apps: Vec::new(),
            focused_idx: None,
        }
    }

    /// Uygulama ekle
    ///
    /// # Dönüş
    /// Eklenen uygulamanın indeksi
    pub fn add(&mut self, app: Box<dyn App>) -> usize {
        self.apps.push(app);
        let idx = self.apps.len() - 1;
        self.focused_idx = Some(idx);
        idx
    }

    /// Uygulamayı kaldır
    pub fn remove(&mut self, idx: usize) -> Option<Box<dyn App>> {
        if idx < self.apps.len() {
            let app = self.apps.remove(idx);
            // Odaklanmış pencere kaldırıldıysa, odağı güncelle
            if self.focused_idx == Some(idx) {
                self.focused_idx = if self.apps.is_empty() {
                    None
                } else {
                    Some(self.apps.len().saturating_sub(1))
                };
            } else if let Some(focused) = self.focused_idx {
                if focused > idx {
                    self.focused_idx = Some(focused - 1);
                }
            }
            Some(app)
        } else {
            None
        }
    }

    /// Tüm uygulamaları döndür
    pub fn iter(&self) -> impl Iterator<Item = &Box<dyn App>> {
        self.apps.iter()
    }

    /// Tüm uygulamaları mutable olarak döndür
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Box<dyn App>> {
        self.apps.iter_mut()
    }

    /// Uygulama sayısı
    pub fn len(&self) -> usize {
        self.apps.len()
    }

    /// Boş mu?
    pub fn is_empty(&self) -> bool {
        self.apps.is_empty()
    }

    /// İndekse göre uygulama al
    pub fn get(&self, idx: usize) -> Option<&Box<dyn App>> {
        self.apps.get(idx)
    }

    /// İndekse göre mutable uygulama al
    pub fn get_mut(&mut self, idx: usize) -> Option<&mut Box<dyn App>> {
        self.apps.get_mut(idx)
    }

    /// Odaklanmış uygulamanın indeksi
    pub fn focused_index(&self) -> Option<usize> {
        self.focused_idx
    }

    /// Odaklanmış uygulamayı al
    pub fn focused(&self) -> Option<&Box<dyn App>> {
        self.focused_idx.and_then(|idx| self.apps.get(idx))
    }

    /// Odaklanmış uygulamayı mutable olarak al
    pub fn focused_mut(&mut self) -> Option<&mut Box<dyn App>> {
        self.focused_idx.and_then(|idx| self.apps.get_mut(idx))
    }

    /// Odağı belirli bir uygulamaya ver
    pub fn set_focus(&mut self, idx: usize) -> bool {
        if idx < self.apps.len() {
            // Eski odaklanmış uygulamaya blur gönder
            if let Some(old_idx) = self.focused_idx {
                if let Some(old_app) = self.apps.get_mut(old_idx) {
                    old_app.on_blur();
                }
            }
            
            // Yeni odaklanmış uygulamaya focus gönder
            if let Some(new_app) = self.apps.get_mut(idx) {
                new_app.on_focus();
            }
            
            self.focused_idx = Some(idx);
            true
        } else {
            false
        }
    }

    /// Verilen koordinattaki uygulamayı bul (Z-sırasına göre)
    ///
    /// En üstteki (son eklenen) uygulamadan başlayarak arar.
    pub fn hit_test(&self, x: i32, y: i32) -> Option<usize> {
        // Z-sırası: sondan başa doğru ara
        for (idx, app) in self.apps.iter().enumerate().rev() {
            let bounds = app.bounds();
            if x >= bounds.x
                && x < bounds.x + bounds.width
                && y >= bounds.y
                && y < bounds.y + bounds.height
            {
                return Some(idx);
            }
        }
        None
    }

    /// Tüm uygulamaları çiz (Z-sırasına göre)
    pub fn draw_all(&mut self, fb: &mut Framebuffer) {
        for app in &mut self.apps {
            app.draw(fb);
        }
    }

    /// Tüm uygulamaları güncelle
    pub fn update_all(&mut self, dt: f32) {
        for app in &mut self.apps {
            app.update(dt);
        }
    }
}

impl Default for AppManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// YARDIMCI TRAIT'LER
// ============================================================================

/// Uygulama için menü öğelerini tanımlayan trait
pub trait AppMenu {
    /// Menü öğelerini döndür
    ///
    /// Her öğe: (label, action_id)
    fn menu_items(&self) -> Vec<(&str, u32)> {
        Vec::new()
    }

    /// Menü öğesi seçildiğinde çağrılır
    fn on_menu_action(&mut self, _action_id: u32) {}
}

/// Dosya açabilen uygulamalar için trait
pub trait FileHandler {
    /// Desteklenen dosya uzantıları
    fn supported_extensions(&self) -> &[&str];

    /// Dosya aç
    fn open_file(&mut self, path: &str) -> bool;

    /// Dosya kaydet
    fn save_file(&mut self, path: &str) -> bool;
}
