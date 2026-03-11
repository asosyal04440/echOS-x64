//! Hybrid Titan login and lock screen.

use alloc::string::String;
use alloc::vec::Vec;
use libm::sinf;
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::protocol::Rect;
use crate::gui::shell;
use crate::gui::theme::{Theme, ThemeMode};

pub const AVATAR_SIZE: usize = 96;
pub const AVATAR_SPACING: usize = 120;
pub const PASSWORD_WIDTH: usize = 280;
pub const PASSWORD_HEIGHT: usize = 36;

#[derive(Clone, Debug)]
pub struct UserAccount {
    pub id: u32,
    pub username: String,
    pub display_name: String,
    pub avatar: AvatarType,
    pub is_admin: bool,
    pub logged_in: bool,
    pub last_login: u64,
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
        let initials = display_name
            .split_whitespace()
            .take(2)
            .filter_map(|word| word.chars().next())
            .collect::<String>();
        Self {
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
        Self {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoginState {
    UserSelection,
    PasswordEntry,
    Authenticating,
    AuthFailed,
    LoggedIn,
    Locked,
    Boot,
}

pub struct LoginScreen {
    pub state: LoginState,
    pub users: Vec<UserAccount>,
    pub selected_user: Option<usize>,
    pub password: String,
    pub cursor_visible: bool,
    pub cursor_timer: f32,
    pub animation_progress: f32,
    pub screen_width: usize,
    pub screen_height: usize,
    pub hovered_user: Option<usize>,
    pub error_message: String,
    pub error_timer: f32,
    pub boot_progress: f32,
    pub show_shutdown_menu: bool,
    pub time_string: String,
    pub date_string: String,
    pub blur_amount: f32,
}

impl LoginScreen {
    pub fn new(screen_width: usize, screen_height: usize) -> Self {
        let mut login = Self {
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
        login.users.push(UserAccount::admin(1, "admin", "Administrator"));
        login.users.push(UserAccount::new(2, "user", "User"));
        login.users.push(UserAccount::guest());
        login
    }

    pub fn show(&mut self) {
        self.state = LoginState::Boot;
        self.boot_progress = 0.0;
        self.animation_progress = 0.0;
        self.selected_user = None;
        self.password.clear();
        self.error_message.clear();
        self.show_shutdown_menu = false;
    }

    pub fn lock(&mut self) {
        self.state = LoginState::Locked;
        self.selected_user = None;
        self.password.clear();
        self.animation_progress = 0.0;
    }

    pub fn select_user(&mut self, index: usize) {
        if index < self.users.len() {
            self.selected_user = Some(index);
            self.state = LoginState::PasswordEntry;
            self.password.clear();
            self.error_message.clear();
            self.error_timer = 0.0;
        }
    }

    pub fn go_back(&mut self) {
        self.selected_user = None;
        self.password.clear();
        self.state = LoginState::UserSelection;
        self.error_message.clear();
    }

    pub fn submit_password(&mut self) -> LoginEvent {
        if self.password.is_empty() {
            return LoginEvent::None;
        }

        self.state = LoginState::Authenticating;
        if let Some(index) = self.selected_user {
            let username = self.users[index].username.clone();
            let user_id = self.users[index].id;
            if username == "guest" || self.password == "password" || self.password == "echos" {
                self.users[index].logged_in = true;
                self.state = LoginState::LoggedIn;
                return LoginEvent::LoginSuccess(user_id, username);
            }
        }

        self.state = LoginState::AuthFailed;
        self.error_message = String::from("Incorrect password");
        self.error_timer = 2.0;
        self.password.clear();
        LoginEvent::LoginFailed
    }

    pub fn update(&mut self, dt: f32) -> LoginEvent {
        self.cursor_timer += dt;
        if self.cursor_timer >= 0.5 {
            self.cursor_timer = 0.0;
            self.cursor_visible = !self.cursor_visible;
        }

        self.animation_progress = (self.animation_progress + dt * 1.6).min(1.0);
        self.blur_amount = (self.blur_amount + dt * 0.7).min(1.0);

        if self.state == LoginState::Boot {
            self.boot_progress = (self.boot_progress + dt * 0.5).min(1.0);
            if self.boot_progress >= 1.0 {
                self.state = LoginState::UserSelection;
                return LoginEvent::BootComplete;
            }
        }

        if self.error_timer > 0.0 {
            self.error_timer -= dt;
            if self.error_timer <= 0.0 && self.state == LoginState::AuthFailed {
                self.error_message.clear();
                self.state = LoginState::PasswordEntry;
            }
        }

        LoginEvent::None
    }

    pub fn draw(&self, fb: &mut Framebuffer) {
        let screen = Rect::new(0, 0, self.screen_width as u32, self.screen_height as u32);
        shell::draw_wallpaper_backdrop(fb, screen, screen, ThemeMode::Dark);
        shell::fill_blended_rect(
            fb,
            screen,
            screen,
            Theme::tokens(ThemeMode::Dark).surfaces.overlay,
            if self.state == LoginState::Locked { 0x88 } else { 0x70 },
        );

        match self.state {
            LoginState::Boot => self.draw_boot_screen(fb),
            LoginState::UserSelection | LoginState::Locked => self.draw_user_selection(fb),
            LoginState::PasswordEntry | LoginState::Authenticating | LoginState::AuthFailed => {
                self.draw_password_screen(fb)
            }
            LoginState::LoggedIn => {}
        }

        if self.show_shutdown_menu {
            self.draw_shutdown_menu(fb);
        }
    }

    fn draw_boot_screen(&self, fb: &mut Framebuffer) {
        let screen = Rect::new(0, 0, self.screen_width as u32, self.screen_height as u32);
        let center_x = self.screen_width as i32 / 2;
        let panel = Rect::new(center_x - 180, self.screen_height as i32 / 2 - 110, 360, 180);
        let beam = Rect::new(panel.x + 40, panel.y + 92, 280, 6);
        shell::fill_blended_rect(
            fb,
            panel,
            screen,
            Theme::tokens(ThemeMode::Dark).surfaces.overlay,
            0xB0,
        );
        shell::draw_rect_outline_clipped(fb, panel, screen, Theme::BORDER.to_u32());
        shell::draw_emblem_wordmark(fb, center_x, panel.y + 24, ThemeMode::Dark, true);
        shell::fill_blended_rect(
            fb,
            beam,
            screen,
            Theme::tokens(ThemeMode::Dark).surfaces.field,
            0xFF,
        );
        let fill = Rect::new(
            beam.x,
            beam.y,
            ((beam.width as f32) * self.boot_progress) as u32,
            beam.height,
        );
        shell::fill_rect_clipped(fb, fill, screen, Theme::ACCENT_PRIMARY.to_u32());
        let status = alloc::format!("Starting shell runtime   {:>3}%", (self.boot_progress * 100.0) as u32);
        fb.draw_string(
            (center_x - 104).max(0) as usize,
            (beam.y + 18).max(0) as usize,
            &status,
            Theme::TEXT_SECONDARY.to_u32(),
        );
    }

    fn draw_user_selection(&self, fb: &mut Framebuffer) {
        fb.draw_string(
            self.screen_width / 2 - 48,
            self.screen_height / 5,
            &self.time_string,
            Theme::TEXT_PRIMARY.to_u32(),
        );
        fb.draw_string(
            self.screen_width / 2 - 92,
            self.screen_height / 5 + 26,
            &self.date_string,
            Theme::TEXT_SECONDARY.to_u32(),
        );

        let users_y = self.screen_height / 2 + 10;
        let total_width = self.users.len() * AVATAR_SPACING;
        let start_x = (self.screen_width - total_width) / 2 + AVATAR_SPACING / 2;

        for (index, user) in self.users.iter().enumerate() {
            let x = start_x + index * AVATAR_SPACING;
            self.draw_user_avatar(
                fb,
                x,
                users_y,
                user,
                if self.hovered_user == Some(index) { 1.08 } else { 1.0 },
                self.hovered_user == Some(index),
            );
        }

        fb.draw_string(
            self.screen_width / 2 - 58,
            self.screen_height - 52,
            "Power / Lock",
            Theme::TEXT_SECONDARY.to_u32(),
        );
    }

    fn draw_password_screen(&self, fb: &mut Framebuffer) {
        let Some(index) = self.selected_user else {
            return;
        };
        let screen = Rect::new(0, 0, self.screen_width as u32, self.screen_height as u32);
        let user = &self.users[index];
        let avatar_x = self.screen_width / 2;
        let avatar_y = self.screen_height / 2 - 108;
        let panel = Rect::new(avatar_x as i32 - 200, avatar_y as i32 - 44, 400, 244);
        shell::fill_blended_rect(
            fb,
            panel,
            screen,
            Theme::tokens(ThemeMode::Dark).surfaces.overlay,
            0xB6,
        );
        shell::draw_rect_outline_clipped(fb, panel, screen, Theme::BORDER.to_u32());

        self.draw_user_avatar(fb, avatar_x, avatar_y, user, 0.74, false);
        fb.draw_string(
            avatar_x - user.display_name.len() * 4,
            avatar_y + 84,
            &user.display_name,
            Theme::TEXT_PRIMARY.to_u32(),
        );

        let shake = if self.error_timer > 0.0 {
            (sinf(self.error_timer * 24.0) * 6.0) as i32
        } else {
            0
        };
        let field_x = self.screen_width as i32 / 2 - PASSWORD_WIDTH as i32 / 2 + shake;
        let field_y = avatar_y as i32 + 116;

        fb.draw_rect(
            field_x.max(0) as usize,
            field_y.max(0) as usize,
            PASSWORD_WIDTH,
            PASSWORD_HEIGHT,
            Theme::SIDEBAR_BG.to_u32(),
        );
        fb.draw_rect_outline(
            field_x.max(0) as usize,
            field_y.max(0) as usize,
            PASSWORD_WIDTH,
            PASSWORD_HEIGHT,
            if self.error_timer > 0.0 {
                Theme::ERROR.to_u32()
            } else {
                Theme::BORDER.to_u32()
            },
        );

        let dot_count = self.password.len().min(20) as i32;
        let dot_spacing = 12;
        let dots_width = dot_count * dot_spacing;
        let dots_start = field_x + (PASSWORD_WIDTH as i32 / 2) - dots_width / 2;
        for dot in 0..dot_count {
            let dot_x = dots_start + dot * dot_spacing;
            let dot_y = field_y + PASSWORD_HEIGHT as i32 / 2 - 3;
            fb.draw_rect(dot_x.max(0) as usize, dot_y.max(0) as usize, 6, 6, Theme::TEXT_PRIMARY.to_u32());
        }

        if self.cursor_visible && self.state != LoginState::Authenticating {
            let cursor_x = dots_start + dot_count * dot_spacing + 4;
            fb.draw_rect(
                cursor_x.max(0) as usize,
                (field_y + 8).max(0) as usize,
                2,
                PASSWORD_HEIGHT - 16,
                Theme::TEXT_PRIMARY.to_u32(),
            );
        }

        if !self.error_message.is_empty() {
            fb.draw_string(
                self.screen_width / 2 - self.error_message.len() * 4,
                (field_y + PASSWORD_HEIGHT as i32 + 16).max(0) as usize,
                &self.error_message,
                Theme::ERROR.to_u32(),
            );
        } else if !user.password_hint.is_empty() {
            let hint = alloc::format!("Hint: {}", user.password_hint);
            fb.draw_string(
                self.screen_width / 2 - hint.len() * 4,
                (field_y + PASSWORD_HEIGHT as i32 + 16).max(0) as usize,
                &hint,
                Theme::TEXT_SECONDARY.to_u32(),
            );
        }

        if self.state == LoginState::Authenticating {
            self.draw_spinner(fb, self.screen_width / 2, (field_y + PASSWORD_HEIGHT as i32 + 48).max(0) as usize);
        }

        fb.draw_string(
            (field_x - 54).max(0) as usize,
            (field_y + 10).max(0) as usize,
            "Back",
            Theme::TEXT_SECONDARY.to_u32(),
        );
        fb.draw_string(
            (field_x + PASSWORD_WIDTH as i32 + 16).max(0) as usize,
            (field_y + 10).max(0) as usize,
            "Enter",
            Theme::TEXT_SECONDARY.to_u32(),
        );
    }

    fn draw_user_avatar(
        &self,
        fb: &mut Framebuffer,
        x: usize,
        y: usize,
        user: &UserAccount,
        scale: f32,
        hovered: bool,
    ) {
        let size = (AVATAR_SIZE as f32 * scale) as usize;
        let draw_x = x.saturating_sub(size / 2);
        let draw_y = y.saturating_sub(size / 2);
        let screen = Rect::new(0, 0, self.screen_width as u32, self.screen_height as u32);
        let card = Rect::new(
            draw_x as i32 - 18,
            draw_y as i32 - 18,
            (size + 36) as u32,
            (size + 74) as u32,
        );
        shell::fill_blended_rect(
            fb,
            card,
            screen,
            Theme::tokens(ThemeMode::Dark).surfaces.overlay,
            if hovered { 0xD0 } else { 0xA0 },
        );
        shell::draw_rect_outline_clipped(
            fb,
            card,
            screen,
            if hovered {
                Theme::BORDER_FOCUS.to_u32()
            } else {
                Theme::BORDER.to_u32()
            },
        );

        let bg = match &user.avatar {
            AvatarType::Color(color) => *color,
            AvatarType::Icon(AvatarIcon::Admin) => Theme::ACCENT_WARNING.to_u32(),
            AvatarType::Icon(AvatarIcon::Guest) => Theme::TEXT_SECONDARY.to_u32(),
            _ => Theme::ACCENT_PRIMARY.to_u32(),
        };
        fb.draw_rect(draw_x, draw_y, size, size, bg);
        fb.draw_rect_outline(draw_x, draw_y, size, size, Theme::BORDER.to_u32());

        match &user.avatar {
            AvatarType::Initials(initials) => fb.draw_string(
                draw_x + size / 2 - initials.len() * 4,
                draw_y + size / 2 - 8,
                initials,
                Theme::TEXT_ON_ACCENT.to_u32(),
            ),
            AvatarType::Icon(icon) => {
                let glyph = match icon {
                    AvatarIcon::User => "U",
                    AvatarIcon::Admin => "A",
                    AvatarIcon::Guest => "G",
                    AvatarIcon::Custom(_) => "*",
                };
                fb.draw_string(
                    draw_x + size / 2 - 4,
                    draw_y + size / 2 - 8,
                    glyph,
                    Theme::TEXT_ON_ACCENT.to_u32(),
                )
            }
            AvatarType::Image(_) => fb.draw_string(
                draw_x + size / 2 - 4,
                draw_y + size / 2 - 8,
                "I",
                Theme::TEXT_ON_ACCENT.to_u32(),
            ),
            AvatarType::Color(_) => fb.draw_string(
                draw_x + size / 2 - 4,
                draw_y + size / 2 - 8,
                "C",
                Theme::TEXT_ON_ACCENT.to_u32(),
            ),
        }

        fb.draw_string(
            x.saturating_sub(user.display_name.len() * 4),
            draw_y + size + 12,
            &user.display_name,
            if hovered {
                Theme::TEXT_PRIMARY.to_u32()
            } else {
                Theme::TEXT_SECONDARY.to_u32()
            },
        );
    }

    fn draw_spinner(&self, fb: &mut Framebuffer, x: usize, y: usize) {
        for index in 0..8 {
            let angle = self.animation_progress * core::f32::consts::TAU
                + index as f32 * (core::f32::consts::PI / 4.0);
            let px = x as i32 + (libm::cosf(angle) * 12.0) as i32;
            let py = y as i32 + (sinf(angle) * 12.0) as i32;
            if px >= 0 && py >= 0 {
                fb.plot_pixel(
                    px as usize,
                    py as usize,
                    shell::blend_color(
                        Theme::SIDEBAR_BG.to_u32(),
                        Theme::ACCENT_PRIMARY.to_u32(),
                        (255 - index as u8 * 24).max(48),
                    ),
                );
            }
        }
    }

    fn draw_shutdown_menu(&self, fb: &mut Framebuffer) {
        let rect = Rect::new(self.screen_width as i32 / 2 - 100, self.screen_height as i32 - 140, 200, 100);
        let screen = Rect::new(0, 0, self.screen_width as u32, self.screen_height as u32);
        shell::fill_blended_rect(
            fb,
            rect,
            screen,
            Theme::tokens(ThemeMode::Dark).surfaces.sidebar,
            0xF0,
        );
        shell::draw_rect_outline_clipped(fb, rect, screen, Theme::BORDER.to_u32());
        for (index, label) in ["Shutdown", "Restart", "Lock"].iter().enumerate() {
            let row = Rect::new(rect.x + 8, rect.y + 8 + index as i32 * 28, rect.width - 16, 22);
            shell::fill_rect_clipped(
                fb,
                row,
                screen,
                if index == 2 {
                    Theme::button_fill(crate::gui::theme::ButtonRole::Secondary, ThemeMode::Dark, false, true)
                } else {
                    Theme::SIDEBAR_BG.to_u32()
                },
            );
            fb.draw_string((row.x + 10).max(0) as usize, (row.y + 5).max(0) as usize, label, Theme::TEXT_PRIMARY.to_u32());
        }
    }

    pub fn on_click(&mut self, mx: i32, my: i32) -> LoginEvent {
        match self.state {
            LoginState::UserSelection | LoginState::Locked => {
                let users_y = self.screen_height / 2 + 10;
                let total_width = self.users.len() * AVATAR_SPACING;
                let start_x = (self.screen_width - total_width) / 2 + AVATAR_SPACING / 2;
                for (index, _) in self.users.iter().enumerate() {
                    let x = start_x + index * AVATAR_SPACING;
                    if mx >= (x - AVATAR_SIZE / 2) as i32
                        && mx < (x + AVATAR_SIZE / 2) as i32
                        && my >= (users_y - AVATAR_SIZE / 2) as i32
                        && my < (users_y + AVATAR_SIZE / 2) as i32
                    {
                        self.select_user(index);
                        return LoginEvent::None;
                    }
                }

                if mx >= (self.screen_width / 2 - 60) as i32
                    && mx < (self.screen_width / 2 + 60) as i32
                    && my >= (self.screen_height - 60) as i32
                    && my < (self.screen_height - 36) as i32
                {
                    self.show_shutdown_menu = !self.show_shutdown_menu;
                    return LoginEvent::None;
                }

                if self.show_shutdown_menu {
                    let rect = Rect::new(self.screen_width as i32 / 2 - 100, self.screen_height as i32 - 140, 200, 100);
                    if mx >= rect.x && mx < rect.right() && my >= rect.y && my < rect.bottom() {
                        match ((my - rect.y - 8) / 28) as usize {
                            0 => return LoginEvent::Shutdown,
                            1 => return LoginEvent::Restart,
                            2 => {
                                self.lock();
                                self.show_shutdown_menu = false;
                            }
                            _ => {}
                        }
                    }
                }
            }
            LoginState::PasswordEntry | LoginState::AuthFailed => {
                let field_x = self.screen_width as i32 / 2 - PASSWORD_WIDTH as i32 / 2;
                let field_y = self.screen_height as i32 / 2 + 8;
                if mx >= field_x - 60 && mx < field_x && my >= field_y && my < field_y + PASSWORD_HEIGHT as i32 {
                    self.go_back();
                }
            }
            _ => {}
        }
        LoginEvent::None
    }

    pub fn on_key_press(&mut self, c: char) -> LoginEvent {
        match self.state {
            LoginState::PasswordEntry | LoginState::AuthFailed => {
                if c == '\x1b' {
                    self.go_back();
                } else if c == '\n' || c == '\r' {
                    return self.submit_password();
                } else if c == '\x08' {
                    self.password.pop();
                } else if !c.is_control() && self.password.len() < 32 {
                    self.password.push(c);
                }
            }
            LoginState::UserSelection | LoginState::Locked => {
                if !c.is_control() && !self.users.is_empty() {
                    self.select_user(0);
                }
            }
            _ => {}
        }
        LoginEvent::None
    }

    pub fn on_mouse_move(&mut self, mx: i32, my: i32) {
        if self.state != LoginState::UserSelection && self.state != LoginState::Locked {
            return;
        }
        let users_y = self.screen_height / 2 + 10;
        let total_width = self.users.len() * AVATAR_SPACING;
        let start_x = (self.screen_width - total_width) / 2 + AVATAR_SPACING / 2;
        self.hovered_user = None;
        for (index, _) in self.users.iter().enumerate() {
            let x = start_x + index * AVATAR_SPACING;
            if mx >= (x - AVATAR_SIZE / 2) as i32
                && mx < (x + AVATAR_SIZE / 2) as i32
                && my >= (users_y - AVATAR_SIZE / 2) as i32
                && my < (users_y + AVATAR_SIZE / 2) as i32
            {
                self.hovered_user = Some(index);
                break;
            }
        }
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        self.screen_width = width;
        self.screen_height = height;
    }

    pub fn set_time(&mut self, time: &str, date: &str) {
        self.time_string = String::from(time);
        self.date_string = String::from(date);
    }
}

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

lazy_static::lazy_static! {
    static ref LOGIN: Mutex<LoginScreen> = Mutex::new(LoginScreen::new(1920, 1080));
}

pub fn init(width: usize, height: usize) {
    let mut login = LOGIN.lock();
    login.resize(width, height);
    crate::serial_println!("[GUI] Login screen initialized");
}

pub fn get_login() -> &'static Mutex<LoginScreen> {
    &LOGIN
}
