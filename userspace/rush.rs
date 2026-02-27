//! # rush - echOS Rust Kabuğu
//!
//! echOS için yerleşik komutlar, boru (pipe) desteği ve süreç yönetimi
//! içeren minimal ama işlevsel bir kabuk.

#![no_std]
#![no_main]
#![feature(asm)]

extern crate echos_userspace;

use echos_userspace::*;
use core::arch::asm;
use core::panic::PanicInfo;

// ============================================================================
// KABUK DURUMU
// ============================================================================

/// Komut geçmişi (döngüsel arabellek)
struct History {
    commands: [[u8; 128]; 16],
    count: usize,
    head: usize,
}

impl History {
    const fn new() -> Self {
        Self {
            commands: [[0; 128]; 16],
            count: 0,
            head: 0,
        }
    }

    fn push(&mut self, cmd: &[u8]) {
        let len = cmd.len().min(127);
        self.commands[self.head][..len].copy_from_slice(&cmd[..len]);
        self.commands[self.head][len] = 0;
        self.head = (self.head + 1) % 16;
        if self.count < 16 {
            self.count += 1;
        }
    }

    fn get(&self, index: usize) -> Option<&[u8]> {
        if index >= self.count {
            return None;
        }
        let actual_index = if self.count < 16 {
            index
        } else {
            (self.head + index) % 16
        };
        let cmd = &self.commands[actual_index];
        let len = cmd.iter().position(|&b| b == 0).unwrap_or(128);
        Some(&cmd[..len])
    }
}

/// Geçerli çalışma dizini
static mut CWD: [u8; 256] = [0; 256];

/// Komut geçmişi
static mut HISTORY: History = History::new();

// ============================================================================
// YERLEŞİK KOMUTLAR
// ============================================================================

fn cmd_help() {
    println("rush - echOS Rust Shell v0.1");
    println("");
    println("Yerleşik komutlar:");
    println("  help          - Bu yardımı göster");
    println("  clear         - Ekranı temizle");
    println("  exit          - Kabuktan çık");
    println("  pwd           - Çalışma dizinini yazdır");
    println("  cd <dir>      - Dizin değiştir");
    println("  ls [dir]      - Dizin listele");
    println("  cat <file>    - Dosya içeriğini göster");
    println("  echo <text>   - Metin yazdır");
    println("  mkdir <dir>   - Dizin oluştur");
    println("  rm <file>     - Dosya sil");
    println("  uname         - Sistem bilgisi");
    println("  ps            - Süreçleri listele");
    println("  free          - Bellek kullanımı");
    println("  uptime        - Sistem çalışma süresi");
    println("  history       - Komut geçmişi");
    println("");
    println("Özellikler:");
    println("  - Boru desteği: cmd1 | cmd2");
    println("  - Arka plan: cmd &");
    println("  - Yeniden yönlendirme: cmd > dosya, cmd >> dosya");
}

fn cmd_clear() {
    // Ekranı temizlemek için ANSI kaçış dizisi
    print("\x1b[2J\x1b[H");
}

fn cmd_pwd() {
    unsafe {
        let len = CWD.iter().position(|&b| b == 0).unwrap_or(0);
        if len > 0 {
            let s = core::str::from_utf8(&CWD[..len]).unwrap_or("/");
            print(s);
        } else {
            print("/");
        }
    }
    println("");
}

fn cmd_cd(args: &[&str]) {
    if args.is_empty() {
        // köke git
        unsafe {
            CWD[0] = b'/';
            CWD[1] = 0;
        }
        return;
    }

    let path = args[0];

    // Mutlak ve göreli yolları işle
    unsafe {
        let cwd_len = CWD.iter().position(|&b| b == 0).unwrap_or(0);

        // Yeni yolu oluştur
        let mut new_path = [0u8; 256];
        let mut new_len = 0;

        if path.as_bytes()[0] == b'/' {
            // Mutlak yol
            new_path[..path.len()].copy_from_slice(path.as_bytes());
            new_len = path.len();
        } else {
            // Göreli yol
            if cwd_len > 0 {
                new_path[..cwd_len].copy_from_slice(&CWD[..cwd_len]);
                new_len = cwd_len;
            }
            if new_len > 0 && new_path[new_len - 1] != b'/' {
                new_path[new_len] = b'/';
                new_len += 1;
            }
            new_path[new_len..new_len + path.len()].copy_from_slice(path.as_bytes());
            new_len += path.len();
        }

        // Dizin değiştirmeyi dene
        match chdir(core::str::from_utf8(&new_path[..new_len]).unwrap_or("/")) {
            Ok(()) => {
                CWD[..new_len].copy_from_slice(&new_path[..new_len]);
                CWD[new_len] = 0;
            }
            Err(e) => {
                print("cd: ");
                print(path);
                print(": ");
                match e {
                    ENOENT => println("Böyle bir dosya veya dizin yok"),
                    ENOTDIR => println("Dizin değil"),
                    _ => println("Hata"),
                }
            }
        }
    }
}

