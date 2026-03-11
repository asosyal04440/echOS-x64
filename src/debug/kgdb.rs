//! # echOS KGDB — Kernel GDB Remote Serial Protocol Stub
//!
//! GDB uzak hata ayıklama desteği. Seri port (COM1/COM2) üzerinden
//! GDB RSP (Remote Serial Protocol) konuşarak çekirdek seviyesinde
//! hata ayıklama sağlar.
//!
//! ## Mimari
//!
//! ```text
//!  Geliştirici Makinesi              echOS Çekirdeği
//!  ┌─────────────────┐               ┌──────────────────────┐
//!  │ GDB Client      │               │ KGDB Stub            │
//!  │                 │  Serial/TCP    │                      │
//!  │ (gdb) target    │◄─────────────►│ RSP Parser           │
//!  │ remote :1234    │  $g#67        │  ├─ read_registers    │
//!  │                 │  $m addr,len  │  ├─ read_memory       │
//!  │ (gdb) break main│  $Z0,addr    │  ├─ set_breakpoint    │
//!  │ (gdb) continue  │  $c          │  ├─ continue          │
//!  │ (gdb) step      │  $s          │  └─ single_step       │
//!  └─────────────────┘               └──────────────────────┘
//! ```
//!
//! ## GDB RSP Paketi Format
//!
//! ```text
//!  $<payload>#<checksum>
//!    │            │
//!    │            └── 2 hex karakter = modulo 256 toplam
//!    └── Komut + argümanlar (hex-encoded)
//!
//!  Örnek:
//!    $g#67             → tüm register'ları oku
//!    $G<hex>#<cs>      → tüm register'ları yaz
//!    $mAAAA,LL#<cs>    → bellekten oku (adres=AAAA, uzunluk=LL)
//!    $MAAAA,LL:<hex>#  → belleğe yaz
//!    $Z0,AAAA,00#<cs>  → software breakpoint kur (INT3)
//!    $z0,AAAA,00#<cs>  → software breakpoint kaldır
//!    $s#<cs>           → single step (EFLAGS.TF=1)
//!    $c#<cs>           → continue (devam et)
//!    $?#<cs>           → stop reason (neden durdu?)
//! ```
//!
//! ## x86_64 Register Düzeni (GDB Sırası)
//!
//! GDB, x86_64 register'larını belirli bir sırada bekler:
//! RAX, RBX, RCX, RDX, RSI, RDI, RBP, RSP, R8-R15, RIP, EFLAGS, CS-GS
//!
//! ## Kullanım
//!
//! ```text
//! # echOS tarafı (seri port COM2 üzerinden dinle):
//! kgdb::init(KgdbTransport::Serial { port: 0x2F8 });
//!
//! # GDB tarafı:
//! $ gdb vmlinux
//! (gdb) set architecture i386:x86-64
//! (gdb) target remote /dev/ttyS1
//! (gdb) break kernel_main
//! (gdb) continue
//! ```

use core::sync::atomic::{AtomicBool, Ordering};

// ────────────────────────────────────────────────────────────
// Sabitler
// ────────────────────────────────────────────────────────────

/// KGDB aktif mi?
static KGDB_ACTIVE: AtomicBool = AtomicBool::new(false);

/// KGDB bağlantısı kuruldu mu?
static KGDB_CONNECTED: AtomicBool = AtomicBool::new(false);

/// INT3 opcode (software breakpoint)
const INT3_OPCODE: u8 = 0xCC;

/// Maksimum RSP paket boyutu (GDB varsayılanı ~4KB)
const MAX_PACKET_SIZE: usize = 4096;

/// Maksimum breakpoint sayısı
const MAX_BREAKPOINTS: usize = 64;

/// x86_64 register sayısı (GDB sırasıyla)
const NUM_REGISTERS: usize = 24;

// ────────────────────────────────────────────────────────────
// Transport Katmanı
// ────────────────────────────────────────────────────────────

/// KGDB iletişim yolu
#[derive(Clone, Copy, Debug)]
pub enum KgdbTransport {
    /// Seri port (COM1=0x3F8, COM2=0x2F8)
    Serial { port: u16 },
    /// Meşgul bekleme modu (test için)
    None,
}

/// Mevcut transport
static mut TRANSPORT: KgdbTransport = KgdbTransport::None;

// ────────────────────────────────────────────────────────────
// Register Durumu
// ────────────────────────────────────────────────────────────

