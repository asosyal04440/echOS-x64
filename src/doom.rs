//! # echOS Doom Portu
//!
//! DoomGeneric implementasyonu — echOS üzerinde çalışan Doom oyun motoru.
//! Orijinal kaynak: https://github.com/ozkl/doomgeneric
//!
//! ## Doom Hakkında
//! Doom, 1993 yılında id Software tarafından geliştirilen efsanevi FPS oyunudur.
//! "DoomGeneric" projesi, Doom'u minimum platform bağımlılığıyla taşımanın
//! yolunu sunar. Bu modül, echOS'a özgü grafik ve ses arka uçlarını implemente eder.

use alloc::vec::Vec;
use alloc::vec;
use alloc::string::String;
use alloc::boxed::Box;
use spin::Mutex;

// ============================================================================
// DOOM SABİTLERİ
// ============================================================================

/// Ekran genişliği — piksel cinsinden (Doom orijinal 320x200 çözünürlük kullanır)
pub const SCREEN_WIDTH: usize = 320;
/// Ekran yüksekliği — piksel cinsinden
pub const SCREEN_HEIGHT: usize = 200;
/// Ekran satır genişliği (pitch) — her satır için bayt sayısı (4 bayt = ARGB)
pub const SCREEN_PITCH: usize = 320 * 4;

// ============================================================================
// DOOM TÜRLERİ
// ============================================================================

/// Piksel renk formatı (ARGB).
/// A=Alpha (şeffaflık), R=Kırmızı, G=Yeşil, B=Mavi.
/// Her kanal 8 bit (0-255).
#[derive(Clone, Copy, Debug, Default)]
pub struct Pixel {
    pub a: u8,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Pixel {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Pixel { a: 255, r, g, b }
    }

    /// 32-bit u32 renk değerinden Pixel oluşturur.
    /// Bit kaydırma ile her kanalı ayıklar: AARRGGBB formatı.
    pub fn from_u32(color: u32) -> Self {
        Pixel {
            a: ((color >> 24) & 0xFF) as u8,
            r: ((color >> 16) & 0xFF) as u8,
            g: ((color >> 8) & 0xFF) as u8,
            b: (color & 0xFF) as u8,
        }
    }

    /// Pixel'i 32-bit u32 renk değerine dönüştürür.
    pub fn to_u32(&self) -> u32 {
        ((self.a as u32) << 24) | ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
    }
}

/// Doom tuş kodları.
/// PS/2 klavye tarama kodlarını Doom'un anlayacağı tuş değerlerine eşler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DoomKey {
    Left,
    Right,
    Up,
    Down,
    Enter,
    Escape,
    Space,
    Tab,
    Shift,
    Ctrl,
    Alt,
    Key0,
    Key1,
    Key2,
    Key3,
    Key4,
    Key5,
    Key6,
    Key7,
    Key8,
    Key9,
    KeyA,
    KeyB,
    KeyC,
    KeyD,
    KeyE,
    KeyF,
    KeyG,
    KeyH,
    KeyI,
    KeyJ,
    KeyK,
    KeyL,
    KeyM,
    KeyN,
    KeyO,
    KeyP,
    KeyQ,
    KeyR,
    KeyS,
    KeyT,
    KeyU,
    KeyV,
    KeyW,
    KeyX,
    KeyY,
    KeyZ,
    Unknown,
}

/// Fare düğmeleri.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DoomMouseButton {
    Left,
    Right,
    Middle,
}

/// Doom giriş olayı (event).
/// Oyun motoru bu olayları işleyerek oyun durumunu günceller.
#[derive(Clone, Debug)]
pub enum DoomEvent {
    KeyDown(DoomKey),
    KeyUp(DoomKey),
    MouseDown(DoomMouseButton, i32, i32),
    MouseUp(DoomMouseButton, i32, i32),
    MouseMove(i32, i32),
    Quit,
}

// ============================================================================
// DOOM KARE TAMPONU (FRAMEBUFFER)
// ============================================================================

/// Doom render çıktısı için kare tamponu (framebuffer).
/// Oyun motoru, her kareyi bu tampona çizer; ardından gerçek ekrana kopyalanır.
#[derive(Clone)]
pub struct DoomFramebuffer {
    pub width: usize,
    pub height: usize,
    pub pitch: usize,
    pub pixels: Vec<u32>,
}

impl DoomFramebuffer {
    pub fn new() -> Self {
        DoomFramebuffer {
            width: SCREEN_WIDTH,
            height: SCREEN_HEIGHT,
            pitch: SCREEN_PITCH,
            pixels: vec![0u32; SCREEN_WIDTH * SCREEN_HEIGHT],
        }
    }

