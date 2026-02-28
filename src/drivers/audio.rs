//! # echOS Ses Alt Sistemi
//!
//! Intel High Definition Audio (HDA) sürücüsü.
//!
//! ## HDA Mimarisi
//!
//! ```
//!  +-------------------+     CORB (Komut Halkası)    +------------------+
//!  |   HDA Kontrolcü   |----------------------------->|   Codec Widget   |
//!  |                   |<----------------------------|  (DAC/ADC/Pin)   |
//!  |  MMIO Yazmacları  |     RIRB (Yanıt Halkası)    +------------------+
//!  |  - GCAP           |                                      |
//!  |  - GCTL (reset)   |     +----------------+               |
//!  |  - CORB/RIRB      |     | Stream (DMA)   |               v
//!  |  - Stream Desc.   |---->| BDL (Buffer    |    Hoparlör / Mikrofon
//!  +-------------------+     | Descriptor     |
//!                             | List)          |
//!                             +----------------+
//! ```
//!
//! - **CORB**: Komut Çıktı Halka Tamponu — CPU'dan codec'e komut gönderir.
//! - **RIRB**: Yanıt Girdi Halka Tamponu — Codec'ten gelen yanıtları alır.
//! - **BDL**: DMA transfer listesi; ses verisi fiziksel bellekten doğrudan okunur.
//! - **Widget**: Codec içindeki ses işleme düğümü (DAC, ADC, Pin, Mixer vb.).

use alloc::vec::Vec;
use alloc::vec;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use spin::Mutex;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

// ============================================================================
// HDA SABİTLERİ
// ============================================================================

/// HDA PCI sınıf kodları
const PCI_CLASS_MULTIMEDIA: u8 = 0x04;
const PCI_SUBCLASS_HDA: u8 = 0x03;

/// HDA denetleyici yazmacları (bellek eşlemeli — MMIO)
const HDA_GCAP: usize = 0x00;      // Genel Yetenekler (Global Capabilities)
const HDA_GCTL: usize = 0x08;      // Genel Kontrol (Global Control)
const HDA_GSTS: usize = 0x0C;      // Genel Durum (Global Status)
const HDA_OUTSTR: usize = 0x10;    // Çıktı Akışı Yükü (Output Stream Payload)
const HDA_INSTR: usize = 0x14;     // Giriş Akışı Yükü (Input Stream Payload)
const HDA_INTCTL: usize = 0x20;    // Kesme Kontrolü (Interrupt Control)
const HDA_INTSTS: usize = 0x24;    // Kesme Durumu (Interrupt Status)
const HDA_WAKEEN: usize = 0x0C;    // Uyandırma Etkinleştirme (Wake Enable)

/// Akış yazmacı taban ofseti
const HDA_STREAM_BASE: usize = 0x80;
const HDA_STREAM_INTERVAL: usize = 0x20;

/// Akış tanımlayıcı yazmacları
const HDA_SD_CTL: usize = 0x00;
const HDA_SD_STS: usize = 0x03;
const HDA_SD_LPIB: usize = 0x04;
const HDA_SD_CBL: usize = 0x08;
const HDA_SD_LVI: usize = 0x0C;
const HDA_SD_FIFOS: usize = 0x10;
const HDA_SD_FMT: usize = 0x12;
const HDA_SD_BDPL: usize = 0x18;
const HDA_SD_BDPU: usize = 0x1C;

/// CORB/RIRB yazmacları
const HDA_CORBLBASE: usize = 0x40;
const HDA_CORBUBASE: usize = 0x44;
const HDA_CORBWP: usize = 0x48;
const HDA_CORBRP: usize = 0x4A;
const HDA_CORBCTL: usize = 0x4C;
const HDA_CORBSTS: usize = 0x4D;
const HDA_CORBSIZE: usize = 0x4E;

const HDA_IRBLBASE: usize = 0x50;
const HDA_IRBUBASE: usize = 0x54;
const HDA_IRBWP: usize = 0x58;
const HDA_IRBRP: usize = 0x5A;
const HDA_IRBCTL: usize = 0x5C;
const HDA_IRBSTS: usize = 0x5D;
const HDA_IRBSIZE: usize = 0x5E;

/// HDA codec komutları (fiil kodları)
const HDA_VERB_GET_PARAMETER: u32 = 0xF0000;
const HDA_VERB_SET_POWER_STATE: u32 = 0x70500;
const HDA_VERB_SET_CONVERTER_FORMAT: u32 = 0x20000;
const HDA_VERB_SET_CONVERTER_STREAM: u32 = 0x70600;
const HDA_VERB_SET_AMP_GAIN: u32 = 0x30000;
const HDA_VERB_SET_PIN_WIDGET_CTRL: u32 = 0x70700;

/// Codec parametreleri
const HDA_PARAM_VENDOR_ID: u32 = 0x00;
const HDA_PARAM_REVISION_ID: u32 = 0x02;
const HDA_PARAM_NODE_COUNT: u32 = 0x04;
const HDA_PARAM_FUNCTION_TYPE: u32 = 0x05;
const HDA_PARAM_AUDIO_WIDGET_CAPS: u32 = 0x09;
const HDA_PARAM_AUDIO_SUPPORTED_PCM: u32 = 0x0A;
const HDA_PARAM_AUDIO_SUPPORTED_STREAM: u32 = 0x0B;
const HDA_PARAM_AUDIO_INPUT_AMP_CAPS: u32 = 0x0D;
const HDA_PARAM_AUDIO_OUTPUT_AMP_CAPS: u32 = 0x12;

/// Widget türleri
const HDA_WIDGET_OUTPUT_DAC: u8 = 0x0;
const HDA_WIDGET_INPUT_ADC: u8 = 0x1;
const HDA_WIDGET_MIXER: u8 = 0x3;
const HDA_WIDGET_PIN: u8 = 0x4;
const HDA_WIDGET_POWER: u8 = 0x7;
const HDA_WIDGET_VOLUME: u8 = 0x8;
const HDA_WIDGET_BEEP: u8 = 0x9;

/// Akış format bitleri (örnekleme hızı)
const HDA_FMT_48KHZ: u16 = 0x00;
const HDA_FMT_44_1KHZ: u16 = 0x40;
const HDA_FMT_96KHZ: u16 = 0x80;
const HDA_FMT_192KHZ: u16 = 0xC0;

const HDA_FMT_8BIT: u16 = 0x00;
const HDA_FMT_16BIT: u16 = 0x01;
const HDA_FMT_20BIT: u16 = 0x02;
const HDA_FMT_24BIT: u16 = 0x03;
const HDA_FMT_32BIT: u16 = 0x04;

const HDA_FMT_MONO: u16 = 0x00;
const HDA_FMT_STEREO: u16 = 0x01;

// ============================================================================
// HDA DENETLEYİCİ
// ============================================================================

/// HDA Denetleyicisi.
///
/// Intel High Definition Audio donanım denetleyicisini temsil eder.
/// PCI bus üzerinden bulunur; MMIO adresi PCI BAR'dan alınır.
/// Sistem başlangıcında `init()` ile başlatılmalıdır.
#[derive(Clone, Debug)]
pub struct HdaController {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub mmio_base: u64,
    pub mmio_size: u64,
    pub vendor_id: u16,
    pub device_id: u16,
    /// Çıktı akışı sayısı
    pub output_streams: u8,
    /// Giriş akışı sayısı
    pub input_streams: u8,
    /// Çift yönlü akış sayısı
    pub bidir_streams: u8,
    /// 64-bit adres desteği
    pub addr64: bool,
    /// CORB boyutu
    pub corb_size: u16,
    /// RIRB boyutu (IRB → RIRB)
    pub irb_size: u16,
    /// Bulunan codec'ler
    pub codecs: Vec<HdaCodec>,
}

