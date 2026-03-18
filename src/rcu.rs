//! # echOS RCU (Read-Copy-Update - Oku-Kopyala-Güncelle) Modülü
//!
//! Tier 1 OS seviyesinde kilit-serbest (lock-free) veri yapıları.
//! Linux RCU ile aynı prensipler, Rust optimizasyonları ile iyileştirilmiş.
//!
//! ## RCU Nedir?
//! RCU, okuyucuların hiç kilit almadan veriyi okumasına, yazıcıların ise
//! eski veriyi güvenle serbest bırakmasına olanak tanıyan bir eşzamanlılık
//! mekanizmasıdır.
//!
//! ```ascii
//! Okuyucu 1: [okuma_kilidi_al] --[veriyi oku]-- [okuma_kilidini_bırak]
//! Okuyucu 2:      [okuma_kilidi_al] --[veriyi oku]-- [okuma_kilidini_bırak]
//! Yazıcı  :              [yeni_veri_yaz] [zariflik_dönemi_başlat]
//!                                                      |
//!                               [tüm okuyucular çıktıktan sonra eski veriyi serbest bırak]
//! ```

use crate::memory_barriers::{smp_acquire, smp_mb, smp_release, smp_rmb, smp_wmb};
use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicU64, AtomicUsize, Ordering};
use core::time::Duration;

const MAX_RCU_CPUS: usize = 8192;
const TREE_RCU_LEAF_SHIFT: usize = 6;
const TREE_RCU_LEAF_SIZE: usize = 1 << TREE_RCU_LEAF_SHIFT;
const TREE_RCU_LEAF_COUNT: usize = (MAX_RCU_CPUS + TREE_RCU_LEAF_SIZE - 1) / TREE_RCU_LEAF_SIZE;

/// Global RCU dönem (epoch) sayacı.
///
/// Her zariflik dönemi başladığında bu sayaç artırılır.
/// Okuyucular hangi dönemde olduklarını bu sayaca göre takip eder.
static RCU_EPOCH: AtomicU64 = AtomicU64::new(0);

/// Global RCU zariflik dönemi durum izleyicisi.
///
/// Mevcut ve tamamlanan zariflik dönemlerini, dönem başlangıç tick'ini tutar.
static RCU_GP_STATE: RcuGracePeriodState = RcuGracePeriodState::new();

/// CPU başına RCU okuyucu sayacı dizisi.
///
/// Her CPU'nun kaç aktif RCU okuyucusu olduğunu saklar.
/// `unsafe` çünkü global değişken; ancak her CPU yalnızca kendi indisine erişir.
static mut RCU_READER_COUNT: [AtomicUsize; MAX_RCU_CPUS] =
    [const { AtomicUsize::new(0) }; MAX_RCU_CPUS];
static TREE_RCU_DOMAIN: TreeRcuDomain = TreeRcuDomain::new();
static SRCU_DOMAIN: SrcuDomain = SrcuDomain::new();

/// RCU zariflik dönemi durumu.
///
/// Mevcut dönem, tamamlanan dönem ve dönem başlangıç tick'ini atomik olarak tutar.
struct RcuGracePeriodState {
    current_gp: AtomicU64,
    completed_gp: AtomicU64,
    gp_start_tick: AtomicU64,
}

impl RcuGracePeriodState {
    /// Sabit (const) başlatıcı; global static ataması için gerekli.
    const fn new() -> Self {
        Self {
            current_gp: AtomicU64::new(0),
            completed_gp: AtomicU64::new(0),
            gp_start_tick: AtomicU64::new(0),
        }
    }
}

/// RCU okuma tarafı kritik bölüm muhafızı.
///
/// Oluşturulduğunda okuyucu sayacını artırır, düşürüldüğünde azaltır.
/// Bu sayede yazıcılar, okuyucuların veriyi ne zaman bıraktığını anlayabilir.
pub struct RcuReadLock {
    cpu_id: u32,
    epoch: u64,
}

impl RcuReadLock {
    /// RCU okuma tarafı kritik bölümüne girer.
    ///
    /// Mevcut CPU'nun okuyucu sayacını artırır ve bellek bariyeri uygular.
    /// Bariyer, veriyi okuyan kodun gerçekten okuma bölgesi içinde olmasını sağlar.
    pub fn new() -> Self {
        let cpu_id = crate::cpu::smp::current_cpu_id();
        let epoch = RCU_EPOCH.load(Ordering::Acquire);

        // Bu CPU için okuyucu sayacını artır
        unsafe {
            RCU_READER_COUNT[cpu_id as usize].fetch_add(1, Ordering::Relaxed);
        }

        // Sıralamayı güvence altına almak için bellek bariyeri uygula
        smp_rmb();

        Self { cpu_id, epoch }
    }

