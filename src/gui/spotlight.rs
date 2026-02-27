//! # Spotlight-style Global Search
//!
//! Full-screen search overlay for apps, files, and system commands
//! Real-time search with categories and previews

use alloc::boxed::Box;
use alloc::string::String;
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use alloc::collections::BTreeMap;
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};

// ============================================================================
// SEARCH RESULT
// ============================================================================

/// A search result item
#[derive(Clone, Debug)]
pub struct SearchResult {
    /// Display title
    pub title: String,
    /// Subtitle/description
    pub subtitle: String,
    /// Result type
    pub result_type: ResultType,
    /// Match score (0.0 - 1.0)
    pub score: f32,
    /// Action to perform
    pub action: SearchAction,
    /// Icon identifier
    pub icon: SearchIcon,
    /// Path (for files)
    pub path: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResultType {
    Application,
    File,
    Folder,
    SystemCommand,
    Setting,
    Contact,
    Calendar,
    WebSearch,
    Calculator,
    Dictionary,
}

#[derive(Clone, Debug)]
pub enum SearchAction {
    LaunchApp(String),
    OpenFile(String),
    OpenFolder(String),
    OpenSetting(String),
    ExecuteCommand(String),
    WebSearch(String),
    Calculate(String),
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchIcon {
    App,
    File,
    Folder,
    Setting,
    Command,
    Contact,
    Calendar,
    Web,
    Calculator,
    Dictionary,
    Custom(u16),
}

impl SearchResult {
    pub fn app(name: &str, app_id: &str) -> Self {
        SearchResult {
            title: String::from(name),
            subtitle: String::from("Application"),
            result_type: ResultType::Application,
            score: 1.0,
            action: SearchAction::LaunchApp(String::from(app_id)),
            icon: SearchIcon::App,
            path: String::new(),
        }
    }
    
    pub fn file(name: &str, path: &str) -> Self {
        SearchResult {
            title: String::from(name),
            subtitle: String::from(path),
            result_type: ResultType::File,
            score: 0.8,
            action: SearchAction::OpenFile(String::from(path)),
            icon: SearchIcon::File,
            path: String::from(path),
        }
    }
    
    pub fn folder(name: &str, path: &str) -> Self {
        SearchResult {
            title: String::from(name),
            subtitle: String::from(path),
            result_type: ResultType::Folder,
            score: 0.9,
            action: SearchAction::OpenFolder(String::from(path)),
            icon: SearchIcon::Folder,
            path: String::from(path),
        }
    }
    
    pub fn setting(name: &str, category: &str) -> Self {
        SearchResult {
            title: String::from(name),
            subtitle: String::from(category),
            result_type: ResultType::Setting,
            score: 0.7,
            action: SearchAction::OpenSetting(String::from(name)),
            icon: SearchIcon::Setting,
            path: String::new(),
        }
    }
    
    pub fn command(name: &str, description: &str, cmd: &str) -> Self {
        SearchResult {
            title: String::from(name),
            subtitle: String::from(description),
            result_type: ResultType::SystemCommand,
            score: 0.6,
            action: SearchAction::ExecuteCommand(String::from(cmd)),
            icon: SearchIcon::Command,
            path: String::new(),
        }
    }
}

// ============================================================================
// SEARCH INDEX
// ============================================================================

/// Search index for fast lookups
pub struct SearchIndex {
    /// Applications
    apps: Vec<SearchResult>,
    /// Files (cached)
    files: Vec<SearchResult>,
    /// Settings
    settings: Vec<SearchResult>,
    /// Commands
    commands: Vec<SearchResult>,
    /// Is indexed
    indexed: bool,
}

impl SearchIndex {
    pub fn new() -> Self {
        let mut index = SearchIndex {
            apps: Vec::new(),
            files: Vec::new(),
            settings: Vec::new(),
            commands: Vec::new(),
            indexed: false,
        };
        
        index.build_default_index();
        index
    }
    
