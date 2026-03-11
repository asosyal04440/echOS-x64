//! # echOS USB CDC Sürücüsü (Communication Device Class)
//!
//! CDC (İletişim Cihaz Sınıfı), USB üzerinden seri port ve Ethernet emülasyonu sağlar.
//! Fiziksel seri kablo veya ağ kartı olmadan USB ile bu işlevleri sunar.
//!
//! ## CDC Katman Mimarisi
//!
//! ```
//!  ┌──────────────────────────────────────────────────────────┐
//!  │  Uygulama: printf() / socket() / read() / write()       │
//!  ├──────────────────────────────────────────────────────────┤
//!  │  CDC-ACM (Seri)          CDC-ECM (Ethernet)             │
//!  │  CdcAcmDevice            CdcEcmDevice                   │
//!  ├──────────────────────────────────────────────────────────┤
//!  │  CdcDevice (ortak temel: TX/RX tampon, kontrol)         │
//!  ├──────────────────────────────────────────────────────────┤
//!  │  USB Bulk OUT ←→ USB Bulk IN (veri kanalı)             │
//!  │  USB Control (SET_LINE_CODING, SET_CONTROL_LINE_STATE)  │
//!  └──────────────────────────────────────────────────────────┘
//! ```
//!
//! ## CDC Alt Sınıfları
//!
//! - **CDC-ACM** (Abstract Control Model): USB seri port emülasyonu.
//!   Baud rate, parite, data bit sayısı `SET_LINE_CODING` isteğiyle ayarlanır.
//!   DTR/RTS sinyal hatları `SET_CONTROL_LINE_STATE` ile kontrol edilir.
//!
//! - **CDC-ECM** (Ethernet Control Model): USB ağ kartı emülasyonu.
//!   Ethernet çerçeveleri 2-byte uzunluk öneki ile gönderilir.
//!   MAC adresi tanımlayıcıdan (descriptor) okunur.
//!
//! ## Line Coding (Seri Port Parametreleri)
//!
//! ```
//! SET_LINE_CODING isteği 7 byte veri taşır:
//!  [0-3] dwDTERate   : Baud rate (örn. 0x00C200 = 115200)  little-endian
//!  [4]   bCharFormat : Stop bit (0=1bit, 1=1.5bit, 2=2bit)
//!  [5]   bParityType : Parite (0=Yok, 1=Tek, 2=Çift, 3=İşaret, 4=Boşluk)
//!  [6]   bDataBits   : Veri bit sayısı (5, 6, 7, 8 veya 16)
//! ```
//!
//! ## CDC Arabirim Çifti
//!
//! Her CDC cihazı iki arabirim kullanır:
//! - **Kontrol Arabirimi** (CDC Control, sınıf 0x02): Komutlar (Interrupt IN ucu noktası)
//! - **Veri Arabirimi** (CDC Data, sınıf 0x0A): Gerçek veri aktarımı (Bulk IN + Bulk OUT)

use super::{UsbClass, UsbDevice, UsbError};
use alloc::vec::Vec;

/// CDC cihaz alt tipi.
///
/// Hangi CDC protokolünün kullanılacağını belirler:
/// - `Serial`: CDC-ACM, USB seri port
/// - `Ethernet`: CDC-ECM, USB ağ kartı
/// - `Wireless`: CDC-WMC, kablosuz modem
/// - `NetworkControl`: CDC-NCM/NCM, yüksek hız ağ
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CdcType {
    Serial,
    Ethernet,
    Wireless,
    NetworkControl,
}

/// Temel CDC cihaz yapısı.
///
/// Tüm CDC alt tipleri (ACM, ECM) bu yapıyı içerir.
/// Kontrol ve veri arabirim numaraları USB tanımlayıcıdan ayrıştırılır.
///
/// `rx_buffer`: Gelen veriyi geçici olarak bekletir (uygulama okyana kadar).
/// `tx_buffer`: Gönderilecek veriyi bekletir (Bulk OUT aktarımı için).
#[derive(Clone, Debug)]
pub struct CdcDevice {
    pub device: UsbDevice,
    pub cdc_type: CdcType,
    /// Kontrol arabirimi numarası (CDC Kontrol sınıfı - sınıf 0x02)
    pub control_interface: u8,
    /// Veri arabirimi numarası (CDC Veri sınıfı - sınıf 0x0A)
    pub data_interface: u8,
    /// Bulk IN uç noktası (cihazdan ana bilgisayara veri)
    pub in_endpoint: u8,
    /// Bulk OUT uç noktası (ana bilgisayardan cihaza veri)
    pub out_endpoint: u8,
    /// CDC-ECM için Ethernet MAC adresi (6 byte)
    pub mac_address: [u8; 6],
    /// Gelen veri tamponu (receive buffer)
    pub rx_buffer: Vec<u8>,
    /// Gönderilecek veri tamponu (transmit buffer)
    pub tx_buffer: Vec<u8>,
}

