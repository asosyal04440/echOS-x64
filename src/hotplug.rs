//! # echOS CPU Hot-Plug Destek Modülü
//!
//! Katman 1 işletim sistemi seviyesinde çalışma zamanı CPU ekleme/çıkarma desteği.
//! Linux CPU hotplug ile eşdeğer yetenekler sunar.
//!
//! ## CPU Hotplug Nedir?
//! CPU hotplug, sistemin çalışırken CPU çekirdeklerini etkinleştirme veya
//! devre dışı bırakma yeteneğidir. Kullanım durumları:
//! - Güç tasarrufu: az yük altında bazı CPU'ları kapat
//! - Hata toleransı: arızalı CPU'yu çalışma zamanında çıkar
//! - Sanallaştırma: sanal makineye dinamik CPU ekleme/çıkarma

use crate::memory_barriers::{smp_mb, smp_rmb, smp_wmb};
use crate::preempt::{preempt_enabled, PreemptDisableGuard};
use crate::rcu::{synchronize_rcu, RcuPtr};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

/// CPU hotplug durum makinesi (Linux cpu_states ile uyumlu).
/// Durum geçişleri: Offline → ComingUp → Online → GoingDown → Offline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum CpuState {
    /// CPU çevrimdışı ve kullanılamaz
    Offline = 0,
    /// CPU açılıyor (çevrimiçie hazırlanıyor)
    ComingUp = 1,
    /// CPU çevrimiçi ve kullanılabilir
    Online = 2,
    /// CPU kapanıyor (çevrimdışına hazırlanıyor)
    GoingDown = 3,
    /// CPU ölü durumda ve kullanılamaz
    Dead = 4,
}

/// CPU hotplug olayları.
/// Olay bildirimleri, tüm kayıtlı geri çağrılara gönderilir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum CpuHotplugEvent {
    /// CPU çevrimiçe alınmak üzere hazırlanıyor
    PrepareOnline = 0,
    /// CPU başarıyla çevrimiçi oldu
    Online = 1,
    /// CPU çevrimdışına alınmak üzere hazırlanıyor
    PrepareOffline = 2,
    /// CPU başarıyla çevrimdışı oldu
    Offline = 3,
    /// CPU beklenmedik şekilde öldü
    Dead = 4,
}

/// CPU hotplug bildirim geri çağrı türü.
/// Tüm hotplug olaylarında çağrılır; hata durumunda işlem iptal edilir.
pub type HotplugCallback = fn(cpu_id: u32, event: CpuHotplugEvent) -> Result<(), HotplugError>;

/// Hotplug hata türleri.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotplugError {
    /// CPU kimliği geçersiz
    InvalidCpuId,
    /// CPU zaten hedef durumda
    AlreadyInState,
    /// Mevcut durumda bu işleme izin verilmiyor
    InvalidStateTransition,
    /// CPU'yu işlem için hazırlama başarısız
    PrepareFailed,
    /// İşlem tamamlanamadı
    OperationFailed,
    /// Bellek yetersiz
    OutOfMemory,
    /// Geri çağrı hata döndürdü
    CallbackError,
}