impl HdaController {
    /// Yeni bir HDA denetleyicisi nesnesi oluşturur.
    pub fn new(bus: u8, device: u8, function: u8) -> Self {
        HdaController {
            bus,
            device,
            function,
            mmio_base: 0,
            mmio_size: 0,
            vendor_id: 0,
            device_id: 0,
            output_streams: 0,
            input_streams: 0,
            bidir_streams: 0,
            addr64: false,
            corb_size: 0,
            irb_size: 0,
            codecs: Vec::new(),
        }
    }
    
    /// Denetleyiciyi başlatır.
    /// Reset → Yetenek okuma → CORB/RIRB başlatma → Codec tarama adımlarını uygular.
    pub fn init(&mut self) -> Result<(), AudioError> {
        // Denetleyiciyi sıfırla
        self.reset()?;

        // Yetenekleri oku
        self.read_capabilities();

        // CORB/RIRB başlat
        self.init_corb_irb()?;

        // Codec'leri bul
        self.detect_codecs();
        
        crate::serial_println!("[HDA] Controller initialized: {} out, {} in, {} bidir streams",
            self.output_streams, self.input_streams, self.bidir_streams);
        
        Ok(())
    }
    
    /// Denetleyiciyi sıfırlar (GCTL.CRST biti aracılığıyla).
    fn reset(&mut self) -> Result<(), AudioError> {
        // CRST bitini GCTL yazmacına yaz
        // YAPILACAK: Gerçek MMIO yazma işlemi
        Ok(())
    }

    /// GCAP yazmacından denetleyici yeteneklerini okur.
    fn read_capabilities(&mut self) {
        // YAPILACAK: MMIO'dan oku
        // Varsayılan değerler
        self.output_streams = 4;
        self.input_streams = 4;
        self.bidir_streams = 0;
        self.addr64 = true;
        self.corb_size = 256;
        self.irb_size = 256;
    }

    /// CORB (Komut Çıktı Halka Tamponu) ve RIRB (Yanıt Girdi Halka Tamponu) başlatır.
    fn init_corb_irb(&mut self) -> Result<(), AudioError> {
        // CORB ve RIRB tampon alanlarını tahsis et
        // YAPILACAK: Gerçek implementasyon
        Ok(())
    }

    /// HDA bağlantısındaki codec'leri tarar (0-15 arası adresler).
    fn detect_codecs(&mut self) {
        // Codec'leri tara (genellikle 0-15)
        for codec_addr in 0..=15 {
            // Vendor ID almayı dene
            // Yanıt geçerliyse codec var demektir
            let vendor_id = 0x8086u16; // Yer tutucu Intel codec
            let device_id = 0x0001u16;

            if vendor_id != 0xFFFF {
                let mut codec = HdaCodec::new(codec_addr, vendor_id, device_id);
                codec.scan_widgets();
                self.codecs.push(codec);
            }
        }
    }

    /// Codec'e komut gönderir (CORB aracılığıyla).
    pub fn send_command(&self, codec: u8, nid: u8, verb: u32) -> u32 {
        // YAPILACAK: CORB komutu gönderme
        let _ = (codec, nid, verb);
        0
    }

    /// Oynatma için kullanılabilir bir akış döndürür.
    pub fn get_playback_stream(&self) -> Option<u8> {
        if self.output_streams > 0 {
            Some(0) // İlk çıktı akışı
        } else {
            None
        }
    }
}

// ============================================================================
// HDA CODEC
// ============================================================================

/// HDA Codec.
///
/// Fiziksel ses codec donanımını temsil eder. Bir codec,
/// DAC (Dijital→Analog), ADC (Analog→Dijital) ve Pin gibi
/// widget düğümlerini birbiriyle bağlayan mantıksal birimdir.
#[derive(Clone, Debug)]
pub struct HdaCodec {
    pub address: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub revision_id: u8,
    pub widgets: Vec<AudioWidget>,
    pub root_nid: u8,
    pub audio_func_group: u8,
}

impl HdaCodec {
    /// Belirtilen adreste yeni bir HDA codec nesnesi oluşturur.
    pub fn new(address: u8, vendor_id: u16, device_id: u16) -> Self {
        HdaCodec {
            address,
            vendor_id,
            device_id,
            revision_id: 0,
            widgets: Vec::new(),
            root_nid: 0,
            audio_func_group: 0,
        }
    }
    
    /// Codec içindeki widget'ları tarar ve `self.widgets` listesine ekler.
    pub fn scan_widgets(&mut self) {
        // Kök düğüm
        self.root_nid = 0;

        // Ses fonksiyon grubu
        self.audio_func_group = 1;

        // Temel widget'ları ekle
        self.widgets.push(AudioWidget {
            nid: 2,
            widget_type: HdaWidgetType::OutputDac,
            name: "DAC0".into(),
            capabilities: WidgetCaps::OUTPUT_AMP,
            default_gain: 0,
            muted: false,
        });
        
        self.widgets.push(AudioWidget {
            nid: 3,
            widget_type: HdaWidgetType::Pin,
            name: "Speaker".into(),
            capabilities: WidgetCaps::PIN_SENSE,
            default_gain: 0,
            muted: false,
        });
        
        self.widgets.push(AudioWidget {
            nid: 4,
            widget_type: HdaWidgetType::Pin,
            name: "Headphone".into(),
            capabilities: WidgetCaps::PIN_SENSE,
            default_gain: 0,
            muted: false,
        });
        
        self.widgets.push(AudioWidget {
            nid: 5,
            widget_type: HdaWidgetType::InputAdc,
            name: "ADC0".into(),
            capabilities: WidgetCaps::INPUT_AMP,
            default_gain: 0,
            muted: false,
        });
        
        self.widgets.push(AudioWidget {
            nid: 6,
            widget_type: HdaWidgetType::Pin,
            name: "Mic".into(),
            capabilities: WidgetCaps::PIN_SENSE,
            default_gain: 0,
            muted: false,
        });
    }
    
    /// NID (Node ID) ile widget arar.
    pub fn find_widget(&self, nid: u8) -> Option<&AudioWidget> {
        self.widgets.iter().find(|w| w.nid == nid)
    }

    /// Türüne göre widget arar.
    pub fn find_widget_by_type(&self, widget_type: HdaWidgetType) -> Option<&AudioWidget> {
        self.widgets.iter().find(|w| w.widget_type == widget_type)
    }

    /// Çıktı DAC widget'ını bulur.
    pub fn find_output_dac(&self) -> Option<&AudioWidget> {
        self.find_widget_by_type(HdaWidgetType::OutputDac)
    }

    /// Giriş ADC widget'ını bulur.
    pub fn find_input_adc(&self) -> Option<&AudioWidget> {
        self.find_widget_by_type(HdaWidgetType::InputAdc)
    }
}

// ============================================================================
// SES WİDGET'I
// ============================================================================

/// Widget türü.
///
/// HDA spec'te tanımlanan ses işleme düğüm türleri.
/// DAC → dijitalden analog dönüştürücü (hoparlör için).
/// ADC → analogdan dijital dönüştürücü (mikrofon için).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HdaWidgetType {
    OutputDac,
    InputAdc,
    Mixer,
    Pin,
    Power,
    VolumeKnob,
    Beep,
    Unknown,
}