/// x86_64 register seti — GDB RSP sırasıyla
///
/// GDB x86_64 register düzeni:
///   0:  RAX   1: RBX   2: RCX   3: RDX
///   4:  RSI   5: RDI   6: RBP   7: RSP
///   8:  R8    9: R9   10: R10  11: R11
///  12:  R12  13: R13  14: R14  15: R15
///  16:  RIP  17: EFLAGS
///  18:  CS   19: SS   20: DS   21: ES  22: FS  23: GS
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct KgdbRegisters {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub eflags: u64,
    pub cs: u64,
    pub ss: u64,
    pub ds: u64,
    pub es: u64,
    pub fs: u64,
    pub gs: u64,
}

impl KgdbRegisters {
    /// Register'ları GDB hex formatına serialize et (her register 16 hex karakter)
    pub fn to_hex(&self, buf: &mut [u8]) -> usize {
        let regs = [
            self.rax,
            self.rbx,
            self.rcx,
            self.rdx,
            self.rsi,
            self.rdi,
            self.rbp,
            self.rsp,
            self.r8,
            self.r9,
            self.r10,
            self.r11,
            self.r12,
            self.r13,
            self.r14,
            self.r15,
            self.rip,
            self.eflags,
            self.cs,
            self.ss,
            self.ds,
            self.es,
            self.fs,
            self.gs,
        ];

        let mut pos = 0;
        for &reg in &regs {
            // x86_64 little-endian: düşük byte önce
            let bytes = reg.to_le_bytes();
            for &b in &bytes {
                if pos + 2 <= buf.len() {
                    buf[pos] = hex_nibble(b >> 4);
                    buf[pos + 1] = hex_nibble(b & 0x0F);
                    pos += 2;
                }
            }
        }
        pos
    }

    /// GDB hex formatından register'ları deserialize et
    pub fn from_hex(&mut self, hex: &[u8]) {
        let mut regs = [0u64; NUM_REGISTERS];
        for (i, reg) in regs.iter_mut().enumerate() {
            let start = i * 16; // 8 bytes * 2 hex chars
            if start + 16 <= hex.len() {
                let mut bytes = [0u8; 8];
                for j in 0..8 {
                    let hi = unhex(hex[start + j * 2]);
                    let lo = unhex(hex[start + j * 2 + 1]);
                    bytes[j] = (hi << 4) | lo;
                }
                *reg = u64::from_le_bytes(bytes);
            }
        }

        self.rax = regs[0];
        self.rbx = regs[1];
        self.rcx = regs[2];
        self.rdx = regs[3];
        self.rsi = regs[4];
        self.rdi = regs[5];
        self.rbp = regs[6];
        self.rsp = regs[7];
        self.r8 = regs[8];
        self.r9 = regs[9];
        self.r10 = regs[10];
        self.r11 = regs[11];
        self.r12 = regs[12];
        self.r13 = regs[13];
        self.r14 = regs[14];
        self.r15 = regs[15];
        self.rip = regs[16];
        self.eflags = regs[17];
        self.cs = regs[18];
        self.ss = regs[19];
        self.ds = regs[20];
        self.es = regs[21];
        self.fs = regs[22];
        self.gs = regs[23];
    }
}

// ────────────────────────────────────────────────────────────
// Breakpoint Yönetimi
// ────────────────────────────────────────────────────────────

/// Software breakpoint kaydı
#[derive(Clone, Copy, Debug)]
struct Breakpoint {
    /// Breakpoint adresi
    addr: u64,
    /// Orijinal opcode (INT3 ile değiştirilen byte)
    original_byte: u8,
    /// Aktif mi?
    active: bool,
}

/// Global breakpoint tablosu
static mut BREAKPOINTS: [Breakpoint; MAX_BREAKPOINTS] = [Breakpoint {
    addr: 0,
    original_byte: 0,
    active: false,
}; MAX_BREAKPOINTS];

/// Global register durumu (KGDB trap sırasında yakalanan)
static mut SAVED_REGS: KgdbRegisters = KgdbRegisters {
    rax: 0,
    rbx: 0,
    rcx: 0,
    rdx: 0,
    rsi: 0,
    rdi: 0,
    rbp: 0,
    rsp: 0,
    r8: 0,
    r9: 0,
    r10: 0,
    r11: 0,
    r12: 0,
    r13: 0,
    r14: 0,
    r15: 0,
    rip: 0,
    eflags: 0,
    cs: 0,
    ss: 0,
    ds: 0,
    es: 0,
    fs: 0,
    gs: 0,
};

// ────────────────────────────────────────────────────────────
// RSP Parser
// ────────────────────────────────────────────────────────────

