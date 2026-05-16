//! # echOS Güvenlik Alt Sistemi
//!
//! Bu modül, işletim sistemi çekirdeğinin güvenlik katmanını oluşturur.
//! Birden fazla katmanlı savunma (defense-in-depth) mimarisi uygulanır:
//!
//! ```
//! +----------------------------------------------------+
//! |              Güvenlik Katmanları                   |
//! +----------------------------------------------------+
//! |  [NX/DEP]   Veri sayfaları çalıştırılamaz          |
//! |  [SMEP]     Kernel, user-space kodu çalıştıramaz   |
//! |  [SMAP]     Kernel, user-space'e doğrudan erişemez |
//! |  [W^X]      Sayfa aynı anda yazılabilir+çalışamaz  |
//! |  [ASLR]     Bellek düzeni rastgele ofsetlenir       |
//! |  [CANARY]   Stack taşmasını algılar ve durdurur     |
//! |  [MAC]      Politika tabanlı zorunlu erişim kontrolü|
//! |  [CAPABILITY] Kaynak bazlı yetki sistemi            |
//! |  [TPM 2.0]  Donanım güvenlik modülü entegrasyonu   |
//! +----------------------------------------------------+
//! ```
//!
//! Tüm bu özellikler boot sırasında `security::init()` ile etkinleştirilir.

pub mod anti_cheat;
pub mod capability;
pub mod cfi;
pub mod kpti;
pub mod landlock;
pub mod mac;
pub mod mpk;
pub mod seccomp;
pub mod simics_gate;
pub mod spectre;
pub mod tpm;
pub mod users;

/// KASLR — Kernel Address Space Layout Randomization + Manifest İmzalama
pub mod kaslr;

/// Paket yönetim sistemi - .bhd formatında uygulama paketleri
pub mod package;

/// Seed package source abstraction - boot queue + mounted seed stores
pub mod seed_store;

/// Paket izin dialog sistemi - kullanıcı onay mekanizması
pub mod permission_dialog;

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;
use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};
use x86_64::registers::model_specific::Msr;

#[cfg(debug_assertions)]
macro_rules! sec_serial_println {
    ($($arg:tt)*) => {
        crate::serial_println!($($arg)*);
    };
}

#[cfg(not(debug_assertions))]
macro_rules! sec_serial_println {
    ($($arg:tt)*) => {{
        let _ = core::format_args!($($arg)*);
    }};
}

// ============================================================================
// SMEP/SMAP - Supervisor Mode Execution/Access Prevention
//
// Bu iki mekanizma, çekirdek (Ring 0) kodun kullanıcı alanına (Ring 3)
// erişimini kısıtlayarak privilege escalation saldırılarını engeller.
//
//  CPU Koruma Halkalar Diyagramı:
//  +---------------------------+
//  |  Ring 0 (Kernel)          |  <- Tam yetki, donanım erişimi
//  |  +---------------------+  |
//  |  |  Ring 3 (Kullanıcı) |  |  <- Kısıtlı yetki
//  |  +---------------------+  |
//  +---------------------------+
//
//  SMEP: Ring 0 iken Ring 3 sayfalarındaki kodu ÇALIŞTIRAMAZ  (CR4.bit20)
//  SMAP: Ring 0 iken Ring 3 sayfalarına doğrudan ERİŞEMEZ     (CR4.bit21)
//  CLAC/STAC: SMAP'ı geçici olarak devre dışı/aktif bırakır   (EFLAGS.AC)
// ============================================================================

/// SMEP'in aktif olup olmadığını izleyen atomik bayrak.
/// `AtomicBool` kullanılır çünkü çok çekirdekli ortamda yarış koşulsuz okunmalıdır.
static SMEP_ENABLED: AtomicBool = AtomicBool::new(false);
/// SMAP'ın aktif olup olmadığını izleyen atomik bayrak.
static SMAP_ENABLED: AtomicBool = AtomicBool::new(false);

/// SMEP (Supervisor Mode Execution Prevention) aktifleştir.
///
/// CR4 kaydının 20. biti (SUPERVISOR_MODE_EXECUTION_PROTECTION) set edilir.
/// Bu bit set edildikten sonra çekirdek, kullanıcı alanı bellekten asla
/// talimat çekemez; böylece "ret2user" ve JIT-spray saldırıları engellenir.
pub fn enable_smep() {
    use x86_64::registers::control::{Cr4, Cr4Flags};
    if !crate::cpu::smep_supported() {
        SMEP_ENABLED.store(false, Ordering::SeqCst);
        sec_serial_println!("[SEC] SMEP unavailable in current CPU profile");
        return;
    }
    unsafe {
        x86_64::instructions::port::PortWriteOnly::<u8>::new(0xE9).write(b'X');
        Cr4::update(|cr4| cr4.insert(Cr4Flags::SUPERVISOR_MODE_EXECUTION_PROTECTION));
        x86_64::instructions::port::PortWriteOnly::<u8>::new(0xE9).write(b'Y');
    }
    SMEP_ENABLED.store(true, Ordering::SeqCst);
    // Removed serial_println to avoid potential issues
}

