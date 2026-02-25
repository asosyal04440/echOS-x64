//! Line Discipline (N_TTY)
//! 
//! Klavye sürücüsünden fırlatılan karakterlerin bufferlanması, pişirilmesi (cooking)
//! ve özel tuş kombinasyonlarının (Ctrl+C, Backspace vb.) işlenmesinden sorumlu modül.

use super::buffer::TtyBuffer;
use pc_keyboard::DecodedKey;

pub struct LineDiscipline {
    pub input_buf: TtyBuffer,
    pub output_buf: TtyBuffer,
}

impl LineDiscipline {
    pub const fn new() -> Self {
        Self {
            input_buf: TtyBuffer::new(),
            output_buf: TtyBuffer::new(),
        }
    }

    /// Interrupt Handler üzerinden klavye basışlarını alır
    pub fn receive_key(&self, key: DecodedKey) {
        match key {
            DecodedKey::Unicode(c) => {
                // Backspace (0x08)
                if c == '\x08' {
                    if self.input_buf.unpush() {
                        // Echo olarak da backspace yollayıp karakter üzerine boşluk basalım (siliş efekti)
                        let _ = self.output_buf.push(0x08);
                        let _ = self.output_buf.push(0x20); // Boşluk
                        let _ = self.output_buf.push(0x08); // İmleci geri al
                    }
                } 
                // Ctrl+C (0x03)
                else if c == '\x03' {
                    crate::serial_println!("[TTY] Ctrl+C Received - Sinyal yollanacak!");
                    let _ = self.output_buf.push('^' as u8);
                    let _ = self.output_buf.push('C' as u8);
                    let _ = self.output_buf.push('\n' as u8);
                } 
                // Normal tuş basımı
                else {
                    let _ = self.input_buf.push(c as u8);
                    // Echo (Ekranda görünmesi için output_buf'a yansıt)
                    let _ = self.output_buf.push(c as u8);
                    
                    if c == '\n' {
                        // Yeni satır karakteri geldiyse satır pişmiştir.
                        // Bekleyen "read" sys_call varsa, io_uring fırlatıp okutabiliriz.
                    }
                }
            }
            DecodedKey::RawKey(_k) => {
                // Yön tuşları veya oklar
            }
        }
    }
    
    // User-space thread'ler sys_read yaptığında buradan okuyacak
    pub fn sys_read(&self, buffer: &mut [u8]) -> usize {
        let mut count = 0;
        while count < buffer.len() {
            if let Some(byte) = self.input_buf.pop() {
                buffer[count] = byte;
                count += 1;
                // Satır pişirme kuralı: \n görünce buffer sonunu kapat
                if byte == b'\n' {
                    break;
                }
            } else {
                break; // Şimdilik non-blocking gibi kırıyoruz
            }
        }
        count
    }
}
