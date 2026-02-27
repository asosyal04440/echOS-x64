//! # echOS Security Subsystem
//!
//! Kapsamlı güvenlik özellikleri: SMEP/SMAP, Stack Canary, ASLR, NX/DEP, W^X
//! Capability-based security, SELinux-like MAC, TPM 2.0 support

pub mod capability;
pub mod mac;
pub mod simics_gate;
pub mod tpm;

use core::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use spin::Mutex;
use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};
use x86_64::registers::model_specific::Msr;

// ============================================================================
// SMEP/SMAP - Supervisor Mode Execution/Access Prevention
// ============================================================================

/// SMEP aktif mi
static SMEP_ENABLED: AtomicBool = AtomicBool::new(false);
/// SMAP aktif mi  
static SMAP_ENABLED: AtomicBool = AtomicBool::new(false);

/// SMEP (Supervisor Mode Execution Prevention) aktifleştir
/// Ring 0'dayken user space sayfalarında kod çalıştırmayı engeller
pub fn enable_smep() {
    use x86_64::registers::control::{Cr4, Cr4Flags};
    unsafe {
        x86_64::instructions::port::PortWriteOnly::<u8>::new(0xE9).write(b'X');
        Cr4::update(|cr4| cr4.insert(Cr4Flags::SUPERVISOR_MODE_EXECUTION_PROTECTION));
        x86_64::instructions::port::PortWriteOnly::<u8>::new(0xE9).write(b'Y');
    }
    SMEP_ENABLED.store(true, Ordering::SeqCst);
    // Removed serial_println to avoid potential issues
}

/// SMAP (Supervisor Mode Access Prevention) aktifleştir
/// Ring 0'dayken user space sayfalarına erişimi engeller
pub fn enable_smap() {
    use x86_64::registers::control::{Cr4, Cr4Flags};
    unsafe {
        // Debug before SMAP
        x86_64::instructions::port::PortWriteOnly::<u8>::new(0xE9).write(b'A');
        Cr4::update(|cr4| cr4.insert(Cr4Flags::SUPERVISOR_MODE_ACCESS_PREVENTION));
        // Debug after CR4 update
        x86_64::instructions::port::PortWriteOnly::<u8>::new(0xE9).write(b'B');
        // clac (Clear AC flag) ile varsayılan olarak user data erişimini kapat
        core::arch::asm!("clac", options(nomem, nostack));
        // Debug after clac
        x86_64::instructions::port::PortWriteOnly::<u8>::new(0xE9).write(b'C');
    }
    SMAP_ENABLED.store(true, Ordering::SeqCst);
    // Note: serial_println removed to avoid potential SMAP issues
}

/// SMAP'ı geçici olarak devre dışı bırak (AC flag set/clear)
/// Kullanıcı buffer'ına güvenli erişim için
#[inline(always)]
pub unsafe fn smap_disable() {
    core::arch::asm!("clac", options(nomem, nostack));
}

/// SMAP'ı tekrar aktifleştir
#[inline(always)]
pub unsafe fn smap_enable() {
    core::arch::asm!("stac", options(nomem, nostack));
}

/// SMEP aktif mi kontrol et
pub fn is_smep_enabled() -> bool {
    SMEP_ENABLED.load(Ordering::SeqCst)
}

/// SMAP aktif mi kontrol et
pub fn is_smap_enabled() -> bool {
    SMAP_ENABLED.load(Ordering::SeqCst)
}

// ============================================================================
// STACK CANARY - Buffer Overflow Protection
// ============================================================================

/// Global stack canary değeri (boot'ta randomize edilir)
static STACK_CANARY: AtomicU64 = AtomicU64::new(0xDEADBEEF_CAFEBABE);

/// Per-CPU canary değerleri
static PER_CPU_CANARIES: Mutex<alloc::vec::Vec<u64>> = Mutex::new(alloc::vec::Vec::new());

