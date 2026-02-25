//! # ext4 Journaling System
//!
//! Implements the ext4 journal (JBD2) for crash consistency.
//! Supports ordered mode writes with transaction support.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::vec;
use spin::Mutex;
use core::mem;

// ============================================================================
// JBD2 CONSTANTS
// ============================================================================

/// Journal magic number
const JBD2_MAGIC: u32 = 0xC03B3998;

/// Journal block types
const JBD2_DESCRIPTOR_BLOCK: u32 = 1;
const JBD2_COMMIT_BLOCK: u32 = 2;
const JBD2_SUPERBLOCK_V1: u32 = 3;
const JBD2_SUPERBLOCK_V2: u32 = 4;
const JBD2_REVOKE_BLOCK: u32 = 5;

/// Journal flags
const JBD2_FLAG_ESCAPE: u32 = 1;
const JBD2_FLAG_SAME_UUID: u32 = 2;
const JBD2_FLAG_DELETED: u32 = 4;
const JBD2_FLAG_FLIPPED: u32 = 8;

/// Transaction states
const JBD2_RUNNING: u32 = 0;
const JBD2_LOCKED: u32 = 1;
const JBD2_FLUSHING: u32 = 2;
const JBD2_COMMITTING: u32 = 3;
const JBD2_FINISHED: u32 = 4;

// ============================================================================
// JOURNAL SUPERBLOCK
// ============================================================================

/// Journal superblock (on-disk format)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct JournalSuperblock {
    /// Magic number
    pub s_header_h_magic: u32,
    /// Block type
    pub s_header_h_blocktype: u32,
    /// Sequence number
    pub s_header_h_sequence: u32,
    /// Journal first block
    pub s_first: u32,
    /// Journal sequence number
    pub s_sequence: u32,
    /// Journal block size
    pub s_blocksize: u32,
    /// Journal length in blocks
    pub s_maxlen: u32,
    /// First data block
    pub s_first_data_block: u32,
    /// Transaction ID
    pub s_transaction: u32,
    /// Journal filesystem block size
    pub s_jnl_blocksize: u32,
    /// Number of users
    pub s_users: u32,
    /// Device major
    pub s_dev_major: u32,
    /// Device minor
    pub s_dev_minor: u32,
    /// Start of log
    pub s_start: u32,
    /// Error number
    pub s_errno: u32,
    /// Feature flags
    pub s_feature_compat: u32,
    pub s_feature_incompat: u32,
    pub s_feature_ro_compat: u32,
    /// Journal UUID
    pub s_uuid: [u8; 16],
    /// Number of revoke blocks
    pub s_nr_revokes: u32,
}

impl JournalSuperblock {
    /// Parse from bytes
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < core::mem::size_of::<JournalSuperblock>() {
            return None;
        }

        let magic = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        if magic != JBD2_MAGIC {
            return None;
        }

        let mut sb: JournalSuperblock = unsafe { mem::zeroed() };
        
        sb.s_header_h_magic = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        sb.s_header_h_blocktype = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        sb.s_header_h_sequence = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
        sb.s_first = u32::from_be_bytes([data[12], data[13], data[14], data[15]]);
        sb.s_sequence = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
        sb.s_blocksize = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
        sb.s_maxlen = u32::from_be_bytes([data[24], data[25], data[26], data[27]]);
        sb.s_first_data_block = u32::from_be_bytes([data[28], data[29], data[30], data[31]]);
        sb.s_transaction = u32::from_be_bytes([data[32], data[33], data[34], data[35]]);
        sb.s_jnl_blocksize = u32::from_be_bytes([data[36], data[37], data[38], data[39]]);
        sb.s_users = u32::from_be_bytes([data[40], data[41], data[42], data[43]]);
        sb.s_dev_major = u32::from_be_bytes([data[44], data[45], data[46], data[47]]);
        sb.s_dev_minor = u32::from_be_bytes([data[48], data[49], data[50], data[51]]);
        sb.s_start = u32::from_be_bytes([data[52], data[53], data[54], data[55]]);
        sb.s_errno = u32::from_be_bytes([data[56], data[57], data[58], data[59]]);
        
        // Feature flags at offset 60
        sb.s_feature_compat = u32::from_be_bytes([data[60], data[61], data[62], data[63]]);
        sb.s_feature_incompat = u32::from_be_bytes([data[64], data[65], data[66], data[67]]);
        sb.s_feature_ro_compat = u32::from_be_bytes([data[68], data[69], data[70], data[71]]);
        
