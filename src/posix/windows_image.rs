use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use rsa::pkcs1v15::{Signature as RsaSignature, VerifyingKey};
use rsa::{BigUint, RsaPublicKey};
use sha2::{Digest, Sha256};
use signature::Verifier;
#[cfg(target_os = "uefi")]
use uefi::table::runtime::VariableAttributes;
#[cfg(target_os = "uefi")]
use uefi::CStr16;

use super::super::{boot, fs, serial_println};
use super::windows_runtime::{current_windows_runtime, WindowsRuntime, WindowsRuntimeError};
use super::VariableVendor;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PeError {
    Invalid,
    OutOfBounds,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SecureBootError {
    Invalid,
    OutOfBounds,
    NotSigned,
    SignatureInvalid,
    Revoked,
    MissingDb,
    RuntimeUnavailable,
    Unsupported,
}

impl From<PeError> for SecureBootError {
    fn from(value: PeError) -> Self {
        match value {
            PeError::Invalid => SecureBootError::Invalid,
            PeError::OutOfBounds => SecureBootError::OutOfBounds,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeInfo {
    pub is_64: bool,
    pub machine: u16,
    pub section_count: u16,
    pub entry_rva: u32,
    pub image_base: u64,
    pub subsystem: u16,
}

#[derive(Clone, Debug)]
pub struct PeSectionInfo {
    pub name: String,
    pub virtual_address: u32,
    pub virtual_size: u32,
    pub raw_pointer: u32,
    pub raw_size: u32,
}

#[derive(Clone, Debug)]
pub struct WindowsLaunchPlan {
    pub runtime: WindowsRuntime,
    pub pe_info: PeInfo,
}

struct PeLayout {
    optional_offset: usize,
    optional_size: usize,
    is_64: bool,
    cert_table_offset: u32,
    cert_table_size: u32,
}

struct Pkcs7Info {
    econtent: Vec<u8>,
    content_digest: Vec<u8>,
    signed_attrs_der: Vec<u8>,
    signed_attrs_digest: Vec<u8>,
    signature: Vec<u8>,
    signer_issuer: Vec<u8>,
    signer_serial: Vec<u8>,
    certs: Vec<Vec<u8>>,
}

struct SignatureDatabase {
    hashes: Vec<[u8; 32]>,
    certs: Vec<Vec<u8>>,
}

struct CertDetails {
    issuer: Vec<u8>,
    subject: Vec<u8>,
    tbs: Vec<u8>,
    signature: Vec<u8>,
}

#[derive(Clone, Debug)]
struct PeImage {
    info: PeInfo,
    image: Vec<u8>,
}

struct PeSection {
    name: String,
    virtual_address: u32,
    virtual_size: u32,
    raw_pointer: u32,
    raw_size: u32,
}

struct PeMeta {
    info: PeInfo,
    size_of_image: u32,
    size_of_headers: u32,
    sections: Vec<PeSection>,
    cert_table_size: u32,
}

struct SignerInfo {
    signed_attrs_der: Vec<u8>,
    signed_attrs_digest: Vec<u8>,
    signature: Vec<u8>,
    signer_issuer: Vec<u8>,
    signer_serial: Vec<u8>,
}

const OID_SIGNED_DATA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x07, 0x02];
const OID_MESSAGE_DIGEST: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x09, 0x04];
const OID_CONTENT_TYPE: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x09, 0x03];
const OID_SHA256: &[u8] = &[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02, 0x01];
const OID_RSA_ENCRYPTION: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01];
const OID_SHA256_WITH_RSA: &[u8] = &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0b];
const EFI_CERT_SHA256: [u8; 16] = [
    0x26, 0x16, 0xc4, 0xc1, 0x4c, 0x50, 0x92, 0x40, 0xac, 0xa9, 0x41, 0xf9, 0x36, 0x93, 0x43, 0x28,
];
const EFI_CERT_X509: [u8; 16] = [
    0xa1, 0x59, 0xc0, 0xa5, 0xe4, 0x94, 0xa7, 0x4a, 0x87, 0xb5, 0xab, 0x15, 0x5c, 0x2b, 0xf0, 0x72,
];

pub fn run_windows_app(path: &str) -> Result<(), WindowsRuntimeError> {
    if path.trim().is_empty() {
        return Err(WindowsRuntimeError::Invalid);
    }
    current_windows_runtime().ok_or(WindowsRuntimeError::NotFound)?;
    let image = load_windows_image(path)?;
    run_windows_app_image(&image)
}

pub fn pe_info_from_image(image: &[u8]) -> Result<PeInfo, WindowsRuntimeError> {
    parse_pe(image).map_err(|_| WindowsRuntimeError::Invalid)
}

pub fn prepare_windows_launch(image: &[u8]) -> Result<WindowsLaunchPlan, WindowsRuntimeError> {
    let runtime = current_windows_runtime().ok_or(WindowsRuntimeError::NotFound)?;
    if boot::secure_boot_enabled() {
        verify_authenticode(image).map_err(|_| WindowsRuntimeError::SecureBootViolation)?;
    }
    let pe_info = parse_pe(image).map_err(|_| WindowsRuntimeError::Invalid)?;
    Ok(WindowsLaunchPlan { runtime, pe_info })
}

pub fn pe_sections_from_image(image: &[u8]) -> Result<Vec<PeSectionInfo>, WindowsRuntimeError> {
    let meta = parse_pe_meta(image).map_err(|_| WindowsRuntimeError::Invalid)?;
    Ok(meta
        .sections
        .into_iter()
        .map(|section| PeSectionInfo {
            name: section.name,
            virtual_address: section.virtual_address,
            virtual_size: section.virtual_size,
            raw_pointer: section.raw_pointer,
            raw_size: section.raw_size,
        })
        .collect())
}

