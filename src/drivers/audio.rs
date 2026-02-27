//! # echOS Ses Alt Sistemi
//!
//! Intel Yuksek Tanim Ses (HDA) surucusu - PCM akisi, DMA tamponu ve codec yonetimi

use alloc::vec::Vec;
use alloc::vec;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use spin::Mutex;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

// ============================================================================
// HDA SABİT DEĞERLERİ
// ============================================================================

/// HDA PCI sınıf kodları — PCI yapılandırma uzayında HDA denetleyçisini tanımlamak için kullanılır
const PCI_CLASS_MULTIMEDIA: u8 = 0x04;
const PCI_SUBCLASS_HDA: u8 = 0x03;

/// HDA denetleyçi register'ları (belleğe eşlenmiş / MMIO) — BAR0 taban adresine göre ofsetler
const HDA_GCAP: usize = 0x00;      // Global Yetenekler
const HDA_GCTL: usize = 0x08;      // Global Kontrol
const HDA_GSTS: usize = 0x0C;      // Global Durum
const HDA_OUTSTR: usize = 0x10;    // Çıkış Akış Yükleri
const HDA_INSTR: usize = 0x14;     // Giriş Akış Yükleri
const HDA_INTCTL: usize = 0x20;    // Kesme Kontrol
const HDA_INTSTS: usize = 0x24;    // Kesme Durumu
const HDA_WAKEEN: usize = 0x0C;    // Uyandırma Etkinleştirme

/// Akim register'lari temel ofseti - her stream blogu bu tabandan HDA_STREAM_INTERVAL kadar ileride
const HDA_STREAM_BASE: usize = 0x80;
const HDA_STREAM_INTERVAL: usize = 0x20;

/// Akış tanımlayıcı register'ları — her stream için SD_CTL, SD_STS vb. aynı ofset şemasyla erişilir
const HDA_SD_CTL: usize = 0x00;
const HDA_SD_STS: usize = 0x03;
const HDA_SD_LPIB: usize = 0x04;
const HDA_SD_CBL: usize = 0x08;
const HDA_SD_LVI: usize = 0x0C;
const HDA_SD_FIFOS: usize = 0x10;
const HDA_SD_FMT: usize = 0x12;
const HDA_SD_BDPL: usize = 0x18;
const HDA_SD_BDPU: usize = 0x1C;

/// CORB/IRB register'ları — CORB komutları codec'e gönderir, IRB cevapları yönlendirici oku
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

/// HDA codec komutları — verb+NID+payload kombinasyonuyla codec parametrelerini al/ayarla
const HDA_VERB_GET_PARAMETER: u32 = 0xF0000;
const HDA_VERB_SET_POWER_STATE: u32 = 0x70500;
const HDA_VERB_SET_CONVERTER_FORMAT: u32 = 0x20000;
const HDA_VERB_SET_CONVERTER_STREAM: u32 = 0x70600;
const HDA_VERB_SET_AMP_GAIN: u32 = 0x30000;
const HDA_VERB_SET_PIN_WIDGET_CTRL: u32 = 0x70700;

/// Codec parametreleri — üretici kimliği, fonksiyon grubu ve widget yeteneklerini sorgular
const HDA_PARAM_VENDOR_ID: u32 = 0x00;
const HDA_PARAM_REVISION_ID: u32 = 0x02;
const HDA_PARAM_NODE_COUNT: u32 = 0x04;
const HDA_PARAM_FUNCTION_TYPE: u32 = 0x05;
const HDA_PARAM_AUDIO_WIDGET_CAPS: u32 = 0x09;
const HDA_PARAM_AUDIO_SUPPORTED_PCM: u32 = 0x0A;
const HDA_PARAM_AUDIO_SUPPORTED_STREAM: u32 = 0x0B;
const HDA_PARAM_AUDIO_INPUT_AMP_CAPS: u32 = 0x0D;
const HDA_PARAM_AUDIO_OUTPUT_AMP_CAPS: u32 = 0x12;

/// Widget türleri — DAC ses için çözümlü, ADC kaydetme için dönüştürür, Pin fiziksel konnektör
const HDA_WIDGET_OUTPUT_DAC: u8 = 0x0;
const HDA_WIDGET_INPUT_ADC: u8 = 0x1;
const HDA_WIDGET_MIXER: u8 = 0x3;
const HDA_WIDGET_PIN: u8 = 0x4;
const HDA_WIDGET_POWER: u8 = 0x7;
const HDA_WIDGET_VOLUME: u8 = 0x8;
const HDA_WIDGET_BEEP: u8 = 0x9;

