use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use core::f32::consts::PI;

use lazy_static::lazy_static;
use libm::sinf;

pub const TTS_SAMPLE_RATE: u32 = 16_000;
pub const TTS_CHANNELS: u8 = 1;
const MAX_SPOKEN_CHARS: usize = 256;

const VOICE_ALEX: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/tts/voices/espeak-ng/Alex"
));
const VOICE_ALICIA: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/tts/voices/espeak-ng/Alicia"
));
const VOICE_GENE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/tts/voices/espeak-ng/Gene"
));

#[derive(Clone, Debug)]
pub struct VoiceProfile {
    pub id: &'static str,
    pub display_name: String,
    pub gender: Option<String>,
    pub pitch_low_hz: f32,
    pub pitch_high_hz: f32,
    pub voicing_gain: f32,
    pub roughness: f32,
    pub echo_delay_ms: u16,
    pub echo_decay: f32,
    pub formant_freq_scale: [f32; 3],
    pub formant_bandwidth_scale: [f32; 3],
    pub formant_gain_scale: [f32; 3],
    pub supported_locales: &'static [&'static str],
}

#[derive(Clone, Debug)]
pub struct SynthesizedSpeech {
    pub voice_id: &'static str,
    pub sample_rate: u32,
    pub channels: u8,
    pub pcm16_le: Vec<u8>,
    pub duration_ns: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceCatalogEntry {
    pub id: String,
    pub display_name: String,
    pub supported_locales: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceSelectionErrorKind {
    UnknownVoice,
    UnsupportedLocale,
    VoiceLocaleMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoiceSelectionError {
    pub kind: VoiceSelectionErrorKind,
    pub detail: String,
}

static VOICE_CATALOG: spin::Lazy<Vec<VoiceProfile>> = spin::Lazy::new(|| vec![
        parse_voice_profile("alex", VOICE_ALEX),
        parse_voice_profile("alicia", VOICE_ALICIA),
        parse_voice_profile("gene", VOICE_GENE),
    ]);

pub fn builtin_voices() -> &'static [VoiceProfile] {
    VOICE_CATALOG.as_slice()
}

pub fn voice_catalog() -> Vec<VoiceCatalogEntry> {
    builtin_voices()
        .iter()
        .map(|voice| VoiceCatalogEntry {
            id: voice.id.to_string(),
            display_name: voice.display_name.clone(),
            supported_locales: voice
                .supported_locales
                .iter()
                .map(|locale| (*locale).to_string())
                .collect(),
        })
        .collect()
}

pub fn voice_by_id(id: &str) -> Option<&'static VoiceProfile> {
    builtin_voices()
        .iter()
        .find(|voice| voice.id.eq_ignore_ascii_case(id))
}

pub fn select_voice(
    locale: &str,
    preferred_voice_id: Option<&str>,
) -> Result<&'static VoiceProfile, VoiceSelectionError> {
    let normalized = normalize_locale_tag(locale);
    if normalized.is_empty() {
        return Err(VoiceSelectionError {
            kind: VoiceSelectionErrorKind::UnsupportedLocale,
            detail: String::from("locale is empty"),
        });
    }
    if let Some(voice_id) = preferred_voice_id {
        let Some(voice) = voice_by_id(voice_id) else {
            return Err(VoiceSelectionError {
                kind: VoiceSelectionErrorKind::UnknownVoice,
                detail: alloc::format!("voice {} not found", voice_id),
            });
        };
        if voice_supports_locale(voice, normalized.as_str()) {
            return Ok(voice);
        }
        return Err(VoiceSelectionError {
            kind: VoiceSelectionErrorKind::VoiceLocaleMismatch,
            detail: alloc::format!("voice {} does not support locale {}", voice.id, locale),
        });
    }
    builtin_voices()
        .iter()
        .find(|voice| voice_supports_locale(voice, normalized.as_str()))
        .ok_or_else(|| VoiceSelectionError {
            kind: VoiceSelectionErrorKind::UnsupportedLocale,
            detail: alloc::format!("no speech voice supports locale {}", locale),
        })
}

