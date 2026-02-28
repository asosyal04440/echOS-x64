//! # Gap Buffer — Satır Düzenleyici
//!
//! Shell komut satırı girişi için O(1) imleç konumunda ekleme/silme
//! işlemi sağlayan Gap Buffer (Boşluk Tamponu) veri yapısı.
//!
//! ## Gap Buffer Nedir?
//!
//! Gap Buffer, metin editörlerinde kullanılan özel bir dinamik dizi
//! implementasyonudur. İki basit kural üzerine kurulmuştur:
//!
//! 1. Metnin "aktif" kısmı bir boşluk (gap) ile ikiye bölünür.
//! 2. Boşluğun başı (`gap_start`) imlecin bulunduğu konumdur.
//!
//! ## ASCII Bellek Düzeni
//!
//! ```
//! Örnek: "Hello World" metninde imleç 'W' harfinin üstündeyken:
//!
//!  buffer indisi:  0   1   2   3   4   5   6   7   8   9  10  11  12  13
//!  içerik:        'H' 'e' 'l' 'l' 'o' ' ' \0  \0  \0  \0 'W' 'o' 'r' 'l'
//!                 ├───── ön bölge ──────────┤  gap  ├──── arka bölge ──────┤
//!                 [0 .. gap_start)        [gap_start..gap_end)  [gap_end .. len)
//!
//!  gap_start = 6  (imlecin konumu — buraya yeni karakter girilir)
//!  gap_end   = 10 (boşluğun bitişi)
//!
//!  to_string() = buffer[0..6] + buffer[10..14] = "Hello World"
//! ```
//!
//! ## Karmaşıklık Analizi
//!
//! | İşlem          | Gap Buffer | Basit Vec |
//! |----------------|------------|-----------|
//! | İmleçte ekleme | O(1)       | O(n)      |
//! | İmleçte silme  | O(1)       | O(n)      |
//! | İmleç hareketi | O(k)       | O(1)      |
//! | Metin okuma    | O(n)       | O(n)      |
//!
//! **Not:** İmleç hareketi `k` adım ileri/geri gidişte O(k)'dır çünkü
//! buffer'daki karakterlerin boşluktan diğer tarafa kopyalanması gerekir.
//! Kabuk satır düzenleyicisinde bu kabul edilebilir bir maliyet.
//!
//! ## İşlem Örnekleri
//!
//! ```
//! insert('W'):   buffer[gap_start] = 'W';  gap_start += 1;
//! delete():      gap_start -= 1;  (geri silme — karakter "boşluğa düşer")
//! move_left():   gap_start--; gap_end--;  buffer[gap_end] = buffer[gap_start];
//! move_right():  buffer[gap_start] = buffer[gap_end];  gap_start++; gap_end++;
//! grow():        buffer kapasitesini 2× genişlet, boşluğu ortaya ekle
//! ```

use alloc::string::String;
use alloc::vec::Vec;

/// Gap Buffer satır editörü.
///
/// Shell komut satırı girişini tutar. `gap_start` imlecin konumunu,
/// `gap_end` boşluğun bitişini gösterir. İkisi arasındaki alan
/// kullanılmayan "boşluk" olarak rezerve edilmiştir.
pub struct GapBuffer {
    /// Tek boyutlu karakter dizisi (ön bölge + boşluk + arka bölge)
    buffer: Vec<char>,
    /// Boşluğun başlangıcı = imlecin konumu (0-tabanlı)
    gap_start: usize,
    /// Boşluğun bitişi (bu indis artık metnin devamını işaret eder)
    gap_end: usize,
}

impl GapBuffer {
    /// Belirtilen kapasiteyle yeni bir `GapBuffer` oluşturur.
    ///
    /// Başlangıçta tüm buffer boşluktur: `gap_start = 0`, `gap_end = capacity`.
    /// İlk karakter eklendiğinde `gap_start` artmaya başlar.
    ///
    /// `capacity`: Başlangıç tampon boyutu (karakter sayısı).
    /// Tampon dolunca `grow()` otomatik olarak 2× genişletir.
    pub fn new(capacity: usize) -> Self {
        let mut buffer = Vec::with_capacity(capacity);
        // Fill with dummy data initially, though strictly we access via indices
        buffer.resize(capacity, '\0');

        Self {
            buffer,
            gap_start: 0,
            gap_end: capacity,
        }
    }

    /// İmleç konumuna bir karakter ekler.
    ///
    /// ## Algoritma
    ///
    /// ```
    /// Eğer boşluk doluysa (gap_start == gap_end) → grow() çağır
    /// buffer[gap_start] = c;
    /// gap_start += 1;
    /// ```
    ///
    /// `gap_start == gap_end` durumu boşluğun tükendiği anlamına gelir.
    /// `grow()` buffer'ı 2× büyütür ve boşluk miktarını artırır.
    pub fn insert(&mut self, c: char) {
        if self.gap_start == self.gap_end {
            self.grow();
        }

        self.buffer[self.gap_start] = c;
        self.gap_start += 1;
    }

    /// İmleçten önceki karakteri siler (Backspace davranışı).
    ///
    /// ## Algoritma
    ///
    /// ```
    /// gap_start -= 1;
    /// // Silinen karakter "boşluğa düşer" — üstüne yazılmayı bekler
    /// ```
    ///
    /// Dönüş değeri: silinen karakter (`Some(c)`) ya da başa ulaşıldıysa `None`.
    pub fn delete(&mut self) -> Option<char> {
        // Backspace behavior (delete char before gap)
        if self.gap_start > 0 {
            self.gap_start -= 1;
            Some(self.buffer[self.gap_start])
        } else {
            None
        }
    }

