//! # echOS Shell (Komut Satırı Yorumlayıcısı)
//!
//! Ring 3 shell altyapısı. Tüm komutlar echshell (Ring 3 user-mode) üzerinden çalıştırılır.
//! Bu modül shell API soyutlaması (shell_api), syscall wrapper'ları (shell_syscall),
//! veRing 3 shell başlatma (run_shell_ring3) sağlar.

pub mod advanced;
pub mod cmd_eon;
pub mod editor;
pub mod expr;
pub mod scripting;

#[cfg(test)]
static SHELL_GLOBAL_TEST_EPOCH: spin::Lazy<spin::Mutex<()>> =
    spin::Lazy::new(|| spin::Mutex::new(()));

#[cfg(test)]
pub(crate) fn shell_global_test_epoch() -> spin::MutexGuard<'static, ()> {
    SHELL_GLOBAL_TEST_EPOCH.lock()
}
pub mod shell_api;
pub mod shell_syscall;

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use editor::GapBuffer;

#[repr(align(64))]
struct CacheLineAtomicBool {
    value: AtomicBool,
}

impl CacheLineAtomicBool {
    const fn new(value: bool) -> Self {
        Self {
            value: AtomicBool::new(value),
        }
    }

    fn load(&self, order: Ordering) -> bool {
        self.value.load(order)
    }

    fn store(&self, value: bool, order: Ordering) {
        self.value.store(value, order);
    }
}

#[repr(align(64))]
struct CacheLineAtomicU8 {
    value: AtomicU8,
}

impl CacheLineAtomicU8 {
    const fn new(value: u8) -> Self {
        Self {
            value: AtomicU8::new(value),
        }
    }

    fn load(&self, order: Ordering) -> u8 {
        self.value.load(order)
    }

    fn store(&self, value: u8, order: Ordering) {
        self.value.store(value, order);
    }
}

static SHELL_RUNTIME_READY: AtomicBool = AtomicBool::new(false);
const SESSION_HISTORY_LIMIT: usize = 1000;
const PRODUCT_NETWORK_SURFACE_ENABLED: bool = true;

#[cfg(any(
    test,
    all(
        feature = "host_smoke",
        not(target_os = "none"),
        not(target_os = "uefi")
    )
))]
lazy_static::lazy_static! {
    static ref HOST_SHELL_FILES: spin::Mutex<BTreeMap<String, Vec<u8>>> =
        spin::Mutex::new(BTreeMap::new());
}

#[cfg(any(
    test,
    all(
        feature = "host_smoke",
        not(target_os = "none"),
        not(target_os = "uefi")
    )
))]
fn host_shell_file(path: &str) -> Option<Vec<u8>> {
    HOST_SHELL_FILES.lock().get(path).cloned()
}

#[cfg(not(any(
    test,
    all(
        feature = "host_smoke",
        not(target_os = "none"),
        not(target_os = "uefi")
    )
)))]
fn host_shell_file(_path: &str) -> Option<Vec<u8>> {
    None
}

#[cfg(any(
    test,
    all(
        feature = "host_smoke",
        not(target_os = "none"),
        not(target_os = "uefi")
    )
))]
fn host_shell_write_file(path: &str, data: &[u8], append: bool) -> usize {
    let mut files = HOST_SHELL_FILES.lock();
    let entry = files.entry(path.to_string()).or_default();
    if append {
        entry.extend_from_slice(data);
    } else {
        entry.clear();
        entry.extend_from_slice(data);
    }
    data.len()
}

#[cfg(not(any(
    test,
    all(
        feature = "host_smoke",
        not(target_os = "none"),
        not(target_os = "uefi")
    )
)))]
fn host_shell_write_file(_path: &str, _data: &[u8], _append: bool) -> usize {
    0
}

#[cfg(any(
    test,
    all(
        feature = "host_smoke",
        not(target_os = "none"),
        not(target_os = "uefi")
    )
))]
fn host_shell_remove_file(path: &str) -> bool {
    HOST_SHELL_FILES.lock().remove(path).is_some()
}

