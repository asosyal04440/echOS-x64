//! # Simics Kapısı (Simics Gate) - CI/CD Kalite Geçidi
//!
//! Bu modül, Intel Simics simülatörü üzerinde çalışan otomatik test altyapısının
//! kalite geçidini (quality gate) uygular. CI/CD boru hattında bir derleme/commit
//! birleştirilmeden önce bu geçitten geçmesi zorunludur.
//!
//! ```
//! CI/CD Geçit Akışı:
//!
//!   Git Push / PR
//!       |
//!       v
//!   [Simics Simülatörü]
//!       |
//!       +-- BootIrqInputSmoke       -> Boot + IRQ + fare girişi testi
//!       +-- SyscallWinServerSecurity-> Syscall güvenlik/pointer testi
//!       +-- FsNetworkHealth         -> Dosya sistemi + ağ sağlık testi
//!       +-- PerformanceBaseline     -> Gecikme/kare titremesi testi
//!       +-- ExtremeIronShimFuzz     -> VM kaçış + bellek fuzzing testi
//!       |
//!       v
//!   GateReport -> should_block_merge()
//!       |
//!       +-- true  -> FAIL: Birleştirme engellendi (Day1HardBlock)
//!       +-- false -> PASS: Birleştirme onaylandı
//! ```
//!
//! GateMode::Day1HardBlock:
//!   Herhangi bir test ekseninde başarısız sonuç varsa commit BLOKE edilir.
//!   Bu, "sıfır regresyon" politikasını zorunlu kılar.

use alloc::vec::Vec;

// ============================================================================
// GATE MODU (GateMode)
//
// Geçit politikasını belirler. Şu an yalnızca Day1HardBlock desteklenmektedir.
//
//  Day1HardBlock: İlk günden itibaren sert engelleme politikası.
//    - Herhangi bir AxisVerdict::Fail sonucu -> commit bloke edilir
//    - CI/CD boru hattında "sıfır regresyon toleransı" anlamına gelir
// ============================================================================

/// Geçit çalışma modu - commit engelleme politikasını belirler
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateMode {
    /// Herhangi bir başarısızlık anında birleştirmeyi engelle (sıfır regresyon toleransı)
    Day1HardBlock,
}

// ============================================================================
// TEST EKSENLERİ (TestAxis)
//
// Her eksen bağımsız bir test senaryosunu temsil eder.
// Simics ortamında paralel olarak çalıştırılabilirler.
//
//  BootIrqInputSmoke:
//    - Sistem boot ediyor mu?
//    - Kesmeler (IRQ) etkinleşiyor mu?
//    - Fare/klavye girişi alınıyor mu?
//    - GUI compositor başlıyor mu?
//
//  SyscallWinServerSecurity:
//    - Windows Server benzeri syscall güvenlik kontrolleri
//    - Sahiplik denetimleri (ownership check)
//    - Kullanıcı alanı pointer geçerliliği
//    - Çekirdek pointer sızıntısı tespiti
//
//  FsNetworkHealth:
//    - FAT dosya sistemi sağlığı
//    - Ağ yığını başlatma ve temel iletişim
//
//  PerformanceBaseline:
//    - Waker gecikme ölçümü (max_waker_latency_us)
//    - Giriş gecikme ölçümü (max_input_latency_us)
//    - Kare titremesi ölçümü (max_frame_jitter_us)
//
//  ExtremeIronShimVmEscapeMemoryFuzz:
//    - VM kaçışı fuzzing (ring3->ring0 geçişi engellendi mi?)
//    - Bellek bozulma tespiti (OOM queue collapse)
//    - IronShim sanallaştırma güvenlik tespiti
// ============================================================================

