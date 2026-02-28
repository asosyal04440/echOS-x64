//! # Müzik Çalar Uygulaması
//!
//! Çalarlist desteği, ses görselleştirme ve oynatma kontrolleri içeren tam özellikli
//! bir müzik çalar uygulaması. Ses arka ucu ile entegre çalışır.
//!
//! ## Mimari
//! - `AudioFormat`: Desteklenen ses formatı türleri (MP3, WAV, OGG, FLAC, AAC)
//! - `TrackInfo`: Parçaya ait meta veri (başlık, sanatçı, süre, vb.)
//! - `Playlist`: Parça listesi ve sıralama yöneticisi
//! - `AudioVisualizer`: Frekans çubuğu / dalga formu / daire görselleştirmesi
//! - `MusicPlayer`: Tüm bileşenleri bir araya getiren ana pencere yapısı

use alloc::boxed::Box;
use alloc::string::String;
use alloc::format;
use alloc::vec::Vec;
use alloc::vec;
use alloc::collections::VecDeque;
use spin::Mutex;
use core::f32::consts::PI;
use libm::{sinf, cosf};

use crate::gop::framebuffer::Framebuffer;
use crate::gui::theme::{Theme, Color};
use crate::gui::widgets::{Widget, Rect};

// ============================================================================
// SES FORMATI — AudioFormat
// ============================================================================

/// Desteklenen ses formatları.
///
/// Rust'ta `enum` ile farklı durum/tür alternatifleri tanımlanır.
/// `#[derive(...)]` otomatik olarak `Clone`, `Copy`, `Debug`,
/// `PartialEq` ve `Eq` trait'lerini üretir. Bu sayede
/// `format == AudioFormat::Mp3` gibi karşılaştırmalar derleme
/// zamanında güvenli biçimde yapılabilir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioFormat {
    Unknown,
    Mp3,
    Wav,
    Ogg,
    Flac,
    Aac,
}

impl AudioFormat {
    /// Dosya uzantısından ses formatını belirler.
    ///
    /// `match` ifadesi Rust'ta desen eşleme (pattern matching) için kullanılır.
    /// `to_lowercase()` büyük/küçük harf duyarsızlığını sağlar; örneğin
    /// "MP3", "mp3" ve "Mp3" aynı kola düşer.
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "mp3" => AudioFormat::Mp3,
            "wav" | "wave" => AudioFormat::Wav,
            "ogg" | "oga" => AudioFormat::Ogg,
            "flac" => AudioFormat::Flac,
            "aac" | "m4a" => AudioFormat::Aac,
            _ => AudioFormat::Unknown,
        }
    }

    /// Formata karşılık gelen dosya uzantısını döndürür.
    ///
    /// `&'static str` dönüş tipi, string diliminin program
    /// boyunca geçerli olduğunu (yani yığında veya statik
    /// bellekte yaşadığını) ifade eder.
    pub fn extension(&self) -> &'static str {
        match self {
            AudioFormat::Mp3 => "mp3",
            AudioFormat::Wav => "wav",
            AudioFormat::Ogg => "ogg",
            AudioFormat::Flac => "flac",
            AudioFormat::Aac => "aac",
            _ => "unknown",
        }
    }
}

// ============================================================================
// PARÇA BİLGİSİ — TrackInfo
// ============================================================================

/// Bir müzik parçasının meta verilerini tutan yapı.
///
/// Meta veri: dosya sistemi dışında, parçanın kendisine gömülü bilgilerdir
/// (ID3 tag, Vorbis comment, FLAC block, vb.).
/// `pub` alanlar doğrudan erişime açıktır; daha kısıtlı
/// kapsülleme (encapsulation) gerekirse `get/set` metodları yazılabilir.
#[derive(Clone, Debug)]
pub struct TrackInfo {
    /// Dosya yolu (tam veya göreceli)
    pub path: String,
    /// Parça başlığı
    pub title: String,
    /// Sanatçı adı
    pub artist: String,
    /// Albüm adı
    pub album: String,
    /// Süre (saniye cinsinden, kesirli)
    pub duration: f32,
    /// Örnekleme hızı — tipik değer: 44100 Hz (CD kalitesi)
    pub sample_rate: u32,
    /// Kanal sayısı — 1: mono, 2: stereo
    pub channels: u8,
    /// Bit hızı (kbps cinsinden)
    pub bitrate: u32,
    /// Ses formatı (MP3, WAV, vb.)
    pub format: AudioFormat,
    /// Albüm içindeki sıra numarası (bilinmiyorsa None)
    pub track_number: Option<u32>,
    /// Yayın yılı (bilinmiyorsa None)
    pub year: Option<u32>,
    /// Müzik türü (genre)
    pub genre: String,
    /// Albüm resmi var mı? (gerçek veri yerine yer tutucu bayrak)
    pub has_album_art: bool,
}

impl TrackInfo {
    /// Yeni bir `TrackInfo` nesnesini dosya yolundan oluşturur.
    ///
    /// `rsplit('/')` yolun son bileşenini (dosya adını) alır.
    /// `unwrap_or` ise hata durumunda güvenli bir varsayılan döndürür.
    /// "Bilinmiyor" varsayılanları gerçek uygulamada ID3/Vorbis
    /// parse kütüphanesiyle doldurulacaktır.
    pub fn new(path: &str) -> Self {
        let name = path.rsplit('/').next().unwrap_or(path);
        let title = name.rsplit('.').next().unwrap_or(name);

        TrackInfo {
            path: String::from(path),
            title: String::from(title),
            artist: String::from("Unknown Artist"),
            album: String::from("Unknown Album"),
            duration: 0.0,
            sample_rate: 44100,
            channels: 2,
            bitrate: 128,
            format: AudioFormat::from_extension(path.rsplit('.').next().unwrap_or("")),
            track_number: None,
            year: None,
            genre: String::from("Unknown"),
            has_album_art: false,
        }
    }

