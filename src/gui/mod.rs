//! # echOS GUI Framework
//!
//! Bare-metal grafiksel kullanıcı arayüzü.
//! Window yönetimi, tema sistemi ve widget desteği.
//! Tüm GUI bileşenleri bu modül altında alt modüller biçiminde toplanmıştır.

/// Cyber-Industrial WM temel veri yapıları (WindowId, WindowFrame, SnapTarget, ShortcutId vs.)
pub mod echos_wm;

/// Borderless pencere kontrolleri için üst komut çubuğu
pub mod global_command_bar;

/// Üst sistem paneli: CPU sparkline, RAM bar, workspace göstergesi, saat
pub mod cyber_panel;

/// Kullanıcı-alanı ELF süreçleri için çekirdek pencere sunucusu (Faz 5)
pub mod win_server;

/// Fare imleci çizimi.
/// Donanım imleci yerine yazılımsal sprite olarak framebuffer üzerine çizilir.
pub mod cursor;

/// Masaüstü ortamı (arka plan ve görev çubuğu).
/// Sistemin görsel temelini oluşturur; tüm pencereler bu katmanın üzerinde görünür.
pub mod desktop;

/// Pencere bileşeni (başlık çubuğu ve içerik alanı).
/// Pencerelerin çerçevesini, yeniden boyutlandırma tutamaçlarını ve içerik alanını tanımlar.
pub mod window;

/// Renk teması (VS Code ilhamlı koyu tema).
/// `Color` yapısı ile `Theme` sabitleri tüm GUI bileşenleri tarafından paylaşılır.
pub mod theme;

/// Widget sistemi (button, label, matrix).
/// Tekrar kullanılabilir UI bileşenlerinin temel katmanı; `Widget` trait'i ile genişletilebilir.
pub mod widgets;

/// Dosya yöneticisi widget'ı.
/// Dizin gezginini ve dosya listeleme bileşenlerini içerir.
pub mod file_manager;

/// Başlat menüsü widget'ı.
/// Uygulama başlatma arayüzünü sağlar.
pub mod start_menu;

/// Sistem tepsisi simgeleri.
/// Ağ, ses, pil gibi durum simgelerini görev çubuğunda gösterir.
pub mod system_tray;

/// Bildirim sistemi.
/// Toast bildirimleri ve uyarı popup'larını yönetir.
pub mod notification;

/// Pencere yöneticisi (küçültme, büyütme, yeniden boyutlandırma).
/// Pencerelerin yaşam döngüsünü ve konumunu yönetir.
pub mod window_manager;

/// Font çizimi (TrueType, rasterizer, layout).
/// Glyph tabanlı metin çizimi ve satır düzeni hesaplamalarını içerir.
pub mod font;

/// Animasyon sistemi (easing, timeline, kare hızı yönetimi).
/// Pürüzsüz geçişler ve zamanlı animasyonlar için altyapı sağlar.
pub mod animation;

/// Kirlilik takipli (dirty tracking) widget ağacı.
/// Yalnızca değişen widget'ların yeniden çizilmesini sağlayarak performansı artırır.
pub mod widget_tree;

/// Alt piksel kenar yumuşatmalı glyph atlası.
/// Glyph'leri bir doku atlasında önbelleğe alarak metin çizimini hızlandırır.
pub mod glyph_atlas;

/// Masaüstü simgeleri sistemi.
/// Masaüstünde dosya ve uygulama simgelerini yerleştirir ve tıklama olaylarını işler.
pub mod desktop_icons;

/// Başlat menüsü ve sistem tepsisiyle geliştirilmiş görev çubuğu.
/// Açık uygulamaları, sistem bilgilerini ve hızlı erişim öğelerini barındırır.
pub mod taskbar;

/// Yerleşik uygulamalar.
/// echOS ile birlikte gelen temel uygulamaları (metin editörü, terminaL vb.) içerir.
pub mod apps;

/// Büyütme efektli macOS tarzı dock.
/// Uygulama başlatıcı çubuğu; fare üzerine gelince simgeler büyür.
pub mod dock;

/// Uygulama menüleriyle global menü çubuğu.
/// Ekranın üstünde sabit konumda, etkin uygulamanın menülerini gösterir.
pub mod menu_bar;

/// Spotlight tarzı global arama katmanı.
/// Klavye kısayoluyla tetiklenen, uygulama/dosya/ayar arama paneli.
pub mod spotlight;

/// Widget'lı bildirim merkezi.
/// macOS tarzı sağ panel; bildirimler, takvim ve hava durumu widget'larını barındırır.
pub mod notification_center;

/// Kontrol merkezi paneli (hızlı ayarlar).
/// WiFi, Bluetooth, parlaklık gibi sistem ayarlarına tek tıkla erişim sağlar.
pub mod control_center;

/// Uygulama ızgara başlatıcı (Launchpad).
/// Tüm kurulu uygulamaları sayfalı ızgara düzeninde listeler.
pub mod launchpad;

/// Pencere gölgeleri ve bulanıklık efektleri.
/// Pencerelere derinlik hissi katan görsel katman efektleri.
pub mod effects;

/// Mission Control (pencere genel görünümü).
/// Tüm açık pencereleri ve masaüstü alanlarını kuşbakışı gösterir.
pub mod mission_control;

/// Sanal masaüstü desteği (Spaces).
/// Birden fazla masaüstü alanı oluşturmayı ve aralarında geçiş yapmayı sağlar.
pub mod spaces;

/// Seçim bölgesiyle ekran görüntüsü alma aracı.
/// Kullanıcının istediği alanı seçip ekran görüntüsü almasını sağlar.
pub mod screenshot;

/// Dosya iletişim kutuları (Aç / Kaydet).
/// Standart dosya seçici ve kaydet diyalog bileşenlerini içerir.
pub mod dialogs;

/// Geçiş efektleriyle masaüstü duvar kağıtları.
/// Animasyonlu, dinamik ve slayt gösterisi duvar kağıtlarını yönetir.
pub mod wallpaper;

/// Kullanıcı seçimli oturum açma ekranı.
/// Sistem başlangıcında kullanıcı kimlik doğrulamasını sunar.
pub mod login;

/// Sürükle ve bırak desteği.
/// GUI bileşenleri arasında veri transferini mümkün kılan etkileşim katmanı.
pub mod drag_drop;

/// Pano yöneticisi.
/// Kesme, kopyalama ve yapıştırma işlemleri için geçici veri deposu.
pub mod clipboard;

pub use desktop::Desktop;
pub use theme::Theme;
pub use window::Window;
pub use widgets::Rect;
