//! # echOS Power Management Module
//!
//! Tier 1 OS seviyesinde CPU power management
//! Linux cpufreq ile aynı seviyede yetenekler

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use alloc::vec::Vec;
use alloc::boxed::Box;
use crate::memory_barriers::{smp_mb, smp_rmb, smp_wmb};
use crate::preempt::{PreemptDisableGuard, preempt_enabled};
use crate::rcu::{RcuPtr, synchronize_rcu};

/// CPU guc durumlari (Linux C-states ile uyumlu) - her seviye farkli miktarda enerji tasarrufu saglar
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum CpuState {
    /// C0: Aktif durum — CPU tam hızda çalışıyor, görev yürütülüyor
    C0 = 0,
    /// C1: Temel boşta durumu — CPU talimat işlemeyi durdurdu, ancak önbellek sıcak kaldı
    C1 = 1,
    /// C2: Daha derin boşta durumu — dahili saat durdurulabilir, giriş gecikmesi artar
    C2 = 2,
    /// C3: Derin boşta durumu — önbellek boşaltılabilir, daha fazla güç tasarrufu
    C3 = 3,
    /// C6: Çok derin boşta durumu — çekirdek voltajı sıfıra indirilir, bağlam kaydedilir
    C6 = 4,
    /// C7: En derin boşta durumu — LLC boşaltılır, maksimum güç tasarrufu sağlanır
    C7 = 5,
}

/// CPU frekans durumlari (P-durumlari) - ACPI _PSS metodundan alinan performans durumu tanimlayicisi
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuFrequency {
    /// Frekans MHz cinsinden — CPU'nun saniyede kaç milyon döngü gerçekleştirdiğini gösterir
    pub frequency_mhz: u32,
    /// Voltaj milivolt cinsinden — daha yüksek frekans daha yüksek voltaj gerektirir
    pub voltage_mv: u32,
    /// Güç tüketimi miliwatt cinsinden — frekans ve voltaja göre üssel artar
    pub power_mw: u32,
    /// Bu frekansın turbo modu olup olmadığı — kısa süreli aşırı performans için kullanılır
    pub is_turbo: bool,
}

impl CpuFrequency {
    pub fn new(frequency_mhz: u32, voltage_mv: u32, power_mw: u32) -> Self {
        Self {
            frequency_mhz,
            voltage_mv,
            power_mw,
            is_turbo: false,
        }
    }
    
    pub fn turbo(frequency_mhz: u32, voltage_mv: u32, power_mw: u32) -> Self {
        Self {
            frequency_mhz,
            voltage_mv,
            power_mw,
            is_turbo: true,
        }
    }
}

/// CPU bosta durum tanimlayicisi - C-durumunun gecikme, guc ve hedef kalis suresi bilgilerini tutar
#[derive(Debug)]
pub struct CpuIdleState {
    /// Durum tanımlayıcısı (C1, C2, vb.) — hangi güç seviyesinde çalıştığımızı belirtir
    pub state: CpuState,
    /// Mikrosaniye cinsinden çıkış gecikmesi — uyku sonrası CPU'nun aktif hale gelme süresi
    pub exit_latency_us: u32,
    /// Miliwatt cinsinden güç tüketimi — bu durumda ne kadar enerji harcandığını gösterir
    pub power_mw: u32,
    /// Mikrosaniye cinsinden hedef kalış süresi — bu durum, bu süreden kısa boşta kalmalarda seçilmez
    pub target_residency_us: u32,
    /// Bu durumun önbelleği devre dışı bırakıp bırakmadığı — C3+ genellikle LLC önbelleğini boşaltır
    pub disables_cache: bool,
    /// Bu durumun TLB'yi temizleyip temizlemediği — uyanış sonrası sayfa tablosu yeniden yüklenir
    pub flushes_tlb: bool,
    /// Kullanım sayacı — bu duruma kaç kez girildiğini izler
    pub usage_count: AtomicU64,
    /// Bu durumda geçirilen toplam süre (tick cinsinden) — güç tasarrufu analizinde kullanılır
    pub total_time: AtomicU64,
}

impl CpuIdleState {
    pub fn new(state: CpuState, exit_latency_us: u32, power_mw: u32, target_residency_us: u32) -> Self {
        Self {
            state,
            exit_latency_us,
            power_mw,
            target_residency_us,
            disables_cache: state == CpuState::C3 || state == CpuState::C6 || state == CpuState::C7,
            flushes_tlb: state == CpuState::C3 || state == CpuState::C6 || state == CpuState::C7,
            usage_count: AtomicU64::new(0),
            total_time: AtomicU64::new(0),
        }
    }
    
    /// Verilen boşta kalma süresi için bu durumun diğerinden daha uygun olup olmadığını kontrol eder
    pub fn is_better_than(&self, other: &CpuIdleState, idle_time_us: u32) -> bool {
        // Boşta kalma süresi yeterliyse daha derin durumu tercih et — hedef kalış süresinden uzun bekleme varsa
        if idle_time_us >= self.target_residency_us && idle_time_us >= other.target_residency_us {
            // Daha derin durum (yüksek enum değeri) daha iyi — daha az enerji tüketir
            (self.state as u32) > (other.state as u32)
        } else if idle_time_us >= self.target_residency_us {
            true
        } else {
            false
        }
    }
    
    /// Bu boşta durumuna girişi kaydeder — istatistik takibi için sayıcıyı artırır
    pub fn enter(&self) {
        self.usage_count.fetch_add(1, Ordering::Relaxed);
    }
    
    /// Bu boşta durumundan çıkışı kaydeder — geçirilen süreyi toplamışsal süreye ekler
    pub fn exit(&self, duration_ticks: u64) {
        self.total_time.fetch_add(duration_ticks, Ordering::Relaxed);
    }
    
    /// Kullanım istatistiklerini döndürür — (toplam giriş sayısı, toplam geçirilen süre) çifti
    pub fn get_stats(&self) -> (u64, u64) {
        (
            self.usage_count.load(Ordering::Relaxed),
            self.total_time.load(Ordering::Relaxed),
        )
    }
}