pub fn run_windows_app_image(image: &[u8]) -> Result<(), WindowsRuntimeError> {
    let runtime = current_windows_runtime().ok_or(WindowsRuntimeError::NotFound)?;
    if boot::secure_boot_enabled() {
        verify_authenticode(image).map_err(|_| WindowsRuntimeError::SecureBootViolation)?;
    }
    let loaded = load_pe_image(image).map_err(|_| WindowsRuntimeError::Invalid)?;
    serial_println!(
        "windows launch runtime={} entry=0x{:08x} image_base=0x{:016x} pe64={}",
        runtime.name,
        loaded.info.entry_rva,
        loaded.info.image_base,
        loaded.info.is_64
    );
    Ok(())
}

pub fn secure_boot_db_available() -> bool {
    if !boot::secure_boot_enabled() {
        return true;
    }
    read_secure_boot_keys().is_ok()
}

pub fn secure_boot_verify_image(image: &[u8]) -> bool {
    if !boot::secure_boot_enabled() {
        return true;
    }
    verify_authenticode(image).is_ok()
}

fn load_windows_image(path: &str) -> Result<Vec<u8>, WindowsRuntimeError> {
    let inode = fs::vfs_open_inode(path).map_err(|_| WindowsRuntimeError::Invalid)?;
    let size = fs::vfs_inode_metadata(&inode)
        .map_err(|_| WindowsRuntimeError::Invalid)?
        .size;
    let mut data = vec![0u8; size];
    let mut offset = 0usize;
    while offset < data.len() {
        let read = fs::vfs_read_at(&inode, offset, &mut data[offset..])
            .map_err(|_| WindowsRuntimeError::Invalid)?;
        if read == 0 {
            break;
        }
        offset += read;
    }
    data.truncate(offset);
    Ok(data)
}

fn verify_authenticode(image: &[u8]) -> Result<(), SecureBootError> {
    let layout = parse_pe_layout(image)?;
    if layout.cert_table_size == 0 || layout.cert_table_offset == 0 {
        return Err(SecureBootError::NotSigned);
    }
    let pkcs7 = extract_pkcs7(image, &layout)?;
    let info = parse_pkcs7(&pkcs7)?;
    let file_hash = compute_authenticode_hash(image, &layout)?;
    if info.content_digest.len() != file_hash.len() || info.content_digest != file_hash {
        return Err(SecureBootError::SignatureInvalid);
    }
    let econtent_hash = sha256_hash(&info.econtent);
    if info.signed_attrs_digest.len() != econtent_hash.len()
        || info.signed_attrs_digest != econtent_hash
    {
        return Err(SecureBootError::SignatureInvalid);
    }
    let signer_cert = select_signer_cert(&info)?;
    let rsa_key = extract_rsa_public_key(&signer_cert)?;
    let sig = RsaSignature::try_from(info.signature.as_slice())
        .map_err(|_| SecureBootError::SignatureInvalid)?;
    let verifying_key = VerifyingKey::<Sha256>::new(rsa_key);
    verifying_key
        .verify(&info.signed_attrs_der, &sig)
        .map_err(|_| SecureBootError::SignatureInvalid)?;
    let (_pk, _kek, db, dbx) = read_secure_boot_keys()?;
    if dbx.matches_hash(&file_hash) || dbx.matches_cert(&signer_cert) {
        return Err(SecureBootError::Revoked);
    }
    if db.matches_hash(&file_hash) {
        return Ok(());
    }
    if db.matches_cert(&signer_cert) {
        return Ok(());
    }
    if verify_cert_chain_to_db(&signer_cert, &info.certs, &db, &dbx)? {
        Ok(())
    } else {
        Err(SecureBootError::SignatureInvalid)
    }
}

fn parse_pe_layout(image: &[u8]) -> Result<PeLayout, SecureBootError> {
    if image.len() < 64 {
        return Err(SecureBootError::Invalid);
    }
    if image[0] != b'M' || image[1] != b'Z' {
        return Err(SecureBootError::Invalid);
    }
    let pe_offset = read_u32(image, 0x3C)? as usize;
    if pe_offset + 4 + 20 > image.len() {
        return Err(SecureBootError::OutOfBounds);
    }
    if image[pe_offset] != b'P'
        || image[pe_offset + 1] != b'E'
        || image[pe_offset + 2] != 0
        || image[pe_offset + 3] != 0
    {
        return Err(SecureBootError::Invalid);
    }
    let coff_offset = pe_offset + 4;
    let optional_size = read_u16(image, coff_offset + 16)? as usize;
    let optional_offset = coff_offset + 20;
    if optional_offset + optional_size > image.len() {
        return Err(SecureBootError::OutOfBounds);
    }
    if optional_size < 64 {
        return Err(SecureBootError::Invalid);
    }
    let magic = read_u16(image, optional_offset)?;
    let is_64 = match magic {
        0x20B => true,
        0x10B => false,
        _ => return Err(SecureBootError::Invalid),
    };
    let data_dir_offset = optional_offset + if is_64 { 112 } else { 96 };
    let optional_end = optional_offset + optional_size;
    let security_offset = data_dir_offset + 4 * 8;
    let mut cert_table_offset = 0u32;
    let mut cert_table_size = 0u32;
    if security_offset + 8 <= optional_end {
        cert_table_offset = read_u32(image, security_offset)?;
        cert_table_size = read_u32(image, security_offset + 4)?;
    }
    Ok(PeLayout {
        optional_offset,
        optional_size,
        is_64,
        cert_table_offset,
        cert_table_size,
    })
}

