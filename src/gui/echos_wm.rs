//! # echOS Cyber-Industrial Window Manager
//!
//! Tüm window manager bileşenlerinin temel veri yapıları.
//! Hem fare hem klavye olaylarını aynı anda işleyen hibrit input
//! mimarisi, alpha compositing için katmanlı pencere modeli ve
//! Cyber-Industrial görsel sistemi burada tanımlanır.

use crate::gui::widgets::Rect;
use alloc::string::String;
use alloc::vec::Vec;

// ============================================================
// CYBER-INDUSTRIAL TEMA
// ============================================================

/// echOS özgün Cyber-Industrial renk paleti.
/// Koyu zemin, siyanik accent, mat cam efektleri için alpha sabitleri.
pub struct CyberTheme;

impl CyberTheme {
    // --- Zemin Katmanları ---
    /// En derin arkaplan: #0A0C0F
    pub const BG_DEEP: u32      = 0x000A0C0F;
    /// Normal pencere arka planı: #10141A
    pub const BG_WINDOW: u32    = 0x0010141A;
    /// Başlık çubuğu: #0E1117 + %50 saydamlık (cam efekti)
    pub const BG_TITLEBAR: u32  = 0x800E1117;
    /// Panel arkaplanı: #0D0F13 + %85 saydamlık
    pub const BG_PANEL: u32     = 0xD90D0F13;

    // --- Accent Renkleri ---
    /// Ana accent: #00FFB2 (Biyonik siyanik yeşil)
    pub const ACCENT: u32       = 0xFF00FFB2;
    /// Hover accent: #00D495
    pub const ACCENT_HOVER: u32 = 0xFF00D495;
    /// Tehlike/kapat: #FF2D55
    pub const DANGER: u32       = 0xFFFF2D55;
    /// Uyarı: #FFB800
    pub const WARNING: u32      = 0xFFFFB800;
    /// Başarı: #39FF89
    pub const SUCCESS: u32      = 0xFF39FF89;

    // --- Metin ---
    /// Ana metin: #E8ECF0
    pub const TEXT_PRIMARY: u32   = 0xFFE8ECF0;
    /// İkincil metin: #8892A0
    pub const TEXT_SECONDARY: u32 = 0xFF8892A0;
    /// Devre dışı metin: #3A4050
    pub const TEXT_DISABLED: u32  = 0xFF3A4050;
    /// Accent metin: #00FFB2
    pub const TEXT_ACCENT: u32    = 0xFF00FFB2;

    // --- Kenarlık & Izgara ---
    /// İnce border: #1E2530
    pub const BORDER: u32         = 0xFF1E2530;
    /// Parlak border (aktif pencere): #00FFB2 + %40
    pub const BORDER_ACTIVE: u32  = 0x6600FFB2;
    /// Grid çizgisi: #131820
    pub const GRID_LINE: u32      = 0xFF131820;

    // --- Pencere Kontrol Butonları ---
    pub const BTN_CLOSE: u32    = 0xFFFF3B30;
    pub const BTN_MIN: u32      = 0xFFFFCC00;
    pub const BTN_MAX: u32      = 0xFF28C840;
    pub const BTN_HOVER_CLOSE: u32 = 0xFFFF6B6B;
    pub const BTN_HOVER_MIN: u32   = 0xFFFFD93D;
    pub const BTN_HOVER_MAX: u32   = 0xFF4CD964;

    // --- Gölge ---
    pub const SHADOW: u32 = 0xA0000000;

    // --- Alpha sabitleri (0..255) ---
    pub const ALPHA_GLASS: u8   = 128; // %50 cam
    pub const ALPHA_PANEL: u8   = 217; // %85 panel
    pub const ALPHA_SHADOW: u8  = 100; // gölge
}

// ============================================================
// PENCERE KİMLİĞİ
// ============================================================

/// Evrensel pencere kimliği. 0 == geçersiz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct WindowId(pub u32);

