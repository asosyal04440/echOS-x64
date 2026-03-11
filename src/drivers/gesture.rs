//! # Mouse Gesture Engine
//!
//! Son hareketleri analiz ederek jestleri (swipe) tanır.
//! lock-free ve allocation-minimal tasarlanmıştır.

use alloc::vec::Vec;
use crate::cpu::tsc;

#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(u8)]
pub enum Gesture {
    SwipeLeft,
    SwipeRight,
    SwipeUp,
    SwipeDown,
}

struct MotionPoint {
    dx: i32,
    dy: i32,
    timestamp: u64,
}

pub struct GestureRecognizer {
    history: Vec<MotionPoint>,
}

impl GestureRecognizer {
    pub const fn new() -> Self {
        Self {
            history: Vec::new(),
        }
    }

    pub fn init(&mut self) {
        self.history = Vec::with_capacity(64);
    }

    pub fn feed(&mut self, dx: i32, dy: i32) -> Option<Gesture> {
        let now = unsafe { tsc::read() };
        
        // Geçmişi temizle (eski kayıtları sil)
        // 3GHz işlemci için 300ms = 900,000,000 tick.
        // Daha hassas bir zamanlama için kalibrasyon gerekir ama şimdilik sabit değer.
        const TIMEOUT_TICKS: u64 = 900_000_000; 
        
        if !self.history.is_empty() {
            // retain yerine manuel loop daha performanslı olabilir ama Vec için retain optimize edilmiştir
            self.history.retain(|p| now >= p.timestamp && now - p.timestamp < TIMEOUT_TICKS);
        }
        
        self.history.push(MotionPoint { dx, dy, timestamp: now });
        
        self.analyze()
    }
    
    fn analyze(&mut self) -> Option<Gesture> {
        if self.history.len() < 5 {
            return None;
        }

        // Toplam yer değiştirme
        let mut total_dx = 0;
        let mut total_dy = 0;
        
        for p in &self.history {
            total_dx += p.dx;
            total_dy += p.dy;
        }
        
        // Eşik değerler (hız/mesafe)
        // Çok düşük eşik yanlış tetiklemelere yol açar
        const SWIPE_THRESHOLD: i32 = 300; 
        
        if total_dx.abs() > SWIPE_THRESHOLD && total_dx.abs() > total_dy.abs() * 3 {
            // Yatay hareket baskın
            self.history.clear(); // Jest algılandı, geçmişi temizle
            if total_dx > 0 {
                return Some(Gesture::SwipeRight);
            } else {
                return Some(Gesture::SwipeLeft);
            }
        }
        
        if total_dy.abs() > SWIPE_THRESHOLD && total_dy.abs() > total_dx.abs() * 3 {
            // Dikey hareket baskın
            self.history.clear();
            if total_dy > 0 {
                // PS/2 Y ekseni: yukarı pozitif, ama bizim mouse driver'da Y ekseni ters çevriliyor.
                // mouse.rs: Y yukarı = negatif (ekran koordinatı).
                // dy > 0 ise aşağı hareket.
                return Some(Gesture::SwipeDown);
            } else {
                return Some(Gesture::SwipeUp);
            }
        }
        
        None
    }
}
