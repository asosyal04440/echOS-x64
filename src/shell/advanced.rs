//! # echOS Gelişmiş Shell Özellikleri
//!
//! Pipe, Yönlendirme (Redirect), Tab Tamamlama, Ortam Değişkenleri,
//! Glob (Joker Karakter), Geçmiş Arama gibi Linux seviyesinde shell
//! yetenekleri sağlar.
//!
//! ## Modül Bağımlılık Diyagramı
//!
//! ```
//!  shell/mod.rs
//!       │
//!       ├── Environment (ENV)     -- $VAR genişletme
//!       ├── History (HISTORY)     -- Yukarı/Aşağı ok; Ctrl+R arama
//!       ├── Glob                  -- *.txt → file1.txt file2.txt
//!       ├── Completer             -- Tab tuşu tamamlama
//!       ├── Tokenizer             -- Lexer  (| > >> < & && || ; \n)
//!       ├── Parser                -- Pipeline AST
//!       └── AliasManager (ALIASES)-- ll → ls -la
//!
//!  Her bileşen `spin::Mutex` ile korunur → no_std + thread-safe
//! ```
//!
//! ## Desteklenen Token Türleri
//!
//! | Token          | Sembol | Açıklama                              |
//! |----------------|--------|---------------------------------------|
//! | Pipe           | `\|`   | Stdout→Stdin bağlantısı               |
//! | RedirectOut    | `>`    | Stdout'u dosyaya yaz (üstüne yaz)     |
//! | RedirectAppend | `>>`   | Stdout'u dosyaya ekle (append)        |
//! | RedirectIn     | `<`    | Dosyadan stdin oku                    |
//! | RedirectErr    | `2>`   | Stderr'i yönlendir                    |
//! | RedirectAll    | `&>`   | Stdout+Stderr'i yönlendir             |
//! | Background     | `&`    | Arka planda çalıştır                  |
//! | And            | `&&`   | Kısa devre AND (önceki başarılıysa)   |
//! | Or             | `\|\|` | Kısa devre OR (önceki başarısızsa)    |
//! | Semicolon      | `;`    | Sıralı çalıştırma                     |

use alloc::borrow::ToOwned;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

// ============================================================================
// ENVIRONMENT VARIABLES (ORTAM DEĞİŞKENLERİ)
// ============================================================================

/// Ortam değişkeni yöneticisi.
///
/// Linux'taki `environ` dizisinin basit bir kernel implementasyonudur.
/// `BTreeMap<String, String>` ile anahtar/değer çiftleri tutar.
/// `spin::Mutex` ile no_std ortamında thread-safe erişim sağlanır.
///
/// ## Standart Önceden Tanımlı Değişkenler
///
/// | Değişken   | Varsayılan Değer      | Açıklama                              |
/// |------------|-----------------------|---------------------------------------|
/// | PATH       | /bin:/usr/bin:/sbin   | Komut arama yolları                   |
/// | HOME       | /root                 | Kullanıcı ev dizini                   |
/// | USER       | root                  | Mevcut kullanıcı adı                  |
/// | SHELL      | /bin/echsh            | Aktif shell çalıştırılabiliri         |
/// | PWD        | /                     | Mevcut çalışma dizini                 |
/// | HOSTNAME   | echos                 | Makine adı                            |
/// | TERM       | xterm-256color        | Terminal tipi                         |
/// | LANG       | en_US.UTF-8           | Dil/karakter kodlaması                |
/// | EDITOR     | nano                  | Varsayılan metin düzenleyici          |
/// | PAGER      | less                  | Sayfalama programı                    |
pub struct Environment {
    vars: Mutex<BTreeMap<String, String>>,
}

impl Environment {
    pub const fn new() -> Self {
        Self {
            vars: Mutex::new(BTreeMap::new()),
        }
    }

    /// Değişken ayarlar
    pub fn set(&self, key: &str, value: &str) {
        self.vars.lock().insert(key.to_string(), value.to_string());
    }

    /// Değişken döndürür
    pub fn get(&self, key: &str) -> Option<String> {
        self.vars.lock().get(key).cloned()
    }

    /// Değişken siler
    pub fn unset(&self, key: &str) {
        self.vars.lock().remove(key);
    }

    pub fn clear(&self) {
        self.vars.lock().clear();
    }