impl WindowId {
    pub const INVALID: Self = WindowId(0);
    pub fn valid(&self) -> bool { self.0 != 0 }
}

// ============================================================
// SNAP (YAPIŞMA) KENARLARI
// ============================================================

/// Pencere yapışma hedefi (Snap Assist).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapTarget {
    Left,       // Ekranın sol yarısı
    Right,      // Ekranın sağ yarısı
    Top,        // Ekranın üst yarısı (maximize gibi ama %50 yükseklik)
    Maximize,   // Tam ekran (başlık çubuğu hariç)
    TopLeft,    // Sol üst çeyrek
    TopRight,   // Sağ üst çeyrek
    BotLeft,    // Sol alt çeyrek
    BotRight,   // Sağ alt çeyrek
}

impl SnapTarget {
    /// Verilen ekran boyutlarına göre hedef Rect'i hesaplar.
    pub fn compute_rect(&self, screen_w: i32, screen_h: i32, panel_h: i32) -> Rect {
        let usable_h = screen_h - panel_h;
        let half_w = screen_w / 2;
        let half_h = usable_h / 2;
        match self {
            SnapTarget::Left     => Rect::new(0, panel_h, half_w, usable_h),
            SnapTarget::Right    => Rect::new(half_w, panel_h, half_w, usable_h),
            SnapTarget::Top      => Rect::new(0, panel_h, screen_w, half_h),
            SnapTarget::Maximize => Rect::new(0, panel_h, screen_w, usable_h),
            SnapTarget::TopLeft  => Rect::new(0, panel_h, half_w, half_h),
            SnapTarget::TopRight => Rect::new(half_w, panel_h, half_w, half_h),
            SnapTarget::BotLeft  => Rect::new(0, panel_h + half_h, half_w, usable_h - half_h),
            SnapTarget::BotRight => Rect::new(half_w, panel_h + half_h, half_w, usable_h - half_h),
        }
    }
}

// ============================================================
// PENCERE DURUM MAKİNESİ
// ============================================================

/// Bir pencerenin geçerli durumu ve animasyon ilerlemesi.
/// `f32` alanları 0.0..=1.0 arasında animasyon t değeridir.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WinState {
    /// Normal serbest pencere.
    Normal,
    /// Açılış animasyonu (t: 0→1 ease-out-expo).
    Opening(f32),
    /// Kapatma animasyonu (t: 0→1 ease-in-back, Dock'a büzülür).
    Closing(f32),
    /// Küçültme animasyonu (t: 0→1, Dock'a büzülür).
    Minimizing(f32),
    /// Açılma animasyonu (Dock'tan → normal, t: 0→1 ease-out-back).
    Restoring(f32),
    /// Snap animasyonu (t: 0→1 ease-out-cubic, hedef `SnapTarget`).
    Snapping { target: SnapTarget, t: f32 },
    /// Snap tamamlandı, sabit konum.
    Snapped(SnapTarget),
    /// Maximize animasyonu (t: 0→1).
    Maximizing(f32),
    /// Tam büyütülmüş.
    Maximized,
    /// Küçültülmüş (Dock'ta, görünmez).
    Minimized,
}

impl WinState {
    /// Pencere görünür ve etkileşilebilir durumda mı?
    pub fn is_visible(&self) -> bool {
        !matches!(self, WinState::Minimized | WinState::Closing(_))
    }
    /// Animasyon devam ediyor mu?
    pub fn is_animating(&self) -> bool {
        matches!(
            self,
            WinState::Opening(_)
                | WinState::Closing(_)
                | WinState::Minimizing(_)
                | WinState::Restoring(_)
                | WinState::Snapping { .. }
                | WinState::Maximizing(_)
        )
    }
}

// ============================================================
// PENCERE ÇERÇEVE BİLGİSİ
// ============================================================

