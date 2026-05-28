//! # qcow2 Image Parser
//!
//! QEMU Copy-On-Write version 2 image format parser.
//!
//! ## Header Layout (v2, 72 bytes minimum)
//! - Magic: "QFI\xfb" = 0x514649FB
//! - Version: u32 (2 or 3)
//! - Backing file offset: u64
//! - Backing file size: u32
//! - Cluster bits: u32 (cluster_size = 1 << cluster_bits)
//! - Size: u64 (virtual disk size in bytes)
//! - L1 table offset: u64
//! - L1 size: u32
//! - Refcount table offset: u64
//! - Refcount table clusters: u32
//! - Number of snapshots: u32 (v3)
//! - Snapshots offset: u64 (v3)
//!
//! ## L1/L2 Translation
//! - L1 table: array of u64 entries, each pointing to an L2 table
//! - L2 table: array of u64 entries, each pointing to a data cluster
//! - L2 entry bit 63: allocated flag
//! - L2 entry bits 0-62: cluster offset within the image

use alloc::vec::Vec;
use spin::Mutex;

/// qcow2 magic number: "QFI\xfb"
const QCOW2_MAGIC: u32 = 0x5146_49FB;

/// Minimum header size for v2
const QCOW2_HEADER_V2_SIZE: usize = 72;

/// Minimum header size for v3
const QCOW2_HEADER_V3_SIZE: usize = 104;

/// L2 entry allocated bit (bit 63)
const L2_ENTRY_ALLOCATED: u64 = 1 << 63;

/// L2 entry offset mask (bits 0-62)
const L2_ENTRY_OFFSET_MASK: u64 = !(1u64 << 63);

/// qcow2 Header structure
#[derive(Debug, Clone)]
pub struct Qcow2Header {
    pub magic: u32,
    pub version: u32,
    pub backing_file_offset: u64,
    pub backing_file_size: u32,
    pub cluster_bits: u32,
    pub size: u64,
    pub crypt_method: u32,
    pub l1_size: u32,
    pub l1_table_offset: u64,
    pub refcount_table_offset: u64,
    pub refcount_table_clusters: u32,
    pub nb_snapshots: u32,
    pub snapshots_offset: u64,
}

impl Qcow2Header {
    /// Parse a qcow2 header from raw bytes.
    /// Returns None if magic doesn't match or data is too small.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < QCOW2_HEADER_V2_SIZE {
            return None;
        }

        let magic = u32::from_be_bytes(data[0..4].try_into().unwrap());
        if magic != QCOW2_MAGIC {
            return None;
        }

        let version = u32::from_be_bytes(data[4..8].try_into().unwrap());
        if version != 2 && version != 3 {
            return None;
        }

        let min_size = if version == 3 {
            QCOW2_HEADER_V3_SIZE
        } else {
            QCOW2_HEADER_V2_SIZE
        };

        if data.len() < min_size {
            return None;
        }

        let backing_file_offset = u64::from_be_bytes(data[8..16].try_into().unwrap());
        let backing_file_size = u32::from_be_bytes(data[16..20].try_into().unwrap());
        let cluster_bits = u32::from_be_bytes(data[20..24].try_into().unwrap());
        let size = u64::from_be_bytes(data[24..32].try_into().unwrap());
        let crypt_method = u32::from_be_bytes(data[32..36].try_into().unwrap());
        let l1_size = u32::from_be_bytes(data[36..40].try_into().unwrap());
        let l1_table_offset = u64::from_be_bytes(data[40..48].try_into().unwrap());
        let refcount_table_offset = u64::from_be_bytes(data[48..56].try_into().unwrap());
        let refcount_table_clusters = u32::from_be_bytes(data[56..60].try_into().unwrap());

        let (nb_snapshots, snapshots_offset) = if version >= 3 {
            (
                u32::from_be_bytes(data[60..64].try_into().unwrap()),
                u64::from_be_bytes(data[64..72].try_into().unwrap()),
            )
        } else {
            (0, 0)
        };

        Some(Self {
            magic,
            version,
            backing_file_offset,
            backing_file_size,
            cluster_bits,
            size,
            crypt_method,
            l1_size,
            l1_table_offset,
            refcount_table_offset,
            refcount_table_clusters,
            nb_snapshots,
            snapshots_offset,
        })
    }

    /// Returns the cluster size in bytes (1 << cluster_bits).
    pub fn cluster_size(&self) -> usize {
        1usize << self.cluster_bits
    }

    /// Returns the virtual disk size in bytes.
    pub fn virtual_size(&self) -> u64 {
        self.size
    }

    /// Returns the qcow2 version.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Check basic validity: magic, version, cluster bits range.
    pub fn is_valid(&self) -> bool {
        if self.magic != QCOW2_MAGIC {
            return false;
        }
        if self.version != 2 && self.version != 3 {
            return false;
        }
        // Cluster bits must be reasonable (9..=21 covers 512B to 2MB)
        if self.cluster_bits < 9 || self.cluster_bits > 21 {
            return false;
        }
        if self.size == 0 {
            return false;
        }
        true
    }
}