    fn build_default_index(&mut self) {
        // Add default apps
        self.apps = vec![
            SearchResult::app("Finder", "finder"),
            SearchResult::app("Safari", "safari"),
            SearchResult::app("Mail", "mail"),
            SearchResult::app("Messages", "messages"),
            SearchResult::app("Music", "music"),
            SearchResult::app("Photos", "photos"),
            SearchResult::app("Calendar", "calendar"),
            SearchResult::app("Notes", "notes"),
            SearchResult::app("Terminal", "terminal"),
            SearchResult::app("Files", "files"),
            SearchResult::app("Settings", "settings"),
            SearchResult::app("Text Editor", "textedit"),
            SearchResult::app("Calculator", "calculator"),
        ];
        
        // Add default settings
        self.settings = vec![
            SearchResult::setting("Display", "Hardware"),
            SearchResult::setting("Sound", "Hardware"),
            SearchResult::setting("Network", "Hardware"),
            SearchResult::setting("Bluetooth", "Hardware"),
            SearchResult::setting("Personalization", "Appearance"),
            SearchResult::setting("Privacy", "Security"),
            SearchResult::setting("Users", "System"),
            SearchResult::setting("Storage", "System"),
        ];
        
        // Add default commands
        self.commands = vec![
            SearchResult::command("Sleep", "Put system to sleep", "sleep"),
            SearchResult::command("Restart", "Restart the system", "restart"),
            SearchResult::command("Shut Down", "Power off the system", "shutdown"),
            SearchResult::command("Log Out", "Log out current user", "logout"),
            SearchResult::command("Lock Screen", "Lock the screen", "lock"),
            SearchResult::command("Empty Trash", "Empty the trash", "empty_trash"),
            SearchResult::command("Screenshot", "Take a screenshot", "screenshot"),
            SearchResult::command("Force Quit", "Force quit application", "force_quit"),
        ];
        
        self.indexed = true;
    }
    
    /// Search all categories
    pub fn search(&self, query: &str) -> Vec<SearchResult> {
        if query.is_empty() {
            return Vec::new();
        }
        
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();
        
        // Search apps
        for app in &self.apps {
            if let Some(score) = self.match_score(&app.title, &query_lower) {
                let mut result = app.clone();
                result.score = score;
                results.push(result);
            }
        }
        
        // Search files
        for file in &self.files {
            if let Some(score) = self.match_score(&file.title, &query_lower) {
                let mut result = file.clone();
                result.score = score;
                results.push(result);
            }
        }
        
        // Search settings
        for setting in &self.settings {
            if let Some(score) = self.match_score(&setting.title, &query_lower) {
                let mut result = setting.clone();
                result.score = score;
                results.push(result);
            }
        }
        
        // Search commands
        for cmd in &self.commands {
            if let Some(score) = self.match_score(&cmd.title, &query_lower) {
                let mut result = cmd.clone();
                result.score = score;
                results.push(result);
            }
        }
        
        // Check for calculator expression
        if self.is_math_expression(query) {
            if let Some(result) = self.evaluate_math(query) {
                results.push(result);
            }
        }
        
        // Check for web search
        if query.len() > 2 {
            results.push(SearchResult {
                title: format!("Search web for '{}'", query),
                subtitle: String::from("Web Search"),
                result_type: ResultType::WebSearch,
                score: 0.3,
                action: SearchAction::WebSearch(String::from(query)),
                icon: SearchIcon::Web,
                path: String::new(),
            });
        }
        
        // Sort by score
        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(core::cmp::Ordering::Equal));
        
        // Limit results
        results.truncate(20);
        
        results
    }
    
    fn match_score(&self, text: &str, query: &str) -> Option<f32> {
        let text_lower = text.to_lowercase();
        
        // Exact match
        if text_lower == query {
            return Some(1.0);
        }
        
        // Starts with
        if text_lower.starts_with(query) {
            return Some(0.9);
        }
        
        // Contains
        if text_lower.contains(query) {
            return Some(0.7);
        }
        
        // Fuzzy match
        if self.fuzzy_match(&text_lower, query) {
            return Some(0.5);
        }
        
        None
    }
    