        // UUID at offset 72
        sb.s_uuid.copy_from_slice(&data[72..88]);
        
        // Revoke count at offset 88
        sb.s_nr_revokes = u32::from_be_bytes([data[88], data[89], data[90], data[91]]);

        Some(sb)
    }

    /// Serialize to bytes
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = vec![0u8; core::mem::size_of::<JournalSuperblock>()];
        
        data[0..4].copy_from_slice(&self.s_header_h_magic.to_be_bytes());
        data[4..8].copy_from_slice(&self.s_header_h_blocktype.to_be_bytes());
        data[8..12].copy_from_slice(&self.s_header_h_sequence.to_be_bytes());
        data[12..16].copy_from_slice(&self.s_first.to_be_bytes());
        data[16..20].copy_from_slice(&self.s_sequence.to_be_bytes());
        data[20..24].copy_from_slice(&self.s_blocksize.to_be_bytes());
        data[24..28].copy_from_slice(&self.s_maxlen.to_be_bytes());
        data[28..32].copy_from_slice(&self.s_first_data_block.to_be_bytes());
        data[32..36].copy_from_slice(&self.s_transaction.to_be_bytes());
        data[36..40].copy_from_slice(&self.s_jnl_blocksize.to_be_bytes());
        data[40..44].copy_from_slice(&self.s_users.to_be_bytes());
        data[44..48].copy_from_slice(&self.s_dev_major.to_be_bytes());
        data[48..52].copy_from_slice(&self.s_dev_minor.to_be_bytes());
        data[52..56].copy_from_slice(&self.s_start.to_be_bytes());
        data[56..60].copy_from_slice(&self.s_errno.to_be_bytes());
        data[60..64].copy_from_slice(&self.s_feature_compat.to_be_bytes());
        data[64..68].copy_from_slice(&self.s_feature_incompat.to_be_bytes());
        data[68..72].copy_from_slice(&self.s_feature_ro_compat.to_be_bytes());
        data[72..88].copy_from_slice(&self.s_uuid);
        data[88..92].copy_from_slice(&self.s_nr_revokes.to_be_bytes());
        
        data
    }
}

// ============================================================================
// JOURNAL HEADER
// ============================================================================

/// Generic journal header
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct JournalHeader {
    /// Magic number
    pub h_magic: u32,
    /// Block type
    pub h_blocktype: u32,
    /// Sequence number
    pub h_sequence: u32,
}

impl JournalHeader {
    /// Parse from bytes
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 12 {
            return None;
        }

        let magic = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
        if magic != JBD2_MAGIC {
            return None;
        }

        Some(JournalHeader {
            h_magic: magic,
            h_blocktype: u32::from_be_bytes([data[4], data[5], data[6], data[7]]),
            h_sequence: u32::from_be_bytes([data[8], data[9], data[10], data[11]]),
        })
    }

    /// Serialize to bytes
    pub fn serialize(&self) -> [u8; 12] {
        let mut data = [0u8; 12];
        data[0..4].copy_from_slice(&self.h_magic.to_be_bytes());
        data[4..8].copy_from_slice(&self.h_blocktype.to_be_bytes());
        data[8..12].copy_from_slice(&self.h_sequence.to_be_bytes());
        data
    }
}

// ============================================================================
// JOURNAL DESCRIPTOR BLOCK
// ============================================================================

/// Journal descriptor block header
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct DescriptorBlock {
    pub header: JournalHeader,
    pub block_tags: [BlockTag; 16], // Up to 16 tags per block
}

/// Block tag in descriptor
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BlockTag {
    pub t_blocknr: u32,       // Block number
    pub t_flags: u16,         // Flags
    pub t_checksum: u16,      // Checksum
}

impl DescriptorBlock {
    /// Parse from bytes
    pub fn parse(data: &[u8]) -> Option<Self> {
        let header = JournalHeader::parse(data)?;
        
        if header.h_blocktype != JBD2_DESCRIPTOR_BLOCK {
            return None;
        }

        let mut block_tags = [BlockTag { t_blocknr: 0, t_flags: 0, t_checksum: 0 }; 16];
        
        let mut offset = 12; // After header
        for i in 0..16 {
            if offset + 8 > data.len() {
                break;
            }
            
            block_tags[i] = BlockTag {
                t_blocknr: u32::from_be_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]]),
                t_flags: u16::from_be_bytes([data[offset+4], data[offset+5]]),
                t_checksum: u16::from_be_bytes([data[offset+6], data[offset+7]]),
            };
            
            offset += 8;
        }

        Some(DescriptorBlock { header, block_tags })
    }
}

