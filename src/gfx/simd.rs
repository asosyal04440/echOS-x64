//! # echOS SIMD Grafik Operasyonları
//!
//! AVX2/SSE optimizasyonları için grafik fonksiyonları.

/// AVX2 ile bellek kopyalama.
#[target_feature(enable = "avx2")]
pub unsafe fn stream_copy_avx2(src: *const u8, dst: *mut u8, len: usize) {
    use core::arch::x86_64::{_mm256_loadu_si256, _mm256_storeu_si256};
    if len == 0 {
        return;
    }
    let mut offset = 0usize;
    let align = (dst as usize) & 31;
    if align != 0 {
        let prefix = (32 - align).min(len);
        core::ptr::copy_nonoverlapping(src, dst, prefix);
        offset += prefix;
    }
    while offset + 32 <= len {
        let v = _mm256_loadu_si256(src.add(offset) as *const _);
        _mm256_storeu_si256(dst.add(offset) as *mut _, v);
        offset += 32;
    }
    if offset < len {
        core::ptr::copy_nonoverlapping(src.add(offset), dst.add(offset), len - offset);
    }
}

#[target_feature(enable = "sse2")]
pub unsafe fn stream_copy_sse2(src: *const u8, dst: *mut u8, len: usize) {
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
        core::ptr::copy_nonoverlapping(src, dst, len);
    }
}

/// `count` adet ardarda gelen `u32` hücresiö `val` ile doldurur.
///
/// ## `rep stosd` Nedir?
///
/// `STOSD` = “Store String Double-word” — `[RDI] = EAX` yaz, `RDI += 4`, `RCX--`.
/// `REP` öneki bunu `RCX` sıfır olana kadar tekrarlar — tek bir talimat olarak.
///
/// ```asm
///   mov rcx, count    ; kaç kez
///   mov rdi, dst      ; nereye
///   mov eax, val      ; ne yaz
///   rep stosd         ; döngü (CPU mikroop özel-case ile optimize eder)
/// ```
///
/// ## Neden AVX/SSE Değil?
///
/// `rep stosd` basit fill için AVX2 yaklasan hızda çalışır ve CPU bunu
/// dahili olarak özel “fast string” mikroop'larıyla işler. Ayrıca YMM
/// register kaydetme/yükleme overhead'i gerekmez.
///
/// Framebuffer temizleme (tek renk) için ideal, çok renklerde
/// `stream_copy` + src buffer daha verimlidir.
///
/// # Safety
/// `dst`, `count * 4` byte yazım için geçerli ve hizalı olmalıdır.
pub unsafe fn fill_u32(dst: *mut u32, val: u32, count: usize) {
    if count == 0 {
        return;
    }
    // Use rep stosd for bulk fill
    core::arch::asm!(
        "rep stosd",
        inout("rcx") count => _,
        inout("rdi") dst   => _,
        in("eax") val,
        options(nostack, preserves_flags)
    );
}
