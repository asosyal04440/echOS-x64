//! # Müzik Çalar Uygulaması
//!
//! Çalma listesi desteği, görselleştirmeler ve kontrollerle ses çalar
//! Oynatma için ses arka ucuyla entegre olur

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
// SES FORMATI
// ============================================================================

/// Desteklenen ses formatları
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
// PARÇA BİLGİSİ
// ============================================================================

/// Parça meta verisi
#[derive(Clone, Debug)]
pub struct TrackInfo {
    /// Dosya yolu
    pub path: String,
    /// Başlık
    pub title: String,
    /// Sanatçı
    pub artist: String,
    /// Albüm
    pub album: String,
    /// Saniye cinsinden süre
    pub duration: f32,
    /// Örnekleme hızı
    pub sample_rate: u32,
    /// Kanal sayısı
    pub channels: u8,
    /// Bit hızı (kbps)
    pub bitrate: u32,
    /// Format
    pub format: AudioFormat,
    /// Parça numarası
    pub track_number: Option<u32>,
    /// Yıl
    pub year: Option<u32>,
    /// Tür
    pub genre: String,
    /// Albüm kapağı (yer tutucu - görüntü verisi olacak)
    pub has_album_art: bool,
}

impl TrackInfo {
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
    
    pub fn format_duration(&self) -> String {
        let total_secs = self.duration as u32;
        let mins = total_secs / 60;
        let secs = total_secs % 60;
        format!("{}:{:02}", mins, secs)
    }
}

// ============================================================================
// OYNATMA DURUMU
// ============================================================================

/// Oynatma durumu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Playing,
    Paused,
}

/// Tekrar modu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepeatMode {
    None,
    One,
    All,
}

/// Karıştırma modu
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ShuffleMode {
    #[default]
    Off,
    On,
}

// ============================================================================
// ÇALMA LİSTESİ
// ============================================================================

/// Çalma listesi yöneticisi
pub struct Playlist {
    /// Parçalar
    tracks: Vec<TrackInfo>,
    /// Mevcut parça indeksi
    current_index: Option<usize>,
    /// Çalma sırası (karıştırma için)
    play_order: Vec<usize>,
    /// Sıra konumu
    order_position: usize,
    /// Karıştırma modu
    shuffle: ShuffleMode,
    /// Tekrar modu
    repeat: RepeatMode,
    /// Ad
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
    
    /// Parça ekle
    pub fn add_track(&mut self, track: TrackInfo) {
        self.tracks.push(track);
        self.rebuild_play_order();
    }
    
    /// Parça kaldır
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
    
    /// Çalma listesini temizle
    pub fn clear(&mut self) {
        self.tracks.clear();
        self.current_index = None;
        self.play_order.clear();
        self.order_position = 0;
    }
    
    /// Parça sayısını al
    pub fn count(&self) -> usize {
        self.tracks.len()
    }
    
    /// Mevcut parçayı al
    pub fn current(&self) -> Option<&TrackInfo> {
        self.current_index.and_then(|i| self.tracks.get(i))
    }
    
    /// İndekse göre parça al
    pub fn get(&self, index: usize) -> Option<&TrackInfo> {
        self.tracks.get(index)
    }
    
    /// Mevcut parçayı ayarla
    pub fn set_current(&mut self, index: usize) {
        if index < self.tracks.len() {
            self.current_index = Some(index);
            
            // Sıra konumunu güncelle
            if self.shuffle == ShuffleMode::On {
                self.order_position = self.play_order.iter().position(|&i| i == index).unwrap_or(0);
            }
        }
    }
    
    /// Sonraki parça
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
    
    /// Önceki parça
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
    
    /// Karıştırma modunu ayarla
    pub fn set_shuffle(&mut self, mode: ShuffleMode) {
        self.shuffle = mode;
        self.rebuild_play_order();
    }
    
    /// Tekrar modunu ayarla
    pub fn set_repeat(&mut self, mode: RepeatMode) {
        self.repeat = mode;
    }
    