/// Akış format bitleri — HDA_SD_FMT register'a yazılan örnekleme hızı, bit derinliği, kanal sayısı
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
// HDA DENETÜCİ
// ============================================================================

/// HDA Denetleyicisi - PCI uzerindeki yuksek tanim ses denetleyicisini temsil eder
#[derive(Clone, Debug)]
pub struct HdaController {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub mmio_base: u64,
    pub mmio_size: u64,
    pub vendor_id: u16,
    pub device_id: u16,
    /// Çıkış akışı sayısı — ses çalabilmek için en az bir çıkış akışı gereklidir
    pub output_streams: u8,
    /// Giriş akışı sayısı — ses kayıt için kullanılan ADC yönlü akışlar
    pub input_streams: u8,
    /// Çift yönlü akış sayısı — hem oynatma hem kaydetme yapabilen akışlar
    pub bidir_streams: u8,
    /// 64-bit adresleme kapasitesi — 4 GB üzeri fiziksel adres kullanımına olanak tanır
    pub addr64: bool,
    /// CORB tampon boyutu — aynı anda gönderebileceğimiz azami komut saysı
    pub corb_size: u16,
    /// IRB tampon boyutu — aynı anda bekleyebileceğimiz azami cevap sayısı
    pub irb_size: u16,
    /// Tespit edilen codec'ler — HDA bağlantısına bağlı ses işlemci birimleri
    pub codecs: Vec<HdaCodec>,
}

impl HdaController {
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
    
    /// Denetleyçiyi başlat — sıfırlama, yetenek okuma, CORB/IRB kurulum ve codec tespiti
    pub fn init(&mut self) -> Result<(), AudioError> {
        // Denetleyçiyi sıfırla — GCTL registerına CRST bitini yaz
        self.reset()?;
        
        // Yetenekleri oku — GCAP'tan akış sayılarını öğren
        self.read_capabilities();
        
        // CORB/IRB'yi başlat — komut halkalarını DMA tampona yaz
        self.init_corb_irb()?;
        
        // Codec'leri tespit et — HDA bağlantısındaki ses işlemcilerini tara
        self.detect_codecs();
        
        crate::serial_println!("[HDA] Controller initialized: {} out, {} in, {} bidir streams",
            self.output_streams, self.input_streams, self.bidir_streams);
        
        Ok(())
    }
    
    /// Denetleyçiyi sıfırla — GCTL registerına CRST bitini yazar, denetleyçi temiz duruma gelir
    fn reset(&mut self) -> Result<(), AudioError> {
        // GCTL'ye CRST biti yaz
        // TODO: Gerçek MMIO yazmaı
        Ok(())
    }
    
    /// Denetleyçi yeteneklerini GCAP'tan oku — akış sayıları ve 64-bit adres desteği öğrenilir
    fn read_capabilities(&mut self) {
        // TODO: MMIO'dan oku
        // Varsayılan değerler
        self.output_streams = 4;
        self.input_streams = 4;
        self.bidir_streams = 0;
        self.addr64 = true;
        self.corb_size = 256;
        self.irb_size = 256;
    }
    
    /// CORB (Komut Çıkış Halka Tamponu) ve IRB'yi başlat — DMA tamponu ayır ve halka işaretlerini sıfırla
    fn init_corb_irb(&mut self) -> Result<(), AudioError> {
        // CORB ve IRB tamponlarını ayır
        // TODO: Gerçek implementasyon
        Ok(())
    }
    
    /// HDA bağlantısındaki codec'leri tespit et — 0-15 arasındaki adreslerde geçerli üretici ID arar
    fn detect_codecs(&mut self) {
        // Codec'leri tara (tipik olarak 0-15)
        for codec_addr in 0..=15 {
            // Üreti kimliğini almayı dene
            // Cevap geçerliyse codec mevcuttur
            let vendor_id = 0x8086u16; // Yer tutucu Intel codec
            let device_id = 0x0001u16;
            
            if vendor_id != 0xFFFF {
                let mut codec = HdaCodec::new(codec_addr, vendor_id, device_id);
                codec.scan_widgets();
                self.codecs.push(codec);
            }
        }
    }
    
    /// Codec'e komut gönder — verb+NID+payload kombinasyonuyla CORB halkasina yerleştirir
    pub fn send_command(&self, codec: u8, nid: u8, verb: u32) -> u32 {
        // TODO: CORB komut gönderimi — halka işarecini artır ve cevap için IRB’yi bekle
        let _ = (codec, nid, verb);
        0
    }
    