/// Bir pencerenin tüm görsel ve durum bilgisi.
#[derive(Debug, Clone)]
pub struct WindowFrame {
    /// Pencere benzersiz kimliği.
    pub id: WindowId,
    /// Başlık çubuğunda gösterilen başlık.
    pub title: String,
    /// Pencerenin ekrandaki konumu ve boyutu (belirleyici kayıt).
    pub rect: Rect,
    /// Snap/maximize öncesi kayıt (geri yükleme için).
    pub normal_rect: Rect,
    /// Geçerli animasyon durumu.
    pub state: WinState,
    /// Z-sırası: düşük = arka, yüksek = ön.
    pub z_order: u32,
    /// Aktif (fokuslanmış) pencere mi?
    pub focused: bool,
    /// Genel saydamlık (0.0=tamamen saydam, 1.0=opak).
    pub opacity: f32,
    /// Cam bulanıklığı yarıçapı (piksel cinsinden).
    pub blur_radius: u8,
    /// Gölge yayılma miktarı.
    pub shadow_spread: u8,
    /// Pencere dekorasyonu bayrakları.
    pub decorations: DecoFlags,
    /// İlişkilendirilmiş süreç (ELF user-space) — None ise kernel-içi.
    pub pid: Option<u32>,
}

/// Pencere dekorasyonu özellikleri.
#[derive(Debug, Clone, Copy)]
pub struct DecoFlags {
    pub has_titlebar: bool,
    pub has_close: bool,
    pub has_minimize: bool,
    pub has_maximize: bool,
    pub resizable: bool,
    pub borderless: bool,
    pub always_on_top: bool,
}

impl Default for DecoFlags {
    fn default() -> Self {
        Self {
            has_titlebar: true,
            has_close: true,
            has_minimize: true,
            has_maximize: true,
            resizable: true,
            borderless: false,
            always_on_top: false,
        }
    }
}

// ============================================================
// INPUT OLAYLARI — Birleşik fare + klavye enum
// ============================================================

/// Klavye değiştirici tuşlar (bitmask).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers(pub u8);

impl Modifiers {
    pub const SHIFT: u8   = 0b00000001;
    pub const CTRL: u8    = 0b00000010;
    pub const ALT: u8     = 0b00000100;
    pub const SUPER: u8   = 0b00001000; // Windows/Command tuşu

    pub fn shift(&self) -> bool  { self.0 & Self::SHIFT  != 0 }
    pub fn ctrl(&self) -> bool   { self.0 & Self::CTRL   != 0 }
    pub fn alt(&self) -> bool    { self.0 & Self::ALT    != 0 }
    pub fn sup(&self) -> bool    { self.0 & Self::SUPER  != 0 }
}

/// Fare buton durumu.
#[derive(Debug, Clone, Copy, Default)]
pub struct MouseButtons {
    pub left: bool,
    pub right: bool,
    pub middle: bool,
}

/// Birleşik input olayı — fare ve klavyeden gelen tüm olaylar.
#[derive(Debug, Clone)]
pub enum InputAction {
    /// Sol tık bırakıldı.
    LeftClick { x: i32, y: i32 },
    /// Sağ tık bırakıldı.
    RightClick { x: i32, y: i32 },
    /// Sol tuş basılı tutularak sürükleme.
    Drag { from_x: i32, from_y: i32, to_x: i32, to_y: i32 },
    /// Sürükleme bırakıldı.
    DragEnd { x: i32, y: i32 },
    /// Fare hareketi (basılı tuş yok).
    MouseMove { x: i32, y: i32 },
    /// Scroll tekerleği.
    Scroll { x: i32, y: i32, dx: i32, dy: i32 },
    /// Unicode karakter tuşu.
    KeyChar { c: char, mods: Modifiers },
    /// Raw tarama kodu (ok tuşları, F1-F12 vs).
    KeyRaw { scancode: u8, mods: Modifiers },
    /// Tanınan global kısayol.
    Shortcut(ShortcutId),
}

// ============================================================
// GLOBAL KLAVYE KISAYOLLARI
// ============================================================

