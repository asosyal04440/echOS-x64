//! # echOS UART Sürücüsü
//!
//! 16550 UART serial port sürücüsü (COM1 - 0x3F8).
//! Debug çıktısı için kullanılır.
//!
//! ## UART (Universal Asynchronous Receiver-Transmitter) Nedir?
//!
//! UART, asenkron seri iletişim sağlayan bir donanım bileşenidir.
//! "Evrensel Eşzamansız Alıcı-Verici" anlamına gelir.
//! 16550 chip'i, IBM PC döneminden bu yana standart haline gelmiş
//! ve FIFO tampon desteğiyle yaygınlaşmıştır.
//!
//! ## 16550 UART Donanım Register Haritası
//!
//! ```
//!  COM1 Taban Adresi: 0x3F8
//!
//!  ┌────────────┬──────────────┬────────────────────────────────────────────┐
//!  │ Offset     │ DLAB=0       │ Açıklama                                   │
//!  ├────────────┼──────────────┼────────────────────────────────────────────┤
//!  │ base + 0   │ RBR / THR   │ Alınan veri / Gönderilecek veri tamponu    │
//!  │ base + 1   │ IER          │ Interrupt Enable Register (kesme etkin.)   │
//!  │ base + 2   │ IIR / FCR   │ Interrupt Id / FIFO Kontrol Register       │
//!  │ base + 3   │ LCR          │ Line Control (veri bitleri, parity, stop)  │
//!  │ base + 4   │ MCR          │ Modem Control (RTS, DTR, IRQ enable)       │
//!  │ base + 5   │ LSR          │ Line Status (TX hazır mı? RX var mı?)      │
//!  │ base + 6   │ MSR          │ Modem Status (CTS, DSR, RI, DCD)           │
//!  │ base + 7   │ Scratch      │ Geçici veri (sürücüde kullanılmıyor)       │
//!  ├────────────┼──────────────┼────────────────────────────────────────────┤
//!  │ DLAB=1     │              │ Baud Rate Divisor'ı ayarlamak için:         │
//!  │ base + 0   │ DLL          │ Divisor Latch Low  (baud low byte)         │
//!  │ base + 1   │ DLM          │ Divisor Latch High (baud high byte)        │
//!  └────────────┴──────────────┴────────────────────────────────────────────┘
//!
//!  DLAB (Divisor Latch Access Bit): LCR'ın 7. bitidir.
//!  DLAB=1 yapılınca base+0 ve base+1, baud divisor'a erişim sağlar.
//!
//!  Baud Rate hesabı:
//!  Divisor = 115200 / istenen_baud
//!  38400 baud için: Divisor = 115200 / 38400 = 3 (0x03)
//! ```
//!
//! ## Başlatma Sırası (Init Sequence)
//!
//! ```
//! 1. IER <- 0x00   Tüm interrupt'ları kapat (güvenli init için)
//! 2. LCR <- 0x80   DLAB bit'ini set et (baud rate erişimi)
//! 3. DLL <- 0x03   Divisor low byte = 3 (38400 baud)
//! 4. DLM <- 0x00   Divisor high byte = 0
//! 5. LCR <- 0x03   DLAB kapat, 8 bit, parity yok, 1 stop bit (8N1)
//! 6. FCR <- 0xC7   FIFO etkinleştir, RX/TX temizle, 14-byte trigger level
//! 7. MCR <- 0x0B   DTR+RTS set, IRQ enable (OUT2 bit)
//! ```

use core::fmt;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;
#[cfg(all(not(target_os = "none"), not(target_os = "uefi")))]
use std::io::{self, Write as IoWrite};
use x86_64::instructions::port::Port;