/// CPU hotplug tanımlayıcısı.
/// 64 bayta hizalanmış (cache line boyutu) — yanlış paylaşımı (false sharing) önler.
/// Her alandaki atomik tipler, çok çekirdekli güvenli erişimi garanti eder.
#[repr(C, align(64))]
pub struct CpuHotplugDesc {
    /// CPU kimliği
    pub cpu_id: u32,
    /// Mevcut durum (u32 olarak saklanır, CpuState enum'undan dönüştürülür)
    pub state: AtomicU32,
    /// Geçiş hedef durumu
    pub target_state: AtomicU32,
    /// APIC kimliği (x86 için — kesinti yönlendirmede kullanılır)
    pub apic_id: u32,
    /// ACPI işlemci UID'si (ACPI tablolarından gelen benzersiz kimlik)
    pub acpi_uid: u32,
    /// CPU aile/model/adımlama bilgisi (CPUID leaf 1'den)
    pub cpu_signature: u32,
    /// CPU özellik bit maskesi (SSE, AVX, vb.)
    pub cpu_features: u64,
    /// Fiziksel CPU paket (soket) kimliği
    pub package_id: u32,
    /// Fiziksel CPU çekirdek kimliği
    pub core_id: u32,
    /// Mantıksal CPU iş parçacığı kimliği
    pub thread_id: u32,
    /// NUMA düğüm kimliği
    pub numa_node: u32,
    /// CPU şu anda çevrimiçi mi?
    pub online: AtomicBool,
    /// CPU şu anda hotplug işleminde mi?
    pub hotplugging: AtomicBool,
    /// Bu CPU'ya yapılan referans sayısı
    pub refcount: AtomicUsize,
    /// Son hotplug işleminin zaman damgası (tik sayısı)
    pub last_hotplug: AtomicU64,
    /// Toplam hotplug deneme sayısı
    pub hotplug_attempts: AtomicU32,
    /// Yanlış paylaşımı önlemek için dolgu
    _padding: [u8; 0],
}

impl CpuHotplugDesc {
    /// Belirtilen CPU ve APIC kimliği için yeni bir hotplug tanımlayıcısı oluşturur.
    /// Başlangıç durumu: Offline, referans sayısı = 0.
    pub fn new(cpu_id: u32, apic_id: u32) -> Self {
        Self {
            cpu_id,
            state: AtomicU32::new(CpuState::Offline as u32),
            target_state: AtomicU32::new(CpuState::Offline as u32),
            apic_id,
            acpi_uid: 0,
            cpu_signature: 0,
            cpu_features: 0,
            package_id: 0,
            core_id: 0,
            thread_id: 0,
            numa_node: 0,
            online: AtomicBool::new(false),
            hotplugging: AtomicBool::new(false),
            refcount: AtomicUsize::new(0),
            last_hotplug: AtomicU64::new(0),
            hotplug_attempts: AtomicU32::new(0),
            _padding: [0; 0],
        }
    }

    /// Mevcut CPU durumunu atomik olarak okur.
    /// Acquire semantiği: bu yüklemeden önceki tüm yazmaların görünür olması garantisi.
    pub fn get_state(&self) -> CpuState {
        match self.state.load(Ordering::Acquire) {
            0 => CpuState::Offline,
            1 => CpuState::ComingUp,
            2 => CpuState::Online,
            3 => CpuState::GoingDown,
            4 => CpuState::Dead,
            _ => CpuState::Offline,
        }
    }

    /// Hedef durumu ayarlar ve bellek bariyeri uygular.
    pub fn set_target_state(&self, target: CpuState) {
        self.target_state.store(target as u32, Ordering::Release);
        smp_wmb(); // SMP yazma bariyeri: diğer CPU'ların güncel değeri görmesini sağlar
    }

    /// Hedef durumu atomik olarak okur.
    pub fn get_target_state(&self) -> CpuState {
        match self.target_state.load(Ordering::Acquire) {
            0 => CpuState::Offline,
            1 => CpuState::ComingUp,
            2 => CpuState::Online,
            3 => CpuState::GoingDown,
            4 => CpuState::Dead,
            _ => CpuState::Offline,
        }
    }

    /// CPU'nun çevrimiçi olup olmadığını atomik olarak kontrol eder.
    pub fn is_online(&self) -> bool {
        self.online.load(Ordering::Acquire)
    }

    /// CPU'nun hotplug işlemi sırasında olup olmadığını kontrol eder.
    pub fn is_hotplugging(&self) -> bool {
        self.hotplugging.load(Ordering::Acquire)
    }

    /// Referans sayacını artırır (fetch_add + AcqRel — hem acquire hem release).
    /// AcqRel: bu işlem önceki okumalar tamamlandıktan sonra ve sonraki yazmalar
    /// bu değeri gördükten sonra gerçekleşir.
    pub fn get(&self) -> usize {
        self.refcount.fetch_add(1, Ordering::AcqRel)
    }

