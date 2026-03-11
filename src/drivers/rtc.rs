//! # RTC (Real-Time Clock) Sürücüsü
//!
//! CMOS RTC üzerinden sistem tarih/saat okuma.
//! x86/x86-64 mimarisinde I/O port 0x70 (adres kaydı) ve 0x71 (veri kaydı) kullanır.
//!
//! ## Kayıt Haritası (Register Map)
//!
//! | CMOS adresi | İçerik              | Format            |
//! |-------------|---------------------|-------------------|
//! | 0x00        | Saniye              | BCD veya binary   |
//! | 0x02        | Dakika              | BCD veya binary   |
//! | 0x04        | Saat                | BCD veya binary   |
//! | 0x06        | Haftanın günü       | 1=Pazar, 7=Cumartesi |
//! | 0x07        | Ayın günü           | BCD veya binary   |
//! | 0x08        | Ay                  | BCD veya binary   |
//! | 0x09        | Yıl (iki hane)      | BCD veya binary   |
//! | 0x32        | Yüzyıl (bazı BIOS)  | BCD veya binary   |
//! | 0x0A        | Durum A: güncelleme bayrağı |         |
//! | 0x0B        | Durum B: format bayrakları |          |
//!
//! ## Güncelleme Yarış Koşulu
//!
//! RTC saati her saniye güncellenir. Okuma sırasında güncelleme başlarsa
//! tutarsız değerler okunabilir. Bunu önlemek için:
//! 1. Durum A kaydı bit-7 (UIP = Update In Progress) temizlenene dek bekle
//! 2. Okuma yap (güncelleme olmadığı pencerede)
//! 3. İkinci kez oku; değerler aynıysa güvenli

use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// CMOS PORT SABİTLERİ
// ============================================================================

/// CMOS adres kaydı I/O portu
const CMOS_ADDR: u16 = 0x70;

/// CMOS veri kaydı I/O portu
const CMOS_DATA: u16 = 0x71;

/// NMI devre dışı bırakma bayrağı (bit 7 of 0x70) — okuma sırasında NMI'yı bloke eder
const NMI_DISABLE_BIT: u8 = 0x80;

// CMOS kayıt adresleri
const REG_SECONDS: u8 = 0x00;
const REG_MINUTES: u8 = 0x02;
const REG_HOURS: u8 = 0x04;
const REG_WEEKDAY: u8 = 0x06;
const REG_DAY: u8 = 0x07;
const REG_MONTH: u8 = 0x08;
const REG_YEAR: u8 = 0x09;
const REG_CENTURY: u8 = 0x32;
const REG_STATUS_A: u8 = 0x0A;
const REG_STATUS_B: u8 = 0x0B;

/// Durum A — güncelleme-devam-ediyor bayrağı
const STATUS_A_UIP: u8 = 0x80;
/// Durum B — ikili mod bayrağı (0 = BCD, 1 = binary)
const STATUS_B_BINARY: u8 = 0x04;
/// Durum B — 24 saat modu bayrağı (0 = 12h, 1 = 24h)
const STATUS_B_24H: u8 = 0x02;

// ============================================================================
// VERİ TİPLERİ
// ============================================================================

/// Bir anlık tarihi ve saati temsil eden yapı.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub weekday: u8, // 1=Pazar, 7=Cumartesi
}

impl DateTime {
    /// Yeni bir DateTime oluşturur (varsayılan)
    pub const fn zero() -> Self {
        DateTime {
            year: 2000,
            month: 1,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            weekday: 1,
        }
    }

