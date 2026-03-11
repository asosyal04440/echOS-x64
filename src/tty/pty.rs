//! Sözde Terminal (PTY - Pseudo Terminal) Sürücüsü
//!
//! Linux uyumlu PTY implementasyonu.
//! SSH, screen, tmux gibi uygulamalar için altyapı sağlar.
//!
//! ## PTY Nedir?
//!
//! Pseudo Terminal (sözde terminal), gerçek bir donanıma bağlı olmayan,
//! yazılım tarafından simüle edilen bir TTY çiftidir.
//! Çift yönlü bir boru (pipe) gibi çalışır: bir uca yazılan,
//! diğer uçtan okunabilir.
//!
//! ## Master/Slave Mimarisi
//!
//! ```
//!  ┌────────────────────────────────────────────────────────────┐
//!  │                    PTY ÇİFTİ                               │
//!  │                                                            │
//!  │  Terminal Emülatörü           Shell / Uygulama             │
//!  │  (SSH client, xterm, tmux)   (bash, zsh, python)          │
//!  │                                                            │
//!  │  ┌─────────────┐   yazma    ┌─────────────┐               │
//!  │  │             │ ─────────> │             │               │
//!  │  │  PTY MASTER │           │  PTY SLAVE  │               │
//!  │  │ /dev/ptmx   │ <───────── │ /dev/pts/N  │               │
//!  │  │             │   okuma   │             │               │
//!  │  └─────────────┘           └─────────────┘               │
//!  │                                                            │
//!  │  Master: terminal verisini sağlar (kullanıcı girdisi)     │
//!  │  Slave:  kabuk/uygulama tarafından kullanılır             │
//!  └────────────────────────────────────────────────────────────┘
//!
//!  Örnek akış (SSH bağlantısı):
//!  Kullanıcı tuş basar --> SSH client --> PTY Master --> PTY Slave --> shell
//!  Shell çıktı üretir  --> PTY Slave  --> PTY Master --> SSH client --> ekran
//! ```
//!
//! ## Termios Nedir?
//!
//! "Terminal I/O Settings" - terminal davranışını kontrol eden flag'ler kümesidir.
//! Unix/POSIX standardında `struct termios` olarak tanımlıdır.
//! `ioctl(TCGETS/TCSETS)` sistem çağrılarıyla okunup yazılır.
//!
//! ```
//! struct termios {
//!     c_iflag: input flags  (ICRNL, IXON ...)
//!     c_oflag: output flags (OPOST, ONLCR ...)
//!     c_cflag: control flags (baud rate, parity ...)
//!     c_lflag: local flags  (ECHO, ICANON, ISIG ...)
//!     c_cc:    control chars (Ctrl+C=VINTR, Ctrl+D=VEOF ...)
//! }
//! ```

use super::buffer::TtyBuffer;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

/// PTY çifti: Master ve Slave
///
/// Bir PTY oturumundaki iki tarafı bir arada tutar.
/// Arc (Atomically Reference Counted) ile hem master hem slave
/// birbirinin yaşam süresinden bağımsız olarak kullanılabilir.
pub struct PtyPair {
    /// Master taraf - terminal emülatörü okur/yazar
    pub master: Arc<PtyMaster>,
    /// Slave taraf - shell/uygulama okur/yazar
    pub slave: Arc<PtySlave>,
    /// PTY numarası (örn: /dev/pts/0, /dev/pts/1 ...)
    pub pty_num: usize,
}

/// Master taraf (terminal emülatörü tarafı)
///
/// Terminal emülatörü (xterm, SSH, screen vb.) bu tarafı kullanır.
/// Kullanıcı girdisini slave'e yazar, slave'in çıktısını okur.
pub struct PtyMaster {
    /// Master'dan Slave'e veri tamponu (kullanıcı girdisi)
    to_slave: Arc<Mutex<TtyBuffer>>,
    /// Slave'den Master'a veri tamponu (uygulama çıktısı)
    from_slave: Arc<Mutex<TtyBuffer>>,
    /// PTY numarası (/dev/pts/N için N)
    pty_num: usize,
    /// Kanonik mod: satır tamponlama aktif mi?
    /// true = canonical (cooked), false = raw mode
    canonical: bool,
    /// Echo modu: yazdığını ekranda gör
    echo: bool,
}