impl HdaWidgetType {
    pub fn from_u8(val: u8) -> Self {
        match val {
            HDA_WIDGET_OUTPUT_DAC => HdaWidgetType::OutputDac,
            HDA_WIDGET_INPUT_ADC => HdaWidgetType::InputAdc,
            HDA_WIDGET_MIXER => HdaWidgetType::Mixer,
            HDA_WIDGET_PIN => HdaWidgetType::Pin,
            HDA_WIDGET_POWER => HdaWidgetType::Power,
            HDA_WIDGET_VOLUME => HdaWidgetType::VolumeKnob,
            HDA_WIDGET_BEEP => HdaWidgetType::Beep,
            _ => HdaWidgetType::Unknown,
        }
    }
}

/// Widget yetenekleri (bit bayrakları).
///
/// Her bit, widget'ın desteklediği bir özelliği gösterir.
/// Örneğin `INPUT_AMP` biti varsa widget giriş kazancı kontrolü yapabilir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WidgetCaps(pub u32);

impl WidgetCaps {
    pub const STEREO: WidgetCaps = WidgetCaps(1 << 0);
    pub const INPUT_AMP: WidgetCaps = WidgetCaps(1 << 1);
    pub const OUTPUT_AMP: WidgetCaps = WidgetCaps(1 << 2);
    pub const AMP_OVERRIDE: WidgetCaps = WidgetCaps(1 << 3);
    pub const FORMAT_OVERRIDE: WidgetCaps = WidgetCaps(1 << 4);
    pub const STRIPE: WidgetCaps = WidgetCaps(1 << 5);
    pub const PROCESSING: WidgetCaps = WidgetCaps(1 << 6);
    pub const UNSOLICITED: WidgetCaps = WidgetCaps(1 << 7);
    pub const CONNECTION_LIST: WidgetCaps = WidgetCaps(1 << 8);
    pub const DIGITAL: WidgetCaps = WidgetCaps(1 << 9);
    pub const POWER_CTL: WidgetCaps = WidgetCaps(1 << 10);
    pub const LR_SWAP: WidgetCaps = WidgetCaps(1 << 11);
    pub const COPY: WidgetCaps = WidgetCaps(1 << 12);
    pub const PIN_SENSE: WidgetCaps = WidgetCaps(1 << 13);
    pub const TRIGGER: WidgetCaps = WidgetCaps(1 << 14);
    pub const IMPEDANCE: WidgetCaps = WidgetCaps(1 << 15);
    
    pub fn contains(&self, other: WidgetCaps) -> bool {
        (self.0 & other.0) != 0
    }
    
    pub fn insert(&mut self, other: WidgetCaps) {
        self.0 |= other.0;
    }
}

/// Ses widget düğümü.
///
/// Codec içindeki bir işleme düğümünü temsil eder.
/// `nid`: Node ID (codec içinde benzersiz).
/// `capabilities`: Bu widget'ın desteklediği özellikler.
#[derive(Clone, Debug)]
pub struct AudioWidget {
    pub nid: u8,
    pub widget_type: HdaWidgetType,
    pub name: alloc::string::String,
    pub capabilities: WidgetCaps,
    pub default_gain: i16,
    pub muted: bool,
}

impl AudioWidget {
    /// Ses düzeyini 0-100% arasında ayarlar.
    /// Değeri dahili kazanç birimine dönüştürür.
    pub fn set_volume(&mut self, volume: u8) {
        // 0-100% değerini kazanç değerine dönüştür
        self.default_gain = ((volume as i16) * 100 / 100) - 100;
    }

    /// Widget'ı susturur veya susturmayı kaldırır.
    pub fn set_mute(&mut self, mute: bool) {
        self.muted = mute;
    }
}

// ============================================================================
// SES AKIŞI
// ============================================================================

/// Ses akış formatı (PCM parametreleri).
///
/// PCM: Pulse Code Modulation — dijital sesin ham temsili.
/// `sample_rate`: Örnekleme sıklığı (örn. 44100 Hz, 48000 Hz).
/// `bits_per_sample`: Bit derinliği (örn. 16, 24 bit).
/// `channels`: Kanal sayısı (1 = mono, 2 = stereo).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub bits_per_sample: u8,
    pub channels: u8,
}

impl AudioFormat {
    pub fn new(sample_rate: u32, bits_per_sample: u8, channels: u8) -> Self {
        AudioFormat {
            sample_rate,
            bits_per_sample,
            channels,
        }
    }
    
    /// CD kalitesi format (44.1 kHz, 16-bit stereo).
    pub fn cd_quality() -> Self {
        AudioFormat::new(44100, 16, 2)
    }

    /// DVD kalitesi format (48 kHz, 16-bit stereo).
    pub fn dvd_quality() -> Self {
        AudioFormat::new(48000, 16, 2)
    }

    /// Yüksek kalite format (96 kHz, 24-bit stereo).
    pub fn high_quality() -> Self {
        AudioFormat::new(96000, 24, 2)
    }

    /// Bu formatı HDA SD_FMT yazmacı değerine dönüştürür.
    pub fn to_hda_format(&self) -> u16 {
        let rate_bits = match self.sample_rate {
            8000..=48000 => HDA_FMT_48KHZ,
            44100 => HDA_FMT_44_1KHZ,
            88200 => HDA_FMT_44_1KHZ | 0x80,
            96000 => HDA_FMT_96KHZ,
            192000 => HDA_FMT_192KHZ,
            _ => HDA_FMT_48KHZ,
        };
        
        let bits_bits = match self.bits_per_sample {
            8 => HDA_FMT_8BIT,
            16 => HDA_FMT_16BIT,
            20 => HDA_FMT_20BIT,
            24 => HDA_FMT_24BIT,
            32 => HDA_FMT_32BIT,
            _ => HDA_FMT_16BIT,
        };
        
        let chan_bits = if self.channels == 1 {
            HDA_FMT_MONO
        } else {
            HDA_FMT_STEREO
        };
        
        rate_bits | (bits_bits << 4) | (chan_bits << 8)
    }
}

/// Ses akışı yönü.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamDirection {
    Playback,
    Capture,
}

/// Ses akışı (PCM veri tamponu ve durum).
///
/// Bir oynatma veya kayıt akışını yönetir.
/// `buffer`: PCM veri tamponu.
/// `position`: Tampon içindeki mevcut okuma konumu (byte).
/// `loop_enabled`: Tampon tükenince başa dönme.
#[derive(Clone, Debug)]
pub struct AudioStream {
    pub stream_id: u8,
    pub direction: StreamDirection,
    pub format: AudioFormat,
    pub buffer: Vec<u8>,
    pub buffer_size: usize,
    pub position: usize,
    pub playing: bool,
    pub loop_enabled: bool,
}

impl AudioStream {
    pub fn new(stream_id: u8, direction: StreamDirection, format: AudioFormat) -> Self {
        AudioStream {
            stream_id,
            direction,
            format,
            buffer: Vec::new(),
            buffer_size: 0,
            position: 0,
            playing: false,
            loop_enabled: false,
        }
    }
    
    /// PCM veri tamponunu ayarlar, konumu sıfırlar.
    pub fn set_buffer(&mut self, data: Vec<u8>) {
        self.buffer = data;
        self.buffer_size = self.buffer.len();
        self.position = 0;
    }

    /// Oynatmayı başlatır, konumu sıfırlar.
    pub fn start(&mut self) {
        self.playing = true;
        self.position = 0;
    }

    /// Oynatmayı tamamen durdurur.
    pub fn stop(&mut self) {
        self.playing = false;
    }

    /// Oynatmayı duraklatır (konum korunur).
    pub fn pause(&mut self) {
        self.playing = false;
    }

    /// Duraklatılmış oynatmayı devam ettirir.
    pub fn resume(&mut self) {
        self.playing = true;
    }

