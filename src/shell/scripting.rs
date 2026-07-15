//! # echOS Shell Scripting Dili
//!
//! echOS için tam özellikli bir kabuk betik dili implementasyonu.
//! POSIX sh / bash sözdizimini temel alır; no_std ortamında çalışır.
//!
//! ## Desteklenen Özellikler
//!
//! | Özellik                | Sözdizimi                      | Açıklama                            |
//! |------------------------|--------------------------------|-------------------------------------|
//! | Değişken atama         | `VAR=deger`                    | Yerel ya da ortam değişkeni         |
//! | Koşul cümlesi          | `if cond; then ...; fi`        | elif ve else desteklenir            |
//! | While döngüsü          | `while cond; do ...; done`     | break/continue desteklenir          |
//! | For döngüsü            | `for x in list; do ...; done`  | Öğe listesi üzerinde gezinme        |
//! | Until döngüsü          | `until cond; do ...; done`     | Koşul yanlışken çalışır             |
//! | Fonksiyon tanımı       | `function f() { ... }`         | return ile değer döndürme           |
//! | Aritmetik genişletme   | `$((ifade))`                   | Tam sayı aritmetiği                  |
//! | Komut değiştirme       | `$(komut)`                     | Komut çıktısını değere dönüştürme   |
//! | Yerel değişken         | `local VAR=deger`              | Fonksiyon kapsamı                   |
//! | Ortam değişkeni        | `export VAR=deger`             | ENV üzerinden aktarım               |
//!
//! ## Mimari Genel Görünüm
//!
//! ```
//!  Kaynak Metin (String)
//!       │
//!       ▼
//!  ScriptLexer::tokenize()
//!  ┌────────────────────────────────────────────────┐
//!  │  ' ' '\t' '\r'  → atla (boşluk)               │
//!  │  '#'            → yorum satırı sonu kadar atla │
//!  │  '\''           → tek tırnak string           │
//!  │  '"'            → çift tırnak string (\\escape)│
//!  │  '$'            → değişken / $() / $(())      │
//!  │  '0'..'9'       → sayı literali               │
//!  │  'a'..'z' vd.   → anahtar kelime veya Word    │
//!  └────────────────────────────────────────────────┘
//!       │
//!       ▼  Vec<ScriptToken>
//!  ScriptParser::parse()
//!  ┌────────────────────────────────────────────────┐
//!  │  Özyinelemeli İniş (Recursive Descent)         │
//!  │                                                │
//!  │  parse_or                                      │
//!  │    └── parse_and                               │
//!  │          └── parse_comparison                  │
//!  │                └── parse_additive              │
//!  │                      └── parse_multiplicative  │
//!  │                            └── parse_unary     │
//!  │                                  └── parse_primary│
//!  └────────────────────────────────────────────────┘
//!       │
//!       ▼  Vec<Stmt>  (Soyut Sözdizim Ağacı)
//!  Interpreter::execute()
//!  ┌────────────────────────────────────────────────┐
//!  │  exec_stmt  ──  eval_expr  ── eval_arithmetic  │
//!  │  is_truthy  ──  SCRIPT_STATE (spin::Mutex)     │
//!  └────────────────────────────────────────────────┘
//!       │
//!       ▼  i64 (çıkış kodu)
//! ```
//!
//! ## Operatör Öncelik Tablosu (düşükten yükseğe)
//!
//! | Seviye | Operatörler     | Birleşim kuralı |
//! |--------|-----------------|-----------------|
//! | 1      | `\|\|`          | Soldan          |
//! | 2      | `&&`            | Soldan          |
//! | 3      | `==` `!=` `<` `>` `<=` `>=` | Soldan |
//! | 4      | `+` `-`         | Soldan          |
//! | 5      | `*` `/` `%`     | Soldan          |
//! | 6      | `!` `-` (tekil) | Sağdan          |
//! | 7      | Birincil        | —               |

use alloc::borrow::ToOwned;
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

use super::shell_api;

// ============================================================================
// SCRIPT TOKENS (BETİK TOKEN'LARI)
// ============================================================================

/// Betik dilinin lexer token türleri.
///
/// `ScriptLexer::tokenize()` kaynak metni bu varyantlara dönüştürür.
/// `ScriptParser` bu token listesini alarak AST inşa eder.
///
/// ## Anahtar Kelimeler
///
/// | Token      | Sözdizimsel rol                            |
/// |------------|--------------------------------------------|
/// | If/Then    | `if koşul; then gövde`                    |
/// | Elif/Else  | Ek koşul dalları                           |
/// | Fi         | `if` bloğunu kapatır                       |
/// | While/Do   | `while koşul; do gövde`                   |
/// | For/In     | `for değişken in liste`                   |
/// | Done       | While/For/Until bloğunu kapatır            |
/// | Until      | Koşul yanlışken tekrar eden döngü          |
/// | Break      | Döngüden çık                               |
/// | Continue   | Sonraki iterasyona geç                     |
/// | Return     | Fonksiyondan değer döndür                  |
/// | Function   | Fonksiyon tanımı başlatır                  |
/// | Local      | Değişkeni fonksiyon kapsamına sınırla      |
/// | Export     | Değişkeni ortam değişkenine aktar          |
/// | Readonly   | Değişkeni salt okunur yap                 |
/// | Declare    | Değişken tipi bildirimi                    |
///
/// ## Operatörler
///
/// | Token        | Sembol | Açıklama                        |
/// |--------------|--------|---------------------------------|
/// | Assign       | `=`    | Değer atama                     |
/// | Equal        | `==`   | Eşitlik karşılaştırması         |
/// | NotEqual     | `!=`   | Eşitsizlik karşılaştırması      |
/// | Less/Greater | `<` `>`| İkili karşılaştırmalar          |
/// | LessEqual    | `<=`   | Küçük eşit                      |
/// | GreaterEqual | `>=`   | Büyük eşit                      |
/// | And          | `&&`   | Mantıksal ve (kısa devre)       |
/// | Or           | `\|\|` | Mantıksal veya (kısa devre)     |
/// | Not          | `!`    | Mantıksal değil (tekil)         |
///
/// ## Özel Token'lar
///
/// | Token            | Sembol    | Açıklama                        |
/// |------------------|-----------|---------------------------------|
/// | ArithStart       | `$((`     | Aritmetik genişletme başlangıcı |
/// | ArithEnd         | `))`      | Aritmetik genişletme sonu       |
/// | CommandSubStart  | `$(`      | Komut değiştirme başlangıcı     |
/// | CommandSubEnd    | `)`       | Komut değiştirme sonu           |
/// | Variable(String) | `$VAR`    | Değişken referansı              |
#[derive(Clone, Debug, PartialEq)]
pub enum ScriptToken {
    // Anahtar kelimeler (keywords)
    If,
    Then,
    Elif,
    Else,
    Fi,
    While,
    For,
    In,
    Do,
    Done,
    Until,
    Break,
    Continue,
    Return,
    Function,
    Local,
    Export,
    Readonly,
    Declare,
    Case,
    Esac,
    DoubleSemicolon, // ;;

    // Operatörler
    Assign,       // =
    Plus,         // +
    Minus,        // -
    Star,         // *
    Slash,        // /
    Percent,      // %
    Equal,        // ==
    NotEqual,     // !=
    Less,         // <
    Greater,      // >
    LessEqual,    // <=
    GreaterEqual, // >=
    And,          // &&
    Or,           // ||
    Not,          // !

    // Sınırlayıcılar (delimiters)
    LeftParen,    // (
    RightParen,   // )
    LeftBracket,  // [
    RightBracket, // ]
    LeftBrace,    // {
    RightBrace,   // }
    Semicolon,    // ;
    Newline,

    // Literaller
    Word(String),
    Number(i64),
    String(String),

    // Özel token'lar
    ArithStart,       // $((
    ArithEnd,         // ))
    CommandSubStart,  // $(
    CommandSubEnd,    // )
    Variable(String), // $VAR veya ${VAR}
    Eof,
}

// ============================================================================
// SCRIPT LEXER (BETİK LEXER'I)
// ============================================================================

/// Betik kaynak metnini token dizisine dönüştüren lexer (sözcüksel çözümleyici).
///
/// ## Tek Geçişli Lexer Algoritması
///
/// ```
/// while kaynak_metni_bitmedi:
///     c = sonraki_karakter()
///     match c:
///         ' '\t'\r -> atla (boşluk/tab/CR)
///         '\n'     -> Newline token
///         ';'      -> Semicolon token
///         '('      -> '(' sonraki '(' ise ArithStart, değilse LeftParen
///         ')'      -> ')' sonraki ')' ise ArithEnd, değilse RightParen
///         '$'      -> '((' ise ArithStart, '(' ise CommandSubStart,
///                     '{' ise ${VAR}, aksi hâlde $VAR
///         '#'      -> '\n' kadar atla (yorum)
///         '\''     -> '\'' kadar tek tırnak string
///         '"'      -> '"' kadar çift tırnak string (\\c escape)
///         '0'..'9' -> ardışık rakamları oku, i64 parse et
///         'a'..'z', 'A'..'Z', '_' -> kelime oku, anahtar kelime mi kontrol et
/// ```
///
/// ## Alıntı Kuralları
///
/// | Alıntı türü | Escape | Değişken genişletme |
/// |-------------|--------|---------------------|
/// | `'...'`     | Hayır  | Hayır               |
/// | `"..."`     | `\\c`  | Kısmi escape desteği |
pub struct ScriptLexer;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LexGrouping {
    Arithmetic,
    CommandSub,
}

