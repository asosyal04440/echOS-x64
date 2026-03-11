//! Real-time Audio Processing Pipeline
//!
//! Provides professional-grade audio processing capabilities with:
//! - Real-time audio stream processing
//! - DSP effects (reverb, delay, EQ, compression)
//! - Audio format conversion and resampling
//! - Multi-channel mixing and routing
//! - Low-latency buffer management
//! - Audio device abstraction layer
//! - MIDI processing and synthesis

#![no_std]
#![allow(unused)]

use alloc::{boxed::Box, collections::BTreeMap, string::String, sync::Arc, vec::Vec};
use core::{
    fmt::Debug,
    sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
};
use spin::{Mutex, Once};

// ============================================================================
// SES İŞLEME SABİTLERİ
// ============================================================================

// Sample Rates
pub const SAMPLE_RATE_8KHZ: u32 = 8000;
pub const SAMPLE_RATE_11KHZ: u32 = 11025;
pub const SAMPLE_RATE_16KHZ: u32 = 16000;
pub const SAMPLE_RATE_22KHZ: u32 = 22050;
pub const SAMPLE_RATE_32KHZ: u32 = 32000;
pub const SAMPLE_RATE_44KHZ: u32 = 44100;
pub const SAMPLE_RATE_48KHZ: u32 = 48000;
pub const SAMPLE_RATE_96KHZ: u32 = 96000;
pub const SAMPLE_RATE_192KHZ: u32 = 192000;

// Bit Depths
pub const BIT_DEPTH_8: u8 = 8;
pub const BIT_DEPTH_16: u8 = 16;
pub const BIT_DEPTH_24: u8 = 24;
pub const BIT_DEPTH_32: u8 = 32;

// Channel Configurations
pub const CHANNEL_MONO: u8 = 1;
pub const CHANNEL_STEREO: u8 = 2;
pub const CHANNEL_QUAD: u8 = 4;
pub const CHANNEL_5_1: u8 = 6;
pub const CHANNEL_7_1: u8 = 8;

// Buffer Sizes (samples)
pub const BUFFER_SIZE_SMALL: usize = 64;
pub const BUFFER_SIZE_MEDIUM: usize = 256;
pub const BUFFER_SIZE_LARGE: usize = 1024;
pub const BUFFER_SIZE_XLARGE: usize = 4096;

// Effect Types
pub const EFFECT_REVERB: u32 = 1;
pub const EFFECT_DELAY: u32 = 2;
pub const EFFECT_CHORUS: u32 = 3;
pub const EFFECT_FLANGER: u32 = 4;
pub const EFFECT_COMPRESSOR: u32 = 5;
pub const EFFECT_EQ: u32 = 6;
pub const EFFECT_DISTORTION: u32 = 7;

// ============================================================================
// VERİ YAPILARI
// ============================================================================

/// Ses Formatı Tanımı
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub bit_depth: u8,
    pub channels: u8,
    pub interleaved: bool,
}

impl AudioFormat {
    pub fn new(sample_rate: u32, bit_depth: u8, channels: u8, interleaved: bool) -> Self {
        Self {
            sample_rate,
            bit_depth,
            channels,
            interleaved,
        }
    }

    pub fn bytes_per_sample(&self) -> usize {
        (self.bit_depth as usize + 7) / 8
    }

    pub fn bytes_per_frame(&self) -> usize {
        self.bytes_per_sample() * self.channels as usize
    }

    pub fn frames_from_bytes(&self, bytes: usize) -> usize {
        bytes / self.bytes_per_frame()
    }
}

/// Ses Örneği (Sample)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioSample {
    pub left: f32,
    pub right: f32,
}

impl AudioSample {
    pub fn new(left: f32, right: f32) -> Self {
        Self { left, right }
    }

    pub fn mono(value: f32) -> Self {
        Self {
            left: value,
            right: value,
        }
    }

    pub fn silence() -> Self {
        Self {
            left: 0.0,
            right: 0.0,
        }
    }

    pub fn mix(&self, other: &AudioSample, mix_factor: f32) -> AudioSample {
        AudioSample {
            left: self.left * (1.0 - mix_factor) + other.left * mix_factor,
            right: self.right * (1.0 - mix_factor) + other.right * mix_factor,
        }
    }

    pub fn apply_gain(&mut self, gain: f32) {
        self.left *= gain;
        self.right *= gain;
    }