/// RSP paket parse durumu
#[derive(Debug, PartialEq)]
enum RspState {
    /// '$' bekliyor
    WaitStart,
    /// Payload okunuyor
    ReadPayload,
    /// '#' sonrası ilk checksum hex karakteri bekleniyor
    ReadChecksum1,
    /// İkinci checksum hex karakteri bekleniyor
    ReadChecksum2,
}

/// Parse edilmiş RSP paketi
struct RspPacket {
    payload: [u8; MAX_PACKET_SIZE],
    len: usize,
}

impl RspPacket {
    fn new() -> Self {
        RspPacket {
            payload: [0u8; MAX_PACKET_SIZE],
            len: 0,
        }
    }
}

/// Gelen bayt akışından bir RSP paketi parse et
fn parse_rsp_packet(data: &[u8]) -> Option<(RspPacket, usize)> {
    let mut state = RspState::WaitStart;
    let mut packet = RspPacket::new();
    let mut checksum: u8 = 0;
    let mut expected_cs: u8 = 0;
    let mut consumed = 0;

    for (i, &byte) in data.iter().enumerate() {
        match state {
            RspState::WaitStart => {
                if byte == b'$' {
                    state = RspState::ReadPayload;
                    checksum = 0;
                    packet.len = 0;
                } else if byte == b'+' || byte == b'-' {
                    // ACK/NACK — atla
                    continue;
                }
            }
            RspState::ReadPayload => {
                if byte == b'#' {
                    state = RspState::ReadChecksum1;
                } else if packet.len < MAX_PACKET_SIZE {
                    packet.payload[packet.len] = byte;
                    packet.len += 1;
                    checksum = checksum.wrapping_add(byte);
                }
            }
            RspState::ReadChecksum1 => {
                expected_cs = unhex(byte) << 4;
                state = RspState::ReadChecksum2;
            }
            RspState::ReadChecksum2 => {
                expected_cs |= unhex(byte);
                consumed = i + 1;

                if checksum == expected_cs {
                    return Some((packet, consumed));
                } else {
                    // Checksum hatası — paketi at
                    return None;
                }
            }
        }
    }

    None // Eksik paket
}

// ────────────────────────────────────────────────────────────
// Komut İşleme
// ────────────────────────────────────────────────────────────

