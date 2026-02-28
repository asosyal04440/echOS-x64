//! # echOS SIMD Grafik Operasyonları
//!
//! AVX2/SSE talimat seti uzantılarıyla grafik hızlandırması.
//!
//! ## SIMD Nedir?
//!
//! SIMD (Single Instruction, Multiple Data — Tek Komut, Çok Veri), modern
//! işlemcilerin tek bir CPU talimatıyla birden fazla veri öğesini paralel olarak
//! işlemesine olanak tanıyan donanım özelliğidir.
//!
//! ## SIMD Kayıt Genişlikleri ve Bant Genişliği
//!
//! ```text
//!  Uzantı   Kayıt    Genişlik   Piksel/talimat (32-bit)
//!  ──────   ──────   ────────   ───────────────────────
//!  SSE2     XMM      128-bit    4 piksel
//!  SSSE3    XMM      128-bit    4 piksel
//!  SSE4.1   XMM      128-bit    4 piksel
//!  AVX2     YMM      256-bit    8 piksel  ← en verimli
//!
//!  Örnek: 1920×1080 ekranı temizleme (scalar vs AVX2)
//!  Scalar: 2.073.600 write işlemi
//!  AVX2:     259.200 write işlemi  (~8× hızlı)
//! ```
//!
//! ## Bellek Hizalama ve Performans
//!
//! ```text
//!  ┌──────────────────────────────────────────────────┐
//!  │  AVX2 loadu/storeu: hizalanmamış belleğe erişir  │
//!  │  (yavaş ama güvenli)                             │
//!  │                                                   │
//!  │  Hizalama tespiti:                               │
//!  │    align = (dst_ptr & 31)                        │
//!  │    Eğer align != 0 → önce (32-align) byte'ı     │
//!  │      scalar kopyala, ardından hizalı devam et   │
//!  └──────────────────────────────────────────────────┘
//!
//!  Bellek adresi:  0x...FD00_0042
//!                            ^^^^
//!                            0x42 & 31 = 2 → 30 byte önceden scalar kopyala
//!                            sonra 0x...FD00_0060'tan 32-byte AVX2 chunks
//! ```
//!
//! ## Otomatik Seçim Hiyerarşisi
//!
//! ```text
//!  stream_copy() çağrısı
//!       │
//!       ├─ AVX2 mevcut? ──→ stream_copy_avx2()   (256-bit, 8 piksel/talimat)
//!       ├─ SSE4.1 mevcut? → stream_copy_sse41()  (128-bit, 4 piksel/talimat)
//!       ├─ SSSE3 mevcut? → stream_copy_ssse3()   (128-bit, 4 piksel/talimat)
//!       ├─ SSE2 mevcut? ──→ stream_copy_sse2()   (128-bit, 4 piksel/talimat)
//!       └─ Hiçbiri yok ──→ copy_nonoverlapping()  (scalar, 1 byte/talimat)
//! ```

/// AVX2 ile 256-bit SIMD bellek kopyalama.
/// Önce hedef belleği 32-byte sınırına hizalar, ardından 32-byte chunks kopyalar.
#[target_feature(enable = "avx2")]
pub unsafe fn stream_copy_avx2(src: *const u8, dst: *mut u8, len: usize) {
    use core::arch::x86_64::{_mm256_loadu_si256, _mm256_storeu_si256};
    if len == 0 {
        return;
    }
    let mut offset = 0usize;
    // Hedef adresi 32-byte sınırına hizala (AVX2 yüklemesi için önemli)
    let align = (dst as usize) & 31;
    if align != 0 {
        let prefix = (32 - align).min(len);
        core::ptr::copy_nonoverlapping(src, dst, prefix);
        offset += prefix;
    }
    // 32-byte (256-bit) bloklar halinde AVX2 ile kopyala
    while offset + 32 <= len {
        let v = _mm256_loadu_si256(src.add(offset) as *const _);
        _mm256_storeu_si256(dst.add(offset) as *mut _, v);
        offset += 32;
    }
    // Kalan baytları scalar olarak tamamla
    if offset < len {
        core::ptr::copy_nonoverlapping(src.add(offset), dst.add(offset), len - offset);
    }
}