    /// Süreyi "dk:ss" biçiminde biçimlendirir.
    ///
    /// `{:02}` format belirteci saniyeleri iki basamaklı gösterir
    /// (örn. 5 saniye → "05"). Bu, müzik çalar arayüzlerinde
    /// standart zaman gösterimi biçimidir.
    pub fn format_duration(&self) -> String {
        let total_secs = self.duration as u32;
        let mins = total_secs / 60;
        let secs = total_secs % 60;
        format!("{}:{:02}", mins, secs)
    }
}

// ============================================================================
// OYNATMA DURUMU — PlaybackState / RepeatMode / ShuffleMode
// ============================================================================

/// Oynatma durumu: durdurulmuş, oynatılıyor veya duraklatılmış.
///
/// Durum makinesi (state machine) tasarım deseni: her an
/// yalnızca bir durum aktiftir. `match` ile tüm durumlar
/// derleme zamanında ele alınmak zorundadır; böylece
/// "durumu unutma" hataları önlenir.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

/// Tekrar modu: tekrar yok, tek parçayı tekrar et, tümünü döngüye al.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepeatMode {
    None,
    One,
    All,
}

/// Karıştırma modu: kapalı veya açık.
///
/// `#[default]` niteliği, `Default::default()` çağrısında
/// hangi varyantın kullanılacağını belirtir. Bu, `.unwrap_or_default()`
/// çağrılarında ve `..Default::default()` struct update söz
/// diziminde kullanışlıdır.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ShuffleMode {
    #[default]
    Off,
    On,
}

// ============================================================================
// ÇALARLIST — Playlist
// ============================================================================

/// Çalarlist yöneticisi: parça koleksiyonu ve oynatma sırası.
///
/// `play_order` alanı shuffle açıkken Fisher-Yates algoritmasıyla
/// yeniden oluşturulan karıştırılmış indeks dizisini tutar.
/// Shuffle kapalıyken dizin sırası doğrusal kalır (0,1,2,...).
pub struct Playlist {
    /// Parça koleksiyonu
    tracks: Vec<TrackInfo>,
    /// Şu anda çalınan parçanın indeksi (hiç seçilmediyse None)
    current_index: Option<usize>,
    /// Oynatma sırası (shuffle için karıştırılmış indeks dizisi)
    play_order: Vec<usize>,
    /// `play_order` içindeki mevcut konum
    order_position: usize,
    /// Karıştırma modu
    shuffle: ShuffleMode,
    /// Tekrar modu
    repeat: RepeatMode,
    /// Çalarlist adı
    name: String,
}

impl Playlist {
    pub fn new(name: &str) -> Self {
        Playlist {
            tracks: Vec::new(),
            current_index: None,
            play_order: Vec::new(),
            order_position: 0,
            shuffle: ShuffleMode::Off,
            repeat: RepeatMode::None,
            name: String::from(name),
        }
    }

    /// Çalarliste yeni parça ekler ve oynatma sırasını yeniden oluşturur.
    pub fn add_track(&mut self, track: TrackInfo) {
        self.tracks.push(track);
        self.rebuild_play_order();
    }

    /// Belirtilen indeksteki parçayı çalarlisteden kaldırır.
    /// Kaldırma işleminin ardından `current_index` güncellenir
    /// böylece üstteki bir parça silindiğinde işaretçi kayması önlenir.
    pub fn remove_track(&mut self, index: usize) {
        if index < self.tracks.len() {
            self.tracks.remove(index);
            self.rebuild_play_order();

            if let Some(current) = self.current_index {
                if current >= index && current > 0 {
                    self.current_index = Some(current - 1);
                }
            }
        }
    }

    /// Çalarlisti tamamen boşaltır ve tüm durumu sıfırlar.
    pub fn clear(&mut self) {
        self.tracks.clear();
        self.current_index = None;
        self.play_order.clear();
        self.order_position = 0;
    }

    /// Çalarlistteki toplam parça sayısını döndürür.
    pub fn count(&self) -> usize {
        self.tracks.len()
    }

    /// Şu an çalan parçayı döndürür; parça yoksa `None`.
    pub fn current(&self) -> Option<&TrackInfo> {
        self.current_index.and_then(|i| self.tracks.get(i))
    }

    /// Belirtilen indeksteki parçaya referans döndürür.
    pub fn get(&self, index: usize) -> Option<&TrackInfo> {
        self.tracks.get(index)
    }

    /// Mevcut parçayı belirtilen indekse ayarlar.
    ///
    /// Shuffle açıksa `play_order` içinde de konumu günceller;
    /// böylece sonraki/önceki parça navigasyonu tutarlı kalır.
    pub fn set_current(&mut self, index: usize) {
        if index < self.tracks.len() {
            self.current_index = Some(index);

            // Shuffle açıksa play_order içindeki konumu da güncelle
            if self.shuffle == ShuffleMode::On {
                self.order_position = self.play_order.iter().position(|&i| i == index).unwrap_or(0);
            }
        }
    }

    /// Sonraki parçaya geçer ve yeni indeksi döndürür.
    ///
    /// Tekrar modu "One" ise aynı parça yeniden oynatılır.
    /// Shuffle açıksa karıştırılmış sıradaki bir sonraki parça seçilir.
    /// Listenin sonuna gelindiğinde ve tekrar modu "All" ise
    /// başa döner, aksi hâlde `None` döner (liste bitti).
    pub fn next(&mut self) -> Option<usize> {
        match self.repeat {
            RepeatMode::One => self.current_index,
            _ => {
                if self.shuffle == ShuffleMode::On {
                    if self.order_position + 1 < self.play_order.len() {
                        self.order_position += 1;
                    } else if self.repeat == RepeatMode::All {
                        self.order_position = 0;
                    } else {
                        return None;
                    }
                    self.current_index = Some(self.play_order[self.order_position]);
                } else {
                    let next_index = self.current_index.map(|i| i + 1).unwrap_or(0);
                    if next_index < self.tracks.len() {
                        self.current_index = Some(next_index);
                    } else if self.repeat == RepeatMode::All {
                        self.current_index = Some(0);
                    } else {
                        return None;
                    }
                }
                self.current_index
            }
        }
    }