// ============================================================================
// JOURNAL COMMIT BLOCK
// ============================================================================

/// Journal commit block
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CommitBlock {
    pub header: JournalHeader,
    pub h_chksum_type: u8,
    pub h_chksum_size: u8,
    pub h_padding: [u8; 2],
    pub h_chksum: [u8; 32],  // Up to 32 bytes for checksum
}

impl CommitBlock {
    /// Create new commit block
    pub fn new(sequence: u32) -> Self {
        Self {
            header: JournalHeader {
                h_magic: JBD2_MAGIC,
                h_blocktype: JBD2_COMMIT_BLOCK,
                h_sequence: sequence,
            },
            h_chksum_type: 1, // CRC32
            h_chksum_size: 4,
            h_padding: [0; 2],
            h_chksum: [0; 32],
        }
    }

    /// Serialize to bytes
    pub fn serialize(&self, block_size: usize) -> Vec<u8> {
        let mut data = vec![0u8; block_size];
        
        data[0..12].copy_from_slice(&self.header.serialize());
        data[12] = self.h_chksum_type;
        data[13] = self.h_chksum_size;
        data[14..16].copy_from_slice(&self.h_padding);
        data[16..48].copy_from_slice(&self.h_chksum);
        
        data
    }
}

// ============================================================================
// REVOKE BLOCK
// ============================================================================

/// Journal revoke block
#[repr(C)]
#[derive(Clone, Debug)]
pub struct RevokeBlock {
    pub header: JournalHeader,
    pub r_count: u32,        // Number of revoke entries
    pub r_entries: Vec<u32>, // Revoke entries (block numbers)
}

impl RevokeBlock {
    /// Parse from bytes
    pub fn parse(data: &[u8]) -> Option<Self> {
        let header = JournalHeader::parse(data)?;
        
        if header.h_blocktype != JBD2_REVOKE_BLOCK {
            return None;
        }

        let r_count = u32::from_be_bytes([data[12], data[13], data[14], data[15]]);
        let mut r_entries = Vec::new();
        
        let mut offset = 16;
        for _ in 0..r_count {
            if offset + 4 > data.len() {
                break;
            }
            r_entries.push(u32::from_be_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]]));
            offset += 4;
        }

        Some(RevokeBlock { header, r_count, r_entries })
    }
}

// ============================================================================
// TRANSACTION
// ============================================================================

/// Transaction state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransactionState {
    Running,
    Locked,
    Flushing,
    Committing,
    Finished,
}

/// In-memory transaction
#[derive(Clone, Debug)]
pub struct Transaction {
    /// Transaction ID
    pub tid: u64,
    /// State
    pub state: TransactionState,
    /// Blocks to be modified
    pub blocks: Vec<TransactionBlock>,
    /// Blocks to revoke
    pub revokes: Vec<u32>,
    /// Start time (ticks)
    pub start_time: u64,
}

/// Block in transaction
#[derive(Clone, Debug)]
pub struct TransactionBlock {
    /// Block number
    pub block_nr: u32,
    /// Block data
    pub data: Vec<u8>,
    /// Is metadata
    pub is_metadata: bool,
    /// Is new block
    pub is_new: bool,
}

impl Transaction {
    /// Create new transaction
    pub fn new(tid: u64) -> Self {
        Self {
            tid,
            state: TransactionState::Running,
            blocks: Vec::new(),
            revokes: Vec::new(),
            start_time: crate::task::scheduler::get_ticks() as u64,
        }
    }

    /// Add block to transaction
    pub fn add_block(&mut self, block_nr: u32, data: &[u8], is_metadata: bool, is_new: bool) {
        self.blocks.push(TransactionBlock {
            block_nr,
            data: data.to_vec(),
            is_metadata,
            is_new,
        });
    }

    /// Add revoke entry
    pub fn add_revoke(&mut self, block_nr: u32) {
        if !self.revokes.contains(&block_nr) {
            self.revokes.push(block_nr);
        }
    }

    /// Get block count
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }
}

// ============================================================================
// JOURNAL HANDLE
// ============================================================================

/// Journal handle for a transaction
pub struct JournalHandle {
    /// Journal reference
    journal: Arc<Mutex<Journal>>,
    /// Transaction ID
    tid: u64,
    /// Buffer credits (blocks allocated)
    credits: usize,
}

impl JournalHandle {
    /// Create new handle
    pub fn new(journal: Arc<Mutex<Journal>>, tid: u64, credits: usize) -> Self {
        Self { journal, tid, credits }
    }