/// Slave taraf (shell/uygulama tarafı)
///
/// Kabuk (bash, sh) ya da terminal uygulamaları bu tarafı kullanır.
/// Standart I/O (stdin/stdout/stderr) bu taraftan yönlendirilir.
pub struct PtySlave {
    /// Slave'den Master'a veri tamponu (uygulama çıktısı)
    to_master: Arc<Mutex<TtyBuffer>>,
    /// Master'dan Slave'e veri tamponu (kullanıcı girdisi)
    from_master: Arc<Mutex<TtyBuffer>>,
    /// PTY numarası
    pty_num: usize,
    /// Ön plan process group ID (job control için)
    /// Hangi process grubunun terminale sahip olduğunu belirler
    foreground_pgid: Mutex<usize>,
    /// Terminal pencere boyutu (SIGWINCH sinyali için)
    winsize: Mutex<Winsize>,
    /// Terminal I/O ayarları (termios)
    termios: Mutex<Termios>,
}

/// Terminal pencere boyutu yapısı (ioctl TIOCGWINSZ için)
///
/// Terminal uygulamaları, ekranın boyutunu öğrenmek için bu yapıyı kullanır.
/// Boyut değiştiğinde SIGWINCH sinyali gönderilir.
///
/// `#[repr(C)]`: C dili ile uyumlu bellek düzeni (ioctl için gerekli)
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Winsize {
    /// Satır sayısı (karakter cinsinden)
    pub ws_row: u16,
    /// Sütun sayısı (karakter cinsinden)
    pub ws_col: u16,
    /// Piksel genişliği (0 = bilinmiyor)
    pub ws_xpixel: u16,
    /// Piksel yüksekliği (0 = bilinmiyor)
    pub ws_ypixel: u16,
}

/// Termios yapısı (terminal I/O ayarları)
///
/// POSIX `struct termios` ile birebir uyumlu.
/// `ioctl(TCGETS)` ve `ioctl(TCSETS)` sistem çağrıları bu yapıyı kullanır.
///
/// `#[repr(C)]`: C ABI uyumluluğu için zorunlu
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Termios {
    /// Girdi bayrakları (input flags): satır sonu dönüşümü, parity kontrolü vb.
    pub c_iflag: u32,
    /// Çıktı bayrakları (output flags): satır sonu dönüşümü, sonrası işleme vb.
    pub c_oflag: u32,
    /// Kontrol bayrakları (control flags): baud hızı, veri bitleri, parity vb.
    pub c_cflag: u32,
    /// Yerel bayraklar (local flags): echo, kanonik mod, sinyaller vb.
    pub c_lflag: u32,
    /// Satır disiplini tipi (genellikle 0 = N_TTY)
    pub c_line: u8,
    /// Kontrol karakterleri dizisi (VINTR=Ctrl+C, VEOF=Ctrl+D vb.)
    pub c_cc: [u8; 19],
}

// ============================================================================
// Termios girdi bayrakları (c_iflag)
// ============================================================================
/// Break sinyalini yoksay
pub const IGNBRK: u32 = 0o000001;
/// Break'te interrupt sinyali gönder (SIGINT)
pub const BRKINT: u32 = 0o000002;
/// Parity hatası olan karakterleri yoksay
pub const IGNPAR: u32 = 0o000004;
/// Parity ve framing hatalarını işaretle
pub const PARMRK: u32 = 0o000010;
/// Girdi parity kontrolünü etkinleştir
pub const INPCK: u32 = 0o000020;
/// Karakterlerin 8. bitini sil (7-bit ASCII modu)
pub const ISTRIP: u32 = 0o000040;
/// Girdide NL'yi CR'ye dönüştür
pub const INLCR: u32 = 0o000100;
/// CR karakterini yoksay
pub const IGNCR: u32 = 0o000200;
/// Girdide CR'yi NL'ye dönüştür (en yaygın kullanılan: terminal '\r' -> '\n')
pub const ICRNL: u32 = 0o000400;
/// Büyük harfleri küçüğe dönüştür (artık kullanılmıyor)
pub const IUCLC: u32 = 0o001000;
/// XON/XOFF akış kontrolünü etkinleştir (Ctrl+S durdurur, Ctrl+Q devam ettirir)
pub const IXON: u32 = 0o002000;
/// Herhangi bir karakterin XON'u yeniden başlatabileceğine izin ver
pub const IXANY: u32 = 0o004000;
/// Girdi XON/XOFF akış kontrolünü etkinleştir
pub const IXOFF: u32 = 0o010000;