/// CPU frekans valisator turleri - hangi algoritmanin P-durumunu kontrol edecegini belirler
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum FreqGovernor {
    /// Performans valisatörü — her zaman maksimum frekansda çalışır, gecikmeye duyarlı işler için
    Performance = 0,
    /// Güç tasarrufu valisatörü — her zaman minimum frekansda çalışır, pil ömrünü uzatır
    Powersave = 1,
    /// Kullanıcı alanı valisatörü — kullanıcı uygulaması frekansı doğrudan kontrol eder
    Userspace = 2,
    /// İsteğe bağlı valisatörü — yüke göre frekansı dinamik ayarlar, genel amaçlı kullanım
    OnDemand = 3,
    /// Muhafazakâr valisatörü — frekansı kademeli değiştirir, ani artışlanmayı önler
    Conservative = 4,
    /// Zamanlama tabanlı valisatörü — Linux schedutil gibi görev zamanınlığı tarafından sürülür
    Schedutil = 5,
}

/// CPU guc yonetimi tanimlayicisi - bir mantiksal CPU'nun tum P/C-durum bilgilerini icerir
#[repr(C, align(64))]
pub struct CpuPowerDesc {
    /// CPU kimliği — hangi işlem birimine ait olduğunu belirtir
    pub cpu_id: u32,
    /// Mevcut C-durumu — CPU'nun şu anda bulunduğu güç tasarrufu seviyesi
    pub current_cstate: AtomicU32, // CpuState as u32
    /// Hedef C-durumu — boşta kalma süresine göre seçilen ideal uyku seviyesi
    pub target_cstate: AtomicU32,
    /// Mevcut P-durumu (frekans indeksi) — aktif frekans tablosundaki pozisyon
    pub current_pstate: AtomicU32,
    /// Hedef P-durumu — valisörün hesapladığı ideal frekans indeksi
    pub target_pstate: AtomicU32,
    /// Kullanılabilir boşta durumları — C1'den C7'ye kadar desteklenen uyku seviyeleri
    pub idle_states: Vec<CpuIdleState>,
    /// Kullanılabilir frekanslar — desteklenen P-durumlarının tam listesi
    pub frequencies: Vec<CpuFrequency>,
    /// Mevcut frekans valisatörü — yüke göre frekans kararını veren politika
    pub governor: AtomicU32, // FreqGovernor as u32
    /// Minimum frekans indeksi — en düşük güç tasarrufu noktası
    pub min_freq_idx: u32,
    /// Maksimum frekans indeksi — temel yüksek performans noktası
    pub max_freq_idx: u32,
    /// Turbo frekans indeksi (varsa) — kısa süreli aşırı yük için ek hız
    pub turbo_freq_idx: Option<u32>,
    /// Mevcut yük (0-100%) — CPU'nun ne kadar meşgul olduğunun göstergesi
    pub current_load: AtomicU32,
    /// Ortalama yük (isteğe bağlı valisator için) — ani değişimleri yumulatmak için EMA
    pub avg_load: AtomicU32,
    /// Her C-durumunda geçirilen süre — güç profilleme için C0-C7 zaman istatistikleri
    pub cstate_time: [AtomicU64; 6], // C0-C7
    /// C-durumu giriş sayısı — her seviyeye kaç kez girildiğini kaydeder
    pub cstate_count: [AtomicU64; 6],
    /// Frekans geçiş sayısı — P-durum değişikliklerinin toplamı
    pub freq_transitions: AtomicU64,
    /// Son frekans değişikliği zaman damgası — valisator döngü periyodunu hesaplamak için
    pub last_freq_change: AtomicU64,
    /// Güç yönetimi etkin mi — devre dışı bırakılırsa CPU tam frekansda kalır
    pub pm_enabled: AtomicBool,
    /// Boşta döngüsü yürütülüyor mu — CPU'nun boşta olup olmadığını işaretler
    pub in_idle: AtomicBool,
    /// Yanlış paylaşımı önlemek için dolgu — farklı CPU'ların aynı cache satırına dokunmamasını sağlar
    _padding: [u8; 0],
}

impl CpuPowerDesc {
    /// Yeni CPU güç tanımlayıcısı oluştur
    pub fn new(cpu_id: u32) -> Self {
        let mut idle_states = Vec::new();
        let mut frequencies = Vec::new();
        
        // Varsayılan boşta durumları (tipik x86 değerleri) — gerçek donanımda ACPI'dan okunur
        idle_states.push(CpuIdleState::new(CpuState::C1, 1, 100, 2));
        idle_states.push(CpuIdleState::new(CpuState::C2, 10, 50, 10));
        idle_states.push(CpuIdleState::new(CpuState::C3, 50, 20, 100));
        idle_states.push(CpuIdleState::new(CpuState::C6, 100, 10, 200));
        idle_states.push(CpuIdleState::new(CpuState::C7, 150, 5, 300));
        
        // Varsayılan frekanslar (tipik masaistü CPU) — gerçek donanımda MSR veya ACPI _PSS'dan alınır
        frequencies.push(CpuFrequency::new(800, 800, 5000));   // Min
        frequencies.push(CpuFrequency::new(1200, 900, 8000));
        frequencies.push(CpuFrequency::new(1600, 1000, 12000));
        frequencies.push(CpuFrequency::new(2000, 1100, 17000));
        frequencies.push(CpuFrequency::new(2400, 1200, 23000));
        frequencies.push(CpuFrequency::new(2800, 1300, 30000));
        frequencies.push(CpuFrequency::new(3200, 1400, 38000));
        frequencies.push(CpuFrequency::turbo(3600, 1500, 47000)); // Turbo
        
        Self {
            cpu_id,
            current_cstate: AtomicU32::new(CpuState::C0 as u32),
            target_cstate: AtomicU32::new(CpuState::C1 as u32),
            current_pstate: AtomicU32::new(3), // Orta frekanstan basla - ne cok dusuk ne cok yuksek
            target_pstate: AtomicU32::new(3),
            idle_states,
            frequencies,
            governor: AtomicU32::new(FreqGovernor::OnDemand as u32),
            min_freq_idx: 0,
            max_freq_idx: 7,
            turbo_freq_idx: Some(7),
            current_load: AtomicU32::new(0),
            avg_load: AtomicU32::new(0),
            cstate_time: [const { AtomicU64::new(0) }; 6],
            cstate_count: [const { AtomicU64::new(0) }; 6],
            freq_transitions: AtomicU64::new(0),
            last_freq_change: AtomicU64::new(0),
            pm_enabled: AtomicBool::new(true),
            in_idle: AtomicBool::new(false),
            _padding: [0; 0],
        }
    }
    