/// Sistem genelinde geçerli klavye kısayolları.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutId {
    // --- Pencere Yönetimi ---
    SnapLeft,       // Super+←
    SnapRight,      // Super+→
    SnapMaximize,   // Super+↑
    SnapRestore,    // Super+↓
    SnapTopLeft,    // Super+Ctrl+←
    SnapTopRight,   // Super+Ctrl+→
    CloseWindow,    // Alt+F4
    MinimizeWindow, // Super+H
    MaximizeToggle, // Super+F
    CycleWindows,   // Alt+Tab / Super+Tab
    // --- Masaüstü ---
    ShowDesktop,    // Super+D
    LockScreen,     // Super+L
    // --- Workspace (Spaces) ---
    Workspace1,     // Super+1
    Workspace2,     // Super+2
    Workspace3,     // Super+3
    Workspace4,     // Super+4
    WorkspacePrev,  // Ctrl+Alt+←
    WorkspaceNext,  // Ctrl+Alt+→
    // --- Uygulamalar ---
    OpenTerminal,   // Ctrl+Alt+T
    OpenLauncher,   // Super (basıp bırak) / Super+A
    OpenSpotlight,  // Super+Space
    TakeScreenshot, // Super+Shift+S
    // --- Sistem ---
    CycleTheme,     // Ctrl+Alt+T gerekli değilse bu
}

/// Statik kısayol tablosu: `(mods_mask, raw_scancode, ShortcutId)`.
/// Modifier maskesi: SUPER=0x08, ALT=0x04, CTRL=0x02, SHIFT=0x01
pub static SHORTCUT_TABLE: &[(u8, u8, ShortcutId)] = &[
    // Snap
    (Modifiers::SUPER,                       0x4B, ShortcutId::SnapLeft),    // Super+←
    (Modifiers::SUPER,                       0x4D, ShortcutId::SnapRight),   // Super+→
    (Modifiers::SUPER,                       0x48, ShortcutId::SnapMaximize),// Super+↑
    (Modifiers::SUPER,                       0x50, ShortcutId::SnapRestore), // Super+↓
    (Modifiers::SUPER | Modifiers::CTRL,     0x4B, ShortcutId::SnapTopLeft), // Super+Ctrl+←
    (Modifiers::SUPER | Modifiers::CTRL,     0x4D, ShortcutId::SnapTopRight),// Super+Ctrl+→
    // Pencere
    (Modifiers::ALT,                         0x3B, ShortcutId::CloseWindow), // Alt+F4 (0x3B=F4)
    (Modifiers::SUPER,                       0x23, ShortcutId::MinimizeWindow), // Super+H
    (Modifiers::SUPER,                       0x21, ShortcutId::MaximizeToggle), // Super+F  
    (Modifiers::ALT,                         0x0F, ShortcutId::CycleWindows),// Alt+Tab (0x0F=Tab)
    (Modifiers::SUPER,                       0x0F, ShortcutId::CycleWindows),// Super+Tab
    // Desktop
    (Modifiers::SUPER,                       0x20, ShortcutId::ShowDesktop), // Super+D
    (Modifiers::SUPER,                       0x26, ShortcutId::LockScreen),  // Super+L
    // Workspace
    (Modifiers::SUPER,                       0x02, ShortcutId::Workspace1),  // Super+1
    (Modifiers::SUPER,                       0x03, ShortcutId::Workspace2),  // Super+2
    (Modifiers::SUPER,                       0x04, ShortcutId::Workspace3),  // Super+3
    (Modifiers::SUPER,                       0x05, ShortcutId::Workspace4),  // Super+4
    (Modifiers::CTRL | Modifiers::ALT,       0x4B, ShortcutId::WorkspacePrev),// Ctrl+Alt+←
    (Modifiers::CTRL | Modifiers::ALT,       0x4D, ShortcutId::WorkspaceNext),// Ctrl+Alt+→
    // Uygulamalar
    (Modifiers::CTRL | Modifiers::ALT,       0x14, ShortcutId::OpenTerminal),// Ctrl+Alt+T
    (Modifiers::SUPER,                       0x1E, ShortcutId::OpenLauncher),// Super+A (0x1E=A)
    (Modifiers::SUPER,                       0x39, ShortcutId::OpenSpotlight),//Super+Space
    (Modifiers::SUPER | Modifiers::SHIFT,    0x1F, ShortcutId::TakeScreenshot),//Super+Shift+S
];