/// qcow2 image reader backed by in-memory data.
pub struct Qcow2Image {
    header: Qcow2Header,
    data: Vec<u8>,
    l1_table: Vec<u64>,
}

impl Qcow2Image {
    /// Open a qcow2 image from raw bytes.
    /// Parses the header and loads the L1 table.
    pub fn open(data: &[u8]) -> Result<Self, &'static str> {
        let header = Qcow2Header::from_bytes(data).ok_or("qcow2: invalid header")?;

        if !header.is_valid() {
            return Err("qcow2: header validation failed");
        }

        if header.crypt_method != 0 {
            return Err("qcow2: encrypted images not supported");
        }

        if header.backing_file_offset != 0 || header.backing_file_size != 0 {
            return Err("qcow2: backing files not supported");
        }

        if header.nb_snapshots != 0 || header.snapshots_offset != 0 {
            return Err("qcow2: internal snapshots not supported");
        }

        // Load L1 table
        let l1_entry_size = core::mem::size_of::<u64>();
        let l1_byte_size = header.l1_size as usize * l1_entry_size;
        let l1_offset = header.l1_table_offset as usize;

        if l1_offset + l1_byte_size > data.len() {
            return Err("qcow2: L1 table out of bounds");
        }

        let mut l1_table = Vec::with_capacity(header.l1_size as usize);
        for i in 0..header.l1_size as usize {
            let offset = l1_offset + i * l1_entry_size;
            let entry_bytes: [u8; 8] = data[offset..offset + 8]
                .try_into()
                .map_err(|_| "qcow2: L1 entry read failed")?;
            l1_table.push(u64::from_be_bytes(entry_bytes));
        }