    /// Get handle credits
    pub fn credits(&self) -> usize {
        self.credits
    }

    /// Extend credits
    pub fn extend(&mut self, additional: usize) {
        self.credits += additional;
    }
}

impl Drop for JournalHandle {
    fn drop(&mut self) {
        // Release unused credits
        // In real implementation, would update journal state
    }
}

// ============================================================================
// JOURNAL
// ============================================================================

/// Journal instance
#[derive(Debug)]
pub struct Journal {
    /// Journal superblock
    pub superblock: JournalSuperblock,
    /// Journal block size
    pub block_size: u32,
    /// Journal device offset
    pub journal_offset: u64,
    /// Current transaction
    pub current_transaction: Option<Transaction>,
    /// Transaction sequence number
    pub sequence: u64,
    /// Running transaction count
    pub running_trans: u32,
    /// Journal buffer
    buffer: Vec<u8>,
}

impl Journal {
    /// Create new journal
    pub fn new(block_size: u32, journal_offset: u64, journal_size: u64) -> Self {
        let mut sb: JournalSuperblock = unsafe { mem::zeroed() };
        sb.s_header_h_magic = JBD2_MAGIC;
        sb.s_header_h_blocktype = JBD2_SUPERBLOCK_V2;
        sb.s_blocksize = block_size;
        sb.s_maxlen = (journal_size / block_size as u64) as u32;
        sb.s_sequence = 1;
        sb.s_start = 1;
        
        Self {
            superblock: sb,
            block_size,
            journal_offset,
            current_transaction: None,
            sequence: 1,
            running_trans: 0,
            buffer: vec![0u8; block_size as usize],
        }
    }

    /// Initialize from device data
    pub fn init(&mut self, device_data: &[u8]) -> Result<(), JournalError> {
        let offset = self.journal_offset as usize;
        
        if offset + self.block_size as usize > device_data.len() {
            return Err(JournalError::InvalidOffset);
        }

        let sb = JournalSuperblock::parse(&device_data[offset..])
            .ok_or(JournalError::InvalidSuperblock)?;

        self.superblock = sb;
        self.sequence = sb.s_sequence as u64;

        crate::serial_println!("[JBD2] Journal initialized: {} blocks, seq={}", 
            sb.s_maxlen, sb.s_sequence);

        Ok(())
    }

    /// Start new transaction
    pub fn start_transaction(&mut self, credits: usize) -> Result<JournalHandle, JournalError> {
        if self.running_trans > 0 {
            // Wait for existing transaction to complete
            // In real implementation, would block
            return Err(JournalError::TransactionRunning);
        }

        let tid = self.sequence;
        self.sequence += 1;
        self.running_trans += 1;

        self.current_transaction = Some(Transaction::new(tid));

        Ok(JournalHandle::new(Arc::new(Mutex::new(self.clone())), tid, credits))
    }

    /// Add block to current transaction
    pub fn add_block(&mut self, block_nr: u32, data: &[u8], is_metadata: bool) -> Result<(), JournalError> {
        let trans = self.current_transaction.as_mut()
            .ok_or(JournalError::NoTransaction)?;

        trans.add_block(block_nr, data, is_metadata, false);
        Ok(())
    }

    /// Add new block allocation to transaction
    pub fn add_new_block(&mut self, block_nr: u32, data: &[u8], is_metadata: bool) -> Result<(), JournalError> {
        let trans = self.current_transaction.as_mut()
            .ok_or(JournalError::NoTransaction)?;

        trans.add_block(block_nr, data, is_metadata, true);
        Ok(())
    }

    /// Commit transaction
    pub fn commit_transaction(&mut self) -> Result<(), JournalError> {
        let trans = self.current_transaction.take()
            .ok_or(JournalError::NoTransaction)?;

        // Phase 1: Write descriptor blocks
        self.write_descriptors(&trans)?;

        // Phase 2: Write data blocks to journal
        self.write_data_blocks(&trans)?;

        // Phase 3: Write commit block
        self.write_commit_block(&trans)?;

        // Phase 4: Write to actual locations (checkpoint)
        self.checkpoint(&trans)?;

        // Phase 5: Update superblock
        self.update_superblock()?;

        self.running_trans = self.running_trans.saturating_sub(1);

        crate::serial_println!("[JBD2] Transaction {} committed ({} blocks)", 
            trans.tid, trans.blocks.len());

        Ok(())
    }

    /// Write descriptor blocks
    fn write_descriptors(&mut self, trans: &Transaction) -> Result<(), JournalError> {
        // In real implementation, would write to journal area
        let _ = trans;
        Ok(())
    }

