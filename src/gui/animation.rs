//! # Animasyon Sistemi
//!
//! Yumuşatma fonksiyonlu yüksek performanslı animasyon zaman çizelgesi.
//! Kare hızlandırma ile 60 FPS garantili çalışma.
//!
//! ## Mimari
//! - `EasingType`: 31 farklı yumuşatma eğrisi (Quadratic'ten Bounce'a)
//! - `EasingCache`: Tüm örnekler önceden hesaplanır → çalışma zamanında sıfır işlem
//! - `Animation`: Tek bir animasyon örneği (başlangıç/bitiş değeri, süre, döngü)
//! - `AnimationTimeline`: Tüm aktif animasyonları yönetir
//! - `FramePacer`: TSC tabanlı hassas kare zamanlaması (hedef: 60 FPS)

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use spin::Mutex;
use core::f32::consts::PI;
use libm::{sinf, cosf, powf, sqrtf};

// ============================================================================
// YUMUŞATMA TÜRLERİ
// ============================================================================

/// Yumuşatma fonksiyonu türü
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EasingType {
    // Doğrusal
    Linear,

    // İkinci Dereceden (Quadratic)
    EaseInQuad,
    EaseOutQuad,
    EaseInOutQuad,

    // Üçüncü Dereceden (Cubic)
    EaseInCubic,
    EaseOutCubic,
    EaseInOutCubic,

    // Dördüncü Dereceden (Quartic)
    EaseInQuart,
    EaseOutQuart,
    EaseInOutQuart,

    // Beşinci Dereceden (Quintic)
    EaseInQuint,
    EaseOutQuint,
    EaseInOutQuint,

    // Sinüzoidal
    EaseInSine,
    EaseOutSine,
    EaseInOutSine,

    // Üstel (Exponential)
    EaseInExpo,
    EaseOutExpo,
    EaseInOutExpo,

    // Dairesel (Circular)
    EaseInCirc,
    EaseOutCirc,
    EaseInOutCirc,

    // Elastik (Elastic)
    EaseInElastic,
    EaseOutElastic,
    EaseInOutElastic,

    // Geri (Back)
    EaseInBack,
    EaseOutBack,
    EaseInOutBack,

    // Sekme (Bounce)
    EaseInBounce,
    EaseOutBounce,
    EaseInOutBounce,
}

// ============================================================================
// ÖNCEDENhesaplanmiş YUMUŞATMA ÖNBELLEĞİ
// ============================================================================

/// Yumuşatma eğrisi başına örnek nokta sayısı
const EASING_SAMPLES: usize = 256;

/// Önceden hesaplanmış yumuşatma fonksiyonu örnekleri.
/// Yaygın yumuşatma fonksiyonları için çalışma zamanı hesaplamalarından kaçınır.
struct EasingCache {
    samples: [[f32; EASING_SAMPLES]; 31], // 31 yumuşatma türü
}

impl EasingCache {
    const fn new() -> Self {
        // Varsayılan olarak doğrusal (linear) ile başlat
        let mut samples = [[0.0f32; EASING_SAMPLES]; 31];

        let mut i = 0;
        while i < EASING_SAMPLES {
            let t = i as f32 / (EASING_SAMPLES - 1) as f32;
            samples[EasingType::Linear as usize][i] = t;
            i += 1;
        }

        EasingCache { samples }
    }

