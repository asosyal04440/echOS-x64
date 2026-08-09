//! # echOS ACPI Modülü
//!
//! ACPI (Advanced Configuration and Power Interface) tablolarını bulma ve okuma.
//! UEFI config tablosundan RSDP adresini alır.
//!
//! ## ACPI Başlatma Akışı
//! ```ascii
//! Boot protokolü (UEFI/Limine/MB2)
//!      |
//!      v
//! boot::context::RsdpCandidate → publish_rsdp() → authoritative state
//!      |
//!      v
//! init() → AcpiTables::from_rsdp()
//!      |
//!      v
//! platform_info() → InterruptModel::Apic
//!      |
//!      v
//! APIC_INFO → madt::from_apic()
//! ```

use crate::boot::context::{FieldState, RsdpAddressKind, RsdpCandidate, RsdpError};
use acpi::platform::interrupt::InterruptModel;
use acpi::{AcpiHandler, AcpiTables, PhysicalMapping};
use core::ptr::NonNull;
use spin::Mutex;

#[cfg(target_os = "uefi")]
use log::info;
#[cfg(target_os = "uefi")]
use uefi::table::cfg::{ConfigTableEntry, ACPI2_GUID, ACPI_GUID};

pub mod madt;

/// Tek authoritative ACPI bootstrap durumu.
///
/// Wave 1 kararı (boot-harmonization): `cpu::acpi::UEFI_RSDP_ADDRESS` ve eski
/// `RSDP_PHYS` biçimindeki iki bağımsız doğruluk kaynağı kaldırıldı; tek
/// doğruluk kaynağı bu state'tir. `publish_rsdp` adresi *bellek okumadan*
/// kaydeder (`PresentUntrusted`); `validate_authoritative_rsdp` ACPI fazında
/// signature/checksum/length doğrulayıp `PresentValidated`/`Invalid`'e
/// yükseltir. İkinci ve çelişkili bir RSDP sessizce kabul edilmez.
pub struct RsdpBootstrapState {
    candidate: Mutex<Option<RsdpCandidate>>,
}

static RSDP_BOOTSTRAP: RsdpBootstrapState = RsdpBootstrapState {
    candidate: Mutex::new(None),
};

/// RSDP adayını authoritative state'e yayınlar.
///
/// Reddedilme kuralları:
/// - sıfır adres asla kabul edilmez (`ZeroAddress`);
/// - canonical lower-half olmayan adres reddedilir (`NonCanonicalAddress`);
/// - fiziksel/sanal adres türleri karıştırılamaz (`KindMismatch`);
/// - farklı adresteki ikinci RSDP reddedilir (`ConflictingExisting`);
/// - aynı adres + aynı tür yeniden yayınlanırsa idempotent kabul edilir
///   (ilk kayıt korunur).
pub fn publish_rsdp(candidate: RsdpCandidate) -> Result<(), RsdpError> {
    if candidate.address == 0 {
        return Err(RsdpError::ZeroAddress);
    }
    if !is_canonical_lower_half(candidate.address) {
        return Err(RsdpError::NonCanonicalAddress(candidate.address));
    }
    let mut guard = RSDP_BOOTSTRAP.candidate.lock();
    match *guard {
        None => {
            *guard = Some(candidate);
            Ok(())
        }
        Some(existing) => {
            if existing.address_kind != candidate.address_kind {
                Err(RsdpError::KindMismatch {
                    existing: existing.address_kind,
                    incoming: candidate.address_kind,
                })
            } else if existing.address != candidate.address {
                Err(RsdpError::ConflictingExisting {
                    existing: existing.address,
                    incoming: candidate.address,
                })
            } else {
                Ok(())
            }
        }
    }
}

/// Authoritative state'teki adayın kopyası.
pub fn authoritative_rsdp() -> Option<RsdpCandidate> {
    RSDP_BOOTSTRAP.candidate.lock().clone()
}

/// Authoritative state'teki RSDP'nin fiziksel adresi (yoksa 0).
pub fn rsdp_address() -> u64 {
    authoritative_rsdp().map(|c| c.address).unwrap_or(0)
}