/// SMAP (Supervisor Mode Access Prevention) aktifleştir.
///
/// CR4 kaydının 21. biti (SUPERVISOR_MODE_ACCESS_PREVENTION) set edilir.
/// SMAP aktif olduğunda, çekirdek kodu kullanıcı sayfalarına erişmek için
/// özel assembly talimatları (STAC/CLAC) kullanmalıdır; bu sayede
/// çekirdek hatası/exploit sonucu oluşan yetkisiz kullanıcı belleği
/// okumaları önlenir.
pub fn enable_smap() {
    use x86_64::registers::control::{Cr4, Cr4Flags};
    if !crate::cpu::smap_supported() {
        SMAP_ENABLED.store(false, Ordering::SeqCst);
        sec_serial_println!("[SEC] SMAP unavailable in current CPU profile");
        return;
    }
    unsafe {
        // SMAP'ı etkinleştirmeden önce debugcon üzerinden izleme yapılıyor
        x86_64::instructions::port::PortWriteOnly::<u8>::new(0xE9).write(b'A');
        Cr4::update(|cr4| cr4.insert(Cr4Flags::SUPERVISOR_MODE_ACCESS_PREVENTION));
        // CR4 güncellemesinden sonra debugcon'a bildir
        x86_64::instructions::port::PortWriteOnly::<u8>::new(0xE9).write(b'B');
        // CLAC: AC (Alignment Check) flag'ini temizler -> kullanıcı belleğine erişimi kapatır
        // STAC komutu ile geçici olarak açılabilir (copy_to_user/copy_from_user gibi yerlerde)
        core::arch::asm!("clac", options(nomem, nostack));
        // CLAC sonrası debugcon bildirimi
        x86_64::instructions::port::PortWriteOnly::<u8>::new(0xE9).write(b'C');
    }
    SMAP_ENABLED.store(true, Ordering::SeqCst);
    // Not: SMAP etkin olduğunda serial_println gibi işlevler kullanıcı
    // alanına erişmeye çalışabilir; bu yüzden bu fonksiyon içinde serial_println kaldırıldı.
}

/// SMAP korumasını geçici olarak devre dışı bırakmak için EFLAGS.AC bitini TEMİZLER (CLAC).
///
/// SMAP etkinken AC=0 ise çekirdek kullanıcı sayfalarına ERİŞEMEZ.
/// Kullanıcı tamponuna güvenli kopyalama yaparken smap_enable/smap_disable
/// çifti birlikte kullanılır.
#[inline(always)]
pub unsafe fn smap_disable() {
    core::arch::asm!("clac", options(nomem, nostack));
}

/// SMAP korumasını geçici erişim için geçersiz kılar; EFLAGS.AC bitini SETLER (STAC).
///
/// AC=1 iken SMAP koruması atlanır ve çekirdek kullanıcı sayfasına erişebilir.
/// Bu pencereyi kısa tutmak kritiktir - işlem sonrası smap_disable() çağrılmalıdır.
#[inline(always)]
pub unsafe fn smap_enable() {
    core::arch::asm!("stac", options(nomem, nostack));
}

/// SMEP'in şu an aktif olup olmadığını SeqCst sıralaması ile okur.
pub fn is_smep_enabled() -> bool {
    SMEP_ENABLED.load(Ordering::SeqCst)
}

/// SMAP'ın şu an aktif olup olmadığını SeqCst sıralaması ile okur.
pub fn is_smap_enabled() -> bool {
    SMAP_ENABLED.load(Ordering::SeqCst)
}

// ============================================================================
// STACK CANARY - Buffer Overflow Protection (Tampon Taşması Koruması)
//
// Stack canary, bir fonksiyon çağrısında yerel değişkenlerle dönüş adresi
// arasına yerleştirilen gizli bir değerdir. Fonksiyon dönmeden önce bu
// değer kontrol edilir; farklıysa bellek taşması yaşandığı anlaşılır.
//
// Stack çerçeve düzeni (x86-64):
//
//   +------------------+  <-- Yüksek adres
//   |   Dönüş Adresi   |  <- Exploit bunu değiştirmeye çalışır
//   +------------------+
//   |   Kaydedilmiş RBP|
//   +------------------+
//   |  *** CANARY  ***  |  <- Taşma bu değeri bozacaktır
//   +------------------+
//   |   Yerel Değişk.  |  <- Buradan taşma başlar (buffer overflow)
//   +------------------+  <-- Düşük adres
//
// Canary değeri:
//  - Boot sırasında CSPRNG ile randomize edilir
//  - Her CPU için farklı türetilmiş değerler kullanılır (per-CPU)
//  - Bozulursa __stack_chk_fail() çağrılarak çekirdek durdurulur
// ============================================================================

/// Global stack canary değeri (boot sırasında rastgeleleştirilir).
/// Varsayılan DEADBEEF_CAFEBABE değeri salt başlangıç yer tutucusudur;
/// `init_stack_canary()` çağrısı bu değeri gerçek rastgele ile değiştirir.
static STACK_CANARY: AtomicU64 = AtomicU64::new(0xDEADBEEF_CAFEBABE);

/// Her CPU için ayrı canary değerleri. CPU ID'ye göre türetilerek
/// aynı sistemde farklı çekirdeklerin canary'leri farklı olur.
static PER_CPU_CANARIES: Mutex<alloc::vec::Vec<u64>> = Mutex::new(alloc::vec::Vec::new());

/// Stack canary'yi başlatır: rastgele bir değer üretip global değişkene yazar.
pub fn init_stack_canary() {
    // Random canary oluştur
    let canary = crate::random::rand_u64() ^ 0xCAFEBABE_DEADBEEF;
    STACK_CANARY.store(canary, Ordering::SeqCst);

    sec_serial_println!("[SEC] Stack canary initialized");
}

/// CPU başına farklı canary değeri türetir.
/// Her CPU, global canary + sabit çarpan * cpu_id formülüyle hesaplanır.
pub fn init_per_cpu_canary(cpu_id: u32) {
    let canary = STACK_CANARY
        .load(Ordering::SeqCst)
        .wrapping_add(cpu_id as u64 * 0x12345678);

    let mut canaries = PER_CPU_CANARIES.lock();
    let idx = cpu_id as usize;
    if canaries.len() <= idx {
        canaries.resize(idx + 1, 0);
    }
    canaries[idx] = canary;

    sec_serial_println!("[SEC] CPU {} stack canary derived", cpu_id);
}