    /// Mevcut RCU dönemini döner.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Okunan verinin hâlâ geçerli olup olmadığını kontrol eder.
    ///
    /// Dönem değişmediyse veri geçerlidir; değiştiyse yeni bir okuma gerekebilir.
    pub fn is_valid(&self) -> bool {
        let current_epoch = RCU_EPOCH.load(Ordering::Acquire);
        current_epoch == self.epoch
    }
}

impl Drop for RcuReadLock {
    /// RCU okuma tarafı kritik bölümünden çıkar.
    ///
    /// Okuyucu sayacını azaltır; bu sayım sıfıra düştüğünde
    /// zariflik dönemi tamamlanabilir.
    fn drop(&mut self) {
        // Bu CPU için okuyucu sayacını azalt
        unsafe {
            RCU_READER_COUNT[self.cpu_id as usize].fetch_sub(1, Ordering::Relaxed);
        }

        // Çıkışın sıralı görünmesi için bellek bariyeri uygula
        smp_rmb();
    }
}

/// RCU ile korunan atomik işaretçi (pointer).
///
/// Birden fazla okuyucunun aynı anda kilitsiz okumasına,
/// tek bir yazıcının güvenle güncellemesine olanak tanır.
pub struct RcuPtr<T> {
    ptr: AtomicPtr<T>,
}

impl<T> Clone for RcuPtr<T> {
    fn clone(&self) -> Self {
        let ptr = self.ptr.load(Ordering::Acquire);
        Self {
            ptr: AtomicPtr::new(ptr),
        }
    }
}

impl<T> RcuPtr<T> {
    /// Yeni bir RCU korumalı işaretçi oluşturur.
    pub fn new(ptr: *mut T) -> Self {
        Self {
            ptr: AtomicPtr::new(ptr),
        }
    }

    /// RCU koruması altında işaretçiyi okur.
    ///
    /// Okuma kilidi alır, ardından `Acquire` sıralamasıyla işaretçiyi yükler.
    /// Dönen muhafız (guard) düşürüldüğünde kilit otomatik serbest bırakılır.
    pub fn read(&self) -> RcuReadGuard<'_, T> {
        let _lock = RcuReadLock::new();
        let ptr = self.ptr.load(Ordering::Acquire);

        RcuReadGuard {
            ptr,
            _lock: _lock,
            _phantom: core::marker::PhantomData,
        }
    }

    /// İşaretçiyi RCU stilinde günceller.
    ///
    /// Eski işaretçiyi değiştirir, ardından zariflik dönemi başlatır.
    /// Döner değer eski (önceki) işaretçidir; zariflik dönemi tamamlandıktan
    /// sonra güvenle serbest bırakılabilir.
    pub fn update(&self, new_ptr: *mut T) -> *mut T {
        let old_ptr = self.ptr.swap(new_ptr, Ordering::Release);

        // Eski işaretçi için zariflik dönemi başlat
        smp_wmb();
        start_grace_period();

        old_ptr
    }

    /// RCU semantiği ile karşılaştır-ve-değiştir (CAS) işlemi yapar.
    ///
    /// Başarılı takas durumunda eski işaretçi için zariflik dönemi başlatılır.
    /// Döner değer, takas öncesi işaretçidir.
    pub fn compare_and_swap(&self, current: *mut T, new: *mut T) -> *mut T {
        let result = self
            .ptr
            .compare_exchange(current, new, Ordering::AcqRel, Ordering::Acquire)
            .unwrap_or_else(|x| x);

        if result == current && result != new {
            // Başarılı takas: eski işaretçi için zariflik dönemi başlat
            smp_wmb();
            start_grace_period();
        }

        result
    }
}

/// RCU okuma muhafızı.
///
/// `RcuPtr::read()` tarafından döndürülür. Düşürüldüğünde `RcuReadLock`'ı
/// otomatik serbest bırakarak okuma kritik bölümünden çıkar.
pub struct RcuReadGuard<'a, T> {
    ptr: *mut T,
    _lock: RcuReadLock,
    _phantom: core::marker::PhantomData<&'a T>,
}

impl<'a, T> RcuReadGuard<'a, T> {
    /// Veriye salt okunur referans döner.
    pub fn as_ref(&self) -> &'a T {
        unsafe { &*self.ptr }
    }

    /// Veriye değiştirilebilir referans döner (güvensiz).
    ///
    /// # Güvenlik
    /// Yazıcı çalışmadığı sürece aynı anda yalnızca bir `as_mut` çağrısı yapılmalıdır.
    pub fn as_mut(&self) -> &'a mut T {
        unsafe { &mut *self.ptr }
    }

    /// Ham işaretçiyi (raw pointer) döner.
    pub fn as_ptr(&self) -> *mut T {
        self.ptr
    }
}

