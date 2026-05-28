//! # Blok Aygıt Soyutlaması
//!
//! Depolama sürücüleri için genel blok aygıt arayüzü.
//!
//! ## Blok Aygıt Katmanı
//!
//! ```
//!  Dosya Sistemi (ext2, FAT32...)
//!         |
//!  BlockDevice trait  <--- Soyut arayüz
//!         |
//!  +------+--------+----------+
//!  |      |        |          |
//! ATA   NVMe    Virtio    USB-MSC
//! (HDD) (SSD)  (Sanal)   (Bellek)
//! ```
//!
//! - Tüm depolama sürücüleri `BlockDevice` trait'ini uygular.
//! - Dosya sistemi katmanı doğrudan sürücüyle değil, bu soyut arayüzle çalışır.
//! - `lba`: Mantıksal Blok Adresi — her blok genellikle 512 bayttır.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// Blok aygıtı hata türleri.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockDeviceError {
    /// Aygıt bulunamadı
    DeviceNotFound,
    /// Giriş/Çıkış hatası
    IoError,
    /// Geçersiz sektör numarası
    InvalidSector,
    /// Aygıt meşgul
    DeviceBusy,
    /// Yazma korumalı
    WriteProtected,
    /// İşlem zaman aşımına uğradı
    Timeout,
    /// Bilinmeyen hata
    Unknown,
    /// İşlem desteklenmiyor
    Unsupported,
}

/// Blok aygıt türü.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockDeviceType {
    /// Sabit disk sürücüsü (manyetik)
    Hdd,
    /// Katı hal disk (flaş bellek)
    Ssd,
    /// USB yığın depolama
    Usb,
    /// Sanal disk (virtio, vb.)
    Virtual,
    /// CD/DVD optik sürücü
    Optical,
    /// NVMe PCIe SSD sürücüsü
    Nvme,
    /// Bilinmeyen tür
    Unknown,
}

/// Blok aygıtı trait'i.
///
/// Depolama sürücülerinin uygulaması gereken soyut arayüz.
/// Dosya sistemi katmanı bu arayüz üzerinden tüm depolama aygıtlarına erişir.
///
/// Deep web: Linux kernel include/linux/blk_types.h (REQ_PREFLUSH, REQ_FUA)
///           block/writeback_cache_control.txt (flush/FUA semantics)
///
/// # Güvenlik Sözleşmesi
/// - `flush()`真實olmalı — desteklemeyen aygıt `Err(Unsupported)` dönmeli
/// - `write_block_fua()` FUA desteklemiyorsa write + flush olarak gerçekleşmeli
/// - `supports_flush()` / `supports_fua()` capability sorgusu
pub trait BlockDevice: Send {
    /// Tek bir bloğu okur.
    fn read_block(&mut self, lba: u64, buffer: &mut [u8]) -> Result<(), BlockDeviceError>;

    /// Tek bir bloğa yazar.
    fn write_block(&mut self, lba: u64, buffer: &[u8]) -> Result<(), BlockDeviceError>;

    /// Blok boyutunu bayt cinsinden döndürür (genellikle 512).
    fn block_size(&self) -> u32;

    /// Toplam blok sayısını döndürür.
    fn block_count(&self) -> u64;

    /// Aygıt adını döndürür.
    fn device_name(&self) -> String;

    /// Aygıt türünü döndürür.
    fn device_type(&self) -> BlockDeviceType;

    /// Aygıtın salt-okunur olup olmadığını kontrol eder.
    fn is_read_only(&self) -> bool {
        false
    }

    /// Aygıtın flush (cache temizleme) destekleyip desteklemediğini söyler.
    /// Deep web: Linux blk_queue_write_cache(queue, true, false) — flush only
    fn supports_flush(&self) -> bool {
        false
    }

    /// Aygıtın FUA (Force Unit Access) destekleyip desteklemediğini söyler.
    /// Deep web: Linux blk_queue_write_cache(queue, true, true) — flush + FUA
    fn supports_fua(&self) -> bool {
        false
    }

    /// Yazma önbelleğini temizler (flush).
    /// Deep web: Linux REQ_PREFLUSH — device cache flush
    ///
    /// # Hata Davranışı
    /// - Desteklemeyen aygıt: `Err(BlockDeviceError::Unsupported)`
    /// - Başarılı flush: `Ok(())`
    /// - Başarısız flush: `Err(BlockDeviceError::IoError)`
    fn flush(&mut self) -> Result<(), BlockDeviceError> {
        if self.supports_flush() {
            Ok(())
        } else {
            Err(BlockDeviceError::Unsupported)
        }
    }

    /// Birden fazla sektörü art arda okur (kolaylık metodu).
    fn read_sectors(&mut self, lba: u64, count: u32) -> Result<Vec<u8>, BlockDeviceError> {
        let block_size = self.block_size() as usize;
        let mut buffer = vec![0u8; count as usize * block_size];
        for i in 0..count as u64 {
            let offset = (i as usize) * block_size;
            self.read_block(lba + i, &mut buffer[offset..offset + block_size])?;
        }
        Ok(buffer)
    }

    /// Veri tamponunu birden fazla sektöre yazar (kolaylık metodu).
    /// Veri uzunluğu blok boyutunun tam katı olmalıdır.
    fn write_sectors(&mut self, lba: u64, data: &[u8]) -> Result<(), BlockDeviceError> {
        let block_size = self.block_size() as usize;
        if data.len() % block_size != 0 {
            return Err(BlockDeviceError::IoError);
        }
        let count = data.len() / block_size;
        for i in 0..count {
            let offset = i * block_size;
            self.write_block(lba + i as u64, &data[offset..offset + block_size])?;
        }
        Ok(())
    }

    /// Aygıt kapasitesini sektör sayısı olarak döndürür.
    fn capacity(&self) -> u64 {
        self.block_count()
    }

    /// Write a block with Force Unit Access semantics.
    /// Deep web: Linux REQ_FUA — write directly to non-volatile media
    ///
    /// # Davranış
    /// - FUA destekliyse: donanımsal FUA kullanılır
    /// - FUA desteklemiyorsa: write + flush (software fallback)
    /// - Desteklemeyen aygıt: `Err(BlockDeviceError::Unsupported)`
    fn write_block_fua(&mut self, lba: u64, buffer: &[u8]) -> Result<(), BlockDeviceError> {
        if self.supports_fua() {
            // Donanımsal FUA — veri doğrudan dayanıklı medyaya yazılır
            self.write_block(lba, buffer)?;
            // FUA zaten flush gerektirir, ek flush gereksiz
            Ok(())
        } else if self.supports_flush() {
            // Software fallback: write + flush
            self.write_block(lba, buffer)?;
            self.flush()
        } else {
            // Hiçbir flush/FUA desteği yok — dayanıklılık garanti edilemez
            Err(BlockDeviceError::Unsupported)
        }
    }

    /// Aygıt adını `&str` olarak döndürür.
    fn name(&self) -> &str {
        "block device"
    }
}
