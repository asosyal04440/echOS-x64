//! PTY (Pseudo Terminal) Driver
//!
//! Linux uyumlu PTY implementasyonu.
//! SSH, screen, tmux gibi uygulamalar için altyapı.

use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;
use super::buffer::TtyBuffer;

/// PTY çifti: Master ve Slave
pub struct PtyPair {
    /// Master taraf - terminal emülatörü okur/yazar
    pub master: Arc<PtyMaster>,
    /// Slave taraf - shell/uygulama okur/yazar
    pub slave: Arc<PtySlave>,
    /// PTY numarası (örn: /dev/pts/0)
    pub pty_num: usize,
}

/// Master taraf (terminal emülatörü)
pub struct PtyMaster {
    /// Master'dan Slave'e veri
    to_slave: Arc<Mutex<TtyBuffer>>,
    /// Slave'den Master'a veri
    from_slave: Arc<Mutex<TtyBuffer>>,
    /// PTY numarası
    pty_num: usize,
    /// Canonical mode flag
    canonical: bool,
    /// Echo flag
    echo: bool,
}

/// Slave taraf (shell/uygulama)
pub struct PtySlave {
    /// Slave'den Master'a veri
    to_master: Arc<Mutex<TtyBuffer>>,
    /// Master'dan Slave'e veri
    from_master: Arc<Mutex<TtyBuffer>>,
    /// PTY numarası
    pty_num: usize,
    /// Foreground process group ID
    foreground_pgid: Mutex<usize>,
    /// Window size
    winsize: Mutex<Winsize>,
}

/// Window size yapısı (ioctl TIOCGWINSZ için)
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Winsize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

/// Termios yapısı (terminal ayarları)
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Termios {
    pub c_iflag: u32,  // Input modes
    pub c_oflag: u32,  // Output modes
    pub c_cflag: u32,  // Control modes
    pub c_lflag: u32,  // Local modes
    pub c_line: u8,    // Line discipline
    pub c_cc: [u8; 19], // Control characters
}

// Termios flag'leri
pub const IGNBRK: u32 = 0o000001;  // Ignore break condition
pub const BRKINT: u32 = 0o000002;  // Signal interrupt on break
pub const IGNPAR: u32 = 0o000004;  // Ignore characters with parity errors
pub const PARMRK: u32 = 0o000010;  // Mark parity and framing errors
pub const INPCK: u32 = 0o000020;   // Enable input parity check
pub const ISTRIP: u32 = 0o000040;  // Strip 8th bit off characters
pub const INLCR: u32 = 0o000100;   // Map NL to CR on input
pub const IGNCR: u32 = 0o000200;   // Ignore CR
pub const ICRNL: u32 = 0o000400;   // Map CR to NL on input
pub const IUCLC: u32 = 0o001000;   // Map uppercase to lowercase
pub const IXON: u32 = 0o002000;    // Enable start/stop output control
pub const IXANY: u32 = 0o004000;   // Enable any character to restart output
pub const IXOFF: u32 = 0o010000;   // Enable start/stop input control

pub const OPOST: u32 = 0o000001;   // Post-process output
pub const OLCUC: u32 = 0o000002;   // Map lowercase to uppercase
pub const ONLCR: u32 = 0o000004;   // Map NL to CR-NL
pub const OCRNL: u32 = 0o000010;   // Map CR to NL
pub const ONOCR: u32 = 0o000020;   // No CR output at column 0
pub const ONLRET: u32 = 0o000040;  // NL performs CR function

pub const ISIG: u32 = 0o000001;    // Enable signals
pub const ICANON: u32 = 0o000002;  // Canonical mode
pub const XCASE: u32 = 0o000004;   // Enable ERASE and KILL processing
pub const ECHO: u32 = 0o000010;    // Enable echo
pub const ECHOE: u32 = 0o000020;   // Echo ERASE as BS-SPACE-BS
pub const ECHOK: u32 = 0o000040;   // Echo KILL by erasing line
pub const ECHONL: u32 = 0o000100;  // Echo NL
pub const NOFLSH: u32 = 0o000200;  // Disable flush after interrupt
pub const TOSTOP: u32 = 0o000400;  // Send SIGTTOU for background output
pub const ECHOCTL: u32 = 0o001000; // Echo control characters as ^X
pub const ECHOPRT: u32 = 0o002000; // Echo ERASE as character erased
pub const ECHOKE: u32 = 0o004000;  // Echo KILL by erasing line
pub const FLUSHO: u32 = 0o010000;  // Output being flushed
pub const PENDIN: u32 = 0o040000;  // Retype pending input
pub const IEXTEN: u32 = 0o100000;  // Enable extended functions

// Control character indices
pub const VINTR: usize = 0;    // Interrupt character (Ctrl+C)
pub const VQUIT: usize = 1;    // Quit character (Ctrl+\)
pub const VERASE: usize = 2;   // Erase character (Backspace)
pub const VKILL: usize = 3;    // Kill line character (Ctrl+U)
pub const VEOF: usize = 4;     // End-of-file character (Ctrl+D)
pub const VTIME: usize = 5;    // Timeouts
pub const VMIN: usize = 6;     // Minimum read count
pub const VSWTC: usize = 7;    // Switch character
pub const VSTART: usize = 8;   // Start character (Ctrl+Q)
pub const VSTOP: usize = 9;    // Stop character (Ctrl+S)
pub const VSUSP: usize = 10;   // Suspend character (Ctrl+Z)
pub const VEOL: usize = 11;    // End-of-line character
pub const VREPRINT: usize = 12; // Reprint line (Ctrl+R)
pub const VDISCARD: usize = 13; // Discard (Ctrl+O)
pub const VWERASE: usize = 14; // Word erase (Ctrl+W)
pub const VLNEXT: usize = 15;  // Literal next (Ctrl+V)
pub const VEOL2: usize = 16;   // Alternative EOL