fn compute_authenticode_hash(image: &[u8], layout: &PeLayout) -> Result<Vec<u8>, SecureBootError> {
    let checksum_offset = layout.optional_offset + 64;
    if checksum_offset + 4 > image.len() {
        return Err(SecureBootError::OutOfBounds);
    }
    let data_dir_offset = layout.optional_offset + if layout.is_64 { 112 } else { 96 };
    let security_entry_offset = data_dir_offset + 4 * 8;
    if security_entry_offset + 8 > layout.optional_offset + layout.optional_size {
        return Err(SecureBootError::OutOfBounds);
    }
    let mut hasher = Sha256::new();
    hasher.update(&image[..checksum_offset]);
    hasher.update(&[0u8; 4]);
    let mut pos = checksum_offset + 4;
    if pos < security_entry_offset {
        hasher.update(&image[pos..security_entry_offset]);
    }
    pos = security_entry_offset + 8;
    let cert_offset = layout.cert_table_offset as usize;
    let cert_size = layout.cert_table_size as usize;
    if cert_offset == 0 || cert_size == 0 {
        if pos < image.len() {
            hasher.update(&image[pos..]);
        }
        return Ok(hasher.finalize().to_vec());
    }
    if cert_offset > image.len() {
        return Err(SecureBootError::OutOfBounds);
    }
    if pos < cert_offset {
        hasher.update(&image[pos..cert_offset]);
    }
    let cert_end = cert_offset.saturating_add(cert_size).min(image.len());
    pos = cert_end;
    if pos < image.len() {
        hasher.update(&image[pos..]);
    }
    Ok(hasher.finalize().to_vec())
}

fn sha256_hash(data: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}

fn extract_pkcs7(image: &[u8], layout: &PeLayout) -> Result<Vec<u8>, SecureBootError> {
    let cert_offset = layout.cert_table_offset as usize;
    let cert_size = layout.cert_table_size as usize;
    if cert_offset == 0 || cert_size == 0 {
        return Err(SecureBootError::NotSigned);
    }
    if cert_offset + cert_size > image.len() {
        return Err(SecureBootError::OutOfBounds);
    }
    let mut pos = cert_offset;
    let end = cert_offset + cert_size;
    while pos + 8 <= end {
        let length = read_u32(image, pos)? as usize;
        if length < 8 || pos + length > end {
            return Err(SecureBootError::OutOfBounds);
        }
        let cert_type = read_u16(image, pos + 6)?;
        if cert_type == 0x0002 {
            return Ok(image[pos + 8..pos + length].to_vec());
        }
        let aligned = (length + 7) & !7;
        pos = pos.saturating_add(aligned);
    }
    Err(SecureBootError::NotSigned)
}

fn parse_pkcs7(data: &[u8]) -> Result<Pkcs7Info, SecureBootError> {
    let (_, seq, _, _) = der_expect_tag(data, 0x30)?;
    let mut rest = seq;
    let (_, oid, rest_after_oid, _) = der_expect_tag(rest, 0x06)?;
    if !oid_eq(oid, OID_SIGNED_DATA) {
        return Err(SecureBootError::Unsupported);
    }
    rest = rest_after_oid;
    let (_, signed_data_wrapper, _, _) = der_expect_tag(rest, 0xa0)?;
    let (_, signed_data, _, _) = der_expect_tag(signed_data_wrapper, 0x30)?;
    let mut sd = signed_data;
    sd = der_skip_tlv(sd)?;
    sd = der_skip_tlv(sd)?;
    let (_, encap, rest_after_encap, _) = der_expect_tag(sd, 0x30)?;
    let econtent = parse_encap_content(encap)?;
    let mut sd_rest = rest_after_encap;
    let mut certs = Vec::new();
    if let Some(tag) = sd_rest.first().copied() {
        if tag == 0xa0 {
            let (_, cert_block, rest_after_certs, _) = der_expect_tag(sd_rest, 0xa0)?;
            certs = parse_certificate_set(cert_block)?;
            sd_rest = rest_after_certs;
        }
    }
    let (_, signer_set, _, _) = der_expect_tag(sd_rest, 0x31)?;
    let signer_info = parse_first_signer_info(signer_set)?;
    let content_digest = parse_spc_indirect_data(&econtent)?;
    Ok(Pkcs7Info {
        econtent,
        content_digest,
        signed_attrs_der: signer_info.signed_attrs_der,
        signed_attrs_digest: signer_info.signed_attrs_digest,
        signature: signer_info.signature,
        signer_issuer: signer_info.signer_issuer,
        signer_serial: signer_info.signer_serial,
        certs,
    })
}

fn parse_encap_content(encap: &[u8]) -> Result<Vec<u8>, SecureBootError> {
    let (_, _, rest_after_oid, _) = der_expect_tag(encap, 0x06)?;
    let (_, content_wrapper, _, _) = der_expect_tag(rest_after_oid, 0xa0)?;
    let (_, content_bytes, _, _) = der_expect_tag(content_wrapper, 0x04)?;
    Ok(content_bytes.to_vec())
}

fn parse_spc_indirect_data(content: &[u8]) -> Result<Vec<u8>, SecureBootError> {
    let (_, seq, _, _) = der_expect_tag(content, 0x30)?;
    let rest = der_skip_tlv(seq)?;
    let (_, digest_info, _, _) = der_expect_tag(rest, 0x30)?;
    let (_, alg_seq, rest_after_alg, _) = der_expect_tag(digest_info, 0x30)?;
    let (_, alg_oid, _, _) = der_expect_tag(alg_seq, 0x06)?;
    if !oid_eq(alg_oid, OID_SHA256) {
        return Err(SecureBootError::Unsupported);
    }
    let (_, digest_bytes, _, _) = der_expect_tag(rest_after_alg, 0x04)?;
    Ok(digest_bytes.to_vec())
}

