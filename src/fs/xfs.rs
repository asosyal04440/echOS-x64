//! # XFS Dosya Sistemi — Superblock + AG Parsing + B+Tree
//!
//! XFS, Silicon Graphics (SGI) tarafından geliştirilen yüksek performanslı
//! 64-bit journaling dosya sistemi. Linux'ta varsayılan FS olarak kullanılır.
//!
//! ## Mimari
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────┐
//! │                      XFS Volume                              │
//! ├──────────┬──────────┬──────────┬───────────────────────────────┤
//! │   AG 0   │   AG 1   │   AG 2   │  ...  │   AG N-1            │
//! │(Primary) │          │          │       │                      │
//! ├──────────┤          │          │       │                      │
//! │ SB+AGF   │          │          │       │                      │
//! │ AGI+AGFL │          │          │       │                      │
//! │ B+Tree   │          │          │       │                      │
//! └──────────┴──────────┴──────────┴───────┴──────────────────────┘
//! ```
//!
//! ## Özellikler
//!
//! - 64-bit inode numaraları
//! - Allocation Groups (AG) tabanlı paralelizm
//! - B+Tree inode/extent indeksleme
//! - Extent-based allocation
//! - Delayed allocation
//! - Online defragmentation
//! - Journaling (Write-Ahead Log)
//!
//! ## Disk Yapıları
//!
//! - Superblock (sektör 0, her AG'de kopya)
//! - AGF (AG Free Space) header
//! - AGI (AG Inode) header
//! - AGFL (AG Free List)
//! - Inode B+Tree
//! - Free Space B+Trees (by block, by size)

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::mem;
use spin::Mutex;

// ============================================================================
// XFS Magic Numbers & Constants
// ============================================================================

/// XFS superblock magic: "XFSB"
pub const XFS_SB_MAGIC: u32 = 0x58465342;

/// AGF magic: "XAGF"
pub const XFS_AGF_MAGIC: u32 = 0x58414746;

/// AGI magic: "XAGI"
pub const XFS_AGI_MAGIC: u32 = 0x58414749;

/// AGFL magic: "XAFL"
pub const XFS_AGFL_MAGIC: u32 = 0x5841464C;

/// Inode magic: "IN"
pub const XFS_INODE_MAGIC: u16 = 0x494E;

/// Dinode magic: "IN" (same)
pub const XFS_DINODE_MAGIC: u16 = 0x494E;

/// B+Tree short block magic: "ABTB" (by block), "ABTC" (by count)
pub const XFS_ABTB_MAGIC: u32 = 0x41425442;
pub const XFS_ABTC_MAGIC: u32 = 0x41425443;

/// Inode B+Tree magic: "IABT"
pub const XFS_IBT_MAGIC: u32 = 0x49414254;

/// Default block size
pub const XFS_DEFAULT_BLOCKSIZE: u32 = 4096;

/// Superblock version flags
pub const XFS_SB_VERSION_5: u16 = 5;
pub const XFS_SB_VERSION_NUMBITS: u16 = 0x000F;

/// Inode format types
pub const XFS_DINODE_FMT_DEV: u8 = 0; // Device
pub const XFS_DINODE_FMT_LOCAL: u8 = 1; // Inline data
pub const XFS_DINODE_FMT_EXTENTS: u8 = 2; // Extent list
pub const XFS_DINODE_FMT_BTREE: u8 = 3; // B+Tree

/// Inode mode bits
pub const S_IFMT: u16 = 0o170000;
pub const S_IFREG: u16 = 0o100000;
pub const S_IFDIR: u16 = 0o040000;
pub const S_IFLNK: u16 = 0o120000;
pub const S_IFBLK: u16 = 0o060000;
pub const S_IFCHR: u16 = 0o020000;
pub const S_IFIFO: u16 = 0o010000;
pub const S_IFSOCK: u16 = 0o140000;

// ============================================================================
// On-Disk Structures (Big-Endian on disk!)
// ============================================================================