// ============================================================
// INPUT ROUTER
// ============================================================

/// Modifier tuş durumu takipçisi.
#[derive(Debug, Clone, Default)]
pub struct ModifierState {
    pub mods: Modifiers,
    pub left_shift: bool,
    pub right_shift: bool,
    pub left_ctrl: bool,
    pub right_ctrl: bool,
    pub left_alt: bool,
    pub right_alt: bool,
    pub super_key: bool,
}

impl ModifierState {
    /// Tarama koduna göre modifier durumunu günceller.
    /// `pressed`: true=bastı, false=bıraktı.
    pub fn update(&mut self, scancode: u8, pressed: bool) {
        match scancode {
            0x2A => { self.left_shift  = pressed; }  // LShift
            0x36 => { self.right_shift = pressed; }  // RShift
            0x1D => { self.left_ctrl   = pressed; }  // LCtrl
            0x9D => { self.right_ctrl  = pressed; }  // RCtrl
            0x38 => { self.left_alt    = pressed; }  // LAlt
            0xB8 => { self.right_alt   = pressed; }  // RAlt
            0x5B | 0x5C => { self.super_key = pressed; } // LSuper/RSuper
            _ => {}
        }
        let mut m = 0u8;
        if self.left_shift || self.right_shift { m |= Modifiers::SHIFT; }
        if self.left_ctrl  || self.right_ctrl  { m |= Modifiers::CTRL;  }
        if self.left_alt   || self.right_alt   { m |= Modifiers::ALT;   }
        if self.super_key                      { m |= Modifiers::SUPER; }
        self.mods = Modifiers(m);
    }

    /// Verilen (mods, scancode) çifti için kısayol var mı?
    pub fn match_shortcut(&self, scancode: u8) -> Option<ShortcutId> {
        for &(required_mods, sc, id) in SHORTCUT_TABLE {
            if self.mods.0 == required_mods && sc == scancode {
                return Some(id);
            }
        }
        None
    }
}

// ============================================================
// SNAP OVERLAY (Hover gösterimi)
// ============================================================

/// Bir snap bölgesinin fare hover sırasında ekranda gösterilen yarı-saydam
/// hedef dikdörtgeni.
#[derive(Debug, Clone)]
pub struct SnapZone {
    pub target: SnapTarget,
    pub rect: Rect,
    pub hover: bool,
    pub hover_alpha: f32, // animasyonlu: 0.0→1.0
}

/// Tüm snap bölgelerini tutan yönetici.
pub struct SnapOverlay {
    pub zones: Vec<SnapZone>,
    pub visible: bool,
    pub screen_w: i32,
    pub screen_h: i32,
    pub panel_h: i32,
}

impl SnapOverlay {
    const EDGE_THRESHOLD: i32 = 20; // fare ekran kenarına bu kadar px yaklaşınca aktif

    pub fn new(screen_w: i32, screen_h: i32, panel_h: i32) -> Self {
        const TARGETS: &[SnapTarget] = &[
            SnapTarget::Left, SnapTarget::Right, SnapTarget::Maximize,
            SnapTarget::TopLeft, SnapTarget::TopRight, SnapTarget::BotLeft, SnapTarget::BotRight,
        ];
        let zones = TARGETS.iter().map(|&t| SnapZone {
            rect: t.compute_rect(screen_w, screen_h, panel_h),
            target: t,
            hover: false,
            hover_alpha: 0.0,
        }).collect();
        Self { zones, visible: false, screen_w, screen_h, panel_h }
    }