#[cfg(not(any(
    test,
    all(
        feature = "host_smoke",
        not(target_os = "none"),
        not(target_os = "uefi")
    )
)))]
fn host_shell_remove_file(_path: &str) -> bool {
    false
}

#[cfg(any(
    test,
    all(
        feature = "host_smoke",
        not(target_os = "none"),
        not(target_os = "uefi")
    )
))]
fn host_shell_truncate_file(path: &str, new_size: usize) -> bool {
    let mut files = HOST_SHELL_FILES.lock();
    if let Some(data) = files.get_mut(path) {
        data.resize(new_size, 0u8);
        true
    } else {
        false
    }
}

#[cfg(not(any(
    test,
    all(
        feature = "host_smoke",
        not(target_os = "none"),
        not(target_os = "uefi")
    )
)))]
fn host_shell_truncate_file(_path: &str, _new_size: usize) -> bool {
    false
}

pub(crate) fn output_indicates_failure(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    output.starts_with("Kullanim:")
        || output.starts_with("Usage:")
        || output.starts_with("Bilinmeyen komut:")
        || lower.contains(" hata")
        || lower.contains("hatasi")
        || lower.contains("basarisiz")
        || lower.contains("bulunamadi")
}

pub(crate) fn command_exit_code(output: &Option<String>) -> i64 {
    if output
        .as_ref()
        .map(|value| output_indicates_failure(value))
        .unwrap_or(false)
    {
        1
    } else {
        0
    }
}

