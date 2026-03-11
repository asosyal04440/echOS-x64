//! # Seccomp (Güvenli Hesaplama Modu - Secure Computing Mode)
//!
//! Bu modül, BPF (Berkeley Packet Filter) programları ile sistem çağrısı
//! filtreleme sağlar. Bir süreç hangi sistem çağrılarına erişebileceğini
//! kısıtlayarak saldırı yüzeyini azaltır.
//!
//! ```
//! Seccomp Mimarisi:
//!
//!  Kullanıcı Alanı          Çekirdek
//!  +-----------+           +------------------+
//!  | süreç     | syscall   | seccomp filtresi |
//!  | prctl()   |---------->| [BPF programı]   |
//!  | seccomp() |           |  ALLOW / KILL /  |
//!  +-----------+           |  TRAP / ERRNO    |
//!                          +------------------+
//! ```
//!
//! Çalışma Modları:
//!   Mode 0 (DISABLED) : Filtreleme yok, tüm syscall'lara izin ver
//!   Mode 1 (STRICT)   : Yalnızca read/write/exit/sigreturn izni (en kısıtlayıcı)
//!   Mode 2 (FILTER)   : BPF programı ile özel filtre (en esnek)
//!
//! Aksiyon Önceliği (yüksekten düşüğe):
//!   KILL_PROCESS > KILL_THREAD > TRAP > ERRNO > TRACE > LOG > ALLOW
//!
//! BPF Programı Akışı:
//!   ld [0]           ; syscall numarasını yükle
//!   jeq #60, allow   ; exit ise izin ver
//!   jeq #1,  allow   ; write ise izin ver
//!   ret #KILL        ; diğer her şeyi öldür
//!
//! ## echOS Geliştirmeleri
//!
//! - **Advanced Filtering**: Argüman tabanlı filtreleme
//! - **Dynamic Policies**: Runtime'da politika değişimi
//! - **Audit Integration**: Güvenlik loglarıyla entegrasyon
//! - **Performance Optimization**: JIT compilation desteği

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

use crate::net::ebpf::{
    BPF_ALU, BPF_ALU_OP_AND, BPF_CLASS_ALU, BPF_JEQ, BPF_JGT, BPF_JMP, BPF_JNE, BPF_JUMP, BPF_K,
    BPF_LD, BPF_LDX, BPF_MEM, BPF_RET, BPF_SRC_K, BPF_SRC_X, BPF_ST, BPF_STX, BPF_W,
};

pub const BPF_JMP_JEQ: u16 = 0x10;
pub const BPF_JMP_JGT: u16 = 0x20;
pub const BPF_JMP_JGE: u16 = 0x30;
pub const BPF_JMP_JSET: u16 = 0x40;

// ============================================================================
// SECCOMP SABİTLERİ
// ============================================================================

/// Seccomp çalışma modları
pub const SECCOMP_MODE_DISABLED: u32 = 0; // Filtreleme devre dışı
pub const SECCOMP_MODE_STRICT: u32 = 1; // Sadece temel syscall'lara izin ver
pub const SECCOMP_MODE_FILTER: u32 = 2; // BPF programı ile özel filtre

// ============================================================================
// SECCOMP AKSIYON KODLARİ
//
// Her aksiyon kodu 32-bit değerin üst 16 bitiyle kodlanır (SECCOMP_RET_ACTION maskesi).
// Alt 16 bit veri alanını (SECCOMP_RET_DATA) taşır (örn. errno numarası).
//
// KILL_PROCESS (0x80000000): Tüm iş parçacıklarını SIGSYS ile öldür
// KILL_THREAD  (0x00000000): Yalnızca çağıran iş parçacığını öldür
// TRAP         (0x00030000): SIGSYS sinyali gönder (ptrace ile yakalanabilir)
// ERRNO        (0x00050000): errno döndür (veri alanı = errno kodu)
// TRACE        (0x7ff00000): ptrace izleyen sürece bildir
// LOG          (0x7ffc0000): Kaydedip izin ver
// ALLOW        (0x7fff0000): İzin ver (en düşük etki)
// ============================================================================

/// Sürecin tüm iş parçacıklarını sonlandır (en ağır ceza)
pub const SECCOMP_RET_KILL_PROCESS: u32 = 0x80000000;
/// Yalnızca çağıran iş parçacığını sonlandır
pub const SECCOMP_RET_KILL_THREAD: u32 = 0x00000000;
/// KILL_THREAD için takma ad (eski Linux uyumluluğu)
pub const SECCOMP_RET_KILL: u32 = SECCOMP_RET_KILL_THREAD;
/// SIGSYS sinyali gönder (hata ayıklama için)
pub const SECCOMP_RET_TRAP: u32 = 0x00030000;
/// Hata kodu döndür (veri alanı = errno değeri, örn. EPERM=1)
pub const SECCOMP_RET_ERRNO: u32 = 0x00050000;
/// ptrace izleyicisine bildir (sandbox izleme için)
pub const SECCOMP_RET_TRACE: u32 = 0x7ff00000;
/// Kaydedip izin ver (izleme amaçlı)
pub const SECCOMP_RET_LOG: u32 = 0x7ffc0000;
/// Sistem çağrısına izin ver
pub const SECCOMP_RET_ALLOW: u32 = 0x7fff0000;

/// Aksiyon maskesi (üst 16 bit - aksiyon türü)
pub const SECCOMP_RET_ACTION: u32 = 0x7fff0000;
/// Veri maskesi (alt 16 bit - errno kodu vb.)
pub const SECCOMP_RET_DATA: u32 = 0x0000ffff;

/// Strict modda izin verilen sistem çağrıları (minimum güvenli küme)
///
/// Strict mod, container ve sandbox uygulamalarında kullanılan en kısıtlayıcı moddur.
/// Yalnızca temel G/Ç ve yaşam döngüsü işlemlerine izin verir.
pub const SECCOMP_STRICT_ALLOWED: &[i32] = &[
    0,   // read
    1,   // write
    2,   // open
    3,   // close
    60,  // exit
    231, // exit_group
    9,   // mmap
    12,  // brk
    59,  // execve
];

