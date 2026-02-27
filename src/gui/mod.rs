//! # echOS GUI Framework
//!
//! Bare-metal grafiksel kullanıcı arayüzü.
//! Window yönetimi, tema sistemi ve widget desteği.

/// Fare imleci çizimi
pub mod cursor;

/// Masaüstü ortamı (arka plan, görev çubuğu)
pub mod desktop;

/// Pencere bileşeni (başlık çubuğu, içerik alanı)
pub mod window;

/// Renk teması (VS Code inspired dark theme)
pub mod theme;

/// Widget sistemi (button, label, matrix)
pub mod widgets;

/// Dosya yöneticisi widget'ı
pub mod file_manager;

/// Başlat menüsü widget'ı
pub mod start_menu;

/// Sistem tepsisi simgeleri
pub mod system_tray;

/// Bildirim sistemi
pub mod notification;

/// Pencere yöneticisi (küçültme, büyütme, yeniden boyutlandırma)
pub mod window_manager;

/// Yazı tipi işleme (TrueType, rasterleştirici, düzen)
pub mod font;

/// Animasyon sistemi (yumuşatma, zaman çizelgesi, kare hızlandırma)
pub mod animation;

/// Kirlilik takibiyle korunan mod widget ağacı
pub mod widget_tree;

/// Alt piksel kenar yumuşatmalı glyph atlası
pub mod glyph_atlas;

/// Masaüstü simgeleri sistemi
pub mod desktop_icons;

/// Pencere döşeme ve yaslama
pub mod tiling;

/// Sanal masaüstü desteği
pub mod virtual_desktop;

/// Başlat menüsü ve sistem tepsili gelişmiş görev çubuğu
pub mod taskbar;

/// Yerleşik uygulamalar
pub mod apps;

/// Büyütme efektli macOS tarzı Dock
pub mod dock;

/// Uygulama menülü global menü çubuğu
pub mod menu_bar;

/// Spotlight tarzı global arama katmanı
pub mod spotlight;

/// Widget'lı Bildirim Merkezi
pub mod notification_center;

/// Kontrol Merkezi paneli (hızlı ayarlar)
pub mod control_center;

/// Launchpad uygulama ızgara başlatıcısı
pub mod launchpad;

/// Pencere gölgeleri ve bulanıklık efektleri
pub mod effects;

/// Görev Kontrolü (pencere genel görünümü)
pub mod mission_control;

/// Sanal Masaüstleri/Uzaylar desteği
pub mod spaces;

/// Seçimli ekran görüntüsü aracı
pub mod screenshot;

/// Dosya iletişim kutuları (Aç/Kaydet)
pub mod dialogs;

/// Geçişli masaüstü duvar kağıtları
pub mod wallpaper;

/// Kullanıcı seçimli giriş ekranı
pub mod login;

/// Sürükle ve bırak desteği
pub mod drag_drop;

/// Pano yöneticisi
pub mod clipboard;

pub use desktop::Desktop;
pub use theme::Theme;
pub use window::Window;
pub use widgets::Rect;