/// RSDP baytlarını spec'e göre doğrular (saf; host test edilebilir).
///
/// - ilk 8 bayt "RSD PTR " imzası;
/// - ilk 20 bayt checksum toplamı 0;
/// - ACPI 2.0+ (revision >= 2): offset 20'deki length >= 36 ve tam length
///   üzerinden ikinci checksum 0.
///
/// Başarıda `(revision, length)` döner.
pub fn validate_rsdp_bytes(bytes: &[u8]) -> Result<(u8, u32), RsdpError> {
    if bytes.len() < 20 {
        return Err(RsdpError::TooShort { len: bytes.len() });
    }
    if &bytes[0..8] != b"RSD PTR " {
        return Err(RsdpError::SignatureMismatch);
    }
    let sum1 = bytes[..20].iter().fold(0u8, |acc, b| acc.wrapping_add(*b));
    if sum1 != 0 {
        return Err(RsdpError::ChecksumMismatch);
    }
    let revision = bytes[15];
    if revision >= 2 {
        let length = u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        if length < 36 {
            return Err(RsdpError::LengthInvalid { declared: length });
        }
        if bytes.len() < length as usize {
            return Err(RsdpError::TooShort { len: bytes.len() });
        }
        let sum2 = bytes[..length as usize]
            .iter()
            .fold(0u8, |acc, b| acc.wrapping_add(*b));
        if sum2 != 0 {
            return Err(RsdpError::ChecksumMismatch);
        }
        Ok((revision, length))
    } else {
        Ok((revision, 20))
    }
}

/// Authoritative adayı bellekten okuyup doğrular ve state'i günceller.
///
/// Yalnızca ACPI fazında (paging + HHDM hazırken) çağrılmalıdır; erken boot'ta
/// fiziksel adres henüz sanal alana eşlenmemiş olabilir.
/// `parse_acpi_tables` bu fonksiyonu çağırır.
pub fn validate_authoritative_rsdp() -> Result<(u8, u32), RsdpError> {
    let candidate = authoritative_rsdp().ok_or(RsdpError::NoCandidate)?;
    let virt = match candidate.address_kind {
        RsdpAddressKind::Physical => {
            crate::memory::active_physical_offset().wrapping_add(candidate.address)
        }
        RsdpAddressKind::Virtual => candidate.address,
    };
    if !is_canonical(virt) {
        return Err(RsdpError::NonCanonicalAddress(candidate.address));
    }
    const READ_LEN: usize = 256;
    let bytes = unsafe { core::slice::from_raw_parts(virt as *const u8, READ_LEN) };
    let result = validate_rsdp_bytes(bytes);
    if let Some(c) = RSDP_BOOTSTRAP.candidate.lock().as_mut() {
        match &result {
            Ok((rev, _len)) => {
                c.field_state = FieldState::PresentValidated;
                c.acpi_revision = *rev;
            }
            Err(_) => {
                c.field_state = FieldState::Invalid;
            }
        }
    }
    result
}

fn is_canonical_lower_half(addr: u64) -> bool {
    addr >> 47 == 0
}

/// Canonical adres kontrolü — hem lower hem upper half kabul edilir.
///
/// HHDM penceresi upper-half canonical bölgede olduğundan, sanal adreslerle
/// okuma yapılmadan önce bu kontrol kullanılmalıdır (`is_canonical_lower_half`
/// yalnızca fiziksel aday yayınlarken geçerlidir).
fn is_canonical(addr: u64) -> bool {
    let upper = addr >> 47;
    upper == 0 || upper == 0x1_FFFF
}

/// Küresel APIC yapılandırma bilgisi.
///
/// `init()` başarılı olduğunda MADT'tan çıkarılan APIC bilgisi burada saklanır.
pub static APIC_INFO: Mutex<madt::ApicInfo> = Mutex::new(madt::ApicInfo::empty());