    /// Referans sayacını azaltır.
    pub fn put(&self) -> usize {
        self.refcount.fetch_sub(1, Ordering::AcqRel)
    }

    /// Mevcut referans sayısını döndürür.
    pub fn refcount(&self) -> usize {
        self.refcount.load(Ordering::Acquire)
    }
}

/// CPU hotplug yöneticisi.
/// Tüm CPU tanımlayıcılarını, geri çağrıları ve istatistikleri yönetir.
pub struct CpuHotplugManager {
    /// Desteklenen maksimum CPU sayısı
    max_cpus: u32,
    /// CPU tanımlayıcıları (RCU korumalı — okurken kilit gerekmez)
    cpu_descs: Vec<RcuPtr<CpuHotplugDesc>>,
    /// Hotplug geri çağrı listesi
    callbacks: Vec<HotplugCallback>,
    /// Hotplug kilidi — aynı anda tek işlem çalışmasını garantiler
    hotplug_gate: AtomicBool,
    /// Şu anda çevrimiçi olan CPU sayısı
    online_cpus: AtomicU32,
    /// Hotplug işlem istatistikleri
    stats: HotplugStats,
}

/// Hotplug işlem istatistikleri.
/// Başarılı/başarısız çevrimiçi ve çevrimdışı işlem sayılarını tutar.
#[derive(Debug)]
pub struct HotplugStats {
    pub successful_online: AtomicU32,
    pub successful_offline: AtomicU32,
    pub failed_online: AtomicU32,
    pub failed_offline: AtomicU32,
    pub total_operations: AtomicU32,
}

impl HotplugStats {
    pub const fn new() -> Self {
        Self {
            successful_online: AtomicU32::new(0),
            successful_offline: AtomicU32::new(0),
            failed_online: AtomicU32::new(0),
            failed_offline: AtomicU32::new(0),
            total_operations: AtomicU32::new(0),
        }
    }

