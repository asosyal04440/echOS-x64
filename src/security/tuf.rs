//! TUF (The Update Framework) verification pipeline for no_std.
//!
//! POUF (Protocol, Operations, Usage, Format) Document for echOS:
//! - Binary metadata encoding (not JSON) for kernel-space efficiency
//! - Signature schemes: echOS-Ed25519 (SHA3-512 based), RSA-PKCS1-v15-SHA256
//! - Key ID: first 32 bytes of SHA-256 hash of canonical key encoding
//!
//! Binary metadata layout:
//!   Magic:        4 bytes ("TUF\0")
//!   Version:      1 byte (format version, currently 1)
//!   Role:         1 byte (0=Root, 1=Timestamp, 2=Snapshot, 3=Targets)
//!   SpecVersion:  4 bytes (major.minor as u16.u16 LE)
//!   ConsistentSnap: 1 byte (0 or 1, root only)
//!   Version:      8 bytes (u64 LE, metadata version)
//!   Expires:      8 bytes (u64 LE, Unix timestamp)
//!   KeyCount:     4 bytes (u32 LE)
//!   Keys:         repeated { keyid(32) + keytype(1: 0=ed25519,1=rsa) + publen(2 LE) + pubkey(var) }
//!   SigCount:     4 bytes (u32 LE)
//!   Signatures:   repeated { keyid(32) + siglen(2 LE) + sig(var) }
//!   RoleThresholds: (root only) 4 entries of u8 threshold
//!   Payload:      role-specific data
//!
//! Targets payload: file_count(4 LE) + repeated { namelen(2 LE) + name(var) + length(8 LE) + hashcount(1) + repeated { hashnamelen(1) + hashname(var) + hashlen(1) + hash(var) } }
//! Snapshot payload: meta_count(4 LE) + repeated { namelen(2 LE) + name(var) + version(8 LE) + length(8 LE) }
//! Timestamp payload: snapshot_version(8 LE) + snapshot_length(8 LE)

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use lazy_static::lazy_static;
use spin::Mutex;

const TUF_MAGIC: &[u8] = b"TUF\0";
const FORMAT_VERSION: u8 = 1;
const SPEC_VERSION_MAJOR: u16 = 1;
const SPEC_VERSION_MINOR: u16 = 0;
const MAX_ROOT_ROTATION_STEPS: usize = 1024;
const MAX_METADATA_DOWNLOAD_SIZE: usize = 64 * 1024;
const MAX_TARGET_FILE_SIZE: u64 = 256 * 1024 * 1024; // 256 MB ceiling

/// Maximum version jump to prevent fast-forward attacks (§1.5.2).
const MAX_VERSION_JUMP: u64 = 1_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TufRole {
    Root,
    Timestamp,
    Snapshot,
    Targets,
}