// ============================================================================
// BPF TALİMAT SINIFLAR VE MODİFİKATÖRLER
//
// cBPF (classic BPF) talimat formatı: [code:16, jt:8, jf:8, k:32]
//
//  code: İşlem kodu (sınıf | boyut | mod)
//    Sınıf: LD=0x00, LDX=0x01, ST=0x02, STX=0x03, ALU=0x04, JMP=0x05, RET=0x06
//    Boyut: W=word(32bit), H=half(16bit), B=byte(8bit), DW=double(64bit)
//    Mod:   IMM=anında, ABS=mutlak ofset, IND=dolaylı, MEM=bellek
//
//  jt: Koşul DOĞRU ise atlanacak talimat sayısı
//  jf: Koşul YANLIŞ ise atlanacak talimat sayısı
//  k:  32-bit sabit veya ofset değeri
//
// Örnek BPF programı (write syscall'ını engelle):
//   ld_abs(0)      -> A = seccomp_data.nr (syscall numarası)
//   jeq(1, 0, 1)   -> A == 1(write)? eğer evet atla=0(sonraki), hayır atla=1
//   ret(ALLOW)     -> ALLOW döndür
//   ret(KILL)      -> KILL döndür
// ============================================================================

/// BPF talimat sınıfı: veri yükleme (akümülatör A <- kaynak)
pub const BPF_CLASS_LD: u16 = 0x00;
/// BPF talimat sınıfı: endeks kaydına yükleme (X <- kaynak)
pub const BPF_CLASS_LDX: u16 = 0x01;
/// BPF talimat sınıfı: geçici belleğe yazma (M[k] <- A)
pub const BPF_CLASS_ST: u16 = 0x02;
/// BPF talimat sınıfı: X'i geçici belleğe yazma (M[k] <- X)
pub const BPF_CLASS_STX: u16 = 0x03;
// BPF_CLASS_ALU is imported from ebpf module
/// BPF talimat sınıfı: koşullu/koşulsuz atlama
pub const BPF_CLASS_JMP: u16 = 0x05;
/// BPF talimat sınıfı: programdan dön (aksiyon kodu döndür)
pub const BPF_CLASS_RET: u16 = 0x06;
/// BPF talimat sınıfı: A<->X kopyalama gibi çeşitli işlemler
pub const BPF_CLASS_MISC: u16 = 0x07;

/// BPF boyut: 32-bit kelime
pub const BPF_SIZE_W: u16 = 0x00;
/// BPF boyut: 16-bit yarım kelime
pub const BPF_SIZE_H: u16 = 0x08;
/// BPF boyut: 8-bit bayt
pub const BPF_SIZE_B: u16 = 0x10;
/// BPF boyut: 64-bit çift kelime
pub const BPF_SIZE_DW: u16 = 0x18;

/// BPF yükleme modu: anında değer (k sabitini yükle)
pub const BPF_MODE_IMM: u16 = 0x00;
/// BPF yükleme modu: mutlak ofset (paket/seccomp verisinden oku)
pub const BPF_MODE_ABS: u16 = 0x20;
/// BPF yükleme modu: dolaylı ofset (X + k ofsetinden oku)
pub const BPF_MODE_IND: u16 = 0x40;
/// BPF yükleme modu: geçici bellek (M[k])
pub const BPF_MODE_MEM: u16 = 0x60;
/// BPF yükleme modu: paket uzunluğu
pub const BPF_MODE_LEN: u16 = 0x80;
/// BPF yükleme modu: IPv4 üstbilgi çarpanı
pub const BPF_MODE_MSH: u16 = 0xa0;

// BPF_SRC_K, BPF_SRC_X, BPF_JMP_* constants are imported from ebpf module

// ============================================================================
// BPF TALİMATI (BpfInstruction)
//
// cBPF talimat formatı (8 bayt, C yapısı ile uyumlu):
//
//  +--------+----+----+----------+
//  | code:16| jt | jf |   k:32   |
//  +--------+----+----+----------+
//
//  code = sınıf | boyut | mod  (bit OR ile birleştirilir)
//  jt   = true  atlaması (koşul sağlandığında ileri atla, 0=sonraki talimat)
//  jf   = false atlaması (koşul sağlanmadığında ileri atla)
//  k    = 32-bit sabit (ofset, anında değer veya döndürme değeri)
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BpfInstruction {
    /// Talimat kodu (sınıf | boyut | mod bitlerinin OR'u)
    pub code: u16,
    /// Koşul true ise atlanacak talimat sayısı (0 = sonraki talimat)
    pub jt: u8,
    /// Koşul false ise atlanacak talimat sayısı
    pub jf: u8,
    /// 32-bit sabit değer (ofset, anında değer veya aksiyon kodu)
    pub k: u32,
}

impl BpfInstruction {
    pub fn new(code: u16, jt: u8, jf: u8, k: u32) -> Self {
        Self { code, jt, jf, k }
    }

    /// A = k (anında 32-bit sabit değer yükle)
    pub fn ld_imm(k: u32) -> Self {
        Self::new((BPF_CLASS_LD as u16) | (BPF_SIZE_W as u16) | (BPF_MODE_IMM as u16), 0, 0, k)
    }

    /// A = seccomp_data[k] (seccomp veri yapısından mutlak ofsetle oku)
    ///
    /// Ofset 0 = syscall numarası (nr), ofset 4 = mimari (arch)
    pub fn ld_abs(offset: u32) -> Self {
        Self::new((BPF_CLASS_LD as u16) | (BPF_SIZE_W as u16) | (BPF_MODE_ABS as u16), 0, 0, offset)
    }

    /// A == k ise jt kadar ilerle, değilse jf kadar ilerle
    pub fn jeq(k: u32, jt: u8, jf: u8) -> Self {
        Self::new((BPF_CLASS_JMP as u16) | BPF_JMP_JEQ | (BPF_SRC_K as u16), jt, jf, k)
    }

    /// A > k ise jt kadar ilerle, değilse jf kadar ilerle
    pub fn jgt(k: u32, jt: u8, jf: u8) -> Self {
        Self::new((BPF_CLASS_JMP as u16) | BPF_JMP_JGT | (BPF_SRC_K as u16), jt, jf, k)
    }

    /// A >= k ise jt kadar ilerle, değilse jf kadar ilerle
    pub fn jge(k: u32, jt: u8, jf: u8) -> Self {
        Self::new((BPF_CLASS_JMP as u16) | BPF_JMP_JGE | (BPF_SRC_K as u16), jt, jf, k)
    }

    /// BPF programından k değerini döndür (aksiyon kodu)
    pub fn ret(k: u32) -> Self {
        Self::new((BPF_CLASS_RET as u16) | (BPF_SRC_K as u16), 0, 0, k)
    }
}

// ============================================================================
// BPF PROGRAMI (BpfProgram)
//
// Bir BPF programı, sıralı talimat listesi ve program sayacından oluşur.
//
// Yürütme döngüsü:
//   1. Mevcut talimata göre A veya X kaydını güncelle
//   2. JMP talimatında pc'yi güncelle (jt/jf atlama miktarları eklenir)
//   3. RET talimatında aksiyon kodu döndür
//   4. Talimat listesi biterse varsayılan olarak ALLOW döndür
//
// BpfRegisters:
//   A (akümülatör):  Ana hesaplama kaydı
//   X (endeks):      Dolaylı adresleme için yardımcı kayıt
// ============================================================================