    /// Belirtilen (x, y) konumuna renk yazar.
    /// Sınır dışı erişim sessizce görmezden gelinir.
    pub fn set_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x < self.width && y < self.height {
            self.pixels[y * self.width + x] = color;
        }
    }

    /// Belirtilen (x, y) konumundaki piksel rengini döndürür.
    pub fn get_pixel(&self, x: usize, y: usize) -> u32 {
        if x < self.width && y < self.height {
            self.pixels[y * self.width + x]
        } else {
            0
        }
    }

    /// Belirtilen dikdörtgen bölgeyi tek bir renkle doldurur.
    pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        for py in y..(y + h).min(self.height) {
            for px in x..(x + w).min(self.width) {
                self.pixels[py * self.width + px] = color;
            }
        }
    }

    /// Tüm kareyi belirtilen renkle temizler.
    pub fn clear(&mut self, color: u32) {
        for pixel in &mut self.pixels {
            *pixel = color;
        }
    }

    /// Kare tamponunu gerçek ekrana kopyalar (blit işlemi).
    /// Gerçek uygulamada grafik alt sistemini kullanır.
    pub fn blit_to_screen(&self) {
        // Kare tamponunu gerçek ekrana kopyalar.
        // Gerçek uygulamada grafik alt sistemi kullanılır.
        for y in 0..self.height {
            for x in 0..self.width {
                let color = self.pixels[y * self.width + x];
                // crate::gfx::draw_pixel(x as u32, y as u32, color);
            }
        }
    }
}

impl Default for DoomFramebuffer {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// DOOM SESİ
// ============================================================================

/// Doom ses tamponu.
/// Ses örneklerini (PCM) tutar ve ses sürücüsüne iletir.
#[derive(Clone)]
pub struct DoomAudio {
    pub sample_rate: u32,
    pub channels: u8,
    pub buffer: Vec<u8>,
}

impl DoomAudio {
    pub fn new() -> Self {
        DoomAudio {
            sample_rate: 44100,
            channels: 2,
            buffer: Vec::new(),
        }
    }

    /// Ses tamponunu çalar.
    /// Gerçek uygulamada ses sürücüsüne iletilir.
    pub fn play(&mut self, data: &[u8]) {
        // Gerçek uygulamada ses sürücüsüne gönderilir
        self.buffer = data.to_vec();
    }

    /// Sesi durdurur ve tamponu temizler.
    pub fn stop(&mut self) {
        self.buffer.clear();
    }
}

impl Default for DoomAudio {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// DOOM MOTOR ARAYÜZÜ
// ============================================================================

/// Doom motor arayüzü (trait).
/// Platform arka ucu bu trait'i uygular; Doom motoru bu fonksiyonları çağırır.
/// Bu tasarım, Doom'u herhangi bir platforma taşımayı kolaylaştırır.
pub trait DoomEngine {
    /// Grafik alt sistemini başlatır
    fn init_graphics(&mut self) -> bool;

    /// Bir kareyi çizer — pixels dizisi ARGB formatında piksel verileri içerir
    fn draw_frame(&mut self, pixels: &[u32]);

    /// Giriş olaylarını işler ve birikmiş olaylar kuyruğunu döndürür
    fn process_events(&mut self) -> Vec<DoomEvent>;

    /// Başlangıçtan bu yana geçen süreyi milisaniye cinsinden döndürür
    fn get_ticks(&self) -> u32;

    /// Belirtilen milisaniye kadar bekler (gecikme döngüsü)
    fn sleep(&mut self, ms: u32);

    /// Belirtilen ses efektini çalar
    fn play_sound(&mut self, sound_id: u32, volume: u8);

    /// Belirtilen ses efektini durdurur
    fn stop_sound(&mut self, sound_id: u32);
}

// ============================================================================
// ECHOS DOOM UYGULAMASI
// ============================================================================

/// echOS'a özgü Doom uygulaması.
/// DoomEngine trait'ini uygular ve echOS grafik/ses altyapısını kullanır.
#[derive(Clone)]
pub struct EchosDoom {
    framebuffer: DoomFramebuffer,
    audio: DoomAudio,
    running: bool,
    start_tick: u64,
    event_queue: Vec<DoomEvent>,
}

impl EchosDoom {
    pub fn new() -> Self {
        EchosDoom {
            framebuffer: DoomFramebuffer::new(),
            audio: DoomAudio::new(),
            running: false,
            start_tick: 0,
            event_queue: Vec::new(),
        }
    }