impl ScriptLexer {
    /// Kaynak metni token listesine dönüştürür.
    ///
    /// Her zaman `ScriptToken::Eof` ile biter — parser sonsuz döngüye girmez.
    ///
    /// `source`: Betik kaynak metni (UTF-8 string)
    /// Dönüş: `Vec<ScriptToken>` — son eleman daima `Eof`
    pub fn tokenize(source: &str) -> Vec<ScriptToken> {
        let mut tokens = Vec::new();
        let mut chars = source.chars().peekable();
        let mut grouping_stack = Vec::new();

        while let Some(c) = chars.next() {
            match c {
                // Boşluk ve tab karakterleri kelime sınırı olarak atlanır
                ' ' | '\t' | '\r' => continue,

                // Satır sonu token'ı — ifade sonlandırıcı
                '\n' => {
                    tokens.push(ScriptToken::Newline);
                }

                // Noktalı virgül — sıralı komut ayırıcı veya case-arm sonlandırıcı
                ';' => {
                    if chars.peek() == Some(&';') {
                        chars.next();
                        tokens.push(ScriptToken::DoubleSemicolon);
                    } else {
                        tokens.push(ScriptToken::Semicolon);
                    }
                }

                // '(' veya '((' — LeftParen vs ArithStart
                '(' => {
                    if chars.peek() == Some(&'(') {
                        chars.next();
                        tokens.push(ScriptToken::ArithStart);
                        grouping_stack.push(LexGrouping::Arithmetic);
                    } else {
                        tokens.push(ScriptToken::LeftParen);
                    }
                }

                // ')' veya '))' — RightParen vs ArithEnd
                ')' => {
                    if chars.peek() == Some(&')') {
                        chars.next();
                        tokens.push(ScriptToken::ArithEnd);
                        if matches!(grouping_stack.last(), Some(LexGrouping::Arithmetic)) {
                            grouping_stack.pop();
                        }
                    } else if matches!(grouping_stack.last(), Some(LexGrouping::CommandSub)) {
                        grouping_stack.pop();
                        tokens.push(ScriptToken::CommandSubEnd);
                    } else {
                        tokens.push(ScriptToken::RightParen);
                    }
                }

                '[' => {
                    tokens.push(ScriptToken::LeftBracket);
                }

                ']' => {
                    tokens.push(ScriptToken::RightBracket);
                }

                '{' => {
                    tokens.push(ScriptToken::LeftBrace);
                }

                '}' => {
                    tokens.push(ScriptToken::RightBrace);
                }

                // '=' — atama operatörü ('==' değerlendirmesi parser'da)
                '=' => {
                    tokens.push(ScriptToken::Assign);
                }

                '+' => {
                    tokens.push(ScriptToken::Plus);
                }

                '-' => {
                    tokens.push(ScriptToken::Minus);
                }

                '*' => {
                    tokens.push(ScriptToken::Star);
                }

                '/' => {
                    tokens.push(ScriptToken::Slash);
                }

                '%' => {
                    tokens.push(ScriptToken::Percent);
                }

                // '!' veya '!=' — Not vs NotEqual
                '!' => {
                    if chars.peek() == Some(&'=') {
                        chars.next();
                        tokens.push(ScriptToken::NotEqual);
                    } else {
                        tokens.push(ScriptToken::Not);
                    }
                }

                // '<' veya '<=' — Less vs LessEqual
                '<' => {
                    if chars.peek() == Some(&'=') {
                        chars.next();
                        tokens.push(ScriptToken::LessEqual);
                    } else {
                        tokens.push(ScriptToken::Less);
                    }
                }

                // '>' veya '>=' — Greater vs GreaterEqual
                '>' => {
                    if chars.peek() == Some(&'=') {
                        chars.next();
                        tokens.push(ScriptToken::GreaterEqual);
                    } else {
                        tokens.push(ScriptToken::Greater);
                    }
                }

                // '&&' — Mantıksal AND (tek '&' atlanır — shell pipe değil)
                '&' => {
                    if chars.peek() == Some(&'&') {
                        chars.next();
                        tokens.push(ScriptToken::And);
                    }
                }

                // '||' — Mantıksal OR (tek '|' atlanır — shell pipe değil)
                '|' => {
                    if chars.peek() == Some(&'|') {
                        chars.next();
                        tokens.push(ScriptToken::Or);
                    }
                }

                // '$' — değişken, aritmetik genişletme veya komut değiştirme
                // Üç farklı form:
                //   $((ifade))  → ArithStart + ifade + ArithEnd
                //   $(komut)    → CommandSubStart + komut + CommandSubEnd
                //   ${VAR}      → Variable(VAR)
                //   $VAR        → Variable(VAR)
                '$' => {
                    if chars.peek() == Some(&'(') {
                        chars.next();
                        if chars.peek() == Some(&'(') {
                            chars.next();
                            tokens.push(ScriptToken::ArithStart);
                            grouping_stack.push(LexGrouping::Arithmetic);
                        } else {
                            tokens.push(ScriptToken::CommandSubStart);
                            grouping_stack.push(LexGrouping::CommandSub);
                        }
                    } else if chars.peek() == Some(&'{') {
                        // ${VAR} formu — '}' ye kadar değişken adı oku
                        chars.next();
                        let mut var_name = String::new();
                        while let Some(&ch) = chars.peek() {
                            if ch == '}' {
                                chars.next();
                                break;
                            }
                            var_name.push(ch);
                            chars.next();
                        }
                        tokens.push(ScriptToken::Variable(var_name));
                    } else {
                        // $VAR formu — alfasayısal ve '_' bitene kadar oku
                        let mut var_name = String::new();
                        while let Some(&ch) = chars.peek() {
                            if ch.is_alphanumeric() || ch == '_' {
                                var_name.push(ch);
                                chars.next();
                            } else {
                                break;
                            }
                        }
                        tokens.push(ScriptToken::Variable(var_name));
                    }
                }

                // '#' — yorum satırı; satır sonuna kadar tüm karakterleri atla
                '#' => {
                    while let Some(&ch) = chars.peek() {
                        if ch == '\n' {
                            break;
                        }
                        chars.next();
                    }
                }

                // Tek tırnak string — içeride escape yoktur, her karakter literaldir
                '\'' => {
                    let mut s = String::new();
                    while let Some(ch) = chars.next() {
                        if ch == '\'' {
                            break;
                        }
                        s.push(ch);
                    }
                    tokens.push(ScriptToken::String(s));
                }

                // Çift tırnak string — '\\c' escape dizileri tanınır
                // Örnek: "Merhaba\nDünya" → "Merhaba" + yeni satır + "Dünya"
                '"' => {
                    let mut s = String::new();
                    while let Some(ch) = chars.next() {
                        if ch == '"' {
                            break;
                        }
                        if ch == '\\' {
                            if let Some(escaped) = chars.next() {
                                s.push(escaped);
                            }
                        } else {
                            s.push(ch);
                        }
                    }
                    tokens.push(ScriptToken::String(s));
                }

                // Sayı literali — ardışık ASCII rakamlarını i64'e parse eder
                // Parse başarısız olursa Word token'ı üretir
                '0'..='9' => {
                    let mut num_str = String::new();
                    num_str.push(c);
                    while let Some(&ch) = chars.peek() {
                        if ch.is_ascii_digit() {
                            num_str.push(ch);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                    if let Ok(n) = num_str.parse::<i64>() {
                        tokens.push(ScriptToken::Number(n));
                    } else {
                        tokens.push(ScriptToken::Word(num_str));
                    }
                }

                // Harf veya alt çizgi ile başlayan kelime — anahtar kelime mi?
                // Anahtar kelimeler: if, then, elif, else, fi, while, for, in,
                //                    do, done, until, break, continue, return,
                //                    function, local, export, readonly, declare
                'a'..='z' | 'A'..='Z' | '_' => {
                    let mut word = String::new();
                    word.push(c);
                    while let Some(&ch) = chars.peek() {
                        if ch.is_alphanumeric() || ch == '_' {
                            word.push(ch);
                            chars.next();
                        } else {
                            break;
                        }
                    }

                    // Anahtar kelime tablosuyla karşılaştır
                    let token = match word.as_str() {
                        "if" => ScriptToken::If,
                        "then" => ScriptToken::Then,
                        "elif" => ScriptToken::Elif,
                        "else" => ScriptToken::Else,
                        "fi" => ScriptToken::Fi,
                        "while" => ScriptToken::While,
                        "for" => ScriptToken::For,
                        "in" => ScriptToken::In,
                        "do" => ScriptToken::Do,
                        "done" => ScriptToken::Done,
                        "until" => ScriptToken::Until,
                        "break" => ScriptToken::Break,
                        "continue" => ScriptToken::Continue,
                        "return" => ScriptToken::Return,
                        "function" => ScriptToken::Function,
                        "local" => ScriptToken::Local,
                        "export" => ScriptToken::Export,
                        "readonly" => ScriptToken::Readonly,
                        "declare" => ScriptToken::Declare,
                        "case" => ScriptToken::Case,
                        "esac" => ScriptToken::Esac,
                        _ => ScriptToken::Word(word),
                    };
                    tokens.push(token);
                }

                _ => {
                    // Bilinmeyen karakter — atla
                }
            }
        }

        // Her token dizisi Eof ile biter — parser'ın sonsuz döngüye girmemesi için
        tokens.push(ScriptToken::Eof);
        tokens
    }
}

// ============================================================================
// AST NODES (Soyut Sözdizim Ağacı Düğümleri)
// ============================================================================

/// Betik dilinin ifade deyimi (statement) AST düğümü.
///
/// Parser, token dizisini bu enum varyantlarından oluşan bir `Vec<Stmt>`'e dönüştürür.
/// `Interpreter::exec_stmt()` her düğümü sırasıyla yürütür.
///
/// ## Varyant Açıklamaları
///
/// ```
/// Assign  → VAR=değer  |  local VAR=değer  |  export VAR=değer
/// Command → komut [arg1] [arg2] ...
/// If      → if koşul; then gövde; [elif koşul; then gövde;]* [else gövde;] fi
/// While   → while koşul; do gövde; done
/// For     → for değişken in öğe1 öğe2 ...; do gövde; done
/// Until   → until koşul; do gövde; done   (koşul yanlışken çalışır)
/// Function→ function isim() { gövde }
/// Return  → return [ifade]
/// Break   → break  (döngüden çık)
/// Continue→ continue  (sonraki iterasyona geç)
/// Nop     → no-op (boş deyim)
/// ```
#[derive(Clone, Debug)]
pub enum Stmt {
    /// Değişken atama: `VAR=deger`
    ///
    /// `local=true` → yalnızca fonksiyon kapsamına yazar
    /// `export=true` → ortam değişkenine (ENV) yazar
    Assign {
        name: String,
        value: Expr,
        local: bool,
        export: bool,
    },
    /// Basit komut çalıştırma
    ///
    /// `args[0]` komut adı, `args[1..]` argümanlardır.
    Command { args: Vec<Expr> },
    /// Koşullu dal yapısı
    ///
    /// `elif_clauses`: `(koşul, gövde)` çiftlerinin listesi
    /// `else_body`: `Some(gövde)` veya `None`
    If {
        condition: Expr,
        then_body: Vec<Stmt>,
        elif_clauses: Vec<(Expr, Vec<Stmt>)>,
        else_body: Option<Vec<Stmt>>,
    },
    /// While döngüsü — koşul doğru olduğu sürece gövdeyi tekrar eder
    While { condition: Expr, body: Vec<Stmt> },
    /// For döngüsü — her öğe için `var` değişkenini günceller ve gövdeyi çalıştırır
    For {
        var: String,
        items: Vec<Expr>,
        body: Vec<Stmt>,
    },
    /// Until döngüsü — koşul yanlış olduğu sürece gövdeyi tekrar eder
    Until { condition: Expr, body: Vec<Stmt> },
    /// Fonksiyon tanımı — `SCRIPT_STATE.functions` map'ine kaydedilir
    Function {
        name: String,
        params: Vec<String>,
        body: Vec<Stmt>,
    },
    /// Return deyimi — `SCRIPT_STATE.return_value`'yu ayarlar
    Return(Option<Expr>),
    /// Break deyimi — `SCRIPT_STATE.break_flag`'i true yapar
    Break,
    /// Continue deyimi — `SCRIPT_STATE.continue_flag`'i true yapar
    Continue,
    /// Case deyimi — `case $VAR in pattern) body;; esac`
    Case {
        expr: Expr,
        arms: Vec<(Vec<String>, Vec<Stmt>)>, // (patterns, body)
    },
    /// No-op (boş deyim)
    Nop,
}

/// İfade (expression) AST düğümü.
///
/// `Stmt` içinde kullanılır; özyinelemeli yapı `Box<Expr>` ile sağlanır.
/// Rust'ta öz-referanslı enum için `Box` zorunludur — boyut derleme sırasında bilinmelidir.
///
/// ## Değerlendirme Sonuçları
///
/// Tüm ifadeler `Interpreter::eval_expr()` tarafından `String`'e dönüştürülür.
/// Aritmetik karşılaştırmalar için `1` (doğru) veya `0` (yanlış) döndürülür.
///
/// ```
/// Expr::Number(42)     → "42"
/// Expr::String("hi")   → "hi"
/// Expr::Variable("X")  → SCRIPT_STATE.get_var("X")
/// Expr::Binary(Eq, 3, 3) → "1"   (true)
/// Expr::Binary(Eq, 3, 4) → "0"   (false)
/// Expr::Arithmetic(...)  → eval_arithmetic → i64 → to_string()
/// ```
#[derive(Clone, Debug)]
pub enum Expr {
    /// Karakter dizisi literali
    String(String),
    /// Tam sayı literali
    Number(i64),
    /// Değişken referansı — `$VAR` veya `${VAR}`
    Variable(String),
    /// Aritmetik ifade — `$((ifade))`; iç ifade i64'e dönüştürülür
    Arithmetic(Box<Expr>),
    /// Komut değiştirme — `$(komut)`; komut çıktısı değer olarak kullanılır
    CommandSub(Vec<Expr>),
    /// İkili operatör ifadesi
    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// Tekil operatör ifadesi — `!` (mantıksal değil) veya `-` (negatif)
    Unary { op: UnaryOp, operand: Box<Expr> },
    /// Test ifadesi — `[ ifade ]`; bash benzeri koşul testi
    Test(Box<Expr>),
    /// String karşılaştırma ifadesi — `left op right`
    StrCompare {
        op: StrCompareOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
}

/// İkili operatör türleri.
///
/// `Expr::Binary` içinde kullanılır.
/// Aritmetik (`Add`, `Sub`, ...) ve mantıksal (`And`, `Or`) operatörleri içerir.
/// Tüm operatörler `eval_arithmetic()` ile `i64` üzerinde değerlendirilir.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BinOp {
    Add, // +
    Sub, // -
    Mul, // *
    Div, // /
    Mod, // %
    Eq,  // ==
    Ne,  // !=
    Lt,  // <
    Gt,  // >
    Le,  // <=
    Ge,  // >=
    And, // &&
    Or,  // ||
}

/// Tekil operatör türleri.
///
/// `Expr::Unary` içinde kullanılır.
/// `Not`: sıfır değilse 0, sıfırsa 1 döndürür (mantıksal değil)
/// `Neg`: işareti çevirir (sayısal negatif)
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UnaryOp {
    Not, // !
    Neg, // - (negatif)
}

/// String karşılaştırma operatörü türleri.
///
/// `Expr::StrCompare` içinde kullanılır.
/// `Match` / `Nmatch`: `contains()` ile alt string eşleştirmesi yapar (regex değil).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StrCompareOp {
    Eq,     // =
    Ne,     // !=
    Lt,     // <
    Gt,     // >
    Le,     // <=
    Ge,     // >=
    Match,  // =~    (alt string eşleşmesi)
    Nmatch, // !~    (alt string eşleşmesi yok)
}

// ============================================================================
// SCRIPT PARSER (BETİK PARSER'I)
// ============================================================================

/// Özyinelemeli iniş (recursive descent) parser.
///
/// Token listesini alır, AST (`Vec<Stmt>`) üretir.
///
/// ## Durum
///
/// - `tokens`: `ScriptLexer::tokenize()` çıktısı
/// - `pos`: Mevcut token indisi (`peek()` ve `advance()` ile kullanılır)
///
/// ## Yardımcı Metotlar
///
/// | Metot           | Açıklama                                                    |
/// |-----------------|-------------------------------------------------------------|
/// | `peek()`        | Mevcut token'ı kopyalamadan döndürür                        |
/// | `advance()`     | Mevcut token'ı döndürür ve `pos`'u artırır                 |
/// | `check(t)`      | Mevcut token `t`'ye eşit mi?                               |
/// | `check_next(t)` | Bir sonraki token `t`'ye eşit mi? (lookahead)              |
/// | `is_at_end()`   | `Eof` token'ına ulaşıldı mı?                               |
/// | `is_keyword()`  | Mevcut token bir anahtar kelime mi?                         |
/// | `expect(t)`     | Token eşleşmezse `ScriptError::ExpectedToken(t)` döndürür |
pub struct ScriptParser {
    tokens: Vec<ScriptToken>,
    pos: usize,
}

impl ScriptParser {
    /// Token listesinden yeni bir parser oluşturur.
    ///
    /// `pos = 0` ile başlar; `tokens` listesi `ScriptLexer::tokenize()` çıktısıdır.
    pub fn new(tokens: Vec<ScriptToken>) -> Self {
        Self { tokens, pos: 0 }
    }