impl Default for Termios {
    fn default() -> Self {
        let mut c_cc = [0u8; 19];
        c_cc[VINTR] = 0x03;     // Ctrl+C
        c_cc[VQUIT] = 0x1C;     // Ctrl+\
        c_cc[VERASE] = 0x7F;    // DEL/Backspace
        c_cc[VKILL] = 0x15;     // Ctrl+U
        c_cc[VEOF] = 0x04;      // Ctrl+D
        c_cc[VTIME] = 0;
        c_cc[VMIN] = 1;
        c_cc[VSWTC] = 0;
        c_cc[VSTART] = 0x11;    // Ctrl+Q
        c_cc[VSTOP] = 0x13;     // Ctrl+S
        c_cc[VSUSP] = 0x1A;     // Ctrl+Z
        c_cc[VEOL] = 0;
        c_cc[VREPRINT] = 0x12;  // Ctrl+R
        c_cc[VDISCARD] = 0x0F;  // Ctrl+O
        c_cc[VWERASE] = 0x17;   // Ctrl+W
        c_cc[VLNEXT] = 0x16;    // Ctrl+V
        c_cc[VEOL2] = 0;
        
        Self {
            c_iflag: ICRNL,
            c_oflag: OPOST | ONLCR,
            c_cflag: 0,
            c_lflag: ISIG | ICANON | ECHO | ECHOE | ECHOK | ECHOCTL | IEXTEN,
            c_line: 0,
            c_cc,
        }
    }
}

/// PTY Yöneticisi
pub struct PtyManager {
    /// Aktif PTY çiftleri
    pairs: Mutex<Vec<Option<Arc<PtyPair>>>>,
    /// Sıradaki PTY numarası
    next_pty_num: Mutex<usize>,
}

impl PtyManager {
    pub const fn new() -> Self {
        Self {
            pairs: Mutex::new(Vec::new()),
            next_pty_num: Mutex::new(0),
        }
    }
    
    /// Yeni PTY çifti oluşturur
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
    
    /// PTY numarasına göre slave döndürür
    pub fn get_slave(&self, pty_num: usize) -> Option<Arc<PtySlave>> {
        let pairs = self.pairs.lock();
        pairs.iter()
            .filter_map(|p| p.as_ref())
            .find(|p| p.pty_num == pty_num)
            .map(|p| p.slave.clone())
    }
    
    /// PTY numarasına göre master döndürür
    pub fn get_master(&self, pty_num: usize) -> Option<Arc<PtyMaster>> {
        let pairs = self.pairs.lock();
        pairs.iter()
            .filter_map(|p| p.as_ref())
            .find(|p| p.pty_num == pty_num)
            .map(|p| p.master.clone())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PtyError {
    NoFreePty,
    InvalidPty,
    BufferFull,
    BufferEmpty,
}

impl PtyMaster {
    /// Master'dan slave'e veri yazar (kullanıcı girdisi)
    pub fn write(&self, data: &[u8]) -> Result<usize, PtyError> {
        let mut buf = self.to_slave.lock();
        let mut written = 0;
        for &b in data {
            match buf.push(b) {
                Ok(()) => written += 1,
                Err(()) => break,
            }
        }
        Ok(written)
    }
    
    /// Slave'den master'a veri okur (çıktı)
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, PtyError> {
        let mut slave_buf = self.from_slave.lock();
        let mut read = 0;
        for b in buf.iter_mut() {
            match slave_buf.pop() {
                Some(byte) => {
                    *b = byte;
                    read += 1;
                }
                None => break,
            }
        }
        Ok(read)
    }
    
    /// PTY numarasını döndürür
    pub fn pty_num(&self) -> usize {
        self.pty_num
    }
}

impl PtySlave {
    /// Slave'den master'a veri yazar (çıktı)
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
    
    /// Master'dan slave'e veri okur (girdi)
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
    
    /// PTY numarasını döndürür
    pub fn pty_num(&self) -> usize {
        self.pty_num
    }
    
    /// Window size ayarlar
    pub fn set_winsize(&self, ws: Winsize) {
        *self.winsize.lock() = ws;
    }
    
    /// Window size döndürür
    pub fn get_winsize(&self) -> Winsize {
        *self.winsize.lock()
    }
    
    /// Foreground process group ayarlar
    pub fn set_foreground_pgid(&self, pgid: usize) {
        *self.foreground_pgid.lock() = pgid;
    }
    
    /// Foreground process group döndürür
    pub fn get_foreground_pgid(&self) -> usize {
        *self.foreground_pgid.lock()
    }
}

lazy_static::lazy_static! {
    /// Global PTY yöneticisi
    pub static ref PTY_MANAGER: PtyManager = PtyManager::new();
}

/// PTY alt sistemini başlatır
pub fn init() {
    crate::serial_println!("[PTY] Subsystem initialized");
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