    fn fuzzy_match(&self, text: &str, query: &str) -> bool {
        let mut text_chars = text.chars();
        for q in query.chars() {
            loop {
                match text_chars.next() {
                    Some(t) if t == q => break,
                    Some(_) => continue,
                    None => return false,
                }
            }
        }
        true
    }
    
    fn is_math_expression(&self, query: &str) -> bool {
        let chars: Vec<char> = query.chars().collect();
        let has_digit = chars.iter().any(|c| c.is_ascii_digit());
        let has_op = chars.iter().any(|c| "+-*/^%".contains(*c));
        has_digit && has_op
    }
    
    fn evaluate_math(&self, query: &str) -> Option<SearchResult> {
        // Very simple expression evaluator
        let expr = query.replace(" ", "");
        
        // Try to parse and evaluate
        let result = self.eval_expr(&expr)?;
        
        Some(SearchResult {
            title: format!("{} = {}", query, result),
            subtitle: String::from("Calculator"),
            result_type: ResultType::Calculator,
            score: 1.0,
            action: SearchAction::Calculate(format!("{}", result)),
            icon: SearchIcon::Calculator,
            path: String::new(),
        })
    }
    
    fn eval_expr(&self, expr: &str) -> Option<f64> {
        // Simple recursive descent parser for basic math
        // Only handles + - * / and parentheses
        
        let mut pos = 0;
        self.parse_expr(expr, &mut pos)
    }
    
    fn parse_expr(&self, expr: &str, pos: &mut usize) -> Option<f64> {
        let mut left = self.parse_term(expr, pos)?;
        
        while *pos < expr.len() {
            let op = expr.chars().nth(*pos)?;
            if op == '+' || op == '-' {
                *pos += 1;
                let right = self.parse_term(expr, pos)?;
                left = if op == '+' { left + right } else { left - right };
            } else {
                break;
            }
        }
        
        Some(left)
    }
    
    fn parse_term(&self, expr: &str, pos: &mut usize) -> Option<f64> {
        let mut left = self.parse_factor(expr, pos)?;
        
        while *pos < expr.len() {
            let op = expr.chars().nth(*pos)?;
            if op == '*' || op == '/' {
                *pos += 1;
                let right = self.parse_factor(expr, pos)?;
                left = if op == '*' { left * right } else { left / right };
            } else {
                break;
            }
        }
        
        Some(left)
    }
    
    fn parse_factor(&self, expr: &str, pos: &mut usize) -> Option<f64> {
        if *pos >= expr.len() {
            return None;
        }
        
        let c = expr.chars().nth(*pos)?;
        
        if c == '(' {
            *pos += 1;
            let result = self.parse_expr(expr, pos)?;
            if *pos < expr.len() && expr.chars().nth(*pos)? == ')' {
                *pos += 1;
            }
            return Some(result);
        }
        
        if c.is_ascii_digit() || c == '.' {
            let mut num_str = String::new();
            while *pos < expr.len() {
                let c = expr.chars().nth(*pos)?;
                if c.is_ascii_digit() || c == '.' {
                    num_str.push(c);
                    *pos += 1;
                } else {
                    break;
                }
            }
            return num_str.parse::<f64>().ok();
        }
        
        None
    }
    
