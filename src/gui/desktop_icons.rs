//! # Masaüstü İkon Sistemi
//!
//! Çift tıklamayla başlatma desteğine sahip sürüklenebilir masaüstü ikonları.
//! Otomatik düzenleme seçeneğiyle ızgara tabanlı yerleşim.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use alloc::collections::BTreeMap;
use core::cmp::{min, max};
use spin::Mutex;
use libm::{sinf, cosf};

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::Theme;
use crate::gui::widgets::{Widget, Rect};

// ============================================================================
// İKON BOYUTU
// ============================================================================

/// Standart ikon boyutları
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconSize {
    Small = 16,
    Medium = 32,
    Large = 48,
    ExtraLarge = 64,
    Jumbo = 128,
}

impl IconSize {
    /// Sayısal piksel boyutunu döndürür.
    pub fn size(&self) -> usize {
        *self as usize
    }

    /// Piksel değerinden uygun `IconSize` varyantını döndürür.
    pub fn from_size(size: usize) -> Self {
        match size {
            0..=24 => IconSize::Small,
            25..=40 => IconSize::Medium,
            41..=56 => IconSize::Large,
            57..=96 => IconSize::ExtraLarge,
            _ => IconSize::Jumbo,
        }
    }
}

// ============================================================================
// MASAÜSTÜ İKONU
// ============================================================================

/// Tek bir masaüstü ikonu.
///
/// Her ikon; bir kimlik, görünen ad, tür, piksel konumu,
/// ızgara koordinatları, seçim/sürükleme durumu ve
/// çalıştırılacak eylemden oluşur.
pub struct DesktopIcon {
    /// Benzersiz kimlik
    id: u32,
    /// Görünen ad
    name: String,
    /// İkon türü
    icon_type: IconType,
    /// Piksel konumu (x, y)
    x: i32,
    y: i32,
    /// İkon boyutu
    size: IconSize,
    /// Izgara konumu (otomatik düzenleme için)
    grid_x: i32,
    grid_y: i32,
    /// Seçili mi
    selected: bool,
    /// Sürükleniyor mu
    dragging: bool,
    /// Tıklama noktasından sürükleme ofseti (x)
    drag_offset_x: i32,
    /// Tıklama noktasından sürükleme ofseti (y)
    drag_offset_y: i32,
    /// İlişkili eylem
    action: IconAction,
    /// Önbelleğe alınmış sınır dikdörtgeni
    bounds: Rect,
    /// Çift tıklama algılaması için son tıklama zamanı
    last_click_time: u64,
    /// Çift tıklama eşiği (ms)
    double_click_threshold: u64,
}

/// İkon görünüm türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconType {
    Folder,
    File,
    Application,
    Drive,
    Trash,
    Home,
    Computer,
    Network,
    Settings,
    Custom(u16),
}

/// İkon tıklamasında çalıştırılacak eylem
#[derive(Clone, Debug)]
pub enum IconAction {
    None,
    OpenFolder(String),
    OpenFile(String),
    LaunchApp(String),
    OpenSettings,
    EmptyTrash,
}

impl DesktopIcon {
    /// Yeni bir masaüstü ikonu oluşturur.
    pub fn new(id: u32, name: &str, icon_type: IconType, x: i32, y: i32) -> Self {
        let size = IconSize::Large;
        let bounds = Rect::new(x, y, size.size() as i32, size.size() as i32 + 20); // İkon + etiket

        DesktopIcon {
            id,
            name: String::from(name),
            icon_type,
            x,
            y,
            size,
            grid_x: 0,
            grid_y: 0,
            selected: false,
            dragging: false,
            drag_offset_x: 0,
            drag_offset_y: 0,
            action: IconAction::None,
            bounds,
            last_click_time: 0,
            double_click_threshold: 500, // 500ms
        }
    }

