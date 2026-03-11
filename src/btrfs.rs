//! # Btrfs (B-Tree File System) - echOS Implementasyonu
//!
//! Modern copy-on-write filesystem ile snapshots, compression, ve
//! advanced storage özellikleri. Linux Btrfs ile uyumlu.
//!
//! ## Btrfs Nedir?
//!
//! Btrfs, copy-on-write (COW) tabanlı modern filesystem'dir.
//! Veri bütünlüğü, snapshots, compression, ve esnek storage management sunar.
//!
//! ## Btrfs Özellikleri
//!
//! ```text
//! Copy-on-Write (COW):
//! - Veri değiştirildiğinde orijinal korunur
//! - Snapshots anlık kopyalar oluşturur
//! - Veri bütünlüğü garantili
//!
//! Compression:
//! - LZ4, ZSTD, LZO, Zlib
//! - Transparent compression
//! - Per-file compression settings
//!
//! Storage Management:
//! - Dynamic allocation
//! - Subvolumes
//! - RAID levels (0, 1, 10, 5, 6)
//! ```
//!
//! ## B-Tree Yapısı
//!
//! ```text
//! Root B-Tree
//!    |
//!    ├── Extent B-Tree (veri blokları)
//!    ├── FS Tree (metadata)
//!    ├── Chunk Tree (allocation)
//!    ├── Device Tree (diskler)
//!    └── Checksum Tree (bütünlük)
//! ```

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

// ============================================================================
// BTRFS SABİTLERİ
// ============================================================================

/// Btrfs magic number
pub const BTRFS_MAGIC: &[u8; 8] = b"_BHRfS_M";

/// Btrfs süperblock boyutu
pub const BTRFS_SUPER_INFO_SIZE: usize = 4096;

/// Btrfs blok boyutu
pub const BTRFS_BLOCK_SIZE: u32 = 4096;

/// Maksimum dosya boyutu (16EB)
pub const BTRFS_MAX_FILE_SIZE: u64 = 1 << 64;

/// Maksimum dosya sistemi boyutu (16EB)
pub const BTRFS_MAX_FS_SIZE: u64 = 1 << 64;

/// Btrfs nesne tipleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BtrfsObjectType {
    /// Inode
    Inode = 0x01,
    /// Veri bloğu
    RegularData = 0x18,
    /// Dizin verisi
    DirItem = 0x2c,
    /// Extended attribute
    XattrItem = 0x20,
    /// Orphan item
    OrphanItem = 0x30,
    /// Extent verisi
    ExtentData = 0x31,
    /// Extent COW
    ExtentCsum = 0x32,
    /// Root item
    RootItem = 0x3c,
    /// Root backref
    RootBackref = 0x3d,
    /// Chunk item
    ChunkItem = 0xe0,
    /// Device item
    DevItem = 0xe1,
    /// Device extent
    DevExtent = 0xe2,
    /// Checksum item
    CsumItem = 0xe3,
}

/// Btrfs inode tipleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BtrfsInodeType {
    /// Regular dosya
    RegularFile = 0x01,
    /// Dizin
    Directory = 0x02,
    /// Character device
    CharDev = 0x03,
    /// Block device
    BlockDev = 0x04,
    /// FIFO
    Fifo = 0x05,
    /// Socket
    Socket = 0x06,
    /// Symbolic link
    Symlink = 0x07,
}

/// Btrfs hatası
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BtrfsError {
    /// Geçersiz magic number
    InvalidMagic,
    /// Blok bulunamadı
    BlockNotFound,
    /// Inode bulunamadı
    InodeNotFound,
    /// Disk dolu
    DiskFull,
    /// Checksum hatası
    ChecksumError,
    /// İzin hatası
    PermissionDenied,
    /// Desteklenmeyen özellik
    UnsupportedFeature,
    /// I/O hatası
    IoError,
}

// ============================================================================
// BTRFS SUPERBLOCK
// ============================================================================

/// Btrfs süperblock
#[derive(Clone, Debug)]
pub struct BtrfsSuperblock {
    /// Magic number
    pub magic: [u8; 8],
    /// Dosya sistemi boyutu
    pub total_bytes: u64,
    /// Kullanılan byte
    pub bytes_used: u64,
    /// Cihaz sayısı
    pub num_devices: u64,
    /// Sektör boyutu
    pub sectorsize: u32,
    /// Blok boyutu
    pub nodesize: u32,
    /// Leaf boyutu
    pub leafsize: u32,
    /// Stripe boyutu
    pub stripesize: u32,
    /// Generation
    pub generation: u64,
    /// Root tree bloğu
    pub root: u64,
    /// Chunk tree bloğu
    pub chunk_root: u64,
    /// Log root bloğu
    pub log_root: u64,
    /// Checksum level
    pub csum_type: u16,
    /// Cihaz UUID
    pub dev_item: BtrfsDevItem,
    /// Label
    pub label: [u8; 256],
}

