//! # Sanal Masaüstü Yöneticisi
//!
//! macOS/Linux tarzı çoklu sanal masaüstü desteği.
//! Her masaüstü kendi pencere setine sahipti ve bağımsız yönetilir.
//!
//! ## Mimari
//! - `VirtualDesktop`: Pencere listesi, arka plan rengi ve durum bilgisi
//! - `DesktopManager`: 16 adede kadar masaüstü yönetimi; ekleme, silme, geçiş
//! - `SwitchAnimation`: Geçiş animasyonu türleri (Slide, Fade, Cube)
//!
//! ## Geçiş Animasyonu
//! Masaüstleri arasında geçiş yapılırken alt kısımda nokta göstergesi gösterilir.
//! Aktif masaüstü beyaz (0xFFFFFF), pasifler gri (0x666666) ile gösterilir.
//!
//! ## Klavye Kısayolları
//! Sol/sağ ok tuşları ve 1-9 sayı tuşları ile masaüstleri arasında geçiş yapılır.

use crate::gop::framebuffer::Framebuffer;
use crate::gui::Rect;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::format;

// ============================================================================
// SANAL MASAÜSTÜ
// ============================================================================

/// Sanal masaüstü yapısı
#[derive(Clone, Debug)]
pub struct VirtualDesktop {
    /// Masaüstü kimliği
    pub id: usize,
    /// Masaüstü adı
    pub name: String,
    /// Sıralama indeksi
    pub index: usize,
    /// Bu masaüstündeki pencere kimlikleri
    pub windows: Vec<usize>,
    /// Arka plan rengi
    pub bg_color: u32,
    /// Aktif mi
    pub active: bool,
    /// Duvar kağıdı yolu (gelecek özellik)
    pub wallpaper: String,
}

impl VirtualDesktop {
    /// Belirtilen kimlik, ad ve sıra indeksiyle yeni bir sanal masaüstü oluşturur.
    /// Varsayılan arka plan rengi `0x1E1E1E` (koyu antrasit); pencere listesi boş başlar.
    pub fn new(id: usize, name: &str, index: usize) -> Self {
        VirtualDesktop {
            id,
            name: String::from(name),
            index,
            windows: Vec::new(),
            bg_color: 0x1E1E1E, // Koyu arka plan
            active: false,
            wallpaper: String::new(),
        }
    }

    /// Masaüstüne pencere ekle
    pub fn add_window(&mut self, window_id: usize) {
        if !self.windows.contains(&window_id) {
            self.windows.push(window_id);
        }
    }

    /// Masaüstünden pencere kaldır
    pub fn remove_window(&mut self, window_id: usize) {
        self.windows.retain(|&id| id != window_id);
    }

    /// Masaüstünde pencere var mı kontrol et
    pub fn has_window(&self, window_id: usize) -> bool {
        self.windows.contains(&window_id)
    }

    /// Pencere sayısını döndür
    pub fn window_count(&self) -> usize {
        self.windows.len()
    }
}

// ============================================================================
// MASAÜSTÜ YÖNETİCİSİ
// ============================================================================

/// Masaüstü geçiş animasyonu türleri
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SwitchAnimation {
    None,
    SlideLeft,  // Sola kaydırma
    SlideRight, // Sağa kaydırma
    Fade,       // Soluklaşma
    Cube,       // Küp efekti
}

/// Sanal masaüstü yöneticisi
pub struct DesktopManager {
    /// Sanal masaüstleri listesi
    desktops: Vec<VirtualDesktop>,
    /// Aktif masaüstü indeksi
    active_index: usize,
    /// Önceki masaüstü indeksi
    prev_index: usize,
    /// Maksimum masaüstü sayısı
    max_desktops: usize,
    /// Geçiş animasyonu türü
    switch_animation: SwitchAnimation,
    /// Animasyon ilerlemesi (0.0 - 1.0)
    anim_progress: f32,
    /// Animasyon devam ediyor mu
    is_animating: bool,
    /// Ekran genişliği
    screen_width: usize,
    /// Ekran yüksekliği
    screen_height: usize,
}

impl DesktopManager {
    /// Ekran boyutlarını alarak masaüstü yöneticisini başlatır.
    /// Varsayılan olarak 3 masaüstü oluşturulur ve ilki aktif yapılır.
    /// `max_desktops` = 16; bu sayıyı aşan ekleme talepleri reddedilir.
    pub fn new(screen_width: usize, screen_height: usize) -> Self {
        let mut manager = DesktopManager {
            desktops: Vec::new(),
            active_index: 0,
            prev_index: 0,
            max_desktops: 16,
            switch_animation: SwitchAnimation::SlideLeft,
            anim_progress: 0.0,
            is_animating: false,
            screen_width,
            screen_height,
        };

        // Varsayılan masaüstleri oluştur
        manager.add_desktop("Desktop 1");
        manager.add_desktop("Desktop 2");
        manager.add_desktop("Desktop 3");

        // İlkini aktif olarak ayarla
        if let Some(d) = manager.desktops.first_mut() {
            d.active = true;
        }

        manager
    }