/// Mevcut CPU'nun canary değerini döndürür.
/// CPU ID'ye göre per-CPU tablosuna bakılır; bulunamazsa global canary kullanılır.
pub fn get_current_canary() -> u64 {
    let cpu_id = crate::cpu::smp::current_cpu_id();
    let canaries = PER_CPU_CANARIES.lock();
    canaries
        .get(cpu_id as usize)
        .copied()
        .unwrap_or_else(|| STACK_CANARY.load(Ordering::SeqCst))
}

/// GCC/Clang'ın ürettiği stack-protector epilogu bu sembolü çağırır.
///
/// Derleyici, fonksiyon prolog'unda canary değerini stack'e yazar;
/// epilog'da ise orijinal değerle karşılaştırır. Bozulmuşsa bu
/// fonksiyon tetiklenir ve çekirdek paniğe girerek saldırıyı durdurur.
/// Dönüş tipi `!` (never) = bu fonksiyon asla geri dönmez.
#[no_mangle]
pub extern "C" fn __stack_chk_fail() -> ! {
    sec_serial_println!("[SEC] *** STACK CANARY VIOLATION ***");
    sec_serial_println!("[SEC] Buffer overflow detected! Halting...");

    // Kernel panic - saldırı tespit edildi, sistem güvenli biçimde durduruldu
    panic!("Stack buffer overflow detected - possible exploit attempt!");
}

/// GCC/Clang'ın `__stack_chk_guard` sembolü için canary değeri sağlar.
///
/// Derleyici bazı mimarilerde canary değerini bu sembolden okur.
/// Her çekirdek çağrısında mevcut CPU'nun canary'si döndürülür.
#[no_mangle]
pub extern "C" fn __stack_chk_guard() -> u64 {
    get_current_canary()
}

// ============================================================================
// ASLR (Address Space Layout Randomization) - Adres Alanı Düzeni Rastgeleleştirme
//
// ASLR, saldırganın bellek adreslerini tahmin edememesini sağlar.
// Her process başlatılışında stack, heap ve mmap alanları rastgele
// ofsetlerle yerleştirilir.
//
// Koruma yokken (sabit adresler):        ASLR ile:
//  Stack  -> 0x7FFF_0000_0000 (sabit)    Stack  -> 0x7FFF_0000_0000 + rastgele_ofset
//  Heap   -> 0x0000_1234_0000 (sabit)    Heap   -> 0x0000_1234_0000 + rastgele_ofset
//  Code   -> 0xFFFF_8000_0000 (sabit)    Code   -> KASLR ile kernel de taşınabilir
//
// Bu mod, kullanıcı alanı ASLR ofsetlerini yönetir.
// Ofsetler sayfa sınırına (4KB) hizalanır: `& !0xFFF`
// ============================================================================

/// Kullanıcı alanı mmap bölgesi için rastgele ofset (ASLR).
static MMAP_ASLR_OFFSET: AtomicU64 = AtomicU64::new(0);
/// Kullanıcı stack bölgesi için rastgele ofset (ASLR).
static STACK_ASLR_OFFSET: AtomicU64 = AtomicU64::new(0);
/// Kullanıcı heap bölgesi için rastgele ofset (ASLR).
static HEAP_ASLR_OFFSET: AtomicU64 = AtomicU64::new(0);
/// ASLR'ın aktif olup olmadığını izleyen bayrak.
static ASLR_ENABLED: AtomicBool = AtomicBool::new(false);

/// ASLR'ı başlatır: mmap, stack ve heap için rastgele ofsetler oluşturur.
///
/// Tüm ofsetler sayfa sınırına (`& !0xFFF`) hizalanır; böylece
/// page table eşlemesi tutarlı kalır.
pub fn init_aslr() {
    // Her alan için bağımsız rastgele ofset üret, sayfa sınırına hizala
    let mmap_offset = (crate::random::rand_u64() % crate::memory::USER_MMAP_RANDOM_RANGE) & !0xFFF;
    let stack_offset =
        (crate::random::rand_u64() % crate::memory::USER_STACK_RANDOM_RANGE) & !0xFFF;
    let heap_offset = (crate::random::rand_u64() % crate::memory::USER_HEAP_RANDOM_RANGE) & !0xFFF;

    MMAP_ASLR_OFFSET.store(mmap_offset, Ordering::SeqCst);
    STACK_ASLR_OFFSET.store(stack_offset, Ordering::SeqCst);
    HEAP_ASLR_OFFSET.store(heap_offset, Ordering::SeqCst);
    ASLR_ENABLED.store(true, Ordering::SeqCst);

    sec_serial_println!("[SEC] ASLR enabled");
}

/// ASLR uygulanmış mmap başlangıç adresini hesaplar.
/// `base` üzerine random ofset eklenerek döndürülür.
pub fn aslr_mmap_addr(base: u64) -> u64 {
    if ASLR_ENABLED.load(Ordering::SeqCst) {
        base + MMAP_ASLR_OFFSET.load(Ordering::SeqCst)
    } else {
        base
    }
}

/// ASLR uygulanmış stack başlangıç adresini hesaplar.
/// Stack aşağı büyüdüğünden `base` üzerinden ofset ÇIKARILIR.
pub fn aslr_stack_addr(base: u64) -> u64 {
    if ASLR_ENABLED.load(Ordering::SeqCst) {
        base - STACK_ASLR_OFFSET.load(Ordering::SeqCst)
    } else {
        base
    }
}

/// ASLR uygulanmış heap başlangıç adresini hesaplar.
/// Heap yukarı büyüdüğünden `base` üzerine ofset EKLENİR.
pub fn aslr_heap_addr(base: u64) -> u64 {
    if ASLR_ENABLED.load(Ordering::SeqCst) {
        base + HEAP_ASLR_OFFSET.load(Ordering::SeqCst)
    } else {
        base
    }
}