    /// Token listesini `Vec<Stmt>` AST'ine dönüştürür.
    ///
    /// ## Algoritma
    ///
    /// ```
    /// while !Eof:
    ///     newline/semicolon token'larını atla
    ///     parse_stmt() çağır, sonucu listeye ekle
    /// ```
    ///
    /// Hata durumunda `Err(ScriptError)` döndürür.
    pub fn parse(&mut self) -> Result<Vec<Stmt>, ScriptError> {
        let mut stmts = Vec::new();

        while !self.is_at_end() {
            // Boş satırları ve fazladan noktalı virgülleri atla
            while self.check(&ScriptToken::Newline) || self.check(&ScriptToken::Semicolon) {
                self.advance();
            }

            if self.is_at_end() {
                break;
            }

            stmts.push(self.parse_stmt()?);
        }

        Ok(stmts)
    }

    /// Tek bir deyimi (statement) parse eder.
    ///
    /// Token türüne göre aşağıdaki parse fonksiyonlarından birini çağırır:
    /// - `If`       → `parse_if()`
    /// - `While`    → `parse_while()`
    /// - `For`      → `parse_for()`
    /// - `Until`    → `parse_until()`
    /// - `Function` → `parse_function()`
    /// - `Return`   → inline parse (isteğe bağlı ifade)
    /// - `Break / Continue` → doğrudan `Stmt::Break/Continue`
    /// - `Local/Export/Declare` → `parse_declaration()`
    /// - `Word = ...` → `parse_assignment()`
    /// - Diğer      → `parse_command()`
    fn parse_stmt(&mut self) -> Result<Stmt, ScriptError> {
        match self.peek() {
            ScriptToken::If => self.parse_if(),
            ScriptToken::While => self.parse_while(),
            ScriptToken::For => self.parse_for(),
            ScriptToken::Until => self.parse_until(),
            ScriptToken::Case => self.parse_case(),
            ScriptToken::Function => self.parse_function(),
            ScriptToken::Return => {
                self.advance();
                let expr =
                    if !self.check(&ScriptToken::Newline) && !self.check(&ScriptToken::Semicolon) {
                        Some(self.parse_expr()?)
                    } else {
                        None
                    };
                Ok(Stmt::Return(expr))
            }
            ScriptToken::Break => {
                self.advance();
                Ok(Stmt::Break)
            }
            ScriptToken::Continue => {
                self.advance();
                Ok(Stmt::Continue)
            }
            ScriptToken::Local | ScriptToken::Export | ScriptToken::Declare => {
                self.parse_declaration()
            }
            ScriptToken::Word(name) if self.check_next(&ScriptToken::Assign) => {
                self.parse_assignment(false, false)
            }
            ScriptToken::Word(name) if self.check_next(&ScriptToken::LeftParen) => {
                // Fonksiyon çağrısı veya komut
                self.parse_command()
            }
            _ => self.parse_command(),
        }
    }