fn parse_first_signer_info(data: &[u8]) -> Result<SignerInfo, SecureBootError> {
    let (_, signer_info_der, _, _) = der_expect_tag(data, 0x30)?;
    let mut rest = der_skip_tlv(signer_info_der)?;
    let (_, sid, rest_after_sid, _) = der_expect_tag(rest, 0x30)?;
    let (issuer, serial) = parse_signer_sid(sid)?;
    rest = rest_after_sid;
    rest = der_skip_tlv(rest)?;
    let (tag, attrs_value, rest_after_attrs, _) = der_read_tlv(rest)?;
    let signed_attrs_der = if tag == 0xa0 {
        build_set_der(attrs_value)
    } else {
        return Err(SecureBootError::Invalid);
    };
    let signed_attrs_digest = parse_signed_attrs_digest(attrs_value)?;
    rest = rest_after_attrs;
    rest = der_skip_tlv(rest)?;
    let (_, signature_bytes, _, _) = der_expect_tag(rest, 0x04)?;
    Ok(SignerInfo {
        signed_attrs_der,
        signed_attrs_digest,
        signature: signature_bytes.to_vec(),
        signer_issuer: issuer,
        signer_serial: serial,
    })
}

fn parse_signer_sid(data: &[u8]) -> Result<(Vec<u8>, Vec<u8>), SecureBootError> {
    let (issuer_tag, _, rest_after_issuer, issuer_full) = der_read_tlv(data)?;
    if issuer_tag != 0x30 {
        return Err(SecureBootError::Invalid);
    }
    let (_, serial_value, _, _) = der_expect_tag(rest_after_issuer, 0x02)?;
    Ok((issuer_full.to_vec(), serial_value.to_vec()))
}

fn parse_signed_attrs_digest(attrs: &[u8]) -> Result<Vec<u8>, SecureBootError> {
    let mut rest = attrs;
    let mut digest = None;
    while !rest.is_empty() {
        let (_, attr_seq, new_rest, _) = der_expect_tag(rest, 0x30)?;
        rest = new_rest;
        let (_, oid, rest_after_oid, _) = der_expect_tag(attr_seq, 0x06)?;
        let (_, set_val, _, _) = der_expect_tag(rest_after_oid, 0x31)?;
        if oid_eq(oid, OID_MESSAGE_DIGEST) {
            let (_, digest_bytes, _, _) = der_expect_tag(set_val, 0x04)?;
            digest = Some(digest_bytes.to_vec());
        } else if oid_eq(oid, OID_CONTENT_TYPE) {
            let _ = der_expect_tag(set_val, 0x06)?;
        }
    }
    digest.ok_or(SecureBootError::Invalid)
}

fn parse_certificate_set(data: &[u8]) -> Result<Vec<Vec<u8>>, SecureBootError> {
    let mut certs = Vec::new();
    let mut rest = data;
    while !rest.is_empty() {
        let (tag, _, new_rest, full) = der_read_tlv(rest)?;
        if tag == 0x30 {
            certs.push(full.to_vec());
        }
        rest = new_rest;
    }
    Ok(certs)
}

fn select_signer_cert(info: &Pkcs7Info) -> Result<Vec<u8>, SecureBootError> {
    for cert in &info.certs {
        if let Ok((issuer, serial, _)) = parse_cert_issuer_serial_key(cert) {
            if issuer == info.signer_issuer && serial == info.signer_serial {
                return Ok(cert.clone());
            }
        }
    }
    Err(SecureBootError::SignatureInvalid)
}

fn verify_cert_chain_to_db(
    signer_cert: &[u8],
    intermediates: &[Vec<u8>],
    db: &SignatureDatabase,
    dbx: &SignatureDatabase,
) -> Result<bool, SecureBootError> {
    let mut current = signer_cert.to_vec();
    let mut seen: Vec<Vec<u8>> = Vec::new();
    let mut depth = 0usize;
    loop {
        depth += 1;
        if depth > 8 {
            return Ok(false);
        }
        if dbx.matches_cert(&current) {
            return Err(SecureBootError::Revoked);
        }
        if db.matches_cert(&current) {
            return Ok(true);
        }
        let (issuer, subject) = parse_cert_subject_issuer(&current)?;
        if seen
            .iter()
            .any(|seen_subject| seen_subject.as_slice() == subject.as_slice())
        {
            return Ok(false);
        }
        seen.push(subject);
        let issuer_cert = find_cert_by_subject(&issuer, intermediates)
            .or_else(|| find_cert_by_subject(&issuer, &db.certs));
        let issuer_cert = match issuer_cert {
            Some(cert) => cert,
            None => return Ok(false),
        };
        verify_cert_signature(&current, issuer_cert)?;
        current = issuer_cert.clone();
    }
}

fn find_cert_by_subject<'a>(subject: &[u8], certs: &'a [Vec<u8>]) -> Option<&'a Vec<u8>> {
    for cert in certs {
        if let Ok((_, cert_subject)) = parse_cert_subject_issuer(cert) {
            if cert_subject == subject {
                return Some(cert);
            }
        }
    }
    None
}

fn verify_cert_signature(child_cert: &[u8], issuer_cert: &[u8]) -> Result<(), SecureBootError> {
    let details = parse_cert_details(child_cert)?;
    let rsa_key = extract_rsa_public_key(issuer_cert)?;
    let sig = RsaSignature::try_from(details.signature.as_slice())
        .map_err(|_| SecureBootError::SignatureInvalid)?;
    let verifying_key = VerifyingKey::<Sha256>::new(rsa_key);
    verifying_key
        .verify(&details.tbs, &sig)
        .map_err(|_| SecureBootError::SignatureInvalid)?;
    Ok(())
}

