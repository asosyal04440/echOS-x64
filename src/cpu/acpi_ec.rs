//! # echOS ACPI Embedded Controller (EC) Driver
//!
//! Faz 6: EC erişimi — port 0x62 (data) / 0x66 (command).
//! Laptop'larda fan, sıcaklık, batarya, lid switch EC üzerinden kontrol edilir.

/// EC komut portları
const EC_DATA_PORT: u16 = 0x62;
const EC_CMD_PORT: u16 = 0x66;

/// EC komutları
const EC_CMD_READ: u8 = 0x80;
const EC_CMD_WRITE: u8 = 0x81;
const EC_CMD_BURST_ENABLE: u8 = 0x82;
const EC_CMD_BURST_DISABLE: u8 = 0x83;
const EC_CMD_QUERY: u8 = 0x84;

/// EC status register bit'leri
const EC_STATUS_OBF: u8 = 0x01;  // Output buffer full
const EC_STATUS_IBF: u8 = 0x02;  // Input buffer full
const EC_STATUS_BURST: u8 = 0x10; // Burst mode

/// EC başlatıldı mı
static EC_AVAILABLE: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// EC'nin varlığını kontrol et ve başlat
pub fn init_ec() {
    // ACPI namespace'te EC objesini ara
    if !crate::cpu::acpi_aml::is_initialized() {
        crate::serial_println!("[EC] AML not initialized — EC skipped");
        return;
    }

    // EC genelde \_SB.PCI0.LPC.EC veya \_SB.EC altında olur
    let ec_paths = [
        "\\_SB.PCI0.LPC.EC",
        "\\_SB.PCI0.LPCB.EC0",
        "\\_SB.EC",
        "\\_SB.EC0",
    ];

    for path in &ec_paths {
        if let Ok(_) = crate::cpu::acpi_aml::invoke_method(
            &alloc::format!("{}._HID", path), &[]
        ) {
            EC_AVAILABLE.store(true, core::sync::atomic::Ordering::SeqCst);
            crate::serial_println!("[EC] Embedded Controller found at {}", path);
            return;
        }
    }

    // EC namespace'te yoksa I/O port probe dene
    let status = unsafe {
        x86_64::instructions::port::Port::<u8>::new(EC_CMD_PORT).read()
    };
    if status != 0xFF {
        EC_AVAILABLE.store(true, core::sync::atomic::Ordering::SeqCst);
        crate::serial_println!("[EC] EC detected via I/O port probe (status=0x{:02X})", status);
    } else {
        crate::serial_println!("[EC] No Embedded Controller found (desktop/VM)");
    }
}

/// EC mevcut mu?
pub fn is_available() -> bool {
    EC_AVAILABLE.load(core::sync::atomic::Ordering::SeqCst)
}

/// EC'den bir byte oku
pub fn ec_read(offset: u8) -> Option<u8> {
    if !is_available() {
        return None;
    }

    // Input buffer boşalmasını bekle
    if !wait_ibf_clear() {
        return None;
    }

    // READ komutu gönder
    unsafe {
        x86_64::instructions::port::Port::<u8>::new(EC_CMD_PORT).write(EC_CMD_READ);
    }

    // Offset gönder
    if !wait_ibf_clear() {
        return None;
    }
    unsafe {
        x86_64::instructions::port::Port::<u8>::new(EC_DATA_PORT).write(offset);
    }

    // Output buffer dolmasını bekle
    if !wait_obf_set() {
        return None;
    }

    // Veriyi oku
    let data = unsafe {
        x86_64::instructions::port::Port::<u8>::new(EC_DATA_PORT).read()
    };

    Some(data)
}

/// EC'ye bir byte yaz
pub fn ec_write(offset: u8, value: u8) -> bool {
    if !is_available() {
        return false;
    }

    if !wait_ibf_clear() {
        return false;
    }

    // WRITE komutu
    unsafe {
        x86_64::instructions::port::Port::<u8>::new(EC_CMD_PORT).write(EC_CMD_WRITE);
    }

    if !wait_ibf_clear() {
        return false;
    }

    // Offset
    unsafe {
        x86_64::instructions::port::Port::<u8>::new(EC_DATA_PORT).write(offset);
    }

    if !wait_ibf_clear() {
        return false;
    }

    // Value
    unsafe {
        x86_64::instructions::port::Port::<u8>::new(EC_DATA_PORT).write(value);
    }

    true
}

/// EC query — en son hangi event tetiklendi?
pub fn ec_query() -> Option<u8> {
    if !is_available() {
        return None;
    }

    if !wait_ibf_clear() {
        return None;
    }

    unsafe {
        x86_64::instructions::port::Port::<u8>::new(EC_CMD_PORT).write(EC_CMD_QUERY);
    }

    if !wait_obf_set() {
        return None;
    }

    let query_val = unsafe {
        x86_64::instructions::port::Port::<u8>::new(EC_DATA_PORT).read()
    };

    if query_val == 0 {
        None
    } else {
        Some(query_val)
    }
}

/// Input buffer boşalmasını bekle (timeout: ~1ms)
fn wait_ibf_clear() -> bool {
    for _ in 0..1000 {
        let status = unsafe {
            x86_64::instructions::port::Port::<u8>::new(EC_CMD_PORT).read()
        };
        if status & EC_STATUS_IBF == 0 {
            return true;
        }
        // Kısa bekleme
        core::hint::spin_loop();
    }
    false
}

/// Output buffer dolmasını bekle (timeout: ~1ms)
fn wait_obf_set() -> bool {
    for _ in 0..1000 {
        let status = unsafe {
            x86_64::instructions::port::Port::<u8>::new(EC_CMD_PORT).read()
        };
        if status & EC_STATUS_OBF != 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}