    /// `if` deyimini parse eder.
    ///
    /// ## Gramer
    ///
    /// ```
    /// if_stmt ::= 'if' ifade 'then' deyim*
    ///             ('elif' ifade 'then' deyim*)*
    ///             ('else' deyim*)?
    ///             'fi'
    /// ```
    fn parse_if(&mut self) -> Result<Stmt, ScriptError> {
        self.expect(ScriptToken::If)?;

        let condition = self.parse_expr()?;
        self.expect(ScriptToken::Then)?;

        let mut then_body = Vec::new();
        while !self.check(&ScriptToken::Elif)
            && !self.check(&ScriptToken::Else)
            && !self.check(&ScriptToken::Fi)
        {
            then_body.push(self.parse_stmt()?);
        }

        let mut elif_clauses = Vec::new();
        while self.check(&ScriptToken::Elif) {
            self.advance();
            let elif_cond = self.parse_expr()?;
            self.expect(ScriptToken::Then)?;

            let mut elif_body = Vec::new();
            while !self.check(&ScriptToken::Elif)
                && !self.check(&ScriptToken::Else)
                && !self.check(&ScriptToken::Fi)
            {
                elif_body.push(self.parse_stmt()?);
            }
            elif_clauses.push((elif_cond, elif_body));
        }

        let mut else_body = None;
        if self.check(&ScriptToken::Else) {
            self.advance();
            let mut body = Vec::new();
            while !self.check(&ScriptToken::Fi) {
                body.push(self.parse_stmt()?);
            }
            else_body = Some(body);
        }

        self.expect(ScriptToken::Fi)?;

        Ok(Stmt::If {
            condition,
            then_body,
            elif_clauses,
            else_body,
        })
    }

    /// `while` döngüsünü parse eder.
    ///
    /// ## Gramer
    ///
    /// ```
    /// while_stmt ::= 'while' ifade 'do' deyim* 'done'
    /// ```
    fn parse_while(&mut self) -> Result<Stmt, ScriptError> {
        self.expect(ScriptToken::While)?;

        let condition = self.parse_expr()?;
        self.expect(ScriptToken::Do)?;

        let mut body = Vec::new();
        while !self.check(&ScriptToken::Done) {
            body.push(self.parse_stmt()?);
        }
        self.expect(ScriptToken::Done)?;

        Ok(Stmt::While { condition, body })
    }

    /// `for` döngüsünü parse eder.
    ///
    /// ## Gramer
    ///
    /// ```
    /// for_stmt ::= 'for' WORD 'in' ifade* 'do' deyim* 'done'
    /// ```
    ///
    /// Her iterasyonda döngü değişkeni `SCRIPT_STATE.set_local()` ile güncellenir.
    fn parse_for(&mut self) -> Result<Stmt, ScriptError> {
        self.expect(ScriptToken::For)?;

        let var = if let ScriptToken::Word(name) = self.advance() {
            name.clone()
        } else {
            return Err(ScriptError::ExpectedVariable);
        };

        self.expect(ScriptToken::In)?;

        let mut items = Vec::new();
        while !self.check(&ScriptToken::Do) {
            items.push(self.parse_expr()?);
        }

        self.expect(ScriptToken::Do)?;

        let mut body = Vec::new();
        while !self.check(&ScriptToken::Done) {
            body.push(self.parse_stmt()?);
        }
        self.expect(ScriptToken::Done)?;

        Ok(Stmt::For { var, items, body })
    }

    /// `until` döngüsünü parse eder.
    ///
    /// ## Gramer
    ///
    /// ```
    /// until_stmt ::= 'until' ifade 'do' deyim* 'done'
    /// ```
    ///
    /// `while`'ın tersidir: koşul **yanlış** olduğu sürece çalışır.
    fn parse_until(&mut self) -> Result<Stmt, ScriptError> {
        self.expect(ScriptToken::Until)?;

        let condition = self.parse_expr()?;
        self.expect(ScriptToken::Do)?;

        let mut body = Vec::new();
        while !self.check(&ScriptToken::Done) {
            body.push(self.parse_stmt()?);
        }
        self.expect(ScriptToken::Done)?;

        Ok(Stmt::Until { condition, body })
    }

    /// `case` deyimini parse eder.
    ///
    /// ## Gramer
    /// ```
    /// case_stmt ::= 'case' expr 'in'
    ///               (pattern ('|' pattern)* ')' deyim* ';;')*
    ///               'esac'
    /// ```
    fn parse_case(&mut self) -> Result<Stmt, ScriptError> {
        self.expect(ScriptToken::Case)?;
        let expr = self.parse_expr()?;
        self.expect(ScriptToken::In)?;
        while self.check(&ScriptToken::Newline) || self.check(&ScriptToken::Semicolon) {
            self.advance();
        }

        let mut arms: Vec<(Vec<String>, Vec<Stmt>)> = Vec::new();

        while !self.check(&ScriptToken::Esac) && !self.check(&ScriptToken::Eof) {
            // Pattern'ları oku: pat1 | pat2 | pat3 )
            let mut patterns = Vec::new();
            loop {
                match self.advance().clone() {
                    ScriptToken::Word(s) | ScriptToken::String(s) => patterns.push(s),
                    ScriptToken::Star => patterns.push("*".into()),
                    _ => break,
                }
                // '|' ile ayrılmış pattern'lar
                if let ScriptToken::Word(ref w) = self.peek() {
                    if w == "|" {
                        self.advance();
                        continue;
                    }
                }
                break;
            }
            // ')' bekle — pattern sonu
            if self.check(&ScriptToken::RightParen) {
                self.advance();
            }
            while self.check(&ScriptToken::Newline) || self.check(&ScriptToken::Semicolon) {
                self.advance();
            }

            // Body — ;; veya esac'a kadar oku
            let mut body = Vec::new();
            while !self.check(&ScriptToken::DoubleSemicolon)
                && !self.check(&ScriptToken::Esac)
                && !self.check(&ScriptToken::Eof)
            {
                body.push(self.parse_stmt()?);
                while self.check(&ScriptToken::Newline) || self.check(&ScriptToken::Semicolon) {
                    self.advance();
                }
            }
            if self.check(&ScriptToken::DoubleSemicolon) {
                self.advance();
            }
            while self.check(&ScriptToken::Newline) || self.check(&ScriptToken::Semicolon) {
                self.advance();
            }

            if !patterns.is_empty() {
                arms.push((patterns, body));
            }
        }
        self.expect(ScriptToken::Esac)?;
        Ok(Stmt::Case { expr, arms })
    }

    /// Fonksiyon tanımını parse eder.
    ///
    /// ## Gramer
    ///
    /// ```
    /// func_stmt ::= 'function' WORD '(' ')' '{' deyim* '}'
    /// ```
    ///
    /// Parametre listesi şu an desteklenmez — `params: Vec::new()` ile boş bırakılır.
    /// Fonksiyon gövdesi `SCRIPT_STATE.functions` map'ine `Stmt::Function` olarak kaydedilir.
    fn parse_function(&mut self) -> Result<Stmt, ScriptError> {
        self.expect(ScriptToken::Function)?;

        let name = if let ScriptToken::Word(name) = self.advance() {
            name.clone()
        } else {
            return Err(ScriptError::ExpectedFunctionName);
        };

        self.expect(ScriptToken::LeftParen)?;
        self.expect(ScriptToken::RightParen)?;

        self.expect(ScriptToken::LeftBrace)?;

        let mut body = Vec::new();
        while !self.check(&ScriptToken::RightBrace) {
            body.push(self.parse_stmt()?);
        }
        self.expect(ScriptToken::RightBrace)?;

        Ok(Stmt::Function {
            name,
            params: Vec::new(),
            body,
        })
    }

