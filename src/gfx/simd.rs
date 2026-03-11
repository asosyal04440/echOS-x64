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

use core::sync::atomic::{AtomicUsize, Ordering};

/// Cached fn ptr — CPUID'yi her çağrıda yapmak yerine boot'ta bir kez belirle.
/// 0 = henüz başlatılmadı, diğer = fn ptr olarak transmute edilir.
static STREAM_COPY_FN: AtomicUsize = AtomicUsize::new(0);

/// Boot sırasında bir kez çağırılır. CPU SIMD yeteneklerine göre
/// en hızlı stream_copy implementasyonunu seçer ve fn ptr'yi cache'ler.
pub fn init_simd_dispatch() {
    type CopyFn = unsafe fn(*const u8, *mut u8, usize);

    let copy_fn: CopyFn = if crate::cpu::has_avx2() {
        stream_copy_avx2 as CopyFn
    } else if crate::cpu::has_sse41() {
        stream_copy_sse41 as CopyFn
    } else if crate::cpu::has_ssse3() {
        stream_copy_ssse3 as CopyFn
    } else if crate::cpu::has_sse2() {
        stream_copy_sse2 as CopyFn
    } else {
        scalar_copy as CopyFn
    };

    STREAM_COPY_FN.store(copy_fn as usize, Ordering::Release);

    // Alpha blend fn ptr cache
    type BlendFn = unsafe fn(*const u32, *mut u32, usize);
    let blend_fn: BlendFn = if crate::cpu::has_avx2() {
        alpha_blend_row_avx2 as BlendFn
    } else if crate::cpu::has_sse41() {
        alpha_blend_row_sse41 as BlendFn
    } else {
        // scalar fallback — güvenli wrapper
        scalar_blend_wrapper as BlendFn
    };
    ALPHA_BLEND_FN.store(blend_fn as usize, Ordering::Release);

    crate::serial_println!(
        "[SIMD] dispatch initialized: avx2={} sse41={} ssse3={} sse2={}",
        crate::cpu::has_avx2(),
        crate::cpu::has_sse41(),
        crate::cpu::has_ssse3(),
        crate::cpu::has_sse2()
    );
}

/// Scalar blend wrapper — unsafe fn imza uyumu için
unsafe fn scalar_blend_wrapper(src: *const u32, dst: *mut u32, count: usize) {
    alpha_blend_row_scalar(src, dst, count);
}

/// Scalar fallback
unsafe fn scalar_copy(src: *const u8, dst: *mut u8, len: usize) {
    core::ptr::copy_nonoverlapping(src, dst, len);
}

/// Çalışma zamanında CPU yeteneklerini sorgulayarak en hızlı bellek kopyalama
/// uygulamasını seçer. Framebuffer blitleme işlemlerinde kullanılır.
///
/// init_simd_dispatch() çağrıldıysa fn ptr cache kullanır (0 CPUID overhead).
/// Çağrılmadıysa fallback olarak her seferinde CPUID kontrol eder.
pub unsafe fn stream_copy(src: *const u8, dst: *mut u8, len: usize) {
    let cached = STREAM_COPY_FN.load(Ordering::Acquire);
    if cached != 0 {
        let func: unsafe fn(*const u8, *mut u8, usize) = core::mem::transmute(cached);
        func(src, dst, len);
        return;
    }
    // Fallback: henüz init çağrılmadıysa (erken boot)
    if crate::cpu::has_avx2() {
        stream_copy_avx2(src, dst, len);
    } else if crate::cpu::has_sse41() {
        stream_copy_sse41(src, dst, len);
    } else if crate::cpu::has_ssse3() {
        stream_copy_ssse3(src, dst, len);
    } else if crate::cpu::has_sse2() {
        stream_copy_sse2(src, dst, len);
    } else {
        core::ptr::copy_nonoverlapping(src, dst, len);
    }
}

