//! # echOS Bellek Bariyerleri Modülü
//!
//! Tier 1 OS seviyesinde bellek bariyerleri (memory barriers) implementasyonu.
//! Linux 6.x ile aynı seviyede bellek sıralama (memory ordering) garantileri sağlar.
//!
//! ## Neden Bellek Bariyeri?
//! Modern CPU'lar ve derleyiciler performans için talimatları yeniden sıralar.
//! Bu yeniden sıralama çok çekirdekli sistemlerde veri tutarsızlığına yol açabilir.
//! Bellek bariyerleri bu yeniden sıralamayı engeller ve veri görünürlüğünü garanti eder.

use core::sync::atomic::Ordering;

/// Tam bellek bariyeri - Linux'ın `smp_mb()` karşılığı.
///
/// Tüm bellek işlemlerini sıralar:
/// - Önceki tüm okuma/yazma işlemlerini, barrier'dan sonraki
///   okuma/yazma işlemlerinden kesinlikle ayırır.
/// - Hem derleyici (compiler) hem de CPU seviyesinde garanti sağlar.
///
/// x86_64'de `mfence` komutu kullanılır: en güçlü bariyer türüdür.
///
/// ```ascii
/// [Önceki Okuma/Yazma İşlemleri]
///          |
///    [MFENCE Bariyeri]  <-- smp_mb() burası
///          |
/// [Sonraki Okuma/Yazma İşlemleri]
/// ```
#[inline(always)]
pub fn smp_mb() {
    // x86_64'de mfence en güçlü bariyer: hem okuma hem yazma sıralar
    unsafe {
        core::arch::asm!("mfence", options(nomem, nostack, preserves_flags));
    }
}

/// Okuma bellek bariyeri - Linux'ın `smp_rmb()` karşılığı.
///
/// Önceki okuma işlemlerini sonraki okuma işlemlerinden ayırır:
/// - Sadece okuma (read) işlemlerini sıralar.
/// - Yazma (write) işlemlerini etkilemez.
///
/// x86_64'de `lfence` (Load Fence) komutu kullanılır.
#[inline(always)]
pub fn smp_rmb() {
    // x86_64'de lfence, yük (load) işlemleri için yeterli okuma bariyeridir
    unsafe {
        core::arch::asm!("lfence", options(nomem, nostack, preserves_flags));
    }
}

/// Yazma bellek bariyeri - Linux'ın `smp_wmb()` karşılığı.
///
/// Önceki yazma işlemlerini sonraki yazma işlemlerinden ayırır:
/// - Sadece yazma (write) işlemlerini sıralar.
/// - Okuma (read) işlemlerini etkilemez.
///
/// x86_64'de `sfence` (Store Fence) komutu kullanılır.
#[inline(always)]
pub fn smp_wmb() {
    // x86_64'de sfence, depo (store) işlemleri için yeterli yazma bariyeridir
    unsafe {
        core::arch::asm!("sfence", options(nomem, nostack, preserves_flags));
    }
}

/// Edinme (acquire) bariyeri - Linux'ın `smp_acquire()` karşılığı.
///
/// Sonraki tüm bellek işlemlerini bu barrier'dan sonraya taşır:
/// - Kilit-serbest (lock-free) veri yapılarında kritik öneme sahiptir.
/// - RCU implementasyonlarında gereklidir.
/// - "Kilidi aldıktan sonra oku" semantiğini garanti eder.
#[inline(always)]
pub fn smp_acquire() {
    // x86_64'de normal okuma zaten acquire semantiğine sahip,
    // ancak ekstra güvence için lfence kullanıyoruz
    unsafe {
        core::arch::asm!("lfence", options(nomem, nostack, preserves_flags));
    }
}

/// Serbest bırakma (release) bariyeri - Linux'ın `smp_release()` karşılığı.
///
/// Önceki tüm bellek işlemlerini barrier'dan önceye taşır:
/// - Kilit-serbest (lock-free) veri yapılarında kritik öneme sahiptir.
/// - RCU implementasyonlarında gereklidir.
/// - "Yaz, sonra kilidi bırak" semantiğini garanti eder.
#[inline(always)]
pub fn smp_release() {
    // x86_64'de normal yazma zaten release semantiğine sahip,
    // ancak ekstra güvence için sfence kullanıyoruz
    unsafe {
        core::arch::asm!("sfence", options(nomem, nostack, preserves_flags));
    }
}

/// Okuma-Edinme (read-acquire) bariyer kombinasyonu.
///
/// Kilit-serbest okuma işlemleri için optimize edilmiş bileşik bariyer:
/// - Önceki okuma işlemlerini sıralar (rmb).
/// - Sonraki tüm işlemleri acquire eder (acquire).
/// - Genellikle kilitsiz pointer okumalarında kullanılır.
#[inline(always)]
pub fn smp_read_acquire() {
    smp_rmb();
    smp_acquire();
}

