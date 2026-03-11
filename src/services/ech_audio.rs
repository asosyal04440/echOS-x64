//! # EchAudio - Audio Service
//!
//! Ses servisi. VirtIO ses desteği ve ses çıkışı yönetimi sağlar.
//!
//! ## Özellikler
//!
//! - VirtIO ses sürücüsü entegrasyonu
//! - PCM ses akışı
//! - Çoklu ses kanalı desteği
//! - Ses efektleri ve karıştırma
//!
//! ## Mimari
//!
//! EchAudio ayrı bir kernel görevi olarak çalışır ve:
//! - VirtIO ses aygıtıyla iletişim kurar
//! - Ses verilerini işler ve çıkışa yönlendirir
//! - IPC üzerinden ses komutları alır

use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

use lazy_static::lazy_static;

/// Ses formatları
#[derive(Clone, Debug)]
pub enum AudioFormat {
    PCM16,
    PCM24,
    PCM32,
    Float32,
}

/// Ses kanalı
#[derive(Clone, Debug)]
pub struct AudioChannel {
    pub id: u32,
    pub format: AudioFormat,
    pub sample_rate: u32,
    pub channels: u8, // 1 = mono, 2 = stereo
    pub volume: f32,
}

/// EchAudio servisi komutları
#[derive(Clone, Debug)]
pub enum AudioCommand {
    /// Yeni ses kanalı oluştur
    CreateChannel {
        format: AudioFormat,
        sample_rate: u32,
        channels: u8,
    },
    /// Ses kanalını kapat
    CloseChannel { channel_id: u32 },
    /// Ses verisi gönder
    SendAudioData { channel_id: u32, data: Vec<u8> },
    /// Ses seviyesini ayarla
    SetVolume { channel_id: u32, volume: f32 },
    /// Ses efektini uygula
    ApplyEffect {
        channel_id: u32,
        effect_type: AudioEffect,
    },
}

/// Ses efektleri
#[derive(Clone, Debug)]
pub enum AudioEffect {
    Reverb,
    Echo,
    Chorus,
    Distortion,
    Equalizer,
}