impl<'a, T> core::ops::Deref for RcuReadGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

/// Belirtilen CPU için RCU durağan durum (quiescent state) bildirir.
///
/// Her bağlam değişimi (context switch) bir durağan durumdur.
/// Scheduler bu fonksiyonu çağırarak zariflik dönemlerinin tamamlanmasını hızlandırır.
/// CPU'nun okuyucu sayacı sıfırlanır: bu CPU tüm RCU okumalarını tamamlamıştır.
pub fn note_quiescent_state(cpu_id: u32) {
    unsafe {
        // Bu CPU'daki aktif okuyucu sayacını sıfırla
        // Context switch = tüm okumalar tamamlandı
        RCU_READER_COUNT[cpu_id as usize].store(0, Ordering::Release);
    }
    TREE_RCU_DOMAIN.note_quiescent_state(cpu_id);
    smp_mb();
}

/// Yeni bir RCU zariflik dönemi başlatır.
///
/// Dönem sayacını artırır ve tam bellek bariyeri uygular.
/// Zariflik dönemi, tüm mevcut okuyucuların bölgeden çıkmasını bekler.
pub fn start_grace_period() {
    let new_gp = RCU_EPOCH.fetch_add(1, Ordering::AcqRel) + 1;
    RCU_GP_STATE.current_gp.store(new_gp, Ordering::Release);
    RCU_GP_STATE.gp_start_tick.store(
        crate::task::scheduler::get_ticks() as u64,
        Ordering::Relaxed,
    );

    // Tüm CPU'ların yeni dönemi görmesi için tam bariyer
    smp_mb();
}

/// Zariflik döneminin tamamlanıp tamamlanmadığını kontrol eder.
///
/// Tüm CPU'lardaki okuyucu sayacı sıfırsa dönem tamamdır;
/// aksi hâlde en az bir okuyucu hâlâ kritik bölümdedir.
///
/// ```ascii
/// grace_period_completed() çağrısı
///         |
///   Mevcut dönem <= tamamlanan dönem?
///   Evet → true (zaten tamamlandı)
///   Hayır → Her CPU'nun okuyucu sayacını kontrol et
///              |
///       Herhangi biri > 0?
///       Evet → false (hâlâ okuyucu var)
///       Hayır → tamamlandı olarak işaretle → true
/// ```
pub fn grace_period_completed() -> bool {
    let current_gp = RCU_GP_STATE.current_gp.load(Ordering::Acquire);
    let completed_gp = RCU_GP_STATE.completed_gp.load(Ordering::Acquire);

    if current_gp <= completed_gp {
        return true;
    }

    // Tüm CPU'ların okuma tarafı kritik bölümünden çıkıp çıkmadığını kontrol et
    let cpu_count = crate::cpu::smp::get_cpu_count();

    for cpu_id in 0..cpu_count {
        unsafe {
            if RCU_READER_COUNT[cpu_id as usize].load(Ordering::Relaxed) > 0 {
                return false; // Hâlâ aktif okuyucu var
            }
        }
    }

    // Zariflik dönemi tamamlandı: sonucu kaydet ve bariyer uygula
    RCU_GP_STATE
        .completed_gp
        .store(current_gp, Ordering::Release);
    smp_mb();

    true
}

/// Zariflik döneminin tamamlanmasını bekler (senkron RCU).
///
/// Yeni dönem başlatır, ardından tamamlanana veya zaman aşımına uğrayana dek döngü kurar.
/// Zaman aşımı: 1000 tick.
pub fn synchronize_rcu() {
    start_grace_period();

    // Zaman aşımlı bekleme döngüsü
    let start_tick = crate::task::scheduler::get_ticks();
    let timeout = 1000; // 1000 tick zaman aşımı

    while !grace_period_completed() {
        let elapsed = crate::task::scheduler::get_ticks().saturating_sub(start_tick);
        if elapsed > timeout {
            crate::serial_println!("RCU: Grace period timeout!");
            break;
        }

        // CPU'yu bırak: diğer görevler/okuyucular çalışsın
        crate::task::scheduler::sleep(1);
    }
}

/// RCU ile korunan bağlantılı liste düğümü.
///
/// Her düğüm veri alanı ve sonraki düğüme atomik işaretçi içerir.
pub struct RcuListNode<T> {
    data: T,
    next: AtomicPtr<RcuListNode<T>>,
}

impl<T> RcuListNode<T> {
    /// Yeni bir liste düğümü oluşturur; `next` null olarak başlatılır.
    pub fn new(data: T) -> Self {
        Self {
            data,
            next: AtomicPtr::new(ptr::null_mut()),
        }
    }

    /// Düğümdeki veriye referans döner.
    pub fn data(&self) -> &T {
        &self.data
    }

