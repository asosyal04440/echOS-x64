//! # Animasyon Sistemi
//!
//! Easing fonksiyonlarıyla donatılmış yüksek performanslı animasyon zaman çizelgesi.
//! Kare hızlaması (frame pacing) ile 60 FPS garantisi sunar.
//!
//! ## Easing Nedir?
//!
//! Easing, bir animasyonun zaman içinde nasıl hareket ettiğini belirleyen
//! matematiksel bir eğridir. `t` değeri 0.0 (başlangıç) ile 1.0 (bitiş)
//! arasında değişir.
//!
//! ```
//! Ease-In (Yavaş Başlar, Hızlanır):
//!  1.0 |          ╭──
//!      |        ╱
//!      |      ╱
//!      |    ╱
//!  0.0 |───╯
//!       0.0          1.0  (zaman t)
//!
//! Ease-Out (Hızlı Başlar, Yavaşlar):
//!  1.0 |    ╭────────
//!      |  ╱
//!      | ╱
//!      |╱
//!  0.0 |
//!       0.0          1.0  (zaman t)
//!
//! Ease-In-Out (Orta Noktada Maksimum Hız):
//!  1.0 |       ╭─────
//!      |     ╱
//!      |    │   <- maksimum hız burada
//!      |  ╱
//!  0.0 |──╯
//!       0.0          1.0  (zaman t)
//!
//! Linear (Sabit Hız - Doğrusal):
//!  1.0 |         ╱
//!      |       ╱
//!      |     ╱
//!      |   ╱
//!  0.0 |─╱
//!       0.0          1.0  (zaman t)
//! ```
//!
//! ## Easing Kategorileri
//!
//! - **Quad/Cubic/Quart/Quint**: Polinom eğrileri (daha yüksek üs = daha sert)
//! - **Sinusoidal**: Sin/Cos tabanlı yumuşak eğri
//! - **Exponential**: 2^x tabanlı keskin hız değişimi
//! - **Circular**: Daire yayı tabanlı eğri
//! - **Elastic**: Yay gibi titreşimli etki
//! - **Back**: Hedefin biraz ötesine gidip geri döner (overshoot)
//! - **Bounce**: Yerçekimi ile zıplama efekti

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use spin::Mutex;
use core::f32::consts::PI;
use libm::{sinf, cosf, powf, sqrtf};

// ============================================================================
// EASING TÜRLERİ
// ============================================================================

/// Animasyon için kullanılan easing (hız profili) türleri.
///
/// Her varyant üç formatta gelebilir:
/// - `EaseIn*`    : Yavaş başlar, giderek hızlanır (ivme kazanır)
/// - `EaseOut*`   : Hızlı başlar, giderek yavaşlar (frenleme)
/// - `EaseInOut*` : Yavaş başlar, ortada hızlanır, sonda yavaşlar
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EasingType {
    // Doğrusal (sabit hız — animasyon robotik görünür)
    Linear,

    // İkinci dereceden polinom: f(t) = t^2
    EaseInQuad,
    EaseOutQuad,
    EaseInOutQuad,

    // Üçüncü dereceden polinom: f(t) = t^3
    EaseInCubic,
    EaseOutCubic,
    EaseInOutCubic,

    // Dördüncü derece: f(t) = t^4  (daha sert ivme)
    EaseInQuart,
    EaseOutQuart,
    EaseInOutQuart,

    // Beşinci derece: f(t) = t^5  (en sert polinom grubu)
    EaseInQuint,
    EaseOutQuint,
    EaseInOutQuint,

    // Sinüzoidal: 1 - cos(t * π/2)  (doğal ve yumuşak)
    EaseInSine,
    EaseOutSine,
    EaseInOutSine,

    // Üstel: 2^(10t - 10)  (çok keskin, dramatik efekt)
    EaseInExpo,
    EaseOutExpo,
    EaseInOutExpo,

    // Dairesel: 1 - sqrt(1 - t^2)  (çeyrek daire yayı)
    EaseInCirc,
    EaseOutCirc,
    EaseInOutCirc,

    // Elastik: yay titreşimi — hedefin etrafında salınım
    EaseInElastic,
    EaseOutElastic,
    EaseInOutElastic,

    // Geri (Back / Overshoot): biraz geri çekilip fırlar
    EaseInBack,
    EaseOutBack,
    EaseInOutBack,

    // Zıplama (Bounce): yerçekiminde top gibi zıplar
    EaseInBounce,
    EaseOutBounce,
    EaseInOutBounce,
}