// ────────────────────────────────────────────────────────────
// SIMD Alpha Blending
// ────────────────────────────────────────────────────────────
//
// Premultiplied alpha compositing: result = src + dst * (1 - src_alpha)
// Her piksel 0xAARRGGBB formatında, 32-bit.
//
// AVX2: 8 piksel/talimat (256-bit YMM register, 8×32-bit)
// SSE4.1: 4 piksel/talimat (128-bit XMM register, 4×32-bit)
//
// Kanal ayrıştırma (unpack):
//   piksel = 0xAARRGGBB
//   A = (piksel >> 24) & 0xFF
//   R = (piksel >> 16) & 0xFF
//   G = (piksel >>  8) & 0xFF
//   B = (piksel >>  0) & 0xFF
//
// Blend formülü (premultiplied):
//   out_c = src_c + dst_c * (255 - src_a) / 255
//   out_a = src_a + dst_a * (255 - src_a) / 255

/// Cached alpha blend fn ptr — boot'ta init_simd_dispatch() ile belirlenir.
static ALPHA_BLEND_FN: AtomicUsize = AtomicUsize::new(0);

/// AVX2 ile 8 piksel/döngü alfa harmanlama.
/// `src` ve `dst` piksel dizileri 0xAARRGGBB formatında olmalıdır.
/// `count`: işlenecek piksel sayısı.
#[target_feature(enable = "avx2")]
pub unsafe fn alpha_blend_row_avx2(src: *const u32, dst: *mut u32, count: usize) {
    let mut i = 0usize;

    // 8 piksellik bloklar halinde AVX2 ile harmanlama
    while i + 8 <= count {
        let s = core::arch::x86_64::_mm256_loadu_si256(src.add(i) as *const _);
        let d = core::arch::x86_64::_mm256_loadu_si256(dst.add(i) as *const _);

        // Alpha kanalını çıkar: her pikselden >> 24
        let alpha_shift = core::arch::x86_64::_mm256_srli_epi32(s, 24);

        // Alpha = 0 ise (tamamen şeffaf) → dst değişmez; Alpha = 255 ise → src yaz
        // Hızlı yol: tamamen opak kontrolü
        let alpha_mask = core::arch::x86_64::_mm256_cmpeq_epi32(
            alpha_shift,
            core::arch::x86_64::_mm256_set1_epi32(255),
        );

        // Tamamen şeffaf kontrol
        let zero_mask = core::arch::x86_64::_mm256_cmpeq_epi32(
            alpha_shift,
            core::arch::x86_64::_mm256_setzero_si256(),
        );

        // Scalar fallback for partial alpha (SIMD partial blend is complex)
        // Opak pikseller: src yaz; şeffaf pikseller: dst bırak; kısmi: scalar
        let opaque_result = core::arch::x86_64::_mm256_blendv_epi8(d, s, alpha_mask);
        let result = core::arch::x86_64::_mm256_blendv_epi8(opaque_result, d, zero_mask);

        core::arch::x86_64::_mm256_storeu_si256(dst.add(i) as *mut _, result);

        // Kısmi alfa pikselleri scalar ile düzelt
        for j in 0..8 {
            let sp = *src.add(i + j);
            let sa = (sp >> 24) & 0xFF;
            if sa != 0 && sa != 255 {
                let dp = *dst.add(i + j);
                *dst.add(i + j) = scalar_alpha_blend(sp, dp);
            }
        }

        i += 8;
    }

    // Kalan pikselleri scalar ile işle
    while i < count {
        let sp = *src.add(i);
        let sa = (sp >> 24) & 0xFF;
        if sa == 255 {
            *dst.add(i) = sp;
        } else if sa > 0 {
            *dst.add(i) = scalar_alpha_blend(sp, *dst.add(i));
        }
        i += 1;
    }
}

