//! # echOS Pencere Yöneticisi
//!
//! Pencere yönetimi: simge durumuna küçültme, büyütme, boyutlandırma,
//! yapıştırma (snap) ve Alt+Tab pencere döngüsü.
//!
//! ## Mimari
//! - `WindowState`: Normal, simge, büyütülmüş, yapıştırılmış (sol/sağ/üst/alt)
//! - `WindowInfo`: Z-sırası, odak durumu, yeniden boyutlandırılabilirlik bilgisi
//! - `ResizeEdge`: 8 yön + merkez için kenar tespiti
//! - `WindowManager`: Z-sırası normalizasyonu, çalışma alanı sınırlaması, yapıştırma eşiği

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;
use crate::gui::window::Window;
use crate::gui::widgets::{Rect, Widget};
use alloc::string::String;
use alloc::vec::Vec;

/// Pencere durumu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowState {
    Normal,
    Minimized,   // Simge durumuna küçültülmüş
    Maximized,   // Büyütülmüş
    SnappedLeft, // Sola yapıştırılmış
    SnappedRight,  // Sağa yapıştırılmış
    SnappedTop,    // Üste yapıştırılmış
    SnappedBottom, // Alta yapıştırılmış
}

/// Yönetim için pencere bilgisi
pub struct WindowInfo {
    pub id: u32,
    pub title: String,
    pub state: WindowState,
    pub normal_rect: Rect,    // Normal boyuttaki dikdörtgen
    pub current_rect: Rect,   // Mevcut (görüntülenen) dikdörtgen
    pub z_index: usize,       // Z-sırası (büyük = öndeki)
    pub focused: bool,        // Odaklı mı
    pub resizable: bool,      // Yeniden boyutlandırılabilir mi
    pub minimizable: bool,    // Simge durumuna küçültülebilir mi
    pub maximizable: bool,    // Büyütülebilir mi
}

impl WindowInfo {
    pub fn new(id: u32, title: &str, x: i32, y: i32, width: i32, height: i32) -> Self {
        let rect = Rect::new(x, y, width, height);
        Self {
            id,
            title: String::from(title),
            state: WindowState::Normal,
            normal_rect: rect,
            current_rect: rect,
            z_index: 0,
            focused: false,
            resizable: true,
            minimizable: true,
            maximizable: true,
        }
    }
}

/// Yeniden boyutlandırma kenarı
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeEdge {
    None,        // Kenar yok
    Top,         // Üst kenar
    Bottom,      // Alt kenar
    Left,        // Sol kenar
    Right,       // Sağ kenar
    TopLeft,     // Sol üst köşe
    TopRight,    // Sağ üst köşe
    BottomLeft,  // Sol alt köşe
    BottomRight, // Sağ alt köşe
}

/// Pencere Yöneticisi
pub struct WindowManager {
    windows: Vec<WindowInfo>,
    focused_id: Option<u32>,          // Odaklı pencere kimliği
    resize_edge: ResizeEdge,          // Aktif boyutlandırma kenarı
    resize_start: (i32, i32),         // Boyutlandırma başlangıç fare konumu
    resize_window_id: Option<u32>,    // Boyutlandırılan pencere kimliği
    drag_offset: (i32, i32),          // Sürükleme fare ofseti
    dragging_id: Option<u32>,         // Sürüklenen pencere kimliği
    screen_width: usize,
    screen_height: usize,
    taskbar_height: usize,            // Görev çubuğu yüksekliği (çalışma alanı kısıtlaması için)
    snap_threshold: i32,              // Yapıştırma eşiği (piksel)
}

impl WindowManager {
    /// Ekran boyutlarını alarak pencere yöneticisini başlatır.
    /// `taskbar_height` = 40 px; pencerelerin görev çubuğunun altına inmemesi için kullanılır.
    /// `snap_threshold` = 20 px; ekran kenarına bu kadar yaklaşıldığında yapıştırma tetiklenir.
    pub fn new(screen_width: usize, screen_height: usize) -> Self {
        Self {
            windows: Vec::new(),
            focused_id: None,
            resize_edge: ResizeEdge::None,
            resize_start: (0, 0),
            resize_window_id: None,
            drag_offset: (0, 0),
            dragging_id: None,
            screen_width,
            screen_height,
            taskbar_height: 40,
            snap_threshold: 20,
        }
    }

