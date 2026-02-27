//! # Giriş Ekranı
//!
//! macOS tarzı kullanıcı seçimi özellikli giriş ekranı.
//! Kullanıcı avatarları, şifre alanı ve kilit ekranı içerir.
//!
//! ## Mimari
//! - `UserAccount`: Kullanıcı verisi; isim, avatar türü, yönetici bayrağı
//! - `LoginScreen`: Durum makinesi; Boot → UserSelection → PasswordEntry → LoggedIn
//! - `LoginState`: Boot, UserSelection, PasswordEntry, Authenticating, AuthFailed, LoggedIn, Locked
//!
//! ## Animasyon Algoritmaları
//! - **Boot ekranı**: Matrix dijital yağmur efekti; progress * 100 damla, sahte-rastgele konumlar
//! - **Glitch logosu**: boot_progress * 50/70 değerlerine göre x/y ofseti kayması
//! - **Arka plan**: 50 parçacık; `sinf/cosf` ile dairesel hareket + alpha karıştırma
//! - **Dairesel avatar**: Merkez orijinli yarıçap testi; hover halinde `sqrtf` ile halka çizimi
//! - **Yükleyici (spinner)**: 8 nokta + `cosf/sinf`; her nokta için `alpha = 1 - i/8` kararması

use alloc::boxed::Box;
use alloc::string::String;
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;
use libm::{sinf, cosf, sqrtf};

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};

// ============================================================================
// GİRİŞ SABİTLERİ
// ============================================================================

/// Kullanıcı avatar boyutu (piksel)
pub const AVATAR_SIZE: usize = 96;

/// Avatar arası boşluk (piksel)
pub const AVATAR_SPACING: usize = 120;

/// Şifre alanı genişliği (piksel)
pub const PASSWORD_WIDTH: usize = 280;

/// Şifre alanı yüksekliği (piksel)
pub const PASSWORD_HEIGHT: usize = 36;

// ============================================================================
// KULLANICI HESABI
// ============================================================================

/// Kullanıcı hesap bilgisi
#[derive(Clone, Debug)]
pub struct UserAccount {
    /// Kullanıcı kimliği
    pub id: u32,
    /// Kullanıcı adı
    pub username: String,
    /// Görüntü adı
    pub display_name: String,
    /// Avatar türü
    pub avatar: AvatarType,
    /// Yönetici mi
    pub is_admin: bool,
    /// Giriş yapılmış mı
    pub logged_in: bool,
    /// Son giriş zamanı
    pub last_login: u64,
    /// Şifre ipucu
    pub password_hint: String,
}