    fn compute_all(&mut self) {
        // Tüm yumuşatma fonksiyonu örneklerini hesapla
        for i in 0..EASING_SAMPLES {
            let t = i as f32 / (EASING_SAMPLES - 1) as f32;

            self.samples[EasingType::Linear as usize][i] = t;

            // İkinci Dereceden (Quadratic)
            self.samples[EasingType::EaseInQuad as usize][i] = t * t;
            self.samples[EasingType::EaseOutQuad as usize][i] = 1.0 - (1.0 - t) * (1.0 - t);
            self.samples[EasingType::EaseInOutQuad as usize][i] = {
                if t < 0.5 { 2.0 * t * t } else { 1.0 - powf(-2.0 * t + 2.0, 2.0) / 2.0 }
            };

            // Üçüncü Dereceden (Cubic)
            self.samples[EasingType::EaseInCubic as usize][i] = t * t * t;
            self.samples[EasingType::EaseOutCubic as usize][i] = 1.0 - powf(1.0 - t, 3.0);
            self.samples[EasingType::EaseInOutCubic as usize][i] = {
                if t < 0.5 { 4.0 * t * t * t } else { 1.0 - powf(-2.0 * t + 2.0, 3.0) / 2.0 }
            };

            // Dördüncü Dereceden (Quartic)
            self.samples[EasingType::EaseInQuart as usize][i] = t * t * t * t;
            self.samples[EasingType::EaseOutQuart as usize][i] = 1.0 - powf(1.0 - t, 4.0);
            self.samples[EasingType::EaseInOutQuart as usize][i] = {
                if t < 0.5 { 8.0 * powf(t, 4.0) } else { 1.0 - powf(-2.0 * t + 2.0, 4.0) / 2.0 }
            };

            // Beşinci Dereceden (Quintic)
            self.samples[EasingType::EaseInQuint as usize][i] = powf(t, 5.0);
            self.samples[EasingType::EaseOutQuint as usize][i] = 1.0 - powf(1.0 - t, 5.0);
            self.samples[EasingType::EaseInOutQuint as usize][i] = {
                if t < 0.5 { 16.0 * powf(t, 5.0) } else { 1.0 - powf(-2.0 * t + 2.0, 5.0) / 2.0 }
            };

            // Sinüzoidal
            self.samples[EasingType::EaseInSine as usize][i] = 1.0 - cosf(t * PI / 2.0);
            self.samples[EasingType::EaseOutSine as usize][i] = sinf(t * PI / 2.0);
            self.samples[EasingType::EaseInOutSine as usize][i] = {
                -(cosf(PI * t) - 1.0) / 2.0
            };

            // Üstel (Exponential)
            self.samples[EasingType::EaseInExpo as usize][i] = {
                if t == 0.0 { 0.0 } else { powf(2.0_f32, 10.0 * t - 10.0) }
            };
            self.samples[EasingType::EaseOutExpo as usize][i] = {
                if t == 1.0 { 1.0 } else { 1.0 - powf(2.0_f32, -10.0 * t) }
            };
            self.samples[EasingType::EaseInOutExpo as usize][i] = {
                if t == 0.0 { 0.0 }
                else if t == 1.0 { 1.0 }
                else if t < 0.5 { powf(2.0_f32, 20.0 * t - 10.0) / 2.0 }
                else { (2.0 - powf(2.0_f32, -20.0 * t + 10.0)) / 2.0 }
            };

            // Dairesel (Circular)
            self.samples[EasingType::EaseInCirc as usize][i] = 1.0 - sqrtf(1.0 - t * t);
            self.samples[EasingType::EaseOutCirc as usize][i] = sqrtf(1.0 - powf(t - 1.0, 2.0));
            self.samples[EasingType::EaseInOutCirc as usize][i] = {
                if t < 0.5 {
                    (1.0 - sqrtf(1.0 - powf(2.0 * t, 2.0))) / 2.0
                } else {
                    (sqrtf(1.0 - powf(-2.0 * t + 2.0, 2.0)) + 1.0) / 2.0
                }
            };

            // Geri (Back)
            const C1: f32 = 1.70158;
            const C2: f32 = C1 * 1.525;
            const C3: f32 = C1 + 1.0;

            self.samples[EasingType::EaseInBack as usize][i] = {
                C3 * t * t * t - C1 * t * t
            };
            self.samples[EasingType::EaseOutBack as usize][i] = {
                1.0 + C3 * powf(t - 1.0, 3.0) + C1 * powf(t - 1.0, 2.0)
            };
            self.samples[EasingType::EaseInOutBack as usize][i] = {
                if t < 0.5 {
                    (powf(2.0 * t, 2.0) * ((C2 + 1.0) * 2.0 * t - C2)) / 2.0
                } else {
                    (powf(2.0 * t - 2.0, 2.0) * ((C2 + 1.0) * (t * 2.0 - 2.0) + C2) + 2.0) / 2.0
                }
            };

            // Elastik (Elastic)
            const C4: f32 = (2.0 * PI) / 3.0;
            const C5: f32 = (2.0 * PI) / 4.5;

            self.samples[EasingType::EaseInElastic as usize][i] = {
                if t == 0.0 { 0.0 }
                else if t == 1.0 { 1.0 }
                else {
                    -powf(2.0_f32, 10.0 * t - 10.0) * sinf((t * 10.0 - 10.75) * C4)
                }
            };
            self.samples[EasingType::EaseOutElastic as usize][i] = {
                if t == 0.0 { 0.0 }
                else if t == 1.0 { 1.0 }
                else {
                    powf(2.0_f32, -10.0 * t) * sinf((t * 10.0 - 0.75) * C4) + 1.0
                }
            };
            self.samples[EasingType::EaseInOutElastic as usize][i] = {
                if t == 0.0 { 0.0 }
                else if t == 1.0 { 1.0 }
                else if t < 0.5 {
                    -(powf(2.0_f32, 20.0 * t - 10.0) * sinf((20.0 * t - 11.125) * C5)) / 2.0
                } else {
                    (powf(2.0_f32, -20.0 * t + 10.0) * sinf((20.0 * t - 11.125) * C5)) / 2.0 + 1.0
                }
            };

            // Sekme (Bounce)
            self.samples[EasingType::EaseOutBounce as usize][i] = {
                const N1: f32 = 7.5625;
                const D1: f32 = 2.75;

                if t < 1.0 / D1 {
                    N1 * t * t
                } else if t < 2.0 / D1 {
                    N1 * powf(t - 1.5 / D1, 2.0) + 0.75
                } else if t < 2.5 / D1 {
                    N1 * powf(t - 2.25 / D1, 2.0) + 0.9375
                } else {
                    N1 * powf(t - 2.625 / D1, 2.0) + 0.984375
                }
            };
            self.samples[EasingType::EaseInBounce as usize][i] = {
                1.0 - self.samples[EasingType::EaseOutBounce as usize][(EASING_SAMPLES - 1 - i) as usize]
            };
            self.samples[EasingType::EaseInOutBounce as usize][i] = {
                if t < 0.5 {
                    (1.0 - self.samples[EasingType::EaseOutBounce as usize][(EASING_SAMPLES - 1 - 2 * i as usize).min(EASING_SAMPLES - 1)]) / 2.0
                } else {
                    (1.0 + self.samples[EasingType::EaseOutBounce as usize][(2 * i as usize - EASING_SAMPLES).max(0)]) / 2.0
                }
            };
        }
    }
}