// ============================================================================
// ÖN HESAPLANMIŞ EASİNG ÖNBELLEĞI
// ============================================================================

/// Her easing eğrisi için örnekleme noktası sayısı.
/// 256 nokta = yaklaşık 0.4% hassasiyet (çift doğrusal interpolasyon ile artar).
const EASING_SAMPLES: usize = 256;

/// Easing fonksiyonlarının önceden hesaplanmış örnek tablosu.
///
/// ## Neden Önbellek?
///
/// sin/cos/pow gibi kayan nokta hesaplamaları pahalıdır.
/// Animasyon sisteminde her kare onlarca çağrı yapılabilir.
/// Tabloyu bir kez doldurup sonra doğrusal interpolasyonla
/// ara değerleri bulmak çok daha hızlıdır.
///
/// ```
/// Bellek: 31 tür × 256 f32 = 31 744 bayt (~31 KB)
/// Arama: O(1) — iki dizi erişimi + çarpım
/// ```
struct EasingCache {
    samples: [[f32; EASING_SAMPLES]; 31], // 31 easing türü
}

impl EasingCache {
    const fn new() -> Self {
        // Varsayılan olarak doğrusal başlat
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
        // Tüm easing fonksiyonu örneklerini hesapla
        for i in 0..EASING_SAMPLES {
            let t = i as f32 / (EASING_SAMPLES - 1) as f32;

            self.samples[EasingType::Linear as usize][i] = t;

            // --- İkinci Dereceden (Quadratic) ---
            // EaseIn: eğri yavaş başlar  → t^2
            // EaseOut: eğri hızlı başlar → 1-(1-t)^2
            // EaseInOut: iki parça → t<0.5 için easeIn, ötesi için easeOut
            self.samples[EasingType::EaseInQuad as usize][i] = t * t;
            self.samples[EasingType::EaseOutQuad as usize][i] = 1.0 - (1.0 - t) * (1.0 - t);
            self.samples[EasingType::EaseInOutQuad as usize][i] = {
                if t < 0.5 { 2.0 * t * t } else { 1.0 - powf(-2.0 * t + 2.0, 2.0) / 2.0 }
            };

            // --- Üçüncü Dereceden (Cubic) ---
            self.samples[EasingType::EaseInCubic as usize][i] = t * t * t;
            self.samples[EasingType::EaseOutCubic as usize][i] = 1.0 - powf(1.0 - t, 3.0);
            self.samples[EasingType::EaseInOutCubic as usize][i] = {
                if t < 0.5 { 4.0 * t * t * t } else { 1.0 - powf(-2.0 * t + 2.0, 3.0) / 2.0 }
            };

            // --- Dördüncü Derece (Quartic) ---
            self.samples[EasingType::EaseInQuart as usize][i] = t * t * t * t;
            self.samples[EasingType::EaseOutQuart as usize][i] = 1.0 - powf(1.0 - t, 4.0);
            self.samples[EasingType::EaseInOutQuart as usize][i] = {
                if t < 0.5 { 8.0 * powf(t, 4.0) } else { 1.0 - powf(-2.0 * t + 2.0, 4.0) / 2.0 }
            };

            // --- Beşinci Derece (Quintic) ---
            self.samples[EasingType::EaseInQuint as usize][i] = powf(t, 5.0);
            self.samples[EasingType::EaseOutQuint as usize][i] = 1.0 - powf(1.0 - t, 5.0);
            self.samples[EasingType::EaseInOutQuint as usize][i] = {
                if t < 0.5 { 16.0 * powf(t, 5.0) } else { 1.0 - powf(-2.0 * t + 2.0, 5.0) / 2.0 }
            };

            // --- Sinüzoidal ---
            // Formül: 1 - cos(t * π/2)
            // Doğal, insan algısına uygun yumuşak hareket
            self.samples[EasingType::EaseInSine as usize][i] = 1.0 - cosf(t * PI / 2.0);
            self.samples[EasingType::EaseOutSine as usize][i] = sinf(t * PI / 2.0);
            self.samples[EasingType::EaseInOutSine as usize][i] = {
                -(cosf(PI * t) - 1.0) / 2.0
            };

            // --- Üstel (Exponential) ---
            // Formül: 2^(10t - 10)
            // t=0 için özel durum: tam sıfır döndür (süreksizliği önler)
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

            // --- Dairesel (Circular) ---
            // Formül: 1 - sqrt(1 - t^2)  (birim dairenin çeyrek yayı)
            self.samples[EasingType::EaseInCirc as usize][i] = 1.0 - sqrtf(1.0 - t * t);
            self.samples[EasingType::EaseOutCirc as usize][i] = sqrtf(1.0 - powf(t - 1.0, 2.0));
            self.samples[EasingType::EaseInOutCirc as usize][i] = {
                if t < 0.5 {
                    (1.0 - sqrtf(1.0 - powf(2.0 * t, 2.0))) / 2.0
                } else {
                    (sqrtf(1.0 - powf(-2.0 * t + 2.0, 2.0)) + 1.0) / 2.0
                }
            };

            // --- Geri (Back / Overshoot) ---
            // C1 (1.70158) ve C3 (C1+1) sabitleri overshoot miktarını belirler.
            // Pencere açılırken biraz küçülüp sonra büyüyen "pop" efekti için kullanılır.
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

            // --- Elastik (Elastic) ---
            // Yay titresimine benzer sinüzoidal salınım.
            // C4 = 2π/3 (tek titreşim), C5 = 2π/4.5 (çoklu titreşim)
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

            // --- Zıplama (Bounce) ---
            // Parabolik segmentlerin birleşimi ile fiziksel top zıplama simülasyonu.
            // EaseOutBounce: düşüş + zıplama; EaseInBounce: simetrik tersi.
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

// Global easing önbelleği — uygulama başlangıcında bir kez hesaplanır
lazy_static::lazy_static! {
    static ref EASING_CACHE: Mutex<EasingCache> = {
        let mut cache = EasingCache::new();
        cache.compute_all();
        Mutex::new(cache)
    };
}

/// Önbellekten easing değerini döndürür (doğrusal interpolasyonla).
///
/// ## Algoritma
///
/// ```
/// t  → [0.0, 1.0] aralığına kırpılır
/// idx = t * (SAMPLES - 1)  (tam sayı kısmı)
/// frac = kesirli kısım
/// sonuç = samples[idx] + (samples[idx+1] - samples[idx]) * frac
/// ```
///
/// Bu yöntem O(1) sürede çalışır ve kayan nokta hesaplamasından
/// çok daha hızlıdır.
pub fn get_easing(easing: EasingType, t: f32) -> f32 {
    let cache = EASING_CACHE.lock();

    // t değerini [0, 1] aralığına kırp
    let t = t.max(0.0).min(1.0);

    // Örnek indeksini hesapla
    let exact = t * (EASING_SAMPLES - 1) as f32;
    let idx = exact as usize;
    let frac = exact - idx as f32;

    let idx2 = (idx + 1).min(EASING_SAMPLES - 1);

    // Örnekler arası doğrusal interpolasyon (lerp)
    let a = cache.samples[easing as usize][idx];
    let b = cache.samples[easing as usize][idx2];

    a + (b - a) * frac
}

// ============================================================================
// ANİMASYON HEDEFİ
// ============================================================================

/// Animasyonun etkilediği nesneyi tanımlayan kimlik yapısı.
///
/// Bir animasyon hedefi; bir pencere, widget veya özel nesne olabilir.
/// `target_type` ve `id` kombinasyonu eşsiz bir hedefi tanımlar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AnimationTarget {
    pub target_type: AnimationTargetType,
    pub id: u32,
}

/// Animasyon hedefi türleri — hangi özelliğin animate edileceğini gösterir.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AnimationTargetType {
    WindowPosition,  // Pencere X/Y konumu
    WindowSize,      // Pencere genişlik/yüksekliği
    WindowOpacity,   // Pencere saydamlığı (0.0=görünmez, 1.0=opak)
    WidgetPosition,  // Widget X/Y konumu
    WidgetSize,      // Widget boyutu
    WidgetOpacity,   // Widget saydamlığı
    WidgetProperty,  // Widget'a özel sayısal özellik
    Custom,          // Kullanıcı tanımlı özel hedef
}

// ============================================================================
// ANİMASYON
// ============================================================================

/// Tek bir animasyon örneği.
///
/// Bir animasyon; başlangıç değerinden bitiş değerine belirli
/// bir sürede, seçilen easing eğrisiyle geçiş yapar.
///
/// ```
/// Değer
///  end |           ╭───────
///      |         ╱         <- easing eğrisi burada bükülür
///      |       ╱
///      |     ╱
/// start|────╯
///       |delay|←── duration ──→|
///                              zaman
/// ```
pub struct Animation {
    /// Anime edilen hedef nesne
    pub target: AnimationTarget,
    /// Başlangıç değeri
    pub start_value: f32,
    /// Bitiş değeri
    pub end_value: f32,
    /// Süre (saniye cinsinden)
    pub duration: f64,
    /// Geçen süre (saniye cinsinden)
    pub elapsed: f64,
    /// Easing fonksiyonu
    pub easing: EasingType,
    /// Güncel değer (her kare güncellenir)
    pub current_value: f32,
    /// Animasyon tamamlandı mı
    pub complete: bool,
    /// Tamamlanınca çağrılacak geri çağırım fonksiyonu
    pub on_complete: Option<fn(&AnimationTarget)>,
    /// Başlamadan önceki bekleme süresi (saniye)
    pub delay: f64,
    /// Duraklatıldı mı
    pub paused: bool,
    /// Döngü modu
    pub loop_mode: LoopMode,
    /// Döngü sayısı (0 = sonsuz)
    pub loop_count: u32,
    /// Mevcut döngü sayacı
    pub current_loop: u32,
    /// Oynatma yönü (ping-pong için)
    pub forward: bool,
}

/// Animasyon döngü modları.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopMode {
    None,      // Döngüsüz — bir kez oynatılır
    Loop,      // Sonsuz tekrar — her bitişte başa döner
    PingPong,  // İleri-geri — baştan sona, sondan başa
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