/// ASLR'ın şu an aktif olup olmadığını döndürür.
pub fn is_aslr_enabled() -> bool {
    ASLR_ENABLED.load(Ordering::SeqCst)
}

// ============================================================================
// NX/DEP (No-Execute / Data Execution Prevention) - Çalıştırma Engeli
//
// NX (No-Execute), sayfa tablosu girişlerindeki bit 63 (XD/NX bit) üzerinden
// çalışır. Bu bit set edilen sayfalara CPU'nun talimat getirmesi engellenir.
//
// Etkinleştirme: EFER MSR (0xC000_0080) bit 11 = NXE set edilmelidir.
// Ardından her sayfa tablosu girişinde bireysel NX biti kullanılabilir.
//
//  Sayfa Tablosu Girişi (PTE) düzeni (64-bit):
//  Bit 63 : NX (No-Execute) -- bu bit set ise kod çalıştırılamaz
//  Bit  0 : Present
//  Bit  1 : Read/Write
//  ...
//
// NX olmadan bir saldırgan veri bölgesine shellcode yazıp oradan atlatabilir.
// ============================================================================

/// NX desteğinin aktif olup olmadığını izleyen bayrak.
static NX_ENABLED: AtomicBool = AtomicBool::new(false);

/// EFER MSR'ın NXE bitini (bit 11) set ederek NX/DEP'i etkinleştirir.
///
/// EFER = Extended Feature Enable Register (MSR 0xC000_0080).
/// NXE bit set edildikten sonra sayfa tablosundaki her PTE'nin bit 63'ü
/// bağımsız olarak "bu sayfada kod çalışmasın" anlamı taşır.
pub fn enable_nx() {
    const MSR_EFER: u32 = 0xC000_0080;

    unsafe {
        let mut efer = Msr::new(MSR_EFER);
        let val = efer.read();
        // Bit 11 = NXE (No-Execute Enable) - tüm PTE'lerin NX bitini aktifleştirir
        efer.write(val | (1 << 11));
        NX_ENABLED.store(true, Ordering::SeqCst);
        sec_serial_println!("[SEC] NX/DEP enabled - Non-executable memory protection active");
    }
}

/// NX/DEP'in etkin olup olmadığını döndürür.
pub fn is_nx_enabled() -> bool {
    NX_ENABLED.load(Ordering::SeqCst)
}

// ============================================================================
// W^X (Write XOR Execute) Policy - Yaz ya da Çalıştır, İkisi Birden Olmaz
//
// W^X: Bir bellek sayfası aynı anda hem yazılabilir (W) hem çalıştırılabilir (X)
// olamaz. Bu kural, JIT derleyicileri bile içerir; çalışma zamanı kod üretimi
// yapılacaksa önce W açık kod yazılır, ardından W kapatılıp X açılır.
//
//  Geçerli durumlar:
//    W=1, X=0  -> Veri sayfası (değiştirilebilir, çalıştırılamaz)
//    W=0, X=1  -> Kod sayfası  (değiştirilemez, çalıştırılabilir)
//    W=0, X=0  -> Salt okunur  (ne değişebilir ne çalışabilir)
//
//  GEÇERSİZ durum:
//    W=1, X=1  -> YASAK - bu kombinasyon saldırı yüzeyini genişletir
// ============================================================================

/// W^X politikasının aktif olup olmadığını izleyen bayrak.
static WXORX_ENABLED: AtomicBool = AtomicBool::new(false);

/// W^X politikasını etkinleştirir.
/// Etkinleştirildikten sonra `check_wxorx()` ile her sayfa eşlemesi denetlenir.
pub fn enable_wxorx() {
    WXORX_ENABLED.store(true, Ordering::SeqCst);
    sec_serial_println!("[SEC] W^X policy enabled - Pages cannot be both writable and executable");
}

/// W^X kuralını kontrol eder: hem yazılabilir hem çalıştırılabilir ise `false` döner.
///
/// Sayfa haritalama kodunda her yeni eşleme öncesi çağrılmalıdır.
/// Politika kapalıysa her kombinasyona izin verilir (geliştirme modunda kullanışlı).
pub fn check_wxorx(writable: bool, executable: bool) -> bool {
    if !WXORX_ENABLED.load(Ordering::SeqCst) {
        return true; // Politika kapalı, her şey serbest
    }

    // W^X: writable XOR executable - ikisi aynı anda set olamaz
    !(writable && executable)
}

/// W^X politikasının etkin olup olmadığını döndürür.
pub fn is_wxorx_enabled() -> bool {
    WXORX_ENABLED.load(Ordering::SeqCst)
}

// ============================================================================
// SECURITY INITIALIZATION - Güvenlik Alt Sistemi Başlatma
//
// Güvenlik özellikleri belirli bir sırayla etkinleştirilmelidir.
// Örneğin SMAP, stack canary'den önce gelmelidir; çünkü canary
// başlatılırken kullanıcı bellek erişimi gerekmeyebilir.
//
// Başlatma sırası:
//   1. NX/DEP   -> EFER.NXE (MSR yazma, her VM çekirdeğinde güvenli)
//   2. SMEP     -> CR4.bit20 (kullanıcı kod çalıştırma yasağı)
//   3. SMAP     -> CR4.bit21 + CLAC (kullanıcı bellek erişim yasağı)
//   4. Canary   -> Rastgele değer CSPRNG ile üretilir
//   5. ASLR     -> Kullanıcı alanı ofsetleri üretilir
//   6. W^X      -> Politika bayrağı set edilir
// ============================================================================