pub fn default_voice() -> &'static VoiceProfile {
    select_voice("en-US", Some("alicia")).unwrap_or(&builtin_voices()[0])
}

pub fn synthesize(text: &str) -> SynthesizedSpeech {
    synthesize_with_voice(text, default_voice())
}

pub fn synthesize_with_voice(text: &str, voice: &VoiceProfile) -> SynthesizedSpeech {
    let normalized = text
        .chars()
        .filter(|ch| !ch.is_control())
        .take(MAX_SPOKEN_CHARS)
        .collect::<String>();
    let mut samples = Vec::new();
    if normalized.trim().is_empty() {
        append_silence(&mut samples, 60);
    } else {
        append_silence(&mut samples, 20);
        let chars: Vec<char> = normalized.chars().collect();
        for (index, ch) in chars.iter().enumerate() {
            emit_symbol(&mut samples, *ch, index, chars.len(), voice);
        }
        append_silence(&mut samples, 40);
    }
    apply_echo(&mut samples, voice);
    let duration_ns = (samples.len() as u64 * 1_000_000_000u64) / TTS_SAMPLE_RATE as u64;
    let mut pcm16_le = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        pcm16_le.extend_from_slice(&sample.to_le_bytes());
    }
    SynthesizedSpeech {
        voice_id: voice.id,
        sample_rate: TTS_SAMPLE_RATE,
        channels: TTS_CHANNELS,
        pcm16_le,
        duration_ns,
    }
}

fn parse_voice_profile(id: &'static str, source: &'static str) -> VoiceProfile {
    let mut profile = VoiceProfile {
        id,
        display_name: id.to_string(),
        gender: None,
        pitch_low_hz: 110.0,
        pitch_high_hz: 180.0,
        voicing_gain: 0.65,
        roughness: 0.03,
        echo_delay_ms: 0,
        echo_decay: 0.0,
        formant_freq_scale: [1.0; 3],
        formant_bandwidth_scale: [1.0; 3],
        formant_gain_scale: [1.0; 3],
        supported_locales: supported_locales_for_voice(id),
    };
    for raw_line in source.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(keyword) = parts.next() else {
            continue;
        };
        match keyword {
            "name" => {
                let rest = parts.collect::<Vec<_>>().join(" ");
                if !rest.is_empty() {
                    profile.display_name = rest;
                }
            }
            "gender" => {
                let rest = parts.collect::<Vec<_>>().join(" ");
                if !rest.is_empty() {
                    profile.gender = Some(rest);
                }
            }
            "pitch" => {
                let low = parts
                    .next()
                    .and_then(|value| value.parse::<f32>().ok())
                    .unwrap_or(profile.pitch_low_hz);
                let high = parts
                    .next()
                    .and_then(|value| value.parse::<f32>().ok())
                    .unwrap_or(low.max(profile.pitch_high_hz));
                profile.pitch_low_hz = low.max(60.0);
                profile.pitch_high_hz = high.max(profile.pitch_low_hz + 10.0);
            }
            "voicing" => {
                if let Some(value) = parts.next().and_then(|value| value.parse::<f32>().ok()) {
                    profile.voicing_gain = (value / 100.0).clamp(0.15, 1.8);
                }
            }
            "roughness" => {
                if let Some(value) = parts.next().and_then(|value| value.parse::<f32>().ok()) {
                    profile.roughness = (value / 32.0).clamp(0.0, 0.35);
                }
            }
            "echo" => {
                let delay = parts
                    .next()
                    .and_then(|value| value.parse::<u16>().ok())
                    .unwrap_or(0);
                let decay = parts
                    .next()
                    .and_then(|value| value.parse::<f32>().ok())
                    .unwrap_or(0.0);
                profile.echo_delay_ms = delay.min(240);
                profile.echo_decay = (decay / 100.0).clamp(0.0, 0.7);
            }
            "formant" => {
                let Some(index) = parts.next().and_then(|value| value.parse::<usize>().ok()) else {
                    continue;
                };
                if index >= 3 {
                    continue;
                }
                let freq = parts
                    .next()
                    .and_then(|value| value.parse::<f32>().ok())
                    .unwrap_or(100.0);
                let bandwidth = parts
                    .next()
                    .and_then(|value| value.parse::<f32>().ok())
                    .unwrap_or(100.0);
                let gain = parts
                    .next()
                    .and_then(|value| value.parse::<f32>().ok())
                    .unwrap_or(100.0);
                profile.formant_freq_scale[index] = (freq / 100.0).clamp(0.55, 2.5);
                profile.formant_bandwidth_scale[index] = (bandwidth / 100.0).clamp(0.4, 2.2);
                profile.formant_gain_scale[index] = (gain / 100.0).clamp(0.35, 2.5);
            }
            _ => {}
        }
    }
    profile
}

