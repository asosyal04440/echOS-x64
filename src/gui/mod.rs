//! # echOS GUI Framework
//! 
//! Bare-metal grafiksel kullanıcı arayüzü.
//! Window yönetimi, tema sistemi ve widget desteği.

/// Mouse cursor rendering
pub mod cursor;

/// Desktop environment (background, taskbar)
pub mod desktop;

/// Window component (title bar, content area)
pub mod window;

/// Renk teması (VS Code inspired dark theme)
pub mod theme;

/// Widget sistemi (button, label, matrix)
pub mod widgets;

pub use desktop::Desktop;
pub use window::Window;
pub use theme::Theme;
