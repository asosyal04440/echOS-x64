//! # echOS ACPI Olay Yöneticisi (SCI + GPE)
//!
//! ## SCI (System Control Interrupt) Nedir?
//! SCI, ACPI'nin işletim sistemine donanım olaylarını bildirmek için kullandığı
//! paylaşımlı bir kesmedir (shared interrupt). IRQ numarası FADT'nin `SCI_INT` alanından okunur
//! (genellikle IRQ 9). Geleneksel IRQ'ların aksine SCI seviye tetiklemeli (level-triggered) ve
//! aktif-düşük (active-low) çalışır.
//!
//! ```text
//! SCI Olay Akışı:
//!   Donanım Olayı
//!        │
//!        ▼
//!   PM1_STS veya GPE_STS register'ı set olur
//!        │
//!        ▼
//!   SCI kesmesi tetiklenir (FADT'deki IRQ numarasıyla)
//!        │
//!        ▼
//!   handle_sci() çağrılır
//!        │
//!        ├─► PM1_STS'i oku ve temizle (Write-1-to-Clear)
//!        │       ├─► Bit 8: Güç düğmesi → S5'e geç
//!        │       └─► Bit 9: Uyku düğmesi → S3'e geç
//!        │
//!        └─► GPE_STS'i oku → ilgili _Lxx/_Exx metodunu çalıştır
//! ```
//!
//! ## GPE (General Purpose Events) Nedir?
//! GPE, ACPI'nin donanım bağımlı olayları bildirmesi için genel amaçlı olay mekanizmasıdır.
//! Her GPE bit'i bir donanım olayına karşılık gelir ve ilgili `_Lxx` (level) veya
//! `_Exx` (edge) AML metodu çalıştırılır. Örn: GPE#0x17 → `_L17` metodu.
//!
//! Faz 7: SCI (System Control Interrupt) ve GPE (General Purpose Events).
//! Donanım olayları runtime'da yakalanır ve ilgili AML metodları çalıştırılır.

use core::sync::atomic::{AtomicBool, AtomicU16, Ordering};

/// SCI interrupt numarası — FADT'deki `SCI_INT` alanından okunur (genellikle IRQ 9)
static SCI_IRQ: AtomicU16 = AtomicU16::new(0);
/// SCI kesme işleyicisinin aktif olup olmadığını atomik olarak izler
static SCI_ACTIVE: AtomicBool = AtomicBool::new(false);

// ============================================================================
// SCI Başlatma
// ============================================================================

/// SCI kesme işleyicisini başlatır ve FADT'deki IRQ numarasına kaydeder.
///
/// SCI etkinleştirilmeden önce PM1 ve GPE enable bitleri set edilmelidir;
/// bu implementasyon süreci loglayarak sonraki aşamalara hazırlanır.
pub fn init_sci() {
    if !crate::cpu::acpi_aml::is_initialized() {
        crate::serial_println!("[SCI] AML not initialized — SCI handler skipped");
        return;
    }

    let state = crate::cpu::acpi::ACPI_STATE.lock();
    let sci_int = state.sci_interrupt;
    let pm1a_evt = state.pm1a_evt_blk;
    drop(state);

    if sci_int == 0 {
        crate::serial_println!("[SCI] SCI interrupt not defined in FADT");
        return;
    }

    SCI_IRQ.store(sci_int, Ordering::SeqCst);
    SCI_ACTIVE.store(true, Ordering::SeqCst);

    crate::serial_println!(
        "[SCI] SCI registered on IRQ {} (PM1a_EVT=0x{:X})",
        sci_int,
        pm1a_evt
    );
}

/// SCI işleyicisinin aktif olup olmadığını döndürür.
pub fn is_active() -> bool {
    SCI_ACTIVE.load(Ordering::SeqCst)
}

// ============================================================================
// SCI Kesme İşleyicisi
// ============================================================================

/// SCI kesmesi geldiğinde çağrılır.
///
/// PM1 Status ve GPE Status kayıtlarını okur, hangi ACPI olayının tetiklendiğini tespit eder
/// ve ilgili olay işleyicisini çalıştırır.
///
/// PM1_STS kaydı "Write-1-to-Clear" yöntemiyle temizlenir:
/// bitleri sıfırlamak için o bitlere 1 yazmak gerekir.
pub fn handle_sci() {
    if !is_active() {
        return;
    }

    let state = crate::cpu::acpi::ACPI_STATE.lock();
    let pm1a_evt = state.pm1a_evt_blk;
    drop(state);

    if pm1a_evt == 0 {
        return;
    }

    // PM1a_EVT_BLK Status kaydını oku — hangi ACPI olaylarının tetiklendiğini gösterir
    let pm1_sts = unsafe { x86_64::instructions::port::Port::<u16>::new(pm1a_evt).read() };

    // PM1_STS bit haritası kontrolü — ACPI Spec §4.8.3:
    if pm1_sts & (1 << 8) != 0 {
        // Bit 8: PWRBTN_STS — Güç düğmesine basıldı
        crate::serial_println!("[SCI] Power button pressed");
        handle_power_button();
    }

    if pm1_sts & (1 << 9) != 0 {
        // Bit 9: SLPBTN_STS — Uyku düğmesine basıldı
        crate::serial_println!("[SCI] Sleep button pressed");
        handle_sleep_button();
    }

    if pm1_sts & (1 << 5) != 0 {
        // Bit 5: GBL_STS — Global Lock çakışması; genellikle görmezden gelinir
    }

    if pm1_sts & (1 << 0) != 0 {
        // Bit 0: TMR_STS — PM Timer 24 bitlik sayacı taştı (genel olarak yok sayılır)
    }

    // Status kaydını temizle: Write-1-to-Clear ile okunan bitleri sıfırla
    if pm1_sts != 0 {
        unsafe {
            x86_64::instructions::port::Port::<u16>::new(pm1a_evt).write(pm1_sts);
        }
    }

    // GPE olay kayıtlarını kontrol et ve ilgili AML metodlarını çalıştır
    handle_gpe_events();
}