/// Stack canary'yi initialize et
pub fn init_stack_canary() {
    // Random canary oluştur
    let canary = crate::random::rand_u64() ^ 0xCAFEBABE_DEADBEEF;
    STACK_CANARY.store(canary, Ordering::SeqCst);
    
    crate::serial_println!("[SEC] Stack canary initialized: {:#x}", canary);
}

/// Per-CPU canary oluştur
pub fn init_per_cpu_canary(cpu_id: u32) {
    let canary = STACK_CANARY.load(Ordering::SeqCst).wrapping_add(cpu_id as u64 * 0x12345678);
    
    let mut canaries = PER_CPU_CANARIES.lock();
    let idx = cpu_id as usize;
    if canaries.len() <= idx {
        canaries.resize(idx + 1, 0);
    }
    canaries[idx] = canary;
    
    crate::serial_println!("[SEC] CPU {} stack canary: {:#x}", cpu_id, canary);
}

/// Mevcut CPU'nun canary'sini al
pub fn get_current_canary() -> u64 {
    let cpu_id = crate::cpu::smp::current_cpu_id();
    let canaries = PER_CPU_CANARIES.lock();
    canaries.get(cpu_id as usize).copied().unwrap_or_else(|| STACK_CANARY.load(Ordering::SeqCst))
}

/// Stack canary doğrulama hatası
#[no_mangle]
pub extern "C" fn __stack_chk_fail() -> ! {
    crate::serial_println!("[SEC] *** STACK CANARY VIOLATION ***");
    crate::serial_println!("[SEC] Buffer overflow detected! Halting...");
    
    // Kernel panic
    panic!("Stack buffer overflow detected - possible exploit attempt!");
}

/// GCC/Clang stack protector için canary değeri
#[no_mangle]
pub extern "C" fn __stack_chk_guard() -> u64 {
    get_current_canary()
}

// ============================================================================
// ASLR (Address Space Layout Randomization)
// ============================================================================

/// User space mmap random offset
static MMAP_ASLR_OFFSET: AtomicU64 = AtomicU64::new(0);
/// User stack random offset
static STACK_ASLR_OFFSET: AtomicU64 = AtomicU64::new(0);
/// User heap random offset
static HEAP_ASLR_OFFSET: AtomicU64 = AtomicU64::new(0);
/// ASLR aktif mi
static ASLR_ENABLED: AtomicBool = AtomicBool::new(false);

/// ASLR'ı initialize et
pub fn init_aslr() {
    // Random offset'ler oluştur (page-aligned)
    let mmap_offset = (crate::random::rand_u64() % crate::memory::USER_MMAP_RANDOM_RANGE) & !0xFFF;
    let stack_offset = (crate::random::rand_u64() % crate::memory::USER_STACK_RANDOM_RANGE) & !0xFFF;
    let heap_offset = (crate::random::rand_u64() % (64 * 1024 * 1024)) & !0xFFF;
    
    MMAP_ASLR_OFFSET.store(mmap_offset, Ordering::SeqCst);
    STACK_ASLR_OFFSET.store(stack_offset, Ordering::SeqCst);
    HEAP_ASLR_OFFSET.store(heap_offset, Ordering::SeqCst);
    ASLR_ENABLED.store(true, Ordering::SeqCst);
    
    crate::serial_println!("[SEC] ASLR enabled:");
    crate::serial_println!("  MMAP offset: {:#x}", mmap_offset);
    crate::serial_println!("  Stack offset: {:#x}", stack_offset);
    crate::serial_println!("  Heap offset: {:#x}", heap_offset);
}

/// ASLR uygulanmış mmap adresi
pub fn aslr_mmap_addr(base: u64) -> u64 {
    if ASLR_ENABLED.load(Ordering::SeqCst) {
        base + MMAP_ASLR_OFFSET.load(Ordering::SeqCst)
    } else {
        base
    }
}

