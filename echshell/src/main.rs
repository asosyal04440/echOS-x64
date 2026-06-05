#![no_std]
#![no_main]

use core::alloc::{GlobalAlloc, Layout};
use core::cell::UnsafeCell;
use core::panic::PanicInfo;

extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::format;

mod shell_syscall;
mod tokenizer;
mod builtins;
mod executor;
mod environment;
mod history;
mod scripting;

use shell_syscall as sc;
use environment::ShellEnv;
use history::ShellHistory;

// ============================================================================
// BUMP ALLOCATOR
// ============================================================================

fn dbg_print(_msg: &[u8], _val: usize) {
    // Debug print devre disi — allocation loglari shell test output'la karisiyordu
}

use core::sync::atomic::{AtomicUsize, Ordering};

struct BumpAllocator {
    heap_end: AtomicUsize,
    next: AtomicUsize,
}

unsafe impl Send for BumpAllocator {}
unsafe impl Sync for BumpAllocator {}

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let align = layout.align();
        let size = layout.size();
        loop {
            let current_next = self.next.load(Ordering::Relaxed);
            let current_heap_end = self.heap_end.load(Ordering::Relaxed);
            
            // If the allocator hasn't been initialized yet, return null
            if current_heap_end == 0 {
                return core::ptr::null_mut();
            }

            let ptr = (current_next + align - 1) & !(align - 1);
            let new_next = ptr + size;
            if new_next > current_heap_end {
                let new_break = (new_next + 4095) & !4095;
                dbg_print(b"[ALLOC] requesting new_break", new_break);
                let ret = sc::raw_syscall(12, new_break, 0, 0, 0, 0, 0);
                if ret < 0 {
                    dbg_print(b"[ALLOC] raw_syscall 12 failed", ret as usize);
                    return core::ptr::null_mut();
                }
                let returned_heap_end = ret as usize;
                self.heap_end.store(returned_heap_end, Ordering::Relaxed);
                dbg_print(b"[ALLOC] raw_syscall 12 returned heap_end", returned_heap_end);
                if new_next > returned_heap_end {
                    dbg_print(b"[ALLOC] new_next still > heap_end", new_next);
                    return core::ptr::null_mut();
                }
                if self.next.compare_exchange(current_next, new_next, Ordering::Relaxed, Ordering::Relaxed).is_ok() {
                    dbg_print(b"[ALLOC] returning ptr", ptr);
                    return ptr as *mut u8;
                }
            } else {
                if self.next.compare_exchange(current_next, new_next, Ordering::Relaxed, Ordering::Relaxed).is_ok() {
                    dbg_print(b"[ALLOC] returning ptr", ptr);
                    return ptr as *mut u8;
                }
            }
        }
    }
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator {
    heap_end: AtomicUsize::new(0),
    next: AtomicUsize::new(0),
};

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    let _ = sc::sys_write(2, b"[echshell] PANIC\n");
    loop { unsafe { core::arch::asm!("hlt"); } }
}

// ============================================================================
// SHELL STATE
// ============================================================================

pub struct ShellState {
    pub env: ShellEnv,
    pub history: ShellHistory,
    pub exit_code: i32,
    pub running: bool,
    pub job_id_counter: u32,
    pub jobs: Vec<Job>,
}

pub struct Job {
    pub id: u32,
    pub pid: usize,
    pub cmd: String,
    pub running: bool,
    pub background: bool,
}

impl ShellState {
    fn new() -> Self {
        let mut env = ShellEnv::new();
        env.set("SHELL", "/bin/echshell");
        env.set("HOME", "/root");
        env.set("PATH", "/bin:/usr/bin:/sbin");
        env.set("USER", "root");
        env.set("TERM", "xterm-256color");
        env.set("PWD", "/");
        env.set("PS1", "$ ");
        env.set("PS2", "> ");
        env.set("PS4", "+ ");
        env.set("IFS", " \t\n");
        env.set("LINENO", "1");
        env.set("PPID", "0");
        env.set("SECONDS", "0");
        env.set("_shell_name", "echshell");
        env.set("ECHOSHELL_VERSION", "1.0.0");
        env.set("OPTIND", "1");
        Self {
            env,
            history: ShellHistory::new(),
            exit_code: 0,
            running: true,
            job_id_counter: 0,
            jobs: Vec::new(),
        }
    }
}