    /// Sonraki düğümün ham işaretçisini `Acquire` sıralamasıyla okur.
    pub fn next(&self) -> *mut RcuListNode<T> {
        self.next.load(Ordering::Acquire)
    }

    /// Sonraki düğümü `Release` sıralamasıyla günceller.
    pub fn set_next(&self, next: *mut RcuListNode<T>) {
        self.next.store(next, Ordering::Release);
    }
}

/// RCU ile korunan bağlantılı liste.
///
/// Okuma işlemleri kilit almadan yapılır; yazma işlemleri (ekleme/silme)
/// compare-exchange ile atomik olarak gerçekleştirilir.
pub struct RcuList<T> {
    head: AtomicPtr<RcuListNode<T>>,
}

impl<T> RcuList<T> {
    /// Boş bir RCU listesi oluşturur.
    pub fn new() -> Self {
        Self {
            head: AtomicPtr::new(ptr::null_mut()),
        }
    }

    /// RCU koruması altında listeyi okur.
    ///
    /// Okuma kilidi alır ve liste başını yükler. Dönen muhafız
    /// düşürüldüğünde kilit otomatik serbest bırakılır.
    pub fn read(&self) -> RcuListReadGuard<'_, T> {
        let _lock = RcuReadLock::new();
        let head = self.head.load(Ordering::Acquire);

        RcuListReadGuard {
            head,
            _lock,
            _phantom: core::marker::PhantomData,
        }
    }

    /// Listeye başa yeni bir düğüm ekler.
    ///
    /// Compare-exchange döngüsü ile atomik olarak başa ekleme yapar.
    /// Çakışma durumunda otomatik olarak yeniden dener.
    pub fn insert_head(&self, new_node: *mut RcuListNode<T>) {
        loop {
            let current_head = self.head.load(Ordering::Acquire);
            unsafe {
                (*new_node).set_next(current_head);
            }

            match self.head.compare_exchange(
                current_head,
                new_node,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    // Ekleme başarılı; diğer CPU'ların görmesi için bariyer uygula
                    smp_wmb();
                    break;
                }
                Err(_) => {
                    // Çakışma: baştan tekrar dene
                    continue;
                }
            }
        }
    }

    /// Listeden başdaki düğümü kaldırır.
    ///
    /// Compare-exchange ile atomik çıkarma yapar. Başarılıysa zariflik dönemi başlatılır.
    /// Çakışma durumunda `None` döner; çağıran tekrar deneyebilir.
    pub fn remove_head(&self) -> Option<*mut RcuListNode<T>> {
        let current_head = self.head.load(Ordering::Acquire);
        if current_head.is_null() {
            return None;
        }

        let new_head = unsafe { (*current_head).next() };

        match self.head.compare_exchange(
            current_head,
            new_head,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                // Başarılı çıkarma: bariyer + zariflik dönemi
                smp_wmb();
                start_grace_period();
                Some(current_head)
            }
            Err(_) => None, // Tekrar dene veya None döndür
        }
    }
}

/// RCU liste okuma muhafızı.
///
/// `RcuList::read()` tarafından döndürülür. Tutulduğu sürece okuma kilidi aktiftir.
pub struct RcuListReadGuard<'a, T> {
    head: *mut RcuListNode<T>,
    _lock: RcuReadLock,
    _phantom: core::marker::PhantomData<&'a ()>,
}

impl<'a, T> RcuListReadGuard<'a, T> {
    /// Liste elemanları üzerinde yineleyici (iterator) döner.
    pub fn iter(&self) -> RcuListIterator<'a, T> {
        RcuListIterator {
            current: self.head,
            _phantom: core::marker::PhantomData,
        }
    }
}

/// RCU liste yineleyicisi.
///
/// Güvenli okuma bölgesi içinde liste üzerinde ilerler.
/// Her `next()` çağrısı bir sonraki düğümü `Acquire` sıralamasıyla yükler.
pub struct RcuListIterator<'a, T> {
    current: *mut RcuListNode<T>,
    _phantom: core::marker::PhantomData<&'a T>,
}

impl<'a, T> Iterator for RcuListIterator<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current.is_null() {
            return None;
        }

        let node = unsafe { &*self.current };
        let data = &node.data;
        self.current = node.next();

        Some(data)
    }
}

/// Hata ayıklama için RCU istatistikleri.
///
/// Mevcut dönem, tamamlanan zariflik dönemleri, aktif okuyucu sayısı
/// ve dönem başlangıç tick'ini içerir.
#[derive(Debug, Clone, Copy)]
pub struct RcuStats {
    pub current_epoch: u64,
    pub completed_grace_periods: u64,
    pub active_readers: usize,
    pub grace_period_start_tick: u64,
}

