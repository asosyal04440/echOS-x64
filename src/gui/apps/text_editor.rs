//! # Text Editor Application
//!
//! Simple text editor with syntax highlighting and basic editing
//! Supports search, replace, and multiple file formats

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use alloc::collections::VecDeque;
use spin::Mutex;

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};
use crate::gui::widgets::{Widget, Rect};

// ============================================================================
// TEXT BUFFER
// ============================================================================

/// Text buffer with line-based storage
pub struct TextBuffer {
    /// Lines of text
    lines: Vec<String>,
    /// Modified flag
    modified: bool,
    /// File path
    file_path: String,
    /// Undo stack
    undo_stack: VecDeque<EditAction>,
    /// Redo stack
    redo_stack: VecDeque<EditAction>,
    /// Max undo history
    max_undo: usize,
}

#[derive(Clone, Debug)]
pub struct EditAction {
    action_type: ActionType,
    line: usize,
    column: usize,
    text: String,
    old_text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionType {
    Insert,
    Delete,
    Replace,
}

impl TextBuffer {
    pub fn new() -> Self {
        TextBuffer {
            lines: vec![String::new()],
            modified: false,
            file_path: String::new(),
            undo_stack: VecDeque::new(),
            redo_stack: VecDeque::new(),
            max_undo: 100,
        }
    }
    
    /// Load from string
    pub fn load(&mut self, text: &str) {
        self.lines = text.lines().map(String::from).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.modified = false;
        self.undo_stack.clear();
        self.redo_stack.clear();
    }
    
    /// Load from file
    pub fn load_file(&mut self, path: &str) -> bool {
        self.file_path = String::from(path);
        
        // VFS not available in no_std yet
        false
    }
    
    /// Save to file
    pub fn save(&mut self) -> bool {
        if self.file_path.is_empty() {
            return false;
        }
        
        let path = self.file_path.clone();
        self.save_as(&path)
    }
    
    /// Save as new file
    pub fn save_as(&mut self, path: &str) -> bool {
        let text = self.to_string();
        
        // VFS not available in no_std yet
        false
    }
    
    /// Convert to string
    pub fn to_string(&self) -> String {
        self.lines.join("\n")
    }
    
    /// Get line count
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }
    
    /// Get line
    pub fn get_line(&self, line: usize) -> &str {
        self.lines.get(line).map(|s| s.as_str()).unwrap_or("")
    }
    
    /// Get line length
    pub fn line_length(&self, line: usize) -> usize {
        self.lines.get(line).map(|s| s.len()).unwrap_or(0)
    }
    
    /// Insert character
    pub fn insert_char(&mut self, line: usize, col: usize, c: char) {
        if line < self.lines.len() {
            let line_text = &mut self.lines[line];
            if col <= line_text.len() {
                line_text.insert(col, c);
                self.modified = true;
            }
        }
    }
    
    /// Insert string
    pub fn insert_str(&mut self, line: usize, col: usize, text: &str) {
        if line < self.lines.len() {
            let lines: Vec<&str> = text.split('\n').collect();
            
            if lines.len() == 1 {
                // Single line insert
                let max_col = self.lines[line].len();
                self.lines[line].insert_str(col.min(max_col), text);
            } else {
                // Multi-line insert
                let current_line = &mut self.lines[line];
                let after_cursor = current_line[col..].to_string();
                current_line.truncate(col);
                current_line.push_str(lines[0]);
                
                for i in 1..lines.len() - 1 {
                    self.lines.insert(line + i, String::from(lines[i]));
                }
                
                let last_line = format!("{}{}", lines.last().unwrap_or(&""), after_cursor);
                self.lines.insert(line + lines.len() - 1, last_line);
            }
            
            self.modified = true;
        }
    }
    
    /// Delete character
    pub fn delete_char(&mut self, line: usize, col: usize) {
        if line < self.lines.len() {
            let line_text = &mut self.lines[line];
            if col < line_text.len() {
                line_text.remove(col);
                self.modified = true;
            } else if line < self.lines.len() - 1 {
                // Join with next line
                let next_line = self.lines.remove(line + 1);
                self.lines[line].push_str(&next_line);
                self.modified = true;
            }
        }
    }
    