/// RSP komutu işle ve yanıt üret
///
/// Desteklenen komutlar:
///   g            — Register'ları oku
///   G<hex>       — Register'ları yaz
///   m<addr>,<len> — Bellekten oku
///   M<addr>,<len>:<hex> — Belleğe yaz
///   Z0,<addr>,<kind> — Software breakpoint kur
///   z0,<addr>,<kind> — Software breakpoint kaldır
///   Z1,<addr>,<kind> — Hardware breakpoint kur (DR0-DR3)
///   s            — Single step (EFLAGS.TF=1)
///   c            — Continue
///   ?            — Stop reason
///   qSupported   — GDB özellik sorgusu
///   qAttached    — Bağlantı durumu
fn handle_command(packet: &RspPacket, response: &mut [u8]) -> usize {
    if packet.len == 0 {
        return build_response(response, b"");
    }

    let cmd = packet.payload[0];
    let args = &packet.payload[1..packet.len];

    match cmd {
        // '?' — Neden durdu? (SIGTRAP = breakpoint/single-step)
        b'?' => build_response(response, b"S05"),

        // 'g' — Tüm register'ları oku
        b'g' => {
            let regs = unsafe { &SAVED_REGS };
            let mut hex_buf = [0u8; NUM_REGISTERS * 16];
            let len = regs.to_hex(&mut hex_buf);
            build_response(response, &hex_buf[..len])
        }

        // 'G' — Tüm register'ları yaz
        b'G' => {
            unsafe { SAVED_REGS.from_hex(args) };
            build_response(response, b"OK")
        }

        // 'm' — Bellekten oku: m<addr>,<length>
        b'm' => {
            if let Some((addr, len)) = parse_addr_len(args) {
                let mut hex_buf = [0u8; MAX_PACKET_SIZE];
                let mut pos = 0;
                for i in 0..len {
                    let byte = unsafe { *((addr + i as u64) as *const u8) };
                    if pos + 2 <= hex_buf.len() {
                        hex_buf[pos] = hex_nibble(byte >> 4);
                        hex_buf[pos + 1] = hex_nibble(byte & 0x0F);
                        pos += 2;
                    }
                }
                build_response(response, &hex_buf[..pos])
            } else {
                build_response(response, b"E01")
            }
        }

        // 'M' — Belleğe yaz: M<addr>,<length>:<hex>
        b'M' => {
            if let Some((addr, len, hex_data)) = parse_addr_len_data(args) {
                for i in 0..len {
                    let hi = unhex(hex_data[i * 2]);
                    let lo = unhex(hex_data[i * 2 + 1]);
                    unsafe {
                        *((addr + i as u64) as *mut u8) = (hi << 4) | lo;
                    }
                }
                build_response(response, b"OK")
            } else {
                build_response(response, b"E02")
            }
        }

        // 'Z0' — Software breakpoint kur
        b'Z' if args.first() == Some(&b'0') => {
            if let Some((addr, _kind)) = parse_addr_len(&args[2..]) {
                if set_sw_breakpoint(addr) {
                    build_response(response, b"OK")
                } else {
                    build_response(response, b"E03")
                }
            } else {
                build_response(response, b"E01")
            }
        }

        // 'z0' — Software breakpoint kaldır
        b'z' if args.first() == Some(&b'0') => {
            if let Some((addr, _kind)) = parse_addr_len(&args[2..]) {
                if clear_sw_breakpoint(addr) {
                    build_response(response, b"OK")
                } else {
                    build_response(response, b"E04")
                }
            } else {
                build_response(response, b"E01")
            }
        }

        // 'Z1' — Hardware breakpoint kur (DR0-DR3)
        b'Z' if args.first() == Some(&b'1') => {
            if let Some((addr, _kind)) = parse_addr_len(&args[2..]) {
                if set_hw_breakpoint(addr) {
                    build_response(response, b"OK")
                } else {
                    build_response(response, b"E05")
                }
            } else {
                build_response(response, b"E01")
            }
        }

        // 'z1' — Hardware breakpoint kaldır
        b'z' if args.first() == Some(&b'1') => {
            if let Some((addr, _kind)) = parse_addr_len(&args[2..]) {
                clear_hw_breakpoint(addr);
                build_response(response, b"OK")
            } else {
                build_response(response, b"E01")
            }
        }

        // 'Z2' — Write watchpoint kur (DR0-DR3, RW=01)
        b'Z' if args.first() == Some(&b'2') => {
            if let Some((addr, kind)) = parse_addr_len(&args[2..]) {
                if set_hw_watchpoint(addr, kind, WatchpointType::Write) {
                    build_response(response, b"OK")
                } else {
                    build_response(response, b"E05")
                }
            } else {
                build_response(response, b"E01")
            }
        }

        // 'z2' — Write watchpoint kaldır
        b'z' if args.first() == Some(&b'2') => {
            if let Some((addr, _kind)) = parse_addr_len(&args[2..]) {
                clear_hw_breakpoint(addr);
                build_response(response, b"OK")
            } else {
                build_response(response, b"E01")
            }
        }

        // 'Z3' — Read watchpoint kur (DR0-DR3, RW=11 x86'da read-only yok, access kullanılır)
        b'Z' if args.first() == Some(&b'3') => {
            if let Some((addr, kind)) = parse_addr_len(&args[2..]) {
                // x86 read-only watchpoint desteklemez; read/write (access) kullanılır
                if set_hw_watchpoint(addr, kind, WatchpointType::ReadWrite) {
                    build_response(response, b"OK")
                } else {
                    build_response(response, b"E05")
                }
            } else {
                build_response(response, b"E01")
            }
        }

        // 'z3' — Read watchpoint kaldır
        b'z' if args.first() == Some(&b'3') => {
            if let Some((addr, _kind)) = parse_addr_len(&args[2..]) {
                clear_hw_breakpoint(addr);
                build_response(response, b"OK")
            } else {
                build_response(response, b"E01")
            }
        }

        // 'Z4' — Access watchpoint kur (DR0-DR3, RW=11)
        b'Z' if args.first() == Some(&b'4') => {
            if let Some((addr, kind)) = parse_addr_len(&args[2..]) {
                if set_hw_watchpoint(addr, kind, WatchpointType::ReadWrite) {
                    build_response(response, b"OK")
                } else {
                    build_response(response, b"E05")
                }
            } else {
                build_response(response, b"E01")
            }
        }

        // 'z4' — Access watchpoint kaldır
        b'z' if args.first() == Some(&b'4') => {
            if let Some((addr, _kind)) = parse_addr_len(&args[2..]) {
                clear_hw_breakpoint(addr);
                build_response(response, b"OK")
            } else {
                build_response(response, b"E01")
            }
        }

        // 's' — Single step (EFLAGS.TF=1 ile devam)
        b's' => {
            unsafe {
                // TF (Trap Flag) bit 8'i set et
                SAVED_REGS.eflags |= 1 << 8;
            }
            build_response(response, b"S05")
        }

        // 'c' — Continue (devam et)
        b'c' => {
            unsafe {
                // TF'yi temizle
                SAVED_REGS.eflags &= !(1 << 8);
            }
            // Döngüden çık (resume execution)
            0
        }

        // 'qSupported' — GDB özellik sorgusu
        b'q' => {
            if starts_with(args, b"Supported") {
                build_response(response, b"PacketSize=1000;swbreak+;hwbreak+;watchpoint+")
            } else if starts_with(args, b"Attached") {
                build_response(response, b"1")
            } else if starts_with(args, b"TStatus") {
                build_response(response, b"")
            } else if starts_with(args, b"fThreadInfo") {
                build_response(response, b"m1")
            } else if starts_with(args, b"sThreadInfo") {
                build_response(response, b"l")
            } else if starts_with(args, b"C") {
                build_response(response, b"QC1")
            } else {
                build_response(response, b"")
            }
        }

        // 'H' — Thread seçimi (tek çekirdek: her zaman başarılı)
        b'H' => build_response(response, b"OK"),

        // Bilinmeyen komut
        _ => build_response(response, b""),
    }
}

