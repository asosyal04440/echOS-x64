//! # echOS Widget Sistemi
//!
//! GUI widget'ları için temel trait ve ortak türler.
//! Button, Label, Matrix gibi widget'lar için altyapı.
//!
//! ## Widget Nedir?
//!
//! Widget, kullanıcı arayüzündeki temel görsel bileşenlerdir: butonlar,
//! etiketler, liste kutuları vb. Her widget ekranda bir dikdörtgen alan kaplar
//! ve kullanıcı etkileşimlerine (tıklama, klavye, kaydırma) yanıt verir.
//!
//! ## Trait Tabanlı Tasarım
//!
//! Rust'ta polimorfizm için trait'ler kullanılır. `Widget` trait'i sayesinde
//! farklı widget türleri aynı arayüz üzerinden yönetilebilir. Bu, nesne
//! yönelimli programlamadaki abstract class kavramına benzer.

use crate::gop::framebuffer::Framebuffer;

/// Ekran üzerindeki dikdörtgen bölge.
///
/// GUI sistemlerindeki her bileşen bir dikdörtgen alanla tanımlanır.
/// `x`, `y` sol üst köşenin koordinatları; `width` ve `height` ise
/// bileşenin piksel cinsinden boyutlarıdır.
///
/// `i32` kullanılmasının sebebi: negatif koordinatlar (ekran dışı konumlar)
/// mümkün olmalı ve kırpma/çakışma hesaplamalarında negatif ara değerler çıkabilir.
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Default for Rect {
    fn default() -> Self {
        Self { x: 0, y: 0, width: 0, height: 0 }
    }
}

impl Rect {
    /// Yeni dikdörtgen oluşturur.
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Verilen nokta bu dikdörtgenin içinde mi?
    ///
    /// Hit-testing için kullanılır: kullanıcı tıkladığında hangi widget'ın
    /// tıklandığını bulmak için her widget'ın `bounds()` alanı bu yöntemle
    /// kontrol edilir. `x < self.x + self.width` koşulu sağ sınırı dahil etmez
    /// (yarı açık aralık [x, x+w) ), bu standart piksel sınırı konvansiyonudur.
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    /// İki dikdörtgenin kesişip kesişmediğini kontrol eder.
    ///
    /// Ekran güncelleme optimizasyonunda (dirty region tracking) kullanılır:
    /// sadece değişen bölgeleri yeniden çizmek için hangi widget'ların
    /// etkilendiğini bulmaya yarar. AABB (Axis-Aligned Bounding Box) algoritması
    /// iki dikdörtgenin birbirinin dışında olmadığını kontrol eder.
    pub fn intersects(&self, other: &Rect) -> bool {
        let self_right = self.x + self.width;
        let self_bottom = self.y + self.height;
        let other_right = other.x + other.width;
        let other_bottom = other.y + other.height;
        self.x < other_right
            && self_right > other.x
            && self.y < other_bottom
            && self_bottom > other.y
    }

    /// İki dikdörtgeni kapsayan en küçük dikdörtgeni döndürür.
    ///
    /// Birden fazla widget'ın güncellenmesi gerektiğinde her ikisini de
    /// kapsayan tek bir bölgeyi yeniden çizmek için kullanılır. Min/max
    /// hesaplaması yaparak iki dikdörtgenin birleşim (union) bounding box'ını
    /// döndürür.
    pub fn union(&self, other: &Rect) -> Rect {
        let x1 = if self.x < other.x { self.x } else { other.x };
        let y1 = if self.y < other.y { self.y } else { other.y };
        let x2 = if self.x + self.width > other.x + other.width {
            self.x + self.width
        } else {
            other.x + other.width
        };
        let y2 = if self.y + self.height > other.y + other.height {
            self.y + self.height
        } else {
            other.y + other.height
        };
        Rect::new(x1, y1, x2 - x1, y2 - y1)
    }
}

/// Klavye modifier tuşları
///
/// Bit bayrakları olarak tanımlanır; birden fazla modifier aynı anda
/// basılı tutulabilir (örn. CTRL+SHIFT = 0x01 | 0x02 = 0x03).
/// `&` operatörü ile belirli bir modifier aktif mi diye kontrol edilir:
/// `if modifiers & MOD_CTRL != 0 { ... }`.
pub const MOD_SHIFT: u8 = 0x01;
pub const MOD_CTRL: u8 = 0x02;
pub const MOD_ALT: u8 = 0x04;
pub const MOD_SUPER: u8 = 0x08;