    /// Delete range
    pub fn delete_range(&mut self, start_line: usize, start_col: usize, end_line: usize, end_col: usize) {
        if start_line == end_line {
            if start_line < self.lines.len() {
                let line = &mut self.lines[start_line];
                line.replace_range(start_col..end_col.min(line.len()), "");
                self.modified = true;
            }
        } else if start_line < end_line {
            // Delete across lines
            let start = self.lines[start_line][..start_col].to_string();
            let end = if end_line < self.lines.len() {
                self.lines[end_line][end_col..].to_string()
            } else {
                String::new()
            };
            
            // Remove lines between
            for _ in start_line..end_line {
                self.lines.remove(start_line + 1);
            }
            
            // Merge
            self.lines[start_line] = format!("{}{}", start, end);
            self.modified = true;
        }
    }
    
    /// Insert new line
    pub fn insert_newline(&mut self, line: usize, col: usize) {
        if line < self.lines.len() {
            let current = &mut self.lines[line];
            let after = current[col..].to_string();
            current.truncate(col);
            
            self.lines.insert(line + 1, after);
            self.modified = true;
        }
    }
    
    /// Is modified
    pub fn is_modified(&self) -> bool {
        self.modified
    }
    
    /// Get file path
    pub fn file_path(&self) -> &str {
        &self.file_path
    }
    
    /// Undo last action
    pub fn undo(&mut self) -> Option<EditAction> {
        self.undo_stack.pop_back()
    }
    
    /// Redo last undone action
    pub fn redo(&mut self) -> Option<EditAction> {
        self.redo_stack.pop_back()
    }
}

impl Default for TextBuffer {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// CURSOR AND SELECTION
// ============================================================================

/// Cursor position and selection
pub struct Cursor {
    /// Line number (0-indexed)
    pub line: usize,
    /// Column number (0-indexed)
    pub column: usize,
    /// Desired column (for vertical movement)
    pub desired_column: usize,
    /// Selection anchor (if any)
    pub selection_anchor: Option<(usize, usize)>,
}

impl Cursor {
    pub fn new() -> Self {
        Cursor {
            line: 0,
            column: 0,
            desired_column: 0,
            selection_anchor: None,
        }
    }
    
    /// Move cursor up
    pub fn move_up(&mut self, buffer: &TextBuffer) {
        if self.line > 0 {
            self.line -= 1;
            self.column = self.desired_column.min(buffer.line_length(self.line));
        }
    }
    
    /// Move cursor down
    pub fn move_down(&mut self, buffer: &TextBuffer) {
        if self.line < buffer.line_count() - 1 {
            self.line += 1;
            self.column = self.desired_column.min(buffer.line_length(self.line));
        }
    }
    
    /// Move cursor left
    pub fn move_left(&mut self, buffer: &TextBuffer) {
        if self.column > 0 {
            self.column -= 1;
            self.desired_column = self.column;
        } else if self.line > 0 {
            self.line -= 1;
            self.column = buffer.line_length(self.line);
            self.desired_column = self.column;
        }
    }
    
    /// Move cursor right
    pub fn move_right(&mut self, buffer: &TextBuffer) {
        if self.column < buffer.line_length(self.line) {
            self.column += 1;
            self.desired_column = self.column;
        } else if self.line < buffer.line_count() - 1 {
            self.line += 1;
            self.column = 0;
            self.desired_column = 0;
        }
    }
    
    /// Move to line start
    pub fn move_home(&mut self) {
        self.column = 0;
        self.desired_column = 0;
    }
    
    /// Move to line end
    pub fn move_end(&mut self, buffer: &TextBuffer) {
        self.column = buffer.line_length(self.line);
        self.desired_column = self.column;
    }
    