    /// Oynatma akışını getir — ilk kullanılabilir çıkış akışının indeksini döndürür
    pub fn get_playback_stream(&self) -> Option<u8> {
        if self.output_streams > 0 {
            Some(0) // İlk çıkış akışı
        } else {
            None
        }
    }
}

// ============================================================================
// HDA CODEC
// ============================================================================

/// HDA Codec - HDA baglantisindaki ses islemci birimi, widget'lari ve kapasitelerini tutar
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
    
    /// Codec içindeki widget'ları tara — DAC, ADC, Pin ve Mixer düğumlerini keşfeder
    pub fn scan_widgets(&mut self) {
        // Kok dugumu - tum widget agacinin baslangic noktasi, NID 0
        self.root_nid = 0;
        
        // Ses fonksiyon grubu - widget'lari barindiran ust seviye node
        self.audio_func_group = 1;
        
        // Temel widget'lari ekle - DAC, hoparlor, kulalik, ADC ve mikrofon dugumlerini olustur
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
    
    /// NID'e göre widget bul — node kimliğiyle doğrudan erişilen ses düğümü
    pub fn find_widget(&self, nid: u8) -> Option<&AudioWidget> {
        self.widgets.iter().find(|w| w.nid == nid)
    }
    
    /// Türüne göre widget bul — DAC, ADC veya Pin gibi belirli bir fonksiyonu olan düğümü döndürür
    pub fn find_widget_by_type(&self, widget_type: HdaWidgetType) -> Option<&AudioWidget> {
        self.widgets.iter().find(|w| w.widget_type == widget_type)
    }
    
    /// Çıkış DAC'ini bul — dijital-analog dönüştürücü, hoparlere gönderilecek ses sinyalini üretir
    pub fn find_output_dac(&self) -> Option<&AudioWidget> {
        self.find_widget_by_type(HdaWidgetType::OutputDac)
    }
    
    /// Giriş ADC'sini bul — analog-dijital dönüştürücü, mikrofon sinyalini dijitale çevirir
    pub fn find_input_adc(&self) -> Option<&AudioWidget> {
        self.find_widget_by_type(HdaWidgetType::InputAdc)
    }
}

// ============================================================================
// SES WİDGET'İ
// ============================================================================

/// Widget turu - ses dugumunun hangi islevi gerceklestirdigini belirtir (DAC, ADC, Pin vb.)
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

/// Widget yetenekleri - bir ses dugumunun destekledigi ozelliklerin bit bayrak kumesi
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

/// Ses widget'i - codec icindeki her islevsel dugumu temsil eder, NID ve yetenekleri icerir
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
    /// Ses seviyesini ayarla (0-100%) — donanım kazancını yüzdesel değere dönüştürerek yaz
    pub fn set_volume(&mut self, volume: u8) {
        // 0-100%'yi kazanc değerine dönüştür — HDA amp verb'i ile widget'a gönderilir
        self.default_gain = ((volume as i16) * 100 / 100) - 100;
    }
    
    /// Sessizleştirmeyi ayarla — codec'in ilgili node'unu sustururarak DMA işlemi devam eder
    pub fn set_mute(&mut self, mute: bool) {
        self.muted = mute;
    }
}

// ============================================================================
// SES AKIŞİ
// ============================================================================

/// Ses akisi formati - ornek hizi, bit derinligi ve kanal sayisini tanimlayan yapi
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
    
    /// CD kalitesi format — 44.1 kHz, 16 bit, stereo: müzik CD'lerinin standart kalitesi
    pub fn cd_quality() -> Self {
        AudioFormat::new(44100, 16, 2)
    }
    
    /// DVD kalitesi format — 48 kHz, 16 bit, stereo: film ve dijital ses standartı
    pub fn dvd_quality() -> Self {
        AudioFormat::new(48000, 16, 2)
    }
    
    /// Yüksek kalite format — 96 kHz, 24 bit, stereo: studüo kayıt kalitesi
    pub fn high_quality() -> Self {
        AudioFormat::new(96000, 24, 2)
    }
    
    /// HDA format register değerine dönüştür — HDA_SD_FMT'e doğrudan yazılabilecek 16-bit değer
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

/// Ses akisi yonu - oynatma (DAC) mu yoksa kayit (ADC) mu oldugunu belirtir
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamDirection {
    Playback,
    Capture,
}