// ────────────────────────────────────────────────────────────
// Breakpoint Operasyonları
// ────────────────────────────────────────────────────────────

/// Software breakpoint kur: hedef adresteki byte'ı INT3 (0xCC) ile değiştir
fn set_sw_breakpoint(addr: u64) -> bool {
    unsafe {
        for bp in BREAKPOINTS.iter_mut() {
            if !bp.active {
                bp.addr = addr;
                bp.original_byte = *(addr as *const u8);
                bp.active = true;
                // INT3 yaz
                *(addr as *mut u8) = INT3_OPCODE;
                return true;
            }
        }
    }
    false // Tablo dolu
}

/// Software breakpoint kaldır: orijinal byte'ı geri yükle
fn clear_sw_breakpoint(addr: u64) -> bool {
    unsafe {
        for bp in BREAKPOINTS.iter_mut() {
            if bp.active && bp.addr == addr {
                // Orijinal byte'ı geri yaz
                *(addr as *mut u8) = bp.original_byte;
                bp.active = false;
                return true;
            }
        }
    }
    false
}

/// x86_64 Debug Register (DR0-DR3) ile hardware breakpoint kur
fn set_hw_breakpoint(addr: u64) -> bool {
    unsafe {
        // DR7 oku
        let mut dr7: u64;
        core::arch::asm!("mov {}, dr7", out(reg) dr7);

        // Boş DR slot bul (DR0-DR3)
        for i in 0..4u64 {
            let enable_bit = 1 << (i * 2); // Local enable bit
            if (dr7 & enable_bit) == 0 {
                // DR[i]'e adresi yaz
                match i {
                    0 => core::arch::asm!("mov dr0, {}", in(reg) addr),
                    1 => core::arch::asm!("mov dr1, {}", in(reg) addr),
                    2 => core::arch::asm!("mov dr2, {}", in(reg) addr),
                    3 => core::arch::asm!("mov dr3, {}", in(reg) addr),
                    _ => unreachable!(),
                }

                // DR7: local enable + execution breakpoint (RW=00, LEN=00)
                dr7 |= enable_bit;
                // RW bits (condition): 00 = execution only
                let rw_shift = 16 + i * 4;
                dr7 &= !(0b11 << rw_shift); // RW = 00 (execute)
                let len_shift = 18 + i * 4;
                dr7 &= !(0b11 << len_shift); // LEN = 00 (1 byte)

                core::arch::asm!("mov dr7, {}", in(reg) dr7);
                return true;
            }
        }
    }
    false // Tüm DR slotları dolu
}

/// Hardware breakpoint kaldır
fn clear_hw_breakpoint(addr: u64) {
    unsafe {
        let mut dr7: u64;
        core::arch::asm!("mov {}, dr7", out(reg) dr7);

        for i in 0..4u64 {
            let dr_val: u64 = match i {
                0 => {
                    let v: u64;
                    core::arch::asm!("mov {}, dr0", out(reg) v);
                    v
                }
                1 => {
                    let v: u64;
                    core::arch::asm!("mov {}, dr1", out(reg) v);
                    v
                }
                2 => {
                    let v: u64;
                    core::arch::asm!("mov {}, dr2", out(reg) v);
                    v
                }
                3 => {
                    let v: u64;
                    core::arch::asm!("mov {}, dr3", out(reg) v);
                    v
                }
                _ => unreachable!(),
            };

            if dr_val == addr {
                let enable_bit = 1 << (i * 2);
                dr7 &= !enable_bit; // Local enable kapat
                core::arch::asm!("mov dr7, {}", in(reg) dr7);
                return;
            }
        }
    }
}