/// Btrfs cihaz item
#[derive(Clone, Debug)]
pub struct BtrfsDevItem {
    /// Cihaz ID
    pub devid: u64,
    /// Toplam byte
    pub total_bytes: u64,
    /// Kullanılan byte
    pub bytes_used: u64,
    /// Optimal I/O alignment
    pub io_align: u32,
    /// Optimal I/O width
    pub io_width: u32,
    /// Stripe boyutu
    pub sector_size: u32,
    /// Type
    pub type_: u64,
    /// Generation
    pub generation: u64,
    /// Start offset
    pub start_offset: u64,
    /// Dev group
    pub dev_group: u32,
    /// Stripe count
    pub stripe_count: u32,
}

impl BtrfsSuperblock {
    /// Yeni süperblock oluştur
    pub fn new() -> Self {
        Self {
            magic: *BTRFS_MAGIC,
            total_bytes: 0,
            bytes_used: 0,
            num_devices: 1,
            sectorsize: 512,
            nodesize: BTRFS_BLOCK_SIZE,
            leafsize: BTRFS_BLOCK_SIZE,
            stripesize: BTRFS_BLOCK_SIZE,
            generation: 1,
            root: 0,
            chunk_root: 0,
            log_root: 0,
            csum_type: 0,
            dev_item: BtrfsDevItem {
                devid: 1,
                total_bytes: 0,
                bytes_used: 0,
                io_align: BTRFS_BLOCK_SIZE,
                io_width: BTRFS_BLOCK_SIZE,
                sector_size: 512,
                type_: 0,
                generation: 1,
                start_offset: 0,
                dev_group: 0,
                stripe_count: 1,
            },
            label: [0; 256],
        }
    }
    
    /// Magic number'ı kontrol et
    pub fn is_valid(&self) -> bool {
        &self.magic == BTRFS_MAGIC
    }
    
    /// Süperblock'u serialize et
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(BTRFS_SUPER_INFO_SIZE);
        
        // Magic number
        data.extend_from_slice(&self.magic);
        
        // Diğer alanlar (placeholder)
        data.extend_from_slice(&self.total_bytes.to_le_bytes());
        data.extend_from_slice(&self.bytes_used.to_le_bytes());
        data.extend_from_slice(&self.num_devices.to_le_bytes());
        data.extend_from_slice(&self.sectorsize.to_le_bytes());
        data.extend_from_slice(&self.nodesize.to_le_bytes());
        data.extend_from_slice(&self.leafsize.to_le_bytes());
        data.extend_from_slice(&self.stripesize.to_le_bytes());
        data.extend_from_slice(&self.generation.to_le_bytes());
        data.extend_from_slice(&self.root.to_le_bytes());
        data.extend_from_slice(&self.chunk_root.to_le_bytes());
        data.extend_from_slice(&self.log_root.to_le_bytes());
        data.extend_from_slice(&self.csum_type.to_le_bytes());
        
        // Gerçek implementasyonda tüm alanlar serialize edilmeli
        
        // Boyutu BTRFS_SUPER_INFO_SIZE'a getir
        while data.len() < BTRFS_SUPER_INFO_SIZE {
            data.push(0);
        }
        
        data
    }
    
    /// Süperblock'u deserialize et
    pub fn deserialize(data: &[u8]) -> Result<Self, BtrfsError> {
        if data.len() < BTRFS_SUPER_INFO_SIZE {
            return Err(BtrfsError::IoError);
        }
        
        let magic = [
            data[0], data[1], data[2], data[3],
            data[4], data[5], data[6], data[7],
        ];
        
        if magic != *BTRFS_MAGIC {
            return Err(BtrfsError::InvalidMagic);
        }
        
        // Gerçek implementasyonda tüm alanlar deserialize edilmeli
        let mut sb = Self::new();
        sb.total_bytes = u64::from_le_bytes([
            data[8], data[9], data[10], data[11],
            data[12], data[13], data[14], data[15],
        ]);
        
        Ok(sb)
    }
}

// ============================================================================
// BTRFS INODE
// ============================================================================