    /// Klasör ikonu oluşturur.
    pub fn folder(id: u32, name: &str, path: &str, x: i32, y: i32) -> Self {
        let mut icon = Self::new(id, name, IconType::Folder, x, y);
        icon.action = IconAction::OpenFolder(String::from(path));
        icon
    }

    /// Dosya ikonu oluşturur.
    pub fn file(id: u32, name: &str, path: &str, x: i32, y: i32) -> Self {
        let icon_type = Self::get_file_icon_type(name);
        let mut icon = Self::new(id, name, icon_type, x, y);
        icon.action = IconAction::OpenFile(String::from(path));
        icon
    }

    /// Uygulama ikonu oluşturur.
    pub fn app(id: u32, name: &str, app_id: &str, x: i32, y: i32) -> Self {
        let mut icon = Self::new(id, name, IconType::Application, x, y);
        icon.action = IconAction::LaunchApp(String::from(app_id));
        icon
    }

    /// Dosya uzantısından ikon türünü belirler.
    fn get_file_icon_type(filename: &str) -> IconType {
        let ext = filename.rsplit('.').next().unwrap_or("");
        match ext.to_lowercase().as_str() {
            "txt" | "md" | "doc" | "docx" => IconType::Custom(0), // Metin
            "png" | "jpg" | "jpeg" | "gif" | "bmp" => IconType::Custom(1), // Görüntü
            "mp3" | "wav" | "ogg" | "flac" => IconType::Custom(2), // Ses
            "mp4" | "avi" | "mkv" | "mov" => IconType::Custom(3), // Video
            "rs" | "c" | "cpp" | "h" | "py" | "js" => IconType::Custom(4), // Kod
            "zip" | "tar" | "gz" | "7z" => IconType::Custom(5), // Arşiv
            "exe" | "bin" | "sh" => IconType::Application,
            _ => IconType::File,
        }
    }

    /// Konum veya boyut değiştikten sonra sınır dikdörtgenini günceller.
    fn update_bounds(&mut self) {
        let icon_size = self.size.size() as i32;
        self.bounds = Rect::new(
            self.x,
            self.y,
            icon_size,
            icon_size + 20, // İkon + etiket yüksekliği
        );
    }

    /// İkon konumunu ayarlar ve sınır dikdörtgenini günceller.
    pub fn set_position(&mut self, x: i32, y: i32) {
        self.x = x;
        self.y = y;
        self.update_bounds();
    }

    /// İkon konumunu döndürür.
    pub fn position(&self) -> (i32, i32) {
        (self.x, self.y)
    }

    /// İkon boyutunu ayarlar ve sınır dikdörtgenini günceller.
    pub fn set_size(&mut self, size: IconSize) {
        self.size = size;
        self.update_bounds();
    }

    /// Seçim durumunu ayarlar.
    pub fn set_selected(&mut self, selected: bool) {
        self.selected = selected;
    }

    /// Verilen noktanın ikon alanına denk gelip gelmediğini kontrol eder.
    pub fn hit_test(&self, x: i32, y: i32) -> bool {
        self.bounds.contains(x, y)
    }

    /// Fare tuşuna basma olayını işler.
    ///
    /// Çift tıklamayı `double_click_threshold` ms ile algılar.
    /// Tek tıklamada sürüklemeyi başlatır.
    pub fn on_mouse_down(&mut self, x: i32, y: i32, time: u64) -> IconEvent {
        // Çift tıklama kontrolü
        if time - self.last_click_time < self.double_click_threshold {
            self.last_click_time = 0;
            return IconEvent::DoubleClick(self.id);
        }

        self.last_click_time = time;

        // Sürüklemeyi başlat
        self.dragging = true;
        self.drag_offset_x = self.x - x;
        self.drag_offset_y = self.y - y;

        IconEvent::Selected(self.id)
    }

