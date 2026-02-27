//! # Calculator Application
//!
//! macOS tarzı hesap makinesi uygulaması
//! Temel matematik işlemleri ve bilimsel mod

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};
use crate::gui::Rect;
use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec::Vec;

// ============================================================================
// CALCULATOR BUTTON
// ============================================================================

/// Calculator button
#[derive(Clone, Debug)]
pub struct CalcButton {
    pub label: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub color: u32,
    pub text_color: u32,
    pub action: CalcAction,
    pub hovered: bool,
    pub pressed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CalcAction {
    Digit(u8),
    Operator(Operator),
    Function(Function),
    Clear,
    ClearEntry,
    Backspace,
    Equals,
    Decimal,
    Percent,
    Negate,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Operator {
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Function {
    Sqrt,
    Sin,
    Cos,
    Tan,
    Log,
    Ln,
    Pow,
    Pi,
    E,
    Factorial,
    Abs,
}

impl CalcButton {
    pub fn new(label: &str, x: i32, y: i32, width: i32, height: i32, action: CalcAction) -> Self {
        let (color, text_color) = Self::get_colors(&action);
        CalcButton {
            label: String::from(label),
            x, y, width, height,
            color,
            text_color,
            action,
            hovered: false,
            pressed: false,
        }
    }
    
    fn get_colors(action: &CalcAction) -> (u32, u32) {
        match action {
            CalcAction::Operator(_) => (0xFF9500, 0xFFFFFF), // Orange
            CalcAction::Function(_) => (0x505050, 0xFFFFFF), // Dark gray
            CalcAction::Clear | CalcAction::ClearEntry => (0x3A3A3C, 0xFFFFFF), // Darker
            CalcAction::Equals => (0xFF9500, 0xFFFFFF), // Orange
            _ => (0x333333, 0xFFFFFF), // Standard button
        }
    }
    
    pub fn draw(&self, fb: &mut Framebuffer) {
        let color = if self.pressed {
            Self::darken_color(self.color, 0.7)
        } else if self.hovered {
            Self::lighten_color(self.color, 1.2)
        } else {
            self.color
        };
        
        // Button background
        fb.draw_rect(self.x as usize, self.y as usize, self.width as usize, self.height as usize, color);
        
        // Button border (subtle)
        fb.draw_rect_outline(self.x as usize, self.y as usize, self.width as usize, self.height as usize, Self::darken_color(color, 0.8));
        
        // Button label
        let label_x = self.x + self.width / 2 - (self.label.len() as i32 * 4);
        let label_y = self.y + self.height / 2 - 6;
        fb.draw_string(label_x as usize, label_y as usize, &self.label, self.text_color);
    }
    
    fn darken_color(c: u32, factor: f32) -> u32 {
        let r = (((c >> 16) & 0xFF) as f32 * factor) as u32;
        let g = (((c >> 8) & 0xFF) as f32 * factor) as u32;
        let b = ((c & 0xFF) as f32 * factor) as u32;
        ((r.min(255) << 16) | (g.min(255) << 8) | b.min(255))
    }
    
    fn lighten_color(c: u32, factor: f32) -> u32 {
        let r = (((c >> 16) & 0xFF) as f32 * factor) as u32;
        let g = (((c >> 8) & 0xFF) as f32 * factor) as u32;
        let b = ((c & 0xFF) as f32 * factor) as u32;
        ((r.min(255) << 16) | (g.min(255) << 8) | b.min(255))
    }
    
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }
}

// ============================================================================
// CALCULATOR STATE
// ============================================================================

/// Calculator state
pub struct CalculatorState {
    /// Current display value
    display: String,
    /// Previous value (for operations)
    previous: f64,
    /// Current value being entered
    current: f64,
    /// Pending operator
    operator: Option<Operator>,
    /// Has decimal point
    has_decimal: bool,
    /// Is entering new number
    entering_new: bool,
    /// Memory
    memory: f64,
    /// Scientific mode
    scientific_mode: bool,
    /// History
    history: Vec<String>,
}

impl CalculatorState {
    pub fn new() -> Self {
        CalculatorState {
            display: String::from("0"),
            previous: 0.0,
            current: 0.0,
            operator: None,
            has_decimal: false,
            entering_new: true,
            memory: 0.0,
            scientific_mode: false,
            history: Vec::new(),
        }
    }
    
    pub fn handle_action(&mut self, action: CalcAction) {
        match action {
            CalcAction::Digit(d) => self.input_digit(d),
            CalcAction::Operator(op) => self.input_operator(op),
            CalcAction::Function(f) => self.input_function(f),
            CalcAction::Clear => self.clear(),
            CalcAction::ClearEntry => self.clear_entry(),
            CalcAction::Backspace => self.backspace(),
            CalcAction::Equals => self.calculate(),
            CalcAction::Decimal => self.input_decimal(),
            CalcAction::Percent => self.percent(),
            CalcAction::Negate => self.negate(),
        }
    }
    
    fn input_digit(&mut self, digit: u8) {
        if self.entering_new {
            self.display.clear();
            self.display.push((b'0' + digit) as char);
            self.entering_new = false;
        } else {
            if self.display == "0" && digit == 0 {
                return;
            }
            if self.display.len() < 15 {
                self.display.push((b'0' + digit) as char);
            }
        }
        self.current = self.display.parse().unwrap_or(0.0);
    }
    
    fn input_decimal(&mut self) {
        if self.entering_new {
            self.display = String::from("0.");
            self.has_decimal = true;
            self.entering_new = false;
        } else if !self.has_decimal {
            self.display.push('.');
            self.has_decimal = true;
        }
    }
    
    fn input_operator(&mut self, op: Operator) {
        if self.operator.is_some() && !self.entering_new {
            self.calculate();
        }
        self.previous = self.current;
        self.operator = Some(op);
        self.entering_new = true;
        self.has_decimal = false;
    }
    
    fn input_function(&mut self, func: Function) {
        let value = self.current;
        let result = match func {
            Function::Sqrt => {
                // Simple sqrt using exponentiation
                if value >= 0.0 { 
                    // Newton's method for sqrt
                    let mut x = value / 2.0;
                    for _ in 0..20 {
                        x = 0.5 * (x + value / x);
                    }
                    x
                } else { f64::NAN }
            },
            Function::Sin | Function::Cos | Function::Tan => {
                // Not supported in basic mode
                f64::NAN
            },
            Function::Log | Function::Ln => {
                // Not supported in basic mode
                f64::NAN
            },
            Function::Pow => value * value,
            Function::Pi => 3.14159265358979,
            Function::E => 2.71828182845904,
            Function::Factorial => Self::factorial(value),
            Function::Abs => if value < 0.0 { -value } else { value },
        };
        self.current = result;
        self.update_display();
        self.entering_new = true;
    }
    
    fn factorial(n: f64) -> f64 {
        if n < 0.0 {
            return f64::NAN;
        }
        // Check if integer
        let n_int = n as u64;
        let diff = n - n_int as f64;
        // Manual abs check
        let is_not_integer = if diff < 0.0 { -diff } else { diff } > 0.0001;
        if is_not_integer {
            return f64::NAN;
        }
        let mut result = 1.0;
        for i in 2..=n_int {
            result *= i as f64;
        }
        result
    }
    
    fn calculate(&mut self) {
        if let Some(op) = self.operator {
            let result = match op {
                Operator::Add => self.previous + self.current,
                Operator::Subtract => self.previous - self.current,
                Operator::Multiply => self.previous * self.current,
                Operator::Divide => {
                    if self.current == 0.0 {
                        f64::NAN
                    } else {
                        self.previous / self.current
                    }
                }
            };
            
            // Add to history
            let op_str = match op {
                Operator::Add => "+",
                Operator::Subtract => "-",
                Operator::Multiply => "×",
                Operator::Divide => "÷",
            };
            let history_entry = format!("{} {} {} = {}", self.previous, op_str, self.current, result);
            self.history.push(history_entry);
            if self.history.len() > 10 {
                self.history.remove(0);
            }
            
            self.current = result;
            self.operator = None;
            self.update_display();
            self.entering_new = true;
        }
    }
    
    fn clear(&mut self) {
        self.display = String::from("0");
        self.previous = 0.0;
        self.current = 0.0;
        self.operator = None;
        self.has_decimal = false;
        self.entering_new = true;
    }
    
    fn clear_entry(&mut self) {
        self.display = String::from("0");
        self.current = 0.0;
        self.has_decimal = false;
        self.entering_new = true;
    }
    
    fn backspace(&mut self) {
        if !self.entering_new && self.display.len() > 1 {
            self.display.pop();
            if self.display.ends_with('.') {
                self.display.pop();
                self.has_decimal = false;
            }
            self.current = self.display.parse().unwrap_or(0.0);
        } else {
            self.display = String::from("0");
            self.current = 0.0;
        }
    }
    
    fn percent(&mut self) {
        self.current = self.current / 100.0;
        self.update_display();
    }
    
    fn negate(&mut self) {
        self.current = -self.current;
        self.update_display();
    }
    
    fn update_display(&mut self) {
        if self.current.is_nan() {
            self.display = String::from("Error");
        } else if self.current.is_infinite() {
            self.display = String::from("Infinity");
        } else {
            // Format number nicely - check if integer by comparing with floor
            let floor = (self.current as i64) as f64;
            let diff = self.current - floor;
            let diff_abs = if diff < 0.0 { -diff } else { diff };
            let is_integer = diff_abs < 0.0001;
            let current_abs = if self.current < 0.0 { -self.current } else { self.current };
            if is_integer && current_abs < 1e15 {
                self.display = format!("{}", self.current as i64);
            } else {
                self.display = format!("{:.10}", self.current);
                // Remove trailing zeros
                while self.display.ends_with('0') && self.display.contains('.') {
                    self.display.pop();
                }
                if self.display.ends_with('.') {
                    self.display.pop();
                }
            }
        }
    }
    
    pub fn get_display(&self) -> &str {
        &self.display
    }
    
    pub fn toggle_scientific(&mut self) {
        self.scientific_mode = !self.scientific_mode;
    }
    
    pub fn is_scientific_mode(&self) -> bool {
        self.scientific_mode
    }
}

// ============================================================================
// CALCULATOR WINDOW
// ============================================================================

/// Calculator window
pub struct CalculatorWindow {
    pub rect: Rect,
    state: CalculatorState,
    buttons: Vec<CalcButton>,
    titlebar_height: i32,
    display_height: i32,
    button_size: i32,
    button_spacing: i32,
}

impl CalculatorWindow {
    pub fn new(rect: Rect) -> Self {
        let mut calc = CalculatorWindow {
            rect,
            state: CalculatorState::new(),
            buttons: Vec::new(),
            titlebar_height: 28,
            display_height: 60,
            button_size: 50,
            button_spacing: 4,
        };
        calc.create_buttons();
        calc
    }
    
    fn create_buttons(&mut self) {
        self.buttons.clear();
        
        let start_x = self.rect.x + 10;
        let start_y = self.rect.y + self.titlebar_height + self.display_height + 10;
        let btn_w = self.button_size;
        let btn_h = self.button_size;
        let gap = self.button_spacing;
        
        // Row 0: Clear, +/-, %, ÷
        let row0_y = start_y;
        self.buttons.push(CalcButton::new("AC", start_x, row0_y, btn_w, btn_h, CalcAction::Clear));
        self.buttons.push(CalcButton::new("±", start_x + btn_w + gap, row0_y, btn_w, btn_h, CalcAction::Negate));
        self.buttons.push(CalcButton::new("%", start_x + (btn_w + gap) * 2, row0_y, btn_w, btn_h, CalcAction::Percent));
        self.buttons.push(CalcButton::new("÷", start_x + (btn_w + gap) * 3, row0_y, btn_w, btn_h, CalcAction::Operator(Operator::Divide)));
        
        // Row 1-3: 7-9, 4-6, 1-3
        for row in 0usize..3 {
            let row_y = start_y + (btn_h + gap) * (row as i32 + 1);
            for col in 0usize..3 {
                let digit = 7 - row * 3 + col;
                let x = start_x + (btn_w + gap) * col as i32;
                self.buttons.push(CalcButton::new(&format!("{}", digit), x, row_y, btn_w, btn_h, CalcAction::Digit(digit as u8)));
            }
            // Operator
            let ops = [Operator::Multiply, Operator::Subtract, Operator::Add];
            let op_labels = ["×", "-", "+"];
            self.buttons.push(CalcButton::new(op_labels[row], start_x + (btn_w + gap) * 3, row_y, btn_w, btn_h, CalcAction::Operator(ops[row])));
        }
        
        // Row 4: 0, ., =
        let row4_y = start_y + (btn_h + gap) * 4;
        self.buttons.push(CalcButton::new("0", start_x, row4_y, btn_w * 2 + gap, btn_h, CalcAction::Digit(0)));
        self.buttons.push(CalcButton::new(".", start_x + (btn_w + gap) * 2, row4_y, btn_w, btn_h, CalcAction::Decimal));
        self.buttons.push(CalcButton::new("=", start_x + (btn_w + gap) * 3, row4_y, btn_w, btn_h, CalcAction::Equals));
    }
    
    pub fn draw(&self, fb: &mut Framebuffer) {
        // Window background
        fb.draw_rect(self.rect.x as usize, self.rect.y as usize, self.rect.width as usize, self.rect.height as usize, 0x1C1C1E);
        
        // Titlebar
        fb.draw_rect(self.rect.x as usize, self.rect.y as usize, self.rect.width as usize, self.titlebar_height as usize, Theme::TITLEBAR_BG.to_u32());
        
        // Title
        fb.draw_string(self.rect.x as usize + 10, self.rect.y as usize + 8, "Calculator", Theme::TEXT_PRIMARY.to_u32());
        
        // Display area
        let display_y = self.rect.y + self.titlebar_height;
        fb.draw_rect(self.rect.x as usize, display_y as usize, self.rect.width as usize, self.display_height as usize, 0x000000);
        
        // Display value
        let display_text = self.state.get_display();
        let display_x = self.rect.x + self.rect.width - 20 - (display_text.len() as i32 * 12);
        let display_y = display_y + self.display_height - 30;
        fb.draw_string(display_x as usize, display_y as usize, display_text, 0xFFFFFF);
        
        // Buttons
        for btn in &self.buttons {
            btn.draw(fb);
        }
    }
    
    pub fn on_click(&mut self, x: i32, y: i32) -> bool {
        for btn in &mut self.buttons {
            if btn.contains(x, y) {
                btn.pressed = true;
                self.state.handle_action(btn.action);
                return true;
            }
        }
        false
    }
    
    pub fn on_release(&mut self) {
        for btn in &mut self.buttons {
            btn.pressed = false;
        }
    }
    
    pub fn on_hover(&mut self, x: i32, y: i32) {
        for btn in &mut self.buttons {
            btn.hovered = btn.contains(x, y);
        }
    }
}