/// Tüm güvenlik özelliklerini sırayla başlatır.
///
/// Bu fonksiyon çekirdek başlangıcında (boot) yalnızca bir kez çağrılır.
/// Debugcon (port 0xE9) üzerinden her adım izlenir; Simics/QEMU ile
/// donanım hata ayıklaması yapılabilir.
pub fn init() {
    // Debugcon 'S' baytı: security::init() girildi (seri port hazır olmadan önce izleme)
    unsafe {
        use x86_64::instructions::port::PortWriteOnly;
        PortWriteOnly::<u8>::new(0xE9).write(b'S'); // Entered security::init
    }
    sec_serial_println!("[SEC] Initializing security subsystem...");

    unsafe {
        x86_64::instructions::port::PortWriteOnly::<u8>::new(0xE9).write(b'1');
    }
    // Adım 1: NX/DEP - EFER MSR'ın NXE bitini set eder
    enable_nx();

    unsafe {
        x86_64::instructions::port::PortWriteOnly::<u8>::new(0xE9).write(b'2');
    }
    // Adım 2: SMEP - CR4 bit 20'yi set eder (kullanıcı kodu çalıştırma yasağı)
    enable_smep();

    unsafe {
        x86_64::instructions::port::PortWriteOnly::<u8>::new(0xE9).write(b'3');
    }
    // Adım 3: SMAP - CR4 bit 21'i set eder + CLAC ile varsayılan koruma aktif
    enable_smap();

    unsafe {
        x86_64::instructions::port::PortWriteOnly::<u8>::new(0xE9).write(b'4');
    }
    // Adım 4: Stack Canary - CSPRNG ile rastgele değer üretilir
    init_stack_canary();

    unsafe {
        x86_64::instructions::port::PortWriteOnly::<u8>::new(0xE9).write(b'5');
    }
    // Adım 5: ASLR - mmap/stack/heap için rastgele page-aligned ofsetler
    init_aslr();

    unsafe {
        x86_64::instructions::port::PortWriteOnly::<u8>::new(0xE9).write(b'6');
    }
    // Adım 6: W^X - hem yazılabilir hem çalıştırılabilir sayfa politikası engeli
    enable_wxorx();

    // Adım 7: CFI ve KPTI koruma katmanları
    cfi::init();
    kpti::init();
    spectre::init();
    kpti::register_sensitive_range(0xFFFF_8000_0000_0000, 0x0000_8000_0000_0000);

    // Adım 8: Seccomp ve path-based sandbox policy motoru
    seccomp::init();
    landlock::init();
    if let Err(err) = crate::valkyrie_virt::init_valkyrie() {
        sec_serial_println!(
            "[SEC] Valkyrie-V init skipped: {:?} (fallback to non-hypervisor sandbox)",
            err
        );
    }

    // Adım 9: MPK/PKEYS (donanım destekliyse aktif edilir)
    mpk::init();
    anti_cheat::init();

    unsafe {
        x86_64::instructions::port::PortWriteOnly::<u8>::new(0xE9).write(b'9');
    }
    sec_serial_println!("[SEC] Security subsystem initialized ✓");
}

/// Her CPU'ya özgü güvenlik başlatması yapar (per-CPU canary türetme).
/// SMP sistemlerde her fiziksel çekirdek başlatıldığında çağrılır.
pub fn init_cpu_security(cpu_id: u32) {
    init_per_cpu_canary(cpu_id);
    spectre::init_cpu();
    sec_serial_println!("[SEC] CPU {} security initialized", cpu_id);
}

// ============================================================================
// SECURITY AUDIT LOGGING - Güvenlik Olay Günlüğü
//
// Kernel, güvenlik ihlallerini bu enum üzerinden sınıflandırır.
// Her olay tipi farklı bir saldırı vektörüne karşılık gelir:
//
//  StackCanaryViolation  -> Buffer overflow (stack bozuldu)
//  NxViolation           -> DEP ihlali (veri sayfası çalıştırılmaya çalışıldı)
//  SmepViolation         -> Kernel, kullanıcı kodu çalıştırmaya çalıştı
//  SmapViolation         -> Kernel, kullanıcı belleğine doğrudan erişti
//  WxorxViolation        -> W^X kuralı ihlal edildi
//  SeccompViolation(n)   -> n numaralı syscall BPF filtresi tarafından engellendi
//  SuspiciousSyscall(n)  -> n numaralı syscall şüpheli örüntü tespit edildi
// ============================================================================

/// Çekirdek güvenlik ihlali olay türleri.
#[derive(Debug, Clone, Copy)]
pub enum SecurityEvent {
    StackCanaryViolation,
    NxViolation,
    SmepViolation,
    SmapViolation,
    WxorxViolation,
    SeccompViolation(u64),
    SuspiciousSyscall(u64),
}

/// Güvenlik olayını seri porta yazar (denetim kaydı).
///
/// Gelecekte bu fonksiyon kalıcı günlük deposuna (ring buffer vb.) yazacak.
/// Şu an için serial çıktıyla sınırlıdır; üretim ortamında genişletilmeli.
pub fn log_security_event(event: SecurityEvent) {
    match event {
        SecurityEvent::StackCanaryViolation => {
            sec_serial_println!("[SEC/AUDIT] *** STACK CANARY VIOLATION ***");
        }
        SecurityEvent::NxViolation => {
            sec_serial_println!(
                "[SEC/AUDIT] NX violation - attempt to execute non-executable memory"
            );
        }
        SecurityEvent::SmepViolation => {
            sec_serial_println!("[SEC/AUDIT] SMEP violation - kernel tried to execute user code");
        }
        SecurityEvent::SmapViolation => {
            sec_serial_println!("[SEC/AUDIT] SMAP violation - kernel tried to access user memory");
        }
        SecurityEvent::WxorxViolation => {
            sec_serial_println!("[SEC/AUDIT] W^X violation - page is both writable and executable");
        }
        SecurityEvent::SeccompViolation(syscall) => {
            sec_serial_println!(
                "[SEC/AUDIT] Seccomp violation - syscall {} blocked",
                syscall
            );
        }
        SecurityEvent::SuspiciousSyscall(syscall) => {
            sec_serial_println!("[SEC/AUDIT] Suspicious syscall {} detected", syscall);
        }
    }
}

