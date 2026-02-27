//! # echOS Shell (Komut Satırı)
//!
//! Linux-level shell implementation.
//! Pipe, redirect, job control, tab completion, globbing, history search.
//! Scripting: variables, if/else, loops, functions, arithmetic.

pub mod editor;
pub mod advanced;
pub mod scripting;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use editor::GapBuffer;

/// Shell'i bir task olarak başlat
pub fn spawn_shell_task() {
    crate::task::scheduler::spawn_with_priority(shell_entry, crate::task::task::Priority::Normal, "shell");
}

/// Shell'i doğrudan çalıştır (blocking - scheduler olmadan)
pub fn run_shell() -> ! {
    shell_entry()
}

/// Hem serial hem de framebuffer'a yaz
fn print(s: &str) {
    // Framebuffer'a yaz
    crate::boot::term_print(s);
    // Serial çıktı
    crate::serial_print!("{}", s);
}

/// Hem serial hem de framebuffer'a satır yaz
fn println(s: &str) {
    print(s);
    print("\n");
}

/// Ekranı temizle
fn clear_screen() {
    // Framebuffer'ı temizle
    crate::boot::term_clear();
    // Serial clear
    crate::serial_print!("\x1b[2J\x1b[H");
}

/// Shell task entry point
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
                            // Ctrl+Z - SIGTSTP (job control - TODO)
                            println("^Z");
                            // TODO: Job control implement edildiğinde mevcut process'i suspend et
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
                                let words: Vec<&str> = input[..cursor_pos].split_whitespace().collect();
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
            // NOT: schedule() çağrısı PAGE FAULT'a neden olabiliyor
            for _ in 0..1000 {
                core::hint::spin_loop();
            }
        }
    }
}

/// Komut satırı shell yapısı
pub struct Shell {
    /// Metin düzenleme için gap buffer
    editor: GapBuffer,
    /// Komut geçmişi
    history: Vec<String>,
}

impl Shell {
    /// Yeni bir shell instance oluşturur.
    pub fn new() -> Self {
        Self {
            editor: GapBuffer::new(64),
            history: Vec::new(),
        }
    }

    /// Klavye tuşunu işler.
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