    /// Mevcut C-durumunu oku — atomik satın alma semantikle güvenli okuma yapılır
    pub fn get_current_cstate(&self) -> CpuState {
        match self.current_cstate.load(Ordering::Acquire) {
            0 => CpuState::C0,
            1 => CpuState::C1,
            2 => CpuState::C2,
            3 => CpuState::C3,
            4 => CpuState::C6,
            5 => CpuState::C7,
            _ => CpuState::C0,
        }
    }
    
    /// Mevcut C-durumunu yaz — Release+smp_wmb ile diğer çekirdeklere görünür kılır
    pub fn set_current_cstate(&self, state: CpuState) {
        self.current_cstate.store(state as u32, Ordering::Release);
        smp_wmb();
    }
    
    /// Mevcut frekans bilgisini döndür — frekans tablosundan P-durum indeksine göre arar
    pub fn get_current_frequency(&self) -> Option<CpuFrequency> {
        let idx = self.current_pstate.load(Ordering::Acquire) as usize;
        self.frequencies.get(idx).copied()
    }
    
    /// Frekansı ayarla — indeks geçerliyse P-durumunu atomik günceller ve istatistik kaydeder
    pub fn set_frequency(&self, freq_idx: u32) -> Result<(), PowerError> {
        if freq_idx > self.max_freq_idx {
            return Err(PowerError::InvalidFrequency);
        }
        
        // Turbo kullanılabilirliğini kontrol et — yüksek yüklerde enerji bütçesi aşılabilir
        if let Some(turbo_idx) = self.turbo_freq_idx {
            if freq_idx == turbo_idx && !self.can_use_turbo() {
                return Err(PowerError::TurboUnavailable);
            }
        }
        
        let old_idx = self.current_pstate.load(Ordering::Acquire);
        if old_idx != freq_idx {
            self.current_pstate.store(freq_idx, Ordering::Release);
            self.freq_transitions.fetch_add(1, Ordering::Relaxed);
            self.last_freq_change.store(crate::task::scheduler::get_ticks() as u64, Ordering::Relaxed);
            smp_mb();
            
            crate::serial_println!("Power: CPU {} frequency changed to {} MHz", 
                self.cpu_id, self.frequencies[freq_idx as usize].frequency_mhz);
        }
        
        Ok(())
    }
    
    /// Turbo modunun kullanılıp kullanılamayacağını kontrol eder
    pub fn can_use_turbo(&self) -> bool {
        // Basit turbo kullanılabilirlik kontrolü
        // Gerçek uygulamada sıcaklık sınırları, güç bütçesi vb. kontrol edilirdi
        let load = self.current_load.load(Ordering::Acquire);
        load < 80 // Sadece yük çok yüksek değilse turbo kullan — termal baskıyı önler
    }
    
    /// Boşta durumuna gir — tahmini boşta kalma süresine göre en uygun C-durumunu seçer
    pub fn enter_idle(&self, idle_time_us: u32) -> CpuState {
        if !self.pm_enabled.load(Ordering::Acquire) {
            return CpuState::C0;
        }
        
        // Verilen süre için en iyi boşta durumunu bul — daha derin = daha az enerji
        let mut best_state = &self.idle_states[0]; // Varsayılan olarak C1
        
        for state in &self.idle_states {
            if state.is_better_than(best_state, idle_time_us) {
                best_state = state;
            }
        }
        
        // Seçilen duruma gir — istatistikleri güncelle ve çekirdek durumunu işaretle
        best_state.enter();
        self.set_current_cstate(best_state.state);
        self.in_idle.store(true, Ordering::Release);
        
        best_state.state
    }
    
    /// Boşta durumundan çık — geçirilen süreyi kaydeder ve CPU'yu C0'a döndürür
    pub fn exit_idle(&self, duration_ticks: u64) {
        let current_state = self.get_current_cstate();
        
        // Mevcut durum için istatistikleri güncelle — enerji tasarrufu profilleme için
        if let Some(state) = self.idle_states.iter().find(|s| s.state == current_state) {
            state.exit(duration_ticks);
        }
        
        // C-durum istatistiklerini güncelle — her seviyede kaç tick geçirildiğini takip eder
        let state_idx = current_state as usize;
        if state_idx < 6 {
            self.cstate_time[state_idx].fetch_add(duration_ticks, Ordering::Relaxed);
            self.cstate_count[state_idx].fetch_add(1, Ordering::Relaxed);
        }
        
        // C0'a dön — CPU aktif çalışmaya hazır, boşta değil işaretini kaldır
        self.set_current_cstate(CpuState::C0);
        self.in_idle.store(false, Ordering::Release);
        smp_mb();
    }
    
    /// CPU yükleme oranını güncelle — valisator hesapları için EMA tablaylı ortalama tutar
    pub fn update_load(&self, load: u32) {
        self.current_load.store(load, Ordering::Release);
        
        // Ortalama yükü güncelle (katlamalı hareketli ortalama) — 0.75 ağırlıkla eski değeri koru
        let current_avg = self.avg_load.load(Ordering::Acquire);
        let new_avg = (current_avg * 3 + load) / 4; // Eski değere 0.75 ağırlık ver
        self.avg_load.store(new_avg, Ordering::Release);
        
        // Frekans valisatorunu uygula — yeni yükü dikkate alarak frekansı belirle
        self.apply_governor();
    }
    
    /// Frekans valisatorunu uygula — seçili politikaya göre CPU frekansını ayarlar
    fn apply_governor(&self) {
        let governor = self.get_governor();
        let load = self.avg_load.load(Ordering::Acquire);
        
        match governor {
            FreqGovernor::Performance => {
                self.set_frequency(self.max_freq_idx);
            }
            FreqGovernor::Powersave => {
                self.set_frequency(self.min_freq_idx);
            }
            FreqGovernor::OnDemand => {
                self.apply_ondemand_governor(load);
            }
            FreqGovernor::Conservative => {
                self.apply_conservative_governor(load);
            }
            FreqGovernor::Schedutil => {
                self.apply_schedutil_governor(load);
            }
            FreqGovernor::Userspace => {
                // Kullanıcı tarafından kontrol edilir, otomatik değişiklik yok
            }
        }
    }
    