// ============================================================================
// SECURITY STATUS - Güvenlik Durumu Özeti
//
// SecurityStatus yapısı, etkin güvenlik özelliklerini tek bir veri
// yapısında toplar. `score()` metodu 0-10 arası puan üretir:
//  NX    -> +2  (kritik: shellcode önleme)
//  SMEP  -> +2  (kritik: ring0 kaçışı engeli)
//  SMAP  -> +2  (kritik: user bellek erişim engeli)
//  ASLR  -> +1  (orta: adres tahmin güçleştirme)
//  W^X   -> +1  (orta: JIT-spray engeli)
//  Canary-> +2  (kritik: stack overflow tespiti)
//  Toplam: 10 = maksimum güvenlik skoru
// ============================================================================

/// Tüm güvenlik özelliklerinin anlık durumunu döndürür.
/// Canary başlatılmışsa varsayılan DEADBEEF değerinden farklı olur.
pub fn security_status() -> SecurityStatus {
    SecurityStatus {
        nx: is_nx_enabled(),
        smep: is_smep_enabled(),
        smap: is_smap_enabled(),
        aslr: is_aslr_enabled(),
        wxorx: is_wxorx_enabled(),
        canary: STACK_CANARY.load(Ordering::SeqCst) != 0xDEADBEEF_CAFEBABE,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SecurityStatus {
    pub nx: bool,
    pub smep: bool,
    pub smap: bool,
    pub aslr: bool,
    pub wxorx: bool,
    pub canary: bool,
}

impl SecurityStatus {
    pub fn score(&self) -> u8 {
        let mut score = 0u8;
        if self.nx {
            score += 2;
        }
        if self.smep {
            score += 2;
        }
        if self.smap {
            score += 2;
        }
        if self.aslr {
            score += 1;
        }
        if self.wxorx {
            score += 1;
        }
        if self.canary {
            score += 2;
        }
        score
    }
}

// ============================================================================
// THE ETERNAL SEAL - Kernel Kod Bütünlüğü Koruması (Runtime Integrity Check)
//
// Eternal Seal, çekirdek kod bölgelerini çalışma zamanında izleyen
// bir bütünlük denetim sistemidir. Üç katmanlı hiyerarşik doğrulama kullanır:
//
//  Düzey 0 - XOR Parity (~1 döngü/64 bayt, ultra hızlı):
//    Her 4KB sayfa için 64-bit XOR özeti tutulur.
//    Yanlış pozitif (tesadüfi değişim) olasılığı: 2^-64
//
//  Düzey 1 - CRC32 (SSE4.2 HW hızlandırmalı, ~3 döngü/bayt):
//    Parity uyuşmazlığında CRC32 ile çapraz doğrulama yapılır.
//
//  Düzey 2 - SHA-256 (yalnızca şüpheli sayfalarda):
//    İki düzey de uyuşmazsa kriptografik kanıt toplanır.
//
//  Öncelik tabanlı tarama:
//    priority=0 (kritik): Her tick kontrol
//    priority=1 (yüksek): Her tick'te %50 olasılıkla kontrol
//    priority=2 (normal) : Konfigüre edilebilir örnekleme oranı
// ============================================================================

use alloc::collections::BTreeMap;

/// Kernel kod bölgesi tanımı.
/// `priority` alanı: 0=kritik (her tick), 1=yüksek (%50), 2=normal (örneklemeli).
#[derive(Clone, Copy, Debug)]
pub struct KernelRegion {
    pub start: u64,
    pub size: u64,
    pub name: &'static str,
    pub priority: u8, // 0=critical, 1=high, 2=normal
}

/// Checksum doğrulama düzeyleri (hızdan güvenliğe doğru sıralı).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChecksumLevel {
    /// Düzey 0: XOR Parity - ~1 döngü/64 bayt (en hızlı)
    Parity,
    /// Düzey 1: CRC32 Donanım Hızlandırmalı (SSE4.2) - ~3 döngü/bayt
    Crc32,
    /// Düzey 2: SHA-256 - Yalnızca şüpheli sayfalarda kullanılır
    Sha256,
}

/// Bir sayfanın bütünlük bilgisi (checksum + ihlal sayacı).
#[derive(Clone, Debug)]
pub struct PageIntegrity {
    pub parity: u64,     // 4KB sayfa için 64-bit XOR parity (8 bayt)
    pub crc32: u32,      // CRC32 sağlama toplamı (donanım hızlandırmalı)
    pub last_check: u64, // Son kontrol edildiği syscall tick sayısı
    pub violations: u32, // Bu sayfada toplam ihlal sayısı
}

/// Eternal Seal global durumu (kilitli koleksiyon değişkenleri)
static SEAL_REGIONS: Mutex<alloc::vec::Vec<KernelRegion>> = Mutex::new(alloc::vec::Vec::new());
/// Adres -> PageIntegrity haritası (BTreeMap = sıralı, önbellek dostu)
static SEAL_INTEGRITY: Mutex<BTreeMap<u64, PageIntegrity>> = Mutex::new(BTreeMap::new());
/// Eternal Seal'ın etkin olup olmadığı
static SEAL_ENABLED: AtomicBool = AtomicBool::new(false);
/// Normal öncelikli sayfalar için tick başına örnekleme oranı (varsayılan: %5)
static SEAL_SAMPLING_RATE: AtomicU64 = AtomicU64::new(5); // %5 per tick

/// Yeni bir kernel bölgesini Eternal Seal'a kaydeder.
/// Boot sonrası kod bölgeleri, syscall tablosu vb. için çağrılır.
pub fn seal_register_region(start: u64, size: u64, name: &'static str, priority: u8) {
    let region = KernelRegion {
        start,
        size,
        name,
        priority,
    };
    SEAL_REGIONS.lock().push(region);
    crate::serial_println!(
        "[SEAL] Region registered: {} @ {:#x} (priority {})",
        name,
        start,
        priority
    );
}

/// XOR Parity hesaplar - O(n/8) karmaşıklık, en hızlı yöntem.
///
/// 8 baytlık bloklar halinde XOR işlemi uygulanır; son kısım sıfır-pad
/// edilmiş 8 baytlık tamponla tamamlanır. Sonuç 64-bit XOR özetidir.
#[inline(always)]
fn compute_parity(data: &[u8]) -> u64 {
    // 64-bit bloklar halinde XOR - en hızlı yol
    let chunks = data.chunks_exact(8);
    let remainder = chunks.remainder();

    let mut parity = chunks.fold(0u64, |acc, chunk| {
        acc ^ u64::from_le_bytes(chunk.try_into().unwrap())
    });

    // Kalan baytlar (8'in katı olmayan son parça) sıfırla doldurulup XOR'lanır
    if !remainder.is_empty() {
        let mut last = [0u8; 8];
        last[..remainder.len()].copy_from_slice(remainder);
        parity ^= u64::from_le_bytes(last);
    }

    parity
}

/// CRC32-C hesaplar - SSE4.2 `_mm_crc32_u64` talimatı ile donanım hızlandırmalı.
///
/// CRC-32C (Castagnoli) polinom kullanılır; Ethernet CRC32'den farklıdır.
/// SSE4.2 desteği olmayan mimarilerde yazılım fallback devreye girer.
/// Dönen değer: `!crc` (sonuç inversinin alınması standarttır).
#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn compute_crc32(data: &[u8]) -> u32 {
    use core::arch::x86_64::_mm_crc32_u64;

    let mut crc = 0xFFFFFFFFu32;
    let len = data.len();
    let mut i = 0;

    // 8 baytlık bloklar: _mm_crc32_u64 ile tek talimat işlemi
    while i + 8 <= len {
        unsafe {
            let val = u64::from_le_bytes(data[i..i + 8].try_into().unwrap());
            crc = _mm_crc32_u64(crc as u64, val) as u32;
        }
        i += 8;
    }

    // Kalan baytlar (8'in katı olmayan son parça) tek tek işlenir
    while i < len {
        unsafe {
            crc = core::arch::x86_64::_mm_crc32_u8(crc, data[i]);
        }
        i += 1;
    }

    !crc
}