#[derive(Clone, Debug)]
pub struct AudioRoute {
    pub from_channel: u32,
    pub to_channel: u32,
    pub gain_q15: i16,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DspProfile {
    pub hrtf_enabled: bool,
    pub agc_enabled: bool,
    pub aec_enabled: bool,
}

/// Audio servisi yanıtı
#[derive(Clone, Debug)]
pub enum AudioResponse {
    /// Komut başarıyla işlendi
    Success,
    /// Kanal oluşturuldu
    ChannelCreated { channel_id: u32 },
    /// Hata oluştu
    Error(String),
}

/// EchAudio servisi
pub struct EchAudio {
    /// Çalışma durumu
    running: AtomicBool,
    /// Ses kanalları
    channels: Mutex<Vec<AudioChannel>>,
    /// Ses veri kuyruğu
    audio_queue: Mutex<VecDeque<(u32, Vec<u8>)>>, // (channel_id, data)
    /// Komut kuyruğu
    command_queue: Mutex<Vec<AudioCommand>>,
    /// Yanıt kuyruğu
    response_queue: Mutex<Vec<AudioResponse>>,
    /// PipeWire-benzeri kanal yönlendirme grafı
    routing_graph: Mutex<Vec<AudioRoute>>,
    /// Kanal başına DSP profili (HRTF/AGC/AEC)
    dsp_profiles: Mutex<BTreeMap<u32, DspProfile>>,
    /// AEC için kanal başına son örnek
    echo_taps: Mutex<BTreeMap<u32, i16>>,
    /// Düşük gecikmeli gerçek zamanlı miksleme modu
    rt_mixing: AtomicBool,
    /// VirtIO ses sürücüsü referansı
    virtio_audio: Option<Arc<Mutex<()>>>, // VirtIO driver handle
}

impl EchAudio {
    /// Yeni EchAudio örneği oluştur
    pub fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            channels: Mutex::new(Vec::new()),
            audio_queue: Mutex::new(VecDeque::new()),
            command_queue: Mutex::new(Vec::new()),
            response_queue: Mutex::new(Vec::new()),
            routing_graph: Mutex::new(Vec::new()),
            dsp_profiles: Mutex::new(BTreeMap::new()),
            echo_taps: Mutex::new(BTreeMap::new()),
            rt_mixing: AtomicBool::new(true),
            virtio_audio: None,
        }
    }

    /// VirtIO ses sürücüsünü ayarla
    pub fn set_virtio_driver(&mut self, driver: Arc<Mutex<()>>) {
        self.virtio_audio = Some(driver);
    }

    /// Servisi başlat
    pub fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
        crate::serial_println!("[ECHAUDIO] Audio service started");
    }

    /// Servisi durdur
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        crate::serial_println!("[ECHAUDIO] Audio service stopped");
    }

    /// Komut gönder
    pub fn send_command(&self, command: AudioCommand) {
        self.command_queue.lock().push(command);
    }

    /// Yanıt al (non-blocking)
    pub fn receive_response(&self) -> Option<AudioResponse> {
        self.response_queue.lock().pop()
    }

    /// Ana servis döngüsü (kernel task olarak çalıştırılır)
    pub fn run_service(&self) {
        while self.running.load(Ordering::SeqCst) {
            // Komutları işle
            let commands = {
                let mut queue = self.command_queue.lock();
                core::mem::take(&mut *queue)
            };

            for command in commands {
                let response = self.process_command(command);
                self.response_queue.lock().push(response);
            }

            // Ses verilerini işle
            self.process_audio_data();

            // Kısa bekleme
            for _ in 0..1000 {
                core::hint::spin_loop();
            }
        }
    }

    /// Komutu işle
    fn process_command(&self, command: AudioCommand) -> AudioResponse {
        match command {
            AudioCommand::CreateChannel {
                format,
                sample_rate,
                channels,
            } => self.create_channel(format, sample_rate, channels),
            AudioCommand::CloseChannel { channel_id } => self.close_channel(channel_id),
            AudioCommand::SendAudioData { channel_id, data } => {
                self.send_audio_data(channel_id, data)
            }
            AudioCommand::SetVolume { channel_id, volume } => self.set_volume(channel_id, volume),
            AudioCommand::ApplyEffect {
                channel_id,
                effect_type,
            } => self.apply_effect(channel_id, effect_type),
        }
    }

    /// Ses kanalını oluştur
    fn create_channel(&self, format: AudioFormat, sample_rate: u32, channels: u8) -> AudioResponse {
        let channel_id = self.channels.lock().len() as u32 + 1;

        let channel = AudioChannel {
            id: channel_id,
            format: format.clone(),
            sample_rate,
            channels,
            volume: 1.0,
        };

        self.channels.lock().push(channel);
        self.routing_graph.lock().push(AudioRoute {
            from_channel: channel_id,
            to_channel: 0,
            gain_q15: i16::MAX,
        });
        self.dsp_profiles
            .lock()
            .insert(channel_id, DspProfile::default());

        crate::serial_println!(
            "[ECHAUDIO] Created audio channel {}: {}Hz, {}ch, {:?}",
            channel_id,
            sample_rate,
            channels,
            format
        );

        AudioResponse::ChannelCreated { channel_id }
    }

    /// Ses kanalını kapat
    fn close_channel(&self, channel_id: u32) -> AudioResponse {
        let mut channels = self.channels.lock();
        if let Some(pos) = channels.iter().position(|c| c.id == channel_id) {
            channels.remove(pos);
            self.routing_graph
                .lock()
                .retain(|r| r.from_channel != channel_id && r.to_channel != channel_id);
            self.dsp_profiles.lock().remove(&channel_id);
            self.echo_taps.lock().remove(&channel_id);
            crate::serial_println!("[ECHAUDIO] Closed audio channel {}", channel_id);
            AudioResponse::Success
        } else {
            AudioResponse::Error(alloc::format!("Channel {} not found", channel_id))
        }
    }

    /// Ses verisi gönder
    fn send_audio_data(&self, channel_id: u32, data: Vec<u8>) -> AudioResponse {
        // Ses verisini kuyruğa ekle
        self.audio_queue.lock().push_back((channel_id, data));
        AudioResponse::Success
    }

    /// Ses seviyesini ayarla
    fn set_volume(&self, channel_id: u32, volume: f32) -> AudioResponse {
        let mut channels = self.channels.lock();
        if let Some(channel) = channels.iter_mut().find(|c| c.id == channel_id) {
            channel.volume = volume.clamp(0.0, 1.0);
            crate::serial_println!(
                "[ECHAUDIO] Set volume for channel {}: {:.2}",
                channel_id,
                volume
            );
            AudioResponse::Success
        } else {
            AudioResponse::Error(alloc::format!("Channel {} not found", channel_id))
        }
    }

    /// Ses efektini uygula
    fn apply_effect(&self, channel_id: u32, effect_type: AudioEffect) -> AudioResponse {
        let mut profiles = self.dsp_profiles.lock();
        let profile = profiles.entry(channel_id).or_insert(DspProfile::default());
        match effect_type {
            AudioEffect::Reverb | AudioEffect::Echo => {
                profile.aec_enabled = true;
            }
            AudioEffect::Chorus => {
                profile.hrtf_enabled = true;
            }
            AudioEffect::Distortion => {
                profile.agc_enabled = true;
            }
            AudioEffect::Equalizer => {
                profile.hrtf_enabled = true;
                profile.agc_enabled = true;
            }
        }
        // Ses efekti uygulama mantığı
        // Gerçek implementasyonda DSP algoritmaları kullanılır
        crate::serial_println!(
            "[ECHAUDIO] Applied effect {:?} to channel {}",
            effect_type,
            channel_id
        );
        AudioResponse::Success
    }

    /// Ses verilerini işle
    fn process_audio_data(&self) {
        let audio_data = {
            let mut queue = self.audio_queue.lock();
            queue.drain(..).collect::<Vec<_>>()
        };

        let routes = self.routing_graph.lock().clone();
        let mut mix_bus: BTreeMap<u32, Vec<i16>> = BTreeMap::new();

        for (channel_id, data) in audio_data {
            let mut samples = Self::bytes_to_pcm16(&data);
            self.apply_dsp_pipeline(channel_id, &mut samples);

            for route in routes.iter().filter(|r| r.from_channel == channel_id) {
                let sink = mix_bus.entry(route.to_channel).or_default();
                Self::mix_into(sink, &samples, route.gain_q15);
            }
        }

        for (sink_channel, mixed) in mix_bus {
            let bytes = Self::pcm16_to_bytes(&mixed);
            crate::serial_println!(
                "[ECHAUDIO] Mixed {} bytes for sink {} (rt={})",
                bytes.len(),
                sink_channel,
                self.rt_mixing.load(Ordering::Relaxed)
            );
        }
    }

    fn bytes_to_pcm16(data: &[u8]) -> Vec<i16> {
        let mut out = Vec::with_capacity(data.len() / 2);
        let mut i = 0usize;
        while i + 1 < data.len() {
            out.push(i16::from_le_bytes([data[i], data[i + 1]]));
            i += 2;
        }
        out
    }

    fn pcm16_to_bytes(samples: &[i16]) -> Vec<u8> {
        let mut out = Vec::with_capacity(samples.len() * 2);
        for s in samples {
            out.extend_from_slice(&s.to_le_bytes());
        }
        out
    }

    fn mix_into(dst: &mut Vec<i16>, src: &[i16], gain_q15: i16) {
        if dst.len() < src.len() {
            dst.resize(src.len(), 0);
        }

        for (i, &sample) in src.iter().enumerate() {
            let scaled = ((sample as i32 * gain_q15 as i32) >> 15) as i32;
            let mixed = dst[i] as i32 + scaled;
            dst[i] = mixed.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        }
    }

    fn apply_dsp_pipeline(&self, channel_id: u32, samples: &mut [i16]) {
        let profile = self
            .dsp_profiles
            .lock()
            .get(&channel_id)
            .copied()
            .unwrap_or_default();

        if profile.hrtf_enabled {
            // Basit HRTF pan: kanal id parity'e göre L/R dengele.
            let pan_q15: i16 = if channel_id & 1 == 0 { 23170 } else { 16384 }; // ~0.707 / 0.5
            for pair in samples.chunks_exact_mut(2) {
                pair[0] = ((pair[0] as i32 * pan_q15 as i32) >> 15) as i16;
            }
        }

        if profile.agc_enabled {
            let mut peak = 1i32;
            for &s in samples.iter() {
                peak = peak.max((s as i32).abs());
            }
            let target = 24_000i32;
            let gain_q15 = ((target << 15) / peak).clamp(16384, 32768); // 0.5x..1.0x
            for s in samples.iter_mut() {
                let scaled = ((*s as i32 * gain_q15) >> 15).clamp(i16::MIN as i32, i16::MAX as i32);
                *s = scaled as i16;
            }
        }

        if profile.aec_enabled {
            let mut taps = self.echo_taps.lock();
            let echo = *taps.get(&channel_id).unwrap_or(&0i16);
            for s in samples.iter_mut() {
                let cancelled = (*s as i32 - ((echo as i32 * 3) / 4))
                    .clamp(i16::MIN as i32, i16::MAX as i32);
                *s = cancelled as i16;
            }
            if let Some(last) = samples.last().copied() {
                taps.insert(channel_id, last);
            }
        }
    }
}

/// Global EchAudio örneği
lazy_static::lazy_static! {
    static ref ECH_AUDIO: Arc<EchAudio> = Arc::new(EchAudio::new());
}

/// EchAudio'yu başlat
pub fn init() {
    ECH_AUDIO.start();
    crate::serial_println!("[ECHAUDIO] Initialized");
}

/// Global EchAudio referansı
pub fn get_audio() -> Arc<EchAudio> {
    Arc::clone(&ECH_AUDIO)
}

pub fn service_task() -> ! {
    let svc = get_audio();
    svc.run_service();
    loop {
        core::hint::spin_loop();
    }
}