#[derive(Clone, Debug)]
pub enum AvatarType {
    Initials(String),
    Image(String),
    Icon(AvatarIcon),
    Color(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AvatarIcon {
    User,
    Admin,
    Guest,
    Custom(u16),
}

impl UserAccount {
    pub fn new(id: u32, username: &str, display_name: &str) -> Self {
        let initials = display_name.split_whitespace()
            .take(2)
            .filter_map(|w| w.chars().next())
            .collect::<String>();

        UserAccount {
            id,
            username: String::from(username),
            display_name: String::from(display_name),
            avatar: AvatarType::Initials(initials),
            is_admin: false,
            logged_in: false,
            last_login: 0,
            password_hint: String::new(),
        }
    }

    pub fn admin(id: u32, username: &str, display_name: &str) -> Self {
        let mut user = Self::new(id, username, display_name);
        user.is_admin = true;
        user.avatar = AvatarType::Icon(AvatarIcon::Admin);
        user
    }

    pub fn guest() -> Self {
        UserAccount {
            id: 0,
            username: String::from("guest"),
            display_name: String::from("Guest"),
            avatar: AvatarType::Icon(AvatarIcon::Guest),
            is_admin: false,
            logged_in: false,
            last_login: 0,
            password_hint: String::new(),
        }
    }
}

// ============================================================================
// GİRİŞ EKRANI DURUMU
// ============================================================================

/// Giriş ekranı durumu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginState {
    /// Kullanıcı seçimi gösteriliyor
    UserSelection,
    /// Seçili kullanıcı için şifre girişi
    PasswordEntry,
    /// Kimlik doğrulanıyor
    Authenticating,
    /// Kimlik doğrulama başarısız
    AuthFailed,
    /// Başarıyla giriş yapıldı
    LoggedIn,
    /// Kilit ekranı
    Locked,
    /// Açılış animasyonu
    Boot,
}

// ============================================================================
// GİRİŞ EKRANI
// ============================================================================

/// Giriş ekranı
pub struct LoginScreen {
    /// Geçerli durum
    pub state: LoginState,
    /// Kullanılabilir kullanıcılar
    pub users: Vec<UserAccount>,
    /// Seçili kullanıcı indeksi
    pub selected_user: Option<usize>,
    /// Şifre girişi
    pub password: String,
    /// İmleç görünür mü
    pub cursor_visible: bool,
    /// İmleç yanıp sönme zamanlayıcısı
    pub cursor_timer: f32,
    /// Animasyon ilerlemesi
    pub animation_progress: f32,
    /// Ekran genişliği
    pub screen_width: usize,
    /// Ekran yüksekliği
    pub screen_height: usize,
    /// Üzerine gelinen kullanıcı indeksi
    pub hovered_user: Option<usize>,
    /// Hata mesajı
    pub error_message: String,
    /// Hata zamanlayıcısı
    pub error_timer: f32,
    /// Açılış ilerlemesi (0.0 - 1.0)
    pub boot_progress: f32,
    /// Kapatma seçenekleri göster
    pub show_shutdown_menu: bool,
    /// Saat dizisi
    pub time_string: String,
    /// Tarih dizisi
    pub date_string: String,
    /// Arka plan bulanıklığı
    pub blur_amount: f32,
}

impl LoginScreen {
    pub fn new(screen_width: usize, screen_height: usize) -> Self {
        let mut login = LoginScreen {
            state: LoginState::Boot,
            users: Vec::new(),
            selected_user: None,
            password: String::new(),
            cursor_visible: true,
            cursor_timer: 0.0,
            animation_progress: 0.0,
            screen_width,
            screen_height,
            hovered_user: None,
            error_message: String::new(),
            error_timer: 0.0,
            boot_progress: 0.0,
            show_shutdown_menu: false,
            time_string: String::from("12:00"),
            date_string: String::from("Monday, January 1"),
            blur_amount: 0.0,
        };

        login.add_default_users();
        login
    }

    fn add_default_users(&mut self) {
        self.users.push(UserAccount::admin(1, "admin", "Administrator"));
        self.users.push(UserAccount::new(2, "user", "User"));
        self.users.push(UserAccount::guest());
    }

    /// Giriş ekranını göster
    pub fn show(&mut self) {
        self.state = LoginState::Boot;
        self.boot_progress = 0.0;
        self.animation_progress = 0.0;
        self.selected_user = None;
        self.password.clear();
        self.error_message.clear();
        self.show_shutdown_menu = false;
    }

    /// Ekranı kilitle
    pub fn lock(&mut self) {
        self.state = LoginState::Locked;
        self.selected_user = None;
        self.password.clear();
        self.animation_progress = 0.0;
    }

    /// Kullanıcı seç
    pub fn select_user(&mut self, index: usize) {
        if index >= self.users.len() {
            return;
        }

        self.selected_user = Some(index);
        self.state = LoginState::PasswordEntry;
        self.password.clear();
        self.animation_progress = 0.0;
        self.error_message.clear();
    }

    /// Kullanıcı seçimine geri dön
    pub fn go_back(&mut self) {
        self.selected_user = None;
        self.password.clear();
        self.state = LoginState::UserSelection;
        self.animation_progress = 0.0;
        self.error_message.clear();
    }

    /// Şifreyi gönder
    pub fn submit_password(&mut self) -> LoginEvent {
        if self.password.is_empty() {
            return LoginEvent::None;
        }

        self.state = LoginState::Authenticating;

        // Kimlik doğrulamayı simüle et (depolanmış hash ile doğrulanacak)
        // Demo için: misafir her şifreyi kabul eder, diğerleri "password" veya "echos" kabul eder
        if let Some(idx) = self.selected_user {
            let (user_id, username) = {
                let user = &self.users[idx];
                (user.id, user.username.clone())
            };

            if username == "guest" || self.password == "password" || self.password == "echos" {
                // Başarılı
                self.state = LoginState::LoggedIn;
                self.users[idx].logged_in = true;
                return LoginEvent::LoginSuccess(user_id, username);
            }
        }

        // Başarısız
        self.state = LoginState::AuthFailed;
        self.error_message = String::from("Incorrect password. Try again.");
        self.error_timer = 3.0;
        self.password.clear();

        LoginEvent::LoginFailed
    }

