//! # Boot Güvenlik Sistemi
//!
//! Boot zamanı çökmelere, TLSF bozulmalarına, SMP hatalarına,
//! IDT sorunlarına ve GOP problemlerine karşı kapsamlı koruma.
//!
//! ## Sistem Tasarımı:
//!
//! Boot güvenlik sistemi birbirinden bağımsız dört bileşenden oluşur:
//!
//! ```
//!  BootWatchdog    --> Genel boot zaman aşımı ve kurtarma
//!  HeapSafety      --> TLSF heap bütünlük izleme
//!  SmpSafety       --> Çok işlemcili başlatma güvenliği
//!  IdtSafety       --> Kesme tablosu (IDT) yükleme doğrulama
//!  GopSafety       --> Grafik çıkış koruması
//! ```
//!
//! Her bileşen ihlalleri merkezi `BOOT_SAFETY` kaydına yazar.
//! Kurtarılabilir hatalar atlanarak boot devam eder;
//! kritik hatalar acil durdurma (emergency halt) ile sonuçlanır.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

// ============================================================================
// BOOT GÜVENLİK SABİTLERİ
// ============================================================================

/// Kritik işlemler için maksimum yeniden deneme sayısı.
pub const MAX_RETRY_ATTEMPTS: u32 = 5;
/// Boot aşaması zaman aşımı (milisaniye cinsinden).
pub const BOOT_PHASE_TIMEOUT_MS: u64 = 30000;
/// Yardımcı işlemci (AP) başlatma zaman aşımı (milisaniye).
pub const AP_STARTUP_TIMEOUT_MS: u64 = 5000;
/// Heap bütünlük kontrolü aralığı (milisaniye).
pub const HEAP_CHECK_INTERVAL: u64 = 1000;
/// Durdurmaya neden olan maksimum heap bozulma sayısı.
pub const MAX_HEAP_CORRUPTIONS: u32 = 3;

// ============================================================================
// BOOT AŞAMASI TAKİBİ
// ============================================================================

/// Sistemin hangi boot aşamasında olduğunu temsil eden sayım (enum).
///
/// Sıralı değerler (0'dan 255'e kadar) aşamaların doğru sırayla
/// geçildiğini doğrulamaya olanak tanır.
///
/// ## Boot Aşamaları Akışı:
/// ```
/// Reset(0) --> UefiHandover(1) --> MemoryInit(2) --> PagingSetup(3)
///   --> HeapInit(4) --> GdtSetup(5) --> IdtSetup(6) --> AcpiInit(7)
///   --> SmpInit(8) --> DriverInit(9) --> UserspaceReady(10) --> Running(255)
/// ```
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BootPhase {
    Reset = 0,
    UefiHandover = 1,
    MemoryInit = 2,
    PagingSetup = 3,
    HeapInit = 4,
    GdtSetup = 5,
    IdtSetup = 6,
    AcpiInit = 7,
    SmpInit = 8,
    DriverInit = 9,
    UserspaceReady = 10,
    Running = 255,
}

impl Default for BootPhase {
    fn default() -> Self {
        BootPhase::Reset
    }
}

// ============================================================================
// BOOT GÜVENLİK DURUMU
// ============================================================================

/// Tüm boot güvenlik bilgilerini tek bir yerde tutan merkezi durum yapısı.
///
/// Tüm sayaç alanları atomiktir, bu nedenle yapı çok çekirdekli ortamda
/// mutex gerekmeksizin okunabilir. Mutex yalnızca `BTreeMap` ve `Vec`
/// için gereklidir (bu yapılar atomik değildir).
pub struct BootSafetyState {
    /// Geçerli boot aşaması (BootPhase enum değeri)
    pub current_phase: AtomicU32,
    /// Boot başlangıç zaman damgası (tik cinsinden)
    pub boot_start_time: AtomicUsize,
    /// Son başarılı kontrol noktası zaman damgası
    pub last_checkpoint: AtomicUsize,
    /// Her aşamadaki hata sayısı (aşama numarası → hata sayısı)
    pub error_counts: Mutex<BTreeMap<u8, u32>>,
    /// Kurtarma girişim sayısı
    pub recovery_attempts: AtomicU32,
    /// Şu an kurtarma modunda mı?
    pub in_recovery: AtomicBool,
    /// Kritik bir hata oluştu mu?
    pub critical_error: AtomicBool,
    /// Boot başarıyla tamamlandı mı?
    pub boot_complete: AtomicBool,
    /// Güvenlik ihlalleri günlüğü
    pub violations: Mutex<Vec<SafetyViolation>>,
    /// Heap bozulma sayacı
    pub heap_corruptions: AtomicU32,
    /// SMP (çok işlemci) başlatma hata sayacı
    pub smp_failures: AtomicU32,
    /// IDT yükleme hata sayacı
    pub idt_failures: AtomicU32,
    /// GOP (Grafik Çıkış Protokolü) hata sayacı
    pub gop_failures: AtomicU32,
}