    /// Fare hareketi olayını işler; sürükleme sırasında konumu günceller.
    pub fn on_mouse_move(&mut self, x: i32, y: i32) -> bool {
        if self.dragging {
            self.x = x + self.drag_offset_x;
            self.y = y + self.drag_offset_y;
            self.update_bounds();
            true
        } else {
            false
        }
    }

    /// Fare tuşu bırakma olayını işler; sürüklemeyi sonlandırır.
    pub fn on_mouse_up(&mut self) -> IconEvent {
        self.dragging = false;
        IconEvent::DragEnd(self.id)
    }

    /// İkonu framebuffer'a çizer.
    ///
    /// Seçili ikonun altına vurgu arka planı çizer,
    /// ardından ikon grafiği ve ortalanmış etiketi çizer.
    pub fn draw(&self, fb: &mut Framebuffer) {
        let icon_size = self.size.size() as usize;
        let x = self.x as usize;
        let y = self.y as usize;

        // Seçili ise seçim arka planını çiz
        if self.selected {
            let padding = 4;
            fb.draw_rect(
                x.saturating_sub(padding),
                y.saturating_sub(padding),
                icon_size + padding * 2,
                icon_size + 20 + padding * 2,
                Theme::ACCENT_PRIMARY.to_u32(),
            );
        }

        // İkon türüne göre grafiği çiz
        self.draw_icon(fb, x, y, icon_size);

        // İkon altında etiketi çiz
        let label_y = y + icon_size + 4;
        let label_color = if self.selected {
            Theme::TEXT_PRIMARY.to_u32()
        } else {
            Theme::TEXT_PRIMARY.to_u32()
        };

        // Etiketi ikona göre ortala
        let label_width = self.name.len() * 8;
        let label_x = if label_width > icon_size {
            x.saturating_sub((label_width - icon_size) / 2)
        } else {
            x + (icon_size - label_width) / 2
        };

        // Okunabilirlik için etiket arka planı çiz
        let bg_rect = Rect::new(
            label_x as i32 - 2,
            label_y as i32 - 1,
            label_width as i32 + 4,
            12,
        );

        // Yarı saydam siyah arka plan karıştırması
        for py in bg_rect.y..bg_rect.y + bg_rect.height {
            for px in bg_rect.x..bg_rect.x + bg_rect.width {
                if px >= 0 && py >= 0 && (px as usize) < fb.width && (py as usize) < fb.height {
                    let idx = (py as usize) * fb.pixels_per_scan_line + (px as usize);
                    let ptr = unsafe { (fb.base_addr as *mut u32).add(idx) };
                    unsafe {
                        let bg = *ptr;
                        // Yarı saydam siyahla karıştır
                        *ptr = ((bg & 0xFF) >> 1) | ((bg >> 1) & 0x007F7F7F);
                    }
                }
            }
        }

        fb.draw_string(label_x, label_y, &self.name, label_color);
    }