fn supported_locales_for_voice(id: &'static str) -> &'static [&'static str] {
    match id {
        "alex" => &["en", "en-us"],
        "alicia" => &["en", "en-us"],
        "gene" => &["en", "en-gb"],
        _ => &["en"],
    }
}

fn normalize_locale_tag(locale: &str) -> String {
    locale
        .trim()
        .chars()
        .map(|ch| match ch {
            '_' => '-',
            other => other.to_ascii_lowercase(),
        })
        .collect()
}

fn locale_language_prefix(locale: &str) -> &str {
    locale.split('-').next().unwrap_or(locale)
}

fn voice_supports_locale(voice: &VoiceProfile, normalized_locale: &str) -> bool {
    let language = locale_language_prefix(normalized_locale);
    voice.supported_locales.iter().any(|candidate| {
        *candidate == normalized_locale || locale_language_prefix(candidate) == language
    })
}

fn emit_symbol(out: &mut Vec<i16>, ch: char, index: usize, total: usize, voice: &VoiceProfile) {
    let lowercase = ch.to_ascii_lowercase();
    match lowercase {
        'a' => append_vowel(out, [850.0, 1610.0, 2500.0], 96, index, total, voice),
        'e' => append_vowel(out, [530.0, 1840.0, 2480.0], 92, index, total, voice),
        'i' | 'y' => append_vowel(out, [270.0, 2290.0, 3010.0], 88, index, total, voice),
        'o' => append_vowel(out, [570.0, 840.0, 2410.0], 100, index, total, voice),
        'u' => append_vowel(out, [300.0, 870.0, 2240.0], 108, index, total, voice),
        'm' | 'n' => append_voiced_consonant(out, [320.0, 1100.0, 2400.0], 70, voice),
        'l' | 'r' | 'w' => append_voiced_consonant(out, [380.0, 1400.0, 2500.0], 66, voice),
        's' | 'z' | 'f' | 'v' | 'h' => append_fricative(out, lowercase, 64, voice),
        'p' | 'b' | 't' | 'd' | 'k' | 'g' => append_plosive(out, lowercase, 58, voice),
        ' ' | '\t' | '\n' => append_silence(out, 44),
        '.' | ',' | ';' | ':' => append_silence(out, 80),
        '!' | '?' => {
            append_vowel(out, [530.0, 1840.0, 2480.0], 70, index, total, voice);
            append_silence(out, 90);
        }
        _ if lowercase.is_ascii_alphanumeric() => {
            append_voiced_consonant(out, [420.0, 1500.0, 2600.0], 54, voice)
        }
        _ => append_silence(out, 28),
    }
}

fn append_vowel(
    out: &mut Vec<i16>,
    base_formants: [f32; 3],
    duration_ms: usize,
    index: usize,
    total: usize,
    voice: &VoiceProfile,
) {
    let sample_count = ms_to_samples(duration_ms);
    let pitch_span = (voice.pitch_high_hz - voice.pitch_low_hz).max(12.0);
    let contour = if total <= 1 {
        0.5
    } else {
        index as f32 / (total.saturating_sub(1)) as f32
    };
    let base_pitch = voice.pitch_low_hz + pitch_span * (0.38 + 0.2 * contour);
    let mut source_phase = 0.0f32;
    let mut formant_phase = [0.0f32; 3];
    let mut noise = 0x53a9_1c2d_u32.wrapping_add(index as u32 * 7919);
    for n in 0..sample_count {
        let env = envelope(n, sample_count, 0.08, 0.16);
        let jitter = voice.roughness * 4.0 * (lcg_signed(&mut noise) * 0.25);
        let pitch = (base_pitch + jitter).max(70.0);
        source_phase += 2.0 * PI * pitch / TTS_SAMPLE_RATE as f32;
        let glottal = 0.7 * sinf(source_phase)
            + 0.2 * sinf(source_phase * 2.0)
            + 0.1 * sinf(source_phase * 3.0);
        let mut formant_mix = 0.0f32;
        for lane in 0..3 {
            let freq = base_formants[lane] * voice.formant_freq_scale[lane];
            formant_phase[lane] += 2.0 * PI * freq / TTS_SAMPLE_RATE as f32;
            let lane_amp = [0.58f32, 0.28, 0.14][lane] * voice.formant_gain_scale[lane];
            formant_mix += lane_amp * sinf(formant_phase[lane]);
        }
        let breath = lcg_signed(&mut noise) * voice.roughness * 0.12;
        let sample = env * ((glottal * voice.voicing_gain * 0.42) + formant_mix * 0.68 + breath);
        out.push(float_to_pcm16(sample));
    }
}

fn append_voiced_consonant(
    out: &mut Vec<i16>,
    base_formants: [f32; 3],
    duration_ms: usize,
    voice: &VoiceProfile,
) {
    let sample_count = ms_to_samples(duration_ms);
    let base_pitch = ((voice.pitch_low_hz + voice.pitch_high_hz) * 0.5).max(75.0);
    let mut source_phase = 0.0f32;
    let mut formant_phase = [0.0f32; 3];
    let mut noise = 0x7f4a_193b_u32;
    for n in 0..sample_count {
        let env = envelope(n, sample_count, 0.04, 0.22);
        source_phase += 2.0 * PI * base_pitch / TTS_SAMPLE_RATE as f32;
        let voiced = 0.75 * sinf(source_phase) + 0.25 * sinf(source_phase * 2.0);
        let mut formant_mix = 0.0f32;
        for lane in 0..3 {
            formant_phase[lane] += 2.0 * PI * base_formants[lane] * voice.formant_freq_scale[lane]
                / TTS_SAMPLE_RATE as f32;
            formant_mix += [0.5f32, 0.3, 0.2][lane]
                * voice.formant_gain_scale[lane]
                * sinf(formant_phase[lane]);
        }
        let sample = env
            * (0.5 * voiced * voice.voicing_gain
                + 0.35 * formant_mix
                + 0.08 * lcg_signed(&mut noise));
        out.push(float_to_pcm16(sample));
    }
}

fn append_fricative(out: &mut Vec<i16>, consonant: char, duration_ms: usize, voice: &VoiceProfile) {
    let sample_count = ms_to_samples(duration_ms);
    let mut noise = 0x1234_5678_u32.wrapping_add(consonant as u32 * 1021);
    let color = match consonant {
        's' | 'z' => 0.75,
        'f' | 'v' => 0.48,
        _ => 0.35,
    };
    let voiced = matches!(consonant, 'z' | 'v');
    let mut phase = 0.0f32;
    let pitch = ((voice.pitch_low_hz + voice.pitch_high_hz) * 0.5).max(75.0);
    for n in 0..sample_count {
        let env = envelope(n, sample_count, 0.05, 0.3);
        let hiss = lcg_signed(&mut noise) * color;
        let voiced_component = if voiced {
            phase += 2.0 * PI * pitch / TTS_SAMPLE_RATE as f32;
            sinf(phase) * voice.voicing_gain * 0.18
        } else {
            0.0
        };
        out.push(float_to_pcm16(env * (hiss * 0.55 + voiced_component)));
    }
}

fn append_plosive(out: &mut Vec<i16>, consonant: char, duration_ms: usize, voice: &VoiceProfile) {
    let burst_count = ms_to_samples(duration_ms / 2);
    let mut noise = 0x91ab_cdef_u32.wrapping_add(consonant as u32 * 313);
    for n in 0..burst_count {
        let env = 1.0 - (n as f32 / burst_count.max(1) as f32);
        out.push(float_to_pcm16(lcg_signed(&mut noise) * env * 0.45));
    }
    append_voiced_consonant(out, [420.0, 1500.0, 2600.0], duration_ms / 2 + 20, voice);
}

fn apply_echo(samples: &mut [i16], voice: &VoiceProfile) {
    if voice.echo_delay_ms == 0 || voice.echo_decay <= 0.0 {
        return;
    }
    let delay = ms_to_samples(voice.echo_delay_ms as usize);
    if delay == 0 || delay >= samples.len() {
        return;
    }
    for index in delay..samples.len() {
        let wet = samples[index - delay] as f32 * voice.echo_decay;
        let mixed = samples[index] as f32 + wet;
        samples[index] = mixed.clamp(i16::MIN as f32, i16::MAX as f32) as i16;
    }
}

fn append_silence(out: &mut Vec<i16>, duration_ms: usize) {
    out.resize(out.len() + ms_to_samples(duration_ms), 0);
}

fn ms_to_samples(duration_ms: usize) -> usize {
    ((duration_ms as u64 * TTS_SAMPLE_RATE as u64) / 1000) as usize
}

fn envelope(position: usize, len: usize, attack_ratio: f32, release_ratio: f32) -> f32 {
    if len == 0 {
        return 0.0;
    }
    let pos = position as f32 / len as f32;
    let attack = attack_ratio.clamp(0.01, 0.45);
    let release = release_ratio.clamp(0.05, 0.45);
    if pos < attack {
        pos / attack
    } else if pos > 1.0 - release {
        (1.0 - pos) / release
    } else {
        1.0
    }
    .clamp(0.0, 1.0)
}

fn lcg_signed(seed: &mut u32) -> f32 {
    *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
    let centered = ((*seed >> 9) & 0x7fff) as f32 / 16384.0;
    centered - 1.0
}

fn float_to_pcm16(value: f32) -> i16 {
    (value.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_voice_catalog_parses_real_assets() {
        let voices = builtin_voices();
        assert!(voices.len() >= 3);
        assert!(voices.iter().any(|voice| voice.display_name == "Alicia"));
        assert!(voices
            .iter()
            .all(|voice| voice.pitch_high_hz > voice.pitch_low_hz));
        assert!(voices
            .iter()
            .all(|voice| !voice.supported_locales.is_empty()));
    }

    #[test]
    fn synthesized_speech_produces_bounded_pcm() {
        let clip = synthesize("Open settings");
        assert_eq!(clip.sample_rate, TTS_SAMPLE_RATE);
        assert_eq!(clip.channels, TTS_CHANNELS);
        assert!(!clip.pcm16_le.is_empty());
        assert_eq!(clip.pcm16_le.len() % 2, 0);
        assert!(clip.duration_ns > 0);
    }

    #[test]
    fn voice_catalog_and_selection_are_locale_aware_and_fail_closed() {
        let catalog = voice_catalog();
        assert!(catalog.iter().any(|voice| voice.id == "alicia"));
        assert!(catalog.iter().any(|voice| voice.id == "gene"
            && voice
                .supported_locales
                .iter()
                .any(|locale| locale == "en-gb")));

        assert_eq!(
            select_voice("en-GB", Some("gene")).map(|voice| voice.id),
            Ok("gene")
        );
        let selected = select_voice("en-US", None).map(|voice| voice.id);
        assert!(matches!(selected, Ok("alex") | Ok("alicia")));
        assert_eq!(
            select_voice("tr-TR", None).unwrap_err().kind,
            VoiceSelectionErrorKind::UnsupportedLocale
        );
        assert_eq!(
            select_voice("en-US", Some("unknown")).unwrap_err().kind,
            VoiceSelectionErrorKind::UnknownVoice
        );
    }
}