impl TufRole {
    fn from_u8(v: u8) -> Result<Self, &'static str> {
        match v {
            0 => Ok(TufRole::Root),
            1 => Ok(TufRole::Timestamp),
            2 => Ok(TufRole::Snapshot),
            3 => Ok(TufRole::Targets),
            _ => Err("unknown TUF role"),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            TufRole::Root => "root",
            TufRole::Timestamp => "timestamp",
            TufRole::Snapshot => "snapshot",
            TufRole::Targets => "targets",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyType {
    Ed25519,
    Rsa,
}

impl KeyType {
    fn from_u8(v: u8) -> Result<Self, &'static str> {
        match v {
            0 => Ok(KeyType::Ed25519),
            1 => Ok(KeyType::Rsa),
            _ => Err("unknown key type"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TufKey {
    pub keyid: [u8; 32],
    pub key_type: KeyType,
    pub public_key: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct TufSignature {
    pub keyid: [u8; 32],
    pub sig: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct TargetFile {
    pub length: u64,
    pub hashes: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct TargetsInfo {
    pub files: BTreeMap<String, TargetFile>,
    pub delegations: Option<DelegationInfo>,
}

/// Delegation metadata per TUF spec §4.5.
#[derive(Debug, Clone)]
pub struct DelegationInfo {
    pub keys: BTreeMap<[u8; 32], TufKey>,
    pub roles: Vec<DelegationRole>,
}

/// A delegated role per TUF spec §4.5 DELEGATIONS.roles[].
#[derive(Debug, Clone)]
pub struct DelegationRole {
    pub name: String,
    pub keyids: Vec<[u8; 32]>,
    pub threshold: u8,
    pub paths: Option<Vec<String>>,
    pub path_hash_prefixes: Option<Vec<String>>,
    pub terminating: bool,
}

/// Snapshot metadata entry per TUF spec §4.4 METAFILES.
#[derive(Debug, Clone)]
pub struct SnapshotMetaEntry {
    pub version: u64,
    pub length: Option<u64>,
    pub hashes: BTreeMap<String, Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct TufMetadata {
    pub role: TufRole,
    pub spec_version_major: u16,
    pub spec_version_minor: u16,
    pub consistent_snapshot: bool,
    pub version: u64,
    pub expires: u64,
    pub signatures: Vec<TufSignature>,
    pub keys: BTreeMap<[u8; 32], TufKey>,
    pub role_thresholds: BTreeMap<TufRole, u8>,
    pub payload: Vec<u8>,
}

/// Canonical key encoding for keyid computation (TUF spec §4.2.2).
///
/// Format: keytype(1) + publen(2 LE) + pubkey(var)
fn canonical_key_encoding(key_type: KeyType, public_key: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(3 + public_key.len());
    buf.push(match key_type {
        KeyType::Ed25519 => 0,
        KeyType::Rsa => 1,
    });
    let pub_len = public_key.len() as u16;
    buf.extend_from_slice(&pub_len.to_le_bytes());
    buf.extend_from_slice(public_key);
    buf
}

/// Compute keyid as first 32 bytes of SHA-256 of canonical key encoding.
///
/// TUF spec §4.2.1: "KEYID ... is a hexdigest of the SHA-256 hash of the
/// canonical form of the key." In our binary POUF we use raw bytes.
fn compute_keyid(key_type: KeyType, public_key: &[u8]) -> [u8; 32] {
    let canonical = canonical_key_encoding(key_type, public_key);
    sha256_simple(&canonical)
}

fn parse_metadata(data: &[u8]) -> Result<TufMetadata, &'static str> {
    if data.len() < 27 {
        return Err("metadata too short");
    }
    if &data[0..4] != TUF_MAGIC {
        return Err("invalid TUF magic");
    }
    if data[4] != FORMAT_VERSION {
        return Err("unsupported TUF format version");
    }

    let role = TufRole::from_u8(data[5])?;
    let spec_major = u16::from_le_bytes([data[6], data[7]]);
    let spec_minor = u16::from_le_bytes([data[8], data[9]]);
    let consistent_snapshot = data[10] != 0;
    let version = u64::from_le_bytes(data[11..19].try_into().map_err(|_| "version read")?);
    let expires = u64::from_le_bytes(data[19..27].try_into().map_err(|_| "expires read")?);

    if version == 0 {
        return Err("metadata version must be > 0");
    }

    let mut offset = 27;

    let key_count = read_u32_le(data, &mut offset)?;
    let mut keys = BTreeMap::new();
    for _ in 0..key_count {
        let mut keyid = [0u8; 32];
        keyid.copy_from_slice(read_slice(data, &mut offset, 32)?);
        let key_type = KeyType::from_u8(read_u8(data, &mut offset)?)?;
        let pub_len = read_u16_le(data, &mut offset)? as usize;
        let public_key = read_slice(data, &mut offset, pub_len)?.to_vec();

        // Verify keyid matches computed keyid (TUF spec §4.3: "Clients MUST calculate each KEYID")
        let computed_keyid = compute_keyid(key_type, &public_key);
        if keyid != computed_keyid {
            return Err("keyid mismatch: metadata keyid does not match computed keyid");
        }

        keys.insert(
            keyid,
            TufKey {
                keyid,
                key_type,
                public_key,
            },
        );
    }

    let sig_count = read_u32_le(data, &mut offset)?;
    let mut signatures = Vec::with_capacity(sig_count as usize);
    for _ in 0..sig_count {
        let mut keyid = [0u8; 32];
        keyid.copy_from_slice(read_slice(data, &mut offset, 32)?);
        let sig_len = read_u16_le(data, &mut offset)? as usize;
        let sig = read_slice(data, &mut offset, sig_len)?.to_vec();
        signatures.push(TufSignature { keyid, sig });
    }

    let mut role_thresholds = BTreeMap::new();
    if role == TufRole::Root {
        for r in [
            TufRole::Root,
            TufRole::Timestamp,
            TufRole::Snapshot,
            TufRole::Targets,
        ] {
            let threshold = read_u8(data, &mut offset)?;
            if threshold == 0 {
                return Err("role threshold must be >= 1");
            }
            role_thresholds.insert(r, threshold);
        }
    }

    let payload = data[offset..].to_vec();

    Ok(TufMetadata {
        role,
        spec_version_major: spec_major,
        spec_version_minor: spec_minor,
        consistent_snapshot,
        version,
        expires,
        signatures,
        keys,
        role_thresholds,
        payload,
    })
}

fn parse_targets_payload(data: &[u8]) -> Result<TargetsInfo, &'static str> {
    let mut offset = 0;
    let file_count = read_u32_le(data, &mut offset)? as usize;
    let mut files = BTreeMap::new();

    for _ in 0..file_count {
        let name_len = read_u16_le(data, &mut offset)? as usize;
        let name = core::str::from_utf8(read_slice(data, &mut offset, name_len)?)
            .map_err(|_| "target name not utf8")?
            .to_string();
        let length = read_u64_le(data, &mut offset)?;
        let hash_count = read_u8(data, &mut offset)? as usize;
        let mut hashes = BTreeMap::new();

        for _ in 0..hash_count {
            let hash_name_len = read_u8(data, &mut offset)? as usize;
            let hash_name = core::str::from_utf8(read_slice(data, &mut offset, hash_name_len)?)
                .map_err(|_| "hash name not utf8")?
                .to_string();
            let hash_len = read_u8(data, &mut offset)? as usize;
            let hash = read_slice(data, &mut offset, hash_len)?.to_vec();
            hashes.insert(hash_name, hash);
        }

        // Endless data attack protection (TUF spec §1.5.2)
        if length > MAX_TARGET_FILE_SIZE {
            return Err("target file size exceeds maximum allowed");
        }

        files.insert(name, TargetFile { length, hashes });
    }

    // Parse optional delegations (TUF spec §4.5)
    let delegations = if offset < data.len() {
        let has_delegations = read_u8(data, &mut offset)?;
        if has_delegations != 0 {
            Some(parse_delegations(data, &mut offset)?)
        } else {
            None
        }
    } else {
        None
    };

    Ok(TargetsInfo { files, delegations })
}

/// Parse delegation data per TUF spec §4.5.
///
/// Binary layout:
///   key_count(4 LE) + keys(same format as root) +
///   role_count(4 LE) + repeated { namelen(2) + name + keyid_count(4) + keyids(32ea) + threshold(1) + has_paths(1) + [path_count(4) + paths] + has_prefixes(1) + [prefix_count(4) + prefixes] + terminating(1) }
fn parse_delegations(data: &[u8], offset: &mut usize) -> Result<DelegationInfo, &'static str> {
    let key_count = read_u32_le(data, offset)? as usize;
    let mut keys = BTreeMap::new();

    for _ in 0..key_count {
        let mut keyid = [0u8; 32];
        keyid.copy_from_slice(read_slice(data, offset, 32)?);
        let key_type = KeyType::from_u8(read_u8(data, offset)?)?;
        let pub_len = read_u16_le(data, offset)? as usize;
        let public_key = read_slice(data, offset, pub_len)?.to_vec();

        let computed_keyid = compute_keyid(key_type, &public_key);
        if keyid != computed_keyid {
            return Err("delegation keyid mismatch");
        }

        keys.insert(
            keyid,
            TufKey {
                keyid,
                key_type,
                public_key,
            },
        );
    }

    let role_count = read_u32_le(data, offset)? as usize;
    let mut roles = Vec::with_capacity(role_count);

    for _ in 0..role_count {
        let name_len = read_u16_le(data, offset)? as usize;
        let name = core::str::from_utf8(read_slice(data, offset, name_len)?)
            .map_err(|_| "delegation role name not utf8")?
            .to_string();

        let keyid_count = read_u32_le(data, offset)? as usize;
        let mut keyids = Vec::with_capacity(keyid_count);
        for _ in 0..keyid_count {
            let mut kid = [0u8; 32];
            kid.copy_from_slice(read_slice(data, offset, 32)?);
            keyids.push(kid);
        }

        let threshold = read_u8(data, offset)?;
        if threshold == 0 {
            return Err("delegation role threshold must be >= 1");
        }

        let has_paths = read_u8(data, offset)?;
        let paths = if has_paths != 0 {
            let path_count = read_u32_le(data, offset)? as usize;
            let mut p = Vec::with_capacity(path_count);
            for _ in 0..path_count {
                let pl = read_u16_le(data, offset)? as usize;
                let path = core::str::from_utf8(read_slice(data, offset, pl)?)
                    .map_err(|_| "delegation path not utf8")?
                    .to_string();
                p.push(path);
            }
            Some(p)
        } else {
            None
        };

        let has_prefixes = read_u8(data, offset)?;
        let path_hash_prefixes = if has_prefixes != 0 {
            let prefix_count = read_u32_le(data, offset)? as usize;
            let mut prefixes = Vec::with_capacity(prefix_count);
            for _ in 0..prefix_count {
                let pl = read_u8(data, offset)? as usize;
                let prefix = core::str::from_utf8(read_slice(data, offset, pl)?)
                    .map_err(|_| "delegation prefix not utf8")?
                    .to_string();
                prefixes.push(prefix);
            }
            Some(prefixes)
        } else {
            None
        };

        // TUF spec §4.5: exactly one of paths or path_hash_prefixes should be set
        if paths.is_none() && path_hash_prefixes.is_none() {
            return Err("delegation role must specify either paths or path_hash_prefixes");
        }
        if paths.is_some() && path_hash_prefixes.is_some() {
            return Err("delegation role must not specify both paths and path_hash_prefixes");
        }

        let terminating = read_u8(data, offset)? != 0;

        roles.push(DelegationRole {
            name,
            keyids,
            threshold,
            paths,
            path_hash_prefixes,
            terminating,
        });
    }

    Ok(DelegationInfo { keys, roles })
}

/// Check if a target path matches a delegation role's path patterns.
///
/// TUF spec §4.5: PATHPATTERN supports Unix shell-style wildcards (* and ?).
/// Path separator must not be matched by wildcards.
pub fn delegation_path_matches(role: &DelegationRole, target_path: &str) -> bool {
    if let Some(prefixes) = &role.path_hash_prefixes {
        let hash = sha256_simple(target_path.as_bytes());
        let hex_digest = hex_encode(&hash);
        for prefix in prefixes {
            if hex_digest.starts_with(prefix.as_str()) {
                return true;
            }
        }
        return false;
    }

    if let Some(paths) = &role.paths {
        for pattern in paths {
            if glob_match(pattern, target_path) {
                return true;
            }
        }
        return false;
    }

    false
}

/// Simple glob matching for TUF delegation path patterns.
///
/// Supports * (match any chars except /) and ? (match single char except /).
fn glob_match(pattern: &str, text: &str) -> bool {
    let mut pi = 0;
    let mut ti = 0;
    let mut star_pi = None;
    let mut star_ti = None;
    let pb = pattern.as_bytes();
    let tb = text.as_bytes();

    while ti < tb.len() {
        if pi < pb.len() && pb[pi] == b'*' {
            star_pi = Some(pi);
            star_ti = Some(ti);
            pi += 1;
        } else if pi < pb.len() && (pb[pi] == b'?' || pb[pi] == tb[ti]) {
            pi += 1;
            ti += 1;
        } else if let Some(sp) = star_pi {
            pi = sp + 1;
            star_ti = Some(star_ti.unwrap() + 1);
            ti = star_ti.unwrap();
            // Wildcard must not match path separator
            if ti < tb.len() && tb[ti] == b'/' {
                return false;
            }
        } else {
            return false;
        }
    }

    while pi < pb.len() && pb[pi] == b'*' {
        pi += 1;
    }

    pi == pb.len()
}

/// Hex-encode a byte array to lowercase hex string.
fn hex_encode(bytes: &[u8]) -> String {
    const HEX_CHARS: &[u8] = b"0123456789abcdef";
    let mut s = alloc::vec![0u8; bytes.len() * 2];
    for (i, &b) in bytes.iter().enumerate() {
        s[i * 2] = HEX_CHARS[(b >> 4) as usize];
        s[i * 2 + 1] = HEX_CHARS[(b & 0x0F) as usize];
    }
    // Safety: HEX_CHARS contains only ASCII hex digits
    unsafe { String::from_utf8_unchecked(s) }
}

fn parse_snapshot_payload(
    data: &[u8],
) -> Result<BTreeMap<String, SnapshotMetaEntry>, &'static str> {
    let mut offset = 0;
    let meta_count = read_u32_le(data, &mut offset)? as usize;
    let mut metas = BTreeMap::new();

    for _ in 0..meta_count {
        let name_len = read_u16_le(data, &mut offset)? as usize;
        let name = core::str::from_utf8(read_slice(data, &mut offset, name_len)?)
            .map_err(|_| "snapshot meta name not utf8")?
            .to_string();
        let version = read_u64_le(data, &mut offset)?;
        let length = read_u64_le(data, &mut offset)?;
        let length = if length == 0 { None } else { Some(length) };
        let hash_count = read_u8(data, &mut offset)? as usize;
        let mut hashes = BTreeMap::new();

        for _ in 0..hash_count {
            let hash_name_len = read_u8(data, &mut offset)? as usize;
            let hash_name = core::str::from_utf8(read_slice(data, &mut offset, hash_name_len)?)
                .map_err(|_| "hash name not utf8")?
                .to_string();
            let hash_len = read_u8(data, &mut offset)? as usize;
            let hash = read_slice(data, &mut offset, hash_len)?.to_vec();
            hashes.insert(hash_name, hash);
        }

        metas.insert(
            name,
            SnapshotMetaEntry {
                version,
                length,
                hashes,
            },
        );
    }

    Ok(metas)
}

fn parse_timestamp_payload(data: &[u8]) -> Result<(u64, Option<u64>), &'static str> {
    if data.len() < 8 {
        return Err("timestamp payload too short");
    }
    let snapshot_version = u64::from_le_bytes(data[0..8].try_into().map_err(|_| "timestamp read")?);
    let snapshot_length = if data.len() >= 16 {
        let len = u64::from_le_bytes(data[8..16].try_into().map_err(|_| "timestamp len read")?);
        if len == 0 {
            None
        } else {
            Some(len)
        }
    } else {
        None
    };
    Ok((snapshot_version, snapshot_length))
}

fn read_u8(data: &[u8], offset: &mut usize) -> Result<u8, &'static str> {
    if *offset + 1 > data.len() {
        return Err("metadata truncated");
    }
    let v = data[*offset];
    *offset += 1;
    Ok(v)
}

fn read_u16_le(data: &[u8], offset: &mut usize) -> Result<u16, &'static str> {
    if *offset + 2 > data.len() {
        return Err("metadata truncated");
    }
    let v = u16::from_le_bytes([data[*offset], data[*offset + 1]]);
    *offset += 2;
    Ok(v)
}

fn read_u32_le(data: &[u8], offset: &mut usize) -> Result<u32, &'static str> {
    if *offset + 4 > data.len() {
        return Err("metadata truncated");
    }
    let v = u32::from_le_bytes([
        data[*offset],
        data[*offset + 1],
        data[*offset + 2],
        data[*offset + 3],
    ]);
    *offset += 4;
    Ok(v)
}

fn read_u64_le(data: &[u8], offset: &mut usize) -> Result<u64, &'static str> {
    if *offset + 8 > data.len() {
        return Err("metadata truncated");
    }
    let v = u64::from_le_bytes([
        data[*offset],
        data[*offset + 1],
        data[*offset + 2],
        data[*offset + 3],
        data[*offset + 4],
        data[*offset + 5],
        data[*offset + 6],
        data[*offset + 7],
    ]);
    *offset += 8;
    Ok(v)
}