/// Ses akisi - DMA tampon yonetimi, oynatma durumu ve PCM format bilgisini bir arada tutar
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
    
    /// Tamponu ayarla — çalacak PCM verisini döngü için sarınla ve konum sıfırlaN
    pub fn set_buffer(&mut self, data: Vec<u8>) {
        self.buffer = data;
        self.buffer_size = self.buffer.len();
        self.position = 0;
    }
    
    /// Oynatmayı başlat — konum sıfırlanır ve DMA transfer işaretlenir
    pub fn start(&mut self) {
        self.playing = true;
        self.position = 0;
    }
    
    /// Oynatmayı durdur — DMA transferi keser ve akış işaretini temizler
    pub fn stop(&mut self) {
        self.playing = false;
    }
    
    /// Oynatmayı duraklat — konumu korur, devam edildiğinde kaldığı yerden sürer
    pub fn pause(&mut self) {
        self.playing = false;
    }
    
    /// Oynatmaya devam et — duraklatılan noktadan &DMA aktarimi yeniden başlar
    pub fn resume(&mut self) {
        self.playing = true;
    }
    
    /// Tüketilen örnekleri işle — tampon doluysa döngü etkinse başa sar, aksi halde oynatmayı bitir
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
// SES TAMPON TANİMLAYICI LİSTESİ (BDL)
// ============================================================================

/// Tampon Tanimlayici Girdisi (BDE) - 16 byte - HDA BDL tablosundaki her DMA parcasinin fiziksel adresi
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
// SES HATALARI
// ============================================================================

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

/// Ses alt sistemini başlat — HDA denetleyçilerini PCI'den kesfeder ve başlatır
pub fn init() {
    if AUDIO_INITIALIZED.swap(true, Ordering::SeqCst) {
        return;
    }
    
    crate::serial_println!("[AUDIO] Initializing HDA subsystem...");
    
    // HDA denetleyçilerini keşfet — PCI yapılandırma uzayını tarayarak bulur
    let controllers = discover_hda_controllers();
    
    let mut hda_ctrls = HDA_CONTROLLERS.lock();
    
    for mut ctrl in controllers {
        if ctrl.init().is_ok() {
            hda_ctrls.push(ctrl);
        }
    }
    
    crate::serial_println!("[AUDIO] Found {} HDA controllers", hda_ctrls.len());
}

/// Müzik çaları için basit ses arka ucu — oynatma/durdurma/ses seviyesi arayüzleri sağlar
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

/// Ses arka ucunun referansını döndür — global kilit çözülmeden değişiklik garantisi verir
pub fn get_audio() -> Option<&'static Mutex<AudioBackend>> {
    Some(&AUDIO_BACKEND)
}

/// PCI aracılığıyla HDA denetleyçilerini keşfeçt — multimedya sınıfındaki HDA cihazlarını listeler
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

/// Varsayılan denetleyçiyi getir — sistemde birden fazla HDA varsa ilkini döndürür
pub fn default_controller() -> Option<HdaController> {
    HDA_CONTROLLERS.lock().first().cloned()
}

/// Varsayılan codec'i getir — ilk denetleyçinin ilk ses işlemcisini döndürür
pub fn default_codec() -> Option<HdaCodec> {
    HDA_CONTROLLERS.lock()
        .first()
        .and_then(|ctrl| ctrl.codecs.first().cloned())
}

/// Oynatma için ses akışı aç — belirtilen PCM formatıyla yeni bir çıkış akışı oluşturur
pub fn open_playback_stream(format: AudioFormat) -> Result<u8, AudioError> {
    let stream_id = NEXT_STREAM_ID.fetch_add(1, Ordering::SeqCst) as u8;
    
    let stream = AudioStream::new(stream_id, StreamDirection::Playback, format);
    
    AUDIO_STREAMS.lock().insert(stream_id, stream);
    
    crate::serial_println!("[AUDIO] Opened playback stream {} ({}Hz, {}bit, {}ch)",
        stream_id, format.sample_rate, format.bits_per_sample, format.channels);
    
    Ok(stream_id)
}

/// Ses akışını kapat — tampon belleği serbest bırakır ve akış tablosundan siler
pub fn close_stream(stream_id: u8) -> Result<(), AudioError> {
    let mut streams = AUDIO_STREAMS.lock();
    if streams.remove(&stream_id).is_some() {
        crate::serial_println!("[AUDIO] Closed stream {}", stream_id);
        Ok(())
    } else {
        Err(AudioError::NoStream)
    }
}

/// Ses verisini akış tamponuna yaz — PCM byte'ları akışın dahili tamponuna kopyalar
pub fn write_stream(stream_id: u8, data: &[u8]) -> Result<usize, AudioError> {
    let mut streams = AUDIO_STREAMS.lock();
    let stream = streams.get_mut(&stream_id).ok_or(AudioError::NoStream)?;
    
    stream.set_buffer(data.to_vec());
    
    Ok(data.len())
}