    /// `bytes` kadar tamponu tüketir; döngü ya da durma mantığını uygular.
    /// `true` döner → oynatma devam ediyor, `false` → bitti.
    pub fn consume(&mut self, bytes: usize) -> bool {
        if !self.playing {
            return false;
        }

        self.position += bytes;

        if self.position >= self.buffer_size {
            if self.loop_enabled {
                self.position = 0;
            } else {
                self.playing = false;
                self.position = 0;
                return false;
            }
        }

        true
    }
}

// ============================================================================
// SES TAMPON TANIMLAYICI LİSTESİ (BDL)
// ============================================================================

/// Tampon Tanımlayıcı Girişi (BDE) — 16 bayt.
///
/// DMA motoruna hangi fiziksel bellek sayfasından ses verisi okuyacağını söyler.
/// `address_low/high`: Fiziksel adres (64-bit, iki 32-bit parçaya bölünmüş).
/// `length`: Bu girişteki veri uzunluğu (byte).
/// `flags`: Bit 0 = son giriş, Bit 1 = kesme üret.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct BufferDescriptorEntry {
    pub address_low: u32,
    pub address_high: u32,
    pub length: u32,
    pub flags: u32,
}

impl BufferDescriptorEntry {
    pub fn new(address: u64, length: u32, last: bool, interrupt: bool) -> Self {
        BufferDescriptorEntry {
            address_low: address as u32,
            address_high: (address >> 32) as u32,
            length,
            flags: (if last { 1 } else { 0 }) | (if interrupt { 2 } else { 0 }),
        }
    }
}

// ============================================================================
// SES HATASI
// ============================================================================

/// Ses alt sistemi hata türleri.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioError {
    NoController,
    NoCodec,
    NoStream,
    BufferError,
    FormatNotSupported,
    CodecError,
    ControllerError,
    Timeout,
}

// ============================================================================
// SES YÖNETİCİSİ
// ============================================================================

static HDA_CONTROLLERS: Mutex<Vec<HdaController>> = Mutex::new(Vec::new());
static AUDIO_STREAMS: Mutex<BTreeMap<u8, AudioStream>> = Mutex::new(BTreeMap::new());
static NEXT_STREAM_ID: AtomicU32 = AtomicU32::new(1);
static AUDIO_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Ses alt sistemini başlatır — HDA denetleyicilerini bulur ve açar.
pub fn init() {
    if AUDIO_INITIALIZED.swap(true, Ordering::SeqCst) {
        return;
    }
    
    crate::serial_println!("[AUDIO] HDA alt sistemi başlatılıyor...");

    // HDA denetleyicilerini bul
    let controllers = discover_hda_controllers();
    
    let mut hda_ctrls = HDA_CONTROLLERS.lock();
    
    for mut ctrl in controllers {
        if ctrl.init().is_ok() {
            hda_ctrls.push(ctrl);
        }
    }
    
    crate::serial_println!("[AUDIO] Found {} HDA controllers", hda_ctrls.len());
}

/// Müzik çaları için basit ses arka uç nesnesi.
///
/// Yüksek seviyeli ses kontrolü sağlar: oynat/duraklat/durdur/ses düzeyi.
pub struct AudioBackend {
    pub volume: f32,
    pub playing: bool,
    pub position: f32,
}

impl AudioBackend {
    pub fn new() -> Self {
        Self {
            volume: 1.0,
            playing: false,
            position: 0.0,
        }
    }
    
    pub fn play(&mut self, _path: &str) {
        self.playing = true;
    }
    
    pub fn pause(&mut self) {
        self.playing = false;
    }
    
    pub fn stop(&mut self) {
        self.playing = false;
        self.position = 0.0;
    }
    
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
    }
    
    pub fn seek(&mut self, position: f32) {
        self.position = position;
    }
}

lazy_static::lazy_static! {
    static ref AUDIO_BACKEND: Mutex<AudioBackend> = Mutex::new(AudioBackend::new());
}

/// Global ses arka ucuna referans döndürür.
pub fn get_audio() -> Option<&'static Mutex<AudioBackend>> {
    Some(&AUDIO_BACKEND)
}

/// PCI taramasıyla HDA denetleyicilerini keşfeder.
pub fn discover_hda_controllers() -> Vec<HdaController> {
    let mut controllers = Vec::new();
    
    let devices = crate::drivers::pci::scan();
    for dev in devices {
        if dev.class_code == PCI_CLASS_MULTIMEDIA && dev.subclass == PCI_SUBCLASS_HDA {
            controllers.push(HdaController::new(dev.bus, dev.device, dev.function));
        }
    }
    
    controllers
}

/// Varsayılan (ilk) HDA denetleyicisini döndürür.
pub fn default_controller() -> Option<HdaController> {
    HDA_CONTROLLERS.lock().first().cloned()
}

/// Varsayılan denetleyicideki ilk codec'i döndürür.
pub fn default_codec() -> Option<HdaCodec> {
    HDA_CONTROLLERS.lock()
        .first()
        .and_then(|ctrl| ctrl.codecs.first().cloned())
}

/// Oynatma için yeni bir ses akışı açar, akış kimliği döndürür.
pub fn open_playback_stream(format: AudioFormat) -> Result<u8, AudioError> {
    let stream_id = NEXT_STREAM_ID.fetch_add(1, Ordering::SeqCst) as u8;
    
    let stream = AudioStream::new(stream_id, StreamDirection::Playback, format);
    
    AUDIO_STREAMS.lock().insert(stream_id, stream);
    
    crate::serial_println!("[AUDIO] Opened playback stream {} ({}Hz, {}bit, {}ch)",
        stream_id, format.sample_rate, format.bits_per_sample, format.channels);
    
    Ok(stream_id)
}

/// Bir ses akışını kapatır ve tampon belleği serbest bırakır.
pub fn close_stream(stream_id: u8) -> Result<(), AudioError> {
    let mut streams = AUDIO_STREAMS.lock();
    if streams.remove(&stream_id).is_some() {
        crate::serial_println!("[AUDIO] Closed stream {}", stream_id);
        Ok(())
    } else {
        Err(AudioError::NoStream)
    }
}

/// Ses verisi (PCM) akış tamponuna yazar.
pub fn write_stream(stream_id: u8, data: &[u8]) -> Result<usize, AudioError> {
    let mut streams = AUDIO_STREAMS.lock();
    let stream = streams.get_mut(&stream_id).ok_or(AudioError::NoStream)?;
    
    stream.set_buffer(data.to_vec());
    
    Ok(data.len())
}

/// Belirtilen akışı oynatmaya başlatır.
pub fn start_stream(stream_id: u8) -> Result<(), AudioError> {
    let mut streams = AUDIO_STREAMS.lock();
    let stream = streams.get_mut(&stream_id).ok_or(AudioError::NoStream)?;
    
    stream.start();
    
    crate::serial_println!("[AUDIO] Started stream {}", stream_id);
    
    Ok(())
}

/// Belirtilen akışı durdurur.
pub fn stop_stream(stream_id: u8) -> Result<(), AudioError> {
    let mut streams = AUDIO_STREAMS.lock();
    let stream = streams.get_mut(&stream_id).ok_or(AudioError::NoStream)?;
    
    stream.stop();
    
    crate::serial_println!("[AUDIO] Stopped stream {}", stream_id);
    
    Ok(())
}

/// Ses düzeyini 0-100 arasında ayarlar.
pub fn set_volume(volume: u8) -> Result<(), AudioError> {
    let mut controllers = HDA_CONTROLLERS.lock();
    let ctrl = controllers.first_mut().ok_or(AudioError::NoController)?;

    if let Some(codec) = ctrl.codecs.first_mut() {
        if let Some(dac) = codec.find_output_dac() {
            // DAC üzerindeki ses düzeyini ayarla
            let _ = dac;
        }
    }
    
    crate::serial_println!("[AUDIO] Volume set to {}%", volume);
    
    Ok(())
}