    /// Önceki parçaya geçer ve yeni indeksi döndürür.
    ///
    /// Shuffle açıksa karıştırılmış sıradaki bir önceki parça seçilir.
    /// `saturating_sub(1)` taşmayı (underflow) önler:
    /// 0 - 1 = 0 olur, paniklemez.
    pub fn prev(&mut self) -> Option<usize> {
        if self.shuffle == ShuffleMode::On {
            if self.order_position > 0 {
                self.order_position -= 1;
            } else if self.repeat == RepeatMode::All {
                self.order_position = self.play_order.len() - 1;
            } else {
                return None;
            }
            self.current_index = Some(self.play_order[self.order_position]);
        } else {
            let prev_index = self.current_index.map(|i| i.saturating_sub(1)).unwrap_or(0);
            self.current_index = Some(prev_index);
        }
        self.current_index
    }

    /// Karıştırma modunu değiştirir ve oynatma sırasını yeniden oluşturur.
    pub fn set_shuffle(&mut self, mode: ShuffleMode) {
        self.shuffle = mode;
        self.rebuild_play_order();
    }

    /// Tekrar modunu değiştirir.
    pub fn set_repeat(&mut self, mode: RepeatMode) {
        self.repeat = mode;
    }

    /// Oynatma sırasını yeniden oluşturur.
    ///
    /// Shuffle açıksa Fisher-Yates algoritması uygulanır:
    /// en sondan başa doğru her eleman rastgele bir konumla yer değiştirir.
    /// Bu, tüm permütasyonlara eşit olasılık tanır (unbiased shuffle).
    fn rebuild_play_order(&mut self) {
        self.play_order = (0..self.tracks.len()).collect();

        if self.shuffle == ShuffleMode::On {
            // Fisher-Yates shuffle algoritması: O(n) karmaşıklıkla tüm sıralamalara eşit olasılık
            for i in (1..self.play_order.len()).rev() {
                let j = (self.get_random() as usize) % (i + 1);
                self.play_order.swap(i, j);
            }
        }

        // Mevcut parçanın play_order içindeki konumunu yeniden hesapla
        if let Some(current) = self.current_index {
            self.order_position = self.play_order.iter().position(|&i| i == current).unwrap_or(0);
        }
    }

    /// Basit rastgele sayı üreteci: TSC (Time Stamp Counter) kullanır.
    ///
    /// Gerçek bir uygulamada CSPRNG veya PRNG kullanılmalıdır.
    /// TSC, CPU'nun çevrim sayacını okur; her çağrıda farklı değer verir.
    fn get_random(&self) -> u64 {
        // Basit rastgelelik — gerçek RNG ile değiştirilmeli
        crate::cpu::tsc::read() as u64
    }
}

// ============================================================================
// SES GÖRSELLEŞTİRİCİ — AudioVisualizer
// ============================================================================

/// Ses görselleştiricisi: çubuk, dalga formu ve daire modlarını destekler.
///
/// Gerçek bir FFT yerine basit bant ortalaması kullanılır.
/// Hafıza kısıtlı `no_std` ortamında ağır FFT kütüphaneleri
/// kullanılamayabileceğinden bu yaklaşım tercih edilmiştir.
/// Pik tutucular (`peaks`, `peak_hold`) gösterimi daha etkileyici yapar.
pub struct AudioVisualizer {
    /// Her frekans bandı için normalize edilmiş genlik değerleri (0.0 – 1.0)
    bars: Vec<f32>,
    /// Görüntülenecek çubuk sayısı
    bar_count: usize,
    /// Yumuşatma için geçmiş veri (şu an kullanılmıyor; gelecek sürüm için ayrıldı)
    history: Vec<Vec<f32>>,
    /// Üstel hareketli ortalama katsayısı — 0: anlık, 1: donuk görüntü
    smoothing: f32,
    /// Her bandın en yüksek gördüğü değerin tutulduğu pik listesi
    peaks: Vec<f32>,
    /// Her pik için geri sayım sayacı (kare cinsinden tutma süresi)
    peak_hold: Vec<u32>,
    /// Aktif görselleştirme modu
    mode: VisualizationMode,
}

/// Görselleştirme modu seçenekleri.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisualizationMode {
    /// Dikey çubuk spektrum analiz görüntüsü
    Bars,
    /// Yatay dalga formu görüntüsü
    Waveform,
    /// Radyal daire görüntüsü
    Circle,
}

impl AudioVisualizer {
    pub fn new(bar_count: usize) -> Self {
        AudioVisualizer {
            bars: vec![0.0; bar_count],
            bar_count,
            history: Vec::new(),
            smoothing: 0.7,
            peaks: vec![0.0; bar_count],
            peak_hold: vec![0; bar_count],
            mode: VisualizationMode::Bars,
        }
    }