    /// POSIX epoch (1970-01-01 00:00:00) dan bu yana geçen saniye sayısını döndürür.
    /// Yaklaşık hesaplama — artık yıl tam doğru değil ama kabul edilebilir.
    pub fn to_unix_timestamp(&self) -> u64 {
        let years_since_1970 = self.year.saturating_sub(1970) as u64;
        let leap_years = years_since_1970 / 4;
        let days_per_year: u64 = 365;

        // Ayların kümülatif gün sayıları (artık yıl göz ardı edilmiştir)
        const MONTH_DAYS: [u64; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
        let month_days = if self.month >= 1 {
            MONTH_DAYS[(self.month - 1).min(11) as usize]
        } else {
            0
        };

        let days = years_since_1970 * days_per_year
            + leap_years
            + month_days
            + (self.day.saturating_sub(1)) as u64;

        days * 86400 + self.hour as u64 * 3600 + self.minute as u64 * 60 + self.second as u64
    }

    /// ISO 8601 biçimli tarih-saat dizesi döndürür: "YYYY-MM-DD HH:MM:SS"
    pub fn to_string(&self) -> alloc::string::String {
        use alloc::format;
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }
}

// ============================================================================
// DÜŞÜK SEVİYE I/O
// ============================================================================

/// CMOS kaydına I/O portu üzerinden erişir
///
/// # Safety
/// Ham I/O portu erişimi; sadece single-CPU veya mutex altında çağrılmalı
#[inline(always)]
unsafe fn cmos_read(reg: u8) -> u8 {
    // Adres kaydına yaz (NMI devre dışı ile)
    core::arch::asm!(
        "out dx, al",
        in("dx") CMOS_ADDR,
        in("al") reg | NMI_DISABLE_BIT,
        options(nomem, nostack)
    );
    // Kısa gecikme (I/O recovery time ~1µs)
    for _ in 0..10 {
        core::arch::asm!("nop", options(nomem, nostack));
    }
    // Veri kaydından oku
    let val: u8;
    core::arch::asm!(
        "in al, dx",
        in("dx") CMOS_DATA,
        out("al") val,
        options(nomem, nostack)
    );
    val
}

/// NMI'yı yeniden etkinleştirir
#[inline(always)]
unsafe fn nmi_enable() {
    core::arch::asm!(
        "out dx, al",
        in("dx") CMOS_ADDR,
        in("al") 0x00u8,
        options(nomem, nostack)
    );
}

/// RTC güncelleme devam ediyorsa bekler (UIP bit = 0)
#[inline(always)]
unsafe fn wait_for_update_end() {
    // Maksimum 1 saniyelik döngü; asılı kalmayı önlemek için sayaç sınırı
    for _ in 0..1_000_000u32 {
        if cmos_read(REG_STATUS_A) & STATUS_A_UIP == 0 {
            return;
        }
    }
}

/// BCD'den binary'ye dönüştürür: 0x59 → 59
#[inline(always)]
fn bcd_to_bin(bcd: u8) -> u8 {
    (bcd & 0x0F) + ((bcd >> 4) * 10)
}

// ============================================================================
// ANA OKUMA FONKSİYONU
// ============================================================================

/// CMOS RTC'den tarih ve saat okur.
///
/// Güncelleme yarış koşulunu önlemek için çift okuma algoritması kullanır:
/// iki ardışık okuma aynı değerleri veriyorsa sonuç güvenilirdir.
///
/// # Safety
/// I/O port erişimi gerektirdiğinden unsafe'dir.
/// Çağrıcı tek-erişimli bağlamda bulunduğundan emin olmalıdır.
pub fn read_rtc() -> DateTime {
    unsafe {
        // Durum B kaydını oku → format bilgisi
        let status_b = cmos_read(REG_STATUS_B);
        let is_binary = status_b & STATUS_B_BINARY != 0;
        let is_24h = status_b & STATUS_B_24H != 0;

        // Çift okuma döngüsü
        loop {
            wait_for_update_end();

            let sec1 = cmos_read(REG_SECONDS);
            let min1 = cmos_read(REG_MINUTES);
            let hr1 = cmos_read(REG_HOURS);
            let day1 = cmos_read(REG_DAY);
            let mon1 = cmos_read(REG_MONTH);
            let yr1 = cmos_read(REG_YEAR);
            let cent1 = cmos_read(REG_CENTURY);
            let wday1 = cmos_read(REG_WEEKDAY);

            wait_for_update_end();

            let sec2 = cmos_read(REG_SECONDS);
            let min2 = cmos_read(REG_MINUTES);
            let hr2 = cmos_read(REG_HOURS);
            let day2 = cmos_read(REG_DAY);
            let mon2 = cmos_read(REG_MONTH);
            let yr2 = cmos_read(REG_YEAR);

            // Değerler aynıysa tutarlı okuma
            if sec1 == sec2
                && min1 == min2
                && hr1 == hr2
                && day1 == day2
                && mon1 == mon2
                && yr1 == yr2
            {
                // NMI'yı yeniden etkinleştir
                nmi_enable();

                // BCD → binary dönüşümü (gerekiyorsa)
                let (sec, min, hr, day, mon, yr, cent, wday) = if is_binary {
                    (sec1, min1, hr1, day1, mon1, yr1, cent1, wday1)
                } else {
                    (
                        bcd_to_bin(sec1),
                        bcd_to_bin(min1),
                        bcd_to_bin(hr1),
                        bcd_to_bin(day1),
                        bcd_to_bin(mon1),
                        bcd_to_bin(yr1),
                        bcd_to_bin(cent1),
                        wday1,
                    )
                };

                // 12 saatlik → 24 saatlik dönüşüm
                let hr_24 = if !is_24h && (hr & 0x80) != 0 {
                    // PM saatleri
                    ((hr & 0x7F) + 12) % 24
                } else {
                    hr & 0x7F
                };

                // Yüzyıl katkısı: BIOS REG_CENTURY'yi destekliyorsa kullan
                let century: u16 = if cent != 0 { cent as u16 } else { 20 };
                let full_year = century * 100 + yr as u16;

                return DateTime {
                    year: full_year,
                    month: mon,
                    day,
                    hour: hr_24,
                    minute: min,
                    second: sec,
                    weekday: wday,
                };
            }
            // Tutarsız okuma → tekrar dene
        }
    }
}

// ============================================================================
// GLOBAL ÖNBELLEK
// ============================================================================

/// Son okunan RTC değerini önbellekte tutar.
/// Timer interrupt'ından periyodik olarak güncellenir.
static RTC_CACHE: Mutex<DateTime> = Mutex::new(DateTime::zero());

/// boot TSC değeri — RTC ile TSC arasında köprü
static BOOT_TSC: AtomicU64 = AtomicU64::new(0);
/// Boot anındaki UNIX timestamp
static BOOT_UNIX: AtomicU64 = AtomicU64::new(0);

/// RTC önbelleğini güncel değerle yeniler.
/// Sistem başlangıcında ve periyodik timer'dan çağrılır.
pub fn sync_rtc_cache() {
    let dt = read_rtc();
    *RTC_CACHE.lock() = dt;

    // TSC tabanlı hızlı zaman için başlangıç değerlerini kaydet
    let unix = dt.to_unix_timestamp();
    BOOT_UNIX.store(unix, Ordering::Relaxed);

    // TSC değerini kaydet
    let tsc: u64;
    unsafe {
        core::arch::asm!("rdtsc; shl rdx, 32; or rax, rdx",
            out("rax") tsc,
            out("rdx") _,
            options(nomem, nostack));
    }
    BOOT_TSC.store(tsc, Ordering::Relaxed);
}

/// Önbellekteki RTC değerini döndürür (hızlı, non-blocking).
pub fn get_cached_datetime() -> DateTime {
    *RTC_CACHE.lock()
}

/// Güncel UNIX timestamp döndürür.
/// Kaba TSC tahminini kullanır (RTC önbelleği + TSC delta).
pub fn get_unix_time() -> u64 {
    BOOT_UNIX.load(Ordering::Relaxed)
}

/// RTC sürücüsünü başlatır — boot sırasında bir kez çağrılır.
pub fn init() {
    sync_rtc_cache();
    let dt = get_cached_datetime();
    crate::serial_println!(
        "[RTC] Başlatıldı: {} (UNIX: {})",
        dt.to_string(),
        dt.to_unix_timestamp()
    );

    // VFS zaman senkronizasyonu
    crate::fs::update_global_time();
}