/// Tüm widget'ların implement etmesi gereken trait.
///
/// `Send` bound'u: widget'ların thread'ler arasında güvenle taşınabilmesini
/// sağlar. `no_std` ortamında async runtime olmasa da bu güvenlik garantisi
/// önemlidir. Trait object (`dyn Widget`) kullanımı için `Sized` olmayan
/// türlere de uygulanabilirlik sağlanır.
pub trait Widget: Send {
    /// Widget'ı framebuffer'a çizer.
    ///
    /// Framebuffer doğrudan ekran belleğine karşılık gelir. Bu yöntem her
    /// çerçeve (frame) güncellenmesinde çağrılır. `&self` alması okunabilir
    /// erişim yeterli olduğu anlamına gelir; draw işlemi widget'ın durumunu
    /// değiştirmez, sadece pikselleri belleğe yazar.
    fn draw(&self, fb: &mut Framebuffer);

    /// Mouse click event'ini işler. True dönerse event yakalandı demektir.
    ///
    /// Event bubbling: true döndüğünde üst kap widget olay yayılımını durdurur.
    /// Bu şekilde üst üste widget'larda yalnızca en üstteki tıklamayı yakalar.
    fn on_click(&mut self, x: i32, y: i32) -> bool;

    /// Klavye event'ini işler. True dönerse event yakalandı demektir.
    ///
    /// Varsayılan implementasyon `false` döndürür; yani klavye olayını
    /// işlemeyen widget'lar bu yöntemi override etmek zorunda değildir.
    /// `_key`, `_modifiers`, `_scancode` öneki kullanılmayan parametreleri
    /// derleyici uyarısı olmadan tanımlamayı sağlar.
    fn on_key(&mut self, _key: char, _modifiers: u8, _scancode: u8) -> bool {
        false
    }

    /// Mouse hover event'ini işler.
    ///
    /// Mouse imleci widget üzerine geldiğinde tetiklenir. Hover efekti
    /// (renk değişimi, tooltip vb.) için kullanılır. True dönerse yeniden
    /// çizim gerektiğini belirtir.
    fn on_hover(&mut self, _x: i32, _y: i32) -> bool {
        false
    }

    /// Mouse drag event'ini işler.
    ///
    /// Tıklı tutarak sürükleme için kullanılır. `dx`/`dy` değerleri bir önceki
    /// konuma göre fark (delta) değerleridir; mutlak koordinat değil.
    fn on_drag(&mut self, _dx: i32, _dy: i32) -> bool {
        false
    }

    /// Mouse scroll event'ini işler.
    ///
    /// `delta` pozitifse yukarı, negatifse aşağı kaydırma anlamına gelir.
    /// Kaydırma çarkı (scroll wheel) veya dokunmatik yüzey hareketleriyle
    /// tetiklenir.
    fn on_scroll(&mut self, _delta: i32) -> bool {
        false
    }

    /// Widget'ın sınır kutusunu döndürür.
    ///
    /// Hit-testing ve yeniden çizim bölgesi hesaplaması için kullanılır.
    /// Her widget kendi konum ve boyutunu bu yöntem aracılığıyla bildirir.
    fn bounds(&self) -> Rect;

    /// Widget durumunu günceller (animasyonlar için).
    ///
    /// Her çerçevede çağrılır; animasyon karesi ilerletme, zamanlayıcı
    /// güncelleme gibi işlemler burada yapılır. Varsayılan implementasyon
    /// boştur; animasyon gerektirmeyen widget'lar override etmek zorunda değildir.
    fn update(&mut self) {}

    /// Widget odaklı mı?
    ///
    /// Odak (focus), klavye girdisini hangi widget'ın alacağını belirler.
    /// Yalnızca bir widget aynı anda odaklı olabilir. Varsayılan olarak
    /// false; yani odaklanamayan widget'lar (etiket vb.) için override gerekmez.
    fn is_focused(&self) -> bool {
        false
    }

    /// Widget odak durumunu ayarlar.
    ///
    /// Focus manager tarafından çağrılır. `true` geçilirse widget odağı alır,
    /// `false` geçilirse bırakır. Odak alındığında genellikle görsel bir
    /// geri bildirim (kenarlık rengi değişimi vb.) gösterilir.
    fn set_focus(&mut self, _focused: bool) {}
}

/// Button widget
pub mod button;
/// Label widget (text display)
pub mod label;
/// Matrix animasyon widget (Matrix filmi efekti)
pub mod matrix;
/// Text input widget (TextBox, TextArea)
pub mod text_input;
/// Checkbox and RadioButton widgets
pub mod checkbox;
/// ListView and TreeView widgets
pub mod list;
/// Menu widgets (Menu, ContextMenu, MenuItem)
pub mod menu;
/// ScrollBar and Slider widgets
pub mod scroll;
/// ProgressBar and Spinner widgets
pub mod progress;
/// Dialog widgets (Dialog, MessageBox, FileDialog)
pub mod dialog;
/// Container widgets (Panel, TabControl, Splitter)
pub mod container;
