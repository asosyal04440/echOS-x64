//! Zaman yardımcı fonksiyonları — TSC/HPET bazlı timestamp, uyku

/// Mevcut zamanı nanosaniye cinsinden döndürür (TSC bazlı).
pub fn current_timestamp_nanos() -> u64 {
    crate::cpu::tsc::read_ns()
}

/// Belirtilen süre kadar uyur (milisaniyeye çevirerek scheduler sleep kullanır).
pub fn sleep(duration: core::time::Duration) {
    let ms = duration.as_millis() as u64;
    if ms > 0 {
        crate::task::scheduler::sleep(ms as usize);
    }
}