    /// Animasyonu güncelle
    pub fn update(&mut self, dt: f32) -> LoginEvent {
        // İmleç yanıp sönmesini güncelle
        self.cursor_timer += dt;
        if self.cursor_timer >= 0.5 {
            self.cursor_timer = 0.0;
            self.cursor_visible = !self.cursor_visible;
        }

        // Animasyonu güncelle
        if self.animation_progress < 1.0 {
            self.animation_progress = (self.animation_progress + dt * 3.0).min(1.0);
        }

        // Açılış ilerlemesini güncelle
        if self.state == LoginState::Boot {
            self.boot_progress += dt * 0.5;
            if self.boot_progress >= 1.0 {
                self.state = LoginState::UserSelection;
                return LoginEvent::BootComplete;
            }
        }

        // Hata zamanlayıcısını güncelle
        if self.error_timer > 0.0 {
            self.error_timer -= dt;
            if self.error_timer <= 0.0 {
                self.error_message.clear();
                if self.state == LoginState::AuthFailed {
                    self.state = LoginState::PasswordEntry;
                }
            }
        }

        LoginEvent::None
    }

    /// Giriş ekranını çiz
    pub fn draw(&self, fb: &mut Framebuffer) {
        // Arka planı çiz
        self.draw_background(fb);

        match self.state {
            LoginState::Boot => {
                self.draw_boot_screen(fb);
            }
            LoginState::UserSelection | LoginState::Locked => {
                self.draw_user_selection(fb);
            }
            LoginState::PasswordEntry | LoginState::Authenticating | LoginState::AuthFailed => {
                self.draw_password_screen(fb);
            }
            LoginState::LoggedIn => {
                // Soluklaşarak kapat
            }
        }

        // Kapatma menüsünü çiz
        if self.show_shutdown_menu {
            self.draw_shutdown_menu(fb);
        }
    }

    fn draw_background(&self, fb: &mut Framebuffer) {
        // Degrade arka plan
        let top_color = 0x1E1E2E;
        let bottom_color = 0x0F0F1F;

        for y in 0..self.screen_height {
            let t = y as f32 / self.screen_height as f32;
            let color = Self::lerp_color(top_color, bottom_color, t);

            for x in 0..self.screen_width {
                fb.plot_pixel(x, y, color);
            }
        }

        // Hafif animasyonlu efekt ekle
        let time = self.animation_progress;
        for i in 0..50 {
            let x = (self.screen_width as f32 * 0.3 + sinf(i as f32 * 37.0 + time * 20.0) * 100.0) as usize;
            let y = (self.screen_height as f32 * 0.3 + cosf(i as f32 * 23.0 + time * 15.0) * 100.0) as usize;

            if x < self.screen_width && y < self.screen_height {
                let alpha = 0.05 + 0.03 * sinf(i as f32 * 0.1 + time);
                let ptr = unsafe { (fb.base_addr as *mut u32).add(y * fb.pixels_per_scan_line + x) };
                let bg = unsafe { *ptr };
                unsafe { *ptr = Self::blend_color(bg, 0x4080FF, alpha); }
            }
        }
    }