// ============================================================================
// Termios çıktı bayrakları (c_oflag)
// ============================================================================
/// Çıktıyı işle (newline dönüşümleri vb.) - kapalıysa ham çıktı
pub const OPOST: u32 = 0o000001;
/// Küçük harfleri büyüğe dönüştür (artık kullanılmıyor)
pub const OLCUC: u32 = 0o000002;
/// Çıktıda NL'yi CR+NL'ye dönüştür (Unix->Windows satır sonu)
pub const ONLCR: u32 = 0o000004;
/// CR'yi NL'ye dönüştür
pub const OCRNL: u32 = 0o000010;
/// Sütun 0'da CR çıktılamayı engelle
pub const ONOCR: u32 = 0o000020;
/// NL, CR işlevi görsün
pub const ONLRET: u32 = 0o000040;

// ============================================================================
// Termios yerel bayraklar (c_lflag)
// ============================================================================
/// Sinyal üretmeyi etkinleştir (Ctrl+C -> SIGINT, Ctrl+Z -> SIGTSTP)
pub const ISIG: u32 = 0o000001;
/// Kanonik mod (satır tamponlama + özel tuş işleme)
pub const ICANON: u32 = 0o000002;
/// ERASE ve KILL işlemeyi etkinleştir (artık kullanılmıyor)
pub const XCASE: u32 = 0o000004;
/// Echo modunu etkinleştir (yazdığın görünür)
pub const ECHO: u32 = 0o000010;
/// ERASE karakterini BS-SPACE-BS olarak yankıla (görsel silme efekti)
pub const ECHOE: u32 = 0o000020;
/// KILL karakterini satırı silerek yankıla
pub const ECHOK: u32 = 0o000040;
/// NL karakterini yankıla (ICANON kapalıyken bile)
pub const ECHONL: u32 = 0o000100;
/// Interrupt/quit sonrası flush'u devre dışı bırak
pub const NOFLSH: u32 = 0o000200;
/// Arka plan çıktısı için SIGTTOU gönder (job control)
pub const TOSTOP: u32 = 0o000400;
/// Kontrol karakterlerini ^X formatında yankıla (Ctrl+C -> ^C)
pub const ECHOCTL: u32 = 0o001000;
/// ERASE karakterini silinen karakter olarak yankıla
pub const ECHOPRT: u32 = 0o002000;
/// KILL ile satırı silme (BS-SPACE-BS tarzı görsel silme)
pub const ECHOKE: u32 = 0o004000;
/// Çıktı boşaltılıyor (flush devam ediyor)
pub const FLUSHO: u32 = 0o010000;
/// Bekleyen girdinin yeniden yazılmasını sağla
pub const PENDIN: u32 = 0o040000;
/// Genişletilmiş fonksiyonları etkinleştir (IEXTEN + ECHOKE vb.)
pub const IEXTEN: u32 = 0o100000;