    /// Index files from filesystem
    pub fn index_files(&mut self) {
        self.files.clear();
        
        // Add common folders
        self.files.push(SearchResult::folder("Home", "/home"));
        self.files.push(SearchResult::folder("Documents", "/home/documents"));
        self.files.push(SearchResult::folder("Downloads", "/home/downloads"));
        self.files.push(SearchResult::folder("Pictures", "/home/pictures"));
        self.files.push(SearchResult::folder("Music", "/home/music"));
        self.files.push(SearchResult::folder("Videos", "/home/videos"));
        
        // Would scan filesystem here
    }
}

// ============================================================================
// SPOTLIGHT OVERLAY
// ============================================================================

/// Spotlight-style search overlay
pub struct Spotlight {
    /// Is visible
    pub visible: bool,
    /// Search query
    pub query: String,
    /// Search results
    pub results: Vec<SearchResult>,
    /// Selected result index
    pub selected_index: usize,
    /// Search index
    pub index: SearchIndex,
    /// Animation progress (0.0 - 1.0)
    pub animation_progress: f32,
    /// Screen width
    pub screen_width: usize,
    /// Screen height
    pub screen_height: usize,
    /// Cursor position in query
    pub cursor_pos: usize,
    /// Show categories
    pub show_categories: bool,
}

impl Spotlight {
    pub fn new(screen_width: usize, screen_height: usize) -> Self {
        Spotlight {
            visible: false,
            query: String::new(),
            results: Vec::new(),
            selected_index: 0,
            index: SearchIndex::new(),
            animation_progress: 0.0,
            screen_width,
            screen_height,
            cursor_pos: 0,
            show_categories: true,
        }
    }
    
    /// Show spotlight
    pub fn show(&mut self) {
        self.visible = true;
        self.animation_progress = 0.0;
        self.query.clear();
        self.results.clear();
        self.selected_index = 0;
        self.cursor_pos = 0;
    }
    
    /// Hide spotlight
    pub fn hide(&mut self) {
        self.visible = false;
        self.animation_progress = 0.0;
    }
    
    /// Toggle visibility
    pub fn toggle(&mut self) {
        if self.visible {
            self.hide();
        } else {
            self.show();
        }
    }
    
    /// Update animation
    pub fn update(&mut self, dt: f32) {
        if self.visible && self.animation_progress < 1.0 {
            self.animation_progress = (self.animation_progress + dt * 8.0).min(1.0);
        } else if !self.visible && self.animation_progress > 0.0 {
            self.animation_progress = (self.animation_progress - dt * 8.0).max(0.0);
        }
    }

    pub fn is_animating(&self) -> bool {
        (self.visible && self.animation_progress < 1.0)
            || (!self.visible && self.animation_progress > 0.0)
    }

    pub fn needs_redraw(&self) -> bool {
        self.visible || self.is_animating()
    }
    
    /// Update search query
    pub fn set_query(&mut self, query: &str) {
        self.query = String::from(query);
        self.cursor_pos = query.len();
        self.results = self.index.search(query);
        self.selected_index = 0;
    }
    
    /// Handle key press
    pub fn on_key_press(&mut self, c: char) -> SpotlightEvent {
        if c == '\x1b' { // Escape
            self.hide();
            return SpotlightEvent::Cancelled;
        }
        
        if c == '\n' || c == '\r' { // Enter
            return self.activate_selected();
        }
        
        if c == '\x08' { // Backspace
            if self.cursor_pos > 0 {
                self.cursor_pos -= 1;
                self.query.remove(self.cursor_pos);
                self.results = self.index.search(&self.query);
                self.selected_index = 0;
            }
            return SpotlightEvent::None;
        }
        
        if !c.is_control() {
            self.query.insert(self.cursor_pos, c);
            self.cursor_pos += 1;
            self.results = self.index.search(&self.query);
            self.selected_index = 0;
        }
        
        SpotlightEvent::None
    }
    
    /// Handle special key
    pub fn on_special_key(&mut self, key: SpotlightKey) -> SpotlightEvent {
        match key {
            SpotlightKey::Up => {
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                }
                SpotlightEvent::None
            }
            SpotlightKey::Down => {
                if self.selected_index < self.results.len().saturating_sub(1) {
                    self.selected_index += 1;
                }
                SpotlightEvent::None
            }
            SpotlightKey::Escape => {
                self.hide();
                SpotlightEvent::Cancelled
            }
            SpotlightKey::Enter => {
                self.activate_selected()
            }
            SpotlightKey::Tab => {
                // Autocomplete - use first result
                if !self.results.is_empty() {
                    self.query = self.results[0].title.clone();
                    self.cursor_pos = self.query.len();
                }
                SpotlightEvent::None
            }
        }
    }
    