    fn draw_boot_screen(&self, fb: &mut Framebuffer) {
        let center_x = self.screen_width / 2;
        let center_y = self.screen_height / 2 - 50;

        // Açılış ilerlemesine göre matrix dijital yağmur arka planı efekti
        let max_drops = (self.boot_progress * 100.0) as usize;
        for i in 0..max_drops {
            let drop_x = (i * 73) % self.screen_width;
            let drop_y = ((self.boot_progress * 1000.0) as usize + i * 41) % self.screen_height;
            if drop_y > 20 {
                fb.draw_string(drop_x, drop_y, &format!("{}", (i % 10)), 0x00FF00); // Hacker yeşili
            }
        }

        // Logo üzerinde zamana/ilerlemeye göre glitch efekti
        let glitch_offset_x = if (self.boot_progress * 50.0) as u32 % 5 == 0 { 2 } else { 0 };
        let glitch_offset_y = if (self.boot_progress * 70.0) as u32 % 7 == 0 { -1 } else { 0 };

        // echOS logosunu glitch ile çiz
        if glitch_offset_x > 0 {
            fb.draw_string(center_x - 40 + glitch_offset_x as usize, center_y - 20, "echOS", 0xFF0000); // Kırmızı glitch
            fb.draw_string(center_x - 40 - glitch_offset_x as usize, center_y - 20, "echOS", 0x0000FF); // Mavi glitch
        }
        fb.draw_string(center_x - 40, (center_y as i32 - 20 + glitch_offset_y) as usize, "echOS", 0xFFFFFFFF);

        // İlerleme çubuğu
        let bar_width = 200;
        let bar_height = 4;
        let bar_x = center_x - bar_width / 2;
        let bar_y = center_y + 40;

        fb.draw_rect(bar_x, bar_y, bar_width, bar_height, 0x404040);
        fb.draw_rect(bar_x, bar_y, (bar_width as f32 * self.boot_progress) as usize, bar_height, Theme::ACCENT_PRIMARY.to_u32());

        // İlerleme metni
        let progress_text = format!("Loading... {}%", (self.boot_progress * 100.0) as u32);
        fb.draw_string(center_x - 50, bar_y + 16, &progress_text, 0x808080);
    }

    fn draw_user_selection(&self, fb: &mut Framebuffer) {
        // Saat görüntüsü
        let time_y = self.screen_height / 4;
        fb.draw_string(self.screen_width / 2 - 40, time_y, &self.time_string, 0xFFFFFFFF);
        fb.draw_string(self.screen_width / 2 - 80, time_y + 28, &self.date_string, 0x808080);

        // Kullanıcı avatarları
        let users_y = self.screen_height / 2 + 20;
        let total_width = self.users.len() * AVATAR_SPACING;
        let start_x = (self.screen_width - total_width) / 2 + AVATAR_SPACING / 2;

        for (i, user) in self.users.iter().enumerate() {
            let x = start_x + i * AVATAR_SPACING;
            let is_hovered = self.hovered_user == Some(i);
            let scale = if is_hovered { 1.1 } else { 1.0 };

            self.draw_user_avatar(fb, x, users_y, user, scale, is_hovered);
        }

        // Kapatma düğmesi
        fb.draw_string(self.screen_width / 2 - 60, self.screen_height - 60, "⏻ Shut Down", 0x808080);
    }