// ============================================================================
// YAZI YARDIMCILARI
// ============================================================================

pub fn print(s: &str) { let _ = sc::sys_write(1, s.as_bytes()); }
pub fn eprint(s: &str) { let _ = sc::sys_write(2, s.as_bytes()); }
pub fn println(s: &str) { print(s); print("\n"); }
pub fn eprintln_fn(s: &str) { eprint(s); eprint("\n"); }
pub fn write_fd(fd: usize, s: &str) { let _ = sc::sys_write(fd, s.as_bytes()); }

pub fn read_line(prompt: &str, state: Option<&ShellState>) -> Option<String> {
    print(prompt);
    let mut line = String::new();
    let mut cursor_pos = 0usize;
    let mut buf = [0u8; 1];
    let mut escape_buf = [0u8; 3];
    let mut history_index: Option<usize> = None;
    loop {
        match sc::sys_read(0, &mut buf) {
            Ok(1) => match buf[0] {
                b'\n' | b'\r' => { print("\n"); return Some(line); }
                0x08 | 0x7F => {
                    if cursor_pos > 0 {
                        line.remove(cursor_pos - 1);
                        cursor_pos -= 1;
                        print("\x08");
                        let remaining: String = line[cursor_pos..].chars().collect();
                        print(&remaining);
                        print(" \x08");
                        for _ in remaining.chars() { print("\x08"); }
                    }
                }
                0x03 => { println("^C"); return Some(String::from("__ECHOS_SIGINT__")); }
                0x04 => { println("^D"); return None; }
                0x09 => {
                    if let Some(s) = state {
                        let completed = complete_word(&line, cursor_pos, s);
                        if completed != line {
                            let old_len = line.len();
                            line = completed;
                            cursor_pos = line.len();
                            let _tail: String = line.chars().skip(cursor_pos).collect();
                            for _ in 0..old_len { print("\x08 \x08"); }
                            print(&line);
                        }
                    }
                }
                0x12 => {
                    if let Some(s) = state {
                        let history = s.history.list();
                        if !history.is_empty() {
                            let idx = history_index.unwrap_or(history.len());
                            if idx > 0 {
                                let new_idx = idx - 1;
                                history_index = Some(new_idx);
                                let old_len = line.len();
                                for _ in 0..old_len { print("\x08 \x08"); }
                                line = history[new_idx].clone();
                                cursor_pos = line.len();
                                print(&line);
                            }
                        }
                    }
                }
                0x1A => {
                    println("^Z");
                    return Some(String::from("__ECHOS_SIGTSTP__"));
                }
                0x1C => {
                    println("^\\");
                    return Some(String::from("__ECHOS_SIGQUIT__"));
                }
                0x01 => {
                    while cursor_pos > 0 {
                        print("\x08");
                        cursor_pos -= 1;
                    }
                }
                0x05 => {
                    let remaining: String = line[cursor_pos..].chars().collect();
                    print(&remaining);
                    cursor_pos = line.len();
                }
                0x15 => {
                    if cursor_pos > 0 {
                        let removed: String = line[..cursor_pos].chars().collect();
                        line = line[cursor_pos..].to_string();
                        cursor_pos = 0;
                        for _ in removed.chars() { print("\x08 \x08"); }
                        let remaining: String = line.clone();
                        print(&remaining);
                        for _ in remaining.chars() { print("\x08"); }
                    }
                }
                0x0B => {
                    line.truncate(cursor_pos);
                    print("\x1b[K");
                }
                0x06 => {
                    if cursor_pos < line.len() {
                        cursor_pos += 1;
                        print("\x1b[C");
                    }
                }
                0x02 => {
                    if cursor_pos > 0 {
                        cursor_pos -= 1;
                        print("\x1b[D");
                    }
                }
                0x1B => {
                    if sc::sys_read(0, &mut escape_buf[..1]).is_ok() && escape_buf[0] == b'[' {
                        if sc::sys_read(0, &mut escape_buf[..1]).is_ok() {
                            match escape_buf[0] {
                                b'A' => {
                                    if let Some(s) = state {
                                        let history = s.history.list();
                                        if !history.is_empty() {
                                            let idx = history_index.unwrap_or(history.len());
                                            if idx > 0 {
                                                let new_idx = idx - 1;
                                                history_index = Some(new_idx);
                                                let old_len = line.len();
                                                for _ in 0..old_len { print("\x08 \x08"); }
                                                line = history[new_idx].clone();
                                                cursor_pos = line.len();
                                                print(&line);
                                            }
                                        }
                                    }
                                }
                                b'B' => {
                                    if let Some(s) = state {
                                        let history = s.history.list();
                                        if let Some(idx) = history_index {
                                            if idx + 1 < history.len() {
                                                let new_idx = idx + 1;
                                                history_index = Some(new_idx);
                                                let old_len = line.len();
                                                for _ in 0..old_len { print("\x08 \x08"); }
                                                line = history[new_idx].clone();
                                                cursor_pos = line.len();
                                                print(&line);
                                            } else {
                                                history_index = None;
                                                let old_len = line.len();
                                                for _ in 0..old_len { print("\x08 \x08"); }
                                                line.clear();
                                                cursor_pos = 0;
                                            }
                                        }
                                    }
                                }
                                b'C' => {
                                    if cursor_pos < line.len() {
                                        cursor_pos += 1;
                                        print("\x1b[C");
                                    }
                                }
                                b'D' => {
                                    if cursor_pos > 0 {
                                        cursor_pos -= 1;
                                        print("\x1b[D");
                                    }
                                }
                                b'H' => {
                                    while cursor_pos > 0 {
                                        print("\x08");
                                        cursor_pos -= 1;
                                    }
                                }
                                b'F' => {
                                    let remaining: String = line[cursor_pos..].chars().collect();
                                    print(&remaining);
                                    cursor_pos = line.len();
                                }
                                _ => {}
                            }
                        }
                    }
                }
                c if c >= 0x20 => {
                    history_index = None;
                    if cursor_pos < line.len() {
                        let ch = c as char;
                        line.insert(cursor_pos, ch);
                        cursor_pos += 1;
                        let remaining: String = line[cursor_pos..].chars().collect();
                        print(&remaining);
                        print(" \x08");
                        for _ in remaining.chars() { print("\x08"); }
                    } else {
                        line.push(c as char);
                        cursor_pos += 1;
                        let _ = sc::sys_write(1, &[c]);
                    }
                }
                _ => {}
            },
            _ => return None,
        }
    }
}