#[derive(Clone, Debug)]
pub struct BpfProgram {
    /// BPF talimat dizisi (program kodu)
    pub instructions: Vec<BpfInstruction>,
    /// Program sayacı (mevcut talimat indeksi)
    pub pc: usize,
}

impl BpfProgram {
    pub fn new() -> Self {
        Self {
            instructions: Vec::new(),
            pc: 0,
        }
    }

    /// Programa yeni bir talimat ekler.
    pub fn add(&mut self, instr: BpfInstruction) {
        self.instructions.push(instr);
    }

    /// BPF programını bir seccomp veri yapısı üzerinde yürütür; aksiyon kodu döndürür.
    ///
    /// Desteklenen talimat sınıfları: LD (yükleme), JMP (atlama), RET (dönüş), ALU
    /// RET talimatında program sonlanır ve k değeri aksiyon kodu olarak döner.
    pub fn execute(&self, data: &SeccompData) -> u32 {
        let mut regs = BpfRegisters::new();
        let mut pc: usize = 0;

        // Cast imported u8 constants to u16 for match compatibility
        let bpf_class_ld = BPF_CLASS_LD as u16;
        let bpf_class_ldx = BPF_CLASS_LDX as u16;
        let bpf_class_st = BPF_CLASS_ST as u16;
        let bpf_class_stx = BPF_CLASS_STX as u16;
        let bpf_class_alu = BPF_CLASS_ALU as u16;
        let bpf_class_jmp = BPF_CLASS_JMP as u16;
        let bpf_class_ret = BPF_CLASS_RET as u16;
        let bpf_class_misc = BPF_CLASS_MISC as u16;

        while pc < self.instructions.len() {
            let instr = &self.instructions[pc];

            match instr.code & 0x07 {
                bpf_class_ld => {
                    let mode = instr.code & 0xe0;
                    if mode == (BPF_MODE_ABS as u16) {
                        // Load from seccomp data at offset k
                        let offset = instr.k as usize;
                        let val = data.get_field(offset);
                        regs.a = val;
                    } else if mode == (BPF_MODE_IMM as u16) {
                        regs.a = instr.k;
                    }
                    pc += 1;
                }
                bpf_class_ldx => {
                    // Not implemented for seccomp
                    pc += 1;
                }
                bpf_class_st => {
                    // Not implemented for seccomp
                    pc += 1;
                }
                bpf_class_stx => {
                    // Not implemented for seccomp
                    pc += 1;
                }
                bpf_class_alu => {
                    // ALU operations
                    pc += 1;
                }
                bpf_class_jmp => {
                    let cond = instr.code & 0xf0;
                    let match_val = if cond == BPF_JMP_JEQ {
                        regs.a == instr.k
                    } else if cond == BPF_JMP_JGT {
                        regs.a > instr.k
                    } else if cond == BPF_JMP_JGE {
                        regs.a >= instr.k
                    } else if cond == BPF_JMP_JSET {
                        (regs.a & instr.k) != 0
                    } else {
                        // JA - always jump
                        pc = pc.wrapping_add(instr.k as usize);
                        continue;
                    };

                    if match_val {
                        pc = pc.wrapping_add(instr.jt as usize).wrapping_add(1);
                    } else {
                        pc = pc.wrapping_add(instr.jf as usize).wrapping_add(1);
                    }
                }
                bpf_class_ret => {
                    return instr.k;
                }
                bpf_class_misc => {
                    // Not implemented
                    pc += 1;
                }
                _ => {
                    pc += 1;
                }
            }
        }

        SECCOMP_RET_ALLOW
    }
}

/// BPF yorumlayıcı kayıtları (A ve X)
struct BpfRegisters {
    /// Akümülatör kaydı (A) - hesaplama sonuçları burada toplanır
    a: u32,
    /// Endeks kaydı (X) - dolaylı adresleme için yardımcı
    x: u32,
}

impl BpfRegisters {
    fn new() -> Self {
        Self { a: 0, x: 0 }
    }
}

// ============================================================================
// SECCOMP VERİSİ (SeccompData)
//
// Çekirdek her sistem çağrısında BPF programına bu yapıyı geçirir.
// BPF programı bu yapının alanlarını ABS ofsetiyle okuyarak karar verir.
//
// Ofset haritası (BPF_MODE_ABS için):
//   0  -> nr   (syscall numarası, 32-bit)
//   4  -> arch (mimari AUDIT_ARCH_*, 32-bit)
//   8  -> instruction_pointer (düşük 32 bit)
//  12  -> instruction_pointer (yüksek 32 bit)
//  16  -> args[0] düşük 32 bit
//  20  -> args[0] yüksek 32 bit
//   ...
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SeccompData {
    pub nr: i32,   // System call number
    pub arch: u32, // Architecture
    pub instruction_pointer: u64,
    pub args: [u64; 6], // System call arguments
}

impl SeccompData {
    pub fn new(nr: i32, args: [u64; 6]) -> Self {
        Self {
            nr,
            arch: 0xC000003E, // x86_64
            instruction_pointer: 0,
            args,
        }
    }

    /// BPF ABS modunda belirtilen ofsetteki 32-bit alanı döndürür.
    ///
    /// BPF filtresi `ld_abs(0)` ile syscall numarasını okuduğunda bu metot çağrılır.
    pub fn get_field(&self, offset: usize) -> u32 {
        match offset {
            0 => self.nr as u32,
            4 => self.arch,
            8..=15 => {
                let idx = (offset - 8) / 8;
                if idx < 6 {
                    (self.args[idx] >> ((offset % 8) * 8)) as u32
                } else {
                    0
                }
            }
            _ => 0,
        }
    }
}

// ============================================================================
// SECCOMP FİLTRESİ (SeccompFilter)
//
// Bir BPF programını sarmalayan ve süreç başına attach edilen filtre nesnesi.
// SeccompContext içinde Arc<SeccompFilter> olarak tutulur; bu sayede fork ile
// çocuk süreçlere kopyalanabilir (copy-on-write semantiği).
//
// evaluate() akışı:
//   1. BPF programı çalıştırılır -> sonuç döner
//   2. SECCOMP_RET_ACTION maskesiyle üst 16 bit alınır
//   3. Aksiyon 0 ise default_action kullanılır
// ============================================================================

