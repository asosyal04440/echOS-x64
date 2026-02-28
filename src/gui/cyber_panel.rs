//! # echOS CyberPanel (Üst Durum Çubuğu)
//!
//! Ekranın üstünde 32 piksel yüksekliğinde duran minimal, mat koyu Cyber-Industrial panel.
//!
//! ## Panel Bölgeleri
//! - **Sol**: `⬡` logo sembolü + aktif pencere başlığı (WM'dan alınır)
//! - **Orta**: Workspace göstergesi; animasyonlu geçişli bullet nokta serisi
//! - **Sağ**: CPU sparkline grafiği | RAM doluluk çubuğu | Saat | Bildirim sayacı
//!
//! ## SparkBuf (Kayan Örnek Tamponu)
//! `SparkBuf`, sabit boyutlu dairesel bir tampondur (`SPARKLINE_SAMPLES = 60` kare).
//! `push()` çağrıldığında baş (`head`) indeksi bir ilerler; bu sayede hiçbir bellek
//! kopyalaması olmadan son 60 örnek her zaman hazır tutulur. `ordered()` metodu
//! bu dairesel tamponu doğrusal görünümlü bir dizi olarak sunar.
//!
//! ## Workspace Animasyonu
//! `WorkspaceAnim.t` değişkeni 0.0→1.0 arası linear olarak artar. Geçiş sırasında
//! eski nokta soluklaşırken (`1-t` parlaklığı) yeni nokta belirginleşir (`t` parlaklığı).
//! `t >= 1.0` olunca `prev = current` atanır ve animasyon tamamlanmış sayılır.

use crate::gop::framebuffer::Framebuffer;
use crate::gui::echos_wm::CyberTheme;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Panel yüksekliği (piksel).
pub const PANEL_HEIGHT: i32 = 32;

/// CPU/RAM örnekleme penceresi (kare sayısı).
const SPARKLINE_SAMPLES: usize = 60;

/// Tek bir canlı metrik sparkline tamponu.
struct SparkBuf {
    samples: [f32; SPARKLINE_SAMPLES],
    head: usize,
}

impl SparkBuf {
    const fn new() -> Self {
        Self { samples: [0.0; SPARKLINE_SAMPLES], head: 0 }
    }
    fn push(&mut self, v: f32) {
        self.samples[self.head] = v.clamp(0.0, 1.0);
        self.head = (self.head + 1) % SPARKLINE_SAMPLES;
    }
    /// En son değeri (son itilen).
    fn latest(&self) -> f32 {
        self.samples[(self.head + SPARKLINE_SAMPLES - 1) % SPARKLINE_SAMPLES]
    }
    /// Zamansal sırayla dizi (en eskiden en yeniye).
    fn ordered(&self) -> [f32; SPARKLINE_SAMPLES] {
        let mut out = [0.0f32; SPARKLINE_SAMPLES];
        for i in 0..SPARKLINE_SAMPLES {
            out[i] = self.samples[(self.head + i) % SPARKLINE_SAMPLES];
        }
        out
    }
}

/// Workspace göstergesi animasyon durumu.
struct WorkspaceAnim {
    /// Geçerli workspace (0-tabanlı).
    current: usize,
    /// Önceki workspace.
    prev: usize,
    /// Geçiş animasyonu ilerleme (0.0→1.0).
    t: f32,
    /// Toplam workspace sayısı.
    count: usize,
}

impl WorkspaceAnim {
    const fn new() -> Self {
        Self { current: 0, prev: 0, t: 1.0, count: 4 }
    }
    fn switch_to(&mut self, idx: usize) {
        if idx != self.current {
            self.prev = self.current;
            self.current = idx;
            self.t = 0.0;
        }
    }
    fn update(&mut self, dt: f32) {
        if self.t < 1.0 {
            self.t = (self.t + dt * 10.0).min(1.0); // hızlı 100ms geçiş
        }
    }
}