fn parse_cert_details(cert: &[u8]) -> Result<CertDetails, SecureBootError> {
    let (_, cert_seq, _, _) = der_expect_tag(cert, 0x30)?;
    let (tbs_tag, tbs_value, rest_after_tbs, tbs_full) = der_read_tlv(cert_seq)?;
    if tbs_tag != 0x30 {
        return Err(SecureBootError::Invalid);
    }
    let (issuer, subject) = parse_tbs_issuer_subject(tbs_value)?;
    let (_, sig_alg, rest_after_sig_alg, _) = der_expect_tag(rest_after_tbs, 0x30)?;
    let (_, sig_oid, _, _) = der_expect_tag(sig_alg, 0x06)?;
    if !oid_eq(sig_oid, OID_SHA256_WITH_RSA) {
        return Err(SecureBootError::Unsupported);
    }
    let (_, sig_bits, _, _) = der_expect_tag(rest_after_sig_alg, 0x03)?;
    if sig_bits.is_empty() {
        return Err(SecureBootError::Invalid);
    }
    Ok(CertDetails {
        issuer,
        subject,
        tbs: tbs_full.to_vec(),
        signature: sig_bits[1..].to_vec(),
    })
}

fn parse_cert_subject_issuer(cert: &[u8]) -> Result<(Vec<u8>, Vec<u8>), SecureBootError> {
    let (_, cert_seq, _, _) = der_expect_tag(cert, 0x30)?;
    let (tbs_tag, tbs_value, _, _) = der_read_tlv(cert_seq)?;
    if tbs_tag != 0x30 {
        return Err(SecureBootError::Invalid);
    }
    parse_tbs_issuer_subject(tbs_value)
}

fn parse_tbs_issuer_subject(tbs: &[u8]) -> Result<(Vec<u8>, Vec<u8>), SecureBootError> {
    let mut tbs_rest = tbs;
    if let Some(tag) = tbs_rest.first().copied() {
        if tag == 0xa0 {
            tbs_rest = der_skip_tlv(tbs_rest)?;
        }
    }
    let (_, _, rest_after_serial, _) = der_expect_tag(tbs_rest, 0x02)?;
    tbs_rest = rest_after_serial;
    tbs_rest = der_skip_tlv(tbs_rest)?;
    let (issuer_tag, _, rest_after_issuer, issuer_full) = der_read_tlv(tbs_rest)?;
    if issuer_tag != 0x30 {
        return Err(SecureBootError::Invalid);
    }
    tbs_rest = rest_after_issuer;
    tbs_rest = der_skip_tlv(tbs_rest)?;
    let (subject_tag, _, _, subject_full) = der_read_tlv(tbs_rest)?;
    if subject_tag != 0x30 {
        return Err(SecureBootError::Invalid);
    }
    Ok((issuer_full.to_vec(), subject_full.to_vec()))
}

fn extract_rsa_public_key(cert: &[u8]) -> Result<RsaPublicKey, SecureBootError> {
    let (_, _, key) = parse_cert_issuer_serial_key(cert)?;
    Ok(key)
}

fn parse_cert_issuer_serial_key(
    cert: &[u8],
) -> Result<(Vec<u8>, Vec<u8>, RsaPublicKey), SecureBootError> {
    let (_, cert_seq, _, _) = der_expect_tag(cert, 0x30)?;
    let (_, tbs, _, _) = der_expect_tag(cert_seq, 0x30)?;
    let mut tbs_rest = tbs;
    if let Some(tag) = tbs_rest.first().copied() {
        if tag == 0xa0 {
            tbs_rest = der_skip_tlv(tbs_rest)?;
        }
    }
    let (_, serial, rest_after_serial, _) = der_expect_tag(tbs_rest, 0x02)?;
    tbs_rest = rest_after_serial;
    tbs_rest = der_skip_tlv(tbs_rest)?;
    let (issuer_tag, _, rest_after_issuer, issuer_full) = der_read_tlv(tbs_rest)?;
    if issuer_tag != 0x30 {
        return Err(SecureBootError::Invalid);
    }
    let issuer = issuer_full.to_vec();
    tbs_rest = rest_after_issuer;
    tbs_rest = der_skip_tlv(tbs_rest)?;
    tbs_rest = der_skip_tlv(tbs_rest)?;
    let (_, spki, _, _) = der_expect_tag(tbs_rest, 0x30)?;
    let (_, alg_seq, rest_after_alg, _) = der_expect_tag(spki, 0x30)?;
    let (_, alg_oid, _, _) = der_expect_tag(alg_seq, 0x06)?;
    if !oid_eq(alg_oid, OID_RSA_ENCRYPTION) {
        return Err(SecureBootError::Unsupported);
    }
    let (_, bit_string, _, _) = der_expect_tag(rest_after_alg, 0x03)?;
    if bit_string.is_empty() {
        return Err(SecureBootError::Invalid);
    }
    let (_, rsa_seq, _, _) = der_expect_tag(&bit_string[1..], 0x30)?;
    let (_, modulus, rest_after_modulus, _) = der_expect_tag(rsa_seq, 0x02)?;
    let (_, exponent, _, _) = der_expect_tag(rest_after_modulus, 0x02)?;
    let n = BigUint::from_bytes_be(modulus);
    let e = BigUint::from_bytes_be(exponent);
    let key = RsaPublicKey::new(n, e).map_err(|_| SecureBootError::Invalid)?;
    Ok((issuer, serial.to_vec(), key))
}