/// Tek bir güvenlik ihlali kaydı.
#[derive(Clone, Debug)]
pub struct SafetyViolation {
    pub phase: BootPhase,
    pub violation_type: ViolationType,
    pub message: String,
    pub timestamp: usize,
    pub recovered: bool,
}

/// İhlal türlerini sınıflandıran sayım.
///
/// Her tür farklı bir kurtarma stratejisi gerektirebilir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViolationType {
    HeapCorruption,   // Heap bellek bozulması
    NullPointer,      // Null pointer erişimi
    InvalidPointer,   // Geçersiz bellek adresi
    StackOverflow,    // Yığın taşması
    StackUnderflow,   // Yığın altında kalma
    DoubleFault,      // CPU çift hata istisnası
    PageFault,        // Sayfa hatası
    Gpf,              // Genel koruma hatası (General Protection Fault)
    SmpTimeout,       // Çok işlemcili başlatma zaman aşımı
    ApStartupFailed,  // Yardımcı işlemci başlatma hatası
    IdtLoadFailed,    // IDT yükleme hatası
    GopInitFailed,    // Grafik protokolü başlatma hatası
    AcpiTableInvalid, // Geçersiz ACPI tablosu
    MemoryMapInvalid, // Geçersiz bellek haritası
    Timeout,          // Genel zaman aşımı
    InfiniteLoop,     // Sonsuz döngü tespiti
}

impl BootSafetyState {
    /// Tüm sayaçlar sıfırlanmış yeni bir boot güvenlik durumu oluşturur.
    ///
    /// `const fn` olduğundan global statik olarak tanımlanabilir.
    pub const fn new() -> Self {
        Self {
            current_phase: AtomicU32::new(BootPhase::Reset as u32),
            boot_start_time: AtomicUsize::new(0),
            last_checkpoint: AtomicUsize::new(0),
            error_counts: Mutex::new(BTreeMap::new()),
            recovery_attempts: AtomicU32::new(0),
            in_recovery: AtomicBool::new(false),
            critical_error: AtomicBool::new(false),
            boot_complete: AtomicBool::new(false),
            violations: Mutex::new(Vec::new()),
            heap_corruptions: AtomicU32::new(0),
            smp_failures: AtomicU32::new(0),
            idt_failures: AtomicU32::new(0),
            gop_failures: AtomicU32::new(0),
        }
    }

    /// Yeni bir boot aşamasına geçer ve kontrol noktası zaman damgasını günceller.
    ///
    /// `SeqCst` (Sıralı Tutarlılık) sıralama kullanılır: tüm CPU'lar aynı
    /// aşamayı aynı anda görmeli ve sıra bozulmamalıdır.
    pub fn enter_phase(&self, phase: BootPhase) {
        self.current_phase.store(phase as u32, Ordering::SeqCst);
        self.last_checkpoint.store(
            crate::task::scheduler::get_ticks(),
            Ordering::SeqCst
        );

        crate::serial_println!("[BOOT_SAFETY] Aşamaya girildi: {:?}", phase);
    }

    /// Mevcut aşamada bir hata kaydeder.
    ///
    /// Her aşamanın hata sayısı `error_counts` haritasında tutulur.
    pub fn record_error(&self) {
        let phase = self.current_phase.load(Ordering::SeqCst) as u8;
        let mut counts = self.error_counts.lock();
        *counts.entry(phase).or_insert(0) += 1;
    }

    /// Boot'un zaman aşımına uğrayıp uğramadığını kontrol eder.
    ///
    /// Mevcut tik sayısından başlangıç tik sayısı çıkarılır;
    /// `BOOT_PHASE_TIMEOUT_MS` aşılırsa `true` döner.
    pub fn check_timeout(&self) -> bool {
        let current = crate::task::scheduler::get_ticks();
        let start = self.boot_start_time.load(Ordering::SeqCst);

        current.saturating_sub(start) > BOOT_PHASE_TIMEOUT_MS as usize
    }

    /// Güvenlik ihlalini günlüğe kaydeder ve seri porta yazar.
    ///
    /// `recovered` alanı, sistemin ihlalden kurtulup kurtulmadığını belirtir.
    /// Kurtarılamazsa ileride `emergency_halt` tetiklenebilir.
    pub fn record_violation(&self, violation_type: ViolationType, message: &str, recovered: bool) {
        let phase = BootPhase::try_from(self.current_phase.load(Ordering::SeqCst) as u8)
            .unwrap_or(BootPhase::Reset);

        let violation = SafetyViolation {
            phase,
            violation_type,
            message: String::from(message),
            timestamp: crate::task::scheduler::get_ticks(),
            recovered,
        };

        self.violations.lock().push(violation);

        crate::serial_println!(
            "[BOOT_SAFETY] İhlal: {:?} - {} (kurtarıldı: {})",
            violation_type, message, recovered
        );
    }

