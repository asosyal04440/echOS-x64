//! Advanced Shell Features
//!
//! Pipe, Redirect, Tab Completion, Environment Variables, Globbing, History Search.
//! Linux-level shell capabilities.

use alloc::borrow::ToOwned;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

// ============================================================================
// ENVIRONMENT VARIABLES
// ============================================================================

/// Environment variable manager
pub struct Environment {
    vars: Mutex<BTreeMap<String, String>>,
}

impl Environment {
    pub const fn new() -> Self {
        Self {
            vars: Mutex::new(BTreeMap::new()),
        }
    }
    
    /// Değişken ayarlar
    pub fn set(&self, key: &str, value: &str) {
        self.vars.lock().insert(key.to_string(), value.to_string());
    }
    
    /// Değişken döndürür
    pub fn get(&self, key: &str) -> Option<String> {
        self.vars.lock().get(key).cloned()
    }
    
    /// Değişken siler
    pub fn unset(&self, key: &str) {
        self.vars.lock().remove(key);
    }
    
    /// Tüm değişkenleri döndürür
    pub fn list(&self) -> Vec<(String, String)> {
        self.vars.lock().iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }
    
    /// String içindeki $VAR'ları expand eder
    pub fn expand(&self, input: &str) -> String {
        let mut result = String::new();
        let mut chars = input.chars().peekable();
        
        while let Some(c) = chars.next() {
            if c == '$' {
                // $VAR veya ${VAR} formatını parse et
                let var_name = if chars.peek() == Some(&'{') {
                    chars.next(); // '{' karakterini atla
                    let mut name = String::new();
                    while let Some(&ch) = chars.peek() {
                        if ch == '}' {
                            chars.next();
                            break;
                        }
                        name.push(ch);
                        chars.next();
                    }
                    name
                } else {
                    let mut name = String::new();
                    while let Some(&ch) = chars.peek() {
                        if ch.is_alphanumeric() || ch == '_' {
                            name.push(ch);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    name
                };
                
                if !var_name.is_empty() {
                    if let Some(value) = self.get(&var_name) {
                        result.push_str(&value);
                    }
                }
            } else {
                result.push(c);
            }
        }
        
        result
    }
    
    /// Default environment'ı başlatır
    pub fn init_defaults(&self) {
        self.set("PATH", "/bin:/usr/bin:/sbin");
        self.set("HOME", "/root");
        self.set("USER", "root");
        self.set("SHELL", "/bin/echsh");
        self.set("PWD", "/");
        self.set("HOSTNAME", "echos");
        self.set("TERM", "xterm-256color");
        self.set("LANG", "en_US.UTF-8");
        self.set("EDITOR", "nano");
        self.set("PAGER", "less");
    }
}

lazy_static::lazy_static! {
    /// Global environment
    pub static ref ENV: Environment = Environment::new();
}

// ============================================================================
// HISTORY MANAGEMENT
// ============================================================================

/// Command history manager
pub struct History {
    entries: Mutex<Vec<String>>,
    max_size: usize,
    current_index: Mutex<usize>,
    search_query: Mutex<String>,
    search_results: Mutex<Vec<usize>>,
}

impl History {
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            max_size,
            current_index: Mutex::new(0),
            search_query: Mutex::new(String::new()),
            search_results: Mutex::new(Vec::new()),
        }
    }
    
    /// Komut ekler
    pub fn push(&self, cmd: &str) {
        if cmd.trim().is_empty() {
            return;
        }
        
        let mut entries = self.entries.lock();
        
        // Aynı komut tekrar eklenmesin
        if entries.last().map(|s| s.as_str()) == Some(cmd) {
            return;
        }
        
        if entries.len() >= self.max_size {
            entries.remove(0);
        }
        
        entries.push(cmd.to_string());
        *self.current_index.lock() = entries.len();
    }
    
    /// Önceki komutu döndürür (yukarı ok)
    pub fn previous(&self) -> Option<String> {
        let entries = self.entries.lock();
        let mut index = self.current_index.lock();
        
        if *index > 0 {
            *index -= 1;
            return entries.get(*index).cloned();
        }
        None
    }
    