/// Serial port yapısı.
///
/// 16550 UART'ın tüm I/O register'larını `Port<u8>` tipleri ile temsil eder.
/// `Port<u8>`, x86_64 mimarisinde I/O port'larına `in`/`out` talimatlarıyla
/// erişim sağlayan güvenli bir wrapper'dır.
///
/// ## Port Adresleme
///
/// Her alan, taban adresine sabit bir offset eklenerek oluşturulur:
/// ```
///  COM1 = 0x3F8
///  data     = 0x3F8 + 0 = 0x3F8  (RBR/THR/DLL)
///  int_en   = 0x3F8 + 1 = 0x3F9  (IER/DLM)
///  fifo_ctrl= 0x3F8 + 2 = 0x3FA  (IIR/FCR)
///  line_ctrl= 0x3F8 + 3 = 0x3FB  (LCR - DLAB biti burada)
///  modem_ctrl=0x3F8 + 4 = 0x3FC  (MCR)
///  line_sts = 0x3F8 + 5 = 0x3FD  (LSR - bit 5 = TX hazır)
/// ```
pub struct SerialPort {
    /// Data register (okuma/yazma): normal modda RBR/THR, DLAB=1'de DLL
    data: Port<u8>,
    /// Interrupt enable register: normal modda IER, DLAB=1'de DLM
    int_en: Port<u8>,
    /// FIFO control register: FCR (yazma) / IIR (okuma)
    fifo_ctrl: Port<u8>,
    /// Line control register: veri bitleri, parity, stop bit ve DLAB biti
    line_ctrl: Port<u8>,
    /// Modem control register: RTS, DTR, IRQ enable (OUT2)
    modem_ctrl: Port<u8>,
    /// Line status register: TX boş mu? RX var mı? Hata var mı?
    line_sts: Port<u8>,
}

impl SerialPort {
    /// Yeni bir SerialPort instance oluşturur.
    ///
    /// `base`: UART taban I/O port adresi.
    /// - COM1 için `0x3F8`
    /// - COM2 için `0x2F8`
    /// - COM3 için `0x3E8`
    /// - COM4 için `0x2E8`
    ///
    /// `unsafe`: Ham I/O port erişimi gerektirdiğinden unsafe olarak işaretlenmiştir.
    /// `const fn`: Statik (compile-time) olarak oluşturulabilir.
    pub const unsafe fn new(base: u16) -> Self {
        Self {
            data: Port::new(base),
            int_en: Port::new(base + 1),
            fifo_ctrl: Port::new(base + 2),
            line_ctrl: Port::new(base + 3),
            modem_ctrl: Port::new(base + 4),
            line_sts: Port::new(base + 5),
        }
    }

    /// Serial port'u yapılandırır (38400 baud, 8N1).
    ///
    /// ## Konfigürasyon Adımları
    ///
    /// 1. Tüm interrupt'ları kapat (init sırasında kesme istemiyoruz)
    /// 2. DLAB=1 yap (baud rate divisor'a erişmek için)
    /// 3. Baud divisor = 3 yaz (38400 baud = 115200 / 3)
    /// 4. DLAB=0 yap, 8N1 ayarla (8 data bit, no parity, 1 stop bit)
    /// 5. FIFO etkinleştir (14-byte receive trigger level)
    /// 6. Modem kontrol: DTR+RTS=1, OUT2=1 (IRQ enable)
    pub fn init(&mut self) {
        unsafe {
            self.int_en.write(0x00); // Tüm interrupt'ları kapat
            self.line_ctrl.write(0x80); // DLAB aktif (baud rate ayarı)
            self.data.write(0x03); // Divisor low byte (38400 baud)
            self.int_en.write(0x00); // Divisor high byte
            self.line_ctrl.write(0x03); // 8 bit, parity yok, 1 stop bit
            self.fifo_ctrl.write(0xC7); // FIFO aktif, 14 byte threshold
            self.modem_ctrl.write(0x0B); // IRQ aktif, RTS/DSR set
        }
    }

