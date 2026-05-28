//! # GPT Partition Parser
//!
//! UEFI Spec 2.10 compliant GUID Partition Table parser.
//!
//! ## Layout
//! - LBA 0: Protective MBR
//! - LBA 1: Primary GPT Header
//! - LBA 2..N: Partition Entries
//! - LBA Last-1..N: Backup Partition Entries
//! - LBA Last: Backup GPT Header

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

/// GPT Header signature: "EFI PART" in little-endian
const GPT_SIGNATURE: u64 = 0x5452_4150_2049_4645;

/// Minimum GPT header size per UEFI spec
const GPT_HEADER_MIN_SIZE: usize = 92;

/// Standard partition entry size
const GPT_PARTITION_ENTRY_SIZE: usize = 128;

/// Partition name field size (36 UTF-16 chars = 72 bytes)
const GPT_PARTITION_NAME_SIZE: usize = 72;

/// IEEE 802.3 CRC32 (polynomial 0xEDB88320, reflected)
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// GPT Partition Entry (128 bytes)
#[derive(Debug, Clone)]
pub struct GptPartitionEntry {
    pub partition_type_guid: [u8; 16],
    pub unique_partition_guid: [u8; 16],
    pub starting_lba: u64,
    pub ending_lba: u64,
    pub attributes: u64,
    partition_name: [u8; GPT_PARTITION_NAME_SIZE],
}

impl GptPartitionEntry {
    /// Parse a partition entry from raw bytes.
    /// Returns None if the slice is too small.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < GPT_PARTITION_ENTRY_SIZE {
            return None;
        }

        let mut partition_type_guid = [0u8; 16];
        partition_type_guid.copy_from_slice(&data[0..16]);

        let mut unique_partition_guid = [0u8; 16];
        unique_partition_guid.copy_from_slice(&data[16..32]);

        let starting_lba = u64::from_le_bytes(data[32..40].try_into().unwrap());
        let ending_lba = u64::from_le_bytes(data[40..48].try_into().unwrap());
        let attributes = u64::from_le_bytes(data[48..56].try_into().unwrap());

        let mut partition_name = [0u8; GPT_PARTITION_NAME_SIZE];
        partition_name.copy_from_slice(&data[56..56 + GPT_PARTITION_NAME_SIZE]);

        Some(Self {
            partition_type_guid,
            unique_partition_guid,
            starting_lba,
            ending_lba,
            attributes,
            partition_name,
        })
    }

    /// Returns true if this entry is in use (PartitionTypeGUID is not all zeros).
    pub fn is_used(&self) -> bool {
        self.partition_type_guid != [0u8; 16]
    }

    /// Decode the partition name from UTF-16LE to a Rust String.
    pub fn name(&self) -> String {
        let mut chars = Vec::new();
        let mut i = 0;
        while i + 1 < GPT_PARTITION_NAME_SIZE {
            let code_point = u16::from_le_bytes([self.partition_name[i], self.partition_name[i + 1]]);
            if code_point == 0 {
                break;
            }
            chars.push(code_point);
            i += 2;
        }
        String::from_utf16_lossy(&chars)
    }
}

/// GPT Header structure
#[derive(Debug, Clone)]
pub struct GptHeader {
    pub signature: u64,
    pub revision: u32,
    pub header_size: u32,
    pub header_crc32: u32,
    pub reserved: u32,
    pub my_lba: u64,
    pub alternate_lba: u64,
    pub first_usable_lba: u64,
    pub last_usable_lba: u64,
    pub disk_guid: [u8; 16],
    pub partition_entry_lba: u64,
    pub number_of_partition_entries: u32,
    pub size_of_partition_entry: u32,
    pub partition_entry_array_crc32: u32,
}