    /// Mevcut komutu çalıştırır ve sonucu döndürür.
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
        let has_redirect = tokens.iter().any(|t| matches!(t, 
            advanced::Token::RedirectOut | 
            advanced::Token::RedirectAppend | 
            advanced::Token::RedirectIn));
        
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
                "Mevcut komutlar: help, ver, echo, clear, ls, cat, launch, exe, wine, proton, linux",
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
                    return Some(String::from("Kullanim: launch <elf_dosyasi> [args...]"));
                }
                let argv: alloc::vec::Vec<&str> = parts[1..].to_vec();
                match load_and_run_elf_with_argv(parts[1], &argv) {
                    Ok(()) => None,
                    Err(msg) => Some(msg),
                }
            }
            "exe" => {
                // Kullanim:
                //   exe [--runtime wine|proton] [--save-only] <url> [dosya_adi]
                //   exe [--runtime wine|proton] --file <yerel_yol>
                if parts.len() < 2 {
                    return Some(String::from(
                        "Kullanim:\n  exe [--runtime wine|proton] [--save-only] <url> [dosya_adi]\n  exe [--runtime wine|proton] --file <yerel_yol>",
                    ));
                }
                // Parse flags
                let mut idx = 1usize;
                let mut runtime_kind: Option<crate::posix::WineRuntimeKind> = None;
                let mut save_only = false;
                let mut local_file: Option<&str> = None;

                while idx < parts.len() {
                    match parts[idx] {
                        "--runtime" => {
                            idx += 1;
                            runtime_kind = match parts.get(idx).copied() {
                                Some("wine")   => Some(crate::posix::WineRuntimeKind::Wine),
                                Some("proton") => Some(crate::posix::WineRuntimeKind::Proton),
                                other => return Some(format!(
                                    "Bilinmeyen runtime: {}", other.unwrap_or("(yok)")
                                )),
                            };
                            idx += 1;
                        }
                        "--save-only" => { save_only = true; idx += 1; }
                        "--file" => {
                            idx += 1;
                            local_file = parts.get(idx).copied();
                            if local_file.is_none() {
                                return Some(String::from("--file: yol eksik"));
                            }
                            idx += 1;
                        }
                        _ => break,
                    }
                }

                // Activate requested runtime
                if let Some(kind) = runtime_kind {
                    if let Err(e) = select_runtime_by_kind(kind) {
                        return Some(e);
                    }
                }

                // Helper: run PE image and return result string
                let run_image = |data: &[u8], path: &str| -> String {
                    crate::serial_println!("[EXE] Launching PE: {} ({} bytes)", path, data.len());
                    match crate::posix::run_windows_app_image(data) {
                        Ok(()) => {
                            if let Some(launch) = crate::posix::last_windows_launch() {
                                format!(
                                    "launch ok path={} runtime={} pid={} tid={} entry=0x{:08x} base=0x{:016x} imports={} exports={}",
                                    path,
                                    launch.runtime_name,
                                    launch.process_id,
                                    launch.thread_id,
                                    launch.entry_rva,
                                    launch.image_base,
                                    launch.import_count,
                                    launch.export_count,
                                )
                            } else {
                                format!("launch ok path={}", path)
                            }
                        }
                        Err(crate::posix::WineRuntimeError::NotFound) =>
                            String::from("HATA: Runtime secilmedi (once 'wine set <kok>' calistirin)"),
                        Err(crate::posix::WineRuntimeError::Invalid) =>
                            String::from("HATA: Gecersiz PE dosyasi"),
                        Err(crate::posix::WineRuntimeError::NotSupported) =>
                            String::from("HATA: PE calistirma henuz desteklenmiyor"),
                        Err(crate::posix::WineRuntimeError::SecureBootViolation) =>
                            String::from("HATA: Secure Boot aktif, imzasiz PE reddedildi"),
                    }
                };

                if let Some(fpath) = local_file {
                    // --file: yerel dosyayi oku ve calistir
                    crate::serial_println!("[EXE] --file mode: {}", fpath);
                    let data = match load_file(fpath) {
                        Ok(d) => d,
                        Err(msg) => return Some(format!("HATA: dosya okunamadi: {}", msg)),
                    };
                    crate::serial_println!("[EXE] Loaded {} bytes from {}", data.len(), fpath);
                    Some(run_image(&data, fpath))
                } else {
                    // URL modu
                    let url = match parts.get(idx) {
                        Some(u) => *u,
                        None => return Some(String::from("HATA: URL eksik")),
                    };
                    let filename = match parts.get(idx + 1).copied() {
                        Some(name) => sanitize_filename(name),
                        None => filename_from_url(url),
                    };
                    let path = format!("/{}", filename);

                    crate::serial_println!("[EXE] Indiriliyor: {} -> {}", url, path);
                    let data = match crate::net::http::HttpClient::new().get(url) {
                        Ok(resp) => {
                            crate::serial_println!("[EXE] Indirildi: {} bytes", resp.body.len());
                            resp.body
                        }
                        Err(_) => return Some(format!("HATA: Indirme basarisiz ({})", url)),
                    };

                    if let Err(e) = save_bytes_to_path(&path, &data) {
                        return Some(format!("HATA: Kaydetme basarisiz: {}", e));
                    }
                    crate::serial_println!("[EXE] Kaydedildi: {}", path);

                    if save_only {
                        Some(format!("kaydedildi={} boyut={} bytes", path, data.len()))
                    } else {
                        Some(run_image(&data, &path))
                    }
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
            "whoami" => Some(String::from("root")),
            "id" => Some(String::from("uid=0(root) gid=0(root)")),
            "uptime" => {
                let ticks = crate::task::scheduler::get_ticks();
                let secs = ticks / 100;
                let mins = secs / 60;
                let hours = mins / 60;
                Some(format!("up {}:{:02}:{:02}", hours, mins % 60, secs % 60))
            }
            "date" => {
                // TODO: RTC'den tarih oku
                Some(String::from("2026-01-01 00:00:00 (TODO: RTC)"))
            }
            "free" => {
                // TODO: Gerçek memory info
                Some(String::from("              total        used        free\nMem:         256M         64M        192M\nSwap:          0B          0B          0B"))
            }
            "df" => {
                // TODO: Gerçek disk info
                Some(String::from("Filesystem     Size  Used Avail Use% Mounted on\n/dev/sda1      256M   64M  192M  25% /"))
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
                    return Some(String::from("Kullanim: ping <ip|hostname>"));
                }
                let host = parts[1];
                Some(format!("PING {}: TODO (icmp modulu gerekli)", host))
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
            _ => Some(format!("Bilinmeyen komut: {}", parts[0])),
        }
    }

    /// Mevcut input satırını döndürür.
    pub fn get_input_line(&self) -> String {
        self.editor.to_string()
    }
}

/// ELF dosyasını yükler, argv'yi stack'e yazar ve Ring 3'e geçer.
fn load_and_run_elf_with_argv(path: &str, argv: &[&str]) -> Result<(), String> {
    let data = load_file(path)?;
    if data.is_empty() {
        return Err(String::from("ELF bos veya okunamadi"));
    }
    if argv.is_empty() {
        crate::task::user::enter_user_elf_from_image(&data)
            .map_err(|_| String::from("ELF yukleme basarisiz"))?;
    } else {
        crate::task::user::enter_user_elf_from_image_with_argv(&data, argv)
            .map_err(|_| String::from("ELF yukleme basarisiz"))?;
    }
    Ok(())
}

