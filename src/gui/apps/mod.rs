//! # GUI Applications
//!
//! Built-in applications for echOS desktop environment

/// Settings application
pub mod settings;

/// File Explorer application
pub mod file_explorer;

/// Image Viewer application
pub mod image_viewer;

/// Text Editor application
pub mod text_editor;

/// Music Player application
pub mod music_player;

/// Finder-like file browser
pub mod finder;

/// Safari-like web browser
pub mod browser;

/// System Preferences app
pub mod system_preferences;

/// Terminal app with tabs and themes
pub mod terminal;

/// Calculator app
pub mod calculator;

/// Preview app for documents
pub mod preview;

/// Activity Monitor app
pub mod activity_monitor;

/// Font Book app
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
