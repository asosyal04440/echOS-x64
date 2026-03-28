//! # echOS Kap (Container) Widget'ları
//!
//! Düzen yönetimi için Panel, TabControl ve Splitter bileşenleri.
//!
//! ## Kap Widget Kavramı
//!
//! Kap widget'ları, içinde başka widget'ları barındıran ve onların yerleşimini
//! yöneten bileşenlerdir. Olaylar (tıklama, klavye, hover) önce kaba iletilir;
//! kap bunları `children` listesi üzerinden alt widget'lara yayar (event propagation).
//!
//! ## `Box<dyn Widget + 'a>` Neden Gerekli?
//!
//! Farklı türdeki widget'ları (`Button`, `Label`, `CheckBox` vb.) aynı vektörde
//! tutmak için trait object (`dyn Widget`) kullanılır. `Box` heap tahsisi yaparak
//! farklı boyutlu türlerin tek tip pointer ile saklanmasını sağlar. `'a` lifetime
//! parametresi, kap ile içindeki widget'ların yaşam sürelerini ilişkilendirir.
//!
//! ## Builder Pattern
//!
//! `with_title`, `with_background` gibi metodlar `mut self → Self` döndürerek
//! zincirleme yapılandırma imkanı sunar. Bu Rust'ta yaygın bir ergonomi kalıbıdır.

use super::{
    border_rect_objects, draw_render_objects, solid_rect_object, text_render_object_with_width,
    Rect, Widget,
};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::protocol::{DamageLane, RenderObject};
use crate::gui::theme::Theme;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// Panel kap widget'ı; alt widget'ları gruplayan temel kap.
///
/// `children: Vec<Box<dyn Widget + 'a>>` alanı farklı türlerdeki widget'ları
/// tek vektörde tutar. `title: Option<String>` başlık çubuğunu isteğe bağlı
/// yapar; `None` ise başlık çubuğu çizilmez, `Some(s)` ise başlık gösterilir.
pub struct Panel<'a> {
    rect: Rect,
    children: Vec<Box<dyn Widget + 'a>>,
    background: u32,
    border: bool,
    title: Option<String>,
    /// İç boşluk (padding) — [top, right, bottom, left] piksel
    padding: [i32; 4],
    /// Dış boşluk (margin) — [top, right, bottom, left] piksel
    margin: [i32; 4],
}