    /// Doom motorunu başlatır.
    pub fn init(&mut self) -> bool {
        self.start_tick = Self::get_tick_count();
        self.running = true;
        crate::serial_println!("[DOOM] Başlatıldı");
        true
    }

    /// Doom motorunu kapatır.
    pub fn shutdown(&mut self) {
        self.running = false;
        crate::serial_println!("[DOOM] Kapatıldı");
    }

    /// Bir oyun karesi işler.
    /// Olayları kontrol eder; Quit olayı gelirse false döner.
    pub fn tick(&mut self) -> bool {
        if !self.running {
            return false;
        }

        // Olayları işle
        let events = self.process_events();
        for event in &events {
            if matches!(event, DoomEvent::Quit) {
                self.running = false;
                return false;
            }
        }

        true
    }

    /// Sistem zamanlayıcısından milisaniye cinsinden tik sayısını döndürür.
    fn get_tick_count() -> u64 {
        // Gerçek uygulamada sistem zamanlayıcısı kullanılır
        0
    }

    /// Tuş olayını olay kuyruğuna ekler.
    pub fn add_key_event(&mut self, key: DoomKey, pressed: bool) {
        if pressed {
            self.event_queue.push(DoomEvent::KeyDown(key));
        } else {
            self.event_queue.push(DoomEvent::KeyUp(key));
        }
    }

    /// Fare olayını olay kuyruğuna ekler.
    pub fn add_mouse_event(&mut self, button: Option<DoomMouseButton>, x: i32, y: i32, pressed: bool) {
        if let Some(btn) = button {
            if pressed {
                self.event_queue.push(DoomEvent::MouseDown(btn, x, y));
            } else {
                self.event_queue.push(DoomEvent::MouseUp(btn, x, y));
            }
        } else {
            self.event_queue.push(DoomEvent::MouseMove(x, y));
        }
    }

    /// Oyundan çıkış isteği gönderir.
    pub fn quit(&mut self) {
        self.event_queue.push(DoomEvent::Quit);
    }
}

impl DoomEngine for EchosDoom {
    fn init_graphics(&mut self) -> bool {
        self.init()
    }

    fn draw_frame(&mut self, pixels: &[u32]) {
        // Piksel verilerini kare tamponuna kopyala
        for (i, &pixel) in pixels.iter().enumerate() {
            if i < self.framebuffer.pixels.len() {
                self.framebuffer.pixels[i] = pixel;
            }
        }

        // Ekrana aktar
        self.framebuffer.blit_to_screen();
    }

    fn process_events(&mut self) -> Vec<DoomEvent> {
        let events = self.event_queue.clone();
        self.event_queue.clear();
        events
    }

    fn get_ticks(&self) -> u32 {
        (Self::get_tick_count() - self.start_tick) as u32
    }

    fn sleep(&mut self, ms: u32) {
        // Basit spin-loop gecikme — gerçek uygulamada preemptible bekleme kullanılır
        for _ in 0..ms * 10000 {
            core::hint::spin_loop();
        }
    }

    fn play_sound(&mut self, sound_id: u32, volume: u8) {
        crate::serial_println!("[DOOM] Ses çal {} (ses seviyesi {})", sound_id, volume);
    }

