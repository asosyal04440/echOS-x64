# echOS Kernel API Reference

Bu belge, echOS çekirdeğinin dış-yüzey (public) API'sini özetler.
Detaylı API için `cargo doc --document-private-items` çalıştırın.

---

## 1. Sürücü API (Frozen)

### 1.1 AsyncBlockDevice
```rust
pub trait AsyncBlockDevice: Send + Sync {
    fn read_block(&self, lba: u64, buf: &mut [u8]) -> Result<(), BlockDeviceError>;
    fn write_block(&self, lba: u64, buf: &[u8]) -> Result<(), BlockDeviceError>;
    fn flush(&self) -> Result<(), BlockDeviceError>;
    fn capacity_blocks(&self) -> u64;
    fn block_size(&self) -> usize;
}
```

### 1.2 AsyncNetDevice
```rust
pub trait AsyncNetDevice: Send + Sync {
    fn send_packet(&self, buf: &[u8]) -> Result<usize, NetError>;
    fn recv_packet(&self, buf: &mut [u8]) -> Result<usize, NetError>;
    fn mac_address(&self) -> [u8; 6];
    fn link_up(&self) -> bool;
}
```

### 1.3 BlockDevice
```rust
pub trait BlockDevice {
    fn read(&self, sector: u64, buf: &mut [u8]) -> Result<(), BlockDeviceError>;
    fn write(&self, sector: u64, buf: &[u8]) -> Result<(), BlockDeviceError>;
    fn sector_size(&self) -> usize;
    fn total_sectors(&self) -> u64;
    fn device_type(&self) -> BlockDeviceType;
}
```

---

## 2. Dosya Sistemi API (Frozen)

### 2.1 VFS Trait
```rust
pub trait FileSystem {
    fn read_file(&self, path: &str, buf: &mut [u8]) -> Result<usize, FsError>;
    fn write_file(&self, path: &str, data: &[u8]) -> Result<usize, FsError>;
    fn create_file(&self, path: &str) -> Result<(), FsError>;
    fn delete_file(&self, path: &str) -> Result<(), FsError>;
    fn list_dir(&self, path: &str) -> Result<Vec<DirEntry>, FsError>;
    fn stat(&self, path: &str) -> Result<FileInfo, FsError>;
    fn mkdir(&self, path: &str) -> Result<(), FsError>;
    fn rmdir(&self, path: &str) -> Result<(), FsError>;
}
```

### 2.2 Desteklenen Dosya Sistemleri
| FS | Durum | Okuma | Yazma | Journal |
|----|-------|-------|-------|---------|
| ext4 | ✅ Tam | ✅ | ✅ | ✅ |
| FAT32 | ✅ Tam | ✅ | ✅ | N/A |
| tmpfs | ✅ Tam | ✅ | ✅ | N/A |
| overlayfs | ✅ Tam | ✅ | ✅ | N/A |
| procfs | ✅ Tam | ✅ | ❌ | N/A |
| sysfs | ✅ Tam | ✅ | ❌ | N/A |
| devtmpfs | ✅ Tam | ✅ | ❌ | N/A |
| XFS | 🔶 Temel | ✅ | 🔶 | ❌ |
| Btrfs | 🔶 Temel | ✅ | 🔶 | ❌ |

---

## 3. Syscall Arayüzü (Stable)

### 3.1 POSIX Core (nr 0-63) — Frozen
```
read(0), write(1), open(2), close(3), stat(4), fstat(5),
mmap(9), mprotect(10), munmap(11), brk(12),
ioctl(16), pipe(22), select(23),
getpid(39), fork(57), execve(59), exit(60), kill(62)
```

### 3.2 POSIX Extended (nr 64-150) — Frozen
```
socket(41), connect(42), accept(43), sendto(44), recvfrom(45),
bind(49), listen(50), getsockopt(55),
clone(56), getcwd(79), chdir(80), mkdir(83), rmdir(84),
readlink(89), chmod(90), chown(92)
```

### 3.3 Linux Compat (nr 151-299) — Stable
```
io_uring_setup(425), io_uring_enter(426), io_uring_register(427),
openat(257), statx(332), readv(19), writev(20)
```

---

## 4. Bellek Yönetimi API

### 4.1 Allocator
- **TLSF**: O(1) malloc/free — çekirdek varsayılanı
- **Fibonacci Buddy**: 2^n boyutlu bloklar — fiziksel sayfa ayırıcı
- **Slab**: Sabit boyutlu nesne cache'i

### 4.2 HHDM (Higher-Half Direct Map)
```rust
/// Fiziksel adresi sanal adrese çevirir
pub fn phys_to_virt(phys: u64) -> u64;

/// Sanal adresi fiziksel adrese çevirir
pub fn virt_to_phys(virt: u64) -> u64;
```

---

## 5. Güvenlik API

### 5.1 KASLR
```rust
pub fn kaslr::init() -> KaslrInfo;
pub fn kaslr::get_slide() -> u64;
pub fn kaslr::get_kernel_base() -> u64;
```

### 5.2 Manifest Signing
```rust
pub fn verify_manifest(data: &[u8], sig: &ManifestSignature) -> ManifestVerifyResult;
pub fn sign_manifest(data: &[u8], signer: &[u8; 32], algo: SignatureAlgorithm) -> ManifestSignature;
```

### 5.3 Capabilities (POSIX)
```rust
pub fn check_capability(pid: u32, cap: Capability) -> bool;
pub fn grant_capability(pid: u32, cap: Capability);
pub fn drop_capability(pid: u32, cap: Capability);
```

---

## 6. Scheduler API

### 6.1 Task Management
```rust
pub fn spawn(entry: fn(), name: &str, priority: u8) -> TaskId;
pub fn current_task_id() -> TaskId;
pub fn yield_now();
pub fn sleep(ms: u64);
```

### 6.2 Scheduling Classes
| Sınıf | Politika | Kullanım |
|-------|----------|----------|
| CFS | Completely Fair | Genel amaçlı görevler |
| RT | Real-Time (FIFO/RR) | Düşük gecikme |
| Deadline | EDF | Zaman kısıtlı görevler |
| Idle | Background | Boşta çalışan görevler |

---

## 7. Shell Komutları (75+)

Tam komut listesi için `help` komutunu çalıştırın.

### Kategori Özeti
| Kategori | Komut Sayısı | Örnekler |
|----------|-------------|----------|
| Core | ~12 | `help`, `echo`, `clear`, `ls`, `cat` |
| Process | ~6 | `ps`, `kill`, `top`, `bg`, `fg` |
| Filesystem | ~15 | `mount`, `chmod`, `mkdir`, `rm`, `ln` |
| System | ~12 | `uname`, `free`, `df`, `uptime`, `lsmod` |
| Network | ~10 | `ping`, `ifconfig`, `http`, `dns`, `curl` |
| Driver/Debug | ~15 | `tier-dashboard`, `hotplug`, `perf-audit`, `strace` |
| Security | ~5 | `kaslr`, `jail-fence`, `cgroup` |
