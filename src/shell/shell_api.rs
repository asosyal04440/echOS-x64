//! # Shell API Katmanı
//!
//! Bu modül, shell'in kernel fonksiyonlarına erişimini soyutlar.
//! Tüm çağrılar `shell_syscall::*` üzerinden Ring 3 syscall'ları ile yapılır.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ============================================================================
// POSIX SABİTLERİ — Dosya bayrakları
// ============================================================================

pub const O_RDONLY: u32 = 0;
pub const O_WRONLY: u32 = 1;
pub const O_RDWR: u32 = 2;
pub const O_CREAT: u32 = 0x200;
pub const O_TRUNC: u32 = 0x240;
pub const O_APPEND: u32 = 0x400;
pub const O_EXCL: u32 = 0x800;

// ============================================================================
// DOSYA SİSTEMİ
// ============================================================================

/// Dosya aç — sys_open veya crate::fs::sys_open
/// Dönüş: fd (başarı) veya negatif errno (hata)
pub fn fs_open(path: &str, flags: u32) -> usize {
    {
        match super::shell_syscall::sys_open(path, flags) {
            Ok(fd) => fd as usize,
            Err(e) => -(e as isize) as usize,
        }
    }
}

/// Dosya kapat
pub fn fs_close(fd: usize) -> bool {
    {
        super::shell_syscall::sys_close(fd).is_ok()
    }
}

/// Dosyadan oku — Dönüş: Result<usize, ...> (mevcut API ile uyumlu)
pub fn fs_read(fd: usize, buf: &mut [u8]) -> Result<usize, crate::fs::FsError> {
    {
        match super::shell_syscall::sys_read(fd, buf) {
            Ok(n) => Ok(n),
            Err(e) => Err(crate::fs::FsError::IoError),
        }
    }
}

/// Dosyaya yaz — Dönüş: Result<usize, ...> (mevcut API ile uyumlu)
pub fn fs_write(fd: usize, buf: &[u8]) -> Result<usize, crate::fs::FsError> {
    {
        match super::shell_syscall::sys_write(fd, buf) {
            Ok(n) => Ok(n),
            Err(e) => Err(crate::fs::FsError::IoError),
        }
    }
}

/// Dosya konumunu değiştir
pub fn fs_seek(fd: usize, offset: usize) -> bool {
    {
        super::shell_syscall::sys_lseek(fd, offset as isize, 0).is_ok()
    }
}

/// Dizin oluştur
pub fn fs_mkdir(path: &str, mode: u32) -> Result<(), i32> {
    {
        super::shell_syscall::sys_mkdir(path, mode)
    }
}

/// Dosya sil
pub fn fs_unlink(path: &str) -> Result<(), i32> {
    {
        super::shell_syscall::sys_unlink(path)
    }
}

/// Dosya yeniden adlandır
pub fn fs_rename(old: &str, new: &str) -> Result<(), i32> {
    {
        super::shell_syscall::sys_rename(old, new)
    }
}

/// Dosya izinlerini değiştir (chmod)
pub fn fs_chmod(path: &str, mode: u32) -> Result<(), i32> {
    {
        super::shell_syscall::sys_chmod(path, mode)
    }
}

/// Dosya sahipliğini değiştir (chown)
pub fn fs_chown(path: &str, uid: u32, gid: u32) -> Result<(), i32> {
    {
        super::shell_syscall::sys_chown(path, uid, gid)
    }
}

/// Sabit bağlantı oluştur (link)
pub fn fs_link(target: &str, link_path: &str) -> Result<(), i32> {
    {
        super::shell_syscall::sys_link(target, link_path)
    }
}

/// Sembolik bağlantı oluştur (symlink)
pub fn fs_symlink(target: &str, link_path: &str) -> Result<(), i32> {
    {
        super::shell_syscall::sys_symlink(target, link_path)
    }
}

/// Dosya boyutunu değiştir (truncate)
pub fn fs_truncate(path: &str, size: u64) -> Result<(), i32> {
    {
        super::shell_syscall::sys_truncate(path, size)
    }
}