impl GptHeader {
    /// Parse a GPT header from raw bytes.
    /// Returns None if data is too small or signature doesn't match.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < GPT_HEADER_MIN_SIZE {
            return None;
        }

        let signature = u64::from_le_bytes(data[0..8].try_into().unwrap());
        if signature != GPT_SIGNATURE {
            return None;
        }

        let revision = u32::from_le_bytes(data[8..12].try_into().unwrap());
        let header_size = u32::from_le_bytes(data[12..16].try_into().unwrap());
        let header_crc32 = u32::from_le_bytes(data[16..20].try_into().unwrap());
        let reserved = u32::from_le_bytes(data[20..24].try_into().unwrap());
        let my_lba = u64::from_le_bytes(data[24..32].try_into().unwrap());
        let alternate_lba = u64::from_le_bytes(data[32..40].try_into().unwrap());
        let first_usable_lba = u64::from_le_bytes(data[40..48].try_into().unwrap());
        let last_usable_lba = u64::from_le_bytes(data[48..56].try_into().unwrap());

        let mut disk_guid = [0u8; 16];
        disk_guid.copy_from_slice(&data[56..72]);

        let partition_entry_lba = u64::from_le_bytes(data[72..80].try_into().unwrap());
        let number_of_partition_entries =
            u32::from_le_bytes(data[80..84].try_into().unwrap());
        let size_of_partition_entry = u32::from_le_bytes(data[84..88].try_into().unwrap());
        let partition_entry_array_crc32 =
            u32::from_le_bytes(data[88..92].try_into().unwrap());

        Some(Self {
            signature,
            revision,
            header_size,
            header_crc32,
            reserved,
            my_lba,
            alternate_lba,
            first_usable_lba,
            last_usable_lba,
            disk_guid,
            partition_entry_lba,
            number_of_partition_entries,
            size_of_partition_entry,
            partition_entry_array_crc32,
        })
    }

    /// Verify the header CRC32.
    /// Computes CRC32 over the header with the CRC field zeroed out.
    pub fn verify_crc(&self, raw_data: &[u8]) -> bool {
        let header_size = self.header_size as usize;
        if raw_data.len() < header_size || header_size < GPT_HEADER_MIN_SIZE {
            return false;
        }

        let mut buf = alloc::vec![0u8; header_size];
        buf.copy_from_slice(&raw_data[..header_size]);

        // Zero out the CRC field (bytes 16..20)
        buf[16..20].copy_from_slice(&0u32.to_le_bytes());

        let computed = crc32(&buf);
        computed == self.header_crc32
    }

    /// Check basic validity: signature, header size, revision.
    pub fn is_valid(&self, raw_data: &[u8]) -> bool {
        if self.signature != GPT_SIGNATURE {
            return false;
        }
        if self.header_size < GPT_HEADER_MIN_SIZE as u32 {
            return false;
        }
        if self.revision != 0x0001_0000 {
            return false;
        }
        if !self.verify_crc(raw_data) {
            return false;
        }
        if self.number_of_partition_entries == 0 {
            return false;
        }
        if self.size_of_partition_entry < GPT_PARTITION_ENTRY_SIZE as u32 {
            return false;
        }
        if self.partition_entry_lba == 0 {
            return false;
        }
        true
    }
}

/// Parsed GPT table containing header and partition entries.
#[derive(Debug, Clone)]
pub struct GptTable {
    header: GptHeader,
    entries: Vec<GptPartitionEntry>,
}

impl GptTable {
    /// Parse a GPT table from raw block data.
    /// `data` must contain at least the partition entry area starting at offset 0.
    /// `block_size` is the device block size (typically 512).
    pub fn parse(data: &[u8], block_size: usize) -> Result<Self, &'static str> {
        if data.len() < block_size {
            return Err("gpt: data too small for header block");
        }

        let header = GptHeader::from_bytes(&data[..block_size])
            .ok_or("gpt: invalid header signature")?;

        if !header.is_valid(&data[..block_size]) {
            return Err("gpt: header validation failed");
        }

        let entry_count = header.number_of_partition_entries as usize;
        let entry_size = header.size_of_partition_entry as usize;
        let total_entries_size = entry_count
            .checked_mul(entry_size)
            .ok_or("gpt: partition entries size overflow")?;

        if data.len() < block_size + total_entries_size {
            return Err("gpt: data too small for partition entries");
        }

        let entries_start = block_size;
        let mut entries = Vec::with_capacity(entry_count);

        for i in 0..entry_count {
            let offset = entries_start + i * entry_size;
            if offset + entry_size > data.len() {
                break;
            }
            if let Some(entry) = GptPartitionEntry::from_bytes(&data[offset..offset + entry_size]) {
                entries.push(entry);
            }
        }

        let computed_crc = crc32(&data[entries_start..entries_start + total_entries_size]);
        if computed_crc != header.partition_entry_array_crc32 {
            return Err("gpt: partition entry array CRC mismatch");
        }

        Ok(Self { header, entries })
    }

    /// Return a slice of all partition entries.
    pub fn entries(&self) -> &[GptPartitionEntry] {
        &self.entries
    }

    /// Return the disk GUID as a byte array.
    pub fn disk_guid(&self) -> &[u8; 16] {
        &self.header.disk_guid
    }

    /// Return the GPT header.
    pub fn header(&self) -> &GptHeader {
        &self.header
    }

    /// Return the number of usable partitions.
    pub fn partition_count(&self) -> usize {
        self.entries.iter().filter(|e| e.is_used()).count()
    }
}

/// Parse the primary GPT table.
/// `data` should contain at least 2 blocks: block 0 (header at LBA 1) and block 1+ (partition entries).
/// In practice, pass the raw data starting from the GPT header LBA.
pub fn parse_gpt_primary(data: &[u8], block_size: usize) -> Result<GptTable, &'static str> {
    GptTable::parse(data, block_size)
}

/// Parse the backup GPT table.
/// `data` should contain the backup header and partition entries.
/// `last_lba` is the last LBA of the disk (where the backup header resides).
pub fn parse_gpt_backup(
    data: &[u8],
    block_size: usize,
    last_lba: u64,
) -> Result<GptTable, &'static str> {
    if data.len() < block_size {
        return Err("gpt backup: data too small for header block");
    }

    let header = GptHeader::from_bytes(&data[..block_size])
        .ok_or("gpt backup: invalid header signature")?;

    if header.my_lba != last_lba {
        return Err("gpt backup: header my_lba does not match last_lba");
    }

    GptTable::parse(data, block_size)
}