fn der_read_tlv<'a>(
    input: &'a [u8],
) -> Result<(u8, &'a [u8], &'a [u8], &'a [u8]), SecureBootError> {
    if input.len() < 2 {
        return Err(SecureBootError::Invalid);
    }
    let tag = input[0];
    let (len, len_len) = der_read_len(&input[1..])?;
    let header = 1 + len_len;
    if input.len() < header + len {
        return Err(SecureBootError::OutOfBounds);
    }
    let value = &input[header..header + len];
    let rest = &input[header + len..];
    let full = &input[..header + len];
    Ok((tag, value, rest, full))
}

fn der_expect_tag<'a>(
    input: &'a [u8],
    expected: u8,
) -> Result<(u8, &'a [u8], &'a [u8], &'a [u8]), SecureBootError> {
    let (tag, value, rest, full) = der_read_tlv(input)?;
    if tag != expected {
        return Err(SecureBootError::Invalid);
    }
    Ok((tag, value, rest, full))
}

fn der_read_len(input: &[u8]) -> Result<(usize, usize), SecureBootError> {
    if input.is_empty() {
        return Err(SecureBootError::Invalid);
    }
    let first = input[0];
    if first & 0x80 == 0 {
        return Ok((first as usize, 1));
    }
    let count = (first & 0x7f) as usize;
    if count == 0 || count > 4 || input.len() < 1 + count {
        return Err(SecureBootError::Invalid);
    }
    let mut len = 0usize;
    for i in 0..count {
        len = (len << 8) | input[1 + i] as usize;
    }
    Ok((len, 1 + count))
}

fn der_skip_tlv<'a>(input: &'a [u8]) -> Result<&'a [u8], SecureBootError> {
    let (_, _, rest, _) = der_read_tlv(input)?;
    Ok(rest)
}

fn build_set_der(content: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + content.len());
    out.push(0x31);
    der_write_len(content.len(), &mut out);
    out.extend_from_slice(content);
    out
}

fn der_write_len(len: usize, out: &mut Vec<u8>) {
    if len < 0x80 {
        out.push(len as u8);
        return;
    }
    let mut tmp = [0u8; 4];
    let mut idx = 4;
    let mut value = len;
    while value > 0 {
        idx -= 1;
        tmp[idx] = (value & 0xff) as u8;
        value >>= 8;
    }
    let count = 4 - idx;
    out.push(0x80 | count as u8);
    out.extend_from_slice(&tmp[idx..]);
}

fn oid_eq(value: &[u8], oid: &[u8]) -> bool {
    value == oid
}

fn read_secure_boot_keys() -> Result<
    (
        SignatureDatabase,
        SignatureDatabase,
        SignatureDatabase,
        SignatureDatabase,
    ),
    SecureBootError,
> {
    let pk = read_signature_database("PK", VariableVendor::GLOBAL_VARIABLE)?;
    let kek = read_signature_database("KEK", VariableVendor::GLOBAL_VARIABLE)?;
    let db = read_signature_database("db", VariableVendor::IMAGE_SECURITY_DATABASE)?;
    let dbx = read_signature_database("dbx", VariableVendor::IMAGE_SECURITY_DATABASE)?;
    if pk.hashes.is_empty() && pk.certs.is_empty() {
        return Err(SecureBootError::MissingDb);
    }
    if kek.hashes.is_empty() && kek.certs.is_empty() {
        return Err(SecureBootError::MissingDb);
    }
    if db.hashes.is_empty() && db.certs.is_empty() {
        return Err(SecureBootError::MissingDb);
    }
    Ok((pk, kek, db, dbx))
}

fn read_signature_database(
    name: &'static str,
    vendor: VariableVendor,
) -> Result<SignatureDatabase, SecureBootError> {
    let data = read_uefi_variable(name, vendor)?;
    parse_signature_database(&data)
}

#[cfg(target_os = "uefi")]
fn uefi_variable_allowed(name: &str) -> bool {
    matches!(name, "PK" | "KEK" | "db" | "dbx")
}

#[cfg(target_os = "uefi")]
fn uefi_variable_attributes(_name: &str) -> VariableAttributes {
    VariableAttributes::NON_VOLATILE
        | VariableAttributes::BOOTSERVICE_ACCESS
        | VariableAttributes::RUNTIME_ACCESS
        | VariableAttributes::TIME_BASED_AUTHENTICATED_WRITE_ACCESS
}

#[cfg(target_os = "uefi")]
fn read_uefi_variable(
    name: &'static str,
    vendor: VariableVendor,
) -> Result<Vec<u8>, SecureBootError> {
    if boot::secure_boot_enabled() && !uefi_variable_allowed(name) {
        return Err(SecureBootError::Unsupported);
    }
    let runtime_services = boot::runtime_services().ok_or(SecureBootError::RuntimeUnavailable)?;
    let mut buf = vec![0u16; name.encode_utf16().count() + 1];
    let var_name =
        CStr16::from_str_with_buf(name, &mut buf).map_err(|_| SecureBootError::Invalid)?;
    let (data, attrs) = runtime_services
        .get_variable_boxed(var_name, &vendor)
        .map_err(|_| SecureBootError::MissingDb)?;
    if !validate_uefi_variable_attributes(name, attrs) {
        return Err(SecureBootError::Invalid);
    }
    Ok(data.into_vec())
}

#[cfg(target_os = "uefi")]
#[allow(dead_code)]
fn write_uefi_variable(
    name: &'static str,
    vendor: VariableVendor,
    data: &[u8],
) -> Result<(), SecureBootError> {
    if boot::secure_boot_enabled() && !uefi_variable_allowed(name) {
        return Err(SecureBootError::Unsupported);
    }
    let runtime_services = boot::runtime_services().ok_or(SecureBootError::RuntimeUnavailable)?;
    let mut buf = vec![0u16; name.encode_utf16().count() + 1];
    let var_name =
        CStr16::from_str_with_buf(name, &mut buf).map_err(|_| SecureBootError::Invalid)?;
    let attributes = uefi_variable_attributes(name);
    runtime_services
        .set_variable(var_name, &vendor, attributes, data)
        .map_err(|_| SecureBootError::Invalid)?;
    Ok(())
}