/// Dizin içeriğini listele
pub fn fs_list_dir(path: &str) -> Result<Vec<String>, i32> {
    {
        // Ring 3'te GETDENTS64 syscall kullanılır
        let mut buf = alloc::vec![0u8; 4096];
        let fd = super::shell_syscall::sys_open(path, 0)?; // O_RDONLY
        let mut entries = Vec::new();
        loop {
            match super::shell_syscall::sys_getdents64(fd, &mut buf) {
                Ok(n) if n > 0 => {
                    // Linux dirent64 formatını parse et
                    let mut offset = 0;
                    while offset < n {
                        let _inode = u64::from_le_bytes([
                            buf[offset],
                            buf[offset + 1],
                            buf[offset + 2],
                            buf[offset + 3],
                            buf[offset + 4],
                            buf[offset + 5],
                            buf[offset + 6],
                            buf[offset + 7],
                        ]);
                        let rec_len =
                            u16::from_le_bytes([buf[offset + 8], buf[offset + 9]]) as usize;
                        let name_len =
                            u16::from_le_bytes([buf[offset + 10], buf[offset + 11]]) as usize;
                        let name_bytes = &buf[offset + 18..offset + 18 + name_len];
                        if let Ok(name) = core::str::from_utf8(name_bytes) {
                            let name = name.trim_end_matches('\0');
                            if !name.is_empty() && name != "." && name != ".." {
                                entries.push(name.to_string());
                            }
                        }
                        offset += rec_len;
                    }
                }
                _ => break,
            }
        }
        let _ = super::shell_syscall::sys_close(fd);
        Ok(entries)
    }
}

/// Symlink hedefini oku
pub fn fs_readlink(path: &str) -> Result<String, i32> {
    {
        let mut buf = alloc::vec![0u8; 4096];
        match super::shell_syscall::sys_readlink(path, &mut buf) {
            Ok(n) => {
                if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                    Ok(s.to_string())
                } else {
                    Err(-84) // EILSEQ
                }
            }
            Err(e) => Err(e),
        }
    }
}

// ============================================================================
// SÜREÇ YÖNETİMİ
// ============================================================================

/// Task bilgi yapısı
pub struct TaskInfo {
    pub pid: usize,
    pub state: TaskState,
    pub name: String,
    pub priority: TaskPriority,
}

/// Task durumu
#[derive(Clone, Copy, PartialEq)]
pub enum TaskState {
    Ready,
    Running,
    Blocked,
    Sleeping { wake_tick: usize },
    Terminated,
    Stopped,
    Zombie,
}

/// Task önceliği
#[derive(Clone, Copy, PartialEq)]
pub enum TaskPriority {
    Idle,
    Low,
    Normal,
    High,
}

/// Fork — child process oluştur
pub fn proc_fork() -> Result<usize, i32> {
    {
        super::shell_syscall::sys_fork()
    }
}

/// Exec — program çalıştır
pub fn proc_exec(path: &str, argv: &[&str], envp: &[&str]) -> Result<(), i32> {
    {
        super::shell_syscall::sys_execve(path, argv, envp)
    }
}

/// Exit — process sonlandır
pub fn proc_exit(code: i32) -> ! {
    {
        super::shell_syscall::sys_exit(code)
    }
}

/// Wait — child process bekle
pub fn proc_wait(pid: isize, status: &mut i32, options: u32) -> Result<usize, i32> {
    {
        super::shell_syscall::sys_wait4(pid, status, options)
    }
}

/// Kill — sinyal gönder
pub fn proc_kill(pid: usize, sig: i32) -> Result<(), i32> {
    {
        super::shell_syscall::sys_kill(pid, sig)
    }
}

/// Mevcut PID
pub fn proc_getpid() -> usize {
    {
        super::shell_syscall::sys_getpid()
    }
}

/// Sinyal gönder (kill benzeri)
pub fn proc_kill_signal(pid: usize, signal: usize) {
    {
        let _ = super::shell_syscall::sys_kill(pid, signal as i32);
    }
}

/// Terminate edilmiş child'ı bekle
pub fn proc_wait_for_terminated(pid: isize) -> Option<(usize, i32)> {
    {
        // Ring 3'te wait4 syscall kullanılır
        let mut status: i32 = 0;
        match super::shell_syscall::sys_wait4(pid, &mut status, 1) {
            // WNOHANG=1
            Ok(tid) => Some((tid, status)),
            Err(_) => None,
        }
    }
}