    /// İkon grafiğini türe göre çizer.
    ///
    /// Her `IconType` varyantı kendine özgü geometrik şekil kullanır.
    fn draw_icon(&self, fb: &mut Framebuffer, x: usize, y: usize, size: usize) {
        match self.icon_type {
            IconType::Folder => {
                // Sarı dikdörtgen gövde + üst sekme (klasör görünümü)
                let color = 0xFFC107; // Sarı
                let tab_height = size / 4;
                let tab_width = size / 2;

                // Üst sekme
                fb.draw_rect(x, y, tab_width, tab_height, color);
                // Gövde
                fb.draw_rect(x, y + tab_height, size, size - tab_height, color);
            }

            IconType::File => {
                // Köşesi kıvrılmış beyaz dikdörtgen (dosya görünümü)
                let color = Theme::TEXT_PRIMARY.to_u32();
                let fold_size = size / 4;

                // Ana gövde
                fb.draw_rect(x, y, size - fold_size, size, color);
                // Kıvrık köşe parçası
                fb.draw_rect(x + size - fold_size * 2, y, fold_size * 2, fold_size, color);
            }

            IconType::Application => {
                // Uygulama ikonu: renkli kare + dişli çark
                let color = Theme::ACCENT_PRIMARY.to_u32();
                fb.draw_rect(x, y, size, size, color);

                // Basit dişli çark gösterimi (8 diş, 45° aralıklarla)
                let center = size / 2;
                let radius = size / 4;
                for angle in 0..8 {
                    let a = angle as f32 * core::f32::consts::PI / 4.0;
                    let dx = (cosf(a) * radius as f32) as i32;
                    let dy = (sinf(a) * radius as f32) as i32;
                    fb.draw_rect(
                        (x as i32 + center as i32 + dx - 2) as usize,
                        (y as i32 + center as i32 + dy - 2) as usize,
                        4, 4,
                        Theme::DESKTOP_BG.to_u32()
                    );
                }
            }

            IconType::Drive => {
                // Disk sürücüsü: gri kare + yatay slot
                let color = 0x607D8B; // Mavi-Gri
                fb.draw_rect(x, y, size, size, color);

                // Disk okuma slotu
                let slot_y = y + size * 2 / 3;
                let slot_height = size / 6;
                fb.draw_rect(x + size / 4, slot_y, size / 2, slot_height, 0x000000);
            }

            IconType::Trash => {
                // Çöp kutusu: üst kapak + gövde
                let color = 0x9E9E9E; // Gri
                let lid_height = size / 4;

                // Kapak
                fb.draw_rect(x, y, size, lid_height, color);
                // Gövde
                fb.draw_rect(x + size / 8, y + lid_height, size * 3 / 4, size - lid_height, color);
            }

            IconType::Home => {
                // Ev ikonu: üçgen çatı + dörtgen gövde
                let color = 0x4CAF50; // Yeşil

                // Çatı (üçgen yaklaşımı: her satırda daralan çizgi)
                let roof_height = size / 3;
                for row in 0..roof_height {
                    let width = (size as f32 * (1.0 - row as f32 / roof_height as f32)) as usize;
                    let start_x = x + (size - width) / 2;
                    fb.draw_rect(start_x, y + row, width, 1, color);
                }

                // Gövde
                let body_y = y + roof_height;
                let body_height = size - roof_height;
                fb.draw_rect(x + size / 4, body_y, size / 2, body_height, color);
            }

            IconType::Computer => {
                // Bilgisayar ikonu: monitör ekran + ayak
                let color = Theme::TEXT_PRIMARY.to_u32();
                let screen_height = size * 3 / 4;

                // Ekran çerçevesi
                fb.draw_rect(x, y, size, screen_height, color);
                // Ekran içeriği (koyu)
                fb.draw_rect(x + 2, y + 2, size - 4, screen_height - 4, Theme::DESKTOP_BG.to_u32());
                // Monitör ayağı
                let stand_width = size / 3;
                let stand_x = x + (size - stand_width) / 2;
                fb.draw_rect(stand_x, y + screen_height, stand_width, size - screen_height, color);
            }

            IconType::Network => {
                // Ağ ikonu: mavi dolu daire (küre temsili)
                let color = 0x2196F3; // Mavi
                let center_x = x + size / 2;
                let center_y = y + size / 2;
                let radius = size / 3;

                // Daire dolgu (piksel bazlı hesaplama)
                for py in 0..size {
                    for px in 0..size {
                        let dx = px as i32 - center_x as i32;
                        let dy = py as i32 - center_y as i32;
                        if dx * dx + dy * dy < (radius * radius) as i32 {
                            fb.plot_pixel(x + px, y + py, color);
                        }
                    }
                }
            }

            IconType::Settings => {
                // Ayarlar ikonu: dışarıya çıkıntılı dişli çark + merkez daire
                let color = Theme::TEXT_SECONDARY.to_u32();
                let center = size / 2;
                let radius = size / 3;

                // Dış dişler (12 diş, 30° aralıklarla)
                for angle in 0..12 {
                    let a = angle as f32 * core::f32::consts::PI / 6.0;
                    let dx = (cosf(a) * radius as f32) as i32;
                    let dy = (sinf(a) * radius as f32) as i32;
                    fb.draw_rect(
                        (x as i32 + center as i32 + dx - 3) as usize,
                        (y as i32 + center as i32 + dy - 3) as usize,
                        6, 6, color
                    );
                }

                // Merkez daire
                fb.draw_rect(x + center - 4, y + center - 4, 8, 8, color);
            }

            IconType::Custom(subtype) => {
                // Özel ikon: alt türe göre renklendirilmiş kare + harf
                let color = match subtype {
                    0 => Theme::TEXT_PRIMARY.to_u32(),    // Metin
                    1 => 0x4CAF50,                        // Görüntü (yeşil)
                    2 => 0xE91E63,                        // Ses (pembe)
                    3 => 0xFF5722,                        // Video (portakal)
                    4 => 0x00BCD4,                        // Kod (camgöbeği)
                    5 => 0x795548,                        // Arşiv (kahverengi)
                    _ => Theme::TEXT_SECONDARY.to_u32(),
                };

                fb.draw_rect(x, y, size, size, color);

                // Tür belirtici harf göstergesi
                let letter = match subtype {
                    0 => "T",
                    1 => "I",
                    2 => "A",
                    3 => "V",
                    4 => "C",
                    5 => "Z",
                    _ => "?",
                };
                fb.draw_string(x + size / 2 - 4, y + size / 2 - 4, letter, Theme::DESKTOP_BG.to_u32());
            }
        }
    }