    /// Yeni pencereyi z-sırasına ekler ve odağı ona verir.
    /// Z-indeksi = mevcut pencere sayısı; yani yeni pencere en üstte başlar.
    pub fn add_window(&mut self, mut window: WindowInfo) -> u32 {
        window.z_index = self.windows.len();
        let id = window.id;
        self.windows.push(window);
        self.focus_window(id);
        id
    }

    /// Pencereyi listeden kaldırır; odaklıysa en yüksek z-indeksli kalan pencereye odak verilir.
    pub fn remove_window(&mut self, id: u32) {
        self.windows.retain(|w| w.id != id);
        if self.focused_id == Some(id) {
            // En üstteki kalan pencereye odaklan
            self.focused_id = self.windows.iter()
                .max_by_key(|w| w.z_index)
                .map(|w| w.id);
        }
    }

    /// Tüm pencere listesine salt-okunur erişim sağlar.
    pub fn windows(&self) -> &Vec<WindowInfo> {
        &self.windows
    }

    /// Kimliğe göre pencereye değiştirilebilir erişim sağlar.
    pub fn window_mut(&mut self, id: u32) -> Option<&mut WindowInfo> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    /// Kimliğe göre pencereye salt-okunur erişim sağlar.
    pub fn window(&self, id: u32) -> Option<&WindowInfo> {
        self.windows.iter().find(|w| w.id == id)
    }

    /// Odaklı pencereye salt-okunur erişim sağlar; hiç odak yoksa `None` döner.
    pub fn focused_window(&self) -> Option<&WindowInfo> {
        self.focused_id.and_then(|id| self.window(id))
    }

    /// Odaklı pencereye değiştirilebilir erişim sağlar.
    pub fn focused_window_mut(&mut self) -> Option<&mut WindowInfo> {
        self.focused_id.and_then(|id| self.window_mut(id))
    }

    /// Belirtilen pencereye odak verir: tüm odakları kaldırır, seçileni öne taşır.
    /// Z-indeksi mevcut en büyük değer + 1 yapılarak pencere en üste çıkarılır.
    pub fn focus_window(&mut self, id: u32) {
        // Tümünün odağını kaldır
        for w in &mut self.windows {
            w.focused = false;
        }

        // Pencere indeksini bul
        if let Some(idx) = self.windows.iter().position(|w| w.id == id) {
            // Öne getir
            let max_z = self.windows.iter().map(|w| w.z_index).max().unwrap_or(0);
            self.windows[idx].focused = true;
            self.windows[idx].z_index = max_z + 1;
            self.focused_id = Some(id);
        }
    }

    /// Pencereyi simge durumuna küçültür; `minimizable` = false ise işlem yapılmaz.
    /// Simge durumundaki pencereler `window_at` ve çizim döngüsünde atlanır.
    pub fn minimize(&mut self, id: u32) {
        if let Some(window) = self.window_mut(id) {
            if window.minimizable {
                window.state = WindowState::Minimized;
                // Sonraki pencereye odaklan
                if self.focused_id == Some(id) {
                    self.focused_id = self.windows.iter()
                        .filter(|w| w.state != WindowState::Minimized)
                        .max_by_key(|w| w.z_index)
                        .map(|w| w.id);
                }
            }
        }
    }

    /// Simge durumundaki veya yapıştırılmış pencereyi normal konuma/boyutuna geri yükler.
    pub fn restore(&mut self, id: u32) {
        if let Some(window) = self.window_mut(id) {
            // Geri yükle
            window.state = WindowState::Normal;
            window.current_rect = window.normal_rect;
            self.focus_window(id);
        }
    }