/// FAT32 üzerinden ELF dosyasını yükler ve Ring 3'e geçirir.
fn load_and_run_elf(path: &str) -> Result<(), String> {
    let data = load_file(path)?;
    if data.is_empty() {
        return Err(String::from("ELF bos veya okunamadi"));
    }
    crate::task::user::enter_user_elf_from_image(&data)
        .map_err(|_| String::from("ELF yukleme basarisiz"))?;
    Ok(())
}

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

fn sanitize_filename(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        String::from("app.exe")
    } else {
        out
    }
}

fn filename_from_url(url: &str) -> String {
    let without_query = url.split('?').next().unwrap_or(url);
    let without_fragment = without_query.split('#').next().unwrap_or(without_query);
    let raw = without_fragment.rsplit('/').next().unwrap_or("");
    sanitize_filename(raw)
}

fn save_bytes_to_path(path: &str, data: &[u8]) -> Result<(), String> {
    let trimmed = path.trim_start_matches('/');
    let (parent, name) = if let Some(pos) = trimmed.rfind('/') {
        (&trimmed[..pos], &trimmed[pos + 1..])
    } else {
        ("", trimmed)
    };
    if name.is_empty() {
        return Err(String::from("Gecersiz dosya adi"));
    }
    crate::fs::f2fs::create_f2fs_file_with_data(&format!("/{}", parent), name, data)
        .map_err(|_| String::from("Dosya kaydedilemedi"))
}