/// XFS Superblock — sektör 0 ve her AG'nin başında
///
/// XFS superblock'u big-endian olarak saklanır.
/// Bu yapı parse edilmiş (host-endian) haldir.
#[derive(Debug, Clone)]
pub struct XfsSuperblock {
    /// Magic number: 0x58465342 ("XFSB")
    pub sb_magicnum: u32,
    /// Filesystem block size (bytes)
    pub sb_blocksize: u32,
    /// Total data blocks in filesystem
    pub sb_dblocks: u64,
    /// Realtime blocks
    pub sb_rblocks: u64,
    /// Realtime extents
    pub sb_rextents: u64,
    /// UUID
    pub sb_uuid: [u8; 16],
    /// First block of log (journal)
    pub sb_logstart: u64,
    /// Root inode number
    pub sb_rootino: u64,
    /// Realtime bitmap inode
    pub sb_rbmino: u64,
    /// Realtime summary inode
    pub sb_rsumino: u64,
    /// Realtime extent size (blocks)
    pub sb_rextsize: u32,
    /// AG size (blocks)
    pub sb_agblocks: u32,
    /// Number of AGs
    pub sb_agcount: u32,
    /// Realtime bitmap block count
    pub sb_rbmblocks: u32,
    /// Log block count
    pub sb_logblocks: u32,
    /// Version flags
    pub sb_versionnum: u16,
    /// Sector size (bytes)
    pub sb_sectsize: u16,
    /// Inode size (bytes)
    pub sb_inodesize: u16,
    /// Inodes per block
    pub sb_inopblock: u16,
    /// Filesystem name
    pub sb_fname: [u8; 12],
    /// Log2 of block size
    pub sb_blocklog: u8,
    /// Log2 of sector size
    pub sb_sectlog: u8,
    /// Log2 of inode size
    pub sb_inodelog: u8,
    /// Log2 of inodes per block
    pub sb_inopblog: u8,
    /// Log2 of AG size
    pub sb_agblklog: u8,
    /// Free data blocks
    pub sb_fdblocks: u64,
    /// Free inodes
    pub sb_ifree: u64,
    /// Allocated inodes
    pub sb_icount: u64,
    /// Incompatible feature flags
    pub sb_features_incompat: u32,
    /// Compatible feature flags
    pub sb_features_compat: u32,
}

impl XfsSuperblock {
    /// Ham baytlardan superblock parse eder (big-endian → host)
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 272 {
            return None;
        }

        let magic = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        if magic != XFS_SB_MAGIC {
            return None;
        }