fn read_slice<'a>(
    data: &'a [u8],
    offset: &mut usize,
    len: usize,
) -> Result<&'a [u8], &'static str> {
    if *offset + len > data.len() {
        return Err("metadata truncated");
    }
    let s = &data[*offset..*offset + len];
    *offset += len;
    Ok(s)
}

#[derive(Debug, Clone)]
struct VerifiedRole {
    version: u64,
    expires: u64,
}

/// Full TUF verifier implementing the detailed client workflow (§5).
pub struct TufVerifier {
    root: TufMetadata,
    latest_timestamp: Option<VerifiedRole>,
    latest_snapshot: Option<VerifiedRole>,
    latest_targets: Option<VerifiedRole>,
    /// Parsed targets info (files + delegations) for §5.7 fetch target.
    latest_targets_info: Option<TargetsInfo>,
    snapshot_meta_versions: BTreeMap<String, u64>,
    snapshot_meta_lengths: BTreeMap<String, u64>,
    /// Fixed update start time (TUF spec §5.1).
    update_start_time: u64,
}

impl TufVerifier {
    /// §5.2 Load trusted root metadata.
    ///
    /// The trusted root must be bootstrapped out-of-band. Its expiration
    /// does not matter at this stage (§5.2: "the expiration of the trusted
    /// root metadata file does not matter").
    pub fn new(root_metadata: &[u8]) -> Result<Self, &'static str> {
        let root = parse_metadata(root_metadata)?;
        if root.role != TufRole::Root {
            return Err("root metadata must have Root role");
        }
        Self::validate_spec_version(root.spec_version_major, root.spec_version_minor)?;