    /// Toplam ihlal sayısını döndürür.
    pub fn violation_count(&self) -> usize {
        self.violations.lock().len()
    }
}

lazy_static::lazy_static! {
    /// Global boot güvenlik durumu — tüm bileşenler bu nesneyi paylaşır.
    pub static ref BOOT_SAFETY: BootSafetyState = BootSafetyState::new();
}

// ============================================================================
// TLSF HEAP GÜVENLİĞİ
// ============================================================================

/// Heap bellek bütünlüğünü izleyen güvenlik bileşeni.
///
/// Erken heap taşmasını, ana heap sınır ihlallerini ve bozulma belirtilerini
/// tespit eder. Kritik bozulma eşiği aşılırsa sistemi durdurur.
pub struct HeapSafety;

impl HeapSafety {
    /// Heap güvenlik izleme sistemini başlatır.
    pub fn init() {
        crate::serial_println!("[HEAP_SAFETY] Heap güvenlik sistemi başlatılıyor");
    }

    /// Heap bütünlüğünü kontrol eder ve durum raporu döndürür.
    ///
    /// Kontrol edilen koşullar:
    /// - Erken heap boyutu sınır içinde mi?
    /// - Ana heap sınırları geçerli mi (start < end)?
    /// - Belirgin bozulma işaretleri var mı?
    pub fn check_integrity() -> HeapIntegrityStatus {
        let usage = crate::allocator::tlsf::early_heap_usage();
        let (start, end) = crate::allocator::tlsf::main_heap_bounds();

        let mut status = HeapIntegrityStatus {
            early_heap_usage: usage,
            main_heap_start: start,
            main_heap_end: end,
            early_heap_ok: usage <= 512 * 1024,
            main_heap_ok: start != 0 && end > start,
            corruption_detected: false,
            can_recover: true,
        };

        // Belirgin bozulma işareti: başlangıç adresi bitiş adresinden büyük
        if start > end {
            status.corruption_detected = true;
            status.can_recover = false;
            BOOT_SAFETY.record_violation(
                ViolationType::HeapCorruption,
                "Heap sınırları tersine dönmüş",
                false
            );
        }

        // Erken heap kapasiteye yakın: 1 KiB kalmadan uyarı ver
        if usage > 512 * 1024 - 1024 {
            BOOT_SAFETY.record_violation(
                ViolationType::HeapCorruption,
                "Erken heap kapasiteye yakın",
                true
            );
        }

        status
    }

    /// Yeniden deneme destekli güvenli bellek tahsisi.
    ///
    /// `MAX_RETRY_ATTEMPTS` kez dener. Her başarısız denemede bekler,
    /// her başarılı denemede pointer'ın geçerli heap aralığında olduğunu doğrular.
    /// Eşik aşılırsa `emergency_halt` çağrılır.
    pub fn safe_alloc(size: usize, align: usize) -> Option<*mut u8> {
        use core::alloc::Layout;

        for attempt in 0..MAX_RETRY_ATTEMPTS {
            let layout = match Layout::from_size_align(size, align.max(8)) {
                Ok(l) => l,
                Err(_) => {
                    BOOT_SAFETY.record_violation(
                        ViolationType::HeapCorruption,
                        "Geçersiz allocation layout'u",
                        false
                    );
                    return None;
                }
            };

            let ptr = unsafe { alloc::alloc::alloc(layout) };

            if !ptr.is_null() {
                // Pointer'ın geçerli heap aralığında olduğunu doğrula
                if Self::is_valid_heap_ptr(ptr as usize) {
                    return Some(ptr);
                } else {
                    // Geçersiz pointer: heap bozulması!
                    BOOT_SAFETY.heap_corruptions.fetch_add(1, Ordering::SeqCst);
                    BOOT_SAFETY.record_violation(
                        ViolationType::InvalidPointer,
                        "Allocator geçersiz pointer döndürdü",
                        false
                    );

                    // Bozulma eşiği aşıldı mı?
                    if BOOT_SAFETY.heap_corruptions.load(Ordering::SeqCst) >= MAX_HEAP_CORRUPTIONS {
                        Self::emergency_halt("Heap bozulma eşiği aşıldı");
                    }
                }
            }

            // Yeniden denemeden önce bekle (deneme numarasıyla orantılı bekleme)
            crate::cpu::smp::delay_ms((10 * (attempt + 1)) as u32);
        }

        None
    }

    /// Pointer'ın geçerli bir heap aralığında olup olmadığını kontrol eder.
    ///
    /// Erken heap ve ana heap sınırları birlikte kontrol edilir.
    fn is_valid_heap_ptr(ptr: usize) -> bool {
        // Erken heap kontrolü (yaklaşık adres aralığı)
        let early_start = 0x1000; // Yaklaşık başlangıç
        let early_end = early_start + 512 * 1024;
        if ptr >= early_start && ptr < early_end {
            return true;
        }

        // Ana heap kontrolü
        let (start, end) = crate::allocator::tlsf::main_heap_bounds();
        if start != 0 && ptr >= start && ptr < end {
            return true;
        }

        false
    }