impl RcuStats {
    /// Anlık RCU istatistiklerini toplar.
    ///
    /// Tüm CPU'lardaki aktif okuyucu sayısını özetler.
    pub fn current() -> Self {
        let cpu_count = crate::cpu::smp::get_cpu_count();
        let mut active_readers = 0;

        // Tüm CPU'lardaki okuyucu sayılarını topla
        for cpu_id in 0..cpu_count {
            unsafe {
                active_readers += RCU_READER_COUNT[cpu_id as usize].load(Ordering::Relaxed);
            }
        }

        Self {
            current_epoch: RCU_EPOCH.load(Ordering::Relaxed),
            completed_grace_periods: RCU_GP_STATE.completed_gp.load(Ordering::Relaxed),
            active_readers,
            grace_period_start_tick: RCU_GP_STATE.gp_start_tick.load(Ordering::Relaxed),
        }
    }
}

/// Tree RCU istatistikleri.
///
/// Okuyucu taramasını CPU başına değil, 64 CPU'luk yaprak kümeleri
/// üzerinden yapar. `active_leaves == 0` olduğunda grace period tamamdır.
#[derive(Debug, Clone, Copy)]
pub struct TreeRcuStats {
    pub current_epoch: u64,
    pub completed_grace_periods: u64,
    pub active_cpus: usize,
    pub active_leaves: usize,
    pub grace_period_start_tick: u64,
}

/// Sleepable RCU (SRCU) istatistikleri.
///
/// Aktif slot yeni okuyucuları, draining slot ise kapanması beklenen
/// okuyucuları temsil eder. Epoch flip sonrası yalnızca draining slot sıfıra
/// indiğinde synchronize tamamlanır.
#[derive(Debug, Clone, Copy)]
pub struct SrcuStats {
    pub current_epoch: u64,
    pub completed_epoch: u64,
    pub current_slot: usize,
    pub active_slot_readers: usize,
    pub draining_slot_readers: usize,
}

/// Capraz cekirdek atomikleri tek cache line'a izole eder.
///
/// 8192 cekirdekte bitisik sayaclar ayni satira duserse her read-side giris/cikisi
/// gereksiz coherence invalidation uretir. Bu sarmalayici sicak sayaclari 64 byte'a ayirir.
#[repr(align(64))]
struct CacheAlignedAtomicUsize {
    value: AtomicUsize,
}

impl CacheAlignedAtomicUsize {
    const fn new(value: usize) -> Self {
        Self {
            value: AtomicUsize::new(value),
        }
    }

    #[inline(always)]
    fn load(&self, ordering: Ordering) -> usize {
        self.value.load(ordering)
    }

    #[inline(always)]
    fn store(&self, value: usize, ordering: Ordering) {
        self.value.store(value, ordering);
    }

    #[inline(always)]
    fn fetch_add(&self, value: usize, ordering: Ordering) -> usize {
        self.value.fetch_add(value, ordering)
    }

    #[inline(always)]
    fn fetch_sub(&self, value: usize, ordering: Ordering) -> usize {
        self.value.fetch_sub(value, ordering)
    }

    #[inline(always)]
    fn fetch_xor(&self, value: usize, ordering: Ordering) -> usize {
        self.value.fetch_xor(value, ordering)
    }

    #[inline(always)]
    fn swap(&self, value: usize, ordering: Ordering) -> usize {
        self.value.swap(value, ordering)
    }
}

#[repr(align(64))]
struct CacheAlignedAtomicU64 {
    value: AtomicU64,
}

impl CacheAlignedAtomicU64 {
    const fn new(value: u64) -> Self {
        Self {
            value: AtomicU64::new(value),
        }
    }

    #[inline(always)]
    fn load(&self, ordering: Ordering) -> u64 {
        self.value.load(ordering)
    }

    #[inline(always)]
    fn store(&self, value: u64, ordering: Ordering) {
        self.value.store(value, ordering);
    }

    #[inline(always)]
    fn fetch_add(&self, value: u64, ordering: Ordering) -> u64 {
        self.value.fetch_add(value, ordering)
    }

    #[inline(always)]
    fn fetch_min(&self, value: u64, ordering: Ordering) -> u64 {
        self.value.fetch_min(value, ordering)
    }
}
/// 64 CPU'luk yaprak kümeleri üzerinden toplama yapan Tree RCU domain'i.
///
/// Matematik:
/// - Her CPU için `cpu_readers[cpu] > 0` ise ilgili yaprak aktiftir.
/// - `leaf_active[leaf] = |{ cpu in leaf | cpu_readers[cpu] > 0 }|`
/// - `active_leaves = |{ leaf | leaf_active[leaf] > 0 }|`
/// - Grace period tamamlanma koşulu: `active_leaves == 0`
pub struct TreeRcuDomain {
    epoch: CacheAlignedAtomicU64,
    completed_gp: CacheAlignedAtomicU64,
    gp_start_tick: CacheAlignedAtomicU64,
    active_leaves: CacheAlignedAtomicUsize,
    cpu_readers: [CacheAlignedAtomicUsize; MAX_RCU_CPUS],
    leaf_active: [CacheAlignedAtomicUsize; TREE_RCU_LEAF_COUNT],
}