/// CRC32-C yazılım geri dönüşü (x86_64 dışı mimari veya SSE4.2 yok ise).
///
/// CRC-32C polinom sabiti: 0x82F63B78 (Castagnoli polinom ters yansıması)
#[cfg(not(target_arch = "x86_64"))]
fn compute_crc32(data: &[u8]) -> u32 {
    // Yazılım fallback - CRC-32C polinom (0x1EDC6F41 ters yansıma = 0x82F63B78)
    let mut crc = 0xFFFFFFFFu32;
    for &byte in data {
        crc = (crc >> 8) ^ CRC_TABLE[(crc as u8 ^ byte) as usize];
    }
    !crc
}

#[cfg(not(target_arch = "x86_64"))]
const CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut j = 0;
        while j < 8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0x82F63B78
            } else {
                crc >> 1
            };
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};

/// Basitleştirilmiş SHA-256 benzeri hash (yalnızca Düzey 2 doğrulama için).
///
/// UYARI: Bu gerçek bir SHA-256 uygulaması DEĞİLDİR.
/// Parity + CRC32 + avalanche efekti tabanlı bir pseudo-hash'tir.
/// Üretim ortamı için `crate::crypto::sha256` ile değiştirilmelidir.
fn compute_sha256_simple(data: &[u8]) -> [u8; 32] {
    // Basitleştirilmiş: XOR tabanlı pseudo-hash (production'da değiştirilmeli)
    // Not: Gerçek implementasyon için crate::crypto::sha256 kullanılmalı
    let mut hash = [0u8; 32];

    // Parity + CRC32 sonuçları hash dizisine yazılır
    let parity = compute_parity(data);
    let crc = compute_crc32(data);

    hash[..8].copy_from_slice(&parity.to_le_bytes());
    hash[8..12].copy_from_slice(&crc.to_le_bytes());
    hash[12..16].copy_from_slice(&(data.len() as u32).to_le_bytes());

    // Avalanche efekti: bitlerin yayılmasını artırmak için ek geçişler
    for i in 16..32 {
        hash[i] = hash[i - 8] ^ hash[i - 4] ^ (i as u8);
    }

    hash
}