    /// Gerçek ses verisiyle görselleştiriciyi günceller.
    ///
    /// Basit bant analizi: ses örnekleri eşit sayıda gruba bölünür,
    /// her grubun ortalama mutlak değeri o bandın genliğini verir.
    /// Ardından üstel yumuşatma uygulanır: `new = old * s + value * (1-s)`
    pub fn update(&mut self, audio_data: &[f32]) {
        // Basit FFT yaklaşımı: örnekleri bantlara böl
        let samples_per_bar = audio_data.len() / self.bar_count;

        for i in 0..self.bar_count {
            let start = i * samples_per_bar;
            let end = (start + samples_per_bar).min(audio_data.len());

            let mut sum = 0.0;
            for j in start..end {
                sum += audio_data[j].abs();
            }

            let avg = sum / (end - start).max(1) as f32;

            // Üstel hareketli ortalama ile yumuşat
            self.bars[i] = self.bars[i] * self.smoothing + avg * (1.0 - self.smoothing);

            // Pik değeri güncelle: yeni pik gelirse tut; zamanla söndür
            if self.bars[i] > self.peaks[i] {
                self.peaks[i] = self.bars[i];
                self.peak_hold[i] = 30;
            } else if self.peak_hold[i] > 0 {
                self.peak_hold[i] -= 1;
            } else {
                self.peaks[i] *= 0.95;
            }
        }
    }

    /// Gerçek ses yokken sahte görselleştirme verisi üretir.
    ///
    /// Sinüs dalgaları kullanılarak animasyonlu bir efekt oluşturulur.
    /// `libm::sin` `no_std` ortamında matematiksel fonksiyon sağlar
    /// (standart kütüphane float `sin` işlevi `std` gerektirir).
    pub fn generate_dummy(&mut self, time: f64) {
        for i in 0..self.bar_count {
            let freq = (i + 1) as f64 * 0.5;
            let phase = time * 2.0;
            let value = (0.5 + 0.5 * libm::sin(freq * phase) * (1.0 - i as f64 / self.bar_count as f64)) as f32;

            self.bars[i] = self.bars[i] * self.smoothing + value * (1.0 - self.smoothing);

            if self.bars[i] > self.peaks[i] {
                self.peaks[i] = self.bars[i];
                self.peak_hold[i] = 30;
            } else if self.peak_hold[i] > 0 {
                self.peak_hold[i] -= 1;
            } else {
                self.peaks[i] *= 0.95;
            }
        }
    }

    /// Aktif moda göre görselleştirmeyi çizer.
    pub fn draw(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize) {
        match self.mode {
            VisualizationMode::Bars => self.draw_bars(fb, x, y, width, height),
            VisualizationMode::Waveform => self.draw_waveform(fb, x, y, width, height),
            VisualizationMode::Circle => self.draw_circle(fb, x, y, width, height),
        }
    }

    /// Frekans çubuklarını dikey olarak çizer.
    /// Her çubuğun üstüne pik noktası eklenir (aksan rengiyle).
    fn draw_bars(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize) {
        let bar_width = width / self.bar_count;
        let bar_gap = 2;

        for i in 0..self.bar_count {
            let bar_x = x + i * bar_width;
            let bar_height = (self.bars[i] * height as f32 * 0.9) as usize;
            let bar_y = y + height - bar_height;

            // Çubuğu çiz
            let color = self.get_bar_color(i, self.bars[i]);
            fb.draw_rect(bar_x, bar_y, bar_width - bar_gap, bar_height, color);

            // Pik çizgisini çiz (değer anlamlıysa)
            if self.peaks[i] > 0.01 {
                let peak_y = y + height - (self.peaks[i] * height as f32 * 0.9) as usize;
                fb.draw_rect(bar_x, peak_y, bar_width - bar_gap, 2, Theme::ACCENT_PRIMARY.to_u32());
            }
        }
    }

    /// Yatay dalga formu görselleştirmesini çizer.
    /// Merkez çizgisinden yukarı ve aşağı uzanan dikey çizgiler oluşturur.
    fn draw_waveform(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize) {
        let center_y = y + height / 2;
        let half_height = height / 2;

        for i in 0..width {
            let bar_idx = (i * self.bar_count / width).min(self.bar_count - 1);
            let amplitude = (self.bars[bar_idx] * half_height as f32) as usize;

            // Merkez çizgisinden yukarı ve aşağı uzat
            let color = self.get_bar_color(bar_idx, self.bars[bar_idx]);
            fb.draw_rect(x + i, center_y - amplitude, 1, amplitude * 2, color);
        }
    }

    /// Radyal daire görselleştirmesini çizer.
    ///
    /// Her bant için çemberin çevresinden dışarıya doğru bir çizgi çizilir.
    /// `sinf` / `cosf` trigonometrik fonksiyonları açıyı koordinata çevirir.
    /// Bresenham benzeri çizgi algoritması piksel ara noktalarını lineer
    /// interpolasyonla hesaplar.
    fn draw_circle(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize) {
        let center_x = x + width / 2;
        let center_y = y + height / 2;
        let radius = (width.min(height) / 3) as f32;

        for i in 0..self.bar_count {
            let angle = i as f32 * 2.0 * PI / self.bar_count as f32;
            let amplitude = self.bars[i] * radius * 0.5;

            let inner_x = (center_x as f32 + cosf(angle) * radius) as usize;
            let inner_y = (center_y as f32 + sinf(angle) * radius) as usize;
            let outer_x = (center_x as f32 + cosf(angle) * (radius + amplitude)) as usize;
            let outer_y = (center_y as f32 + sinf(angle) * (radius + amplitude)) as usize;

            let color = self.get_bar_color(i, self.bars[i]);

            // İçten dışa doğru çizgi çiz (lineer interpolasyon)
            let dx = outer_x as i32 - inner_x as i32;
            let dy = outer_y as i32 - inner_y as i32;
            let steps = (dx.abs().max(dy.abs()).max(1)) as usize;

            for j in 0..steps {
                let t = j as f32 / steps as f32;
                let px = (inner_x as f32 + dx as f32 * t) as usize;
                let py = (inner_y as f32 + dy as f32 * t) as usize;

                if px < x + width && py < y + height {
                    fb.plot_pixel(px, py, color);
                }
            }
        }
    }