    /// Line status register'ı okur.
    ///
    /// ## LSR Bit Anlamları
    ///
    /// ```
    ///  bit 0 (0x01): DR  - Data Ready (RX tamponda okunacak veri var)
    ///  bit 1 (0x02): OE  - Overrun Error (tampon taştı)
    ///  bit 2 (0x04): PE  - Parity Error
    ///  bit 3 (0x08): FE  - Framing Error
    ///  bit 4 (0x10): BI  - Break Interrupt
    ///  bit 5 (0x20): THRE- Transmitter Holding Register Empty (TX hazır)
    ///  bit 6 (0x40): TEMT- Transmitter Empty (FIFO de boş, TX tamamen bitti)
    ///  bit 7 (0x80): ERR - FIFO'da hata var
    /// ```
    ///
    /// `send()` fonksiyonu bit 5 (THRE = 0x20) bekler:
    /// TX holding register boşaldığında yeni byte yazılabilir.
    fn line_sts(&mut self) -> u8 {
        unsafe { self.line_sts.read() }
    }

    /// Bir byte gönderir (blocking - bekleme döngüsü).
    ///
    /// ## Gönderme Algoritması
    ///
    /// ```
    /// while LSR.bit5 == 0:   // TX hazır değil, döngüde bekle
    ///     spin_loop_hint()   // cpu'ya "meşgul bekleme" ipucu ver
    /// THR <- data           // Veriyi TX holding register'a yaz
    /// ```
    ///
    /// `spin_loop_hint()`: x86'da PAUSE talimatı üretir.
    /// Bu, CPU'nun spin-loop'ta gereksiz güç harcamasını önler
    /// ve Hyper-Threading verimini artırır.
    ///
    /// Interrupt-driven (kesme tabanlı) gönderme yerine polling kullanılmaktadır.
    /// Bu, debug çıktısı için yeterlidir ve daha basit bir implementasyondur.
    pub fn send(&mut self, data: u8) {
        while self.line_sts() & 0x20 == 0 {
            core::hint::spin_loop();
        }
        let byte = match data {
            b'\n' | b'\r' | b'\t' => data,
            0x20..=0x7e => data,
            _ => b'?',
        };
        unsafe { self.data.write(byte) }
    }
}

/// `fmt::Write` trait implementasyonu.
///
/// Bu, `write!()` ve `write_fmt()` makrolarının `SerialPort` üzerinde
/// çalışmasını sağlar. Rust'ın standart `fmt::Write` trait'i,
/// formatlı string çıktısı için gereken `write_str()` metodunu tanımlar.
///
/// Her byte ayrı ayrı `send()` ile gönderilir.
impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            self.send(byte);
        }
        Ok(())
    }
}

/// Global COM1 serial port instance (0x3F8).
///
/// `Mutex<SerialPort>`: Spin-mutex ile koruma sağlar.
/// `without_interrupts()` ile birlikte, interrupt bağlamında da güvenlidir.
///
/// `const { unsafe { SerialPort::new(0x3F8) } }`:
/// Compile-time'da statik olarak oluşturulur; heap tahsisi yapılmaz.
pub static SERIAL1: Mutex<SerialPort> = Mutex::new(unsafe { SerialPort::new(0x3F8) });

/// Monoton artan log sıra numarası.
///
/// Her `serial_println!` çağrısında bir artırılır.
/// Fetch-and-add atomic operasyonu (Relaxed ordering yeterlidir,
/// sadece tekdüze artış gerekli, senkronizasyon gerekmez).
static LOG_SEQ: AtomicU64 = AtomicU64::new(0);

/// Serial port'u başlatır.
///
/// `SERIAL1`'in kilidini alarak `init()` metodunu çağırır.
/// Baud rate, frame format ve FIFO ayarlarını yapar.
pub fn init() {
    #[cfg(all(not(target_os = "none"), not(target_os = "uefi")))]
    {
        return;
    }

    #[cfg(any(target_os = "none", target_os = "uefi"))]
    SERIAL1.lock().init();
}

/// İç kullanım için temel print fonksiyonu (satır sonu yok, meta bilgi yok).
///
/// `#[doc(hidden)]`: Public API'nin dışında, makro tarafından kullanılır.
/// `without_interrupts()`: Serial port mutex'i interrupt handler'dan da
/// güvenle alınabilsin diye interrupt'ları geçici olarak devre dışı bırakır.
/// Bu, IRQ bağlamında `serial_print!` kullanıldığında deadlock'u önler.
#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    #[cfg(all(not(target_os = "none"), not(target_os = "uefi")))]
    {
        let mut stderr = io::stderr().lock();
        let _ = stderr.write_fmt(args);
        return;
    }

    #[cfg(any(target_os = "none", target_os = "uefi"))]
    {
        use core::fmt::Write;
        use x86_64::instructions::interrupts;

        interrupts::without_interrupts(|| {
            SERIAL1.lock().write_fmt(args).unwrap();
        });
    }
}