    /// Ekran boyutlarını güncelle
    pub fn update_screen(&mut self, width: usize, height: usize) {
        self.screen_width = width;
        self.screen_height = height;
    }

    /// Yeni masaüstü ekle
    pub fn add_desktop(&mut self, name: &str) -> bool {
        if self.desktops.len() >= self.max_desktops {
            return false;
        }

        let id = self.desktops.len();
        let index = id;
        let desktop = VirtualDesktop::new(id, name, index);
        self.desktops.push(desktop);
        true
    }

    /// İndekse göre masaüstü kaldır
    pub fn remove_desktop(&mut self, index: usize) -> bool {
        if self.desktops.len() <= 1 {
            return false; // En az bir masaüstü kalmalı
        }

        if index < self.desktops.len() {
            self.desktops.remove(index);

            // İndeksleri güncelle
            for (i, d) in self.desktops.iter_mut().enumerate() {
                d.index = i;
            }

            // Gerekirse aktif indeksi düzelt
            if self.active_index >= self.desktops.len() {
                self.active_index = self.desktops.len() - 1;
            }

            true
        } else {
            false
        }
    }

    /// İndekse göre masaüstüne geç
    pub fn switch_to(&mut self, index: usize) -> bool {
        if index >= self.desktops.len() || index == self.active_index {
            return false;
        }

        // Öncekini sakla
        self.prev_index = self.active_index;

        // Aktif durumları güncelle
        self.desktops[self.active_index].active = false;
        self.desktops[index].active = true;
        self.active_index = index;

        // Animasyonu başlat
        self.anim_progress = 0.0;
        self.is_animating = true;

        // Animasyon yönünü belirle
        if index > self.prev_index {
            self.switch_animation = SwitchAnimation::SlideLeft;
        } else {
            self.switch_animation = SwitchAnimation::SlideRight;
        }

        true
    }

    /// Sonraki masaüstüne geç
    pub fn switch_next(&mut self) -> bool {
        let next = if self.active_index + 1 >= self.desktops.len() {
            0
        } else {
            self.active_index + 1
        };
        self.switch_to(next)
    }

    /// Önceki masaüstüne geç
    pub fn switch_prev(&mut self) -> bool {
        let prev = if self.active_index == 0 {
            self.desktops.len() - 1
        } else {
            self.active_index - 1
        };
        self.switch_to(prev)
    }

    /// Aktif masaüstünü döndür
    pub fn active_desktop(&self) -> Option<&VirtualDesktop> {
        self.desktops.get(self.active_index)
    }

    /// Aktif masaüstünü değiştirilebilir olarak döndür
    pub fn active_desktop_mut(&mut self) -> Option<&mut VirtualDesktop> {
        self.desktops.get_mut(self.active_index)
    }

    /// İndekse göre masaüstünü döndür
    pub fn get_desktop(&self, index: usize) -> Option<&VirtualDesktop> {
        self.desktops.get(index)
    }

    /// İndekse göre masaüstünü değiştirilebilir döndür
    pub fn get_desktop_mut(&mut self, index: usize) -> Option<&mut VirtualDesktop> {
        self.desktops.get_mut(index)
    }

    /// Masaüstü sayısını döndür
    pub fn desktop_count(&self) -> usize {
        self.desktops.len()
    }

    /// Aktif indeksi döndür
    pub fn active_index(&self) -> usize {
        self.active_index
    }

    /// Masaüstünü yeniden adlandır
    pub fn rename_desktop(&mut self, index: usize, name: &str) -> bool {
        if let Some(d) = self.desktops.get_mut(index) {
            d.name = String::from(name);
            true
        } else {
            false
        }
    }

    /// Pencereyi bir masaüstünden diğerine taşı
    pub fn move_window_to_desktop(&mut self, window_id: usize, from_desktop: usize, to_desktop: usize) -> bool {
        if from_desktop >= self.desktops.len() || to_desktop >= self.desktops.len() {
            return false;
        }

        self.desktops[from_desktop].remove_window(window_id);
        self.desktops[to_desktop].add_window(window_id);
        true
    }

    /// Aktif masaüstüne pencere ekle
    pub fn add_window_to_active(&mut self, window_id: usize) {
        if let Some(d) = self.active_desktop_mut() {
            d.add_window(window_id);
        }
    }

    /// Tüm masaüstlerinden pencereyi kaldır
    pub fn remove_window(&mut self, window_id: usize) {
        for d in &mut self.desktops {
            d.remove_window(window_id);
        }
    }

    /// Pencereyi içeren masaüstünü bul
    pub fn find_window_desktop(&self, window_id: usize) -> Option<usize> {
        self.desktops.iter()
            .position(|d| d.has_window(window_id))
    }

    /// Animasyonu güncelle
    pub fn update_animation(&mut self) {
        if !self.is_animating {
            return;
        }

        self.anim_progress += 0.1;
        if self.anim_progress >= 1.0 {
            self.anim_progress = 1.0;
            self.is_animating = false;
        }
    }

