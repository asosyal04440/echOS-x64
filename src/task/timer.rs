//! # echOS Zaman Çarkı (Timing Wheel)
//!
//! Bu modül, yüksek performanslı timer yönetimi için "Hierarchical Timing Wheel"
//! (Hiyerarşik Zaman Çarkı) algoritmasını uygular.
//!
//! O(N) karmaşıklığına sahip basit bir liste taraması yerine, O(1) sabit zamanlı
//! ekleme ve silme işlemleri sunar. Milyonlarca task uyusa bile sistem performansı düşmez.
//!
//! Kaynak: "Hashed and Hierarchical Timing Wheels", Varghese & Lauck (1987)

use super::task::{Task, TaskState};
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use alloc::boxed::Box;

/// Çarkın her bir dilimi (bucket).
/// Aynı tick'te uyanacak task'ları tutar.
type TimerBucket = VecDeque<Box<Task>>;

const WHEEL_SIZE: usize = 256;
const WHEEL_MASK: usize = WHEEL_SIZE - 1;
const WHEEL_BITS: usize = 8; // 2^8 = 256

/// Hiyerarşik Zaman Çarkı (Timing Wheel) yapısı.
/// 4 Seviyeli:
/// 1. Seviye: 0 - 255 tick (2^8)
/// 2. Seviye: 256 - 65535 tick (2^16)
/// 3. Seviye: 65536 - 16M tick (2^24)
/// 4. Seviye: 16M - 4G tick (2^32)
pub struct TimingWheel {
    /// Çarklar (4 seviye)
    wheels: [Vec<TimerBucket>; 4],
    /// Şu anki tick (imleç)
    current_tick: usize,
}

impl TimingWheel {
    /// Yeni bir Timing Wheel oluşturur.
    pub fn new(_size: usize) -> Self {
        // Size parametresi şimdilik göz ardı ediliyor, sabit hiyerarşi kullanılıyor.
        let mut wheels: [Vec<TimerBucket>; 4] = [
            Vec::with_capacity(WHEEL_SIZE),
            Vec::with_capacity(WHEEL_SIZE),
            Vec::with_capacity(WHEEL_SIZE),
            Vec::with_capacity(WHEEL_SIZE),
        ];

        for i in 0..4 {
            for _ in 0..WHEEL_SIZE {
                wheels[i].push(VecDeque::new());
            }
        }

        Self {
            wheels,
            current_tick: 0,
        }
    }

    /// Bir task'ı belirtilen tick sayısında uyanmak üzere zamanlar.
    ///
    /// # Parametreler
    /// * `task`: Uyutulacak task
    /// * `wake_tick`: Uyanması gereken mutlak tick zamanı
    pub fn schedule(&mut self, mut task: Box<Task>, wake_tick: usize) {
        task.hot.state = TaskState::Sleeping { wake_tick };

        // Geçmiş zaman kontrolü
        if wake_tick <= self.current_tick {
            // Hemen bir sonraki slotta uyandır
            self.wheels[0][(self.current_tick + 1) & WHEEL_MASK].push_back(task);
            return;
        }

        let diff = wake_tick - self.current_tick;

        // Hangi çarka ekleneceğini bul
        if diff < WHEEL_SIZE {
            // Seviye 1: Doğrudan indeks
            let idx = wake_tick & WHEEL_MASK;
            self.wheels[0][idx].push_back(task);
        } else if diff < 1 << (2 * WHEEL_BITS) {
            // Seviye 2
            let idx = (wake_tick >> WHEEL_BITS) & WHEEL_MASK;
            self.wheels[1][idx].push_back(task);
        } else if diff < 1 << (3 * WHEEL_BITS) {
            // Seviye 3
            let idx = (wake_tick >> (2 * WHEEL_BITS)) & WHEEL_MASK;
            self.wheels[2][idx].push_back(task);
        } else {
            // Seviye 4 (Overflow)
            let idx = (wake_tick >> (3 * WHEEL_BITS)) & WHEEL_MASK;
            self.wheels[3][idx].push_back(task);
        }
    }

    /// Çarkı bir tick ilerletir ve uyanması gereken task'ları döndürür.
    /// O(1) amortized complexity.
    pub fn tick(&mut self) -> Vec<Box<Task>> {
        let current = self.current_tick;
        self.current_tick += 1;

        let mut woken_tasks = Vec::new();

        // 1. Seviye 1'deki (Fast Wheel) şu anki slotu işle
        let idx = current & WHEEL_MASK;
        while let Some(task) = self.wheels[0][idx].pop_front() {
            woken_tasks.push(task);
        }

        // Cascade (Şelale) işlemi: Üst çarklardan alt çarklara task taşıma
        // Eğer 1. çark başa döndüyse (index 0), 2. çarktan bir slot taşı
        if (current + 1) & WHEEL_MASK == 0 {
            self.cascade(1, current + 1);
        }
        // Eğer 2. çark da başa döndüyse, 3. çarktan taşı
        if (current + 1) & ((1 << (2 * WHEEL_BITS)) - 1) == 0 {
            self.cascade(2, current + 1);
        }
        // Eğer 3. çark da başa döndüyse, 4. çarktan taşı
        if (current + 1) & ((1 << (3 * WHEEL_BITS)) - 1) == 0 {
            self.cascade(3, current + 1);
        }

        woken_tasks
    }

    /// Üst seviye çarktan alt seviye çarka task'ları taşır (Cascade).
    fn cascade(&mut self, level: usize, tick: usize) {
        let idx = (tick >> (level * WHEEL_BITS)) & WHEEL_MASK;
        
        // O slot'taki tüm task'ları al
        let mut tasks_to_move = Vec::new();
        while let Some(task) = self.wheels[level][idx].pop_front() {
            tasks_to_move.push(task);
        }

        // Task'ları tekrar schedule et (otomatik olarak alt çarka düşecekler)
        for task in tasks_to_move {
            if let TaskState::Sleeping { wake_tick } = task.state {
                self.schedule(task, wake_tick);
            } else {
                // Hata durumu, ama güvenli olması için tekrar schedule et
                self.schedule(task, tick); 
            }
        }
    }
}