/// Test ekseni - bağımsız bir doğrulama senaryosunu temsil eder
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestAxis {
    /// Boot + IRQ + fare girişi + GUI compositor duman testi
    BootIrqInputSmoke,
    /// Syscall güvenlik ve Windows Server benzeri API doğrulaması
    SyscallWinServerSecurity,
    /// Dosya sistemi (FAT) ve ağ yığını sağlık testi
    FsNetworkHealth,
    /// Waker/giriş/kare gecikme temel hat ölçümü
    PerformanceBaseline,
    /// VM kaçışı + IronShim + bellek fuzzing aşırı testi
    ExtremeIronShimVmEscapeMemoryFuzz,
}

// ============================================================================
// EKSEN KARARI (AxisVerdict)
//
// Bir test ekseninin son değerlendirme sonucu.
//  Pass: Tüm required_markers bulundu, forbidden_markers bulunmadı,
//        eşik değerleri aşılmadı.
//  Fail: En az bir kural ihlal edildi.
// ============================================================================

/// Eksen değerlendirme kararı
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisVerdict {
    /// Test başarılı - tüm gereksinimler karşılandı
    Pass,
    /// Test başarısız - en az bir kural ihlal edildi
    Fail,
}

// ============================================================================
// EŞİK KÜMESİ (ThresholdSet)
//
// Performans ekseninin geçme/kalma sınırlarını tanımlar.
// Tüm değerler mikrosaniye (us) veya milisaniye (ms) cinsindendir.
//
//  timeout_ms:           Test çalıştırma süre limiti
//  max_waker_latency_us: Zamanlayıcı uyandırma maksimum gecikmesi
//  max_input_latency_us: Giriş olayı maksimum işleme gecikmesi
//  max_frame_jitter_us:  Ekran karesi maksimum titremesi (jitter)
// ============================================================================

/// Performans test eşik değerleri (mikrosaniye/milisaniye cinsinden)
#[derive(Clone, Copy, Debug)]
pub struct ThresholdSet {
    /// Test için maksimum çalışma süresi (ms)
    pub timeout_ms: u32,
    /// Zamanlayıcı uyandırma maksimum gecikmesi (us)
    pub max_waker_latency_us: u32,
    /// Giriş olayı maksimum gecikme süresi (us)
    pub max_input_latency_us: u32,
    /// Ekran karesi maksimum titremesi (us)
    pub max_frame_jitter_us: u32,
}

// ============================================================================
// KURAL (GateRule)
//
// Her test ekseni için bağlayıcı bir doğrulama kuralı.
//
//  required_markers:  Simics çıktısında bulunması ZORUNLU dizeler.
//    Bunlar boot adımlarını, güvenlik kontrollerini veya sağlık
//    bildirimlerini temsil eder. Eksik marker = başarısız test.
//
//  forbidden_markers: Simics çıktısında ASLA bulunmaması gereken dizeler.
//    Panik mesajları, ölü kilit (deadlock), bellek bozulması gibi
//    hata göstergeleri bu listede yer alır.
//
//  thresholds: PerformanceBaseline gibi gecikme testleri için sınırlar.
// ============================================================================

/// Geçit kuralı - bir test ekseni için tüm doğrulama kriterlerini içerir
#[derive(Clone, Copy, Debug)]
pub struct GateRule {
    /// Bu kuralın ait olduğu test ekseni
    pub axis: TestAxis,
    /// Simics çıktısında bulunması zorunlu metin kalıpları
    pub required_markers: &'static [&'static str],
    /// Simics çıktısında bulunması yasak metin kalıpları (hata göstergeleri)
    pub forbidden_markers: &'static [&'static str],
    /// Performans eşik değerleri (gecikme sınırları)
    pub thresholds: ThresholdSet,
}

// ============================================================================
// KARAR KAYDI (VerdictRecord)
//
// Bir test ekseninin değerlendirme sonucunu ve gerekçesini tutar.
// GateReport içinde birden fazla VerdictRecord bulunabilir (her eksen için).
//
//  reason: Geçme veya kalma gerekçesi (hangi marker eksik/fazla, hangi eşik aşıldı)
// ============================================================================

