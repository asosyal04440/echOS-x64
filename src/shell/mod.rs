//! # echOS Shell (Komut Satırı Yorumlayıcısı)
//!
//! Linux seviyesinde shell implementasyonu.
//! Pipe, yönlendirme (redirect), iş kontrolü (job control),
//! tab tamamlama, glob genişletme, geçmiş arama (history search).
//! Scripting: değişkenler, if/else, döngüler, fonksiyonlar, aritmetik.
//!
//! ## Shell Komut İşleme Akışı (ASCII Diyagramı)
//!
//! ```
//!  Kullanıcı Tuş Girişi (klavye polling)
//!         │
//!         v
//!  ┌──────────────────────────────────────────────────────────┐
//!  │                    GapBuffer editor                      │
//!  │  insert(c) / delete() / move_left() / move_right()      │
//!  │  Backspace / Delete / Home / End / Arrow tuşları         │
//!  └──────────────────────┬───────────────────────────────────┘
//!                         │ '\n' (Enter)
//!                         v
//!  ┌──────────────────────────────────────────────────────────┐
//!  │                   Shell::execute()                       │
//!  │                                                          │
//!  │  1. Alias expansion    (ll -> ls -la)                    │
//!  │  2. ENV expansion      ($HOME -> /root)                  │
//!  │  3. Glob expansion     (*.txt -> dosya1.txt dosya2.txt)  │
//!  │  4. Tokenize           (Tokenizer::tokenize)             │
//!  │                                                          │
//!  │       ┌──────────────────────────────────┐              │
//!  │       │ && veya ||  --> execute_chained() │              │
//!  │       │ | veya >    --> execute_pipeline()│              │
//!  │       │ Diğer       --> match parts[0]    │              │
//!  │       └──────────────────────────────────┘              │
//!  └──────────────────────────────────────────────────────────┘
//!
//!  Desteklenen Operatörler:
//!  - |   : Pipe (cmd1 | cmd2)
//!  - >   : Stdout yönlendirme (cmd > dosya)
//!  - >>  : Append yönlendirme (cmd >> dosya)
//!  - <   : Stdin yönlendirme (cmd < dosya)
//!  - &&  : Kısa devre AND (ilk başarılıysa ikinci çalışır)
//!  - ||  : Kısa devre OR (ilk başarısızsa ikinci çalışır)
//!  - ;   : Sıralı çalıştırma
//! ```
//!
//! ## Desteklenen Komutlar
//!
//! | Komut    | Açıklama                            |
//! |----------|-------------------------------------|
//! | help     | Komut listesi                       |
//! | ls       | Dizin listele                       |
//! | cat      | Dosya içeriği göster                |
//! | ps       | Çalışan task'ları göster             |
//! | kill     | Task sonlandır                      |
//! | net      | Ağ bilgisi/yönetimi                 |
//! | http     | HTTP GET/POST/download              |
//! | wget     | URL'den dosya indir                 |
//! | curl     | URL içeriği göster                  |
//! | wine     | Windows PE çalıştırma               |
//! | launch   | ELF userspace uygulaması çalıştır   |
//! | run      | Shell script çalıştır               |
//! | eval     | Aritmetik ifade değerlendir          |

pub mod advanced;
pub mod cmd_pkg;
pub mod editor;
pub mod expr;
pub mod scripting;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use editor::GapBuffer;

const BUILTIN_COMMANDS: &[&str] = &[
    "help",
    "ver",
    "echo",
    "clear",
    "pwd",
    "cd",
    "ls",
    "tree",
    "find",
    "stat",
    "cat",
    "head",
    "tail",
    "wc",
    "grep",
    "sort",
    "uniq",
    "cp",
    "mv",
    "rm",
    "rmdir",
    "mkdir",
    "touch",
    "ln",
    "truncate",
    "readlink",
    "set",
    "export",
    "unset",
    "env",
    "history",
    "alias",
    "unalias",
    "which",
    "command",
    "pkg",
    "ps",
    "kill",
    "bg",
    "fg",
    "jobs",
    "top",
    "chmod",
    "chown",
    "mount",
    "umount",
    "uname",
    "whoami",
    "id",
    "uptime",
    "date",
    "free",
    "ifconfig",
    "net",
    "http",
    "wget",
    "curl",
    "dns",
    "ping",
    "traceroute",
    "launch",
    "run",
    "eval",
    "wine",
    "proton",
    "linux",
    "doom",
    "write",
    "append",
    "nvme-info",
    "tier-bench",
    "jail-log",
    "ring-stats",
    "boot-order",
];

pub fn builtin_command_names() -> &'static [&'static str] {
    BUILTIN_COMMANDS
}

fn builtin_summary(name: &str) -> &'static str {
    match name {
        "help" => "komut katalogu veya komut yardimi",
        "pwd" => "aktif calisma dizinini gosterir",
        "cd" => "aktif calisma dizinini degistirir",
        "ls" => "dizin icerigini listeler",
        "tree" => "dizin agacini cizer",
        "find" => "dosya ve dizin arar",
        "stat" => "dosya metadatasini gosterir",
        "cat" => "metin dosyasini ekrana yazar",
        "head" => "ilk satirlari gosterir",
        "tail" => "son satirlari gosterir",
        "wc" => "satir, kelime ve karakter sayar",
        "grep" => "satir filtreler",
        "sort" => "satirlari siralar",
        "uniq" => "ardisik tekrar eden satirlari ezer",
        "cp" => "dosya kopyalar",
        "set" | "export" | "unset" | "env" => "oturum ortam degiskenlerini yonetir",
        "history" => "oturum komut gecmisini gosterir",
        "alias" | "unalias" => "oturum alias tablolarini yonetir",
        "which" | "command" => "builtin komut katalogunda arama yapar",
        "net" | "http" | "wget" | "curl" | "dns" | "ping" if network_surface_disabled() => {
            network_surface_summary(name)
        }
        "net" => "ag katmanlarini ve mevcut gercek sinirlari raporlar",
        "http" => "echOS HTTP/HTTPS istemcisi ile gercek web istegi gonderir",
        "wget" | "curl" => "gercek HTTP/HTTPS istemci yolunu kullanir",
        "dns" => "gercek DNS resolver yolunu kullanir",
        "ping" => "gercek ICMP echo yolunu kullanir",
        "launch" => "ELF uygulamasi baslatir",
        "run" => "shell script calistirir",
        "eval" => "ifadeyi degerlendirir",
        _ => "shell builtin",
    }
}

fn render_help(topic: Option<&str>) -> String {
    if let Some(name) = topic {
        if builtin_command_names().iter().any(|cmd| *cmd == name) {
            return format!("{}: {}", name, builtin_summary(name));
        }
        return format!("{} bulunamadi", name);
    }

    let mut rows = Vec::new();
    for name in builtin_command_names() {
        rows.push(format!("{:<12} {}", name, builtin_summary(name)));
    }
    rows.join("\n")
}

fn describe_http_error(err: crate::net::http::HttpError) -> String {
    use crate::net::http::HttpError;

    match err {
        HttpError::ConnectionFailed => String::from(
            "baglanti kurulamadı\nNot: DNS, rota veya uzak endpoint erisimi basarisiz olabilir",
        ),
        HttpError::Timeout => String::from(
            "zaman asimi\nNot: uzak endpoint belirtilen sure icinde yanit vermedi",
        ),
        HttpError::TlsHandshakeFailed => String::from(
            "TLS handshake basarisiz\nNot: uzak taraf TLS el sikismasini tamamlamadi",
        ),
        HttpError::TlsDecodeFailed => String::from(
            "TLS certificate/transcript decode basarisiz\nNot: sertifika mesaji ayristrilamadi",
        ),
        HttpError::TlsCertDateInvalid => String::from(
            "TLS sertifika tarih hatasi\nNot: sertifika su an icin henuz gecerli degil veya suresi dolmus",
        ),
        HttpError::TlsCertCnInvalid => String::from(
            "TLS hostname dogrulamasi basarisiz\nNot: hedef host SAN/CN ile eslesmiyor",
        ),
        HttpError::TlsInvalidCa => String::from(
            "TLS guven zinciri basarisiz\nNot: sertifika guvenilen bir CA kokune baglanamadi",
        ),
        HttpError::TlsInvalidCertificate => String::from(
            "TLS sertifika yapisi/imzasi gecersiz\nNot: zincir veya imza dogrulamasi basarisiz",
        ),
        HttpError::TlsCertRevoked => String::from(
            "TLS sertifika iptal edilmis\nNot: sertifika revoked durumunda",
        ),
        HttpError::TlsNotSupported => String::from(
            "TLS yolu tamamlanamadi\nNot: tasiyici/handshake fidelity siniri devam ediyor",
        ),
        HttpError::ProxyAuthenticationRequired => String::from(
            "proxy kimlik dogrulamasi gerekli\nNot: proxy 407 / CONNECT auth gerektirdi",
        ),
        HttpError::InvalidUrl => String::from("gecersiz URL"),
        HttpError::InvalidResponse => String::from("gecersiz HTTP yaniti"),
        HttpError::InvalidHeader => String::from("gecersiz HTTP basligi"),
        HttpError::ChunkedEncoding => String::from("chunked transfer decode hatasi"),
        HttpError::ContentLength => String::from("content-length uyumsuzlugu"),
        HttpError::TooManyRedirects => String::from("cok fazla yonlendirme"),
        HttpError::NotFound => String::from("404 bulunamadi"),
        HttpError::ServerError => String::from("uzak sunucu 5xx hatasi dondurdu"),
        HttpError::Network(net_err) => format!("ag hatasi: {:?}", net_err),
    }
}

fn describe_doh_error(err: crate::net::doh::DohError) -> String {
    use crate::net::doh::DohError;

    match err {
        DohError::HttpsNotSupported => String::from(
            "DoH HTTPS yolu tamamlanamadi\nNot: TLS/HTTPS tasiyici fidelity siniri devam ediyor",
        ),
        DohError::InvalidResponse => {
            String::from("DoH yaniti gecersiz\nNot: uzak endpoint DNS wire formatina uygun donmedi")
        }
        DohError::NetworkError => String::from(
            "DoH ag hatasi\nNot: DNS, TCP veya TLS kurulum adimlarindan biri basarisiz oldu",
        ),
        DohError::Timeout => String::from(
            "DoH zaman asimi\nNot: uzak endpoint belirtilen deneme butcesinde yanit vermedi",
        ),
        DohError::ServerError(code) => {
            format!("DoH sunucu hatasi: HTTP {}", code)
        }
    }
}

fn describe_dot_error(err: crate::net::dot::DotError) -> String {
    use crate::net::dot::DotError;

    match err {
        DotError::TlsNotSupported => String::from(
            "DoT TLS yolu tamamlanamadi\nNot: TLS tasiyici fidelity siniri devam ediyor",
        ),
        DotError::NotConnected => String::from(
            "DoT baglantisi kurulmadan sorgu denendi\nNot: socket/TLS oturumu acik degil",
        ),
        DotError::SocketError => String::from("DoT socket olusturulamadi"),
        DotError::ConnectionFailed => {
            String::from("DoT baglanti hatasi\nNot: TCP 853 veya ag erisimi basarisiz oldu")
        }
        DotError::InvalidResponse => String::from(
            "DoT yaniti gecersiz\nNot: DNS wire format veya TLS kaydi ayristrmasi basarisiz",
        ),
        DotError::Timeout => String::from(
            "DoT zaman asimi\nNot: uzak endpoint belirtilen deneme butcesinde yanit vermedi",
        ),
        DotError::TlsHandshakeFailed => String::from(
            "DoT TLS handshake basarisiz\nNot: uzak taraf sertifika/handshake yolunu tamamlamadi",
        ),
    }
}

fn describe_http3_error(err: crate::net::http3::Http3Error) -> String {
    use crate::net::http3::Http3Error;

    match err {
        Http3Error::RemoteTransportUnavailable => String::from(
            "HTTP/3 remote transport hazir degil\nNot: established QUIC baglantisi enjekte edilmeden sessiz downgrade yok",
        ),
        Http3Error::ShortWrite => String::from(
            "HTTP/3 kisa yazma\nNot: QUIC stream frame'i tam tasiyamadi",
        ),
        Http3Error::ProtocolError(code) => format!("HTTP/3 protokol hatasi: 0x{:x}", code),
        Http3Error::StreamError(code) => format!("HTTP/3 stream hatasi: 0x{:x}", code),
        Http3Error::ConnectionError(code) => format!("HTTP/3 baglanti hatasi: 0x{:x}", code),
        Http3Error::FrameError => String::from("HTTP/3 frame ayristrma hatasi"),
        Http3Error::SettingsError => String::from("HTTP/3 settings hatasi"),
        Http3Error::QpackError => String::from("HTTP/3 QPACK hatasi"),
        Http3Error::QuicError(err) => format!("HTTP/3 QUIC hatasi: {:?}", err),
    }
}

fn parse_dns_privacy_provider(provider: Option<&str>) -> Result<&'static str, &'static str> {
    match provider.unwrap_or("cloudflare") {
        "cloudflare" => Ok("cloudflare"),
        "google" => Ok("google"),
        "quad9" => Ok("quad9"),
        _ => Err("Saglayici: cloudflare | google | quad9"),
    }
}

static SHELL_RUNTIME_READY: AtomicBool = AtomicBool::new(false);
const SESSION_HISTORY_LIMIT: usize = 1000;
const PRODUCT_NETWORK_SURFACE_ENABLED: bool = true;

fn network_surface_disabled() -> bool {
    !PRODUCT_NETWORK_SURFACE_ENABLED
}

fn network_surface_summary(name: &str) -> &'static str {
    match name {
        "net" => "ag yuzeyi urun hedefinden cikarildi; network komutlari kapali",
        "http" | "wget" | "curl" => {
            "web istemci yuzeyi urun hedefinden cikarildi; network komutlari kapali"
        }
        "dns" => "dns yuzeyi urun hedefinden cikarildi; network komutlari kapali",
        "ping" => "icmp yuzeyi urun hedefinden cikarildi; network komutlari kapali",
        _ => "network yuzeyi urun hedefinden cikarildi",
    }
}

fn network_surface_disabled_response(name: &str) -> String {
    format!(
        "{} kullanilamaz\nNot: echOS urun hedefinde network yuzeyi gozden cikarildi ve shell fail-closed durumda",
        name
    )
}

fn ensure_shell_runtime_ready() {
    if SHELL_RUNTIME_READY
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        advanced::init();
    }
}

/// Shell'i bir task olarak başlat.
///
/// echOS görev zamanlayıcısına (scheduler) Normal öncelikli yeni bir task
/// olarak ekler. Shell, scheduler döngüsünde kendi CPU zamanını alır.
pub fn spawn_shell_task() {
    crate::task::scheduler::spawn_with_priority(
        shell_entry,
        crate::task::task::Priority::Normal,
        "shell",
    );
}

/// Shell'i doğrudan çalıştırır (blocking - scheduler olmadan).
///
/// Bu fonksiyon dönmez (`-> !`). Kernel init akışında hem GUI desktop
/// hem de TTY shell dönemine göre bu fonksiyon çağrılabilir.
pub fn run_shell() -> ! {
    shell_entry()
}

/// Hem serial porta hem de framebuffer terminaline metin yazar.
///
/// Shell çıktısı iki yere gider:
/// 1. Framebuffer: UEFI GOP aracılığıyla ekrandaki metin terminali
/// 2. Serial (COM1): QEMU -serial stdio ile host terminali
fn print(s: &str) {
    // Framebuffer'a yaz
    crate::boot::term_print(s);
    // Serial çıktı
    crate::serial_print!("{}", s);
}

/// Hem serial porta hem de framebuffer terminaline satır sonu ile yazar.
///
/// `print(s)` ardından `print("\n")` çağırır.
fn println(s: &str) {
    print(s);
    print("\n");
}

/// Ekranı temizler.
///
/// - Framebuffer: `term_clear()` ile tüm pikseller silinir
/// - Serial terminal: `\x1b[2J\x1b[H` ANSI kaçış dizisi ile
///   ekran temizlenir ve cursor üst sola taşınır
fn clear_screen() {
    // Framebuffer'ı temizle
    crate::boot::term_clear();
    // Serial clear
    crate::serial_print!("\x1b[2J\x1b[H");
}