pub fn builtin_command_names() -> &'static [&'static str] {
    &[]
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

/// Shell'i Ring 3'te (user mode) çalıştır
/// Boot sırasında çağrılır — shell'i user-mode task olarak başlatır
pub fn run_shell_ring3() -> ! {
    crate::serial_println!("[SHELL] Starting Ring 3 shell...");

    let shell_data: &[u8] = &[];
    #[cfg(any(target_os = "none", target_os = "uefi"))]
    let shell_data = include_bytes!(concat!(env!("OUT_DIR"), "/echshell.bin"));

    match shell_api::proc_spawn_user_image(shell_data, 2, "echshell") {
        Ok(task_id) => {
            crate::serial_println!("[SHELL] Ring 3 shell spawned as task {}", task_id);
        }
        Err(_e) => {
            crate::serial_println!("[SHELL] Failed to spawn Ring 3 shell");
        }
    }
    loop {
        crate::debug_diag!(
            "[SHELL_TEST] scheduler pump queue0={}",
            crate::task::scheduler::queued_task_count(0)
        );
        crate::task::scheduler::schedule();
        #[cfg(any(target_os = "none", target_os = "uefi"))]
        x86_64::instructions::hlt();
        #[cfg(not(any(target_os = "none", target_os = "uefi")))]
        core::hint::spin_loop();
    }
}

// ============================================================================
// SHELL STATE TYPES (used by scripting, PTY, GUI terminal)
// ============================================================================

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

            if chars.peek() == Some(&'?') {
                chars.next();
                if let Some(value) = self.get("?") {
                    result.push_str(&value);
                } else {
                    result.push('0');
                }
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

    fn save_to_file(&self, path: &str) {
        let mut content = alloc::string::String::new();
        for entry in &self.entries {
            content.push_str(entry);
            content.push('\n');
        }
        let fd = shell_api::fs_open(
            path,
            shell_api::O_WRONLY | shell_api::O_CREAT | shell_api::O_TRUNC,
        );
        if fd >= 0 {
            let _ = shell_api::fs_write(fd, content.as_bytes());
            shell_api::fs_close(fd);
        }
    }

    fn load_from_file(&mut self, path: &str) {
        let fd = shell_api::fs_open(path, shell_api::O_RDONLY);
        if fd < 0 {
            return;
        }
        let mut buf = alloc::vec![0u8; 65536];
        if let Ok(n) = shell_api::fs_read(fd, &mut buf) {
            shell_api::fs_close(fd);
            if n == 0 {
                return;
            }
            buf.truncate(n);
            if let Ok(content) = core::str::from_utf8(&buf) {
                for line in content.lines() {
                    let line = line.trim();
                    if !line.is_empty() && self.entries.len() < SESSION_HISTORY_LIMIT {
                        self.entries.push(line.to_string());
                    }
                }
                self.cursor = self.entries.len();
            }
        } else {
            shell_api::fs_close(fd);
        }
    }
}

/// Shell durum yapısı — scripting, PTY, ve GUI terminal tarafından kullanılır
pub struct Shell {
    editor: GapBuffer,
    history: ShellHistory,
    env: ShellEnvironment,
    aliases: ShellAliases,
    last_exit_code: i64,
}

impl Shell {
    pub fn new() -> Self {
        ensure_shell_runtime_ready();
        Self {
            editor: GapBuffer::new(64),
            history: ShellHistory::default(),
            env: ShellEnvironment::seeded(),
            aliases: ShellAliases::seeded(),
            last_exit_code: 0,
        }
    }

    fn set_exit_code(&mut self, code: i64) {
        self.last_exit_code = code;
        self.env.set("?", &code.to_string());
    }

    fn load_startup_files(&mut self) {
        self.execute_profile_file("/etc/profile");
        let home = self
            .env
            .get("HOME")
            .unwrap_or_else(|| String::from("/root"));
        let profile = alloc::format!("{}/.profile", home);
        self.execute_profile_file(&profile);
        let history_path = alloc::format!("{}/.history", home);
        self.history.load_from_file(&history_path);
    }

    fn execute_profile_file(&mut self, path: &str) {
        let fd = shell_api::fs_open(path, shell_api::O_RDONLY);
        if fd < 0 {
            return;
        }
        let mut buf = alloc::vec![0u8; 32768];
        if let Ok(n) = shell_api::fs_read(fd, &mut buf) {
            shell_api::fs_close(fd);
            if n == 0 {
                return;
            }
            buf.truncate(n);
            if let Ok(content) = core::str::from_utf8(&buf) {
                for line in content.lines() {
                    let line = line.trim();
                    if !line.is_empty() && !line.starts_with('#') {
                        self.execute_line(line);
                    }
                }
            }
        } else {
            shell_api::fs_close(fd);
        }
    }

    fn save_history(&self) {
        let home = self
            .env
            .get("HOME")
            .unwrap_or_else(|| String::from("/root"));
        let history_path = alloc::format!("{}/.history", home);
        self.history.save_to_file(&history_path);
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

    pub(crate) fn set_session_env(&mut self, key: &str, value: &str) {
        self.env.set(key, value);
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
        shell_api::fs_list_dir(&desired).map_err(|_| String::from("Dizin bulunamadi"))?;
        self.env.set("OLDPWD", &previous);
        self.env.set("PWD", &desired);
        self.sync_runtime_state();
        Ok(desired)
    }

    pub fn handle_key(&mut self, key: pc_keyboard::DecodedKey) {
        use pc_keyboard::DecodedKey;
        match key {
            DecodedKey::Unicode(c) => match c {
                '\n' => {}
                '\x08' => {
                    self.editor.delete();
                }
                _ => self.editor.insert(c),
            },
            DecodedKey::RawKey(code) => {
                use pc_keyboard::KeyCode;
                match code {
                    KeyCode::ArrowLeft => self.editor.move_left(),
                    KeyCode::ArrowRight => self.editor.move_right(),
                    KeyCode::ArrowUp => {}
                    KeyCode::ArrowDown => {}
                    _ => {}
                }
            }
        }
    }

    pub fn execute(&mut self) -> Option<String> {
        let cmd_line = self.editor.to_string();
        self.editor = GapBuffer::new(64);
        self.execute_line(&cmd_line)
    }

    pub fn execute_line(&mut self, cmd_line: &str) -> Option<String> {
        self.history.push(cmd_line);

        let trimmed = cmd_line.trim();
        if trimmed.is_empty() {
            self.set_exit_code(0);
            return None;
        }
        self.sync_runtime_state();

        let expanded_cmd = self.aliases.expand_line(trimmed);
        let expanded_cmd = self.env.expand(&expanded_cmd);

        let parts: Vec<&str> = expanded_cmd.split_whitespace().collect();
        if parts.is_empty() {
            self.set_exit_code(0);
            return None;
        }

        let output = match parts[0] {
            "cd" => match self.change_directory(parts.get(1).copied()) {
                Ok(path) => Some(path),
                Err(msg) => Some(msg),
            },
            "echo" => Some(parts[1..].join(" ")),
            "pwd" => Some(self.current_working_directory()),
            "export" | "set" | "unset" | "env" => self.execute_export_command(&parts),
            "history" => {
                let entries: Vec<String> = self
                    .history
                    .list()
                    .iter()
                    .map(|(i, cmd)| format!("{:>4}  {}", i, cmd))
                    .collect();
                Some(entries.join("\n"))
            }
            "alias" | "unalias" => self.execute_alias_command(&expanded_cmd),
            "clear" => Some(String::from("__CLEAR__")),
            _ => Some(format!("Bilinmeyen komut: {}", parts[0])),
        };

        self.set_exit_code(command_exit_code(&output));
        output
    }

    fn execute_export_command(&mut self, parts: &[&str]) -> Option<String> {
        match parts[0] {
            "export" => {
                if parts.len() < 3 {
                    let vars: Vec<String> = self
                        .env
                        .list()
                        .iter()
                        .map(|(k, v)| format!("{}={}", k, v))
                        .collect();
                    Some(vars.join("\n"))
                } else {
                    self.env.set(parts[1], &parts[2..].join(" "));
                    self.sync_runtime_state();
                    None
                }
            }
            "set" => {
                if parts.len() < 2 {
                    let vars: Vec<String> = self
                        .env
                        .list()
                        .iter()
                        .map(|(k, v)| format!("{}={}", k, v))
                        .collect();
                    Some(vars.join("\n"))
                } else {
                    self.env.set(parts[1], &parts[2..].join(" "));
                    self.sync_runtime_state();
                    None
                }
            }
            "unset" => {
                if parts.len() < 2 {
                    Some(String::from("Kullanim: unset <degisken>"))
                } else {
                    self.env.unset(parts[1]);
                    self.sync_runtime_state();
                    None
                }
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
            _ => unreachable!(),
        }
    }

    fn execute_alias_command(&mut self, line: &str) -> Option<String> {
        let tokens = advanced::Tokenizer::tokenize(line);
        let aliases: Vec<String> = tokens
            .into_iter()
            .filter_map(|token| match token {
                advanced::Token::Word(word) => Some(word),
                _ => None,
            })
            .skip(1)
            .collect();
        if aliases.is_empty() {
            let aliases: Vec<String> = self
                .aliases
                .list()
                .iter()
                .map(|(name, expansion)| format!("alias {}='{}'", name, expansion))
                .collect();
            return Some(aliases.join("\n"));
        }
        for alias in aliases {
            if let Some((name, value)) = alias.split_once('=') {
                self.aliases.set(name, value);
            } else {
                return Some(String::from("Kullanim: alias ad='genisleme'"));
            }
        }
        self.sync_runtime_state();
        None
    }

    pub fn get_input_line(&self) -> String {
        self.editor.to_string()
    }
}

fn ensure_shell_runtime_ready() {
    if SHELL_RUNTIME_READY.load(Ordering::Acquire) {
        return;
    }
    advanced::ENV.init_defaults();
    advanced::init();
    SHELL_RUNTIME_READY.store(true, Ordering::Release);
}

// ============================================================================
// TERMINAL GUI BRIDGE  (Faz 7)
// ============================================================================

/// UEFI GUI Terminal'inden doğrudan komut satırı çağırma köprüsü.
pub fn run_command_in_shell(shell: &mut Shell, cmd_line: &str) -> Option<String> {
    shell.execute_line(cmd_line)
}

pub fn run_command(cmd_line: &str) -> Option<String> {
    let mut s = Shell::new();
    run_command_in_shell(&mut s, cmd_line)
}

// ============================================================================
// ANSI COLOR HELPERS
// ============================================================================

pub mod colors {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
    pub const ITALIC: &str = "\x1b[3m";
    pub const UNDERLINE: &str = "\x1b[4m";

    pub const BLACK: &str = "\x1b[30m";
    pub const RED: &str = "\x1b[31m";
    pub const GREEN: &str = "\x1b[32m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const BLUE: &str = "\x1b[34m";
    pub const MAGENTA: &str = "\x1b[35m";
    pub const CYAN: &str = "\x1b[36m";
    pub const WHITE: &str = "\x1b[37m";

    pub const BRIGHT_BLACK: &str = "\x1b[90m";
    pub const BRIGHT_RED: &str = "\x1b[91m";
    pub const BRIGHT_GREEN: &str = "\x1b[92m";
    pub const BRIGHT_YELLOW: &str = "\x1b[93m";
    pub const BRIGHT_BLUE: &str = "\x1b[94m";
    pub const BRIGHT_MAGENTA: &str = "\x1b[95m";
    pub const BRIGHT_CYAN: &str = "\x1b[96m";
    pub const BRIGHT_WHITE: &str = "\x1b[97m";

    pub const BG_BLACK: &str = "\x1b[40m";
    pub const BG_RED: &str = "\x1b[41m";
    pub const BG_GREEN: &str = "\x1b[42m";
    pub const BG_YELLOW: &str = "\x1b[43m";
    pub const BG_BLUE: &str = "\x1b[44m";
    pub const BG_MAGENTA: &str = "\x1b[45m";
    pub const BG_CYAN: &str = "\x1b[46m";
    pub const BG_WHITE: &str = "\x1b[47m";
}

pub fn error_msg(msg: &str) -> String {
    format!("{}{}{}", colors::RED, msg, colors::RESET)
}

pub fn success_msg(msg: &str) -> String {
    format!("{}{}{}", colors::GREEN, msg, colors::RESET)
}

pub fn warning_msg(msg: &str) -> String {
    format!("{}{}{}", colors::YELLOW, msg, colors::RESET)
}

pub fn info_msg(msg: &str) -> String {
    format!("{}{}{}", colors::CYAN, msg, colors::RESET)
}

#[cfg(test)]
mod tests {
    use super::*;

    static SHELL_SESSION_TEST_EPOCH: spin::Lazy<spin::Mutex<()>> =
        spin::Lazy::new(|| spin::Mutex::new(()));

    struct ShellSessionTestEpoch {
        _global: spin::MutexGuard<'static, ()>,
        _session: spin::MutexGuard<'static, ()>,
    }

    fn shell_session_test_epoch() -> ShellSessionTestEpoch {
        let global = crate::shell::shell_global_test_epoch();
        let session = SHELL_SESSION_TEST_EPOCH.lock();
        reset_shell_test_globals();
        ShellSessionTestEpoch {
            _global: global,
            _session: session,
        }
    }

    fn reset_shell_test_globals() {
        advanced::reset_advanced_test_globals();
    }

    #[test]
    fn shell_env_is_session_scoped() {
        let _epoch = shell_session_test_epoch();
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
        let _epoch = shell_session_test_epoch();
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
        let _epoch = shell_session_test_epoch();
        let mut shell = Shell::new();

        let _ = run_command_in_shell(&mut shell, "echo one");
        let _ = run_command_in_shell(&mut shell, "echo two");

        let history = run_command_in_shell(&mut shell, "history").unwrap_or_default();
        assert!(history.contains("echo one"));
        assert!(history.contains("echo two"));
    }
}