    /// İsteğe bağlı valisatoru uygula — yüke göre frekansı agresif biçimde yukarı/aşağı ayarlar
    fn apply_ondemand_governor(&self, load: u32) {
        let current_idx = self.current_pstate.load(Ordering::Acquire);
        
        if load > 80 && current_idx < self.max_freq_idx {
            // Frekansı artır — yüksek yük, daha fazla performans gerektirir
            self.set_frequency(current_idx + 1);
        } else if load < 20 && current_idx > self.min_freq_idx {
            // Frekansı azalt — düşük yükte enerji tasarrufu yap
            self.set_frequency(current_idx - 1);
        }
    }
    
    /// Muhafazakâr valisatoru uygula — frekansı kademeli döngülerle ayarlar, aşırı sallanmayı önler
    fn apply_conservative_governor(&self, load: u32) {
        let current_idx = self.current_pstate.load(Ordering::Acquire);
        
        // İsteğe bağlıya göre daha yavaş değişiklikler — yüksek eşik 90, düşük eşik 10 ile karar alınır
        if load > 90 && current_idx < self.max_freq_idx {
            self.set_frequency(current_idx + 1);
        } else if load < 10 && current_idx > self.min_freq_idx {
            self.set_frequency(current_idx - 1);
        }
    }
    
    /// Schedutil valisatorunu uygula — Linux'daki gibi yüklemeyi doğrudan frekans aralığına eşler
    fn apply_schedutil_governor(&self, load: u32) {
        // Zamanlama tabanlı frekans seçimi — görev çalıştırılabilirlik bilgisi kullanılır
        // Yükü doğrudan frekansa eşle — proporaşiyonel bir model
        let freq_range = self.max_freq_idx - self.min_freq_idx;
        let target_idx = self.min_freq_idx + (load * freq_range / 100);
        
        self.set_frequency(target_idx);
    }
    
    /// Mevcut valisatörü döndür — atomik değerden FreqGovernor enum'unu çözümler
    pub fn get_governor(&self) -> FreqGovernor {
        match self.governor.load(Ordering::Acquire) {
            0 => FreqGovernor::Performance,
            1 => FreqGovernor::Powersave,
            2 => FreqGovernor::Userspace,
            3 => FreqGovernor::OnDemand,
            4 => FreqGovernor::Conservative,
            5 => FreqGovernor::Schedutil,
            _ => FreqGovernor::OnDemand,
        }
    }
    
    /// Frekans valisatorünü ayarla — politikayı hemen uygular ve log yazar
    pub fn set_governor(&self, governor: FreqGovernor) {
        self.governor.store(governor as u32, Ordering::Release);
        smp_wmb();
        
        // Yeni valisatörü anında uygula — mevcut yüke göre ilk frekans kararını ver
        self.apply_governor();
        
        crate::serial_println!("Power: CPU {} governor changed to {:?}", self.cpu_id, governor);
    }
    
    /// Güç yönetimini etkinleştir/devre dışı bırak — test veya debug sırasında tam kontrol için
    pub fn set_pm_enabled(&self, enabled: bool) {
        self.pm_enabled.store(enabled, Ordering::Release);
        smp_wmb();
        
        if !enabled {
            // Devre dışı bırakıldığında C0'a ve maksimum frekansa geri dön — öngörülebilir performans sağlar
            self.set_current_cstate(CpuState::C0);
            self.set_frequency(self.max_freq_idx);
        }
    }
    
    /// Güç istatistiklerini döndür — tüm C-durumu ve frekans geçiş verilerini özetle
    pub fn get_power_stats(&self) -> PowerStats {
        let mut cstate_times = [0u64; 6];
        let mut cstate_counts = [0u64; 6];
        
        for i in 0..6 {
            cstate_times[i] = self.cstate_time[i].load(Ordering::Relaxed);
            cstate_counts[i] = self.cstate_count[i].load(Ordering::Relaxed);
        }
        
        PowerStats {
            current_frequency: self.get_current_frequency(),
            current_load: self.current_load.load(Ordering::Relaxed),
            avg_load: self.avg_load.load(Ordering::Relaxed),
            freq_transitions: self.freq_transitions.load(Ordering::Relaxed),
            cstate_times,
            cstate_counts,
            idle_state_stats: self.idle_states.iter().map(|s| s.get_stats()).collect(),
        }
    }
}

/// Guc istatistikleri - mevcut frekans, yuklenme ve C-durum sure verilerini ozetler
#[derive(Debug, Clone)]
pub struct PowerStats {
    pub current_frequency: Option<CpuFrequency>,
    pub current_load: u32,
    pub avg_load: u32,
    pub freq_transitions: u64,
    pub cstate_times: [u64; 6],
    pub cstate_counts: [u64; 6],
    pub idle_state_stats: Vec<(u64, u64)>,
}

/// Guc yonetimi yoneticisi - tum CPU'larin P/C-durum politikasini merkezi olarak yonetir
pub struct PowerManager {
    /// Maksimum CPU sayısı — sistem başlangıcında belirlenir, dinamik olarak değişmez
    max_cpus: u32,
    /// CPU güç tanımlayıcıları — her mantiksal çekirdek için ayrı bir tanımlayıcı tutulur
    cpu_descs: Vec<RcuPtr<CpuPowerDesc>>,
    /// Global güç yönetimi etkin mi — tüm CPU'lar için ortak açma/kapama dümeşi
    pm_enabled: AtomicBool,
    /// Global güç politikası — tüm çekirdeklere aynı valisatörü uygulamak için
    global_policy: AtomicU32, // FreqGovernor as u32
    /// Güç istatistikleri — sistem genelinde enerji tasarrufu ve geçiş sayıları
    stats: PowerManagerStats,
}

/// Guc yoneticisi istatistikleri - sistem genelinde toplam bosta gecis, frekans degisikligi ve enerji tasarrufu
#[derive(Debug)]
pub struct PowerManagerStats {
    pub total_idle_transitions: AtomicU64,
    pub total_freq_changes: AtomicU64,
    pub total_energy_saved: AtomicU64, // Miliwatt-saat cinsinden (yaklasik deger)
}