/// Sesi susturur veya susturmayı kaldırır.
pub fn set_mute(mute: bool) -> Result<(), AudioError> {
    let mut controllers = HDA_CONTROLLERS.lock();
    let ctrl = controllers.first_mut().ok_or(AudioError::NoController)?;
    
    if let Some(codec) = ctrl.codecs.first_mut() {
        if let Some(dac) = codec.find_output_dac() {
            let _ = dac;
        }
    }
    
    crate::serial_println!("[AUDIO] Mute: {}", mute);
    
    Ok(())
}

/// Akışın tampondaki mevcut okuma konumunu (byte) döndürür.
pub fn get_stream_position(stream_id: u8) -> Result<usize, AudioError> {
    let streams = AUDIO_STREAMS.lock();
    let stream = streams.get(&stream_id).ok_or(AudioError::NoStream)?;
    
    Ok(stream.position)
}

/// Belirtilen akışın oynatılıp oynatılmadığını kontrol eder.
pub fn is_stream_playing(stream_id: u8) -> Result<bool, AudioError> {
    let streams = AUDIO_STREAMS.lock();
    let stream = streams.get(&stream_id).ok_or(AudioError::NoStream)?;
    
    Ok(stream.playing)
}

/// Ses denetleyicisinin yeteneklerini döndürür.
pub fn get_capabilities() -> Option<AudioCapabilities> {
    let controllers = HDA_CONTROLLERS.lock();
    let ctrl = controllers.first()?;
    
    Some(AudioCapabilities {
        max_channels: 8,
        max_sample_rate: 192000,
        max_bits_per_sample: 32,
        output_streams: ctrl.output_streams,
        input_streams: ctrl.input_streams,
    })
}

/// Ses denetleyicisi yetenek bilgileri.
#[derive(Clone, Copy, Debug)]
pub struct AudioCapabilities {
    pub max_channels: u8,
    pub max_sample_rate: u32,
    pub max_bits_per_sample: u8,
    pub output_streams: u8,
    pub input_streams: u8,
}

// ============================================================================
// DMA SES TRANSFERİ
// ============================================================================

/// DMA transfer durumu.
///
/// Ses verisini CPU müdahalesi olmadan bellekten ses donanımına aktaran
/// DMA (Direct Memory Access) transferini yönetir.
/// BDL ile birlikte çalışır; `buffer_addr` fiziksel bellek adresidir.
#[derive(Clone, Debug)]
pub struct DmaAudioTransfer {
    pub buffer_addr: u64,
    pub buffer_size: usize,
    pub position: usize,
    pub active: bool,
    pub callback: Option<fn()>,
}

impl DmaAudioTransfer {
    pub fn new(buffer_addr: u64, buffer_size: usize) -> Self {
        DmaAudioTransfer {
            buffer_addr,
            buffer_size,
            position: 0,
            active: false,
            callback: None,
        }
    }

    /// DMA transferini başlatır.
    pub fn start(&mut self) {
        self.active = true;
        self.position = 0;
    }

    /// DMA transferini durdurur.
    pub fn stop(&mut self) {
        self.active = false;
    }

    /// Transfer tamamlandığında çağrılacak geri çağırım fonksiyonunu ayarlar.
    pub fn set_callback(&mut self, callback: fn()) {
        self.callback = Some(callback);
    }

    /// DMA için sonraki tampon parçasını döndürür.
    /// Transferin tamamlanıp tamamlanmadığını kontrol eder ve geri çağırımı tetikler.
    pub fn get_next_fragment(&mut self, fragment_size: usize) -> Option<(u64, usize)> {
        if !self.active {
            return None;
        }

        if self.position >= self.buffer_size {
            // Transfer tamamlandı
            if let Some(cb) = self.callback {
                cb();
            }
            return None;
        }

        let remaining = self.buffer_size - self.position;
        let size = fragment_size.min(remaining);
        let addr = self.buffer_addr + self.position as u64;

        self.position += size;

        Some((addr, size))
    }
}

// ============================================================================
// SES MİKSERİ
// ============================================================================

/// Mikser kanalı.
///
/// Bir ses kaynağını karıştırıcıya bağlar.
/// `volume`: Ses düzeyi 0-100.
/// `pan`: Stereo pan — negatif = sol, pozitif = sağ (-100..100).
/// `solo`: Yalnızca bu kanal aktifken diğerleri susturulur.
#[derive(Clone, Debug)]
pub struct MixerChannel {
    pub id: u8,
    pub name: alloc::string::String,
    pub volume: u8,       // 0-100
    pub pan: i8,          // -100 (sol) ile 100 (sağ) arası
    pub muted: bool,
    pub solo: bool,
    pub input_stream: Option<u8>,
}

impl MixerChannel {
    pub fn new(id: u8, name: &str) -> Self {
        MixerChannel {
            id,
            name: name.into(),
            volume: 100,
            pan: 0,
            muted: false,
            solo: false,
            input_stream: None,
        }
    }

    /// Stereo örneğe kanal ses düzeyi ve pan ayarını uygular.
    pub fn apply_to_sample(&self, left: i16, right: i16) -> (i16, i16) {
        if self.muted {
            return (0, 0);
        }

        // Ses düzeyini uygula (0-100%)
        let vol = self.volume as i32;
        let left_vol = left as i32 * vol / 100;
        let right_vol = right as i32 * vol / 100;

        // Pan'ı uygula (-100 ile 100 arası)
        let pan = self.pan as i32;
        let left_pan = if pan > 0 {
            (100 - pan) * left_vol / 100
        } else {
            left_vol
        };
        let right_pan = if pan < 0 {
            (100 + pan) * right_vol / 100
        } else {
            right_vol
        };

        (left_pan.clamp(-32768, 32767) as i16, right_pan.clamp(-32768, 32767) as i16)
    }
}

/// Ses mikseri — birden fazla ses akışını tek çıktıda birleştirir.
///
/// Her akış bir `MixerChannel`'a bağlanır.
/// Tüm kanallar `mix_to_buffer()` ile karıştırılır;
/// master ses düzeyi en son uygulanır.
#[derive(Clone, Debug)]
pub struct AudioMixer {
    pub channels: Vec<MixerChannel>,
    pub master_volume: u8,
    pub master_muted: bool,
    pub sample_rate: u32,
    pub buffer_size: usize,
}

impl AudioMixer {
    pub fn new(sample_rate: u32, buffer_size: usize) -> Self {
        AudioMixer {
            channels: Vec::new(),
            master_volume: 100,
            master_muted: false,
            sample_rate,
            buffer_size,
        }
    }

    /// Yeni bir kanal ekler, kanal kimliği döndürür.
    pub fn add_channel(&mut self, name: &str) -> u8 {
        let id = self.channels.len() as u8;
        self.channels.push(MixerChannel::new(id, name));
        id
    }

    /// Belirtilen kimlikli kanalı kaldırır.
    pub fn remove_channel(&mut self, id: u8) {
        self.channels.retain(|c| c.id != id);
    }

    /// Kanal referansını döndürür.
    pub fn get_channel(&self, id: u8) -> Option<&MixerChannel> {
        self.channels.iter().find(|c| c.id == id)
    }

    /// Değiştirilebilir kanal referansını döndürür.
    pub fn get_channel_mut(&mut self, id: u8) -> Option<&mut MixerChannel> {
        self.channels.iter_mut().find(|c| c.id == id)
    }