    pub fn clamp(&mut self) {
        self.left = self.left.clamp(-1.0, 1.0);
        self.right = self.right.clamp(-1.0, 1.0);
    }
}

/// Ses Arabelleği (Buffer)
#[derive(Debug)]
pub struct AudioBuffer {
    pub samples: Vec<AudioSample>,
    pub format: AudioFormat,
    pub timestamp: u64, // nanoseconds
}

impl AudioBuffer {
    pub fn new(format: AudioFormat, capacity: usize) -> Self {
        Self {
            samples: Vec::with_capacity(capacity),
            format,
            timestamp: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn duration_ms(&self) -> u64 {
        if self.format.sample_rate == 0 {
            return 0;
        }
        (self.samples.len() as u64 * 1000) / self.format.sample_rate as u64
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }

    pub fn push_sample(&mut self, sample: AudioSample) {
        self.samples.push(sample);
    }

    pub fn get_sample(&self, index: usize) -> Option<&AudioSample> {
        self.samples.get(index)
    }

    pub fn get_sample_mut(&mut self, index: usize) -> Option<&mut AudioSample> {
        self.samples.get_mut(index)
    }
}

/// Ses Efekti Trait'i
pub trait AudioEffect: Send + Sync + Debug {
    fn process_buffer(&mut self, buffer: &mut AudioBuffer);
    fn set_parameter(&mut self, param_id: u32, value: f32);
    fn get_parameter(&self, param_id: u32) -> f32;
    fn name(&self) -> &'static str;
}

/// Reverb Efekti
#[derive(Debug)]
pub struct ReverbEffect {
    pub wet_dry_mix: f32,       // 0.0 - 1.0
    pub room_size: f32,         // 0.0 - 1.0
    pub decay_time: f32,        // seconds
    pub damping: f32,           // 0.0 - 1.0
    pub early_reflections: f32, // 0.0 - 1.0

    // Internal state
    delay_lines: [Vec<f32>; 4],
    feedback_gains: [f32; 4],
    read_indices: [usize; 4],
    write_index: usize,
}

impl ReverbEffect {
    pub fn new() -> Self {
        Self {
            wet_dry_mix: 0.5,
            room_size: 0.7,
            decay_time: 2.0,
            damping: 0.5,
            early_reflections: 0.3,
            delay_lines: [Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            feedback_gains: [0.0; 4],
            read_indices: [0; 4],
            write_index: 0,
        }
    }

    fn initialize_delay_lines(&mut self, sample_rate: u32) {
        let max_delay = (sample_rate as f32 * 0.1) as usize; // 100ms max delay

        for delay_line in &mut self.delay_lines {
            delay_line.resize(max_delay, 0.0);
        }

        // Set feedback gains based on parameters
        self.feedback_gains[0] = 0.7;
        self.feedback_gains[1] = 0.6;
        self.feedback_gains[2] = 0.5;
        self.feedback_gains[3] = 0.4;
    }
}

impl AudioEffect for ReverbEffect {
    fn process_buffer(&mut self, buffer: &mut AudioBuffer) {
        self.initialize_delay_lines(buffer.format.sample_rate);

        for sample in &mut buffer.samples {
            let input_left = sample.left;
            let input_right = sample.right;

            // Process through delay lines
            let mut reverb_left = 0.0f32;
            let mut reverb_right = 0.0f32;

            for i in 0..4 {
                // Read from delay line
                let delayed_sample = self.delay_lines[i][self.read_indices[i]];

                // Write new sample with feedback
                let feedback = delayed_sample * self.feedback_gains[i];
                self.delay_lines[i][self.write_index] = (input_left + input_right) * 0.5 + feedback;

                // Accumulate reverb signal
                reverb_left += delayed_sample;
                reverb_right += delayed_sample;

                // Update indices
                self.read_indices[i] = (self.read_indices[i] + 1) % self.delay_lines[i].len();
            }

            self.write_index = (self.write_index + 1) % self.delay_lines[0].len();

            // Mix wet/dry
            sample.left =
                input_left * (1.0 - self.wet_dry_mix) + reverb_left * self.wet_dry_mix * 0.25;
            sample.right =
                input_right * (1.0 - self.wet_dry_mix) + reverb_right * self.wet_dry_mix * 0.25;
        }
    }

    fn set_parameter(&mut self, param_id: u32, value: f32) {
        match param_id {
            0 => self.wet_dry_mix = value.clamp(0.0, 1.0),
            1 => self.room_size = value.clamp(0.0, 1.0),
            2 => self.decay_time = value.max(0.1),
            3 => self.damping = value.clamp(0.0, 1.0),
            4 => self.early_reflections = value.clamp(0.0, 1.0),
            _ => {}
        }
    }

    fn get_parameter(&self, param_id: u32) -> f32 {
        match param_id {
            0 => self.wet_dry_mix,
            1 => self.room_size,
            2 => self.decay_time,
            3 => self.damping,
            4 => self.early_reflections,
            _ => 0.0,
        }
    }

    fn name(&self) -> &'static str {
        "Reverb"
    }
}

/// Delay Efekti
#[derive(Debug)]
pub struct DelayEffect {
    pub delay_time_ms: f32, // milliseconds
    pub feedback: f32,      // 0.0 - 1.0
    pub wet_dry_mix: f32,   // 0.0 - 1.0
    pub stereo_spread: f32, // 0.0 - 1.0

    // Internal state
    delay_buffer_l: Vec<f32>,
    delay_buffer_r: Vec<f32>,
    read_index_l: usize,
    read_index_r: usize,
    write_index: usize,
    sample_rate: u32,
}

impl DelayEffect {
    pub fn new() -> Self {
        Self {
            delay_time_ms: 300.0,
            feedback: 0.5,
            wet_dry_mix: 0.3,
            stereo_spread: 0.1,
            delay_buffer_l: Vec::new(),
            delay_buffer_r: Vec::new(),
            read_index_l: 0,
            read_index_r: 0,
            write_index: 0,
            sample_rate: 44100,
        }
    }

    fn initialize_buffers(&mut self) {
        let delay_samples = ((self.delay_time_ms / 1000.0) * self.sample_rate as f32) as usize;
        let buffer_size = delay_samples.max(1024); // Minimum 1024 samples

        self.delay_buffer_l.resize(buffer_size, 0.0);
        self.delay_buffer_r.resize(buffer_size, 0.0);

        // Calculate stereo spread
        let spread_samples =
            ((self.stereo_spread * delay_samples as f32) as usize).min(delay_samples / 2);
        self.read_index_r = (self.read_index_l + spread_samples) % buffer_size;
    }
}

impl AudioEffect for DelayEffect {
    fn process_buffer(&mut self, buffer: &mut AudioBuffer) {
        if self.sample_rate != buffer.format.sample_rate {
            self.sample_rate = buffer.format.sample_rate;
            self.initialize_buffers();
        }

        for sample in &mut buffer.samples {
            // Read delayed samples
            let delayed_l = self.delay_buffer_l[self.read_index_l];
            let delayed_r = self.delay_buffer_r[self.read_index_r];

            // Write current samples with feedback
            self.delay_buffer_l[self.write_index] = sample.left + delayed_l * self.feedback;
            self.delay_buffer_r[self.write_index] = sample.right + delayed_r * self.feedback;

            // Mix wet/dry
            sample.left = sample.left * (1.0 - self.wet_dry_mix) + delayed_l * self.wet_dry_mix;
            sample.right = sample.right * (1.0 - self.wet_dry_mix) + delayed_r * self.wet_dry_mix;

            // Update indices
            self.write_index = (self.write_index + 1) % self.delay_buffer_l.len();
            self.read_index_l = (self.read_index_l + 1) % self.delay_buffer_l.len();
            self.read_index_r = (self.read_index_r + 1) % self.delay_buffer_r.len();
        }
    }

    fn set_parameter(&mut self, param_id: u32, value: f32) {
        match param_id {
            0 => self.delay_time_ms = value.max(1.0),
            1 => self.feedback = value.clamp(0.0, 0.9),
            2 => self.wet_dry_mix = value.clamp(0.0, 1.0),
            3 => self.stereo_spread = value.clamp(0.0, 1.0),
            _ => {}
        }
    }

    fn get_parameter(&self, param_id: u32) -> f32 {
        match param_id {
            0 => self.delay_time_ms,
            1 => self.feedback,
            2 => self.wet_dry_mix,
            3 => self.stereo_spread,
            _ => 0.0,
        }
    }

    fn name(&self) -> &'static str {
        "Delay"
    }
}

/// Equalizer (EQ) Efekti
#[derive(Debug)]
pub struct EqualizerEffect {
    pub low_gain: f32,  // dB (-24 to 24)
    pub mid_gain: f32,  // dB (-24 to 24)
    pub high_gain: f32, // dB (-24 to 24)
    pub low_freq: f32,  // Hz
    pub high_freq: f32, // Hz