// ============================================================================
// Kontrol karakter dizini sabitleri (c_cc[])
// ============================================================================
/// c_cc[VINTR]: Interrupt karakteri - varsayılan: Ctrl+C (0x03) -> SIGINT
pub const VINTR: usize = 0;
/// c_cc[VQUIT]: Quit karakteri - varsayılan: Ctrl+\ (0x1C) -> SIGQUIT
pub const VQUIT: usize = 1;
/// c_cc[VERASE]: Silme karakteri - varsayılan: DEL/Backspace (0x7F)
pub const VERASE: usize = 2;
/// c_cc[VKILL]: Satır silme karakteri - varsayılan: Ctrl+U (0x15)
pub const VKILL: usize = 3;
/// c_cc[VEOF]: Dosya sonu karakteri - varsayılan: Ctrl+D (0x04)
pub const VEOF: usize = 4;
/// c_cc[VTIME]: Raw mod zaman aşımı (1/10 saniye cinsinden)
pub const VTIME: usize = 5;
/// c_cc[VMIN]: Raw modda minimum okunacak karakter sayısı
pub const VMIN: usize = 6;
/// c_cc[VSWTC]: Switch karakteri (artık kullanılmıyor)
pub const VSWTC: usize = 7;
/// c_cc[VSTART]: Çıktıyı başlat - varsayılan: Ctrl+Q (0x11)
pub const VSTART: usize = 8;
/// c_cc[VSTOP]: Çıktıyı durdur - varsayılan: Ctrl+S (0x13)
pub const VSTOP: usize = 9;
/// c_cc[VSUSP]: Suspend karakteri - varsayılan: Ctrl+Z (0x1A) -> SIGTSTP
pub const VSUSP: usize = 10;
/// c_cc[VEOL]: Satır sonu karakteri (NL ek olarak)
pub const VEOL: usize = 11;
/// c_cc[VREPRINT]: Satırı yeniden yazdır - varsayılan: Ctrl+R (0x12)
pub const VREPRINT: usize = 12;
/// c_cc[VDISCARD]: Çıktıyı at - varsayılan: Ctrl+O (0x0F)
pub const VDISCARD: usize = 13;
/// c_cc[VWERASE]: Kelime silme - varsayılan: Ctrl+W (0x17)
pub const VWERASE: usize = 14;
/// c_cc[VLNEXT]: Sonraki karakteri literal al - varsayılan: Ctrl+V (0x16)
pub const VLNEXT: usize = 15;
/// c_cc[VEOL2]: Alternatif satır sonu karakteri
pub const VEOL2: usize = 16;

impl Default for Termios {
    /// POSIX uyumlu varsayılan terminal ayarları.
    ///
    /// Bu ayarlar, tipik bir Unix terminal oturumunun başlangıç durumunu temsil eder:
    /// - Kanonik mod aktif (satır tamponlama)
    /// - Echo aktif (yazdığın görünür)
    /// - Sinyal üretimi aktif (Ctrl+C çalışır)
    /// - Girdi: CR -> NL dönüşümü aktif
    /// - Çıktı: NL -> CR+NL dönüşümü aktif
    fn default() -> Self {
        let mut c_cc = [0u8; 19];
        c_cc[VINTR] = 0x03; // Ctrl+C -> SIGINT
        c_cc[VQUIT] = 0x1C; // Ctrl+\ -> SIGQUIT
        c_cc[VERASE] = 0x7F; // DEL/Backspace
        c_cc[VKILL] = 0x15; // Ctrl+U (satırı sil)
        c_cc[VEOF] = 0x04; // Ctrl+D (EOF)
        c_cc[VTIME] = 0; // Zaman aşımı yok
        c_cc[VMIN] = 1; // En az 1 karakter oku
        c_cc[VSWTC] = 0;
        c_cc[VSTART] = 0x11; // Ctrl+Q (XON)
        c_cc[VSTOP] = 0x13; // Ctrl+S (XOFF)
        c_cc[VSUSP] = 0x1A; // Ctrl+Z -> SIGTSTP
        c_cc[VEOL] = 0;
        c_cc[VREPRINT] = 0x12; // Ctrl+R
        c_cc[VDISCARD] = 0x0F; // Ctrl+O
        c_cc[VWERASE] = 0x17; // Ctrl+W
        c_cc[VLNEXT] = 0x16; // Ctrl+V
        c_cc[VEOL2] = 0;

        Self {
            c_iflag: ICRNL,         // CR -> NL dönüşümü
            c_oflag: OPOST | ONLCR, // Çıktı işleme + NL -> CR+NL
            c_cflag: 0,
            c_lflag: ISIG | ICANON | ECHO | ECHOE | ECHOK | ECHOCTL | IEXTEN,
            c_line: 0,
            c_cc,
        }
    }
}

