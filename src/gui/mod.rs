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

/// File manager widget
pub mod file_manager;

/// Start menu widget
pub mod start_menu;

/// System tray icons
pub mod system_tray;

/// Notification system
pub mod notification;

/// Window manager (minimize, maximize, resize)
pub mod window_manager;

/// Font rendering (TrueType, rasterizer, layout)
pub mod font;

/// Animation system (easing, timeline, frame pacing)
pub mod animation;

/// Retained mode widget tree with dirty tracking
pub mod widget_tree;

/// Glyph atlas with subpixel antialiasing
pub mod glyph_atlas;

/// Desktop icons system
pub mod desktop_icons;

/// Enhanced taskbar with start menu and system tray
pub mod taskbar;

/// Built-in applications
pub mod apps;

/// macOS-style Dock with magnification
pub mod dock;

/// Global menu bar with app menus
pub mod menu_bar;

/// Spotlight-style global search overlay
pub mod spotlight;

/// Notification Center with widgets
pub mod notification_center;

/// Control Center panel (quick settings)
pub mod control_center;

/// Launchpad app grid launcher
pub mod launchpad;

/// Window shadows and blur effects
pub mod effects;

/// Mission Control (window overview)
pub mod mission_control;

/// Virtual Desktops/Spaces support
pub mod spaces;

/// Screenshot tool with selection
pub mod screenshot;

/// File dialogs (Open/Save)
pub mod dialogs;

/// Desktop wallpapers with transitions
pub mod wallpaper;

/// Login screen with user selection
pub mod login;

/// Drag and drop support
pub mod drag_drop;

/// Clipboard manager
pub mod clipboard;

pub use desktop::Desktop;
pub use theme::Theme;
pub use window::Window;
pub use widgets::Rect;