/// İç kullanım için meta bilgili print fonksiyonu.
///
/// `serial_println!` makrosu tarafından çağrılır.
/// Her log satırına şu formatı ekler:
/// `[sıra_no dosya:satır modül_yolu] mesaj\n`
///
/// Örnek çıktı:
/// `[42 src/kernel/init.rs:77 echOS::kernel::init] Boot complete`
///
/// `file!()`, `line!()`, `module_path!()`: Rust compile-time makroları,
/// çağrının yapıldığı kaynak konumunu döndürür.
#[doc(hidden)]
pub fn _print_with_meta(args: fmt::Arguments, file: &'static str, line: u32, module: &'static str) {
    use core::fmt::Write;
    use x86_64::instructions::interrupts;
    // Sıra numarasını atomik olarak artır (Relaxed: sadece sıra önemli, senkronizasyon değil)
    let seq = LOG_SEQ.fetch_add(1, Ordering::Relaxed);
    interrupts::without_interrupts(|| {
        let mut port = SERIAL1.lock();
        write!(port, "[{} {}:{} {}] ", seq, file, line, module).unwrap();
        port.write_fmt(args).unwrap();
        port.write_str("\n").unwrap();
    });
}

/// Serial porta formatlı çıktı yazdırır (satır sonu eklenmez).
///
/// `format_args!()` ile argümanlar `fmt::Arguments` tipine dönüştürülür,
/// bu sayede heap tahsisi yapılmadan formatlama gerçekleşir.
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::serial::uart::_print(format_args!($($arg)*));
    };
}

/// Serial porta formatlı çıktı yazdırır (otomatik yeni satır + meta bilgi ekler).
///
/// Çıktı formatı: `[seq file:line module] mesaj\n`
///
/// Üç kullanım şekli:
/// - `serial_println!()` - sadece yeni satır
/// - `serial_println!("mesaj")` - sabit metin
/// - `serial_println!("değer: {}", x)` - format argümanları ile
#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial::uart::_print_with_meta_hostsafe(format_args!(""), file!(), line!(), module_path!()));
    ($fmt:expr) => ($crate::serial::uart::_print_with_meta_hostsafe(format_args!($fmt), file!(), line!(), module_path!()));
    ($fmt:expr, $($arg:tt)*) => ($crate::serial::uart::_print_with_meta_hostsafe(format_args!($fmt, $($arg)*), file!(), line!(), module_path!()));
}

/// `println!` makrosunu serial porta yönlendirir.
///
/// `std::println!` ile aynı sözdizimi, ancak çıktı serial port'a gider.
/// Bu, çekirdek kodunda `println!` kullanılabilmesi için `std` olmadan
/// alternatif bir implementasyon sağlar.
#[macro_export]
macro_rules! println {
    () => ($crate::serial_println!());
    ($fmt:expr) => ($crate::serial_println!($fmt));
    ($fmt:expr, $($arg:tt)*) => ($crate::serial_println!($fmt, $($arg)*));
}

#[doc(hidden)]
pub fn _print_with_meta_hostsafe(
    args: fmt::Arguments,
    file: &'static str,
    line: u32,
    module: &'static str,
) {
    #[cfg(all(not(target_os = "none"), not(target_os = "uefi")))]
    {
        let seq = LOG_SEQ.fetch_add(1, Ordering::Relaxed);
        let mut stderr = io::stderr().lock();
        let _ = stderr.write_fmt(format_args!("[{} {}:{} {}] ", seq, file, line, module));
        let _ = stderr.write_fmt(args);
        let _ = stderr.write_all(b"\n");
        return;
    }

    #[cfg(any(target_os = "none", target_os = "uefi"))]
    _print_with_meta(args, file, line, module);
}
