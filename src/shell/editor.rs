//! Gap Buffer Implementation for Line Editing
//!
//! Provides O(1) insertion/deletion at cursor position.

use alloc::string::String;
use alloc::vec::Vec;

pub struct GapBuffer {
    buffer: Vec<char>,
    gap_start: usize,
    gap_end: usize,
}

impl GapBuffer {
    pub fn new(capacity: usize) -> Self {
        let mut buffer = Vec::with_capacity(capacity);
        // Fill with dummy data initially, though strictly we access via indices
        buffer.resize(capacity, '\0');

        Self {
            buffer,
            gap_start: 0,
            gap_end: capacity,
        }
    }

    pub fn insert(&mut self, c: char) {
        if self.gap_start == self.gap_end {
            self.grow();
        }

        self.buffer[self.gap_start] = c;
        self.gap_start += 1;
    }

    pub fn delete(&mut self) -> Option<char> {
        // Backspace behavior (delete char before gap)
        if self.gap_start > 0 {
            self.gap_start -= 1;
            Some(self.buffer[self.gap_start])
        } else {
            None
        }
    }

    pub fn move_left(&mut self) {
        if self.gap_start > 0 {
            self.gap_start -= 1;
            self.gap_end -= 1;
            self.buffer[self.gap_end] = self.buffer[self.gap_start];
        }
    }

    pub fn move_right(&mut self) {
        if self.gap_end < self.buffer.len() {
            self.buffer[self.gap_start] = self.buffer[self.gap_end];
            self.gap_start += 1;
            self.gap_end += 1;
        }
    }

    fn grow(&mut self) {
        // Expand buffer
        let new_capacity = self.buffer.len() * 2;
        let mut new_buffer = Vec::with_capacity(new_capacity);

        // Copy pre-gap
        for i in 0..self.gap_start {
            new_buffer.push(self.buffer[i]);
        }

        // Fill new gap
        let gap_size = new_capacity - self.buffer.len();
        for _ in 0..gap_size {
            new_buffer.push('\0');
        }

        // Copy post-gap
        for i in self.gap_end..self.buffer.len() {
            new_buffer.push(self.buffer[i]);
        }

        self.gap_end += gap_size;
        self.buffer = new_buffer;
    }

    pub fn to_string(&self) -> String {
        let mut s = String::new();
        for i in 0..self.gap_start {
            s.push(self.buffer[i]);
        }
        for i in self.gap_end..self.buffer.len() {
            s.push(self.buffer[i]);
        }
        s
    }

    /// Cursor pozisyonunu döndür (gap_start = cursor position)
    pub fn cursor_pos(&self) -> usize {
        self.gap_start
    }

    /// Toplam metin uzunluğunu döndür
    pub fn len(&self) -> usize {
        self.gap_start + (self.buffer.len() - self.gap_end)
    }

    /// Cursor'dan sonraki metni döndür
    pub fn text_after_cursor(&self) -> String {
        let mut s = String::new();
        for i in self.gap_end..self.buffer.len() {
            s.push(self.buffer[i]);
        }
        s
    }

    /// İleri silme (Delete tuşu için) - cursor'dan sonraki karakteri sil
    pub fn delete_forward(&mut self) -> Option<char> {
        if self.gap_end < self.buffer.len() {
            let c = self.buffer[self.gap_end];
            self.gap_end += 1;
            Some(c)
        } else {
            None
        }
    }
}