    fn activate_selected(&mut self) -> SpotlightEvent {
        if self.selected_index < self.results.len() {
            let result = self.results[self.selected_index].clone();
            self.hide();
            return SpotlightEvent::ResultSelected(result);
        }
        SpotlightEvent::None
    }
    
    /// Draw spotlight overlay
    pub fn draw(&self, fb: &mut Framebuffer) {
        if self.animation_progress <= 0.0 {
            return;
        }
        
        let progress = self.animation_progress;
        
        // Dim background
        let bg_alpha = (0.4 * progress) as f32;
        for y in 0..self.screen_height {
            for x in 0..self.screen_width {
                let ptr = unsafe { (fb.base_addr as *mut u32).add(y * fb.pixels_per_scan_line + x) };
                let bg = unsafe { *ptr };
                let dimmed = Self::blend_color(bg, 0x000000, bg_alpha);
                unsafe { *ptr = dimmed; }
            }
        }
        
        // Calculate search box position (centered, animated)
        let box_width = 600;
        let box_height = 44;
        let box_x = (self.screen_width - box_width) / 2;
        let box_y = (self.screen_height / 3) as f32;
        
        // Animate box sliding in
        let animated_y = (box_y + (1.0 - progress) * -50.0) as usize;
        
        // Draw search box background
        fb.draw_rect(box_x, animated_y, box_width, box_height, 0xE0FFFFFF);
        fb.draw_rect_outline(box_x, animated_y, box_width, box_height, 0x40888888);
        
        // Draw search icon
        fb.draw_string(box_x + 12, animated_y + 12, "🔍", 0xFF888888);
        
        // Draw query text
        let text_x = box_x + 40;
        if self.query.is_empty() {
            fb.draw_string(text_x, animated_y + 12, "Search apps, files, settings...", 0xFF888888);
        } else {
            fb.draw_string(text_x, animated_y + 12, &self.query, 0xFF333333);
            
            // Draw cursor
            let cursor_x = text_x + self.cursor_pos * 8;
            fb.draw_rect(cursor_x, animated_y + 10, 2, 24, 0xFF333333);
        }
        
        // Draw results
        if !self.results.is_empty() {
            let results_y = animated_y + box_height + 8;
            let result_height = 48;
            let max_visible = 8;
            let visible_count = self.results.len().min(max_visible);
            
            let results_width = box_width;
            let results_height = visible_count * result_height;
            
            // Results background
            fb.draw_rect(box_x, results_y, results_width, results_height, 0xE0FFFFFF);
            fb.draw_rect_outline(box_x, results_y, results_width, results_height, 0x40888888);
            
            for (i, result) in self.results.iter().take(max_visible).enumerate() {
                let item_y = results_y + i * result_height;
                let is_selected = i == self.selected_index;
                
                // Selection highlight
                if is_selected {
                    fb.draw_rect(box_x + 2, item_y + 2, results_width - 4, result_height - 4, Theme::ACCENT_PRIMARY.to_u32());
                }
                
                // Icon
                let icon = self.get_icon(result.icon);
                let icon_color = if is_selected { 0xFFFFFFFF } else { self.get_icon_color(result.result_type) };
                fb.draw_string(box_x + 12, item_y + 12, icon, icon_color);
                
                // Title
                let text_color = if is_selected { 0xFFFFFFFF } else { 0xFF333333 };
                fb.draw_string(box_x + 52, item_y + 8, &result.title, text_color);
                
                // Subtitle
                let sub_color = if is_selected { 0xFFCCCCCC } else { 0xFF888888 };
                fb.draw_string(box_x + 52, item_y + 24, &result.subtitle, sub_color);
                
                // Type badge
                let badge = self.get_type_badge(result.result_type);
                fb.draw_string(box_x + results_width - badge.len() * 8 - 12, item_y + 16, badge, sub_color);
            }
            
            // Show count if more results
            if self.results.len() > max_visible {
                let more = format!("+{} more", self.results.len() - max_visible);
                fb.draw_string(box_x + results_width - more.len() * 8 - 12, results_y + results_height + 4, &more, 0xFF888888);
            }
        }
        
        // Draw keyboard shortcuts hint
        let hint_y = animated_y + box_height + if !self.results.is_empty() { self.results.len().min(8) * 48 + 20 } else { 8 };
        fb.draw_string(box_x, hint_y, "↵ Select  ↑↓ Navigate  Tab Autocomplete  Esc Close", 0xFF888888);
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
    
    fn get_icon(&self, icon: SearchIcon) -> &'static str {
        match icon {
            SearchIcon::App => "📱",
            SearchIcon::File => "📄",
            SearchIcon::Folder => "📁",
            SearchIcon::Setting => "⚙",
            SearchIcon::Command => "⚡",
            SearchIcon::Contact => "👤",
            SearchIcon::Calendar => "📅",
            SearchIcon::Web => "🌐",
            SearchIcon::Calculator => "🔢",
            SearchIcon::Dictionary => "📖",
            SearchIcon::Custom(_) => "⬚",
        }
    }
    