impl<'a> Panel<'a> {
    /// Yeni panel oluşturur; kenarlıklı, başlıksız varsayılan yapılandırma ile.
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            rect: Rect::new(x, y, width, height),
            children: Vec::new(),
            background: Theme::WINDOW_BG.to_u32(),
            border: true,
            title: None,
            padding: [0; 4],
            margin: [0; 4],
        }
    }

    /// Builder: panele başlık çubuğu ekler.
    pub fn with_title(mut self, title: &str) -> Self {
        self.title = Some(String::from(title));
        self
    }

    /// Builder: arka plan rengini özelleştirir.
    pub fn with_background(mut self, color: u32) -> Self {
        self.background = color;
        self
    }

    /// Builder: kenarlık görünümünü açar veya kapatır.
    pub fn with_border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    /// Builder: dört taraflı iç boşluk (padding) ayarlar.
    /// Sıralama: [top, right, bottom, left].
    pub fn with_padding(mut self, top: i32, right: i32, bottom: i32, left: i32) -> Self {
        self.padding = [top, right, bottom, left];
        self
    }

    /// Builder: tek değerli eşit iç boşluk ayarlar.
    pub fn with_padding_all(mut self, p: i32) -> Self {
        self.padding = [p, p, p, p];
        self
    }

    /// Builder: dört taraflı dış boşluk (margin) ayarlar.
    pub fn with_margin(mut self, top: i32, right: i32, bottom: i32, left: i32) -> Self {
        self.margin = [top, right, bottom, left];
        self
    }

    /// Builder: tek değerli eşit dış boşluk ayarlar.
    pub fn with_margin_all(mut self, m: i32) -> Self {
        self.margin = [m, m, m, m];
        self
    }

    /// Padding uygulandıktan sonra kullanılabilir iç Rect'i döndürür.
    pub fn content_rect(&self) -> Rect {
        let title_offset = if self.title.is_some() { 24 } else { 0 };
        Rect::new(
            self.rect.x + self.margin[3] + self.padding[3],
            self.rect.y + self.margin[0] + self.padding[0] + title_offset,
            self.rect.width - self.margin[1] - self.margin[3] - self.padding[1] - self.padding[3],
            self.rect.height
                - self.margin[0]
                - self.margin[2]
                - self.padding[0]
                - self.padding[2]
                - title_offset,
        )
    }

    /// Alt widget ekler. `Box<dyn Widget>` alarak trait object sahipliğini devralır.
    pub fn add_child(&mut self, child: Box<dyn Widget + 'a>) {
        self.children.push(child);
    }

    /// Tüm alt widget'ları kaldırır.
    pub fn clear_children(&mut self) {
        self.children.clear();
    }

    /// Alt widget'lara salt okunur erişim sağlar.
    pub fn children(&self) -> &Vec<Box<dyn Widget + 'a>> {
        &self.children
    }

    /// Alt widget'lara değiştirilebilir erişim sağlar.
    pub fn children_mut(&mut self) -> &mut Vec<Box<dyn Widget + 'a>> {
        &mut self.children
    }

    fn render_primitives(&self) -> Vec<RenderObject> {
        let mut objects = Vec::new();
        let rect = Rect::new(
            self.rect.x + self.margin[3],
            self.rect.y + self.margin[0],
            self.rect.width - self.margin[1] - self.margin[3],
            self.rect.height - self.margin[0] - self.margin[2],
        );
        let base_id = ((rect.x as u64) << 32) ^ (rect.y as u64);
        objects.push(solid_rect_object(
            base_id,
            rect,
            self.background,
            DamageLane::Window,
            0,
        ));
        if let Some(title) = &self.title {
            objects.push(solid_rect_object(
                base_id ^ 0x10,
                Rect::new(rect.x, rect.y, rect.width, 24),
                Theme::TITLEBAR_BG.to_u32(),
                DamageLane::Window,
                1,
            ));
            objects.push(text_render_object_with_width(
                base_id ^ 0x11,
                Rect::new(rect.x + 8, rect.y + 4, (rect.width - 16).max(1), 18),
                title,
                Theme::TEXT_PRIMARY.to_u32(),
                false,
                DamageLane::Text,
                2,
            ));
        }
        if self.border {
            objects.extend(border_rect_objects(
                base_id ^ 0x20,
                rect,
                Theme::BORDER.to_u32(),
                DamageLane::Window,
                3,
            ));
        }
        objects
    }
}

impl<'a> Widget for Panel<'a> {
    fn draw(&self, fb: &mut Framebuffer) {
        let objects = self.render_primitives();
        draw_render_objects(fb, self.rect, &objects);

        // Alt widget'ları çiz: her biri kendi `draw` metoduyla kendini çizer.
        // Bu, polimorfizmin güzel bir örneğidir; hangi widget türü olduğuna
        // bakılmaksızın aynı `draw` çağrısı doğru implementasyonu yürütür.
        for child in &self.children {
            child.draw(fb);
        }
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if !self.rect.contains(x, y) {
            return false;
        }

        // Alt widget'lara olayı ters sırayla ilet: en son eklenen (üstte görünen)
        // widget önce kontrol edilir. Bu, Z-order (derinlik sırası) mantığını
        // basitçe uygular; üst üste widget'larda en üstteki olayı yakalar.
        for child in self.children.iter_mut().rev() {
            if child.on_click(x, y) {
                return true;
            }
        }
        true
    }