        let sb = Self {
            sb_magicnum: magic,
            sb_blocksize: u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
            sb_dblocks: u64::from_be_bytes([
                data[8], data[9], data[10], data[11], data[12], data[13], data[14], data[15],
            ]),
            sb_rblocks: u64::from_be_bytes([
                data[16], data[17], data[18], data[19], data[20], data[21], data[22], data[23],
            ]),
            sb_rextents: u64::from_be_bytes([
                data[24], data[25], data[26], data[27], data[28], data[29], data[30], data[31],
            ]),
            sb_uuid: {
                let mut uuid = [0u8; 16];
                uuid.copy_from_slice(&data[32..48]);
                uuid
            },
            sb_logstart: u64::from_be_bytes([
                data[48], data[49], data[50], data[51], data[52], data[53], data[54], data[55],
            ]),
            sb_rootino: u64::from_be_bytes([
                data[56], data[57], data[58], data[59], data[60], data[61], data[62], data[63],
            ]),
            sb_rbmino: u64::from_be_bytes([
                data[64], data[65], data[66], data[67], data[68], data[69], data[70], data[71],
            ]),
            sb_rsumino: u64::from_be_bytes([
                data[72], data[73], data[74], data[75], data[76], data[77], data[78], data[79],
            ]),
            sb_rextsize: u32::from_be_bytes([data[80], data[81], data[82], data[83]]),
            sb_agblocks: u32::from_be_bytes([data[84], data[85], data[86], data[87]]),
            sb_agcount: u32::from_be_bytes([data[88], data[89], data[90], data[91]]),
            sb_rbmblocks: u32::from_be_bytes([data[92], data[93], data[94], data[95]]),
            sb_logblocks: u32::from_be_bytes([data[96], data[97], data[98], data[99]]),
            sb_versionnum: u16::from_be_bytes([data[100], data[101]]),
            sb_sectsize: u16::from_be_bytes([data[102], data[103]]),
            sb_inodesize: u16::from_be_bytes([data[104], data[105]]),
            sb_inopblock: u16::from_be_bytes([data[106], data[107]]),
            sb_fname: {
                let mut name = [0u8; 12];
                name.copy_from_slice(&data[108..120]);
                name
            },
            sb_blocklog: data[120],
            sb_sectlog: data[121],
            sb_inodelog: data[122],
            sb_inopblog: data[123],
            sb_agblklog: data[124],
            // fdblocks @ offset 144
            sb_fdblocks: u64::from_be_bytes([
                data[144], data[145], data[146], data[147], data[148], data[149], data[150],
                data[151],
            ]),
            sb_ifree: u64::from_be_bytes([
                data[152], data[153], data[154], data[155], data[156], data[157], data[158],
                data[159],
            ]),
            sb_icount: u64::from_be_bytes([
                data[160], data[161], data[162], data[163], data[164], data[165], data[166],
                data[167],
            ]),
            sb_features_incompat: 0,
            sb_features_compat: 0,
        };

        Some(sb)
    }

    /// Toplam kapasiteyi bayt olarak hesaplar
    pub fn total_bytes(&self) -> u64 {
        self.sb_dblocks * self.sb_blocksize as u64
    }

    /// Boş alanı bayt olarak hesaplar
    pub fn free_bytes(&self) -> u64 {
        self.sb_fdblocks * self.sb_blocksize as u64
    }

    /// Kullanılan alanı bayt olarak hesaplar
    pub fn used_bytes(&self) -> u64 {
        (self.sb_dblocks - self.sb_fdblocks) * self.sb_blocksize as u64
    }

    /// Filesystem ismini string olarak döndürür
    pub fn name(&self) -> &str {
        let end = self.sb_fname.iter().position(|&b| b == 0).unwrap_or(12);
        core::str::from_utf8(&self.sb_fname[..end]).unwrap_or("")
    }

    /// XFS versiyon numarası
    pub fn version(&self) -> u16 {
        self.sb_versionnum & XFS_SB_VERSION_NUMBITS
    }

    /// V5 format mı?
    pub fn is_v5(&self) -> bool {
        self.version() >= XFS_SB_VERSION_5
    }
}

// ============================================================================
// Allocation Group Structures
// ============================================================================

/// AG Free Space Header (AGF)
///
/// Her AG'nin free space bilgilerini tutar.
#[derive(Debug, Clone)]
pub struct XfsAgf {
    /// Magic: 0x58414746 ("XAGF")
    pub agf_magicnum: u32,
    /// AG numarası
    pub agf_seqno: u32,
    /// AG'deki toplam blok sayısı
    pub agf_length: u32,
    /// Blok B+Tree kök blok numarası (by block number)
    pub agf_roots_bno: u32,
    /// Blok B+Tree kök blok numarası (by count)
    pub agf_roots_cnt: u32,
    /// B+Tree seviye sayıları
    pub agf_levels_bno: u32,
    pub agf_levels_cnt: u32,
    /// Free list bilgileri
    pub agf_flfirst: u32,
    pub agf_fllast: u32,
    pub agf_flcount: u32,
    /// Boş blok sayısı
    pub agf_freeblks: u32,
    /// En uzun boş extent
    pub agf_longest: u32,
}

impl XfsAgf {
    /// Ham baytlardan AGF parse eder (big-endian)
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 64 {
            return None;
        }