    fn stop_sound(&mut self, sound_id: u32) {
        crate::serial_println!("[DOOM] Ses durdur {}", sound_id);
    }
}

impl Default for EchosDoom {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// DOOMGENERIC C ARAYÜZÜ
// ============================================================================

/// C uyumlu geri çağrı türleri.
/// extern "C" garantisi ile C ABI kullanılır (calling convention uyumu için).
pub type DrawFrameCallback = extern "C" fn(*const u32);
pub type GetTicksCallback = extern "C" fn() -> u32;
pub type SleepCallback = extern "C" fn(u32);

/// DoomGeneric C arayüzü yapısı.
/// Orijinal Doom C kodu ile köprü kurmak için kullanılır.
#[repr(C)]
pub struct DoomGenericCallbacks {
    pub draw_frame: DrawFrameCallback,
    pub get_ticks: GetTicksCallback,
    pub sleep: SleepCallback,
}

// ============================================================================
// GLOBAL DOOM ÖRNEĞI
// ============================================================================

/// Global Doom örneği — Mutex ile çoklu iş parçacığı (multi-thread) güvencesi sağlar.
static DOOM_INSTANCE: Mutex<Option<EchosDoom>> = Mutex::new(None);

/// Doom motorunu başlatır ve global örneği oluşturur.
pub fn init_doom() -> bool {
    let mut instance = DOOM_INSTANCE.lock();
    let mut doom = EchosDoom::new();
    let result = doom.init();
    *instance = Some(doom);
    result
}

/// Global Doom örneğinin klonunu döndürür.
pub fn get_doom() -> Option<EchosDoom> {
    DOOM_INSTANCE.lock().clone()
}

/// Global örnek üzerinden kare çizer.
pub fn draw_doom_frame(pixels: &[u32]) {
    if let Some(ref mut doom) = *DOOM_INSTANCE.lock() {
        doom.draw_frame(pixels);
    }
}

/// Global örnek üzerinden olayları işler.
pub fn process_doom_events() -> Vec<DoomEvent> {
    if let Some(ref mut doom) = *DOOM_INSTANCE.lock() {
        doom.process_events()
    } else {
        Vec::new()
    }
}

/// Global örnek üzerinden bir oyun tikini işler.
pub fn doom_tick() -> bool {
    if let Some(ref mut doom) = *DOOM_INSTANCE.lock() {
        doom.tick()
    } else {
        false
    }
}

/// Doom motorunu kapatır ve global örneği temizler.
pub fn shutdown_doom() {
    if let Some(ref mut doom) = *DOOM_INSTANCE.lock() {
        doom.shutdown();
    }
    *DOOM_INSTANCE.lock() = None;
}

/// Global örneğe tuş olayı iletir.
pub fn doom_key_event(key: DoomKey, pressed: bool) {
    if let Some(ref mut doom) = *DOOM_INSTANCE.lock() {
        doom.add_key_event(key, pressed);
    }
}

/// Global örneğe fare olayı iletir.
pub fn doom_mouse_event(button: Option<DoomMouseButton>, x: i32, y: i32, pressed: bool) {
    if let Some(ref mut doom) = *DOOM_INSTANCE.lock() {
        doom.add_mouse_event(button, x, y, pressed);
    }
}

/// Oyundan çıkış isteği gönderir.
pub fn doom_quit() {
    if let Some(ref mut doom) = *DOOM_INSTANCE.lock() {
        doom.quit();
    }
}

// ============================================================================
// KLAVYE EŞLEMESİ
// ============================================================================

/// PS/2 klavye tarama kodunu Doom tuş koduna dönüştürür.
///
/// Tarama kodu (scancode): PS/2 klavye kontrolcüsünden gelen ham donanım kodu.
/// Her tuşa benzersiz bir sayı atanmıştır (IBM AT/XT standardı).
/// Örnek: 0x01 = Escape, 0x48 = Yukarı ok.
pub fn scancode_to_doom_key(scancode: u8) -> DoomKey {
    match scancode {
        0x01 => DoomKey::Escape,
        0x02 => DoomKey::Key1,
        0x03 => DoomKey::Key2,
        0x04 => DoomKey::Key3,
        0x05 => DoomKey::Key4,
        0x06 => DoomKey::Key5,
        0x07 => DoomKey::Key6,
        0x08 => DoomKey::Key7,
        0x09 => DoomKey::Key8,
        0x0A => DoomKey::Key9,
        0x0B => DoomKey::Key0,
        0x10 => DoomKey::KeyQ,
        0x11 => DoomKey::KeyW,
        0x12 => DoomKey::KeyE,
        0x13 => DoomKey::KeyR,
        0x14 => DoomKey::KeyT,
        0x15 => DoomKey::KeyY,
        0x16 => DoomKey::KeyU,
        0x17 => DoomKey::KeyI,
        0x18 => DoomKey::KeyO,
        0x19 => DoomKey::KeyP,
        0x1E => DoomKey::KeyA,
        0x1F => DoomKey::KeyS,
        0x20 => DoomKey::KeyD,
        0x21 => DoomKey::KeyF,
        0x22 => DoomKey::KeyG,
        0x23 => DoomKey::KeyH,
        0x24 => DoomKey::KeyJ,
        0x25 => DoomKey::KeyK,
        0x26 => DoomKey::KeyL,
        0x2C => DoomKey::KeyZ,
        0x2D => DoomKey::KeyX,
        0x2E => DoomKey::KeyC,
        0x2F => DoomKey::KeyV,
        0x30 => DoomKey::KeyB,
        0x31 => DoomKey::KeyN,
        0x32 => DoomKey::KeyM,
        0x39 => DoomKey::Space,
        0x48 => DoomKey::Up,
        0x50 => DoomKey::Down,
        0x4B => DoomKey::Left,
        0x4D => DoomKey::Right,
        0x1C => DoomKey::Enter,
        0x2A | 0x36 => DoomKey::Shift,
        0x1D => DoomKey::Ctrl,
        0x38 => DoomKey::Alt,
        0x0F => DoomKey::Tab,
        _ => DoomKey::Unknown,
    }
}