    /// Klavye olayını odaklı alt widget'a iletir.
    ///
    /// İlk `true` döndüren alt widget'ta yayılım durur; bu event bubbling'in
    /// tersine "event sinking" (olay batma) kalıbıdır.
    fn on_key(&mut self, key: char, modifiers: u8, scancode: u8) -> bool {
        for child in &mut self.children {
            if child.on_key(key, modifiers, scancode) {
                return true;
            }
        }
        false
    }

    /// Hover olayını tüm alt widget'lara iletir; herhangi biri değiştiyse true döner.
    ///
    /// Hover birden fazla widget'ı etkileyebilir (biri hover'a girerken diğeri çıkar),
    /// bu yüzden tüm alt widget'lar kontrol edilir ve herhangi bir değişim `changed`
    /// ile takip edilir.
    fn on_hover(&mut self, x: i32, y: i32) -> bool {
        let mut changed = false;
        for child in &mut self.children {
            if child.on_hover(x, y) {
                changed = true;
            }
        }
        changed
    }

    /// Scroll olayını ilk kabul eden alt widget'ta durdurur.
    fn on_scroll(&mut self, delta: i32) -> bool {
        for child in &mut self.children {
            if child.on_scroll(delta) {
                return true;
            }
        }
        false
    }

    fn bounds(&self) -> Rect {
        self.rect
    }

    /// Tüm alt widget'ların animasyon durumlarını günceller.
    fn update(&mut self) {
        for child in &mut self.children {
            child.update();
        }
    }

    fn render_objects(&self) -> Vec<RenderObject> {
        self.render_primitives()
    }
}

/// Sekme sayfası; `TabControl`'ün her bir sekmesini temsil eder.
///
/// Her sekme bir başlık (`title`) ve içerik alanı (`panel`) içerir.
/// `TabControl` sekme başlıklarını üstte çizer; aktif sekmenin panelini
/// içerik alanında gösterir.
pub struct TabPage<'a> {
    title: String,
    panel: Panel<'a>,
}

impl<'a> TabPage<'a> {
    /// Yeni sekme sayfası oluşturur; boş panel ile başlar.
    pub fn new(title: &str) -> Self {
        Self {
            title: String::from(title),
            panel: Panel::new(0, 0, 0, 0),
        }
    }

    /// Builder: içerik panelini ayarlar.
    pub fn with_content(mut self, panel: Panel<'a>) -> Self {
        self.panel = panel;
        self
    }

    /// Sekme başlığına referans döndürür.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// İçerik paneline salt okunur erişim.
    pub fn panel(&self) -> &Panel<'a> {
        &self.panel
    }

    /// İçerik paneline değiştirilebilir erişim.
    pub fn panel_mut(&mut self) -> &mut Panel<'a> {
        &mut self.panel
    }
}

/// Sekme kontrolü widget'ı; sekmeli arayüz yönetimi.
///
/// `active_tab` hangi sekmenin içeriğinin gösterileceğini belirler.
/// `tab_height` sekme başlıklarının piksel cinsinden yüksekliğidir.
/// `hovered_tab: Option<usize>` imlecin hangi sekmenin üzerinde olduğunu tutar.
/// `on_tab_change` sekme değişiminde tetiklenen isteğe bağlı callback'tir.
pub struct TabControl<'a> {
    rect: Rect,
    tabs: Vec<TabPage<'a>>,
    active_tab: usize,
    tab_height: usize,
    hovered_tab: Option<usize>,
    on_tab_change: Option<fn(usize)>,
}