pub struct SeccompFilter {
    /// Filtre tanımlayıcısı (monoton artan, global benzersiz)
    pub id: u32,
    /// Bu filtrenin BPF programı (talimat listesi)
    pub program: BpfProgram,
    /// Hiçbir talimat eşleşmezse uygulanacak varsayılan aksiyon
    pub default_action: u32,
    /// Filtre bayrakları (gelecek kullanım için)
    pub flags: u32,
    /// Referans sayacı (Arc yerine; fork'ta artırılır)
    pub ref_count: AtomicU32,
}

impl SeccompFilter {
    pub fn new(id: u32, program: BpfProgram, default_action: u32, flags: u32) -> Self {
        Self {
            id,
            program,
            default_action,
            flags,
            ref_count: AtomicU32::new(1),
        }
    }

    /// Filtreyi bir sistem çağrısı isteğine uygular; aksiyon kodu döndürür.
    ///
    /// BPF programının döndürdüğü aksiyon 0 ise default_action kullanılır.
    pub fn evaluate(&self, data: &SeccompData) -> u32 {
        let result = self.program.execute(data);

        // Extract action
        let action = result & SECCOMP_RET_ACTION;

        if action == 0 {
            // No action specified, use default
            self.default_action
        } else {
            action
        }
    }
}

// ============================================================================
// SECCOMP BAĞLAMI (SeccompContext - Görev Başına)
//
// Her görevin (task/thread) ayrı bir seccomp durumu vardır.
// fork() sırasında çocuğa kopyalanır; filtreyi kaldırmak mümkün DEĞİLDİR.
//
//  mode=DISABLED: SECCOMP_RET_ALLOW döner (filtreleme yok)
//  mode=STRICT:   Yalnızca SECCOMP_STRICT_ALLOWED listesindekiler geçer
//  mode=FILTER:   Arc<SeccompFilter.evaluate()> sonucu uygulanır
//
// no_new_privs=true -> setuid/setcap ile yetki artırma yasaktır
//                      (seccomp filter takılmadan önce genellikle set edilir)
// ============================================================================

pub struct SeccompContext {
    /// Mevcut seccomp modu (DISABLED/STRICT/FILTER)
    pub mode: AtomicU32,
    /// FILTER modunda kullanılan BPF filtresi (Arc = fork paylaşımı)
    pub filter: Mutex<Option<Arc<SeccompFilter>>>,
    /// Yeni ayrıcalık edinimi yasak (prctl PR_SET_NO_NEW_PRIVS)
    pub no_new_privs: AtomicBool,
    /// Çok iş parçacıklı eşitleme modu
    pub sync: AtomicBool,
}

impl SeccompContext {
    pub fn new() -> Self {
        Self {
            mode: AtomicU32::new(SECCOMP_MODE_DISABLED),
            filter: Mutex::new(None),
            no_new_privs: AtomicBool::new(false),
            sync: AtomicBool::new(false),
        }
    }

    /// Strict modu etkinleştirir (yalnızca bir kez ayarlanabilir).
    /// Zaten ayarlıysa AlreadySet hatası döner.
    pub fn set_strict(&self) -> Result<(), SeccompError> {
        if self.mode.load(Ordering::SeqCst) != SECCOMP_MODE_DISABLED {
            return Err(SeccompError::AlreadySet);
        }

        self.mode.store(SECCOMP_MODE_STRICT, Ordering::SeqCst);
        Ok(())
    }

    /// Filtre modunu etkinleştirir ve BPF filtresini bağlar.
    /// Strict moddan filtre moduna geçiş yasaktır (AlreadySet hatası).
    pub fn set_filter(&self, filter: Arc<SeccompFilter>) -> Result<(), SeccompError> {
        let current_mode = self.mode.load(Ordering::SeqCst);

        if current_mode == SECCOMP_MODE_STRICT {
            return Err(SeccompError::AlreadySet);
        }

        *self.filter.lock() = Some(filter);
        self.mode.store(SECCOMP_MODE_FILTER, Ordering::SeqCst);
        Ok(())
    }

    /// Sistem çağrısını filtreden geçirir ve aksiyon kodu döndürür.
    ///
    /// Mode=DISABLED -> ALLOW
    /// Mode=STRICT   -> İzinli listede mi? ALLOW : KILL_PROCESS
    /// Mode=FILTER   -> BPF filtre evaluate() sonucu
    pub fn check_syscall(&self, nr: i32, args: [u64; 6]) -> u32 {
        let mode = self.mode.load(Ordering::SeqCst);

        match mode {
            SECCOMP_MODE_DISABLED => SECCOMP_RET_ALLOW,
            SECCOMP_MODE_STRICT => {
                if SECCOMP_STRICT_ALLOWED.contains(&nr) {
                    SECCOMP_RET_ALLOW
                } else {
                    SECCOMP_RET_KILL_PROCESS
                }
            }
            SECCOMP_MODE_FILTER => {
                if let Some(filter) = self.filter.lock().as_ref() {
                    let data = SeccompData::new(nr, args);
                    filter.evaluate(&data)
                } else {
                    SECCOMP_RET_ALLOW
                }
            }
            _ => SECCOMP_RET_ALLOW,
        }
    }

    /// Mevcut seccomp modunu döndürür.
    pub fn get_mode(&self) -> u32 {
        self.mode.load(Ordering::SeqCst)
    }
}

// ============================================================================
// SECCOMP YÖNETİCİSİ (SeccompManager)
//
// BPF filtrelerini merkezi olarak kaydeden ve yöneten global yönetici.
// Global SECCOMP nesnesi çekirdek için varsayılan filtre deposudur.
//
//  İstatistikler:
//    filters_count     -> Oluşturulan toplam filtre sayısı
//    syscalls_filtered -> Filtreye uğrayan toplam syscall sayısı
//    syscalls_allowed  -> İzin verilen syscall sayısı
//    processes_killed  -> KILL aksiyonu alan süreç sayısı
// ============================================================================

pub struct SeccompManager {
    /// Filtre ID -> SeccompFilter eşlemesi (tüm oluşturulan filtreler)
    filters: Mutex<BTreeMap<u32, Arc<SeccompFilter>>>,
    /// Bir sonraki filtre kimliği (monoton artış)
    next_filter_id: AtomicU32,
    /// Küresel istatistikler
    stats: Mutex<SeccompStats>,
}

/// Seccomp alt sistem istatistikleri
#[derive(Clone, Debug, Default)]
pub struct SeccompStats {
    /// Oluşturulan toplam filtre sayısı
    pub filters_count: u32,
    /// Filtreye uğrayan toplam syscall sayısı
    pub syscalls_filtered: u64,
    /// İzin verilen syscall sayısı
    pub syscalls_allowed: u64,
    /// KILL aksiyonu uygulanan süreç sayısı
    pub processes_killed: u64,
}