/// Module initialization
static INIT_DONE: spin::Once<()> = spin::Once::new();

pub fn init() {
    INIT_DONE.call_once(|| {
        crate::serial_println!("[GPT] GPT partition parser initialized");
        crate::serial_println!("[GPT] UEFI Spec 2.10 compliant");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_gpt_data() -> Vec<u8> {
        let block_size = 512;
        let entry_size = 128;
        let entry_count = 128;
        let total_size = block_size + entry_count * entry_size;
        let mut data = alloc::vec![0u8; total_size];

        // Build a valid GPT header
        data[0..8].copy_from_slice(&GPT_SIGNATURE.to_le_bytes());
        data[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
        data[12..16].copy_from_slice(&92u32.to_le_bytes());
        // CRC32 at 16..20 is zeroed for computation
        data[24..32].copy_from_slice(&1u64.to_le_bytes());
        data[32..40].copy_from_slice(&999u64.to_le_bytes());
        data[40..48].copy_from_slice(&34u64.to_le_bytes());
        data[48..56].copy_from_slice(&965u64.to_le_bytes());
        // Disk GUID at 56..72
        data[56..72].copy_from_slice(&[
            0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E,
            0x0F, 0x10,
        ]);
        data[72..80].copy_from_slice(&2u64.to_le_bytes());
        data[80..84].copy_from_slice(&128u32.to_le_bytes());
        data[84..88].copy_from_slice(&128u32.to_le_bytes());

        // Compute header CRC
        let mut header_buf = alloc::vec![0u8; 92];
        header_buf.copy_from_slice(&data[..92]);
        header_buf[16..20].copy_from_slice(&0u32.to_le_bytes());
        let header_crc = crc32(&header_buf);
        data[16..20].copy_from_slice(&header_crc.to_le_bytes());

        // Compute partition entry array CRC
        let entries_crc = crc32(&data[block_size..]);
        data[88..92].copy_from_slice(&entries_crc.to_le_bytes());

        data
    }

    #[test]
    fn gpt_header_parses_correctly() {
        let data = make_test_gpt_data();
        let header = GptHeader::from_bytes(&data[..512]).expect("header parse");
        assert_eq!(header.signature, GPT_SIGNATURE);
        assert_eq!(header.revision, 0x0001_0000);
        assert_eq!(header.header_size, 92);
        assert_eq!(header.my_lba, 1);
        assert_eq!(header.alternate_lba, 999);
        assert_eq!(header.first_usable_lba, 34);
        assert_eq!(header.last_usable_lba, 965);
        assert_eq!(header.partition_entry_lba, 2);
        assert_eq!(header.number_of_partition_entries, 128);
        assert_eq!(header.size_of_partition_entry, 128);
    }

    #[test]
    fn gpt_header_crc_verifies() {
        let data = make_test_gpt_data();
        let header = GptHeader::from_bytes(&data[..512]).expect("header parse");
        assert!(header.verify_crc(&data[..512]));
    }

    #[test]
    fn gpt_table_parses() {
        let data = make_test_gpt_data();
        let table = GptTable::parse(&data, 512).expect("table parse");
        assert_eq!(table.entries.len(), 128);
        assert_eq!(table.partition_count(), 0);
    }

    #[test]
    fn gpt_partition_entry_decode() {
        let mut entry_data = [0u8; 128];
        // Set a non-zero partition type GUID
        entry_data[0] = 0xEF;
        entry_data[1] = 0x02;
        // Starting LBA = 2048
        entry_data[32..40].copy_from_slice(&2048u64.to_le_bytes());
        // Ending LBA = 4095
        entry_data[40..48].copy_from_slice(&4095u64.to_le_bytes());
        // Partition name: "EFI" in UTF-16LE
        entry_data[56] = b'E';
        entry_data[57] = 0;
        entry_data[58] = b'F';
        entry_data[59] = 0;
        entry_data[60] = b'I';
        entry_data[61] = 0;

        let entry = GptPartitionEntry::from_bytes(&entry_data).expect("entry parse");
        assert!(entry.is_used());
        assert_eq!(entry.starting_lba, 2048);
        assert_eq!(entry.ending_lba, 4095);
        assert_eq!(entry.name(), "EFI");
    }

    #[test]
    fn gpt_unused_entry_detects() {
        let entry_data = [0u8; 128];
        let entry = GptPartitionEntry::from_bytes(&entry_data).expect("entry parse");
        assert!(!entry.is_used());
    }

    #[test]
    fn gpt_invalid_signature_rejected() {
        let data = [0u8; 512];
        assert!(GptHeader::from_bytes(&data).is_none());
    }

    #[test]
    fn gpt_backup_validates() {
        let data = make_test_gpt_data();
        let result = parse_gpt_backup(&data, 512, 1);
        assert!(result.is_ok());
    }

    #[test]
    fn crc32_known_value() {
        let data = b"123456789";
        let crc = crc32(data);
        assert_eq!(crc, 0xCBF43926);
    }
}