/// ACPI alt sistemini başlatır.
///
/// Authoritative state'teki RSDP adresinden ACPI tablolarını ayrıştırır ve
/// APIC bilgisini çıkarır. Başarılıysa `true`, başarısızsa `false` döner.
pub fn init() -> bool {
    crate::serial_println!("[W3] acpi authoritative begin");
    let candidate = match authoritative_rsdp() {
        Some(c) if c.address != 0 => c,
        _ => return false,
    };
    let rsdp = match candidate.address_kind {
        RsdpAddressKind::Physical => candidate.address,
        RsdpAddressKind::Virtual => {
            candidate.address.saturating_sub(crate::memory::active_physical_offset())
        }
    };
    if rsdp == 0 {
        return false;
    }
    crate::serial_println!("[W3] acpi rsdp physical={:#x} kind={:?} active_hhdm={:#x}", rsdp, candidate.address_kind, crate::memory::active_physical_offset());

    let handler = HhdmAcpiHandler;
    crate::serial_println!("[W3] acpi from_rsdp begin");
    let tables = unsafe { AcpiTables::from_rsdp(handler, rsdp as usize) };
    crate::serial_println!("[W3] acpi from_rsdp returned");
    let tables = match tables {
        Ok(tables) => tables,
        Err(_) => return false,
    };

    crate::serial_println!("[W3] acpi platform_info begin");
    let platform_info = match tables.platform_info() {
        Ok(info) => info,
        Err(_) => return false,
    };
    crate::serial_println!("[W3] acpi platform_info returned");

    match platform_info.interrupt_model {
        InterruptModel::Apic(apic) => {
            *APIC_INFO.lock() = madt::from_apic(&apic);
            true
        }
        _ => false,
    }
}

/// Küresel APIC yapılandırma bilgisinin klonunu döner.
pub fn get_apic_info() -> madt::ApicInfo {
    APIC_INFO.lock().clone()
}

/// PM Timer (ACPI 3.579545 MHz, 24-bit sayıcı) I/O port adresini döndürür.
/// TSC kalibrasyonu için kullanılır. 0 dönerse PM Timer mevcut değildir.
pub fn get_pm_tmr_port() -> u16 {
    crate::cpu::acpi::get_pm_tmr_port()
}

/// HPET (High Precision Event Timer) MMIO taban adresini döndürür.
/// TSC kalibrasyonu için kullanılır. 0 dönerse HPET mevcut değildir.
pub fn get_hpet_base() -> u64 {
    crate::cpu::acpi::get_hpet_base()
}

/// HHDM (Higher Half Direct Map) tabanlı ACPI bellek eşleyici.
///
/// Fiziksel adresleri HHDM ofsetiyle sanal adrese çevirerek ACPI tablolarına
/// erişim sağlar. `AcpiHandler` trait'ini uygular.
#[derive(Clone, Copy)]
struct HhdmAcpiHandler;

impl AcpiHandler for HhdmAcpiHandler {
    /// Fiziksel bellek bölgesini sanal adres alanına eşler.
    ///
    /// HHDM ofseti eklenerek fiziksel adres sanal adrese dönüştürülür.
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> PhysicalMapping<Self, T> {
        let virtual_address =
            (crate::memory::active_physical_offset() + physical_address as u64) as *mut T;
        let virtual_address = NonNull::new(virtual_address).unwrap();
        PhysicalMapping::new(physical_address, virtual_address, size, size, *self)
    }

    /// Fiziksel bellek bölgesinin eşlemesini kaldırır.
    ///
    /// HHDM tabanlı eşleme için temizleme gerekmez; boş bırakılır.
    fn unmap_physical_region<T>(_region: &PhysicalMapping<Self, T>) {}
}

/// UEFI config tablosundan ACPI RSDP (Root System Description Pointer) adresini bulur.
///
/// Önce ACPI 2.0 RSDP arar, bulamazsa ACPI 1.0'a düşer.
///
/// # Dönüş
/// - `Some(adres)`: RSDP'nin fiziksel bellek adresi
/// - `None`: Hiçbir ACPI tablosu bulunamadı
#[cfg(target_os = "uefi")]
pub fn find_acpi_table(config_entries: &[ConfigTableEntry]) -> Option<usize> {
    // Önce ACPI 2.0 ara (daha yeni ve kapsamlı)
    if let Some(entry) = config_entries.iter().find(|entry| entry.guid == ACPI2_GUID) {
        info!("ACPI 2.0 RSDP found at {:?}", entry.address);
        return Some(entry.address as usize);
    }

    // Bulunamazsa ACPI 1.0'a düş
    if let Some(entry) = config_entries.iter().find(|entry| entry.guid == ACPI_GUID) {
        info!("ACPI 1.0 RSDP found at {:?}", entry.address);
        return Some(entry.address as usize);
    }

    None
}

