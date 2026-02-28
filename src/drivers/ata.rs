//! # echOS ATA Sürücüsü (PIO Modu)
//!
//! IDE/ATA disk sürücüsü implementasyonu.
//! Disk algılama, sektör okuma/yazma ve IDENTIFY komutlarını destekler.
//!
//! ## ATA PIO Komut Akışı
//!
//! ```
//!  CPU                     ATA Denetleyici               Disk
//!   |                           |                           |
//!   |-- select_drive(lba) ----->|                           |
//!   |-- cmd yazma ------------->|-- komut fiziksel gönder ->|
//!   |                           |<-- BSY=1 (meşgul) --------|
//!   |-- wait_busy() loop ------>|                           |
//!   |                           |<-- DRQ=1 (veri hazır) ----|
//!   |-- data okuma (256 word) ->|<-- 512 byte veri ---------|
//!   |   (her word 16 bit)       |                           |
//! ```
//!
//! - **LBA28**: 28-bit mantıksal blok adresleme, ~128 GB'a kadar disk desteği.
//! - **LBA48**: 48-bit adresleme, günümüz büyük diskler için.
//! - **PIO**: Programmed I/O — DMA olmadan CPU tüm veriyi port üzerinden taşır.
//!   Yavaş ama basit; eğitim amaçlı idealdir.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use x86_64::instructions::port::{Port, PortReadOnly, PortWriteOnly};

/// Sektör boyutu (512 byte)
pub const BLOCK_SIZE: usize = 512;

/// ATA komut kodları
mod commands {
    pub const READ_SECTORS: u8 = 0x20;
    pub const WRITE_SECTORS: u8 = 0x30;
    pub const IDENTIFY: u8 = 0xEC;
    pub const FLUSH_CACHE: u8 = 0xE7;
}

/// ATA status register bitleri
mod status {
    pub const BSY: u8 = 0x80; // Meşgul
    #[allow(dead_code)]
    pub const DRDY: u8 = 0x40; // Sürücü hazır
    pub const DRQ: u8 = 0x08; // Veri isteği
    pub const ERR: u8 = 0x01; // Hata
}

/// ATA sürücüsü hata türleri.
///
/// PIO modunda disk okuma/yazma işlemleri sırasında oluşabilecek hatalar.
/// Her varyant farklı bir hata durumunu temsil eder.
#[derive(Debug, Clone, Copy)]
pub enum AtaError {
    DriveNotFound,
    NotAta,
    Timeout,
    ReadError,
    WriteError,
    InvalidParameter,
}

/// Disk bilgisi (IDENTIFY komutundan gelen).
#[derive(Debug, Clone)]
pub struct DriveInfo {
    pub model: String,
    pub serial: String,
    pub firmware: String,
    pub sectors: u64,
    pub size_mb: u64,
    pub lba48_supported: bool,
}

/// ATA disk kontrolcüsü yapısı.
pub struct AtaDrive {
    #[allow(dead_code)]
    base: u16,
    data: Port<u16>,
    #[allow(dead_code)]
    error: PortReadOnly<u8>,
    features: PortWriteOnly<u8>,
    sector_count: Port<u8>,
    lba_low: Port<u8>,
    lba_mid: Port<u8>,
    lba_high: Port<u8>,
    device: Port<u8>,
    command: PortWriteOnly<u8>,
    status: PortReadOnly<u8>,
    is_slave: bool,
}

impl AtaDrive {
    /// Yeni bir ATA sürücüsü oluşturur.
    /// master/slave seçimi varsayılan olarak master.
    pub const fn new(base: u16) -> Self {
        Self::with_slave(base, false)
    }

    /// Master veya Slave olarak yapılandırılmış sürücü oluşturur.
    /// base: 0x1F0 (Primary) veya 0x170 (Secondary)
    pub const fn with_slave(base: u16, is_slave: bool) -> Self {
        Self {
            base,
            data: Port::new(base),
            error: PortReadOnly::new(base + 1),
            features: PortWriteOnly::new(base + 1),
            sector_count: Port::new(base + 2),
            lba_low: Port::new(base + 3),
            lba_mid: Port::new(base + 4),
            lba_high: Port::new(base + 5),
            device: Port::new(base + 6),
            command: PortWriteOnly::new(base + 7),
            status: PortReadOnly::new(base + 7),
            is_slave,
        }
    }

    /// Sürücünün meşguliyetinin bitmesini bekler.
    fn wait_busy(&mut self) {
        while unsafe { self.status.read() } & status::BSY != 0 {
            core::hint::spin_loop();
        }
    }

    /// Sürücünün hazır olmasını bekler.
    fn wait_ready(&mut self) -> Result<(), AtaError> {
        let mut timeout = 100000;
        while unsafe { self.status.read() } & status::DRQ == 0 {
            if timeout == 0 {
                return Err(AtaError::Timeout);
            }
            timeout -= 1;
            core::hint::spin_loop();
        }
        Ok(())
    }

    /// Hata durumunu kontrol eder.
    fn check_error(&mut self) -> Result<(), AtaError> {
        let status = unsafe { self.status.read() };
        if status & status::ERR != 0 {
            Err(AtaError::ReadError)
        } else {
            Ok(())
        }
    }

    /// Sürücüyü seçer (Master/Slave ve LBA bitleri).
    fn select_drive(&mut self, lba: u32) {
        let drive_bits = if self.is_slave { 0xF0 } else { 0xE0 };
        unsafe {
            self.device.write(drive_bits | ((lba >> 24) as u8 & 0x0F));
        }
    }