fn complete_word(line: &str, _cursor_pos: usize, state: &ShellState) -> String {
    let words: Vec<&str> = line.split_whitespace().collect();
    if words.is_empty() { return line.to_string(); }
    let last_word = words.last().unwrap();
    if last_word.is_empty() { return line.to_string(); }
    let prefix = *last_word;
    let paths = state.env.get("PATH").unwrap_or(String::from("/bin:/usr/bin"));
    let mut candidates: Vec<String> = Vec::new();
    for dir in paths.split(':') {
        let mut buf = [0u8; 8192];
        if let Ok(fd) = sc::sys_open(if dir.is_empty() { "." } else { dir }, 0) {
            if let Ok(n) = sc::sys_getdents64(fd, &mut buf) {
                let mut offset = 0;
                while offset < n {
                    let rec_len = u16::from_le_bytes([buf[offset + 8], buf[offset + 9]]) as usize;
                    let name_len = u16::from_le_bytes([buf[offset + 10], buf[offset + 11]]) as usize;
                    if name_len > 0 {
                        let name_bytes = &buf[offset + 18..offset + 18 + name_len];
                        if let Ok(name) = core::str::from_utf8(name_bytes) {
                            let name = name.trim_end_matches('\0');
                            if name.starts_with(prefix) && name != "." && name != ".." {
                                candidates.push(name.to_string());
                            }
                        }
                    }
                    if rec_len == 0 { break; }
                    offset += rec_len;
                }
            }
            let _ = sc::sys_close(fd);
        }
    }
    if let Some(s) = state.env.get("HOME") {
        let home_dir = s;
        let mut buf = [0u8; 8192];
        if let Ok(fd) = sc::sys_open(&home_dir, 0) {
            if let Ok(n) = sc::sys_getdents64(fd, &mut buf) {
                let mut offset = 0;
                while offset < n {
                    let rec_len = u16::from_le_bytes([buf[offset + 8], buf[offset + 9]]) as usize;
                    let name_len = u16::from_le_bytes([buf[offset + 10], buf[offset + 11]]) as usize;
                    if name_len > 0 {
                        let name_bytes = &buf[offset + 18..offset + 18 + name_len];
                        if let Ok(name) = core::str::from_utf8(name_bytes) {
                            let name = name.trim_end_matches('\0');
                            if name.starts_with(prefix) && name != "." && name != ".." {
                                candidates.push(name.to_string());
                            }
                        }
                    }
                    if rec_len == 0 { break; }
                    offset += rec_len;
                }
            }
            let _ = sc::sys_close(fd);
        }
    }
    if candidates.len() == 1 {
        let completed = &candidates[0];
        let suffix = if completed.contains('/') { " " } else { "/" };
        let mut result = String::new();
        for w in words.iter().take(words.len() - 1) {
            result.push_str(w);
            result.push(' ');
        }
        result.push_str(completed);
        result.push_str(suffix);
        result
    } else if candidates.len() > 1 {
        crate::println("");
        for c in &candidates { crate::print(&format!("{} ", c)); }
        crate::println("");
        line.to_string()
    } else {
        line.to_string()
    }
}