        let magic = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        if magic != XFS_AGF_MAGIC {
            return None;
        }

        Some(Self {
            agf_magicnum: magic,
            agf_seqno: u32::from_be_bytes([data[8], data[9], data[10], data[11]]),
            agf_length: u32::from_be_bytes([data[12], data[13], data[14], data[15]]),
            agf_roots_bno: u32::from_be_bytes([data[16], data[17], data[18], data[19]]),
            agf_roots_cnt: u32::from_be_bytes([data[20], data[21], data[22], data[23]]),
            agf_levels_bno: u32::from_be_bytes([data[28], data[29], data[30], data[31]]),
            agf_levels_cnt: u32::from_be_bytes([data[32], data[33], data[34], data[35]]),
            agf_flfirst: u32::from_be_bytes([data[36], data[37], data[38], data[39]]),
            agf_fllast: u32::from_be_bytes([data[40], data[41], data[42], data[43]]),
            agf_flcount: u32::from_be_bytes([data[44], data[45], data[46], data[47]]),
            agf_freeblks: u32::from_be_bytes([data[48], data[49], data[50], data[51]]),
            agf_longest: u32::from_be_bytes([data[52], data[53], data[54], data[55]]),
        })
    }
}

/// AG Inode Header (AGI)
///
/// Her AG'nin inode bilgilerini tutar.
#[derive(Debug, Clone)]
pub struct XfsAgi {
    /// Magic: 0x58414749 ("XAGI")
    pub agi_magicnum: u32,
    /// AG numarası
    pub agi_seqno: u32,
    /// AG'deki toplam blok sayısı
    pub agi_length: u32,
    /// Inode B+Tree kök blok numarası
    pub agi_root: u32,
    /// Inode B+Tree seviye sayısı
    pub agi_level: u32,
    /// AG'deki toplam inode sayısı
    pub agi_count: u32,
    /// AG'deki boş inode sayısı
    pub agi_freecount: u32,
    /// Yeni inode numarası (son atanan)
    pub agi_newino: u32,
}

impl XfsAgi {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 44 {
            return None;
        }

        let magic = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        if magic != XFS_AGI_MAGIC {
            return None;
        }

        Some(Self {
            agi_magicnum: magic,
            agi_seqno: u32::from_be_bytes([data[8], data[9], data[10], data[11]]),
            agi_length: u32::from_be_bytes([data[12], data[13], data[14], data[15]]),
            agi_root: u32::from_be_bytes([data[16], data[17], data[18], data[19]]),
            agi_level: u32::from_be_bytes([data[20], data[21], data[22], data[23]]),
            agi_count: u32::from_be_bytes([data[24], data[25], data[26], data[27]]),
            agi_freecount: u32::from_be_bytes([data[28], data[29], data[30], data[31]]),
            agi_newino: u32::from_be_bytes([data[32], data[33], data[34], data[35]]),
        })
    }
}

// ============================================================================
// XFS Inode
// ============================================================================

/// XFS Inode yapısı (dinode core)
///
/// 64 veya 128 baytlık sabit kısım + data fork + attr fork
#[derive(Debug, Clone)]
pub struct XfsInode {
    /// Inode numarası (absolute, 64-bit)
    pub ino: u64,
    /// Magic: 0x494E ("IN")
    pub di_magic: u16,
    /// File mode (permissions + type)
    pub di_mode: u16,
    /// Version (1, 2, or 3)
    pub di_version: u8,
    /// Data fork format (DEV, LOCAL, EXTENTS, BTREE)
    pub di_format: u8,
    /// Number of hard links (v1)
    pub di_nlink: u32,
    /// Owner UID
    pub di_uid: u32,
    /// Owner GID
    pub di_gid: u32,
    /// File size (bytes)
    pub di_size: u64,
    /// Data blocks count (512-byte units)
    pub di_nblocks: u64,
    /// Extent size hint (blocks)
    pub di_extsize: u32,
    /// Data fork extent count
    pub di_nextents: u32,
    /// Attr fork extent count
    pub di_anextents: u16,
    /// Attr fork offset (64-byte units)
    pub di_forkoff: u8,
    /// Attr fork format
    pub di_aformat: u8,
    /// Access time (seconds)
    pub di_atime_sec: u32,
    /// Modification time (seconds)
    pub di_mtime_sec: u32,
    /// Change time (seconds)
    pub di_ctime_sec: u32,
    /// Generation number
    pub di_gen: u32,
    /// Inode flags
    pub di_flags: u16,
}

