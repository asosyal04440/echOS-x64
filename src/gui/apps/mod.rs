//! # GUI Applications
//!
//! Built-in applications for echOS desktop environment
//!
//! Bu modül, echOS masaüstü ortamının yerleşik uygulamalarını barındırır.
//! Her uygulama kendi alt modülünde tanımlanmış olup `pub use` ile dışa aktarılır.
//!
//! Rust'ta modül sistemi: `pub mod` ile bir alt modül dışarıya açılır,
//! `pub use` ile ise o modülün içindeki tipler bu modül düzeyinden
//! doğrudan erişilebilir hale getirilir (re-export).

/// Ayarlar uygulaması - sistem tercihlerini yönetir
pub mod settings;

/// Dosya gezgini uygulaması - dizin gezinme ve dosya işlemleri
pub mod file_explorer;

/// Resim görüntüleyici - zoom, pan ve slayt gösterisi destekler
pub mod image_viewer;

/// Metin düzenleyici uygulaması
pub mod text_editor;

/// Müzik çalar uygulaması
pub mod music_player;

/// macOS Finder benzeri dosya tarayıcısı (kenar çubuğu, sekmeler, sütun görünümü)
pub mod finder;

/// Safari benzeri sekmeli web tarayıcısı
pub mod browser;

/// Sistem tercihleri uygulaması
pub mod system_preferences;

/// Sekmeli ve temalı terminal uygulaması
pub mod terminal;

/// Belgeler için önizleme uygulaması
pub mod preview;

/// Sistem etkinlik izleyicisi - CPU, bellek, disk ve ağ kullanımını gösterir
pub mod activity_monitor;

/// Font kitaplığı - yazı tiplerini görüntüleme ve yönetme
pub mod font_book;

pub use settings::SettingsApp;
pub use file_explorer::FileExplorer;
pub use image_viewer::ImageViewer;
pub use text_editor::TextEditor;
pub use music_player::MusicPlayer;
pub use finder::FinderWindow;
pub use browser::BrowserWindow;
pub use system_preferences::SystemPreferences;
pub use terminal::TerminalWindow;
pub use preview::PreviewWindow;
pub use activity_monitor::ActivityMonitor;
pub use font_book::FontBookWindow;