    /// Start selection
    pub fn start_selection(&mut self) {
        self.selection_anchor = Some((self.line, self.column));
    }
    
    /// End selection
    pub fn end_selection(&mut self) {
        self.selection_anchor = None;
    }
    
    /// Has selection
    pub fn has_selection(&self) -> bool {
        self.selection_anchor.is_some()
    }
    
    /// Get selection range
    pub fn get_selection(&self) -> Option<((usize, usize), (usize, usize))> {
        self.selection_anchor.map(|(anchor_line, anchor_col)| {
            if (anchor_line, anchor_col) <= (self.line, self.column) {
                ((anchor_line, anchor_col), (self.line, self.column))
            } else {
                ((self.line, self.column), (anchor_line, anchor_col))
            }
        })
    }
    
    /// Set position
    pub fn set_position(&mut self, line: usize, column: usize) {
        self.line = line;
        self.column = column;
        self.desired_column = column;
    }
}

impl Default for Cursor {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// SYNTAX HIGHLIGHTING
// ============================================================================

/// Syntax highlighter
pub struct SyntaxHighlighter {
    language: Language,
    keywords: Vec<&'static str>,
    comment_start: &'static str,
    comment_end: &'static str,
    string_delimiters: (&'static str, &'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    PlainText,
    Rust,
    C,
    JavaScript,
    Python,
    Config,
}

impl Language {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "rs" => Language::Rust,
            "c" | "h" | "cpp" | "hpp" => Language::C,
            "js" | "ts" => Language::JavaScript,
            "py" => Language::Python,
            "cfg" | "conf" | "ini" | "toml" => Language::Config,
            _ => Language::PlainText,
        }
    }
}

impl SyntaxHighlighter {
    pub fn new(language: Language) -> Self {
        let (keywords, comment_start, comment_end, string_delims) = match language {
            Language::Rust => (
                vec!["fn", "let", "mut", "if", "else", "match", "struct", "enum", "impl", "pub", 
                     "use", "mod", "crate", "self", "super", "const", "static", "type", "trait",
                     "where", "for", "while", "loop", "break", "continue", "return", "as", "in",
                     "true", "false", "None", "Some", "Ok", "Err", "async", "await", "move"],
                "//", "\n", ("\"", "\"")
            ),
            Language::C => (
                vec!["int", "char", "void", "if", "else", "for", "while", "do", "switch", "case",
                     "break", "continue", "return", "struct", "typedef", "enum", "union",
                     "const", "static", "extern", "volatile", "sizeof", "NULL", "true", "false"],
                "//", "\n", ("\"", "\"")
            ),
            Language::JavaScript => (
                vec!["function", "var", "let", "const", "if", "else", "for", "while", "do",
                     "switch", "case", "break", "continue", "return", "class", "extends",
                     "new", "this", "super", "import", "export", "default", "async", "await",
                     "true", "false", "null", "undefined", "typeof", "instanceof"],
                "//", "\n", ("\"", "\"")
            ),
            Language::Python => (
                vec!["def", "class", "if", "elif", "else", "for", "while", "try", "except",
                     "finally", "with", "as", "import", "from", "return", "yield", "lambda",
                     "True", "False", "None", "and", "or", "not", "in", "is", "pass", "break",
                     "continue", "global", "nonlocal", "async", "await"],
                "#", "\n", ("\"", "\"")
            ),
            Language::Config => (
                vec!["true", "false", "yes", "no", "on", "off"],
                "#", "\n", ("\"", "\"")
            ),
            Language::PlainText => (
                vec![],
                "", "", ("", "")
            ),
        };
        
        SyntaxHighlighter {
            language,
            keywords,
            comment_start,
            comment_end,
            string_delimiters: string_delims,
        }
    }
    
