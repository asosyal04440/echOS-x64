use alloc::vec::Vec;
use alloc::string::String;

pub struct ShellHistory {
    entries: Vec<String>,
    cursor: usize,
    limit: usize,
}

impl ShellHistory {
    pub fn new() -> Self {
        Self { entries: Vec::new(), cursor: 0, limit: 1000 }
    }
    pub fn push(&mut self, cmd: String) {
        if self.entries.len() >= self.limit { self.entries.remove(0); }
        self.entries.push(cmd);
        self.cursor = self.entries.len();
    }
    pub fn previous(&mut self) -> Option<&str> {
        if self.cursor > 0 { self.cursor -= 1; self.entries.get(self.cursor).map(|s| s.as_str()) }
        else { None }
    }
    pub fn next(&mut self) -> Option<&str> {
        if self.cursor < self.entries.len() { self.cursor += 1; self.entries.get(self.cursor).map(|s| s.as_str()) }
        else { self.cursor = self.entries.len(); None }
    }
    pub fn list(&self) -> &[String] { &self.entries }
    pub fn search(&self, pattern: &str) -> Option<&str> {
        self.entries.iter().rev().find(|e| e.contains(pattern)).map(|s| s.as_str())
    }
}
