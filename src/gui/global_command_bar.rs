//! # echOS Global Command Bar
//!
//! Borderless window controls and active-window command surface.

use crate::gop::framebuffer::Framebuffer;
use crate::gui::echos_wm::CyberTheme;

pub const COMMAND_BAR_H: i32 = 22;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandAction {
    None,
    Close,
    Minimize,
    MaximizeToggle,
}

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

    const fn none() -> Self {
        Self { kind: Self::KIND_NONE, win_id: 0, flags: 0, action: CommandAction::None }
    }

    fn focus_changed(win_id: u32, isolated: bool) -> Self {
        Self {
            kind: Self::KIND_FOCUS_CHANGED,
            win_id,
            flags: if isolated { 1 } else { 0 },
            action: CommandAction::None,
        }
    }

    fn action(action: CommandAction) -> Self {
        Self { kind: Self::KIND_ACTION, win_id: 0, flags: 0, action }
    }
}

struct CommandBus {
    q: [CommandMsg; 32],
    head: usize,
    tail: usize,
}

impl CommandBus {
    fn new() -> Self {
        Self { q: [CommandMsg::none(); 32], head: 0, tail: 0 }
    }

    fn post(&mut self, msg: CommandMsg) {
        let next = (self.tail + 1) % self.q.len();
        if next == self.head {
            self.head = (self.head + 1) % self.q.len();
        }
        self.q[self.tail] = msg;
        self.tail = next;
    }

    fn poll(&mut self) -> Option<CommandMsg> {
        if self.head == self.tail {
            return None;
        }
        let msg = self.q[self.head];
        self.head = (self.head + 1) % self.q.len();
        Some(msg)
    }
}

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

    pub fn set_screen_width(&mut self, w: i32) {
        self.screen_w = w;
    }

    pub fn post_focus_changed(&mut self, win_id: u32, isolated: bool) {
        self.bus.post(CommandMsg::focus_changed(win_id, isolated));
    }

    pub fn set_active_title(&mut self, title: &str) {
        self.active_title_len = 0;
        for (i, &b) in title.as_bytes().iter().take(self.active_title.len()).enumerate() {
            self.active_title[i] = b;
            self.active_title_len = i + 1;
        }
    }

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

    pub fn poll_action(&mut self) -> Option<CommandAction> {
        while let Some(msg) = self.bus.poll() {
            if msg.kind == CommandMsg::KIND_ACTION {
                return Some(msg.action);
            }
        }
        None
    }

    pub fn on_mouse_move(&mut self, mx: i32, my: i32) {
        self.hover_action = self.hit_action(mx, my);
    }

    pub fn on_mouse_down(&mut self, mx: i32, my: i32) -> bool {
        let action = self.hit_action(mx, my);
        if action != CommandAction::None {
            self.bus.post(CommandMsg::action(action));
            return true;
        }
        false
    }

    fn bar_rect(&self) -> (i32, i32, i32, i32) {
        let w = 560;
        let h = COMMAND_BAR_H;
        let x = (self.screen_w - w) / 2;
        let y = (self.panel_h - h).max(1) / 2;
        (x, y, w, h)
    }

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

    pub fn draw(&self, fb: &mut Framebuffer) {
        let (x, y, w, h) = self.bar_rect();
        if x < 0 || y < 0 {
            return;
        }

        let bg = 0xB00B1016;
        fb.draw_rect(x as usize, y as usize, w as usize, h as usize, bg);
        fb.draw_rect_outline(x as usize, y as usize, w as usize, h as usize, CyberTheme::BORDER);

        // Isolation indicator (IronShim ring-3 style)
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