    /// Sınır dikdörtgenini döndürür.
    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    /// İkon kimliğini döndürür.
    pub fn id(&self) -> u32 {
        self.id
    }

    /// İkon adını döndürür.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// İkonun ilişkili eylemini döndürür.
    pub fn action(&self) -> &IconAction {
        &self.action
    }
}

/// İkon etkileşimlerinden üretilen olaylar
#[derive(Clone, Copy, Debug)]
pub enum IconEvent {
    None,
    Selected(u32),
    Deselected(u32),
    DoubleClick(u32),
    DragStart(u32),
    DragEnd(u32),
    ContextMenu(u32),
}

// ============================================================================
// MASAÜSTÜ İKON YÖNETİCİSİ
// ============================================================================

/// Tüm masaüstü ikonlarını yöneten yapı.
///
/// ## Izgara Düzeni
///
/// İkonlar `grid_cell_size × grid_cell_size` boyutlu hücrelere otomatik yerleştirilir.
/// Sol üst köşeden başlar, sütun sayısı ekran genişliğinden hesaplanır.
///
/// ```text
/// [0,0] [1,0] [2,0] ...
/// [0,1] [1,1] [2,1] ...
/// ```
///
/// ## Çoklu Seçim
///
/// Boş alana tıklayıp sürükleyerek lastik-bant (rubber-band) seçim dikdörtgeni oluşturulur.
/// Dikdörtgenle kesişen tüm ikonlar seçilir.
pub struct DesktopIconsManager {
    /// Tüm ikonlar (kimlik → ikon eşlemesi)
    icons: BTreeMap<u32, DesktopIcon>,
    /// Sonraki ikon kimliği
    next_id: u32,
    /// Izgara hücre boyutu (piksel)
    grid_cell_size: i32,
    /// Otomatik düzenleme etkin mi
    auto_arrange: bool,
    /// Izgaraya yapıştırma etkin mi
    snap_to_grid: bool,
    /// Masaüstü genişliği
    width: i32,
    /// Masaüstü yüksekliği (görev çubuğu hariç)
    height: i32,
    /// Şu an seçili ikon kimlikleri
    selected: Vec<u32>,
    /// Şu an sürüklenen ikon kimliği
    dragged: Option<u32>,
    /// Seçim dikdörtgeni (çoklu seçim için)
    selection_rect: Option<Rect>,
    /// Seçim başlangıç noktası
    selection_start: Option<(i32, i32)>,
}

