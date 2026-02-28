//! # echOS Global Komut Çubuğu (Command Bar)
//!
//! Pencere çerçevesiz arayüzde aktif pencere kontrollerini ve komut yüzeyini sağlar.
//! Ekranın üst panelinde merkeze yerleştirilmiş, yüzer bir çubuk olarak çalışır.
//!
//! ## Mimari
//! - `CommandAction`: Minimize/Maksimize/Kapat gibi pencere eylemlerini tanımlar
//! - `CommandMsg`: Dahili mesaj kuyruğu için eylem veya odak değişimi mesajı
//! - `CommandBus`: Dairesel 32 elemanlı mesaj kuyruğu (ring buffer)
//! - `GlobalCommandBar`: Ekran genişliği, odak bilgisi ve hover durumunu yönetir
//!
//! ## Ring Buffer (Dairesel Kuyruk) Tasarımı
//! `CommandBus` 32 elemanlı sabit boyutlu bir dizi üzerinde çalışır.
//! `head` ve `tail` indeksleri mod 32 ile güncellenir; bellek tahsisi yoktur.
//! Kuyruk doluyken (`(tail+1) % 32 == head`) en eski mesajın üzerine yazılır.
//! Çekirdek ortamında `alloc` kullanmak maliyetli olduğundan bu yaklaşım tercih edilir.
//!
//! ## IronShim İzolasyon Göstergesi
//! Çubuğun sol kenarındaki renkli 4×(h-6) piksel şerit aktif pencerenin izolasyon
//! seviyesini gösterir:
//! - Yeşil (`SUCCESS`, #39FF89) = ring-3 kullanıcı alanı yalıtımı (ISOLATED) aktif
//! - Sarı (`WARNING`, #FFB800) = pencere çekirdek modunda (KERNEL) çalışıyor
//!
//! ## Hit-Test Algoritması
//! `hit_action(mx, my)` çubuk dikdörtgenini hesaplar, ardından sağdan sola
//! Kapat → Maksimize → Minimize düğmelerini sıralı aralıklarla denetler.
//! Her düğme `btn_w = 18` piksel, `gap = 8` piksel boşlukla yerleştirilir.

use crate::gop::framebuffer::Framebuffer;
use crate::gui::echos_wm::CyberTheme;

pub const COMMAND_BAR_H: i32 = 22;

/// Komut çubuğu düğmelerinden gerçekleştirilebilecek pencere eylemleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandAction {
    None,
    Close,
    Minimize,
    MaximizeToggle,
}

/// Dahili mesaj kuyruğu için mesaj yapısı; odak değişimi veya pencere eylemi taşır
#[derive(Clone, Copy)]
struct CommandMsg {
    kind: u8,
    win_id: u32,
    flags: u8,
    action: CommandAction,
}

impl CommandMsg {
    const KIND_NONE: u8 = 0;
    const KIND_FOCUS_CHANGED: u8 = 1;
    const KIND_ACTION: u8 = 2;

/// Boş (hiçbir şey yok) mesaj oluşturur; kuyruk başlatmada kullanılır
    const fn none() -> Self {
        Self { kind: Self::KIND_NONE, win_id: 0, flags: 0, action: CommandAction::None }
    }

    /// Odak değişimi mesajı oluşturur; hangi pencerenin aktif olduğunu ve izole mi olduğunu bildirir
    fn focus_changed(win_id: u32, isolated: bool) -> Self {
        Self {
            kind: Self::KIND_FOCUS_CHANGED,
            win_id,
            flags: if isolated { 1 } else { 0 },
            action: CommandAction::None,
        }
    }

    /// Pencere eylemi mesajı oluşturur (kapat, minimize, maksimize)
    fn action(action: CommandAction) -> Self {
        Self { kind: Self::KIND_ACTION, win_id: 0, flags: 0, action }
    }
}

/// Dairesel 32 elemanlı mesaj kuyruğu; komut çubuğu olaylarını WM'a iletir
struct CommandBus {
    q: [CommandMsg; 32],
    head: usize,
    tail: usize,
}