impl XfsInode {
    /// Ham baytlardan inode parse eder
    pub fn from_bytes(data: &[u8], ino: u64) -> Option<Self> {
        if data.len() < 96 {
            return None;
        }

        let magic = u16::from_be_bytes([data[0], data[1]]);
        if magic != XFS_DINODE_MAGIC {
            return None;
        }

        Some(Self {
            ino,
            di_magic: magic,
            di_mode: u16::from_be_bytes([data[2], data[3]]),
            di_version: data[4],
            di_format: data[5],
            di_nlink: u32::from_be_bytes([data[8], data[9], data[10], data[11]]),
            di_uid: u32::from_be_bytes([data[12], data[13], data[14], data[15]]),
            di_gid: u32::from_be_bytes([data[16], data[17], data[18], data[19]]),
            di_size: u64::from_be_bytes([
                data[56], data[57], data[58], data[59], data[60], data[61], data[62], data[63],
            ]),
            di_nblocks: u64::from_be_bytes([
                data[64], data[65], data[66], data[67], data[68], data[69], data[70], data[71],
            ]),
            di_extsize: u32::from_be_bytes([data[72], data[73], data[74], data[75]]),
            di_nextents: u32::from_be_bytes([data[76], data[77], data[78], data[79]]),
            di_anextents: u16::from_be_bytes([data[80], data[81]]),
            di_forkoff: data[82],
            di_aformat: data[83],
            di_atime_sec: u32::from_be_bytes([data[28], data[29], data[30], data[31]]),
            di_mtime_sec: u32::from_be_bytes([data[36], data[37], data[38], data[39]]),
            di_ctime_sec: u32::from_be_bytes([data[44], data[45], data[46], data[47]]),
            di_gen: u32::from_be_bytes([data[52], data[53], data[54], data[55]]),
            di_flags: u16::from_be_bytes([data[84], data[85]]),
        })
    }

    /// Regular file mı?
    pub fn is_regular(&self) -> bool {
        self.di_mode & S_IFMT == S_IFREG
    }

    /// Directory mi?
    pub fn is_directory(&self) -> bool {
        self.di_mode & S_IFMT == S_IFDIR
    }

    /// Symlink mi?
    pub fn is_symlink(&self) -> bool {
        self.di_mode & S_IFMT == S_IFLNK
    }

    /// Dosya tipi string
    pub fn file_type_str(&self) -> &'static str {
        match self.di_mode & S_IFMT {
            S_IFREG => "regular",
            S_IFDIR => "directory",
            S_IFLNK => "symlink",
            S_IFBLK => "block device",
            S_IFCHR => "char device",
            S_IFIFO => "fifo",
            S_IFSOCK => "socket",
            _ => "unknown",
        }
    }

    /// Inline data mı?
    pub fn is_inline(&self) -> bool {
        self.di_format == XFS_DINODE_FMT_LOCAL
    }

    /// Extent list mi?
    pub fn is_extents(&self) -> bool {
        self.di_format == XFS_DINODE_FMT_EXTENTS
    }

    /// B+Tree mi?
    pub fn is_btree(&self) -> bool {
        self.di_format == XFS_DINODE_FMT_BTREE
    }
}

// ============================================================================
// XFS Extent Record
// ============================================================================