/// User image task oluştur
pub fn proc_spawn_user_image(
    data: &[u8],
    priority: usize,
    name: &'static str,
) -> Result<usize, i32> {
    let priority = match priority {
        0 => crate::task::Priority::Idle,
        1 => crate::task::Priority::Low,
        2 => crate::task::Priority::Normal,
        3 => crate::task::Priority::High,
        _ => crate::task::Priority::Normal,
    };

    crate::task::scheduler::spawn_user_image_task(data, priority, name).map_err(|_| 5)
}

/// Uyku (saniye cinsinden)
pub fn proc_sleep(secs: u64) {
    {
        let _ = super::shell_syscall::sys_nanosleep(secs, 0);
    }
}

/// Çalışan task listesini al
pub fn proc_list_tasks() -> Vec<TaskInfo> {
    {
        let mut buf = alloc::vec![0u8; 8192];
        match super::shell_syscall::sys_eon_list_tasks(&mut buf) {
            Ok(n) => {
                if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                    // JSON parse - her satır bir task
                    s.lines()
                        .filter_map(|line| {
                            // Basit JSON parse: {"pid":123,"state":"Ready","name":"shell","prio":2}
                            let pid = extract_json_u64(line, "pid").unwrap_or(0) as usize;
                            let state_str = extract_json_string(line, "state").unwrap_or_default();
                            let name = extract_json_string(line, "name").unwrap_or_default();
                            let prio = extract_json_u64(line, "prio").unwrap_or(2) as usize;

                            let state = match state_str.as_str() {
                                "Ready" => TaskState::Ready,
                                "Running" => TaskState::Running,
                                "Blocked" => TaskState::Blocked,
                                "Sleeping" => TaskState::Sleeping { wake_tick: 0 },
                                "Terminated" => TaskState::Terminated,
                                "Stopped" => TaskState::Stopped,
                                "Zombie" => TaskState::Zombie,
                                _ => TaskState::Ready,
                            };

                            let priority = match prio {
                                0 => TaskPriority::Idle,
                                1 => TaskPriority::Low,
                                2 => TaskPriority::Normal,
                                3 => TaskPriority::High,
                                _ => TaskPriority::Normal,
                            };

                            Some(TaskInfo {
                                pid,
                                state,
                                name,
                                priority,
                            })
                        })
                        .collect()
                } else {
                    Vec::new()
                }
            }
            Err(_) => Vec::new(),
        }
    }
}

/// JSON'dan string değeri çıkar
fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let pattern = alloc::format!("\"{}\":\"", key);
    if let Some(pos) = json.find(&pattern) {
        let start = pos + pattern.len();
        let end = json[start..]
            .find('"')
            .map(|i| start + i)
            .unwrap_or(json.len());
        Some(json[start..end].to_string())
    } else {
        None
    }
}

// ============================================================================
// TERMİNAL
// ============================================================================

/// Klavyeden tuş oku (non-blocking)
/// Dönüş: ASCII karakter veya özel tuş (0x80+ offset ile kodlanmış)
pub fn term_read_key() -> Option<u16> {
    {
        match super::shell_syscall::sys_eon_keyboard_read() {
            Ok(k) => Some(k as u16),
            Err(_) => None,
        }
    }
}

/// Ekranı temizle
pub fn term_clear() {
    {
        let _ = super::shell_syscall::sys_eon_term_clear();
    }
}

/// Ekrana yaz
pub fn term_print(s: &str) {
    {
        // stdout'a yaz (fd 1)
        let _ = super::shell_syscall::sys_write(1, s.as_bytes());
    }
}

/// Serial port'a yaz
pub fn serial_print(s: &str) {
    {
        // stderr'a yaz (fd 2) — serial port için
        let _ = super::shell_syscall::sys_write(2, s.as_bytes());
    }
}

/// Serial port'a satır yaz
pub fn serial_println(s: &str) {
    serial_print(s);
    serial_print("\n");
}

// ============================================================================
// SİSTEM BİLGİSİ
// ============================================================================

/// Sistem zamanı (saniye)
pub fn sys_uptime() -> u64 {
    {
        let mut tp = [0usize; 2];
        let _ = super::shell_syscall::sys_clock_gettime(1, &mut tp); // CLOCK_MONOTONIC
        tp[0] as u64
    }
}