    /// Sürücünün var olup olmadığını algılar.
    pub fn detect(&mut self) -> Result<bool, AtaError> {
        let drive_bits = if self.is_slave { 0xB0 } else { 0xA0 };
        unsafe {
            self.device.write(drive_bits);
            self.command.write(commands::IDENTIFY);
        }

        // Kısa bir bekleme
        for _ in 0..15 {
            unsafe {
                self.status.read();
            }
        }

        let status = unsafe { self.status.read() };
        if status == 0 {
            return Ok(false); // Sürücü yok
        }

        let mut spins: u32 = 1000;
        while unsafe { self.status.read() } & status::BSY != 0 {
            if spins == 0 {
                return Err(AtaError::Timeout);
            }
            spins = spins.saturating_sub(1);
            core::hint::spin_loop();
        }

        // ATA sürücüsü mü kontrol et (ATAPI farklı değerlere sahip)
        let lba_mid = unsafe { self.lba_mid.read() };
        let lba_high = unsafe { self.lba_high.read() };

        if lba_mid != 0 || lba_high != 0 {
            return Err(AtaError::NotAta); // Muhtemelen ATAPI
        }

        Ok(true)
    }

    /// Sürücü bilgilerini (IDENTIFY) okur.
    pub fn get_info(&mut self) -> Result<DriveInfo, AtaError> {
        let drive_bits = if self.is_slave { 0xA0 } else { 0xA0 };
        unsafe {
            self.device.write(drive_bits);
            self.sector_count.write(0);
            self.lba_low.write(0);
            self.lba_mid.write(0);
            self.lba_high.write(0);
            self.command.write(commands::IDENTIFY);
        }

        let status = unsafe { self.status.read() };
        if status == 0 {
            return Err(AtaError::DriveNotFound);
        }

        self.wait_busy();
        self.wait_ready()?;

        // 256 word (512 byte) veriyi oku
        let mut data = [0u16; 256];
        for i in 0..256 {
            data[i] = unsafe { self.data.read() };
        }

        // Bilgileri ayrıştır
        let serial = Self::parse_ata_string(&data[10..20]);
        let firmware = Self::parse_ata_string(&data[23..27]);
        let model = Self::parse_ata_string(&data[27..47]);

        // LBA28 sektör sayısı
        let sectors_28 = (data[61] as u64) << 16 | (data[60] as u64);

        // LBA48 desteği kontrolü
        let lba48_supported = (data[83] & (1 << 10)) != 0;

        // LBA48 sektör sayısı
        let sectors = if lba48_supported {
            (data[103] as u64) << 48
                | (data[102] as u64) << 32
                | (data[101] as u64) << 16
                | (data[100] as u64)
        } else {
            sectors_28
        };

        let size_mb = sectors * BLOCK_SIZE as u64 / (1024 * 1024);

        Ok(DriveInfo {
            model,
            serial,
            firmware,
            sectors,
            size_mb,
            lba48_supported,
        })
    }

    /// Byte-swapped ATA string'ini düzeltir.
    fn parse_ata_string(words: &[u16]) -> String {
        let mut chars = Vec::new();
        for word in words {
            chars.push((*word >> 8) as u8);
            chars.push((*word & 0xFF) as u8);
        }
        String::from_utf8_lossy(&chars).trim().to_string()
    }

    /// Sektör okur.
    pub fn read_sectors(&mut self, lba: u32, count: u8) -> Vec<u8> {
        self.wait_busy();
        self.select_drive(lba);

        unsafe {
            self.features.write(0x00);
            self.sector_count.write(count);
            self.lba_low.write(lba as u8);
            self.lba_mid.write((lba >> 8) as u8);
            self.lba_high.write((lba >> 16) as u8);
            self.command.write(commands::READ_SECTORS);
        }

        let mut buffer = Vec::with_capacity((count as usize) * BLOCK_SIZE);

        for _ in 0..count {
            self.wait_busy();
            let _ = self.wait_ready();

            for _ in 0..256 {
                let data = unsafe { self.data.read() };
                buffer.push((data & 0xFF) as u8);
                buffer.push((data >> 8) as u8);
            }
        }

        buffer
    }

    /// Sektör yazar.
    pub fn write_sectors(&mut self, lba: u32, data: &[u8]) -> Result<(), AtaError> {
        if data.len() % BLOCK_SIZE != 0 {
            return Err(AtaError::InvalidParameter);
        }

        let count = (data.len() / BLOCK_SIZE) as u8;

        self.wait_busy();
        self.select_drive(lba);

        unsafe {
            self.features.write(0x00);
            self.sector_count.write(count);
            self.lba_low.write(lba as u8);
            self.lba_mid.write((lba >> 8) as u8);
            self.lba_high.write((lba >> 16) as u8);
            self.command.write(commands::WRITE_SECTORS);
        }

        for sector in 0..count as usize {
            self.wait_busy();
            self.wait_ready()?;

            let sector_start = sector * BLOCK_SIZE;
            for i in 0..256 {
                let offset = sector_start + i * 2;
                let word = (data[offset] as u16) | ((data[offset + 1] as u16) << 8);
                unsafe {
                    self.data.write(word);
                }
            }
        }

        // Cache flush yap
        self.flush()?;

        self.check_error()?;
        Ok(())
    }

    /// Disk önbelleğini diske yazar.
    pub fn flush(&mut self) -> Result<(), AtaError> {
        self.wait_busy();
        unsafe {
            self.command.write(commands::FLUSH_CACHE);
        }
        self.wait_busy();
        self.check_error()
    }
}