        Ok(Self {
            header,
            data: data.to_vec(),
            l1_table,
        })
    }

    /// Read data from the guest's virtual disk at `guest_offset` into `buf`.
    /// Returns the number of bytes actually read.
    ///
    /// Translation: guest_offset -> L1 index -> L2 index -> cluster offset -> data
    pub fn read_cluster(&self, guest_offset: u64, buf: &mut [u8]) -> Result<usize, &'static str> {
        let cluster_size = self.header.cluster_size();
        let cluster_bits = self.header.cluster_bits as u64;
        let l2_bits = cluster_bits - 3; // log2(cluster_size / 8)

        let l1_index = guest_offset >> (2 * cluster_bits - 3);
        let l2_index = (guest_offset >> cluster_bits) & ((1u64 << l2_bits) - 1);
        let offset_in_cluster = (guest_offset & ((1u64 << cluster_bits) - 1)) as usize;

        if l1_index as usize >= self.l1_table.len() {
            return Ok(0);
        }

        let l1_entry = self.l1_table[l1_index as usize];
        if l1_entry == 0 {
            return Ok(0);
        }

        let l2_table_offset = (l1_entry & L2_ENTRY_OFFSET_MASK) as usize;
        let l2_entry_offset = l2_table_offset + (l2_index as usize * 8);

        if l2_entry_offset + 8 > self.data.len() {
            return Err("qcow2: L2 entry out of bounds");
        }

        let l2_entry_bytes: [u8; 8] = self.data[l2_entry_offset..l2_entry_offset + 8]
            .try_into()
            .map_err(|_| "qcow2: L2 entry read failed")?;
        let l2_entry = u64::from_be_bytes(l2_entry_bytes);

        if l2_entry == 0 {
            return Ok(0);
        }

        if l2_entry & L2_ENTRY_ALLOCATED == 0 {
            return Ok(0);
        }

        let cluster_offset = (l2_entry & L2_ENTRY_OFFSET_MASK) as usize;
        let data_offset = cluster_offset + offset_in_cluster;
        let available = self.data.len().saturating_sub(data_offset);
        let to_read = buf.len().min(available);

        if to_read == 0 {
            return Ok(0);
        }

        buf[..to_read].copy_from_slice(&self.data[data_offset..data_offset + to_read]);
        Ok(to_read)
    }

    /// Read a full cluster into a buffer.
    /// The buffer must be at least cluster_size bytes.
    pub fn read_full_cluster(
        &self,
        guest_cluster_index: u64,
        buf: &mut [u8],
    ) -> Result<usize, &'static str> {
        let cluster_size = self.header.cluster_size();
        if buf.len() < cluster_size {
            return Err("qcow2: buffer too small for cluster");
        }

        let guest_offset = guest_cluster_index << self.header.cluster_bits;
        self.read_cluster(guest_offset, &mut buf[..cluster_size])
    }

    /// Return the header.
    pub fn header(&self) -> &Qcow2Header {
        &self.header
    }

    /// Return the virtual disk size.
    pub fn virtual_size(&self) -> u64 {
        self.header.size
    }

    /// Return the number of L1 entries.
    pub fn l1_size(&self) -> usize {
        self.l1_table.len()
    }
}

/// Module initialization
static INIT_DONE: spin::Once<()> = spin::Once::new();