    /// HSL renk uzayından RGB renk kodu üretir.
    ///
    /// Ton (hue) frekans bandı indeksine göre 0–360 arasında değişir;
    /// bu sayede her çubuk farklı bir renk alır (gökkuşağı efekti).
    /// Parlaklık (lightness) genliğe göre dinamik olarak ayarlanır.
    fn get_bar_color(&self, index: usize, value: f32) -> u32 {
        let hue = (index as f32 / self.bar_count as f32 * 360.0) as u16;
        let saturation = 0.8;
        let lightness = 0.5 + value * 0.3;

        // HSL → RGB dönüşümü (basitleştirilmiş)
        let c = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
        let x = c * (1.0 - ((hue as f32 / 60.0) % 2.0 - 1.0).abs());
        let m = lightness - c / 2.0;

        let (r, g, b) = match hue {
            0..=59 => (c, x, 0.0),
            60..=119 => (x, c, 0.0),
            120..=179 => (0.0, c, x),
            180..=239 => (0.0, x, c),
            240..=299 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };

        let r = ((r + m) * 255.0) as u32;
        let g = ((g + m) * 255.0) as u32;
        let b = ((b + m) * 255.0) as u32;

        (r << 16) | (g << 8) | b
    }

    /// Görselleştirme modunu değiştirir.
    pub fn set_mode(&mut self, mode: VisualizationMode) {
        self.mode = mode;
    }
}

// ============================================================================
// MÜZİK ÇALAR — MusicPlayer
// ============================================================================

/// Müzik Çalar ana yapısı.
///
/// Tüm bileşenleri (çalarlist, görselleştirici, ekolayzer bantları,
/// oynatma durumu) bir araya getirir. GUI çizimi ve fare/klavye
/// olayları bu yapı üzerinden yönetilir.
pub struct MusicPlayer {
    /// Pencerenin ekran konumu ve boyutu
    rect: Rect,
    /// Çalarlist yöneticisi
    playlist: Playlist,
    /// Şu anki oynatma durumu (durdurulmuş / oynatılıyor / duraklatılmış)
    state: PlaybackState,
    /// Geçerli parça içindeki konum (saniye cinsinden)
    position: f32,
    /// Ses seviyesi (0.0 – 1.0)
    volume: f32,
    /// Frekans görselleştiricisi
    visualizer: AudioVisualizer,
    /// Görselleştirici görünür mü?
    show_visualizer: bool,
    /// Çalarlist paneli açık mı?
    show_playlist: bool,
    /// Ekolayzer paneli açık mı?
    show_equalizer: bool,
    /// 10 bantlı ekolayzer kazanç değerleri (-12 dB – +12 dB)
    eq_bands: Vec<f32>,
    /// Animasyon zamanı (sahte görselleştirme için kullanılır)
    time: f64,
    /// Çalarlistte seçili (vurgulu) parça indeksi
    selected_index: Option<usize>,
    /// Çalarlist kaydırma ofseti (görünen ilk parcanın indeksi)
    scroll_offset: usize,
}

impl MusicPlayer {
    pub fn new() -> Self {
        MusicPlayer {
            rect: Rect::new(250, 100, 700, 500),
            playlist: Playlist::new("Now Playing"),
            state: PlaybackState::Stopped,
            position: 0.0,
            volume: 0.7,
            visualizer: AudioVisualizer::new(32),
            show_visualizer: true,
            show_playlist: true,
            show_equalizer: false,
            eq_bands: vec![0.0; 10],
            time: 0.0,
            selected_index: None,
            scroll_offset: 0,
        }
    }

    /// Bir ses dosyasını çalarliste ekler ve oynatmaya başlar.
    pub fn load_file(&mut self, path: &str) {
        let track = TrackInfo::new(path);
        self.playlist.add_track(track);
        self.playlist.set_current(self.playlist.count() - 1);
        self.play();
    }

    /// Mevcut parçayı oynatmaya başlar.
    ///
    /// Ses arka ucu (audio backend) `Option` ile sarıldığından
    /// `if let Some(audio)` kalıbıyla güvenli şekilde erişilir.
    /// Sürücü yoksa sessizce devam edilir (panic olmaz).
    pub fn play(&mut self) {
        self.state = PlaybackState::Playing;

        // Gerekirse ses arka ucunu başlat
        if let Some(audio) = crate::drivers::audio::get_audio() {
            let mut audio = audio.lock();
            if let Some(track) = self.playlist.current() {
                audio.play(&track.path);
            }
        }
    }

    /// Oynatmayı duraklatır (konum korunur).
    pub fn pause(&mut self) {
        self.state = PlaybackState::Paused;

        if let Some(audio) = crate::drivers::audio::get_audio() {
            audio.lock().pause();
        }
    }

    /// Oynatmayı durdurur ve konumu başa alır.
    pub fn stop(&mut self) {
        self.state = PlaybackState::Stopped;
        self.position = 0.0;

        if let Some(audio) = crate::drivers::audio::get_audio() {
            audio.lock().stop();
        }
    }

    /// Oynat / Duraklat arasında geçiş yapar.
    pub fn toggle_play(&mut self) {
        match self.state {
            PlaybackState::Playing => self.pause(),
            PlaybackState::Paused => self.play(),
            PlaybackState::Stopped => {
                if self.playlist.current().is_some() {
                    self.play();
                }
            }
        }
    }

    /// Bir sonraki parçaya geçer.
    pub fn next(&mut self) {
        if self.playlist.next().is_some() {
            self.position = 0.0;
            if self.state == PlaybackState::Playing {
                self.play();
            }
        }
    }

    /// Önceki parçaya geri döner.
    ///
    /// Parça 3 saniyeden fazla oynatıldıysa başa sar;
    /// aksi hâlde gerçekten önceki parçaya git. Bu davranış
    /// birçok müzik çalarda standart hâline gelmiştir.
    pub fn prev(&mut self) {
        if self.position > 3.0 {
            // 3 saniyeden uzun süre geçtiyse mevcut parçayı baştan başlat
            self.position = 0.0;
        } else if self.playlist.prev().is_some() {
            self.position = 0.0;
        }

        if self.state == PlaybackState::Playing {
            self.play();
        }
    }