/// XFS Extent kaydı (128-bit, packed)
///
/// ```text
/// Bit layout (128 bits):
/// [127]      flag (unwritten)
/// [126:73]   startoff (file offset in blocks, 54 bits)
/// [72:21]    startblock (AG-relative block, 52 bits)
/// [20:0]     blockcount (extent length, 21 bits)
/// ```
#[derive(Debug, Clone, Copy)]
pub struct XfsExtent {
    /// Unwritten flag
    pub flag: bool,
    /// Dosya içindeki blok offset
    pub startoff: u64,
    /// Disk üzerindeki başlangıç bloğu (AG-relative veya absolute)
    pub startblock: u64,
    /// Extent uzunluğu (blok sayısı)
    pub blockcount: u32,
}

impl XfsExtent {
    /// 16 baytlık (128-bit) packed extent kaydını parse eder
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 16 {
            return None;
        }

        // 128 bit big-endian packed record
        let w0 = u64::from_be_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ]);
        let w1 = u64::from_be_bytes([
            data[8], data[9], data[10], data[11], data[12], data[13], data[14], data[15],
        ]);

        let flag = (w0 >> 63) != 0;
        let startoff = (w0 >> 9) & 0x003F_FFFF_FFFF_FFFF; // 54 bits
        let startblock = ((w0 & 0x1FF) << 43) | (w1 >> 21); // 52 bits
        let blockcount = (w1 & 0x001F_FFFF) as u32; // 21 bits

        Some(Self {
            flag,
            startoff,
            startblock,
            blockcount,
        })
    }
}

// ============================================================================
// XFS Directory Entry
// ============================================================================

/// XFS short form directory entry (inline dir)
#[derive(Debug, Clone)]
pub struct XfsDirEntry {
    /// Inode numarası
    pub inumber: u64,
    /// İsim uzunluğu
    pub namelen: u8,
    /// Dosya ismi
    pub name: String,
    /// Dosya tipi (1=regular, 2=dir, 7=symlink vs.)
    pub filetype: u8,
}

/// XFS directory file type codes
pub const XFS_DIR3_FT_UNKNOWN: u8 = 0;
pub const XFS_DIR3_FT_REG_FILE: u8 = 1;
pub const XFS_DIR3_FT_DIR: u8 = 2;
pub const XFS_DIR3_FT_CHRDEV: u8 = 3;
pub const XFS_DIR3_FT_BLKDEV: u8 = 4;
pub const XFS_DIR3_FT_FIFO: u8 = 5;
pub const XFS_DIR3_FT_SOCK: u8 = 6;
pub const XFS_DIR3_FT_SYMLINK: u8 = 7;

// ============================================================================
// B+Tree Node Structures
// ============================================================================

/// XFS B+Tree short block header (AG-relative)
#[derive(Debug, Clone)]
pub struct XfsBtreeShortBlock {
    /// Magic number
    pub bb_magic: u32,
    /// B+Tree seviyesi (0=leaf)
    pub bb_level: u16,
    /// Bu bloktaki kayıt sayısı
    pub bb_numrecs: u16,
    /// Sol kardeş blok
    pub bb_leftsib: u32,
    /// Sağ kardeş blok
    pub bb_rightsib: u32,
}

impl XfsBtreeShortBlock {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 16 {
            return None;
        }
        Some(Self {
            bb_magic: u32::from_be_bytes([data[0], data[1], data[2], data[3]]),
            bb_level: u16::from_be_bytes([data[4], data[5]]),
            bb_numrecs: u16::from_be_bytes([data[6], data[7]]),
            bb_leftsib: u32::from_be_bytes([data[8], data[9], data[10], data[11]]),
            bb_rightsib: u32::from_be_bytes([data[12], data[13], data[14], data[15]]),
        })
    }

    pub fn is_leaf(&self) -> bool {
        self.bb_level == 0
    }
}

/// Free space B+Tree record (by block number)
#[derive(Debug, Clone, Copy)]
pub struct XfsAllocRec {
    /// Başlangıç bloğu (AG-relative)
    pub ar_startblock: u32,
    /// Blok sayısı
    pub ar_blockcount: u32,
}