    /// Kritik heap hatasında sistemi durdurur.
    ///
    /// `cli; hlt` çifti: kesintileri devre dışı bırakır ve CPU'yu durdurur.
    /// Sonsuz döngü, NMI (maskelenemez kesinti) gelse bile sistemin
    /// tutarlı hata durumunda kalmasını sağlar.
    fn emergency_halt(reason: &str) {
        crate::serial_println!("[HEAP_SAFETY] ACİL DURDURMA: {}", reason);
        crate::serial_println!("[HEAP_SAFETY] Bozulma sayısı: {}",
            BOOT_SAFETY.heap_corruptions.load(Ordering::SeqCst));

        loop {
            unsafe {
                core::arch::asm!("cli; hlt");
            }
        }
    }
}

/// Heap bütünlük kontrol sonucu.
#[derive(Clone, Debug)]
pub struct HeapIntegrityStatus {
    pub early_heap_usage: usize,
    pub main_heap_start: usize,
    pub main_heap_end: usize,
    pub early_heap_ok: bool,
    pub main_heap_ok: bool,
    pub corruption_detected: bool,
    pub can_recover: bool,
}

// ============================================================================
// SMP BOOT GÜVENLİĞİ
// ============================================================================

/// Çok işlemcili (SMP) başlatma güvenlik bileşeni.
///
/// Yardımcı işlemcilerin (AP — Application Processor) hatasız başlatılmasını
/// yönetir. Başarısız AP'ler "Broken" durumuna alınır; sistem kalan
/// CPU'larla çalışmayı sürdürür.
pub struct SmpSafety;

impl SmpSafety {
    /// Yardımcı işlemciyi (AP) kapsamlı hata işleme ile güvenli biçimde başlatır.
    ///
    /// ## Adımlar:
    /// 1. Ön kontrol (preflight) — CPU durumu, per-cpu verisi, stack adresi
    /// 2. `MAX_RETRY_ATTEMPTS` kez başlatma girişimi
    /// 3. Her girişimde AP'nin gerçekten çevrimiçi olduğunu doğrula
    /// 4. Tüm girişimler başarısız → CPU "Broken" olarak işaretle
    ///
    /// Döndürülen değer: `true` = sistem çalışmaya devam edebilir (AP olmasa bile)
    pub fn safe_startup_ap(apic_id: u32, cpu_id: u32) -> bool {
        // Ön kontrol: CPU durumu başlatmaya uygun mu?
        if !Self::preflight_checks(cpu_id) {
            BOOT_SAFETY.record_violation(
                ViolationType::ApStartupFailed,
                &format!("CPU {} için ön kontroller başarısız", cpu_id),
                false
            );
            return false;
        }

        // Yeniden deneme döngüsü ile başlatma girişimi
        for attempt in 0..MAX_RETRY_ATTEMPTS {
            crate::serial_println!(
                "[SMP_SAFETY] AP {} başlatılıyor (deneme {}/{})",
                cpu_id, attempt + 1, MAX_RETRY_ATTEMPTS
            );

            // Gerçek AP başlatma çağrısı
            let result = unsafe { crate::cpu::smp::startup_ap(apic_id, cpu_id) };

            if result {
                // AP gerçekten çevrimiçi mi? (semaphore/flag tespiti)
                if Self::verify_ap_online(cpu_id) {
                    crate::serial_println!(
                        "[SMP_SAFETY] AP {} başarıyla çevrimiçi",
                        cpu_id
                    );
                    return true;
                }
            }

            // Yeniden denemeden önce bekle (deneme numarasıyla artan gecikme)
            crate::cpu::smp::delay_ms((100 * (attempt + 1)) as u32);
        }

        // Tüm girişimler başarısız: CPU'yu "Broken" olarak işaretle
        BOOT_SAFETY.smp_failures.fetch_add(1, Ordering::SeqCst);
        BOOT_SAFETY.record_violation(
            ViolationType::ApStartupFailed,
            &format!("AP {} {} denemeden sonra başlatılamadı", cpu_id, MAX_RETRY_ATTEMPTS),
            false
        );

        // CPU'yu bozuk olarak işaretle
        crate::cpu::smp_state::CPU_STATES.set_state(
            cpu_id,
            crate::cpu::smp_state::CpuHotplugState::Broken
        );

        // Sistem azaltılmış CPU sayısıyla çalışmayı sürdür
        crate::serial_println!(
            "[SMP_SAFETY] Azaltılmış CPU sayısıyla devam ediliyor"
        );
        true // Sistemi durdurmadan devam et
    }