fn cmd_ls(args: &[&str]) {
    let _dir = if args.is_empty() {
        "."
    } else {
        args[0]
    };

    // TODO: getdents syscall'ı ile dizin listelemeyi uygula
    // Şimdilik yer tutucu göster
    println("Dizin listeleme henüz uygulanmadı");
    println("(getdents syscall'ı gerektirir)");
}

fn cmd_cat(args: &[&str]) {
    if args.is_empty() {
        println("cat: dosya işleneni eksik");
        return;
    }

    let path = args[0];

    match open(path, O_RDONLY, 0) {
        Ok(fd) => {
            let mut buf = [0u8; 512];
            loop {
                match read(fd, &mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let _ = write(STDOUT, &buf[..n]);
                    }
                    Err(_) => break,
                }
            }
            let _ = close(fd);
        }
        Err(e) => {
            print("cat: ");
            print(path);
            print(": ");
            match e {
                ENOENT => println("Böyle bir dosya veya dizin yok"),
                EACCES => println("Erişim reddedildi"),
                _ => println("Hata"),
            }
        }
    }
}

fn cmd_echo(args: &[&str]) {
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            print(" ");
        }
        print(arg);
    }
    println("");
}

fn cmd_mkdir(args: &[&str]) {
    if args.is_empty() {
        println("mkdir: işlenen eksik");
        return;
    }

    match mkdir(args[0], 0o755) {
        Ok(()) => {}
        Err(e) => {
            print("mkdir: ");
            print(args[0]);
            print(": ");
            match e {
                EEXIST => println("Dosya zaten var"),
                ENOENT => println("Böyle bir dosya veya dizin yok"),
                _ => println("Hata"),
            }
        }
    }
}

fn cmd_rm(args: &[&str]) {
    if args.is_empty() {
        println("rm: işlenen eksik");
        return;
    }

    match unlink(args[0]) {
        Ok(()) => {}
        Err(e) => {
            print("rm: ");
            print(args[0]);
            print(": ");
            match e {
                ENOENT => println("Böyle bir dosya veya dizin yok"),
                EACCES => println("Erişim reddedildi"),
                _ => println("Hata"),
            }
        }
    }
}

fn cmd_uname() {
    println("echOS x86_64");
}

fn cmd_ps() {
    println("  PID TTY          TIME CMD");
    let pid = getpid();
    print(" ");
    print_int(pid);
    print("  ?            00:00 rush");
    println("");
}

fn cmd_free() {
    // TODO: sysinfo syscall'ını uygula
    println("              toplam       kullanılan   boş");
    println("Bellek:       N/A         N/A         N/A");
    println("Takas:        N/A         N/A         N/A");
}

fn cmd_uptime() {
    // TODO: /proc/uptime'dan oku veya syscall kullan
    println("Çalışma süresi: N/A");
}

fn cmd_history() {
    unsafe {
        for i in 0..HISTORY.count {
            print("  ");
            print_int(i + 1);
            print("  ");
            if let Some(cmd) = HISTORY.get(i) {
                let s = core::str::from_utf8(cmd).unwrap_or("");
                println(s);
            }
        }
    }
}

fn cmd_unknown(cmd: &str) {
    print("rush: ");
    print(cmd);
    println(": komut bulunamadı");
}

// ============================================================================
// YARDIMCI FONKSİYONLAR
// ============================================================================

/// Tamsayıyı ondalık olarak yazdır
fn print_int(mut n: usize) {
    if n == 0 {
        print("0");
        return;
    }

    let mut buf = [0u8; 20];
    let mut i = 20;

    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }

    let s = core::str::from_utf8(&buf[i..]).unwrap_or("");
    print(s);
}

/// Komut satırını komut ve argümanlara ayır
fn parse_line(line: &[u8]) -> (&str, Vec<&str>) {
    let line_str = core::str::from_utf8(line).unwrap_or("");
    let trimmed = trim(line_str);

    if trimmed.is_empty() {
        return ("", Vec::new());
    }

    let parts: Vec<&str> = trimmed.split(' ').filter(|s| !s.is_empty()).collect();

    if parts.is_empty() {
        return ("", Vec::new());
    }

    let cmd = parts[0];
    let args = parts[1..].to_vec();

    (cmd, args)
}

/// Boru operatörünü kontrol et
fn has_pipe(line: &[u8]) -> bool {
    line.contains(&b'|')
}

/// Arka plan operatörünü kontrol et
fn is_background(line: &[u8]) -> bool {
    line.last() == Some(&b'&')
}

/// Yeniden yönlendirmeyi kontrol et
fn has_redirection(line: &[u8]) -> Option<(usize, bool)> {
    // (konum, ekleme_modu) döndürür
    for i in 0..line.len().saturating_sub(1) {
        if line[i] == b'>' && line[i + 1] == b'>' {
            return Some((i, true));
        }
        if line[i] == b'>' {
            return Some((i, false));
        }
    }
    None
}