// Global yumuşatma önbelleği
lazy_static::lazy_static! {
    static ref EASING_CACHE: Mutex<EasingCache> = {
        let mut cache = EasingCache::new();
        cache.compute_all();
        Mutex::new(cache)
    };
}

/// Önbellekten yumuşatma değerini al (aradeğerleme ile)
pub fn get_easing(easing: EasingType, t: f32) -> f32 {
    let cache = EASING_CACHE.lock();

    // t'yi [0, 1] aralığına kısıt
    let t = t.max(0.0).min(1.0);

    // Örnek indeksini al
    let exact = t * (EASING_SAMPLES - 1) as f32;
    let idx = exact as usize;
    let frac = exact - idx as f32;

    let idx2 = (idx + 1).min(EASING_SAMPLES - 1);

    // Örnekler arasında doğrusal aradeğerleme
    let a = cache.samples[easing as usize][idx];
    let b = cache.samples[easing as usize][idx2];

    a + (b - a) * frac
}

// ============================================================================
// ANİMASYON HEDEFİ
// ============================================================================

/// Animasyon hedef tanımlayıcısı
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AnimationTarget {
    pub target_type: AnimationTargetType,
    pub id: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AnimationTargetType {
    WindowPosition,
    WindowSize,
    WindowOpacity,
    WidgetPosition,
    WidgetSize,
    WidgetOpacity,
    WidgetProperty,
    Custom,
}

// ============================================================================
// ANİMASYON
// ============================================================================

/// Tek animasyon örneği
pub struct Animation {
    /// Animasyonun uygulandığı hedef
    pub target: AnimationTarget,
    /// Başlangıç değeri
    pub start_value: f32,
    /// Bitiş değeri
    pub end_value: f32,
    /// Saniye cinsinden süre
    pub duration: f64,
    /// Saniye cinsinden geçen süre
    pub elapsed: f64,
    /// Yumuşatma fonksiyonu
    pub easing: EasingType,
    /// Mevcut değer
    pub current_value: f32,
    /// Animasyon tamamlandı mı
    pub complete: bool,
    /// Tamamlandığında çağrılacak geri çağırma
    pub on_complete: Option<fn(&AnimationTarget)>,
    /// Başlamadan önceki gecikme
    pub delay: f64,
    /// Duraklatıldı mı
    pub paused: bool,
    /// Döngü modu
    pub loop_mode: LoopMode,
    /// Döngü sayısı (0 = sonsuz)
    pub loop_count: u32,
    /// Mevcut döngü yineleme
    pub current_loop: u32,
    /// Oynatma yönü (ping-pong için)
    pub forward: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopMode {
    None,
    Loop,
    PingPong,
}

impl Animation {
    pub fn new(
        target: AnimationTarget,
        start_value: f32,
        end_value: f32,
        duration: f64,
        easing: EasingType,
    ) -> Self {
        Animation {
            target,
            start_value,
            end_value,
            duration,
            elapsed: 0.0,
            easing,
            current_value: start_value,
            complete: false,
            on_complete: None,
            delay: 0.0,
            paused: false,
            loop_mode: LoopMode::None,
            loop_count: 0,
            current_loop: 0,
            forward: true,
        }
    }

    /// Konum animasyonu oluştur
    pub fn position(window_id: u32, start: f32, end: f32, duration: f64) -> Self {
        Animation::new(
            AnimationTarget { target_type: AnimationTargetType::WindowPosition, id: window_id },
            start, end, duration, EasingType::EaseOutCubic,
        )
    }

    /// Boyut animasyonu oluştur
    pub fn size(window_id: u32, start: f32, end: f32, duration: f64) -> Self {
        Animation::new(
            AnimationTarget { target_type: AnimationTargetType::WindowSize, id: window_id },
            start, end, duration, EasingType::EaseOutCubic,
        )
    }

    /// Opaklık animasyonu oluştur (soldurma)
    pub fn opacity(window_id: u32, start: f32, end: f32, duration: f64) -> Self {
        Animation::new(
            AnimationTarget { target_type: AnimationTargetType::WindowOpacity, id: window_id },
            start, end, duration, EasingType::EaseOutSine,
        )
    }

    /// Gecikme ayarla
    pub fn with_delay(mut self, delay: f64) -> Self {
        self.delay = delay;
        self
    }

    /// Döngü modu ayarla
    pub fn with_loop(mut self, mode: LoopMode, count: u32) -> Self {
        self.loop_mode = mode;
        self.loop_count = count;
        self
    }

    /// Geri çağırma fonksiyonu ayarla
    pub fn with_callback(mut self, callback: fn(&AnimationTarget)) -> Self {
        self.on_complete = Some(callback);
        self
    }

    /// Animasyonu güncelle
    pub fn update(&mut self, dt: f64) -> bool {
        if self.paused || self.complete {
            return false;
        }

        // Gecikmeyi işle
        if self.delay > 0.0 {
            self.delay -= dt;
            return false;
        }

        // Geçen süreyi güncelle
        self.elapsed += dt;

        // Tamamlanıp tamamlanmadığını kontrol et
        if self.elapsed >= self.duration {
            if self.loop_mode != LoopMode::None {
                // Döngüyü işle
                self.current_loop += 1;

                if self.loop_count > 0 && self.current_loop >= self.loop_count {
                    self.complete = true;
                    self.current_value = self.end_value;
                } else {
                    // Sonraki döngü için sıfırla
                    if self.loop_mode == LoopMode::PingPong {
                        self.forward = !self.forward;
                        if self.forward {
                            self.elapsed = 0.0;
                            self.start_value = self.start_value;
                        } else {
                            core::mem::swap(&mut self.start_value, &mut self.end_value);
                            self.elapsed = 0.0;
                        }
                    } else {
                        self.elapsed = 0.0;
                    }
                }
            } else {
                self.complete = true;
                self.current_value = self.end_value;
            }

            if self.complete {
                if let Some(callback) = self.on_complete {
                    callback(&self.target);
                }
            }

            return true;
        }

        // Mevcut değeri hesapla
        let t = self.elapsed / self.duration;
        let eased = get_easing(self.easing, t as f32);
        self.current_value = self.start_value + (self.end_value - self.start_value) * eased;

        true
    }

    /// Mevcut değeri al
    pub fn value(&self) -> f32 {
        self.current_value
    }

    /// Animasyon çalışıyor mu
    pub fn is_running(&self) -> bool {
        !self.paused && !self.complete && self.delay <= 0.0
    }

    /// Animasyonu duraklat
    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// Animasyonu devam ettir
    pub fn resume(&mut self) {
        self.paused = false;
    }

    /// Animasyonu sıfırla
    pub fn reset(&mut self) {
        self.elapsed = 0.0;
        self.complete = false;
        self.current_value = self.start_value;
        self.current_loop = 0;
        self.forward = true;
    }
}

// ============================================================================
// ANİMASYON ZAMAN ÇİZELGESİ
// ============================================================================

/// Animasyon zaman çizelgesi yöneticisi
pub struct AnimationTimeline {
    /// Aktif animasyonlar
    animations: Vec<Animation>,
    /// Saniye cinsinden kare süresi
    frame_time: f64,
    /// Toplam süre
    total_time: f64,
    /// Animasyon kimliği sayacı
    next_id: u64,
}

impl AnimationTimeline {
    pub fn new(target_fps: u32) -> Self {
        AnimationTimeline {
            animations: Vec::new(),
            frame_time: 1.0 / target_fps as f64,
            total_time: 0.0,
            next_id: 0,
        }
    }

    /// Animasyon ekle
    pub fn add(&mut self, animation: Animation) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.animations.push(animation);
        id
    }

    /// İndekse göre animasyonu kaldır
    pub fn remove(&mut self, target: &AnimationTarget) {
        self.animations.retain(|a| a.target != *target);
    }

    /// Tüm animasyonları temizle
    pub fn clear(&mut self) {
        self.animations.clear();
    }

    /// Tüm animasyonları güncelle
    pub fn update(&mut self, dt: f64) -> bool {
        let mut needs_redraw = false;

        self.total_time += dt;

        for animation in &mut self.animations {
            if animation.update(dt) {
                needs_redraw = true;
            }
        }

        // Tamamlanan animasyonları kaldır
        self.animations.retain(|a| !a.complete);

        needs_redraw
    }

    /// Hedef için animasyon değerini al
    pub fn get_value(&self, target: &AnimationTarget) -> Option<f32> {
        for animation in &self.animations {
            if animation.target == *target {
                return Some(animation.current_value);
            }
        }
        None
    }

    /// Herhangi bir animasyon çalışıyor mu kontrol et
    pub fn is_animating(&self) -> bool {
        self.animations.iter().any(|a| a.is_running())
    }

    /// Aktif animasyon sayısını al
    pub fn count(&self) -> usize {
        self.animations.len()
    }

    /// Tüm animasyonları duraklat
    pub fn pause_all(&mut self) {
        for animation in &mut self.animations {
            animation.pause();
        }
    }

    /// Tüm animasyonları devam ettir
    pub fn resume_all(&mut self) {
        for animation in &mut self.animations {
            animation.resume();
        }
    }
}

// ============================================================================
// KARE HIZLANDIRICI
// ============================================================================

/// Tutarlı kare zamanlaması için kare hızlandırıcı.
/// TSC tabanlı hassas zamanlama kullanır — çekirdek için vsync alternatifi.
pub struct FramePacer {
    /// Nanosaniye cinsinden hedef kare süresi
    target_frame_ns: u64,
    /// Son kare zaman damgası
    last_frame_ns: u64,
    /// Birikmiş zamanlama hatası
    accumulated_error_ns: i64,
    /// Kare sayısı
    frame_count: u64,
    /// Ortalama için toplam kare süresi
    total_frame_time_ns: u64,
}

impl FramePacer {
    pub fn new(target_fps: u32) -> Self {
        FramePacer {
            target_frame_ns: 1_000_000_000 / target_fps as u64,
            last_frame_ns: 0,
            accumulated_error_ns: 0,
            frame_count: 0,
            total_frame_time_ns: 0,
        }
    }

    /// Kare başlangıcı — karenin başında çağrılır
    pub fn begin_frame(&mut self) {
        self.last_frame_ns = get_time_ns();
    }

    /// Kare sonu — sonraki kare zamanlamasını bekler
    pub fn end_frame(&mut self) {
        let now = get_time_ns();
        let elapsed = now - self.last_frame_ns;

        // Hata düzeltmeli hedefi hesapla
        let target = self.target_frame_ns as i64 - self.accumulated_error_ns;
        let remaining = target - elapsed as i64;

        if remaining > 0 {
            // Kalan sürenin büyük kısmı için uyu
            if remaining > 2_000_000 { // > 2ms
                sleep_ns((remaining - 1_000_000) as u64);
            }

            // Hassas zamanlama için döngü beklet
            while get_time_ns() - self.last_frame_ns < target as u64 {
                core::hint::spin_loop();
            }
        }

        // İstatistikleri güncelle
        let actual_elapsed = get_time_ns() - self.last_frame_ns;
        self.accumulated_error_ns = actual_elapsed as i64 - target;

        // Kontrolden çıkmayı önlemek için hatayı sınırla
        self.accumulated_error_ns = self.accumulated_error_ns
            .max(-(self.target_frame_ns as i64) / 2)
            .min(self.target_frame_ns as i64 / 2);

        self.frame_count += 1;
        self.total_frame_time_ns += actual_elapsed;
    }

    /// Ortalama kare süresini milisaniye cinsinden al
    pub fn avg_frame_time_ms(&self) -> f64 {
        if self.frame_count == 0 {
            return 0.0;
        }
        self.total_frame_time_ns as f64 / self.frame_count as f64 / 1_000_000.0
    }

    /// Mevcut FPS değerini al
    pub fn current_fps(&self) -> f64 {
        let avg = self.avg_frame_time_ms();
        if avg > 0.0 {
            1000.0 / avg
        } else {
            0.0
        }
    }

    /// Kare sayısını al
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }
}