impl XfsAllocRec {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }
        Some(Self {
            ar_startblock: u32::from_be_bytes([data[0], data[1], data[2], data[3]]),
            ar_blockcount: u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
        })
    }
}

/// Inode B+Tree record
#[derive(Debug, Clone, Copy)]
pub struct XfsInoBtRec {
    /// Başlangıç inode numarası
    pub ir_startino: u32,
    /// Free mask (bit=1 → inode serbest)
    pub ir_free: u64,
}

impl XfsInoBtRec {
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < 16 {
            return None;
        }
        Some(Self {
            ir_startino: u32::from_be_bytes([data[0], data[1], data[2], data[3]]),
            ir_free: u64::from_be_bytes([
                data[4], data[5], data[6], data[7], data[8], data[9], data[10], data[11],
            ]),
        })
    }
}

// ============================================================================
// XFS Filesystem Manager
// ============================================================================

/// XFS dosya sistemi yöneticisi
pub struct XfsFilesystem {
    /// Parse edilmiş superblock
    pub superblock: XfsSuperblock,
    /// AG bilgileri: (AGF, AGI)
    pub ags: Vec<(XfsAgf, XfsAgi)>,
    /// Inode cache
    pub inode_cache: BTreeMap<u64, XfsInode>,
    /// Mount noktası
    pub mount_point: String,
}

impl XfsFilesystem {
    /// Superblock'tan yeni XFS oluşturur
    pub fn new(sb: XfsSuperblock, mount_point: &str) -> Self {
        Self {
            superblock: sb,
            ags: Vec::new(),
            inode_cache: BTreeMap::new(),
            mount_point: String::from(mount_point),
        }
    }

    /// AG header'larını okur ve parse eder
    pub fn parse_ags(&mut self, disk_data: &[u8]) {
        let block_size = self.superblock.sb_blocksize as usize;

        for ag_no in 0..self.superblock.sb_agcount {
            let ag_offset = ag_no as usize * self.superblock.sb_agblocks as usize * block_size;

            // AGF = superblock'dan 1 sektör sonra (blok 1)
            let agf_offset = ag_offset + block_size;
            if agf_offset + 64 > disk_data.len() {
                break;
            }

            let agf = XfsAgf::from_bytes(&disk_data[agf_offset..]);

            // AGI = superblock'dan 2 sektör sonra (blok 2)
            let agi_offset = ag_offset + 2 * block_size;
            if agi_offset + 44 > disk_data.len() {
                break;
            }

            let agi = XfsAgi::from_bytes(&disk_data[agi_offset..]);

            if let (Some(agf), Some(agi)) = (agf, agi) {
                crate::serial_println!(
                    "[XFS] AG {}: blocks={}, free_blocks={}, inodes={}, free_inodes={}",
                    ag_no,
                    agf.agf_length,
                    agf.agf_freeblks,
                    agi.agi_count,
                    agi.agi_freecount
                );
                self.ags.push((agf, agi));
            }
        }
    }

    /// Inode numarasından AG numarası ve AG-relative inode hesaplar
    pub fn ino_to_agno(&self, ino: u64) -> u32 {
        (ino >> self.superblock.sb_agblklog as u64) as u32
    }

    /// Inode numarasından AG-relative offset hesaplar
    pub fn ino_to_agino(&self, ino: u64) -> u32 {
        let mask = (1u64 << self.superblock.sb_agblklog as u64) - 1;
        (ino & mask) as u32
    }

    /// Root inode'u okur ve cache'e ekler
    pub fn read_root_inode(&mut self, disk_data: &[u8]) -> Option<&XfsInode> {
        let root_ino = self.superblock.sb_rootino;
        self.read_inode(disk_data, root_ino)
    }