    fn draw_user_avatar(&self, fb: &mut Framebuffer, x: usize, y: usize, user: &UserAccount, scale: f32, hovered: bool) {
        let size = (AVATAR_SIZE as f32 * scale) as usize;
        let offset = (size - AVATAR_SIZE) / 2;
        let draw_x = x - offset;
        let draw_y = y - offset;

        // Gölge
        for sy in 0..8 {
            let shadow_y = draw_y + size + sy;
            let shadow_alpha = 0.2 - sy as f32 * 0.025;

            for sx in 0..size {
                let screen_x = draw_x + sx;
                if screen_x < self.screen_width && shadow_y < self.screen_height {
                    let ptr = unsafe {
                        (fb.base_addr as *mut u32).add(shadow_y * fb.pixels_per_scan_line + screen_x)
                    };
                    let bg = unsafe { *ptr };
                    unsafe { *ptr = Self::blend_color(bg, 0x000000, shadow_alpha); }
                }
            }
        }

        // Avatar arka planı
        let bg_color = match &user.avatar {
            AvatarType::Color(color) => *color,
            AvatarType::Icon(AvatarIcon::Admin) => 0xFF6B35,
            AvatarType::Icon(AvatarIcon::Guest) => 0x808080,
            _ => Theme::ACCENT_PRIMARY.to_u32(),
        };

        // Dairesel avatar çiz
        let radius = size / 2;
        for py in 0..size {
            for px in 0..size {
                let dx = px as i32 - radius as i32;
                let dy = py as i32 - radius as i32;
                if dx * dx + dy * dy <= (radius * radius) as i32 {
                    let screen_x = draw_x + px;
                    let screen_y = draw_y + py;
                    if screen_x < self.screen_width && screen_y < self.screen_height {
                        fb.plot_pixel(screen_x, screen_y, bg_color);
                    }
                }
            }
        }

        // Avatar içeriğini çiz
        match &user.avatar {
            AvatarType::Initials(initials) => {
                let text_x = draw_x + radius - initials.len() * 4;
                let text_y = draw_y + radius - 8;
                fb.draw_string(text_x, text_y, initials, 0xFFFFFFFF);
            }
            AvatarType::Icon(icon) => {
                let icon_str = match icon {
                    AvatarIcon::User => "👤",
                    AvatarIcon::Admin => "👑",
                    AvatarIcon::Guest => "👤",
                    AvatarIcon::Custom(_) => "👤",
                };
                fb.draw_string(draw_x + radius - 8, draw_y + radius - 8, icon_str, 0xFFFFFFFF);
            }
            AvatarType::Image(_) => {
                // Görüntü çizilecek
                fb.draw_string(draw_x + radius - 8, draw_y + radius - 8, "👤", 0xFFFFFFFF);
            }
            AvatarType::Color(_) => {
                fb.draw_string(draw_x + radius - 8, draw_y + radius - 8, "👤", 0xFFFFFFFF);
            }
        }

        // Kullanıcı adı
        let name_y = draw_y + size + 12;
        let text_color = if hovered { 0xFFFFFFFF } else { 0xC0C0C0 };
        fb.draw_string(x - user.display_name.len() * 4, name_y, &user.display_name, text_color);

        // Hover vurgusu
        if hovered {
            // Avatarın etrafına halka çiz
            for py in 0..size + 4 {
                for px in 0..size + 4 {
                    let dx = px as i32 - (radius + 2) as i32;
                    let dy = py as i32 - (radius + 2) as i32;
                    let dist = sqrtf((dx * dx + dy * dy) as f32);
                    if dist >= radius as f32 && dist <= (radius + 2) as f32 {
                        let screen_x = draw_x - 2 + px;
                        let screen_y = draw_y - 2 + py;
                        if screen_x < self.screen_width && screen_y < self.screen_height {
                            fb.plot_pixel(screen_x, screen_y, Theme::ACCENT_PRIMARY.to_u32());
                        }
                    }
                }
            }
        }
    }

    fn draw_password_screen(&self, fb: &mut Framebuffer) {
        if let Some(idx) = self.selected_user {
            let user = &self.users[idx];

            // Kullanıcı avatarı (daha küçük)
            let avatar_x = self.screen_width / 2;
            let avatar_y = self.screen_height / 2 - 80;

            self.draw_user_avatar(fb, avatar_x, avatar_y, user, 0.8, false);

            // Kullanıcı adı
            fb.draw_string(avatar_x - user.display_name.len() * 4, avatar_y + AVATAR_SIZE as f32 as usize * 8 / 10 + 16,
                          &user.display_name, 0xFFFFFFFF);

            // Şifre alanı
            let field_x = self.screen_width / 2 - PASSWORD_WIDTH / 2;
            let field_y = avatar_y + AVATAR_SIZE + 60;

            // Arka plan
            fb.draw_rect(field_x, field_y, PASSWORD_WIDTH, PASSWORD_HEIGHT, 0x202020);
            fb.draw_rect_outline(field_x, field_y, PASSWORD_WIDTH, PASSWORD_HEIGHT, 0x404040);

            // Şifre noktaları
            let dot_count = self.password.len().min(20);
            let dot_spacing = 12;
            let dots_width = dot_count * dot_spacing;
            let dots_start = field_x + PASSWORD_WIDTH / 2 - dots_width / 2;

            for i in 0..dot_count {
                let dot_x = dots_start + i * dot_spacing;
                let dot_y = field_y + PASSWORD_HEIGHT / 2 - 3;
                fb.draw_rect(dot_x, dot_y, 6, 6, 0xC0C0C0);
            }

            // İmleç
            if self.cursor_visible && self.state != LoginState::Authenticating {
                let cursor_x = dots_start + dot_count * dot_spacing + 4;
                fb.draw_rect(cursor_x, field_y + 8, 2, PASSWORD_HEIGHT - 16, 0xFFFFFFFF);
            }

            // İpucu
            if !user.password_hint.is_empty() && self.state != LoginState::Authenticating {
                fb.draw_string(field_x, field_y + PASSWORD_HEIGHT + 8,
                              &format!("Hint: {}", user.password_hint), 0x808080);
            }

            // Hata mesajı
            if !self.error_message.is_empty() {
                fb.draw_string(self.screen_width / 2 - self.error_message.len() * 4,
                              field_y + PASSWORD_HEIGHT + 32, &self.error_message, Theme::ERROR.to_u32());
            }

            // Kimlik doğrulama yükleyicisi
            if self.state == LoginState::Authenticating {
                let spinner_x = self.screen_width / 2;
                let spinner_y = field_y + PASSWORD_HEIGHT + 40;
                self.draw_spinner(fb, spinner_x, spinner_y);
            }

            // Geri düğmesi
            fb.draw_string(field_x - 60, field_y + 8, "← Back", 0x808080);

            // Gönderme ipucu
            fb.draw_string(field_x + PASSWORD_WIDTH + 16, field_y + 8, "↵ Enter", 0x808080);
        }
    }