    /// Ses seviyesini (0.0 – 1.0) ayarlar.
    ///
    /// `max(0.0).min(1.0)` değeri geçerli aralıkta tutar.
    /// Rust'ta bu zincirleme (method chaining) yaygın kullanılan bir kalıptır.
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.max(0.0).min(1.0);

        if let Some(audio) = crate::drivers::audio::get_audio() {
            audio.lock().set_volume(self.volume);
        }
    }

    /// Oynatma konumunu verilen saniyeye taşır.
    pub fn seek(&mut self, position: f32) {
        self.position = position.max(0.0);

        if let Some(audio) = crate::drivers::audio::get_audio() {
            audio.lock().seek(self.position);
        }
    }

    /// Her kare çağrılır; zamana bağlı durumları günceller.
    ///
    /// `dt` (delta time): bir önceki kareden bu kareye geçen süre (saniye).
    /// Bu değer kareye bağımsız animasyon ve ilerleme hesaplaması için gereklidir.
    pub fn update(&mut self, dt: f64) {
        if self.state == PlaybackState::Playing {
            self.position += dt as f32;
            self.time += dt;

            // Parça bitişini kontrol et
            if let Some(track) = self.playlist.current() {
                if self.position >= track.duration && track.duration > 0.0 {
                    self.next();
                }
            }

            // Görselleştiriciyi güncelle (sahte veri ile)
            self.visualizer.generate_dummy(self.time);
        }
    }

    /// Müzik çalar penceresini çizer.
    ///
    /// Katmanlar şu sıraya göre çizilir:
    /// 1. Pencere arka planı
    /// 2. Başlık çubuğu
    /// 3. Parça bilgi alanı (kapak sanatı + meta veri)
    /// 4. İlerleme çubuğu
    /// 5. Kontrol düğmeleri + ses seviyesi
    /// 6. Görselleştirici (isteğe bağlı)
    /// 7. Çalarlist (isteğe bağlı)
    pub fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let width = self.rect.width as usize;
        let height = self.rect.height as usize;

        // Pencere arka planı
        fb.draw_rect(x, y, width, height, Theme::WINDOW_BG.to_u32());

        // Başlık çubuğu
        fb.draw_rect(x, y, width, 32, Theme::TITLEBAR_BG.to_u32());
        fb.draw_string(x + 12, y + 8, "Music Player", Theme::TEXT_PRIMARY.to_u32());

        // Kapat düğmesi
        fb.draw_rect(x + width - 28, y + 4, 24, 24, Theme::ERROR.to_u32());
        fb.draw_string(x + width - 20, y + 8, "×", Theme::TEXT_ON_ACCENT.to_u32());

        // Parça bilgi alanı
        let info_y = y + 32;
        let info_height = 120;
        fb.draw_rect(x, info_y, width, info_height, Theme::SIDEBAR_BG.to_u32());

        if let Some(track) = self.playlist.current() {
            // Albüm resmi yer tutucu
            let art_size = 80;
            let art_x = x + 20;
            let art_y = info_y + 20;

            // Yer tutucu albüm resmi çiz (dama tahtası deseni)
            for py in 0..art_size {
                for px in 0..art_size {
                    let color = if (px / 10 + py / 10) % 2 == 0 {
                        Theme::ACCENT_PRIMARY.to_u32()
                    } else {
                        Theme::ACCENT_SUCCESS.to_u32()
                    };
                    fb.plot_pixel(art_x + px, art_y + py, color);
                }
            }

            // Parça bilgileri metni
            let text_x = art_x + art_size + 20;
            fb.draw_string(text_x, art_y, &track.title, Theme::TEXT_PRIMARY.to_u32());
            fb.draw_string(text_x, art_y + 20, &track.artist, Theme::TEXT_SECONDARY.to_u32());
            fb.draw_string(text_x, art_y + 40, &track.album, Theme::TEXT_SECONDARY.to_u32());
        } else {
            fb.draw_string(x + 20, info_y + 40, "No track playing", Theme::TEXT_SECONDARY.to_u32());
        }

        // İlerleme çubuğu alanı
        let progress_y = info_y + info_height;
        let progress_height = 40;
        fb.draw_rect(x, progress_y, width, progress_height, Theme::WINDOW_BG.to_u32());

        let track_duration = self.playlist.current().map(|t| t.duration).unwrap_or(1.0).max(1.0);
        let progress = self.position / track_duration;

        // İlerleme çubuğu arka planı ve dolgu
        let bar_x = x + 20;
        let bar_width = width - 40;
        let bar_y = progress_y + 10;
        let bar_height = 8;

        fb.draw_rect(bar_x, bar_y, bar_width, bar_height, Theme::BORDER.to_u32());
        fb.draw_rect(bar_x, bar_y, (bar_width as f32 * progress) as usize, bar_height, Theme::ACCENT_PRIMARY.to_u32());

        // Zaman etiketleri
        let current_time = self.format_time(self.position);
        let total_time = self.playlist.current().map(|t| t.format_duration()).unwrap_or_else(|| String::from("0:00"));

        fb.draw_string(bar_x, bar_y + 14, &current_time, Theme::TEXT_SECONDARY.to_u32());
        fb.draw_string(bar_x + bar_width - 30, bar_y + 14, &total_time, Theme::TEXT_SECONDARY.to_u32());

        // Kontrol düğmeleri alanı
        let controls_y = progress_y + progress_height;
        let controls_height = 60;
        fb.draw_rect(x, controls_y, width, controls_height, Theme::TOOLBAR_BG.to_u32());

        let center_x = x + width / 2;
        let center_y = controls_y + controls_height / 2;

        // Önceki parça düğmesi
        self.draw_control_button(fb, center_x - 80, center_y - 15, "◀◀");

        // Oynat / Duraklat düğmesi (büyük, aksan rengi)
        let play_icon = match self.state {
            PlaybackState::Playing => "❚❚",
            _ => "▶",
        };
        self.draw_control_button_large(fb, center_x - 15, center_y - 20, play_icon);

        // Sonraki parça düğmesi
        self.draw_control_button(fb, center_x + 50, center_y - 15, "▶▶");

        // Ses seviyesi göstergesi
        let vol_x = x + width - 100;
        fb.draw_string(vol_x, center_y - 8, "🔊", Theme::TEXT_PRIMARY.to_u32());

        // Ses seviyesi çubuğu
        let vol_bar_x = vol_x + 24;
        let vol_bar_width = 60;
        fb.draw_rect(vol_bar_x, center_y - 4, vol_bar_width, 8, Theme::BORDER.to_u32());
        fb.draw_rect(vol_bar_x, center_y - 4, (vol_bar_width as f32 * self.volume) as usize, 8, Theme::ACCENT_PRIMARY.to_u32());

        // Görselleştirici (kapalı arka plan + görsel)
        if self.show_visualizer {
            let viz_y = controls_y + controls_height;
            let viz_height = 100;
            fb.draw_rect(x, viz_y, width, viz_height, 0x0A0A0A);

            self.visualizer.draw(fb, x + 10, viz_y + 10, width - 20, viz_height - 20);
        }

        // Çalarlist paneli
        if self.show_playlist {
            let list_y = controls_y + controls_height + if self.show_visualizer { 100 } else { 0 };
            let list_height = height - (list_y - y);

            fb.draw_rect(x, list_y, width, list_height, Theme::WINDOW_BG.to_u32());

            // Çalarlist başlığı
            fb.draw_rect(x, list_y, width, 24, Theme::TOOLBAR_BG.to_u32());
            fb.draw_string(x + 12, list_y + 4, &format!("Playlist ({} tracks)", self.playlist.count()), Theme::TEXT_PRIMARY.to_u32());

            // Çalarlist öğeleri
            let item_height = 28;
            let visible_items = (list_height - 24) / item_height;

            for i in 0..visible_items.min(self.playlist.count()) {
                let item_idx = self.scroll_offset + i;
                if item_idx >= self.playlist.count() {
                    break;
                }

                let item_y = list_y + 24 + i * item_height;
                let is_current = Some(item_idx) == self.playlist.current_index;
                let is_selected = Some(item_idx) == self.selected_index;

                let bg = if is_current {
                    Theme::ACCENT_PRIMARY.to_u32()
                } else if is_selected {
                    Theme::LIST_ITEM_HOVER.to_u32()
                } else {
                    Theme::TRANSPARENT.to_u32()
                };

                fb.draw_rect(x, item_y, width, item_height, bg);

                if let Some(track) = self.playlist.get(item_idx) {
                    let text_color = if is_current { Theme::TEXT_ON_ACCENT.to_u32() } else { Theme::TEXT_PRIMARY.to_u32() };

                    // Parça numarası
                    fb.draw_string(x + 8, item_y + 6, &format!("{:2}", item_idx + 1), Theme::TEXT_SECONDARY.to_u32());

                    // Başlık (30 karakterden uzunsa kes)
                    let title = if track.title.len() > 30 {
                        format!("{}...", &track.title[..27])
                    } else {
                        track.title.clone()
                    };
                    fb.draw_string(x + 40, item_y + 6, &title, text_color);

                    // Süre
                    fb.draw_string(x + width - 60, item_y + 6, &track.format_duration(), Theme::TEXT_SECONDARY.to_u32());
                }
            }
        }
    }

    /// Küçük kontrol düğmesi çizer (önceki / sonraki).
    fn draw_control_button(&self, fb: &mut Framebuffer, x: usize, y: usize, icon: &str) {
        fb.draw_rect(x, y, 30, 30, Theme::BORDER.to_u32());
        fb.draw_string(x + 6, y + 6, icon, Theme::TEXT_PRIMARY.to_u32());
    }

    /// Büyük oynat/duraklat düğmesi çizer (aksan rengi arka plan).
    fn draw_control_button_large(&self, fb: &mut Framebuffer, x: usize, y: usize, icon: &str) {
        fb.draw_rect(x, y, 40, 40, Theme::ACCENT_PRIMARY.to_u32());
        fb.draw_string(x + 10, y + 10, icon, Theme::TEXT_ON_ACCENT.to_u32());
    }

    /// Saniyeyi "dk:ss" biçimine dönüştürür.
    fn format_time(&self, secs: f32) -> String {
        let total = secs as u32;
        let mins = total / 60;
        let s = total % 60;
        format!("{}:{:02}", mins, s)
    }

    /// Fare tıklaması olayını işler; gerçekleştirilen eylemi döndürür.
    ///
    /// Hit-test (isabet testi): tıklanan koordinatın hangi UI öğesine
    /// karşılık geldiğini belirler. Koordinat hesaplamaları
    /// `self.rect` üzerinden türetilir.
    pub fn on_click(&mut self, mx: i32, my: i32) -> MusicPlayerAction {
        // Kapat düğmesi kontrolü
        let close_x = self.rect.x + self.rect.width - 28;
        if mx >= close_x && mx < close_x + 24 && my >= self.rect.y + 4 && my < self.rect.y + 28 {
            return MusicPlayerAction::Close;
        }

        // Kontrol düğmeleri bölgesi
        let controls_y = self.rect.y + 32 + 120 + 40;
        let center_x = self.rect.x + self.rect.width / 2;
        let center_y = controls_y + 30;

        // Önceki parça
        if mx >= center_x - 80 - 15 && mx < center_x - 80 + 15 && my >= center_y - 15 && my < center_y + 15 {
            self.prev();
            return MusicPlayerAction::None;
        }

        // Oynat / Duraklat
        if mx >= center_x - 15 && mx < center_x + 25 && my >= center_y - 20 && my < center_y + 20 {
            self.toggle_play();
            return MusicPlayerAction::None;
        }

        // Sonraki parça
        if mx >= center_x + 50 - 15 && mx < center_x + 50 + 15 && my >= center_y - 15 && my < center_y + 15 {
            self.next();
            return MusicPlayerAction::None;
        }

        // İlerleme çubuğuna tıklama (seek)
        let bar_x = self.rect.x + 20;
        let bar_width = self.rect.width - 40;
        let bar_y = self.rect.y + 32 + 120 + 10;

        if mx >= bar_x && mx < bar_x + bar_width && my >= bar_y && my < bar_y + 20 {
            let progress = (mx - bar_x) as f32 / bar_width as f32;
            let duration = self.playlist.current().map(|t| t.duration).unwrap_or(0.0);
            self.seek(progress * duration);
            return MusicPlayerAction::None;
        }

        // Ses seviyesi çubuğuna tıklama
        let vol_x = self.rect.x + self.rect.width - 100 + 24;
        let vol_width = 60;
        let vol_y = controls_y + 22;

        if mx >= vol_x && mx < vol_x + vol_width && my >= vol_y && my < vol_y + 16 {
            let volume = (mx - vol_x) as f32 / vol_width as f32;
            self.set_volume(volume);
            return MusicPlayerAction::None;
        }

        // Çalarlist öğelerine tıklama
        let list_y = controls_y + 60 + if self.show_visualizer { 100 } else { 0 } + 24;
        let item_height = 28;

        if mx >= self.rect.x && my >= list_y {
            let item_idx = self.scroll_offset + ((my - list_y as i32) / item_height as i32) as usize;

            if item_idx < self.playlist.count() {
                self.selected_index = Some(item_idx);
                self.playlist.set_current(item_idx);
                self.position = 0.0;
                if self.state == PlaybackState::Playing {
                    self.play();
                }
            }
        }

        MusicPlayerAction::None
    }

    /// Fare tekerleği olayını işler; çalarlist kaydırması için kullanılır.
    pub fn on_scroll(&mut self, delta: i32) {
        self.scroll_offset = (self.scroll_offset as i32 + delta * 3).max(0) as usize;
        self.scroll_offset = self.scroll_offset.min(self.playlist.count().saturating_sub(1));
    }

    /// Pencerenin mevcut konumunu ve boyutunu döndürür.
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// Pencerenin konumunu ve boyutunu ayarlar.
    pub fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }

    /// Mevcut oynatma durumunu döndürür.
    pub fn state(&self) -> PlaybackState {
        self.state
    }

    /// Mevcut ses seviyesini döndürür.
    pub fn volume(&self) -> f32 {
        self.volume
    }
}