/// PTY Yöneticisi
///
/// Sistemdeki tüm PTY çiftlerini takip eder ve yeni çiftler oluşturur.
/// Linux'ta `/dev/ptmx` açıldığında bu yönetici (/dev/pts/N) numarasını atar.
pub struct PtyManager {
    /// Aktif PTY çiftleri listesi (indeks = PTY numarası)
    pairs: Mutex<Vec<Option<Arc<PtyPair>>>>,
    /// Sıradaki tahsis edilecek PTY numarası
    next_pty_num: Mutex<usize>,
}

impl PtyManager {
    pub const fn new() -> Self {
        Self {
            pairs: Mutex::new(Vec::new()),
            next_pty_num: Mutex::new(0),
        }
    }

    /// Yeni bir PTY çifti oluşturur ve döndürür.
    ///
    /// Başarılı olursa `/dev/pts/N` yolunu loglar.
    /// Paylaşılan tampon bellek alanı Arc ile hem master hem slave'e bağlanır.
    ///
    /// ```
    /// PTY tampon bağlantısı:
    ///   master_to_slave Arc<Mutex<TtyBuffer>>
    ///      ├── PtyMaster.to_slave (yazma)
    ///      └── PtySlave.from_master (okuma)
    ///
    ///   slave_to_master Arc<Mutex<TtyBuffer>>
    ///      ├── PtySlave.to_master (yazma)
    ///      └── PtyMaster.from_slave (okuma)
    /// ```
    pub fn create_pair(&self) -> Result<Arc<PtyPair>, PtyError> {
        let mut next_num = self.next_pty_num.lock();
        let pty_num = *next_num;
        *next_num += 1;

        // Lock-free buffer'ları oluştur
        let master_to_slave = Arc::new(Mutex::new(TtyBuffer::new()));
        let slave_to_master = Arc::new(Mutex::new(TtyBuffer::new()));

        let master = Arc::new(PtyMaster {
            to_slave: master_to_slave.clone(),
            from_slave: slave_to_master.clone(),
            pty_num,
            canonical: true,
            echo: true,
        });

        let slave = Arc::new(PtySlave {
            to_master: slave_to_master,
            from_master: master_to_slave,
            pty_num,
            foreground_pgid: Mutex::new(0),
            winsize: Mutex::new(Winsize {
                ws_row: 24,
                ws_col: 80,
                ws_xpixel: 0,
                ws_ypixel: 0,
            }),
            termios: Mutex::new(Termios::default()),
        });

        let pair = Arc::new(PtyPair {
            master,
            slave,
            pty_num,
        });

        // PTY listesine ekle
        let mut pairs = self.pairs.lock();
        pairs.push(Some(pair.clone()));

        crate::serial_println!("[PTY] Created /dev/pts/{}", pty_num);
        Ok(pair)
    }

    /// PTY numarasına göre slave tarafı döndürür.
    /// Shell/uygulamanın /dev/pts/N'i açması bu yolla gerçekleşir.
    pub fn get_slave(&self, pty_num: usize) -> Option<Arc<PtySlave>> {
        let pairs = self.pairs.lock();
        pairs
            .iter()
            .filter_map(|p| p.as_ref())
            .find(|p| p.pty_num == pty_num)
            .map(|p| p.slave.clone())
    }

    /// PTY numarasına göre master tarafı döndürür.
    /// Terminal emülatörünün /dev/ptmx'ten master'a erişimi bu yolla gerçekleşir.
    pub fn get_master(&self, pty_num: usize) -> Option<Arc<PtyMaster>> {
        let pairs = self.pairs.lock();
        pairs
            .iter()
            .filter_map(|p| p.as_ref())
            .find(|p| p.pty_num == pty_num)
            .map(|p| p.master.clone())
    }
}