impl CdcDevice {
    pub fn new(device: UsbDevice, control_if: u8, data_if: u8) -> Self {
        CdcDevice {
            device,
            cdc_type: CdcType::Serial,
            control_interface: control_if,
            data_interface: data_if,
            in_endpoint: 0,
            out_endpoint: 0,
            mac_address: [0; 6],
            rx_buffer: Vec::new(),
            tx_buffer: Vec::new(),
        }
    }

    /// Seri port parametrelerini ayarlar (SET_LINE_CODING komutu).
    ///
    /// USB kontrol aktarımıyla cihaza 7-byte "Line Coding" yapısı gönderilir:
    /// ```
    /// [baud_rate (4B LE)] [stop_bits (1B)] [parity (1B)] [data_bits (1B)]
    /// ```
    /// Örnek: 115200-8N1 → dwDTERate=0x0001C200, bCharFormat=0, bParityType=0, bDataBits=8
    pub fn set_line_coding(
        &mut self,
        baud_rate: u32,
        stop_bits: u8,
        parity: u8,
        data_bits: u8,
    ) -> Result<(), UsbError> {
        // Line coding yapısı: 7 byte
        // dwDTERate (4 byte): Baud rate, little-endian formatı
        // bCharFormat (1 byte): Stop bit sayısı
        // bParityType (1 byte): Parite türü
        // bDataBits (1 byte): Veri bit sayısı
        let _line_coding = [
            (baud_rate & 0xFF) as u8,
            ((baud_rate >> 8) & 0xFF) as u8,
            ((baud_rate >> 16) & 0xFF) as u8,
            ((baud_rate >> 24) & 0xFF) as u8,
            stop_bits,
            parity,
            data_bits,
        ];

        // TODO: SET_LINE_CODING kontrol isteği gönder
        Ok(())
    }

    /// Kontrol hat durumunu ayarlar (SET_CONTROL_LINE_STATE komutu).
    ///
    /// DTR (Data Terminal Ready): Terminalde yazılım hazır sinyali.
    /// RTS (Request To Send): Gönderim isteği sinyali.
    ///
    /// `value` biti: bit0=DTR, bit1=RTS
    /// Modem simülasyonu için kullanılır (USB→RS-232 dönüştürücüler gibi).
    pub fn set_control_line_state(&mut self, dtr: bool, rts: bool) -> Result<(), UsbError> {
        let value = (dtr as u16) | ((rts as u16) << 1);
        let _ = value;
        // TODO: SET_CONTROL_LINE_STATE kontrol isteği gönder
        Ok(())
    }

    /// Veri gönderir (USB Bulk OUT aktarımı).
    ///
    /// Gerçek implementasyonda `data` Bulk OUT uç noktasından gönderilir.
    /// Şimdilik veri TX tamponuna eklenir.
    pub fn send(&mut self, data: &[u8]) -> Result<usize, UsbError> {
        // TODO: USB bulk out aktarımı gerçekleştir
        self.tx_buffer.extend_from_slice(data);
        Ok(data.len())
    }

    /// Veri alır (USB Bulk IN ara belleğinden).
    ///
    /// RX tamponu -> buf kopyalaması: en az `len` byte veya tampon boyutu kadar.
    /// `drain(..len)`: kopyalanan kısım tampondan kaldırılır (FIFO sırası).
    pub fn receive(&mut self, buf: &mut [u8]) -> Result<usize, UsbError> {
        if self.rx_buffer.is_empty() {
            return Ok(0);
        }

        let len = buf.len().min(self.rx_buffer.len());
        buf[..len].copy_from_slice(&self.rx_buffer[..len]);
        self.rx_buffer.drain(..len);
        Ok(len)
    }