/// Bir test ekseni için değerlendirme kaydı
#[derive(Clone, Debug)]
pub struct VerdictRecord {
    /// Değerlendirilen test ekseni
    pub axis: TestAxis,
    /// Bu eksenin kararı (Pass/Fail)
    pub verdict: AxisVerdict,
    /// Karar gerekçesi (hata açıklaması veya başarı notu)
    pub reason: &'static str,
}

// ============================================================================
// GEÇIT RAPORU (GateReport)
//
// Tüm test eksenlerinin değerlendirme sonuçlarını toplar.
// CI/CD boru hattı bu raporu okuyarak birleştirme kararını verir.
//
//  should_block_merge():
//    Day1HardBlock modunda herhangi bir Fail kararı varsa true döner.
//    CI sistemi bu değeri kontrol ederek pull request'i bloke eder veya onaylar.
// ============================================================================

/// Tüm test eksenlerinin değerlendirme raporu
#[derive(Clone, Debug)]
pub struct GateReport {
    /// Uygulanan geçit politikası
    pub mode: GateMode,
    /// Her test ekseni için değerlendirme kayıtları
    pub records: Vec<VerdictRecord>,
}

impl GateReport {
    /// Belirtilen modda yeni boş bir geçit raporu oluşturur.
    pub fn new(mode: GateMode) -> Self {
        Self {
            mode,
            records: Vec::new(),
        }
    }

    /// Rapora yeni bir eksen değerlendirme kaydı ekler.
    pub fn push(&mut self, axis: TestAxis, verdict: AxisVerdict, reason: &'static str) {
        self.records.push(VerdictRecord {
            axis,
            verdict,
            reason,
        });
    }

    /// Commit birleştirmesinin bloke edilip edilmeyeceğini döndürür.
    ///
    /// Day1HardBlock modunda: herhangi bir Fail kararı varsa true.
    /// CI/CD boru hattı bu değeri kullanarak PR'ı onaylar veya reddeder.
    pub fn should_block_merge(&self) -> bool {
        match self.mode {
            GateMode::Day1HardBlock => self.records.iter().any(|record| record.verdict == AxisVerdict::Fail),
        }
    }
}

// ============================================================================
// KURAL TANIMLARI - Boot + IRQ + Giriş Testi
//
// required_markers: Boot aşamaları, GUI başlatma ve fare girişi kalıpları.
// forbidden_markers: Panik, ölü kilit ve spinlock takılması göstergeleri.
// thresholds: Standart gecikmeler (2ms waker, 8ms giriş, 20ms kare).
// ============================================================================

/// Boot, IRQ ve giriş duman testi kuralı
const RULE_BOOT_IRQ_INPUT: GateRule = GateRule {
    axis: TestAxis::BootIrqInputSmoke,
    required_markers: &[
        "[INT] Interrupts enabled",
        "[BOOT] Starting GUI compositor...",
        "Mouse: Başarıyla başlatıldı",
        "[COMPOSITOR] Ana döngü başlatıldı",
    ],
    forbidden_markers: &["[PANIC]", "deadlock", "spinlock stuck"],
    thresholds: ThresholdSet {
        timeout_ms: 120_000,
        max_waker_latency_us: 2_000,
        max_input_latency_us: 8_000,
        max_frame_jitter_us: 20_000,
    },
};

// ============================================================================
// KURAL TANIMLARI - Syscall Güvenlik Testi
//
// required_markers: Windows Server API katmanı, sahiplik kontrolleri,
//                   kullanıcı alanı pointer geçerliliği.
// forbidden_markers: Çekirdek pointer sızıntısı, geçersiz kullanıcı pointer
//                    kabul edilmesi (ciddi güvenlik açıkları).
// ============================================================================

/// Syscall güvenlik ve Windows Server API doğrulama kuralı
const RULE_SYSCALL_SECURITY: GateRule = GateRule {
    axis: TestAxis::SyscallWinServerSecurity,
    required_markers: &[
        "[WINSRV]",
        "ownership check",
        "user-range",
    ],
    forbidden_markers: &[
        "kernel pointer leak",
        "invalid user pointer accepted",
    ],
    thresholds: ThresholdSet {
        timeout_ms: 120_000,
        max_waker_latency_us: 2_000,
        max_input_latency_us: 8_000,
        max_frame_jitter_us: 20_000,
    },
};