/// Bellek istatistikleri
pub fn sys_memory_stats() -> (u64, u64, u64) {
    {
        let mut buf = alloc::vec![0u8; 256];
        match super::shell_syscall::sys_eon_memory_stats(&mut buf) {
            Ok(n) => {
                if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                    // Basit JSON parse
                    let total = extract_json_u64(s, "total_kb").unwrap_or(0);
                    let free = extract_json_u64(s, "free_kb").unwrap_or(0);
                    let avail = extract_json_u64(s, "available_kb").unwrap_or(0);
                    (total, free, avail)
                } else {
                    (0, 0, 0)
                }
            }
            Err(_) => (0, 0, 0),
        }
    }
}

/// JSON'dan u64 değeri çıkar (basit)
fn extract_json_u64(json: &str, key: &str) -> Option<u64> {
    let pattern = alloc::format!("\"{}\":", key);
    if let Some(pos) = json.find(&pattern) {
        let start = pos + pattern.len();
        let end = json[start..]
            .find(|c: char| !c.is_ascii_digit())
            .map(|i| start + i)
            .unwrap_or(json.len());
        json[start..end].parse().ok()
    } else {
        None
    }
}

// ============================================================================
// SİNYAL
// ============================================================================

/// Foreground process group ID'yi al
pub fn term_get_foreground() -> usize {
    {
        super::shell_syscall::sys_eon_get_foreground().unwrap_or(0)
    }
}

/// Foreground process group ID'yi ayarla
pub fn term_set_foreground(pgid: usize) {
    {
        let _ = super::shell_syscall::sys_eon_set_foreground(pgid);
    }
}

/// Mevcut process'i arka plana al
pub fn proc_background() -> Option<usize> {
    {
        // Ring 3'te process group değiştirme
        None // Şimdilik desteklenmiyor
    }
}

// ============================================================================
// AĞ (NETWORK) — shell_api wrapper'ları
// ============================================================================

/// Ağ yapılandırmasını al
pub fn net_get_config() -> crate::net::NetworkConfig {
    {
        crate::net::NetworkConfig::new()
    }
}

/// Yerel IP adresini al
pub fn net_local_ip() -> [u8; 4] {
    net_get_config().ip_addr
}

/// Ağ arayüzlerini listele
pub fn net_get_interfaces() -> alloc::vec::Vec<crate::net::InterfaceInfo> {
    {
        alloc::vec::Vec::new()
    }
}

/// TCP bağlantılarını listele
pub fn net_list_tcp() -> alloc::vec::Vec<crate::net::tcp::TcpConnInfo> {
    {
        alloc::vec::Vec::new()
    }
}

/// UDP soketlerini listele
pub fn net_list_udp() -> alloc::vec::Vec<crate::net::udp::UdpSocketInfo> {
    {
        alloc::vec::Vec::new()
    }
}

/// DHCP lease bilgisini al
pub fn net_get_dhcp_lease() -> Option<crate::net::dhcp::DhcpLease> {
    {
        None
    }
}

/// ICMP ping gönder — gercek ping Real yoluyla
pub fn net_ping(dest_ip: [u8; 4], count: u8) -> Result<alloc::vec::Vec<(u32, bool)>, i32> {
    {
        let _ = (dest_ip, count);
        Err(-38) // ENOSYS
    }
}

/// DoH (DNS over HTTPS) sorgusu — tek A record döndürür
pub fn net_doh_lookup(host: &str, provider: &str) -> Result<[u8; 4], i32> {
    {
        let _ = (host, provider);
        Err(-38)
    }
}

/// DoT (DNS over TLS) sorgusu — tek A record döndürür
pub fn net_dot_lookup(host: &str, provider: &str) -> Result<[u8; 4], i32> {
    {
        let _ = (host, provider);
        Err(-38)
    }
}

/// HTTP GET isteği — (status_code, body) döndürür
pub fn net_http_get(url: &str) -> Result<(u16, alloc::vec::Vec<u8>), i32> {
    {
        let _ = url;
        Err(-38)
    }
}

/// DHCP yapılandırması tetikle — true = başarılı
pub fn net_dhcp_configure() -> bool {
    {
        false
    }
}

/// Ağ gateway adresini al
pub fn net_get_gateway() -> Option<[u8; 4]> {
    {
        None
    }
}

/// Ağ DNS sunucusunu al
pub fn net_get_dns() -> Option<[u8; 4]> {
    {
        None
    }
}