#[cfg(target_os = "uefi")]
fn validate_uefi_variable_attributes(name: &str, attrs: VariableAttributes) -> bool {
    let required = VariableAttributes::NON_VOLATILE
        | VariableAttributes::BOOTSERVICE_ACCESS
        | VariableAttributes::RUNTIME_ACCESS;
    if !attrs.contains(required) {
        return false;
    }
    if matches!(name, "PK" | "KEK" | "db" | "dbx") {
        let authenticated = VariableAttributes::TIME_BASED_AUTHENTICATED_WRITE_ACCESS
            | VariableAttributes::ENHANCED_AUTHENTICATED_ACCESS
            | VariableAttributes::AUTHENTICATED_WRITE_ACCESS;
        return attrs.intersects(authenticated)
            && !attrs.contains(VariableAttributes::APPEND_WRITE);
    }
    true
}

#[cfg(not(target_os = "uefi"))]
fn read_uefi_variable(
    _name: &'static str,
    _vendor: VariableVendor,
) -> Result<Vec<u8>, SecureBootError> {
    Err(SecureBootError::RuntimeUnavailable)
}

#[cfg(not(target_os = "uefi"))]
#[allow(dead_code)]
fn write_uefi_variable(
    _name: &'static str,
    _vendor: VariableVendor,
    _data: &[u8],
) -> Result<(), SecureBootError> {
    Err(SecureBootError::RuntimeUnavailable)
}

fn parse_signature_database(data: &[u8]) -> Result<SignatureDatabase, SecureBootError> {
    let mut hashes = Vec::new();
    let mut certs = Vec::new();
    let mut offset = 0usize;
    while offset + 28 <= data.len() {
        let sig_type = &data[offset..offset + 16];
        let list_size = read_u32(data, offset + 16)? as usize;
        let header_size = read_u32(data, offset + 20)? as usize;
        let sig_size = read_u32(data, offset + 24)? as usize;
        if list_size < 28 || sig_size < 16 || offset + list_size > data.len() {
            return Err(SecureBootError::OutOfBounds);
        }
        let mut entry = offset + 28 + header_size;
        let list_end = offset + list_size;
        while entry + sig_size <= list_end {
            let sig_data = &data[entry + 16..entry + sig_size];
            if sig_type == EFI_CERT_SHA256.as_slice() {
                if sig_data.len() == 32 {
                    let mut hash = [0u8; 32];
                    hash.copy_from_slice(sig_data);
                    hashes.push(hash);
                }
            } else if sig_type == EFI_CERT_X509.as_slice() {
                certs.push(sig_data.to_vec());
            }
            entry += sig_size;
        }
        offset += list_size;
    }
    Ok(SignatureDatabase { hashes, certs })
}

impl SignatureDatabase {
    fn matches_hash(&self, hash: &[u8]) -> bool {
        hash.len() == 32
            && self
                .hashes
                .iter()
                .any(|known_hash| known_hash.as_slice() == hash)
    }

    fn matches_cert(&self, cert: &[u8]) -> bool {
        self.certs
            .iter()
            .any(|known_cert| known_cert.as_slice() == cert)
    }
}

fn load_pe_image(image: &[u8]) -> Result<PeImage, PeError> {
    let meta = parse_pe_meta(image)?;
    let size_of_image = meta.size_of_image as usize;
    if size_of_image == 0 || size_of_image > 256 * 1024 * 1024 {
        return Err(PeError::OutOfBounds);
    }
    let mut loaded = vec![0u8; size_of_image];
    let header_bytes = meta
        .size_of_headers
        .min(image.len() as u32)
        .min(size_of_image as u32) as usize;
    loaded[..header_bytes].copy_from_slice(&image[..header_bytes]);
    for section in meta.sections {
        if section.raw_size == 0 {
            continue;
        }
        let virt_start = section.virtual_address as usize;
        let raw_start = section.raw_pointer as usize;
        let raw_size = section.raw_size as usize;
        let virt_size = section.virtual_size as usize;
        let copy_size = if virt_size == 0 {
            raw_size
        } else {
            raw_size.min(virt_size)
        };
        if raw_start + raw_size > image.len() {
            return Err(PeError::OutOfBounds);
        }
        if virt_start + copy_size > loaded.len() {
            return Err(PeError::OutOfBounds);
        }
        loaded[virt_start..virt_start + copy_size]
            .copy_from_slice(&image[raw_start..raw_start + copy_size]);
    }
    Ok(PeImage {
        info: meta.info,
        image: loaded,
    })
}