// ============================================================================
// Olay İşleyicileri
// ============================================================================

/// Güç düğmesine basılınca çağrılır — sistemi S5 (Soft Off) durumuna geçirir.
fn handle_power_button() {
    crate::serial_println!("[SCI] → Initiating S5 shutdown");
    // _PTS(5) metodunu çalıştır — S5'e geçiş öncesi hazırlık
    let _ = crate::cpu::acpi_power::prepare_to_sleep(5);
}

/// Uyku düğmesine basılınca çağrılır — sistemi S3 (Askı — Suspend to RAM) durumuna geçirir.
fn handle_sleep_button() {
    crate::serial_println!("[SCI] → Sleep requested (S3)");
    // _PTS(3) metodunu çalıştır — S3 askı moduna geçiş hazırlığı
    let _ = crate::cpu::acpi_power::prepare_to_sleep(3);
}

/// GPE (General Purpose Events) kayıtlarını okur ve ilgili AML metodlarını çalıştırır.
///
/// GPE0/GPE1 blokları FADT'den okunur; her bit bir olay kanalına karşılık gelir.
/// Gerçek donanımda bu metodlar fan değişimi, sıcaklık uyarısı gibi olayları yakalar.
/// QEMU sanal makinesinde GPE genellikle tanımsız veya boştur.
fn handle_gpe_events() {
    let state = crate::cpu::acpi::ACPI_STATE.lock();
    let fadt_parsed = state.fadt_parsed;
    drop(state);

    if !fadt_parsed {
        return;
    }

    // GPE0/GPE1 blok adresleri FADT'den okunabilir
    // Basit implementasyon: QEMU'da genelde GPE yoktur; gerçek donanımda aktif olur
    // Tam implementasyon GPE durum bitlerini tarar ve _L<XX> veya _E<XX> metodlarını çağırır
}

/// Belirli bir GPE kanalını etkinleştirir.
///
/// GPE enable register'ına ilgili bit yazılarak donanım olayları dinlenmeye başlanır.
/// Gerçek implementasyon FADT GPE0/GPE1 blok adreslerini kullanır.
pub fn enable_gpe(_gpe_number: u8) {
    // GPE enable register'ına ilgili bit yazılır
    // Tam implementasyon FADT'deki GPE0_BLK ve GPE1_BLK adreslerini kullanır
    crate::serial_println!("[GPE] GPE#{} enabled", _gpe_number);
}

/// EC SCI olay işleyicisi — EC Query komutu ve `_Qxx` AML metod dispatch.
///
/// EC bir olay tetiklediğinde (örn: pil durumu değişimi, fan hızı değişimi) SCI üretir.
/// Bu fonksiyon EC Query komutu ile hangi olayın tetiklendiğini öğrenir ve
/// `_Q<XX>` AML metodunu çalıştırır.
pub fn handle_ec_sci() {
    if !crate::cpu::acpi_ec::is_available() {
        return;
    }

    // EC Query: 0x84 komutuyla son tetiklenen olayın numarasını al
    if let Some(query_val) = crate::cpu::acpi_ec::ec_query() {
        let method = alloc::format!("\\_SB.PCI0.LPC.EC._Q{:02X}", query_val);
        crate::serial_println!("[EC-SCI] Query=0x{:02X} → {}", query_val, method);

        // _Q<XX> metodunu çalıştır — EC olay işleyicisi AML kodu
        match crate::cpu::acpi_aml::invoke_method(&method, &[]) {
            Ok(_) => crate::serial_println!("[EC-SCI] {} executed", method),
            Err(_) => crate::serial_println!("[EC-SCI] {} not found", method),
        }
    }
}

// ============================================================================
// Notify Olay İşleyicisi
// ============================================================================

/// ACPI Notify olay işleyicisi — bir cihazın durumu değiştiğinde firmware bu olayı gönderir.
///
/// Notify olayları şu durumlarda gönderilir:
/// - Sıcak takma/çıkarma (hot-plug): bellek, CPU, PCIe kartı
/// - Pil / şarj durumu değişimi
/// - Termal eşik aşımı
///
/// `notify_value` standardize edilmiş değerler içerir (ACPI Spec §5.6.6).
pub fn handle_notify(device_path: &str, notify_value: u32) {
    crate::serial_println!("[NOTIFY] {} → 0x{:X}", device_path, notify_value);

    match notify_value {
        0x00 => crate::serial_println!("[NOTIFY] Bus Check — bus topolojisini yeniden tara"),
        0x01 => crate::serial_println!("[NOTIFY] Device Check — cihaz listesini güncelle"),
        0x02 => crate::serial_println!("[NOTIFY] Device Wake — cihaz uykudan uyandı"),
        0x03 => crate::serial_println!("[NOTIFY] Eject Request — cihaz çıkarma isteği"),
        0x80 => crate::serial_println!("[NOTIFY] Status Change — pil/termal durum değişti"),
        0x81 => crate::serial_println!("[NOTIFY] Information Change — cihaz bilgisi güncellendi"),
        _ => {} // Diğer vendor-specific bildirimler sessizce geçilir
    }
}