    fn draw_spinner(&self, fb: &mut Framebuffer, x: usize, y: usize) {
        let radius = 12;
        let angle = self.animation_progress * core::f32::consts::PI * 2.0;

        for i in 0..8 {
            let a = angle + i as f32 * core::f32::consts::PI / 4.0;
            let px = x as i32 + (cosf(a) * radius as f32) as i32;
            let py = y as i32 + (sinf(a) * radius as f32) as i32;

            let alpha = 1.0 - i as f32 / 8.0;
            let color = Self::alpha_color(0xFFFFFFFF, alpha);

            if px >= 0 && py >= 0 && (px as usize) < self.screen_width && (py as usize) < self.screen_height {
                fb.plot_pixel(px as usize, py as usize, color);
            }
        }
    }

    fn draw_shutdown_menu(&self, fb: &mut Framebuffer) {
        let menu_x = self.screen_width / 2 - 100;
        let menu_y = self.screen_height - 140;
        let menu_width = 200;
        let menu_height = 100;

        // Arka plan
        fb.draw_rect(menu_x, menu_y, menu_width, menu_height, 0xE0202020);

        // Seçenekler
        let options = [("⏻ Shut Down", "shutdown"), ("🔄 Restart", "restart"), ("🔒 Lock", "lock")];

        for (i, (label, _)) in options.iter().enumerate() {
            let item_y = menu_y + 8 + i * 28;
            fb.draw_rect(menu_x + 4, item_y, menu_width - 8, 24, Theme::SIDEBAR_BG.to_u32());
            fb.draw_string(menu_x + 16, item_y + 4, label, Theme::TEXT_PRIMARY.to_u32());
        }
    }

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

    fn alpha_color(color: u32, alpha: f32) -> u32 {
        let r = (((color >> 16) & 0xFF) as f32 * alpha) as u32;
        let g = (((color >> 8) & 0xFF) as f32 * alpha) as u32;
        let b = ((color & 0xFF) as f32 * alpha) as u32;
        (r << 16) | (g << 8) | b
    }