impl CommandBus {
    /// Kuyruğu boş mesajlarla başlatır
    fn new() -> Self {
        Self { q: [CommandMsg::none(); 32], head: 0, tail: 0 }
    }

    /// Kuyruğa yeni mesaj ekler; kuyruk doluysa en eski mesajın üzerine yazar
    fn post(&mut self, msg: CommandMsg) {
        let next = (self.tail + 1) % self.q.len();
        if next == self.head {
            self.head = (self.head + 1) % self.q.len();
        }
        self.q[self.tail] = msg;
        self.tail = next;
    }

    /// Kuyruktan sonraki mesajı alır; kuyruk boşsa `None` döner
    fn poll(&mut self) -> Option<CommandMsg> {
        if self.head == self.tail {
            return None;
        }
        let msg = self.q[self.head];
        self.head = (self.head + 1) % self.q.len();
        Some(msg)
    }
}

/// Ekranın üst panelinde yüzen global komut çubuğu;
/// aktif pencerenin başlığını, izolasyon durumunu ve pencere kontrol düğmelerini gösterir
pub struct GlobalCommandBar {
    screen_w: i32,
    panel_h: i32,
    active_win_id: u32,
    active_isolated: bool,
    active_title: [u8; 48],
    active_title_len: usize,
    hover_action: CommandAction,
    bus: CommandBus,
}

impl GlobalCommandBar {
    /// Yeni komut çubuğu örneği oluşturur; ekran genişliği ve panel yüksekliği gerektirir
    pub fn new(screen_w: i32, panel_h: i32) -> Self {
        Self {
            screen_w,
            panel_h,
            active_win_id: 0,
            active_isolated: false,
            active_title: [0; 48],
            active_title_len: 0,
            hover_action: CommandAction::None,
            bus: CommandBus::new(),
        }
    }

    /// Ekran genişliğini günceller; pencere yeniden boyutlandırıldığında çağrılır
    pub fn set_screen_width(&mut self, w: i32) {
        self.screen_w = w;
    }

    /// Odak değişimi olayını mesaj kuyruğuna gönderir; WM tarafından çağrılır
    pub fn post_focus_changed(&mut self, win_id: u32, isolated: bool) {
        self.bus.post(CommandMsg::focus_changed(win_id, isolated));
    }

    /// Aktif pencerenin başlığını günceller; en fazla 48 bayt saklanır
    pub fn set_active_title(&mut self, title: &str) {
        self.active_title_len = 0;
        for (i, &b) in title.as_bytes().iter().take(self.active_title.len()).enumerate() {
            self.active_title[i] = b;
            self.active_title_len = i + 1;
        }
    }

    /// Her kare çağrılır; mesaj kuyruğundan odak değişimlerini işler
    pub fn update(&mut self) {
        while let Some(msg) = self.bus.poll() {
            match msg.kind {
                CommandMsg::KIND_FOCUS_CHANGED => {
                    self.active_win_id = msg.win_id;
                    self.active_isolated = (msg.flags & 1) != 0;
                }
                CommandMsg::KIND_ACTION => {
                    self.bus.post(msg);
                    break;
                }
                _ => {}
            }
        }
    }

    /// Kuyruktan bir pencere eylemi varsa döndürür; WM tarafından sorgulanır
    pub fn poll_action(&mut self) -> Option<CommandAction> {
        while let Some(msg) = self.bus.poll() {
            if msg.kind == CommandMsg::KIND_ACTION {
                return Some(msg.action);
            }
        }
        None
    }

    /// Fare hareketi olayını işler; hangi düğmenin üzerinde olunduğunu günceller
    pub fn on_mouse_move(&mut self, mx: i32, my: i32) {
        self.hover_action = self.hit_action(mx, my);
    }

    /// Fare tıklaması olayını işler; bir düğmeye tıklandıysa eylemi kuyruğa ekler ve `true` döner
    pub fn on_mouse_down(&mut self, mx: i32, my: i32) -> bool {
        let action = self.hit_action(mx, my);
        if action != CommandAction::None {
            self.bus.post(CommandMsg::action(action));
            return true;
        }
        false
    }