    /// Fare sürükleme sırasında aktif edilir; kenar yakınlığını kontrol eder.
    pub fn update(&mut self, mx: i32, my: i32, dt: f32) {
        let near_left   = mx < Self::EDGE_THRESHOLD;
        let near_right  = mx > self.screen_w - Self::EDGE_THRESHOLD;
        let near_top    = my < self.panel_h + Self::EDGE_THRESHOLD;

        for zone in &mut self.zones {
            let active = match zone.target {
                SnapTarget::Left    => near_left,
                SnapTarget::Right   => near_right,
                SnapTarget::Maximize | SnapTarget::Top => near_top,
                SnapTarget::TopLeft  => near_left && near_top,
                SnapTarget::TopRight => near_right && near_top,
                SnapTarget::BotLeft  => near_left && my > self.screen_h - Self::EDGE_THRESHOLD,
                SnapTarget::BotRight => near_right && my > self.screen_h - Self::EDGE_THRESHOLD,
            };
            zone.hover = active;
            let target_alpha = if active { 1.0f32 } else { 0.0f32 };
            let speed = 8.0;
            if zone.hover_alpha < target_alpha {
                zone.hover_alpha = (zone.hover_alpha + speed * dt).min(1.0);
            } else {
                zone.hover_alpha = (zone.hover_alpha - speed * dt).max(0.0);
            }
        }
    }

    /// Fareye en yakın aktif snap bölgesini döndürür.
    pub fn hovered_target(&self) -> Option<SnapTarget> {
        self.zones.iter()
            .filter(|z| z.hover)
            .max_by(|a, b| a.hover_alpha.partial_cmp(&b.hover_alpha).unwrap())
            .map(|z| z.target)
    }
}

// ============================================================
// ANİMASYON YARDIMCILARı
// ============================================================

/// Animasyon eğrileri (no_std uyumlu, libm kullanır).
pub fn ease_out_expo(t: f32) -> f32 {
    if t >= 1.0 { return 1.0; }
    1.0 - libm::powf(2.0, -10.0 * t)
}

pub fn ease_out_cubic(t: f32) -> f32 {
    let t1 = t - 1.0;
    1.0 + t1 * t1 * t1
}

pub fn ease_in_back(t: f32) -> f32 {
    const C1: f32 = 1.70158;
    const C3: f32 = C1 + 1.0;
    C3 * t * t * t - C1 * t * t
}