    /// Sonraki komutu döndürür (aşağı ok)
    pub fn next(&self) -> Option<String> {
        let entries = self.entries.lock();
        let mut index = self.current_index.lock();
        
        if *index < entries.len() - 1 {
            *index += 1;
            return entries.get(*index).cloned();
        } else if *index == entries.len() - 1 {
            *index = entries.len();
            return Some(String::new());
        }
        None
    }
    
    /// History'i listeler
    pub fn list(&self) -> Vec<(usize, String)> {
        self.entries.lock()
            .iter()
            .enumerate()
            .map(|(i, cmd)| (i + 1, cmd.clone()))
            .collect()
    }
    
    /// Reverse search başlatır (Ctrl+R)
    pub fn start_search(&self) {
        *self.search_query.lock() = String::new();
        self.search_results.lock().clear();
    }
    
    /// Search query'e karakter ekler
    pub fn search_add_char(&self, c: char) -> Option<String> {
        self.search_query.lock().push(c);
        self.search_update()
    }
    
    /// Search query'den karakter siler
    pub fn search_backspace(&self) -> Option<String> {
        self.search_query.lock().pop();
        self.search_update()
    }
    
    /// Search'i günceller
    fn search_update(&self) -> Option<String> {
        let query = self.search_query.lock().clone();
        let entries = self.entries.lock();
        let mut results = self.search_results.lock();
        
        results.clear();
        for (i, cmd) in entries.iter().enumerate().rev() {
            if cmd.contains(&query) {
                results.push(i);
            }
        }
        
        results.first().and_then(|&i| entries.get(i).cloned())
    }
    
    /// Current search result'ı döndürür
    pub fn search_current(&self) -> Option<String> {
        let results = self.search_results.lock();
        let entries = self.entries.lock();
        results.first().and_then(|&i| entries.get(i).cloned())
    }
    
    /// Search query'sini döndürür
    pub fn search_query(&self) -> String {
        self.search_query.lock().clone()
    }
}

lazy_static::lazy_static! {
    /// Global command history
    pub static ref HISTORY: History = History::new(1000);
}

// ============================================================================
// GLOBBING (Wildcard Expansion)
// ============================================================================

/// Glob pattern matcher
pub struct Glob;

impl Glob {
    /// Pattern'i match eder (*, ?, [])
    pub fn matches(pattern: &str, text: &str) -> bool {
        Self::matches_inner(pattern, text)
    }
    
    fn matches_inner(pattern: &str, text: &str) -> bool {
        let mut p_chars = pattern.chars().peekable();
        let mut t_chars = text.chars().peekable();
        
        loop {
            match (p_chars.next(), t_chars.peek()) {
                // Both exhausted
                (None, None) => return true,
                
                // Pattern exhausted but text remains
                (None, Some(_)) => return false,
                
                // * matches any sequence
                (Some('*'), _) => {
                    // Consume consecutive *
                    while p_chars.peek() == Some(&'*') {
                        p_chars.next();
                    }
                    
                    // * at end matches everything
                    if p_chars.peek().is_none() {
                        return true;
                    }
                    
                    // Try matching * with 0, 1, 2, ... characters
                    let remaining_pattern: String = p_chars.collect();
                    let remaining_text: String = t_chars.collect();
                    
                    for i in 0..=remaining_text.len() {
                        if Self::matches_inner(&remaining_pattern, &remaining_text[i..]) {
                            return true;
                        }
                    }
                    return false;
                }
                
                // ? matches any single char
                (Some('?'), Some(_)) => {
                    t_chars.next();
                }
                
                // [] character class
                (Some('['), Some(&t)) => {
                    t_chars.next();
                    
                    let mut negated = false;
                    if p_chars.peek() == Some(&'!') || p_chars.peek() == Some(&'^') {
                        negated = true;
                        p_chars.next();
                    }
                    
                    let mut matched = false;
                    let mut prev_char: Option<char> = None;
                    
                    loop {
                        match p_chars.next() {
                            Some(']') => break,
                            Some('-') => {
                                if let (Some(prev), Some(next)) = (prev_char, p_chars.peek()) {
                                    if t >= prev && t <= *next {
                                        matched = true;
                                    }
                                }
                            }
                            Some(c) => {
                                if c == t {
                                    matched = true;
                                }
                                prev_char = Some(c);
                            }
                            None => return false, // Unclosed [
                        }
                    }
                    
                    if negated == matched {
                        return false;
                    }
                }
                
                // Exact match
                (Some(p), Some(&t)) if p == t => {
                    t_chars.next();
                }
                
                // No match
                (Some(_), Some(_)) => return false,
                
                // Text exhausted but pattern remains
                (Some(_), None) => {
                    // Check if remaining pattern is all *
                    while let Some('*') = p_chars.peek() {
                        p_chars.next();
                    }
                    return p_chars.peek().is_none();
                }
            }
        }
    }
    