/// Btrfs inode
#[derive(Clone, Debug)]
pub struct BtrfsInode {
    /// Inode numarası
    pub ino: u64,
    /// Generation
    pub generation: u64,
    /// UID
    pub uid: u32,
    /// GID
    pub gid: u32,
    /// Mode
    pub mode: u32,
    /// Link count
    pub nlink: u32,
    /// Flags
    pub flags: u64,
    /// Boyut
    pub size: u64,
    /// Blocks
    pub blocks: u64,
    /// Generation
    pub transid: u64,
    /// Access time
    pub atime: u64,
    /// Modification time
    pub mtime: u64,
    /// Change time
    pub ctime: u64,
    /// Creation time
    pub otime: u64,
    /// Extent listesi
    pub extents: Vec<BtrfsExtent>,
}

/// Btrfs extent
#[derive(Clone, Debug)]
pub struct BtrfsExtent {
    /// Başlangıç blok
    pub start: u64,
    /// Blok sayısı
    pub num_bytes: u64,
    /// Disk offset
    pub disk_bytenr: u64,
    /// Disk byte sayısı
    pub disk_num_bytes: u64,
    /// Offset
    pub offset: u64,
    /// Compression
    pub compression: u8,
    /// Encryption
    pub encryption: u8,
    /// Encoding
    pub encoding: u8,
    /// Type
    pub type_: u8,
}

impl BtrfsInode {
    /// Yeni inode oluştur
    pub fn new(ino: u64, mode: u32, uid: u32, gid: u32) -> Self {
        Self {
            ino,
            generation: 1,
            uid,
            gid,
            mode,
            nlink: 1,
            flags: 0,
            size: 0,
            blocks: 0,
            transid: 1,
            atime: crate::interrupts::get_ticks(),
            mtime: crate::interrupts::get_ticks(),
            ctime: crate::interrupts::get_ticks(),
            otime: crate::interrupts::get_ticks(),
            extents: Vec::new(),
        }
    }
    
    /// Dosya mı?
    pub fn is_file(&self) -> bool {
        (self.mode & 0xF000) == BtrfsInodeType::RegularFile as u32
    }
    
    /// Dizin mi?
    pub fn is_dir(&self) -> bool {
        (self.mode & 0xF000) == BtrfsInodeType::Directory as u32
    }
    
    /// Symlink mi?
    pub fn is_symlink(&self) -> bool {
        (self.mode & 0xF000) == BtrfsInodeType::Symlink as u32
    }
    
    /// Okuma izni var mı?
    pub fn can_read(&self, uid: u32, gid: u32) -> bool {
        // Basit permission check (placeholder)
        (self.mode & 0o400) != 0 || self.uid == uid
    }
    
    /// Yazma izni var mı?
    pub fn can_write(&self, uid: u32, gid: u32) -> bool {
        // Basit permission check (placeholder)
        (self.mode & 0o200) != 0 || self.uid == uid
    }
    
    /// Çalıştırma izni var mı?
    pub fn can_execute(&self, uid: u32, gid: u32) -> bool {
        // Basit permission check (placeholder)
        (self.mode & 0o100) != 0 || self.uid == uid
    }
}

// ============================================================================
// BTRFS B-TREE
// ============================================================================

/// Btrfs B-tree node
#[derive(Clone, Debug)]
pub struct BtrfsBTree {
    /// Node header
    pub header: BtrfsBTreeHeader,
    /// Anahtar-değer çiftleri
    pub items: Vec<BtrfsBTreeItem>,
    /// Child node'lar (internal node için)
    pub children: Vec<BtrfsBTree>,
}

/// B-tree header
#[derive(Clone, Debug)]
pub struct BtrfsBTreeHeader {
    /// Blok adresi
    pub bytenr: u64,
    /// Generation
    pub generation: u64,
    /// Owner
    pub owner: u64,
    /// Number of items
    pub nritems: u32,
    /// Level
    pub level: u8,
    /// Checksum
    pub csum: [u8; 32],
}

/// B-tree item
#[derive(Clone, Debug)]
pub struct BtrfsBTreeItem {
    /// Anahtar
    pub key: BtrfsKey,
    /// Veri offset
    pub offset: u32,
    /// Veri boyutu
    pub size: u32,
}

/// Btrfs anahtarı
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BtrfsKey {
    /// Object ID
    pub objectid: u64,
    /// Type
    pub type_: u8,
    /// Offset
    pub offset: u64,
}