fn parse_pe_meta(image: &[u8]) -> Result<PeMeta, PeError> {
    if image.len() < 64 {
        return Err(PeError::Invalid);
    }
    if image[0] != b'M' || image[1] != b'Z' {
        return Err(PeError::Invalid);
    }
    let pe_offset = read_u32(image, 0x3C)? as usize;
    if pe_offset + 4 + 20 > image.len() {
        return Err(PeError::OutOfBounds);
    }
    if image[pe_offset] != b'P'
        || image[pe_offset + 1] != b'E'
        || image[pe_offset + 2] != 0
        || image[pe_offset + 3] != 0
    {
        return Err(PeError::Invalid);
    }
    let coff_offset = pe_offset + 4;
    let machine = read_u16(image, coff_offset)?;
    let section_count = read_u16(image, coff_offset + 2)?;
    let optional_size = read_u16(image, coff_offset + 16)? as usize;
    let optional_offset = coff_offset + 20;
    if optional_offset + optional_size > image.len() {
        return Err(PeError::OutOfBounds);
    }
    if optional_size < 64 {
        return Err(PeError::Invalid);
    }
    let magic = read_u16(image, optional_offset)?;
    let (is_64, entry_rva, image_base, subsystem) = match magic {
        0x20B => {
            if optional_size < 112 {
                return Err(PeError::Invalid);
            }
            (
                true,
                read_u32(image, optional_offset + 16)?,
                read_u64(image, optional_offset + 24)?,
                read_u16(image, optional_offset + 68)?,
            )
        }
        0x10B => {
            if optional_size < 96 {
                return Err(PeError::Invalid);
            }
            (
                false,
                read_u32(image, optional_offset + 16)?,
                read_u32(image, optional_offset + 28)? as u64,
                read_u16(image, optional_offset + 68)?,
            )
        }
        _ => return Err(PeError::Invalid),
    };
    let size_of_image = read_u32(image, optional_offset + 56)?;
    let size_of_headers = read_u32(image, optional_offset + 60)?;
    let section_table = optional_offset + optional_size;
    let total_section_bytes = section_count as usize * 40;
    if section_table + total_section_bytes > image.len() {
        return Err(PeError::OutOfBounds);
    }
    let mut sections = Vec::new();
    for idx in 0..section_count as usize {
        let offset = section_table + idx * 40;
        let mut name = String::new();
        let name_end = (offset + 8).min(image.len());
        for &byte in &image[offset..name_end] {
            if byte == 0 {
                break;
            }
            name.push(byte as char);
        }
        let virtual_size = read_u32(image, offset + 8)?;
        let virtual_address = read_u32(image, offset + 12)?;
        let raw_size = read_u32(image, offset + 16)?;
        let raw_pointer = read_u32(image, offset + 20)?;
        if virtual_address
            .checked_add(virtual_size)
            .map(|end| end as usize <= size_of_image as usize)
            != Some(true)
        {
            return Err(PeError::OutOfBounds);
        }
        sections.push(PeSection {
            name,
            virtual_address,
            virtual_size,
            raw_pointer,
            raw_size,
        });
    }
    let data_dir_offset = optional_offset + if is_64 { 112 } else { 96 };
    let optional_end = optional_offset + optional_size;
    let security_offset = data_dir_offset + 4 * 8;
    let mut cert_table_size = 0u32;
    if security_offset + 8 <= optional_end {
        let _ = read_u32(image, security_offset)?;
        cert_table_size = read_u32(image, security_offset + 4)?;
    }
    Ok(PeMeta {
        info: PeInfo {
            is_64,
            machine,
            section_count,
            entry_rva,
            image_base,
            subsystem,
        },
        size_of_image,
        size_of_headers,
        sections,
        cert_table_size,
    })
}

fn parse_pe(image: &[u8]) -> Result<PeInfo, PeError> {
    if image.len() < 64 {
        return Err(PeError::Invalid);
    }
    if image[0] != b'M' || image[1] != b'Z' {
        return Err(PeError::Invalid);
    }
    let pe_offset = read_u32(image, 0x3C)? as usize;
    if pe_offset + 4 + 20 > image.len() {
        return Err(PeError::OutOfBounds);
    }
    if image[pe_offset] != b'P'
        || image[pe_offset + 1] != b'E'
        || image[pe_offset + 2] != 0
        || image[pe_offset + 3] != 0
    {
        return Err(PeError::Invalid);
    }
    let coff_offset = pe_offset + 4;
    let machine = read_u16(image, coff_offset)?;
    let section_count = read_u16(image, coff_offset + 2)?;
    let optional_size = read_u16(image, coff_offset + 16)? as usize;
    let optional_offset = coff_offset + 20;
    if optional_offset + optional_size > image.len() {
        return Err(PeError::OutOfBounds);
    }
    if optional_size < 2 {
        return Err(PeError::Invalid);
    }
    let magic = read_u16(image, optional_offset)?;
    let (is_64, entry_rva, image_base, subsystem) = match magic {
        0x20B => {
            if optional_size < 112 {
                return Err(PeError::Invalid);
            }
            (
                true,
                read_u32(image, optional_offset + 16)?,
                read_u64(image, optional_offset + 24)?,
                read_u16(image, optional_offset + 68)?,
            )
        }
        0x10B => {
            if optional_size < 96 {
                return Err(PeError::Invalid);
            }
            (
                false,
                read_u32(image, optional_offset + 16)?,
                read_u32(image, optional_offset + 28)? as u64,
                read_u16(image, optional_offset + 68)?,
            )
        }
        _ => return Err(PeError::Invalid),
    };
    Ok(PeInfo {
        is_64,
        machine,
        section_count,
        entry_rva,
        image_base,
        subsystem,
    })
}

fn read_u16(image: &[u8], offset: usize) -> Result<u16, PeError> {
    if offset + 2 > image.len() {
        return Err(PeError::OutOfBounds);
    }
    Ok(u16::from_le_bytes([image[offset], image[offset + 1]]))
}

fn read_u32(image: &[u8], offset: usize) -> Result<u32, PeError> {
    if offset + 4 > image.len() {
        return Err(PeError::OutOfBounds);
    }
    Ok(u32::from_le_bytes([
        image[offset],
        image[offset + 1],
        image[offset + 2],
        image[offset + 3],
    ]))
}

fn read_u64(image: &[u8], offset: usize) -> Result<u64, PeError> {
    if offset + 8 > image.len() {
        return Err(PeError::OutOfBounds);
    }
    Ok(u64::from_le_bytes([
        image[offset],
        image[offset + 1],
        image[offset + 2],
        image[offset + 3],
        image[offset + 4],
        image[offset + 5],
        image[offset + 6],
        image[offset + 7],
    ]))
}