/// PTY hatası türleri
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtyError {
    /// Boş PTY slotu yok (sistem limiti aşıldı)
    NoFreePty,
    /// Geçersiz PTY numarası
    InvalidPty,
    /// Yazma tamponu dolu
    BufferFull,
    /// Okuma tamponu boş
    BufferEmpty,
}

impl PtyMaster {
    /// Master'dan slave'e veri yazar (kullanıcı girdisi - terminal'den shell'e).
    ///
    /// Terminal emülatörü, kullanıcının tuş basmalarını bu metod ile slave'e iletir.
    /// Döndürülen değer: başarıyla yazılan byte sayısı.
    pub fn write(&self, data: &[u8]) -> Result<usize, PtyError> {
        let mut buf = self.to_slave.lock();
        let mut written = 0;
        for &b in data {
            match buf.push(b) {
                Ok(()) => written += 1,
                Err(()) => break, // Tampon doldu, daha fazla yazılamaz
            }
        }
        Ok(written)
    }

    /// Slave'den master'a veri okur (uygulama çıktısı - shell'den terminal'e).
    ///
    /// Terminal emülatörü, shell'in ürettiği çıktıyı bu metod ile okur
    /// ve kullanıcının ekranına yazdırır.
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, PtyError> {
        let mut slave_buf = self.from_slave.lock();
        let mut read = 0;
        for b in buf.iter_mut() {
            match slave_buf.pop() {
                Some(byte) => {
                    *b = byte;
                    read += 1;
                }
                None => break, // Tampon boş
            }
        }
        Ok(read)
    }

    /// Bu PTY'nin numarasını döndürür (/dev/pts/N için N değeri).
    pub fn pty_num(&self) -> usize {
        self.pty_num
    }
}

impl PtySlave {
    /// Slave'den master'a veri yazar (uygulama çıktısı - shell'den terminal'e).
    ///
    /// Shell'in stdout/stderr'e yazdığı veri bu metod ile master'a iletilir.
    pub fn write(&self, data: &[u8]) -> Result<usize, PtyError> {
        let mut buf = self.to_master.lock();
        let mut written = 0;
        for &b in data {
            match buf.push(b) {
                Ok(()) => written += 1,
                Err(()) => break,
            }
        }
        Ok(written)
    }

    /// Master'dan slave'e veri okur (kullanıcı girdisi - terminal'den shell'e).
    ///
    /// Shell'in stdin'den okuduğu veri bu metod ile master'dan alınır.
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, PtyError> {
        let mut master_buf = self.from_master.lock();
        let mut read = 0;
        for b in buf.iter_mut() {
            match master_buf.pop() {
                Some(byte) => {
                    *b = byte;
                    read += 1;
                }
                None => break,
            }
        }
        Ok(read)
    }

    /// Bu PTY slave'inin numarasını döndürür.
    pub fn pty_num(&self) -> usize {
        self.pty_num
    }

    /// Terminal pencere boyutunu ayarlar.
    ///
    /// Kullanıcı terminal penceresini yeniden boyutlandırdığında çağrılır.
    /// Ardından foreground process grubuna SIGWINCH sinyali gönderilmelidir.
    pub fn set_winsize(&self, ws: Winsize) {
        *self.winsize.lock() = ws;
    }

    /// Mevcut terminal pencere boyutunu döndürür.
    /// `ioctl(TIOCGWINSZ)` sistem çağrısı bu metoda yönlendirilir.
    pub fn get_winsize(&self) -> Winsize {
        *self.winsize.lock()
    }

    /// Ön plan process group ID'sini ayarlar.
    ///
    /// Hangi process grubunun terminale sahip olduğunu belirler.
    /// Job control (fg/bg komutları) için kullanılır.
    pub fn set_foreground_pgid(&self, pgid: usize) {
        *self.foreground_pgid.lock() = pgid;
    }