    /// RX tamponunda veri var mı?
    pub fn data_available(&self) -> bool {
        !self.rx_buffer.is_empty()
    }
}

/// CDC-ECM (Ethernet Control Model) cihazı.
///
/// USB üzerinden Ethernet çerçeveleri aktarır.
/// Her çerçevenin başına 2-byte uzunluk alanı eklenir (çerçeve sınırı tespiti için).
///
/// ## ECM Çerçeve Formatı
///
/// ```
/// Gönderme (send_frame):
///   [len_low] [len_high] [Ethernet çerçevesi...]
///
/// Alma (receive_frame):
///   [len_low] [len_high] → uzunluk hesaplanır
///   [Ethernet çerçevesi...] tampondan kopyalanır
/// ```
#[derive(Clone, Debug)]
pub struct CdcEcmDevice {
    pub cdc: CdcDevice,
    /// Ethernet istatistikleri (toplam gönderilen/alınan paket ve byte sayıları)
    pub ethernet_statistics: EthernetStatistics,
}

/// Ethernet katmanı istatistikleri.
#[derive(Clone, Copy, Debug, Default)]
pub struct EthernetStatistics {
    pub tx_packets: u64,
    pub rx_packets: u64,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub tx_errors: u64,
    pub rx_errors: u64,
}

impl CdcEcmDevice {
    pub fn new(device: UsbDevice, control_if: u8, data_if: u8) -> Self {
        let mut cdc = CdcDevice::new(device, control_if, data_if);
        cdc.cdc_type = CdcType::Ethernet;

        CdcEcmDevice {
            cdc,
            ethernet_statistics: EthernetStatistics::default(),
        }
    }

    /// Ethernet çerçevesi gönderir.
    ///
    /// CDC-ECM protokolü çerçeve başına 2-byte LE uzunluk öneki gerektirir.
    /// Donanım bu öneki okuyarak çerçeve sınırlarını tespit eder.
    pub fn send_frame(&mut self, frame: &[u8]) -> Result<usize, UsbError> {
        // CDC-ECM: çerçeveler 2-byte LE uzunluk öneki gerektirir
        let len = frame.len() as u16;
        let mut packet = Vec::with_capacity(frame.len() + 4);
        packet.push((len & 0xFF) as u8);
        packet.push(((len >> 8) & 0xFF) as u8);
        packet.extend_from_slice(frame);

        self.cdc.send(&packet)?;
        self.ethernet_statistics.tx_packets += 1;
        self.ethernet_statistics.tx_bytes += frame.len() as u64;
        Ok(frame.len())
    }

    /// Ethernet çerçevesi alır.
    ///
    /// İlk 2 byte: çerçeve uzunluğu (LE). Gerisi: gerçek Ethernet çerçevesi.
    /// `frame_len.min(buf.len()).min(len-2)`: taşma önleme (buffer sınırı).
    pub fn receive_frame(&mut self, buf: &mut [u8]) -> Result<usize, UsbError> {
        let mut temp_buf = [0u8; 2048];
        let len = self.cdc.receive(&mut temp_buf)?;

        if len < 2 {
            return Ok(0);
        }

        let frame_len = temp_buf[0] as usize | ((temp_buf[1] as usize) << 8);
        let frame_len = frame_len.min(buf.len()).min(len - 2);

        buf[..frame_len].copy_from_slice(&temp_buf[2..2 + frame_len]);
        self.ethernet_statistics.rx_packets += 1;
        self.ethernet_statistics.rx_bytes += frame_len as u64;
        Ok(frame_len)
    }

    /// Cihazın MAC adresini döndürür (tanımlayıcıdan okunmuş).
    pub fn mac_address(&self) -> [u8; 6] {
        self.cdc.mac_address
    }

    /// MAC adresini ayarlar (USB tanımlayıcıdan string çevrilmiş).
    pub fn set_mac_address(&mut self, mac: [u8; 6]) {
        self.cdc.mac_address = mac;
    }
}