/// CyberPanel ana yapısı.
pub struct CyberPanel {
    width: i32,
    /// Aktif pencere başlığı (sol bölge).
    active_title: String,
    cpu: SparkBuf,
    ram: SparkBuf,
    workspace: WorkspaceAnim,
    /// Okunmamış bildirim sayısı.
    notif_count: u32,
    /// Saat metni "HH:MM".
    clock_text: String,
    /// Frame sayacı (metrik güncelleme throttle).
    frame: u64,
}

impl CyberPanel {
    pub fn new(width: i32) -> Self {
        Self {
            width,
            active_title: String::new(),
            cpu: SparkBuf::new(),
            ram: SparkBuf::new(),
            workspace: WorkspaceAnim::new(),
            notif_count: 0,
            clock_text: "00:00".to_string(),
            frame: 0,
        }
    }

    pub fn set_active_title(&mut self, title: &str) {
        self.active_title = title.to_string();
    }

    pub fn set_workspace(&mut self, idx: usize) {
        self.workspace.switch_to(idx);
    }

    pub fn set_workspace_count(&mut self, n: usize) {
        self.workspace.count = n.max(1);
    }

    pub fn add_notification(&mut self) {
        self.notif_count = self.notif_count.saturating_add(1);
    }

    pub fn clear_notifications(&mut self) {
        self.notif_count = 0;
    }

    /// Her kare çağrılır (dt saniye cinsinden).
    pub fn update(&mut self, dt: f32) {
        self.frame += 1;
        self.workspace.update(dt);

        // Her 60 karede bir metrikleri güncelle (yaklaşık 1 saniye)
        if self.frame % 60 == 0 {
            self.cpu.push(self.sample_cpu());
            self.ram.push(self.sample_ram());
            self.update_clock();
        }
    }

    /// Simüle edilmiş CPU kullanımı (gerçek scheduler entegrasyonu hazır).
    fn sample_cpu(&self) -> f32 {
        // TODO: crate::task::scheduler::cpu_utilization() ile değiştir
        let tick = crate::task::scheduler::get_ticks();
        // basit sahte değer: ticks üzerinden küçük salınım
        let base = 0.12f32;
        let osc = libm::sinf(tick as f32 * 0.05) * 0.08;
        (base + osc).clamp(0.0, 1.0)
    }

    /// Simüle edilmiş RAM kullanımı.
    fn sample_ram(&self) -> f32 {
        // TODO: crate::memory::get_used_fraction() ile değiştir
        0.38
    }

    fn update_clock(&mut self) {
        // TODO: gerçek RTC entegrasyonu
        // Şimdilik ticks → MM:SS (saniye/dakika cinsinden)
        let ticks = crate::task::scheduler::get_ticks();
        let secs = ticks / 100; // varsayım: 100 tick/s
        let minutes = (secs / 60) % 60;
        let hours = (secs / 3600) % 24;
        // Format: HH:MM
        let h = hours as u8;
        let m = minutes as u8;
        self.clock_text.clear();
        push_two_digit(&mut self.clock_text, h);
        self.clock_text.push(':');
        push_two_digit(&mut self.clock_text, m);
    }