/// ASLR uygulanmış stack adresi
pub fn aslr_stack_addr(base: u64) -> u64 {
    if ASLR_ENABLED.load(Ordering::SeqCst) {
        base - STACK_ASLR_OFFSET.load(Ordering::SeqCst)
    } else {
        base
    }
}

/// ASLR uygulanmış heap adresi
pub fn aslr_heap_addr(base: u64) -> u64 {
    if ASLR_ENABLED.load(Ordering::SeqCst) {
        base + HEAP_ASLR_OFFSET.load(Ordering::SeqCst)
    } else {
        base
    }
}

/// ASLR aktif mi
pub fn is_aslr_enabled() -> bool {
    ASLR_ENABLED.load(Ordering::SeqCst)
}

// ============================================================================
// NX/DEP (No-Execute / Data Execution Prevention)
// ============================================================================

/// NX desteği aktif mi
static NX_ENABLED: AtomicBool = AtomicBool::new(false);

/// NX bit'i aktifleştir (EFER.NXE)
pub fn enable_nx() {
    const MSR_EFER: u32 = 0xC000_0080;
    
    unsafe {
        let mut efer = Msr::new(MSR_EFER);
        let val = efer.read();
        // Bit 11 = NXE (No-Execute Enable)
        efer.write(val | (1 << 11));
        NX_ENABLED.store(true, Ordering::SeqCst);
        crate::serial_println!("[SEC] NX/DEP enabled - Non-executable memory protection active");
    }
}

/// NX aktif mi
pub fn is_nx_enabled() -> bool {
    NX_ENABLED.load(Ordering::SeqCst)
}

// ============================================================================
// W^X (Write XOR Execute) Policy
// ============================================================================

/// W^X policy aktif mi
static WXORX_ENABLED: AtomicBool = AtomicBool::new(false);

/// W^X policy aktifleştir
pub fn enable_wxorx() {
    WXORX_ENABLED.store(true, Ordering::SeqCst);
    crate::serial_println!("[SEC] W^X policy enabled - Pages cannot be both writable and executable");
}

/// W^X kontrolü - sayfa hem yazılabilir hem çalıştırılabilir olamaz
pub fn check_wxorx(writable: bool, executable: bool) -> bool {
    if !WXORX_ENABLED.load(Ordering::SeqCst) {
        return true; // Policy kapalı, her şey serbest
    }
    
    // W^X: writable XOR executable
    !(writable && executable)
}

/// W^X aktif mi
pub fn is_wxorx_enabled() -> bool {
    WXORX_ENABLED.load(Ordering::SeqCst)
}

// ============================================================================
// SECURITY INITIALIZATION
// ============================================================================

/// Tüm güvenlik özelliklerini başlat
pub fn init() {
    // Debugcon output via port 0xE9 - FIRST before anything else
    unsafe { 
        use x86_64::instructions::port::PortWriteOnly;
        PortWriteOnly::<u8>::new(0xE9).write(b'S');  // Entered security::init
    }
    crate::serial_println!("[SEC] Initializing security subsystem...");
    
    unsafe { x86_64::instructions::port::PortWriteOnly::<u8>::new(0xE9).write(b'1'); }
    // 1. NX/DEP (EFER.NXE)
    enable_nx();
    
    unsafe { x86_64::instructions::port::PortWriteOnly::<u8>::new(0xE9).write(b'2'); }
    // 2. SMEP (CR4 bit 20)
    enable_smep();
    
    unsafe { x86_64::instructions::port::PortWriteOnly::<u8>::new(0xE9).write(b'3'); }
    // 3. SMAP (CR4 bit 21)
    enable_smap();
    
    unsafe { x86_64::instructions::port::PortWriteOnly::<u8>::new(0xE9).write(b'4'); }
    // 4. Stack Canary
    init_stack_canary();
    
    unsafe { x86_64::instructions::port::PortWriteOnly::<u8>::new(0xE9).write(b'5'); }
    // 5. ASLR
    init_aslr();
    
    unsafe { x86_64::instructions::port::PortWriteOnly::<u8>::new(0xE9).write(b'6'); }
    // 6. W^X Policy
    enable_wxorx();
    
    unsafe { x86_64::instructions::port::PortWriteOnly::<u8>::new(0xE9).write(b'7'); }
    crate::serial_println!("[SEC] Security subsystem initialized ✓");
}