impl PowerManagerStats {
    pub const fn new() -> Self {
        Self {
            total_idle_transitions: AtomicU64::new(0),
            total_freq_changes: AtomicU64::new(0),
            total_energy_saved: AtomicU64::new(0),
        }
    }
    
    pub fn record_idle_transition(&self) {
        self.total_idle_transitions.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn record_freq_change(&self) {
        self.total_freq_changes.fetch_add(1, Ordering::Relaxed);
    }
    
    pub fn record_energy_saved(&self, energy_mwh: u64) {
        self.total_energy_saved.fetch_add(energy_mwh, Ordering::Relaxed);
    }
    
    pub fn get_stats(&self) -> (u64, u64, u64) {
        (
            self.total_idle_transitions.load(Ordering::Relaxed),
            self.total_freq_changes.load(Ordering::Relaxed),
            self.total_energy_saved.load(Ordering::Relaxed),
        )
    }
}

impl PowerManager {
    /// Yeni güç yöneticisi oluştur — tüm CPU'lar için varsayılan OnDemand valisatörüyle başlat
    pub fn new(max_cpus: u32) -> Self {
        let mut cpu_descs = Vec::with_capacity(max_cpus as usize);
        
        // CPU güç tanımlayıcılarını başlat — her çekirdek kendi bağımsız durumunu yönetir
        for cpu_id in 0..max_cpus {
            let desc = Box::new(CpuPowerDesc::new(cpu_id));
            cpu_descs.push(RcuPtr::new(Box::into_raw(desc)));
        }
        
        Self {
            max_cpus,
            cpu_descs,
            pm_enabled: AtomicBool::new(true),
            global_policy: AtomicU32::new(FreqGovernor::OnDemand as u32),
            stats: PowerManagerStats::new(),
        }
    }
    
    /// CPU güç tanımlayıcısını getir — geçersiz kimlikde None döndürür
    pub fn get_cpu_desc(&self, cpu_id: u32) -> Option<RcuPtr<CpuPowerDesc>> {
        if cpu_id >= self.max_cpus {
            return None;
        }
        
        Some(self.cpu_descs[cpu_id as usize].clone())
    }
    
    /// CPU için boşta durumuna gir — en uygun C-durumunu seçer ve istatistik kaydeder
    pub fn cpu_idle_enter(&self, cpu_id: u32, idle_time_us: u32) -> Result<CpuState, PowerError> {
        if !self.pm_enabled.load(Ordering::Acquire) {
            return Ok(CpuState::C0);
        }
        
        let desc = match self.get_cpu_desc(cpu_id) {
            Some(desc) => desc,
            None => return Err(PowerError::InvalidCpuId),
        };
        
        let state = desc.read().enter_idle(idle_time_us);
        self.stats.record_idle_transition();
        
        Ok(state)
    }
    
    /// CPU için boşta durumundan çık — geçirilen süreyi kaydeder ve C0'a döndürür
    pub fn cpu_idle_exit(&self, cpu_id: u32, duration_ticks: u64) -> Result<(), PowerError> {
        let desc = match self.get_cpu_desc(cpu_id) {
            Some(desc) => desc,
            None => return Err(PowerError::InvalidCpuId),
        };
        
        desc.read().exit_idle(duration_ticks);
        Ok(())
    }
    
    /// CPU yükleme oranını güncelle — valisator kararını tetikler
    pub fn update_cpu_load(&self, cpu_id: u32, load: u32) -> Result<(), PowerError> {
        let desc = match self.get_cpu_desc(cpu_id) {
            Some(desc) => desc,
            None => return Err(PowerError::InvalidCpuId),
        };
        
        desc.read().update_load(load);
        Ok(())
    }
    
    /// Belirli bir CPU için frekans valisatörünü ayarla
    pub fn set_cpu_governor(&self, cpu_id: u32, governor: FreqGovernor) -> Result<(), PowerError> {
        let desc = match self.get_cpu_desc(cpu_id) {
            Some(desc) => desc,
            None => return Err(PowerError::InvalidCpuId),
        };
        
        desc.read().set_governor(governor);
        Ok(())
    }
    
    /// Tüm CPU'lar için global frekans valisatörünü ayarla — sistem politikasını tek noktadan yönetir
    pub fn set_global_governor(&self, governor: FreqGovernor) {
        self.global_policy.store(governor as u32, Ordering::Release);
        smp_wmb();
        
        // Tüm CPU'lara uygula — sistemi tuğarlı bir güç politikasyla yönetir
        for cpu_id in 0..self.max_cpus {
            if let Some(desc) = self.get_cpu_desc(cpu_id) {
                let _ = self.set_cpu_governor(cpu_id, governor);
            }
        }
        
        crate::serial_println!("Power: Global governor changed to {:?}", governor);
    }
    
    /// Güç yönetimini etkinleştir/devre dışı bırak — sistem genelinde enerji tasarrufu modunu kontrol eder
    pub fn set_pm_enabled(&self, enabled: bool) {
        self.pm_enabled.store(enabled, Ordering::Release);
        smp_wmb();
        
        // Tüm CPU'lara uygula — her çekirdek kendi durumunu günceller
        for cpu_id in 0..self.max_cpus {
            if let Some(desc) = self.get_cpu_desc(cpu_id) {
                desc.read().set_pm_enabled(enabled);
            }
        }
        
        crate::serial_println!("Power: Power management {}", if enabled { "enabled" } else { "disabled" });
    }
    
    /// Belirli bir CPU için güç istatistiklerini al
    pub fn get_cpu_stats(&self, cpu_id: u32) -> Result<PowerStats, PowerError> {
        let desc = match self.get_cpu_desc(cpu_id) {
            Some(desc) => desc,
            None => return Err(PowerError::InvalidCpuId),
        };
        
        Ok(desc.read().get_power_stats())
    }
    
    /// Global güç yöneticisi istatistiklerini al — (boşta geçiş, frekans değişikliği, tasarruf) üçlüsü
    pub fn get_global_stats(&self) -> (u64, u64, u64) {
        self.stats.get_stats()
    }
    