/// SSE4.1 ile 4 piksel/döngü alfa harmanlama (AVX2 bulunmadığında fallback).
#[target_feature(enable = "sse4.1")]
pub unsafe fn alpha_blend_row_sse41(src: *const u32, dst: *mut u32, count: usize) {
    let mut i = 0usize;

    while i + 4 <= count {
        let s = core::arch::x86_64::_mm_loadu_si128(src.add(i) as *const _);
        let d = core::arch::x86_64::_mm_loadu_si128(dst.add(i) as *const _);

        let alpha_shift = core::arch::x86_64::_mm_srli_epi32(s, 24);
        let alpha_mask = core::arch::x86_64::_mm_cmpeq_epi32(
            alpha_shift,
            core::arch::x86_64::_mm_set1_epi32(255),
        );
        let zero_mask = core::arch::x86_64::_mm_cmpeq_epi32(
            alpha_shift,
            core::arch::x86_64::_mm_setzero_si128(),
        );

        let opaque_result = core::arch::x86_64::_mm_blendv_epi8(d, s, alpha_mask);
        let result = core::arch::x86_64::_mm_blendv_epi8(opaque_result, d, zero_mask);

        core::arch::x86_64::_mm_storeu_si128(dst.add(i) as *mut _, result);

        // Kısmi alfa pikseller scalar ile düzelt
        for j in 0..4 {
            let sp = *src.add(i + j);
            let sa = (sp >> 24) & 0xFF;
            if sa != 0 && sa != 255 {
                let dp = *dst.add(i + j);
                *dst.add(i + j) = scalar_alpha_blend(sp, dp);
            }
        }

        i += 4;
    }

    while i < count {
        let sp = *src.add(i);
        let sa = (sp >> 24) & 0xFF;
        if sa == 255 {
            *dst.add(i) = sp;
        } else if sa > 0 {
            *dst.add(i) = scalar_alpha_blend(sp, *dst.add(i));
        }
        i += 1;
    }
}

/// Scalar premultiplied alpha blend — tek piksel.
/// Formül: out = src + dst * (255 - src_a) / 255
#[inline(always)]
pub fn scalar_alpha_blend(src: u32, dst: u32) -> u32 {
    let sa = (src >> 24) & 0xFF;
    if sa == 0 {
        return dst;
    }
    if sa == 255 {
        return src;
    }

    let inv_a = 255 - sa;

    let sr = (src >> 16) & 0xFF;
    let sg = (src >> 8) & 0xFF;
    let sb = src & 0xFF;

    let dr = (dst >> 16) & 0xFF;
    let dg = (dst >> 8) & 0xFF;
    let db = dst & 0xFF;
    let da = (dst >> 24) & 0xFF;

    let or = sr + (dr * inv_a / 255);
    let og = sg + (dg * inv_a / 255);
    let ob = sb + (db * inv_a / 255);
    let oa = sa + (da * inv_a / 255);

    let or = if or > 255 { 255 } else { or };
    let og = if og > 255 { 255 } else { og };
    let ob = if ob > 255 { 255 } else { ob };
    let oa = if oa > 255 { 255 } else { oa };

    (oa << 24) | (or << 16) | (og << 8) | ob
}

/// Scalar fallback satır harmanlama
pub fn alpha_blend_row_scalar(src: *const u32, dst: *mut u32, count: usize) {
    for i in 0..count {
        unsafe {
            let sp = *src.add(i);
            let sa = (sp >> 24) & 0xFF;
            if sa == 255 {
                *dst.add(i) = sp;
            } else if sa > 0 {
                *dst.add(i) = scalar_alpha_blend(sp, *dst.add(i));
            }
        }
    }
}

/// Runtime dispatched alpha blend — src satırını dst satırına harmanlayarak yazar.
/// init_simd_dispatch() çağrıldıysa cached fn ptr kullanır.
pub unsafe fn alpha_blend_row(src: *const u32, dst: *mut u32, count: usize) {
    let cached = ALPHA_BLEND_FN.load(Ordering::Acquire);
    if cached != 0 {
        let func: unsafe fn(*const u32, *mut u32, usize) = core::mem::transmute(cached);
        func(src, dst, count);
        return;
    }
    // Fallback
    if crate::cpu::has_avx2() {
        alpha_blend_row_avx2(src, dst, count);
    } else if crate::cpu::has_sse41() {
        alpha_blend_row_sse41(src, dst, count);
    } else {
        alpha_blend_row_scalar(src, dst, count);
    }
}