impl TreeRcuDomain {
    pub const fn new() -> Self {
        Self {
            epoch: CacheAlignedAtomicU64::new(0),
            completed_gp: CacheAlignedAtomicU64::new(0),
            gp_start_tick: CacheAlignedAtomicU64::new(0),
            active_leaves: CacheAlignedAtomicUsize::new(0),
            cpu_readers: [const { CacheAlignedAtomicUsize::new(0) }; MAX_RCU_CPUS],
            leaf_active: [const { CacheAlignedAtomicUsize::new(0) }; TREE_RCU_LEAF_COUNT],
        }
    }
    #[inline]
    const fn leaf_index(cpu_id: usize) -> usize {
        cpu_id >> TREE_RCU_LEAF_SHIFT
    }

    fn enter_on_cpu(&self, cpu_id: u32) {
        let cpu_id = cpu_id as usize;
        debug_assert!(cpu_id < MAX_RCU_CPUS);

        let prev = self.cpu_readers[cpu_id].fetch_add(1, Ordering::AcqRel);
        if prev == 0 {
            let leaf = Self::leaf_index(cpu_id);
            let leaf_prev = self.leaf_active[leaf].fetch_add(1, Ordering::AcqRel);
            if leaf_prev == 0 {
                self.active_leaves.fetch_add(1, Ordering::AcqRel);
            }
        }

        smp_rmb();
    }

    fn exit_on_cpu(&self, cpu_id: u32) {
        let cpu_id = cpu_id as usize;
        debug_assert!(cpu_id < MAX_RCU_CPUS);

        let prev = self.cpu_readers[cpu_id].fetch_sub(1, Ordering::AcqRel);
        if prev == 1 {
            let leaf = Self::leaf_index(cpu_id);
            let leaf_prev = self.leaf_active[leaf].fetch_sub(1, Ordering::AcqRel);
            if leaf_prev == 1 {
                self.active_leaves.fetch_sub(1, Ordering::AcqRel);
            }
        }

        smp_rmb();
    }

    pub fn read_lock(&'static self) -> TreeRcuReadGuard<'static> {
        self.read_lock_on_cpu(crate::cpu::smp::current_cpu_id())
    }

    pub fn read_lock_on_cpu(&self, cpu_id: u32) -> TreeRcuReadGuard<'_> {
        self.enter_on_cpu(cpu_id);
        TreeRcuReadGuard {
            domain: self,
            cpu_id,
        }
    }

    pub fn note_quiescent_state(&self, cpu_id: u32) {
        let cpu_id = cpu_id as usize;
        debug_assert!(cpu_id < MAX_RCU_CPUS);

        let prev = self.cpu_readers[cpu_id].swap(0, Ordering::AcqRel);
        if prev > 0 {
            let leaf = Self::leaf_index(cpu_id);
            let leaf_prev = self.leaf_active[leaf].fetch_sub(1, Ordering::AcqRel);
            if leaf_prev == 1 {
                self.active_leaves.fetch_sub(1, Ordering::AcqRel);
            }
        }

        smp_mb();
    }

    pub fn start_grace_period(&self) {
        let epoch = self.epoch.fetch_add(1, Ordering::AcqRel) + 1;
        self.gp_start_tick.store(
            crate::task::scheduler::get_ticks() as u64,
            Ordering::Relaxed,
        );
        self.completed_gp
            .fetch_min(epoch.saturating_sub(1), Ordering::Relaxed);
        smp_mb();
    }

    pub fn grace_period_completed(&self) -> bool {
        let current = self.epoch.load(Ordering::Acquire);
        let completed = self.completed_gp.load(Ordering::Acquire);

        if current <= completed {
            return true;
        }

        if self.active_leaves.load(Ordering::Acquire) == 0 {
            self.completed_gp.store(current, Ordering::Release);
            smp_mb();
            return true;
        }

        false
    }

    pub fn synchronize(&self) {
        self.start_grace_period();

        let start_tick = crate::task::scheduler::get_ticks();
        let timeout = 1000;

        while !self.grace_period_completed() {
            if crate::task::scheduler::get_ticks().saturating_sub(start_tick) > timeout {
                crate::serial_println!("TreeRCU: Grace period timeout!");
                break;
            }
            crate::task::scheduler::sleep(1);
        }
    }