    /// İmleci bir karakter sola kaydırır (Sol ok tuşu).
    ///
    /// ## Algoritma
    ///
    /// ```
    /// gap_start--;
    /// gap_end--;
    /// buffer[gap_end] = buffer[gap_start];   // Karakteri boşluğun öbür tarafına kopyala
    /// ```
    ///
    /// Karakter, ön bölgeden arka bölgeye fiziksel olarak taşınır.
    /// Bu, her hareket adımında O(1) bir kopyadır.
    pub fn move_left(&mut self) {
        if self.gap_start > 0 {
            self.gap_start -= 1;
            self.gap_end -= 1;
            self.buffer[self.gap_end] = self.buffer[self.gap_start];
        }
    }

    /// İmleci bir karakter sağa kaydırır (Sağ ok tuşu).
    ///
    /// ## Algoritma
    ///
    /// ```
    /// buffer[gap_start] = buffer[gap_end];   // Karakteri arka bölgeden ön bölgeye kopyala
    /// gap_start++;
    /// gap_end++;
    /// ```
    ///
    /// `move_left()` işleminin tersidir.
    pub fn move_right(&mut self) {
        if self.gap_end < self.buffer.len() {
            self.buffer[self.gap_start] = self.buffer[self.gap_end];
            self.gap_start += 1;
            self.gap_end += 1;
        }
    }

    /// Buffer kapasitesini 2× genişletir.
    ///
    /// ## Genişletme Algoritması
    ///
    /// ```
    /// new_capacity = buffer.len() * 2
    /// gap_size     = new_capacity - buffer.len()  (eklenen boşluk miktarı)
    ///
    /// Yeni düzen:
    ///   [ön bölge (0..gap_start)]  +  [yeni boşluk (gap_size adet \0)]
    ///   +  [arka bölge (gap_end..len)]
    ///
    /// gap_end += gap_size;
    /// ```
    ///
    /// Ön bölge ve arka bölge korunur, ortaya yeni boşluk eklenir.
    /// `gap_start` değişmez; `gap_end` ise `gap_size` kadar ilerler.
    fn grow(&mut self) {
        // Expand buffer
        let new_capacity = self.buffer.len() * 2;
        let mut new_buffer = Vec::with_capacity(new_capacity);

        // Copy pre-gap
        for i in 0..self.gap_start {
            new_buffer.push(self.buffer[i]);
        }

        // Fill new gap
        let gap_size = new_capacity - self.buffer.len();
        for _ in 0..gap_size {
            new_buffer.push('\0');
        }

        // Copy post-gap
        for i in self.gap_end..self.buffer.len() {
            new_buffer.push(self.buffer[i]);
        }

        self.gap_end += gap_size;
        self.buffer = new_buffer;
    }

    /// Tüm metni `String` olarak döndürür.
    ///
    /// Ön bölge (`0..gap_start`) ve arka bölge (`gap_end..len`) birleştirilir.
    /// Boşluk bölgesi (`gap_start..gap_end`) sonuca dahil edilmez.
    ///
    /// O(n) zaman karmaşıklığına sahiptir — her çağrıda yeni bir `String` oluşturur.
    pub fn to_string(&self) -> String {
        let mut s = String::new();
        for i in 0..self.gap_start {
            s.push(self.buffer[i]);
        }
        for i in self.gap_end..self.buffer.len() {
            s.push(self.buffer[i]);
        }
        s
    }

    /// Cursor pozisyonunu döndür (gap_start = cursor position)
    pub fn cursor_pos(&self) -> usize {
        self.gap_start
    }

    /// Toplam metin uzunluğunu döndür.
    ///
    /// Ön bölge + arka bölge uzunluklarının toplamıdır.
    /// Boşluk bölgesi sayılmaz.
    ///
    /// Hesaplama: `gap_start + (buffer.len() - gap_end)`
    pub fn len(&self) -> usize {
        self.gap_start + (self.buffer.len() - self.gap_end)
    }

    /// Cursor'dan sonraki metni döndür.
    ///
    /// Yalnızca arka bölgeyi (`gap_end..buffer.len()`) döndürür.
    /// Backspace/Delete sonrası ekranı yeniden çizmek için kullanılır:
    /// önce `\x1b[K` (satır sonu temizle), sonra bu metin yazılır,
    /// ardından imleç başa döndürülür.
    pub fn text_after_cursor(&self) -> String {
        let mut s = String::new();
        for i in self.gap_end..self.buffer.len() {
            s.push(self.buffer[i]);
        }
        s
    }

    /// İleri silme (Delete tuşu için) - cursor'dan sonraki karakteri sil.
    ///
    /// ## Algoritma
    ///
    /// ```
    /// c = buffer[gap_end];
    /// gap_end += 1;   // Boşluğu bir ileri genişlet (karakteri "yut")
    /// return Some(c);
    /// ```
    ///
    /// `delete()` geri silerken bu fonksiyon ileriye siler (Unix Delete semantiği).
    pub fn delete_forward(&mut self) -> Option<char> {
        if self.gap_end < self.buffer.len() {
            let c = self.buffer[self.gap_end];
            self.gap_end += 1;
            Some(c)
        } else {
            None
        }
    }
}