    /// Sistemi askıya al (basitleştirilmiş) — gerçekte ACPI S3/S4 hazırlık adımları izlenir
    pub fn system_suspend(&self) -> Result<(), PowerError> {
        crate::serial_println!("Power: Preparing system suspend...");
        
        // Tüm CPU'ların durumunu kaydet — en derin uykuya hazırlık yap
        for cpu_id in 0..self.max_cpus {
            if let Some(desc) = self.get_cpu_desc(cpu_id) {
                let desc_guard = desc.read();
                desc_guard.set_pm_enabled(false);
                desc_guard.set_current_cstate(CpuState::C7); // En derin uyku - tam gucu kapat
            }
        }
        
        // Gerçek uygulamada şu adımlar izlenirdi:
        // 1. Tüm cihaz durumlarını kaydet — PCI, USB, ağ kartı vb.
        // 2. Önbellekleri boşalt — RAM'e temiz yazım garantisi
        // 3. Platforma özgü askıya alma durumuna gir — ACPI Sx güç durumu
        
        crate::serial_println!("Power: System suspended");
        Ok(())
    }
    
    /// Sistemi sürdür (basitleştirilmiş) — askıya alınan cihazları ve CPU'ları yeniden etkinleştirir
    pub fn system_resume(&self) -> Result<(), PowerError> {
        crate::serial_println!("Power: Resuming system...");
        
        // Tüm CPU'ları geri yükle — C0'a ve tam performansa dön
        for cpu_id in 0..self.max_cpus {
            if let Some(desc) = self.get_cpu_desc(cpu_id) {
                let desc_guard = desc.read();
                desc_guard.set_pm_enabled(true);
                desc_guard.set_current_cstate(CpuState::C0);
                desc_guard.set_frequency(desc_guard.max_freq_idx);
            }
        }
        
        // Gerçek uygulamada şu adımlar izlenirdi:
        // 1. Cihaz durumlarını geri yükle — kaydedilen register değerlerini yaz
        // 2. Önbellekleri yeniden başlat — TLB ve cache tutarlılığını yenile
        // 3. Diğer CPU'ları uyandır — IPI göndererek diğer çekirdekleri aktif et
        
        crate::serial_println!("Power: System resumed");
        Ok(())
    }
}

/// Guc yonetimi hatalari - gecersiz islem veya erisilemez durum hatalarini temsil eder
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerError {
    /// Geçersiz CPU kimliği — istenen CPU mevcut değil
    InvalidCpuId,
    /// Geçersiz frekans — desteklenmeyen frekans indeksi talep edildi
    InvalidFrequency,
    /// Turbo modu kullanılamaz — sıcaklık veya güç baskısı engelliyor
    TurboUnavailable,
    /// Güç yönetimi devre dışı — işlem yapılabilmesi için önce etkinleştirilmeli
    PowerManagementDisabled,
    /// Geçersiz durum geçişi — izin verilmeyen bir C-durum geçişi denendi
    InvalidStateTransition,
}

/// Global power manager instance
static mut POWER_MANAGER: Option<PowerManager> = None;
static POWER_INIT: AtomicBool = AtomicBool::new(false);

/// Güç yönetimi alt sistemini başlat — belirtilen sayıda CPU için tanımlayıcıları oluşturur
pub fn init(max_cpus: u32) {
    if POWER_INIT.load(Ordering::Acquire) {
        return;
    }
    
    crate::serial_println!("Power: Initializing power management for {} CPUs", max_cpus);
    
    let manager = PowerManager::new(max_cpus);
    
    unsafe {
        POWER_MANAGER = Some(manager);
    }
    
    POWER_INIT.store(true, Ordering::Release);
    smp_mb();
    
    crate::serial_println!("Power: Power management initialized");
}

/// Global güç yöneticisini döndür — başlatma yapılmadıysa None gelir
pub fn get_manager() -> Option<&'static PowerManager> {
    if !POWER_INIT.load(Ordering::Acquire) {
        return None;
    }
    
    unsafe { POWER_MANAGER.as_ref() }
}

/// Yaygın işlemler için kolaylık fonksiyonları — güç yöneticisini doğrudan sarıp kullanıcıya sunar
pub fn cpu_idle_enter(cpu_id: u32, idle_time_us: u32) -> Result<CpuState, PowerError> {
    let manager = get_manager().ok_or(PowerError::PowerManagementDisabled)?;
    manager.cpu_idle_enter(cpu_id, idle_time_us)
}

pub fn cpu_idle_exit(cpu_id: u32, duration_ticks: u64) -> Result<(), PowerError> {
    let manager = get_manager().ok_or(PowerError::PowerManagementDisabled)?;
    manager.cpu_idle_exit(cpu_id, duration_ticks)
}

pub fn update_cpu_load(cpu_id: u32, load: u32) -> Result<(), PowerError> {
    let manager = get_manager().ok_or(PowerError::PowerManagementDisabled)?;
    manager.update_cpu_load(cpu_id, load)
}

pub fn set_cpu_governor(cpu_id: u32, governor: FreqGovernor) -> Result<(), PowerError> {
    let manager = get_manager().ok_or(PowerError::PowerManagementDisabled)?;
    manager.set_cpu_governor(cpu_id, governor)
}

pub fn set_global_governor(governor: FreqGovernor) {
    if let Some(manager) = get_manager() {
        manager.set_global_governor(governor);
    }
}

pub fn system_suspend() -> Result<(), PowerError> {
    let manager = get_manager().ok_or(PowerError::PowerManagementDisabled)?;
    manager.system_suspend()
}

pub fn system_resume() -> Result<(), PowerError> {
    let manager = get_manager().ok_or(PowerError::PowerManagementDisabled)?;
    manager.system_resume()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_cpu_power_desc() {
        let desc = CpuPowerDesc::new(0);
        assert_eq!(desc.get_current_cstate(), CpuState::C0);
        assert!(desc.pm_enabled.load(Ordering::Acquire));
        
        desc.set_governor(FreqGovernor::Performance);
        assert_eq!(desc.get_governor(), FreqGovernor::Performance);
    }
    
    #[test]
    fn test_idle_states() {
        let c1 = CpuIdleState::new(CpuState::C1, 1, 100, 2);
        let c2 = CpuIdleState::new(CpuState::C2, 10, 50, 10);
        
        assert!(c2.is_better_than(&c1, 15)); // 15us > C2 target residency
        assert!(!c2.is_better_than(&c1, 5));  // 5us < C2 target residency
    }
    
    #[test]
    fn test_power_manager() {
        let manager = PowerManager::new(4);
        assert!(manager.pm_enabled.load(Ordering::Acquire));
        
        assert!(manager.cpu_idle_enter(0, 10).is_ok());
        assert!(manager.cpu_idle_exit(0, 100).is_ok());
    }
}