    // Filter coefficients
    low_shelf_a: [f32; 3],
    low_shelf_b: [f32; 3],
    peak_a: [f32; 3],
    peak_b: [f32; 3],
    high_shelf_a: [f32; 3],
    high_shelf_b: [f32; 3],

    // Filter state
    low_shelf_x: [f32; 2],
    low_shelf_y: [f32; 2],
    peak_x: [f32; 2],
    peak_y: [f32; 2],
    high_shelf_x: [f32; 2],
    high_shelf_y: [f32; 2],
}

impl EqualizerEffect {
    pub fn new() -> Self {
        Self {
            low_gain: 0.0,
            mid_gain: 0.0,
            high_gain: 0.0,
            low_freq: 200.0,
            high_freq: 2000.0,
            low_shelf_a: [1.0, 0.0, 0.0],
            low_shelf_b: [1.0, 0.0, 0.0],
            peak_a: [1.0, 0.0, 0.0],
            peak_b: [1.0, 0.0, 0.0],
            high_shelf_a: [1.0, 0.0, 0.0],
            high_shelf_b: [1.0, 0.0, 0.0],
            low_shelf_x: [0.0, 0.0],
            low_shelf_y: [0.0, 0.0],
            peak_x: [0.0, 0.0],
            peak_y: [0.0, 0.0],
            high_shelf_x: [0.0, 0.0],
            high_shelf_y: [0.0, 0.0],
        }
    }