    /// Animasyon devam ediyor mu
    pub fn is_animating(&self) -> bool {
        self.is_animating
    }

    /// Masaüstü geçiş animasyonunu çiz
    pub fn draw_animation(&self, fb: &mut Framebuffer) {
        if !self.is_animating {
            return;
        }

        match self.switch_animation {
            SwitchAnimation::None => {}
            SwitchAnimation::SlideLeft | SwitchAnimation::SlideRight => {
                // Kaydırma animasyonu — alt gösterge çiz
                let indicator_y = self.screen_height - 40;
                let indicator_width = self.desktops.len() * 20;
                let start_x = (self.screen_width - indicator_width) / 2;

                for (i, _) in self.desktops.iter().enumerate() {
                    let x = start_x + i * 20;
                    let color = if i == self.active_index {
                        0xFFFFFF // Aktif: beyaz
                    } else {
                        0x666666 // Pasif: gri
                    };
                    fb.draw_rect(x, indicator_y, 10, 10, color);
                }
            }
            SwitchAnimation::Fade => {
                // Soluklaşma katmanı
                let alpha = 1.0 - self.anim_progress;
                let _overlay_color = (alpha * 255.0) as u32;
                // Basit soluklaşma efekti
                for y in 0..self.screen_height {
                    for x in 0..self.screen_width {
                        if x % 8 == 0 && y % 8 == 0 {
                            let existing = fb.get_pixel(x, y);
                            fb.plot_pixel(x, y, blend_colors(existing, 0x000000, alpha));
                        }
                    }
                }
            }
            SwitchAnimation::Cube => {
                // Küp efekti — perspektifli kaydırma olarak basitleştirilmiş
                // Şimdilik sadece kaydırma göstergesi çiz
                self.draw_slide_indicator(fb);
            }
        }
    }

    /// Kaydırma göstergesini çiz
    fn draw_slide_indicator(&self, fb: &mut Framebuffer) {
        let indicator_y = self.screen_height - 40;
        let indicator_width = self.desktops.len() * 20;
        let start_x = (self.screen_width - indicator_width) / 2;

        for (i, _) in self.desktops.iter().enumerate() {
            let x = start_x + i * 20;
            let color = if i == self.active_index {
                0xFFFFFF // Aktif masaüstü: beyaz
            } else if i == self.prev_index && self.is_animating {
                0xAAAAAA // Önceki masaüstü: açık gri
            } else {
                0x666666 // Pasif: koyu gri
            };
            fb.draw_rect(x, indicator_y, 10, 10, color);
        }
    }

    /// Masaüstü göstergesini çiz (UI için)
    pub fn draw_indicator(&self, fb: &mut Framebuffer, x: usize, y: usize) {
        // Masaüstü noktalarını çiz
        for (i, d) in self.desktops.iter().enumerate() {
            let dot_x = x + i * 16;
            let color = if i == self.active_index {
                0xFFFFFF // Aktif: beyaz
            } else {
                0x888888 // Pasif: gri
            };

            // Noktayı çiz
            fb.draw_rect(dot_x, y, 8, 8, color);

            // Pencere sayısı göstergesi
            if d.window_count() > 0 {
                fb.draw_rect(dot_x + 3, y + 10, 2, 2, 0xAAAAAA);
            }
        }
    }

    /// Masaüstü adlarını listele
    pub fn get_desktop_names(&self) -> Vec<String> {
        self.desktops.iter().map(|d| d.name.clone()).collect()
    }
}

// ============================================================================
// YARDIMCI FONKSİYONLAR
// ============================================================================

/// İki rengi alfa kanalıyla karıştır
fn blend_colors(c1: u32, c2: u32, alpha: f32) -> u32 {
    let r1 = ((c1 >> 16) & 0xFF) as f32;
    let g1 = ((c1 >> 8) & 0xFF) as f32;
    let b1 = (c1 & 0xFF) as f32;

    let r2 = ((c2 >> 16) & 0xFF) as f32;
    let g2 = ((c2 >> 8) & 0xFF) as f32;
    let b2 = (c2 & 0xFF) as f32;

    let r = (r1 * (1.0 - alpha) + r2 * alpha) as u32;
    let g = (g1 * (1.0 - alpha) + g2 * alpha) as u32;
    let b = (b1 * (1.0 - alpha) + b2 * alpha) as u32;

    (r << 16) | (g << 8) | b
}

// ============================================================================
// KLAVYE KISAYOLLARI
// ============================================================================

/// Masaüstü değiştirme kısayolunu işle
pub fn handle_desktop_shortcut(key_code: u8, manager: &mut DesktopManager) -> bool {
    match key_code {
        // Sol ok — önceki masaüstü
        0x25 => manager.switch_prev(),
        // Sağ ok — sonraki masaüstü
        0x27 => manager.switch_next(),
        // 1-9 sayı tuşları — masaüstüne geç
        k if k >= 0x02 && k <= 0x0A => {
            let desktop_idx = (k - 0x02) as usize;
            manager.switch_to(desktop_idx)
        }
        _ => false,
    }
}