impl DesktopIconsManager {
    /// Yeni bir masaüstü ikon yöneticisi oluşturur.
    pub fn new(width: i32, height: i32) -> Self {
        DesktopIconsManager {
            icons: BTreeMap::new(),
            next_id: 1,
            grid_cell_size: 80, // 80×80 piksel ızgara hücreleri
            auto_arrange: true,
            snap_to_grid: true,
            width,
            height,
            selected: Vec::new(),
            dragged: None,
            selection_rect: None,
            selection_start: None,
        }
    }

    /// Yeni bir ikon ekler; yeni kimliğini döndürür.
    pub fn add_icon(&mut self, mut icon: DesktopIcon) -> u32 {
        let id = self.next_id;
        self.next_id += 1;

        icon.id = id;

        // Otomatik düzenleme açıksa ızgara konumunu belirle
        if self.auto_arrange {
            let (gx, gy) = self.find_next_grid_position();
            icon.grid_x = gx;
            icon.grid_y = gy;
            icon.x = gx * self.grid_cell_size + 10;
            icon.y = gy * self.grid_cell_size + 10;
            icon.update_bounds();
        }

        self.icons.insert(id, icon);
        id
    }

    /// Verilen kimlikli ikonu kaldırır.
    pub fn remove_icon(&mut self, id: u32) {
        self.icons.remove(&id);
        self.selected.retain(|&i| i != id);
    }

    /// Bir sonraki boş ızgara konumunu bulur.
    ///
    /// Satır-sütun sıralamasıyla ilk boş hücreyi döndürür.
    fn find_next_grid_position(&self) -> (i32, i32) {
        let cols = self.width / self.grid_cell_size;

        for gy in 0..100 {
            for gx in 0..cols {
                let occupied = self.icons.values().any(|i| i.grid_x == gx && i.grid_y == gy);
                if !occupied {
                    return (gx, gy);
                }
            }
        }

        (0, 0)
    }

    /// Verilen piksel konumunu en yakın ızgara hücresine hizalar.
    fn snap_to_grid(&self, x: i32, y: i32) -> (i32, i32) {
        if !self.snap_to_grid {
            return (x, y);
        }

        let gx = x / self.grid_cell_size;
        let gy = y / self.grid_cell_size;

        (gx * self.grid_cell_size + 10, gy * self.grid_cell_size + 10)
    }

    /// Fare tuşuna basma olayını işler.
    ///
    /// Bir ikona basılıyorsa onu seçer ve sürüklemeyi başlatır.
    /// Boş alana basılıyorsa seçim dikdörtgenini başlatır.
    pub fn on_mouse_down(&mut self, x: i32, y: i32, time: u64) -> IconEvent {
        // Üzerine tıklanan ikon var mı?
        let hit_id = self
            .icons
            .iter()
            .rev()
            .find(|(_, icon)| icon.hit_test(x, y))
            .map(|(&id, _)| id);

        if let Some(id) = hit_id {
            let selected_ids = self.selected.clone();
            for sel_id in selected_ids {
                if sel_id != id {
                    if let Some(sel_icon) = self.icons.get_mut(&sel_id) {
                        sel_icon.set_selected(false);
                    }
                }
            }

            self.selected.clear();
            self.selected.push(id);

            if let Some(icon) = self.icons.get_mut(&id) {
                let event = icon.on_mouse_down(x, y, time);
                self.dragged = Some(id);
                return event;
            }
        }

        // Boş alana tıklandı: seçim dikdörtgenini başlat
        self.clear_selection();
        self.selection_start = Some((x, y));
        self.selection_rect = Some(Rect::new(x, y, 0, 0));

        IconEvent::None
    }