    /// Pencereyi tam ekrana büyütür ya da büyütülmüşse `normal_rect`'e geri yükler.
    /// Görev çubuğu yüksekliği çalışma alanından düşülür; taskbar kaplı alan boşta kalır.
    pub fn maximize(&mut self, id: u32) {
        // Önce ekran boyutlarını al
        let screen_w = self.screen_width as i32;
        let screen_h = (self.screen_height - self.taskbar_height) as i32;

        if let Some(window) = self.window_mut(id) {
            if window.maximizable {
                if window.state == WindowState::Maximized {
                    // Geri yükle
                    window.state = WindowState::Normal;
                    window.current_rect = window.normal_rect;
                } else {
                    // Büyüt
                    window.normal_rect = window.current_rect;
                    window.state = WindowState::Maximized;
                    window.current_rect = Rect::new(0, 0, screen_w, screen_h);
                }
            }
        }
    }

    /// Pencereyi ekranın sol yarısına yapıştırır (Windows 11 / macOS benzeri snap).
    /// `normal_rect` saklanır; geri yükleme sırasında bu değer kullanılır.
    pub fn snap_left(&mut self, id: u32) {
        // Önce ekran boyutlarını al
        let half_w = (self.screen_width / 2) as i32;
        let screen_h = (self.screen_height - self.taskbar_height) as i32;

        if let Some(window) = self.window_mut(id) {
            if window.resizable {
                window.normal_rect = window.current_rect;
                window.state = WindowState::SnappedLeft;
                window.current_rect = Rect::new(0, 0, half_w, screen_h);
            }
        }
    }

    /// Pencereyi ekranın sağ yarısına yapıştırır.
    pub fn snap_right(&mut self, id: u32) {
        // Önce ekran boyutlarını al
        let half_w = (self.screen_width / 2) as i32;
        let screen_h = (self.screen_height - self.taskbar_height) as i32;

        if let Some(window) = self.window_mut(id) {
            if window.resizable {
                window.normal_rect = window.current_rect;
                window.state = WindowState::SnappedRight;
                window.current_rect = Rect::new(half_w, 0, half_w, screen_h);
            }
        }
    }

    /// Fareyle sürüklemeyi başlatır.
    /// Büyütülmüş pencere önce normal boyutuna geri düşürülür, ardından sürükleme başlar.
    /// `drag_offset` = fare konumu − pencerenin sol üst köşesi; sürükleme sırasında sabit kalır.
    pub fn start_drag(&mut self, id: u32, x: i32, y: i32) {
        // Önce pencere durumunu ve dikdörtgenini al
        let (is_maximized, normal_rect) = self.window(id)
            .map(|w| (w.state == WindowState::Maximized, w.normal_rect))
            .unwrap_or((false, Rect::new(0, 0, 0, 0)));

        // Sürükleme ofseti için mevcut dikdörtgeni al
        let current_rect = self.window(id)
            .map(|w| w.current_rect)
            .unwrap_or(Rect::new(0, 0, 0, 0));

        // Büyütülmüşse önce geri yükle
        if is_maximized {
            if let Some(window) = self.window_mut(id) {
                window.state = WindowState::Normal;
                window.current_rect = normal_rect;
            }
        }

        self.dragging_id = Some(id);
        self.drag_offset = (x - current_rect.x, y - current_rect.y);
        self.focus_window(id);
    }

    /// Sürükleme sırasında her fare hareketi olayında çağrılır.
    /// Pencerenin konumunu günceller; yapıştırma önizlemesi için kenar kontrolü yapılır.
    /// Hareket gerçekleştiyse `true` döner.
    pub fn drag(&mut self, x: i32, y: i32) -> bool {
        let dragging_id = self.dragging_id;
        let drag_offset_x = self.drag_offset.0;
        let drag_offset_y = self.drag_offset.1;
        let snap_threshold = self.snap_threshold;
        let screen_width = self.screen_width as i32;
        let screen_width_u = self.screen_width;
        let screen_height_u = self.screen_height;
        let taskbar_height = self.taskbar_height;
        
        if let Some(id) = dragging_id {
            if let Some(window) = self.window_mut(id) {
                let new_x = x - drag_offset_x;
                let new_y = y - drag_offset_y;
                window.current_rect.x = new_x;
                window.current_rect.y = new_y;
                window.normal_rect.x = new_x;
                window.normal_rect.y = new_y;
                window.state = WindowState::Normal;

                // Yapıştırmayı kontrol et
                if x < snap_threshold {
                    // Sola yapıştırma önizlemesi
                } else if x > screen_width - snap_threshold {
                    // Sağa yapıştırma önizlemesi
                }

                return true;
            }
        }
        false
    }