    /// Tüm kanalları çıkış tamponunda karıştırır.
    /// Solo kanallar aktifse diğerleri susturulur.
    /// Master ses düzeyi en son uygulanır.
    pub fn mix_to_buffer(&self, streams: &BTreeMap<u8, AudioStream>) -> Vec<u8> {
        let samples = self.buffer_size / 4; // 16-bit stereo = örnek başına 4 bayt
        let mut output = vec![0i32; samples * 2]; // Stereo

        // Herhangi bir solo kanal var mı kontrol et
        let any_solo = self.channels.iter().any(|c| c.solo);

        for channel in &self.channels {
            // Susturulmuş ya da solo aktif ama bu kanal solo değilse atla
            if channel.muted || (any_solo && !channel.solo) {
                continue;
            }

            if let Some(stream_id) = channel.input_stream {
                if let Some(stream) = streams.get(&stream_id) {
                    if stream.playing && stream.format.channels == 2 && stream.format.bits_per_sample == 16 {
                        // Bu akıştan örnekleri karıştır
                        for i in 0..samples {
                            let sample_offset = (stream.position + i * 4).min(stream.buffer.len() - 4);
                            if sample_offset + 4 <= stream.buffer.len() {
                                let left = i16::from_le_bytes([stream.buffer[sample_offset], stream.buffer[sample_offset + 1]]);
                                let right = i16::from_le_bytes([stream.buffer[sample_offset + 2], stream.buffer[sample_offset + 3]]);

                                let (left_out, right_out) = channel.apply_to_sample(left, right);

                                output[i * 2] += left_out as i32;
                                output[i * 2 + 1] += right_out as i32;
                            }
                        }
                    }
                }
            }
        }

        // Master ses düzeyini uygula ve bayta dönüştür
        let mut output_bytes = Vec::with_capacity(self.buffer_size);
        for i in 0..samples * 2 {
            let sample = if self.master_muted {
                0
            } else {
                (output[i] * self.master_volume as i32 / 100).clamp(-32768, 32767)
            };
            output_bytes.extend_from_slice(&(sample as i16).to_le_bytes());
        }

        output_bytes
    }

    /// Tüm kanalların üstüne uygulanan ana ses düzeyini ayarlar (0-100).
    pub fn set_master_volume(&mut self, volume: u8) {
        self.master_volume = volume.min(100);
    }

    /// Ana kanalı susturur veya susturmayı kaldırır.
    pub fn set_master_mute(&mut self, muted: bool) {
        self.master_muted = muted;
    }
}

// ============================================================================
// PCM SES FORMATI
// ============================================================================

/// PCM format tanımlaması.
///
/// PCM (Pulse Code Modulation) ses formatını açıklar.
/// Örnekleme hızı, bit derinliği, kanal sayısı ve bayt düzeni bilgilerini içerir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PcmFormat {
    pub sample_rate: u32,
    pub bits_per_sample: u8,
    pub channels: u8,
    pub is_float: bool,
    pub is_big_endian: bool,
}

impl PcmFormat {
    pub fn new(sample_rate: u32, bits_per_sample: u8, channels: u8) -> Self {
        PcmFormat {
            sample_rate,
            bits_per_sample,
            channels,
            is_float: false,
            is_big_endian: false,
        }
    }

    /// CD kalitesi PCM (44.1 kHz, 16-bit, stereo).
    pub fn cd_quality() -> Self {
        Self::new(44100, 16, 2)
    }

    /// DVD kalitesi PCM (48 kHz, 16-bit, stereo).
    pub fn dvd_quality() -> Self {
        Self::new(48000, 16, 2)
    }

    /// Blu-ray kalitesi PCM (96 kHz, 24-bit, 5.1 kanal).
    pub fn bluray_quality() -> Self {
        Self::new(96000, 24, 6)
    }

    /// Örnek başına bayt sayısını döndürür.
    pub fn bytes_per_sample(&self) -> usize {
        (self.bits_per_sample as usize + 7) / 8
    }

    /// Çerçeve boyutunu (tüm kanallar için bir örnek) döndürür.
    pub fn frame_size(&self) -> usize {
        self.bytes_per_sample() * self.channels as usize
    }

    /// Saniye başına bayt hızını döndürür.
    pub fn byte_rate(&self) -> u32 {
        self.sample_rate * self.frame_size() as u32
    }

    /// Bir PCM örneğini ham bayta dönüştürür.
    pub fn sample_to_bytes(&self, sample: i32, buf: &mut [u8]) {
        let bytes = self.bytes_per_sample();
        match bytes {
            1 => {
                // 8-bit işaretsiz
                buf[0] = ((sample + 128) & 0xFF) as u8;
            }
            2 => {
                // 16-bit işaretli
                let val = (sample as i16).clamp(-32768, 32767);
                if self.is_big_endian {
                    buf[0] = (val >> 8) as u8;
                    buf[1] = val as u8;
                } else {
                    buf[0] = val as u8;
                    buf[1] = (val >> 8) as u8;
                }
            }
            3 => {
                // 24-bit işaretli
                if self.is_big_endian {
                    buf[0] = ((sample >> 16) & 0xFF) as u8;
                    buf[1] = ((sample >> 8) & 0xFF) as u8;
                    buf[2] = (sample & 0xFF) as u8;
                } else {
                    buf[0] = (sample & 0xFF) as u8;
                    buf[1] = ((sample >> 8) & 0xFF) as u8;
                    buf[2] = ((sample >> 16) & 0xFF) as u8;
                }
            }
            4 => {
                // 32-bit işaretli
                if self.is_big_endian {
                    buf[0] = ((sample >> 24) & 0xFF) as u8;
                    buf[1] = ((sample >> 16) & 0xFF) as u8;
                    buf[2] = ((sample >> 8) & 0xFF) as u8;
                    buf[3] = (sample & 0xFF) as u8;
                } else {
                    buf[0] = (sample & 0xFF) as u8;
                    buf[1] = ((sample >> 8) & 0xFF) as u8;
                    buf[2] = ((sample >> 16) & 0xFF) as u8;
                    buf[3] = ((sample >> 24) & 0xFF) as u8;
                }
            }
            _ => {}
        }
    }

    /// Ham bayttan PCM örneğini okur.
    pub fn bytes_to_sample(&self, buf: &[u8]) -> i32 {
        let bytes = self.bytes_per_sample().min(buf.len());
        match bytes {
            1 => (buf[0] as i32) - 128,
            2 => {
                if self.is_big_endian {
                    ((buf[0] as i32) << 8 | buf[1] as i32) as i16 as i32
                } else {
                    i16::from_le_bytes([buf[0], buf[1]]) as i32
                }
            }
            3 => {
                if self.is_big_endian {
                    (buf[0] as i32) << 16 | (buf[1] as i32) << 8 | buf[2] as i32
                } else {
                    (buf[0] as i32) | (buf[1] as i32) << 8 | (buf[2] as i32) << 16
                }
            }
            4 => {
                if self.is_big_endian {
                    i32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]])
                } else {
                    i32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]])
                }
            }
            _ => 0,
        }
    }
}

// ============================================================================
// SES CODEC'LERİ
// ============================================================================

/// Ses codec özelliği (trait).
/// Kodlama/çözme işlemlerini soyutlar.
pub trait AudioCodec {
    /// Ses verisini çözer (decode).
    fn decode(&self, input: &[u8], output: &mut Vec<u8>) -> Result<usize, AudioError>;

    /// Ses verisini kodlar (encode).
    fn encode(&self, input: &[u8], output: &mut Vec<u8>) -> Result<usize, AudioError>;

    /// Codec adını döndürür.
    fn name(&self) -> &str;

    /// Çıktı PCM formatını döndürür.
    fn output_format(&self) -> PcmFormat;
}