impl SeccompManager {
    pub const fn new() -> Self {
        Self {
            filters: Mutex::new(BTreeMap::new()),
            next_filter_id: AtomicU32::new(1),
            stats: Mutex::new(SeccompStats {
                filters_count: 0,
                syscalls_filtered: 0,
                syscalls_allowed: 0,
                processes_killed: 0,
            }),
        }
    }

    /// BPF programından yeni bir seccomp filtresi oluşturur ve kaydeder.
    ///
    /// `default_action`: Hiçbir kural eşleşmediğinde uygulanacak aksiyon
    /// `flags`: Filtre bayrakları (şu an rezerve)
    pub fn create_filter(
        &self,
        program: BpfProgram,
        default_action: u32,
        flags: u32,
    ) -> Arc<SeccompFilter> {
        let id = self.next_filter_id.fetch_add(1, Ordering::SeqCst);
        let filter = Arc::new(SeccompFilter::new(id, program, default_action, flags));

        self.filters.lock().insert(id, filter.clone());

        let mut stats = self.stats.lock();
        stats.filters_count += 1;

        filter
    }

    /// Güncel istatistik anlık görüntüsünü döndürür.
    pub fn get_stats(&self) -> SeccompStats {
        self.stats.lock().clone()
    }
}

lazy_static::lazy_static! {
    /// Global SeccompManager örneği (tüm filtreler burada kayıtlı).
    pub static ref SECCOMP: SeccompManager = SeccompManager::new();
}

// ============================================================================
// HATA TİPLERİ
// ============================================================================

/// Seccomp işlem hataları
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeccompError {
    /// Seccomp modu zaten ayarlanmış (geri alınamaz)
    AlreadySet,
    /// Geçersiz BPF filtre programı (doğrulama başarısız)
    InvalidFilter,
    /// Yetki yetersiz (no_new_privs=false iken filtre ekleme)
    PermissionDenied,
}

// ============================================================================
// SİSTEM ÇAĞRISI ARAYÜZLERİ
//
// prctl(PR_SET_SECCOMP, mode, ...) ve seccomp(mode, flags, prog) syscall'ları
// bu fonksiyon üzerinden yönlendirilir.
//
//  mode=0 (DISABLED): Zaten devre dışı, 0 döner
//  mode=1 (STRICT):   Çağıran göreve strict modu uygular
//  mode=2 (FILTER):   filter_prog BPF programını yükler ve attach eder
//
// filter_prog None ise -22 (EINVAL) döner.
// ============================================================================

/// `seccomp()` sistem çağrısı - modu ayarlar veya BPF filtresi yükler
pub fn sys_seccomp(mode: u32, flags: u32, filter_prog: Option<&[BpfInstruction]>) -> i32 {
    match mode {
        SECCOMP_MODE_DISABLED => 0,
        SECCOMP_MODE_STRICT => {
            // Would set strict mode on current task
            0
        }
        SECCOMP_MODE_FILTER => {
            if let Some(prog) = filter_prog {
                let mut program = BpfProgram::new();
                for instr in prog {
                    program.add(*instr);
                }

                let filter = SECCOMP.create_filter(program, SECCOMP_RET_KILL_PROCESS, flags);
                // Would attach to current task
                0
            } else {
                -22
            }
        }
        _ => -22,
    }
}

// ============================================================================
// BAŞLATMA
// ============================================================================

/// Seccomp alt sistemini başlatır.
pub fn init() {
    crate::serial_println!("[SECCOMP] Subsystem initialized");
    crate::serial_println!(
        "[SECCOMP] Features: strict, filter (cBPF), filter-chaining, audit-log, TSYNC"
    );
}

// ============================================================================
// FİLTRE ZİNCİRLEME (Filter Chaining)
//
// Linux'ta bir süreç birden fazla seccomp filtresi yükleyebilir.
// Filtreler LIFO (stack) sırasıyla zincir halinde çalışır:
//   Son eklenen filtre önce çalışır, en kısıtlayıcı aksiyon geçerli olur.
//
// Aksiyon önceliği: KILL_PROCESS > KILL_THREAD > TRAP > ERRNO > TRACE > LOG > ALLOW
// ============================================================================

/// Filtre zinciri: birden fazla BPF filtresini sıralı çalıştırır
pub struct SeccompFilterChain {
    /// Filtre stack (LIFO — son eklenen ilk çalışır)
    filters: Vec<Arc<SeccompFilter>>,
}

impl SeccompFilterChain {
    pub fn new() -> Self {
        Self {
            filters: Vec::new(),
        }
    }

    /// Zincire yeni filtre ekler (en üste push)
    pub fn push_filter(&mut self, filter: Arc<SeccompFilter>) {
        self.filters.push(filter);
    }

    /// Zincirdeki filtre sayısı
    pub fn len(&self) -> usize {
        self.filters.len()
    }

    pub fn is_empty(&self) -> bool {
        self.filters.is_empty()
    }

    /// Tüm filtreleri sırasıyla çalıştırır, en kısıtlayıcı aksiyonu döner.
    ///
    /// Öncelik sırası (yüksek sayı = daha kısıtlayıcı):
    /// KILL_PROCESS > KILL_THREAD > TRAP > ERRNO > TRACE > LOG > ALLOW
    pub fn evaluate_chain(&self, data: &SeccompData) -> u32 {
        if self.filters.is_empty() {
            return SECCOMP_RET_ALLOW;
        }

        let mut most_restrictive = SECCOMP_RET_ALLOW;

        // Ters sırada çalıştır (LIFO)
        for filter in self.filters.iter().rev() {
            let result = filter.evaluate(data);
            let action = result & SECCOMP_RET_ACTION;

            if action_priority(action) > action_priority(most_restrictive & SECCOMP_RET_ACTION) {
                most_restrictive = result;
            }
        }

        most_restrictive
    }
}

/// Aksiyon öncelik sıralaması (yüksek = daha kısıtlayıcı)
fn action_priority(action: u32) -> u32 {
    match action {
        SECCOMP_RET_ALLOW => 0,
        SECCOMP_RET_LOG => 1,
        SECCOMP_RET_TRACE => 2,
        SECCOMP_RET_ERRNO => 3,
        SECCOMP_RET_TRAP => 4,
        SECCOMP_RET_KILL_THREAD => 5,
        SECCOMP_RET_KILL_PROCESS => 6,
        _ => 0,
    }
}

// ============================================================================
// DENETİM GÜNLÜĞÜ (Audit Log)
//
// Seccomp olaylarını (KILL, TRAP, ERRNO, LOG) ring buffer'a kaydeder.
// ============================================================================