/// Hostname'i çöz — IPv4 literal veya DNS lookup
pub fn net_resolve_ip(host: &str) -> Option<[u8; 4]> {
    {
        let _ = host;
        None
    }
}

/// TCP bağlantısı kur (netcat benzeri)
pub fn net_nc_connect(host: &str, port: u16) -> Result<usize, i32> {
    {
        let _ = (host, port);
        Err(-38) // ENOSYS
    }
}

/// TCP bağlantısını kapat
pub fn net_nc_close(sock: usize) -> Result<(), i32> {
    {
        let _ = sock;
        Err(-38)
    }
}

/// HTTP/3 GET isteği — (status, headers, body) döndürür
pub fn net_http3_get(
    url: &str,
) -> Result<
    (
        u16,
        alloc::vec::Vec<(alloc::string::String, alloc::string::String)>,
        alloc::vec::Vec<u8>,
    ),
    i32,
> {
    {
        let _ = url;
        Err(-38)
    }
}

// ============================================================================
// GÜVENLİK (SECURITY) — shell_api wrapper'ları
// ============================================================================

/// Mevcut kullanıcı ID'sini al
pub fn sec_current_uid() -> u32 {
    {
        super::shell_syscall::sys_getuid()
    }
}

/// Kullanıcı bilgisini al — doğrudan kernel tipini döndürür
pub fn sec_get_user(uid: u32) -> Option<crate::security::users::UserEntry> {
    {
        let _ = uid;
        None
    }
}

/// Kullanıcının grup GID'lerini al
pub fn sec_get_user_groups(username: &str) -> Vec<u32> {
    {
        let _ = username;
        Vec::new()
    }
}

/// Tüm kullanıcıları listele — doğrudan kernel tipini döndürür
pub fn sec_list_users() -> Vec<crate::security::users::UserEntry> {
    {
        Vec::new()
    }
}

/// Tüm oturumları listele — doğrudan kernel tipini döndürür
pub fn sec_list_sessions() -> Vec<crate::security::users::Session> {
    {
        Vec::new()
    }
}

/// KASLR bilgisini al — doğrudan kernel tipini döndürür
pub fn sec_kaslr_info() -> crate::security::kaslr::KaslrInfo {
    {
        crate::security::kaslr::KaslrInfo {
            enabled: false,
            slide: 0,
            kernel_base: 0,
            slot_index: 0,
            entropy_source: crate::security::kaslr::EntropySource::None,
        }
    }
}

/// Seccomp audit bilgisini al
pub fn sec_seccomp_audit() -> Vec<(usize, usize)> {
    {
        Vec::new()
    }
}

// ============================================================================
// IPC — Paket Yönetimi (Package Registry)
// ============================================================================

/// Paket kur — Ring 3'te string tabanlı JSON yanıt döndürür
pub fn ipc_pkg_install(path: &str) -> Result<String, i32> {
    {
        let mut buf = alloc::vec![0u8; 4096];
        match super::shell_syscall::sys_eon_ipc_pkg_install(path, &mut buf) {
            Ok(n) => core::str::from_utf8(&buf[..n])
                .map(|s| s.to_string())
                .map_err(|_| -84),
            Err(e) => Err(e),
        }
    }
}

/// Paket kaldır
pub fn ipc_pkg_remove(name: &str) -> Result<String, i32> {
    {
        let mut buf = alloc::vec![0u8; 4096];
        match super::shell_syscall::sys_eon_ipc_pkg_remove(name, &mut buf) {
            Ok(n) => core::str::from_utf8(&buf[..n])
                .map(|s| s.to_string())
                .map_err(|_| -84),
            Err(e) => Err(e),
        }
    }
}

/// Paket listesi al — JSON array döndürür
pub fn ipc_pkg_list() -> Result<String, i32> {
    {
        let mut buf = alloc::vec![0u8; 16384];
        match super::shell_syscall::sys_eon_ipc_pkg_list(&mut buf) {
            Ok(n) => core::str::from_utf8(&buf[..n])
                .map(|s| s.to_string())
                .map_err(|_| -84),
            Err(e) => Err(e),
        }
    }
}

/// Paket bilgisi al — JSON döndürür
pub fn ipc_pkg_info(name: &str) -> Result<String, i32> {
    {
        let mut buf = alloc::vec![0u8; 4096];
        match super::shell_syscall::sys_eon_ipc_pkg_info(name, &mut buf) {
            Ok(n) => core::str::from_utf8(&buf[..n])
                .map(|s| s.to_string())
                .map_err(|_| -84),
            Err(e) => Err(e),
        }
    }
}