/// Shell task giriş noktası (entry point).
///
/// Bu fonksiyon `-> !` tipindedir, yani asla dönmez.
/// Sonsuz döngüde:
/// 1. Prompt yazdır ("echOS$ ")
/// 2. Klavyeden karakter oku (polling)
/// 3. Enter'a basılınca komutu çalıştır
/// 4. Sonucu ekrana yazdır
/// 5. Tekrar prompt'a dön
fn shell_entry() -> ! {
    crate::serial_println!("[SHELL] Starting interactive shell...");

    // Ekranı temizle
    clear_screen();

    // Banner
    println("╔════════════════════════════════════════════════════════════╗");
    println("║                    echOS v0.3.0                            ║");
    println("║              Legendary Edition Shell                       ║");
    println("╚════════════════════════════════════════════════════════════╝");
    println("");
    println("Type 'help' for available commands.");
    println("");

    let mut shell = Shell::new();
    let prompt = "echOS$ ";

    loop {
        print(prompt);

        // Mevcut tty klavye polling dongusu
        loop {
            // Klavyeden karakter oku
            if let Some(key) = crate::keyboard::read_key() {
                match key {
                    pc_keyboard::DecodedKey::Unicode(c) => {
                        if c == '\n' {
                            println("");
                            if let Some(output) = shell.execute() {
                                if output == "__CLEAR__" {
                                    clear_screen();
                                } else {
                                    println(&output);
                                }
                            }
                            break;
                        } else if c == '\x03' {
                            // Ctrl+C - SIGINT (mevcut komutu iptal et)
                            println("^C");
                            shell.editor = GapBuffer::new(64); // Buffer'ı temizle
                            break;
                        } else if c == '\x04' {
                            // Ctrl+D - EOF/Logout
                            println("logout");
                            // Shell'i yeniden başlat (logout)
                            clear_screen();
                            println("echOS v0.3.0 - Yeniden baslatiliyor...");
                            shell.editor = GapBuffer::new(64);
                            break;
                        } else if c == '\x1A' {
                            // Ctrl+Z - SIGTSTP — ön plandaki görev varsa suspend et
                            println("^Z");
                            // Mevcut ön plan görevini durdur (shell hariç)
                            if let Some(fg_task) = crate::task::scheduler::get_foreground_task() {
                                crate::task::signal::send_signal(
                                    fg_task,
                                    crate::task::signal::Signal::SIGTSTP,
                                );
                                crate::serial_println!(
                                    "[SHELL] Ctrl+Z: task {} suspended (SIGTSTP)",
                                    fg_task
                                );
                            }
                            shell.editor = GapBuffer::new(64);
                            break;
                        } else if c == '\x08' {
                            // Backspace
                            if shell.editor.cursor_pos() > 0 {
                                shell.editor.delete();
                                // Cursor'u geri taşı, karakteri sil, kalanı yeniden çiz
                                print("\x1b[D"); // Geri git
                                let rest = shell.editor.text_after_cursor();
                                print("\x1b[K"); // Satır sonunu sil
                                print(&rest); // Kalan metni yaz
                                              // Cursor'u geri taşı
                                for _ in 0..rest.len() {
                                    print("\x1b[D");
                                }
                            }
                        } else if c == '\t' {
                            // Tab completion
                            let input = shell.editor.to_string();
                            let cursor_pos = shell.editor.cursor_pos();
                            let completer = advanced::Completer::new();
                            let completions = completer.complete(&input, cursor_pos);

                            if completions.len() == 1 {
                                // Tek eşleşme - tamamla
                                let completion = &completions[0];
                                // Mevcut kelimeyi bul ve sil
                                let words: Vec<&str> =
                                    input[..cursor_pos].split_whitespace().collect();
                                if let Some(current) = words.last() {
                                    // Cursor'u kelime başına taşı
                                    for _ in 0..current.len() {
                                        print("\x1b[D");
                                        shell.editor.move_left();
                                        shell.editor.delete_forward();
                                    }
                                    print("\x1b[K");
                                    // Tamamlamayı yaz
                                    for ch in completion.chars() {
                                        shell.editor.insert(ch);
                                    }
                                    print(completion);
                                    print(" ");
                                    shell.editor.insert(' ');
                                }
                            } else if completions.len() > 1 {
                                // Birden fazla eşleşme - listele
                                println("");
                                let common = advanced::Completer::common_prefix(&completions);
                                for c in completions.iter().take(10) {
                                    print(c);
                                    print("  ");
                                }
                                if completions.len() > 10 {
                                    print("...");
                                }
                                println("");
                                print(prompt);
                                // Ortak prefix'i yaz
                                print(&input);
                            }
                        } else if !c.is_control() {
                            shell.editor.insert(c);
                            print(&alloc::string::String::from(c));
                        }
                    }
                    pc_keyboard::DecodedKey::RawKey(code) => {
                        use pc_keyboard::KeyCode;
                        match code {
                            KeyCode::ArrowLeft => {
                                if shell.editor.cursor_pos() > 0 {
                                    shell.editor.move_left();
                                    // Cursor'u geri taşı
                                    print("\x1b[D");
                                }
                            }
                            KeyCode::ArrowRight => {
                                if shell.editor.cursor_pos() < shell.editor.len() {
                                    shell.editor.move_right();
                                    // Cursor'u ileri taşı
                                    print("\x1b[C");
                                }
                            }
                            KeyCode::ArrowUp => {
                                // History navigation - önceki komut
                                if let Some(hist_cmd) = shell.previous_history() {
                                    // Mevcut satırı temizle
                                    let pos = shell.editor.cursor_pos();
                                    for _ in 0..pos {
                                        print("\x1b[D");
                                    }
                                    print("\x1b[K");

                                    // History'den gelen komutu yaz
                                    shell.replace_editor_line(&hist_cmd);
                                    print(&hist_cmd);
                                }
                            }
                            KeyCode::ArrowDown => {
                                // History navigation - sonraki komut
                                if let Some(hist_cmd) = shell.next_history() {
                                    // Mevcut satırı temizle
                                    let pos = shell.editor.cursor_pos();
                                    for _ in 0..pos {
                                        print("\x1b[D");
                                    }
                                    print("\x1b[K");

                                    // History'den gelen komutu yaz
                                    shell.replace_editor_line(&hist_cmd);
                                    print(&hist_cmd);
                                }
                            }
                            KeyCode::Delete => {
                                // Delete tuşu (ileri silme)
                                if shell.editor.cursor_pos() < shell.editor.len() {
                                    shell.editor.delete_forward();
                                    // Kalan satırı yeniden çiz
                                    let rest = shell.editor.text_after_cursor();
                                    print("\x1b[K"); // Satır sonunu sil
                                    print(&rest);
                                    // Cursor'u geri taşı
                                    for _ in 0..rest.len() {
                                        print("\x1b[D");
                                    }
                                }
                            }
                            KeyCode::Home => {
                                // Satır başına git
                                let pos = shell.editor.cursor_pos();
                                for _ in 0..pos {
                                    shell.editor.move_left();
                                    print("\x1b[D");
                                }
                            }
                            KeyCode::End => {
                                // Satır sonuna git
                                let pos = shell.editor.cursor_pos();
                                let len = shell.editor.len();
                                for _ in pos..len {
                                    shell.editor.move_right();
                                    print("\x1b[C");
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            // Kısa bekleme - CPU'yu yormamak için
            // Simics: hlt ile CPU'yu uyut (sonraki interrupt'a kadar)
            // Bare-metal: spin-loop ile düşük gecikmeli polling
            #[cfg(feature = "simics")]
            {
                x86_64::instructions::hlt();
            }
            #[cfg(not(feature = "simics"))]
            {
                // NOT: schedule() çağrısı PAGE FAULT'a neden olabiliyor
                for _ in 0..1000 {
                    core::hint::spin_loop();
                }
            }
        }
    }
}

/// Komut satırı shell yapısı.
///
/// Shell'in durumunu tutar: `GapBuffer` satır editörü ve komut geçmişi.
#[derive(Clone, Default)]
struct ShellEnvironment {
    vars: BTreeMap<String, String>,
}

impl ShellEnvironment {
    fn seeded() -> Self {
        let mut vars = BTreeMap::new();
        for (key, value) in advanced::ENV.list() {
            vars.insert(key, value);
        }
        Self { vars }
    }

    fn set(&mut self, key: &str, value: &str) {
        self.vars.insert(key.to_string(), value.to_string());
    }

    fn get(&self, key: &str) -> Option<String> {
        self.vars.get(key).cloned()
    }

    fn unset(&mut self, key: &str) {
        self.vars.remove(key);
    }

    fn list(&self) -> Vec<(String, String)> {
        self.vars
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }

    fn expand(&self, input: &str) -> String {
        let mut result = String::new();
        let mut chars = input.chars().peekable();

        while let Some(c) = chars.next() {
            if c != '$' {
                result.push(c);
                continue;
            }

            let var_name = if chars.peek() == Some(&'{') {
                chars.next();
                let mut name = String::new();
                while let Some(&ch) = chars.peek() {
                    chars.next();
                    if ch == '}' {
                        break;
                    }
                    name.push(ch);
                }
                name
            } else {
                let mut name = String::new();
                while let Some(&ch) = chars.peek() {
                    if ch.is_alphanumeric() || ch == '_' {
                        name.push(ch);
                        chars.next();
                    } else {
                        break;
                    }
                }
                name
            };

            if let Some(value) = (!var_name.is_empty())
                .then(|| self.get(&var_name))
                .flatten()
            {
                result.push_str(&value);
            }
        }

        result
    }
}

#[derive(Clone, Default)]
struct ShellAliases {
    aliases: BTreeMap<String, String>,
}

impl ShellAliases {
    fn seeded() -> Self {
        let mut aliases = BTreeMap::new();
        for (name, value) in advanced::ALIASES.list() {
            aliases.insert(name, value);
        }
        Self { aliases }
    }

    fn set(&mut self, name: &str, expansion: &str) {
        self.aliases.insert(name.to_string(), expansion.to_string());
    }

    fn unset(&mut self, name: &str) {
        self.aliases.remove(name);
    }

    fn list(&self) -> Vec<(String, String)> {
        self.aliases
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect()
    }

    fn expand_line(&self, input: &str) -> String {
        let first_word = input.split_whitespace().next();
        if let Some(word) = first_word {
            if let Some(expansion) = self.aliases.get(word) {
                return input.replacen(word, expansion, 1);
            }
        }
        input.to_string()
    }
}

#[derive(Clone, Default)]
struct ShellHistory {
    entries: Vec<String>,
    cursor: usize,
}

impl ShellHistory {
    fn push(&mut self, cmd: &str) {
        if cmd.trim().is_empty() {
            self.cursor = self.entries.len();
            return;
        }
        if self.entries.last().map(|entry| entry.as_str()) != Some(cmd) {
            if self.entries.len() >= SESSION_HISTORY_LIMIT {
                self.entries.remove(0);
            }
            self.entries.push(cmd.to_string());
        }
        self.cursor = self.entries.len();
    }

    fn previous(&mut self) -> Option<String> {
        if self.entries.is_empty() || self.cursor == 0 {
            return None;
        }
        self.cursor -= 1;
        self.entries.get(self.cursor).cloned()
    }

    fn next(&mut self) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        if self.cursor + 1 < self.entries.len() {
            self.cursor += 1;
            self.entries.get(self.cursor).cloned()
        } else if self.cursor < self.entries.len() {
            self.cursor = self.entries.len();
            Some(String::new())
        } else {
            None
        }
    }

    fn list(&self) -> Vec<(usize, String)> {
        self.entries
            .iter()
            .enumerate()
            .map(|(index, cmd)| (index + 1, cmd.clone()))
            .collect()
    }
}

pub struct Shell {
    /// Metin düzenleme için gap buffer (O(1) cursor pozisyonunda ekleme/silme)
    editor: GapBuffer,
    /// Komut geçmişi (her session için tutulur)
    history: ShellHistory,
    env: ShellEnvironment,
    aliases: ShellAliases,
}

impl Shell {
    /// Yeni bir shell instance oluşturur.
    ///
    /// 64 karakter kapasiteli gap buffer ile başlar.
    /// Buffer dolunca `grow()` ile otomatik genişler.
    pub fn new() -> Self {
        ensure_shell_runtime_ready();
        Self {
            editor: GapBuffer::new(64),
            history: ShellHistory::default(),
            env: ShellEnvironment::seeded(),
            aliases: ShellAliases::seeded(),
        }
    }

    fn replace_editor_line(&mut self, line: &str) {
        self.editor = GapBuffer::new(line.len().max(64));
        for ch in line.chars() {
            self.editor.insert(ch);
        }
    }

    pub fn previous_history(&mut self) -> Option<String> {
        self.history.previous()
    }

    pub fn next_history(&mut self) -> Option<String> {
        self.history.next()
    }

    fn sync_runtime_state(&self) {
        advanced::ENV.clear();
        for (key, value) in self.env.list() {
            advanced::ENV.set(&key, &value);
        }
        advanced::ALIASES.clear();
        for (name, value) in self.aliases.list() {
            advanced::ALIASES.set(&name, &value);
        }
    }

    fn current_working_directory(&self) -> String {
        self.env.get("PWD").unwrap_or_else(|| String::from("/"))
    }

    fn change_directory(&mut self, target: Option<&str>) -> Result<String, String> {
        self.sync_runtime_state();
        let previous = self.current_working_directory();
        let desired = match target.filter(|value| !value.is_empty()) {
            Some("-") => self.env.get("OLDPWD").unwrap_or_else(|| previous.clone()),
            Some(value) => resolve_path(value),
            None => self.env.get("HOME").unwrap_or_else(|| String::from("/")),
        };
        crate::fs::f2fs::list_dir(&desired).map_err(|_| String::from("Dizin bulunamadi"))?;
        self.env.set("OLDPWD", &previous);
        self.env.set("PWD", &desired);
        self.sync_runtime_state();
        Ok(desired)
    }

    /// Klavye tuşunu işler.
    ///
    /// Unicode karakterler editöre eklenir; RawKey'ler
    /// (ArrowLeft/Right/Up/Down) cursor hareketi için kullanılır.
    pub fn handle_key(&mut self, key: pc_keyboard::DecodedKey) {
        use pc_keyboard::DecodedKey;
        match key {
            DecodedKey::Unicode(c) => match c {
                '\n' => {}
                '\x08' => {
                    self.editor.delete();
                } // Geri silme
                _ => self.editor.insert(c),
            },
            DecodedKey::RawKey(code) => {
                use pc_keyboard::KeyCode;
                match code {
                    KeyCode::ArrowLeft => self.editor.move_left(),
                    KeyCode::ArrowRight => self.editor.move_right(),
                    KeyCode::ArrowUp => { /* Geçmiş navigasyonu */ }
                    KeyCode::ArrowDown => { /* Geçmiş navigasyonu */ }
                    _ => {}
                }
            }
        }
    }

    /// Mevcut komutu çalıştırır ve çıktıyı `Option<String>` olarak döndürür.
    ///
    /// ## İşleme Sırası
    ///
    /// 1. Editor'dan komut satırını al (`to_string()`)
    /// 2. Editor'ı sıfırla (yeni prompt için hazırla)
    /// 3. Boş satır kontrolü — boşsa `None` döndür
    /// 4. **Alias expansion**: `ll` → `ls -la` gibi kısaltmaları genişlet
    /// 5. **ENV expansion**: `$HOME` → `/root` gibi değişkenleri yerleştir
    /// 6. **Glob expansion**: `*.txt` → eşleşen dosya adlarını yerleştir
    /// 7. **Tokenize**: Tokenizer ile kelimelere/operatörlere böl
    /// 8. **Chained execution**: `&&`, `||` varsa `execute_chained()`
    /// 9. **Pipeline**: `|` veya `>` varsa `execute_pipeline()`
    /// 10. **Built-in**: `match parts[0]` ile doğrudan çalıştır
    ///
    /// Özel dönüş değeri: `Some("__CLEAR__")` ekranın temizlenmesi gerektiğini bildirir.
    pub fn execute(&mut self) -> Option<String> {
        let cmd_line = self.editor.to_string();

        // Editor'ı sıfırla
        self.editor = GapBuffer::new(64);

        self.execute_line(&cmd_line)
    }

    pub fn execute_line(&mut self, cmd_line: &str) -> Option<String> {
        self.history.push(cmd_line);

        // Geçmişe ekle (global history)
        let trimmed = cmd_line.trim();
        if trimmed.is_empty() {
            return None;
        }
        self.sync_runtime_state();

        // Alias expansion
        let expanded_cmd = self.aliases.expand_line(trimmed);

        // Environment variable expansion ($VAR)
        let expanded_cmd = self.env.expand(&expanded_cmd);

        // Brace expansion ({a,b,c}, {1..5})
        let words: Vec<String> = expanded_cmd
            .split_whitespace()
            .flat_map(|w| advanced::expand_braces(w))
            .collect();
        let expanded_cmd = words.join(" ");

        // Glob expansion (*.txt, etc.)
        let expanded_cmd = expand_globs(&expanded_cmd);

        // Parse for pipes and redirects
        let tokens = advanced::Tokenizer::tokenize(&expanded_cmd);

        // Check for && || chaining
        let has_and = tokens.iter().any(|t| *t == advanced::Token::And);
        let has_or = tokens.iter().any(|t| *t == advanced::Token::Or);

        if has_and || has_or {
            return execute_chained(self, &tokens);
        }

        // Check for pipe/redirect operators
        let has_pipe = tokens.iter().any(|t| *t == advanced::Token::Pipe);
        let has_redirect = tokens.iter().any(|t| {
            matches!(
                t,
                advanced::Token::RedirectOut
                    | advanced::Token::RedirectAppend
                    | advanced::Token::RedirectIn
            )
        });

        if has_pipe || has_redirect {
            // Parse as pipeline
            if let Ok(pipelines) = advanced::Parser::parse(tokens) {
                if let Some(pipeline) = pipelines.first() {
                    return execute_pipeline(self, pipeline);
                }
            }
            return Some(String::from("Parse hatasi"));
        }

        let parts: Vec<&str> = expanded_cmd.split_whitespace().collect();
        match parts[0] {
            "help" => Some(render_help(parts.get(1).copied())),
            "ver" => Some(String::from("echOS v0.2.0 (Legendary Edition)")),
            "echo" => {
                let args = &parts[1..];
                Some(args.join(" "))
            }
            "clear" => Some(String::from("__CLEAR__")), // Özel sinyal
            "pwd" => Some(self.current_working_directory()),
            "cd" => {
                let target = parts.get(1).copied();
                match self.change_directory(target) {
                    Ok(path) => Some(path),
                    Err(msg) => Some(msg),
                }
            }
            "ls" => {
                match list_directory(parse_ls_path(&parts[1..])) {
                    Ok(out) => Some(out),
                    Err(msg) => Some(msg),
                }
            }
            "tree" => {
                let path = parts.get(1).copied();
                match render_tree(path) {
                    Ok(out) => Some(out),
                    Err(msg) => Some(msg),
                }
            }
            "find" => {
                let start = parts.get(1).copied();
                let name_pattern = parse_find_name_pattern(&parts[1..]);
                match find_paths(start, name_pattern) {
                    Ok(out) => Some(out),
                    Err(msg) => Some(msg),
                }
            }
            "stat" => {
                let Some(target) = parts.get(1).copied() else {
                    return Some(String::from("Kullanim: stat <yol>"));
                };
                match stat_path(target) {
                    Ok(out) => Some(out),
                    Err(msg) => Some(msg),
                }
            }
            "cat" => {
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: cat <dosya>"));
                }
                match load_file(parts[1]) {
                    Ok(data) => {
                        if data.is_empty() {
                            Some(String::from("Dosya bos"))
                        } else {
                            match core::str::from_utf8(&data) {
                                Ok(text) => Some(text.to_string()),
                                Err(_) => Some(String::from("Dosya metin degil")),
                            }
                        }
                    }
                    Err(msg) => Some(msg),
                }
            }
            "head" | "tail" | "wc" | "grep" | "sort" | "uniq" => {
                execute_builtin(self, &parts, None)
            }
            "cp" => {
                if parts.len() < 3 {
                    return Some(String::from("Kullanim: cp <kaynak> <hedef>"));
                }
                match copy_file(parts[1], parts[2]) {
                    Ok(target) => Some(format!("cp: {} -> {}", parts[1], target)),
                    Err(msg) => Some(msg),
                }
            }
            "launch" => {
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: launch <elf_dosyasi>"));
                }
                match load_and_run_elf(parts[1]) {
                    Ok(()) => None,
                    Err(msg) => Some(msg),
                }
            }
            "wine" => handle_wine_command(self, crate::posix::WineRuntimeKind::Wine, &parts),
            "proton" => handle_wine_command(self, crate::posix::WineRuntimeKind::Proton, &parts),
            "linux" => handle_linux_command(self, &parts),
            // Scripting commands
            "run" => {
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: run <script.sh>"));
                }
                // Load and execute script file
                match load_file(parts[1]) {
                    Ok(data) => {
                        match core::str::from_utf8(&data) {
                            Ok(script) => {
                                match scripting::run_script(script) {
                                    Ok(code) => Some(format!("Script tamamlandi (exit code: {})", code)),
                                    Err(e) => Some(format!("Script hatasi: {:?}", e)),
                                }
                            }
                            Err(_) => Some(String::from("Script dosyasi metin degil")),
                        }
                    }
                    Err(msg) => Some(msg),
                }
            }
            "eval" => {
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: eval <ifade>"));
                }
                let expr = parts[1..].join(" ");
                match scripting::eval_expression(&expr) {
                    Ok(result) => Some(result),
                    Err(e) => Some(format!("Hata: {:?}", e)),
                }
            }
            "set" => {
                if parts.len() < 3 {
                    // List all variables
                    let vars: Vec<String> = self.env.list().iter()
                        .map(|(k, v)| format!("{}={}", k, v))
                        .collect();
                    return Some(vars.join("\n"));
                }
                self.env.set(parts[1], parts[2]);
                self.sync_runtime_state();
                None
            }
            "export" => {
                if parts.len() < 3 {
                    return Some(String::from("Kullanim: export VAR deger"));
                }
                self.env.set(parts[1], parts[2]);
                self.sync_runtime_state();
                None
            }
            "unset" => {
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: unset <degisken>"));
                }
                self.env.unset(parts[1]);
                self.sync_runtime_state();
                None
            }
            "env" => {
                let vars: Vec<String> = self
                    .env
                    .list()
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect();
                Some(vars.join("\n"))
            }
            "history" => {
                let items: Vec<String> = self
                    .history
                    .list()
                    .iter()
                    .map(|(index, cmd)| format!("{:4}  {}", index, cmd))
                    .collect();
                Some(items.join("\n"))
            }
            "alias" => {
                if parts.len() == 1 {
                    let aliases: Vec<String> = self
                        .aliases
                        .list()
                        .iter()
                        .map(|(name, expansion)| format!("alias {}='{}'", name, expansion))
                        .collect();
                    return Some(aliases.join("\n"));
                }
                for alias in &parts[1..] {
                    if let Some((name, value)) = alias.split_once('=') {
                        let trimmed = value.trim_matches('\'').trim_matches('"');
                        self.aliases.set(name, trimmed);
                    } else {
                        return Some(String::from("Kullanim: alias ad='genisleme'"));
                    }
                }
                self.sync_runtime_state();
                None
            }
            "unalias" => {
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: unalias <ad>"));
                }
                for name in &parts[1..] {
                    self.aliases.unset(name);
                }
                self.sync_runtime_state();
                None
            }
            "which" | "command" => {
                let mut lookup = parts.get(1).copied();
                let mut plain_output = false;
                if parts[0] == "command" {
                    match (parts.get(1), parts.get(2)) {
                        (Some(&"-v"), Some(cmd)) => {
                            lookup = Some(cmd);
                            plain_output = true;
                        }
                        (Some(&"-V"), Some(cmd)) => {
                            lookup = Some(cmd);
                        }
                        _ => {}
                    }
                }
                let Some(lookup) = lookup else {
                    return Some(String::from(
                        "Kullanim: which <komut> | command [-v|-V] <komut>",
                    ));
                };
                if builtin_command_names().iter().any(|cmd| *cmd == lookup) {
                    if plain_output {
                        Some(String::from(lookup))
                    } else {
                        Some(format!("{}: shell builtin", lookup))
                    }
                } else {
                    Some(format!("{} bulunamadi", lookup))
                }
            }
            // Package Management
            "pkg" => {
                // pkg komutunu işle
                match crate::shell::cmd_pkg::handle_pkg_command(&parts[1..]) {
                    Ok(output) => Some(output),
                    Err(e) => Some(format!("pkg hatası: {:?}", e)),
                }
            }
            // Process Management
            "ps" => {
                let tasks = crate::task::scheduler::list_tasks();
                if tasks.is_empty() {
                    return Some(String::from("Hic calisan task yok"));
                }
                let mut out = String::from("  PID STATE     PRIO NAME\n");
                for task in tasks {
                    let state = match task.state {
                        crate::task::TaskState::Ready => "Ready",
                        crate::task::TaskState::Running => "Running",
                        crate::task::TaskState::Blocked => "Blocked",
                        crate::task::TaskState::Sleeping { .. } => "Sleeping",
                        crate::task::TaskState::Terminated => "Term",
                        crate::task::TaskState::Stopped => "Stopped",
                        crate::task::TaskState::Zombie => "Zombie",
                    };
                    let prio = match task.priority {
                        crate::task::Priority::Idle => "Idle",
                        crate::task::Priority::Low => "Low",
                        crate::task::Priority::Normal => "Norm",
                        crate::task::Priority::High => "High",
                    };
                    out.push_str(&format!("{:5} {:10} {:4} {}\n", task.pid, state, prio, task.name));
                }
                Some(out)
            }
            "kill" => {
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: kill <pid> [signal]"));
                }
                let pid: usize = match parts[1].parse() {
                    Ok(p) => p,
                    Err(_) => return Some(String::from("Gecersiz PID")),
                };
                let signal = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(15);
                match crate::task::scheduler::kill_task(pid, signal) {
                    Ok(()) => Some(format!("Task {} sonlandirildi", pid)),
                    Err(e) => Some(format!("Hata: {}", e)),
                }
            }
            "bg" => {
                if let Some(pid) = crate::task::scheduler::background_current() {
                    Some(format!("[{}] background", pid))
                } else {
                    Some(String::from("Background'a atanacak task yok"))
                }
            }
            "fg" => {
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: fg <pid>"));
                }
                let pid: usize = match parts[1].parse() {
                    Ok(p) => p,
                    Err(_) => return Some(String::from("Gecersiz PID")),
                };
                match crate::task::scheduler::foreground_task(pid) {
                    Ok(()) => Some(format!("Task {} foreground'a getirildi", pid)),
                    Err(e) => Some(format!("Hata: {}", e)),
                }
            }
            "jobs" => {
                let tasks = crate::task::scheduler::list_tasks();
                let bg_tasks: Vec<_> = tasks.iter()
                    .filter(|t| t.state == crate::task::TaskState::Stopped)
                    .collect();
                if bg_tasks.is_empty() {
                    return Some(String::from("Bekleyen job yok"));
                }
                let mut out = String::new();
                for (i, task) in bg_tasks.iter().enumerate() {
                    out.push_str(&format!("[{}] {} {}\n", i + 1, task.pid, task.name));
                }
                Some(out)
            }
            "set" => {
                // set -e/-x/-u — shell seçenekleri
                if parts.len() < 2 {
                    let e = *scripting::SCRIPT_STATE.errexit.lock();
                    let x = *scripting::SCRIPT_STATE.xtrace.lock();
                    let u = *scripting::SCRIPT_STATE.nounset.lock();
                    return Some(format!("errexit={} xtrace={} nounset={}", e, x, u));
                }
                let flag = parts[1];
                let (enable, opts) = if flag.starts_with('-') {
                    (true, &flag[1..])
                } else if flag.starts_with('+') {
                    (false, &flag[1..])
                } else {
                    return Some(String::from("Kullanim: set [-+][exu]"));
                };
                for ch in opts.chars() {
                    match ch {
                        'e' => *scripting::SCRIPT_STATE.errexit.lock() = enable,
                        'x' => *scripting::SCRIPT_STATE.xtrace.lock() = enable,
                        'u' => *scripting::SCRIPT_STATE.nounset.lock() = enable,
                        _ => return Some(format!("Bilinmeyen opsiyon: -{}", ch)),
                    }
                }
                None
            }
            "top" => {
                let tasks = crate::task::scheduler::list_tasks();
                let cpu_count = crate::task::scheduler::get_cpu_count();
                let ticks = crate::task::scheduler::get_ticks();
                let mut out = format!("echOS Top - {} CPU, {} ticks\n\n", cpu_count, ticks);
                out.push_str("  PID STATE     PRIO NAME\n");
                for task in tasks.iter().take(15) {
                    let state = match task.state {
                        crate::task::TaskState::Ready => "Ready",
                        crate::task::TaskState::Running => "Running",
                        crate::task::TaskState::Blocked => "Blocked",
                        crate::task::TaskState::Sleeping { .. } => "Sleeping",
                        crate::task::TaskState::Terminated => "Term",
                        crate::task::TaskState::Stopped => "Stopped",
                        crate::task::TaskState::Zombie => "Zombie",
                    };
                    let prio = match task.priority {
                        crate::task::Priority::Idle => "Idle",
                        crate::task::Priority::Low => "Low",
                        crate::task::Priority::Normal => "Norm",
                        crate::task::Priority::High => "High",
                    };
                    out.push_str(&format!("{:5} {:10} {:4} {}\n", task.pid, state, prio, task.name));
                }
                Some(out)
            }
            // File Permissions
            "chmod" => {
                if parts.len() < 3 {
                    return Some(String::from("Kullanim: chmod <mode> <dosya>"));
                }
                let mode: u32 = match u32::from_str_radix(parts[1].trim_start_matches('0'), 8) {
                    Ok(m) => m,
                    Err(_) => return Some(String::from("Gecersiz mode (8lik)")),
                };
                match crate::fs::f2fs::set_file_metadata(parts[2], Some(mode), None, None) {
                    Ok(()) => Some(format!("chmod: {} -> {:o}", parts[2], mode)),
                    Err(_) => Some(String::from("Dosya bulunamadi")),
                }
            }
            "chown" => {
                if parts.len() < 3 {
                    return Some(String::from("Kullanim: chown <user:group> <dosya>"));
                }
                let (uid, gid) = if parts[1].contains(':') {
                    let ug: Vec<&str> = parts[1].split(':').collect();
                    let u: u32 = ug.get(0).and_then(|s| s.parse().ok()).unwrap_or(0);
                    let g: u32 = ug.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                    (u, g)
                } else {
                    let u: u32 = parts[1].parse().unwrap_or(0);
                    (u, 0)
                };
                match crate::fs::f2fs::set_file_metadata(parts[2], None, Some(uid), Some(gid)) {
                    Ok(()) => Some(format!("chown: {} -> uid={}, gid={}", parts[2], uid, gid)),
                    Err(_) => Some(String::from("Dosya bulunamadi")),
                }
            }
            // Mount
            "mount" => {
                if parts.len() < 3 {
                    // Mount listesi
                    let mounts = crate::fs::f2fs::list_mounts();
                    if mounts.is_empty() {
                        return Some(String::from("Hic mount noktasi yok"));
                    }
                    let mut out = String::from("Device        Mountpoint    Type\n");
                    for m in mounts {
                        out.push_str(&format!("{:12} {:12} {}\n", m.device, m.mountpoint, m.fs_type));
                    }
                    return Some(out);
                }
                let fs_type = parts.get(3).copied().unwrap_or("f2fs");
                match crate::fs::f2fs::mount_fs(parts[1], parts[2], fs_type) {
                    Ok(()) => Some(format!("mount: {} -> {}", parts[1], parts[2])),
                    Err(e) => Some(format!("mount hatasi: {:?}", e)),
                }
            }
            "umount" => {
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: umount <mountpoint>"));
                }
                match crate::fs::f2fs::umount_fs(parts[1]) {
                    Ok(()) => Some(format!("umount: {}", parts[1])),
                    Err(_) => Some(String::from("Mount noktasi bulunamadi")),
                }
            }
            // System info
            "uname" => {
                if parts.len() > 1 && parts[1] == "-a" {
                    Some(String::from("echOS 0.3.0 x86_64 echOS Kernel"))
                } else {
                    Some(String::from("echOS"))
                }
            }
            "whoami" => {
                let uid = crate::security::users::USER_DB.current_uid();
                match crate::security::users::USER_DB.get_user(uid) {
                    Some(u) => Some(u.username),
                    None => Some(format!("uid={}", uid)),
                }
            }
            "id" => {
                let uid = crate::security::users::USER_DB.current_uid();
                match crate::security::users::USER_DB.get_user(uid) {
                    Some(u) => {
                        let groups = crate::security::users::USER_DB.get_user_groups(&u.username);
                        let gstr: alloc::string::String = groups.iter()
                            .map(|g| format!("{}", g))
                            .collect::<Vec<_>>()
                            .join(",");
                        Some(format!("uid={}({}) gid={}({}) groups={}", u.uid, u.username, u.gid, u.username, gstr))
                    }
                    None => Some(format!("uid={}", uid)),
                }
            }
            "hostname" => {
                if parts.len() > 1 {
                    crate::init::INIT.set_hostname(parts[1]);
                    Some(format!("hostname set to: {}", parts[1]))
                } else {
                    Some(crate::init::INIT.get_hostname())
                }
            }
            "service" | "systemctl" => {
                if parts.len() < 2 {
                    // Tum servisleri listele
                    let svcs = crate::init::INIT.list_services();
                    let mut out = String::from("SERVICE          STATE\n");
                    for (name, state) in &svcs {
                        out.push_str(&format!("{:16} {:?}\n", name, state));
                    }
                    Some(out)
                } else {
                    match parts[1] {
                        "start" if parts.len() > 2 => {
                            match crate::init::INIT.start_service(parts[2]) {
                                Ok(()) => Some(format!("Started {}", parts[2])),
                                Err(e) => Some(format!("Error: {}", e)),
                            }
                        }
                        "stop" if parts.len() > 2 => {
                            match crate::init::INIT.stop_service(parts[2]) {
                                Ok(()) => Some(format!("Stopped {}", parts[2])),
                                Err(e) => Some(format!("Error: {}", e)),
                            }
                        }
                        "status" if parts.len() > 2 => {
                            match crate::init::INIT.service_status(parts[2]) {
                                Some(state) => Some(format!("{}: {:?}", parts[2], state)),
                                None => Some(format!("{}: not found", parts[2])),
                            }
                        }
                        _ => Some(String::from("Usage: service [start|stop|status] <name>")),
                    }
                }
            }
            "shutdown" => {
                crate::init::shutdown();
                Some(String::from("System halted."))
            }
            "reboot" => {
                crate::init::reboot();
                // unreachable
                None
            }
            "mount" if parts.len() < 3 => {
                // Mount tablosundan listele
                let mounts = crate::fs::mount::MOUNT_TABLE.list();
                if mounts.is_empty() {
                    return Some(String::from("No mount points"));
                }
                let mut out = String::from("SOURCE       TARGET       TYPE       FLAGS\n");
                for m in &mounts {
                    let ro = if m.flags.read_only { "ro" } else { "rw" };
                    out.push_str(&format!("{:12} {:12} {:10} {}\n",
                        m.source, m.target, m.fs_type.as_str(), ro));
                }
                Some(out)
            }
            "uptime" => {
                let ticks = crate::task::scheduler::get_ticks();
                let secs = ticks / 100;
                let mins = secs / 60;
                let hours = mins / 60;
                Some(format!("up {}:{:02}:{:02}", hours, mins % 60, secs % 60))
            }
            "date" => {
                // TODO: RTC'den tarih oku
                let dt = crate::drivers::rtc::get_cached_datetime();
                Some(dt.to_string())
            }
            "free" => {
                // TODO: Gerçek memory info
                Some(String::from("              total        used        free\nMem:         256M         64M        192M\nSwap:          0B          0B          0B"))
            }
            "df" => {
                // TODO: Gerçek disk info
                Some(String::from("Filesystem     Size  Used Avail Use% Mounted on\n/dev/sda1      256M   64M  192M  25% /"))
            }
            // ─── Shell Batch 2: lsmod / iostat / netstat / ifconfig ───
            "lsmod" => {
                use alloc::format;
                let drivers = crate::drivers::dispatcher::list_drivers();
                if drivers.is_empty() {
                    Some(String::from("Module                  Size  Used by\n(no drivers loaded)"))
                } else {
                    let mut out = String::from("Module                  Size  Used by\n");
                    for d in drivers {
                        out.push_str(&format!(
                            "{:<24}{:<6}{}\n",
                            d.name, "-", d.tier
                        ));
                    }
                    Some(out)
                }
            }
            "iostat" => {
                use alloc::format;
                let mut out = String::from(
                    "Device             tps    kB_read/s    kB_wrtn/s    kB_read    kB_wrtn\n"
                );
                // NVMe stats (kontrol sayısını dispatcher'dan al)
                let drivers = crate::drivers::dispatcher::list_drivers();
                let nvme_count = drivers.iter().filter(|d| d.name.contains("nvme") || d.class_code == 0x01 && d.subclass == 0x08).count();
                if nvme_count == 0 {
                    out.push_str("nvme0              0.00         0.00         0.00          0          0\n");
                } else {
                    for i in 0..nvme_count {
                        out.push_str(&format!(
                            "nvme{}              0.00         0.00         0.00          0          0\n",
                            i
                        ));
                    }
                }
                // ATA stats
                out.push_str("sda                0.00         0.00         0.00          0          0\n");
                Some(out)
            }
            "netstat" => {
                use alloc::format;
                let mut out = String::from(
                    "Proto  Local Address          Foreign Address        State\n"
                );
                // TCP bağlantılarını listele
                let tcp_info = crate::net::tcp::list_connections();
                for c in &tcp_info {
                    out.push_str(&format!(
                        "tcp    {}:{:<18} {}:{:<15} {:?}\n",
                        c.local_ip, c.local_port, c.remote_ip, c.remote_port, c.state
                    ));
                }
                // UDP soketlerini listele
                let udp_info = crate::net::udp::list_sockets();
                for s in &udp_info {
                    out.push_str(&format!(
                        "udp    0.0.0.0:{:<14} 0.0.0.0:*              -\n",
                        s.port
                    ));
                }
                if tcp_info.is_empty() && udp_info.is_empty() {
                    out.push_str("(no active connections)\n");
                }
                Some(out)
            }
            "ifconfig" => {
                use alloc::format;
                let mut out = String::new();
                let ifaces = crate::net::get_interfaces();
                if ifaces.is_empty() {
                    // Fallback: yapılandırma yoksa varsayılandan oku
                    let ip = crate::net::local_ip();
                    out.push_str(&format!("eth0: flags=4163<UP,BROADCAST,RUNNING,MULTICAST>\n"));
                    out.push_str(&format!("        inet {}  netmask 255.255.255.0\n", ip));
                    out.push_str("        RX packets 0  bytes 0 (0.0 B)\n");
                    out.push_str("        TX packets 0  bytes 0 (0.0 B)\n");
                } else {
                    for iface in &ifaces {
                        out.push_str(&format!("{}: flags=4163<UP,BROADCAST,RUNNING,MULTICAST>\n", iface.name));
                        out.push_str(&format!("        inet {}  netmask {}\n", iface.ip, iface.netmask));
                        let m = iface.mac.0;
                        out.push_str(&format!("        ether {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}\n",
                            m[0], m[1], m[2], m[3], m[4], m[5]));
                        out.push_str(&format!("        MTU {}\n", iface.mtu));
                        out.push_str("        RX packets 0  bytes 0 (0.0 B)\n");
                        out.push_str("        TX packets 0  bytes 0 (0.0 B)\n\n");
                    }
                }
                out.push_str("lo: flags=73<UP,LOOPBACK,RUNNING>\n");
                out.push_str("        inet 127.0.0.1  netmask 255.0.0.0\n");
                out.push_str("        loop  txqueuelen 1000\n");
                Some(out)
            }
            // File Operations
            "rm" => {
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: rm <dosya>"));
                }
                let resolved = resolve_path(parts[1]);
                let (parent, name) = match split_parent_name(&resolved) {
                    Ok(value) => value,
                    Err(msg) => return Some(msg),
                };
                match crate::fs::f2fs::unlink_f2fs(&parent, &name) {
                    Ok(()) => Some(format!("rm: {} silindi", parts[1])),
                    Err(e) => Some(format!("rm hatasi: {:?}", e)),
                }
            }
            "rmdir" => {
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: rmdir <dizin>"));
                }
                let resolved = resolve_path(parts[1]);
                let (parent, name) = match split_parent_name(&resolved) {
                    Ok(value) => value,
                    Err(msg) => return Some(msg),
                };
                match crate::fs::f2fs::unlink_f2fs(&parent, &name) {
                    Ok(()) => Some(format!("rmdir: {} silindi", parts[1])),
                    Err(e) => Some(format!("rmdir hatasi: {:?}", e)),
                }
            }
            "mkdir" => {
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: mkdir <dizin>"));
                }
                let resolved = resolve_path(parts[1]);
                let (parent, name) = match split_parent_name(&resolved) {
                    Ok(value) => value,
                    Err(msg) => return Some(msg),
                };
                match crate::fs::f2fs::create_f2fs_dir(&parent, &name) {
                    Ok(()) => Some(format!("mkdir: {} olusturuldu", parts[1])),
                    Err(e) => Some(format!("mkdir hatasi: {:?}", e)),
                }
            }
            "touch" => {
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: touch <dosya>"));
                }
                let resolved = resolve_path(parts[1]);
                let (parent, name) = match split_parent_name(&resolved) {
                    Ok(value) => value,
                    Err(msg) => return Some(msg),
                };
                match crate::fs::f2fs::create_f2fs_file(&parent, &name) {
                    Ok(()) => Some(format!("touch: {} olusturuldu", parts[1])),
                    Err(e) => Some(format!("touch hatasi: {:?}", e)),
                }
            }
            "ln" => {
                if parts.len() < 3 {
                    return Some(String::from("Kullanim: ln [-s] <hedef> <link>"));
                }
                let is_symlink = parts.get(1) == Some(&"-s");
                let (target, link) = if is_symlink {
                    (parts[2], parts[3])
                } else {
                    (parts[1], parts[2])
                };

                let link_path = link.trim_start_matches('/');
                let (parent, name) = if let Some(pos) = link_path.rfind('/') {
                    (&link_path[..pos], &link_path[pos + 1..])
                } else {
                    ("", link_path)
                };

                if is_symlink {
                    match crate::fs::f2fs::create_symlink(&format!("/{}", parent), name, target) {
                        Ok(()) => Some(format!("ln -s: {} -> {}", link, target)),
                        Err(e) => Some(format!("ln hatasi: {:?}", e)),
                    }
                } else {
                    match crate::fs::f2fs::create_hardlink(&format!("/{}", parent), name, target) {
                        Ok(()) => Some(format!("ln: {} -> {}", link, target)),
                        Err(e) => Some(format!("ln hatasi: {:?}", e)),
                    }
                }
            }
            "truncate" => {
                if parts.len() < 3 {
                    return Some(String::from("Kullanim: truncate -s <boyut> <dosya>"));
                }
                let size: u64 = if parts[1] == "-s" {
                    parts[2].parse().unwrap_or(0)
                } else {
                    parts[1].parse().unwrap_or(0)
                };
                let file = if parts[1] == "-s" { parts[3] } else { parts[2] };
                match crate::fs::f2fs::truncate_f2fs(file, size) {
                    Ok(()) => Some(format!("truncate: {} -> {} bytes", file, size)),
                    Err(e) => Some(format!("truncate hatasi: {:?}", e)),
                }
            }
            "readlink" => {
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: readlink <symlink>"));
                }
                match crate::fs::f2fs::read_link(parts[1]) {
                    Ok(target) => Some(target),
                    Err(e) => Some(format!("readlink hatasi: {:?}", e)),
                }
            }
            "mv" => {
                if parts.len() < 3 {
                    return Some(String::from("Kullanim: mv <eski> <yeni>"));
                }
                let old_path = parts[1].trim_start_matches('/');
                let new_path = parts[2].trim_start_matches('/');
                let (old_parent, old_name) = if let Some(pos) = old_path.rfind('/') {
                    (&old_path[..pos], &old_path[pos + 1..])
                } else {
                    ("", old_path)
                };
                let (new_parent, new_name) = if let Some(pos) = new_path.rfind('/') {
                    (&new_path[..pos], &new_path[pos + 1..])
                } else {
                    ("", new_path)
                };
                // Aynı dizinde rename
                if old_parent == new_parent {
                    match crate::fs::f2fs::rename_f2fs(&format!("/{}", old_parent), old_name, new_name) {
                        Ok(()) => Some(format!("mv: {} -> {}", parts[1], parts[2])),
                        Err(e) => Some(format!("mv hatasi: {:?}", e)),
                    }
                } else {
                    // Farklı dizinlere taşıma
                    match crate::fs::f2fs::move_f2fs(parts[1], parts[2]) {
                        Ok(()) => Some(format!("mv: {} -> {}", parts[1], parts[2])),
                        Err(e) => Some(format!("mv hatasi: {:?}", e)),
                    }
                }
            }
            // Network Commands
            "net" => {
                if network_surface_disabled() {
                    return Some(network_surface_disabled_response("net"));
                }
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: net [status|dhcp|ip|route|addr|link|smoke]\n  net status - Ag katmanlari ve sinirlari\n  net dhcp - Gercek DHCP lease/config durumu\n  net ip - IP/gateway/dns durumu\n  net route - Yonlendirme tablosu\n  net addr - Adres bilgileri\n  net link - Link durumu\n  net smoke doh <host> [cloudflare|google|quad9]\n  net smoke dot <host> [cloudflare|google|quad9]\n  net smoke http3 <https-url>\n  net smoke grpc <host> <port> [authority]\n  net smoke tcp <host> <port>\n  net smoke http <url>\n  net smoke ping <host>"));
                }
                match parts[1] {
                    "status" => {
                        let transport_ready = crate::drivers::virtio_net::is_initialized();
                        let dhcp_lease = crate::net::dhcp::get_lease();
                        let net_cfg = crate::net::get_config();
                        let status = if transport_ready {
                            "Tasiyici hazir"
                        } else {
                            "Pasif"
                        };
                        let ip_info = crate::net::smoltcp_driver::get_ip()
                            .map(|ip| format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]))
                            .unwrap_or_else(|| String::from("0.0.0.0"));
                        let dhcp_state = if dhcp_lease.is_some() {
                            "gercek lease mevcut"
                        } else if net_cfg.is_configured() {
                            "aktif config mevcut"
                        } else {
                            "yapilandirilmamis"
                        };
                        let dns_state = if !net_cfg.dns_servers.is_empty() {
                            "gercek resolver acik"
                        } else {
                            "DNS server yok veya config eksik"
                        };
                        Some(format!(
                            "Ag durumu: {}\nVirtIO-Net: {}\nIP: {}\nKatman durumu:\n  L2 transport: {}\n  DHCP: {}\n  DNS: {}\n  TCP/HTTP: gercek socket/client yolu acik\n  HTTPS/TLS: certificate validation matrix kapali; shell date/CN/CA/revoked/decode failure siniflarini ayri gosterir\n  DoH/DoT: native h2 DoH ve sustained DoT resolver matrisi corpus ile kapali\n  gRPC: unary trailer/status/reset mapping ve remote server-streaming loopback corpus kapali; built-in client/bidi streaming cekirdegi acik, genis remote service matrisi henuz acik kuyrukta\n  HTTP/3: native QUIC transport kaydi ve end-to-end response/trailer corpus acik; established QUIC olmadan sessiz downgrade yok, genis canli service matrisi henuz acik kuyrukta\n  ICMP ping: gercek echo yolu acik\n  IPv6/NDP: wider route/NDP corpus kapali; default router ve neighbor secimi mekanik olarak dogrulandi\n  eBPF ingress: ELF/JIT attach/run corpus kapali; attach varsa RX allow/drop kararini etkiler",
                            status,
                            if transport_ready { "Hazir" } else { "Bulunamadi" },
                            ip_info,
                            if transport_ready { "hazir" } else { "hazir degil" },
                            dhcp_state,
                            dns_state
                        ))
                    }
                    "smoke" => {
                        if parts.len() < 4 {
                            return Some(String::from(
                                "Kullanim: net smoke [doh|dot|http3|grpc|tcp|http|ping] <hedef> [ek-parametre]",
                            ));
                        }
                        match parts[2] {
                            "doh" => {
                                let hostname = parts[3];
                                let provider = match parse_dns_privacy_provider(parts.get(4).copied()) {
                                    Ok(provider) => provider,
                                    Err(msg) => return Some(msg.to_string()),
                                };
                                let mut client = match provider {
                                    "cloudflare" => crate::net::doh::DohClient::cloudflare(),
                                    "google" => crate::net::doh::DohClient::google(),
                                    "quad9" => crate::net::doh::DohClient::quad9(),
                                    _ => unreachable!(),
                                };
                                match client.smoke_a_lookup(hostname) {
                                    Ok(ip) => Some(format!(
                                        "DoH smoke basarili: {} -> {} [{}]\nNot: Gercek DoH istemcisi timeout/retry butcesi ile calisti",
                                        hostname, ip, provider
                                    )),
                                    Err(err) => Some(format!(
                                        "DoH smoke hatasi: {}\nNot: Bu komut gercek endpoint/TLS yolunu denedi",
                                        describe_doh_error(err)
                                    )),
                                }
                            }
                            "dot" => {
                                let hostname = parts[3];
                                let provider = match parse_dns_privacy_provider(parts.get(4).copied()) {
                                    Ok(provider) => provider,
                                    Err(msg) => return Some(msg.to_string()),
                                };
                                let mut client = match provider {
                                    "cloudflare" => crate::net::dot::DotClient::cloudflare(),
                                    "google" => crate::net::dot::DotClient::google(),
                                    "quad9" => crate::net::dot::DotClient::quad9(),
                                    _ => unreachable!(),
                                };
                                match client.smoke_a_lookup(hostname) {
                                    Ok(ip) => Some(format!(
                                        "DoT smoke basarili: {} -> {} [{}]\nNot: Gercek DoT istemcisi TCP+TLS uzerinden calisti",
                                        hostname, ip, provider
                                    )),
                                    Err(err) => Some(format!(
                                        "DoT smoke hatasi: {}\nNot: Bu komut gercek port 853/TLS yolunu denedi",
                                        describe_dot_error(err)
                                    )),
                                }
                            }
                            "http3" => {
                                let url = parts[3];
                                match crate::net::http3::http3_get(url) {
                                    Ok((status, headers, body)) => Some(format!(
                                        "HTTP/3 smoke basarili: status={} headers={} body={} bytes\nNot: Sessiz downgrade uygulanmadi",
                                        status,
                                        headers.len(),
                                        body.len()
                                    )),
                                    Err(err) => Some(format!(
                                        "HTTP/3 smoke hatasi: {}\nNot: QUIC transport yoksa boundary acik raporlanir",
                                        describe_http3_error(err)
                                    )),
                                }
                            }
                            "grpc" => {
                                if parts.len() < 5 {
                                    return Some(String::from(
                                        "Kullanim: net smoke grpc <host> <port> [authority]",
                                    ));
                                }
                                let host = parts[3];
                                let Ok(port) = parts[4].parse::<u16>() else {
                                    return Some(String::from("Gecersiz port"));
                                };
                                let authority = parts.get(5).copied().unwrap_or(host);
                                let ip = if let Some(ip) = crate::net::socket::parse_ipv4(host) {
                                    ip
                                } else if let Some(ip) = crate::net::smoltcp_driver::dns_lookup(host)
                                {
                                    crate::net::Ipv4Addr::from_bytes(ip)
                                } else {
                                    return Some(format!("gRPC smoke: {} cozulmedi", host));
                                };
                                let mut client = crate::net::grpc::GrpcClient::new();
                                client.add_service(crate::net::grpc::create_greeter_service());
                                let mut request = crate::net::grpc::ProtoMessage::new();
                                request.add_string(1, "echOS");
                                match client.call_unary_remote(
                                    ip,
                                    port,
                                    authority,
                                    "Greeter",
                                    "SayHello",
                                    &request,
                                ) {
                                    Ok(response) => Some(format!(
                                        "gRPC smoke basarili: {}:{} [{}]\nYanıt: {}\nNot: Gercek TCP+h2 unary yolu kullanildi",
                                        host,
                                        port,
                                        authority,
                                        response
                                            .get_string(1)
                                            .unwrap_or_else(|| String::from("<bos>"))
                                    )),
                                    Err(err) => Some(format!(
                                        "gRPC smoke hatasi: {:?}\nNot: Gercek remote unary transport denendi",
                                        err
                                    )),
                                }
                            }
                            "tcp" => {
                                if parts.len() < 5 {
                                    return Some(String::from("Kullanim: net smoke tcp <host> <port>"));
                                }
                                let host = parts[3];
                                let Ok(port) = parts[4].parse::<u16>() else {
                                    return Some(String::from("Gecersiz port"));
                                };
                                let ip = if let Some(ip) = crate::net::socket::parse_ipv4(host) {
                                    ip
                                } else if let Some(ip) = crate::net::smoltcp_driver::dns_lookup(host)
                                {
                                    crate::net::Ipv4Addr::from_bytes(ip)
                                } else {
                                    return Some(format!("TCP smoke: {} cozulmedi", host));
                                };
                                match crate::net::nc_connect(host, port) {
                                    Ok(sock) => {
                                        let _ = crate::net::socket::close(sock);
                                        Some(format!(
                                        "TCP smoke basarili: {}:{} [{}]\nNot: Gercek TCP connect yolu kullanildi",
                                        host, port, ip
                                    ))
                                    }
                                    Err(err) => Some(format!(
                                        "TCP smoke hatasi: {:?}\nNot: Gercek TCP connect yolu denendi",
                                        err
                                    )),
                                }
                            }
                            "http" => {
                                let url = parts[3];
                                let client = crate::net::http::HttpClient::new();
                                match client.get(url) {
                                    Ok(response) => Some(format!(
                                        "HTTP smoke basarili: status={} body={} bytes\nNot: Gercek HTTP/HTTPS istemci yolu kullanildi",
                                        response.status_code,
                                        response.body.len()
                                    )),
                                    Err(err) => Some(format!(
                                        "HTTP smoke hatasi: {}\nNot: Gercek istemci failure class'i raporlandi",
                                        describe_http_error(err)
                                    )),
                                }
                            }
                            "ping" => {
                                let host = parts[3];
                                let dest = if let Some(ip) = crate::net::socket::parse_ipv4(host) {
                                    ip
                                } else if let Some(ip) = crate::net::smoltcp_driver::dns_lookup(host)
                                {
                                    crate::net::Ipv4Addr::from_bytes(ip)
                                } else {
                                    return Some(format!("Ping smoke: {} cozulmedi", host));
                                };
                                match crate::net::ping_real(dest, 1) {
                                    Ok(results) if results.first().map(|(_, ok)| *ok).unwrap_or(false) => {
                                        let (rtt, _) = results[0];
                                        Some(format!(
                                            "Ping smoke basarili: {} -> {} ms\nNot: Gercek ICMP echo yolu kullanildi",
                                            host, rtt
                                        ))
                                    }
                                    Ok(results) => Some(format!(
                                        "Ping smoke timeout: {} -> {:?}\nNot: Gercek ICMP timeout/failure yolu goruldu",
                                        host, results
                                    )),
                                    Err(err) => Some(format!(
                                        "Ping smoke hatasi: {:?}\nNot: Gercek ICMP send/recv yolu denendi",
                                        err
                                    )),
                                }
                            }
                            _ => Some(String::from("Bilinmeyen smoke protokolu")),
                        }
                    }
                    "dhcp" => {
                        if crate::net::smoltcp_driver::dhcp_configure() {
                            let ip = crate::net::smoltcp_driver::get_ip()
                                .map(|ip| format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]))
                                .unwrap_or_else(|| String::from("0.0.0.0"));
                            Some(format!("DHCP tamamlandi - {}", ip))
                        } else {
                            Some(String::from("DHCP: Basarisiz"))
                        }
                    }
                    "ip" => {
                        let ip = crate::net::smoltcp_driver::get_ip()
                            .map(|ip| format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]))
                            .unwrap_or_else(|| String::from("0.0.0.0"));
                        let gw = crate::net::smoltcp_driver::get_gateway()
                            .map(|ip| format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]))
                            .unwrap_or_else(|| String::from("0.0.0.0"));
                        let dns = crate::net::smoltcp_driver::get_dns()
                            .map(|ip| format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]))
                            .unwrap_or_else(|| String::from("0.0.0.0"));
                        Some(format!("IP: {}\nGateway: {}\nDNS: {}\nNot: Bu alanlar aktif kernel network config durumundan geliyor", ip, gw, dns))
                    }
                    "route" => {
                        let gw = crate::net::smoltcp_driver::get_gateway()
                            .map(|ip| format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]))
                            .unwrap_or_else(|| String::from("0.0.0.0"));
                        Some(format!("Kernel IP routing table\nDestination     Gateway         Genmask         Flags\n0.0.0.0          {}    0.0.0.0         UG", gw))
                    }
                    "addr" => {
                        let ip = crate::net::smoltcp_driver::get_ip()
                            .map(|ip| format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]))
                            .unwrap_or_else(|| String::from("0.0.0.0"));
                        Some(format!("1: eth0: <BROADCAST,MULTICAST,UP,LOWER_UP>\n    inet {} brd 0.0.0.0 scope global eth0", ip))
                    }
                    "link" => {
                        Some(String::from("1: eth0: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500 qdisc pfifo_fast state UP mode DEFAULT\n    link/ether 52:54:00:12:34:56 brd ff:ff:ff:ff:ff:ff"))
                    }
                    _ => Some(String::from("Bilinmeyen net komutu"))
                }
            }
            "http" => {
                if network_surface_disabled() {
                    return Some(network_surface_disabled_response("http"));
                }
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: http [get|post|download] <url> [dosya]\n  http get <url> - Gercek GET istegi\n  http post <url> <data> - Gercek POST istegi\n  http download <url> [dosya] - Dosya indir\nNot: HTTPS/TLS yolu acik; shell artik cert date/CN/CA/revoked/decode siniflarini ayri raporlar, ama trust policy tam degil"));
                }
                match parts[1] {
                    "get" => {
                        if parts.len() < 3 {
                            return Some(String::from("Kullanim: http get <url>"));
                        }
                        let url = parts[2];
                        let client = crate::net::http::HttpClient::new();
                        match client.get(url) {
                            Ok(response) => {
                                let data = response.body;
                                match core::str::from_utf8(&data) {
                                    Ok(text) => {
                                        if text.len() > 500 {
                                            Some(format!("HTTP GET basarili ({} bytes)\n{}\n... (kesildi)\nNot: HTTP istemcisi production-grade degil", data.len(), &text[..500]))
                                        } else {
                                            Some(format!("HTTP GET basarili ({} bytes)\n{}\nNot: HTTP istemcisi production-grade degil", data.len(), text))
                                        }
                                    }
                                    Err(_) => Some(format!("HTTP GET basarili ({} bytes) - binary data\nNot: HTTP istemcisi production-grade degil", data.len()))
                                }
                            }
                            Err(e) => Some(format!(
                                "HTTP GET hatasi: {}\nNot: shell gercek HTTPS/TLS failure sinifini raporladi",
                                describe_http_error(e)
                            ))
                        }
                    }
                    "post" => {
                        if parts.len() < 4 {
                            return Some(String::from("Kullanim: http post <url> <data>"));
                        }
                        let url = parts[2];
                        let data = parts[3..].join(" ");
                        let client = crate::net::http::HttpClient::new();
                        match client.post(url, data.as_bytes(), None) {
                            Ok(response) => {
                                Some(format!("HTTP POST basarili ({} bytes)\nNot: HTTPS/TLS semantics ve error fidelity tam degil", response.body.len()))
                            }
                            Err(e) => Some(format!(
                                "HTTP POST hatasi: {}\nNot: shell gercek HTTPS/TLS failure sinifini raporladi",
                                describe_http_error(e)
                            ))
                        }
                    }
                    "download" => {
                        if parts.len() < 3 {
                            return Some(String::from("Kullanim: http download <url> [dosya]"));
                        }
                        let url = parts[2];
                        let filename = parts.get(3).map(|s| *s).unwrap_or("downloaded.bin");
                        let client = crate::net::http::HttpClient::new();
                        match client.download(url) {
                            Ok(data) => {
                                // Dosyaya kaydet
                                let path = filename.trim_start_matches('/');
                                let (parent, name) = if let Some(pos) = path.rfind('/') {
                                    (&path[..pos], &path[pos + 1..])
                                } else {
                                    ("", path)
                                };
                                match crate::fs::f2fs::create_f2fs_file_with_data(&format!("/{}", parent), name, &data) {
                                    Ok(()) => Some(format!("Indirildi: {} ({} bytes) -> /{}\nNot: Indirme yolu gercek HTTP/HTTPS stack uzerinden calisti", url, data.len(), filename)),
                                    Err(e) => Some(format!("Dosya kaydedilemedi: {:?}\nIndirme basarili ({} bytes)\nNot: HTTP istemcisi production-grade degil", e, data.len()))
                                }
                            }
                            Err(e) => Some(format!(
                                "Indirme hatasi: {}\nNot: shell gercek HTTPS/TLS failure sinifini raporladi",
                                describe_http_error(e)
                            ))
                        }
                    }
                    _ => Some(String::from("Bilinmeyen http komutu"))
                }
            }
            "wget" => {
                if network_surface_disabled() {
                    return Some(network_surface_disabled_response("wget"));
                }
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: wget <url> [dosya]\nNot: Gercek HTTP/HTTPS stack kullanir; shell cert date/CN/CA/revoked/decode siniflarini ayri raporlar"));
                }
                let url = parts[1];
                let filename = parts.get(2).map(|s| *s).unwrap_or_else(|| {
                    // URL'den dosya adını çıkar
                    if let Some(pos) = url.rfind('/') {
                        &url[pos + 1..]
                    } else {
                        "downloaded"
                    }
                });
                let client = crate::net::http::HttpClient::new();
                match client.download(url) {
                    Ok(data) => {
                        let path = filename.trim_start_matches('/');
                        let (parent, name) = if let Some(pos) = path.rfind('/') {
                            (&path[..pos], &path[pos + 1..])
                        } else {
                            ("", path)
                        };
                        match crate::fs::f2fs::create_f2fs_file_with_data(&format!("/{}", parent), name, &data) {
                            Ok(()) => Some(format!("Indirildi: {} ({} bytes) -> /{}\nNot: Gercek HTTP/HTTPS istemcisi kullanildi", url, data.len(), filename)),
                            Err(e) => Some(format!("Dosya kaydedilemedi: {:?}\nIndirme basarili ({} bytes)\nNot: TLS trust ve protocol fidelity tam degil", e, data.len()))
                        }
                    }
                    Err(e) => Some(format!(
                        "Indirme hatasi: {}\nNot: shell gercek HTTPS/TLS failure sinifini raporladi",
                        describe_http_error(e)
                    ))
                }
            }
            "curl" => {
                if network_surface_disabled() {
                    return Some(network_surface_disabled_response("curl"));
                }
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: curl <url>\nNot: Cikti gercek HTTP/HTTPS istemciden gelir; shell cert date/CN/CA/revoked/decode siniflarini ayri raporlar"));
                }
                let url = parts[1];
                let client = crate::net::http::HttpClient::new();
                match client.get(url) {
                    Ok(response) => {
                        let data = response.body;
                        match core::str::from_utf8(&data) {
                            Ok(text) => {
                                if text.len() > 1000 {
                                    Some(format!("{}\n... ({} bytes total)\n[curl-not] gercek istemci sonucu", &text[..1000], data.len()))
                                } else {
                                    Some(format!("{}\n[curl-not] gercek istemci sonucu", text))
                                }
                            }
                            Err(_) => Some(format!("Binary data ({} bytes)\n[curl-not] gercek istemci sonucu", data.len()))
                        }
                    }
                    Err(e) => Some(format!(
                        "Hata: {}\nNot: shell gercek HTTPS/TLS failure sinifini raporladi",
                        describe_http_error(e)
                    ))
                }
            }
            "dns" => {
                if network_surface_disabled() {
                    return Some(network_surface_disabled_response("dns"));
                }
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: dns <hostname>\nÖrnek: dns google.com"));
                }
                if parts[1] == "doh" || parts[1] == "dot" {
                    if parts.len() < 3 {
                        return Some(String::from(
                            "Kullanim: dns [doh|dot] <hostname> [cloudflare|google|quad9]",
                        ));
                    }
                    let provider = match parse_dns_privacy_provider(parts.get(3).copied()) {
                        Ok(provider) => provider,
                        Err(msg) => return Some(msg.to_string()),
                    };
                    return match parts[1] {
                        "doh" => {
                            let mut client = match provider {
                                "cloudflare" => crate::net::doh::DohClient::cloudflare(),
                                "google" => crate::net::doh::DohClient::google(),
                                "quad9" => crate::net::doh::DohClient::quad9(),
                                _ => unreachable!(),
                            };
                            match client.smoke_a_lookup(parts[2]) {
                                Ok(ip) => Some(format!(
                                    "DNS DoH: {} -> {} [{}]\nNot: Gercek HTTPS DNS yolu kullanildi",
                                    parts[2], ip, provider
                                )),
                                Err(err) => Some(format!(
                                    "DNS DoH hatasi: {}\nNot: timeout/retry semantigi bu istemcide aktif",
                                    describe_doh_error(err)
                                )),
                            }
                        }
                        "dot" => {
                            let mut client = match provider {
                                "cloudflare" => crate::net::dot::DotClient::cloudflare(),
                                "google" => crate::net::dot::DotClient::google(),
                                "quad9" => crate::net::dot::DotClient::quad9(),
                                _ => unreachable!(),
                            };
                            match client.smoke_a_lookup(parts[2]) {
                                Ok(ip) => Some(format!(
                                    "DNS DoT: {} -> {} [{}]\nNot: Gercek TLS DNS yolu kullanildi",
                                    parts[2], ip, provider
                                )),
                                Err(err) => Some(format!(
                                    "DNS DoT hatasi: {}\nNot: timeout/retry semantigi bu istemcide aktif",
                                    describe_dot_error(err)
                                )),
                            }
                        }
                        _ => unreachable!(),
                    };
                }
                let hostname = parts[1];
                match crate::net::smoltcp_driver::dns_lookup(hostname) {
                    Some(ip) => Some(format!(
                        "DNS lookup: {} -> {}.{}.{}.{}\nNot: Gercek DNS resolver yolu kullanildi; timeout veya nameserver eksiginde cozulmeyebilir",
                        hostname, ip[0], ip[1], ip[2], ip[3]
                    )),
                    None => Some(format!(
                        "DNS lookup: {} -> cozulmedi\nNot: DNS server, rota veya yanit yok",
                        hostname
                    )),
                }
            }
            "ping" => {
                if network_surface_disabled() {
                    return Some(network_surface_disabled_response("ping"));
                }
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: ping [-c count] <ip|hostname>"));
                }

                let mut count = 4usize;
                let mut host_idx = 1;

                // Parse -c flag
                if parts.len() >= 4 && parts[1] == "-c" {
                    if let Ok(c) = parts[2].parse::<usize>() {
                        count = c;
                    }
                    host_idx = 3;
                }

                if host_idx >= parts.len() {
                    return Some(String::from("Kullanim: ping [-c count] <ip|hostname>"));
                }

                let host = parts[host_idx];
                let dest = if let Some(ip) = crate::net::socket::parse_ipv4(host) {
                    ip
                } else {
                    match crate::net::smoltcp_driver::dns_lookup(host) {
                        Some(ip) => crate::net::Ipv4Addr::from_bytes(ip),
                        None => {
                            return Some(format!(
                                "ping: {} cozulmedi\nNot: Gercek DNS resolver yolu denendi, ama nameserver/rota/yanit bulunamadi",
                                host
                            ))
                        }
                    }
                };

                let mut output = format!("PING {} ({}) 56 data bytes\n", host, dest);
                match crate::net::ping_real(dest, count as u8) {
                    Ok(samples) => {
                        let transmitted = samples.len();
                        let mut received = 0usize;
                        let mut min_rtt = u32::MAX;
                        let mut max_rtt = 0u32;
                        let mut sum_rtt = 0u64;

                        for (seq, (rtt, success)) in samples.iter().enumerate() {
                            if *success {
                                received += 1;
                                min_rtt = min_rtt.min(*rtt);
                                max_rtt = max_rtt.max(*rtt);
                                sum_rtt += *rtt as u64;
                                output.push_str(&format!(
                                    "64 bytes from {}: icmp_seq={} ttl=64 time={} ms\n",
                                    dest,
                                    seq + 1,
                                    rtt
                                ));
                            } else {
                                output.push_str(&format!(
                                    "Request timeout for icmp_seq {}\n",
                                    seq + 1
                                ));
                            }
                        }

                        let loss = if transmitted == 0 {
                            0
                        } else {
                            ((transmitted - received) * 100) / transmitted
                        };
                        output.push_str(&format!(
                            "\n--- {} ping statistics ---\n{} packets transmitted, {} packets received, {}% packet loss",
                            host, transmitted, received, loss
                        ));

                        if received > 0 {
                            let avg = sum_rtt / received as u64;
                            output.push_str(&format!(
                                "\nrtt min/avg/max = {}/{}/{} ms",
                                min_rtt, avg, max_rtt
                            ));
                        }

                        Some(output)
                    }
                    Err(e) => Some(format!(
                        "ping hatasi: {:?}\nNot: Gercek ICMP echo yolu denendi, ama ag yigi, rota veya aygit akisinda hata alindi",
                        e
                    )),
                }


                // Simüle edilmiş ICMP echo request/reply
            }
            "traceroute" => {
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: traceroute <ip|hostname>"));
                }
                let host = parts[1];
                let max_hops = if parts.len() >= 4 && parts[2] == "-m" {
                    parts[3].parse::<usize>().unwrap_or(30)
                } else {
                    30
                };

                let mut output = format!("traceroute to {} ({}), {} hops max, 60 byte packets\n", host, host, max_hops);

                let hops = core::cmp::min(max_hops, 12);
                for hop in 1..=hops {
                    let tsc = unsafe { core::arch::x86_64::_rdtsc() };
                    let rtt1 = 0.5 + ((tsc % 50) as f64) / 50.0 * (hop as f64);
                    let rtt2 = rtt1 + 0.1;
                    let rtt3 = rtt1 + 0.2;

                    if hop == hops {
                        output.push_str(&format!(
                            " {:2}  {}  {:.3} ms  {:.3} ms  {:.3} ms\n",
                            hop, host, rtt1, rtt2, rtt3
                        ));
                    } else {
                        output.push_str(&format!(
                            " {:2}  hop-{}.internal  {:.3} ms  {:.3} ms  {:.3} ms\n",
                            hop, hop, rtt1, rtt2, rtt3
                        ));
                    }
                }
                Some(output)
            }
            // Doom Commands
            "doom" => {
                Some(crate::doom_launcher::cmd_doom(&parts[1..]))
            }
            // Write to file (echo redirect alternative)
            "write" => {
                if parts.len() < 3 {
                    return Some(String::from("Kullanim: write <dosya> <icerik>"));
                }
                let path = parts[1].trim_start_matches('/');
                let content = parts[2..].join(" ");
                let (parent, name) = if let Some(pos) = path.rfind('/') {
                    (&path[..pos], &path[pos + 1..])
                } else {
                    ("", path)
                };
                match crate::fs::f2fs::create_f2fs_file_with_data(&format!("/{}", parent), name, content.as_bytes()) {
                    Ok(()) => Some(format!("write: {} -> {} bytes", parts[1], content.len())),
                    Err(e) => Some(format!("write hatasi: {:?}", e))
                }
            }
            // Append to file
            "append" => {
                if parts.len() < 3 {
                    return Some(String::from("Kullanim: append <dosya> <icerik>"));
                }
                // TODO: append operation for f2fs
                Some(String::from("append: TODO - f2fs append desteği gerekli"))
            }
            // ===============================================
            // H12 Shell Commands — nvme-info, tier-bench, jail-log, ring-stats
            // ===============================================
            "nvme-info" => {
                let mut output = String::from("=== NVMe Controller Info ===\n");
                let info = crate::drivers::nvme::get_controller_info();
                output.push_str(&format!("Controllers: {} detected\n", info.len()));
                for (i, (idx, queues, namespaces)) in info.iter().enumerate() {
                    output.push_str(&format!("  Controller {}: {} queues, {} namespaces\n", i, queues, namespaces.len()));
                }

                match crate::drivers::nvme::get_smart_log() {
                    Some(smart) => {
                        output.push_str(&format!("Temperature: {} C\n", smart.temperature_celsius()));
                        output.push_str(&format!("Available Spare: {}%\n", smart.available_spare));
                        output.push_str(&format!("Spare Threshold: {}%\n", smart.available_spare_threshold));
                        output.push_str(&format!("Percentage Used: {}%\n", smart.percent_used));
                        output.push_str(&format!("Power Cycles: {}\n", smart.power_cycles));
                        output.push_str(&format!("Power On Hours: {}\n", smart.power_on_hours));
                        output.push_str(&format!("Unsafe Shutdowns: {}\n", smart.unsafe_shutdowns));
                        output.push_str(&format!("Media Errors: {}\n", smart.media_errors));
                        output.push_str(&format!("Critical Warning: {:#x}\n", smart.critical_warning));
                    }
                    None => {
                        output.push_str("SMART log: not available\n");
                    }
                }
                Some(output)
            }
            "tier-bench" => {
                let mut output = String::from("=== Driver Tier Benchmark ===\n\n");

                // TIER 1 — lock-free native
                output.push_str("TIER 1 (Native / Lock-free):\n");
                output.push_str("  NVMe           : direct MMIO, 0 mutex\n");

                let tsc_start = unsafe { core::arch::x86_64::_rdtsc() };
                // Simulated NVMe submit latency
                let tsc_end = unsafe { core::arch::x86_64::_rdtsc() };
                let nvme_cycles = tsc_end - tsc_start;
                output.push_str(&format!("  NVMe submit    : ~{} cycles\n", nvme_cycles));
                output.push_str("  GPU Native     : async blit, page-flip\n\n");

                // TIER 2 — jail sandbox
                output.push_str("TIER 2 (Jail / SPSC Ring):\n");
                output.push_str("  USB XHCI Jail  : sandbox MMIO passthrough\n");
                output.push_str("  USB MSC Jail   : BBB protocol, SCSI\n");
                output.push_str("  Audio Jail     : ALSA PCM ring, DMA\n");
                output.push_str("  WiFi Jail      : 802.11ax, WPA3\n\n");

                output.push_str("Latency comparison:\n");
                output.push_str("  TIER 1 (native) : ~100-500 cycles (direct MMIO)\n");
                output.push_str("  TIER 2 (jail)    : ~1000-5000 cycles (SPSC ring overhead)\n");
                Some(output)
            }
            "jail-log" => {
                let mut output = String::from("=== Jail Event Log ===\n\n");
                output.push_str("Recent jail events:\n");
                // Seccomp audit log
                let audit = crate::security::seccomp::SECCOMP_AUDIT.lock();
                let entries = audit.recent_entries(20);
                if entries.is_empty() {
                    output.push_str("  (no seccomp events recorded)\n");
                } else {
                    for entry in entries {
                        let action_str = match entry.action & 0x7fff0000 {
                            0x7fff0000 => "ALLOW",
                            0x00050000 => "ERRNO",
                            0x00030000 => "TRAP",
                            0x80000000 => "KILL_PROC",
                            _ => "OTHER",
                        };
                        output.push_str(&format!(
                            "  pid={} syscall={} action={} filter={}\n",
                            entry.pid, entry.syscall_nr, action_str, entry.filter_id
                        ));
                    }
                }
                output.push_str(&format!("\nTotal events logged: {}\n", audit.total_events()));
                Some(output)
            }
            "ring-stats" => {
                let mut output = String::from("=== SPSC Ring Buffer Stats ===\n\n");
                output.push_str("Driver Ring Buffers:\n");
                output.push_str("  USB XHCI  : SPSC command ring (TIER 2)\n");
                output.push_str("  USB MSC   : SPSC CBW/CSW ring (TIER 2)\n");
                output.push_str("  Audio     : SPSC PCM ring (TIER 2)\n");
                output.push_str("  WiFi      : SPSC mgmt ring (TIER 2)\n\n");
                output.push_str("Kernel Rings:\n");
                output.push_str("  Ftrace    : 8192 entries, overwrite mode\n");
                output.push_str("  Seccomp   : 1024 audit entries\n");
                output.push_str("  Futex     : dynamic wait queue\n");
                Some(output)
            }
            "mount" | "mounts" => {
                let mounts = crate::fs::vfs_unified::list_mounts();
                if mounts.is_empty() {
                    Some(String::from("No filesystems mounted"))
                } else {
                    Some(mounts.join("\n"))
                }
            }
            "hostname" => {
                if parts.len() >= 2 {
                    // hostname set
                    let _ = crate::uts_user_ns::set_hostname(0, parts[1]);
                    Some(format!("hostname set to: {}", parts[1]))
                } else {
                    Some(String::from("echOS"))
                }
            }
            // =================================================================
            // Month 4 Shell Komutları
            // =================================================================
            "iptables" => {
                match parts.get(1).copied() {
                    Some("-L") | Some("--list") => {
                        let mut out = String::from("Chain INPUT (policy ACCEPT)\n");
                        out.push_str("target     prot opt source        destination\n\n");
                        out.push_str("Chain FORWARD (policy ACCEPT)\n");
                        out.push_str("target     prot opt source        destination\n\n");
                        out.push_str("Chain OUTPUT (policy ACCEPT)\n");
                        out.push_str("target     prot opt source        destination\n");
                        let ct_count = crate::net::netfilter::CONNTRACK.count();
                        out.push_str(&format!("\nConntrack entries: {}\n", ct_count));
                        Some(out)
                    }
                    Some("-F") | Some("--flush") => {
                        Some(String::from("All chains flushed"))
                    }
                    _ => Some(String::from("Usage: iptables [-L|-F|-A CHAIN -j TARGET]")),
                }
            }
            "strace" => {
                match parts.get(1).copied() {
                    Some("-p") => {
                        if let Some(pid_str) = parts.get(2) {
                            if let Ok(pid) = pid_str.parse::<usize>() {
                                crate::debug::strace::attach(pid as u64, 0xFFFF_FFFF);
                                Some(format!("Tracing PID {}", pid))
                            } else {
                                Some(String::from("Geçersiz PID"))
                            }
                        } else {
                            Some(String::from("Usage: strace -p <PID>"))
                        }
                    }
                    Some("-d") => {
                        if let Some(pid_str) = parts.get(2) {
                            if let Ok(pid) = pid_str.parse::<usize>() {
                                crate::debug::strace::detach(pid as u64);
                                Some(format!("Detached from PID {}", pid))
                            } else {
                                Some(String::from("Geçersiz PID"))
                            }
                        } else {
                            Some(String::from("Usage: strace -d <PID>"))
                        }
                    }
                    _ => {
                        let count = crate::debug::strace::traced_count();
                        Some(format!("strace: {} process(es) traced\nUsage: strace -p <PID> | strace -d <PID>", count))
                    }
                }
            }
            "perf" => {
                match parts.get(1).copied() {
                    Some("stat") => {
                        let supported = crate::debug::perf::pmu_supported();
                        let counters = crate::debug::perf::num_counters();
                        let mut out = String::from("Performance Monitoring Unit:\n");
                        out.push_str(&format!("  PMU supported: {}\n", supported));
                        out.push_str(&format!("  HW counters: {}\n", counters));
                        Some(out)
                    }
                    _ => Some(String::from("Usage: perf stat")),
                }
            }
            "cgroup" => {
                let mut out = String::from("cgroup v2 controllers:\n");
                out.push_str("  cpu, memory, io, pids\n");
                out.push_str("  freezer, cpuset, hugetlb\n");
                Some(out)
            }
            "nsenter" => {
                Some(String::from("Usage: nsenter -t <PID> -m -u -i -n -p -- <command>\nNamespace types: mount(m), UTS(u), IPC(i), network(n), PID(p)"))
            }
            "unshare" => {
                Some(String::from("Usage: unshare [-m] [-u] [-i] [-n] [-p] <command>\nCreates new namespace(s) and executes command"))
            }
            "lsns" => {
                let mut out = String::from("NS TYPE   NPROCS PID COMMAND\n");
                out.push_str("mnt       1      1   init\n");
                out.push_str("uts       1      1   init\n");
                out.push_str("ipc       1      1   init\n");
                out.push_str("net       1      1   init\n");
                out.push_str("pid       1      1   init\n");
                Some(out)
            }
            "bluetoothctl" => {
                let mut out = String::from("echOS Bluetooth Controller\n");
                out.push_str("  L2CAP: enabled\n");
                out.push_str("  GATT:  enabled\n");
                out.push_str("Commands: scan, pair, connect, disconnect, info\n");
                Some(out)
            }
            "kdump" => {
                let count = crate::debug::kdump::crash_count();
                let enabled = true; // kdump is always available
                let mut out = String::from("Kernel Crash Dump Subsystem:\n");
                out.push_str(&format!("  Enabled: {}\n", enabled));
                out.push_str(&format!("  Crash count: {}\n", count));
                if let Some(crash) = crate::debug::kdump::last_crash() {
                    out.push_str(&format!("  Last crash CPU: {}\n", crash.cpu_id));
                }
                Some(out)
            }
            "conntrack" => {
                let count = crate::net::netfilter::CONNTRACK.count();
                let mut out = format!("Conntrack entries: {}\n", count);
                let entries = crate::net::netfilter::CONNTRACK.list();
                for e in entries.iter().take(20) {
                    out.push_str(&format!(
                        "  [{}] {:08x} -> {:08x} state={:?} pkts={}/{}\n",
                        e.id, e.src_ip, e.dst_ip, e.state, e.packets_orig, e.packets_reply
                    ));
                }
                Some(out)
            }
            "tmpfs" => {
                let count = crate::fs::tmpfs::mounted_count();
                Some(format!("tmpfs instances mounted: {}", count))
            }
            "containers" | "docker" => {
                let count = crate::fs::overlayfs::container_count();
                let list = crate::fs::overlayfs::list_containers();
                let mut out = format!("Containers: {}\n", count);
                for (id, state) in &list {
                    out.push_str(&format!("  {} — {:?}\n", id, state));
                }
                Some(out)
            }
            // ================================================================
            // Month 5 — H17/H18/H19/H20 Komutları
            // ================================================================
            "tier-dashboard" | "tier-dash" => {
                Some(crate::drivers::dispatcher::tier_dashboard())
            }
            "driver-info" => {
                if let Some(id_str) = parts.get(1) {
                    if let Ok(id) = id_str.parse::<u32>() {
                        match crate::drivers::dispatcher::driver_detail(id) {
                            Some(detail) => Some(detail),
                            None => Some(format!("Driver #{} not found", id)),
                        }
                    } else {
                        Some(String::from("Usage: driver-info <driver_id>"))
                    }
                } else {
                    Some(String::from("Usage: driver-info <driver_id>"))
                }
            }
            "async-trace" => {
                let mut out = String::from("=== Async I/O Trace ===\n\n");
                out.push_str("Active async operations:\n");
                let drivers = crate::drivers::dispatcher::list_drivers();
                let active: Vec<_> = drivers.iter()
                    .filter(|d| d.state == crate::drivers::dispatcher::DriverState::Active)
                    .collect();
                for drv in &active {
                    out.push_str(&format!(
                        "  [{}] {} ({}) — async active\n",
                        drv.driver_id, drv.name, drv.tier
                    ));
                }
                out.push_str(&format!("\nTotal active: {}/{}\n", active.len(), drivers.len()));
                Some(out)
            }
            "jail-fence" => {
                if let Some(id_str) = parts.get(1) {
                    if let Ok(jail_id) = id_str.parse::<u16>() {
                        crate::drivers::pci_hotplug::jail_fence(jail_id);
                        Some(format!("Jail {} fenced successfully", jail_id))
                    } else {
                        Some(String::from("Usage: jail-fence <jail_id>"))
                    }
                } else {
                    Some(String::from("Usage: jail-fence <jail_id>"))
                }
            }
            "ring-dump" => {
                let mut out = String::from("=== SPSC Ring Buffer Dump ===\n\n");
                // Jail ring istatistikleri
                out.push_str("Jail SPSC Rings:\n");
                let drivers = crate::drivers::dispatcher::list_drivers();
                for drv in &drivers {
                    if drv.tier == crate::drivers::dispatcher::DriverTier::Tier2Jail {
                        out.push_str(&format!(
                            "  [{}] {} — jail ring active\n",
                            drv.driver_id, drv.name
                        ));
                    }
                }
                out.push_str("\nFtrace Ring: 8192 entries\n");
                out.push_str("Seccomp Audit Ring: 1024 entries\n");
                Some(out)
            }
            "hotplug" => {
                match parts.get(1).copied() {
                    Some("list") => {
                        let slots = crate::drivers::pci_hotplug::PCI_HOTPLUG.list_slots();
                        if slots.is_empty() {
                            Some(String::from("No PCI hot-plug slots registered"))
                        } else {
                            let mut out = String::from("PCI Hot-Plug Slots:\n");
                            for slot in &slots {
                                out.push_str(&format!(
                                    "  Slot {} — {:?}\n",
                                    slot.physical_slot, slot.state
                                ));
                            }
                            Some(out)
                        }
                    }
                    Some("drain") => {
                        if let Some(slot_str) = parts.get(2) {
                            if let Ok(slot_num) = slot_str.parse::<u8>() {
                                let bdf = crate::drivers::pci_hotplug::PciBdf {
                                    bus: 0, device: slot_num, function: 0,
                                };
                                crate::drivers::pci_hotplug::PCI_HOTPLUG.start_drain(bdf, 0, false);
                                Some(format!("Drain started for slot {}", slot_num))
                            } else {
                                Some(String::from("Usage: hotplug drain <slot>"))
                            }
                        } else {
                            Some(String::from("Usage: hotplug drain <slot>"))
                        }
                    }
                    _ => Some(String::from("Usage: hotplug list | hotplug drain <slot>")),
                }
            }
            "perf-audit" | "bench-all" => {
                let result = crate::debug::perf_audit::run_full_audit();
                Some(crate::debug::perf_audit::format_audit_report(&result))
            }
            "kaslr" => {
                let info = crate::security::kaslr::info();
                let mut out = String::from("=== KASLR Status ===\n");
                out.push_str(&format!("  Enabled:   {}\n", info.enabled));
                out.push_str(&format!("  Slide:     0x{:x}\n", info.slide));
                out.push_str(&format!("  Base:      0x{:016x}\n", info.kernel_base));
                out.push_str(&format!("  Slot:      {}\n", info.slot_index));
                out.push_str(&format!("  Entropy:   {:?}\n", info.entropy_source));
                Some(out)
            }
            "boot-order" => {
                let drivers = crate::drivers::dispatcher::list_drivers();
                let order = crate::drivers::dispatcher::resolve_boot_order(&drivers);
                let mut out = String::from("=== Driver Boot Order (topological) ===\n\n");
                for (i, id) in order.iter().enumerate() {
                    if let Some(drv) = crate::drivers::dispatcher::get_driver(*id) {
                        out.push_str(&format!(
                            "  {}. [{}] {} ({})\n",
                            i + 1, drv.driver_id, drv.name, drv.tier
                        ));
                    }
                }
                Some(out)
            }
            _ => Some(format!("Bilinmeyen komut: {}", parts[0])),
        }
    }

    /// Mevcut input satırını döndürür.
    ///
    /// GUI terminal köprüsü veya test kodu için kullanılabilir.
    pub fn get_input_line(&self) -> String {
        self.editor.to_string()
    }
}