pub fn init() {
    INIT_DONE.call_once(|| {
        crate::serial_println!("[qcow2] qcow2 image parser initialized");
        crate::serial_println!("[qcow2] Supports: v2/v3, L1/L2 translation, uncompressed clusters");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_qcow2_data(cluster_bits: u32, virtual_size: u64, l1_size: u32) -> Vec<u8> {
        let cluster_size = 1usize << cluster_bits;
        let header_size = QCOW2_HEADER_V2_SIZE;
        let l1_byte_size = l1_size as usize * 8;
        // One L2 table + one data cluster
        let l2_size = cluster_size;
        let data_cluster_size = cluster_size;
        let total_size = header_size + l1_byte_size + l2_size + data_cluster_size;

        let mut data = alloc::vec![0u8; total_size];

        // Magic
        data[0..4].copy_from_slice(&QCOW2_MAGIC.to_be_bytes());
        // Version 2
        data[4..8].copy_from_slice(&2u32.to_be_bytes());
        // Backing file offset = 0
        data[8..16].copy_from_slice(&0u64.to_be_bytes());
        // Backing file size = 0
        data[16..20].copy_from_slice(&0u32.to_be_bytes());
        // Cluster bits
        data[20..24].copy_from_slice(&cluster_bits.to_be_bytes());
        // Virtual size
        data[24..32].copy_from_slice(&virtual_size.to_be_bytes());
        // Crypt method = 0
        data[32..36].copy_from_slice(&0u32.to_be_bytes());
        // L1 size
        data[36..40].copy_from_slice(&l1_size.to_be_bytes());
        // L1 table offset (right after header)
        let l1_offset = header_size as u64;
        data[40..48].copy_from_slice(&l1_offset.to_be_bytes());
        // Refcount table offset = 0 (not used in test)
        data[48..56].copy_from_slice(&0u64.to_be_bytes());
        // Refcount table clusters = 0
        data[56..60].copy_from_slice(&0u32.to_be_bytes());

        // L1 entry: points to L2 table
        let l2_offset = (header_size + l1_byte_size) as u64;
        let l1_entry = l2_offset | L2_ENTRY_ALLOCATED;
        data[header_size..header_size + 8].copy_from_slice(&l1_entry.to_be_bytes());

        // L2 entry: points to data cluster
        let data_offset = (header_size + l1_byte_size + l2_size) as u64;
        let l2_entry = data_offset | L2_ENTRY_ALLOCATED;
        let l2_start = header_size + l1_byte_size;
        data[l2_start..l2_start + 8].copy_from_slice(&l2_entry.to_be_bytes());

        // Write test data into the data cluster
        let data_start = header_size + l1_byte_size + l2_size;
        if data_start + 8 <= data.len() {
            data[data_start..data_start + 8].copy_from_slice(b"qcow2img");
        }

        data
    }

    #[test]
    fn qcow2_header_parses() {
        let data = make_test_qcow2_data(16, 64 * 1024 * 1024, 1);
        let header = Qcow2Header::from_bytes(&data).expect("header parse");
        assert_eq!(header.magic, QCOW2_MAGIC);
        assert_eq!(header.version, 2);
        assert_eq!(header.cluster_bits, 16);
        assert_eq!(header.cluster_size(), 65536);
        assert_eq!(header.virtual_size(), 64 * 1024 * 1024);
        assert!(header.is_valid());
    }

    #[test]
    fn qcow2_image_opens() {
        let data = make_test_qcow2_data(16, 64 * 1024 * 1024, 1);
        let image = Qcow2Image::open(&data).expect("image open");
        assert_eq!(image.l1_size(), 1);
        assert_eq!(image.virtual_size(), 64 * 1024 * 1024);
    }

    #[test]
    fn qcow2_read_cluster_data() {
        let data = make_test_qcow2_data(16, 64 * 1024 * 1024, 1);
        let image = Qcow2Image::open(&data).expect("image open");

        let mut buf = [0u8; 8];
        let read = image.read_cluster(0, &mut buf).expect("read");
        assert_eq!(read, 8);
        assert_eq!(&buf, b"qcow2img");
    }

    #[test]
    fn qcow2_read_beyond_data_returns_zero() {
        let data = make_test_qcow2_data(16, 64 * 1024 * 1024, 1);
        let image = Qcow2Image::open(&data).expect("image open");

        // Read from a guest offset that maps to an unallocated cluster
        let huge_offset = (1u64 << 40);
        let mut buf = [0u8; 512];
        let read = image.read_cluster(huge_offset, &mut buf).expect("read");
        assert_eq!(read, 0);
    }

    #[test]
    fn qcow2_invalid_magic_rejected() {
        let mut data = alloc::vec![0u8; 72];
        data[0..4].copy_from_slice(&0xDEADBEEFu32.to_be_bytes());
        assert!(Qcow2Header::from_bytes(&data).is_none());
    }

    #[test]
    fn qcow2_invalid_version_rejected() {
        let mut data = alloc::vec![0u8; 72];
        data[0..4].copy_from_slice(&QCOW2_MAGIC.to_be_bytes());
        data[4..8].copy_from_slice(&1u32.to_be_bytes());
        assert!(Qcow2Header::from_bytes(&data).is_none());
    }

    #[test]
    fn qcow2_too_small_data_rejected() {
        let small = [0u8; 50];
        assert!(Qcow2Header::from_bytes(&small).is_none());
    }

    #[test]
    fn qcow2_read_full_cluster() {
        let data = make_test_qcow2_data(16, 64 * 1024 * 1024, 1);
        let image = Qcow2Image::open(&data).expect("image open");

        let mut buf = alloc::vec![0u8; 65536];
        let read = image.read_full_cluster(0, &mut buf).expect("read");
        assert_eq!(read, 65536);
        assert_eq!(&buf[..8], b"qcow2img");
    }
}
