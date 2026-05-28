//! POSIX ustar tar archive parser with Pax extended header support.
//!
//! Format layout (512-byte header blocks):
//!   Name     100 bytes
//!   Mode       8 bytes (octal)
//!   UID        8 bytes (octal)
//!   GID        8 bytes (octal)
//!   Size      12 bytes (octal)
//!   Mtime     12 bytes (octal)
//!   Checksum   8 bytes (octal + space + null)
//!   Typeflag   1 byte
//!   Linkname 100 bytes
//!   Magic      6 bytes ("ustar\0")
//!   Version    2 bytes ("00")
//!   Uname     32 bytes
//!   Gname     32 bytes
//!   Devmajor   8 bytes
//!   Devminor   8 bytes
//!   Prefix   155 bytes
//!   Padding   12 bytes

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use lazy_static::lazy_static;
use spin::Mutex;

const BLOCK_SIZE: usize = 512;
const USTAR_MAGIC: &[u8] = b"ustar";

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct RawTarHeader {
    name: [u8; 100],
    mode: [u8; 8],
    uid: [u8; 8],
    gid: [u8; 8],
    size: [u8; 12],
    mtime: [u8; 12],
    checksum: [u8; 8],
    typeflag: u8,
    linkname: [u8; 100],
    magic: [u8; 6],
    version: [u8; 2],
    uname: [u8; 32],
    gname: [u8; 32],
    devmajor: [u8; 8],
    devminor: [u8; 8],
    prefix: [u8; 155],
    _padding: [u8; 12],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TarType {
    Regular,
    HardLink,
    SymLink,
    CharDevice,
    BlockDevice,
    Directory,
    Fifo,
    GlobalPax,
    PaxExtended,
    Unknown(u8),
}

#[derive(Debug, Clone)]
pub struct TarHeader {
    pub name_raw: [u8; 100],
    pub prefix_raw: [u8; 155],
    pub mode: u64,
    pub uid: u64,
    pub gid: u64,
    pub size: u64,
    pub mtime: u64,
    pub checksum: u64,
    pub typeflag: u8,
    pub linkname: [u8; 100],
    pub uname: String,
    pub gname: String,
    pub devmajor: u64,
    pub devminor: u64,
}

impl TarHeader {
    pub fn from_bytes(data: &[u8]) -> Result<Self, &'static str> {
        if data.len() < BLOCK_SIZE {
            return Err("tar header too short");
        }
        let raw: &RawTarHeader = unsafe { &*(data.as_ptr() as *const RawTarHeader) };

        if &raw.magic[..5] != USTAR_MAGIC {
            return Err("not a ustar archive");
        }

        let mut name_raw = [0u8; 100];
        name_raw.copy_from_slice(&raw.name);
        let mut prefix_raw = [0u8; 155];
        prefix_raw.copy_from_slice(&raw.prefix);
        let mut linkname = [0u8; 100];
        linkname.copy_from_slice(&raw.linkname);

        Ok(TarHeader {
            name_raw,
            prefix_raw,
            mode: parse_octal(&raw.mode)?,
            uid: parse_octal(&raw.uid)?,
            gid: parse_octal(&raw.gid)?,
            size: parse_octal(&raw.size)?,
            mtime: parse_octal(&raw.mtime)?,
            checksum: parse_octal(&raw.checksum)?,
            typeflag: raw.typeflag,
            linkname,
            uname: null_terminated_str(&raw.uname),
            gname: null_terminated_str(&raw.gname),
            devmajor: parse_octal(&raw.devmajor)?,
            devminor: parse_octal(&raw.devminor)?,
        })
    }

    pub fn verify_checksum(&self, data: &[u8]) -> bool {
        if data.len() < BLOCK_SIZE {
            return false;
        }
        let mut sum: u64 = 0;
        for i in 0..BLOCK_SIZE {
            if i >= 148 && i < 156 {
                sum += b' ' as u64;
            } else {
                sum += data[i] as u64;
            }
        }
        sum == self.checksum
    }

    pub fn name(&self) -> String {
        let prefix = null_terminated_str(&self.prefix_raw);
        let name = null_terminated_str(&self.name_raw);
        if prefix.is_empty() {
            name
        } else {
            alloc::format!("{}/{}", prefix, name)
        }
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn is_dir(&self) -> bool {
        self.typeflag == b'5'
    }

    pub fn is_symlink(&self) -> bool {
        self.typeflag == b'2'
    }

    pub fn typeflag(&self) -> TarType {
        match self.typeflag {
            b'0' | 0 => TarType::Regular,
            b'1' => TarType::HardLink,
            b'2' => TarType::SymLink,
            b'3' => TarType::CharDevice,
            b'4' => TarType::BlockDevice,
            b'5' => TarType::Directory,
            b'6' => TarType::Fifo,
            b'g' => TarType::GlobalPax,
            b'x' => TarType::PaxExtended,
            other => TarType::Unknown(other),
        }
    }

    fn is_empty_block(data: &[u8]) -> bool {
        data.iter().all(|&b| b == 0)
    }
}

#[derive(Debug, Clone)]
pub struct TarEntry {
    pub header: TarHeader,
    pub data: Vec<u8>,
    pub pax_headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct TarArchive {
    pub entries: Vec<TarEntry>,
}

impl TarArchive {
    pub fn parse(data: &[u8]) -> Result<Self, &'static str> {
        let mut entries = Vec::new();
        let mut offset = 0;
        let mut pending_pax: Option<BTreeMap<String, String>> = None;
        let mut empty_blocks = 0;

        while offset + BLOCK_SIZE <= data.len() {
            let block = &data[offset..offset + BLOCK_SIZE];

            if TarHeader::is_empty_block(block) {
                empty_blocks += 1;
                if empty_blocks >= 2 {
                    break;
                }
                offset += BLOCK_SIZE;
                continue;
            }

            empty_blocks = 0;
            let header = TarHeader::from_bytes(block)?;

            if !header.verify_checksum(block) {
                return Err("tar header checksum mismatch");
            }

            let data_size = header.size() as usize;
            let data_blocks = (data_size + BLOCK_SIZE - 1) / BLOCK_SIZE;
            let data_end = offset + BLOCK_SIZE + data_blocks * BLOCK_SIZE;

            if data_end > data.len() {
                return Err("tar archive truncated");
            }

            let file_data = &data[offset + BLOCK_SIZE..offset + BLOCK_SIZE + data_size];

            match header.typeflag() {
                TarType::PaxExtended => {
                    let pax = parse_pax_records(file_data)?;
                    pending_pax = Some(pax);
                }
                TarType::GlobalPax => {
                    let _pax = parse_pax_records(file_data)?;
                }
                _ => {
                    let mut entry_data = Vec::with_capacity(data_size);
                    entry_data.extend_from_slice(file_data);

                    let mut pax = BTreeMap::new();
                    if let Some(prev_pax) = pending_pax.take() {
                        pax = prev_pax;
                    }

                    if let Some(override_name) = pax.get("path") {
                        let name_bytes = override_name.as_bytes();
                        let mut name_raw = [0u8; 100];
                        let copy_len = name_bytes.len().min(100);
                        name_raw[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
                    }

                    entries.push(TarEntry {
                        header,
                        data: entry_data,
                        pax_headers: pax,
                    });
                }
            }

            offset += BLOCK_SIZE + data_blocks * BLOCK_SIZE;
        }

        Ok(TarArchive { entries })
    }

    pub fn entries(&self) -> &[TarEntry] {
        &self.entries
    }

    pub fn find(&self, name: &str) -> Option<&TarEntry> {
        self.entries.iter().find(|e| e.header.name() == name)
    }
}

fn parse_octal(bytes: &[u8]) -> Result<u64, &'static str> {
    let mut val: u64 = 0;
    let mut started = false;
    for &b in bytes {
        if b == 0 || b == b' ' {
            if started {
                break;
            }
            continue;
        }
        if b >= b'0' && b <= b'7' {
            val = val * 8 + (b - b'0') as u64;
            started = true;
        } else if started {
            break;
        }
    }
    Ok(val)
}

fn null_terminated_str(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

fn parse_pax_records(data: &[u8]) -> Result<BTreeMap<String, String>, &'static str> {
    let mut records = BTreeMap::new();
    let mut offset = 0;

    while offset < data.len() {
        let len_start = offset;
        let mut space_pos = None;
        for i in offset..data.len() {
            if data[i] == b' ' {
                space_pos = Some(i);
                break;
            }
        }
        let space_pos = space_pos.ok_or("pax record missing length delimiter")?;
        let len_str = core::str::from_utf8(&data[len_start..space_pos])
            .map_err(|_| "pax length not valid utf8")?;
        let record_len: usize = len_str.parse().map_err(|_| "pax length parse failed")?;

        if record_len == 0 || offset + record_len > data.len() {
            break;
        }

        let record = &data[offset + (space_pos - offset + 1)..offset + record_len - 1];
        if let Some(eq_pos) = record.iter().position(|&b| b == b'=') {
            let key = String::from_utf8_lossy(&record[..eq_pos]).to_string();
            let value = String::from_utf8_lossy(&record[eq_pos + 1..]).to_string();
            records.insert(key, value);
        }

        offset += record_len;
    }

    Ok(records)
}

lazy_static! {
    static ref TAR_INIT: Mutex<bool> = Mutex::new(false);
}

pub fn init() {
    let mut guard = TAR_INIT.lock();
    if *guard {
        return;
    }
    *guard = true;
}