    /// Pencere konum animasyonu oluşturur (EaseOutCubic easing).
    pub fn position(window_id: u32, start: f32, end: f32, duration: f64) -> Self {
        Animation::new(
            AnimationTarget { target_type: AnimationTargetType::WindowPosition, id: window_id },
            start, end, duration, EasingType::EaseOutCubic,
        )
    }

    /// Pencere boyut animasyonu oluşturur (EaseOutCubic easing).
    pub fn size(window_id: u32, start: f32, end: f32, duration: f64) -> Self {
        Animation::new(
            AnimationTarget { target_type: AnimationTargetType::WindowSize, id: window_id },
            start, end, duration, EasingType::EaseOutCubic,
        )
    }

    /// Pencere opaklık (solma/belirme) animasyonu oluşturur (EaseOutSine easing).
    pub fn opacity(window_id: u32, start: f32, end: f32, duration: f64) -> Self {
        Animation::new(
            AnimationTarget { target_type: AnimationTargetType::WindowOpacity, id: window_id },
            start, end, duration, EasingType::EaseOutSine,
        )
    }

    /// Başlama gecikmesi ekler (builder deseni).
    pub fn with_delay(mut self, delay: f64) -> Self {
        self.delay = delay;
        self
    }

    /// Döngü ayarını yapar (builder deseni).
    pub fn with_loop(mut self, mode: LoopMode, count: u32) -> Self {
        self.loop_mode = mode;
        self.loop_count = count;
        self
    }