    /// Get color for text segment
    pub fn highlight_line(&self, line: &str) -> Vec<(String, u32)> {
        let mut result = Vec::new();
        
        if self.language == Language::PlainText {
            result.push((String::from(line), Theme::TEXT_PRIMARY.to_u32()));
            return result;
        }
        
        let mut current = String::new();
        let mut in_string = false;
        let mut in_comment = false;
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        
        while i < chars.len() {
            // Check for comment start
            if !in_string && !in_comment && self.comment_start.len() > 0 {
                let remaining: String = chars[i..].iter().collect();
                if remaining.starts_with(self.comment_start) {
                    if !current.is_empty() {
                        result.push((current.clone(), Theme::TEXT_PRIMARY.to_u32()));
                        current.clear();
                    }
                    
                    let comment: String = chars[i..].iter().collect();
                    result.push((comment, Theme::TEXT_SECONDARY.to_u32()));
                    return result;
                }
            }
            
            // Check for string delimiters
            if !in_comment && self.string_delimiters.0.len() > 0 {
                if chars[i] == self.string_delimiters.0.chars().next().unwrap() {
                    if !current.is_empty() {
                        result.push((current.clone(), Theme::TEXT_PRIMARY.to_u32()));
                        current.clear();
                    }
                    
                    in_string = !in_string;
                    current.push(chars[i]);
                    i += 1;
                    continue;
                }
            }
            
            current.push(chars[i]);
            
            // Check for keywords
            if !in_string && !in_comment {
                // Check if current is a word
                let is_word_end = i + 1 >= chars.len() || !chars[i + 1].is_alphanumeric() && chars[i + 1] != '_';
                
                if is_word_end && self.keywords.contains(&current.as_str()) {
                    result.push((current.clone(), Theme::TEXT_ACCENT.to_u32()));
                    current.clear();
                }
            }
            
            i += 1;
        }
        
        if !current.is_empty() {
            let color = if in_string { Theme::TEXT_ACCENT.to_u32() } else { Theme::TEXT_PRIMARY.to_u32() };
            result.push((current, color));
        }
        
        result
    }
}

// ============================================================================
// TEXT EDITOR
// ============================================================================

/// Text Editor Application
pub struct TextEditor {
    /// Window position and size
    rect: Rect,
    /// Text buffer
    buffer: TextBuffer,
    /// Cursor
    cursor: Cursor,
    /// Syntax highlighter
    highlighter: SyntaxHighlighter,
    /// Scroll offset (lines)
    scroll_y: usize,
    /// Scroll offset (columns)
    scroll_x: usize,
    /// Tab size
    tab_size: usize,
    /// Show line numbers
    show_line_numbers: bool,
    /// Line number width
    line_number_width: usize,
    /// Font size
    font_size: usize,
    /// Line height
    line_height: usize,
    /// Search query
    search_query: String,
    /// Search results
    search_results: Vec<(usize, usize)>,
    /// Current search result index
    search_index: Option<usize>,
    /// Replace string
    replace_str: String,
    /// Show search bar
    show_search: bool,
    /// File modified
    file_modified: bool,
}

impl TextEditor {
    pub fn new() -> Self {
        TextEditor {
            rect: Rect::new(180, 80, 900, 600),
            buffer: TextBuffer::new(),
            cursor: Cursor::new(),
            highlighter: SyntaxHighlighter::new(Language::PlainText),
            scroll_y: 0,
            scroll_x: 0,
            tab_size: 4,
            show_line_numbers: true,
            line_number_width: 48,
            font_size: 14,
            line_height: 18,
            search_query: String::new(),
            search_results: Vec::new(),
            search_index: None,
            replace_str: String::new(),
            show_search: false,
            file_modified: false,
        }
    }
    
    /// Load file
    pub fn load_file(&mut self, path: &str) -> bool {
        if self.buffer.load_file(path) {
            // Set language based on extension
            let ext = path.rsplit('.').next().unwrap_or("");
            self.highlighter = SyntaxHighlighter::new(Language::from_extension(ext));
            self.cursor = Cursor::new();
            self.scroll_y = 0;
            self.scroll_x = 0;
            self.file_modified = false;
            true
        } else {
            false
        }
    }
    