pub fn ease_out_back(t: f32) -> f32 {
    const C1: f32 = 1.70158;
    const C3: f32 = C1 + 1.0;
    let t1 = t - 1.0;
    1.0 + C3 * t1 * t1 * t1 + C1 * t1 * t1
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub fn lerp_rect(a: &Rect, b: &Rect, t: f32) -> Rect {
    Rect {
        x:      lerp(a.x as f32,      b.x as f32,      t) as i32,
        y:      lerp(a.y as f32,      b.y as f32,      t) as i32,
        width:  lerp(a.width as f32,  b.width as f32,  t) as i32,
        height: lerp(a.height as f32, b.height as f32, t) as i32,
    }
}

// ============================================================
// HYPER-SNAP (INTEGER, NO_STD)
// ============================================================

/// Q8 sabit nokta (0..255) progress'e çevirir.
#[inline]
pub fn t_to_q8(t: f32) -> u16 {
    let clamped = if t < 0.0 { 0.0 } else if t > 1.0 { 1.0 } else { t };
    (clamped * 255.0) as u16
}

/// Q8 → f32 (yalnızca WinState geriye yazımı için).
#[inline]
pub fn q8_to_t(q8: u16) -> f32 {
    q8 as f32 / 255.0
}

/// Hyper-Snap easing eğrisi: hızlı başlar, sert kilitlenir.
/// `e = 1 - (1 - t)^3` (tamamı integer).
#[inline]
pub fn hyper_ease_out_q8(t_q8: u16) -> u16 {
    let t = t_q8.min(255) as u32;
    let inv = 255u32 - t;
    let inv3 = inv * inv * inv;
    let eased = 255u32.saturating_sub((inv3 + 32767) / 65535);
    eased.min(255) as u16
}

/// Integer spring step (float yok): hedefe logaritmik yaklaşım.
#[inline]
pub fn hyper_spring_step_q8(cur_q8: u16, target_q8: u16) -> u16 {
    if cur_q8 == target_q8 {
        return cur_q8;
    }
    let cur = cur_q8 as i32;
    let target = target_q8 as i32;
    let diff = target - cur;
    let mut step = (diff >> 1) + (diff >> 3);
    if step == 0 {
        step = if diff > 0 { 1 } else { -1 };
    }
    (cur + step).clamp(0, 255) as u16
}

/// 150ms üst sınır için Q8 ilerleme güncellemesi (60fps ≈ 9 frame).
#[inline]
pub fn hyper_advance_q8(cur_q8: u16) -> u16 {
    let mut next = hyper_spring_step_q8(cur_q8, 255);
    // Çok yavaş kalırsa minimum adım zorla
    if next <= cur_q8 {
        next = (cur_q8 + 24).min(255);
    }
    next
}

/// Q8 sabit nokta ile rect interpolasyonu.
#[inline]
pub fn lerp_rect_q8(a: &Rect, b: &Rect, t_q8: u16) -> Rect {
    let t = t_q8 as i32;
    let inv = 255 - t;
    Rect {
        x: (a.x * inv + b.x * t) / 255,
        y: (a.y * inv + b.y * t) / 255,
        width: (a.width * inv + b.width * t) / 255,
        height: (a.height * inv + b.height * t) / 255,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HyperAnimPhase {
    Idle,
    Opening,
    Closing,
    Minimizing,
    Restoring,
    Snapping,
    Maximizing,
}

#[derive(Clone, Copy, Debug)]
pub struct HyperSnapStateMachine {
    pub phase: HyperAnimPhase,
    pub progress_q8: u16,
    pub target_q8: u16,
}

impl HyperSnapStateMachine {
    pub const fn new() -> Self {
        Self { phase: HyperAnimPhase::Idle, progress_q8: 0, target_q8: 255 }
    }

    pub fn begin(&mut self, phase: HyperAnimPhase) {
        self.phase = phase;
        self.progress_q8 = 0;
        self.target_q8 = 255;
    }

    pub fn tick(&mut self) -> bool {
        if self.phase == HyperAnimPhase::Idle {
            return true;
        }
        self.progress_q8 = hyper_advance_q8(self.progress_q8);
        if self.progress_q8 >= self.target_q8 {
            self.phase = HyperAnimPhase::Idle;
            true
        } else {
            false
        }
    }
}

/// Mevcut pencere durumuna göre görsel rect'i hesaplar (animasyon için).
/// `dock_rect`: minimize animasyonu için hedef küçük rect.
pub fn animated_rect(frame: &WindowFrame, dock_rect: Rect) -> Rect {
    match frame.state {
        WinState::Opening(t) => {
            let et = ease_out_expo(t);
            let cx = frame.rect.x + frame.rect.width / 2;
            let cy = frame.rect.y + frame.rect.height / 2;
            let w = (frame.rect.width as f32 * et) as i32;
            let h = (frame.rect.height as f32 * et) as i32;
            Rect::new(cx - w / 2, cy - h / 2, w, h)
        }
        WinState::Closing(t) | WinState::Minimizing(t) => {
            let et = ease_in_back(t);
            lerp_rect(&frame.rect, &dock_rect, et)
        }
        WinState::Restoring(t) => {
            let et = ease_out_back(t);
            lerp_rect(&dock_rect, &frame.normal_rect, et)
        }
        WinState::Snapping { target: _, t } => {
            // target rect hesaplaması dışarıda yapılır, burada yalnızca t var
            frame.rect // placeholder; compositor override eder
        }
        WinState::Maximizing(t) => {
            frame.rect // compositor override eder
        }
        _ => frame.rect,
    }
}