/// Müzik çalardan yayılan eylemler.
///
/// `enum` tabanlı eylem kalıbı: UI bileşeni kendi durumunu
/// doğrudan değiştirmek yerine bir eylem döndürür; üst katman
/// bu eylemi işleyerek gerekli güncellemeleri yapar. Bu yaklaşım
/// elm mimarisine ve Redux/TEA desenine benzer.
#[derive(Clone, Debug)]
pub enum MusicPlayerAction {
    None,
    Close,
    Play,
    Pause,
    Stop,
    Next,
    Previous,
    VolumeChanged(f32),
    TrackChanged(String),
}

impl Default for MusicPlayer {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GLOBAL MÜZİK ÇALAR — Tek Örnek (Singleton)
// ============================================================================

/// `lazy_static!` makrosu, global değişkenlerin çalışma zamanında
/// ilk kullanımda başlatılmasını sağlar. `Mutex` ile korunan
/// bu yapı, birden fazla iş parçacığından güvenli erişim imkânı tanır.
/// `no_std` ortamında `spin::Mutex` tercih edilir çünkü işletim
/// sistemi kilitleme primitiflerine bağımlılık yoktur.
lazy_static::lazy_static! {
    static ref MUSIC_PLAYER: Mutex<MusicPlayer> = Mutex::new(MusicPlayer::new());
}

/// Global müzik çalar örneğine referans döndürür.
pub fn get_player() -> &'static Mutex<MusicPlayer> {
    &MUSIC_PLAYER
}