    /// Save file
    pub fn save_file(&mut self) -> bool {
        if self.buffer.save() {
            self.file_modified = false;
            true
        } else {
            false
        }
    }
    
    /// Save as
    pub fn save_as(&mut self, path: &str) -> bool {
        if self.buffer.save_as(path) {
            self.file_modified = false;
            true
        } else {
            false
        }
    }
    
    /// Draw the editor
    pub fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let width = self.rect.width as usize;
        let height = self.rect.height as usize;
        
        // Window background
        fb.draw_rect(x, y, width, height, Theme::WINDOW_BG.to_u32());
        
        // Title bar
        fb.draw_rect(x, y, width, 32, Theme::TITLEBAR_BG.to_u32());
        
        let title = if self.buffer.file_path().is_empty() {
            String::from("Text Editor - Untitled")
        } else {
            let name = self.buffer.file_path().rsplit('/').next().unwrap_or(self.buffer.file_path());
            let modified = if self.buffer.is_modified() { " *" } else { "" };
            format!("Text Editor - {}{}", name, modified)
        };
        fb.draw_string(x + 12, y + 8, &title, Theme::TEXT_PRIMARY.to_u32());
        
        // Close button
        fb.draw_rect(x + width - 28, y + 4, 24, 24, Theme::ERROR.to_u32());
        fb.draw_string(x + width - 20, y + 8, "×", Theme::TEXT_ON_ACCENT.to_u32());
        
        // Menu bar
        let menu_y = y + 32;
        fb.draw_rect(x, menu_y, width, 24, Theme::TOOLBAR_BG.to_u32());
        
        let menus = ["File", "Edit", "View", "Search", "Help"];
        let mut menu_x = x + 8;
        for menu in &menus {
            fb.draw_string(menu_x, menu_y + 4, menu, Theme::TEXT_PRIMARY.to_u32());
            menu_x += menu.len() * 8 + 16;
        }
        
        // Search bar
        if self.show_search {
            let search_y = menu_y + 24;
            fb.draw_rect(x, search_y, width, 32, Theme::TOOLBAR_BG.to_u32());
            
            // Search input
            fb.draw_rect(x + 8, search_y + 4, 200, 24, Theme::INPUT_BG.to_u32());
            fb.draw_string(x + 12, search_y + 8, &self.search_query, Theme::TEXT_PRIMARY.to_u32());
            
            // Replace input
            fb.draw_rect(x + 220, search_y + 4, 200, 24, Theme::INPUT_BG.to_u32());
            fb.draw_string(x + 224, search_y + 8, &self.replace_str, Theme::TEXT_SECONDARY.to_u32());
            
            // Buttons
            fb.draw_string(x + 440, search_y + 8, "Find", Theme::ACCENT_PRIMARY.to_u32());
            fb.draw_string(x + 480, search_y + 8, "Replace", Theme::ACCENT_PRIMARY.to_u32());
            fb.draw_string(x + 540, search_y + 8, "Replace All", Theme::ACCENT_PRIMARY.to_u32());
        }
        
        // Line numbers
        let content_y = y + 32 + 24 + if self.show_search { 32 } else { 0 };
        let content_height = height - 32 - 24 - if self.show_search { 32 } else { 0 } - 24;
        
        if self.show_line_numbers {
            fb.draw_rect(x, content_y, self.line_number_width, content_height, Theme::SIDEBAR_BG.to_u32());
            
            // Draw line numbers
            let visible_lines = content_height / self.line_height;
            for i in 0..visible_lines {
                let line_num = self.scroll_y + i + 1;
                if line_num <= self.buffer.line_count() {
                    let num_str = format!("{:4}", line_num);
                    let num_y = content_y + i * self.line_height;
                    fb.draw_string(x + 4, num_y + 2, &num_str, Theme::TEXT_SECONDARY.to_u32());
                }
            }
        }
        
        // Editor content
        let text_x = x + if self.show_line_numbers { self.line_number_width } else { 0 };
        let text_width = width - if self.show_line_numbers { self.line_number_width } else { 0 };
        
