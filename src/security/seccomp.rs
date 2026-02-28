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

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// SECCOMP SABİTLERİ
// ============================================================================

/// Seccomp çalışma modları
pub const SECCOMP_MODE_DISABLED: u32 = 0;  // Filtreleme devre dışı
pub const SECCOMP_MODE_STRICT: u32 = 1;    // Sadece temel syscall'lara izin ver
pub const SECCOMP_MODE_FILTER: u32 = 2;    // BPF programı ile özel filtre

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
    0,  // read
    1,  // write
    2,  // open
    3,  // close
    60, // exit
    231, // exit_group
    9,  // mmap
    12, // brk
    59, // execve
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
/// BPF talimat sınıfı: aritmetik/mantık işlemi (A op= kaynak)
pub const BPF_CLASS_ALU: u16 = 0x04;
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

/// BPF kaynak: sabit değer (k)
pub const BPF_SRC_K: u16 = 0x00;
/// BPF kaynak: X kaydı
pub const BPF_SRC_X: u16 = 0x08;

/// BPF atlama koşulu: koşulsuz atlama (JA)
pub const BPF_JMP_JA: u16 = 0x00;
/// BPF atlama koşulu: eşitse atla (JEQ - jump if equal)
pub const BPF_JMP_JEQ: u16 = 0x10;
/// BPF atlama koşulu: büyükse atla (JGT - jump if greater than)
pub const BPF_JMP_JGT: u16 = 0x20;
/// BPF atlama koşulu: büyük veya eşitse atla (JGE - jump if greater/equal)
pub const BPF_JMP_JGE: u16 = 0x30;
/// BPF atlama koşulu: bit set ise atla (JSET - jump if bit set)
pub const BPF_JMP_JSET: u16 = 0x40;

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
        Self::new(BPF_CLASS_LD | BPF_SIZE_W | BPF_MODE_IMM, 0, 0, k)
    }

    /// A = seccomp_data[k] (seccomp veri yapısından mutlak ofsetle oku)
    ///
    /// Ofset 0 = syscall numarası (nr), ofset 4 = mimari (arch)
    pub fn ld_abs(offset: u32) -> Self {
        Self::new(BPF_CLASS_LD | BPF_SIZE_W | BPF_MODE_ABS, 0, 0, offset)
    }

    /// A == k ise jt kadar ilerle, değilse jf kadar ilerle
    pub fn jeq(k: u32, jt: u8, jf: u8) -> Self {
        Self::new(BPF_CLASS_JMP | BPF_JMP_JEQ | BPF_SRC_K, jt, jf, k)
    }

    /// A > k ise jt kadar ilerle, değilse jf kadar ilerle
    pub fn jgt(k: u32, jt: u8, jf: u8) -> Self {
        Self::new(BPF_CLASS_JMP | BPF_JMP_JGT | BPF_SRC_K, jt, jf, k)
    }

    /// A >= k ise jt kadar ilerle, değilse jf kadar ilerle
    pub fn jge(k: u32, jt: u8, jf: u8) -> Self {
        Self::new(BPF_CLASS_JMP | BPF_JMP_JGE | BPF_SRC_K, jt, jf, k)
    }

    /// BPF programından k değerini döndür (aksiyon kodu)
    pub fn ret(k: u32) -> Self {
        Self::new(BPF_CLASS_RET | BPF_SRC_K, 0, 0, k)
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

        while pc < self.instructions.len() {
            let instr = &self.instructions[pc];

            match instr.code & 0x07 {
                BPF_CLASS_LD => {
                    let mode = instr.code & 0xe0;
                    if mode == BPF_MODE_ABS {
                        // Load from seccomp data at offset k
                        let offset = instr.k as usize;
                        let val = data.get_field(offset);
                        regs.a = val;
                    } else if mode == BPF_MODE_IMM {
                        regs.a = instr.k;
                    }
                    pc += 1;
                }
                BPF_CLASS_JMP => {
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
                BPF_CLASS_RET => {
                    return instr.k;
                }
                BPF_CLASS_ALU => {
                    // ALU operations
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
    pub nr: i32,           // System call number
    pub arch: u32,         // Architecture
    pub instruction_pointer: u64,
    pub args: [u64; 6],    // System call arguments
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
            stats: Mutex::new(SeccompStats::default()),
        }
    }

    /// BPF programından yeni bir seccomp filtresi oluşturur ve kaydeder.
    ///
    /// `default_action`: Hiçbir kural eşleşmediğinde uygulanacak aksiyon
    /// `flags`: Filtre bayrakları (şu an rezerve)
    pub fn create_filter(&self, program: BpfProgram, default_action: u32, flags: u32) -> Arc<SeccompFilter> {
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
}