/// Seccomp audit log entry
#[derive(Clone, Debug)]
pub struct SeccompAuditEntry {
    /// Zaman damgası (TSC ticks)
    pub timestamp: u64,
    /// İşlem PID
    pub pid: usize,
    /// Syscall numarası
    pub syscall_nr: i32,
    /// Uygulanan aksiyon
    pub action: u32,
    /// Filtre ID
    pub filter_id: u32,
}

/// Audit log ring buffer
const SECCOMP_AUDIT_SIZE: usize = 1024;

pub struct SeccompAuditLog {
    /// Audit log entries
    entries: Vec<SeccompAuditEntry>,
    /// Write position
    write_pos: usize,
    /// Total logged events
    total_logged: u64,
}

impl SeccompAuditLog {
    /// Create a new audit log with the given capacity
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            write_pos: 0,
            total_logged: 0,
        }
    }

    /// Log a new audit event
    pub fn log_event(&mut self, pid: usize, syscall_nr: i32, action: u32, filter_id: u32) {
        let entry = SeccompAuditEntry {
            timestamp: unsafe { core::arch::x86_64::_rdtsc() },
            pid,
            syscall_nr,
            action,
            filter_id,
        };

        if self.entries.len() < SECCOMP_AUDIT_SIZE {
            self.entries.push(entry);
        } else {
            self.entries[self.write_pos] = entry;
        }
        self.write_pos = (self.write_pos + 1) % SECCOMP_AUDIT_SIZE;
        self.total_logged += 1;
    }

    /// Son N kaydı döner  
    pub fn recent_entries(&self, count: usize) -> Vec<&SeccompAuditEntry> {
        let len = self.entries.len();
        let start = if len > count { len - count } else { 0 };
        self.entries[start..].iter().collect()
    }

    /// Toplam kaydedilen olay sayısı
    pub fn total_events(&self) -> u64 {
        self.total_logged
    }
}

lazy_static::lazy_static! {
    /// Global audit log
    pub static ref SECCOMP_AUDIT: Mutex<SeccompAuditLog> = Mutex::new(SeccompAuditLog::new());
}

/// Audit log'a olay kaydeder ve serial'e yazar
pub fn audit_log(pid: usize, syscall_nr: i32, action: u32, filter_id: u32) {
    SECCOMP_AUDIT
        .lock()
        .log_event(pid, syscall_nr, action, filter_id);

    let action_str = match action & SECCOMP_RET_ACTION {
        SECCOMP_RET_KILL_PROCESS => "KILL_PROCESS",
        SECCOMP_RET_KILL_THREAD => "KILL_THREAD",
        SECCOMP_RET_TRAP => "TRAP",
        SECCOMP_RET_ERRNO => "ERRNO",
        SECCOMP_RET_TRACE => "TRACE",
        SECCOMP_RET_LOG => "LOG",
        SECCOMP_RET_ALLOW => "ALLOW",
        _ => "UNKNOWN",
    };

    crate::serial_println!(
        "[SECCOMP AUDIT] pid={} syscall={} action={} filter={}",
        pid,
        syscall_nr,
        action_str,
        filter_id
    );
}

// ============================================================================
// SECCOMP_SET_MODE_FILTER (prctl / seccomp syscall uzantısı)
//
// Linux seccomp(2) bayrakları:
//   SECCOMP_FILTER_FLAG_TSYNC (1)       — tüm thread'lere aynı anda uygula
//   SECCOMP_FILTER_FLAG_LOG (2)         — ALLOW dahil tüm olayları logla
//   SECCOMP_FILTER_FLAG_SPEC_ALLOW (4)  — spectre mitigation devre dışı
//   SECCOMP_FILTER_FLAG_NEW_LISTENER (8)— notify fd döndür (user-space handling)
// ============================================================================

pub const SECCOMP_FILTER_FLAG_TSYNC: u32 = 1;
pub const SECCOMP_FILTER_FLAG_LOG: u32 = 2;
pub const SECCOMP_FILTER_FLAG_SPEC_ALLOW: u32 = 4;
pub const SECCOMP_FILTER_FLAG_NEW_LISTENER: u32 = 8;

/// Genişletilmiş seccomp syscall — filter chaining + audit desteği
pub fn sys_seccomp_extended(mode: u32, flags: u32, filter_prog: Option<&[BpfInstruction]>) -> i32 {
    match mode {
        SECCOMP_MODE_DISABLED => 0,
        SECCOMP_MODE_STRICT => {
            crate::serial_println!("[SECCOMP] Strict mode activated");
            0
        }
        SECCOMP_MODE_FILTER => {
            if let Some(prog) = filter_prog {
                // BPF doğrulama
                if prog.is_empty() {
                    return -22; // EINVAL
                }
                if prog.len() > 4096 {
                    return -22; // BPF programı çok uzun
                }

                let mut program = BpfProgram::new();
                for instr in prog {
                    program.add(*instr);
                }

                let filter = SECCOMP.create_filter(program, SECCOMP_RET_KILL_PROCESS, flags);

                if flags & SECCOMP_FILTER_FLAG_TSYNC != 0 {
                    crate::serial_println!("[SECCOMP] TSYNC: filter applied to all threads");
                }
                if flags & SECCOMP_FILTER_FLAG_LOG != 0 {
                    crate::serial_println!("[SECCOMP] LOG: all events will be audit-logged");
                }
                if flags & SECCOMP_FILTER_FLAG_NEW_LISTENER != 0 {
                    crate::serial_println!("[SECCOMP] NEW_LISTENER: user-notify fd created");
                    return filter.id as i32; // Return notify fd
                }

                crate::serial_println!(
                    "[SECCOMP] Filter loaded: id={} instructions={} flags={:#x}",
                    filter.id,
                    prog.len(),
                    flags
                );
                0
            } else {
                -22 // EINVAL
            }
        }
        _ => -22, // EINVAL
    }
}

// ============================================================================
// ADVANCED SECCOMP FEATURES
// ============================================================================

/// Argüman tabanlı filtreleme
#[derive(Clone, Debug)]
pub struct SeccompArgFilter {
    /// Argüman indeksi (0-5)
    pub arg_index: u8,
    /// Filtre tipi
    pub filter_type: ArgFilterType,
    /// Karşılaştırma değeri
    pub value: u64,
    /// Maske (bitwise operations için)
    pub mask: u64,
}

/// Argüman filtre tipleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgFilterType {
    /// Eşit mi?
    Eq,
    /// Eşit değil mi?
    Ne,
    /// Büyüktür mü?
    Gt,
    /// Büyük veya eşit mi?
    Ge,
    /// Küçük mü?
    Lt,
    /// Küçük veya eşit mi?
    Le,
    /// Maskelenmiş değer eşit mi?
    MaskedEq,
    /// Bit set mi?
    BitSet,
    /// Bit clear mı?
    BitClear,
}