// ────────────────────────────────────────────────────────────
// Watchpoint (Z2/Z3/Z4) Desteği
// ────────────────────────────────────────────────────────────

/// Watchpoint türü: DR7'deki RW bitlerini belirler
///
/// x86_64 Debug Register RW alanları:
///   00 = Execution (Z1 — hardware breakpoint)
///   01 = Write only (Z2 — write watchpoint)
///   10 = I/O read/write (kullanılmıyor)
///   11 = Read/Write data (Z3/Z4 — read/access watchpoint)
///
/// NOT: x86 mimarisinde "read-only" watchpoint yoktur.
/// Z3 (read watchpoint) GDB'de desteklenmesine rağmen, x86 donanımı
/// bunu RW=11 (read/write) olarak uygular.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WatchpointType {
    /// Sadece yazma (RW=01) — Z2
    Write,
    /// Okuma veya yazma (RW=11) — Z3/Z4
    ReadWrite,
}

/// GDB 'kind' değerinden LEN bitlerini hesaplar.
///
/// GDB kind parametresi izlenen byte sayısını belirtir:
///   1 → LEN=00 (1 byte)
///   2 → LEN=01 (2 byte)
///   4 → LEN=11 (4 byte)
///   8 → LEN=10 (8 byte, x86_64 uzantısı)
fn watchpoint_len_bits(kind: usize) -> u64 {
    match kind {
        1 => 0b00,
        2 => 0b01,
        4 => 0b11,
        8 => 0b10, // x86_64 uzantısı
        _ => 0b00, // Varsayılan: 1 byte
    }
}

/// Hardware watchpoint kur (DR0-DR3).
///
/// # Parametreler
/// - `addr`: İzlenecek bellek adresi
/// - `kind`: İzlenecek byte sayısı (1, 2, 4, 8)
/// - `wp_type`: Write (Z2) veya ReadWrite (Z3/Z4)
fn set_hw_watchpoint(addr: u64, kind: usize, wp_type: WatchpointType) -> bool {
    unsafe {
        let mut dr7: u64;
        core::arch::asm!("mov {}, dr7", out(reg) dr7);

        // Boş DR slot bul (DR0-DR3)
        for i in 0..4u64 {
            let enable_bit = 1u64 << (i * 2); // Local enable bit
            if (dr7 & enable_bit) == 0 {
                // DR[i]'e adresi yaz
                match i {
                    0 => core::arch::asm!("mov dr0, {}", in(reg) addr),
                    1 => core::arch::asm!("mov dr1, {}", in(reg) addr),
                    2 => core::arch::asm!("mov dr2, {}", in(reg) addr),
                    3 => core::arch::asm!("mov dr3, {}", in(reg) addr),
                    _ => unreachable!(),
                }

                // DR7 yapılandır
                dr7 |= enable_bit; // Local enable

                // RW bits: Write=01, ReadWrite=11
                let rw_shift = 16 + i * 4;
                dr7 &= !(0b11 << rw_shift); // Önce temizle
                match wp_type {
                    WatchpointType::Write => dr7 |= 0b01 << rw_shift,
                    WatchpointType::ReadWrite => dr7 |= 0b11 << rw_shift,
                }

                // LEN bits: kind'a göre ayarla
                let len_shift = 18 + i * 4;
                dr7 &= !(0b11 << len_shift); // Önce temizle
                dr7 |= watchpoint_len_bits(kind) << len_shift;

                core::arch::asm!("mov dr7, {}", in(reg) dr7);

                crate::serial_println!(
                    "[KGDB] Watchpoint set: DR{} addr={:#x} type={:?} len={}",
                    i,
                    addr,
                    wp_type,
                    kind
                );
                return true;
            }
        }
    }
    false // Tüm DR slotları dolu
}

// ────────────────────────────────────────────────────────────
// Seri Port I/O
// ────────────────────────────────────────────────────────────