    pub fn stats(&self) -> TreeRcuStats {
        let mut active_cpus = 0usize;
        let mut active_leaves = 0usize;

        for cpu_id in 0..MAX_RCU_CPUS {
            if self.cpu_readers[cpu_id].load(Ordering::Relaxed) > 0 {
                active_cpus += 1;
            }
        }
        for leaf in 0..TREE_RCU_LEAF_COUNT {
            if self.leaf_active[leaf].load(Ordering::Relaxed) > 0 {
                active_leaves += 1;
            }
        }

        TreeRcuStats {
            current_epoch: self.epoch.load(Ordering::Relaxed),
            completed_grace_periods: self.completed_gp.load(Ordering::Relaxed),
            active_cpus,
            active_leaves: active_leaves.min(self.active_leaves.load(Ordering::Relaxed)),
            grace_period_start_tick: self.gp_start_tick.load(Ordering::Relaxed),
        }
    }

    pub fn reset(&self, _cpu_count: u32) {
        for cpu_id in 0..MAX_RCU_CPUS {
            self.cpu_readers[cpu_id].store(0, Ordering::Relaxed);
        }
        for leaf in 0..TREE_RCU_LEAF_COUNT {
            self.leaf_active[leaf].store(0, Ordering::Relaxed);
        }

        self.active_leaves.store(0, Ordering::Relaxed);
        self.epoch.store(0, Ordering::Relaxed);
        self.completed_gp.store(0, Ordering::Relaxed);
        self.gp_start_tick.store(0, Ordering::Relaxed);
    }
}

pub struct TreeRcuReadGuard<'a> {
    domain: &'a TreeRcuDomain,
    cpu_id: u32,
}

impl Drop for TreeRcuReadGuard<'_> {
    fn drop(&mut self) {
        self.domain.exit_on_cpu(self.cpu_id);
    }
}

/// Sleepable RCU (SRCU) domain'i.
///
/// İki slotlu klasik SRCU dizaynı:
/// - Okuyucular `current_slot` üzerinde sayılır.
/// - `synchronize()` çağrısı slot'u çevirir.
/// - Eski slot (`draining`) sıfıra düşünce grace period tamamlanır.
pub struct SrcuDomain {
    epoch: CacheAlignedAtomicU64,
    completed_epoch: CacheAlignedAtomicU64,
    current_slot: CacheAlignedAtomicUsize,
    gp_start_tick: CacheAlignedAtomicU64,
    slot_active_cpus: [CacheAlignedAtomicUsize; 2],
    reader_counts: [[CacheAlignedAtomicUsize; MAX_RCU_CPUS]; 2],
}

impl SrcuDomain {
    pub const fn new() -> Self {
        Self {
            epoch: CacheAlignedAtomicU64::new(0),
            completed_epoch: CacheAlignedAtomicU64::new(0),
            current_slot: CacheAlignedAtomicUsize::new(0),
            gp_start_tick: CacheAlignedAtomicU64::new(0),
            slot_active_cpus: [const { CacheAlignedAtomicUsize::new(0) }; 2],
            reader_counts: [
                [const { CacheAlignedAtomicUsize::new(0) }; MAX_RCU_CPUS],
                [const { CacheAlignedAtomicUsize::new(0) }; MAX_RCU_CPUS],
            ],
        }
    }
    fn enter_on_cpu(&self, cpu_id: u32, slot: usize) {
        let cpu_id = cpu_id as usize;
        debug_assert!(cpu_id < MAX_RCU_CPUS);

        let prev = self.reader_counts[slot][cpu_id].fetch_add(1, Ordering::AcqRel);
        if prev == 0 {
            self.slot_active_cpus[slot].fetch_add(1, Ordering::AcqRel);
        }
        smp_acquire();
    }

    fn exit_on_cpu(&self, cpu_id: u32, slot: usize) {
        let cpu_id = cpu_id as usize;
        debug_assert!(cpu_id < MAX_RCU_CPUS);

        let prev = self.reader_counts[slot][cpu_id].fetch_sub(1, Ordering::AcqRel);
        if prev == 1 {
            self.slot_active_cpus[slot].fetch_sub(1, Ordering::AcqRel);
        }
        smp_release();
    }

    pub fn read_lock(&'static self) -> SrcuReadGuard<'static> {
        self.read_lock_on_cpu(crate::cpu::smp::current_cpu_id())
    }

    pub fn read_lock_on_cpu(&self, cpu_id: u32) -> SrcuReadGuard<'_> {
        let slot = self.current_slot.load(Ordering::Acquire) & 1;
        self.enter_on_cpu(cpu_id, slot);
        SrcuReadGuard {
            domain: self,
            cpu_id,
            slot,
        }
    }