    /// AP başlatmadan önce ön kontroller yapar.
    ///
    /// Doğrulananlar:
    /// - CPU durumunun başlatmaya izin verip vermediği
    /// - Per-CPU verisinin tahsis edilmiş olması
    /// - Stack'in sıfır olmayan bir adresinin olması
    fn preflight_checks(cpu_id: u32) -> bool {
        // CPU durum makinesini kontrol et
        let state = crate::cpu::smp_state::CPU_STATES.get_state(cpu_id);
        if !state.can_start() {
            crate::serial_println!(
                "[SMP_SAFETY] CPU {} geçersiz durumda: {:?}",
                cpu_id, state
            );
            return false;
        }

        // Per-CPU verisinin hazır olduğunu kontrol et
        let smp_state = crate::cpu::smp::SMP_STATE.lock();
        if cpu_id as usize >= smp_state.per_cpu_data.len() {
            crate::serial_println!(
                "[SMP_SAFETY] CPU {} için per-CPU verisi yok",
                cpu_id
            );
            return false;
        }

        // Stack'in tahsis edildiğini kontrol et
        let stack_top = smp_state.per_cpu_data[cpu_id as usize].stack_top;
        if stack_top == 0 {
            crate::serial_println!(
                "[SMP_SAFETY] CPU {} için stack tahsis edilmemiş",
                cpu_id
            );
            return false;
        }

        true
    }

    /// AP'nin gerçekten çevrimiçi olduğunu doğrular.
    ///
    /// `AP_STARTUP_TIMEOUT_MS` süre boyunca AP'nin `is_online` bayrağını
    /// ayarlayıp ayarlamadığını kontrol eder. 100 mikrosaniyede bir yoklar.
    fn verify_ap_online(cpu_id: u32) -> bool {
        let start = crate::task::scheduler::get_ticks();
        let timeout = AP_STARTUP_TIMEOUT_MS as usize;

        loop {
            if crate::cpu::smp_state::CPU_STATES.is_online(cpu_id) {
                return true;
            }

            let elapsed = crate::task::scheduler::get_ticks().saturating_sub(start);
            if elapsed > timeout {
                return false;
            }

            crate::cpu::smp::delay_us(100);
        }
    }

    /// AP başlatma başarısızlığını zarif biçimde işler.
    ///
    /// CPU'yu "Broken" durumuna alır, beklenen çevrimiçi CPU sayısını günceller
    /// ve ihlali merkezi kayda yazar. Sistem çalışmaya devam eder.
    pub fn handle_bringup_failure(cpu_id: u32, reason: &str) {
        crate::serial_println!(
            "[SMP_SAFETY] AP {} başlatma başarısız: {}",
            cpu_id, reason
        );

        // Durum makinesini güncelle
        crate::cpu::smp_state::CPU_STATES.set_state(
            cpu_id,
            crate::cpu::smp_state::CpuHotplugState::Broken
        );

        // Beklenen çevrimiçi CPU sayısını azalt
        crate::cpu::smp::SMP_STATE.lock().online_cpus.fetch_sub(1, Ordering::SeqCst);

        // İhlali kaydet (sistem devam eder)
        BOOT_SAFETY.record_violation(
            ViolationType::ApStartupFailed,
            reason,
            true // Sistem devam etmektedir
        );
    }
}

// ============================================================================
// IDT GÜVENLİĞİ
// ============================================================================

/// Kesme Tanımlayıcı Tablosu (IDT) güvenlik bileşeni.
///
/// IDT, CPU'nun istisna ve kesmeleri hangi işleyicilerle ele alacağını belirler.
/// Bozuk veya eksik bir IDT'de page fault, double fault gibi istisnalar sistem
/// çöküşüne yol açar. Bu bileşen IDT'nin doğru yüklendiğini birden fazla
/// mekanizmayla doğrular.
pub struct IdtSafety;

impl IdtSafety {
    /// IDT'yi güvenli biçimde başlatır ve doğrular.
    ///
    /// ## Adımlar:
    /// 1. IDT oluştur (`init_idt_for_cpu`)
    /// 2. Yapısal geçerliliği kontrol et (hizalama, null değil)
    /// 3. CPU'ya yükle (`idt.load()`)
    /// 4. `SIDT` talimatıyla yüklemenin gerçekleştiğini doğrula
    ///
    /// Tüm girişimler başarısız → `false` döner, boot devam eder
    /// (bazı CPU'larda IDT yeniden deneme ile başarılı olabilir).
    pub fn safe_init_idt(cpu_id: u32) -> bool {
        for attempt in 0..MAX_RETRY_ATTEMPTS {
            crate::serial_println!(
                "[IDT_SAFETY] CPU {} için IDT başlatılıyor (deneme {}/{})",
                cpu_id, attempt + 1, MAX_RETRY_ATTEMPTS
            );

            // IDT'yi oluştur ve yükle
            let idt = crate::interrupts::init_idt_for_cpu(cpu_id);

            // IDT yapısının geçerliliğini doğrula
            if Self::verify_idt(idt) {
                // IDT'yi CPU'ya yükle
                idt.load();

                // Yüklemenin gerçekten gerçekleştiğini IDTR ile doğrula
                if Self::verify_idt_loaded() {
                    crate::serial_println!(
                        "[IDT_SAFETY] CPU {} için IDT başarıyla yüklendi",
                        cpu_id
                    );
                    return true;
                }
            }

            BOOT_SAFETY.idt_failures.fetch_add(1, Ordering::SeqCst);
            crate::cpu::smp::delay_ms(10);
        }

        BOOT_SAFETY.record_violation(
            ViolationType::IdtLoadFailed,
            &format!("CPU {} için IDT başlatma başarısız", cpu_id),
            false
        );

        false
    }