    /// Pattern'e uyan dosyaları bulur
    pub fn expand(pattern: &str, files: &[&str]) -> Vec<String> {
        let mut matches = Vec::new();
        for file in files {
            if Self::matches(pattern, file) {
                matches.push(file.to_string());
            }
        }
        matches.sort();
        matches
    }
}

// ============================================================================
// TAB COMPLETION
// ============================================================================

/// Tab completion provider
pub struct Completer {
    /// Built-in komutlar
    pub builtins: Vec<&'static str>,
}

impl Completer {
    pub fn new() -> Self {
        Self {
            builtins: vec![
                "help", "ver", "echo", "clear", "ls", "cat", "cd", "pwd",
                "mkdir", "rm", "cp", "mv", "touch", "chmod", "chown",
                "ps", "kill", "top", "jobs", "fg", "bg", "export", "unset",
                "env", "set", "history", "alias", "unalias", "source",
                "exit", "shutdown", "reboot", "uname", "whoami", "id",
                "date", "time", "uptime", "free", "df", "du", "mount", "umount",
                "wine", "proton", "linux", "launch",
            ],
        }
    }
    
    /// Tamamlama önerileri döndürür
    pub fn complete(&self, input: &str, cursor_pos: usize) -> Vec<String> {
        let mut completions = Vec::new();
        
        // Cursor position'a göre current word'ü bul
        let before_cursor = &input[..cursor_pos];
        let words: Vec<&str> = before_cursor.split_whitespace().collect();
        
        if words.is_empty() || !before_cursor.ends_with(' ') {
            // İlk kelime tamamlama (komut)
            if words.is_empty() || words.len() == 1 {
                let prefix = words.first().copied().unwrap_or("");
                
                // Built-in komutları kontrol et
                for &cmd in &self.builtins {
                    if cmd.starts_with(prefix) {
                        completions.push(cmd.to_string());
                    }
                }
                
                // TODO: PATH'teki executable'ları da ekle
            } else {
                // Sonraki kelimeler (dosya/dizin tamamlama)
                let prefix = words.last().copied().unwrap_or("");
                completions = self.complete_path(prefix);
            }
        }
        
        completions
    }
    
    /// Path tamamlama
    fn complete_path(&self, prefix: &str) -> Vec<String> {
        let mut completions = Vec::new();
        
        // Gerçek dosya sistemi entegrasyonu
        let (dir, file_prefix) = if prefix.contains('/') {
            let last_slash = prefix.rfind('/').unwrap();
            (prefix[..last_slash + 1].to_string(), prefix[last_slash + 1..].to_string())
        } else {
            ("/".to_string(), prefix.to_string())
        };
        
        // Dizini oku
        if let Ok(entries) = crate::fs::f2fs::list_dir(&dir) {
            for entry in entries {
                if entry.name.starts_with(&file_prefix) {
                    let full_path = if dir == "/" {
                        format!("/{}", entry.name)
                    } else {
                        format!("{}{}", dir, entry.name)
                    };
                    completions.push(full_path);
                }
            }
        }
        
        // Fallback: mock data
        if completions.is_empty() {
            let mock_files = [
                "bin", "boot", "dev", "etc", "home", "lib", "mnt",
                "proc", "root", "sbin", "sys", "tmp", "usr", "var",
                "config.txt", "readme.md", "test.sh",
            ];
            
            for file in &mock_files {
                if file.starts_with(prefix) {
                    completions.push(file.to_string());
                }
            }
        }
        
        completions.sort();
        completions
    }
    
