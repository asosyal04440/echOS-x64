//! # Scroll Physics
//!
//! Kinetic scrolling with decay (momentum).
//! iOS/macOS benzeri kaydırma hissi sağlar.

#[derive(Debug, Clone, Copy)]
pub struct ScrollMomentum {
    velocity_y: f32,
    velocity_x: f32,
    active: bool,
    max_step_px: f32,
}

impl ScrollMomentum {
    pub fn new() -> Self {
        Self {
            velocity_y: 0.0,
            velocity_x: 0.0,
            active: false,
            max_step_px: 96.0,
        }
    }

    /// Scroll olayını ekle (hız kazandır)
    pub fn add(&mut self, dx: f32, dy: f32) {
        // Hızı biriktir (accumulator)
        // Çok hızlı kaydırmada hız artar
        self.velocity_x += dx * 2.0;
        self.velocity_y += dy * 2.0;
        
        // Hız limiti (clamp)
        let max_v = 100.0;
        self.velocity_x = self.velocity_x.clamp(-max_v, max_v);
        self.velocity_y = self.velocity_y.clamp(-max_v, max_v);
        
        self.active = true;
    }

    /// Fizik motorunu güncelle ve hareket miktarını döndür (dx, dy)
    pub fn update(&mut self, dt: f32) -> (f32, f32) {
        if !self.active {
            return (0.0, 0.0);
        }

        // Hız yükseldikçe sürtünme artar; aşırı fling'te kontrol kaybını engeller.
        let speed = self.velocity_x.abs().max(self.velocity_y.abs());
        let friction = if speed > 48.0 {
            6.0
        } else if speed > 20.0 {
            5.0
        } else {
            4.0
        };
        
        // Hızı azalt (exponential decay)
        let decay = 1.0 - (friction * dt).min(1.0);
        self.velocity_x *= decay;
        self.velocity_y *= decay;

        // Durma eşiği
        let stop_threshold = 0.5;
        if self.velocity_x.abs() < stop_threshold && self.velocity_y.abs() < stop_threshold {
            self.velocity_x = 0.0;
            self.velocity_y = 0.0;
            self.active = false;
            return (0.0, 0.0);
        }

        // Piksel hareketi (hız * zaman * ölçek)
        // 60.0 çarpanı, hızı piksel/frame cinsinden normalize etmek için
        let move_x = (self.velocity_x * dt * 60.0).clamp(-self.max_step_px, self.max_step_px);
        let move_y = (self.velocity_y * dt * 60.0).clamp(-self.max_step_px, self.max_step_px);
        
        (move_x, move_y)
    }
    
    /// Momentum aktif mi?
    pub fn is_active(&self) -> bool {
        self.active
    }
    
    /// Hızı sıfırla (örn. kullanıcı tıkladığında durdurmak için)
    pub fn stop(&mut self) {
        self.velocity_x = 0.0;
        self.velocity_y = 0.0;
        self.active = false;
    }
}