/// Sinüs dalga üreticisi codec'i.
/// Test, zil ve demo amaçlı saf ton üretir.
pub struct SineWaveCodec {
    pub frequency: f32,
    pub sample_rate: u32,
    pub amplitude: f32,
    pub phase: f32,
}

/// `no_std` ortamı için Taylor serisi sinüs yaklaşımı.
/// libm olmadan kayan noktalı sin hesaplar.
fn sin_approx(x: f32) -> f32 {
    // [-PI, PI] aralığını normalize et
    let mut x = x;
    let pi = core::f32::consts::PI;
    let two_pi = 2.0 * pi;

    while x > pi {
        x -= two_pi;
    }
    while x < -pi {
        x += two_pi;
    }

    // Taylor serisi: sin(x) = x - x^3/3! + x^5/5! - x^7/7! + x^9/9!
    let x2 = x * x;
    let x3 = x2 * x;
    let x5 = x3 * x2;
    let x7 = x5 * x2;
    let x9 = x7 * x2;
    
    x - x3 / 6.0 + x5 / 120.0 - x7 / 5040.0 + x9 / 362880.0
}

impl SineWaveCodec {
    pub fn new(frequency: f32, sample_rate: u32) -> Self {
        SineWaveCodec {
            frequency,
            sample_rate,
            amplitude: 0.5,
            phase: 0.0,
        }
    }

    /// Belirli sayıda 16-bit mono sinüs dalga örneği üretir.
    pub fn generate(&mut self, samples: usize) -> Vec<u8> {
        let mut output = Vec::with_capacity(samples * 2);
        let step = 2.0 * core::f32::consts::PI * self.frequency / self.sample_rate as f32;

        for _ in 0..samples {
            let sample = self.amplitude * sin_approx(self.phase);
            let sample_i16 = (sample * 32767.0) as i16;
            output.extend_from_slice(&sample_i16.to_le_bytes());
            self.phase += step;
            if self.phase > 2.0 * core::f32::consts::PI {
                self.phase -= 2.0 * core::f32::consts::PI;
            }
        }

        output
    }

    /// Sol ve sağ kanal için farklı frekanslarda stereo sinüs dalga üretir.
    pub fn generate_stereo(&mut self, samples: usize, left_freq: f32, right_freq: f32) -> Vec<u8> {
        let mut output = Vec::with_capacity(samples * 4);
        let step_left = 2.0 * core::f32::consts::PI * left_freq / self.sample_rate as f32;
        let step_right = 2.0 * core::f32::consts::PI * right_freq / self.sample_rate as f32;
        let mut phase_left = 0.0f32;
        let mut phase_right = 0.0f32;

        for _ in 0..samples {
            let left_sample = self.amplitude * sin_approx(phase_left);
            let right_sample = self.amplitude * sin_approx(phase_right);

            let left_i16 = (left_sample * 32767.0) as i16;
            let right_i16 = (right_sample * 32767.0) as i16;

            output.extend_from_slice(&left_i16.to_le_bytes());
            output.extend_from_slice(&right_i16.to_le_bytes());

            phase_left += step_left;
            phase_right += step_right;

            if phase_left > 2.0 * core::f32::consts::PI {
                phase_left -= 2.0 * core::f32::consts::PI;
            }
            if phase_right > 2.0 * core::f32::consts::PI {
                phase_right -= 2.0 * core::f32::consts::PI;
            }
        }

        output
    }
}

impl AudioCodec for SineWaveCodec {
    fn decode(&self, _input: &[u8], output: &mut Vec<u8>) -> Result<usize, AudioError> {
        // 1024 örnek üret
        let mut codec = self.clone();
        let data = codec.generate(1024);
        output.extend_from_slice(&data);
        Ok(data.len())
    }

    fn encode(&self, _input: &[u8], _output: &mut Vec<u8>) -> Result<usize, AudioError> {
        Err(AudioError::FormatNotSupported)
    }

    fn name(&self) -> &str {
        "SineWave"
    }

    fn output_format(&self) -> PcmFormat {
        PcmFormat::new(self.sample_rate, 16, 1)
    }
}

impl Clone for SineWaveCodec {
    fn clone(&self) -> Self {
        SineWaveCodec {
            frequency: self.frequency,
            sample_rate: self.sample_rate,
            amplitude: self.amplitude,
            phase: self.phase,
        }
    }
}

/// Beyaz gürültü üreticisi codec'i.
/// Rastgele PCM örnekleri üretir (test ve efekt amaçlı).
pub struct WhiteNoiseCodec {
    pub sample_rate: u32,
    pub amplitude: f32,
    pub state: u32,
}

impl WhiteNoiseCodec {
    pub fn new(sample_rate: u32) -> Self {
        WhiteNoiseCodec {
            sample_rate,
            amplitude: 0.3,
            state: 0x12345678,
        }
    }

    /// Basit LFSR (Doğrusal Geri Beslemeli Kaydırma Yazmacı) ile sözde rastgele sayı üretir.
    fn next_random(&mut self) -> u32 {
        let bit = ((self.state >> 0) ^ (self.state >> 2) ^ (self.state >> 3) ^ (self.state >> 5)) & 1;
        self.state = (self.state >> 1) | (bit << 31);
        self.state
    }

    /// Belirtilen sayıda beyaz gürültü örneği üretir.
    pub fn generate(&mut self, samples: usize) -> Vec<u8> {
        let mut output = Vec::with_capacity(samples * 2);

        for _ in 0..samples {
            let rand = self.next_random();
            let sample = ((rand as f32 / u32::MAX as f32) * 2.0 - 1.0) * self.amplitude;
            let sample_i16 = (sample * 32767.0) as i16;
            output.extend_from_slice(&sample_i16.to_le_bytes());
        }

        output
    }
}

impl AudioCodec for WhiteNoiseCodec {
    fn decode(&self, _input: &[u8], output: &mut Vec<u8>) -> Result<usize, AudioError> {
        let mut codec = self.clone();
        let data = codec.generate(1024);
        output.extend_from_slice(&data);
        Ok(data.len())
    }

    fn encode(&self, _input: &[u8], _output: &mut Vec<u8>) -> Result<usize, AudioError> {
        Err(AudioError::FormatNotSupported)
    }

    fn name(&self) -> &str {
        "WhiteNoise"
    }

    fn output_format(&self) -> PcmFormat {
        PcmFormat::new(self.sample_rate, 16, 1)
    }
}

impl Clone for WhiteNoiseCodec {
    fn clone(&self) -> Self {
        WhiteNoiseCodec {
            sample_rate: self.sample_rate,
            amplitude: self.amplitude,
            state: self.state,
        }
    }
}

/// Basit μ-law (G.711) codec'i.
/// Telefon/VoIP kalitesinde ses kodlama/çözme yapar.
/// 8-bit μ-law baytı 16-bit lineer PCM'e dönüştürür.
pub struct MuLawCodec {
    pub sample_rate: u32,
}

impl MuLawCodec {
    pub fn new(sample_rate: u32) -> Self {
        MuLawCodec { sample_rate }
    }

    /// μ-law baytını lineer 16-bit örneğe dönüştürür.
    pub fn decode_sample(sample: u8) -> i16 {
        // μ-law çözme
        let sample = sample ^ 0xFF; // Tüm bitleri ters çevir
        let sign = if sample & 0x80 != 0 { -1 } else { 1 };
        let exponent = (sample >> 4) & 0x07;
        let mantissa = sample & 0x0F;

        let decoded = (33 * (2 * mantissa as i32 + 33) * (1 << exponent) - 33) * sign;
        decoded.clamp(-32768, 32767) as i16
    }