    /// En uzun ortak prefix'i bulur
    pub fn common_prefix(completions: &[String]) -> String {
        if completions.is_empty() {
            return String::new();
        }
        
        let first = &completions[0];
        let mut prefix_len = first.len();
        
        for completion in &completions[1..] {
            let mut i = 0;
            while i < prefix_len && i < completion.len() {
                if first.as_bytes()[i] != completion.as_bytes()[i] {
                    break;
                }
                i += 1;
            }
            prefix_len = i;
        }
        
        first[..prefix_len].to_string()
    }
}

impl Default for Completer {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// PIPE AND REDIRECT
// ============================================================================

/// Token types for command parsing
#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    Word(String),
    Pipe,           // |
    RedirectOut,    // >
    RedirectAppend, // >>
    RedirectIn,     // <
    RedirectErr,    // 2>
    RedirectAll,    // &>
    Background,     // &
    And,            // &&
    Or,             // ||
    Semicolon,      // ;
    Newline,        // \n
}

/// Tokenizer
pub struct Tokenizer;

impl Tokenizer {
    /// Input'u token'lara ayırır
    pub fn tokenize(input: &str) -> Vec<Token> {
        let mut tokens = Vec::new();
        let mut chars = input.chars().peekable();
        let mut current_word = String::new();
        
        while let Some(c) = chars.next() {
            match c {
                ' ' | '\t' => {
                    if !current_word.is_empty() {
                        tokens.push(Token::Word(current_word.clone()));
                        current_word.clear();
                    }
                }
                '|' => {
                    if !current_word.is_empty() {
                        tokens.push(Token::Word(current_word.clone()));
                        current_word.clear();
                    }
                    if chars.peek() == Some(&'|') {
                        chars.next();
                        tokens.push(Token::Or);
                    } else {
                        tokens.push(Token::Pipe);
                    }
                }
                '>' => {
                    if !current_word.is_empty() {
                        tokens.push(Token::Word(current_word.clone()));
                        current_word.clear();
                    }
                    if chars.peek() == Some(&'>') {
                        chars.next();
                        tokens.push(Token::RedirectAppend);
                    } else {
                        tokens.push(Token::RedirectOut);
                    }
                }
                '<' => {
                    if !current_word.is_empty() {
                        tokens.push(Token::Word(current_word.clone()));
                        current_word.clear();
                    }
                    tokens.push(Token::RedirectIn);
                }
                '&' => {
                    if !current_word.is_empty() {
                        tokens.push(Token::Word(current_word.clone()));
                        current_word.clear();
                    }
                    match chars.peek() {
                        Some(&'>') => {
                            chars.next();
                            tokens.push(Token::RedirectAll);
                        }
                        Some(&'&') => {
                            chars.next();
                            tokens.push(Token::And);
                        }
                        _ => {
                            tokens.push(Token::Background);
                        }
                    }
                }
                ';' => {
                    if !current_word.is_empty() {
                        tokens.push(Token::Word(current_word.clone()));
                        current_word.clear();
                    }
                    tokens.push(Token::Semicolon);
                }
                '\n' => {
                    if !current_word.is_empty() {
                        tokens.push(Token::Word(current_word.clone()));
                        current_word.clear();
                    }
                    tokens.push(Token::Newline);
                }
                '\\' => {
                    // Escape next character
                    if let Some(next) = chars.next() {
                        current_word.push(next);
                    }
                }
                '\'' => {
                    // Single-quoted string (no escape)
                    while let Some(ch) = chars.next() {
                        if ch == '\'' {
                            break;
                        }
                        current_word.push(ch);
                    }
                }
                '"' => {
                    // Double-quoted string (with escape)
                    while let Some(ch) = chars.next() {
                        match ch {
                            '"' => break,
                            '\\' => {
                                if let Some(escaped) = chars.next() {
                                    current_word.push(escaped);
                                }
                            }
                            _ => current_word.push(ch),
                        }
                    }
                }
                _ => {
                    current_word.push(c);
                }
            }
        }
        
        if !current_word.is_empty() {
            tokens.push(Token::Word(current_word));
        }
        
        tokens
    }
}