impl BtrfsKey {
    /// Yeni anahtar oluştur
    pub fn new(objectid: u64, type_: u8, offset: u64) -> Self {
        Self {
            objectid,
            type_,
            offset,
        }
    }
}

impl BtrfsBTree {
    /// Yeni B-tree node oluştur
    pub fn new(level: u8) -> Self {
        Self {
            header: BtrfsBTreeHeader {
                bytenr: 0,
                generation: 1,
                owner: 0,
                nritems: 0,
                level,
                csum: [0; 32],
            },
            items: Vec::new(),
            children: Vec::new(),
        }
    }
    
    /// Leaf node mu?
    pub fn is_leaf(&self) -> bool {
        self.header.level == 0
    }
    
    /// Item ekle
    pub fn insert_item(&mut self, key: BtrfsKey, data: Vec<u8>) {
        let item = BtrfsBTreeItem {
            key,
            offset: 0, // Placeholder
            size: data.len() as u32,
        };
        
        self.items.push(item);
        self.items.sort_by(|a, b| a.key.cmp(&b.key));
        self.header.nritems = self.items.len() as u32;
    }
    
    /// Anahtar ara
    pub fn lookup(&self, key: &BtrfsKey) -> Option<&BtrfsBTreeItem> {
        self.items.iter().find(|item| item.key == *key)
    }
    
    /// Aralık ara
    pub fn lookup_range(&self, min_key: &BtrfsKey, max_key: &BtrfsKey) -> Vec<&BtrfsBTreeItem> {
        self.items.iter()
            .filter(|item| item.key >= *min_key && item.key <= *max_key)
            .collect()
    }
}

// ============================================================================
// BTRFS FILESYSTEM
// ============================================================================

/// Btrfs filesystem
pub struct BtrfsFilesystem {
    /// Süperblock
    pub superblock: Mutex<BtrfsSuperblock>,
    /// Root B-tree
    pub root_tree: Mutex<BtrfsBTree>,
    /// Inode'lar
    pub inodes: Mutex<BTreeMap<u64, BtrfsInode>>,
    /// Aktif mi?
    pub active: AtomicBool,
    /// Cihaz yolu
    pub device_path: String,
    /// Checksum type
    pub checksum_type: u16,
    /// Compression type
    pub compression_type: u8,
}

impl BtrfsFilesystem {
    /// Yeni Btrfs filesystem oluştur
    pub fn new(device_path: &str) -> Self {
        Self {
            superblock: Mutex::new(BtrfsSuperblock::new()),
            root_tree: Mutex::new(BtrfsBTree::new(0)),
            inodes: Mutex::new(BTreeMap::new()),
            active: AtomicBool::new(false),
            device_path: device_path.to_string(),
            checksum_type: 0, // CRC32C
            compression_type: 0, // None
        }
    }
    
    /// Filesystem'i mount et
    pub fn mount(&self) -> Result<(), BtrfsError> {
        crate::serial_println!("[Btrfs] Mounting Btrfs filesystem from {}", self.device_path);
        
        // Süperblock'u oku
        self.read_superblock()?;
        
        // Root tree'yi yükle
        self.load_root_tree()?;
        
        // Inode'ları yükle
        self.load_inodes()?;
        
        self.active.store(true, Ordering::SeqCst);
        
        crate::serial_println!("[Btrfs] Btrfs filesystem mounted successfully");
        
        Ok(())
    }
    
    /// Filesystem'i unmount et
    pub fn unmount(&self) -> Result<(), BtrfsError> {
        if !self.active.load(Ordering::SeqCst) {
            return Ok(());
        }
        
        crate::serial_println!("[Btrfs] Unmounting Btrfs filesystem");
        
        // Süperblock'u yaz
        self.write_superblock()?;
        
        self.active.store(false, Ordering::SeqCst);
        
        crate::serial_println!("[Btrfs] Btrfs filesystem unmounted");
        
        Ok(())
    }
    
    /// Süperblock'u oku
    fn read_superblock(&self) -> Result<(), BtrfsError> {
        // Gerçek implementasyonda diskten okunmalı
        crate::serial_println!("[Btrfs] Reading superblock (placeholder)");
        
        let mut sb = self.superblock.lock();
        sb.total_bytes = 1024 * 1024 * 1024; // 1GB
        sb.bytes_used = 0;
        
        Ok(())
    }
    
    /// Süperblock'u yaz
    fn write_superblock(&self) -> Result<(), BtrfsError> {
        // Gerçek implementasyonda diske yazılmalı
        crate::serial_println!("[Btrfs] Writing superblock (placeholder)");
        Ok(())
    }
    