// ============================================================================
// ACPI S-STATES (Sleep States)
// ============================================================================

/// ACPI sleep states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepState {
    /// S1: Güç Askıya Alınması — CPU durdurulur, RAM korunur, hızlı geri dönüş
    S1,
    /// S2: CPU kapatılır, bağlam RAM'e kaydedilir, S1'den daha derin uyku
    S2,
    /// S3: RAM'e Askıya Alınma (STR) — düşuk güç, birkaç saniyede geri dönüş
    S3,
    /// S4: Diske Askıya Alınma (Hibernate) — en düşük güç, RAM içeriği diske yazılır
    S4,
}

impl SleepState {
    pub fn to_acpi(&self) -> u8 {
        match self {
            SleepState::S1 => 1,
            SleepState::S2 => 2,
            SleepState::S3 => 3,
            SleepState::S4 => 4,
        }
    }
}

// ============================================================================
// BATTERY MANAGEMENT
// ============================================================================

/// Battery status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryStatus {
    Discharging,
    Charging,
    Critical,
    Full,
    Unknown,
}

/// Battery information
#[derive(Debug, Clone)]
pub struct BatteryInfo {
    pub id: u32,
    pub present: bool,
    pub status: BatteryStatus,
    pub capacity_percent: u32,
    pub voltage_mv: u32,
    pub current_ma: i32,
    pub remaining_capacity_mwh: u32,
    pub full_capacity_mwh: u32,
    pub design_capacity_mwh: u32,
    pub time_to_empty_sec: u32,
    pub time_to_full_sec: u32,
    pub temperature_celsius: i32,
    pub manufacturer: alloc::string::String,
    pub model: alloc::string::String,
}

impl BatteryInfo {
    pub fn new(id: u32) -> Self {
        BatteryInfo {
            id,
            present: false,
            status: BatteryStatus::Unknown,
            capacity_percent: 0,
            voltage_mv: 0,
            current_ma: 0,
            remaining_capacity_mwh: 0,
            full_capacity_mwh: 0,
            design_capacity_mwh: 0,
            time_to_empty_sec: 0,
            time_to_full_sec: 0,
            temperature_celsius: 25,
            manufacturer: alloc::string::String::new(),
            model: alloc::string::String::new(),
        }
    }

    pub fn is_low(&self) -> bool {
        self.capacity_percent < 20
    }

    pub fn is_critical(&self) -> bool {
        self.capacity_percent < 5 || self.status == BatteryStatus::Critical
    }

    pub fn health_percent(&self) -> u32 {
        if self.design_capacity_mwh > 0 {
            (self.full_capacity_mwh * 100) / self.design_capacity_mwh
        } else {
            100
        }
    }
}

// ============================================================================
// THERMAL MANAGEMENT
// ============================================================================

/// Thermal trip type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThermalTripType {
    Active,
    Passive,
    Critical,
    Hot,
}

/// Thermal trip point
#[derive(Debug, Clone)]
pub struct ThermalTripPoint {
    pub trip_type: ThermalTripType,
    pub temperature_celsius: i32,
    pub hysteresis_celsius: i32,
}

impl ThermalTripPoint {
    pub fn new(trip_type: ThermalTripType, temp: i32) -> Self {
        ThermalTripPoint {
            trip_type,
            temperature_celsius: temp,
            hysteresis_celsius: 2,
        }
    }
}

/// Thermal zone information
#[derive(Debug, Clone)]
pub struct ThermalZoneInfo {
    pub id: u32,
    pub name: alloc::string::String,
    pub temperature_celsius: i32,
    pub trip_points: Vec<ThermalTripPoint>,
    pub passive_temp: i32,
    pub critical_temp: i32,
}

impl ThermalZoneInfo {
    pub fn new(id: u32, name: &str) -> Self {
        ThermalZoneInfo {
            id,
            name: alloc::string::String::from(name),
            temperature_celsius: 25,
            trip_points: Vec::new(),
            passive_temp: 80,
            critical_temp: 95,
        }
    }

    pub fn is_overheating(&self) -> bool {
        self.temperature_celsius >= self.critical_temp
    }

    pub fn needs_cooling(&self) -> bool {
        self.temperature_celsius >= self.passive_temp
    }

    pub fn add_trip_point(&mut self, trip: ThermalTripPoint) {
        match trip.trip_type {
            ThermalTripType::Passive => self.passive_temp = trip.temperature_celsius,
            ThermalTripType::Critical => self.critical_temp = trip.temperature_celsius,
            _ => {}
        }
        self.trip_points.push(trip);
    }
}

/// Cooling device type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoolingType {
    Fan,
    Processor,
    Lcd,
    Gpu,
}

/// Cooling device information
#[derive(Debug, Clone)]
pub struct CoolingDeviceInfo {
    pub id: u32,
    pub cooling_type: CoolingType,
    pub name: alloc::string::String,
    pub state: u32,
    pub max_state: u32,
    pub min_state: u32,
}

impl CoolingDeviceInfo {
    pub fn new(id: u32, cooling_type: CoolingType, name: &str) -> Self {
        CoolingDeviceInfo {
            id,
            cooling_type,
            name: alloc::string::String::from(name),
            state: 0,
            max_state: 10,
            min_state: 0,
        }
    }

    pub fn set_state(&mut self, state: u32) {
        self.state = state.clamp(self.min_state, self.max_state);
    }

    pub fn increase(&mut self) {
        if self.state < self.max_state {
            self.state += 1;
        }
    }

    pub fn decrease(&mut self) {
        if self.state > self.min_state {
            self.state -= 1;
        }
    }