    /// Fare hareketi olayını işler.
    ///
    /// Sürükleme yapılıyorsa ikonu taşır.
    /// Seçim dikdörtgeni aktifse dikdörtgen içindeki ikonları seçer.
    pub fn on_mouse_move(&mut self, x: i32, y: i32) -> bool {
        let mut needs_redraw = false;

        // İkon sürükleme
        if let Some(dragged_id) = self.dragged {
            if let Some(icon) = self.icons.get_mut(&dragged_id) {
                needs_redraw = icon.on_mouse_move(x, y);
            }
        }

        // Seçim dikdörtgeni güncellemesi
        if let Some((sx, sy)) = self.selection_start {
            let rect = Rect::new(
                min(sx, x),
                min(sy, y),
                (x - sx).abs(),
                (y - sy).abs(),
            );
            self.selection_rect = Some(rect);

            // Dikdörtgen içindeki ikonları seç
            for (&id, icon) in self.icons.iter_mut() {
                let was_selected = self.selected.contains(&id);
                let in_rect = rect.intersects(&icon.bounds());

                if in_rect && !was_selected {
                    icon.set_selected(true);
                    self.selected.push(id);
                    needs_redraw = true;
                } else if !in_rect && was_selected {
                    icon.set_selected(false);
                    self.selected.retain(|&i| i != id);
                    needs_redraw = true;
                }
            }
        }

        needs_redraw
    }

    /// Fare tuşu bırakma olayını işler.
    ///
    /// Sürüklemeyi sonlandırır; etkinse ızgaraya hizalar.
    pub fn on_mouse_up(&mut self, x: i32, y: i32) -> IconEvent {
        let mut event = IconEvent::None;

        // İkon sürüklemeyi bitir
        if let Some(dragged_id) = self.dragged {
            let (icon_x, icon_y) = self
                .icons
                .get(&dragged_id)
                .map(|icon| (icon.x, icon.y))
                .unwrap_or((0, 0));
            let snap = if self.snap_to_grid {
                Some(self.snap_to_grid(icon_x, icon_y))
            } else {
                None
            };

            if let Some(icon) = self.icons.get_mut(&dragged_id) {
                event = icon.on_mouse_up();

                // Izgaraya hizala
                if let Some((snap_x, snap_y)) = snap {
                    icon.set_position(snap_x, snap_y);

                    // Izgara konumunu güncelle
                    icon.grid_x = snap_x / self.grid_cell_size;
                    icon.grid_y = snap_y / self.grid_cell_size;
                }
            }
        }

        self.dragged = None;
        self.selection_start = None;
        self.selection_rect = None;

        event
    }

    /// Çift tıklama olayını işler; tıklanan ikonun eylemini döndürür.
    pub fn on_double_click(&mut self, x: i32, y: i32) -> Option<&IconAction> {
        for icon in self.icons.values_mut().rev() {
            if icon.hit_test(x, y) {
                return Some(&icon.action);
            }
        }
        None
    }

    /// Tüm seçimleri temizler.
    pub fn clear_selection(&mut self) {
        for &id in &self.selected {
            if let Some(icon) = self.icons.get_mut(&id) {
                icon.set_selected(false);
            }
        }
        self.selected.clear();
    }

    /// Tüm ikonları seçer.
    pub fn select_all(&mut self) {
        self.selected.clear();
        for (&id, icon) in self.icons.iter_mut() {
            icon.set_selected(true);
            self.selected.push(id);
        }
    }