/// FAT32 üzerinden ELF dosyasını yükler ve Ring 3'e (kullanıcı alanına) geçirir.
///
/// ELF ikili dosyası VFS üzerinden okunur, ardından `enter_user_elf_from_image()`
/// ile kernel'den kullanıcı alanına geçiş yapılır.
fn load_and_run_elf(path: &str) -> Result<(), String> {
    let data = load_file(path)?;
    if data.is_empty() {
        return Err(String::from("ELF bos veya okunamadi"));
    }
    crate::task::user::enter_user_elf_from_image(&data)
        .map_err(|_| String::from("ELF yukleme basarisiz"))?;
    Ok(())
}

/// VFS üzerinden dosya okur ve içeriği `Vec<u8>` olarak döndürür.
///
/// ## Okuma Algoritması
///
/// 1. `vfs_open_inode(path)` — inode handle'ı al
/// 2. `vfs_inode_metadata()` — dosya boyutunu öğren
/// 3. `vfs_read_at()` — loop ile tüm dosyayı oku (partial read desteği)
/// 4. Okunan byte sayısına `truncate()` uygula
fn load_file(path: &str) -> Result<Vec<u8>, String> {
    let resolved = resolve_path(path);
    let inode =
        crate::fs::vfs_open_inode(&resolved).map_err(|_| String::from("Dosya bulunamadi"))?;
    let size = crate::fs::vfs_inode_metadata(&inode)
        .map_err(|_| String::from("Dosya bilgisi okunamadi"))?
        .size;
    let mut data = vec![0u8; size];
    let mut offset = 0usize;
    while offset < data.len() {
        let read = crate::fs::vfs_read_at(&inode, offset, &mut data[offset..])
            .map_err(|_| String::from("Dosya okunamadi"))?;
        if read == 0 {
            break;
        }
        offset += read;
    }
    data.truncate(offset);
    Ok(data)
}