    /// `local` / `export` / `declare` bildirimini parse eder.
    ///
    /// Anahtar kelimeyi tüketir, ardından `parse_assignment()` çağrılır.
    /// `local` bayrağı ya da `export` bayrağı `Stmt::Assign`'a taşınır.
    fn parse_declaration(&mut self) -> Result<Stmt, ScriptError> {
        let is_local = self.check(&ScriptToken::Local);
        let is_export = self.check(&ScriptToken::Export);

        self.advance();

        self.parse_assignment(is_local, is_export)
    }

    /// `VAR=değer` atamasını parse eder.
    ///
    /// `local` ve `export` bayrakları `Stmt::Assign` varyantına aktarılır.
    /// `Interpreter::exec_stmt()` bu bayraklara göre değeri nereye yazacağına karar verir.
    fn parse_assignment(&mut self, local: bool, export: bool) -> Result<Stmt, ScriptError> {
        let name = if let ScriptToken::Word(name) = self.advance() {
            name.clone()
        } else {
            return Err(ScriptError::ExpectedVariable);
        };

        self.expect(ScriptToken::Assign)?;

        let value = self.parse_expr()?;

        Ok(Stmt::Assign {
            name,
            value,
            local,
            export,
        })
    }

    /// Basit komut deyimini parse eder.
    ///
    /// Satır sonu, noktalı virgül, EOF veya anahtar kelime görene kadar
    /// ardışık ifadeleri argüman listesi olarak toplar.
    fn parse_command(&mut self) -> Result<Stmt, ScriptError> {
        let mut args = Vec::new();

        while !self.check(&ScriptToken::Newline)
            && !self.check(&ScriptToken::Semicolon)
            && !self.check(&ScriptToken::Eof)
            && !self.is_keyword()
        {
            args.push(self.parse_expr()?);
        }

        Ok(Stmt::Command { args })
    }

    /// En üst seviye ifade parse noktası — `parse_or()`'a yönlendirir.
    fn parse_expr(&mut self) -> Result<Expr, ScriptError> {
        self.parse_or()
    }