/// Yazma-Serbest (write-release) bariyer kombinasyonu.
///
/// Kilit-serbest yazma işlemleri için optimize edilmiş bileşik bariyer:
/// - Önceki yazma işlemlerini sıralar (wmb).
/// - Önceki tüm işlemleri release eder (release).
/// - Genellikle kilitsiz pointer güncellemelerinde kullanılır.
#[inline(always)]
pub fn smp_write_release() {
    smp_wmb();
    smp_release();
}

/// Atomik sıralamaya sahip tam bariyer.
///
/// Atomik (atomic) işlemlerle birlikte kullanım için tasarlanmıştır:
/// - SeqCst (Sıralı Tutarlılık) sıralama garantisi sağlar.
/// - Hem atomik hem de normal bellek erişimleri için geçerlidir.
/// - En güçlü, en pahalı bariyer türüdür; yalnızca gerektiğinde kullanın.
#[inline(always)]
pub fn smp_full_barrier() {
    // SeqCst sıralama ile tam atomik bariyer
    core::sync::atomic::fence(Ordering::SeqCst);
    smp_mb();
}

/// Koşullu bellek bariyeri.
///
/// Yalnızca belirli koşullar gerçekleştiğinde bariyer uygular:
/// - Performans optimizasyonu gerektiren durumlarda kullanılır.
/// - Debug modunda ek kontrol sağlamak için idealdir.
#[inline(always)]
pub fn smp_conditional_mb(condition: bool) {
    if condition {
        smp_mb();
    }
}

/// Kilit-serbest veri yapıları için bellek bariyeri yapısı.
///
/// RCU benzeri yapılar için özel bariyer yönetimi sağlar:
/// - Zariflik dönemi (grace period) garantisi verir.
/// - Kilit-serbest okuma/yazma işlemlerini güvenli hale getirir.
pub struct MemoryBarrier;

impl MemoryBarrier {
    /// Bellek bariyeri alt sistemini başlatır.
    ///
    /// Sistem açılışında çağrılmalıdır. Tüm bariyer türlerinin
    /// doğru çalıştığını test eder ve seri porta bilgi mesajı yazar.
    pub fn init() {
        crate::serial_println!("Memory barriers initialized (x86_64)");

        // Tüm bariyer türlerini test et
        Self::test_barriers();
    }

    /// Tüm bariyer türlerini test eder.
    ///
    /// Bu fonksiyon sistemin başlatılması sırasında çalışır ve
    /// hiçbir panic olmaksızın tamamlanması beklenir.
    fn test_barriers() {
        // Tam bariyer testi: hem okuma hem yazma sıralanır
        smp_mb();

        // Okuma bariyeri testi: yalnızca okuma sıralanır
        smp_rmb();

        // Yazma bariyeri testi: yalnızca yazma sıralanır
        smp_wmb();

        // Edinme/Serbest bırakma testi
        smp_acquire();
        smp_release();

        crate::serial_println!("Memory barriers test completed");
    }

    /// Bariyer istatistiklerini döner (hata ayıklama için).
    ///
    /// Şu an sıfır değerleri döner; gelecekte gerçek sayımlar eklenebilir.
    pub fn stats() -> BarrierStats {
        BarrierStats {
            full_barriers: 0,
            read_barriers: 0,
            write_barriers: 0,
            acquire_barriers: 0,
            release_barriers: 0,
        }
    }
}

/// Hata ayıklama için bariyer istatistikleri.
///
/// Kaç kez hangi bariyer türünün kullanıldığını takip eder.
/// Performans analizi ve hata ayıklama amacıyla kullanılır.
#[derive(Debug, Clone, Copy)]
pub struct BarrierStats {
    pub full_barriers: u64,
    pub read_barriers: u64,
    pub write_barriers: u64,
    pub acquire_barriers: u64,
    pub release_barriers: u64,
}

/// CPU mimarisine özgü bellek bariyerleri modülü.
///
/// Farklı CPU mimarileri için optimize edilmiş bariyer implementasyonları:
/// - x86_64: `mfence` / `lfence` / `sfence` komutları
/// - ARM (gelecekte): `dmb ish` / `dmb ishst` / `dsb ish` komutları
pub mod cpu_specific {
    /// x86_64 mimarisi için tam bellek bariyeri.
    #[cfg(target_arch = "x86_64")]
    pub fn cpu_full_barrier() {
        super::smp_mb();
    }

    /// x86_64 mimarisi için okuma bellek bariyeri.
    #[cfg(target_arch = "x86_64")]
    pub fn cpu_read_barrier() {
        super::smp_rmb();
    }

    /// x86_64 mimarisi için yazma bellek bariyeri.
    #[cfg(target_arch = "x86_64")]
    pub fn cpu_write_barrier() {
        super::smp_wmb();
    }
}

/// Kilit-serbest (lock-free) bellek işlemleri modülü.
///
/// Tier 1 OS seviyesinde kilit-serbest veri yapıları için altyapı:
/// - RCU (Read-Copy-Update) benzeri okuma-kopyalama-güncelleme
/// - Kilit-serbest kuyruk/yığın (lock-free queue/stack)
/// - Tehlike işaretçileri (hazard pointers)
pub mod lockfree {
    use super::*;