    /// Paneli framebuffer'a çizer.
    pub fn draw(&self, fb: &mut Framebuffer) {
        let w = self.width as usize;
        let h = PANEL_HEIGHT as usize;
        let panel_base = 0xFF0D0F13u32; // koyu mat arka plan

        // --- Arka Plan (frosted glass: %85 opak koyu) ---
        for y in 0..h {
            for x in 0..w {
                let bg = fb.get_pixel(x, y);
                // Panel arka planını bg ile blend: alpha=0.85 panel üste
                let blended = alpha_over(panel_base, bg, 217);
                fb.plot_pixel(x, y, blended);
            }
        }

        // --- Alt ince border çizgisi ---
        let border_color = 0xFF1E2530;
        for x in 0..w {
            fb.plot_pixel(x, h - 1, border_color);
        }

        // --- SOL: Logo + aktif pencere başlığı ---
        let logo = "\u{2B21}"; // ⬡ hexagon
        let logo_x = 8;
        let logo_y = (PANEL_HEIGHT - 10) / 2;
        draw_text(fb, logo_x, logo_y as usize, logo, CyberTheme::ACCENT, 0);
        let title_x = logo_x + 22;
        let title = if self.active_title.is_empty() {
            "echOS"
        } else {
            &self.active_title
        };
        draw_text(fb, title_x, logo_y as usize, title, CyberTheme::TEXT_PRIMARY, 0);

        // --- ORTA: Workspace bullet noktaları ---
        let dot_total = self.workspace.count;
        let dot_size = 7i32;
        let dot_gap = 5i32;
        let total_w = dot_total as i32 * (dot_size + dot_gap) - dot_gap;
        let dots_start_x = (self.width - total_w) / 2;
        let dot_y = (PANEL_HEIGHT - dot_size) / 2;
        for i in 0..dot_total {
            let dx = dots_start_x + i as i32 * (dot_size + dot_gap);
            let is_active = i == self.workspace.current;
            let is_prev = i == self.workspace.prev && self.workspace.t < 1.0;
            let color = if is_active {
                // t ile accent rengi canlanıyor
                let t = self.workspace.t;
                blend_color(CyberTheme::TEXT_SECONDARY, CyberTheme::ACCENT, t)
            } else if is_prev {
                let t = self.workspace.t;
                blend_color(CyberTheme::ACCENT, CyberTheme::TEXT_SECONDARY, t)
            } else {
                CyberTheme::TEXT_SECONDARY
            };
            // Küçük kare nokta
            for py in 0i32..dot_size {
                for px in 0i32..dot_size {
                    let bx = (dx + px) as usize;
                    let by = (dot_y + py) as usize;
                    if bx < w && by < h {
                        fb.plot_pixel(bx, by, color);
                    }
                }
            }
        }

        // --- SAĞ: Saat + CPU sparkline + RAM bar + bildirim ---
        let right_margin = 8i32;
        let mut cursor_x = self.width - right_margin;

        // Bildirim göstergesi
        if self.notif_count > 0 {
            cursor_x -= 24;
            draw_text(fb, cursor_x as usize, logo_y as usize, "●", 0xFFFF2D55, 0);
        }

        // Saat
        cursor_x -= (self.clock_text.len() as i32) * 7 + 8;
        draw_text(fb, cursor_x as usize, logo_y as usize, &self.clock_text, CyberTheme::TEXT_PRIMARY, 0);

        // Separator
        cursor_x -= 12;
        draw_sparkline_separator(fb, cursor_x as usize, 6, h - 6);

        // RAM bar (20px geniş, panel ortasına hizalı)
        cursor_x -= 24;
        let ram = self.ram.latest();
        draw_mini_bar(fb, cursor_x as usize, 6, 20, h - 12, ram, 0xFF8892A0, CyberTheme::ACCENT);
        cursor_x -= 6;
        draw_text(fb, (cursor_x - 14) as usize, logo_y as usize, "M", CyberTheme::TEXT_SECONDARY, 0);
        cursor_x -= 20;

        // CPU sparkline (40px geniş)
        cursor_x -= 44;
        let spark = self.cpu.ordered();
        draw_sparkline(fb, cursor_x as usize, 4, 40, h - 8, &spark, CyberTheme::ACCENT);
        cursor_x -= 6;
        draw_text(fb, (cursor_x - 14) as usize, logo_y as usize, "C", CyberTheme::TEXT_SECONDARY, 0);
    }
}

// ============================================================
// YARDIMCI ÇİZİM FONKSİYONLARI
// ============================================================

/// Basit ASCII/VGA bitmap metin çizimi (8×8 piksel, ölçeksiz).
fn draw_text(fb: &mut Framebuffer, x: usize, y: usize, text: &str, color: u32, _scale: u8) {
    let mut cx = x;
    for c in text.chars() {
        if c == ' ' { cx += 7; continue; }
        let glyph = crate::font::vga_font::get_font_data(c);
        for (row, &byte) in glyph.iter().take(8).enumerate() {
            for bit in 0..8 {
                if byte & (0x80 >> bit) != 0 {
                    let px = cx + bit;
                    let py = y + row;
                    fb.plot_pixel(px, py, color);
                }
            }
        }
        cx += 7;
    }
}