    pub fn synchronize(&self) {
        let old_slot = self.current_slot.fetch_xor(1, Ordering::AcqRel) & 1;
        let epoch = self.epoch.fetch_add(1, Ordering::AcqRel) + 1;
        self.gp_start_tick.store(
            crate::task::scheduler::get_ticks() as u64,
            Ordering::Relaxed,
        );
        smp_mb();

        let start_tick = crate::task::scheduler::get_ticks();
        let timeout = 1000;

        while self.slot_active_cpus[old_slot].load(Ordering::Acquire) > 0 {
            if crate::task::scheduler::get_ticks().saturating_sub(start_tick) > timeout {
                crate::serial_println!("SRCU: Grace period timeout!");
                break;
            }
            crate::task::scheduler::sleep(1);
        }

        self.completed_epoch.store(epoch, Ordering::Release);
        smp_mb();
    }

    pub fn stats(&self) -> SrcuStats {
        let current_slot = self.current_slot.load(Ordering::Relaxed) & 1;
        let draining_slot = current_slot ^ 1;
        SrcuStats {
            current_epoch: self.epoch.load(Ordering::Relaxed),
            completed_epoch: self.completed_epoch.load(Ordering::Relaxed),
            current_slot,
            active_slot_readers: self.slot_active_cpus[current_slot].load(Ordering::Relaxed),
            draining_slot_readers: self.slot_active_cpus[draining_slot].load(Ordering::Relaxed),
        }
    }

    pub fn reset(&self, _cpu_count: u32) {
        for slot in 0..2 {
            for cpu_id in 0..MAX_RCU_CPUS {
                self.reader_counts[slot][cpu_id].store(0, Ordering::Relaxed);
            }
            self.slot_active_cpus[slot].store(0, Ordering::Relaxed);
        }
        self.current_slot.store(0, Ordering::Relaxed);
        self.epoch.store(0, Ordering::Relaxed);
        self.completed_epoch.store(0, Ordering::Relaxed);
        self.gp_start_tick.store(0, Ordering::Relaxed);
    }
}

pub struct SrcuReadGuard<'a> {
    domain: &'a SrcuDomain,
    cpu_id: u32,
    slot: usize,
}

impl Drop for SrcuReadGuard<'_> {
    fn drop(&mut self) {
        self.domain.exit_on_cpu(self.cpu_id, self.slot);
    }
}

pub fn tree_rcu() -> &'static TreeRcuDomain {
    &TREE_RCU_DOMAIN
}

pub fn srcu_default() -> &'static SrcuDomain {
    &SRCU_DOMAIN
}

/// RCU alt sistemini başlatır.
///
/// Tüm CPU'ların okuyucu sayaçlarını sıfırlar. Sistem açılışında çağrılmalıdır.
pub fn init() {
    crate::serial_println!("RCU: Initializing Read-Copy-Update subsystem");

    // Okuyucu sayaçlarını sıfırla
    let cpu_count = crate::cpu::smp::get_cpu_count();
    for cpu_id in 0..cpu_count {
        unsafe {
            RCU_READER_COUNT[cpu_id as usize].store(0, Ordering::Relaxed);
        }
    }

    TREE_RCU_DOMAIN.reset(cpu_count);
    SRCU_DOMAIN.reset(cpu_count);

    crate::serial_println!("RCU: Initialized for {} CPUs", cpu_count);
}

/// RCU softirq callback'lerini işler.
///
/// Tamamlanan zariflik dönemlerinin ertelenmiş serbest bırakma callback'lerini çalıştırır.
/// Timer softirq'dan periyodik olarak çağrılır.
pub fn process_callbacks() {
    // Mevcut zariflik dönemi tamamlandıysa bekleyen callback'leri işle
    if grace_period_completed() {
        // İleride callback listesi (call_rcu) eklenecektir
        // Şu an yalnızca dönem tamamlanma durumunu güncelleriz
        let current = RCU_GP_STATE.current_gp.load(Ordering::Acquire);
        RCU_GP_STATE.completed_gp.store(current, Ordering::Release);
        smp_mb();
    }
}

/// RCU alt sistemini temizler.
///
/// Tüm zariflik dönemlerinin tamamlanmasını bekler, ardından çıkış mesajı basar.
pub fn cleanup() {
    // Tüm açık zariflik dönemlerinin tamamlanmasını bekle
    synchronize_rcu();
    TREE_RCU_DOMAIN.synchronize();
    SRCU_DOMAIN.synchronize();

    crate::serial_println!("RCU: Cleanup completed");
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;

    #[test]
    fn test_rcu_read_lock() {
        let _lock = RcuReadLock::new();
        assert!(_lock.is_valid());
    }

    #[test]
    fn test_rcu_ptr() {
        let data = Box::new(42);
        let ptr = RcuPtr::new(Box::into_raw(data));

        {
            let guard = ptr.read();
            assert_eq!(*guard, 42);
        }
    }
}