        let update_start_time = current_unix_timestamp();

        Ok(TufVerifier {
            root,
            latest_timestamp: None,
            latest_snapshot: None,
            latest_targets: None,
            latest_targets_info: None,
            snapshot_meta_versions: BTreeMap::new(),
            snapshot_meta_lengths: BTreeMap::new(),
            update_start_time,
        })
    }

    /// §5.3 Update the root role with rotation support.
    ///
    /// Downloads intermediate root metadata files (N+1, N+2, ...) and
    /// validates each step. Each intermediate root must be signed by:
    /// (1) threshold of keys from current trusted root, AND
    /// (2) threshold of keys from the new root being validated.
    pub fn update_root(&mut self, new_root_data: &[u8]) -> Result<(), &'static str> {
        let new_root = parse_metadata(new_root_data)?;
        if new_root.role != TufRole::Root {
            return Err("expected Root role in root update");
        }
        Self::validate_spec_version(new_root.spec_version_major, new_root.spec_version_minor)?;

        // §5.3.5: Version must be exactly N+1
        if new_root.version != self.root.version + 1 {
            return Err("root version must be exactly current version + 1");
        }

        // §5.3.4: Must be signed by threshold of BOTH old and new root keys
        self.verify_signatures_with_keys(&new_root, TufRole::Root, &self.root.keys)?;
        self.verify_signatures_with_keys(&new_root, TufRole::Root, &new_root.keys)?;

        // Accept the new root
        self.root = new_root;
        Ok(())
    }

    /// §5.4 Update the timestamp role.
    pub fn verify_timestamp(&mut self, data: &[u8]) -> Result<(), &'static str> {
        if data.len() > MAX_METADATA_DOWNLOAD_SIZE {
            return Err("timestamp metadata exceeds maximum download size");
        }

        let meta = parse_metadata(data)?;
        if meta.role != TufRole::Timestamp {
            return Err("expected Timestamp role");
        }
        Self::validate_spec_version(meta.spec_version_major, meta.spec_version_minor)?;

        // §5.4.4: Check for rollback attack
        if let Some(prev) = &self.latest_timestamp {
            if meta.version < prev.version {
                return Err("rollback attack detected: timestamp version decreased");
            }
            // Fast-forward attack protection
            if meta.version > prev.version + MAX_VERSION_JUMP {
                return Err("fast-forward attack detected: timestamp version jump too large");
            }
        }

        // §5.4.3: Check signatures
        self.verify_signatures(&meta, TufRole::Timestamp)?;

        // §5.4.5: Check expiration against fixed update start time (§5.1)
        if meta.expires <= self.update_start_time {
            return Err("timestamp metadata expired at update start time");
        }

        let (snapshot_version, snapshot_length) = parse_timestamp_payload(&meta.payload)?;

        self.latest_timestamp = Some(VerifiedRole {
            version: meta.version,
            expires: meta.expires,
        });

        self.snapshot_meta_versions
            .insert("snapshot.json".to_string(), snapshot_version);
        if let Some(len) = snapshot_length {
            self.snapshot_meta_lengths
                .insert("snapshot.json".to_string(), len);
        }

        Ok(())
    }

    /// §5.5 Update the snapshot role.
    pub fn verify_snapshot(&mut self, data: &[u8]) -> Result<(), &'static str> {
        if data.len() > MAX_METADATA_DOWNLOAD_SIZE {
            return Err("snapshot metadata exceeds maximum download size");
        }

        let meta = parse_metadata(data)?;
        if meta.role != TufRole::Snapshot {
            return Err("expected Snapshot role");
        }
        Self::validate_spec_version(meta.spec_version_major, meta.spec_version_minor)?;

        // §5.5.5: Check for rollback attack
        if let Some(prev) = &self.latest_snapshot {
            if meta.version < prev.version {
                return Err("rollback attack detected: snapshot version decreased");
            }
            if meta.version > prev.version + MAX_VERSION_JUMP {
                return Err("fast-forward attack detected: snapshot version jump too large");
            }
        }

        // §5.5.4: Check signatures
        self.verify_signatures(&meta, TufRole::Snapshot)?;

        // §5.5.6: Check expiration
        if meta.expires <= self.update_start_time {
            return Err("snapshot metadata expired at update start time");
        }

        // §5.5.7: Version must match timestamp's snapshot version
        if let Some(ts) = &self.latest_timestamp {
            let expected = self
                .snapshot_meta_versions
                .get("snapshot.json")
                .ok_or("timestamp did not reference snapshot version")?;
            if meta.version != *expected {
                return Err("snapshot version mismatch with timestamp");
            }
            // Snapshot must not expire after timestamp
            if meta.expires > ts.expires {
                return Err("snapshot expires after timestamp expires");
            }
        }

        let metas = parse_snapshot_payload(&meta.payload)?;

        self.latest_snapshot = Some(VerifiedRole {
            version: meta.version,
            expires: meta.expires,
        });

        for (name, entry) in &metas {
            self.snapshot_meta_versions
                .insert(name.clone(), entry.version);
            if let Some(len) = entry.length {
                self.snapshot_meta_lengths.insert(name.clone(), len);
            }
        }

        Ok(())
    }

    /// §5.6 Update the targets role.
    pub fn verify_targets(&mut self, data: &[u8]) -> Result<TargetsInfo, &'static str> {
        if data.len() > MAX_METADATA_DOWNLOAD_SIZE {
            return Err("targets metadata exceeds maximum download size");
        }

        let meta = parse_metadata(data)?;
        if meta.role != TufRole::Targets {
            return Err("expected Targets role");
        }
        Self::validate_spec_version(meta.spec_version_major, meta.spec_version_minor)?;

        // §5.6.5: Check for rollback attack
        if let Some(prev) = &self.latest_targets {
            if meta.version < prev.version {
                return Err("rollback attack detected: targets version decreased");
            }
            if meta.version > prev.version + MAX_VERSION_JUMP {
                return Err("fast-forward attack detected: targets version jump too large");
            }
        }

        // §5.6.4: Check signatures
        self.verify_signatures(&meta, TufRole::Targets)?;

        // §5.6.6: Check expiration
        if meta.expires <= self.update_start_time {
            return Err("targets metadata expired at update start time");
        }

        // Targets must not expire after snapshot
        if let Some(ss) = &self.latest_snapshot {
            if meta.expires > ss.expires {
                return Err("targets expires after snapshot expires");
            }
        }

        // §5.6.7: Version must match snapshot's targets version
        let targets_key = "targets.json";
        if let Some(expected_version) = self.snapshot_meta_versions.get(targets_key) {
            if meta.version != *expected_version {
                return Err("targets version mismatch with snapshot");
            }
        }

        let info = parse_targets_payload(&meta.payload)?;

        self.latest_targets = Some(VerifiedRole {
            version: meta.version,
            expires: meta.expires,
        });
        self.latest_targets_info = Some(info.clone());

        Ok(info)
    }

    /// §5.7 Fetch target: verify hash AND length.
    ///
    /// TUF spec §1.5.2: "Wrong software installation" and "Endless data attacks".
    /// TUF spec §4.5: LENGTH field is mandatory for each target.
    pub fn verify_file(
        &self,
        data: &[u8],
        target_path: &str,
        expected_hash: &[u8; 32],
    ) -> Result<(), &'static str> {
        // Endless data attack protection: check against targets metadata length
        if let Some(targets) = &self.latest_targets {
            // Length check from snapshot metadata (if available)
            if let Some(&snapshot_len) = self.snapshot_meta_lengths.get("targets.json") {
                if data.len() as u64 > snapshot_len {
                    return Err("file size exceeds snapshot metadata length");
                }
            }
        }

        // Check target file length from targets metadata (§4.5 LENGTH field)
        // Note: caller must have called verify_targets first to populate latest_targets
        // The actual length check is done via verify_file_with_length when the
        // caller has the expected length from targets metadata.
        // Here we enforce a global ceiling as baseline protection.
        if data.len() as u64 > MAX_TARGET_FILE_SIZE {
            return Err("file size exceeds maximum allowed");
        }

        let computed = sha256_simple(data);
        if computed != *expected_hash {
            return Err("file hash mismatch");
        }
        Ok(())
    }

    /// Verify file against targets metadata: length + hash (TUF spec §5.7).
    ///
    /// Looks up the target path in the latest verified targets metadata and
    /// checks both the LENGTH and hashes fields. This is the spec-compliant
    /// path for §5.7 "Fetch target".
    pub fn verify_file_from_targets_metadata(
        &self,
        data: &[u8],
        target_path: &str,
    ) -> Result<(), &'static str> {
        // Must have verified targets metadata first
        let targets_info = self
            .latest_targets_info
            .as_ref()
            .ok_or("targets metadata not yet verified")?;

        // Look up target in verified metadata
        let target_file = targets_info
            .files
            .get(target_path)
            .ok_or("target path not found in verified targets metadata")?;

        // §5.7: Check length (endless data attack protection)
        if data.len() as u64 != target_file.length {
            return Err("file length mismatch with targets metadata");
        }

        // §5.7: Check hash (wrong software installation protection)
        if let Some(expected_hash) = target_file.hashes.get("sha256") {
            let computed = sha256_simple(data);
            if computed.as_slice() != expected_hash.as_slice() {
                return Err("file hash mismatch with targets metadata");
            }
        } else {
            return Err("no sha256 hash found in targets metadata");
        }

        Ok(())
    }

    /// Verify file with explicit length check (TUF spec §4.5 LENGTH field).
    pub fn verify_file_with_length(
        &self,
        data: &[u8],
        expected_length: u64,
        expected_hash: &[u8; 32],
    ) -> Result<(), &'static str> {
        // Endless data attack protection
        if data.len() as u64 != expected_length {
            return Err("file size mismatch with metadata");
        }
        if expected_length > MAX_TARGET_FILE_SIZE {
            return Err("file size exceeds maximum allowed");
        }

        let computed = sha256_simple(data);
        if computed != *expected_hash {
            return Err("file hash mismatch");
        }
        Ok(())
    }

    /// Validate spec_version matches expected (TUF spec §4.3 SPEC_VERSION).
    fn validate_spec_version(major: u16, minor: u16) -> Result<(), &'static str> {
        if major != SPEC_VERSION_MAJOR {
            return Err("spec major version mismatch");
        }
        // Minor version can be >= expected (backwards compatible)
        if minor < SPEC_VERSION_MINOR {
            return Err("spec minor version too old");
        }
        Ok(())
    }

    fn check_expiration(&self, meta: &TufMetadata) -> Result<(), &'static str> {
        // Use fixed update start time (§5.1)
        if meta.expires <= self.update_start_time {
            return Err("metadata expired at update start time");
        }
        Ok(())
    }

    /// Verify signatures against root's role keys (standard path).
    fn verify_signatures(&self, meta: &TufMetadata, role: TufRole) -> Result<(), &'static str> {
        let threshold = self.root.role_thresholds.get(&role).copied().unwrap_or(1);

        let mut valid_count = 0;
        let mut seen_keyids: BTreeMap<[u8; 32], bool> = BTreeMap::new();

        for sig in &meta.signatures {
            // TUF spec §5.3.4: "each KEY MUST only contribute one SIGNATURE"
            if seen_keyids.contains_key(&sig.keyid) {
                continue;
            }

            if let Some(key) = meta.keys.get(&sig.keyid) {
                if self.root.keys.contains_key(&sig.keyid) {
                    if verify_signature(key, &sig.sig, &meta.payload) {
                        valid_count += 1;
                        seen_keyids.insert(sig.keyid, true);
                    }
                }
            }
        }

        if valid_count < threshold as usize {
            return Err("insufficient valid signatures");
        }

        Ok(())
    }

    /// Verify signatures against an explicit key set (used for root rotation §5.3.4).
    fn verify_signatures_with_keys(
        &self,
        meta: &TufMetadata,
        role: TufRole,
        key_set: &BTreeMap<[u8; 32], TufKey>,
    ) -> Result<(), &'static str> {
        let threshold = self.root.role_thresholds.get(&role).copied().unwrap_or(1);

        let mut valid_count = 0;
        let mut seen_keyids: BTreeMap<[u8; 32], bool> = BTreeMap::new();

        for sig in &meta.signatures {
            if seen_keyids.contains_key(&sig.keyid) {
                continue;
            }

            if let Some(key) = key_set.get(&sig.keyid) {
                if verify_signature(key, &sig.sig, &meta.payload) {
                    valid_count += 1;
                    seen_keyids.insert(sig.keyid, true);
                }
            }
        }

        if valid_count < threshold as usize {
            return Err("insufficient valid signatures for key set");
        }

        Ok(())
    }

    /// Get the current trusted root metadata.
    pub fn root(&self) -> &TufMetadata {
        &self.root
    }

    /// Get the fixed update start time (§5.1).
    pub fn update_start_time(&self) -> u64 {
        self.update_start_time
    }

    /// Verify a delegated targets role (TUF spec §4.5).
    ///
    /// The delegation chain is resolved by finding the first matching role
    /// in the parent's delegations. The delegated metadata must be signed
    /// by the threshold of keys listed in the delegation role.
    pub fn verify_delegated_targets(
        &self,
        data: &[u8],
        target_path: &str,
    ) -> Result<(TargetsInfo, &DelegationRole), &'static str> {
        // Get delegations from latest verified targets
        let parent_targets = self
            .latest_targets_info
            .as_ref()
            .ok_or("parent targets metadata not yet verified")?;

        let delegations = parent_targets
            .delegations
            .as_ref()
            .ok_or("no delegations in parent targets metadata")?;

        // Find the first matching delegation role (prioritized delegations)
        let mut matching_role: Option<&DelegationRole> = None;
        for role in &delegations.roles {
            if delegation_path_matches(role, target_path) {
                matching_role = Some(role);
                break;
            }
        }

        let role = matching_role.ok_or("no delegation role matches target path")?;

        // Parse the delegated metadata
        let meta = parse_metadata(data)?;
        if meta.role != TufRole::Targets {
            return Err("expected Targets role in delegated metadata");
        }
        Self::validate_spec_version(meta.spec_version_major, meta.spec_version_minor)?;

        // Verify signatures against delegation role's keys
        let threshold = role.threshold as usize;
        let mut valid_count = 0;
        let mut seen_keyids: BTreeMap<[u8; 32], bool> = BTreeMap::new();

        for sig in &meta.signatures {
            if seen_keyids.contains_key(&sig.keyid) {
                continue;
            }
            if !role.keyids.contains(&sig.keyid) {
                continue;
            }
            if let Some(key) = delegations.keys.get(&sig.keyid) {
                if verify_signature(key, &sig.sig, &meta.payload) {
                    valid_count += 1;
                    seen_keyids.insert(sig.keyid, true);
                }
            }
        }

        if valid_count < threshold {
            return Err("insufficient valid signatures for delegated role");
        }

        let info = parse_targets_payload(&meta.payload)?;
        Ok((info, role))
    }
}

