//! # echOS ACPI Gömülü Denetleyici (EC) Sürücüsü
//!
//! ## Embedded Controller (Gömülü Denetleyici) Nedir?
//! EC, dizüstü bilgisayarlarda yaygın olarak bulunan küçük bir mikrodenetleyicidir.
//! Ana CPU ile birlikte çalışarak şu görevleri üstlenir:
//!
//! ```text
//!  ┌─────────────────────────────────────────────────────┐
//!  │              Embedded Controller (EC)               │
//!  │  - Batarya durumu (şarj seviyesi, gerilim, akım)    │
//!  │  - Fan hızı kontrolü (termal yönetim)               │
//!  │  - Güç düğmesi / uyku düğmesi sinyalleri            │
//!  │  - Kapak (lid) açık/kapalı durumu                   │
//!  │  - Klavye arka aydınlatması                         │
//!  │  - Termal sensör okumaları                          │
//!  └─────────────────────────────────────────────────────┘
//! ```
//!
//! ## EC İletişim Protokolü
//! EC ile iletişim iki I/O portu üzerinden gerçekleşir:
//!
//! ```text
//!  Port 0x62 (EC_DATA) — Veri okuma/yazma
//!  Port 0x66 (EC_CMD)  — Komut yazma ve durum okuma
//!
//!  Durum Kaydı (Port 0x66'dan okunur):
//!   Bit 0 (OBF): Çıkış Tamponu Dolu — okunabilecek veri var
//!   Bit 1 (IBF): Giriş Tamponu Dolu — EC komutu/veriyi işliyor
//!   Bit 4 (BURST): Burst modu etkin
//!
//!  Okuma Dizisi:
//!   1. IBF=0 bekle (EC hazır)
//!   2. EC_CMD portuna 0x80 (READ) yaz
//!   3. IBF=0 bekle
//!   4. EC_DATA portuna offset yaz
//!   5. OBF=1 bekle (veri hazır)
//!   6. EC_DATA portundan veriyi oku
//!
//!  Yazma Dizisi:
//!   1. IBF=0 bekle
//!   2. EC_CMD portuna 0x81 (WRITE) yaz
//!   3. IBF=0 bekle
//!   4. EC_DATA portuna offset yaz
//!   5. IBF=0 bekle
//!   6. EC_DATA portuna değeri yaz
//! ```
//!
//! Faz 6: EC erişimi — port 0x62 (data) / 0x66 (command).

/// EC veri portu — okuma/yazma işlemleri için veri buradan aktarılır
const EC_DATA_PORT: u16 = 0x62;
/// EC komut/durum portu — komut yazılır, durum okunur
const EC_CMD_PORT: u16 = 0x66;

/// EC komutları — ACPI Spec §12.3 (Embedded Controller Interface)
const EC_CMD_READ: u8 = 0x80; // EC bellek alanından bir bayt oku
const EC_CMD_WRITE: u8 = 0x81; // EC bellek alanına bir bayt yaz
const EC_CMD_BURST_ENABLE: u8 = 0x82; // Burst modunu etkinleştir (toplu erişim için)
const EC_CMD_BURST_DISABLE: u8 = 0x83; // Burst modunu devre dışı bırak
const EC_CMD_QUERY: u8 = 0x84; // GPE sorgula — en son hangi olay tetiklendi?

/// EC durum kaydı bit maskeleri — EC_CMD_PORT'tan okunur
const EC_STATUS_OBF: u8 = 0x01; // Çıkış Tamponu Dolu (Output Buffer Full) — okuma mümkün
const EC_STATUS_IBF: u8 = 0x02; // Giriş Tamponu Dolu (Input Buffer Full) — EC meşgul
const EC_STATUS_BURST: u8 = 0x10; // Burst modu etkin — birden fazla erişim için optimize

/// EC varlığını yapılandırmada atomik olarak izler.
/// Masaüstü sistemlerde veya sanal makinelerde EC genellikle bulunmaz.
static EC_AVAILABLE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// EC'yi keşfeder ve başlatır.
///
/// İki yöntemle EC varlığı tespit edilir:
/// 1. ACPI namespace'te `_HID` ile EC nesnesi aranır
/// 2. EC I/O portu probe edilir — 0xFF dışında bir değer EC varlığına işaret eder
pub fn init_ec() {
    // AML başlatılmadan EC namespace'i taranamaz
    if !crate::cpu::acpi_aml::is_initialized() {
        crate::serial_println!("[EC] AML not initialized — EC skipped");
        return;
    }

    // EC, ACPI namespace'te genellikle bu yollardan birinde bulunur
    let ec_paths = [
        "\\_SB.PCI0.LPC.EC",   // Intel platformu (i440fx/q35)
        "\\_SB.PCI0.LPCB.EC0", // Intel platformu (alternatif)
        "\\_SB.EC",            // Basit platform
        "\\_SB.EC0",           // Basit platform (alternatif)
    ];

    for path in &ec_paths {
        if let Ok(_) = crate::cpu::acpi_aml::invoke_method(&alloc::format!("{}._HID", path), &[]) {
            EC_AVAILABLE.store(true, core::sync::atomic::Ordering::SeqCst);
            crate::serial_println!("[EC] Embedded Controller found at {}", path);
            return;
        }
    }

    // ACPI namespace'te bulunamazsa doğrudan I/O port probe yöntemi kullan
    // EC_CMD_PORT'tan okunan değer 0xFF ise EC yok; başka bir değer EC varlığını gösterir
    let status = unsafe { x86_64::instructions::port::Port::<u8>::new(EC_CMD_PORT).read() };
    if status != 0xFF {
        EC_AVAILABLE.store(true, core::sync::atomic::Ordering::SeqCst);
        crate::serial_println!(
            "[EC] EC detected via I/O port probe (status=0x{:02X})",
            status
        );
    } else {
        crate::serial_println!("[EC] No Embedded Controller found (desktop/VM)");
    }
}