/// Paket ara — JSON array döndürür
pub fn ipc_pkg_search(term: &str) -> Result<String, i32> {
    {
        let mut buf = alloc::vec![0u8; 16384];
        match super::shell_syscall::sys_eon_ipc_pkg_search(term, &mut buf) {
            Ok(n) => core::str::from_utf8(&buf[..n])
                .map(|s| s.to_string())
                .map_err(|_| -84),
            Err(e) => Err(e),
        }
    }
}

/// Paket imzasını doğrula
pub fn ipc_pkg_verify(name: &str) -> Result<String, i32> {
    {
        let mut buf = alloc::vec![0u8; 4096];
        match super::shell_syscall::sys_eon_ipc_pkg_verify(name, &mut buf) {
            Ok(n) => core::str::from_utf8(&buf[..n])
                .map(|s| s.to_string())
                .map_err(|_| -84),
            Err(e) => Err(e),
        }
    }
}

// ============================================================================
// IPC — Güncelleme (Update Installer)
// ============================================================================

/// Update index'ini incele — JSON döndürür
pub fn ipc_update_inspect(locator: &str) -> Result<String, i32> {
    {
        let mut buf = alloc::vec![0u8; 8192];
        match super::shell_syscall::sys_eon_ipc_update_inspect(locator, &mut buf) {
            Ok(n) => core::str::from_utf8(&buf[..n])
                .map(|s| s.to_string())
                .map_err(|_| -84),
            Err(e) => Err(e),
        }
    }
}

/// Update'yi uygula — JSON döndürür
pub fn ipc_update_apply(locator: &str) -> Result<String, i32> {
    {
        let mut buf = alloc::vec![0u8; 8192];
        match super::shell_syscall::sys_eon_ipc_update_apply(locator, &mut buf) {
            Ok(n) => core::str::from_utf8(&buf[..n])
                .map(|s| s.to_string())
                .map_err(|_| -84),
            Err(e) => Err(e),
        }
    }
}

/// Update durumunu al — JSON döndürür
pub fn ipc_update_status() -> Result<String, i32> {
    {
        let mut buf = alloc::vec![0u8; 4096];
        match super::shell_syscall::sys_eon_ipc_update_status(&mut buf) {
            Ok(n) => core::str::from_utf8(&buf[..n])
                .map(|s| s.to_string())
                .map_err(|_| -84),
            Err(e) => Err(e),
        }
    }
}

// ============================================================================
// INIT / SERVİS YÖNETİMİ
// ============================================================================

/// Hostname ayarla
pub fn sys_set_hostname(name: &str) -> Result<(), i32> {
    {
        super::shell_syscall::sys_eon_set_hostname(name)
    }
}

/// Hostname al
pub fn sys_get_hostname() -> Result<String, i32> {
    {
        let mut buf = alloc::vec![0u8; 256];
        match super::shell_syscall::sys_eon_get_hostname(&mut buf) {
            Ok(n) => core::str::from_utf8(&buf[..n])
                .map(|s| s.to_string())
                .map_err(|_| -84),
            Err(e) => Err(e),
        }
    }
}

/// Servisleri listele — JSON array döndürür
pub fn sys_list_services() -> Result<String, i32> {
    {
        let mut buf = alloc::vec![0u8; 4096];
        match super::shell_syscall::sys_eon_list_services(&mut buf) {
            Ok(n) => core::str::from_utf8(&buf[..n])
                .map(|s| s.to_string())
                .map_err(|_| -84),
            Err(e) => Err(e),
        }
    }
}

/// Servis başlat
pub fn sys_start_service(name: &str) -> Result<String, i32> {
    {
        super::shell_syscall::sys_eon_start_service(name)
    }
}

/// Servis durdur
pub fn sys_stop_service(name: &str) -> Result<String, i32> {
    {
        super::shell_syscall::sys_eon_stop_service(name)
    }
}

/// Servis durumunu al
pub fn sys_service_status(name: &str) -> Result<String, i32> {
    {
        super::shell_syscall::sys_eon_service_status(name)
    }
}

/// Sistem kapatma
pub fn sys_shutdown() -> ! {
    {
        super::shell_syscall::sys_eon_shutdown()
    }
}