    pub fn percent(&self) -> u32 {
        if self.max_state > self.min_state {
            ((self.state - self.min_state) * 100) / (self.max_state - self.min_state)
        } else {
            0
        }
    }
}

// ============================================================================
// GLOBAL POWER STATE
// ============================================================================

use spin::Mutex;
use alloc::collections::BTreeMap;

lazy_static::lazy_static! {
    static ref BATTERIES: Mutex<BTreeMap<u32, BatteryInfo>> = Mutex::new(BTreeMap::new());
    static ref THERMAL_ZONES: Mutex<BTreeMap<u32, ThermalZoneInfo>> = Mutex::new(BTreeMap::new());
    static ref COOLING_DEVICES: Mutex<BTreeMap<u32, CoolingDeviceInfo>> = Mutex::new(BTreeMap::new());
}

/// ACPI güç yönetimini başlat — termal bölgeler, souğutma cihazları ve batarya kayıtlarını oluşturur
pub fn init_acpi_power() {
    // CPU için varsayılan termal bölgeyi başlat — sıcaklık aşımında pasif souğutma tetiklenir
    let mut zone = ThermalZoneInfo::new(0, "CPU");
    zone.add_trip_point(ThermalTripPoint::new(ThermalTripType::Passive, 80));
    zone.add_trip_point(ThermalTripPoint::new(ThermalTripType::Critical, 95));
    THERMAL_ZONES.lock().insert(0, zone);

    // Varsayılan souğutma cihazını başlat — fan ve işlemci hızı kademeli kontrol edilir
    let cooling = CoolingDeviceInfo::new(0, CoolingType::Processor, "CPU Cooling");
    COOLING_DEVICES.lock().insert(0, cooling);

    // Varsayılan batarya kaydını başlat — laptop veya güç kaynağı bilgileri için yer tutucu
    let battery = BatteryInfo::new(0);
    BATTERIES.lock().insert(0, battery);

    crate::serial_println!("[PWR] ACPI power management initialized");
}

/// Batarya bilgisini getir — ACPI BIF/BST metodlarından alınan kapasite ve durum verileri
pub fn get_battery(id: u32) -> Option<BatteryInfo> {
    BATTERIES.lock().get(&id).cloned()
}

/// Tüm batarya kayıtlarını getir — çoklu bataryalı sistemlerde tümünü döndürür
pub fn get_all_batteries() -> Vec<BatteryInfo> {
    BATTERIES.lock().values().cloned().collect()
}

/// Ortalama batarya yüzdesini getir — birden fazla batarya varsa aritmeti ortalama alınır
pub fn get_battery_percent() -> u32 {
    let batteries = BATTERIES.lock();
    let batteries: Vec<_> = batteries.values().filter(|b| b.present).collect();
    if batteries.is_empty() {
        return 100;
    }
    batteries.iter().map(|b| b.capacity_percent).sum::<u32>() / batteries.len() as u32
}

/// Batarya düşük mü — %20'nin altında uyarı tetiklenir
pub fn is_battery_low() -> bool {
    get_battery_percent() < 20
}

/// Batarya kritik seviyede mi — %5'in altında acil durum işlemleri başlatılabilir
pub fn is_battery_critical() -> bool {
    get_battery_percent() < 5
}

/// Termal bölge bilgisini getir — ACPI TZ nesnelerinden alınan sıcaklık ve eşik verileri
pub fn get_thermal_zone(id: u32) -> Option<ThermalZoneInfo> {
    THERMAL_ZONES.lock().get(&id).cloned()
}

/// Tüm termal bölgeleri getir — CPU, GPU, batarya gibi farklı sıcaklık noktaları
pub fn get_all_thermal_zones() -> Vec<ThermalZoneInfo> {
    THERMAL_ZONES.lock().values().cloned().collect()
}

/// Ortalama sıcaklığı getir — tüm termal bölgelerden basit ortalama hesaplanır
pub fn get_average_temperature() -> i32 {
    let zones = THERMAL_ZONES.lock();
    let zones: Vec<_> = zones.values().collect();
    if zones.is_empty() {
        return 25;
    }
    zones.iter().map(|z| z.temperature_celsius).sum::<i32>() / zones.len() as i32
}

/// Sistem aşırı ısınıyor mu — herhangi bir bölge kritik eşiğini aştıysa true dönür
pub fn is_overheating() -> bool {
    THERMAL_ZONES.lock().values().any(|z| z.is_overheating())
}

/// Termal bölgeleri güncelle ve souğutma uygula — sıcaklığa göre fan hızını veya CPU oranını ayarla
pub fn update_thermal() {
    let mut zones = THERMAL_ZONES.lock();
    let mut cooling = COOLING_DEVICES.lock();

    for zone in zones.values_mut() {
        if zone.needs_cooling() {
            for device in cooling.values_mut() {
                device.increase();
            }
        } else if zone.temperature_celsius < zone.passive_temp - 5 {
            for device in cooling.values_mut() {
                device.decrease();
            }
        }
    }
}

/// Souğutma cihazı bilgisini getir — fan durumu ve çalışma seviyesi bilgisi
pub fn get_cooling_device(id: u32) -> Option<CoolingDeviceInfo> {
    COOLING_DEVICES.lock().get(&id).cloned()
}

/// Tüm souğutma cihazlarını getir — fan, işlemci hızı ve LCD titiz parıltısı gibi mekanizmalar
pub fn get_all_cooling_devices() -> Vec<CoolingDeviceInfo> {
    COOLING_DEVICES.lock().values().cloned().collect()
}

/// Uyku durumuna gir — S1-S4 arasında ACPI uyku geçişini başlatır
pub fn enter_sleep(state: SleepState) -> Result<(), PowerError> {
    crate::serial_println!("[PWR] Entering sleep state S{}", state.to_acpi());
    // Gerçek uygulamada ACPI metodları çağrılırdı — \_SB.SLP veya FADT SLP_TYP yazımı
    Ok(())
}

/// Sistemi kapat — ACPI S5 (Soft Off) durumuna geçişi başlatır
pub fn system_shutdown() -> Result<(), PowerError> {
    crate::serial_println!("[PWR] System shutdown requested");
    // Gerçek uygulamada ACPI metodları çağrılırdı — PM1a_CNT register'ına SLP_EN yazılır
    Ok(())
}