    fn get_icon_color(&self, result_type: ResultType) -> u32 {
        match result_type {
            ResultType::Application => 0xFF007AFF,
            ResultType::File => 0xFF8E8E93,
            ResultType::Folder => 0xFF007AFF,
            ResultType::SystemCommand => 0xFFFF9500,
            ResultType::Setting => 0xFF8E8E93,
            ResultType::Contact => 0xFF34C759,
            ResultType::Calendar => 0xFFFF3B30,
            ResultType::WebSearch => 0xFF5856D6,
            ResultType::Calculator => 0xFFFF9500,
            ResultType::Dictionary => 0xFF8E8E93,
        }
    }
    
    fn get_type_badge(&self, result_type: ResultType) -> &'static str {
        match result_type {
            ResultType::Application => "App",
            ResultType::File => "File",
            ResultType::Folder => "Folder",
            ResultType::SystemCommand => "Command",
            ResultType::Setting => "Setting",
            ResultType::Contact => "Contact",
            ResultType::Calendar => "Event",
            ResultType::WebSearch => "Web",
            ResultType::Calculator => "Calc",
            ResultType::Dictionary => "Define",
        }
    }
    
    /// Handle mouse click
    pub fn on_click(&mut self, mx: i32, my: i32) -> SpotlightEvent {
        let box_width = 600;
        let box_x = (self.screen_width - box_width) / 2;
        let box_y = self.screen_height / 3;
        
        // Check if clicking on results
        let results_y = box_y + 44 + 8;
        let result_height = 48;
        
        if mx >= box_x as i32 && mx < (box_x + box_width) as i32 
            && my >= results_y as i32 {
            
            let idx = ((my - results_y as i32) / result_height as i32) as usize;
            if idx < self.results.len() {
                self.selected_index = idx;
                return self.activate_selected();
            }
        }
        
        // Click outside - close
        if my < box_y as i32 || my > (results_y + self.results.len().min(8) * result_height) as i32 {
            self.hide();
            return SpotlightEvent::Cancelled;
        }
        
        SpotlightEvent::None
    }
    
    /// Resize
    pub fn resize(&mut self, width: usize, height: usize) {
        self.screen_width = width;
        self.screen_height = height;
    }
}

/// Spotlight key codes
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpotlightKey {
    Up,
    Down,
    Escape,
    Enter,
    Tab,
}

/// Spotlight events
#[derive(Clone, Debug)]
pub enum SpotlightEvent {
    None,
    ResultSelected(SearchResult),
    Cancelled,
}

// ============================================================================
// GLOBAL SPOTLIGHT
// ============================================================================

lazy_static::lazy_static! {
    static ref SPOTLIGHT: Mutex<Spotlight> = Mutex::new(Spotlight::new(1920, 1080));
}

/// Initialize spotlight
pub fn init(width: usize, height: usize) {
    let mut spotlight = SPOTLIGHT.lock();
    spotlight.resize(width, height);
    spotlight.index.index_files();
    crate::serial_println!("[GUI] Spotlight initialized");
}

/// Get spotlight
pub fn get_spotlight() -> &'static Mutex<Spotlight> {
    &SPOTLIGHT
}