    /// Lineer 16-bit örneği μ-law baytına kodlar.
    pub fn encode_sample(sample: i16) -> u8 {
        let sign = if sample < 0 { 0x80 } else { 0 };
        let sample = sample.abs() as i32;

        let exponent = if sample > 0x1F {
            let mut exp = 7;
            while exp > 0 && sample <= (0x20 << exp) {
                exp -= 1;
            }
            exp
        } else {
            0
        };

        let mantissa = (sample >> (exponent + 2)) & 0x0F;
        let encoded = (sign | (exponent << 4) | mantissa) ^ 0xFF;
        encoded as u8
    }
}

impl AudioCodec for MuLawCodec {
    fn decode(&self, input: &[u8], output: &mut Vec<u8>) -> Result<usize, AudioError> {
        output.reserve(input.len() * 2);
        for sample in input {
            let decoded = Self::decode_sample(*sample);
            output.extend_from_slice(&decoded.to_le_bytes());
        }
        Ok(input.len() * 2)
    }

    fn encode(&self, input: &[u8], output: &mut Vec<u8>) -> Result<usize, AudioError> {
        if input.len() % 2 != 0 {
            return Err(AudioError::BufferError);
        }

        output.reserve(input.len() / 2);
        for chunk in input.chunks(2) {
            let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
            output.push(Self::encode_sample(sample));
        }
        Ok(input.len() / 2)
    }

    fn name(&self) -> &str {
        "MuLaw"
    }

    fn output_format(&self) -> PcmFormat {
        PcmFormat::new(self.sample_rate, 16, 1)
    }
}

/// Basit A-law (G.711) codec'i.
/// Avrupa telefon standartlarında kullanılan logaritmik kodlama.
/// 8-bit A-law baytı 16-bit lineer PCM'e dönüştürür.
pub struct ALawCodec {
    pub sample_rate: u32,
}

impl ALawCodec {
    pub fn new(sample_rate: u32) -> Self {
        ALawCodec { sample_rate }
    }

    /// A-law baytını lineer 16-bit örneğe dönüştürür.
    pub fn decode_sample(sample: u8) -> i16 {
        let sample = sample ^ 0x55; // Çift bitleri değiştir
        let sign = if sample & 0x80 != 0 { -1 } else { 1 };
        let exponent = (sample >> 4) & 0x07;
        let mantissa = sample & 0x0F;

        let decoded = if exponent == 0 {
            (mantissa as i32 * 2 + 1) * 16 * sign
        } else {
            ((1 << exponent) * (mantissa as i32 * 2 + 33) - 32) * sign
        };
        decoded.clamp(-32768, 32767) as i16
    }

    /// Lineer 16-bit örneği A-law baytına kodlar.
    pub fn encode_sample(sample: i16) -> u8 {
        let sign = if sample < 0 { 0x80 } else { 0 };
        let sample = sample.abs() as i32;

        let (exponent, mantissa) = if sample > 0x0F {
            let mut exp = 7;
            while exp > 0 && sample <= (0x10 << exp) {
                exp -= 1;
            }
            (exp, (sample >> (exp + 3)) & 0x0F)
        } else {
            (0, sample >> 1)
        };

        let encoded = sign | (exponent << 4) | mantissa;
        (encoded ^ 0x55) as u8
    }
}

impl AudioCodec for ALawCodec {
    fn decode(&self, input: &[u8], output: &mut Vec<u8>) -> Result<usize, AudioError> {
        output.reserve(input.len() * 2);
        for sample in input {
            let decoded = Self::decode_sample(*sample);
            output.extend_from_slice(&decoded.to_le_bytes());
        }
        Ok(input.len() * 2)
    }

    fn encode(&self, input: &[u8], output: &mut Vec<u8>) -> Result<usize, AudioError> {
        if input.len() % 2 != 0 {
            return Err(AudioError::BufferError);
        }

        output.reserve(input.len() / 2);
        for chunk in input.chunks(2) {
            let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
            output.push(Self::encode_sample(sample));
        }
        Ok(input.len() / 2)
    }

    fn name(&self) -> &str {
        "ALaw"
    }

    fn output_format(&self) -> PcmFormat {
        PcmFormat::new(self.sample_rate, 16, 1)
    }
}

// ============================================================================
// GLOBAL SES MİKSERİ
// ============================================================================

static AUDIO_MIXER: Mutex<Option<AudioMixer>> = Mutex::new(None);

/// Ses mikserini başlatır.
pub fn init_mixer(sample_rate: u32, buffer_size: usize) {
    *AUDIO_MIXER.lock() = Some(AudioMixer::new(sample_rate, buffer_size));
}

/// Global ses mikserini klonlanmış olarak döndürür.
pub fn get_mixer() -> Option<AudioMixer> {
    AUDIO_MIXER.lock().clone()
}

/// Miksere yeni bir kanal ekler, kanal kimliği döndürür.
pub fn add_mixer_channel(name: &str) -> Option<u8> {
    let mut mixer = AUDIO_MIXER.lock();
    mixer.as_mut().map(|m| m.add_channel(name))
}

/// Belirtilen kanalın ses düzeyini ayarlar.
pub fn set_channel_volume(channel_id: u8, volume: u8) -> Result<(), AudioError> {
    let mut mixer = AUDIO_MIXER.lock();
    let mixer = mixer.as_mut().ok_or(AudioError::NoController)?;
    let channel = mixer.get_channel_mut(channel_id).ok_or(AudioError::NoStream)?;
    channel.volume = volume.min(100);
    Ok(())
}

/// Kanalın pan (sol-sağ denge) değerini ayarlar (-100 sol, 100 sağ).
pub fn set_channel_pan(channel_id: u8, pan: i8) -> Result<(), AudioError> {
    let mut mixer = AUDIO_MIXER.lock();
    let mixer = mixer.as_mut().ok_or(AudioError::NoController)?;
    let channel = mixer.get_channel_mut(channel_id).ok_or(AudioError::NoStream)?;
    channel.pan = pan.clamp(-100, 100);
    Ok(())
}

/// Kanalı susturur veya susturmayı kaldırır.
pub fn set_channel_mute(channel_id: u8, muted: bool) -> Result<(), AudioError> {
    let mut mixer = AUDIO_MIXER.lock();
    let mixer = mixer.as_mut().ok_or(AudioError::NoController)?;
    let channel = mixer.get_channel_mut(channel_id).ok_or(AudioError::NoStream)?;
    channel.muted = muted;
    Ok(())
}

/// Kanalı solo moduna alır veya çıkarır.
pub fn set_channel_solo(channel_id: u8, solo: bool) -> Result<(), AudioError> {
    let mut mixer = AUDIO_MIXER.lock();
    let mixer = mixer.as_mut().ok_or(AudioError::NoController)?;
    let channel = mixer.get_channel_mut(channel_id).ok_or(AudioError::NoStream)?;
    channel.solo = solo;
    Ok(())
}

/// Bir ses akışını mikser kanalına bağlar.
pub fn link_stream_to_channel(channel_id: u8, stream_id: u8) -> Result<(), AudioError> {
    let mut mixer = AUDIO_MIXER.lock();
    let mixer = mixer.as_mut().ok_or(AudioError::NoController)?;
    let channel = mixer.get_channel_mut(channel_id).ok_or(AudioError::NoStream)?;
    channel.input_stream = Some(stream_id);
    Ok(())
}

/// Tüm akışları karıştırarak tek bir çıkış tamponu üretir.
pub fn mix_streams() -> Option<Vec<u8>> {
    let mixer = AUDIO_MIXER.lock();
    let mixer = mixer.as_ref()?;
    let streams = AUDIO_STREAMS.lock();
    Some(mixer.mix_to_buffer(&streams))
}