    /// Tamamlanma geri çağırımı ekler (builder deseni).
    pub fn with_callback(mut self, callback: fn(&AnimationTarget)) -> Self {
        self.on_complete = Some(callback);
        self
    }

    /// Animasyonu bir kare ilerletir. Yeniden çizim gerekiyorsa `true` döner.
    pub fn update(&mut self, dt: f64) -> bool {
        if self.paused || self.complete {
            return false;
        }

        // Gecikme varsa tüket, animasyonu başlatma
        if self.delay > 0.0 {
            self.delay -= dt;
            return false;
        }

        // Geçen süreyi güncelle
        self.elapsed += dt;

        // Süre doldu mu kontrol et
        if self.elapsed >= self.duration {
            if self.loop_mode != LoopMode::None {
                // Döngü işleme
                self.current_loop += 1;

                if self.loop_count > 0 && self.current_loop >= self.loop_count {
                    self.complete = true;
                    self.current_value = self.end_value;
                } else {
                    // Bir sonraki döngü için sıfırla
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

        // Mevcut değeri hesapla: easing(t) ile başlangıç→bitiş arasında interpolasyon
        let t = self.elapsed / self.duration;
        let eased = get_easing(self.easing, t as f32);
        self.current_value = self.start_value + (self.end_value - self.start_value) * eased;

        true
    }

    /// Güncel animasyon değerini döndürür.
    pub fn value(&self) -> f32 {
        self.current_value
    }

    /// Animasyon aktif olarak oynatılıyor mu?
    pub fn is_running(&self) -> bool {
        !self.paused && !self.complete && self.delay <= 0.0
    }

    /// Animasyonu duraklatır (elapsed korunur).
    pub fn pause(&mut self) {
        self.paused = true;
    }

    /// Duraklatılmış animasyonu devam ettirir.
    pub fn resume(&mut self) {
        self.paused = false;
    }

    /// Animasyonu başlangıç durumuna sıfırlar.
    pub fn reset(&mut self) {
        self.elapsed = 0.0;
        self.complete = false;
        self.current_value = self.start_value;
        self.current_loop = 0;
        self.forward = true;
    }
}

// ============================================================================
// ANİMASYON ZAMAN ÇİZELGESİ (TIMELINE)
// ============================================================================

/// Birden fazla animasyonu eş zamanlı yöneten zaman çizelgesi.
///
/// ```
/// Zaman çizelgesi yapısı:
///
///  Animasyon #1: Pencere A konumu  [=====>          ]
///  Animasyon #2: Pencere B opaklığı [  ======>       ]
///  Animasyon #3: Dock büyütmesi     [        ==>     ]
///                                   |
///                              update(dt) çağrısı hepsini ilerletir
/// ```
pub struct AnimationTimeline {
    /// Aktif animasyon listesi
    animations: Vec<Animation>,
    /// Hedef kare zamanı (saniye)
    frame_time: f64,
    /// Toplam geçen süre
    total_time: f64,
    /// Sıradaki animasyon kimlik sayacı
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

    /// Zaman çizelgesine yeni animasyon ekler; kimliğini döndürür.
    pub fn add(&mut self, animation: Animation) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.animations.push(animation);
        id
    }

    /// Belirtilen hedefe ait animasyonları kaldırır.
    pub fn remove(&mut self, target: &AnimationTarget) {
        self.animations.retain(|a| a.target != *target);
    }

    /// Tüm animasyonları temizler.
    pub fn clear(&mut self) {
        self.animations.clear();
    }

    /// Tüm animasyonları `dt` saniye ilerletir.
    /// En az bir animasyon güncellendiyse `true` (yeniden çizim gerekir) döner.
    pub fn update(&mut self, dt: f64) -> bool {
        let mut needs_redraw = false;

        self.total_time += dt;

        for animation in &mut self.animations {
            if animation.update(dt) {
                needs_redraw = true;
            }
        }

        // Tamamlanan animasyonları listeden temizle
        self.animations.retain(|a| !a.complete);

        needs_redraw
    }

    /// Belirtilen hedefin mevcut animasyon değerini döndürür.
    pub fn get_value(&self, target: &AnimationTarget) -> Option<f32> {
        for animation in &self.animations {
            if animation.target == *target {
                return Some(animation.current_value);
            }
        }
        None
    }

    /// Aktif olarak oynayan animasyon var mı?
    pub fn is_animating(&self) -> bool {
        self.animations.iter().any(|a| a.is_running())
    }

    /// Zaman çizelgesindeki toplam animasyon sayısı.
    pub fn count(&self) -> usize {
        self.animations.len()
    }

    /// Tüm animasyonları duraklatır.
    pub fn pause_all(&mut self) {
        for animation in &mut self.animations {
            animation.pause();
        }
    }

    /// Tüm duraklatılmış animasyonları devam ettirir.
    pub fn resume_all(&mut self) {
        for animation in &mut self.animations {
            animation.resume();
        }
    }
}

// ============================================================================
// KARE HIZLANDIRICI (FRAME PACER)
// ============================================================================

/// Tutarlı kare zamanlaması için kare hızlandırıcı.
///
/// ## Çalışma Prensibi
///
/// ```
/// Kare döngüsü:
///
///  begin_frame()
///       |
///       ├─ Giriş işle
///       ├─ Mantık güncelle
///       └─ Ekrana çiz
///       |
///  end_frame()
///       |
///       ├─ Kalan süreyi hesapla
///       ├─ Büyük gecikme → uyku (sleep_ns)
///       └─ Küçük gecikme → döngü bekle (spin_loop) [hassas zamanlama]
/// ```
///
/// Birikmiş hata düzeltmesi (accumulated_error) ile
/// uzun vadede hedef FPS'e yakın kalmayı sağlar.
pub struct FramePacer {
    /// Hedef kare süresi (nanosaniye)
    target_frame_ns: u64,
    /// Son kare zaman damgası
    last_frame_ns: u64,
    /// Birikmiş zamanlama hatası
    accumulated_error_ns: i64,
    /// Toplam kare sayısı
    frame_count: u64,
    /// Ortalama hesabı için toplam kare süresi
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

    /// Kare başlangıcında çağrılır — zaman damgasını kaydeder.
    pub fn begin_frame(&mut self) {
        self.last_frame_ns = get_time_ns();
    }

    /// Kare sonunda çağrılır — bir sonraki kareye kadar bekler.
    pub fn end_frame(&mut self) {
        let now = get_time_ns();
        let elapsed = now - self.last_frame_ns;

        // Hata düzeltmesi ile hedef süreyi hesapla
        let target = self.target_frame_ns as i64 - self.accumulated_error_ns;
        let remaining = target - elapsed as i64;

        if remaining > 0 {
            // Büyük beklemeler → işletim sistemi uykusu (CPU boşa harcama)
            if remaining > 2_000_000 { // > 2ms
                sleep_ns((remaining - 1_000_000) as u64);
            }

            // Hassas zamanlama → aktif döngü bekleme (spin loop)
            while get_time_ns() - self.last_frame_ns < target as u64 {
                core::hint::spin_loop();
            }
        }

        // İstatistikleri güncelle
        let actual_elapsed = get_time_ns() - self.last_frame_ns;
        self.accumulated_error_ns = actual_elapsed as i64 - target;

        // Hatanın yarım kare sınırını aşmasını önle (frenaway koruması)
        self.accumulated_error_ns = self.accumulated_error_ns
            .max(-(self.target_frame_ns as i64) / 2)
            .min(self.target_frame_ns as i64 / 2);

        self.frame_count += 1;
        self.total_frame_time_ns += actual_elapsed;
    }

    /// Ortalama kare süresini milisaniye cinsinden döndürür.
    pub fn avg_frame_time_ms(&self) -> f64 {
        if self.frame_count == 0 {
            return 0.0;
        }
        self.total_frame_time_ns as f64 / self.frame_count as f64 / 1_000_000.0
    }

    /// Anlık FPS değerini döndürür.
    pub fn current_fps(&self) -> f64 {
        let avg = self.avg_frame_time_ms();
        if avg > 0.0 {
            1000.0 / avg
        } else {
            0.0
        }
    }

    /// Toplam kare sayısını döndürür.
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }
}

// ============================================================================
// ZAMAN YARDIMCI FONKSİYONLARI
// ============================================================================

/// Mevcut zamanı nanosaniye cinsinden döndürür (TSC veya HPET kullanır).
pub fn get_time_ns() -> u64 {
    // TSC (Time Stamp Counter) veya HPET donanım sayacı kullan
    crate::cpu::tsc::read_ns()
}

/// Belirtilen nanosaniye süre kadar uyur (milisaniyeye çevirir).
pub fn sleep_ns(ns: u64) {
    // Milisaniyeye çevirerek zamanlayıcı uykusunu çağır
    let ms = ns / 1_000_000;
    if ms > 0 {
        crate::task::scheduler::sleep(ms as usize);
    }
}

// ============================================================================
// GLOBAL ANİMASYON DURUMU
// ============================================================================

lazy_static::lazy_static! {
    /// Global animasyon zaman çizelgesi — tüm GUI animasyonları burada yönetilir
    static ref ANIMATION_TIMELINE: Mutex<AnimationTimeline> = Mutex::new(AnimationTimeline::new(60));
    /// Global kare hızlandırıcı — 60 FPS hedefiyle kare zamanlaması yapar
    static ref FRAME_PACER: Mutex<FramePacer> = Mutex::new(FramePacer::new(60));
}

/// Global zaman çizelgesine animasyon ekler; animasyon kimliğini döndürür.
pub fn add_animation(animation: Animation) -> u64 {
    ANIMATION_TIMELINE.lock().add(animation)
}

/// Global zaman çizelgesini `dt` saniye ilerletir; yeniden çizim gerekiyorsa `true` döner.
pub fn update_animations(dt: f64) -> bool {
    ANIMATION_TIMELINE.lock().update(dt)
}

/// Belirtilen hedefin global zaman çizelgesindeki güncel animasyon değerini döndürür.
pub fn get_animation_value(target: &AnimationTarget) -> Option<f32> {
    ANIMATION_TIMELINE.lock().get_value(target)
}

/// Kare başlangıcını global kare hızlandırıcıya bildirir.
pub fn begin_frame() {
    FRAME_PACER.lock().begin_frame();
}

/// Kare sonunu bildirir; hedef FPS'e ulaşmak için gerekirse bekler.
pub fn end_frame() {
    FRAME_PACER.lock().end_frame();
}

/// Anlık kare hızını (FPS) döndürür.
pub fn get_fps() -> f64 {
    FRAME_PACER.lock().current_fps()
}

/// Animasyon sistemini başlatır (60 FPS hedefi).
pub fn init() {
    crate::serial_println!("[ANIM] Animasyon sistemi başlatıldı (60 FPS hedef)");
}