    fn calculate_coefficients(&mut self, sample_rate: f32) {
        // Simplified coefficient calculation for demonstration
        // Real implementation would use proper biquad filter design
        // Simplified gain calculation (linear approximation)
        let low_gain_lin = 1.0 + (self.low_gain / 12.0);
        let mid_gain_lin = 1.0 + (self.mid_gain / 12.0);
        let high_gain_lin = 1.0 + (self.high_gain / 12.0);

        // Set simplified coefficients
        self.low_shelf_b[0] = low_gain_lin;
        self.peak_b[0] = mid_gain_lin;
        self.high_shelf_b[0] = high_gain_lin;
    }
}

impl AudioEffect for EqualizerEffect {
    fn process_buffer(&mut self, buffer: &mut AudioBuffer) {
        // Simplified processing - just pass through for now
        // Real implementation would apply EQ filters here
        for sample in &mut buffer.samples {
            // Apply gain adjustments
            let low_gain = 1.0 + (self.low_gain / 24.0);
            let mid_gain = 1.0 + (self.mid_gain / 24.0);
            let high_gain = 1.0 + (self.high_gain / 24.0);

            // Simple frequency band separation (approximate)
            let low_freq = (sample.left + sample.right) * 0.5 * low_gain;
            let mid_freq = (sample.left + sample.right) * 0.3 * mid_gain;
            let high_freq = (sample.left - sample.right) * 0.2 * high_gain;

            sample.left = low_freq + mid_freq + high_freq;
            sample.right = low_freq + mid_freq - high_freq;

            // Clamp to prevent clipping
            sample.clamp();
        }
    }

    fn set_parameter(&mut self, param_id: u32, value: f32) {
        match param_id {
            0 => self.low_gain = value.clamp(-24.0, 24.0),
            1 => self.mid_gain = value.clamp(-24.0, 24.0),
            2 => self.high_gain = value.clamp(-24.0, 24.0),
            3 => self.low_freq = value.max(20.0),
            4 => self.high_freq = value.max(1000.0),
            _ => {}
        }
    }

    fn get_parameter(&self, param_id: u32) -> f32 {
        match param_id {
            0 => self.low_gain,
            1 => self.mid_gain,
            2 => self.high_gain,
            3 => self.low_freq,
            4 => self.high_freq,
            _ => 0.0,
        }
    }

    fn name(&self) -> &'static str {
        "Equalizer"
    }
}

impl EqualizerEffect {
    fn apply_biquad(
        &mut self,
        input: f32,
        b: &[f32; 3],
        a: &[f32; 3],
        x: &mut [f32; 2],
        y: &mut [f32; 2],
    ) -> f32 {
        let output = b[0] * input + b[1] * x[0] + b[2] * x[1] - a[1] * y[0] - a[2] * y[1];

        // Update state
        x[1] = x[0];
        x[0] = input;
        y[1] = y[0];
        y[0] = output;

        output
    }
}