impl<'a> TabControl<'a> {
    /// Yeni sekme kontrolü oluşturur; başlangıçta sekme yok.
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            rect: Rect::new(x, y, width, height),
            tabs: Vec::new(),
            active_tab: 0,
            tab_height: 28,
            hovered_tab: None,
            on_tab_change: None,
        }
    }

    /// Yeni sekme sayfası ekler.
    pub fn add_tab(&mut self, tab: TabPage<'a>) {
        self.tabs.push(tab);
    }

    /// Builder: sekme değişim handler'ı ekler.
    pub fn with_tab_change_handler(mut self, handler: fn(usize)) -> Self {
        self.on_tab_change = Some(handler);
        self
    }

    /// Aktif sekmenin indeksini döndürür.
    pub fn active_tab(&self) -> usize {
        self.active_tab
    }

    /// Aktif sekmeyi değiştirir; callback varsa tetikler.
    pub fn set_active_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active_tab = index;
            if let Some(handler) = self.on_tab_change {
                handler(index);
            }
        }
    }

    /// Belirtilen sekmenin başlık dikdörtgenini hesaplar.
    ///
    /// Her sekmenin genişliği metin uzunluğuna bağlıdır: `len * 8 + 24`.
    /// Önceki sekmelerin genişlikleri toplanarak x konumu bulunur (kümülatif
    /// yerleşim hesabı). Bu dinamik sekme genişliği sistemidir.
    fn tab_rect(&self, index: usize) -> Rect {
        let mut tab_x = self.rect.x;
        for i in 0..index {
            let title = &self.tabs[i].title;
            tab_x += (title.len() * 8 + 24) as i32;
        }
        let title = &self.tabs[index].title;
        let tab_width = (title.len() * 8 + 24) as i32;

        Rect::new(tab_x, self.rect.y, tab_width, self.tab_height as i32)
    }

    /// Sekme başlıkları altında kalan içerik alanının dikdörtgenini döndürür.
    fn content_rect(&self) -> Rect {
        Rect::new(
            self.rect.x,
            self.rect.y + self.tab_height as i32,
            self.rect.width,
            self.rect.height - self.tab_height as i32,
        )
    }

    fn render_primitives(&self) -> Vec<RenderObject> {
        let mut objects = Vec::new();
        let base_id = ((self.rect.x as u64) << 32) ^ (self.rect.y as u64) ^ 0x1000_0000;
        objects.push(solid_rect_object(
            base_id,
            Rect::new(
                self.rect.x,
                self.rect.y,
                self.rect.width,
                self.tab_height as i32,
            ),
            Theme::TITLEBAR_BG.to_u32(),
            DamageLane::Window,
            0,
        ));
        objects.push(solid_rect_object(
            base_id ^ 0x01,
            Rect::new(
                self.rect.x,
                self.rect.y + self.tab_height as i32 - 1,
                self.rect.width,
                1,
            ),
            Theme::BORDER.to_u32(),
            DamageLane::Window,
            1,
        ));

        for (i, tab) in self.tabs.iter().enumerate() {
            let tab_rect = self.tab_rect(i);
            let bg_color = if i == self.active_tab {
                Theme::WINDOW_BG.to_u32()
            } else if self.hovered_tab == Some(i) {
                Theme::BUTTON_HOVER.to_u32()
            } else {
                Theme::TITLEBAR_BG.to_u32()
            };
            objects.push(solid_rect_object(
                base_id ^ 0x100 ^ i as u64,
                tab_rect,
                bg_color,
                DamageLane::Window,
                2,
            ));
            objects.extend(border_rect_objects(
                base_id ^ 0x200 ^ i as u64,
                tab_rect,
                Theme::BORDER.to_u32(),
                DamageLane::Window,
                3,
            ));
            let text_color = if i == self.active_tab {
                Theme::TEXT_PRIMARY.to_u32()
            } else {
                Theme::TEXT_SECONDARY.to_u32()
            };
            objects.push(text_render_object_with_width(
                base_id ^ 0x300 ^ i as u64,
                Rect::new(
                    tab_rect.x + 12,
                    tab_rect.y + ((tab_rect.height - 16).max(0) / 2),
                    (tab_rect.width - 24).max(1),
                    18,
                ),
                &tab.title,
                text_color,
                false,
                DamageLane::Text,
                4,
            ));
        }

        let content = self.content_rect();
        objects.push(solid_rect_object(
            base_id ^ 0x400,
            content,
            Theme::WINDOW_BG.to_u32(),
            DamageLane::Window,
            1,
        ));
        objects.extend(border_rect_objects(
            base_id ^ 0x500,
            content,
            Theme::BORDER.to_u32(),
            DamageLane::Window,
            2,
        ));

        objects
    }
}