/// Pil yüzdesini ACPI üzerinden okumaya çalışır.
///
/// ACPI _BST (Battery Status) ve _BIF (Battery Information) metotlarını kullanarak
/// gerçek pil durumunu okur. Donanımda pil yoksa `None` döner.
pub fn get_battery_percent() -> Option<u8> {
    // ACPI Embedded Controller (EC) üzerinden pil durumu oku
    // EC port: 0x66 (komut), 0x62 (veri)
    let (status, remaining, full_capacity) = unsafe {
        use x86_64::instructions::port::Port;
        let mut ec_cmd = Port::<u8>::new(0x66);
        let mut ec_data = Port::<u8>::new(0x62);

        // EC'nin hazır olup olmadığını kontrol et
        let ec_status = ec_cmd.read();
        if ec_status == 0xFF {
            // EC mevcut değil (sanal makine ortamı)
            return None;
        }

        // _BST okuma: pil durumu, kalan kapasite, voltaj
        // EC komut: 0x80 = pil durumu oku
        ec_cmd.write(0x80);
        // Timeout ile bekle
        let mut timeout = 1000u32;
        while ec_cmd.read() & 0x02 != 0 && timeout > 0 {
            timeout -= 1;
        }

        let status = ec_data.read();
        let remaining = ec_data.read() as u32 * 100 + ec_data.read() as u32;
        let full_cap = ec_data.read() as u32 * 100 + ec_data.read() as u32;
        (status, remaining, full_cap)
    };

    if full_capacity == 0 {
        // Pil bilgisi alınamadı (sanal makine veya masaüstü)
        crate::serial_println!("[ACPI] No battery detected (EC status={:#x})", status);
        return None;
    }

    let percent = ((remaining as u64 * 100) / (full_capacity as u64)).min(100) as u8;
    crate::serial_println!(
        "[ACPI] Battery: {}% ({}/{})",
        percent,
        remaining,
        full_capacity
    );
    Some(percent)
}