/// Seri porttan bir byte oku (blocking)
fn serial_read_byte(port: u16) -> u8 {
    unsafe {
        // Line Status Register (port + 5): bit 0 = Data Ready
        loop {
            let lsr: u8;
            core::arch::asm!("in al, dx", out("al") lsr, in("dx") port + 5);
            if lsr & 1 != 0 {
                break;
            }
            core::hint::spin_loop();
        }
        let byte: u8;
        core::arch::asm!("in al, dx", out("al") byte, in("dx") port);
        byte
    }
}

/// Seri porta bir byte yaz (blocking)
fn serial_write_byte(port: u16, byte: u8) {
    unsafe {
        // Line Status Register (port + 5): bit 5 = Transmitter Holding Register Empty
        loop {
            let lsr: u8;
            core::arch::asm!("in al, dx", out("al") lsr, in("dx") port + 5);
            if lsr & 0x20 != 0 {
                break;
            }
            core::hint::spin_loop();
        }
        core::arch::asm!("out dx, al", in("al") byte, in("dx") port);
    }
}

/// Seri porta veri bloğu yaz
fn serial_write(port: u16, data: &[u8]) {
    for &byte in data {
        serial_write_byte(port, byte);
    }
}

// ────────────────────────────────────────────────────────────
// RSP Yanıt Oluşturma
// ────────────────────────────────────────────────────────────

/// '$' + payload + '#' + checksum formatında RSP yanıtı oluştur
fn build_response(buf: &mut [u8], payload: &[u8]) -> usize {
    let mut pos = 0;
    if pos < buf.len() {
        buf[pos] = b'$';
        pos += 1;
    }

    let mut checksum: u8 = 0;
    for &byte in payload {
        if pos < buf.len() {
            buf[pos] = byte;
            pos += 1;
            checksum = checksum.wrapping_add(byte);
        }
    }

    if pos < buf.len() {
        buf[pos] = b'#';
        pos += 1;
    }
    if pos < buf.len() {
        buf[pos] = hex_nibble(checksum >> 4);
        pos += 1;
    }
    if pos < buf.len() {
        buf[pos] = hex_nibble(checksum & 0x0F);
        pos += 1;
    }

    pos
}

// ────────────────────────────────────────────────────────────
// Hex Yardımcıları
// ────────────────────────────────────────────────────────────

/// 4-bit nibble'ı hex ASCII karakterine çevir
fn hex_nibble(n: u8) -> u8 {
    let n = n & 0x0F;
    if n < 10 {
        b'0' + n
    } else {
        b'a' + n - 10
    }
}

/// Hex ASCII karakterini 4-bit değere çevir
fn unhex(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    }
}

/// Hex string'i u64'e çevir
fn hex_to_u64(hex: &[u8]) -> u64 {
    let mut result: u64 = 0;
    for &c in hex {
        result = result.wrapping_shl(4) | unhex(c) as u64;
    }
    result
}

/// "addr,len" formatını parse et (hex olarak)
fn parse_addr_len(data: &[u8]) -> Option<(u64, usize)> {
    let mut comma_pos = None;
    for (i, &b) in data.iter().enumerate() {
        if b == b',' {
            comma_pos = Some(i);
            break;
        }
    }

    let comma = comma_pos?;
    let addr = hex_to_u64(&data[..comma]);
    let len = hex_to_u64(&data[comma + 1..]) as usize;
    Some((addr, len))
}

/// "addr,len:hex_data" formatını parse et
fn parse_addr_len_data(data: &[u8]) -> Option<(u64, usize, &[u8])> {
    let mut comma_pos = None;
    let mut colon_pos = None;
    for (i, &b) in data.iter().enumerate() {
        if b == b',' && comma_pos.is_none() {
            comma_pos = Some(i);
        }
        if b == b':' {
            colon_pos = Some(i);
            break;
        }
    }

    let comma = comma_pos?;
    let colon = colon_pos?;
    let addr = hex_to_u64(&data[..comma]);
    let len = hex_to_u64(&data[comma + 1..colon]) as usize;
    let hex_data = &data[colon + 1..];
    Some((addr, len, hex_data))
}

/// Başlangıç kontrolü
fn starts_with(data: &[u8], prefix: &[u8]) -> bool {
    data.len() >= prefix.len() && &data[..prefix.len()] == prefix
}

// ────────────────────────────────────────────────────────────
// Public API
// ────────────────────────────────────────────────────────────