    /// Ön plan process group ID'sini döndürür.
    /// `ioctl(TIOCGPGRP)` sistem çağrısı bu metoda yönlendirilir.
    pub fn get_foreground_pgid(&self) -> usize {
        *self.foreground_pgid.lock()
    }

    /// Terminal I/O ayarlarını (termios) döndürür.
    /// `ioctl(TCGETS)` sistem çağrısı bu metoda yönlendirilir.
    pub fn get_termios(&self) -> Termios {
        *self.termios.lock()
    }

    /// Terminal I/O ayarlarını (termios) günceller.
    /// `ioctl(TCSETS)` sistem çağrısı bu metoda yönlendirilir.
    pub fn set_termios(&self, termios: Termios) {
        *self.termios.lock() = termios;
    }

    /// Kanonik modu aç/kapa (raw mode toggle).
    /// Raw modda her karakter anında okunabilir, satır tamponlama yapılmaz.
    pub fn set_canonical(&self, on: bool) {
        let mut t = self.termios.lock();
        if on {
            t.c_lflag |= ICANON;
        } else {
            t.c_lflag &= !ICANON;
        }
    }

    /// Echo modunu aç/kapa.
    pub fn set_echo(&self, on: bool) {
        let mut t = self.termios.lock();
        if on {
            t.c_lflag |= ECHO;
        } else {
            t.c_lflag &= !ECHO;
        }
    }

    /// Kanonik modda olup olmadığını döndürür.
    pub fn is_canonical(&self) -> bool {
        self.termios.lock().c_lflag & ICANON != 0
    }

    /// Echo modunda olup olmadığını döndürür.
    pub fn is_echo(&self) -> bool {
        self.termios.lock().c_lflag & ECHO != 0
    }

    /// Sinyal üretimi aktif mi?
    pub fn is_isig(&self) -> bool {
        self.termios.lock().c_lflag & ISIG != 0
    }

    /// Girdi baytını termios ayarlarına göre işler (line discipline).
    ///
    /// Termios bayraklarına göre:
    /// - ICANON: satır tamponlama ve özel tuş işleme
    /// - ISIG: Ctrl+C/Z/\\ ile sinyal üretimi
    /// - ICRNL: CR → NL dönüşümü
    ///
    /// İşlenmiş baytlar slave'in from_master tamponundan okunabilir.
    pub fn process_input(&self, byte: u8) -> Option<u8> {
        let termios = self.termios.lock();
        let isig = termios.c_lflag & ISIG != 0;
        let icrnl = termios.c_iflag & ICRNL != 0;
        drop(termios);

        // Sinyal üretimi
        if isig {
            let termios = self.termios.lock();
            if byte == termios.c_cc[VINTR] {
                // Ctrl+C → SIGINT
                drop(termios);
                crate::task::signal::send_signal_pgroup(
                    self.get_foreground_pgid(),
                    crate::task::signal::Signal::SIGINT,
                )
                .ok();
                return None;
            }
            if byte == termios.c_cc[VSUSP] {
                // Ctrl+Z → SIGTSTP
                drop(termios);
                crate::task::signal::send_signal_pgroup(
                    self.get_foreground_pgid(),
                    crate::task::signal::Signal::SIGTSTP,
                )
                .ok();
                return None;
            }
            if byte == termios.c_cc[VQUIT] {
                // Ctrl+\ → SIGQUIT
                drop(termios);
                crate::task::signal::send_signal_pgroup(
                    self.get_foreground_pgid(),
                    crate::task::signal::Signal::SIGQUIT,
                )
                .ok();
                return None;
            }
            drop(termios);
        }

        // CR → NL dönüşümü
        let processed = if byte == b'\r' && icrnl { b'\n' } else { byte };

        Some(processed)
    }

    /// Boyut değişikliğinde SIGWINCH sinyali gönderir.
    pub fn send_sigwinch(&self) {
        let pgid = self.get_foreground_pgid();
        if pgid != 0 {
            crate::task::signal::send_signal_pgroup(pgid, crate::task::signal::Signal::SIGWINCH)
                .ok();
        }
    }
}