/// Eternal Seal'ı başlatır: kritik bölgeleri kaydeder ve ilk checksum'ları hesaplar.
///
/// Bölge adresleri linker script veya memory map'ten alınmalıdır.
/// Boot sonrası tek kez çağrılır; SEAL_ENABLED bayrağı set edilerek
/// `seal_guardian_tick()` aktifleştirilir.
pub fn seal_init() {
    crate::serial_println!("[SEAL] Initializing Eternal Seal...");

    // Kernel kritik bölgelerini kaydet (adresler linker script'ten gelmeli)
    seal_register_region(
        0xFFFF_FFFF_8000_0000, // Kernel başlangıç adresi
        0x0010_0000,           // 1MB çekirdek kod alanı
        "kernel_code",
        0, // Kritik öncelik - her tick kontrol edilir
    );

    seal_register_region(
        0xFFFF_FFFF_8010_0000,
        0x0008_0000, // 512KB syscall işleyici bölgesi
        "syscall_table",
        0, // Kritik öncelik
    );

    // İlk referans checksum'larını hesapla (boot sonrası "iyi" durumu kaydet)
    let regions = SEAL_REGIONS.lock();
    let mut integrity = SEAL_INTEGRITY.lock();

    for region in regions.iter() {
        let page_count = region.size / 4096;
        for i in 0..page_count {
            let addr = region.start + i * 4096;

            // Kernel alanı güvenli okuma (fiziksel adresten doğrudan erişim)
            let ptr = addr as *const u8;
            let data = unsafe { core::slice::from_raw_parts(ptr, 4096) };

            integrity.insert(
                addr,
                PageIntegrity {
                    parity: compute_parity(data),
                    crc32: compute_crc32(data),
                    last_check: 0,
                    violations: 0,
                },
            );
        }
    }

    SEAL_ENABLED.store(true, Ordering::SeqCst);
    crate::serial_println!("[SEAL] {} pages sealed", integrity.len());
}

/// Zamanlayıcı tick'inde güvenlik denetimi yapar.
///
/// Her sistem tick'inde çağrılır; öncelik bazlı olasılıksal örnekleme ile
/// yüksek performans korunur. İhlal tespit akışı:
///   1) Düzey 0: XOR Parity hızlı kontrol
///   2) Eşleşmezse Düzey 1: CRC32 doğrulama
///   3) İki düzey de uyuşmazsa ihlal loglanır
pub fn seal_guardian_tick(current_tick: u64) {
    if !SEAL_ENABLED.load(Ordering::SeqCst) {
        return;
    }

    let regions = SEAL_REGIONS.lock();
    let mut integrity = SEAL_INTEGRITY.lock();
    let sampling_rate = SEAL_SAMPLING_RATE.load(Ordering::SeqCst);

    // Olasılıksal örnekleme için rastgele sayı üret
    let rand_val = crate::random::next_u32();

    for region in regions.iter() {
        let page_count = region.size / 4096;

        // Öncelik bazlı örnekleme olasılığı:
        // Kritik (0): her tick kontrol edilir, Yüksek (1): %50, Normal (2): yapılandırılabilir
        let check_prob = match region.priority {
            0 => 100,                  // Her zaman kontrol et
            1 => 50,                   // %50 olasılıkla kontrol et
            _ => sampling_rate as u32, // Yapılandırılabilir örnekleme
        };

        for i in 0..page_count {
            let addr = region.start + i * 4096;

            // Olasılıksal atlama: check_prob < 100 ise random'a göre atla
            if check_prob < 100 && (rand_val % 100) >= check_prob {
                continue;
            }

            // Düzey 0: XOR Parity (ultra hızlı ilk kontrol)
            let ptr = addr as *const u8;
            let data = unsafe { core::slice::from_raw_parts(ptr, 4096) };
            let current_parity = compute_parity(data);

            if let Some(stored) = integrity.get_mut(&addr) {
                if stored.parity != current_parity {
                    // Düzey 1: CRC32 çapraz doğrulama (false positive azaltmak için)
                    let current_crc = compute_crc32(data);

                    if stored.crc32 != current_crc {
                        // BÜTÜNLÜK İHLALİ TESPİT EDİLDİ!
                        stored.violations += 1;

                        crate::serial_println!(
                            "[SEAL] *** INTEGRITY VIOLATION *** {} @ {:#x}",
                            region.name,
                            addr
                        );

                        log_security_event(SecurityEvent::NxViolation);

                        // TODO: Kendi kendini iyileştirme - gölge kopya'dan geri yükle
                        // seal_self_heal(addr);
                    }
                }

                stored.last_check = current_tick;
            }
        }
    }
}

/// Bütünlüğü bozulan sayfayı gölge kopyasından geri yükler (kendi kendini iyileştirme).
///
/// TODO: Gölge kopya desteği henüz eklenmemiştir.
/// Gerçek implementasyonda boot sırasında oluşturulan değiştirilemez (immutable)
/// kopya kullanılacak; ihlal edilen sayfa o kopyayla üzerine yazılacaktır.
pub fn seal_self_heal(addr: u64) -> Result<(), &'static str> {
    crate::serial_println!("[SEAL] Self-healing page at {:#x}", addr);

    // TODO: Shadow copy'den orijinali geri yükle
    // Bu, boot sırasında oluşturulan immutable kopyadan olacak

    // Şimdilik sadece ihlali logla
    log_security_event(SecurityEvent::NxViolation);

    Ok(())
}

/// Eternal Seal'ın etkin olup olmadığını döndürür.
pub fn is_seal_enabled() -> bool {
    SEAL_ENABLED.load(Ordering::SeqCst)
}

/// Normal öncelikli sayfalarda örnekleme oranını ayarlar (0-100 arası yüzde değeri).
/// 100 = her tick tüm sayfalar, 5 = her tick %5 örnekleme (varsayılan).
pub fn seal_set_sampling_rate(rate: u64) {
    SEAL_SAMPLING_RATE.store(rate.min(100), Ordering::SeqCst);
}

/// Toplam mühürlü sayfa sayısını ve toplam ihlal sayısını döndürür.
pub fn seal_stats() -> SealStats {
    let integrity = SEAL_INTEGRITY.lock();
    let total = integrity.len();
    let violations = integrity.values().map(|p| p.violations).sum();

    SealStats {
        total_pages: total,
        total_violations: violations,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SealStats {
    pub total_pages: usize,
    pub total_violations: u32,
}