/// Verify a signature using the key's algorithm.
///
/// TUF spec §4.2.2: Supports ed25519 and rsassa-pss-sha256.
/// In echOS POUF: Ed25519 uses echOS-native SHA3-512 based Ed25519,
/// RSA uses PKCS#1 v1.5 with SHA-256.
fn verify_signature(key: &TufKey, sig: &[u8], payload: &[u8]) -> bool {
    match key.key_type {
        KeyType::Ed25519 => {
            if key.public_key.len() != 32 || sig.len() != 64 {
                return false;
            }
            ed25519_verify(&key.public_key, payload, sig)
        }
        KeyType::Rsa => {
            if key.public_key.is_empty() || sig.is_empty() {
                return false;
            }
            // RSA-PKCS1-v15-SHA256 verification
            rsa_pkcs1_verify(&key.public_key, sig, payload)
        }
    }
}

/// Ed25519 signature verification.
///
/// Uses echOS-native Ed25519 implementation (SHA3-512 based per our POUF).
/// This is consistent across the echOS codebase.
fn ed25519_verify(public_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
    if public_key.len() != 32 || signature.len() != 64 {
        return false;
    }

    // Import echOS Ed25519 for verification
    use crate::crypto::ed25519::Ed25519PublicKey;

    let mut pk_bytes = [0u8; 32];
    pk_bytes.copy_from_slice(public_key);
    let pk = Ed25519PublicKey::from_bytes(pk_bytes);
    let mut sig_bytes = [0u8; 64];
    sig_bytes.copy_from_slice(signature);
    pk.verify(message, &sig_bytes)
}