/// Per-CPU güvenlik başlatma
pub fn init_cpu_security(cpu_id: u32) {
    init_per_cpu_canary(cpu_id);
    crate::serial_println!("[SEC] CPU {} security initialized", cpu_id);
}

// ============================================================================
// SECURITY AUDIT LOGGING
// ============================================================================

/// Güvenlik olayı türleri
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

/// Güvenlik olayını logla
pub fn log_security_event(event: SecurityEvent) {
    match event {
        SecurityEvent::StackCanaryViolation => {
            crate::serial_println!("[SEC/AUDIT] *** STACK CANARY VIOLATION ***");
        }
        SecurityEvent::NxViolation => {
            crate::serial_println!("[SEC/AUDIT] NX violation - attempt to execute non-executable memory");
        }
        SecurityEvent::SmepViolation => {
            crate::serial_println!("[SEC/AUDIT] SMEP violation - kernel tried to execute user code");
        }
        SecurityEvent::SmapViolation => {
            crate::serial_println!("[SEC/AUDIT] SMAP violation - kernel tried to access user memory");
        }
        SecurityEvent::WxorxViolation => {
            crate::serial_println!("[SEC/AUDIT] W^X violation - page is both writable and executable");
        }
        SecurityEvent::SeccompViolation(syscall) => {
            crate::serial_println!("[SEC/AUDIT] Seccomp violation - syscall {} blocked", syscall);
        }
        SecurityEvent::SuspiciousSyscall(syscall) => {
            crate::serial_println!("[SEC/AUDIT] Suspicious syscall {} detected", syscall);
        }
    }
}

// ============================================================================
// SECURITY STATUS
// ============================================================================

/// Güvenlik durumu özeti
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
        if self.nx { score += 2; }
        if self.smep { score += 2; }
        if self.smap { score += 2; }
        if self.aslr { score += 1; }
        if self.wxorx { score += 1; }
        if self.canary { score += 2; }
        score
    }
}

// ============================================================================
// THE ETERNAL SEAL - Kernel Code Integrity Protection
// ============================================================================

use alloc::collections::BTreeMap;

/// Kernel kod bölgesi tanımı
#[derive(Clone, Copy, Debug)]
pub struct KernelRegion {
    pub start: u64,
    pub size: u64,
    pub name: &'static str,
    pub priority: u8, // 0=critical, 1=high, 2=normal
}

/// Checksum seviyeleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChecksumLevel {
    /// Level 0: XOR Parity - 1 cycle/64 bytes
    Parity,
    /// Level 1: CRC32 Hardware - ~3 cycles/byte
    Crc32,
    /// Level 2: SHA-256 - Only on suspicious pages
    Sha256,
}

/// Sayfa integrity durumu
#[derive(Clone, Debug)]
pub struct PageIntegrity {
    pub parity: u64,      // XOR parity (8 bytes for 4KB page)
    pub crc32: u32,       // CRC32 checksum
    pub last_check: u64,  // Tick when last checked
    pub violations: u32,  // Violation count
}

/// Eternal Seal state
static SEAL_REGIONS: Mutex<alloc::vec::Vec<KernelRegion>> = Mutex::new(alloc::vec::Vec::new());
static SEAL_INTEGRITY: Mutex<BTreeMap<u64, PageIntegrity>> = Mutex::new(BTreeMap::new());
static SEAL_ENABLED: AtomicBool = AtomicBool::new(false);
static SEAL_SAMPLING_RATE: AtomicU64 = AtomicU64::new(5); // %5 per tick