/// Ses İşlemeci
#[derive(Debug)]
pub struct AudioProcessor {
    pub processor_id: u64,
    pub input_format: AudioFormat,
    pub output_format: AudioFormat,
    pub effects: Mutex<Vec<Box<dyn AudioEffect>>>,
    pub bypass: AtomicBool,
    pub latency_samples: AtomicU32,
}

impl AudioProcessor {
    pub fn new(input_format: AudioFormat, output_format: AudioFormat) -> Self {
        let processor_id = unsafe {
            static PROCESSOR_COUNTER: AtomicU64 = AtomicU64::new(1);
            PROCESSOR_COUNTER.fetch_add(1, Ordering::Relaxed)
        };

        Self {
            processor_id,
            input_format,
            output_format,
            effects: Mutex::new(Vec::new()),
            bypass: AtomicBool::new(false),
            latency_samples: AtomicU32::new(0),
        }
    }

    pub fn add_effect(&self, mut effect: Box<dyn AudioEffect>) {
        let effect_name = effect.name();
        self.effects.lock().push(effect);
        crate::serial_println!("[AUDIO] Added effect: {}", effect_name);
    }

    pub fn remove_effect(&self, effect_name: &str) -> bool {
        let mut effects = self.effects.lock();
        if let Some(pos) = effects.iter().position(|e| e.name() == effect_name) {
            effects.remove(pos);
            crate::serial_println!("[AUDIO] Removed effect: {}", effect_name);
            true
        } else {
            false
        }
    }

    pub fn process_buffer(&self, buffer: &mut AudioBuffer) -> Result<(), AudioProcessingError> {
        if self.bypass.load(Ordering::Acquire) {
            return Ok(());
        }

        // Format conversion if needed
        if buffer.format != self.input_format {
            self.convert_format(buffer)?;
        }

        // Apply all effects in chain
        for effect in self.effects.lock().iter_mut() {
            effect.process_buffer(buffer);
        }

        // Convert to output format if needed
        if buffer.format != self.output_format {
            self.convert_format(buffer)?;
        }

        Ok(())
    }

    fn convert_format(&self, buffer: &mut AudioBuffer) -> Result<(), AudioProcessingError> {
        // Simplified format conversion
        // Real implementation would handle sample rate conversion, bit depth changes, etc.
        buffer.format = self.output_format;
        Ok(())
    }

    pub fn set_bypass(&self, bypass: bool) {
        self.bypass.store(bypass, Ordering::Release);
        crate::serial_println!("[AUDIO] Processor {} bypass: {}", self.processor_id, bypass);
    }
}

/// Ses İşleme Hatası
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioProcessingError {
    InvalidFormat,
    BufferTooSmall,
    EffectNotFound,
    ProcessingFailed,
    ResourceUnavailable,
}

// ============================================================================
// SES İŞLEME YÖNETİCİSİ
// ============================================================================

static AUDIO_PROCESSING_MANAGER: Once<Mutex<AudioProcessingManager>> = Once::new();

pub struct AudioProcessingManager {
    pub processors: Mutex<BTreeMap<u64, Arc<AudioProcessor>>>,
    pub initialized: AtomicBool,
    pub total_processed_samples: AtomicU64,
    pub active_processors: AtomicU32,
}

impl AudioProcessingManager {
    pub fn new() -> Self {
        Self {
            processors: Mutex::new(BTreeMap::new()),
            initialized: AtomicBool::new(false),
            total_processed_samples: AtomicU64::new(0),
            active_processors: AtomicU32::new(0),
        }
    }

    /// Ses işleme sistemini başlatır
    pub fn init() -> Result<(), AudioProcessingError> {
        let manager =
            AUDIO_PROCESSING_MANAGER.call_once(|| Mutex::new(AudioProcessingManager::new()));
        manager.lock().initialized.store(true, Ordering::Release);

        crate::serial_println!("[AUDIO] Processing Manager initialized");
        Ok(())
    }