impl<'a> Widget for TabControl<'a> {
    fn draw(&self, fb: &mut Framebuffer) {
        let objects = self.render_primitives();
        draw_render_objects(fb, self.rect, &objects);

        // Aktif sekme içeriğini çiz: yalnızca aktif sekmenin paneli görünür.
        // `is_empty()` ve bounds kontrolü index-out-of-bounds güvenliği sağlar.
        if !self.tabs.is_empty() && self.active_tab < self.tabs.len() {
            // Update content panel position
            let panel = &self.tabs[self.active_tab].panel;
            // Note: In a real implementation, we'd need interior mutability here
            // For now, just draw the panel as-is
            panel.draw(fb);
        }
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if !self.rect.contains(x, y) {
            return false;
        }

        // Sekme başlıklarını kontrol et: tıklanan sekmeyi aktifleştir
        for i in 0..self.tabs.len() {
            if self.tab_rect(i).contains(x, y) {
                self.set_active_tab(i);
                return true;
            }
        }

        // Sekme içeriğine tıklama: aktif sekmenin paneline ilet
        if self.active_tab < self.tabs.len() {
            self.tabs[self.active_tab].panel_mut().on_click(x, y);
        }
        true
    }

    /// Hover durumunu günceller; hangi sekme üzerinde olunduğunu takip eder.
    ///
    /// `old_hovered != self.hovered_tab` karşılaştırması `Option<usize>` üzerinde
    /// çalışır; Rust `PartialEq` türetmesi `Option`'ı değer bazında karşılaştırır.
    fn on_hover(&mut self, x: i32, y: i32) -> bool {
        let old_hovered = self.hovered_tab;

        self.hovered_tab = None;
        for i in 0..self.tabs.len() {
            if self.tab_rect(i).contains(x, y) {
                self.hovered_tab = Some(i);
                break;
            }
        }

        old_hovered != self.hovered_tab
    }

    fn bounds(&self) -> Rect {
        self.rect
    }

    /// Yalnızca aktif sekmenin güncellenmesi yeterlidir; görünmeyen sekmeler atlanır.
    fn update(&mut self) {
        if self.active_tab < self.tabs.len() {
            self.tabs[self.active_tab].panel_mut().update();
        }
    }

    fn render_objects(&self) -> Vec<RenderObject> {
        self.render_primitives()
    }
}

/// Bölümleme yönü: yatay veya dikey.
///
/// `#[derive(PartialEq, Eq)]` ile `==` karşılaştırması otomatik türetilir;
/// bu `match` ve `if orientation == ...` ifadelerinde kullanılır.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitOrientation {
    Horizontal,
    Vertical,
}

/// Ayırıcı (splitter) widget'ı; yeniden boyutlandırılabilir iki panelli kap.
///
/// `split_pos` bölme çizgisinin konumunu tutar. Yatay bölmede bu konum sol
/// panelin genişliğidir; dikey bölmede üst panelin yüksekliğidir.
/// `dragging` bayrağı kullanıcının ayırıcıyı sürüklediğini gösterir.
/// `min_first` ve `min_second` minimum panel boyutlarını sınırlar.
pub struct Splitter<'a> {
    rect: Rect,
    orientation: SplitOrientation,
    split_pos: i32,
    min_first: i32,
    min_second: i32,
    first: Option<Box<dyn Widget + 'a>>,
    second: Option<Box<dyn Widget + 'a>>,
    dragging: bool,
    splitter_size: i32,
}