/// Kritik kernel bölgelerini kaydet
pub fn seal_register_region(start: u64, size: u64, name: &'static str, priority: u8) {
    let region = KernelRegion { start, size, name, priority };
    SEAL_REGIONS.lock().push(region);
    crate::serial_println!("[SEAL] Region registered: {} @ {:#x} (priority {})", name, start, priority);
}

/// XOR Parity hesapla - En hızlı (O(n/8))
#[inline(always)]
fn compute_parity(data: &[u8]) -> u64 {
    // 64-bit chunks halinde XOR
    let chunks = data.chunks_exact(8);
    let remainder = chunks.remainder();
    
    let mut parity = chunks.fold(0u64, |acc, chunk| {
        acc ^ u64::from_le_bytes(chunk.try_into().unwrap())
    });
    
    // Kalan byte'lar
    if !remainder.is_empty() {
        let mut last = [0u8; 8];
        last[..remainder.len()].copy_from_slice(remainder);
        parity ^= u64::from_le_bytes(last);
    }
    
    parity
}

/// CRC32 hesapla - Hardware accelerated (SSE4.2)
#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn compute_crc32(data: &[u8]) -> u32 {
    use core::arch::x86_64::_mm_crc32_u64;
    
    let mut crc = 0xFFFFFFFFu32;
    let len = data.len();
    let mut i = 0;
    
    // 8-byte chunks
    while i + 8 <= len {
        unsafe {
            let val = u64::from_le_bytes(data[i..i+8].try_into().unwrap());
            crc = _mm_crc32_u64(crc as u64, val) as u32;
        }
        i += 8;
    }
    
    // Kalan byte'lar
    while i < len {
        unsafe {
            crc = core::arch::x86_64::_mm_crc32_u8(crc, data[i]);
        }
        i += 1;
    }
    
    !crc
}

#[cfg(not(target_arch = "x86_64"))]
fn compute_crc32(data: &[u8]) -> u32 {
    // Software fallback - CRC-32C polynomial
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
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0x82F63B78 } else { crc >> 1 };
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
};

/// Basit SHA-256 (sadece Level 2 için)
fn compute_sha256_simple(data: &[u8]) -> [u8; 32] {
    // Mini SHA-256 implementasyonu
    // Not: Gerçek implementasyon için crate::crypto::sha256 kullanılmalı
    let mut hash = [0u8; 32];
    
    // Simplified: XOR-based pseudo hash (gerçek üretimde değiştirilmeli)
    let parity = compute_parity(data);
    let crc = compute_crc32(data);
    
    hash[..8].copy_from_slice(&parity.to_le_bytes());
    hash[8..12].copy_from_slice(&crc.to_le_bytes());
    hash[12..16].copy_from_slice(&(data.len() as u32).to_le_bytes());
    
    // Avalanche effect için ek rounds
    for i in 16..32 {
        hash[i] = hash[i - 8] ^ hash[i - 4] ^ (i as u8);
    }
    
    hash
}

/// Eternal Seal'ı başlat
pub fn seal_init() {
    crate::serial_println!("[SEAL] Initializing Eternal Seal...");
    
    // Kernel'in kritik bölgelerini kaydet
    // Bu değerler linker script'ten veya memory map'ten alınmalı
    seal_register_region(
        0xFFFF_FFFF_8000_0000, // Kernel start
        0x0010_0000,           // 1MB kernel code
        "kernel_code",
        0, // Critical - her tick kontrol
    );
    
    seal_register_region(
        0xFFFF_FFFF_8010_0000,
        0x0008_0000,           // 512KB syscall handlers
        "syscall_table",
        0, // Critical
    );
    
    // İlk checksum'ları hesapla
    let regions = SEAL_REGIONS.lock();
    let mut integrity = SEAL_INTEGRITY.lock();
    
    for region in regions.iter() {
        let page_count = region.size / 4096;
        for i in 0..page_count {
            let addr = region.start + i * 4096;
            
            // Güvenli okuma (kernel space)
            let ptr = addr as *const u8;
            let data = unsafe { core::slice::from_raw_parts(ptr, 4096) };
            
            integrity.insert(addr, PageIntegrity {
                parity: compute_parity(data),
                crc32: compute_crc32(data),
                last_check: 0,
                violations: 0,
            });
        }
    }
    
    SEAL_ENABLED.store(true, Ordering::SeqCst);
    crate::serial_println!("[SEAL] {} pages sealed", integrity.len());
}