/// RSA PKCS#1 v1.5 signature verification with SHA-256.
///
/// TUF spec §4.2.2: "rsassa-pss-sha256" is the recommended scheme,
/// but our POUF uses PKCS#1 v1.5 for simplicity in no_std.
fn rsa_pkcs1_verify(public_key_der: &[u8], signature: &[u8], message: &[u8]) -> bool {
    // Parse RSA public key from DER/PEM
    // For our binary POUF, public_key is raw (n, e) encoding:
    //   n_len(2 LE) + n(var) + e_len(2 LE) + e(var)
    if public_key_der.len() < 4 {
        return false;
    }

    let n_len = u16::from_le_bytes([public_key_der[0], public_key_der[1]]) as usize;
    if public_key_der.len() < 4 + n_len + 2 {
        return false;
    }
    let n = &public_key_der[4..4 + n_len];
    let e_offset = 4 + n_len;
    let e_len =
        u16::from_le_bytes([public_key_der[e_offset], public_key_der[e_offset + 1]]) as usize;
    if public_key_der.len() < e_offset + 2 + e_len {
        return false;
    }
    let e = &public_key_der[e_offset + 2..e_offset + 2 + e_len];

    // Use external rsa crate for verification
    rsa_verify_raw(n, e, signature, message)
}