    /// Fare bırakıldığında sürüklemeyi sonlandırır.
    /// Ekran kenarına `snap_threshold`'dan yakınsa yapıştırma uygulanır.
    pub fn end_drag(&mut self, x: i32, _y: i32) {
        if let Some(id) = self.dragging_id {
            // Yapıştırmayı kontrol et
            if x < self.snap_threshold {
                self.snap_left(id);
            } else if x > self.screen_width as i32 - self.snap_threshold {
                self.snap_right(id);
            }
        }
        self.dragging_id = None;
    }

    /// Yeniden boyutlandırmayı başlatır.
    /// Pencere `Normal` durumunda ve `resizable` = true ise boyutlandırma kilidini ayarlar.
    pub fn start_resize(&mut self, id: u32, edge: ResizeEdge, x: i32, y: i32) {
        let (resizable, state) = self.window(id)
            .map(|w| (w.resizable, w.state))
            .unwrap_or((false, WindowState::Normal));

        if resizable && state == WindowState::Normal {
            self.resize_window_id = Some(id);
            self.resize_edge = edge;
            self.resize_start = (x, y);
            self.focus_window(id);
        }
    }

    /// Her fare hareketi olayında boyutlandırmakta olduğumuz kenarı fare deltasına göre günceller.
    /// `min_width` = 200 px, `min_height` = 150 px; bu sınırın altına inilmez.
    /// `resize_start` her çağrıdan sonra güncellenir (delta hesabı süreklidir).
    pub fn resize(&mut self, x: i32, y: i32) -> bool {
        let resize_id = self.resize_window_id;
        let edge = self.resize_edge;
        let start_x = self.resize_start.0;
        let start_y = self.resize_start.1;
        let screen_width = self.screen_width;
        let screen_height = self.screen_height;
        let taskbar_height = self.taskbar_height;
        
        if let Some(id) = resize_id {
            if let Some(window) = self.window_mut(id) {
                let dx = x - start_x;
                let dy = y - start_y;

                let min_width = 200;
                let min_height = 150;

                match edge {
                    ResizeEdge::Left => {
                        let new_width = window.current_rect.width - dx;
                        if new_width >= min_width {
                            window.current_rect.x += dx;
                            window.current_rect.width = new_width;
                        }
                    }
                    ResizeEdge::Right => {
                        window.current_rect.width = (window.current_rect.width + dx).max(min_width);
                    }
                    ResizeEdge::Top => {
                        let new_height = window.current_rect.height - dy;
                        if new_height >= min_height {
                            window.current_rect.y += dy;
                            window.current_rect.height = new_height;
                        }
                    }
                    ResizeEdge::Bottom => {
                        window.current_rect.height = (window.current_rect.height + dy).max(min_height);
                    }
                    ResizeEdge::TopLeft => {
                        let new_width = window.current_rect.width - dx;
                        let new_height = window.current_rect.height - dy;
                        if new_width >= min_width {
                            window.current_rect.x += dx;
                            window.current_rect.width = new_width;
                        }
                        if new_height >= min_height {
                            window.current_rect.y += dy;
                            window.current_rect.height = new_height;
                        }
                    }
                    ResizeEdge::TopRight => {
                        let new_height = window.current_rect.height - dy;
                        if new_height >= min_height {
                            window.current_rect.y += dy;
                            window.current_rect.height = new_height;
                        }
                        window.current_rect.width = (window.current_rect.width + dx).max(min_width);
                    }
                    ResizeEdge::BottomLeft => {
                        let new_width = window.current_rect.width - dx;
                        if new_width >= min_width {
                            window.current_rect.x += dx;
                            window.current_rect.width = new_width;
                        }
                        window.current_rect.height = (window.current_rect.height + dy).max(min_height);
                    }
                    ResizeEdge::BottomRight => {
                        window.current_rect.width = (window.current_rect.width + dx).max(min_width);
                        window.current_rect.height = (window.current_rect.height + dy).max(min_height);
                    }
                    ResizeEdge::None => {}
                }

                window.normal_rect = window.current_rect;
            }
        }

        // Eşleşmeden sonra resize_start'ı güncelle
        self.resize_start = (x, y);

        resize_id.is_some()
    }