    /// Tüm değişkenleri döndürür
    pub fn list(&self) -> Vec<(String, String)> {
        self.vars
            .lock()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// String içindeki `$VAR` ve `${VAR}` referanslarını gerçek değerleriyle değiştirir.
    ///
    /// ## Genişletme Algoritması
    ///
    /// ```
    /// Input: "Hello $USER, home is ${HOME}/docs"
    ///
    /// 1. '$' karakterine rastlanır
    /// 2. Sonraki karakter '{' ise  →  ${...}  modunda '}' ye kadar oku
    ///    Aksi hâlde              →  $VAR    modunda alfasayısal/_ bitene kadar oku
    /// 3. BTreeMap'ten değeri bul ve sonucu result'a ekle
    /// 4. Değişken tanımlı değilse boş string eklenir (bash davranışı)
    ///
    /// Output: "Hello root, home is /root/docs"
    /// ```
    pub fn expand(&self, input: &str) -> String {
        let mut result = String::new();
        let mut chars = input.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '$' {
                // $VAR veya ${VAR} formatını parse et
                let var_name = if chars.peek() == Some(&'{') {
                    chars.next(); // '{' karakterini atla
                    let mut name = String::new();
                    while let Some(&ch) = chars.peek() {
                        if ch == '}' {
                            chars.next();
                            break;
                        }
                        name.push(ch);
                        chars.next();
                    }
                    name
                } else {
                    let mut name = String::new();
                    while let Some(&ch) = chars.peek() {
                        if ch.is_alphanumeric() || ch == '_' {
                            name.push(ch);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    name
                };

                if !var_name.is_empty() {
                    if let Some(value) = self.get(&var_name) {
                        result.push_str(&value);
                    }
                }
            } else {
                result.push(c);
            }
        }

        result
    }

    /// Varsayılan ortam değişkenlerini başlatır.
    ///
    /// Shell ilk başladığında bu fonksiyon çağrılarak standart Unix
    /// değişkenleri ayarlanır. Gerçek bir sistemde `/etc/environment`
    /// veya `~/.profile` dosyalarından okunur; echOS'ta sabit değerler
    /// kullanılmaktadır.
    pub fn init_defaults(&self) {
        self.set("PATH", "/bin:/usr/bin:/sbin");
        self.set("HOME", "/root");
        self.set("USER", "root");
        self.set("SHELL", "/bin/echsh");
        self.set("PWD", "/");
        self.set("HOSTNAME", "echos");
        self.set("TERM", "xterm-256color");
        self.set("LANG", "en_US.UTF-8");
        self.set("EDITOR", "nano");
        self.set("PAGER", "less");
    }
}

lazy_static::lazy_static! {
    /// Global ortam değişkeni mağazası.
    ///
    /// `lazy_static!` ile uygulama ömrü boyunca tek bir instance tutulur.
    /// Kernel herhangi bir yerden `advanced::ENV.get("HOME")` şeklinde erişebilir.
    pub static ref ENV: Environment = Environment::new();
}

// ============================================================================
// HISTORY MANAGEMENT (KOMUT GEÇMİŞİ)
// ============================================================================

/// Komut geçmişi yöneticisi.
///
/// ## Veri Yapısı
///
/// ```
/// entries: Vec<String>   (max 1000 komut, FIFO — en eski baştan silinir)
/// current_index: usize   (yukarı/aşağı ok navigasyonu için konum)
///
///   entries[0]  = en eski komut
///   entries[N-1]= en yeni komut
///   current_index = N (başlangıç: listenin sonundan bir ötesi, "boş satır")
/// ```
///
/// ## Navigasyon Mantığı
///
/// ```
/// ArrowUp   → index-- → önceki komutu göster
/// ArrowDown → index++ → sonraki; index == len ise boş string dön
/// ```
///
/// ## Ctrl+R Ters Arama
///
/// ```
/// start_search()        → sorguyu temizle, results listesini sıfırla
/// search_add_char(c)    → sorguya karakter ekle, sonuçları güncelle
/// search_backspace()    → son karakteri sil, sonuçları güncelle
/// search_current()      → en yakın eşleşmeyi döndür
/// ```
pub struct History {
    entries: Mutex<Vec<String>>,
    max_size: usize,
    current_index: Mutex<usize>,
    search_query: Mutex<String>,
    search_results: Mutex<Vec<usize>>,
}

impl History {
    pub fn new(max_size: usize) -> Self {
        Self {
            entries: Mutex::new(Vec::new()),
            max_size,
            current_index: Mutex::new(0),
            search_query: Mutex::new(String::new()),
            search_results: Mutex::new(Vec::new()),
        }
    }

    /// Yeni komutu geçmişe ekler.
    ///
    /// ## Ekleme Kuralları
    ///
    /// - Boş / yalnızca boşluk içeren komutlar eklenmez
    /// - Ardışık aynı komut tekrar eklenilmez (bash `HISTCONTROL=ignoredups`)
    /// - `max_size` dolunca en eski komut (`entries[0]`) silinir
    /// - Ekleme sonrası `current_index` en sona (len) taşınır
    pub fn push(&self, cmd: &str) {
        if cmd.trim().is_empty() {
            return;
        }

        let mut entries = self.entries.lock();

        // Aynı komut tekrar eklenmesin
        if entries.last().map(|s| s.as_str()) == Some(cmd) {
            return;
        }

        if entries.len() >= self.max_size {
            entries.remove(0);
        }

        entries.push(cmd.to_string());
        *self.current_index.lock() = entries.len();
    }

    /// Önceki komutu döndürür (yukarı ok)
    pub fn previous(&self) -> Option<String> {
        let entries = self.entries.lock();
        let mut index = self.current_index.lock();

        if *index > 0 {
            *index -= 1;
            return entries.get(*index).cloned();
        }
        None
    }

    /// Sonraki komutu döndürür (aşağı ok)
    pub fn next(&self) -> Option<String> {
        let entries = self.entries.lock();
        let mut index = self.current_index.lock();

        if *index < entries.len() - 1 {
            *index += 1;
            return entries.get(*index).cloned();
        } else if *index == entries.len() - 1 {
            *index = entries.len();
            return Some(String::new());
        }
        None
    }

    /// Numaralı geçmiş listesi döndürür.
    ///
    /// `history` komutu tarafından kullanılır.
    /// Dönen tuple: `(sıra_no, komut)` — sıra no 1'den başlar.
    pub fn list(&self) -> Vec<(usize, String)> {
        self.entries
            .lock()
            .iter()
            .enumerate()
            .map(|(i, cmd)| (i + 1, cmd.clone()))
            .collect()
    }

    /// Reverse search başlatır (Ctrl+R)
    pub fn start_search(&self) {
        *self.search_query.lock() = String::new();
        self.search_results.lock().clear();
    }

    /// Search query'e karakter ekler
    pub fn search_add_char(&self, c: char) -> Option<String> {
        self.search_query.lock().push(c);
        self.search_update()
    }

    /// Search query'den karakter siler
    pub fn search_backspace(&self) -> Option<String> {
        self.search_query.lock().pop();
        self.search_update()
    }

    /// Arama sorgusunu çalıştırır ve en iyi eşleşmeyi döndürür.
    ///
    /// Geçmiş listesi **ters sırada** (en yeniden eskiye) aranır.
    /// `contains()` ile substring eşleşmesi kullanılır (regex değil).
    fn search_update(&self) -> Option<String> {
        let query = self.search_query.lock().clone();
        let entries = self.entries.lock();
        let mut results = self.search_results.lock();

        results.clear();
        for (i, cmd) in entries.iter().enumerate().rev() {
            if cmd.contains(&query) {
                results.push(i);
            }
        }

        results.first().and_then(|&i| entries.get(i).cloned())
    }

    /// Current search result'ı döndürür
    pub fn search_current(&self) -> Option<String> {
        let results = self.search_results.lock();
        let entries = self.entries.lock();
        results.first().and_then(|&i| entries.get(i).cloned())
    }

    /// Search query'sini döndürür
    pub fn search_query(&self) -> String {
        self.search_query.lock().clone()
    }
}

lazy_static::lazy_static! {
    /// Global komut geçmişi (en fazla 1000 komut tutar).
    ///
    /// Shell her komut çalıştırıldığında `HISTORY.push()` çağrılır.
    /// OK tuşları `HISTORY.previous()` / `HISTORY.next()` ile çalışır.
    pub static ref HISTORY: History = History::new(1000);
}

// ============================================================================
// GLOBBING (Joker Karakter Genişletme)
// ============================================================================

/// Glob (joker karakter) eşleştirme motoru.
///
/// ## Desteklenen Desenler
///
/// | Desen    | Açıklama                              | Örnek             |
/// |----------|---------------------------------------|-------------------|
/// | `*`      | Sıfır veya daha fazla herhangi karakter | `*.txt`         |
/// | `?`      | Tam olarak bir herhangi karakter      | `test?.sh`        |
/// | `[abc]`  | Karakter sınıfı (a, b veya c)        | `[abc].txt`       |
/// | `[a-z]`  | Karakter aralığı (a'dan z'ye)        | `[a-z]*.rs`       |
/// | `[!abc]` | Olumsuzlanmış karakter sınıfı        | `[!.]*`           |
/// | `[^abc]` | `[!abc]` ile eşdeğer (^=! olumsuz)   | `[^0-9]*`         |
///
/// ## Algoritma (Özyinelemeli Eşleştirme)
///
/// ```
/// matches_inner("*.txt", "file.txt"):
///
///   pattern[0] = '*'  →  articulating kısmı tüket, kalan desen = ".txt"
///   kalan metin  = ""  → ".txt" vs ""  → yanlış
///   kalan metin  = "f" → ".txt" vs "ile.txt" → yanlış
///   ...
///   kalan metin  = "file" → ".txt" vs ".txt" → DOĞRU ✓
/// ```
pub struct Glob;

impl Glob {
    /// Pattern'i match eder (*, ?, [])
    pub fn matches(pattern: &str, text: &str) -> bool {
        Self::matches_inner(pattern, text)
    }

    fn matches_inner(pattern: &str, text: &str) -> bool {
        let mut p_chars = pattern.chars().peekable();
        let mut t_chars = text.chars().peekable();

        loop {
            match (p_chars.next(), t_chars.peek()) {
                // Both exhausted
                (None, None) => return true,

                // Pattern exhausted but text remains
                (None, Some(_)) => return false,

                // * matches any sequence
                (Some('*'), _) => {
                    // Consume consecutive *
                    while p_chars.peek() == Some(&'*') {
                        p_chars.next();
                    }

                    // * at end matches everything
                    if p_chars.peek().is_none() {
                        return true;
                    }

                    // Try matching * with 0, 1, 2, ... characters
                    let remaining_pattern: String = p_chars.collect();
                    let remaining_text: String = t_chars.collect();

                    for i in 0..=remaining_text.len() {
                        if Self::matches_inner(&remaining_pattern, &remaining_text[i..]) {
                            return true;
                        }
                    }
                    return false;
                }

                // ? matches any single char
                (Some('?'), Some(_)) => {
                    t_chars.next();
                }

                // [] character class
                (Some('['), Some(&t)) => {
                    t_chars.next();

                    let mut negated = false;
                    if p_chars.peek() == Some(&'!') || p_chars.peek() == Some(&'^') {
                        negated = true;
                        p_chars.next();
                    }

                    let mut matched = false;
                    let mut prev_char: Option<char> = None;

                    loop {
                        match p_chars.next() {
                            Some(']') => break,
                            Some('-') => {
                                if let (Some(prev), Some(next)) = (prev_char, p_chars.peek()) {
                                    if t >= prev && t <= *next {
                                        matched = true;
                                    }
                                }
                            }
                            Some(c) => {
                                if c == t {
                                    matched = true;
                                }
                                prev_char = Some(c);
                            }
                            None => return false, // Unclosed [
                        }
                    }

                    if negated == matched {
                        return false;
                    }
                }

                // Exact match
                (Some(p), Some(&t)) if p == t => {
                    t_chars.next();
                }

                // No match
                (Some(_), Some(_)) => return false,

                // Text exhausted but pattern remains
                (Some(_), None) => {
                    // Check if remaining pattern is all *
                    while let Some('*') = p_chars.peek() {
                        p_chars.next();
                    }
                    return p_chars.peek().is_none();
                }
            }
        }
    }

    /// Verilen dosya listesi üzerinde glob deseniyle eşleşen dosyaları döndürür.
    ///
    /// Sonuç alfabetik olarak sıralanır (bash davranışı).
    /// `shell/mod.rs`'teki `expand_glob_pattern()` tarafından çağrılır.
    pub fn expand(pattern: &str, files: &[&str]) -> Vec<String> {
        let mut matches = Vec::new();
        for file in files {
            if Self::matches(pattern, file) {
                matches.push(file.to_string());
            }
        }
        matches.sort();
        matches
    }
}

// ============================================================================
// TAB COMPLETION (SEKMELİ TAMAMLAMA)
// ============================================================================
// BRACE EXPANSION (Süslü Parantez Genişletme)
// ============================================================================

/// Süslü parantez genişletme — `{a,b,c}` ve `{1..5}` kalıplarını açar.
///
/// ## Örnekler
/// ```
/// expand_braces("file{1,2,3}.txt")  → ["file1.txt", "file2.txt", "file3.txt"]
/// expand_braces("{a,b}{1,2}")       → ["a1", "a2", "b1", "b2"]
/// expand_braces("test{1..4}")       → ["test1", "test2", "test3", "test4"]
/// expand_braces("hello")            → ["hello"]  (değişiklik yok)
/// ```
pub fn expand_braces(input: &str) -> Vec<String> {
    // Süslü parantez bul
    if let Some(open) = input.find('{') {
        if let Some(close) = find_matching_brace(input, open) {
            let prefix = &input[..open];
            let suffix = &input[close + 1..];
            let inner = &input[open + 1..close];

            // Aralık mı? {start..end}
            if let Some(dotdot) = inner.find("..") {
                let start_str = &inner[..dotdot];
                let end_str = &inner[dotdot + 2..];
                if let (Ok(start), Ok(end)) = (start_str.parse::<i64>(), end_str.parse::<i64>()) {
                    let mut results = Vec::new();
                    let step = if start <= end { 1i64 } else { -1i64 };
                    let mut i = start;
                    loop {
                        let expanded = format!("{}{}{}", prefix, i, suffix);
                        // Suffix'te başka brace olabilir — recursive expand
                        results.extend(expand_braces(&expanded));
                        if i == end {
                            break;
                        }
                        i += step;
                    }
                    return results;
                }
            }

            // Virgülle ayrılmış liste: {a,b,c}
            let items = split_brace_items(inner);
            let mut results = Vec::new();
            for item in &items {
                let expanded = format!("{}{}{}", prefix, item, suffix);
                // Recursive expand — iç içe brace'ler için
                results.extend(expand_braces(&expanded));
            }
            return results;
        }
    }
    vec![input.to_string()]
}

/// Eşleşen kapanış süslü parantezi bul (iç içe parantez desteği)
fn find_matching_brace(s: &str, open: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, ch) in s[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Brace içindeki virgülle ayrılmış öğeleri ayır (iç içe brace'leri koru)
fn split_brace_items(inner: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = String::new();
    let mut depth = 0;
    for ch in inner.chars() {
        match ch {
            '{' => {
                depth += 1;
                current.push(ch);
            }
            '}' => {
                depth -= 1;
                current.push(ch);
            }
            ',' if depth == 0 => {
                items.push(core::mem::take(&mut current));
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        items.push(current);
    }
    items
}

// ============================================================================

/// Sekme (Tab) tuşu tamamlama motoru.
///
/// ## Tamamlama Stratejisi
///
/// ```
/// Kullanıcı "hel<TAB>" yazarsa:
///   words = ["hel"]
///   Yerleşik komutlar arasında "hel" ile başlayanlar: ["help"]
///   → Tek eşleşme: "help " yazar
///
/// Kullanıcı "cat /et<TAB>" yazarsa:
///   words = ["cat", "/et"]
///   Son kelime "/et" → dizin="/", prefix="et"
///   → list_dir("/") → isimleri "et" ile başlayanlar: ["etc"]
///
/// Birden fazla eşleşme varsa:
///   İlk 10 seçenek listelenir, ortak prefix tamamlanır
/// ```
pub struct Completer {
    /// Built-in komutlar listesi — tab tamamlama için kullanılır
    pub builtins: Vec<&'static str>,
}

impl Completer {
    pub fn new() -> Self {
        Self {
            builtins: super::builtin_command_names().to_vec(),
        }
    }

    /// Tamamlama önerileri döndürür.
    ///
    /// `input`: Mevcut komut satırı metni
    /// `cursor_pos`: İmlecin byte cinsinden konumu
    ///
    /// Döndürülen `Vec<String>` boşsa tamamlama yok;
    /// tek eleman varsa otomatik tamamla;
    /// birden fazla ise listele + ortak prefix tamamla.
    pub fn complete(&self, input: &str, cursor_pos: usize) -> Vec<String> {
        let mut completions = BTreeSet::new();

        // Cursor position'a göre current word'ü bul
        let before_cursor = &input[..cursor_pos];
        let words: Vec<&str> = before_cursor.split_whitespace().collect();

        if words.is_empty() || !before_cursor.ends_with(' ') {
            // İlk kelime tamamlama (komut)
            if words.is_empty() || words.len() == 1 {
                let prefix = words.first().copied().unwrap_or("");

                // Built-in komutları kontrol et
                for &cmd in &self.builtins {
                    if cmd.starts_with(prefix) {
                        completions.insert(cmd.to_string());
                    }
                }

                for cmd in self.complete_path_executables(prefix) {
                    completions.insert(cmd);
                }
            } else {
                // Sonraki kelimeler (dosya/dizin tamamlama)
                let prefix = words.last().copied().unwrap_or("");
                completions.extend(self.complete_path(prefix));
            }
        }

        completions.into_iter().collect()
    }

    fn complete_path_executables(&self, prefix: &str) -> Vec<String> {
        let mut completions = BTreeSet::new();
        let path_value = ENV
            .get("PATH")
            .unwrap_or_else(|| String::from("/bin:/usr/bin:/sbin"));

        for dir in path_value.split(':').filter(|segment| !segment.is_empty()) {
            if let Ok(entries) = crate::fs::f2fs::list_dir(dir) {
                for entry in entries {
                    if entry.name.starts_with(prefix) {
                        completions.insert(entry.name);
                    }
                }
            }
        }

        completions.into_iter().collect()
    }

    /// Dosya/dizin yolu tamamlama.
    ///
    /// `prefix` içinde `/` varsa → dizin kısmını ayır, dosya önekini çıkar
    /// `/etc/pas` → dir="/etc/", file_prefix="pas" → list_dir ile eşleşenleri bul
    ///
    /// Gerçek dosya sistemi erişimi başarısız olursa sabit mock liste kullanılır.
    fn complete_path(&self, prefix: &str) -> Vec<String> {
        let mut completions = Vec::new();

        // Gerçek dosya sistemi entegrasyonu
        let (dir, file_prefix) = if prefix.contains('/') {
            let last_slash = prefix.rfind('/').unwrap();
            (
                prefix[..last_slash + 1].to_string(),
                prefix[last_slash + 1..].to_string(),
            )
        } else {
            ("/".to_string(), prefix.to_string())
        };

        // Dizini oku
        if let Ok(entries) = crate::fs::f2fs::list_dir(&dir) {
            for entry in entries {
                if entry.name.starts_with(&file_prefix) {
                    let full_path = if dir == "/" {
                        format!("/{}", entry.name)
                    } else {
                        format!("{}{}", dir, entry.name)
                    };
                    completions.push(full_path);
                }
            }
        }

        // Fallback: mock data
        if completions.is_empty() {
            let mock_files = [
                "bin",
                "boot",
                "dev",
                "etc",
                "home",
                "lib",
                "mnt",
                "proc",
                "root",
                "sbin",
                "sys",
                "tmp",
                "usr",
                "var",
                "config.txt",
                "readme.md",
                "test.sh",
            ];

            for file in &mock_files {
                if file.starts_with(prefix) {
                    completions.push(file.to_string());
                }
            }
        }

        completions.sort();
        completions
    }

    /// Birden fazla tamamlama adayının en uzun ortak önekini bulur.
    ///
    /// ## Algoritma
    ///
    /// ```
    /// completions = ["config.txt", "config.sh", "config_old.txt"]
    /// first       = "config.txt",  prefix_len = 10
    ///
    /// "config.sh"     → first[..6] = "config" ile eşleşme yeri 6
    /// "config_old.txt"→ first[..6] = "config" ile eşleşme yeri 6
    ///
    /// Sonuç: "config"
    /// ```
    pub fn common_prefix(completions: &[String]) -> String {
        if completions.is_empty() {
            return String::new();
        }

        let first = &completions[0];
        let mut prefix_len = first.len();

        for completion in &completions[1..] {
            let mut i = 0;
            while i < prefix_len && i < completion.len() {
                if first.as_bytes()[i] != completion.as_bytes()[i] {
                    break;
                }
                i += 1;
            }
            prefix_len = i;
        }

        first[..prefix_len].to_string()
    }
}

impl Default for Completer {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// PIPE AND REDIRECT (BORU HATTI VE YÖNLENDİRME)
// ============================================================================

/// Komut satırı lexer'ının ürettiği token türleri.
///
/// Tek geçişli (`single-pass`) lexer, kaynak metni bu enum varyantlarına
/// dönüştürür. Parser bu token listesini alarak `Pipeline` AST'i oluşturur.
#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    Word(String),
    Pipe,           // |
    RedirectOut,    // >
    RedirectAppend, // >>
    RedirectIn,     // <
    RedirectErr,    // 2>
    RedirectAll,    // &>
    Background,     // &
    And,            // &&
    Or,             // ||
    Semicolon,      // ;
    Newline,        // \n
}

/// Komut satırı lexer'ı (tokenizer).
///
/// ## Lexer Kuralları
///
/// - Boşluk/tab: kelime sınırı (kelime başlarsa token oluştur)
/// - `|`:  tek `|` → Pipe, çift `||` → Or
/// - `>`:  tek `>` → RedirectOut, `>>` → RedirectAppend
/// - `<`:  RedirectIn
/// - `&`:  `&>` → RedirectAll, `&&` → And, tek `&` → Background
/// - `;`:  Semicolon
/// - `\n`: Newline
/// - `\\c`: escape — sonraki karakteri literal olarak ekle
/// - `'...'`: tek tırnak — içeride hiçbir yorumlama yapılmaz
/// - `"..."`: çift tırnak — `\\c` escape desteklenir, `$VAR` henüz değil
pub struct Tokenizer;

impl Tokenizer {
    /// Input'u token'lara ayırır
    pub fn tokenize(input: &str) -> Vec<Token> {
        let mut tokens = Vec::new();
        let mut chars = input.chars().peekable();
        let mut current_word = String::new();

        while let Some(c) = chars.next() {
            match c {
                ' ' | '\t' => {
                    if !current_word.is_empty() {
                        tokens.push(Token::Word(current_word.clone()));
                        current_word.clear();
                    }
                }
                '|' => {
                    if !current_word.is_empty() {
                        tokens.push(Token::Word(current_word.clone()));
                        current_word.clear();
                    }
                    if chars.peek() == Some(&'|') {
                        chars.next();
                        tokens.push(Token::Or);
                    } else {
                        tokens.push(Token::Pipe);
                    }
                }
                '>' => {
                    if !current_word.is_empty() {
                        tokens.push(Token::Word(current_word.clone()));
                        current_word.clear();
                    }
                    if chars.peek() == Some(&'>') {
                        chars.next();
                        tokens.push(Token::RedirectAppend);
                    } else {
                        tokens.push(Token::RedirectOut);
                    }
                }
                '<' => {
                    if !current_word.is_empty() {
                        tokens.push(Token::Word(current_word.clone()));
                        current_word.clear();
                    }
                    tokens.push(Token::RedirectIn);
                }
                '&' => {
                    if !current_word.is_empty() {
                        tokens.push(Token::Word(current_word.clone()));
                        current_word.clear();
                    }
                    match chars.peek() {
                        Some(&'>') => {
                            chars.next();
                            tokens.push(Token::RedirectAll);
                        }
                        Some(&'&') => {
                            chars.next();
                            tokens.push(Token::And);
                        }
                        _ => {
                            tokens.push(Token::Background);
                        }
                    }
                }
                ';' => {
                    if !current_word.is_empty() {
                        tokens.push(Token::Word(current_word.clone()));
                        current_word.clear();
                    }
                    tokens.push(Token::Semicolon);
                }
                '\n' => {
                    if !current_word.is_empty() {
                        tokens.push(Token::Word(current_word.clone()));
                        current_word.clear();
                    }
                    tokens.push(Token::Newline);
                }
                '\\' => {
                    // Escape next character
                    if let Some(next) = chars.next() {
                        current_word.push(next);
                    }
                }
                '\'' => {
                    // Single-quoted string (no escape)
                    while let Some(ch) = chars.next() {
                        if ch == '\'' {
                            break;
                        }
                        current_word.push(ch);
                    }
                }
                '"' => {
                    // Double-quoted string (with escape)
                    while let Some(ch) = chars.next() {
                        match ch {
                            '"' => break,
                            '\\' => {
                                if let Some(escaped) = chars.next() {
                                    current_word.push(escaped);
                                }
                            }
                            _ => current_word.push(ch),
                        }
                    }
                }
                _ => {
                    current_word.push(c);
                }
            }
        }

        if !current_word.is_empty() {
            tokens.push(Token::Word(current_word));
        }

        tokens
    }
}

/// Tek bir basit komut (args + yönlendirmeler + arka plan bayrağı).
///
/// `Pipeline.commands[i]` olarak konumlanır.
#[derive(Clone, Debug, Default)]
pub struct SimpleCommand {
    /// Komut argümanları (parts[0] = komut adı)
    pub args: Vec<String>,
    /// Yönlendirme listesi (sırasıyla uygulanır)
    pub redirects: Vec<Redirect>,
    /// Arka planda çalıştır (`&`)
    pub background: bool,
}

/// Tek bir yönlendirme spesifikasyonu.
#[derive(Clone, Debug)]
pub struct Redirect {
    /// Yönlendirme türü
    pub kind: RedirectKind,
    /// Hedef dosya adı
    pub target: String,
}

/// Yönlendirme türleri.
///
/// | Variant       | Sembol | Açıklama                              |
/// |---------------|--------|---------------------------------------|
/// | Stdout        | `>`    | Standart çıktıyı dosyaya yaz          |
/// | StdoutAppend  | `>>`   | Standart çıktıyı dosyaya ekle         |
/// | Stdin         | `<`    | Standart girdiyi dosyadan oku         |
/// | Stderr        | `2>`   | Hata çıktısını dosyaya yaz            |
/// | All           | `&>`   | Stdout+Stderr'i dosyaya yaz           |
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RedirectKind {
    Stdout,       // >
    StdoutAppend, // >>
    Stdin,        // <
    Stderr,       // 2>
    All,          // &>
}

/// Boru hattı: `|` ile bağlanmış komutlar dizisi.
///
/// ```
/// "cmd1 | cmd2 | cmd3"
///
/// Pipeline {
///   commands: [cmd1, cmd2, cmd3],
///   background: false,
/// }
/// ```
#[derive(Clone, Debug)]
pub struct Pipeline {
    /// Boru hattındaki komutlar (soldan sağa)
    pub commands: Vec<SimpleCommand>,
    /// Tüm pipeline arka planda mı çalışacak?
    pub background: bool,
}

/// Komut satırı parser'ı.
///
/// ## Gramer (BNF benzeri gösterim)
///
/// ```
/// pipeline_list ::= pipeline (; pipeline)*
/// pipeline      ::= command (| command)*
/// command       ::= WORD+ redirect*
/// redirect      ::= (> | >> | < | 2> | &>) WORD
/// ```
///
/// `&&` ve `||` operatörleri `parse()` içinde ayrı `Pipeline` nesneleri
/// oluşturarak `shell/mod.rs`'teki `execute_chained()` tarafından işlenir.
pub struct Parser;

impl Parser {
    /// Token listesinden pipeline listesi oluşturur.
    ///
    /// Başarı durumunda `Vec<Pipeline>` döndürür.
    /// Sözdizimi hatası varsa `ParseError` ile `Err` döndürür.
    pub fn parse(tokens: Vec<Token>) -> Result<Vec<Pipeline>, ParseError> {
        let mut pipelines = Vec::new();
        let mut current_pipeline = Pipeline {
            commands: Vec::new(),
            background: false,
        };
        let mut current_command = SimpleCommand::default();
        let mut i = 0;

        while i < tokens.len() {
            match &tokens[i] {
                Token::Word(word) => {
                    current_command.args.push(word.clone());
                }
                Token::Pipe => {
                    if current_command.args.is_empty() {
                        return Err(ParseError::UnexpectedPipe);
                    }
                    current_pipeline.commands.push(current_command);
                    current_command = SimpleCommand::default();
                }
                Token::RedirectOut => {
                    i += 1;
                    if let Some(Token::Word(target)) = tokens.get(i) {
                        current_command.redirects.push(Redirect {
                            kind: RedirectKind::Stdout,
                            target: target.clone(),
                        });
                    }
                }
                Token::RedirectAppend => {
                    i += 1;
                    if let Some(Token::Word(target)) = tokens.get(i) {
                        current_command.redirects.push(Redirect {
                            kind: RedirectKind::StdoutAppend,
                            target: target.clone(),
                        });
                    }
                }
                Token::RedirectIn => {
                    i += 1;
                    if let Some(Token::Word(target)) = tokens.get(i) {
                        current_command.redirects.push(Redirect {
                            kind: RedirectKind::Stdin,
                            target: target.clone(),
                        });
                    }
                }
                Token::RedirectErr => {
                    i += 1;
                    if let Some(Token::Word(target)) = tokens.get(i) {
                        current_command.redirects.push(Redirect {
                            kind: RedirectKind::Stderr,
                            target: target.clone(),
                        });
                    }
                }
                Token::RedirectAll => {
                    i += 1;
                    if let Some(Token::Word(target)) = tokens.get(i) {
                        current_command.redirects.push(Redirect {
                            kind: RedirectKind::All,
                            target: target.clone(),
                        });
                    }
                }
                Token::Background => {
                    current_pipeline.background = true;
                    if !current_command.args.is_empty() {
                        current_pipeline.commands.push(current_command);
                        current_command = SimpleCommand::default();
                    }
                    pipelines.push(current_pipeline);
                    current_pipeline = Pipeline {
                        commands: Vec::new(),
                        background: false,
                    };
                }
                Token::And | Token::Or => {
                    // && ve || için short-circuit evaluation
                    if !current_command.args.is_empty() {
                        current_pipeline.commands.push(current_command);
                        current_command = SimpleCommand::default();
                    }
                    if !current_pipeline.commands.is_empty() {
                        pipelines.push(current_pipeline);
                        current_pipeline = Pipeline {
                            commands: Vec::new(),
                            background: false,
                        };
                    }
                }
                Token::Semicolon | Token::Newline => {
                    if !current_command.args.is_empty() {
                        current_pipeline.commands.push(current_command);
                        current_command = SimpleCommand::default();
                    }
                    if !current_pipeline.commands.is_empty() {
                        pipelines.push(current_pipeline);
                        current_pipeline = Pipeline {
                            commands: Vec::new(),
                            background: false,
                        };
                    }
                }
            }
            i += 1;
        }

        // Son komutu ekle
        if !current_command.args.is_empty() {
            current_pipeline.commands.push(current_command);
        }
        if !current_pipeline.commands.is_empty() {
            pipelines.push(current_pipeline);
        }

        Ok(pipelines)
    }
}

/// Parser hata türleri.
#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    /// `|` beklenmediği bir yerde bulundu
    UnexpectedPipe,
    /// `>` / `<` beklenmediği bir yerde bulundu
    UnexpectedRedirect,
    /// Yönlendirme hedef dosya adı eksik
    MissingTarget,
    /// Tırnak işareti kapatılmadı
    UnterminatedQuote,
}

// ============================================================================
// ALIAS SUPPORT (ALIAS DESTEĞİ)
// ============================================================================

/// Alias (kısaltma) yöneticisi.
///
/// ## Alias Mantığı
///
/// `expand_line()` **yalnızca ilk kelimeyi** genişletir — bash davranışı.
/// `echo` gibi yerleşik komutlar alias ile maskelenebilir.
///
/// ## Varsayılan Alias'lar
///
/// | Alias | Genişleme  | Açıklama                  |
/// |-------|------------|---------------------------|
/// | ll    | ls -la     | Ayrıntılı liste           |
/// | la    | ls -a      | Gizli dosyalar dahil      |
/// | l     | ls         | Kısa liste                |
/// | ..    | cd ..      | Bir üst dizine çık        |
/// | ...   | cd ../..   | İki üst dizine çık        |
/// | cls   | clear      | Ekranı temizle            |
pub struct AliasManager {
    aliases: Mutex<BTreeMap<String, String>>,
}

impl AliasManager {
    pub const fn new() -> Self {
        Self {
            aliases: Mutex::new(BTreeMap::new()),
        }
    }

    /// Alias tanımlar
    pub fn set(&self, name: &str, expansion: &str) {
        self.aliases
            .lock()
            .insert(name.to_string(), expansion.to_string());
    }

    /// Alias'ı siler
    pub fn unset(&self, name: &str) {
        self.aliases.lock().remove(name);
    }

    pub fn clear(&self) {
        self.aliases.lock().clear();
    }

    /// Alias'ı expand eder
    pub fn expand(&self, name: &str) -> Option<String> {
        self.aliases.lock().get(name).cloned()
    }

    /// Tüm alias'ları listeler
    pub fn list(&self) -> Vec<(String, String)> {
        self.aliases
            .lock()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Komut satırının ilk kelimesini alias ile değiştirir.
    ///
    /// `input` = "ll /tmp"
    /// `ll` → `ls -la` olarak tanımlıysa
    /// Döndürür: "ls -la /tmp"
    pub fn expand_line(&self, input: &str) -> String {
        let first_word = input.split_whitespace().next();
        if let Some(word) = first_word {
            if let Some(expansion) = self.expand(word) {
                return input.replacen(word, &expansion, 1);
            }
        }
        input.to_string()
    }
}

lazy_static::lazy_static! {
    /// Global alias mağazası.
    ///
    /// `init()` tarafından varsayılan alias'larla doldurulur.
    /// Kullanıcı `alias ll='ls -la'` komutunu çalıştırdığında buraya eklenir.
    pub static ref ALIASES: AliasManager = AliasManager::new();
}

// ============================================================================
// INITIALIZATION (BAŞLATMA)
// ============================================================================

/// Gelişmiş shell özelliklerini başlatır.
///
/// Kernel init akışında en erken çağrılması gereken fonksiyon.
/// Sırasıyla:
/// 1. Ortam değişkenlerini varsayılan değerlere ayarlar (`ENV`)
/// 2. Yaygın kullanılan alias'ları tanımlar (`ALIASES`)
/// 3. Serial porta başlangıç mesajı yazar
pub fn init() {
    ENV.init_defaults();

    // Default alias'lar
    ALIASES.set("ll", "ls -la");
    ALIASES.set("la", "ls -a");
    ALIASES.set("l", "ls");
    ALIASES.set("..", "cd ..");
    ALIASES.set("...", "cd ../..");
    ALIASES.set("cls", "clear");

    crate::serial_println!("[SHELL] Advanced features initialized");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_env_expand() {
        ENV.set("HOME", "/root");
        ENV.set("USER", "test");

        assert_eq!(ENV.expand("$HOME"), "/root");
        assert_eq!(ENV.expand("Hello $USER"), "Hello test");
        assert_eq!(ENV.expand("${HOME}/file"), "/root/file");
    }

    #[test]
    fn test_glob() {
        assert!(Glob::matches("*.txt", "file.txt"));
        assert!(!Glob::matches("*.txt", "file.rs"));
        assert!(Glob::matches("test?", "test1"));
        assert!(Glob::matches("[abc]", "a"));
        assert!(!Glob::matches("[abc]", "d"));
    }

    #[test]
    fn test_tokenizer() {
        let tokens = Tokenizer::tokenize("ls -la | grep test > out.txt");
        assert_eq!(tokens.len(), 7);
        assert_eq!(tokens[2], Token::Pipe);
        assert_eq!(tokens[5], Token::RedirectOut);
    }

    #[test]
    fn test_parser() {
        let tokens = Tokenizer::tokenize("ls | grep test");
        let pipelines = Parser::parse(tokens).unwrap();
        assert_eq!(pipelines.len(), 1);
        assert_eq!(pipelines[0].commands.len(), 2);
    }

    #[test]
    fn completer_includes_path_entries_for_command_position() {
        ENV.set("PATH", "/");
        let completions = Completer::new().complete("rea", 3);
        assert!(completions.iter().any(|item| item == "readme.md"));
    }
}