    /// Çalma sırasını yeniden oluştur
    fn rebuild_play_order(&mut self) {
        self.play_order = (0..self.tracks.len()).collect();
        
        if self.shuffle == ShuffleMode::On {
            // Fisher-Yates karıştırma algoritması
            for i in (1..self.play_order.len()).rev() {
                let j = (self.get_random() as usize) % (i + 1);
                self.play_order.swap(i, j);
            }
        }
        
        // Konumu sıfırla
        if let Some(current) = self.current_index {
            self.order_position = self.play_order.iter().position(|&i| i == current).unwrap_or(0);
        }
    }
    
    fn get_random(&self) -> u64 {
        // Basit rastgele - gerçek RNG kullanılacak
        crate::cpu::tsc::read() as u64
    }
}

// ============================================================================
// SES GÖRSELLEŞTİRİCİSİ
// ============================================================================

/// Ses görselleştiricisi
pub struct AudioVisualizer {
    /// Frekans verisi (basitleştirilmiş)
    bars: Vec<f32>,
    /// Çubuk sayısı
    bar_count: usize,
    /// Yumuşatma için geçmiş
    history: Vec<Vec<f32>>,
    /// Yumuşatma faktörü
    smoothing: f32,
    /// Tepe değleri
    peaks: Vec<f32>,
    /// Tepe tutma süresi
    peak_hold: Vec<u32>,
    /// Görselleştirme modu
    mode: VisualizationMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisualizationMode {
    Bars,
    Waveform,
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
    
    /// Yeni ses verisiyle güncelle
    pub fn update(&mut self, audio_data: &[f32]) {
        // Basit FFT yaklaşımı - bantlara böl
        let samples_per_bar = audio_data.len() / self.bar_count;
        
        for i in 0..self.bar_count {
            let start = i * samples_per_bar;
            let end = (start + samples_per_bar).min(audio_data.len());
            
            let mut sum = 0.0;
            for j in start..end {
                sum += audio_data[j].abs();
            }
            
            let avg = sum / (end - start).max(1) as f32;
            
            // Yumuşat
            self.bars[i] = self.bars[i] * self.smoothing + avg * (1.0 - self.smoothing);
            
            // Tepeyi güncelle
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
    
    /// Sahte görselleştirme verisi üret
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
    
    /// Görselleştirmeyi çiz
    pub fn draw(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize) {
        match self.mode {
            VisualizationMode::Bars => self.draw_bars(fb, x, y, width, height),
            VisualizationMode::Waveform => self.draw_waveform(fb, x, y, width, height),
            VisualizationMode::Circle => self.draw_circle(fb, x, y, width, height),
        }
    }
    
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
            
            // Tepeyi çiz
            if self.peaks[i] > 0.01 {
                let peak_y = y + height - (self.peaks[i] * height as f32 * 0.9) as usize;
                fb.draw_rect(bar_x, peak_y, bar_width - bar_gap, 2, Theme::ACCENT_PRIMARY.to_u32());
            }
        }
    }
    
    fn draw_waveform(&self, fb: &mut Framebuffer, x: usize, y: usize, width: usize, height: usize) {
        let center_y = y + height / 2;
        let half_height = height / 2;
        
        for i in 0..width {
            let bar_idx = (i * self.bar_count / width).min(self.bar_count - 1);
            let amplitude = (self.bars[bar_idx] * half_height as f32) as usize;
            
            // Merkezden çizgi çiz
            let color = self.get_bar_color(bar_idx, self.bars[bar_idx]);
            fb.draw_rect(x + i, center_y - amplitude, 1, amplitude * 2, color);
        }
    }
    
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
            
            // İçten dışa çizgi çiz
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
    
    fn get_bar_color(&self, index: usize, value: f32) -> u32 {
        let hue = (index as f32 / self.bar_count as f32 * 360.0) as u16;
        let saturation = 0.8;
        let lightness = 0.5 + value * 0.3;
        
        // HSL'den RGB'ye dönüşüm (basitleştirilmiş)
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
    
    /// Görselleştirme modunu ayarla
    pub fn set_mode(&mut self, mode: VisualizationMode) {
        self.mode = mode;
    }
}

// ============================================================================
// MÜZİK ÇALAR
// ============================================================================

/// Müzik Çalar Uygulaması
pub struct MusicPlayer {
    /// Pencere konumu ve boyutu
    rect: Rect,
    /// Çalma listesi
    playlist: Playlist,
    /// Oynatma durumu
    state: PlaybackState,
    /// Mevcut konum (saniye)
    position: f32,
    /// Ses seviyesi (0.0 - 1.0)
    volume: f32,
    /// Görselleştirici
    visualizer: AudioVisualizer,
    /// Görselleştiriciy göster
    show_visualizer: bool,
    /// Çalma listesini göster
    show_playlist: bool,
    /// Ekalizeörü göster
    show_equalizer: bool,
    /// Ekalizeör bantları
    eq_bands: Vec<f32>,
    /// Görselleştirme için zaman
    time: f64,
    /// Seçili çalma listesi indeksi
    selected_index: Option<usize>,
    /// Kaydırma ofseti
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
    
    /// Dosya yükle
    pub fn load_file(&mut self, path: &str) {
        let track = TrackInfo::new(path);
        self.playlist.add_track(track);
        self.playlist.set_current(self.playlist.count() - 1);
        self.play();
    }
    
    /// Oynat
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
    
    /// Duraklat
    pub fn pause(&mut self) {
        self.state = PlaybackState::Paused;
        
        if let Some(audio) = crate::drivers::audio::get_audio() {
            audio.lock().pause();
        }
    }
    
    /// Durdur
    pub fn stop(&mut self) {
        self.state = PlaybackState::Stopped;
        self.position = 0.0;
        
        if let Some(audio) = crate::drivers::audio::get_audio() {
            audio.lock().stop();
        }
    }
    
    /// Oynat/Duraklat geçişi
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
    
    /// Sonraki parça
    pub fn next(&mut self) {
        if self.playlist.next().is_some() {
            self.position = 0.0;
            if self.state == PlaybackState::Playing {
                self.play();
            }
        }
    }
    
    /// Önceki parça
    pub fn prev(&mut self) {
        if self.position > 3.0 {
            // 3 saniyeden fazla ilerlemişse mevcut parçayı baştan başlat
            self.position = 0.0;
        } else if self.playlist.prev().is_some() {
            self.position = 0.0;
        }
        
        if self.state == PlaybackState::Playing {
            self.play();
        }
    }
    
    /// Ses seviyesini ayarla
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.max(0.0).min(1.0);
        
        if let Some(audio) = crate::drivers::audio::get_audio() {
            audio.lock().set_volume(self.volume);
        }
    }
    
    /// Konuma atla
    pub fn seek(&mut self, position: f32) {
        self.position = position.max(0.0);
        
        if let Some(audio) = crate::drivers::audio::get_audio() {
            audio.lock().seek(self.position);
        }
    }
    
    /// Güncelle (her karede çağrılır)
    pub fn update(&mut self, dt: f64) {
        if self.state == PlaybackState::Playing {
            self.position += dt as f32;
            self.time += dt;
            
            // Parçanın bitip bitmediğini kontrol et
            if let Some(track) = self.playlist.current() {
                if self.position >= track.duration && track.duration > 0.0 {
                    self.next();
                }
            }
            
            // Görselleştiriciy güncelle
            self.visualizer.generate_dummy(self.time);
        }
    }
    
    /// Oynatıcıyı çiz
    pub fn draw(&self, fb: &mut Framebuffer) {
        let x = self.rect.x as usize;
        let y = self.rect.y as usize;
        let width = self.rect.width as usize;
        let height = self.rect.height as usize;
        
        // Pencere arka planı
        fb.draw_rect(x, y, width, height, Theme::WINDOW_BG.to_u32());
        
        // Başlık çubağı
        fb.draw_rect(x, y, width, 32, Theme::TITLEBAR_BG.to_u32());
        fb.draw_string(x + 12, y + 8, "Music Player", Theme::TEXT_PRIMARY.to_u32());
        
        // Kapat düğmesi
        fb.draw_rect(x + width - 28, y + 4, 24, 24, Theme::ERROR.to_u32());
        fb.draw_string(x + width - 20, y + 8, "×", Theme::TEXT_ON_ACCENT.to_u32());
        
        // Mevcut parça bilgisi
        let info_y = y + 32;
        let info_height = 120;
        fb.draw_rect(x, info_y, width, info_height, Theme::SIDEBAR_BG.to_u32());
        
        if let Some(track) = self.playlist.current() {
            // Albüm kapağı yer tutucu
            let art_size = 80;
            let art_x = x + 20;
            let art_y = info_y + 20;
            
            // Yer tutucu albüm kapağı çiz
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
            
            // Parça bilgisi
            let text_x = art_x + art_size + 20;
            fb.draw_string(text_x, art_y, &track.title, Theme::TEXT_PRIMARY.to_u32());
            fb.draw_string(text_x, art_y + 20, &track.artist, Theme::TEXT_SECONDARY.to_u32());
            fb.draw_string(text_x, art_y + 40, &track.album, Theme::TEXT_SECONDARY.to_u32());
        } else {
            fb.draw_string(x + 20, info_y + 40, "No track playing", Theme::TEXT_SECONDARY.to_u32());
        }
        
        // İlerleme çubağı
        let progress_y = info_y + info_height;
        let progress_height = 40;
        fb.draw_rect(x, progress_y, width, progress_height, Theme::WINDOW_BG.to_u32());
        
        let track_duration = self.playlist.current().map(|t| t.duration).unwrap_or(1.0).max(1.0);
        let progress = self.position / track_duration;
        
        // İlerleme çubağı arka planı
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
        
        // Kontroller
        let controls_y = progress_y + progress_height;
        let controls_height = 60;
        fb.draw_rect(x, controls_y, width, controls_height, Theme::TOOLBAR_BG.to_u32());
        
        let center_x = x + width / 2;
        let center_y = controls_y + controls_height / 2;
        
        // Önceki düğme
        self.draw_control_button(fb, center_x - 80, center_y - 15, "◀◀");
        
        // Oynat/Duraklat düğmesi
        let play_icon = match self.state {
            PlaybackState::Playing => "❚❚",
            _ => "▶",
        };
        self.draw_control_button_large(fb, center_x - 15, center_y - 20, play_icon);
        
        // Sonraki düğme
        self.draw_control_button(fb, center_x + 50, center_y - 15, "▶▶");
        
        // Ses seviyesi
        let vol_x = x + width - 100;
        fb.draw_string(vol_x, center_y - 8, "🔊", Theme::TEXT_PRIMARY.to_u32());
        
        // Ses çubağı
        let vol_bar_x = vol_x + 24;
        let vol_bar_width = 60;
        fb.draw_rect(vol_bar_x, center_y - 4, vol_bar_width, 8, Theme::BORDER.to_u32());
        fb.draw_rect(vol_bar_x, center_y - 4, (vol_bar_width as f32 * self.volume) as usize, 8, Theme::ACCENT_PRIMARY.to_u32());
        
        // Görselleştirici
        if self.show_visualizer {
            let viz_y = controls_y + controls_height;
            let viz_height = 100;
            fb.draw_rect(x, viz_y, width, viz_height, 0x0A0A0A);
            
            self.visualizer.draw(fb, x + 10, viz_y + 10, width - 20, viz_height - 20);
        }
        
        // Çalma listesi
        if self.show_playlist {
            let list_y = controls_y + controls_height + if self.show_visualizer { 100 } else { 0 };
            let list_height = height - (list_y - y);
            
            fb.draw_rect(x, list_y, width, list_height, Theme::WINDOW_BG.to_u32());
            
            // Çalma listesi başlığı
            fb.draw_rect(x, list_y, width, 24, Theme::TOOLBAR_BG.to_u32());
            fb.draw_string(x + 12, list_y + 4, &format!("Playlist ({} tracks)", self.playlist.count()), Theme::TEXT_PRIMARY.to_u32());
            
            // Çalma listesi öğeleri
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
                    
                    // Başlık
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
    
    fn draw_control_button(&self, fb: &mut Framebuffer, x: usize, y: usize, icon: &str) {
        fb.draw_rect(x, y, 30, 30, Theme::BORDER.to_u32());
        fb.draw_string(x + 6, y + 6, icon, Theme::TEXT_PRIMARY.to_u32());
    }
    
    fn draw_control_button_large(&self, fb: &mut Framebuffer, x: usize, y: usize, icon: &str) {
        fb.draw_rect(x, y, 40, 40, Theme::ACCENT_PRIMARY.to_u32());
        fb.draw_string(x + 10, y + 10, icon, Theme::TEXT_ON_ACCENT.to_u32());
    }
    
    fn format_time(&self, secs: f32) -> String {
        let total = secs as u32;
        let mins = total / 60;
        let s = total % 60;
        format!("{}:{:02}", mins, s)
    }
    
    /// Tıklamayı işle
    pub fn on_click(&mut self, mx: i32, my: i32) -> MusicPlayerAction {
        // Kapat düğmesi
        let close_x = self.rect.x + self.rect.width - 28;
        if mx >= close_x && mx < close_x + 24 && my >= self.rect.y + 4 && my < self.rect.y + 28 {
            return MusicPlayerAction::Close;
        }
        
        // Kontroller
        let controls_y = self.rect.y + 32 + 120 + 40;
        let center_x = self.rect.x + self.rect.width / 2;
        let center_y = controls_y + 30;
        
        // Önceki
        if mx >= center_x - 80 - 15 && mx < center_x - 80 + 15 && my >= center_y - 15 && my < center_y + 15 {
            self.prev();
            return MusicPlayerAction::None;
        }
        
        // Oynat/Duraklat
        if mx >= center_x - 15 && mx < center_x + 25 && my >= center_y - 20 && my < center_y + 20 {
            self.toggle_play();
            return MusicPlayerAction::None;
        }
        
        // Sonraki
        if mx >= center_x + 50 - 15 && mx < center_x + 50 + 15 && my >= center_y - 15 && my < center_y + 15 {
            self.next();
            return MusicPlayerAction::None;
        }
        
        // İlerleme çubağı
        let bar_x = self.rect.x + 20;
        let bar_width = self.rect.width - 40;
        let bar_y = self.rect.y + 32 + 120 + 10;
        
        if mx >= bar_x && mx < bar_x + bar_width && my >= bar_y && my < bar_y + 20 {
            let progress = (mx - bar_x) as f32 / bar_width as f32;
            let duration = self.playlist.current().map(|t| t.duration).unwrap_or(0.0);
            self.seek(progress * duration);
            return MusicPlayerAction::None;
        }
        
        // Ses çubağı
        let vol_x = self.rect.x + self.rect.width - 100 + 24;
        let vol_width = 60;
        let vol_y = controls_y + 22;
        
        if mx >= vol_x && mx < vol_x + vol_width && my >= vol_y && my < vol_y + 16 {
            let volume = (mx - vol_x) as f32 / vol_width as f32;
            self.set_volume(volume);
            return MusicPlayerAction::None;
        }
        
        // Çalma listesi öğeleri
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
    
    /// Kaydırmayı işle
    pub fn on_scroll(&mut self, delta: i32) {
        self.scroll_offset = (self.scroll_offset as i32 + delta * 3).max(0) as usize;
        self.scroll_offset = self.scroll_offset.min(self.playlist.count().saturating_sub(1));
    }
    
    /// Dikdörtgeni al
    pub fn rect(&self) -> Rect {
        self.rect
    }
    
    /// Dikdörtgeni ayarla
    pub fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
    }
    
    /// Durumu al
    pub fn state(&self) -> PlaybackState {
        self.state
    }
    
    /// Ses seviyesini al
    pub fn volume(&self) -> f32 {
        self.volume
    }
}

/// Müzik çalardan eylemler
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
// GLOBAL MÜZİK ÇALAR
// ============================================================================

lazy_static::lazy_static! {
    static ref MUSIC_PLAYER: Mutex<MusicPlayer> = Mutex::new(MusicPlayer::new());
}

/// Müzik çaları al
pub fn get_player() -> &'static Mutex<MusicPlayer> {
    &MUSIC_PLAYER
}

/// Müzik çaları başlat
pub fn init() {
    crate::serial_println!("[GUI] Music Player initialized");
}

// ============================================================================
// SES MODÜLÜ İYONLAMA KATMANI
// ============================================================================

/// Ses modülü iyonlama katmanı (ayrı dosyada olacak)
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
