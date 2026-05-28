//! # MBR Partition Parser
//!
//! Master Boot Record partition table parser.
//!
//! ## Layout
//! - Bytes 0-439: Boot code
//! - Bytes 440-443: Disk signature
//! - Bytes 444-445: Unused (0x0000)
//! - Bytes 446-509: 4 partition entries (16 bytes each)
//! - Bytes 510-511: Signature 0xAA55

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

/// MBR signature value
const MBR_SIGNATURE: u16 = 0xAA55;

/// MBR partition entry size
const MBR_ENTRY_SIZE: usize = 16;

/// Number of primary partition entries
const MBR_ENTRY_COUNT: usize = 4;

/// Offset of partition entries in MBR
const MBR_PARTITION_OFFSET: usize = 446;

/// Offset of MBR signature
const MBR_SIGNATURE_OFFSET: usize = 510;

/// MBR Partition Entry (16 bytes)
#[derive(Debug, Clone)]
pub struct MbrPartitionEntry {
    pub boot_indicator: u8,
    pub chs_start: [u8; 3],
    pub partition_type: u8,
    pub chs_end: [u8; 3],
    pub lba_start_raw: u32,
    pub lba_size_raw: u32,
}

impl MbrPartitionEntry {
    /// Parse a partition entry from raw bytes.
    /// Returns None if the slice is too small.
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < MBR_ENTRY_SIZE {
            return None;
        }

        Some(Self {
            boot_indicator: data[0],
            chs_start: [data[1], data[2], data[3]],
            partition_type: data[4],
            chs_end: [data[5], data[6], data[7]],
            lba_start_raw: u32::from_le_bytes(data[8..12].try_into().unwrap()),
            lba_size_raw: u32::from_le_bytes(data[12..16].try_into().unwrap()),
        })
    }

    /// Returns true if this entry is valid (non-zero type and non-zero size).
    pub fn is_valid(&self) -> bool {
        self.partition_type != 0x00 && self.lba_size_raw > 0
    }

    /// Returns true if this is an extended partition (type 0x05, 0x0F, or 0x85).
    pub fn is_extended(&self) -> bool {
        matches!(self.partition_type, 0x05 | 0x0F | 0x85)
    }

    /// Returns a human-readable partition type string.
    pub fn partition_type_str(&self) -> &'static str {
        match self.partition_type {
            0x00 => "Unused",
            0x01 => "FAT12",
            0x04 => "FAT16 (<32MB)",
            0x05 => "Extended",
            0x06 => "FAT16 (>32MB)",
            0x07 => "NTFS/exFAT/HPFS",
            0x0B => "FAT32",
            0x0C => "FAT32 (LBA)",
            0x0E => "FAT16 (LBA)",
            0x0F => "Extended (LBA)",
            0x17 => "Hidden NTFS",
            0x1B => "Hidden FAT32",
            0x1C => "Hidden FAT32 (LBA)",
            0x82 => "Linux Swap",
            0x83 => "Linux",
            0x85 => "Linux Extended",
            0x8E => "Linux LVM",
            0xEF => "EFI System Partition",
            0xFD => "Linux RAID Auto",
            _ => "Unknown",
        }
    }

    /// Starting LBA of the partition.
    pub fn lba_start(&self) -> u32 {
        self.lba_start_raw
    }

    /// Size in LBA sectors.
    pub fn lba_size(&self) -> u32 {
        self.lba_size_raw
    }

    /// Returns true if the boot indicator marks this partition as bootable.
    pub fn is_bootable(&self) -> bool {
        self.boot_indicator == 0x80
    }
}

/// Parsed MBR partition table.
#[derive(Debug, Clone)]
pub struct MbrTable {
    pub disk_signature: u32,
    pub unused: u16,
    entries: [MbrPartitionEntry; MBR_ENTRY_COUNT],
    pub has_sig: bool,
}

impl MbrTable {
    /// Parse an MBR table from raw 512-byte data.
    pub fn parse(data: &[u8]) -> Result<Self, &'static str> {
        if data.len() < 512 {
            return Err("mbr: data too small (need 512 bytes)");
        }

        let has_sig = data[MBR_SIGNATURE_OFFSET] == 0x55 && data[MBR_SIGNATURE_OFFSET + 1] == 0xAA;

        let disk_signature = u32::from_le_bytes(data[440..444].try_into().unwrap());
        let unused = u16::from_le_bytes(data[444..446].try_into().unwrap());

        let mut entries: [MbrPartitionEntry; MBR_ENTRY_COUNT] = core::array::from_fn(|i| {
            let offset = MBR_PARTITION_OFFSET + i * MBR_ENTRY_SIZE;
            MbrPartitionEntry::from_bytes(&data[offset..offset + MBR_ENTRY_SIZE])
                .expect("mbr entry parse: slice guaranteed to be 16 bytes")
        });

        Ok(Self {
            disk_signature,
            unused,
            entries,
            has_sig,
        })
    }

    /// Return a slice of all 4 partition entries.
    pub fn entries(&self) -> &[MbrPartitionEntry; MBR_ENTRY_COUNT] {
        &self.entries
    }

    /// Returns true if the MBR signature (0xAA55) is present.
    pub fn has_signature(&self) -> bool {
        self.has_sig
    }

    /// Return the number of valid (non-empty) partitions.
    pub fn valid_partition_count(&self) -> usize {
        self.entries.iter().filter(|e| e.is_valid()).count()
    }
}