/// Müzik çalar modülünü başlatır.
pub fn init() {
    crate::serial_println!("[GUI] Music Player initialized");
}

// ============================================================================
// SES MODÜLÜ TASLAGI (STUB)
// ============================================================================

/// Ses modülü taslağı (gerçek sürücü ayrı dosyada yer alacak).
///
/// Bu modül, ses sürücüsünün arayüzünü tanımlar. Gerçek bir uygulamada
/// ALSA, PulseAudio veya doğrudan HC yapılandırması ile değiştirilir.
/// "Stub" mimarisi: arayüz sabitken implementasyon değişebilir.
pub mod audio {
    use spin::Mutex;

    pub struct AudioBackend {
        playing: bool,
        volume: f32,
    }

    impl AudioBackend {
        pub fn new() -> Self {
            AudioBackend {
                playing: false,
                volume: 1.0,
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
        }

        pub fn set_volume(&mut self, volume: f32) {
            self.volume = volume;
        }

        pub fn seek(&mut self, _position: f32) {}
    }

    lazy_static::lazy_static! {
        static ref AUDIO: Mutex<AudioBackend> = Mutex::new(AudioBackend::new());
    }

    pub fn get_audio() -> Option<&'static Mutex<AudioBackend>> {
        Some(&AUDIO)
    }
}