/// KGDB subsistemini başlat
///
/// Transport katmanını seçer ve seri portu dinlemeye hazırlar.
/// Breakpoint/trap handler'ları IDT'ye kayıt edilmeli (INT3, #DB).
pub fn init(transport: KgdbTransport) {
    if !crate::security::anti_cheat::enforce_debug_attach("kgdb") {
        crate::serial_println!("[KGDB] debug attach denied by anti-cheat policy");
        return;
    }
    unsafe {
        TRANSPORT = transport;
    }
    KGDB_ACTIVE.store(true, Ordering::Release);

    crate::serial_println!("[KGDB] Başlatıldı: {:?}", transport);

    // Seri port baud rate ayarı (115200)
    if let KgdbTransport::Serial { port } = transport {
        init_serial_port(port);
    }
}

/// Seri portu KGDB için yapılandır (115200 baud, 8N1)
fn init_serial_port(port: u16) {
    unsafe {
        // Interrupt'ları kapat
        core::arch::asm!("out dx, al", in("al") 0u8, in("dx") port + 1);
        // DLAB set (baud rate ayarı)
        core::arch::asm!("out dx, al", in("al") 0x80u8, in("dx") port + 3);
        // Baud rate divisor: 115200 baud = divisor 1
        core::arch::asm!("out dx, al", in("al") 1u8, in("dx") port); // LOW
        core::arch::asm!("out dx, al", in("al") 0u8, in("dx") port + 1); // HIGH
                                                                         // 8 bit, no parity, 1 stop bit
        core::arch::asm!("out dx, al", in("al") 0x03u8, in("dx") port + 3);
        // FIFO enable, 14-byte threshold
        core::arch::asm!("out dx, al", in("al") 0xC7u8, in("dx") port + 2);
        // RTS/DTR set
        core::arch::asm!("out dx, al", in("al") 0x0Bu8, in("dx") port + 4);
    }
}

/// KGDB aktif mi kontrol et
pub fn is_active() -> bool {
    KGDB_ACTIVE.load(Ordering::Acquire)
}

/// KGDB trap handler — INT3 veya #DB (debug exception) geldiğinde çağrılır
///
/// Bu fonksiyon register durumunu kaydeder ve GDB ile RSP üzerinden haberleşerek
/// kullanıcının debug komutlarını işler. 'c' (continue) komutu gelene kadar
/// burada bloke olur.
pub fn handle_trap(regs: &KgdbRegisters) {
    if !KGDB_ACTIVE.load(Ordering::Acquire) {
        return;
    }

    // Register'ları kaydet
    unsafe {
        SAVED_REGS = *regs;
    }
    KGDB_CONNECTED.store(true, Ordering::Release);

    crate::serial_println!("[KGDB] Trap @ RIP={:#018x}", regs.rip);

    let port = match unsafe { TRANSPORT } {
        KgdbTransport::Serial { port } => port,
        KgdbTransport::None => return,
    };

    // ACK gönder + SIGTRAP bildir
    serial_write(port, b"+");
    let mut resp_buf = [0u8; MAX_PACKET_SIZE + 8];
    let len = build_response(&mut resp_buf, b"S05");
    serial_write(port, &resp_buf[..len]);

    // GDB komut döngüsü — 'c' (continue) gelene kadar burada kal
    let mut recv_buf = [0u8; MAX_PACKET_SIZE];
    let mut recv_len = 0;

    loop {
        let byte = serial_read_byte(port);
        if recv_len < recv_buf.len() {
            recv_buf[recv_len] = byte;
            recv_len += 1;
        }

        // Paket tamamlandı mı?
        if let Some((packet, consumed)) = parse_rsp_packet(&recv_buf[..recv_len]) {
            // ACK gönder
            serial_write_byte(port, b'+');

            // Komutu işle
            let resp_len = handle_command(&packet, &mut resp_buf);

            if resp_len == 0 {
                // 'c' (continue) komutu — döngüden çık
                break;
            }

            serial_write(port, &resp_buf[..resp_len]);

            // Tüketilen baytları buffer'dan kaldır
            let remaining = recv_len - consumed;
            recv_buf.copy_within(consumed..recv_len, 0);
            recv_len = remaining;
        }
    }

    crate::serial_println!("[KGDB] Resuming @ RIP={:#018x}", unsafe { SAVED_REGS.rip });
}

/// Programlı breakpoint — bu fonksiyonu çağırmak KGDB'yi tetikler
///
/// `kgdb::breakpoint()` çağrıldığında INT3 üretir ve GDB bağlantısını bekler.
#[inline(never)]
pub fn breakpoint() {
    if KGDB_ACTIVE.load(Ordering::Acquire) {
        unsafe {
            core::arch::asm!("int3");
        }
    }
}
