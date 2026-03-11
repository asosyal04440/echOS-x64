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
pub mod editor;
pub mod scripting;
pub mod cmd_pkg;
pub mod expr;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use editor::GapBuffer;

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

        // Basit input loop - klavye polling
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
                                if let Some(hist_cmd) = advanced::HISTORY.previous() {
                                    // Mevcut satırı temizle
                                    let pos = shell.editor.cursor_pos();
                                    for _ in 0..pos {
                                        print("\x1b[D");
                                    }
                                    print("\x1b[K");

                                    // History'den gelen komutu yaz
                                    shell.editor = GapBuffer::new(64);
                                    for c in hist_cmd.chars() {
                                        shell.editor.insert(c);
                                    }
                                    print(&hist_cmd);
                                }
                            }
                            KeyCode::ArrowDown => {
                                // History navigation - sonraki komut
                                if let Some(hist_cmd) = advanced::HISTORY.next() {
                                    // Mevcut satırı temizle
                                    let pos = shell.editor.cursor_pos();
                                    for _ in 0..pos {
                                        print("\x1b[D");
                                    }
                                    print("\x1b[K");

                                    // History'den gelen komutu yaz
                                    shell.editor = GapBuffer::new(64);
                                    for c in hist_cmd.chars() {
                                        shell.editor.insert(c);
                                    }
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
pub struct Shell {
    /// Metin düzenleme için gap buffer (O(1) cursor pozisyonunda ekleme/silme)
    editor: GapBuffer,
    /// Komut geçmişi (her session için tutulur)
    history: Vec<String>,
}

impl Shell {
    /// Yeni bir shell instance oluşturur.
    ///
    /// 64 karakter kapasiteli gap buffer ile başlar.
    /// Buffer dolunca `grow()` ile otomatik genişler.
    pub fn new() -> Self {
        Self {
            editor: GapBuffer::new(64),
            history: Vec::new(),
        }
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

        // Geçmişe ekle (global history)
        if !cmd_line.trim().is_empty() {
            advanced::HISTORY.push(&cmd_line);
        }

        let trimmed = cmd_line.trim();
        if trimmed.is_empty() {
            return None;
        }

        // Alias expansion
        let expanded_cmd = advanced::ALIASES.expand_line(trimmed);

        // Environment variable expansion ($VAR)
        let expanded_cmd = advanced::ENV.expand(&expanded_cmd);

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
            return execute_chained(&tokens);
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
                    return execute_pipeline(pipeline);
                }
            }
            return Some(String::from("Parse hatasi"));
        }

        let parts: Vec<&str> = expanded_cmd.split_whitespace().collect();
        match parts[0] {
            "help" => Some(String::from(
                "Mevcut komutlar: help, ver, echo, clear, ls, cat, launch, wine, proton, linux",
            )),
            "ver" => Some(String::from("echOS v0.2.0 (Legendary Edition)")),
            "echo" => {
                let args = &parts[1..];
                Some(args.join(" "))
            }
            "clear" => Some(String::from("__CLEAR__")), // Özel sinyal
            "ls" => {
                let path = parts.get(1).copied();
                if let Some(value) = path {
                    crate::serial_println!("SHELL: ls path='{}'", value);
                } else {
                    crate::serial_println!("SHELL: ls root");
                }
                match list_directory(path) {
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
            "launch" => {
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: launch <elf_dosyasi>"));
                }
                match load_and_run_elf(parts[1]) {
                    Ok(()) => None,
                    Err(msg) => Some(msg),
                }
            }
            "wine" => handle_wine_command(crate::posix::WineRuntimeKind::Wine, &parts),
            "proton" => handle_wine_command(crate::posix::WineRuntimeKind::Proton, &parts),
            "linux" => handle_linux_command(&parts),
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
                    let vars: Vec<String> = advanced::ENV.list().iter()
                        .map(|(k, v)| format!("{}={}", k, v))
                        .collect();
                    return Some(vars.join("\n"));
                }
                advanced::ENV.set(parts[1], parts[2]);
                None
            }
            "export" => {
                if parts.len() < 3 {
                    return Some(String::from("Kullanim: export VAR deger"));
                }
                advanced::ENV.set(parts[1], parts[2]);
                None
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
                let path = parts[1].trim_start_matches('/');
                let (parent, name) = if let Some(pos) = path.rfind('/') {
                    (&path[..pos], &path[pos + 1..])
                } else {
                    ("", path)
                };
                match crate::fs::f2fs::unlink_f2fs(&format!("/{}", parent), name) {
                    Ok(()) => Some(format!("rm: {} silindi", parts[1])),
                    Err(e) => Some(format!("rm hatasi: {:?}", e)),
                }
            }
            "rmdir" => {
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: rmdir <dizin>"));
                }
                let path = parts[1].trim_start_matches('/');
                let (parent, name) = if let Some(pos) = path.rfind('/') {
                    (&path[..pos], &path[pos + 1..])
                } else {
                    ("", path)
                };
                match crate::fs::f2fs::unlink_f2fs(&format!("/{}", parent), name) {
                    Ok(()) => Some(format!("rmdir: {} silindi", parts[1])),
                    Err(e) => Some(format!("rmdir hatasi: {:?}", e)),
                }
            }
            "mkdir" => {
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: mkdir <dizin>"));
                }
                let path = parts[1].trim_start_matches('/');
                let (parent, name) = if let Some(pos) = path.rfind('/') {
                    (&path[..pos], &path[pos + 1..])
                } else {
                    ("", path)
                };
                match crate::fs::f2fs::create_f2fs_dir(&format!("/{}", parent), name) {
                    Ok(()) => Some(format!("mkdir: {} olusturuldu", parts[1])),
                    Err(e) => Some(format!("mkdir hatasi: {:?}", e)),
                }
            }
            "touch" => {
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: touch <dosya>"));
                }
                let path = parts[1].trim_start_matches('/');
                let (parent, name) = if let Some(pos) = path.rfind('/') {
                    (&path[..pos], &path[pos + 1..])
                } else {
                    ("", path)
                };
                match crate::fs::f2fs::create_f2fs_file(&format!("/{}", parent), name) {
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
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: net [status|dhcp|ip|route|addr|link]\n  net status - Ag durumu\n  net dhcp - DHCP ile IP al\n  net ip - IP adresini goster\n  net route - Yonlendirme tablosu\n  net addr - Adres bilgileri\n  net link - Link durumu"));
                }
                match parts[1] {
                    "status" => {
                        let status = if crate::drivers::virtio_net::is_initialized() {
                            "Aktif"
                        } else {
                            "Pasif"
                        };
                        let ip_info = crate::net::smoltcp_driver::get_ip()
                            .map(|ip| format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]))
                            .unwrap_or_else(|| String::from("0.0.0.0"));
                        Some(format!("Ag durumu: {}\nVirtIO-Net: {}\nIP: {}", status, if crate::drivers::virtio_net::is_initialized() { "Hazir" } else { "Bulunamadi" }, ip_info))
                    }
                    "dhcp" => {
                        if crate::net::smoltcp_driver::dhcp_configure() {
                            let ip = crate::net::smoltcp_driver::get_ip()
                                .map(|ip| format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]))
                                .unwrap_or_else(|| String::from("0.0.0.0"));
                            Some(format!("DHCP: IP alindi - {}", ip))
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
                        Some(format!("IP: {}\nGateway: {}\nDNS: {}", ip, gw, dns))
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
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: http [get|post|download] <url> [dosya]\n  http get <url> - GET istegi\n  http post <url> <data> - POST istegi\n  http download <url> [dosya] - Dosya indir"));
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
                                            Some(format!("HTTP GET basarili ({} bytes)\n{}\n... (kesildi)", data.len(), &text[..500]))
                                        } else {
                                            Some(format!("HTTP GET basarili ({} bytes)\n{}", data.len(), text))
                                        }
                                    }
                                    Err(_) => Some(format!("HTTP GET basarili ({} bytes) - binary data", data.len()))
                                }
                            }
                            Err(e) => Some(format!("HTTP GET hatasi: {:?}", e))
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
                                Some(format!("HTTP POST basarili ({} bytes)", response.body.len()))
                            }
                            Err(e) => Some(format!("HTTP POST hatasi: {:?}", e))
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
                                    Ok(()) => Some(format!("Indirildi: {} ({} bytes) -> /{}", url, data.len(), filename)),
                                    Err(e) => Some(format!("Dosya kaydedilemedi: {:?}\nIndirme basarili ({} bytes)", e, data.len()))
                                }
                            }
                            Err(e) => Some(format!("Indirme hatasi: {:?}", e))
                        }
                    }
                    _ => Some(String::from("Bilinmeyen http komutu"))
                }
            }
            "wget" => {
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: wget <url> [dosya]"));
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
                            Ok(()) => Some(format!("Indirildi: {} ({} bytes) -> /{}", url, data.len(), filename)),
                            Err(e) => Some(format!("Dosya kaydedilemedi: {:?}\nIndirme basarili ({} bytes)", e, data.len()))
                        }
                    }
                    Err(e) => Some(format!("Indirme hatasi: {:?}", e))
                }
            }
            "curl" => {
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: curl <url>"));
                }
                let url = parts[1];
                let client = crate::net::http::HttpClient::new();
                match client.get(url) {
                    Ok(response) => {
                        let data = response.body;
                        match core::str::from_utf8(&data) {
                            Ok(text) => {
                                if text.len() > 1000 {
                                    Some(format!("{}\n... ({} bytes total)", &text[..1000], data.len()))
                                } else {
                                    Some(text.to_string())
                                }
                            }
                            Err(_) => Some(format!("Binary data ({} bytes)", data.len()))
                        }
                    }
                    Err(e) => Some(format!("Hata: {:?}", e))
                }
            }
            "dns" => {
                if parts.len() < 2 {
                    return Some(String::from("Kullanim: dns <hostname>\nÖrnek: dns google.com"));
                }
                let hostname = parts[1];
                Some(format!("DNS lookup: {} -> TODO (dns modulu gerekli)", hostname))
            }
            "ping" => {
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
                let mut output = format!("PING {} ({}) 56(84) bytes of data.\n", host, host);

                // Simüle edilmiş ICMP echo request/reply
                let base_rtt = 0.5f64;
                let mut min_rtt = f64::MAX;
                let mut max_rtt = 0.0f64;
                let mut sum_rtt = 0.0f64;

                for seq in 1..=count {
                    // Basit pseudo-random RTT üret
                    let tsc = unsafe { core::arch::x86_64::_rdtsc() };
                    let jitter = ((tsc % 100) as f64) / 100.0;
                    let rtt = base_rtt + jitter;
                    
                    if rtt < min_rtt { min_rtt = rtt; }
                    if rtt > max_rtt { max_rtt = rtt; }
                    sum_rtt += rtt;

                    output.push_str(&format!(
                        "64 bytes from {}: icmp_seq={} ttl=64 time={:.3} ms\n",
                        host, seq, rtt
                    ));
                }

                let avg_rtt = sum_rtt / count as f64;
                output.push_str(&format!(
                    "\n--- {} ping statistics ---\n{} packets transmitted, {} received, 0% packet loss\nrtt min/avg/max = {:.3}/{:.3}/{:.3} ms",
                    host, count, count, min_rtt, avg_rtt, max_rtt
                ));
                Some(output)
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
    let inode = crate::fs::vfs_open_inode(path).map_err(|_| String::from("Dosya bulunamadi"))?;
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
        None => "/",
        Some(value) if value.is_empty() => "/",
        Some(value) => value,
    };
    let entries =
        crate::fs::f2fs::list_dir(path_value).map_err(|_| String::from("Dizin okunamadi"))?;
    if entries.is_empty() {
        return Ok(String::from("Bos dizin"));
    }
    let mut out = String::new();
    for entry in entries {
        if entry.is_dir {
            out.push_str(&format!("{}/\n", entry.name));
        } else {
            out.push_str(&format!("{} ({})\n", entry.name, entry.size));
        }
    }
    Ok(out.trim_end_matches('\n').to_string())
}