/// EC donanımının mevcut ve kullanılabilir olup olmadığını döndürür.
pub fn is_available() -> bool {
    EC_AVAILABLE.load(core::sync::atomic::Ordering::SeqCst)
}

/// EC bellek alanından bir bayt okur.
///
/// EC'nin 256 baytlık iç bellek alanına (EC Space) erişir.
/// Offset parametresi EC bellek adresini belirtir (0x00-0xFF).
/// Timeout durumunda `None` döner.
pub fn ec_read(offset: u8) -> Option<u8> {
    if !is_available() {
        return None;
    }

    // Giriş tamponu boşalana kdar bekle — EC önceki komutu işliyor olabilir
    if !wait_ibf_clear() {
        return None;
    }

    // READ komutu (0x80) gönder
    unsafe {
        x86_64::instructions::port::Port::<u8>::new(EC_CMD_PORT).write(EC_CMD_READ);
    }

    // Offset adresini gitmeden önce giriş tamponu boşalmalı
    if !wait_ibf_clear() {
        return None;
    }
    unsafe {
        x86_64::instructions::port::Port::<u8>::new(EC_DATA_PORT).write(offset);
    }

    // Çıkış tamponu dolmasını bekle — EC yanıtı hazırladı
    if !wait_obf_set() {
        return None;
    }

    // Yanıt verisini oku
    let data = unsafe { x86_64::instructions::port::Port::<u8>::new(EC_DATA_PORT).read() };

    Some(data)
}

/// EC bellek alanına bir bayt yazar.
///
/// Offset parametresi EC bellek adresini, value ise yazılacak değeri belirtir.
/// Başarı durumunda `true`, timeout veya EC yoksa `false` döner.
pub fn ec_write(offset: u8, value: u8) -> bool {
    if !is_available() {
        return false;
    }

    if !wait_ibf_clear() {
        return false;
    }

    // WRITE komutu (0x81) gönder
    unsafe {
        x86_64::instructions::port::Port::<u8>::new(EC_CMD_PORT).write(EC_CMD_WRITE);
    }

    if !wait_ibf_clear() {
        return false;
    }

    // Hedef offset adresini gönder
    unsafe {
        x86_64::instructions::port::Port::<u8>::new(EC_DATA_PORT).write(offset);
    }

    if !wait_ibf_clear() {
        return false;
    }

    // Yazılacak değeri gönder
    unsafe {
        x86_64::instructions::port::Port::<u8>::new(EC_DATA_PORT).write(value);
    }

    true
}

/// EC olay sorgular (Query) — son tetiklenen EC olayının numarasını döndürür.
///
/// EC, belirli bir olay gerçekleştiğinde (örn: pil durumu değişimi, kapak açma/kapama)
/// SCI (System Control Interrupt) üretir. Bu fonksiyon hangi olayın tetiklendiğini öğrenir.
/// Dönen değer, `_Q<XX>` AML metodunu çalıştırmak için kullanılır (EC-SCI dispatch).
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

    let query_val = unsafe { x86_64::instructions::port::Port::<u8>::new(EC_DATA_PORT).read() };

    if query_val == 0 {
        None // 0 değeri bekleyen olay olmadığını gösterir
    } else {
        Some(query_val)
    }
}

/// Giriş tamponunun boşalmasını spinloop ile bekler (yaklaşık 1 ms timeout, ~1000 iterasyon).
///
/// IBF=1 iken EC comando almaya hazır değildir; önceki komutu işlemeyi beklemek gerekir.
/// Timeout gerçekleşirse `false` döner ve işlem iptal edilebilir.
fn wait_ibf_clear() -> bool {
    for _ in 0..1000 {
        let status = unsafe { x86_64::instructions::port::Port::<u8>::new(EC_CMD_PORT).read() };
        if status & EC_STATUS_IBF == 0 {
            return true; // Giriş tamponu boş — yazma güvenli
        }
        // CPU'ya çevir ipucu: meşgul bekleme sırasında güç tasarrufu möd
        core::hint::spin_loop();
    }
    false // Zaman aşımı
}

/// Çıkış tamponunun dolmasını spinloop ile bekler (yaklaşık 1 ms timeout, ~1000 iterasyon).
///
/// OBF=1 olana kadar okuma yapılmamalıdır; bu bit EC'nin yanıtı hazırladığını gösterir.
/// Timeout gerçekleşirse `false` döner.
fn wait_obf_set() -> bool {
    for _ in 0..1000 {
        let status = unsafe { x86_64::instructions::port::Port::<u8>::new(EC_CMD_PORT).read() };
        if status & EC_STATUS_OBF != 0 {
            return true; // Çıkış tamponu dolu — okuma güvenli
        }
        core::hint::spin_loop();
    }
    false // Zaman aşımı
}