    /// Tıklama olayını işle
    pub fn on_click(&mut self, mx: i32, my: i32) -> LoginEvent {
        match self.state {
            LoginState::UserSelection | LoginState::Locked => {
                // Kullanıcı avatarlarını kontrol et
                let users_y = self.screen_height / 2 + 20;
                let total_width = self.users.len() * AVATAR_SPACING;
                let start_x = (self.screen_width - total_width) / 2 + AVATAR_SPACING / 2;

                for (i, _) in self.users.iter().enumerate() {
                    let x = start_x + i * AVATAR_SPACING;

                    if mx >= (x - AVATAR_SIZE / 2) as i32 && mx < (x + AVATAR_SIZE / 2) as i32
                        && my >= (users_y - AVATAR_SIZE / 2) as i32 && my < (users_y + AVATAR_SIZE / 2) as i32 {
                        self.select_user(i);
                        return LoginEvent::None;
                    }
                }

                // Kapatma düğmesini kontrol et
                if mx >= (self.screen_width / 2 - 60) as i32 && mx < (self.screen_width / 2 + 60) as i32
                    && my >= (self.screen_height - 60) as i32 && my < (self.screen_height - 40) as i32 {
                    self.show_shutdown_menu = !self.show_shutdown_menu;
                }

                // Kapatma menüsü öğelerini kontrol et
                if self.show_shutdown_menu {
                    let menu_x = self.screen_width / 2 - 100;
                    let menu_y = self.screen_height - 140;

                    if mx >= menu_x as i32 && mx < (menu_x + 200) as i32
                        && my >= (menu_y + 8) as i32 && my < (menu_y + 92) as i32 {
                        let idx = ((my - menu_y as i32 - 8) / 28) as usize;

                        match idx {
                            0 => return LoginEvent::Shutdown,
                            1 => return LoginEvent::Restart,
                            2 => {
                                self.lock();
                                self.show_shutdown_menu = false;
                            }
                            _ => {}
                        }
                    }

                    self.show_shutdown_menu = false;
                }
            }
            LoginState::PasswordEntry => {
                // Geri düğmesini kontrol et
                let field_x = self.screen_width / 2 - PASSWORD_WIDTH / 2;
                let field_y = self.screen_height / 2 + AVATAR_SIZE + 60 - 80 + AVATAR_SIZE + 60;

                if mx >= (field_x - 60) as i32 && mx < field_x as i32
                    && my >= field_y as i32 && my < (field_y + PASSWORD_HEIGHT) as i32 {
                    self.go_back();
                }
            }
            _ => {}
        }

        LoginEvent::None
    }

    /// Tuş basışını işle
    pub fn on_key_press(&mut self, c: char) -> LoginEvent {
        match self.state {
            LoginState::PasswordEntry => {
                if c == '\x1b' { // Escape
                    self.go_back();
                } else if c == '\n' || c == '\r' { // Enter
                    return self.submit_password();
                } else if c == '\x08' { // Geri silme
                    self.password.pop();
                } else if !c.is_control() && self.password.len() < 32 {
                    self.password.push(c);
                }
            }
            LoginState::UserSelection | LoginState::Locked => {
                // Herhangi bir tuş ilk kullanıcı için şifre girişini başlatır
                if !c.is_control() && !self.users.is_empty() {
                    self.select_user(0);
                }
            }
            _ => {}
        }

        LoginEvent::None
    }

    /// Mouse hareketini işle
    pub fn on_mouse_move(&mut self, mx: i32, my: i32) {
        if self.state == LoginState::UserSelection || self.state == LoginState::Locked {
            let users_y = self.screen_height / 2 + 20;
            let total_width = self.users.len() * AVATAR_SPACING;
            let start_x = (self.screen_width - total_width) / 2 + AVATAR_SPACING / 2;

            self.hovered_user = None;

            for (i, _) in self.users.iter().enumerate() {
                let x = start_x + i * AVATAR_SPACING;

                if mx >= (x - AVATAR_SIZE / 2) as i32 && mx < (x + AVATAR_SIZE / 2) as i32
                    && my >= (users_y - AVATAR_SIZE / 2) as i32 && my < (users_y + AVATAR_SIZE / 2) as i32 {
                    self.hovered_user = Some(i);
                    break;
                }
            }
        }
    }

    /// Yeniden boyutlandır
    pub fn resize(&mut self, width: usize, height: usize) {
        self.screen_width = width;
        self.screen_height = height;
    }

    /// Saati ayarla
    pub fn set_time(&mut self, time: &str, date: &str) {
        self.time_string = String::from(time);
        self.date_string = String::from(date);
    }
}

/// Giriş olayları
#[derive(Clone, Debug)]
pub enum LoginEvent {
    None,
    BootComplete,
    LoginSuccess(u32, String),
    LoginFailed,
    Shutdown,
    Restart,
    Lock,
}

// ============================================================================
// GLOBAL GİRİŞ EKRANI
// ============================================================================

lazy_static::lazy_static! {
    static ref LOGIN: Mutex<LoginScreen> = Mutex::new(LoginScreen::new(1920, 1080));
}

/// Giriş ekranını başlat
pub fn init(width: usize, height: usize) {
    let mut login = LOGIN.lock();
    login.resize(width, height);
    crate::serial_println!("[GUI] Login screen initialized");
}

/// Giriş ekranına eriş
pub fn get_login() -> &'static Mutex<LoginScreen> {
    &LOGIN
}