    /// Verilen inode numarasını diskten okur
    pub fn read_inode(&mut self, disk_data: &[u8], ino: u64) -> Option<&XfsInode> {
        if self.inode_cache.contains_key(&ino) {
            return self.inode_cache.get(&ino);
        }

        let inode_size = self.superblock.sb_inodesize as usize;
        let block_size = self.superblock.sb_blocksize as usize;
        let inodes_per_block = self.superblock.sb_inopblock as usize;

        // AG ve AG-relative inode hesapla
        let ag_no = self.ino_to_agno(ino) as usize;
        let ag_ino = self.ino_to_agino(ino) as usize;

        // Disk offset hesapla
        let ag_offset = ag_no * self.superblock.sb_agblocks as usize * block_size;
        let inode_block = ag_ino / inodes_per_block;
        let inode_offset_in_block = ag_ino % inodes_per_block;

        let disk_offset = ag_offset + inode_block * block_size + inode_offset_in_block * inode_size;

        if disk_offset + inode_size > disk_data.len() {
            return None;
        }

        let inode = XfsInode::from_bytes(&disk_data[disk_offset..], ino)?;
        self.inode_cache.insert(ino, inode);
        self.inode_cache.get(&ino)
    }

    /// Dosya sistemi bilgilerini yazdırır
    pub fn print_info(&self) {
        crate::serial_println!("[XFS] === Filesystem Info ===");
        crate::serial_println!("[XFS] Name: {}", self.superblock.name());
        crate::serial_println!("[XFS] Version: {}", self.superblock.version());
        crate::serial_println!("[XFS] Block size: {} bytes", self.superblock.sb_blocksize);
        crate::serial_println!("[XFS] Total blocks: {}", self.superblock.sb_dblocks);
        crate::serial_println!("[XFS] Free blocks: {}", self.superblock.sb_fdblocks);
        crate::serial_println!("[XFS] AG count: {}", self.superblock.sb_agcount);
        crate::serial_println!("[XFS] AG size: {} blocks", self.superblock.sb_agblocks);
        crate::serial_println!("[XFS] Inode size: {} bytes", self.superblock.sb_inodesize);
        crate::serial_println!("[XFS] Root inode: {}", self.superblock.sb_rootino);
        crate::serial_println!(
            "[XFS] Total: {} MB",
            self.superblock.total_bytes() / (1024 * 1024)
        );
        crate::serial_println!(
            "[XFS] Free: {} MB",
            self.superblock.free_bytes() / (1024 * 1024)
        );
        crate::serial_println!("[XFS] Mount: {}", self.mount_point);
    }

    /// Toplam AG sayısı
    pub fn ag_count(&self) -> u32 {
        self.superblock.sb_agcount
    }

    /// Toplam boş inode sayısı
    pub fn total_free_inodes(&self) -> u64 {
        self.superblock.sb_ifree
    }

    /// Toplam kullanılan inode sayısı
    pub fn total_used_inodes(&self) -> u64 {
        self.superblock.sb_icount - self.superblock.sb_ifree
    }
}

// ============================================================================
// Global XFS Registry
// ============================================================================

lazy_static::lazy_static! {
    /// Mount edilmiş XFS dosya sistemleri
    static ref XFS_FILESYSTEMS: Mutex<Vec<XfsFilesystem>> = Mutex::new(Vec::new());
}

/// XFS modülünü başlatır
pub fn init() {
    crate::serial_println!("[XFS] XFS filesystem module initialized");
    crate::serial_println!("[XFS] Supported: superblock, AG parsing, B+Tree, extents, inode read");
}

/// XFS'i bir blok cihazından mount eder
pub fn mount_from_data(disk_data: &[u8], mount_point: &str) -> Result<(), &'static str> {
    let sb = XfsSuperblock::from_bytes(disk_data).ok_or("Invalid XFS superblock")?;

    let mut fs = XfsFilesystem::new(sb, mount_point);
    fs.parse_ags(disk_data);
    fs.print_info();

    XFS_FILESYSTEMS.lock().push(fs);
    Ok(())
}

/// Mount edilmiş XFS sayısı
pub fn mounted_count() -> usize {
    XFS_FILESYSTEMS.lock().len()
}
