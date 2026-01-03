//! # echOS Shell (Komut Satırı)
//! 
//! Basit komut satırı arayüzü.
//! Gap buffer tabanlı metin düzenleme ve komut geçmişi.

pub mod editor;

use alloc::vec::Vec;
use alloc::string::String;
use alloc::format;
use editor::GapBuffer;

/// Komut satırı shell yapısı
pub struct Shell {
    /// Metin düzenleme için gap buffer
    editor: GapBuffer,
    /// Komut geçmişi
    history: Vec<String>,
}

impl Shell {
    /// Yeni bir shell instance oluşturur.
    pub fn new() -> Self {
        Self {
            editor: GapBuffer::new(64),
            history: Vec::new(),
        }
    }
    
    /// Klavye tuşunu işler.
    pub fn handle_key(&mut self, key: pc_keyboard::DecodedKey) {
        use pc_keyboard::DecodedKey;
        match key {
            DecodedKey::Unicode(c) => match c {
                '\n' => {},
                '\x08' => { self.editor.delete(); }, // Backspace
                _ => self.editor.insert(c),
            },
            DecodedKey::RawKey(code) => {
                use pc_keyboard::KeyCode;
                match code {
                    KeyCode::ArrowLeft => self.editor.move_left(),
                    KeyCode::ArrowRight => self.editor.move_right(),
                    KeyCode::ArrowUp => { /* Geçmiş navigasyonu */ },
                    KeyCode::ArrowDown => { /* Geçmiş navigasyonu */ },
                    _ => {}
                }
            }
        }
    }
    
    /// Mevcut komutu çalıştırır ve sonucu döndürür.
    pub fn execute(&mut self) -> Option<String> {
        let cmd_line = self.editor.to_string();
        
        // Editor'ı sıfırla
        self.editor = GapBuffer::new(64);
        
        // Geçmişe ekle
        if !cmd_line.is_empty() {
            self.history.push(cmd_line.clone());
        }
        
        let trimmed = cmd_line.trim();
        if trimmed.is_empty() {
            return None;
        }
        
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        match parts[0] {
            "help" => Some(String::from("Mevcut komutlar: help, ver, echo, clear")),
            "ver" => Some(String::from("echOS v0.2.0 (Legendary Edition)")),
            "echo" => {
                let args = &parts[1..];
                Some(args.join(" "))
            },
            "clear" => Some(String::from("__CLEAR__")), // Özel sinyal
            _ => Some(format!("Bilinmeyen komut: {}", parts[0])),
        }
    }
    
    /// Mevcut input satırını döndürür.
    pub fn get_input_line(&self) -> String {
        self.editor.to_string()
    }
}