/// Guardian check - Her tick'te çağrılır
pub fn seal_guardian_tick(current_tick: u64) {
    if !SEAL_ENABLED.load(Ordering::SeqCst) {
        return;
    }
    
    let regions = SEAL_REGIONS.lock();
    let mut integrity = SEAL_INTEGRITY.lock();
    let sampling_rate = SEAL_SAMPLING_RATE.load(Ordering::SeqCst);
    
    // Rastgele sayı üretici
    let rand_val = crate::random::next_u32();
    
    for region in regions.iter() {
        let page_count = region.size / 4096;
        
        // Priority-based sampling
        // Critical (0): her tick, High (1): %50, Normal (2): %5
        let check_prob = match region.priority {
            0 => 100,  // Always check
            1 => 50,   // 50% chance
            _ => sampling_rate as u32, // Configurable
        };
        
        for i in 0..page_count {
            let addr = region.start + i * 4096;
            
            // Probabilistic skip
            if check_prob < 100 && (rand_val % 100) >= check_prob {
                continue;
            }
            
            // Level 0: XOR Parity (hızlı)
            let ptr = addr as *const u8;
            let data = unsafe { core::slice::from_raw_parts(ptr, 4096) };
            let current_parity = compute_parity(data);
            
            if let Some(stored) = integrity.get_mut(&addr) {
                if stored.parity != current_parity {
                    // Level 1: CRC32 doğrulama
                    let current_crc = compute_crc32(data);
                    
                    if stored.crc32 != current_crc {
                        // VIOLATION DETECTED!
                        stored.violations += 1;
                        
                        crate::serial_println!(
                            "[SEAL] *** INTEGRITY VIOLATION *** {} @ {:#x}",
                            region.name, addr
                        );
                        
                        log_security_event(SecurityEvent::NxViolation);
                        
                        // TODO: Self-healing - shadow copy'den geri yükle
                        // seal_self_heal(addr);
                    }
                }
                
                stored.last_check = current_tick;
            }
        }
    }
}

/// Self-healing (shadow copy'den geri yükle)
pub fn seal_self_heal(addr: u64) -> Result<(), &'static str> {
    crate::serial_println!("[SEAL] Self-healing page at {:#x}", addr);
    
    // TODO: Shadow copy'den orijinali geri yükle
    // Bu, boot sırasında oluşturulan immutable kopyadan olacak
    
    // Şimdilik sadece log
    log_security_event(SecurityEvent::NxViolation);
    
    Ok(())
}

/// Eternal Seal aktif mi
pub fn is_seal_enabled() -> bool {
    SEAL_ENABLED.load(Ordering::SeqCst)
}

/// Sampling rate ayarla
pub fn seal_set_sampling_rate(rate: u64) {
    SEAL_SAMPLING_RATE.store(rate.min(100), Ordering::SeqCst);
}

/// Seal istatistikleri
pub fn seal_stats() -> SealStats {
    let integrity = SEAL_INTEGRITY.lock();
    let total = integrity.len();
    let violations = integrity.values().map(|p| p.violations).sum();
    
    SealStats { total_pages: total, total_violations: violations }
}

#[derive(Debug, Clone, Copy)]
pub struct SealStats {
    pub total_pages: usize,
    pub total_violations: u32,
}