impl<'a> Splitter<'a> {
    /// Yeni ayırıcı oluşturur; bölme konumu başlangıçta ortada.
    ///
    /// `match orientation` ile yatay/dikey yönlere göre başlangıç bölme
    /// konumu seçilir. `width / 2` veya `height / 2` tam ortayı verir.
    pub fn new(x: i32, y: i32, width: i32, height: i32, orientation: SplitOrientation) -> Self {
        let split_pos = match orientation {
            SplitOrientation::Horizontal => width / 2,
            SplitOrientation::Vertical => height / 2,
        };

        Self {
            rect: Rect::new(x, y, width, height),
            orientation,
            split_pos,
            min_first: 50,
            min_second: 50,
            first: None,
            second: None,
            dragging: false,
            splitter_size: 5,
        }
    }

    /// Builder: birinci (sol/üst) panel widget'ını ayarlar.
    pub fn with_first(mut self, widget: Box<dyn Widget + 'a>) -> Self {
        self.first = Some(widget);
        self
    }

    /// Builder: ikinci (sağ/alt) panel widget'ını ayarlar.
    pub fn with_second(mut self, widget: Box<dyn Widget + 'a>) -> Self {
        self.second = Some(widget);
        self
    }

    /// Builder: bölme konumunu piksel cinsinden ayarlar.
    pub fn with_split_pos(mut self, pos: i32) -> Self {
        self.split_pos = pos;
        self
    }

    /// Builder: minimum panel boyutlarını ayarlar.
    pub fn with_min_sizes(mut self, first: i32, second: i32) -> Self {
        self.min_first = first;
        self.min_second = second;
        self
    }

    /// Mevcut bölme konumunu döndürür.
    pub fn split_pos(&self) -> i32 {
        self.split_pos
    }

    /// Birinci panelin dikdörtgenini hesaplar.
    ///
    /// Bölme çizgisi genişliğinin yarısı (`splitter_size / 2`) çıkarılır;
    /// bu sayede bölücü çizgisi iki panel arasında ortalanır.
    fn first_rect(&self) -> Rect {
        match self.orientation {
            SplitOrientation::Horizontal => Rect::new(
                self.rect.x,
                self.rect.y,
                self.split_pos - self.splitter_size / 2,
                self.rect.height,
            ),
            SplitOrientation::Vertical => Rect::new(
                self.rect.x,
                self.rect.y,
                self.rect.width,
                self.split_pos - self.splitter_size / 2,
            ),
        }
    }

    /// İkinci panelin dikdörtgenini hesaplar.
    ///
    /// Bölme çizgisinin diğer yarısı ve bir piksel ek boşluk eklenerek
    /// ikinci panel başlangıç konumu belirlenir.
    fn second_rect(&self) -> Rect {
        match self.orientation {
            SplitOrientation::Horizontal => Rect::new(
                self.rect.x + self.split_pos + self.splitter_size / 2 + 1,
                self.rect.y,
                self.rect.width - self.split_pos - self.splitter_size / 2 - 1,
                self.rect.height,
            ),
            SplitOrientation::Vertical => Rect::new(
                self.rect.x,
                self.rect.y + self.split_pos + self.splitter_size / 2 + 1,
                self.rect.width,
                self.rect.height - self.split_pos - self.splitter_size / 2 - 1,
            ),
        }
    }

    /// Bölümleme çizgisinin dikdörtgenini hesaplar.
    ///
    /// Sürükleme hit-testing için kullanılır; kullanıcı bölümleme çizgisine
    /// tıklayıp sürüklediğinde `dragging = true` olur.
    fn splitter_rect(&self) -> Rect {
        match self.orientation {
            SplitOrientation::Horizontal => Rect::new(
                self.rect.x + self.split_pos - self.splitter_size / 2,
                self.rect.y,
                self.splitter_size,
                self.rect.height,
            ),
            SplitOrientation::Vertical => Rect::new(
                self.rect.x,
                self.rect.y + self.split_pos - self.splitter_size / 2,
                self.rect.width,
                self.splitter_size,
            ),
        }
    }