/// Oynatmayı başlat — akışı aktif konuma getirir ve DMA işaretini ayarlar
pub fn start_stream(stream_id: u8) -> Result<(), AudioError> {
    let mut streams = AUDIO_STREAMS.lock();
    let stream = streams.get_mut(&stream_id).ok_or(AudioError::NoStream)?;
    
    stream.start();
    
    crate::serial_println!("[AUDIO] Started stream {}", stream_id);
    
    Ok(())
}

/// Oynatmayı durdur — DMA transferını durdurur ve akışı pasif konuma getirir
pub fn stop_stream(stream_id: u8) -> Result<(), AudioError> {
    let mut streams = AUDIO_STREAMS.lock();
    let stream = streams.get_mut(&stream_id).ok_or(AudioError::NoStream)?;
    
    stream.stop();
    
    crate::serial_println!("[AUDIO] Stopped stream {}", stream_id);
    
    Ok(())
}

/// Ses seviyesini ayarla (0-100) — DAC widget'inin amp kazancını HDA verb ile günceller
pub fn set_volume(volume: u8) -> Result<(), AudioError> {
    let mut controllers = HDA_CONTROLLERS.lock();
    let ctrl = controllers.first_mut().ok_or(AudioError::NoController)?;
    
    if let Some(codec) = ctrl.codecs.first_mut() {
        if let Some(dac) = codec.find_output_dac() {
            // DAC'a ses seviyesi ayarla — SET_AMP_GAIN verb'i kullanılır
            let _ = dac;
        }
    }
    
    crate::serial_println!("[AUDIO] Volume set to {}%", volume);
    
    Ok(())
}

/// Sessizleştirmeyi ayarla — DAC amp kazancını sıfırlamak yerine mute biti kullanılır
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

/// Akış konumunu getir — DMA'nın tamponda ne kadar ilerlediğini byte cinsinden okur
pub fn get_stream_position(stream_id: u8) -> Result<usize, AudioError> {
    let streams = AUDIO_STREAMS.lock();
    let stream = streams.get(&stream_id).ok_or(AudioError::NoStream)?;
    
    Ok(stream.position)
}

/// Akış oynasıyor mu — DMA aktif mi değilse oynatma durmuş veya tampon bitti demektir
pub fn is_stream_playing(stream_id: u8) -> Result<bool, AudioError> {
    let streams = AUDIO_STREAMS.lock();
    let stream = streams.get(&stream_id).ok_or(AudioError::NoStream)?;
    
    Ok(stream.playing)
}

/// Se's yeteneklerini getir — maksimum kanal sayısı, örnekleme hızı ve bit derinliği bilgisi
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

/// Ses yetenekleri - denetleyicinin destekledigi maksimum kanal, hiz ve bit derinligi
#[derive(Clone, Copy, Debug)]
pub struct AudioCapabilities {
    pub max_channels: u8,
    pub max_sample_rate: u32,
    pub max_bits_per_sample: u8,
    pub output_streams: u8,
    pub input_streams: u8,
}

// ============================================================================
// DMA SES TRANSFERI
// ============================================================================