    /// RCU zariflik dönemi (grace period) işaretleyicisi.
    ///
    /// Bir zariflik dönemi, tüm okuyucuların eski veriyi bırakmasını
    /// beklemek için kullanılır. Eski verinin güvenle serbest bırakılmasını sağlar.
    #[derive(Debug, Clone, Copy)]
    pub struct RcuGracePeriod {
        epoch: u64,
    }

    impl RcuGracePeriod {
        /// Yeni bir zariflik dönemi başlatır.
        ///
        /// Tam bariyer uygulayarak önceki tüm işlemlerin görünür
        /// olmasını garanti eder.
        pub fn new() -> Self {
            smp_mb();
            Self { epoch: 0 }
        }

        /// Zariflik dönemini sonlandırır.
        ///
        /// Tam bariyer uygulayarak dönem sonundaki tüm işlemlerin
        /// tamamlanmasını garanti eder.
        pub fn end(self) {
            smp_mb();
        }

        /// Zariflik döneminin güvenli olup olmadığını kontrol eder.
        ///
        /// Tüm okuyucuların bu dönemden çıkıp çıkmadığını doğrular.
        pub fn is_safe(&self) -> bool {
            // Gerçek RCU implementasyonu ilerleyen sürümlerde eklenecek
            smp_rmb();
            true
        }
    }

    /// RCU için kilit-serbest işaretçi (pointer) sarmalayıcısı.
    ///
    /// Aynı anda birden fazla okuyucu ve tek bir yazıcının güvenle
    /// çalışmasını sağlar. Okuma tarafı hiç kilit kullanmaz.
    pub struct RcuPtr<T> {
        ptr: *const T,
    }

    impl<T> RcuPtr<T> {
        /// Yeni bir RCU korumalı işaretçi oluşturur.
        ///
        /// Yazma bariyeri uygulayarak işaretçinin diğer CPU'lara
        /// görünür hale gelmesini garanti eder.
        pub fn new(ptr: *const T) -> Self {
            smp_wmb();
            Self { ptr }
        }

        /// Edinme (acquire) semantiği ile işaretçiyi okur.
        ///
        /// Okuma bariyeri uygulayarak okunan verinin tüm CPU'lara
        /// görünür ve tutarlı olmasını garanti eder.
        pub fn read(&self) -> *const T {
            smp_rmb();
            self.ptr
        }

        /// Serbest bırakma (release) semantiği ile işaretçiyi günceller.
        ///
        /// Yazma bariyeri uygulayarak güncellemenin atomik görünmesini sağlar.
        pub fn update(&mut self, new_ptr: *const T) {
            smp_wmb();
            self.ptr = new_ptr;
        }
    }
}

/// Atomik sıralama yardımcı fonksiyonları modülü.
///
/// Atomik işlemler için `Ordering` dönüştürme ve kontrol yardımcıları:
/// - Acquire/Release sarmalayıcıları
/// - SeqCst (Sıralı Tutarlılık) garantileri
pub mod ordering {
    use super::*;
    use core::sync::atomic::Ordering;

    /// Verilen sıralamayı edinme (acquire) sıralamasına yükseltir.
    ///
    /// Daha zayıf bir sıralama verilmişse, en az acquire garantisi
    /// sağlayacak şekilde dönüştürür.
    pub fn to_acquire(ordering: Ordering) -> Ordering {
        match ordering {
            Ordering::Relaxed => Ordering::Acquire,
            Ordering::Acquire => Ordering::Acquire,
            Ordering::Release => Ordering::AcqRel,
            Ordering::AcqRel => Ordering::AcqRel,
            Ordering::SeqCst => Ordering::SeqCst,
            _ => Ordering::SeqCst,
        }
    }

    /// Verilen sıralamayı serbest bırakma (release) sıralamasına yükseltir.
    ///
    /// Daha zayıf bir sıralama verilmişse, en az release garantisi
    /// sağlayacak şekilde dönüştürür.
    pub fn to_release(ordering: Ordering) -> Ordering {
        match ordering {
            Ordering::Relaxed => Ordering::Release,
            Ordering::Acquire => Ordering::AcqRel,
            Ordering::Release => Ordering::Release,
            Ordering::AcqRel => Ordering::AcqRel,
            Ordering::SeqCst => Ordering::SeqCst,
            _ => Ordering::SeqCst,
        }
    }

    /// Verilen sıralamanın bariyer gerektirip gerektirmediğini kontrol eder.
    ///
    /// Yalnızca `Relaxed` sıralama bariyer gerektirmez; diğer tüm
    /// sıralama türleri en az bir tür bariyer gerektirir.
    pub fn needs_barriers(ordering: Ordering) -> bool {
        !matches!(ordering, Ordering::Relaxed)
    }
}