    /// Bölme konumunu minimum/maksimum sınırlar içinde tutar.
    ///
    /// `pos.max(self.min_first).min(max_pos)` zincirleme: önce alt sınır
    /// uygulanır, sonra üst sınır. Bu yaygın "clamp" (sıkıştırma) operasyonudur.
    fn clamp_split_pos(&mut self, pos: i32) {
        let max_pos = match self.orientation {
            SplitOrientation::Horizontal => self.rect.width - self.min_second,
            SplitOrientation::Vertical => self.rect.height - self.min_second,
        };
        self.split_pos = pos.max(self.min_first).min(max_pos);
    }
}

impl<'a> Widget for Splitter<'a> {
    fn draw(&self, fb: &mut Framebuffer) {
        // Birinci paneli çiz: `if let Some(first)` ile güvenli Option açımı
        if let Some(first) = &self.first {
            first.draw(fb);
        }

        // Bölümleme çizgisini çiz: sürüklenirken vurgu rengi, normalde kenarlık rengi
        let objects = self.render_objects();
        draw_render_objects(fb, self.rect, &objects);

        // İkinci paneli çiz
        if let Some(second) = &self.second {
            second.draw(fb);
        }
    }

    /// Tıklama: bölümleme çizgisine tıklanırsa sürükleme başlar; panellere iletilir.
    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if !self.rect.contains(x, y) {
            self.dragging = false;
            return false;
        }

        // Bölümleme çizgisine tıklandı: sürükleme moduna geç
        if self.splitter_rect().contains(x, y) {
            self.dragging = true;
            return true;
        }

        self.dragging = false;

        // Panellere tıklamayı ilet: önce hangi panelde olduğunu belirle
        let first_rect = self.first_rect();
        let second_rect = self.second_rect();

        if let Some(first) = &mut self.first {
            if first_rect.contains(x, y) {
                return first.on_click(x, y);
            }
        }
        if let Some(second) = &mut self.second {
            if second_rect.contains(x, y) {
                return second.on_click(x, y);
            }
        }
        false
    }

    /// Sürükleme: bölme konumunu delta değeriyle günceller.
    ///
    /// `dx`/`dy` bir önceki konumdan fark değerleridir. Yatay bölmede
    /// yalnızca `dx` (yatay hareket), dikey bölmede yalnızca `dy` kullanılır.
    fn on_drag(&mut self, dx: i32, dy: i32) -> bool {
        if !self.dragging {
            return false;
        }

        let delta = match self.orientation {
            SplitOrientation::Horizontal => dx,
            SplitOrientation::Vertical => dy,
        };

        self.clamp_split_pos(self.split_pos + delta);
        true
    }

    /// Hover olayını uygun alt panele iletir.
    fn on_hover(&mut self, x: i32, y: i32) -> bool {
        // Her iki panel için hover durumunu güncelle
        let mut changed = false;

        let first_rect = self.first_rect();
        let second_rect = self.second_rect();

        if let Some(first) = &mut self.first {
            if first_rect.contains(x, y) {
                changed = first.on_hover(x, y) || changed;
            }
        }
        if let Some(second) = &mut self.second {
            if second_rect.contains(x, y) {
                changed = second.on_hover(x, y) || changed;
            }
        }
        changed
    }

    fn bounds(&self) -> Rect {
        self.rect
    }

    /// Her iki panelin animasyon durumlarını da günceller.
    fn update(&mut self) {
        if let Some(first) = &mut self.first {
            first.update();
        }
        if let Some(second) = &mut self.second {
            second.update();
        }
    }

    fn render_objects(&self) -> Vec<RenderObject> {
        let splitter = self.splitter_rect();
        let splitter_color = if self.dragging {
            Theme::ACCENT_PRIMARY.to_u32()
        } else {
            Theme::BORDER.to_u32()
        };
        vec![solid_rect_object(
            ((splitter.x as u64) << 32) ^ splitter.y as u64,
            splitter,
            splitter_color,
            DamageLane::Window,
            0,
        )]
    }
}