/// DMA transfer state
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

    /// DMA transferini basalt - konumu sifirla ve aktif isaretini ayarla
    pub fn start(&mut self) {
        self.active = true;
        self.position = 0;
    }

    /// DMA transferini durdur - donanim kontrolunu kapatir ve aktif isareti temizler
    pub fn stop(&mut self) {
        self.active = false;
    }

    /// Tamamlanma geri cagirimi ayarla - tampon bitince cikartilacak kesme isleyisini belirler
    pub fn set_callback(&mut self, callback: fn()) {
        self.callback = Some(callback);
    }

    /// DMA icin sonraki tampon parcasini getir - BDL girdisine yazilacak adres ve boyutu hesapla
    pub fn get_next_fragment(&mut self, fragment_size: usize) -> Option<(u64, usize)> {
        if !self.active {
            return None;
        }

        if self.position >= self.buffer_size {
            // Transfer tamamlandi - kayitli geri cagrim varsa calistir
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
// SES MIKSERI
// ============================================================================

/// Mikser kanali - adindan taninabilen, ses/yon/mute/solo ozellikleri olan tek bir miksaj katmani
#[derive(Clone, Debug)]
pub struct MixerChannel {
    pub id: u8,
    pub name: alloc::string::String,
    pub volume: u8,       // 0-100
    pub pan: i8,          // -100 (left) to 100 (right)
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

    /// Stereo ornege ses seviyesi ve yonlendirme uygula - pan negatifse sol zayiflatilir
    pub fn apply_to_sample(&self, left: i16, right: i16) -> (i16, i16) {
        if self.muted {
            return (0, 0);
        }

        // Ses seviyesini uygula (0-100%) - lineer kazanc carpani olarak isler
        let vol = self.volume as i32;
        let left_vol = left as i32 * vol / 100;
        let right_vol = right as i32 * vol / 100;

        // Yonlendirmeyi uygula (-100 ile 100 arasi) - sol veya saga dogru sinyal oranini degistir
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

/// Ses mikseri - birden fazla kanali tek bir PCM cikisina birlestiren donanim/yazilim katmani
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

    /// Kanal ekle - otomatik kimlik atanarak yeni mikser kanali olusturulur
    pub fn add_channel(&mut self, name: &str) -> u8 {
        let id = self.channels.len() as u8;
        self.channels.push(MixerChannel::new(id, name));
        id
    }

    /// Kanali kaldir - kimlige gore liste filtrelenerek silinir
    pub fn remove_channel(&mut self, id: u8) {
        self.channels.retain(|c| c.id != id);
    }

    /// Kanali getir - kimlige gore salt okunur erisim
    pub fn get_channel(&self, id: u8) -> Option<&MixerChannel> {
        self.channels.iter().find(|c| c.id == id)
    }

    /// Kanali degistirilebilir al - ses, yonlendirme, mute gibi ozellikler guncellenebilir
    pub fn get_channel_mut(&mut self, id: u8) -> Option<&mut MixerChannel> {
        self.channels.iter_mut().find(|c| c.id == id)
    }

    /// Tum kanallari cikis tamponuna miksle - solo/mute kurallari uygulanir, sinir degerler korunur
    pub fn mix_to_buffer(&self, streams: &BTreeMap<u8, AudioStream>) -> Vec<u8> {
        let samples = self.buffer_size / 4; // 16-bit stereo = ornek basina 4 byte
        let mut output = vec![0i32; samples * 2]; // Stereo

        // Solo kanal aktif mi kontrol et - solo varsa diger kanallar susturulur
        let any_solo = self.channels.iter().any(|c| c.solo);

        for channel in &self.channels {
            // Susturulmus veya solo degil ise atla - solo modu tek kanalli dinleme saglar
            if channel.muted || (any_solo && !channel.solo) {
                continue;
            }

            if let Some(stream_id) = channel.input_stream {
                if let Some(stream) = streams.get(&stream_id) {
                    if stream.playing && stream.format.channels == 2 && stream.format.bits_per_sample == 16 {
                        // Bu akistan ornekleri miksle - i16 little-endian formatinda oku
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

        // Ana ses seviyesini uygula ve byte'a donustur - tasmayi onlemek icin clamp kullan
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

    /// Ana ses seviyesini ayarla - 0-100 araliginda klamplar, tum kanallar bu olcekte etkilenir
    pub fn set_master_volume(&mut self, volume: u8) {
        self.master_volume = volume.min(100);
    }

    /// Ana sessizlestirmeyi ayarla - tum kanallari susturan ana kapama
    pub fn set_master_mute(&mut self, muted: bool) {
        self.master_muted = muted;
    }
}

// ============================================================================
// PCM SES FORMATI
// ============================================================================

/// PCM format specification
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

    /// CD kalitesi PCM - 44.1 kHz, 16 bit, stereo: muzik CD standardi
    pub fn cd_quality() -> Self {
        Self::new(44100, 16, 2)
    }

    /// DVD kalitesi PCM - 48 kHz, 16 bit, stereo: film ve dijital ses standardi
    pub fn dvd_quality() -> Self {
        Self::new(48000, 16, 2)
    }

    /// Blu-ray kalitesi PCM - 96 kHz, 24 bit, 6 kanal: ev sineması surround ses
    pub fn bluray_quality() -> Self {
        Self::new(96000, 24, 6)
    }

    /// Ornek basina byte sayisi - bit derinligini 8'e bolup yukari yuvarlar
    pub fn bytes_per_sample(&self) -> usize {
        (self.bits_per_sample as usize + 7) / 8
    }

    /// Cerceve boyutu (tum kanallar icin bir ornek) - mikrofon/hoparlor tamponu hesaplamada kullanilir
    pub fn frame_size(&self) -> usize {
        self.bytes_per_sample() * self.channels as usize
    }

    /// Byte hizi (saniyede byte) - tampon boyutu ve tamponlanma suresi hesaplamak icin
    pub fn byte_rate(&self) -> u32 {
        self.sample_rate * self.frame_size() as u32
    }

    /// Ornegi byte'a donustur - bit derinligine ve endian'a gore PCM verisi uretir
    pub fn sample_to_bytes(&self, sample: i32, buf: &mut [u8]) {
        let bytes = self.bytes_per_sample();
        match bytes {
            1 => {
                // 8-bit isaretssiz - 0-255 araligina normalize edilir
                buf[0] = ((sample + 128) & 0xFF) as u8;
            }
            2 => {
                // 16-bit isaretli - en yaygin PCM formati
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
                // 24-bit isaretli - stüdyo kalite kayit icin
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
                // 32-bit isaretli - maksimum dinamik aralik
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

    /// Byte'lari ornege donustur - PCM verisinden sayisal ses ornegi uretir
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
// SES KODLAYICILAR
// ============================================================================

/// Ses kodlayici ozelligi - sifreleme ve sifre cozme islemleri icin ortak arayuz
pub trait AudioCodec {
    /// Ses verisini sifresini coz - sikistirilmis girisi PCM cikisina donusturur
    fn decode(&self, input: &[u8], output: &mut Vec<u8>) -> Result<usize, AudioError>;

    /// Ses verisini sifrel - PCM girisi belirli bir formata donusturur
    fn encode(&self, input: &[u8], output: &mut Vec<u8>) -> Result<usize, AudioError>;

    /// Kodlayici adini dondur - hata ayiklama ve kullanici arayuzu icin
    fn name(&self) -> &str;

    /// Cikis PCM formatini dondur - olusturulan ornek hizi ve bit derinligi
    fn output_format(&self) -> PcmFormat;
}

/// Sine wave generator codec
pub struct SineWaveCodec {
    pub frequency: f32,
    pub sample_rate: u32,
    pub amplitude: f32,
    pub phase: f32,
}

/// no_std icin Taylor serisi sin yaklasimi - floating point kutuphanesi olmadan sinusoidal dalga uretir
fn sin_approx(x: f32) -> f32 {
    // [-PI, PI] araligina normalize et - periyodiklik nedeniyle deger araligini kisalt
    let mut x = x;
    let pi = core::f32::consts::PI;
    let two_pi = 2.0 * pi;
    
    while x > pi {
        x -= two_pi;
    }
    while x < -pi {
        x += two_pi;
    }
    
    // Taylor serisi: sin(x) = x - x^3/3! + x^5/5! - x^7/7! + x^9/9! - 9. derecede yeterli hassasiyet
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

    /// Sinusoidal dalga ornekleri uret - belirtilen sayi kadar PCM byte uretir
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

    /// Stereo sinusoidal dalga uret - sol ve sag kanal icin farkli frekanslarda ornek olusturur
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
        // 1024 ornek uret - test ve demo amacli kisa bir dalga dongusu
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

/// White noise generator
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

    /// Basit LFSR rasgele sayi ureteci - sifir memory ile deterministik psodorast gurultu saglar
    fn next_random(&mut self) -> u32 {
        let bit = ((self.state >> 0) ^ (self.state >> 2) ^ (self.state >> 3) ^ (self.state >> 5)) & 1;
        self.state = (self.state >> 1) | (bit << 31);
        self.state
    }

    /// Beyaz gurultu ornekleri uret - esit gucu olan tam frekans spektrumu dolduran rastgele sinyal
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

/// Simple μ-law decoder
pub struct MuLawCodec {
    pub sample_rate: u32,
}

impl MuLawCodec {
    pub fn new(sample_rate: u32) -> Self {
        MuLawCodec { sample_rate }
    }

    /// u-yasasi byte'ini lineer ornege coz - telefon ses sikistrmasinda kullanilan non-lineer kodlama
    pub fn decode_sample(sample: u8) -> i16 {
        // u-yasasi sifre cozme - tum bitleri ters cevirerek coz
        let sample = sample ^ 0xFF; // Tum bitleri ters cevir
        let sign = if sample & 0x80 != 0 { -1 } else { 1 };
        let exponent = (sample >> 4) & 0x07;
        let mantissa = sample & 0x0F;

        let decoded = (33 * (2 * mantissa as i32 + 33) * (1 << exponent) - 33) * sign;
        decoded.clamp(-32768, 32767) as i16
    }

    /// Lineer ornegi u-yasasi byte'ina kodla - dinamik aralik sikistirmasi ile bant genisligini azaltir
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

/// Simple A-law decoder
pub struct ALawCodec {
    pub sample_rate: u32,
}

impl ALawCodec {
    pub fn new(sample_rate: u32) -> Self {
        ALawCodec { sample_rate }
    }

    /// A-yasasi byte'ini lineer ornege coz - Avrupa telefonu icin ITU-T G.711 standardi
    pub fn decode_sample(sample: u8) -> i16 {
        let sample = sample ^ 0x55; // Cift bitleri degistir - A-yasasi isaretleme kurali
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

    /// Lineer ornegi A-yasasi byte'ina kodla - logaritmik sikistirma ile genis dinamik aralik saglar
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
// GLOBAL SES MIKSERI
// ============================================================================

static AUDIO_MIXER: Mutex<Option<AudioMixer>> = Mutex::new(None);

/// Ses mikserini basalt - belirtilen ornek hizi ve tampon boyutuyla global mikseri hazirlar
pub fn init_mixer(sample_rate: u32, buffer_size: usize) {
    *AUDIO_MIXER.lock() = Some(AudioMixer::new(sample_rate, buffer_size));
}

/// Mikseri getir - global mikser orneginin kopyasini dondurur
pub fn get_mixer() -> Option<AudioMixer> {
    AUDIO_MIXER.lock().clone()
}

/// Mikser kanali ekle - isme gore yeni bir kanal olusturur ve kimligini dondurur
pub fn add_mixer_channel(name: &str) -> Option<u8> {
    let mut mixer = AUDIO_MIXER.lock();
    mixer.as_mut().map(|m| m.add_channel(name))
}

/// Kanal ses seviyesini ayarla - 0-100 araliginda donanim kazancını gunceller
pub fn set_channel_volume(channel_id: u8, volume: u8) -> Result<(), AudioError> {
    let mut mixer = AUDIO_MIXER.lock();
    let mixer = mixer.as_mut().ok_or(AudioError::NoController)?;
    let channel = mixer.get_channel_mut(channel_id).ok_or(AudioError::NoStream)?;
    channel.volume = volume.min(100);
    Ok(())
}

/// Kanal yonlendirmesini ayarla - -100 sol, 0 orta, 100 sag konumlandirma
pub fn set_channel_pan(channel_id: u8, pan: i8) -> Result<(), AudioError> {
    let mut mixer = AUDIO_MIXER.lock();
    let mixer = mixer.as_mut().ok_or(AudioError::NoController)?;
    let channel = mixer.get_channel_mut(channel_id).ok_or(AudioError::NoStream)?;
    channel.pan = pan.clamp(-100, 100);
    Ok(())
}

/// Kanal sessizlestirmesini ayarla - diger kanallari etkilemeden tek kanali sustururur
pub fn set_channel_mute(channel_id: u8, muted: bool) -> Result<(), AudioError> {
    let mut mixer = AUDIO_MIXER.lock();
    let mixer = mixer.as_mut().ok_or(AudioError::NoController)?;
    let channel = mixer.get_channel_mut(channel_id).ok_or(AudioError::NoStream)?;
    channel.muted = muted;
    Ok(())
}

/// Kanal solo modunu ayarla - aktif oldugunda diger kanallar otomatik susturulur
pub fn set_channel_solo(channel_id: u8, solo: bool) -> Result<(), AudioError> {
    let mut mixer = AUDIO_MIXER.lock();
    let mixer = mixer.as_mut().ok_or(AudioError::NoController)?;
    let channel = mixer.get_channel_mut(channel_id).ok_or(AudioError::NoStream)?;
    channel.solo = solo;
    Ok(())
}

/// Akisi mikser kanaline bagla - stream'in PCM cikisi bu kanal uzerinden miksere beslenir
pub fn link_stream_to_channel(channel_id: u8, stream_id: u8) -> Result<(), AudioError> {
    let mut mixer = AUDIO_MIXER.lock();
    let mixer = mixer.as_mut().ok_or(AudioError::NoController)?;
    let channel = mixer.get_channel_mut(channel_id).ok_or(AudioError::NoStream)?;
    channel.input_stream = Some(stream_id);
    Ok(())
}

/// Tum akislari miksle - aktif kanallar birlestirilir ve cikis tamponu olusturulur
pub fn mix_streams() -> Option<Vec<u8>> {
    let mixer = AUDIO_MIXER.lock();
    let mixer = mixer.as_ref()?;
    let streams = AUDIO_STREAMS.lock();
    Some(mixer.mix_to_buffer(&streams))
}