// ============================================================================
// HOST VALIDATION TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boot::context::{FieldState, RsdpAddressKind, RsdpCandidate, RsdpError, RsdpProvenance};

    /// Checksum'ları doğru sentetik ACPI 1.0 RSDP (20 bayt, rev 0).
    fn make_rsdp_v1() -> [u8; 20] {
        let mut b = [0u8; 20];
        b[0..8].copy_from_slice(b"RSD PTR ");
        b[8..12].copy_from_slice(&0x000F_0000u32.to_le_bytes());
        b[15] = 0; // revision
        let sum: u8 = b.iter().fold(0u8, |a, x| a.wrapping_add(*x));
        b[19] = b[19].wrapping_sub(sum);
        b
    }

    /// Checksum'ları doğru sentetik ACPI 2.0 RSDP (36 bayt, rev 2).
    fn make_rsdp_v2() -> [u8; 36] {
        let mut b = [0u8; 36];
        b[0..8].copy_from_slice(b"RSD PTR ");
        b[15] = 2; // revision
        b[20..24].copy_from_slice(&36u32.to_le_bytes());
        let sum1: u8 = b[..20].iter().fold(0u8, |a, x| a.wrapping_add(*x));
        b[19] = b[19].wrapping_sub(sum1);
        let sum2: u8 = b.iter().fold(0u8, |a, x| a.wrapping_add(*x));
        b[35] = b[35].wrapping_sub(sum2);
        b
    }

    #[test]
    fn validate_rsdp_v1_ok() {
        let rsdp = make_rsdp_v1();
        assert_eq!(validate_rsdp_bytes(&rsdp), Ok((0, 20)));
    }

    #[test]
    fn validate_rsdp_v2_ok() {
        let rsdp = make_rsdp_v2();
        assert_eq!(validate_rsdp_bytes(&rsdp), Ok((2, 36)));
    }

    #[test]
    fn validate_rsdp_bad_signature() {
        let mut rsdp = make_rsdp_v2();
        rsdp[0] = b'X';
        assert_eq!(validate_rsdp_bytes(&rsdp), Err(RsdpError::SignatureMismatch));
    }

    #[test]
    fn validate_rsdp_bad_checksum() {
        let mut rsdp = make_rsdp_v2();
        rsdp[10] = rsdp[10].wrapping_add(1);
        assert_eq!(validate_rsdp_bytes(&rsdp), Err(RsdpError::ChecksumMismatch));
    }

    #[test]
    fn validate_rsdp_bad_length_field() {
        let mut rsdp = make_rsdp_v2();
        rsdp[20..24].copy_from_slice(&20u32.to_le_bytes());
        assert_eq!(
            validate_rsdp_bytes(&rsdp),
            Err(RsdpError::LengthInvalid { declared: 20 })
        );
    }

    #[test]
    fn validate_rsdp_too_short() {
        assert_eq!(
            validate_rsdp_bytes(&[0u8; 10]),
            Err(RsdpError::TooShort { len: 10 })
        );
    }

    #[test]
    fn canonical_accepts_lower_and_upper_half() {
        // Lower half canonical: 0x11E8E8 (MB2 tag fiziksel adresi).
        assert!(is_canonical(0x11E8E8));
        assert!(is_canonical_lower_half(0x11E8E8));
        // Upper half canonical: HHDM penceresi (ör. 0xFFFF_FE00_0000_0000 + off).
        assert!(is_canonical(0xFFFF_FE00_0011_E8E8));
        assert!(!is_canonical_lower_half(0xFFFF_FE00_0011_E8E8));
        // Non-canonical: ne lower ne upper.
        assert!(!is_canonical(0x0000_8000_0000_0000));
        assert!(!is_canonical(0x7FFF_0000_0000_0000));
        // Sınırlar: bit 47 = 0 ve 1 uçları.
        assert!(is_canonical(0x0000_7FFF_FFFF_FFFF));
        assert!(is_canonical(0xFFFF_8000_0000_0000));
        assert!(!is_canonical(0x0000_8000_0000_0000));
    }

    #[test]
    fn publish_rules_sequential() {
        // Global static paylaşıldığı için kurallar tek sıralı testte doğrulanır.
        // (Test koşucusu başına state boş kabul edilir.)
        let v1 = make_rsdp_v1();
        let phys = 0x000F_0000u64;

        // Sıfır adres asla kabul edilmez.
        let zero = RsdpCandidate::new(0, RsdpAddressKind::Physical, RsdpProvenance::Uefi);
        assert_eq!(publish_rsdp(zero), Err(RsdpError::ZeroAddress));

        // Canonical olmayan adres reddedilir.
        let non_canonical =
            RsdpCandidate::new(0xFFFF_8000_0000_0000, RsdpAddressKind::Physical, RsdpProvenance::Uefi);
        assert_eq!(
            publish_rsdp(non_canonical),
            Err(RsdpError::NonCanonicalAddress(0xFFFF_8000_0000_0000))
        );

        // İlk yayın kabul edilir, state PresentUntrusted'tır.
        let first = RsdpCandidate::new(phys, RsdpAddressKind::Physical, RsdpProvenance::Uefi);
        assert!(publish_rsdp(first).is_ok());
        let stored = authoritative_rsdp().unwrap();
        assert_eq!(stored.address, phys);
        assert_eq!(stored.field_state, FieldState::PresentUntrusted);

        // Aynı adres + aynı tür idempotent kabul edilir (provenance korunur).
        let dup = RsdpCandidate::new(phys, RsdpAddressKind::Physical, RsdpProvenance::Limine);
        assert!(publish_rsdp(dup).is_ok());
        assert_eq!(authoritative_rsdp().unwrap().provenance, RsdpProvenance::Uefi);

        // Farklı adres = çelişkili ikinci RSDP → reddedilir.
        let conflicting =
            RsdpCandidate::new(0x000E_0000, RsdpAddressKind::Physical, RsdpProvenance::Limine);
        assert_eq!(
            publish_rsdp(conflicting),
            Err(RsdpError::ConflictingExisting {
                existing: phys,
                incoming: 0x000E_0000
            })
        );

        // Fiziksel/sanal karıştırma yasak.
        let virtual_kind =
            RsdpCandidate::new(phys, RsdpAddressKind::Virtual, RsdpProvenance::Limine);
        assert_eq!(
            publish_rsdp(virtual_kind),
            Err(RsdpError::KindMismatch {
                existing: RsdpAddressKind::Physical,
                incoming: RsdpAddressKind::Virtual
            })
        );

        // Authoritative adres API'si aynı değeri döndürür.
        assert_eq!(rsdp_address(), phys);

        // v1 RSDP baytları state adresiyle ilişkili değildir; doğrulama
        // ayrıca yapılabilir (parse_acpi_tables akışı).
        assert!(validate_rsdp_bytes(&v1).is_ok());
    }
}