        fb.draw_rect(text_x, content_y, text_width, content_height, Theme::WINDOW_BG.to_u32());
        
        // Draw text
        self.draw_text(fb, text_x, content_y, text_width, content_height);
        
        // Status bar
        let status_y = y + height - 24;
        fb.draw_rect(x, status_y, width, 24, Theme::TOOLBAR_BG.to_u32());
        
        let status = format!("Ln {}, Col {} | {} lines | {}",
            self.cursor.line + 1,
            self.cursor.column + 1,
            self.buffer.line_count(),
            self.highlighter.language_name()
        );
        fb.draw_string(x + 12, status_y + 4, &status, Theme::TEXT_SECONDARY.to_u32());
        
        // File status
        let file_status = if self.buffer.is_modified() { "Modified" } else { "Saved" };
        fb.draw_string(x + width - 100, status_y + 4, file_status, Theme::TEXT_SECONDARY.to_u32());
    }
    
    fn draw_text(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize) {
        let visible_lines = height / self.line_height;
        
        for i in 0..visible_lines {
            let line_idx = self.scroll_y + i;
            if line_idx >= self.buffer.line_count() {
                break;
            }
            
            let line = self.buffer.get_line(line_idx);
            let line_y = y + i * self.line_height;
            
            // Highlight line if cursor is on it
            if line_idx == self.cursor.line {
                fb.draw_rect(x, line_y, width, self.line_height, Theme::SELECTION_BG.to_u32());
            }
            
            // Highlight selection
            if let Some(((start_line, start_col), (end_line, end_col))) = self.cursor.get_selection() {
                if line_idx >= start_line && line_idx <= end_line {
                    let (sel_start, sel_end) = if start_line == end_line {
                        (start_col, end_col)
                    } else if line_idx == start_line {
                        (start_col, line.len())
                    } else if line_idx == end_line {
                        (0, end_col)
                    } else {
                        (0, line.len())
                    };
                    
                    let sel_x = x + sel_start * 8 - self.scroll_x * 8;
                    let sel_width = (sel_end - sel_start) * 8;
                    fb.draw_rect(sel_x, line_y, sel_width, self.line_height, Theme::SELECTION_BG.to_u32());
                }
            }
            
            // Draw syntax-highlighted text
            let highlighted = self.highlighter.highlight_line(line);
            let mut char_x = x;
            
            for (segment, color) in &highlighted {
                for c in segment.chars() {
                    if char_x >= x + width {
                        break;
                    }
                    
                    if char_x >= x {
                        fb.draw_char(char_x - self.scroll_x * 8, line_y + 2, c, *color);
                    }
                    char_x += 8;
                }
            }
            
            // Draw cursor
            if line_idx == self.cursor.line {
                let cursor_x = x + self.cursor.column * 8 - self.scroll_x * 8;
                if cursor_x >= x && cursor_x < x + width {
                    // Cursor line
                    fb.draw_rect(cursor_x, line_y, 2, self.line_height, Theme::TEXT_PRIMARY.to_u32());
                }
            }
        }
    }
    