// ============================================================================
// KURAL TANIMLARI - Dosya Sistemi ve Ağ Sağlığı
//
// required_markers: Ağ yığını etkinleşmesi, FAT dosya sistemi, genel
//                   "filesystem" bildirimi.
// forbidden_markers: Dosya sistemi bozulması, ağ ölü kilidi.
// ============================================================================

/// Dosya sistemi ve ağ sağlık testi kuralı
const RULE_FS_NETWORK: GateRule = GateRule {
    axis: TestAxis::FsNetworkHealth,
    required_markers: &[
        "[NET]",
        "FAT",
        "filesystem",
    ],
    forbidden_markers: &["fs corruption", "network deadlock"],
    thresholds: ThresholdSet {
        timeout_ms: 120_000,
        max_waker_latency_us: 2_000,
        max_input_latency_us: 8_000,
        max_frame_jitter_us: 20_000,
    },
};

// ============================================================================
// KURAL TANIMLARI - Performans Temel Hattı
//
// required_markers: Performans ölçüm bildirimleri ([PERF], "frame", "latency").
// forbidden_markers: Gecikme gerilemesi ve kare zamanlaması ihlali.
// thresholds: Daha sıkı sınırlar (1ms waker, 4ms giriş, 10ms kare).
// ============================================================================

/// Performans temel hat ölçüm kuralı (daha sıkı eşikler)
const RULE_PERFORMANCE: GateRule = GateRule {
    axis: TestAxis::PerformanceBaseline,
    required_markers: &[
        "[PERF]",
        "frame",
        "latency",
    ],
    forbidden_markers: &["latency regression", "frame pacing violation"],
    thresholds: ThresholdSet {
        timeout_ms: 120_000,
        max_waker_latency_us: 1_000,
        max_input_latency_us: 4_000,
        max_frame_jitter_us: 10_000,
    },
};

// ============================================================================
// KURAL TANIMLARI - Aşırı Güvenlik Testi (IronShim + VM Kaçışı + Fuzzing)
//
// required_markers: IronShim sanallaştırma katmanı, fuzzing aktivasyonu,
//                   ring3->ring0 geçişinin engellendiğini doğrulayan mesaj.
// forbidden_markers: VM kaçışı başarısı, ring0 üzerine yazma, OOM çöküşü.
//                    Bu forbidden_markers gerçekleşirse ciddi güvenlik açığı var demektir.
// thresholds: Aşırı test için uzun timeout (180s), sıkı gecikme sınırları.
// ============================================================================

/// VM kaçışı + IronShim + bellek fuzzing aşırı güvenlik testi kuralı
const RULE_EXTREME: GateRule = GateRule {
    axis: TestAxis::ExtremeIronShimVmEscapeMemoryFuzz,
    required_markers: &[
        "[IRONSHIM]",
        "fuzz",
        "ring3->ring0 blocked",
    ],
    forbidden_markers: &[
        "vm escape success",
        "ring0 overwrite",
        "oom queue collapse",
    ],
    thresholds: ThresholdSet {
        timeout_ms: 180_000,
        max_waker_latency_us: 1_000,
        max_input_latency_us: 4_000,
        max_frame_jitter_us: 10_000,
    },
};

/// CI/CD Day1HardBlock boru hattı için tüm geçit kurallarını döndürür.
///
/// Dizi sırası önem taşır; önce temel boot/güvenlik testleri,
/// en son aşırı dayanıklılık testleri yer alır.
pub fn aggressive_day1_rules() -> &'static [GateRule] {
    &[
        RULE_BOOT_IRQ_INPUT,
        RULE_SYSCALL_SECURITY,
        RULE_FS_NETWORK,
        RULE_PERFORMANCE,
        RULE_EXTREME,
    ]
}