/// Parse an MBR partition table from raw data.
pub fn parse_mbr(data: &[u8]) -> Result<MbrTable, &'static str> {
    MbrTable::parse(data)
}

/// Detect the partition table type from raw data.
/// Returns "GPT", "MBR", or "None".
///
/// Detection logic:
/// - GPT: LBA 1 contains "EFI PART" signature (0x5452415020494645 LE)
/// - MBR: bytes 510-511 are 0xAA55
/// - None: neither signature found
pub fn detect_partition_table(data: &[u8]) -> &'static str {
    if data.len() < 512 {
        return "None";
    }

    // Check for GPT: signature at offset 512 (LBA 1, second block)
    if data.len() >= 1024 {
        let gpt_sig = u64::from_le_bytes(data[512..520].try_into().unwrap());
        if gpt_sig == 0x5452_4150_2049_4645 {
            return "GPT";
        }
    }

    // Check for MBR signature
    if data[510] == 0x55 && data[511] == 0xAA {
        return "MBR";
    }

    "None"
}

/// Module initialization
static INIT_DONE: spin::Once<()> = spin::Once::new();

pub fn init() {
    INIT_DONE.call_once(|| {
        crate::serial_println!("[MBR] MBR partition parser initialized");
        crate::serial_println!("[MBR] Supports: FAT12/16/32, NTFS, Linux, EFI System, Extended");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_mbr_data() -> [u8; 512] {
        let mut data = [0u8; 512];

        // Disk signature
        data[440..444].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());

        // Partition 1: bootable FAT32, LBA 2048, size 100000
        data[446] = 0x80; // bootable
        data[450] = 0x0C; // FAT32 LBA
        data[454..458].copy_from_slice(&2048u32.to_le_bytes());
        data[458..462].copy_from_slice(&100000u32.to_le_bytes());

        // Partition 2: Linux, LBA 102048, size 50000
        data[462 + 4] = 0x83; // Linux
        data[462 + 8..462 + 12].copy_from_slice(&102048u32.to_le_bytes());
        data[462 + 12..462 + 16].copy_from_slice(&50000u32.to_le_bytes());

        // MBR signature
        data[510] = 0x55;
        data[511] = 0xAA;

        data
    }

    #[test]
    fn mbr_parses_correctly() {
        let data = make_test_mbr_data();
        let table = MbrTable::parse(&data).expect("mbr parse");
        assert!(table.has_signature());
        assert_eq!(table.disk_signature, 0xDEADBEEF);
        assert_eq!(table.valid_partition_count(), 2);
    }

    #[test]
    fn mbr_partition_types() {
        let data = make_test_mbr_data();
        let table = MbrTable::parse(&data).expect("mbr parse");
        let entries = table.entries();

        assert!(entries[0].is_bootable());
        assert_eq!(entries[0].partition_type_str(), "FAT32 (LBA)");
        assert_eq!(entries[0].lba_start(), 2048);
        assert_eq!(entries[0].lba_size(), 100000);
        assert!(!entries[0].is_extended());

        assert_eq!(entries[1].partition_type_str(), "Linux");
        assert!(!entries[1].is_bootable());
        assert_eq!(entries[1].lba_start(), 102048);
    }

    #[test]
    fn mbr_extended_detection() {
        let mut data = [0u8; 512];
        data[446 + 4] = 0x0F; // Extended LBA
        data[446 + 8..446 + 12].copy_from_slice(&1u32.to_le_bytes());
        data[446 + 12..446 + 16].copy_from_slice(&100u32.to_le_bytes());
        data[510] = 0x55;
        data[511] = 0xAA;

        let table = MbrTable::parse(&data).expect("mbr parse");
        assert!(table.entries()[0].is_extended());
        assert_eq!(table.entries()[0].partition_type_str(), "Extended (LBA)");
    }

    #[test]
    fn mbr_invalid_data_rejected() {
        let small = [0u8; 100];
        assert!(MbrTable::parse(&small).is_err());
    }

    #[test]
    fn detect_gpt() {
        let mut data = alloc::vec![0u8; 1024];
        // GPT signature at LBA 1
        data[512..520].copy_from_slice(&0x5452_4150_2049_4645u64.to_le_bytes());
        assert_eq!(detect_partition_table(&data), "GPT");
    }

    #[test]
    fn detect_mbr() {
        let mut data = alloc::vec![0u8; 512];
        data[510] = 0x55;
        data[511] = 0xAA;
        assert_eq!(detect_partition_table(&data), "MBR");
    }

    #[test]
    fn detect_none() {
        let data = [0u8; 512];
        assert_eq!(detect_partition_table(&data), "None");
    }

    #[test]
    fn detect_none_small_data() {
        let data = [0u8; 100];
        assert_eq!(detect_partition_table(&data), "None");
    }

    #[test]
    fn mbr_entry_from_bytes_too_small() {
        let small = [0u8; 10];
        assert!(MbrPartitionEntry::from_bytes(&small).is_none());
    }

    #[test]
    fn mbr_unused_entry_not_valid() {
        let data = [0u8; 16];
        let entry = MbrPartitionEntry::from_bytes(&data).expect("entry parse");
        assert!(!entry.is_valid());
    }
}