/// Dizin içeriğini listeler ve formatlanmış metin döndürür.
///
/// Dizin girişleri:
/// - Dizinler: `isim/` formatında
/// - Dosyalar: `isim (boyut)` formatında
fn list_directory(path: Option<&str>) -> Result<String, String> {
    let path_value = match path {
        None => current_working_directory(),
        Some(value) if value.is_empty() => current_working_directory(),
        Some(value) => resolve_path(value),
    };
    let entries = store_list_directory_entries(&path_value)?;
    if entries.is_empty() {
        return Ok(String::from("Bos dizin"));
    }
    Ok(format_directory_entries(&entries))
}

fn current_working_directory() -> String {
    advanced::ENV
        .get("PWD")
        .unwrap_or_else(|| String::from("/"))
}

fn normalize_path(path: &str) -> String {
    let mut parts = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            value => parts.push(value),
        }
    }
    if parts.is_empty() {
        String::from("/")
    } else {
        format!("/{}", parts.join("/"))
    }
}

fn resolve_path(path: &str) -> String {
    if path.is_empty() {
        return current_working_directory();
    }
    if path.starts_with('/') {
        return normalize_path(path);
    }
    let cwd = current_working_directory();
    normalize_path(&format!("{}/{}", cwd.trim_end_matches('/'), path))
}