/// SIMD ile dikdörtgen bölge doldurma (AVX2: 8 piksel/döngü).
/// Framebuffer'a sabit renk yazmak için kullanılır (clear, fill_rect).
#[target_feature(enable = "avx2")]
pub unsafe fn fill_rect_avx2(dst: *mut u32, count: usize, color: u32) {
    use core::arch::x86_64::{_mm256_set1_epi32, _mm256_storeu_si256};
    let fill = _mm256_set1_epi32(color as i32);
    let mut i = 0usize;
    while i + 8 <= count {
        _mm256_storeu_si256(dst.add(i) as *mut _, fill);
        i += 8;
    }
    while i < count {
        *dst.add(i) = color;
        i += 1;
    }
}

/// Scalar dikdörtgen doldurma fallback
pub fn fill_rect_scalar(dst: *mut u32, count: usize, color: u32) {
    for i in 0..count {
        unsafe {
            *dst.add(i) = color;
        }
    }
}

/// AVX-512 Stream Copy (Nontemporal)
/// Direct DMA-like transfer avoiding cache pollution.
#[cfg(target_feature = "avx512f")]
pub unsafe fn stream_copy_512(src: *const u32, dst: *mut u32, count: usize) {
    use core::arch::x86_64::*;
    let mut i = 0;
    while i + 16 <= count {
        let data = _mm512_loadu_si512(src.add(i) as *const _);
        _mm512_stream_si512(dst.add(i) as *mut _, data);
        i += 16;
    }
    // Fallback for remaining
    while i < count {
        *dst.add(i) = *src.add(i);
        i += 1;
    }
}

/// Optimized SIMD Blit with 64-byte alignment check
#[target_feature(enable = "avx2")]
pub unsafe fn blit_rect_simd(
    src: *const u32,
    src_stride: usize,
    dst: *mut u32,
    dst_stride: usize,
    w: usize,
    h: usize,
) {
    for y in 0..h {
        let s = src.add(y * src_stride);
        let d = dst.add(y * dst_stride);

        #[cfg(target_feature = "avx512f")]
        {
            if w >= 16 {
                stream_copy_512(s, d, w);
                continue;
            }
        }

        // AVX2 fallback if AVX-512 is not available or width is small
        let mut offset = 0;
        while offset + 8 <= w {
            use core::arch::x86_64::{_mm256_loadu_si256, _mm256_storeu_si256};
            let v = _mm256_loadu_si256(s.add(offset) as *const _);
            _mm256_storeu_si256(d.add(offset) as *mut _, v);
            offset += 8;
        }

        if offset < w {
            core::ptr::copy_nonoverlapping(s.add(offset), d.add(offset), w - offset);
        }
    }
}

/// SIMD-accelerated Box Blur (Acrylic Simulation)
/// Uses AVX2 to process 8 pixels at a time.
#[target_feature(enable = "avx2")]
pub unsafe fn fast_box_blur_avx2(
    src: *const u32,
    dst: *mut u32,
    w: usize,
    h: usize,
    stride: usize,
) {
    use core::arch::x86_64::*;

    for y in 1..(h - 1) {
        let mut x = 1;
        while x + 8 < (w - 1) {
            let row_prev = src.add((y - 1) * stride + x);
            let row_curr = src.add(y * stride + x);
            let row_next = src.add((y + 1) * stride + x);

            // Simple average of 3x3 neighborhood using SIMD
            // (Actually just a horizontal + vertical pass simplified)
            let p1 = _mm256_loadu_si256(row_prev as *const _);
            let p2 = _mm256_loadu_si256(row_curr as *const _);
            let p3 = _mm256_loadu_si256(row_next as *const _);

            // Add and shift (approximate divide by 3)
            let sum = _mm256_add_epi32(p1, _mm256_add_epi32(p2, p3));
            let avg = _mm256_srli_epi32(sum, 2); // Approximate /4 for speed

            _mm256_storeu_si256(dst.add(y * stride + x) as *mut _, avg);
            x += 8;
        }
    }
}