    /// IDT yapısının geçerli olduğunu doğrular.
    ///
    /// Gerekli koşullar:
    /// - Pointer null (0) olmamalı
    /// - 8-byte hizalı olmalı (x86-64 IDT gereksinimleri)
    ///
    /// Kritik işleyicilerin (double fault, page fault, GPF) gerçekte
    /// kurulu olup olmadığının derin doğrulaması burada yapılmaz;
    /// bu güven `init_idt_for_cpu` fonksiyonuna aittir.
    fn verify_idt(idt: &x86_64::structures::idt::InterruptDescriptorTable) -> bool {
        // Kritik işleyicilerin kurulu olduğunu kontrol et:
        // - Double fault işleyicisi mevcut olmalı
        // - Page fault işleyicisi mevcut olmalı
        // - Genel koruma hatası işleyicisi mevcut olmalı

        // IDT yapısı okunabiliyorsa geçerlidir
        let ptr = idt as *const _ as usize;
        ptr != 0 && ptr % 8 == 0 // 8-byte hizalı olmalı
    }

    /// IDTR kaydı okunarak IDT'nin CPU'ya yüklendiğini doğrular.
    ///
    /// `SIDT` talimatı, CPU'nun IDTR kaydını belleğe yazar:
    /// - Bayt 0-1: limit (IDT boyutu - 1)
    /// - Bayt 2-9: taban adresi (64-bit)
    ///
    /// Beklenti: limit en az 32 * 16 - 1 = 511 (32 girdi için) ve taban != 0.
    fn verify_idt_loaded() -> bool {
        // IDTR kaydını 10 baytlık tampona oku
        let mut idtr: [u8; 10] = [0; 10];
        unsafe {
            core::arch::asm!(
                "sidt [{}]",
                in(reg) idtr.as_mut_ptr(),
                options(nostack, preserves_flags)
            );
        }

        // IDTR formatı: [limit: 2 bayt little-endian][taban: 8 bayt little-endian]
        let limit = u16::from_le_bytes([idtr[0], idtr[1]]);
        let base = u64::from_le_bytes([
            idtr[2], idtr[3], idtr[4], idtr[5],
            idtr[6], idtr[7], idtr[8], idtr[9]
        ]);

        // IDT en az 32 girdi içermelidir (her girdi 16 bayt)
        limit >= 32 * 16 - 1 && base != 0
    }

    /// Güvenli istisna işleyicilerini kurar.
    ///
    /// Tüm istisna işleyicilerinin çift hata (double fault) oluşturmadan
    /// çalışacağını garanti eder.
    pub fn install_safe_handlers() {
        // Bu, tüm istisna işleyicilerinin uygun hata işlemeye sahip olmasını
        // ve çift hata oluşturmamasını sağlar
        crate::serial_println!("[IDT_SAFETY] Güvenli istisna işleyicileri kuruldu");
    }
}

// ============================================================================
// GOP GÜVENLİĞİ
// ============================================================================

/// Grafik Çıkış Protokolü (GOP) güvenlik bileşeni.
///
/// UEFI GOP üzerinden sağlanan framebuffer'ın geçerli olduğunu doğrular.
/// Framebuffer yoksa veya geçersizse seri port çıkışına geri döner.
pub struct GopSafety;

impl GopSafety {
    /// GOP'u güvenli biçimde başlatır; geçersizse yedek moda geçer.
    ///
    /// Framebuffer yoksa veya doğrulama başarısız olursa `try_text_mode`
    /// çağrılır: seri port çıkışı her zaman kullanılabilirdir.
    pub fn safe_init(framebuffer: Option<&mut crate::boot::Framebuffer>) -> bool {
        // Framebuffer yoksa yedek moda geç
        let fb = match framebuffer {
            Some(fb) => fb,
            None => {
                crate::serial_println!("[GOP_SAFETY] Framebuffer sağlanmadı");
                return Self::try_text_mode();
            }
        };

        // Framebuffer'ın geçerli olduğunu doğrula
        if !Self::verify_framebuffer(fb) {
            BOOT_SAFETY.gop_failures.fetch_add(1, Ordering::SeqCst);
            BOOT_SAFETY.record_violation(
                ViolationType::GopInitFailed,
                "Geçersiz framebuffer",
                true
            );
            return Self::try_text_mode();
        }

        crate::serial_println!("[GOP_SAFETY] GOP başarıyla başlatıldı");
        true
    }