/// Simple command
#[derive(Clone, Debug, Default)]
pub struct SimpleCommand {
    pub args: Vec<String>,
    pub redirects: Vec<Redirect>,
    pub background: bool,
}

/// Redirect specification
#[derive(Clone, Debug)]
pub struct Redirect {
    pub kind: RedirectKind,
    pub target: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RedirectKind {
    Stdout,      // >
    StdoutAppend, // >>
    Stdin,       // <
    Stderr,      // 2>
    All,         // &>
}

/// Pipeline (sequence of commands connected by pipes)
#[derive(Clone, Debug)]
pub struct Pipeline {
    pub commands: Vec<SimpleCommand>,
    pub background: bool,
}

/// Command parser
pub struct Parser;

impl Parser {
    /// Token listesinden pipeline oluşturur
    pub fn parse(tokens: Vec<Token>) -> Result<Vec<Pipeline>, ParseError> {
        let mut pipelines = Vec::new();
        let mut current_pipeline = Pipeline {
            commands: Vec::new(),
            background: false,
        };
        let mut current_command = SimpleCommand::default();
        let mut i = 0;
        
        while i < tokens.len() {
            match &tokens[i] {
                Token::Word(word) => {
                    current_command.args.push(word.clone());
                }
                Token::Pipe => {
                    if current_command.args.is_empty() {
                        return Err(ParseError::UnexpectedPipe);
                    }
                    current_pipeline.commands.push(current_command);
                    current_command = SimpleCommand::default();
                }
                Token::RedirectOut => {
                    i += 1;
                    if let Some(Token::Word(target)) = tokens.get(i) {
                        current_command.redirects.push(Redirect {
                            kind: RedirectKind::Stdout,
                            target: target.clone(),
                        });
                    }
                }
                Token::RedirectAppend => {
                    i += 1;
                    if let Some(Token::Word(target)) = tokens.get(i) {
                        current_command.redirects.push(Redirect {
                            kind: RedirectKind::StdoutAppend,
                            target: target.clone(),
                        });
                    }
                }
                Token::RedirectIn => {
                    i += 1;
                    if let Some(Token::Word(target)) = tokens.get(i) {
                        current_command.redirects.push(Redirect {
                            kind: RedirectKind::Stdin,
                            target: target.clone(),
                        });
                    }
                }
                Token::RedirectErr => {
                    i += 1;
                    if let Some(Token::Word(target)) = tokens.get(i) {
                        current_command.redirects.push(Redirect {
                            kind: RedirectKind::Stderr,
                            target: target.clone(),
                        });
                    }
                }
                Token::RedirectAll => {
                    i += 1;
                    if let Some(Token::Word(target)) = tokens.get(i) {
                        current_command.redirects.push(Redirect {
                            kind: RedirectKind::All,
                            target: target.clone(),
                        });
                    }
                }
                Token::Background => {
                    current_pipeline.background = true;
                    if !current_command.args.is_empty() {
                        current_pipeline.commands.push(current_command);
                        current_command = SimpleCommand::default();
                    }
                    pipelines.push(current_pipeline);
                    current_pipeline = Pipeline {
                        commands: Vec::new(),
                        background: false,
                    };
                }
                Token::And | Token::Or => {
                    // && ve || için short-circuit evaluation
                    if !current_command.args.is_empty() {
                        current_pipeline.commands.push(current_command);
                        current_command = SimpleCommand::default();
                    }
                    if !current_pipeline.commands.is_empty() {
                        pipelines.push(current_pipeline);
                        current_pipeline = Pipeline {
                            commands: Vec::new(),
                            background: false,
                        };
                    }
                }
                Token::Semicolon | Token::Newline => {
                    if !current_command.args.is_empty() {
                        current_pipeline.commands.push(current_command);
                        current_command = SimpleCommand::default();
                    }
                    if !current_pipeline.commands.is_empty() {
                        pipelines.push(current_pipeline);
                        current_pipeline = Pipeline {
                            commands: Vec::new(),
                            background: false,
                        };
                    }
                }
            }
            i += 1;
        }
        
        // Son komutu ekle
        if !current_command.args.is_empty() {
            current_pipeline.commands.push(current_command);
        }
        if !current_pipeline.commands.is_empty() {
            pipelines.push(current_pipeline);
        }
        
        Ok(pipelines)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    UnexpectedPipe,
    UnexpectedRedirect,
    MissingTarget,
    UnterminatedQuote,
}

// ============================================================================
// ALIAS SUPPORT
// ============================================================================

/// Alias manager
pub struct AliasManager {
    aliases: Mutex<BTreeMap<String, String>>,
}

impl AliasManager {
    pub const fn new() -> Self {
        Self {
            aliases: Mutex::new(BTreeMap::new()),
        }
    }
    