    /// Çubuğun ekrandaki (x, y, genişlik, yükseklik) dikdörtgenini hesaplar; ekranda ortalanmış 560px genişlik
    fn bar_rect(&self) -> (i32, i32, i32, i32) {
        let w = 560;
        let h = COMMAND_BAR_H;
        let x = (self.screen_w - w) / 2;
        let y = (self.panel_h - h).max(1) / 2;
        (x, y, w, h)
    }

    /// Fare koordinatlarına göre hangi düğme üzerinde olunduğunu döndürür
    fn hit_action(&self, mx: i32, my: i32) -> CommandAction {
        let (x, y, w, h) = self.bar_rect();
        if mx < x || my < y || mx >= x + w || my >= y + h {
            return CommandAction::None;
        }

        let btn_w = 18;
        let gap = 8;
        let right = x + w - 10;
        let close_x = right - btn_w;
        let max_x = close_x - gap - btn_w;
        let min_x = max_x - gap - btn_w;
        let by = y + 2;
        let bh = h - 4;

        if mx >= close_x && mx < close_x + btn_w && my >= by && my < by + bh {
            CommandAction::Close
        } else if mx >= min_x && mx < min_x + btn_w && my >= by && my < by + bh {
            CommandAction::Minimize
        } else if mx >= max_x && mx < max_x + btn_w && my >= by && my < by + bh {
            CommandAction::MaximizeToggle
        } else {
            CommandAction::None
        }
    }

    /// Komut çubuğunu framebuffer'a çizer: arka plan, izolasyon şeridi, başlık ve kontrol düğmeleri
    pub fn draw(&self, fb: &mut Framebuffer) {
        let (x, y, w, h) = self.bar_rect();
        if x < 0 || y < 0 {
            return;
        }

        let bg = 0xB00B1016;
        fb.draw_rect(x as usize, y as usize, w as usize, h as usize, bg);
        fb.draw_rect_outline(x as usize, y as usize, w as usize, h as usize, CyberTheme::BORDER);

        // İzolasyon göstergesi (IronShim ring-3 tarzı): yeşil = izole, sarı = çekirdek
        let iso_color = if self.active_isolated { CyberTheme::SUCCESS } else { CyberTheme::WARNING };
        fb.draw_rect((x + 3) as usize, (y + 3) as usize, 4, (h - 6) as usize, iso_color);

        let status = if self.active_isolated { "ISOLATED" } else { "KERNEL" };
        fb.draw_string((x + 12) as usize, (y + 7) as usize, status, CyberTheme::TEXT_SECONDARY);

        let title = if self.active_title_len == 0 {
            "No Active Window"
        } else {
            core::str::from_utf8(&self.active_title[..self.active_title_len]).unwrap_or("Window")
        };
        fb.draw_string((x + 110) as usize, (y + 7) as usize, title, CyberTheme::TEXT_PRIMARY);

        let btn_w = 18;
        let gap = 8;
        let right = x + w - 10;
        let close_x = right - btn_w;
        let max_x = close_x - gap - btn_w;
        let min_x = max_x - gap - btn_w;
        let by = y + 2;
        let bh = h - 4;

        let min_col = if self.hover_action == CommandAction::Minimize { CyberTheme::BTN_HOVER_MIN } else { CyberTheme::BTN_MIN };
        let max_col = if self.hover_action == CommandAction::MaximizeToggle { CyberTheme::BTN_HOVER_MAX } else { CyberTheme::BTN_MAX };
        let close_col = if self.hover_action == CommandAction::Close { CyberTheme::BTN_HOVER_CLOSE } else { CyberTheme::BTN_CLOSE };

        fb.draw_rect(min_x as usize, by as usize, btn_w as usize, bh as usize, min_col);
        fb.draw_rect(max_x as usize, by as usize, btn_w as usize, bh as usize, max_col);
        fb.draw_rect(close_x as usize, by as usize, btn_w as usize, bh as usize, close_col);
        fb.draw_string((min_x + 6) as usize, (y + 7) as usize, "-", 0xFF101010);
        fb.draw_string((max_x + 5) as usize, (y + 7) as usize, "□", 0xFF101010);
        fb.draw_string((close_x + 5) as usize, (y + 7) as usize, "×", 0xFF101010);
    }
}