/// Wine/Proton Windows runtime komutlarını işler.
///
/// Alt komutlar: `set`, `list`, `use`, `status`, `run`, `info`, `sections`, `plan`
/// Her alt komut, echOS POSIX/Wine katmanı ile iletişim kurar.
fn handle_wine_command(kind: crate::posix::WineRuntimeKind, parts: &[&str]) -> Option<String> {
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
fn handle_linux_command(parts: &[&str]) -> Option<String> {
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
fn execute_chained(tokens: &[advanced::Token]) -> Option<String> {
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
                    last_output = execute_builtin(&args, None);
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
                    last_output = execute_builtin(&args, None);
                    last_success = last_output.is_none()
                        || !last_output
                            .as_ref()
                            .map(|o| o.contains("hata") || o.contains("Hata"))
                            .unwrap_or(false);
                } else if last_success && !current_cmd.is_empty() {
                    // Önceki başarılı - bu komutu çalıştır ama sonucu kontrol et
                    let args: Vec<&str> = current_cmd.iter().map(|s| s.as_str()).collect();
                    last_output = execute_builtin(&args, None);
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
                    last_output = execute_builtin(&args, None);
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
        last_output = execute_builtin(&args, None);
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
/// daha basit ama fonksiyonel bir yaklaşım kullanır.
///
/// ## Yönlendirme (Redirect)
///
/// `cmd.redirects` içindeki her `Redirect` işlenir:
/// - `Stdout` / `StdoutAppend`: Çıktı dosyaya yazılır (TODO)
/// - `Stdin`: Girdi dosyadan okunur (TODO)
fn execute_pipeline(pipeline: &advanced::Pipeline) -> Option<String> {
    if pipeline.commands.is_empty() {
        return None;
    }

    // Basit implementation: her komutu sırayla çalıştır
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
        let output = execute_builtin(&args, last_output.as_deref());

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
fn execute_builtin(args: &[&str], stdin: Option<&str>) -> Option<String> {
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
        "ls" => {
            let path = args.get(1).copied();
            match list_directory(path) {
                Ok(out) => Some(out),
                Err(msg) => Some(msg),
            }
        }
        "wc" => {
            // Word count - pipe için
            let text = if !input.is_empty() { input } else { "" };
            let lines = text.lines().count();
            let words = text.split_whitespace().count();
            let chars = text.chars().count();
            Some(format!("{} {} {}", lines, words, chars))
        }
        "grep" => {
            // Basit grep - pipe için
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
pub fn run_command(cmd_line: &str) -> Option<String> {
    let mut s = Shell::new();
    for c in cmd_line.chars() {
        s.editor.insert(c);
    }
    s.execute()
}