// ============================================================================
// ZAMAN YARDIMCILARI
// ============================================================================

/// Mevcut zamanı nanosaniye cinsinden al
pub fn get_time_ns() -> u64 {
    // TSC veya HPET kullan
    crate::cpu::tsc::read_ns()
}

/// Belirtilen nanosaniye kadar uyu
pub fn sleep_ns(ns: u64) {
    // Uyku için milisaniyeye dönüştür
    let ms = ns / 1_000_000;
    if ms > 0 {
        crate::task::scheduler::sleep(ms as usize);
    }
}

// ============================================================================
// GLOBAL ANİMASYON DURUMU
// ============================================================================

lazy_static::lazy_static! {
    static ref ANIMATION_TIMELINE: Mutex<AnimationTimeline> = Mutex::new(AnimationTimeline::new(60));
    static ref FRAME_PACER: Mutex<FramePacer> = Mutex::new(FramePacer::new(60));
}

/// Global zaman çizelgesine animasyon ekle
pub fn add_animation(animation: Animation) -> u64 {
    ANIMATION_TIMELINE.lock().add(animation)
}

/// Global zaman çizelgesini güncelle
pub fn update_animations(dt: f64) -> bool {
    ANIMATION_TIMELINE.lock().update(dt)
}

/// Animasyon değerini al
pub fn get_animation_value(target: &AnimationTarget) -> Option<f32> {
    ANIMATION_TIMELINE.lock().get_value(target)
}

/// Kare başlangıcı
pub fn begin_frame() {
    FRAME_PACER.lock().begin_frame();
}

/// Kare hızlandırmalı kare sonu
pub fn end_frame() {
    FRAME_PACER.lock().end_frame();
}

/// Mevcut FPS değerini al
pub fn get_fps() -> f64 {
    FRAME_PACER.lock().current_fps()
}

/// Animasyon sistemini başlat
pub fn init() {
    crate::serial_println!("[ANIM] Animasyon sistemi başlatıldı (hedef: 60 FPS)");
}