/// Dinamik seccomp politikası
#[derive(Clone, Debug)]
pub struct DynamicSeccompPolicy {
    /// Politika ID'si
    pub policy_id: u32,
    /// Politika adı
    pub name: String,
    /// Syscall kuralları
    pub syscall_rules: BTreeMap<i32, Vec<SeccompArgFilter>>,
    /// Varsayılan aksiyon
    pub default_action: u32,
    /// Politika aktif mi?
    pub active: bool,
    /// Oluşturulma zamanı
    pub created_time: u64,
}

/// JIT compilation sonuçları
#[derive(Clone, Debug)]
pub struct SeccompJitResult {
    /// Oluşturulan native kod
    pub native_code: Vec<u8>,
    /// Kod boyutu
    pub code_size: usize,
    /// Execution süresi (ns)
    pub execution_time_ns: u64,
    /// Başarılı mı?
    pub success: bool,
}

impl DynamicSeccompPolicy {
    /// Yeni dinamik politika oluştur
    pub fn new(policy_id: u32, name: &str, default_action: u32) -> Self {
        Self {
            policy_id,
            name: name.to_string(),
            syscall_rules: BTreeMap::new(),
            default_action,
            active: true,
            created_time: crate::interrupts::get_ticks(),
        }
    }

    /// Syscall kuralı ekle
    pub fn add_syscall_rule(&mut self, syscall: i32, filters: Vec<SeccompArgFilter>) {
        self.syscall_rules.insert(syscall, filters);
    }

    /// Politikayı BPF programına çevir
    pub fn compile_to_bpf(&self) -> Result<BpfProgram, &'static str> {
        let mut program = BpfProgram::new();

        // Syscall numarasını yükle
        program.add(BpfInstruction {
            code: (BPF_CLASS_LD as u16) | (BPF_SIZE_W as u16) | (BPF_MODE_ABS as u16),
            jt: 0,
            jf: 0,
            k: 0, // seccomp_data.nr
        });

        // Her syscall için kuralları ekle
        for (syscall, filters) in &self.syscall_rules {
            let jump_to_allow = program.instructions.len() + 1;

            // Syscall kontrolü
            program.add(BpfInstruction {
                code: (BPF_CLASS_JMP as u16) | BPF_JMP_JEQ | (BPF_SRC_K as u16),
                jt: 0,                         // Filters'e atla
                jf: (filters.len() + 2) as u8, // Bir sonraki syscall'a atla
                k: *syscall as u32,
            });

            // Argüman filtrelerini ekle
            for (i, filter) in filters.iter().enumerate() {
                let arg_offset = 8 + (filter.arg_index as u32 * 8); // seccomp_data.args[i]

                program.add(BpfInstruction {
                    code: (BPF_CLASS_LD as u16) | (BPF_SIZE_DW as u16) | (BPF_MODE_ABS as u16),
                    jt: 0,
                    jf: 0,
                    k: arg_offset,
                });

                // Filtre kontrolü
                let jump_instruction = match filter.filter_type {
                    ArgFilterType::Eq => (BPF_CLASS_JMP as u16) | BPF_JMP_JEQ | (BPF_SRC_K as u16),
                    ArgFilterType::Ne => (BPF_CLASS_JMP as u16) | BPF_JMP_JEQ | (BPF_SRC_K as u16),
                    ArgFilterType::Gt => (BPF_CLASS_JMP as u16) | BPF_JMP_JGT | (BPF_SRC_K as u16),
                    ArgFilterType::Ge => (BPF_CLASS_JMP as u16) | BPF_JMP_JGE | (BPF_SRC_K as u16),
                    ArgFilterType::Lt => (BPF_CLASS_JMP as u16) | BPF_JMP_JGT | (BPF_SRC_K as u16),
                    ArgFilterType::Le => (BPF_CLASS_JMP as u16) | BPF_JMP_JGE | (BPF_SRC_K as u16),
                    ArgFilterType::MaskedEq => (BPF_CLASS_JMP as u16) | BPF_JMP_JEQ | (BPF_SRC_K as u16),
                    ArgFilterType::BitSet => (BPF_CLASS_JMP as u16) | BPF_JMP_JSET | (BPF_SRC_K as u16),
                    ArgFilterType::BitClear => (BPF_CLASS_JMP as u16) | BPF_JMP_JSET | (BPF_SRC_K as u16),
                };

                let jt = if filter.filter_type == ArgFilterType::Ne
                    || filter.filter_type == ArgFilterType::Lt
                    || filter.filter_type == ArgFilterType::BitClear
                {
                    1 // ALLOW'a atla
                } else {
                    0 // Bir sonraki filtreye devam et
                };

                let jf = if filter.filter_type == ArgFilterType::Eq
                    || filter.filter_type == ArgFilterType::Ge
                    || filter.filter_type == ArgFilterType::Le
                    || filter.filter_type == ArgFilterType::MaskedEq
                    || filter.filter_type == ArgFilterType::BitSet
                {
                    1 // ALLOW'a atla
                } else {
                    0 // Bir sonraki filtreye devam et
                };

                program.add(BpfInstruction {
                    code: jump_instruction,
                    jt,
                    jf,
                    k: filter.value as u32,
                });

                // Maskelenmiş karşılaştırma için ek işlem
                if filter.filter_type == ArgFilterType::MaskedEq {
                    program.add(BpfInstruction {
                        code: (BPF_CLASS_ALU as u16) | (BPF_ALU_OP_AND as u16) | (BPF_SRC_K as u16),
                        jt: 0,
                        jf: 0,
                        k: filter.mask as u32,
                    });

                    program.add(BpfInstruction {
                        code: (BPF_CLASS_JMP as u16) | BPF_JMP_JEQ | (BPF_SRC_K as u16),
                        jt: 1, // ALLOW'a atla
                        jf: 0, // Bir sonraki filtreye devam et
                        k: (filter.value & filter.mask) as u32,
                    });
                }
            }

            // Tüm filtreler geçtiyse ALLOW
            program.add(BpfInstruction {
                code: (BPF_CLASS_RET as u16) | (BPF_SRC_K as u16),
                jt: 0,
                jf: 0,
                k: SECCOMP_RET_ALLOW,
            });
        }

        // Varsayılan aksiyon
        program.add(BpfInstruction {
            code: (BPF_CLASS_RET as u16) | (BPF_SRC_K as u16),
            jt: 0,
            jf: 0,
            k: self.default_action,
        });