    /// Tüm ikonları ve aktif seçim dikdörtgenini framebuffer'a çizer.
    pub fn draw(&self, fb: &mut Framebuffer) {
        // Aktif seçim dikdörtgenini çiz (yarı saydam)
        if let Some(rect) = self.selection_rect {
            for py in rect.y..rect.y + rect.height {
                for px in rect.x..rect.x + rect.width {
                    if px >= 0 && py >= 0 && (px as usize) < fb.width && (py as usize) < fb.height {
                        let idx = (py as usize) * fb.pixels_per_scan_line + (px as usize);
                        let ptr = unsafe { (fb.base_addr as *mut u32).add(idx) };
                        unsafe {
                            let bg = *ptr;
                            // Seçim rengiyle karıştır
                            *ptr = ((bg >> 1) & 0x003F3F3F) | (Theme::ACCENT_PRIMARY.to_u32() >> 1);
                        }
                    }
                }
            }
        }

        // İkonları çiz
        for icon in self.icons.values() {
            icon.draw(fb);
        }
    }

    /// Masaüstü boyutunu günceller; otomatik düzenleme aktifse ikonları yeniden düzenler.
    pub fn resize(&mut self, width: i32, height: i32) {
        self.width = width;
        self.height = height;

        // Otomatik düzenleme açıksa ikonları yeniden yerleştir
        if self.auto_arrange {
            self.arrange_icons();
        }
    }

    /// Tüm ikonları ızgara sırasına göre otomatik düzenler.
    pub fn arrange_icons(&mut self) {
        let cols = self.width / self.grid_cell_size;
        let mut sorted_ids: Vec<u32> = self.icons.keys().copied().collect();
        sorted_ids.sort();

        for (idx, &id) in sorted_ids.iter().enumerate() {
            let gx = (idx as i32) % cols;
            let gy = (idx as i32) / cols;

            if let Some(icon) = self.icons.get_mut(&id) {
                icon.grid_x = gx;
                icon.grid_y = gy;
                icon.x = gx * self.grid_cell_size + 10;
                icon.y = gy * self.grid_cell_size + 10;
                icon.update_bounds();
            }
        }
    }

    /// Toplam ikon sayısını döndürür.
    pub fn count(&self) -> usize {
        self.icons.len()
    }

    /// Seçili ikon kimliklerini döndürür.
    pub fn selected(&self) -> &[u32] {
        &self.selected
    }

    /// Kimliğe göre ikon referansı döndürür.
    pub fn get(&self, id: u32) -> Option<&DesktopIcon> {
        self.icons.get(&id)
    }

    /// Otomatik düzenleme modunu ayarlar; etkinleştirilirse ikonları yeniden düzenler.
    pub fn set_auto_arrange(&mut self, enabled: bool) {
        self.auto_arrange = enabled;
        if enabled {
            self.arrange_icons();
        }
    }

    /// Izgaraya yapıştırma modunu ayarlar.
    pub fn set_snap_to_grid(&mut self, enabled: bool) {
        self.snap_to_grid = enabled;
    }
}

// ============================================================================
// GLOBAL MASAÜSTÜ İKONLARI
// ============================================================================

lazy_static::lazy_static! {
    static ref DESKTOP_ICONS: Mutex<DesktopIconsManager> = Mutex::new(DesktopIconsManager::new(1920, 1080));
}

/// Masaüstü ikon sistemini başlatır; varsayılan ikonları ekler.
pub fn init(width: i32, height: i32) {
    let mut icons = DESKTOP_ICONS.lock();
    icons.resize(width, height);

    // Varsayılan ikonları ekle
    icons.add_icon(DesktopIcon::folder(0, "Home", "/home", 10, 10));
    icons.add_icon(DesktopIcon::folder(0, "Documents", "/home/documents", 10, 10));
    icons.add_icon(DesktopIcon::folder(0, "Downloads", "/home/downloads", 10, 10));
    icons.add_icon(DesktopIcon::app(0, "Settings", "settings", 10, 10));
    icons.add_icon(DesktopIcon::new(0, "Trash", IconType::Trash, 10, 10));

    crate::serial_println!("[GUI] Desktop icons initialized ({} icons)", icons.count());
}

/// Global masaüstü ikon yöneticisine referans döndürür.
pub fn get_icons() -> &'static Mutex<DesktopIconsManager> {
    &DESKTOP_ICONS
}