/// Sistem yeniden başlat
pub fn sys_reboot() -> ! {
    {
        super::shell_syscall::sys_eon_reboot()
    }
}

// ============================================================================
// SÜRÜCÜ (DRIVER) YÖNETİMİ
// ============================================================================

/// Loopback sürücülerini listele — JSON array döndürür
pub fn drv_loop_list() -> Result<String, i32> {
    {
        let mut buf = alloc::vec![0u8; 4096];
        match super::shell_syscall::sys_eon_driver_list(&mut buf) {
            Ok(n) => core::str::from_utf8(&buf[..n])
                .map(|s| s.to_string())
                .map_err(|_| -84),
            Err(e) => Err(e),
        }
    }
}

/// Loopback'e dosya ekle
pub fn drv_loop_attach(file_path: &str, device_name: &str) -> Result<(), i32> {
    {
        let _ = (file_path, device_name);
        Err(-38) // ENOSYS
    }
}

/// Loopback cihazını temizle
pub fn drv_loop_flush(device_name: &str) -> Result<(), i32> {
    {
        let _ = device_name;
        Err(-38)
    }
}

/// Loopback cihazını ayır
pub fn drv_loop_detach(device_name: &str) -> Result<(), i32> {
    {
        let _ = device_name;
        Err(-38)
    }
}

/// Loopback cihazını mount et
pub fn drv_loop_mount(
    device_name: &str,
    mount_point: &str,
    fs_type: Option<&str>,
) -> Result<(), i32> {
    {
        let _ = (device_name, mount_point, fs_type);
        Err(-38)
    }
}

/// Loopback cihazını unmount et
pub fn drv_loop_umount(mount_point: &str) -> Result<(), i32> {
    {
        let _ = mount_point;
        Err(-38)
    }
}

/// RTC tarih/saat bilgisini al
pub fn drv_rtc_datetime() -> Result<String, i32> {
    {
        let mut buf = alloc::vec![0u8; 64];
        match super::shell_syscall::sys_eon_rtc_datetime(&mut buf) {
            Ok(n) => core::str::from_utf8(&buf[..n])
                .map(|s| s.to_string())
                .map_err(|_| -84),
            Err(e) => Err(e),
        }
    }
}

/// Sürücü listesini al — JSON array döndürür
pub fn drv_list() -> Result<String, i32> {
    {
        let mut buf = alloc::vec![0u8; 8192];
        match super::shell_syscall::sys_eon_driver_list(&mut buf) {
            Ok(n) => core::str::from_utf8(&buf[..n])
                .map(|s| s.to_string())
                .map_err(|_| -84),
            Err(e) => Err(e),
        }
    }
}

/// VirtIO net durumunu kontrol et
pub fn drv_net_ready() -> bool {
    {
        false // Ring 3'te doğrudan kontrol desteklenmiyor
    }
}

// ============================================================================
// AĞ (NETWORK) — Ek Wrapper'lar
// ============================================================================

/// DNS lookup (basit)
pub fn net_dns_lookup(host: &str) -> Result<[u8; 4], i32> {
    {
        let _ = host;
        Err(-38) // ENOSYS
    }
}

/// Conntrack tablosunu al — JSON array döndürür
pub fn net_conntrack_list() -> Result<String, i32> {
    {
        Err(-38) // ENOSYS
    }
}

// ============================================================================
// DEBUG / TELEMTRY
// ============================================================================

/// strace ekle
pub fn dbg_strace_attach(pid: usize) -> Result<(), i32> {
    {
        let _ = pid;
        Err(-38)
    }
}

/// strace kaldır
pub fn dbg_strace_detach(pid: usize) -> Result<(), i32> {
    {
        let _ = pid;
        Err(-38)
    }
}

/// Trace edilen process sayısını al
pub fn dbg_strace_count() -> usize {
    {
        0
    }
}

/// PMU desteği var mı
pub fn dbg_perf_supported() -> bool {
    {
        false
    }
}

/// Perf counter sayısını al
pub fn dbg_perf_count() -> usize {
    {
        0
    }
}

/// Crash sayısını al
pub fn dbg_kdump_count() -> usize {
    {
        0
    }
}

/// Son crash bilgisini al
pub fn dbg_kdump_last() -> Result<String, i32> {
    {
        Err(-38)
    }
}