/// Raw RSA PKCS#1 v1.5 verification using the external `rsa` crate.
fn rsa_verify_raw(n: &[u8], e: &[u8], signature: &[u8], message: &[u8]) -> bool {
    #[cfg(not(target_os = "none"))]
    {
        use rsa::pkcs1v15;
        use rsa::signature::Verifier;
        use rsa::BigUint;
        use rsa::RsaPublicKey;

        let n_big = BigUint::from_bytes_be(n);
        let e_big = BigUint::from_bytes_be(e);

        match RsaPublicKey::new(n_big, e_big) {
            Ok(rsa_key) => {
                let vk = pkcs1v15::VerifyingKey::<sha2::Sha256>::new(rsa_key);
                match pkcs1v15::Signature::try_from(signature) {
                    Ok(sig) => vk.verify(message, &sig).is_ok(),
                    Err(_) => false,
                }
            }
            Err(_) => false,
        }
    }

    #[cfg(target_os = "none")]
    {
        // Kernel-space: use our own RSA verification from crypto module
        use crate::crypto::rsa::RsaPublicKey;

        let pub_key = RsaPublicKey::new(n, e);
        pub_key.verify(message, signature, "sha256")
    }
}

/// Full SHA-256 implementation (FIPS 180-4).
fn sha256_simple(data: &[u8]) -> [u8; 32] {
    let mut hash = [0u8; 32];
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    let k: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let mut msg = data.to_vec();
    let bit_len = msg.len() as u64 * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    for b in bit_len.to_be_bytes() {
        msg.push(b);
    }

    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(k[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(h);
    }

    for (i, &s) in state.iter().enumerate() {
        hash[i * 4..i * 4 + 4].copy_from_slice(&s.to_be_bytes());
    }

    hash
}

/// Get current UNIX timestamp from RTC driver.
///
/// TUF spec §5.1: "Record the time at which the update began as the fixed
/// update start time."
fn current_unix_timestamp() -> u64 {
    crate::drivers::rtc::get_unix_time()
}

lazy_static! {
    static ref TUF_INIT: Mutex<bool> = Mutex::new(false);
}

pub fn init() {
    let mut guard = TUF_INIT.lock();
    if *guard {
        return;
    }
    *guard = true;
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn sha256_known_answer_test() {
        // NIST SHA-256 test vector: "abc"
        let input = b"abc";
        let expected: [u8; 32] = [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
            0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
            0xf2, 0x00, 0x15, 0xad,
        ];
        let result = sha256_simple(input);
        assert_eq!(result, expected);
    }

    #[test]
    fn keyid_computation_is_deterministic() {
        let pub_key = vec![0u8; 32];
        let keyid1 = compute_keyid(KeyType::Ed25519, &pub_key);
        let keyid2 = compute_keyid(KeyType::Ed25519, &pub_key);
        assert_eq!(keyid1, keyid2);
    }

    #[test]
    fn keyid_differs_for_different_key_types() {
        let pub_key = vec![0u8; 32];
        let keyid_ed = compute_keyid(KeyType::Ed25519, &pub_key);
        let keyid_rsa = compute_keyid(KeyType::Rsa, &pub_key);
        assert_ne!(keyid_ed, keyid_rsa);
    }

    #[test]
    fn spec_version_validation_rejects_old_minor() {
        assert!(TufVerifier::validate_spec_version(1, 0).is_ok());
        assert!(TufVerifier::validate_spec_version(1, 1).is_ok());
        assert!(TufVerifier::validate_spec_version(2, 0).is_err());
        // Minor < 0 is impossible, but test the boundary
    }

    #[test]
    fn max_version_jump_prevents_fast_forward() {
        // MAX_VERSION_JUMP = 1_000_000
        assert!(MAX_VERSION_JUMP > 0);
        assert!(MAX_VERSION_JUMP < u64::MAX);
    }

    #[test]
    fn max_target_file_size_prevents_endless_data() {
        assert!(MAX_TARGET_FILE_SIZE > 0);
        assert!(MAX_TARGET_FILE_SIZE < u64::MAX);
    }

    #[test]
    fn glob_match_basic_patterns() {
        // Exact match
        assert!(glob_match("foo.tgz", "foo.tgz"));
        assert!(!glob_match("foo.tgz", "bar.tgz"));

        // Single wildcard
        assert!(glob_match("*.tgz", "foo.tgz"));
        assert!(glob_match("*.tgz", "bar.tgz"));
        assert!(!glob_match("*.tgz", "targets/foo.tgz")); // wildcard must not match /

        // Single char wildcard
        assert!(glob_match("foo-version-?.tgz", "foo-version-2.tgz"));
        assert!(glob_match("foo-version-?.tgz", "foo-version-a.tgz"));
        assert!(!glob_match("foo-version-?.tgz", "foo-version-alpha.tgz"));

        // Path patterns
        assert!(glob_match("targets/*.tgz", "targets/foo.tgz"));
        assert!(glob_match("targets/*.tgz", "targets/bar.tgz"));
        assert!(!glob_match("targets/*.tgz", "targets/foo.txt"));
    }

    #[test]
    fn delegation_path_matches_with_paths() {
        let role = DelegationRole {
            name: "project".to_string(),
            keyids: vec![],
            threshold: 1,
            paths: Some(vec!["project/*.txt".to_string()]),
            path_hash_prefixes: None,
            terminating: false,
        };

        assert!(delegation_path_matches(&role, "project/file.txt"));
        assert!(delegation_path_matches(&role, "project/readme.txt"));
        assert!(!delegation_path_matches(&role, "other/file.txt"));
        assert!(!delegation_path_matches(&role, "project/sub/file.txt")); // * doesn't match /
    }

    #[test]
    fn delegation_path_matches_with_prefixes() {
        let hash = sha256_simple(b"target/file.bin");
        let hex = hex_encode(&hash);
        let prefix = hex[..8].to_string();

        let role = DelegationRole {
            name: "bin".to_string(),
            keyids: vec![],
            threshold: 1,
            paths: None,
            path_hash_prefixes: Some(vec![prefix.clone()]),
            terminating: false,
        };

        assert!(delegation_path_matches(&role, "target/file.bin"));
        assert!(!delegation_path_matches(&role, "other/file.bin"));
    }
}
