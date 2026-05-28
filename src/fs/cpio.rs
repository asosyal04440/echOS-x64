//! CPIO "newc" and CRC archive parser.
//!
//! Header layout (110 bytes ASCII):
//!   Magic      6 bytes ("070701" or "070702")
//!   Ino        8 hex chars
//!   Mode       8 hex chars
//!   UID        8 hex chars
//!   GID        8 hex chars
//!   Nlink      8 hex chars
//!   Mtime      8 hex chars
//!   Filesize   8 hex chars
//!   Devmajor   8 hex chars
//!   Devminor   8 hex chars
//!   Rdevmajor  8 hex chars
//!   Rdevminor  8 hex chars
//!   Namesize   8 hex chars
//!   Check      8 hex chars
//!   Name       variable (null-terminated, padded to 4-byte alignment)
//!   Data       variable (padded to 4-byte alignment)
//!
//! Trailer entry has name = "TRAILER!!!"

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use lazy_static::lazy_static;
use spin::Mutex;

const NEWC_HEADER_SIZE: usize = 110;
const MAGIC_NEWC: &[u8] = b"070701";
const MAGIC_CRC: &[u8] = b"070702";
const TRAILER_NAME: &str = "TRAILER!!!";

#[derive(Debug, Clone)]
pub struct CpioHeader {
    pub magic: [u8; 6],
    pub ino: u32,
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub nlink: u32,
    pub mtime: u32,
    pub filesize: u32,
    pub devmajor: u32,
    pub devminor: u32,
    pub rdevmajor: u32,
    pub rdevminor: u32,
    pub namesize: u32,
    pub check: u32,
}

impl CpioHeader {
    pub fn from_bytes(data: &[u8]) -> Result<Self, &'static str> {
        if data.len() < NEWC_HEADER_SIZE {
            return Err("cpio header too short");
        }

        let mut magic = [0u8; 6];
        magic.copy_from_slice(&data[0..6]);

        if &magic != MAGIC_NEWC && &magic != MAGIC_CRC {
            return Err("invalid cpio magic");
        }

        Ok(CpioHeader {
            magic,
            ino: parse_hex_field(&data[6..14])?,
            mode: parse_hex_field(&data[14..22])?,
            uid: parse_hex_field(&data[22..30])?,
            gid: parse_hex_field(&data[30..38])?,
            nlink: parse_hex_field(&data[38..46])?,
            mtime: parse_hex_field(&data[46..54])?,
            filesize: parse_hex_field(&data[54..62])?,
            devmajor: parse_hex_field(&data[62..70])?,
            devminor: parse_hex_field(&data[70..78])?,
            rdevmajor: parse_hex_field(&data[78..86])?,
            rdevminor: parse_hex_field(&data[86..94])?,
            namesize: parse_hex_field(&data[94..102])?,
            check: parse_hex_field(&data[102..110])?,
        })
    }

    pub fn magic(&self) -> &[u8; 6] {
        &self.magic
    }

    pub fn is_crc(&self) -> bool {
        &self.magic == MAGIC_CRC
    }

    pub fn name(&self, name_data: &[u8]) -> String {
        let end = name_data
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(name_data.len());
        String::from_utf8_lossy(&name_data[..end]).to_string()
    }

    pub fn size(&self) -> u32 {
        self.filesize
    }

    pub fn is_trailer(&self, name_data: &[u8]) -> bool {
        let name = self.name(name_data);
        name == TRAILER_NAME
    }

    pub fn mode(&self) -> u32 {
        self.mode
    }
}

#[derive(Debug, Clone)]
pub struct CpioEntry {
    pub header: CpioHeader,
    pub name: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct CpioArchive {
    pub entries: Vec<CpioEntry>,
}

impl CpioArchive {
    pub fn parse(data: &[u8]) -> Result<Self, &'static str> {
        let mut entries = Vec::new();
        let mut offset = 0;

        while offset + NEWC_HEADER_SIZE <= data.len() {
            let header = CpioHeader::from_bytes(&data[offset..])?;

            let name_start = offset + NEWC_HEADER_SIZE;
            let name_len = header.namesize as usize;

            if name_start + name_len > data.len() {
                return Err("cpio archive truncated at name");
            }

            let name_bytes = &data[name_start..name_start + name_len];

            if header.is_trailer(name_bytes) {
                break;
            }

            let name = header.name(name_bytes);

            let name_padded = align_to_4(name_len);
            let data_start = name_start + name_padded;
            let file_size = header.filesize as usize;

            if data_start + file_size > data.len() {
                return Err("cpio archive truncated at data");
            }

            let mut file_data = Vec::with_capacity(file_size);
            file_data.extend_from_slice(&data[data_start..data_start + file_size]);

            entries.push(CpioEntry {
                header,
                name,
                data: file_data,
            });

            let data_padded = align_to_4(file_size);
            offset = data_start + data_padded;
        }

        Ok(CpioArchive { entries })
    }

    pub fn entries(&self) -> &[CpioEntry] {
        &self.entries
    }

    pub fn find(&self, name: &str) -> Option<&CpioEntry> {
        self.entries.iter().find(|e| e.name == name)
    }
}

fn parse_hex_field(bytes: &[u8]) -> Result<u32, &'static str> {
    let s = core::str::from_utf8(bytes).map_err(|_| "cpio hex field not valid utf8")?;
    u32::from_str_radix(s.trim(), 16).map_err(|_| "cpio hex field parse failed")
}

fn align_to_4(len: usize) -> usize {
    (len + 3) & !3
}

lazy_static! {
    static ref CPIO_INIT: Mutex<bool> = Mutex::new(false);
}

pub fn init() {
    let mut guard = CPIO_INIT.lock();
    if *guard {
        return;
    }
    *guard = true;
}