/// SSE2 ile 128-bit SIMD bellek kopyalama.
/// Önce hedef belleği 16-byte sınırına hizalar, ardından 16-byte chunks kopyalar.
#[target_feature(enable = "sse2")]
pub unsafe fn stream_copy_sse2(src: *const u8, dst: *mut u8, len: usize) {
    use core::arch::x86_64::{_mm_loadu_si128, _mm_storeu_si128};
    if len == 0 {
        return;
    }
    let mut offset = 0usize;
    // Hedef adresi 16-byte sınırına hizala (SSE2 yüklemesi için önemli)
    let align = (dst as usize) & 15;
    if align != 0 {
        let prefix = (16 - align).min(len);
        core::ptr::copy_nonoverlapping(src, dst, prefix);
        offset += prefix;
    }
    // 16-byte (128-bit) bloklar halinde SSE2 ile kopyala
    while offset + 16 <= len {
        let v = _mm_loadu_si128(src.add(offset) as *const _);
        _mm_storeu_si128(dst.add(offset) as *mut _, v);
        offset += 16;
    }
    // Kalan baytları scalar olarak tamamla
    if offset < len {
        core::ptr::copy_nonoverlapping(src.add(offset), dst.add(offset), len - offset);
    }
}

/// SSSE3 ile 128-bit SIMD bellek kopyalama (SSE2'nin ötesinde yatay işlem desteği).
#[target_feature(enable = "ssse3")]
pub unsafe fn stream_copy_ssse3(src: *const u8, dst: *mut u8, len: usize) {
    use core::arch::x86_64::{_mm_loadu_si128, _mm_storeu_si128};
    if len == 0 {
        return;
    }
    let mut offset = 0usize;
    let align = (dst as usize) & 15;
    if align != 0 {
        let prefix = (16 - align).min(len);
        core::ptr::copy_nonoverlapping(src, dst, prefix);
        offset += prefix;
    }
    while offset + 16 <= len {
        let v = _mm_loadu_si128(src.add(offset) as *const _);
        _mm_storeu_si128(dst.add(offset) as *mut _, v);
        offset += 16;
    }
    if offset < len {
        core::ptr::copy_nonoverlapping(src.add(offset), dst.add(offset), len - offset);
    }
}

/// SSE4.1 ile 128-bit SIMD bellek kopyalama (blendv, insert/extract gibi ek talimatlar içerir).
#[target_feature(enable = "sse4.1")]
pub unsafe fn stream_copy_sse41(src: *const u8, dst: *mut u8, len: usize) {
    use core::arch::x86_64::{_mm_loadu_si128, _mm_storeu_si128};
    if len == 0 {
        return;
    }
    let mut offset = 0usize;
    let align = (dst as usize) & 15;
    if align != 0 {
        let prefix = (16 - align).min(len);
        core::ptr::copy_nonoverlapping(src, dst, prefix);
        offset += prefix;
    }
    while offset + 16 <= len {
        let v = _mm_loadu_si128(src.add(offset) as *const _);
        _mm_storeu_si128(dst.add(offset) as *mut _, v);
        offset += 16;
    }
    if offset < len {
        core::ptr::copy_nonoverlapping(src.add(offset), dst.add(offset), len - offset);
    }
}

/// Çalışma zamanında CPU yeteneklerini sorgulayarak en hızlı bellek kopyalama
/// uygulamasını seçer. Framebuffer blitleme işlemlerinde kullanılır.
///
/// Öncelik sırası: AVX2 > SSE4.1 > SSSE3 > SSE2 > Scalar
pub unsafe fn stream_copy(src: *const u8, dst: *mut u8, len: usize) {
    if crate::cpu::has_avx2() {
        stream_copy_avx2(src, dst, len);
    } else if crate::cpu::has_sse41() {
        stream_copy_sse41(src, dst, len);
    } else if crate::cpu::has_ssse3() {
        stream_copy_ssse3(src, dst, len);
    } else if crate::cpu::has_sse2() {
        stream_copy_sse2(src, dst, len);
    } else {
        // Hiçbir SIMD desteği yok, güvenli scalar kopyalama kullan
        core::ptr::copy_nonoverlapping(src, dst, len);
    }
}