    /// Fare bırakıldığında boyutlandırmayı sonlandırır ve kilidi serbest bırakır.
    pub fn end_resize(&mut self) {
        self.resize_window_id = None;
        self.resize_edge = ResizeEdge::None;
    }

    /// Fare konumuna göre hangi kenar/köşede olduğunu tespit eder.
    /// `border` = 8 px; kenara bu kadar yakın olmak yeterlidir.
    /// Yapılandırılmamış ya da Normal olmayan pencereler için `None` döner.
    pub fn detect_resize_edge(&self, id: u32, x: i32, y: i32) -> ResizeEdge {
        if let Some(window) = self.window(id) {
            if window.state != WindowState::Normal || !window.resizable {
                return ResizeEdge::None;
            }

            let rect = window.current_rect;
            let border = 8; // Kenar algılama toleransı (piksel)

            let near_left = x >= rect.x - border && x <= rect.x + border;
            let near_right = x >= rect.x + rect.width - border && x <= rect.x + rect.width + border;
            let near_top = y >= rect.y - border && y <= rect.y + border;
            let near_bottom = y >= rect.y + rect.height - border && y <= rect.y + rect.height + border;

            if near_top && near_left {
                ResizeEdge::TopLeft
            } else if near_top && near_right {
                ResizeEdge::TopRight
            } else if near_bottom && near_left {
                ResizeEdge::BottomLeft
            } else if near_bottom && near_right {
                ResizeEdge::BottomRight
            } else if near_top {
                ResizeEdge::Top
            } else if near_bottom {
                ResizeEdge::Bottom
            } else if near_left {
                ResizeEdge::Left
            } else if near_right {
                ResizeEdge::Right
            } else {
                ResizeEdge::None
            }
        } else {
            ResizeEdge::None
        }
    }

    /// Verilen fare koordinatındaki en üst (z-indeksi en yüksek) pencereyi döndürür.
    /// Simge durumundaki pencereler atlanır; tıklama hedefi tespitinde kullanılır.
    pub fn window_at(&self, x: i32, y: i32) -> Option<u32> {
        // Üstten alta doğru kontrol et (en yüksek z-indeksi önce)
        let mut sorted: Vec<_> = self.windows.iter().collect();
        sorted.sort_by(|a, b| b.z_index.cmp(&a.z_index));

        for window in sorted {
            if window.state != WindowState::Minimized {
                if window.current_rect.contains(x, y) {
                    return Some(window.id);
                }
            }
        }
        None
    }

    /// Alt+Tab benzeri döngüsel pencere odak değiştirme.
    /// `forward` = true ise sonraki, false ise önceki pencereye geçer.
    /// Simge durumundaki pencereler bu döngüde atlanır.
    pub fn cycle_windows(&mut self, forward: bool) {
        let visible: Vec<_> = self.windows.iter()
            .filter(|w| w.state != WindowState::Minimized)
            .map(|w| w.id)
            .collect();

        if visible.is_empty() {
            return;
        }

        let current_idx = self.focused_id
            .and_then(|id| visible.iter().position(|&x| x == id))
            .unwrap_or(0);

        let next_idx = if forward {
            (current_idx + 1) % visible.len()
        } else {
            (current_idx + visible.len() - 1) % visible.len()
        };

        self.focus_window(visible[next_idx]);
    }

    /// Ekran boyutu değiştiğinde (örn. çözünürlük değişimi) pencere yöneticisini günceller.
    /// Mevcut pencereler yeni sınırlarla kısıtlanabilir; çağrıdan sonra doğrulamak önerilir.
    pub fn update_screen_size(&mut self, width: usize, height: usize) {
        self.screen_width = width;
        self.screen_height = height;
    }
}