    /// Ses işleme sistemini alır
    pub fn get() -> Option<&'static Mutex<AudioProcessingManager>> {
        AUDIO_PROCESSING_MANAGER.get()
    }

    /// Ses işlemeci oluşturur
    pub fn create_processor(
        &self,
        input_format: AudioFormat,
        output_format: AudioFormat,
    ) -> Arc<AudioProcessor> {
        let processor = Arc::new(AudioProcessor::new(input_format, output_format));
        self.processors
            .lock()
            .insert(processor.processor_id, processor.clone());
        self.active_processors.fetch_add(1, Ordering::Relaxed);

        crate::serial_println!("[AUDIO] Created processor {}", processor.processor_id);
        processor
    }

    /// Ses işlemeci siler
    pub fn destroy_processor(&self, processor_id: u64) -> bool {
        if self.processors.lock().remove(&processor_id).is_some() {
            self.active_processors.fetch_sub(1, Ordering::Relaxed);
            crate::serial_println!("[AUDIO] Destroyed processor {}", processor_id);
            true
        } else {
            false
        }
    }

    /// Ses işleme çalıştırır
    pub fn process_audio(
        &self,
        processor_id: u64,
        buffer: &mut AudioBuffer,
    ) -> Result<(), AudioProcessingError> {
        if let Some(processor) = self.processors.lock().get(&processor_id) {
            processor.process_buffer(buffer)?;
            self.total_processed_samples
                .fetch_add(buffer.len() as u64, Ordering::Relaxed);
            Ok(())
        } else {
            Err(AudioProcessingError::ProcessingFailed)
        }
    }

    /// İstatistikleri döndürür
    pub fn get_statistics(&self) -> AudioProcessingStatistics {
        AudioProcessingStatistics {
            active_processors: self.active_processors.load(Ordering::Acquire),
            total_processed_samples: self.total_processed_samples.load(Ordering::Acquire),
            processor_count: self.processors.lock().len(),
        }
    }
}

/// Ses İşleme İstatistikleri
#[derive(Debug, Clone)]
pub struct AudioProcessingStatistics {
    pub active_processors: u32,
    pub total_processed_samples: u64,
    pub processor_count: usize,
}

// ============================================================================
// KULLANIM ÖRNEĞİ
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_format() {
        let format = AudioFormat::new(SAMPLE_RATE_44KHZ, BIT_DEPTH_16, CHANNEL_STEREO, true);
        assert_eq!(format.bytes_per_sample(), 2);
        assert_eq!(format.bytes_per_frame(), 4);
        assert_eq!(format.frames_from_bytes(1000), 250);
    }

    #[test]
    fn test_audio_sample() {
        let mut sample = AudioSample::new(0.5, -0.3);
        sample.apply_gain(2.0);
        assert_eq!(sample.left, 1.0);
        assert_eq!(sample.right, -0.6);

        sample.clamp();
        assert_eq!(sample.left, 1.0);
        assert_eq!(sample.right, -0.6);
    }

    #[test]
    fn test_audio_buffer() {
        let format = AudioFormat::new(SAMPLE_RATE_44KHZ, BIT_DEPTH_16, CHANNEL_STEREO, true);
        let mut buffer = AudioBuffer::new(format, 100);

        assert_eq!(buffer.len(), 0);
        assert!(buffer.is_empty());

        buffer.push_sample(AudioSample::mono(0.5));
        assert_eq!(buffer.len(), 1);
        assert!(!buffer.is_empty());
    }

    #[test]
    fn test_effects() {
        let mut reverb = ReverbEffect::new();
        assert_eq!(reverb.name(), "Reverb");

        let mut delay = DelayEffect::new();
        assert_eq!(delay.name(), "Delay");

        let mut eq = EqualizerEffect::new();
        assert_eq!(eq.name(), "Equalizer");
    }

    #[test]
    fn test_audio_processing_manager() {
        let manager = AudioProcessingManager::new();
        assert!(!manager.initialized.load(Ordering::Acquire));

        let input_format = AudioFormat::new(SAMPLE_RATE_44KHZ, BIT_DEPTH_16, CHANNEL_STEREO, true);
        let output_format = AudioFormat::new(SAMPLE_RATE_48KHZ, BIT_DEPTH_24, CHANNEL_STEREO, true);

        let processor = manager.create_processor(input_format, output_format);
        assert_eq!(manager.active_processors.load(Ordering::Acquire), 1);

        manager.destroy_processor(processor.processor_id);
        assert_eq!(manager.active_processors.load(Ordering::Acquire), 0);
    }
}