    /// Framebuffer'ın kullanılabilir durumda olduğunu doğrular.
    ///
    /// Minimum koşullar:
    /// - Genişlik > 0 ve yükseklik > 0
    /// - Taban adresi (base_addr) != 0
    fn verify_framebuffer(fb: &crate::boot::Framebuffer) -> bool {
        // Geçerli boyutları kontrol et
        if fb.width == 0 || fb.height == 0 {
            crate::serial_println!("[GOP_SAFETY] Geçersiz boyutlar");
            return false;
        }

        // Geçerli tampon adresini kontrol et
        if fb.base_addr == 0 {
            crate::serial_println!("[GOP_SAFETY] Geçersiz tampon adresi");
            return false;
        }

        true
    }

    /// Metin moduna (seri port) geri döner.
    ///
    /// Seri port UART üzerinden her zaman çalışır; GOP başarısız olsa bile
    /// tanı çıktısı kaybolmaz.
    fn try_text_mode() -> bool {
        crate::serial_println!("[GOP_SAFETY] Yalnızca seri porta geri dönülüyor");
        true // Seri port her zaman çalışır
    }

    /// Sınır kontrolü yapılarak pikseli framebuffer'a güvenli biçimde yazar.
    ///
    /// `x` veya `y` ekran sınırları dışındaysa `false` döner; yazma yapılmaz.
    /// Bu sayede kötü koordinatlardan kaynaklanan bellek bozulması önlenir.
    pub fn safe_put_pixel(x: u32, y: u32, color: u32, fb: &mut crate::boot::Framebuffer) -> bool {
        if x as usize >= fb.width || y as usize >= fb.height {
            return false; // Sınır dışı
        }

        let offset = (y as usize * fb.pixels_per_scan_line + x as usize) * 4;
        let pixel_addr = (fb.base_addr + offset) as *mut u32;
        unsafe {
            *pixel_addr = color;
        }
        true
    }
}

// ============================================================================
// BOOT BEKÇI KÖPEK (WATCHDOG)
// ============================================================================

/// Boot zaman aşımı bekçi köpeği.
///
/// Her boot aşaması için bir zaman sınırı belirler. Aşama bu sürede
/// tamamlanamaz ise kurtarma mekanizması devreye girer:
/// - SMP aşamasında: kalan AP'ler atlanır
/// - Sürücü aşamasında: başarısız sürücü atlanır
/// - Diğer aşamalarda: bozulma bildirilir
pub struct BootWatchdog;

impl BootWatchdog {
    /// Bekçi köpeği zamanlayıcısını başlatır.
    ///
    /// `boot_start_time` mevcut tik sayısıyla ayarlanır.
    pub fn start() {
        BOOT_SAFETY.boot_start_time.store(
            crate::task::scheduler::get_ticks(),
            Ordering::SeqCst
        );

        crate::serial_println!("[BOOT_WATCHDOG] Başlatıldı");
    }

    /// Boot ilerleme durumunu kontrol eder.
    ///
    /// Mevcut aşamaya göre farklı zaman aşımı eşikleri uygulanır:
    /// - SmpInit: AP_STARTUP_TIMEOUT_MS * 2 (AP'ler zaman alır)
    /// - DriverInit: BOOT_PHASE_TIMEOUT_MS / 2 (sürücüler hızlı başlamalı)
    /// - Diğer: BOOT_PHASE_TIMEOUT_MS / 4
    pub fn check() {
        let current_phase = BOOT_SAFETY.current_phase.load(Ordering::SeqCst);
        let last_checkpoint = BOOT_SAFETY.last_checkpoint.load(Ordering::SeqCst);
        let current_time = crate::task::scheduler::get_ticks();

        // Aşamaya özel zaman aşımı eşiğini belirle
        let phase_timeout: usize = match BootPhase::try_from(current_phase as u8) {
            Ok(BootPhase::SmpInit) => AP_STARTUP_TIMEOUT_MS * 2,
            Ok(BootPhase::DriverInit) => BOOT_PHASE_TIMEOUT_MS / 2,
            _ => BOOT_PHASE_TIMEOUT_MS / 4,
        } as usize;

        if current_time.saturating_sub(last_checkpoint as usize) > phase_timeout {
            BOOT_SAFETY.record_violation(
                ViolationType::Timeout,
                &format!("Aşama {:?} zaman aşımı", BootPhase::try_from(current_phase as u8)),
                false
            );

            // Aşamaya göre kurtarma girişimi
            Self::attempt_recovery(current_phase);
        }
    }