/// CDC-ACM (Abstract Control Model) cihazı - USB seri port.
///
/// RS-232 seri portunu USB üzerinden emüle eder.
/// Varsayılan: 115200 baud, 8 data bit, 1 stop bit, parite yok (8N1).
///
/// ## Kullanım Örnekleri
///
/// - Arduino/geliştirici kartı programlama
/// - Mikrodenetleyici debug çıkışı
/// - USB-UART adaptörleri (CP2102, CH340 vb.)
#[derive(Clone, Debug)]
pub struct CdcAcmDevice {
    pub cdc: CdcDevice,
    /// Baud rate (bit/saniye): 9600, 38400, 115200, 921600, ...
    pub baud_rate: u32,
    /// Veri bit sayısı: genellikle 8
    pub data_bits: u8,
    /// Stop bit: 1=1bit, 2=2bit
    pub stop_bits: u8,
    /// Parite: 0=yok, 1=tek, 2=çift
    pub parity: u8,
}

impl CdcAcmDevice {
    pub fn new(device: UsbDevice, control_if: u8, data_if: u8) -> Self {
        let mut cdc = CdcDevice::new(device, control_if, data_if);
        cdc.cdc_type = CdcType::Serial;

        CdcAcmDevice {
            cdc,
            baud_rate: 115200, // Yaygın hata ayıklama baud hızı
            data_bits: 8,
            stop_bits: 1,
            parity: 0, // Parite yok (en yaygın)
        }
    }

    /// Seri port parametrelerini yapılandırır.
    ///
    /// `set_line_coding` USB kontrol aktarımı üzerinden cihaza gönderilir.
    /// Yerel alanlar güncellenir (yazılım durumu self ile senkronize).
    pub fn configure(
        &mut self,
        baud_rate: u32,
        data_bits: u8,
        stop_bits: u8,
        parity: u8,
    ) -> Result<(), UsbError> {
        self.cdc
            .set_line_coding(baud_rate, stop_bits, parity, data_bits)?;
        self.baud_rate = baud_rate;
        self.data_bits = data_bits;
        self.stop_bits = stop_bits;
        self.parity = parity;
        Ok(())
    }

    /// Seri porta veri yazar (USB Bulk OUT).
    pub fn write(&mut self, data: &[u8]) -> Result<usize, UsbError> {
        self.cdc.send(data)
    }

    /// Seri porttan veri okur (USB Bulk IN arabelleğinden).
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, UsbError> {
        self.cdc.receive(buf)
    }

    /// DTR (Data Terminal Ready) sinyalini ayarlar.
    ///
    /// DTR=true: terminaldeki yazılım açık ve hazır.
    /// Bazı cihazlar (Arduino gibi) DTR sinyalinde otomatik sıfırlama yapar.
    pub fn set_dtr(&mut self, state: bool) -> Result<(), UsbError> {
        self.cdc.set_control_line_state(state, false)
    }

    /// RTS (Request To Send) sinyalini ayarlar.
    ///
    /// RTS=true: ana bilgisayar veri almaya hazır.
    /// Donanım akış kontrolü (hardware flow control) için kullanılır.
    pub fn set_rts(&mut self, state: bool) -> Result<(), UsbError> {
        self.cdc.set_control_line_state(false, state)
    }
}

/// USB cihaz listesinden CDC cihazlarını bulur.
///
/// Algoritma: her cihazın arabirimlerini tarar:
/// - CDC Kontrol (0x02) → control_if olarak kaydet
/// - CDC Veri (0x0A)    → data_if olarak kaydet
/// Her ikisi de bulunan cihazlar için `CdcDevice` oluşturulur.
///
/// CDC cihazlar her zaman çift arabirime sahiptir:
/// Kontrol arabirimi: komut kanalı (Interrupt IN)
/// Veri arabirimi: veri kanalı (Bulk IN + Bulk OUT)
pub fn find_cdc_devices(devices: &[UsbDevice]) -> Vec<CdcDevice> {
    let mut cdc_devices = Vec::new();

    for device in devices {
        // CDC arabirimlerini ara
        let mut control_if: Option<u8> = None;
        let mut data_if: Option<u8> = None;

        for iface in &device.interfaces {
            if iface.class == UsbClass::CdcControl {
                control_if = Some(iface.interface_number);
            } else if iface.class == UsbClass::CdcData {
                data_if = Some(iface.interface_number);
            }
        }

        // Her iki arabirim de bulunmuşsa CDC cihazı oluştur
        if let (Some(ctrl), Some(data)) = (control_if, data_if) {
            cdc_devices.push(CdcDevice::new(device.clone(), ctrl, data));
        }
    }

    cdc_devices
}