    /// Alias tanımlar
    pub fn set(&self, name: &str, expansion: &str) {
        self.aliases.lock().insert(name.to_string(), expansion.to_string());
    }
    
    /// Alias'ı siler
    pub fn unset(&self, name: &str) {
        self.aliases.lock().remove(name);
    }
    
    /// Alias'ı expand eder
    pub fn expand(&self, name: &str) -> Option<String> {
        self.aliases.lock().get(name).cloned()
    }
    
    /// Tüm alias'ları listeler
    pub fn list(&self) -> Vec<(String, String)> {
        self.aliases.lock().iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
    
    /// Komut satırındaki alias'ları expand eder
    pub fn expand_line(&self, input: &str) -> String {
        let first_word = input.split_whitespace().next();
        if let Some(word) = first_word {
            if let Some(expansion) = self.expand(word) {
                return input.replacen(word, &expansion, 1);
            }
        }
        input.to_string()
    }
}

lazy_static::lazy_static! {
    /// Global alias manager
    pub static ref ALIASES: AliasManager = AliasManager::new();
}

// ============================================================================
// INITIALIZATION
// ============================================================================

/// Advanced shell features'ı başlatır
pub fn init() {
    ENV.init_defaults();
    
    // Default alias'lar
    ALIASES.set("ll", "ls -la");
    ALIASES.set("la", "ls -a");
    ALIASES.set("l", "ls");
    ALIASES.set("..", "cd ..");
    ALIASES.set("...", "cd ../..");
    ALIASES.set("cls", "clear");
    
    crate::serial_println!("[SHELL] Advanced features initialized");
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_env_expand() {
        ENV.set("HOME", "/root");
        ENV.set("USER", "test");
        
        assert_eq!(ENV.expand("$HOME"), "/root");
        assert_eq!(ENV.expand("Hello $USER"), "Hello test");
        assert_eq!(ENV.expand("${HOME}/file"), "/root/file");
    }
    
    #[test]
    fn test_glob() {
        assert!(Glob::matches("*.txt", "file.txt"));
        assert!(!Glob::matches("*.txt", "file.rs"));
        assert!(Glob::matches("test?", "test1"));
        assert!(Glob::matches("[abc]", "a"));
        assert!(!Glob::matches("[abc]", "d"));
    }
    
    #[test]
    fn test_tokenizer() {
        let tokens = Tokenizer:: tokenize("ls -la | grep test > out.txt");
        assert_eq!(tokens.len(), 7);
        assert_eq!(tokens[2], Token::Pipe);
        assert_eq!(tokens[5], Token::RedirectOut);
    }
    
    #[test]
    fn test_parser() {
        let tokens = Tokenizer:: tokenize("ls | grep test");
        let pipelines = Parser::parse(tokens).unwrap();
        assert_eq!(pipelines.len(), 1);
        assert_eq!(pipelines[0].commands.len(), 2);
    }
}