    /// Handle key press
    pub fn on_key_press(&mut self, c: char, modifiers: u8) -> EditorAction {
        let ctrl = (modifiers & 0x01) != 0;
        let shift = (modifiers & 0x02) != 0;
        
        match c {
            '\n' | '\r' => {
                self.buffer.insert_newline(self.cursor.line, self.cursor.column);
                self.cursor.line += 1;
                self.cursor.column = 0;
                self.cursor.desired_column = 0;
                self.file_modified = true;
            }
            '\t' => {
                let spaces = self.tab_size - (self.cursor.column % self.tab_size);
                for _ in 0..spaces {
                    self.buffer.insert_char(self.cursor.line, self.cursor.column, ' ');
                    self.cursor.column += 1;
                }
                self.cursor.desired_column = self.cursor.column;
                self.file_modified = true;
            }
            '\x08' => {
                // Backspace
                if self.cursor.column > 0 {
                    self.cursor.column -= 1;
                    self.buffer.delete_char(self.cursor.line, self.cursor.column);
                    self.cursor.desired_column = self.cursor.column;
                } else if self.cursor.line > 0 {
                    self.cursor.line -= 1;
                    self.cursor.column = self.buffer.line_length(self.cursor.line);
                    self.buffer.delete_char(self.cursor.line, self.cursor.column);
                    self.cursor.desired_column = self.cursor.column;
                }
                self.file_modified = true;
            }
            '\x7F' => {
                // Delete
                self.buffer.delete_char(self.cursor.line, self.cursor.column);
                self.file_modified = true;
            }
            c if !c.is_control() => {
                if ctrl {
                    match c {
                        's' | 'S' => {
                            return EditorAction::Save;
                        }
                        'f' | 'F' => {
                            self.show_search = !self.show_search;
                            return EditorAction::None;
                        }
                        'z' | 'Z' => {
                            if let Some(_action) = self.buffer.undo() {
                                // Apply undo
                            }
                            return EditorAction::None;
                        }
                        'y' | 'Y' => {
                            if let Some(_action) = self.buffer.redo() {
                                // Apply redo
                            }
                            return EditorAction::None;
                        }
                        'a' | 'A' => {
                            // Select all
                            self.cursor.set_position(0, 0);
                            self.cursor.selection_anchor = Some((0, 0));
                            self.cursor.line = self.buffer.line_count() - 1;
                            self.cursor.column = self.buffer.line_length(self.cursor.line);
                            return EditorAction::None;
                        }
                        _ => {}
                    }
                } else {
                    self.buffer.insert_char(self.cursor.line, self.cursor.column, c);
                    self.cursor.column += 1;
                    self.cursor.desired_column = self.cursor.column;
                    self.file_modified = true;
                }
            }
            _ => {}
        }
        
        // Ensure cursor visible
        self.ensure_cursor_visible();
        
        EditorAction::None
    }
    
    /// Handle special key
    pub fn on_special_key(&mut self, key: SpecialKey, shift: bool) -> EditorAction {
        if shift && !self.cursor.has_selection() {
            self.cursor.start_selection();
        } else if !shift {
            self.cursor.end_selection();
        }
        
        match key {
            SpecialKey::Up => self.cursor.move_up(&self.buffer),
            SpecialKey::Down => self.cursor.move_down(&self.buffer),
            SpecialKey::Left => self.cursor.move_left(&self.buffer),
            SpecialKey::Right => self.cursor.move_right(&self.buffer),
            SpecialKey::Home => self.cursor.move_home(),
            SpecialKey::End => self.cursor.move_end(&self.buffer),
            SpecialKey::PageUp => {
                let page_size = 20;
                for _ in 0..page_size {
                    self.cursor.move_up(&self.buffer);
                }
            }
            SpecialKey::PageDown => {
                let page_size = 20;
                for _ in 0..page_size {
                    self.cursor.move_down(&self.buffer);
                }
            }
            _ => {}
        }
        
        self.ensure_cursor_visible();
        EditorAction::None
    }
    
    /// Ensure cursor is visible
    fn ensure_cursor_visible(&mut self) {
        let visible_lines = ((self.rect.height - 80) / self.line_height as i32) as usize;
        
        if self.cursor.line < self.scroll_y {
            self.scroll_y = self.cursor.line;
        } else if self.cursor.line >= self.scroll_y + visible_lines {
            self.scroll_y = self.cursor.line - visible_lines + 1;
        }
        
        let visible_cols = ((self.rect.width - self.line_number_width as i32) / 8) as usize;
        
        if self.cursor.column < self.scroll_x {
            self.scroll_x = self.cursor.column;
        } else if self.cursor.column >= self.scroll_x + visible_cols {
            self.scroll_x = self.cursor.column - visible_cols + 1;
        }
    }
    