fn expand_ps1(ps1: &str, state: &ShellState) -> String {
    let mut result = String::new();
    let chars: Vec<char> = ps1.chars().collect();
    let len = chars.len();
    let mut i = 0;

    let epoch = sc::sys_time().max(0) as i64;
    let sec = epoch % 60;
    let min = (epoch / 60) % 60;
    let hour24 = (epoch / 3600) % 24;
    let hour12 = {
        let h = hour24 % 12;
        if h == 0 { 12 } else { h }
    };
    let ampm = if hour24 < 12 { "AM" } else { "PM" };

    while i < len {
        if chars[i] == '\\' && i + 1 < len {
            match chars[i + 1] {
                'u' => {
                    result.push_str(&state.env.get("USER").unwrap_or(String::from("root")));
                    i += 2;
                }
                'h' => {
                    let mut buf = [0u8; 256];
                    match sc::sys_eon_get_hostname(&mut buf) {
                        Ok(n) => {
                            if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                                result.push_str(s);
                            } else {
                                result.push_str("echos");
                            }
                        }
                        Err(_) => { result.push_str("echos"); }
                    }
                    i += 2;
                }
                'H' => {
                    let mut buf = [0u8; 256];
                    match sc::sys_eon_get_hostname(&mut buf) {
                        Ok(n) => {
                            if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                                result.push_str(s);
                            } else {
                                result.push_str("echos");
                            }
                        }
                        Err(_) => { result.push_str("echos"); }
                    }
                    result.push(':');
                    result.push_str(&state.env.get("PWD").unwrap_or(String::from("/")));
                    i += 2;
                }
                'w' => {
                    let pwd = state.env.get("PWD").unwrap_or(String::from("/"));
                    let home = state.env.get("HOME").unwrap_or(String::new());
                    if !home.is_empty() && pwd.starts_with(&home) {
                        result.push('~');
                        result.push_str(&pwd[home.len()..]);
                    } else {
                        result.push_str(&pwd);
                    }
                    i += 2;
                }
                'W' => {
                    let pwd = state.env.get("PWD").unwrap_or(String::from("/"));
                    if let Some(pos) = pwd.rfind('/') {
                        let base = if pos == 0 { "/" } else { &pwd[pos + 1..] };
                        result.push_str(if base.is_empty() { "/" } else { base });
                    } else {
                        result.push_str(&pwd);
                    }
                    i += 2;
                }
                't' => {
                    result.push_str(&format!("{:02}:{:02}:{:02}", hour24, min, sec));
                    i += 2;
                }
                'T' => {
                    result.push_str(&format!("{:02}:{:02}", hour24, min));
                    i += 2;
                }
                '@' => {
                    result.push_str(&format!("{:02}:{:02} {}", hour12, min, ampm));
                    i += 2;
                }
                '!' => {
                    result.push_str(&(state.history.list().len() + 1).to_string());
                    i += 2;
                }
                '#' => {
                    result.push_str(&(state.history.list().len() + 1).to_string());
                    i += 2;
                }
                'j' => {
                    let job_count = state.jobs.iter().filter(|j| j.running).count();
                    result.push_str(&job_count.to_string());
                    i += 2;
                }
                'n' => {
                    result.push('\n');
                    i += 2;
                }
                's' => {
                    result.push_str(&state.env.get("_shell_name").unwrap_or(String::from("echshell")));
                    i += 2;
                }
                '$' => {
                    if sc::sys_getuid() == 0 { result.push('#'); } else { result.push('$'); }
                    i += 2;
                }
                '\\' => {
                    result.push('\\');
                    i += 2;
                }
                _ => {
                    result.push(chars[i + 1]);
                    i += 2;
                }
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    // NOTE: env.expand CAGIRMA — PS1 içindeki $ isareti degisken degil,
    // prompt escape olarak kullanilir. env.expand("$ ") → "$" yi siler.
    result
}

// ============================================================================
// ENTRY POINT
// ============================================================================

#[no_mangle]
pub extern "C" fn _start() -> ! {
    let heap_base = unsafe { sc::raw_syscall(12, 0, 0, 0, 0, 0, 0) as usize };
    dbg_print(b"[SHELL] heap_base", heap_base);
    ALLOCATOR.heap_end.store(heap_base, Ordering::Relaxed);
    ALLOCATOR.next.store(heap_base, Ordering::Relaxed);
    let mut state = ShellState::new();
    let _hostname = {
        let mut buf = [0u8; 256];
        match sc::sys_eon_get_hostname(&mut buf) {
            Ok(n) => core::str::from_utf8(&buf[..n]).unwrap_or("echos").to_string(),
            Err(_) => String::from("echos"),
        }
    };
    println("echOS Ring 3 Shell v1.0");
    loop {
        let prompt = {
            let ps1 = state.env.get("PS1").unwrap_or(String::from("$ "));
            expand_ps1(&ps1, &state)
        };
        let Some(line) = read_line(&prompt, Some(&state)) else {
            scripting::run_trap_action(&mut state, "EXIT");
            sc::sys_exit(0);
        };
        let line = line.trim();
        if line == "__ECHOS_SIGINT__" {
            scripting::run_trap_action(&mut state, "INT");
            continue;
        }
        if line == "__ECHOS_SIGQUIT__" {
            scripting::run_trap_action(&mut state, "QUIT");
            continue;
        }
        if line == "__ECHOS_SIGTSTP__" {
            scripting::run_trap_action(&mut state, "TSTP");
            continue;
        }
        if line.is_empty() { continue; }
        state.history.push(line.to_string());
        executor::execute_line(&mut state, line);
        if state.exit_code != 0 {
            scripting::run_trap_action(&mut state, "ERR");
        }
    }
}