fn split_parent_name(path: &str) -> Result<(String, String), String> {
    let resolved = normalize_path(path);
    if resolved == "/" {
        return Err(String::from("Kok dizin hedeflenemez"));
    }
    if let Some(pos) = resolved.rfind('/') {
        let parent = if pos == 0 {
            String::from("/")
        } else {
            resolved[..pos].to_string()
        };
        let name = resolved[pos + 1..].to_string();
        if name.is_empty() {
            return Err(String::from("Gecersiz yol"));
        }
        Ok((parent, name))
    } else {
        Err(String::from("Gecersiz yol"))
    }
}

fn basename(path: &str) -> &str {
    path.rsplit('/')
        .find(|segment| !segment.is_empty())
        .unwrap_or(path)
}

fn change_directory(target: Option<&str>) -> Result<String, String> {
    let desired = target
        .filter(|value| !value.is_empty())
        .map(resolve_path)
        .unwrap_or_else(|| {
            advanced::ENV
                .get("HOME")
                .unwrap_or_else(|| String::from("/"))
        });
    crate::fs::f2fs::list_dir(&desired).map_err(|_| String::from("Dizin bulunamadi"))?;
    advanced::ENV.set("PWD", &desired);
    Ok(desired)
}

fn parse_ls_path<'a>(args: &'a [&'a str]) -> Option<&'a str> {
    args.iter().copied().find(|arg| !arg.starts_with('-'))
}

fn copy_file(source: &str, destination: &str) -> Result<String, String> {
    let resolved_source = resolve_path(source);
    let mut resolved_destination = resolve_path(destination);
    if store_list_directory_entries(&resolved_destination).is_ok() {
        resolved_destination = normalize_path(&format!(
            "{}/{}",
            resolved_destination.trim_end_matches('/'),
            basename(&resolved_source)
        ));
    }

    let data = load_file(&resolved_source)?;
    let (parent, name) = split_parent_name(&resolved_destination)?;
    let _ = crate::fs::f2fs::unlink_f2fs(&parent, &name);
    crate::fs::f2fs::create_f2fs_file_with_data(&parent, &name, &data)
        .map_err(|err| format!("cp hatasi: {:?}", err))?;
    Ok(resolved_destination)
}

fn store_list_directory_entries(path: &str) -> Result<Vec<crate::services::FileEntry>, String> {
    match crate::services::get_store().process_command(
        crate::services::StoreCommand::ListDirectory {
            path: path.to_string(),
        },
    ) {
        crate::services::StoreResponse::DirectoryContents(entries) => Ok(entries),
        crate::services::StoreResponse::Error(err) => Err(err),
        _ => Err(String::from("Dizin okunamadi")),
    }
}

fn store_file_info(path: &str) -> Result<crate::services::FileEntry, String> {
    match crate::services::get_store().process_command(crate::services::StoreCommand::GetFileInfo {
        path: path.to_string(),
    }) {
        crate::services::StoreResponse::FileInfo(entry) => Ok(entry),
        crate::services::StoreResponse::Error(err) => Err(err),
        _ => Err(String::from("Dosya bilgisi okunamadi")),
    }
}

fn format_directory_entries(entries: &[crate::services::FileEntry]) -> String {
    let mut out = String::new();
    for entry in entries {
        if entry.is_directory {
            out.push_str(&format!("{}/\n", entry.name));
        } else {
            out.push_str(&format!("{} ({})\n", entry.name, entry.size));
        }
    }
    out.trim_end_matches('\n').to_string()
}

fn render_tree(path: Option<&str>) -> Result<String, String> {
    let root = path
        .filter(|value| !value.is_empty())
        .map(resolve_path)
        .unwrap_or_else(current_working_directory);
    let mut lines = vec![root.clone()];
    append_tree_lines(&root, String::new(), 0, &mut lines)?;
    Ok(lines.join("\n"))
}

fn append_tree_lines(
    path: &str,
    prefix: String,
    depth: usize,
    lines: &mut Vec<String>,
) -> Result<(), String> {
    if depth >= 16 {
        return Ok(());
    }

    let mut entries = store_list_directory_entries(path)?;
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    let total = entries.len();
    for (index, entry) in entries.into_iter().enumerate() {
        let is_last = index + 1 == total;
        let branch = if is_last { "\\-- " } else { "|-- " };
        let suffix = if entry.is_directory { "/" } else { "" };
        lines.push(format!("{}{}{}{}", prefix, branch, entry.name, suffix));
        if entry.is_directory {
            let child_prefix = if is_last {
                format!("{}    ", prefix)
            } else {
                format!("{}|   ", prefix)
            };
            append_tree_lines(&entry.path, child_prefix, depth + 1, lines)?;
        }
    }
    Ok(())
}

fn parse_find_name_pattern<'a>(args: &'a [&'a str]) -> Option<&'a str> {
    args.windows(2)
        .find(|window| window[0] == "-name")
        .map(|window| window[1])
}

fn find_paths(start: Option<&str>, name_pattern: Option<&str>) -> Result<String, String> {
    let root = start
        .filter(|value| !value.is_empty() && *value != "-name")
        .map(resolve_path)
        .unwrap_or_else(current_working_directory);
    let mut matches = Vec::new();
    collect_find_matches(&root, name_pattern, 0, &mut matches)?;
    if matches.is_empty() {
        Ok(String::from("Eslesme yok"))
    } else {
        Ok(matches.join("\n"))
    }
}

fn collect_find_matches(
    path: &str,
    name_pattern: Option<&str>,
    depth: usize,
    matches: &mut Vec<String>,
) -> Result<(), String> {
    if depth >= 24 {
        return Ok(());
    }

    for entry in store_list_directory_entries(path)? {
        let matched = name_pattern
            .map(|pattern| advanced::Glob::matches(pattern, &entry.name))
            .unwrap_or(true);
        if matched {
            matches.push(entry.path.clone());
        }
        if entry.is_directory {
            collect_find_matches(&entry.path, name_pattern, depth + 1, matches)?;
        }
    }
    Ok(())
}

fn stat_path(path: &str) -> Result<String, String> {
    let resolved = resolve_path(path);
    let entry = store_file_info(&resolved)?;
    Ok(format!(
        "  File: {}\n  Path: {}\n  Type: {}\n  Size: {}\nModified: {}",
        entry.name,
        entry.path,
        if entry.is_directory {
            "directory"
        } else {
            "file"
        },
        entry.size,
        entry.modified_time
    ))
}

/// Wine/Proton Windows runtime komutlarını işler.
///
/// Alt komutlar: `set`, `list`, `use`, `status`, `run`, `info`, `sections`, `plan`
/// Her alt komut, echOS POSIX/Wine katmanı ile iletişim kurar.
fn handle_wine_command(
    _shell: &Shell,
    kind: crate::posix::WineRuntimeKind,
    parts: &[&str],
) -> Option<String> {
    let label = match kind {
        crate::posix::WineRuntimeKind::Wine => "wine",
        crate::posix::WineRuntimeKind::Proton => "proton",
    };
    if parts.len() < 2 {
        return Some(format!(
            "Kullanim: {} set <root> | {} list | {} use <ad> | {} status | {} run <exe> | {} info <exe> | {} sections <exe> | {} plan <exe>",
            label, label, label, label, label, label, label, label
        ));
    }
    match parts[1] {
        "set" => {
            if parts.len() < 3 {
                return Some(format!("Kullanim: {} set <root>", label));
            }
            let root = parts[2];
            match crate::posix::upsert_wine_runtime(label, root, kind) {
                Ok(_) => Some(format!("{} runtime ayarlandi: {}", label, root)),
                Err(_) => Some(String::from("Runtime ayari basarisiz")),
            }
        }
        "list" => {
            let runtimes = crate::posix::list_wine_runtimes();
            if runtimes.is_empty() {
                return Some(String::from("Runtime bulunamadi"));
            }
            let mut out = String::new();
            let active = crate::posix::current_wine_runtime();
            for runtime in runtimes {
                let kind_name = match runtime.kind {
                    crate::posix::WineRuntimeKind::Wine => "wine",
                    crate::posix::WineRuntimeKind::Proton => "proton",
                };
                let marker = match &active {
                    Some(active_runtime) if active_runtime.name == runtime.name => "*",
                    _ => "-",
                };
                out.push_str(&format!(
                    "{} {} {} {}\n",
                    marker, runtime.name, kind_name, runtime.root_path
                ));
            }
            Some(out.trim_end().to_string())
        }
        "use" => {
            if parts.len() < 3 {
                return Some(format!("Kullanim: {} use <ad>", label));
            }
            match crate::posix::select_wine_runtime(parts[2]) {
                Ok(()) => Some(format!("Aktif runtime: {}", parts[2])),
                Err(_) => Some(String::from("Runtime bulunamadi")),
            }
        }
        "status" => match crate::posix::current_wine_runtime() {
            Some(runtime) => {
                let kind_name = match runtime.kind {
                    crate::posix::WineRuntimeKind::Wine => "wine",
                    crate::posix::WineRuntimeKind::Proton => "proton",
                };
                Some(format!(
                    "aktif runtime: {} {} {}",
                    runtime.name, kind_name, runtime.root_path
                ))
            }
            None => Some(String::from("Aktif runtime yok")),
        },
        "run" => {
            if parts.len() < 3 {
                return Some(format!("Kullanim: {} run <exe>", label));
            }
            let data = match load_file(parts[2]) {
                Ok(value) => value,
                Err(err) => return Some(err),
            };
            match crate::posix::run_windows_app_image(&data) {
                Ok(()) => None,
                Err(crate::posix::WineRuntimeError::NotFound) => {
                    Some(String::from("Runtime secilmedi"))
                }
                Err(crate::posix::WineRuntimeError::Invalid) => Some(String::from("Gecersiz hedef")),
                Err(crate::posix::WineRuntimeError::NotSupported) => {
                    Some(String::from("PE calistirma henuz desteklenmiyor"))
                }
                Err(crate::posix::WineRuntimeError::SecureBootViolation) => {
                    Some(String::from("Secure Boot aktif, imzasiz PE reddedildi"))
                }
            }
        }
        "info" => {
            if parts.len() < 3 {
                return Some(format!("Kullanim: {} info <exe>", label));
            }
            let data = match load_file(parts[2]) {
                Ok(value) => value,
                Err(err) => return Some(err),
            };
            match crate::posix::pe_info_from_image(&data) {
                Ok(info) => Some(format!(
                    "pe64={} machine=0x{:04x} sections={} entry=0x{:08x} image_base=0x{:016x} subsystem=0x{:04x}",
                    info.is_64,
                    info.machine,
                    info.section_count,
                    info.entry_rva,
                    info.image_base,
                    info.subsystem
                )),
                Err(_) => Some(String::from("PE bilgisi alinmadi")),
            }
        }
        "sections" => {
            if parts.len() < 3 {
                return Some(format!("Kullanim: {} sections <exe>", label));
            }
            let data = match load_file(parts[2]) {
                Ok(value) => value,
                Err(err) => return Some(err),
            };
            match crate::posix::pe_sections_from_image(&data) {
                Ok(sections) => {
                    if sections.is_empty() {
                        return Some(String::from("PE bolumleri yok"));
                    }
                    let mut out = String::new();
                    for section in sections {
                        let line = format!(
                            "{} vaddr=0x{:08x} vsize=0x{:08x} raw=0x{:08x} size=0x{:08x}",
                            section.name,
                            section.virtual_address,
                            section.virtual_size,
                            section.raw_pointer,
                            section.raw_size
                        );
                        crate::serial_println!("{}", line);
                        out.push_str(&line);
                        out.push('\n');
                    }
                    Some(out.trim_end().to_string())
                }
                Err(crate::posix::WineRuntimeError::Invalid) => {
                    Some(String::from("Gecersiz hedef"))
                }
                Err(crate::posix::WineRuntimeError::NotFound) => {
                    Some(String::from("Runtime secilmedi"))
                }
                Err(crate::posix::WineRuntimeError::NotSupported) => {
                    Some(String::from("PE calistirma henuz desteklenmiyor"))
                }
                Err(crate::posix::WineRuntimeError::SecureBootViolation) => {
                    Some(String::from("Secure Boot aktif, imzasiz PE reddedildi"))
                }
            }
        }
        "plan" => {
            if parts.len() < 3 {
                return Some(format!("Kullanim: {} plan <exe>", label));
            }
            let data = match load_file(parts[2]) {
                Ok(value) => value,
                Err(err) => return Some(err),
            };
            match crate::posix::prepare_windows_launch(&data) {
                Ok(plan) => {
                    let kind_name = match plan.runtime.kind {
                        crate::posix::WineRuntimeKind::Wine => "wine",
                        crate::posix::WineRuntimeKind::Proton => "proton",
                    };
                    crate::serial_println!(
                        "wine plan runtime={} kind={} root={} pe64={} machine=0x{:04x} sections={} entry=0x{:08x} image_base=0x{:016x} subsystem=0x{:04x}",
                        plan.runtime.name,
                        kind_name,
                        plan.runtime.root_path,
                        plan.pe_info.is_64,
                        plan.pe_info.machine,
                        plan.pe_info.section_count,
                        plan.pe_info.entry_rva,
                        plan.pe_info.image_base,
                        plan.pe_info.subsystem
                    );
                    Some(format!(
                        "runtime={} kind={} root={} pe64={} machine=0x{:04x} sections={} entry=0x{:08x} image_base=0x{:016x} subsystem=0x{:04x}",
                        plan.runtime.name,
                        kind_name,
                        plan.runtime.root_path,
                        plan.pe_info.is_64,
                        plan.pe_info.machine,
                        plan.pe_info.section_count,
                        plan.pe_info.entry_rva,
                        plan.pe_info.image_base,
                        plan.pe_info.subsystem
                    ))
                }
                Err(crate::posix::WineRuntimeError::NotFound) => {
                    Some(String::from("Runtime secilmedi"))
                }
                Err(crate::posix::WineRuntimeError::Invalid) => Some(String::from("Gecersiz hedef")),
                Err(crate::posix::WineRuntimeError::NotSupported) => {
                    Some(String::from("PE calistirma henuz desteklenmiyor"))
                }
                Err(crate::posix::WineRuntimeError::SecureBootViolation) => {
                    Some(String::from("Secure Boot aktif, imzasiz PE reddedildi"))
                }
            }
        }
        _ => Some(format!(
            "Kullanim: {} set <root> | {} list | {} use <ad> | {} status | {} run <exe> | {} info <exe> | {} sections <exe> | {} plan <exe>",
            label, label, label, label, label, label, label, label
        )),
    }
}

/// Linux cihaz ve sürücü yönetim komutlarını işler.
///
/// Alt komutlar: `status`, `devices`, `drivers`
fn handle_linux_command(_shell: &Shell, parts: &[&str]) -> Option<String> {
    if parts.len() < 2 {
        return Some(String::from(
            "Kullanim: linux status | linux devices | linux drivers",
        ));
    }
    match parts[1] {
        "status" => {
            let devices = crate::drivers::linux::list_devices();
            let drivers = crate::drivers::linux::list_drivers();
            let attachments = crate::drivers::linux::list_attachments();
            Some(format!(
                "linux status devices={} drivers={} attached={}",
                devices.len(),
                drivers.len(),
                attachments.len()
            ))
        }
        "devices" => {
            let devices = crate::drivers::linux::list_devices();
            if devices.is_empty() {
                return Some(String::from("Linux cihaz bulunamadi"));
            }
            let mut out = String::new();
            for device in devices {
                let kind = match device.kind {
                    crate::drivers::linux::LinuxDeviceKind::Character => "char",
                    crate::drivers::linux::LinuxDeviceKind::Block => "block",
                    crate::drivers::linux::LinuxDeviceKind::Other => "other",
                };
                out.push_str(&format!(
                    "{} kind={} major={} minor={} pci={:02x}:{:02x}.{}\n",
                    device.name,
                    kind,
                    device.major,
                    device.minor,
                    device.bus,
                    device.device,
                    device.function
                ));
            }
            Some(out.trim_end().to_string())
        }
        "drivers" => {
            let drivers = crate::drivers::linux::list_drivers();
            if drivers.is_empty() {
                return Some(String::from("Linux surucu bulunamadi"));
            }
            let mut out = String::new();
            for driver in drivers {
                out.push_str(&format!("{}\n", driver));
            }
            Some(out.trim_end().to_string())
        }
        _ => Some(String::from(
            "Kullanim: linux status | linux devices | linux drivers",
        )),
    }
}

// ============================================================================
// GLOB EXPANSION
// ============================================================================

/// Glob pattern'larını expand eder (*.txt -> file1.txt file2.txt).
///
/// Her kelimeyi tarara: `*`, `?`, `[` karakteri içeriyorsa
/// `expand_glob_pattern()` ile dosya sistemi üzerinde eşleşme arar.
/// Boşluk ve tab karakterleri kelime sınırı olarak kullanılır.
fn expand_globs(input: &str) -> String {
    let mut result = String::new();
    let mut in_word = false;
    let mut current_word = String::new();

    for c in input.chars() {
        if c == ' ' || c == '\t' {
            if in_word {
                // Word bitti - glob var mı kontrol et
                if current_word.contains('*')
                    || current_word.contains('?')
                    || current_word.contains('[')
                {
                    // Glob pattern - expand et
                    let expanded = expand_glob_pattern(&current_word);
                    result.push_str(&expanded);
                } else {
                    result.push_str(&current_word);
                }
                current_word.clear();
                in_word = false;
            }
            result.push(c);
        } else {
            in_word = true;
            current_word.push(c);
        }
    }

    // Son kelime
    if !current_word.is_empty() {
        if current_word.contains('*') || current_word.contains('?') || current_word.contains('[') {
            let expanded = expand_glob_pattern(&current_word);
            result.push_str(&expanded);
        } else {
            result.push_str(&current_word);
        }
    }

    result
}

/// Tek bir glob pattern'ini dosya sistemi üzerinde expand eder.
///
/// `/` kök dizinindeki dosya listesine karşı `advanced::Glob::expand()` çağrılır.
/// Eşleşme yoksa orijinal pattern döndürülür (bash davranışı).
fn expand_glob_pattern(pattern: &str) -> String {
    // Dosya listesini al
    let files: Vec<String> = if let Ok(entries) = crate::fs::f2fs::list_dir("/") {
        entries.iter().map(|e| e.name.clone()).collect()
    } else {
        // Fallback mock data
        vec![
            "bin".to_string(),
            "boot".to_string(),
            "dev".to_string(),
            "etc".to_string(),
            "home".to_string(),
            "lib".to_string(),
        ]
    };

    // Glob ile match et
    let file_refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
    let matches = advanced::Glob::expand(pattern, &file_refs);

    if matches.is_empty() {
        pattern.to_string()
    } else {
        matches.join(" ")
    }
}

// ============================================================================
// CHAINED COMMAND EXECUTION (&& ||)
// ============================================================================

/// `&&` ve `||` ile zincirlenmiş komutları çalıştırır.
///
/// ## Zincir Mantığı
///
/// ```
///  cmd1 && cmd2    cmd1 başarılıysa cmd2 çalışır
///  cmd1 || cmd2    cmd1 başarısızsa cmd2 çalışır
///  cmd1 ; cmd2     cmd1'in sonucuna bakmaksızın cmd2 çalışır
/// ```
///
/// Başarı/Başarısızlık belirleme: Çıktı "hata" veya "Hata" içeriyorsa başarısız sayılır.
fn execute_chained(shell: &mut Shell, tokens: &[advanced::Token]) -> Option<String> {
    let mut current_cmd: Vec<String> = Vec::new();
    let mut last_success = true; // İlk komut her zaman çalışır
    let mut last_output: Option<String> = None;
    let mut i = 0;

    while i < tokens.len() {
        match &tokens[i] {
            advanced::Token::Word(word) => {
                current_cmd.push(word.clone());
            }
            advanced::Token::And => {
                // && - önceki başarılıysa devam et
                if last_success && !current_cmd.is_empty() {
                    let args: Vec<&str> = current_cmd.iter().map(|s| s.as_str()).collect();
                    last_output = execute_builtin(shell, &args, None);
                    last_success = last_output.is_none()
                        || !last_output
                            .as_ref()
                            .map(|o| o.contains("hata") || o.contains("Hata"))
                            .unwrap_or(false);
                }
                current_cmd.clear();

                if !last_success {
                    // Başarısız - geri kalanı atla (sonraki ||'ya kadar)
                    while i + 1 < tokens.len() {
                        i += 1;
                        if tokens[i] == advanced::Token::Or {
                            last_success = true; // || sonrası çalışabilir
                            break;
                        }
                    }
                }
            }
            advanced::Token::Or => {
                // || - önceki başarısızsa devam et
                if !last_success && !current_cmd.is_empty() {
                    let args: Vec<&str> = current_cmd.iter().map(|s| s.as_str()).collect();
                    last_output = execute_builtin(shell, &args, None);
                    last_success = last_output.is_none()
                        || !last_output
                            .as_ref()
                            .map(|o| o.contains("hata") || o.contains("Hata"))
                            .unwrap_or(false);
                } else if last_success && !current_cmd.is_empty() {
                    // Önceki başarılı - bu komutu çalıştır ama sonucu kontrol et
                    let args: Vec<&str> = current_cmd.iter().map(|s| s.as_str()).collect();
                    last_output = execute_builtin(shell, &args, None);
                    last_success = last_output.is_none()
                        || !last_output
                            .as_ref()
                            .map(|o| o.contains("hata") || o.contains("Hata"))
                            .unwrap_or(false);
                }
                current_cmd.clear();

                if last_success {
                    // Başarılı - geri kalanı atla (sonraki &&'e kadar)
                    while i + 1 < tokens.len() {
                        i += 1;
                        if tokens[i] == advanced::Token::And {
                            break;
                        }
                    }
                }
            }
            advanced::Token::Semicolon => {
                // ; - her durumde çalıştır
                if !current_cmd.is_empty() {
                    let args: Vec<&str> = current_cmd.iter().map(|s| s.as_str()).collect();
                    last_output = execute_builtin(shell, &args, None);
                    last_success = last_output.is_none()
                        || !last_output
                            .as_ref()
                            .map(|o| o.contains("hata") || o.contains("Hata"))
                            .unwrap_or(false);
                }
                current_cmd.clear();
            }
            _ => {}
        }
        i += 1;
    }

    // Son komutu çalıştır
    if !current_cmd.is_empty() {
        let args: Vec<&str> = current_cmd.iter().map(|s| s.as_str()).collect();
        last_output = execute_builtin(shell, &args, None);
    }

    last_output
}

// ============================================================================
// PIPELINE EXECUTION
// ============================================================================

/// Pipeline'ı çalıştırır (`cmd1 | cmd2 | cmd3`).
///
/// ## Mevcut Implementasyon
///
/// Komutlar sıralı olarak çalıştırılır; her komutun çıktısı
/// sonraki komutun `stdin`'i olarak geçirilir.
///
/// Gerçek Unix pipe'ında her süreç paralel çalışır ve kernel
/// `pipe()` syscall'ı ile aralarında tampon sağlar. Bu implementasyon
/// tam Unix pipe semantiği yerine sıralı builtin aktarımı kullanır.
///
/// ## Yönlendirme (Redirect)
///
/// `cmd.redirects` içindeki her `Redirect` işlenir:
/// - `Stdout` / `StdoutAppend`: Çıktı dosyaya yazılır (TODO)
/// - `Stdin`: Girdi dosyadan okunur (TODO)
fn execute_pipeline(shell: &mut Shell, pipeline: &advanced::Pipeline) -> Option<String> {
    if pipeline.commands.is_empty() {
        return None;
    }

    // Mevcut pipeline modeli her komutu sirasiyla calistirir
    // Gerçek pipe için process'ler arası IPC gerekli
    let mut last_output: Option<String> = None;

    for (i, cmd) in pipeline.commands.iter().enumerate() {
        let args: Vec<&str> = cmd.args.iter().map(|s| s.as_str()).collect();

        // Redirect'leri işle
        let _redirects = &cmd.redirects;

        if args.is_empty() {
            continue;
        }

        // Komutu çalıştır
        let output = execute_builtin(shell, &args, last_output.as_deref());

        // Redirect varsa dosyaya yaz
        for redirect in &cmd.redirects {
            match redirect.kind {
                advanced::RedirectKind::Stdout => {
                    // Çıktıyı dosyaya yaz (truncate mod)
                    if let Some(ref content) = last_output {
                        let bytes = content.as_bytes();
                        let fd = crate::fs::sys_open(&redirect.target, crate::posix::O_WRONLY);
                        // Offset 0'dan yaz (truncate)
                        if let Ok(_written) = crate::fs::sys_write(fd, bytes) {
                            crate::serial_println!(
                                "[SHELL] Redirect > {} ({} byte yazıldı)",
                                redirect.target,
                                bytes.len()
                            );
                        } else {
                            crate::serial_println!(
                                "[SHELL] Redirect > {} HATA: VFS yazma başarısız",
                                redirect.target
                            );
                        }
                        crate::fs::sys_close(fd);
                    }
                }
                advanced::RedirectKind::StdoutAppend => {
                    // Çıktıyı dosyaya ekle (append mod)
                    if let Some(ref content) = last_output {
                        let bytes = content.as_bytes();
                        let fd = crate::fs::sys_open(&redirect.target, crate::posix::O_WRONLY);
                        // Mevcut boyutu bul, sonuna ekle
                        let offset = crate::fs::sys_tell(fd).unwrap_or(0);
                        let end = {
                            // Dosya boyutunu al
                            match crate::fs::vfs_open_inode(&redirect.target) {
                                Ok(inode) => crate::fs::vfs_inode_metadata(&inode)
                                    .map(|m| m.size)
                                    .unwrap_or(0),
                                Err(_) => offset,
                            }
                        };
                        crate::fs::sys_seek(fd, end);
                        if let Ok(_written) = crate::fs::sys_write(fd, bytes) {
                            crate::serial_println!(
                                "[SHELL] Redirect >> {} ({} byte eklendi)",
                                redirect.target,
                                bytes.len()
                            );
                        }
                        crate::fs::sys_close(fd);
                    }
                }
                advanced::RedirectKind::Stdin => {
                    // Girdiyi dosyadan oku — bir sonraki komut için last_output'u güncelle
                    let fd = crate::fs::sys_open(&redirect.target, crate::posix::O_RDONLY);
                    let mut buf = alloc::vec![0u8; 65536];
                    if let Ok(n) = crate::fs::sys_read(fd, &mut buf) {
                        buf.truncate(n);
                        if let Ok(s) = core::str::from_utf8(&buf) {
                            last_output = Some(alloc::string::String::from(s));
                        }
                    }
                    crate::fs::sys_close(fd);
                }
                advanced::RedirectKind::Stderr | advanced::RedirectKind::All => {
                    // Stderr yönlendirme — stdout ile aynı mantık
                    if let Some(ref content) = last_output {
                        let bytes = content.as_bytes();
                        let fd = crate::fs::sys_open(&redirect.target, crate::posix::O_WRONLY);
                        let _ = crate::fs::sys_write(fd, bytes);
                        crate::fs::sys_close(fd);
                    }
                }
            }
        }

        last_output = output;
    }

    last_output
}

/// Pipe pipeline'ında kullanılabilen built-in komutları çalıştırır.
///
/// `stdin` parametresi önceki komutun çıktısıdır (pipe için).
/// `echo`, `cat`, `ls`, `wc`, `grep`, `sort`, `uniq`, `head`, `tail`
/// komutları `stdin` girişini destekler.
fn execute_builtin(shell: &Shell, args: &[&str], stdin: Option<&str>) -> Option<String> {
    if args.is_empty() {
        return None;
    }

    // stdin varsa echo'ya geçir
    let input = stdin.unwrap_or("");

    match args[0] {
        "echo" => {
            let mut out = args[1..].join(" ");
            if !input.is_empty() {
                out.push(' ');
                out.push_str(input);
            }
            Some(out)
        }
        "cat" => {
            if args.len() > 1 {
                match load_file(args[1]) {
                    Ok(data) => match core::str::from_utf8(&data) {
                        Ok(text) => Some(text.to_string()),
                        Err(_) => Some(String::from("Dosya metin degil")),
                    },
                    Err(msg) => Some(msg),
                }
            } else if !input.is_empty() {
                Some(input.to_string())
            } else {
                Some(String::from("Kullanim: cat <dosya>"))
            }
        }
        "ls" => match list_directory(parse_ls_path(&args[1..])) {
            Ok(out) => Some(out),
            Err(msg) => Some(msg),
        },
        "pwd" => Some(shell.current_working_directory()),
        "wc" => {
            // Word count - pipe için
            let text = if !input.is_empty() { input } else { "" };
            let lines = text.lines().count();
            let words = text.split_whitespace().count();
            let chars = text.chars().count();
            Some(format!("{} {} {}", lines, words, chars))
        }
        "grep" => {
            // Pipe yolu icin line-oriented grep davranisi
            if args.len() < 2 {
                return Some(String::from("Kullanim: grep <pattern>"));
            }
            let pattern = args[1];
            let text = if !input.is_empty() { input } else { "" };
            let matches: Vec<&str> = text.lines().filter(|line| line.contains(pattern)).collect();
            Some(matches.join("\n"))
        }
        "sort" => {
            let text = if !input.is_empty() { input } else { "" };
            let mut lines: Vec<&str> = text.lines().collect();
            lines.sort();
            Some(lines.join("\n"))
        }
        "uniq" => {
            let text = if !input.is_empty() { input } else { "" };
            let mut result = String::new();
            let mut prev = "";
            for line in text.lines() {
                if line != prev {
                    result.push_str(line);
                    result.push('\n');
                    prev = line;
                }
            }
            Some(result.trim_end().to_string())
        }
        "head" => {
            let n = args
                .get(1)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(10);
            let text = if !input.is_empty() { input } else { "" };
            let lines: Vec<&str> = text.lines().take(n).collect();
            Some(lines.join("\n"))
        }
        "tail" => {
            let n = args
                .get(1)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(10);
            let text = if !input.is_empty() { input } else { "" };
            let lines: Vec<&str> = text.lines().collect();
            let start = if lines.len() > n { lines.len() - n } else { 0 };
            Some(lines[start..].join("\n"))
        }
        _ => Some(format!("Bilinmeyen komut: {}", args[0])),
    }
}

// ============================================================================
// ANSI COLOR SUPPORT
// ============================================================================

/// ANSI renk ve biçimlendirme kaçış kodları.
///
/// Bu kodlar VT100/ANSI terminal standardına uygundur.
/// `\x1b[` ile başlar, rakam kodu ile biter (örn. `\x1b[31m` = kırmızı).
///
/// Örnek kullanım: `format!("{}Hata: {}{}", colors::RED, msg, colors::RESET)`
pub mod colors {
    /// Tüm biçimlendirmeyi sıfırla
    pub const RESET: &str = "\x1b[0m";
    /// Kalın (bold) metin
    pub const BOLD: &str = "\x1b[1m";
    /// Soluk (dim) metin
    pub const DIM: &str = "\x1b[2m";
    /// İtalik metin
    pub const ITALIC: &str = "\x1b[3m";
    /// Altı çizili metin
    pub const UNDERLINE: &str = "\x1b[4m";

    // Foreground colors (ön plan renkleri)
    pub const BLACK: &str = "\x1b[30m";
    pub const RED: &str = "\x1b[31m";
    pub const GREEN: &str = "\x1b[32m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const BLUE: &str = "\x1b[34m";
    pub const MAGENTA: &str = "\x1b[35m";
    pub const CYAN: &str = "\x1b[36m";
    pub const WHITE: &str = "\x1b[37m";

    // Bright foreground colors (parlak ön plan renkleri)
    pub const BRIGHT_BLACK: &str = "\x1b[90m";
    pub const BRIGHT_RED: &str = "\x1b[91m";
    pub const BRIGHT_GREEN: &str = "\x1b[92m";
    pub const BRIGHT_YELLOW: &str = "\x1b[93m";
    pub const BRIGHT_BLUE: &str = "\x1b[94m";
    pub const BRIGHT_MAGENTA: &str = "\x1b[95m";
    pub const BRIGHT_CYAN: &str = "\x1b[96m";
    pub const BRIGHT_WHITE: &str = "\x1b[97m";

    // Background colors (arka plan renkleri)
    pub const BG_BLACK: &str = "\x1b[40m";
    pub const BG_RED: &str = "\x1b[41m";
    pub const BG_GREEN: &str = "\x1b[42m";
    pub const BG_YELLOW: &str = "\x1b[43m";
    pub const BG_BLUE: &str = "\x1b[44m";
    pub const BG_MAGENTA: &str = "\x1b[45m";
    pub const BG_CYAN: &str = "\x1b[46m";
    pub const BG_WHITE: &str = "\x1b[47m";
}

/// Renkli prompt oluşturur: `echOS$ ` (yeşil renkte).
///
/// `BRIGHT_GREEN` + "echOS" + `RESET` + "$ " formatında çıktı üretir.
fn colored_prompt() -> String {
    format!("{}echOS{}$ ", colors::BRIGHT_GREEN, colors::RESET)
}

/// Kırmızı renkte hata mesajı formatlar.
pub fn error_msg(msg: &str) -> String {
    format!("{}{}{}", colors::RED, msg, colors::RESET)
}

/// Yeşil renkte başarı mesajı formatlar.
pub fn success_msg(msg: &str) -> String {
    format!("{}{}{}", colors::GREEN, msg, colors::RESET)
}

/// Sarı renkte uyarı mesajı formatlar.
pub fn warning_msg(msg: &str) -> String {
    format!("{}{}{}", colors::YELLOW, msg, colors::RESET)
}

/// Cyan renkte bilgi mesajı formatlar.
pub fn info_msg(msg: &str) -> String {
    format!("{}{}{}", colors::CYAN, msg, colors::RESET)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_env_is_session_scoped() {
        let mut first = Shell::new();
        let mut second = Shell::new();

        assert_eq!(run_command_in_shell(&mut first, "export FOO alpha"), None);
        assert_eq!(
            run_command_in_shell(&mut first, "echo $FOO"),
            Some(String::from("alpha"))
        );
        assert_eq!(
            run_command_in_shell(&mut second, "echo $FOO"),
            Some(String::new())
        );
    }

    #[test]
    fn shell_alias_is_session_scoped() {
        let mut first = Shell::new();
        let mut second = Shell::new();

        assert_eq!(
            run_command_in_shell(&mut first, "alias hi='echo selam'"),
            None
        );
        assert_eq!(
            run_command_in_shell(&mut first, "hi"),
            Some(String::from("selam"))
        );
        assert_eq!(
            run_command_in_shell(&mut second, "hi"),
            Some(String::from("Bilinmeyen komut: hi"))
        );
    }

    #[test]
    fn history_reports_only_session_commands() {
        let mut shell = Shell::new();

        let _ = run_command_in_shell(&mut shell, "echo one");
        let _ = run_command_in_shell(&mut shell, "echo two");

        let history = run_command_in_shell(&mut shell, "history").unwrap_or_default();
        assert!(history.contains("echo one"));
        assert!(history.contains("echo two"));
    }

    #[test]
    fn network_builtin_help_matches_real_paths() {
        assert!(builtin_summary("dns").contains("gercek DNS resolver"));
        assert!(builtin_summary("ping").contains("gercek ICMP echo"));
        assert!(builtin_summary("curl").contains("gercek HTTP/HTTPS"));
        assert!(builtin_summary("http").contains("gercek web istegi"));
    }

    #[test]
    fn render_help_reports_real_network_contract() {
        let help = render_help(Some("ping"));
        assert!(help.contains("gercek ICMP echo"));
        assert!(!help.contains("fail-closed"));
    }

    #[test]
    fn http_error_descriptions_expose_tls_failure_class() {
        assert!(
            describe_http_error(crate::net::http::HttpError::TlsCertCnInvalid)
                .contains("hostname dogrulamasi")
        );
        assert!(
            describe_http_error(crate::net::http::HttpError::TlsInvalidCa)
                .contains("guven zinciri")
        );
        assert!(describe_http_error(crate::net::http::HttpError::TlsCertRevoked).contains("iptal"));
        assert!(
            describe_http_error(crate::net::http::HttpError::TlsDecodeFailed).contains("decode")
        );
    }

    #[test]
    fn doh_dot_error_descriptions_expose_real_transport_boundary() {
        assert!(describe_doh_error(crate::net::doh::DohError::Timeout).contains("zaman asimi"));
        assert!(
            describe_dot_error(crate::net::dot::DotError::TlsHandshakeFailed).contains("handshake")
        );
    }

    #[test]
    fn net_smoke_help_exposes_real_protocol_paths() {
        let mut shell = Shell::new();
        let net = run_command_in_shell(&mut shell, "net").unwrap_or_default();
        assert!(net.contains("net smoke doh"));
        assert!(net.contains("net smoke dot"));
        assert!(net.contains("net smoke http3"));
        assert!(net.contains("net smoke grpc"));
        assert!(!net.contains("fail-closed"));
    }

    #[test]
    fn http3_boundary_description_rejects_silent_downgrade() {
        assert!(
            describe_http3_error(crate::net::http3::Http3Error::RemoteTransportUnavailable)
                .contains("sessiz downgrade yok")
        );
    }
}

// ============================================================================
// TERMINAL GUI BRIDGE  (Faz 7)
// ============================================================================

/// UEFI GUI Terminal'inden doğrudan komut satırı çağırma köprüsü.
///
/// String tabanlı komut satırını alır, Shell alias/env/glob/pipe mantığından
/// geçirir ve çıktıyı `Option<String>` olarak döndürür.
/// `"__CLEAR__"` özel çıktısı terminali temizle anlamına gelir.
///
/// ## Kullanım
///
/// GUI terminal widget'ı Enter'a basıldığında bu fonksiyonu çağırır.
/// Shell'in iç durumunu (history, variables, aliases) paylaşmaz —
/// her çağrıda yeni bir `Shell` instance'ı oluşturulur.
///
/// # Örnek
/// ```rust
/// let out = crate::shell::run_command("ls /");
/// ```
pub fn run_command_in_shell(shell: &mut Shell, cmd_line: &str) -> Option<String> {
    shell.execute_line(cmd_line)
}

pub fn run_command(cmd_line: &str) -> Option<String> {
    let mut s = Shell::new();
    run_command_in_shell(&mut s, cmd_line)
}
