//! # echOS ACPI Event Handler (SCI + GPE)
//!
//! Faz 7: SCI (System Control Interrupt) ve GPE (General Purpose Events).
//! Donanım olaylarını runtime'da yakalar ve AML method'larını çalıştırır.

use core::sync::atomic::{AtomicBool, AtomicU16, Ordering};

/// SCI interrupt numarası (FADT'den okunur)
static SCI_IRQ: AtomicU16 = AtomicU16::new(0);
/// SCI handler aktif mi
static SCI_ACTIVE: AtomicBool = AtomicBool::new(false);

// ============================================================================
// SCI Başlatma
// ============================================================================

/// SCI interrupt handler'ı kur
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

/// SCI aktif mi?
pub fn is_active() -> bool {
    SCI_ACTIVE.load(Ordering::SeqCst)
}

// ============================================================================
// SCI Interrupt Handler
// ============================================================================

/// SCI interrupt geldiğinde çağrılır.
/// PM1 status + GPE status okur ve ilgili event'leri dispatch eder.
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

    // PM1 Status register oku (PM1a_EVT_BLK)
    let pm1_sts = unsafe {
        x86_64::instructions::port::Port::<u16>::new(pm1a_evt).read()
    };

    // Hangi event'ler aktif?
    if pm1_sts & (1 << 8) != 0 {
        // Power button event
        crate::serial_println!("[SCI] Power button pressed");
        handle_power_button();
    }

    if pm1_sts & (1 << 9) != 0 {
        // Sleep button event
        crate::serial_println!("[SCI] Sleep button pressed");
        handle_sleep_button();
    }

    if pm1_sts & (1 << 5) != 0 {
        // Timer event (PM Timer)
        // Genelde ignore edilir
    }

    if pm1_sts & (1 << 0) != 0 {
        // TMR_STS — PM timer overflow
    }

    // Status register'ı temizle (Write-1-to-Clear)
    if pm1_sts != 0 {
        unsafe {
            x86_64::instructions::port::Port::<u16>::new(pm1a_evt).write(pm1_sts);
        }
    }

    // GPE event'lerini kontrol et
    handle_gpe_events();
}

// ============================================================================
// Event Handlers
// ============================================================================

/// Power button basıldı — sistemi kapat
fn handle_power_button() {
    crate::serial_println!("[SCI] → Initiating S5 shutdown");
    // _PTS(5) çalıştır
    let _ = crate::cpu::acpi_power::prepare_to_sleep(5);
}

/// Sleep button basıldı — S3'e geç
fn handle_sleep_button() {
    crate::serial_println!("[SCI] → Sleep requested (S3)");
    let _ = crate::cpu::acpi_power::prepare_to_sleep(3);
}

/// GPE (General Purpose Events) kontrol et ve dispatch et
fn handle_gpe_events() {
    let state = crate::cpu::acpi::ACPI_STATE.lock();
    let fadt_parsed = state.fadt_parsed;
    drop(state);

    if !fadt_parsed {
        return;
    }

    // GPE0/GPE1 block'ları FADT'den okunabilir
    // Basit implementasyon: GPE0 status register oku
    // (QEMU'da genelde GPE yoktur, gerçek donanımda aktif olur)
}

/// Belirli bir GPE event'ini enable et
pub fn enable_gpe(_gpe_number: u8) {
    // GPE enable register'a yaz
    // Gerçek implementasyon FADT GPE0/GPE1 block adreslerini kullanır
    crate::serial_println!("[GPE] GPE#{} enabled", _gpe_number);
}

/// EC SCI event handler — EC query ve _Qxx method dispatch
pub fn handle_ec_sci() {
    if !crate::cpu::acpi_ec::is_available() {
        return;
    }

    // EC query: hangi event tetiklendi?
    if let Some(query_val) = crate::cpu::acpi_ec::ec_query() {
        let method = alloc::format!("\\_SB.PCI0.LPC.EC._Q{:02X}", query_val);
        crate::serial_println!("[EC-SCI] Query=0x{:02X} → {}", query_val, method);

        match crate::cpu::acpi_aml::invoke_method(&method, &[]) {
            Ok(_) => crate::serial_println!("[EC-SCI] {} executed", method),
            Err(_) => crate::serial_println!("[EC-SCI] {} not found", method),
        }
    }
}

// ============================================================================
// Notify Handler
// ============================================================================

/// ACPI Notify event handler — cihaz durumu değiştiğinde çağrılır
pub fn handle_notify(device_path: &str, notify_value: u32) {
    crate::serial_println!("[NOTIFY] {} → 0x{:X}", device_path, notify_value);

    match notify_value {
        0x00 => crate::serial_println!("[NOTIFY] Bus Check"),
        0x01 => crate::serial_println!("[NOTIFY] Device Check"),
        0x02 => crate::serial_println!("[NOTIFY] Device Wake"),
        0x03 => crate::serial_println!("[NOTIFY] Eject Request"),
        0x80 => crate::serial_println!("[NOTIFY] Status Change (battery/thermal)"),
        0x81 => crate::serial_println!("[NOTIFY] Information Change"),
        _ => {}
    }
}