fn select_runtime_by_kind(kind: crate::posix::WineRuntimeKind) -> Result<(), String> {
    let runtimes = crate::posix::list_wine_runtimes();
    let target = runtimes.into_iter().find(|runtime| runtime.kind == kind);
    let Some(runtime) = target else {
        return Err(String::from("Runtime bulunamadi"));
    };
    crate::posix::select_wine_runtime(&runtime.name)
        .map_err(|_| String::from("Runtime bulunamadi"))
}

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
                Ok(()) => {
                    if let Some(launch) = crate::posix::last_windows_launch() {
                        Some(format!(
                            "launch ok runtime={} pid={} tid={} entry=0x{:08x} base=0x{:016x} imports={} exports={}",
                            launch.runtime_name,
                            launch.process_id,
                            launch.thread_id,
                            launch.entry_rva,
                            launch.image_base,
                            launch.import_count,
                            launch.export_count
                        ))
                    } else {
                        Some(String::from("launch ok"))
                    }
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

/// Glob pattern'larını expand eder (*.txt -> file1.txt file2.txt)
fn expand_globs(input: &str) -> String {
    let mut result = String::new();
    let mut in_word = false;
    let mut current_word = String::new();
    
    for c in input.chars() {
        if c == ' ' || c == '\t' {
            if in_word {
                // Word bitti - glob var mı kontrol et
                if current_word.contains('*') || current_word.contains('?') || current_word.contains('[') {
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

/// Tek glob pattern'ini expand eder
fn expand_glob_pattern(pattern: &str) -> String {
    // Dosya listesini al
    let files: Vec<String> = if let Ok(entries) = crate::fs::f2fs::list_dir("/") {
        entries.iter().map(|e| e.name.clone()).collect()
    } else {
        // Fallback mock data
        vec!["bin".to_string(), "boot".to_string(), "dev".to_string(), 
             "etc".to_string(), "home".to_string(), "lib".to_string()]
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

/// && ve || ile zincirlenmiş komutları çalıştırır
fn execute_chained(tokens: &[advanced::Token]) -> Option<String> {
    let mut current_cmd: Vec<String> = Vec::new();
    let mut last_success = true;  // İlk komut her zaman çalışır
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
                    last_success = last_output.is_none() || !last_output.as_ref().map(|o| o.contains("hata") || o.contains("Hata")).unwrap_or(false);
                }
                current_cmd.clear();
                
                if !last_success {
                    // Başarısız - geri kalanı atla (sonraki ||'ya kadar)
                    while i + 1 < tokens.len() {
                        i += 1;
                        if tokens[i] == advanced::Token::Or {
                            last_success = true;  // || sonrası çalışabilir
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
                    last_success = last_output.is_none() || !last_output.as_ref().map(|o| o.contains("hata") || o.contains("Hata")).unwrap_or(false);
                } else if last_success && !current_cmd.is_empty() {
                    // Önceki başarılı - bu komutu çalıştır ama sonucu kontrol et
                    let args: Vec<&str> = current_cmd.iter().map(|s| s.as_str()).collect();
                    last_output = execute_builtin(&args, None);
                    last_success = last_output.is_none() || !last_output.as_ref().map(|o| o.contains("hata") || o.contains("Hata")).unwrap_or(false);
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
                    last_success = last_output.is_none() || !last_output.as_ref().map(|o| o.contains("hata") || o.contains("Hata")).unwrap_or(false);
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

/// Pipeline'ı çalıştırır (cmd1 | cmd2 | cmd3)
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
                advanced::RedirectKind::Stdout | advanced::RedirectKind::StdoutAppend => {
                    // TODO: Dosyaya yaz
                    crate::serial_println!("[SHELL] Redirect to: {}", redirect.target);
                }
                advanced::RedirectKind::Stdin => {
                    // TODO: Dosyadan oku
                }
                advanced::RedirectKind::Stderr | advanced::RedirectKind::All => {
                    // TODO: Stderr redirect
                }
            }
        }
        
        last_output = output;
    }
    
    last_output
}

/// Built-in komut çalıştırır
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
                    Ok(data) => {
                        match core::str::from_utf8(&data) {
                            Ok(text) => Some(text.to_string()),
                            Err(_) => Some(String::from("Dosya metin degil")),
                        }
                    }
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
            let matches: Vec<&str> = text.lines()
                .filter(|line| line.contains(pattern))
                .collect();
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
            let n = args.get(1).and_then(|s| s.parse::<usize>().ok()).unwrap_or(10);
            let text = if !input.is_empty() { input } else { "" };
            let lines: Vec<&str> = text.lines().take(n).collect();
            Some(lines.join("\n"))
        }
        "tail" => {
            let n = args.get(1).and_then(|s| s.parse::<usize>().ok()).unwrap_or(10);
            let text = if !input.is_empty() { input } else { "" };
            let lines: Vec<&str> = text.lines().collect();
            let start = if lines.len() > n { lines.len() - n } else { 0 };
            Some(lines[start..].join("\n"))
        }
        _ => Some(format!("Bilinmeyen komut: {}", args[0]))
    }
}

// ============================================================================
// ANSI COLOR SUPPORT
// ============================================================================

/// ANSI renk kodları
pub mod colors {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
    pub const ITALIC: &str = "\x1b[3m";
    pub const UNDERLINE: &str = "\x1b[4m";
    
    // Foreground colors
    pub const BLACK: &str = "\x1b[30m";
    pub const RED: &str = "\x1b[31m";
    pub const GREEN: &str = "\x1b[32m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const BLUE: &str = "\x1b[34m";
    pub const MAGENTA: &str = "\x1b[35m";
    pub const CYAN: &str = "\x1b[36m";
    pub const WHITE: &str = "\x1b[37m";
    
    // Bright foreground colors
    pub const BRIGHT_BLACK: &str = "\x1b[90m";
    pub const BRIGHT_RED: &str = "\x1b[91m";
    pub const BRIGHT_GREEN: &str = "\x1b[92m";
    pub const BRIGHT_YELLOW: &str = "\x1b[93m";
    pub const BRIGHT_BLUE: &str = "\x1b[94m";
    pub const BRIGHT_MAGENTA: &str = "\x1b[95m";
    pub const BRIGHT_CYAN: &str = "\x1b[96m";
    pub const BRIGHT_WHITE: &str = "\x1b[97m";
    
    // Background colors
    pub const BG_BLACK: &str = "\x1b[40m";
    pub const BG_RED: &str = "\x1b[41m";
    pub const BG_GREEN: &str = "\x1b[42m";
    pub const BG_YELLOW: &str = "\x1b[43m";
    pub const BG_BLUE: &str = "\x1b[44m";
    pub const BG_MAGENTA: &str = "\x1b[45m";
    pub const BG_CYAN: &str = "\x1b[46m";
    pub const BG_WHITE: &str = "\x1b[47m";
}

/// Renkli prompt
fn colored_prompt() -> String {
    format!("{}echOS{}$ ", colors::BRIGHT_GREEN, colors::RESET)
}

/// Hata mesajı (kırmızı)
pub fn error_msg(msg: &str) -> String {
    format!("{}{}{}", colors::RED, msg, colors::RESET)
}

/// Başarı mesajı (yeşil)
pub fn success_msg(msg: &str) -> String {
    format!("{}{}{}", colors::GREEN, msg, colors::RESET)
}

/// Uyarı mesajı (sarı)
pub fn warning_msg(msg: &str) -> String {
    format!("{}{}{}", colors::YELLOW, msg, colors::RESET)
}

/// Bilgi mesajı (mavi)
pub fn info_msg(msg: &str) -> String {
    format!("{}{}{}", colors::CYAN, msg, colors::RESET)
}