    /// Mantıksal OR ifadesini parse eder (en düşük öncelik).
    ///
    /// Gramer: `and_expr ('||' and_expr)*`
    fn parse_or(&mut self) -> Result<Expr, ScriptError> {
        let mut left = self.parse_and()?;

        while self.check(&ScriptToken::Or) {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::Binary {
                op: BinOp::Or,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Mantıksal AND ifadesini parse eder.
    ///
    /// Gramer: `comparison_expr ('&&' comparison_expr)*`
    fn parse_and(&mut self) -> Result<Expr, ScriptError> {
        let mut left = self.parse_comparison()?;

        while self.check(&ScriptToken::And) {
            self.advance();
            let right = self.parse_comparison()?;
            left = Expr::Binary {
                op: BinOp::And,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Karşılaştırma ifadesini parse eder.
    ///
    /// Gramer: `additive_expr (('==' | '!=' | '<' | '>' | '<=' | '>=') additive_expr)?`
    ///
    /// Zincirli karşılaştırma desteklenmez — yalnızca tek bir operatör kabul edilir.
    fn parse_comparison(&mut self) -> Result<Expr, ScriptError> {
        let left = self.parse_additive()?;

        let op = match self.peek() {
            ScriptToken::Equal => BinOp::Eq,
            ScriptToken::NotEqual => BinOp::Ne,
            ScriptToken::Less => BinOp::Lt,
            ScriptToken::Greater => BinOp::Gt,
            ScriptToken::LessEqual => BinOp::Le,
            ScriptToken::GreaterEqual => BinOp::Ge,
            _ => return Ok(left),
        };

        self.advance();
        let right = self.parse_additive()?;

        Ok(Expr::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        })
    }

    /// Toplama/çıkarma ifadesini parse eder.
    ///
    /// Gramer: `multiplicative_expr (('+' | '-') multiplicative_expr)*`
    fn parse_additive(&mut self) -> Result<Expr, ScriptError> {
        let mut left = self.parse_multiplicative()?;

        loop {
            let op = match self.peek() {
                ScriptToken::Plus => BinOp::Add,
                ScriptToken::Minus => BinOp::Sub,
                _ => break,
            };

            self.advance();
            let right = self.parse_multiplicative()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Çarpma/bölme/mod ifadesini parse eder.
    ///
    /// Gramer: `unary_expr (('*' | '/' | '%') unary_expr)*`
    fn parse_multiplicative(&mut self) -> Result<Expr, ScriptError> {
        let mut left = self.parse_unary()?;

        loop {
            let op = match self.peek() {
                ScriptToken::Star => BinOp::Mul,
                ScriptToken::Slash => BinOp::Div,
                ScriptToken::Percent => BinOp::Mod,
                _ => break,
            };

            self.advance();
            let right = self.parse_unary()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    /// Tekil operatör ifadesini parse eder.
    ///
    /// Gramer: `('!' | '-') unary_expr | primary_expr`
    ///
    /// Sağ birleşimlidir: `!!x` → `!(!(x))`
    fn parse_unary(&mut self) -> Result<Expr, ScriptError> {
        let op = match self.peek() {
            ScriptToken::Not => UnaryOp::Not,
            ScriptToken::Minus => UnaryOp::Neg,
            _ => return self.parse_primary(),
        };

        self.advance();
        let operand = self.parse_unary()?;

        Ok(Expr::Unary {
            op,
            operand: Box::new(operand),
        })
    }

    /// Birincil (primary) ifadeyi parse eder — en yüksek öncelik.
    ///
    /// Aşağıdaki token türlerini tanır:
    ///
    /// | Token           | Üretilen `Expr`               |
    /// |-----------------|-------------------------------|
    /// | `Number(n)`     | `Expr::Number(n)`             |
    /// | `String(s)`     | `Expr::String(s)`             |
    /// | `Variable(v)`   | `Expr::Variable(v)`           |
    /// | `ArithStart`    | `Expr::Arithmetic(...)`       |
    /// | `CommandSubStart`| `Expr::CommandSub(...)`      |
    /// | `LeftBracket`   | `Expr::Test(...)`             |
    /// | `LeftParen`     | gruplandırılmış ifade         |
    /// | `Word(w)`       | `Expr::String(w)`             |
    fn parse_primary(&mut self) -> Result<Expr, ScriptError> {
        match self.peek() {
            ScriptToken::Number(n) => {
                let n = *n;
                self.advance();
                Ok(Expr::Number(n))
            }
            ScriptToken::String(s) => {
                let s = s.clone();
                self.advance();
                Ok(Expr::String(s))
            }
            ScriptToken::Variable(name) => {
                let name = name.clone();
                self.advance();
                Ok(Expr::Variable(name))
            }
            ScriptToken::ArithStart => {
                // $((ifade)) — aritmetik genişletme
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(ScriptToken::ArithEnd)?;
                Ok(Expr::Arithmetic(Box::new(expr)))
            }
            ScriptToken::CommandSubStart => {
                // $(komut) — komut değiştirme
                self.advance();
                let mut args = Vec::new();
                while !self.check(&ScriptToken::CommandSubEnd) && !self.check(&ScriptToken::Eof) {
                    args.push(self.parse_expr()?);
                }
                self.expect(ScriptToken::CommandSubEnd)?;
                Ok(Expr::CommandSub(args))
            }
            ScriptToken::LeftBracket => {
                // [ ifade ] — bash test komutu sözdizimi
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(ScriptToken::RightBracket)?;
                Ok(Expr::Test(Box::new(expr)))
            }
            ScriptToken::LeftParen => {
                // ( ifade ) — gruplandırılmış ifade; öncelik yükseltme
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(ScriptToken::RightParen)?;
                Ok(expr)
            }
            ScriptToken::Word(w) => {
                let w = w.clone();
                self.advance();
                Ok(Expr::String(w))
            }
            _ => Err(ScriptError::UnexpectedToken),
        }
    }

    // ---- Yardımcı (helper) metotlar ----

    /// Mevcut token'a referans döndürür (ilerletmez).
    fn peek(&self) -> &ScriptToken {
        self.tokens.get(self.pos).unwrap_or(&ScriptToken::Eof)
    }

    /// Mevcut token'a referans döndürür ve `pos`u bir artırır.
    fn advance(&mut self) -> &ScriptToken {
        self.pos += 1;
        self.tokens.get(self.pos - 1).unwrap_or(&ScriptToken::Eof)
    }

    /// Mevcut token verilen token'a eşit mi?
    fn check(&self, token: &ScriptToken) -> bool {
        self.peek() == token
    }

    /// Bir sonraki token (lookahead) verilen token'a eşit mi?
    fn check_next(&self, token: &ScriptToken) -> bool {
        self.tokens.get(self.pos + 1).unwrap_or(&ScriptToken::Eof) == token
    }

    /// `Eof` token'ına ulaşıldı mı?
    fn is_at_end(&self) -> bool {
        self.peek() == &ScriptToken::Eof
    }

    /// Mevcut token bir anahtar kelime mi?
    ///
    /// Komut parse ederken argüman toplama döngüsünü anahtar kelimelerde durdurur.
    fn is_keyword(&self) -> bool {
        matches!(
            self.peek(),
            ScriptToken::If
                | ScriptToken::Then
                | ScriptToken::Elif
                | ScriptToken::Else
                | ScriptToken::Fi
                | ScriptToken::While
                | ScriptToken::For
                | ScriptToken::Do
                | ScriptToken::Done
                | ScriptToken::Until
                | ScriptToken::Function
                | ScriptToken::Return
                | ScriptToken::Break
                | ScriptToken::Continue
        )
    }

    /// Mevcut token beklenenle eşleşiyorsa tüketir, aksi hâlde hata döndürür.
    fn expect(&mut self, token: ScriptToken) -> Result<(), ScriptError> {
        if self.check(&token) {
            self.advance();
            Ok(())
        } else {
            Err(ScriptError::ExpectedToken(token))
        }
    }
}

/// Betik dili hata türleri.
///
/// `ScriptParser::parse()` ve `Interpreter::execute()` tarafından üretilir.
/// `run_script()` bu hataları `Err(ScriptError)` olarak döndürür.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptError {
    /// Beklenmeyen token — sözdizimi hatası
    UnexpectedToken,
    /// Belirli bir token bekleniyordu ama farklı token bulundu
    ExpectedToken(ScriptToken),
    /// Değişken adı bekleniyordu
    ExpectedVariable,
    /// Fonksiyon adı bekleniyordu
    ExpectedFunctionName,
    /// Tanımlanmamış değişkene erişim
    UndefinedVariable(String),
    /// Tanımlanmamış fonksiyon çağrısı
    UndefinedFunction(String),
    /// Sıfıra bölme hatası
    DivisionByZero,
    /// set -e ile çıkış (errexit)
    Exit(i64),
    /// Genel çalışma zamanı hatası
    RuntimeError(String),
}

// ============================================================================
// SCRIPT INTERPRETER (BETİK YORUMLAYICISI)
// ============================================================================

/// Betik yorumlayıcısının global çalışma zamanı durumu.
///
/// ## Mutex Korumalı Alanlar
///
/// Her alan `spin::Mutex` ile sarmalıdır. Bu sayede:
/// - `no_std` ortamında thread-safe erişim sağlanır
/// - İterrupt handler'lardan da güvenle okunabilir/yazılabilir
///
/// ## Değişken Çözümleme Hiyerarşisi
///
/// ```
/// get_var("NAME") çağrısında:
///   1. local_vars map'inde ara  →  bulunduysa döndür
///   2. ENV.get("NAME") çağır   →  ortam değişkenini döndür
///   3. None                    →  UndefinedVariable hatası
/// ```
///
/// ## Kontrol Akışı Bayrakları
///
/// | Bayrak           | Ayarlayan     | Kontrol Eden                    |
/// |------------------|---------------|---------------------------------|
/// | `return_value`   | `Stmt::Return`| `execute()` — fonksiyon sonu    |
/// | `break_flag`     | `Stmt::Break` | `exec_stmt()` — while/for gövde |
/// | `continue_flag`  | `Stmt::Continue`| `exec_stmt()` — while/for gövde|
pub struct ScriptState {
    /// Yerel değişkenler (fonksiyon kapsamı)
    pub local_vars: Mutex<BTreeMap<String, String>>,
    /// Tanımlı fonksiyonlar (isim → Stmt::Function)
    pub functions: Mutex<BTreeMap<String, Stmt>>,
    /// Return deyiminin çıkış kodu (None = return çağrılmadı)
    pub return_value: Mutex<Option<i64>>,
    /// Break bayrağı — döngü gövdesini erkenden sonlandırır
    pub break_flag: Mutex<bool>,
    /// Continue bayrağı — döngünün geri kalanını atlatır
    pub continue_flag: Mutex<bool>,
    /// Shell seçenekleri — `set -e/-x/-u` ile kontrol edilir
    /// errexit: hata durumunda betiği sonlandır (set -e)
    pub errexit: Mutex<bool>,
    /// xtrace: her komutu çalıştırmadan önce stderr'e yaz (set -x)
    pub xtrace: Mutex<bool>,
    /// nounset: tanımsız değişken kullanımında hata ver (set -u)
    pub nounset: Mutex<bool>,
}

impl ScriptState {
    /// Sıfır başlangıç değerleriyle yeni bir durum oluşturur.
    ///
    /// `const fn` ile derleme zamanında oluşturulabilir.
    /// `lazy_static!` içinde `SCRIPT_STATE` olarak kullanılır.
    pub const fn new() -> Self {
        Self {
            local_vars: Mutex::new(BTreeMap::new()),
            functions: Mutex::new(BTreeMap::new()),
            return_value: Mutex::new(None),
            break_flag: Mutex::new(false),
            continue_flag: Mutex::new(false),
            errexit: Mutex::new(false),
            xtrace: Mutex::new(false),
            nounset: Mutex::new(false),
        }
    }

    /// Yerel değişken ayarlar.
    ///
    /// `local_vars` map'ine `(isim, değer)` çiftini ekler/günceller.
    pub fn set_local(&self, name: &str, value: &str) {
        self.local_vars
            .lock()
            .insert(name.to_string(), value.to_string());
    }

    /// Değişkeni okur; önce yerel, sonra ortam değişkenlerine bakar.
    ///
    /// Çözümleme sırası:
    /// 1. `local_vars` map'inde ara
    /// 2. `super::advanced::ENV.get(name)` çağır
    /// 3. `None` döndür (hata `Interpreter::eval_expr()` tarafından yayılır)
    pub fn get_var(&self, name: &str) -> Option<String> {
        // Önce yerel değişkenleri kontrol et
        if let Some(val) = self.local_vars.lock().get(name) {
            return Some(val.clone());
        }
        // Sonra ortam değişkenlerini kontrol et
        let val = super::advanced::ENV.get(name);
        // set -u (nounset): tanımsız değişken kullanımında hata ver
        if val.is_none() && *SCRIPT_STATE.nounset.lock() {
            // POSIX: "unbound variable" hatası — script'i sonlandır
            shell_api::serial_println(&alloc::format!(
                "[SCRIPT] set -u: unbound variable: {}",
                name
            ));
        }
        val
    }

    /// Break ve continue bayraklarını temizler.
    ///
    /// Döngü iterasyonunun başında ve döngü çıkışında çağrılır.
    pub fn clear_flags(&self) {
        *self.break_flag.lock() = false;
        *self.continue_flag.lock() = false;
    }

    /// Break bayrağı set edilmiş mi?
    pub fn should_break(&self) -> bool {
        *self.break_flag.lock()
    }

    /// Continue bayrağı set edilmiş mi?
    pub fn should_continue(&self) -> bool {
        *self.continue_flag.lock()
    }
}

/// Global betik çalışma zamanı durumu.
///
/// `spin::Lazy` ile uygulama ömrü boyunca tek bir instance tutulur.
/// Yorumlayıcının her yerde `SCRIPT_STATE.set_local()` gibi çağrılarla
/// erişebileceği merkezi durum deposudur.
pub static SCRIPT_STATE: spin::Lazy<ScriptState> = spin::Lazy::new(|| ScriptState::new());
static SCRIPT_SHELL: spin::Lazy<Mutex<crate::shell::Shell>> = spin::Lazy::new(|| Mutex::new(crate::shell::Shell::new()));

fn reset_script_runtime() {
    SCRIPT_STATE.local_vars.lock().clear();
    SCRIPT_STATE.functions.lock().clear();
    SCRIPT_STATE.clear_flags();
    *SCRIPT_STATE.return_value.lock() = None;
    *SCRIPT_SHELL.lock() = crate::shell::Shell::new();
}

fn run_shell_command(cmd_line: &str) -> (i64, Option<String>) {
    let mut shell = SCRIPT_SHELL.lock();
    let output = crate::shell::run_command_in_shell(&mut shell, cmd_line);
    (crate::shell::command_exit_code(&output), output)
}

/// Betik yorumlayıcısı.
///
/// Durumsuz (stateless) yapıdadır — tüm mutable durum `SCRIPT_STATE`'te tutulur.
/// `execute()`, `exec_stmt()`, `eval_expr()`, `eval_arithmetic()`, `is_truthy()`
/// metotları birbirini çağıran özyinelemeli bir değerlendirme zinciri oluşturur.
pub struct Interpreter;

impl Interpreter {
    /// Deyim listesini yürütür ve son çıkış kodunu döndürür.
    ///
    /// ## Çalıştırma Algoritması
    ///
    /// ```
    /// for her deyim:
    ///     exec_stmt(deyim)
    ///     if return_value ayarlandıysa:
    ///         döngüyü kır, return_value döndür
    ///     if break_flag || continue_flag:
    ///         döngüyü kır (üst döngüye bırak)
    /// ```
    ///
    /// `return_value`, `break_flag` ve `continue_flag` kontrolleri her deyimden
    /// sonra yapılır; bu sayede iç içe döngüler ve fonksiyonlar doğru davranır.
    pub fn execute(stmts: &[Stmt]) -> Result<i64, ScriptError> {
        let mut last_exit_code = 0;

        for stmt in stmts {
            last_exit_code = Self::exec_stmt(stmt)?;

            // set -e (errexit): hata durumunda betiği sonlandır
            if *SCRIPT_STATE.errexit.lock() && last_exit_code != 0 {
                return Err(ScriptError::Exit(last_exit_code));
            }

            // Return/break/continue bayraklarını kontrol et
            if SCRIPT_STATE.return_value.lock().is_some() {
                return Ok(SCRIPT_STATE.return_value.lock().unwrap_or(0));
            }
            if SCRIPT_STATE.should_break() || SCRIPT_STATE.should_continue() {
                break;
            }
        }

        Ok(last_exit_code)
    }

    /// Tek bir deyimi yürütür ve çıkış kodu döndürür.
    ///
    /// Her `Stmt` varyantı için farklı bir yürütme stratejisi uygulanır:
    ///
    /// - `Assign`: `eval_expr()` ile değeri hesapla, `local`/`export` bayrağına göre yaz
    /// - `Command`: argümanları `eval_expr()` ile değerlendir, shell session'ında çalıştır
    /// - `If`: `is_truthy()` ile dalı seç, seçilen gövdeyi `execute()` ile çalıştır
    /// - `While`: `is_truthy()` doğru olduğu sürece `execute(body)` döngüsü
    /// - `For`: her öğe için `set_local(var)` yap, `execute(body)` çalıştır
    /// - `Until`: `is_truthy()` yanlış olduğu sürece `execute(body)` döngüsü
    /// - `Function`: `SCRIPT_STATE.functions`'a kaydet (çalıştırma değil)
    /// - `Return`: `return_value`'yu ayarla
    /// - `Break`: `break_flag`'i true yap
    /// - `Continue`: `continue_flag`'i true yap
    fn exec_stmt(stmt: &Stmt) -> Result<i64, ScriptError> {
        match stmt {
            Stmt::Assign {
                name,
                value,
                local,
                export,
            } => {
                let val = Self::eval_expr(value)?;

                if *local {
                    // local VAR=değer — yalnızca fonksiyon kapsamına yazar
                    SCRIPT_STATE.set_local(name, &val);
                } else if *export {
                    // export VAR=değer — ortam değişkenine aktarır
                    super::advanced::ENV.set(name, &val);
                    SCRIPT_SHELL.lock().set_session_env(name, &val);
                } else {
                    // Normal atama — hem yerel hem ortama yazar
                    SCRIPT_STATE.set_local(name, &val);
                    super::advanced::ENV.set(name, &val);
                    SCRIPT_SHELL.lock().set_session_env(name, &val);
                }

                Ok(0)
            }

            Stmt::Command { args } => {
                // Argümanları değerlendir
                let evaluated: Vec<String> = args
                    .iter()
                    .map(|a| Self::eval_expr(a))
                    .collect::<Result<Vec<_>, _>>()?;

                // set -x (xtrace): çalıştırmadan önce komutu stderr'e yaz
                if *SCRIPT_STATE.xtrace.lock() {
                    let trace_line = evaluated.join(" ");
                    shell_api::serial_println(&alloc::format!("+ {}", trace_line));
                }

                // Fonksiyon çağrısı kontrolü — eğer komut tanımlı bir fonksiyonsa
                if let Some(first) = evaluated.first() {
                    if let Some(func_stmt) =
                        SCRIPT_STATE.functions.lock().get(first.as_str()).cloned()
                    {
                        if let Stmt::Function { params, body, .. } = func_stmt {
                            // Pozisyonel parametreleri ayarla: $1, $2, $3...
                            let saved_locals: Vec<(String, String)> = {
                                let locals = SCRIPT_STATE.local_vars.lock();
                                params
                                    .iter()
                                    .enumerate()
                                    .map(|(i, p)| {
                                        let val = evaluated.get(i + 1).cloned().unwrap_or_default();
                                        (p.clone(), val)
                                    })
                                    .collect()
                            };
                            // Eski local'leri kaydet ve yenisini kur
                            {
                                let mut locals = SCRIPT_STATE.local_vars.lock();
                                for (name, val) in &saved_locals {
                                    locals.insert(name.clone(), val.clone());
                                }
                                // $0 = fonksiyon adı, $# = argüman sayısı
                                locals.insert("$0".to_string(), first.clone());
                                locals.insert("$#".to_string(), (evaluated.len() - 1).to_string());
                            }
                            let result = Self::execute(&body);
                            // Local'leri temizle (fonksiyon kapsamından çık)
                            {
                                let mut locals = SCRIPT_STATE.local_vars.lock();
                                for (name, _) in &saved_locals {
                                    locals.remove(name);
                                }
                                locals.remove("$0");
                                locals.remove("$#");
                            }
                            return result.map(|c| c);
                        }
                    }
                }

                let cmd_line = evaluated.join(" ");
                shell_api::serial_println(&alloc::format!("[SCRIPT] Command: {}", cmd_line));
                let (exit_code, _output) = run_shell_command(&cmd_line);
                Ok(exit_code)
            }

            Stmt::If {
                condition,
                then_body,
                elif_clauses,
                else_body,
            } => {
                if Self::is_truthy(condition)? {
                    // Ana koşul doğru — then gövdesini çalıştır
                    Self::execute(then_body)?;
                } else {
                    let mut executed = false;
                    // elif dallarını sırayla dene
                    for (elif_cond, elif_body) in elif_clauses {
                        if Self::is_truthy(elif_cond)? {
                            Self::execute(elif_body)?;
                            executed = true;
                            break;
                        }
                    }
                    // Hiçbir dal çalışmadıysa else gövdesini çalıştır
                    if !executed {
                        if let Some(body) = else_body {
                            Self::execute(body)?;
                        }
                    }
                }
                Ok(0)
            }

            Stmt::While { condition, body } => {
                SCRIPT_STATE.clear_flags();

                while Self::is_truthy(condition)? && !SCRIPT_STATE.should_break() {
                    Self::execute(body)?;

                    if SCRIPT_STATE.should_continue() {
                        // continue — bayrağı temizle ve sonraki iterasyona geç
                        SCRIPT_STATE.clear_flags();
                        continue;
                    }
                }

                SCRIPT_STATE.clear_flags();
                Ok(0)
            }

            Stmt::For { var, items, body } => {
                SCRIPT_STATE.clear_flags();

                for item in items {
                    let val = Self::eval_expr(item)?;
                    // Döngü değişkenini güncelle
                    SCRIPT_STATE.set_local(var, &val);

                    if SCRIPT_STATE.should_break() {
                        break;
                    }

                    Self::execute(body)?;

                    if SCRIPT_STATE.should_continue() {
                        SCRIPT_STATE.clear_flags();
                        continue;
                    }
                }

                SCRIPT_STATE.clear_flags();
                Ok(0)
            }

            Stmt::Until { condition, body } => {
                SCRIPT_STATE.clear_flags();

                // while'ın tersi: koşul YANLIŞ olduğu sürece çalış
                while !Self::is_truthy(condition)? && !SCRIPT_STATE.should_break() {
                    Self::execute(body)?;

                    if SCRIPT_STATE.should_continue() {
                        SCRIPT_STATE.clear_flags();
                        continue;
                    }
                }

                SCRIPT_STATE.clear_flags();
                Ok(0)
            }

            Stmt::Function { name, params, body } => {
                // Fonksiyonu tanımla — gövdeyi SCRIPT_STATE'e kaydet (çalıştırma değil)
                SCRIPT_STATE.functions.lock().insert(
                    name.clone(),
                    Stmt::Function {
                        name: name.clone(),
                        params: params.clone(),
                        body: body.clone(),
                    },
                );
                Ok(0)
            }

            Stmt::Return(expr) => {
                // return [değer] — return_value bayrağını ayarla
                let code = if let Some(e) = expr {
                    Self::eval_expr(e)?.parse().unwrap_or(0)
                } else {
                    0
                };
                *SCRIPT_STATE.return_value.lock() = Some(code);
                Ok(code)
            }

            Stmt::Break => {
                // break — break_flag'i set et; üst döngü kontrol edecek
                *SCRIPT_STATE.break_flag.lock() = true;
                Ok(0)
            }

            Stmt::Continue => {
                // continue — continue_flag'i set et; üst döngü kontrol edecek
                *SCRIPT_STATE.continue_flag.lock() = true;
                Ok(0)
            }

            Stmt::Case { expr, arms } => {
                // case $expr in pattern) body;; esac
                let value = Self::eval_expr(expr)?;
                let mut result = 0i64;
                for (patterns, body) in arms {
                    let matched = patterns.iter().any(|p| {
                        if p == "*" {
                            true
                        } else if p.contains('*') || p.contains('?') {
                            // Basit glob matching
                            crate::shell::advanced::Glob::matches(p, &value)
                        } else {
                            p == &value
                        }
                    });
                    if matched {
                        for stmt in body {
                            result = Self::exec_stmt(stmt)?;
                        }
                        break; // İlk eşleşen arm'da dur
                    }
                }
                Ok(result)
            }

            Stmt::Nop => Ok(0),
        }
    }

    /// Bir ifadeyi değerlendirerek `String` sonuç döndürür.
    ///
    /// ## Değerlendirme Stratejisi
    ///
    /// Her `Expr` varyantı `String`'e dönüştürülür:
    ///
    /// ```
    /// String(s)       → s (kopyala)
    /// Number(n)       → n.to_string()
    /// Variable(name)  → SCRIPT_STATE.get_var(name)
    /// Arithmetic(e)   → eval_arithmetic(e).to_string()
    /// CommandSub(args)→ komut çıktısı
    /// Binary(op,l,r)  → eval_arithmetic(l) op eval_arithmetic(r) → 0 veya 1 → .to_string()
    /// Unary(op,e)     → eval_arithmetic(e) | op uy → .to_string()
    /// Test(inner)     → is_truthy(inner) → "1" veya "0"
    /// StrCompare      → string karşılaştırma → "1" veya "0"
    /// ```
    fn eval_expr(expr: &Expr) -> Result<String, ScriptError> {
        match expr {
            Expr::String(s) => Ok(s.clone()),
            Expr::Number(n) => Ok(n.to_string()),
            Expr::Variable(name) => SCRIPT_STATE
                .get_var(name)
                .ok_or_else(|| ScriptError::UndefinedVariable(name.clone())),
            Expr::Arithmetic(inner) => {
                // $((ifade)) — iç ifadeyi i64'e çevir, string olarak döndür
                let n = Self::eval_arithmetic(inner)?;
                Ok(n.to_string())
            }
            Expr::CommandSub(args) => {
                // $(komut) — komutu çalıştır ve çıktısını yakala
                let cmd: Vec<String> = args
                    .iter()
                    .map(|a| Self::eval_expr(a))
                    .collect::<Result<Vec<_>, _>>()?;
                let cmd_line = cmd.join(" ");
                shell_api::serial_println(&alloc::format!(
                    "[SCRIPT] Command substitution: {}",
                    cmd_line
                ));
                let (_exit_code, output) = run_shell_command(&cmd_line);
                let output = output.unwrap_or_default();
                // Sondaki newline'ları kaldır (bash davranışı)
                Ok(output.trim_end_matches('\n').to_string())
            }
            Expr::Binary { op, left, right } => {
                // Her iki tarafı i64'e çevir, operatörü uygula
                let l = Self::eval_arithmetic(left)?;
                let r = Self::eval_arithmetic(right)?;

                let result = match op {
                    BinOp::Add => l + r,
                    BinOp::Sub => l - r,
                    BinOp::Mul => l * r,
                    BinOp::Div => {
                        if r == 0 {
                            return Err(ScriptError::DivisionByZero);
                        }
                        l / r
                    }
                    BinOp::Mod => {
                        if r == 0 {
                            return Err(ScriptError::DivisionByZero);
                        }
                        l % r
                    }
                    BinOp::Eq => (l == r) as i64, // 1 veya 0
                    BinOp::Ne => (l != r) as i64,
                    BinOp::Lt => (l < r) as i64,
                    BinOp::Gt => (l > r) as i64,
                    BinOp::Le => (l <= r) as i64,
                    BinOp::Ge => (l >= r) as i64,
                    BinOp::And => (l != 0 && r != 0) as i64,
                    BinOp::Or => (l != 0 || r != 0) as i64,
                };

                Ok(result.to_string())
            }
            Expr::Unary { op, operand } => {
                let n = Self::eval_arithmetic(operand)?;

                let result = match op {
                    UnaryOp::Not => (n == 0) as i64, // !0 = 1, !n = 0
                    UnaryOp::Neg => -n,              // -(n)
                };

                Ok(result.to_string())
            }
            Expr::Test(inner) => {
                // [ ifade ] — is_truthy sonucunu "1" veya "0" olarak döndür
                let truthy = Self::is_truthy(inner)?;
                Ok((truthy as i64).to_string())
            }
            Expr::StrCompare { op, left, right } => {
                // String karşılaştırması — her iki taraf eval_expr ile string'e çevrilir
                let l = Self::eval_expr(left)?;
                let r = Self::eval_expr(right)?;

                let result = match op {
                    StrCompareOp::Eq => l == r,
                    StrCompareOp::Ne => l != r,
                    StrCompareOp::Lt => l < r,
                    StrCompareOp::Gt => l > r,
                    StrCompareOp::Le => l <= r,
                    StrCompareOp::Ge => l >= r,
                    StrCompareOp::Match => l.contains(&r), // Alt string var mı?
                    StrCompareOp::Nmatch => !l.contains(&r), // Alt string yok mu?
                };

                Ok((result as i64).to_string())
            }
        }
    }

    /// Bir ifadeyi `i64` aritmetik değere dönüştürür.
    ///
    /// `eval_expr()` çağrısı ile önce `String` elde edilir,
    /// ardından `parse::<i64>()` ile tam sayıya dönüştürülür.
    ///
    /// Dönüşüm başarısız olursa `ScriptError::RuntimeError` döndürülür.
    fn eval_arithmetic(expr: &Expr) -> Result<i64, ScriptError> {
        let s = Self::eval_expr(expr)?;
        s.parse()
            .map_err(|_| ScriptError::RuntimeError(format!("Not a number: {}", s)))
    }

    /// Bir ifadenin "doğruluk değerini" hesaplar.
    ///
    /// ## Doğruluk Kuralları
    ///
    /// - Boş string (`""`) → **yanlış**
    /// - `"0"` → **yanlış**
    /// - Diğer her string → **doğru**
    ///
    /// Bu, bash'taki `test` komutunun davranışıyla uyumludur:
    /// `0` çıkış kodu = başarı = doğru; diğer değerler = doğru (string olarak)
    fn is_truthy(expr: &Expr) -> Result<bool, ScriptError> {
        let val = Self::eval_expr(expr)?;
        Ok(!val.is_empty() && val != "0")
    }
}

// ============================================================================
// PUBLIC API (GENEL API)
// ============================================================================

/// Bir betik kaynak metnini lex → parse → yorumla pipeline'ından geçirir.
///
/// ## İşlem Sırası
///
/// ```
/// 1. ScriptLexer::tokenize(source)   → Vec<ScriptToken>
/// 2. ScriptParser::parse()           → Vec<Stmt>
/// 3. Interpreter::execute(&stmts)    → i64 (çıkış kodu)
/// ```
///
/// Hata durumunda `Err(ScriptError)` döndürür.
/// Başarıda son çalıştırılan deyimin çıkış kodu döndürülür.
///
/// ## Örnek
///
/// ```
/// let code = run_script("x=5\necho $x").unwrap();
/// assert_eq!(code, 0);
/// ```
pub fn run_script(source: &str) -> Result<i64, ScriptError> {
    reset_script_runtime();
    let tokens = ScriptLexer::tokenize(source);
    let mut parser = ScriptParser::new(tokens);
    let stmts = parser.parse()?;
    Interpreter::execute(&stmts)
}

/// Tek bir komut satırını parse eder, AST döndürür (çalıştırmaz).
///
/// `run_script()` ile farkı: yalnızca parse aşamasını çalıştırır,
/// `Interpreter::execute()` çağırmaz.
/// Hata ayıklama ve komut ön işleme amacıyla kullanılabilir.
pub fn parse_line(line: &str) -> Result<Vec<Stmt>, ScriptError> {
    let tokens = ScriptLexer::tokenize(line);
    let mut parser = ScriptParser::new(tokens);
    parser.parse()
}

/// Tek bir ifadeyi değerlendirerek `String` sonuç döndürür.
///
/// `shell/mod.rs`'teki `eval` komutu tarafından kullanılır:
/// ```
/// eval 2 + 3 * 4   →  "14"
/// eval $HOME       →  "/root"
/// ```
///
/// Tokenize → parse (tek ifade) → eval_expr pipeline'ı çalıştırır.
pub fn eval_expression(expr_str: &str) -> Result<String, ScriptError> {
    let tokens = ScriptLexer::tokenize(expr_str);
    let mut parser = ScriptParser::new(tokens);
    let expr = parser.parse_expr()?;
    Interpreter::eval_expr(&expr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::advanced;

    fn scripting_test_epoch() -> spin::MutexGuard<'static, ()> {
        let guard = crate::shell::shell_global_test_epoch();
        advanced::reset_advanced_test_globals();
        guard
    }

    #[test]
    fn lexer_emits_command_sub_end_for_assignment_rhs() {
        let _epoch = scripting_test_epoch();
        let tokens = ScriptLexer::tokenize("MSG=$(echo $FOO)\n");
        assert!(tokens.contains(&ScriptToken::CommandSubStart));
        assert!(tokens.contains(&ScriptToken::CommandSubEnd));
    }

    #[test]
    fn script_command_substitution_reuses_shell_session_env() {
        let _epoch = scripting_test_epoch();
        let result = run_script("export FOO=alpha\nMSG=$(echo $FOO)\n").unwrap();
        assert_eq!(result, 0);
        assert_eq!(SCRIPT_STATE.get_var("MSG"), Some(String::from("alpha")));
    }

    #[test]
    fn script_commands_execute_against_real_shell_session() {
        let _epoch = scripting_test_epoch();
        let result = run_script("export FOO=alpha\necho $FOO\n").unwrap();
        assert_eq!(result, 0);
        assert_eq!(advanced::ENV.get("FOO"), Some(String::from("alpha")));
    }
}