    /// Handle mouse click
    pub fn on_click(&mut self, mx: i32, my: i32) -> EditorAction {
        // Close button
        let close_x = self.rect.x + self.rect.width - 28;
        if mx >= close_x && mx < close_x + 24 && my >= self.rect.y + 4 && my < self.rect.y + 28 {
            return EditorAction::Close;
        }
        
        // Text area click
        let text_x = self.rect.x + if self.show_line_numbers { self.line_number_width as i32 } else { 0 };
        let text_y = self.rect.y + 56 + if self.show_search { 32 } else { 0 };
        
        if mx >= text_x && my >= text_y {
            let line = self.scroll_y + ((my - text_y) as usize / self.line_height);
            let column = self.scroll_x + ((mx - text_x) as usize / 8);
            
            if line < self.buffer.line_count() {
                self.cursor.set_position(line, column.min(self.buffer.line_length(line)));
            }
        }
        
        EditorAction::None
    }
    
    /// Handle scroll
    pub fn on_scroll(&mut self, delta: i32) {
        if delta > 0 {
            self.scroll_y = self.scroll_y.saturating_add(delta as usize);
        } else {
            self.scroll_y = self.scroll_y.saturating_sub((-delta) as usize);
        }
        
        self.scroll_y = self.scroll_y.min(self.buffer.line_count().saturating_sub(1));
    }
    
    /// Search
    pub fn search(&mut self, query: &str) {
        self.search_query = String::from(query);
        self.search_results.clear();
        
        if query.is_empty() {
            return;
        }
        
        for (line_idx, line) in self.buffer.lines.iter().enumerate() {
            let mut start = 0;
            while let Some(pos) = line[start..].find(query) {
                self.search_results.push((line_idx, start + pos));
                start = start + pos + query.len();
            }
        }
        
        self.search_index = if !self.search_results.is_empty() { Some(0) } else { None };
        
        // Jump to first result
        if let Some(idx) = self.search_index {
            let (line, col) = self.search_results[idx];
            self.cursor.set_position(line, col);
            self.ensure_cursor_visible();
        }
    }
    
    /// Replace next
    pub fn replace_next(&mut self) {
        if let Some(idx) = self.search_index {
            let (line, col) = self.search_results[idx];
            let len = self.search_query.len();
            
            self.buffer.delete_range(line, col, line, col + len);
            self.buffer.insert_str(line, col, &self.replace_str);
            
            // Update search results
            let query = self.search_query.clone();
            self.search(&query);
        }
    }
    
    /// Replace all
    pub fn replace_all(&mut self) {
        while !self.search_results.is_empty() {
            self.replace_next();
        }
    }
    
    /// Get rect
    pub fn rect(&self) -> Rect {
        self.rect
    }
    
    /// Set rect
    pub fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }
    
    /// Get file path
    pub fn file_path(&self) -> &str {
        self.buffer.file_path()
    }
    
    /// Is modified
    pub fn is_modified(&self) -> bool {
        self.buffer.is_modified()
    }
}

/// Special keys
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialKey {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    Escape,
    Tab,
    Enter,
}

/// Editor actions
#[derive(Clone, Debug)]
pub enum EditorAction {
    None,
    Close,
    Save,
    SaveAs(String),
    Open(String),
    New,
}

impl SyntaxHighlighter {
    fn language_name(&self) -> &'static str {
        match self.language {
            Language::Rust => "Rust",
            Language::C => "C/C++",
            Language::JavaScript => "JavaScript",
            Language::Python => "Python",
            Language::Config => "Config",
            Language::PlainText => "Plain Text",
        }
    }
}

impl Default for TextEditor {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GLOBAL TEXT EDITOR
// ============================================================================

lazy_static::lazy_static! {
    static ref TEXT_EDITOR: Mutex<TextEditor> = Mutex::new(TextEditor::new());
}

/// Get text editor
pub fn get_editor() -> &'static Mutex<TextEditor> {
    &TEXT_EDITOR
}

/// Initialize text editor
pub fn init() {
    crate::serial_println!("[GUI] Text Editor initialized");
}