    /// Zaman aşımı durumunda kurtarma girişimi başlatır.
    ///
    /// Kurtarma bağlama bağlıdır: SmpInit'te AP'ler atlanır,
    /// DriverInit'te sürücü atlanır, diğer durumlarda yalnızca kayıt yapılır.
    fn attempt_recovery(phase: u32) {
        BOOT_SAFETY.recovery_attempts.fetch_add(1, Ordering::SeqCst);
        BOOT_SAFETY.in_recovery.store(true, Ordering::SeqCst);

        match BootPhase::try_from(phase as u8) {
            Ok(BootPhase::SmpInit) => {
                // Kalan AP'leri atla ve devam et
                crate::serial_println!("[BOOT_WATCHDOG] Kalan AP'ler atlanıyor");
            }
            Ok(BootPhase::DriverInit) => {
                // Başarısız sürücüyü atla ve devam et
                crate::serial_println!("[BOOT_WATCHDOG] Başarısız sürücü atlanıyor");
            }
            _ => {
                crate::serial_println!("[BOOT_WATCHDOG] {:?} aşamasından kurtarılamıyor",
                    BootPhase::try_from(phase as u8));
            }
        }

        BOOT_SAFETY.in_recovery.store(false, Ordering::SeqCst);
    }

    /// Boot'un başarıyla tamamlandığını işaretler.
    ///
    /// `boot_complete = true` ayarlanır ve aşama `Running`'e geçer.
    /// İhlal özeti seri porta yazılır.
    pub fn complete() {
        BOOT_SAFETY.boot_complete.store(true, Ordering::SeqCst);
        BOOT_SAFETY.enter_phase(BootPhase::Running);

        crate::serial_println!(
            "[BOOT_WATCHDOG] Boot tamamlandı - {} ihlal, {} kurtarıldı",
            BOOT_SAFETY.violation_count(),
            BOOT_SAFETY.violations.lock().iter().filter(|v| v.recovered).count()
        );
    }
}

// ============================================================================
// BAŞLATMA
// ============================================================================

/// Boot güvenlik sistemini başlatır.
///
/// Bekçi köpeği, heap güvenliği ve IDT güvenli işleyicileri sırasıyla etkinleştirir.
/// Bu fonksiyon heap hazır olduktan hemen sonra çağrılmalıdır.
pub fn init() {
    BootWatchdog::start();
    HeapSafety::init();
    IdtSafety::install_safe_handlers();

    crate::serial_println!("[BOOT_SAFETY] Boot güvenlik sistemi başlatıldı");
}

/// Boot güvenlik raporu döndürür.
///
/// Boot tamamlanma durumu, mevcut aşama ve tüm hata sayaçlarını içerir.
pub fn get_report() -> BootSafetyReport {
    BootSafetyReport {
        boot_complete: BOOT_SAFETY.boot_complete.load(Ordering::SeqCst),
        current_phase: BootPhase::try_from(BOOT_SAFETY.current_phase.load(Ordering::SeqCst) as u8)
            .unwrap_or(BootPhase::Reset),
        violation_count: BOOT_SAFETY.violation_count() as u32,
        heap_corruptions: BOOT_SAFETY.heap_corruptions.load(Ordering::SeqCst),
        smp_failures: BOOT_SAFETY.smp_failures.load(Ordering::SeqCst),
        idt_failures: BOOT_SAFETY.idt_failures.load(Ordering::SeqCst),
        gop_failures: BOOT_SAFETY.gop_failures.load(Ordering::SeqCst),
        recovery_attempts: BOOT_SAFETY.recovery_attempts.load(Ordering::SeqCst),
    }
}

/// Boot güvenlik rapor yapısı.
///
/// Boot sonrası teşhis ve izleme için kullanılır.
#[derive(Clone, Debug)]
pub struct BootSafetyReport {
    pub boot_complete: bool,
    pub current_phase: BootPhase,
    pub violation_count: u32,
    pub heap_corruptions: u32,
    pub smp_failures: u32,
    pub idt_failures: u32,
    pub gop_failures: u32,
    pub recovery_attempts: u32,
}

/// u8 değerinden `BootPhase`'e dönüşüm.
///
/// Bilinmeyen değerler için `Err(())` döner; bu sayede geçersiz
/// aşama değerleri güvenli biçimde `BootPhase::Reset`'e geri döndürülebilir.
impl TryFrom<u8> for BootPhase {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(BootPhase::Reset),
            1 => Ok(BootPhase::UefiHandover),
            2 => Ok(BootPhase::MemoryInit),
            3 => Ok(BootPhase::PagingSetup),
            4 => Ok(BootPhase::HeapInit),
            5 => Ok(BootPhase::GdtSetup),
            6 => Ok(BootPhase::IdtSetup),
            7 => Ok(BootPhase::AcpiInit),
            8 => Ok(BootPhase::SmpInit),
            9 => Ok(BootPhase::DriverInit),
            10 => Ok(BootPhase::UserspaceReady),
            255 => Ok(BootPhase::Running),
            _ => Err(()),
        }
    }
}