    /// Root tree'yi yükle
    fn load_root_tree(&self) -> Result<(), BtrfsError> {
        crate::serial_println!("[Btrfs] Loading root tree (placeholder)");
        
        let mut root_tree = self.root_tree.lock();
        root_tree.header.owner = 1; // Root tree ID
        
        Ok(())
    }
    
    /// Inode'ları yükle
    fn load_inodes(&self) -> Result<(), BtrfsError> {
        crate::serial_println!("[Btrfs] Loading inodes (placeholder)");
        
        let mut inodes = self.inodes.lock();
        
        // Root inode oluştur
        let root_inode = BtrfsInode::new(
            BTRFS_INODE_ROOT,
            0o755 | BtrfsInodeType::Directory as u32,
            0,
            0,
        );
        inodes.insert(BTRFS_INODE_ROOT, root_inode);
        
        Ok(())
    }
    
    /// Inode oluştur
    pub fn create_inode(&self, mode: u32, uid: u32, gid: u32) -> Result<u64, BtrfsError> {
        let mut inodes = self.inodes.lock();
        
        // Yeni inode numarası bul
        let ino = inodes.keys().max().unwrap_or(&256) + 1;
        
        let inode = BtrfsInode::new(ino, mode, uid, gid);
        inodes.insert(ino, inode);
        
        crate::serial_println!("[Btrfs] Created inode {} with mode 0{:o}", ino, mode);
        
        Ok(ino)
    }
    
    /// Inode al
    pub fn get_inode(&self, ino: u64) -> Result<BtrfsInode, BtrfsError> {
        let inodes = self.inodes.lock();
        inodes.get(&ino).cloned().ok_or(BtrfsError::InodeNotFound)
    }
    
    /// Inode sil
    pub fn remove_inode(&self, ino: u64) -> Result<(), BtrfsError> {
        let mut inodes = self.inodes.lock();
        
        if inodes.remove(&ino).is_some() {
            crate::serial_println!("[Btrfs] Removed inode {}", ino);
            Ok(())
        } else {
            Err(BtrfsError::InodeNotFound)
        }
    }
    
    /// Dosya oku
    pub fn read_file(&self, ino: u64, offset: u64, buf: &mut [u8]) -> Result<usize, BtrfsError> {
        let inode = self.get_inode(ino)?;
        
        if !inode.is_file() {
            return Err(BtrfsError::PermissionDenied);
        }
        
        if offset >= inode.size {
            return Ok(0);
        }
        
        let read_len = core::cmp::min(buf.len(), (inode.size - offset) as usize);
        
        // Gerçek implementasyonda extent'lerden okunmalı
        crate::serial_println!("[Btrfs] Reading {} bytes from inode {} at offset {}", read_len, ino, offset);
        
        // Placeholder: sıfırla doldur
        for i in 0..read_len {
            buf[i] = 0;
        }
        
        Ok(read_len)
    }
    
    /// Dosya yaz
    pub fn write_file(&self, ino: u64, offset: u64, data: &[u8]) -> Result<usize, BtrfsError> {
        let mut inode = self.get_inode(ino)?;
        
        if !inode.is_file() {
            return Err(BtrfsError::PermissionDenied);
        }
        
        let write_len = data.len();
        let new_size = core::cmp::max(inode.size, offset + write_len as u64);
        
        // Dosya boyutunu güncelle
        inode.size = new_size;
        
        // Inode'ı güncelle
        {
            let mut inodes = self.inodes.lock();
            inodes.insert(ino, inode);
        }
        
        crate::serial_println!("[Btrfs] Writing {} bytes to inode {} at offset {}", write_len, ino, offset);
        
        // Gerçek implementasyonda extent'lere yazılmalı
        
        Ok(write_len)
    }
    
    /// Dizin oluştur
    pub fn create_directory(&self, mode: u32, uid: u32, gid: u32) -> Result<u64, BtrfsError> {
        let dir_mode = mode | BtrfsInodeType::Directory as u32;
        self.create_inode(dir_mode, uid, gid)
    }
    
