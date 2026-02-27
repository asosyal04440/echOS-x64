//! # TSC (Time Stamp Counter) — Yüksek Çözünürlüklü Zamanlayıcı Modülü
//!
//! RDTSC komutu ile CPU zaman damgası sayacından okuma yapar.
//! TSC, her clock cycle'da bir artan 64-bit bir sayaçtır; çok hassas
//! ölçümler için işletim sistemleri tarafından yaygın biçimde kullanılır.
//! Doğru nanosaniye dönüşümü için CPU frekansının kalibrasyonu zorunludur.

use core::arch::asm;

/// TSC değerini oku (ham döngü sayısı)
/// RDTSC komutu EAX:EDX çiftine 64-bit sayacı yazar;
/// bunu tek bir u64'e birleştiriyoruz.
pub fn read() -> u64 {
    let low: u32;
    let high: u32;
    
    unsafe {
        asm!(
            "rdtsc",
            out("eax") low,
            out("edx") high,
            options(nomem, nostack)
        );
    }
    
    ((high as u64) << 32) | (low as u64)
}

/// TSC değerini nanosaniyeye çevir (CPU frekansı bilinmeli)
/// Basitleştirilmiş sürüm — gerçek implementasyonda PIT/HPET ile kalibrasyon gerekir.
pub fn read_ns() -> u64 {
    // Şimdilik ~3GHz CPU varsayımı yapılıyor — ileride kalibre edilmeli
    let tsc = read();
    // Nanosaniyeye çevir: 3GHz = her ns'de 3 döngü, yani döngü/3 = ns
    tsc / 3
}

/// TSC değerini mikrosaniyeye çevir
pub fn read_us() -> u64 {
    read_ns() / 1000
}

/// TSC değerini milisaniyeye çevir
pub fn read_ms() -> u64 {
    read_us() / 1000
}

/// CPU frekansını Hz cinsinden döndür (geçici sabit — kalibre edilmeli)
pub fn cpu_frequency() -> u64 {
    // Geçici değer: 3 GHz — gerçek donanımda CPUID veya PIT ile ölçülmeli
    3_000_000_000
}

/// TSC frekansını PIT (Programmable Interval Timer) ile kalibre et
/// Döndürür: Hz cinsinden TSC frekansı
///
/// PIT 1,193,182 Hz ile çalışır; TSC delta ölçümü ile CPU döngü/sn hesaplanır.
/// Basitleştirilmiş sürüm — gerçek implementasyon PIT veya HPET kullanır.
pub fn calibrate() -> u64 {
    // Basit kalibrasyon — sonraki aşamada PIT veya HPET kullanılarak gerçekleştirilmeli
    3_000_000_000
}