// ============================================================================
// ANA KABUK DÖNGÜSÜ
// ============================================================================

fn read_line(buf: &mut [u8]) -> usize {
    let mut len = 0;

    loop {
        let mut ch = [0u8; 1];
        match read(STDIN, &mut ch) {
            Ok(1) => {
                let c = ch[0];

                match c {
                    // Geri al (Backspace)
                    0x7F | 0x08 => {
                        if len > 0 {
                            len -= 1;
                            // İmleci geri al, karakteri sil, tekrar geri al
                            print("\x08 \x08");
                        }
                    }
                    // Enter
                    b'\n' | b'\r' => {
                        buf[len] = b'\n';
                        len += 1;
                        print("\n");
                        break;
                    }
                    // Ctrl+C
                    0x03 => {
                        print("^C\n");
                        len = 0;
                        break;
                    }
                    // Ctrl+D
                    0x04 => {
                        if len == 0 {
                            println("exit");
                            exit(0);
                        }
                    }
                    // Yazılabilir ASCII
                    0x20..=0x7E => {
                        if len < buf.len() - 1 {
                            buf[len] = c;
                            len += 1;
                            // Karakteri yankıla
                            let s = [c];
                            let _ = write(STDOUT, &s);
                        }
                    }
                    // Kaçış dizisi (ok tuşları vb.)
                    0x1B => {
                        // Kaçış dizisinin kalanını oku
                        let mut seq = [0u8; 2];
                        let _ = read(STDIN, &mut seq);
                        // Ok tuşları: ESC [ A/B/C/D
                        // Şimdilik yoksay
                    }
                    _ => {}
                }
            }
            _ => {
                sched_yield();
            }
        }
    }

    len
}

fn execute_line(line: &[u8]) {
    if line.is_empty() || line[0] == b'#' {
        return;
    }

    // Geçmişe ekle
    let cmd_len = line.iter().position(|&b| b == b'\n').unwrap_or(line.len());
    unsafe {
        HISTORY.push(&line[..cmd_len]);
    }

    // Arka plan modunu kontrol et
    let _background = is_background(line);

    // Boru kontrolü
    if has_pipe(line) {
        // TODO: Boru işlemeyi uygula
        println("Boru desteği yakında geliyor");
        return;
    }

    // Yeniden yönlendirme kontrolü
    if let Some((pos, append)) = has_redirection(line) {
        // TODO: Yeniden yönlendirmeyi uygula
        let _ = (pos, append);
        println("Yeniden yönlendirme desteği yakında geliyor");
        return;
    }

    // Komutu ayrıştır
    let (cmd, args) = parse_line(line);

    if cmd.is_empty() {
        return;
    }

    // Yerleşik veya harici komutu çalıştır
    match cmd {
        "help" => cmd_help(),
        "clear" => cmd_clear(),
        "exit" => {
            println("Güle güle!");
            exit(0);
        }
        "pwd" => cmd_pwd(),
        "cd" => cmd_cd(&args),
        "ls" => cmd_ls(&args),
        "cat" => cmd_cat(&args),
        "echo" => cmd_echo(&args),
        "mkdir" => cmd_mkdir(&args),
        "rm" => cmd_rm(&args),
        "uname" => cmd_uname(),
        "ps" => cmd_ps(),
        "free" => cmd_free(),
        "uptime" => cmd_uptime(),
        "history" => cmd_history(),
        _ => cmd_unknown(cmd),
    }
}

// ============================================================================
// GİRİŞ NOKTASI
// ============================================================================

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // Başlangıç çalışma dizinini ayarla
    unsafe {
        CWD[0] = b'/';
        CWD[1] = 0;
    }

    // Karşılama başlığını yazdır
    print("\x1b[2J\x1b[H"); // Ekranı temizle
    println("╔══════════════════════════════════════════════════════════╗");
    println("║           rush - echOS Rust Shell v0.1                   ║");
    println("║                                                          ║");
    println("║  Kullanılabilir komutlar için 'help' yazın               ║");
    println("║  Ring 3 kullanıcı alanında çalışıyor                     ║");
    println("╚══════════════════════════════════════════════════════════╝");
    println("");

    // Ana kabuk döngüsü
    let mut line = [0u8; 256];

    loop {
        // Komut istemini yazdır
        print("\x1b[32m"); // Yeşil renk
        unsafe {
            let cwd_len = CWD.iter().position(|&b| b == 0).unwrap_or(0);
            if cwd_len > 0 {
                let s = core::str::from_utf8(&CWD[..cwd_len]).unwrap_or("/");
                print(s);
            } else {
                print("/");
            }
        }
        print("\x1b[0m"); // Rengi sıfırla
        print(" $ ");

        // Komut satırını oku
        let len = read_line(&mut line);

        // Komutu çalıştır
        execute_line(&line[..len]);
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    println("\nrush: panik!");
    exit(1);
}