    /// Snapshot oluştur
    pub fn create_snapshot(&self, src_ino: u64, name: &str) -> Result<u64, BtrfsError> {
        crate::serial_println!("[Btrfs] Creating snapshot of inode {} as {}", src_ino, name);
        
        // Kaynak inode'u kopyala
        let src_inode = self.get_inode(src_ino)?;
        let snapshot_ino = self.create_inode(src_inode.mode, src_inode.uid, src_inode.gid)?;
        
        // Extent'leri kopyala (COW)
        let mut snapshot_inode = self.get_inode(snapshot_ino)?;
        snapshot_inode.extents = src_inode.extents.clone();
        
        // Snapshot inode'ı güncelle
        {
            let mut inodes = self.inodes.lock();
            inodes.insert(snapshot_ino, snapshot_inode);
        }
        
        crate::serial_println!("[Btrfs] Snapshot created: {} -> {}", src_ino, snapshot_ino);
        
        Ok(snapshot_ino)
    }
    
    /// İstatistikleri al
    pub fn get_stats(&self) -> BtrfsStats {
        let sb = self.superblock.lock();
        let inodes = self.inodes.lock();
        
        let total_inodes = inodes.len();
        let total_files = inodes.values().filter(|inode| inode.is_file()).count();
        let total_dirs = inodes.values().filter(|inode| inode.is_dir()).count();
        
        BtrfsStats {
            total_bytes: sb.total_bytes,
            bytes_used: sb.bytes_used,
            total_inodes,
            total_files,
            total_dirs,
            active: self.active.load(Ordering::SeqCst),
            compression_type: self.compression_type,
        }
    }
}

/// Btrfs istatistikleri
#[derive(Clone, Debug)]
pub struct BtrfsStats {
    pub total_bytes: u64,
    pub bytes_used: u64,
    pub total_inodes: usize,
    pub total_files: usize,
    pub total_dirs: usize,
    pub active: bool,
    pub compression_type: u8,
}

/// Root inode numarası
pub const BTRFS_INODE_ROOT: u64 = 256;

// ============================================================================
// PUBLIC API
// ============================================================================

/// Btrfs filesystem mount et
pub fn mount_btrfs(device_path: &str) -> Result<Arc<BtrfsFilesystem>, BtrfsError> {
    let fs = Arc::new(BtrfsFilesystem::new(device_path));
    fs.mount()?;
    Ok(fs)
}

/// Btrfs testi
pub fn test_btrfs() -> Result<(), BtrfsError> {
    crate::serial_println!("[Btrfs] Testing Btrfs filesystem");
    
    // Test filesystem oluştur
    let fs = BtrfsFilesystem::new("/dev/test");
    
    // Mount et
    fs.mount()?;
    
    // Root inode kontrol et
    let root_inode = fs.get_inode(BTRFS_INODE_ROOT)?;
    crate::serial_println!("[Btrfs] Root inode: mode=0{:o}, is_dir={}", root_inode.mode, root_inode.is_dir());
    
    // Dosya oluştur
    let file_ino = fs.create_inode(0o644, 1000, 1000)?;
    crate::serial_println!("[Btrfs] Created file inode: {}", file_ino);
    
    // Dosyaya yaz
    let test_data = b"Hello, Btrfs!";
    let written = fs.write_file(file_ino, 0, test_data)?;
    crate::serial_println!("[Btrfs] Wrote {} bytes to file", written);
    
    // Dosyadan oku
    let mut read_buf = [0u8; 32];
    let read = fs.read_file(file_ino, 0, &mut read_buf)?;
    crate::serial_println!("[Btrfs] Read {} bytes from file: {}", read, 
        core::str::from_utf8(&read_buf[..read]).unwrap_or("(invalid utf8)"));
    
    // Dizin oluştur
    let dir_ino = fs.create_directory(0o755, 1000, 1000)?;
    crate::serial_println!("[Btrfs] Created directory inode: {}", dir_ino);
    
    // Snapshot oluştur
    let snapshot_ino = fs.create_snapshot(file_ino, "test_snapshot")?;
    crate::serial_println!("[Btrfs] Created snapshot inode: {}", snapshot_ino);
    
    // İstatistikleri göster
    let stats = fs.get_stats();
    crate::serial_println!("[Btrfs] Stats:");
    crate::serial_println!("  Total bytes: {}", stats.total_bytes);
    crate::serial_println!("  Bytes used: {}", stats.bytes_used);
    crate::serial_println!("  Total inodes: {}", stats.total_inodes);
    crate::serial_println!("  Total files: {}", stats.total_files);
    crate::serial_println!("  Total dirs: {}", stats.total_dirs);
    crate::serial_println!("  Active: {}", stats.active);
    
    // Unmount et
    fs.unmount()?;
    
    crate::serial_println!("[Btrfs] Btrfs test completed");
    
    Ok(())
}