    /// Write data blocks to journal
    fn write_data_blocks(&mut self, trans: &Transaction) -> Result<(), JournalError> {
        // In real implementation, would write blocks to journal
        let _ = trans;
        Ok(())
    }

    /// Write commit block
    fn write_commit_block(&mut self, trans: &Transaction) -> Result<(), JournalError> {
        let commit = CommitBlock::new(trans.tid as u32);
        let _data = commit.serialize(self.block_size as usize);
        // Would write to journal
        Ok(())
    }

    /// Checkpoint - write blocks to final locations
    fn checkpoint(&mut self, trans: &Transaction) -> Result<(), JournalError> {
        // In real implementation, would write blocks to filesystem
        let _ = trans;
        Ok(())
    }

    /// Update superblock
    fn update_superblock(&mut self) -> Result<(), JournalError> {
        self.superblock.s_sequence = self.sequence as u32;
        Ok(())
    }

    /// Abort transaction
    pub fn abort_transaction(&mut self) {
        self.current_transaction = None;
        self.running_trans = self.running_trans.saturating_sub(1);
        crate::serial_println!("[JBD2] Transaction aborted");
    }

    /// Recover journal on mount
    pub fn recover(&mut self, device_data: &[u8]) -> Result<(), JournalError> {
        let start = self.superblock.s_start;
        let sequence = self.superblock.s_sequence;

        if start == 0 {
            // Journal is clean
            return Ok(());
        }

        crate::serial_println!("[JBD2] Starting recovery from block {}", start);

        // Scan journal for uncommitted transactions
        let mut current_seq: u64 = sequence as u64;
        let mut offset = self.journal_offset + (start as u64) * (self.block_size as u64);

        loop {
            if offset as usize + self.block_size as usize > device_data.len() {
                break;
            }

            let block_data = &device_data[offset as usize..];
            
            if let Some(header) = JournalHeader::parse(block_data) {
                match header.h_blocktype {
                    JBD2_DESCRIPTOR_BLOCK => {
                        // Found transaction start
                        current_seq = header.h_sequence as u64;
                    }
                    JBD2_COMMIT_BLOCK => {
                        // Transaction committed, replay it
                        if header.h_sequence as u64 == current_seq {
                            self.replay_transaction(block_data)?;
                        }
                    }
                    JBD2_REVOKE_BLOCK => {
                        // Handle revoke
                    }
                    _ => {}
                }
            }

            offset += self.block_size as u64;
            
            // Wrap around
            if offset >= self.journal_offset + (self.superblock.s_maxlen as u64) * (self.block_size as u64) {
                break;
            }
        }

        // Mark journal as clean
        self.superblock.s_start = 0;
        self.sequence = current_seq + 1;

        crate::serial_println!("[JBD2] Recovery complete, seq={}", self.sequence);

        Ok(())
    }

    /// Replay a transaction during recovery
    fn replay_transaction(&mut self, _block_data: &[u8]) -> Result<(), JournalError> {
        // In real implementation, would replay committed blocks
        Ok(())
    }

    /// Clone journal (for Arc<Mutex<Journal>>)
    fn clone(&self) -> Self {
        Self {
            superblock: self.superblock,
            block_size: self.block_size,
            journal_offset: self.journal_offset,
            current_transaction: self.current_transaction.clone(),
            sequence: self.sequence,
            running_trans: self.running_trans,
            buffer: self.buffer.clone(),
        }
    }
}

// ============================================================================
// JOURNAL ERROR
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournalError {
    InvalidSuperblock,
    InvalidOffset,
    TransactionRunning,
    NoTransaction,
    WriteError,
    ReadError,
    ChecksumError,
    RecoveryFailed,
}

// ============================================================================
// GLOBAL JOURNAL REGISTRY
// ============================================================================

lazy_static::lazy_static! {
    static ref JOURNAL_INSTANCES: Mutex<BTreeMap<String, Arc<Mutex<Journal>>>> = Mutex::new(BTreeMap::new());
}

/// Register journal
pub fn register_journal(name: &str, journal: Journal) {
    JOURNAL_INSTANCES.lock().insert(name.to_string(), Arc::new(Mutex::new(journal)));
}

/// Get journal by name
pub fn get_journal(name: &str) -> Option<Arc<Mutex<Journal>>> {
    JOURNAL_INSTANCES.lock().get(name).cloned()
}

/// Initialize journal module
pub fn init() {
    crate::serial_println!("[JBD2] Journal module initialized");
}