/// Yatay ince separator çizgisi.
fn draw_sparkline_separator(fb: &mut Framebuffer, x: usize, y_start: usize, y_end: usize) {
    let color = 0xFF1E2530;
    for y in y_start..y_end {
        fb.plot_pixel(x, y, color);
    }
}

/// Küçük dikey bar grafik (tek değer).
fn draw_mini_bar(
    fb: &mut Framebuffer,
    x: usize, y: usize,
    w: usize, h: usize,
    value: f32,
    bg: u32, fg: u32,
) {
    let filled = (h as f32 * value) as usize;
    for row in 0..h {
        for col in 0..w {
            let color = if row >= h - filled { fg } else { bg };
            fb.plot_pixel(x + col, y + row, color);
        }
    }
}

/// 60-nokta sparkline (CPU grafiği gibi).
fn draw_sparkline(
    fb: &mut Framebuffer,
    x: usize, y: usize,
    w: usize, h: usize,
    samples: &[f32; SPARKLINE_SAMPLES],
    color: u32,
) {
    let step = w as f32 / SPARKLINE_SAMPLES as f32;
    for i in 0..SPARKLINE_SAMPLES {
        let v = samples[i].clamp(0.0, 1.0);
        let px = x + (i as f32 * step) as usize;
        let bar_h = (h as f32 * v) as usize;
        for row in 0..bar_h {
            let py = y + h - 1 - row;
            if px < fb.width && py < fb.height {
                // Bit saydamlık: alt satırlar daha soluk
                let alpha = ((row + 1) as f32 / bar_h.max(1) as f32 * 200.0) as u8;
                let bg = fb.get_pixel(px, py);
                let blended = alpha_over(color, bg, alpha);
                fb.plot_pixel(px, py, blended);
            }
        }
    }
}

/// Alpha-over kompozit işlemi.
/// `src_color`: 0xAARRGGBB, `dst`: opak renk, `alpha`: ek saydamlık katsayısı.
#[inline(always)]
pub fn alpha_over(src: u32, dst: u32, alpha: u8) -> u32 {
    let a = alpha as u32;
    let inv_a = 255 - a;
    let sr = (src >> 16) & 0xFF;
    let sg = (src >> 8)  & 0xFF;
    let sb =  src        & 0xFF;
    let dr = (dst >> 16) & 0xFF;
    let dg = (dst >> 8)  & 0xFF;
    let db =  dst        & 0xFF;
    let or = (sr * a + dr * inv_a) / 255;
    let og = (sg * a + dg * inv_a) / 255;
    let ob = (sb * a + db * inv_a) / 255;
    0xFF000000 | (or << 16) | (og << 8) | ob
}

/// iki u32 rengi t∈[0,1] ile doğrusal interpolasyon.
fn blend_color(a: u32, b: u32, t: f32) -> u32 {
    let ti = (t * 255.0) as u32;
    let inv = 255 - ti;
    let ar = (a >> 16) & 0xFF; let br = (b >> 16) & 0xFF;
    let ag = (a >>  8) & 0xFF; let bg = (b >>  8) & 0xFF;
    let ab =  a        & 0xFF; let bb =  b        & 0xFF;
    let or = (ar * inv + br * ti) / 255;
    let og = (ag * inv + bg * ti) / 255;
    let ob = (ab * inv + bb * ti) / 255;
    0xFF000000 | (or << 16) | (og << 8) | ob
}

/// u8'i iki karakterli string'e çevirir (heap ayırma yok).
fn push_two_digit(s: &mut String, n: u8) {
    let d1 = b'0' + (n / 10);
    let d2 = b'0' + (n % 10);
    s.push(d1 as char);
    s.push(d2 as char);
}