    pub fn record_online_success(&self) {
        self.successful_online.fetch_add(1, Ordering::Relaxed);
        self.total_operations.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_online_failure(&self) {
        self.failed_online.fetch_add(1, Ordering::Relaxed);
        self.total_operations.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_offline_success(&self) {
        self.successful_offline.fetch_add(1, Ordering::Relaxed);
        self.total_operations.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_offline_failure(&self) {
        self.failed_offline.fetch_add(1, Ordering::Relaxed);
        self.total_operations.fetch_add(1, Ordering::Relaxed);
    }

    /// İstatistik özeti: (başarılı_online, başarılı_offline, başarısız_online, başarısız_offline, toplam)
    pub fn get_stats(&self) -> (u32, u32, u32, u32, u32) {
        (
            self.successful_online.load(Ordering::Relaxed),
            self.successful_offline.load(Ordering::Relaxed),
            self.failed_online.load(Ordering::Relaxed),
            self.failed_offline.load(Ordering::Relaxed),
            self.total_operations.load(Ordering::Relaxed),
        )
    }
}

impl CpuHotplugManager {
    /// Belirtilen maksimum CPU sayısı için yeni bir hotplug yöneticisi oluşturur.
    /// Her CPU için Offline durumunda bir tanımlayıcı tahsis eder.
    pub fn new(max_cpus: u32) -> Self {
        let mut cpu_descs = Vec::with_capacity(max_cpus as usize);

        // Her CPU için tanımlayıcı başlat (cpu_id = apic_id varsayımı)
        for cpu_id in 0..max_cpus {
            let desc = Box::new(CpuHotplugDesc::new(cpu_id, cpu_id));
            cpu_descs.push(RcuPtr::new(Box::into_raw(desc)));
        }

        Self {
            max_cpus,
            cpu_descs,
            callbacks: Vec::new(),
            hotplug_gate: AtomicBool::new(false),
            online_cpus: AtomicU32::new(0),
            stats: HotplugStats::new(),
        }
    }

    /// Hotplug geri çağrısı kaydeder.
    /// Tüm hotplug olaylarında bu geri çağrı çağrılır.
    pub fn register_callback(&mut self, callback: HotplugCallback) {
        self.callbacks.push(callback);
    }

    /// Belirtilen CPU'nun tanımlayıcısını döndürür.
    /// cpu_id geçersizse None döner.
    pub fn get_cpu_desc(&self, cpu_id: u32) -> Option<RcuPtr<CpuHotplugDesc>> {
        if cpu_id >= self.max_cpus {
            return None;
        }

        Some(self.cpu_descs[cpu_id as usize].clone())
    }

    /// CPU'yu çevrimiçe alır.
    ///
    /// Akış diyagramı:
    /// ```text
    /// CPU tanımlayıcısını al
    ///   ├── Zaten Online → AlreadyInState hatası
    ///   ├── Offline değil → InvalidStateTransition hatası
    ///   └── Offline → hotplugging = true
    ///         ├── PrepareOnline geri çağrıları → başarısız → PrepareFailed
    ///         ├── do_cpu_online() → başarısız → OperationFailed
    ///         └── Durum: Online, istatistik güncelle, Online geri çağrıları
    /// ```
    pub fn cpu_online(&self, cpu_id: u32) -> Result<(), HotplugError> {
        let _guard = self.acquire_hotplug_gate()?;

        // CPU tanımlayıcısını al
        let desc = match self.get_cpu_desc(cpu_id) {
            Some(desc) => desc,
            None => return Err(HotplugError::InvalidCpuId),
        };

        let desc_guard = desc.read();

        // Mevcut durumu kontrol et
        let current_state = desc_guard.get_state();
        if current_state == CpuState::Online {
            return Err(HotplugError::AlreadyInState);
        }

        if current_state != CpuState::Offline {
            return Err(HotplugError::InvalidStateTransition);
        }

        // Hotplug işlemi başlıyor olarak işaretle
        desc_guard.hotplugging.store(true, Ordering::Release);
        smp_wmb();

        // RCU kilidini bırak; geri çağrılarda kilit tutmak deadlock'a yol açabilir
        drop(desc_guard);

        // Hazırlık geri çağrılarını bildir
        if let Err(_) = self.notify_callbacks(cpu_id, CpuHotplugEvent::PrepareOnline) {
            // Durumu geri al
            let desc_guard = desc.read();
            desc_guard.hotplugging.store(false, Ordering::Release);
            return Err(HotplugError::PrepareFailed);
        }

        // Gerçek CPU başlatma işlemini yap
        if let Err(_) = self.do_cpu_online(cpu_id) {
            // Başarısız — durumu geri al, Offline bildir
            let desc_guard = desc.read();
            desc_guard.hotplugging.store(false, Ordering::Release);
            self.notify_callbacks(cpu_id, CpuHotplugEvent::Offline);
            return Err(HotplugError::OperationFailed);
        }

        // Durumu güncelle — tam bellek bariyeri ile diğer CPU'ların görmesi sağlanır
        let desc_guard = desc.read();
        desc_guard
            .state
            .store(CpuState::Online as u32, Ordering::Release);
        desc_guard.online.store(true, Ordering::Release);
        desc_guard.hotplugging.store(false, Ordering::Release);
        desc_guard.last_hotplug.store(
            crate::task::scheduler::get_ticks() as u64,
            Ordering::Relaxed,
        );
        desc_guard.hotplug_attempts.fetch_add(1, Ordering::Relaxed);
        smp_mb(); // Tam bellek bariyeri — tüm yazmaların görünür olması garantisi

        // Çevrimiçi CPU sayacını artır
        self.online_cpus.fetch_add(1, Ordering::AcqRel);

        // Online geri çağrılarını bildir
        self.notify_callbacks(cpu_id, CpuHotplugEvent::Online);

        // İstatistikleri güncelle
        self.stats.record_online_success();

        crate::serial_println!("Hotplug: CPU {} şimdi çevrimiçi", cpu_id);
        Ok(())
    }

    /// CPU'yu çevrimdışına alır.
    /// BSP (CPU 0) çevrimdışı yapılamaz — sistem bu CPU olmadan çalışamaz.
    pub fn cpu_offline(&self, cpu_id: u32) -> Result<(), HotplugError> {
        let _guard = self.acquire_hotplug_gate()?;

        // BSP'yi çevrimdışı yapma izni yok
        if cpu_id == 0 {
            return Err(HotplugError::InvalidStateTransition);
        }

        // CPU tanımlayıcısını al
        let desc = match self.get_cpu_desc(cpu_id) {
            Some(desc) => desc,
            None => return Err(HotplugError::InvalidCpuId),
        };

        let desc_guard = desc.read();

        // Mevcut durumu kontrol et
        let current_state = desc_guard.get_state();
        if current_state == CpuState::Offline {
            return Err(HotplugError::AlreadyInState);
        }

        if current_state != CpuState::Online {
            return Err(HotplugError::InvalidStateTransition);
        }

        // Referans sayısını kontrol et — başkaları bu CPU'yu kullanıyor mu?
        if desc_guard.refcount() > 0 {
            return Err(HotplugError::InvalidStateTransition);
        }

        // Hotplug işlemi başlıyor olarak işaretle
        desc_guard.hotplugging.store(true, Ordering::Release);
        smp_wmb();

        // RCU kilidini bırak
        drop(desc_guard);

        // Hazırlık geri çağrılarını bildir
        if let Err(_) = self.notify_callbacks(cpu_id, CpuHotplugEvent::PrepareOffline) {
            // Durumu geri al
            let desc_guard = desc.read();
            desc_guard.hotplugging.store(false, Ordering::Release);
            return Err(HotplugError::PrepareFailed);
        }

        // Gerçek CPU kapatma işlemini yap
        if let Err(_) = self.do_cpu_offline(cpu_id) {
            // Başarısız — CPU çevrimiçi kaldı, Online bildir
            let desc_guard = desc.read();
            desc_guard.hotplugging.store(false, Ordering::Release);
            self.notify_callbacks(cpu_id, CpuHotplugEvent::Online);
            return Err(HotplugError::OperationFailed);
        }

        // Durumu güncelle
        let desc_guard = desc.read();
        desc_guard
            .state
            .store(CpuState::Offline as u32, Ordering::Release);
        desc_guard.online.store(false, Ordering::Release);
        desc_guard.hotplugging.store(false, Ordering::Release);
        desc_guard.last_hotplug.store(
            crate::task::scheduler::get_ticks() as u64,
            Ordering::Relaxed,
        );
        desc_guard.hotplug_attempts.fetch_add(1, Ordering::Relaxed);
        smp_mb();

        // Çevrimiçi CPU sayacını azalt
        self.online_cpus.fetch_sub(1, Ordering::AcqRel);

        // Offline geri çağrılarını bildir
        self.notify_callbacks(cpu_id, CpuHotplugEvent::Offline);

        // İstatistikleri güncelle
        self.stats.record_offline_success();

        crate::serial_println!("Hotplug: CPU {} şimdi çevrimdışı", cpu_id);
        Ok(())
    }

    /// Platforma özgü CPU başlatma işlemini gerçekleştirir.
    /// x86'da: INIT-SIPI-SIPI dizisi gönderilir; CPU yanıt beklenir.
    fn do_cpu_online(&self, cpu_id: u32) -> Result<(), HotplugError> {
        // CPU'ya özgü veri yapılarını başlat
        crate::task::scheduler::update_cpu_count(cpu_id + 1);

        // BSP değilse başlat — BSP (CPU 0) zaten çalışıyor
        if cpu_id != 0 {
            // INIT-SIPI-SIPI dizisi gönder (x86 AP başlatma protokolü)
            crate::cpu::smp::start_cpu(cpu_id).map_err(|_| HotplugError::OperationFailed)?;
        }

        // CPU'nun yanıt vermesini bekle — maksimum 1000 tik (zaman aşımı)
        let timeout = 1000; // 1000 tik zaman aşımı
        let start = crate::task::scheduler::get_ticks();

        loop {
            let desc = match self.get_cpu_desc(cpu_id) {
                Some(desc) => desc,
                None => return Err(HotplugError::InvalidCpuId),
            };

            let desc_guard = desc.read();
            if desc_guard.is_online() {
                break;
            }

            let elapsed = crate::task::scheduler::get_ticks().saturating_sub(start);
            if elapsed > timeout {
                return Err(HotplugError::OperationFailed);
            }

            crate::task::scheduler::sleep(1);
        }

        Ok(())
    }

    /// Platforma özgü CPU kapatma işlemini gerçekleştirir.
    fn do_cpu_offline(&self, cpu_id: u32) -> Result<(), HotplugError> {
        // Görevleri diğer CPU'lara taşı
        self.migrate_tasks_away(cpu_id)?;

        // CPU kapatma sinyali gönder
        crate::cpu::smp::stop_cpu(cpu_id).map_err(|_| HotplugError::OperationFailed)?;

        // CPU'nun durmasını bekle — maksimum 1000 tik
        let timeout = 1000; // 1000 tik zaman aşımı
        let start = crate::task::scheduler::get_ticks();

        loop {
            let desc = match self.get_cpu_desc(cpu_id) {
                Some(desc) => desc,
                None => return Err(HotplugError::InvalidCpuId),
            };

            let desc_guard = desc.read();
            if !desc_guard.is_online() {
                break;
            }

            let elapsed = crate::task::scheduler::get_ticks().saturating_sub(start);
            if elapsed > timeout {
                return Err(HotplugError::OperationFailed);
            }

            crate::task::scheduler::sleep(1);
        }

        Ok(())
    }

    /// Çevrimdışı yapılacak CPU'daki tüm görevleri diğer CPU'lara taşır.
    /// Geçiş adımları:
    /// 1. Hedef CPU'daki tüm görevleri bul
    /// 2. Diğer çevrimiçi CPU'lara taşı
    /// 3. CPU affinity maskelerini güncelle
    fn migrate_tasks_away(&self, cpu_id: u32) -> Result<(), HotplugError> {
        crate::serial_println!("Hotplug: CPU {}'deki görevler taşınıyor", cpu_id);

        // Çevrimdışı olan CPU'nun stealer'ından görevleri çal ve
        // diğer çevrimiçi CPU'lara dağıt
        let cpu_count = crate::task::scheduler::get_cpu_count();
        let mut migrated = 0u32;

        // Stealer üzerinden görevleri çek
        loop {
            let task = crate::task::scheduler::steal_from_cpu(cpu_id);
            if task.is_none() {
                break;
            }
            let task = task.unwrap();

            // En az yüklü çevrimiçi CPU'yu bul (çevrimdışı olan hariç)
            let mut best_cpu = None;
            let mut min_load = u32::MAX;

            if let Some(state) = crate::cpu::smp::SMP_STATE.try_lock() {
                for cpu in state.per_cpu_data.iter() {
                    if cpu.online && cpu.cpu_id != cpu_id && cpu.load < min_load {
                        min_load = cpu.load;
                        best_cpu = Some(cpu.cpu_id);
                    }
                }
            }

            let target = best_cpu.unwrap_or(0);
            crate::task::scheduler::push_to_cpu(target, task);
            migrated += 1;
        }

        // Scheduler CPU sayısını güncelle
        let online = self.online_cpus.load(Ordering::Acquire);
        crate::task::scheduler::update_cpu_count(online);

        crate::serial_println!("Hotplug: {} görev CPU {}'den taşındı", migrated, cpu_id);
        Ok(())
    }

    /// Tüm kayıtlı hotplug geri çağrılarını sırayla bildirir.
    /// Herhangi bir geri çağrı hata döndürürse işlem durdurulur.
    fn notify_callbacks(&self, cpu_id: u32, event: CpuHotplugEvent) -> Result<(), HotplugError> {
        for callback in &self.callbacks {
            if let Err(_) = callback(cpu_id, event) {
                return Err(HotplugError::CallbackError);
            }
        }
        Ok(())
    }

    /// Şu anda çevrimiçi olan CPU sayısını döndürür.
    pub fn get_online_cpus(&self) -> u32 {
        self.online_cpus.load(Ordering::Acquire)
    }

    /// Hotplug istatistiklerini döndürür.
    pub fn get_stats(&self) -> (u32, u32, u32, u32, u32) {
        self.stats.get_stats()
    }

    /// Belirtilen CPU'nun çevrimdışı yapılıp yapılamayacağını kontrol eder.
    /// Koşullar: çevrimiçi olmalı, hotplug işleminde olmamalı, referans sayısı = 0.
    pub fn can_cpu_offline(&self, cpu_id: u32) -> bool {
        // BSP (CPU 0) asla çevrimdışı yapılamaz
        if cpu_id == 0 {
            return false;
        }

        let desc = match self.get_cpu_desc(cpu_id) {
            Some(desc) => desc,
            None => return false,
        };

        let desc_guard = desc.read();

        // CPU çevrimiçi olmalı
        if !desc_guard.is_online() {
            return false;
        }

        // CPU hotplug işleminde olmamalı
        if desc_guard.is_hotplugging() {
            return false;
        }

        // CPU'ya sıfır referans olmalı
        if desc_guard.refcount() > 0 {
            return false;
        }

        true
    }

    /// Belirtilen CPU'nun çevrimiçi yapılıp yapılamayacağını kontrol eder.
    pub fn can_cpu_online(&self, cpu_id: u32) -> bool {
        let desc = match self.get_cpu_desc(cpu_id) {
            Some(desc) => desc,
            None => return false,
        };

        let desc_guard = desc.read();

        // CPU çevrimdışı olmalı
        if desc_guard.is_online() {
            return false;
        }

        // CPU hotplug işleminde olmamalı
        if desc_guard.is_hotplugging() {
            return false;
        }

        // CPU ölü durumda olmamalı
        if desc_guard.get_state() == CpuState::Dead {
            return false;
        }

        true
    }

    /// Belirtilen CPU'nun durumunu döndürür.
    pub fn get_cpu_state(&self, cpu_id: u32) -> Option<CpuState> {
        let desc = self.get_cpu_desc(cpu_id)?;
        Some(desc.read().get_state())
    }

    /// Tüm CPU'ların (cpu_id, durum) çiftlerini döndürür.
    pub fn get_all_cpu_states(&self) -> Vec<(u32, CpuState)> {
        let mut states = Vec::new();

        for cpu_id in 0..self.max_cpus {
            if let Some(state) = self.get_cpu_state(cpu_id) {
                states.push((cpu_id, state));
            }
        }

        states
    }
}

/// Global hotplug yöneticisi örneği.
/// unsafe: çekirdek başlatma sırasında tek thread erişimi; sonrası mutex korumalı.
static mut HOTPLUG_MANAGER: Option<CpuHotplugManager> = None;
static HOTPLUG_INIT: AtomicBool = AtomicBool::new(false);

/// Hotplug alt sistemini başlatır.
/// Çift başlatmayı önlemek için atomik flag kontrol edilir.
pub fn init(max_cpus: u32) {
    if HOTPLUG_INIT.load(Ordering::Acquire) {
        return;
    }

    crate::serial_println!(
        "Hotplug: {} CPU için CPU hotplug desteği başlatılıyor",
        max_cpus
    );

    let mut manager = CpuHotplugManager::new(max_cpus);

    // Varsayılan geri çağrısı kaydet
    manager.register_callback(default_hotplug_callback);

    unsafe {
        HOTPLUG_MANAGER = Some(manager);
    }

    HOTPLUG_INIT.store(true, Ordering::Release);
    smp_mb(); // Tam bellek bariyeri: başlatma tamamlanmadan işlem yapılmasını önler

    crate::serial_println!("Hotplug: CPU hotplug desteği başlatıldı");
}

/// Global hotplug yöneticisine yalnızca okunur referans döndürür.
pub fn get_manager() -> Option<&'static CpuHotplugManager> {
    if !HOTPLUG_INIT.load(Ordering::Acquire) {
        return None;
    }

    unsafe { HOTPLUG_MANAGER.as_ref() }
}

/// Varsayılan hotplug geri çağrısı — olayları serial porta yazdırır.
fn default_hotplug_callback(cpu_id: u32, event: CpuHotplugEvent) -> Result<(), HotplugError> {
    match event {
        CpuHotplugEvent::PrepareOnline => {
            crate::serial_println!(
                "Hotplug: CPU {} çevrimiçe alınmak üzere hazırlanıyor",
                cpu_id
            );
        }
        CpuHotplugEvent::Online => {
            crate::serial_println!("Hotplug: CPU {} çevrimiçi", cpu_id);
        }
        CpuHotplugEvent::PrepareOffline => {
            crate::serial_println!(
                "Hotplug: CPU {} çevrimdışına alınmak üzere hazırlanıyor",
                cpu_id
            );
        }
        CpuHotplugEvent::Offline => {
            crate::serial_println!("Hotplug: CPU {} çevrimdışı", cpu_id);
        }
        CpuHotplugEvent::Dead => {
            crate::serial_println!("Hotplug: CPU {} öldü", cpu_id);
        }
    }

    Ok(())
}

/// Harici modüller için kolaylık fonksiyonları.
pub fn cpu_online(cpu_id: u32) -> Result<(), HotplugError> {
    let manager = get_manager().ok_or(HotplugError::InvalidCpuId)?;
    manager.cpu_online(cpu_id)
}

pub fn cpu_offline(cpu_id: u32) -> Result<(), HotplugError> {
    let manager = get_manager().ok_or(HotplugError::InvalidCpuId)?;
    manager.cpu_offline(cpu_id)
}

pub fn get_online_cpus() -> u32 {
    get_manager().map(|m| m.get_online_cpus()).unwrap_or(1)
}

pub fn can_cpu_offline(cpu_id: u32) -> bool {
    get_manager()
        .map(|m| m.can_cpu_offline(cpu_id))
        .unwrap_or(false)
}

pub fn can_cpu_online(cpu_id: u32) -> bool {
    get_manager()
        .map(|m| m.can_cpu_online(cpu_id))
        .unwrap_or(false)
}

impl CpuHotplugManager {
    fn acquire_hotplug_gate(&self) -> Result<HotplugGateGuard<'_>, HotplugError> {
        match self
            .hotplug_gate
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => Ok(HotplugGateGuard {
                gate: &self.hotplug_gate,
            }),
            Err(_) => Err(HotplugError::OperationFailed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_hotplug_states() {
        let desc = CpuHotplugDesc::new(0, 0);
        assert_eq!(desc.get_state(), CpuState::Offline);
        assert!(!desc.is_online());

        desc.set_target_state(CpuState::Online);
        assert_eq!(desc.get_target_state(), CpuState::Online);
    }

    #[test]
    fn test_hotplug_manager() {
        let manager = CpuHotplugManager::new(4);
        assert_eq!(manager.get_online_cpus(), 0);

        assert!(manager.can_cpu_online(0));
        assert!(!manager.can_cpu_offline(0)); // BSP çevrimdışı yapılamaz
    }
}

struct HotplugGateGuard<'a> {
    gate: &'a AtomicBool,
}

impl Drop for HotplugGateGuard<'_> {
    fn drop(&mut self) {
        self.gate.store(false, Ordering::Release);
    }
}