lazy_static::lazy_static! {
    /// Global PTY yöneticisi - sistemdeki tüm PTY çiftlerini yönetir.
    /// Linux'taki /dev/ptmx'e karşılık gelir.
    pub static ref PTY_MANAGER: PtyManager = PtyManager::new();
}

/// PTY alt sistemini başlatır.
///
/// Şu an yalnızca log mesajı basar; gelecekte /dev/pts sanal dosya sistemi
/// bağlanacak ve device node'ları oluşturulacak.
pub fn init() {
    crate::serial_println!("[PTY] Subsystem initialized");
}

// ============================================================================
// PTY SHELL SPAWNING
// ============================================================================

/// PTY'yi shell modu için yapılandırır.
///
/// Terminal emülatörü bu fonksiyonu çağırarak PTY'yi
/// interaktif shell kullanımına hazırlar.
///
/// # Arguments
/// * `pty_pair` - Yapılandırılacak PTY çifti
pub fn configure_pty_for_shell(pty_pair: &Arc<PtyPair>) {
    use crate::tty::pty::Winsize;

    // Varsayılan termios ayarlarını uygula
    let termios = Termios::default();
    pty_pair.slave.set_termios(termios);

    // Varsayılan pencere boyutu
    let ws = Winsize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    pty_pair.slave.set_winsize(ws);

    crate::serial_println!(
        "[PTY] Configured /dev/pts/{} for shell use",
        pty_pair.pty_num
    );
}

/// PTY üzerinden komut çalıştırır ve çıktıyı slave'e yazar.
///
/// Terminal emülatörü bu fonksiyonu çağırarak komutu çalıştırır.
/// Çıktı PTY slave tamponuna yazılır, terminal master'dan okur.
///
/// # Arguments
/// * `pty_pair` - PTY çifti
/// * `cmd` - Çalıştırılacak komut
///
/// # Returns
/// Komut başarıyla çalıştırıldıysa true
pub fn execute_command_on_pty(pty_pair: &Arc<PtyPair>, cmd: &str) -> bool {
    if cmd.is_empty() {
        return false;
    }

    // Komut çalıştır
    if let Some(output) = crate::shell::run_command(cmd) {
        if output == "__CLEAR__" {
            // Clear screen ANSI sequence
            let _ = pty_pair.slave.write(b"\x1b[2J\x1b[H");
        } else {
            let _ = pty_pair.slave.write(output.as_bytes());
            let _ = pty_pair.slave.write(b"\n");
        }
        true
    } else {
        false
    }
}

/// PTY master'dan okunabilir veri var mı kontrol eder.
///
/// Non-blocking kontrol - terminal update döngüsünde kullanılır.
pub fn pty_has_output(pty_pair: &Arc<PtyPair>) -> bool {
    let buf = pty_pair.master.from_slave.lock();
    !buf.is_empty()
}

/// PTY slave'e hogeldiniz mesaji yazar.
/// ASCII-only karakterler kullanilir (font uyumlulugu icin).
pub fn write_welcome_message(pty_pair: &Arc<PtyPair>) {
    let welcome = "echOS Terminal v1.0\n";
    let line = "---------------------------------------------------------------\n\n";
    let help = "Welcome to echOS Terminal!\nType 'help' for available commands.\n\n";
    let prompt = "$ ";

    let _ = pty_pair.slave.write(welcome.as_bytes());
    let _ = pty_pair.slave.write(line.as_bytes());
    let _ = pty_pair.slave.write(help.as_bytes());
    let _ = pty_pair.slave.write(prompt.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pty_create() {
        let pair = PTY_MANAGER.create_pair().unwrap();
        assert_eq!(pair.pty_num, 0);
    }

    #[test]
    fn test_pty_io() {
        let pair = PTY_MANAGER.create_pair().unwrap();

        // Master yazar, slave okur
        pair.master.write(b"hello").unwrap();
        let mut buf = [0u8; 10];
        let n = pair.slave.read(&mut buf).unwrap();
        assert_eq!(n, 5);
        assert_eq!(&buf[..5], b"hello");
    }
}