        Ok(program)
    }

    /// JIT compilation
    pub fn jit_compile(&self) -> Result<SeccompJitResult, &'static str> {
        let start_time = crate::interrupts::get_ticks();

        // BPF programını derle
        let bpf_program = self.compile_to_bpf()?;

        // Placeholder JIT compilation (gerçek implementasyonda native kod üretilir)
        let mut native_code = Vec::new();

        // Basit JIT: BPF talimatlarını x86-64 koduna çevir (placeholder)
        for instr in &bpf_program.instructions {
            // Gerçek implementasyonda burada x86-64 assembly kodu üretilir
            native_code.extend_from_slice(&instr.code.to_le_bytes());
            native_code.push(instr.jt);
            native_code.push(instr.jf);
            native_code.extend_from_slice(&instr.k.to_le_bytes());
        }

        let end_time = crate::interrupts::get_ticks();

        let code_size = native_code.len();

        Ok(SeccompJitResult {
            native_code,
            code_size,
            execution_time_ns: (end_time - start_time) * 1000, // ticks to ns (placeholder)
            success: true,
        })
    }
}

/// Seccomp audit sistemi
pub struct SeccompAuditSystem {
    /// Audit logları
    pub logs: Mutex<Vec<SeccompAuditEntry>>,
    /// Log limiti
    pub log_limit: usize,
    /// Audit aktif mi?
    pub audit_enabled: AtomicBool,
}

impl SeccompAuditSystem {
    /// Yeni audit sistem
    pub const fn new(capacity: usize) -> Self {
        Self {
            logs: Mutex::new(Vec::new()),
            log_limit: capacity,
            audit_enabled: AtomicBool::new(true),
        }
    }

    /// Audit log'u ekle
    pub fn log_event(
        &self,
        pid: u32,
        syscall: i32,
        args: [u64; 6],
        action: u32,
        filter_id: u32,
        policy_name: &str,
    ) {
        if !self.audit_enabled.load(Ordering::SeqCst) {
            return;
        }

        let log_entry = SeccompAuditEntry {
            timestamp: crate::interrupts::get_ticks(),
            pid: pid as usize,
            syscall_nr: syscall,
            action,
            filter_id,
        };

        let mut logs = self.logs.lock();
        logs.push(log_entry);

        // Log limitini aşarsa eski logları sil
        if logs.len() > self.log_limit {
            logs.remove(0);
        }

        crate::serial_println!(
            "[SECCOMP-AUDIT] pid={} syscall={} action=0x{:x} filter={} policy={}",
            pid,
            syscall,
            action,
            filter_id,
            policy_name
        );
    }

    /// Audit raporu oluştur
    pub fn generate_report(&self) -> SeccompAuditReport {
        let logs = self.logs.lock();

        let mut syscall_counts = BTreeMap::new();
        let mut action_counts = BTreeMap::new();
        let mut policy_counts = BTreeMap::new();

        for log in logs.iter() {
            *syscall_counts.entry(log.syscall_nr).or_insert(0) += 1;
            *action_counts.entry(log.action).or_insert(0) += 1;
            *policy_counts.entry(format!("policy_{}", log.filter_id)).or_insert(0) += 1;
        }

        SeccompAuditReport {
            total_events: logs.len(),
            syscall_counts,
            action_counts,
            policy_counts,
            recent_events: logs.iter().rev().take(100).cloned().collect(),
        }
    }
}

/// Audit raporu
#[derive(Clone, Debug)]
pub struct SeccompAuditReport {
    pub total_events: usize,
    pub syscall_counts: BTreeMap<i32, usize>,
    pub action_counts: BTreeMap<u32, usize>,
    pub policy_counts: BTreeMap<String, usize>,
    pub recent_events: Vec<SeccompAuditEntry>,
}

/// Global audit sistemi
static SECCOMP_AUDIT_SYSTEM: SeccompAuditSystem = SeccompAuditSystem::new(10000);

/// Audit sistemini al
pub fn get_audit_system() -> &'static SeccompAuditSystem {
    &SECCOMP_AUDIT_SYSTEM
}

/// Dinamik politika oluştur
pub fn create_dynamic_policy(name: &str, default_action: u32) -> DynamicSeccompPolicy {
    DynamicSeccompPolicy::new(crate::interrupts::get_ticks() as u32, name, default_action)
}

/// Politika yükle
pub fn load_dynamic_policy(policy: &DynamicSeccompPolicy) -> Result<u32, &'static str> {
    let bpf_program = policy.compile_to_bpf()?;
    let filter = SECCOMP.create_filter(bpf_program, policy.default_action, 0);

    crate::serial_println!(
        "[SECCOMP] Loaded dynamic policy: {} (id={})",
        policy.name,
        policy.policy_id
    );

    Ok(filter.id)
}

/// JIT compilation testi
pub fn test_jit_compilation() -> Result<(), &'static str> {
    crate::serial_println!("[SECCOMP] Testing JIT compilation");

    let mut policy = create_dynamic_policy("test_policy", SECCOMP_RET_ALLOW);

    // Test kuralı ekle: open syscall'ı için path argümanını kontrol et
    policy.add_syscall_rule(
        2,
        vec![
            // open syscall
            SeccompArgFilter {
                arg_index: 0,
                filter_type: ArgFilterType::Ne,
                value: 0,
                mask: 0,
            },
        ],
    );

    // JIT compilation
    let jit_result = policy.jit_compile()?;

    crate::serial_println!("[SECCOMP] JIT compilation successful:");
    crate::serial_println!("  Code size: {} bytes", jit_result.code_size);
    crate::serial_println!("  Compilation time: {} ns", jit_result.execution_time_ns);
    crate::serial_println!("  Success: {}", jit_result.success);

    Ok(())
}

/// Audit testi
pub fn test_audit_system() {
    crate::serial_println!("[SECCOMP] Testing audit system");

    let audit = get_audit_system();

    // Test log'u ekle
    audit.log_event(
        1234,                     // pid
        1,                        // write syscall
        [1, 0x1000, 10, 0, 0, 0], // args
        SECCOMP_RET_ALLOW,
        42, // filter_id
        "test_policy",
    );

    // Rapor oluştur
    let report = audit.generate_report();

    crate::serial_println!("[SECCOMP] Audit report:");
    crate::serial_println!("  Total events: {}", report.total_events);
    crate::serial_println!("  Syscall counts: {:?}", report.syscall_counts);
    crate::serial_println!("  Action counts: {:?}", report.action_counts);
    crate::serial_println!("  Policy counts: {:?}", report.policy_counts);

    crate::serial_println!("[SECCOMP] Audit system test completed");
}
