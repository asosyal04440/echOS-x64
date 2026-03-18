//! # echOS Widget Sistemi
//!
//! GUI widget'ları için temel trait ve ortak türler.
//! Button, Label, Matrix gibi widget'lar için altyapı.
//!
//! ## Widget Nedir?
//!
//! Widget, kullanıcı arayüzündeki temel görsel bileşenlerdir: butonlar,
//! etiketler, liste kutuları vb. Her widget ekranda bir dikdörtgen alan kaplar
//! ve kullanıcı etkileşimlerine (tıklama, klavye, kaydırma) yanıt verir.
//!
//! ## Trait Tabanlı Tasarım
//!
//! Rust'ta polimorfizm için trait'ler kullanılır. `Widget` trait'i sayesinde
//! farklı widget türleri aynı arayüz üzerinden yönetilebilir. Bu, nesne
//! yönelimli programlamadaki abstract class kavramına benzer.

use crate::gop::framebuffer::Framebuffer;
use crate::gui::input_pipeline::{FocusTree, GestureArena, GestureKind, InputRouter};
use crate::gui::protocol::{
    AccessibilityNode, AccessibilityRole, AppId, DamageLane, Point, RenderObject,
    RenderObjectKind, TextRunStyle,
};
use crate::gui::renderer::render_object_list;
use crate::gui::text::TextSystem;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// Ekran üzerindeki dikdörtgen bölge.
///
/// GUI sistemlerindeki her bileşen bir dikdörtgen alanla tanımlanır.
/// `x`, `y` sol üst köşenin koordinatları; `width` ve `height` ise
/// bileşenin piksel cinsinden boyutlarıdır.
///
/// `i32` kullanılmasının sebebi: negatif koordinatlar (ekran dışı konumlar)
/// mümkün olmalı ve kırpma/çakışma hesaplamalarında negatif ara değerler çıkabilir.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Default for Rect {
    fn default() -> Self {
        Self {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        }
    }
}

impl Rect {
    /// Yeni dikdörtgen oluşturur.
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Verilen nokta bu dikdörtgenin içinde mi?
    ///
    /// Hit-testing için kullanılır: kullanıcı tıkladığında hangi widget'ın
    /// tıklandığını bulmak için her widget'ın `bounds()` alanı bu yöntemle
    /// kontrol edilir. `x < self.x + self.width` koşulu sağ sınırı dahil etmez
    /// (yarı açık aralık [x, x+w) ), bu standart piksel sınırı konvansiyonudur.
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    /// İki dikdörtgenin kesişip kesişmediğini kontrol eder.
    ///
    /// Ekran güncelleme optimizasyonunda (dirty region tracking) kullanılır:
    /// sadece değişen bölgeleri yeniden çizmek için hangi widget'ların
    /// etkilendiğini bulmaya yarar. AABB (Axis-Aligned Bounding Box) algoritması
    /// iki dikdörtgenin birbirinin dışında olmadığını kontrol eder.
    pub fn intersects(&self, other: &Rect) -> bool {
        let self_right = self.x + self.width;
        let self_bottom = self.y + self.height;
        let other_right = other.x + other.width;
        let other_bottom = other.y + other.height;
        self.x < other_right
            && self_right > other.x
            && self.y < other_bottom
            && self_bottom > other.y
    }

    /// İki dikdörtgeni kapsayan en küçük dikdörtgeni döndürür.
    ///
    /// Birden fazla widget'ın güncellenmesi gerektiğinde her ikisini de
    /// kapsayan tek bir bölgeyi yeniden çizmek için kullanılır. Min/max
    /// hesaplaması yaparak iki dikdörtgenin birleşim (union) bounding box'ını
    /// döndürür.
    pub fn union(&self, other: &Rect) -> Rect {
        let x1 = if self.x < other.x { self.x } else { other.x };
        let y1 = if self.y < other.y { self.y } else { other.y };
        let x2 = if self.x + self.width > other.x + other.width {
            self.x + self.width
        } else {
            other.x + other.width
        };
        let y2 = if self.y + self.height > other.y + other.height {
            self.y + self.height
        } else {
            other.y + other.height
        };
        Rect::new(x1, y1, x2 - x1, y2 - y1)
    }
}

/// Klavye modifier tuşları
///
/// Bit bayrakları olarak tanımlanır; birden fazla modifier aynı anda
/// basılı tutulabilir (örn. CTRL+SHIFT = 0x01 | 0x02 = 0x03).
/// `&` operatörü ile belirli bir modifier aktif mi diye kontrol edilir:
/// `if modifiers & MOD_CTRL != 0 { ... }`.
pub const MOD_SHIFT: u8 = 0x01;
pub const MOD_CTRL: u8 = 0x02;
pub const MOD_ALT: u8 = 0x04;
pub const MOD_SUPER: u8 = 0x08;

// ────────────────────────────────────────────────────────────
// Layout Kısıtlamaları
// ────────────────────────────────────────────────────────────

/// Üst kap widget'ının çocuğa sunduğu alan kısıtlamaları.
///
/// Flex/grid layout engine'in `layout()` çağrısıyla aktarılır.
/// `max_width` / `max_height` = 0 ise sınırsız demektir.
#[derive(Debug, Clone, Copy)]
pub struct LayoutConstraints {
    pub x: i32,
    pub y: i32,
    pub max_width: i32,
    pub max_height: i32,
}

impl LayoutConstraints {
    pub fn new(x: i32, y: i32, max_width: i32, max_height: i32) -> Self {
        Self {
            x,
            y,
            max_width,
            max_height,
        }
    }

    pub fn unconstrained() -> Self {
        Self {
            x: 0,
            y: 0,
            max_width: 0,
            max_height: 0,
        }
    }
}

// ────────────────────────────────────────────────────────────
// Erişilebilirlik (Accessibility)
// ────────────────────────────────────────────────────────────

/// Widget erişilebilirlik rolü (WAI-ARIA benzeri).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessRole {
    Generic,
    Button,
    TextInput,
    Checkbox,
    RadioButton,
    Slider,
    ProgressBar,
    List,
    ListItem,
    Menu,
    MenuItem,
    Dialog,
    Label,
    Container,
    ScrollBar,
    Tab,
    TabPanel,
}

/// Erişilebilirlik durumu bayrakları.
#[derive(Debug, Clone, Copy)]
pub struct AccessState(u8);

impl AccessState {
    pub const FOCUSED: u8 = 0x01;
    pub const DISABLED: u8 = 0x02;
    pub const CHECKED: u8 = 0x04;
    pub const EXPANDED: u8 = 0x08;
    pub const SELECTED: u8 = 0x10;

    pub const fn empty() -> Self {
        Self(0)
    }
    pub const fn has(&self, flag: u8) -> bool {
        self.0 & flag != 0
    }
    pub const fn with(self, flag: u8) -> Self {
        Self(self.0 | flag)
    }
}

/// Erişilebilirlik bilgisi — ekran okuyucular ve yardımcı teknolojiler için.
#[derive(Debug, Clone, Copy)]
pub struct AccessibilityInfo<'a> {
    pub role: AccessRole,
    pub label: &'a str,
    pub value: &'a str,
    pub state: AccessState,
}

pub type WidgetId = u64;
pub type ElementId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusPolicy {
    None,
    Click,
    Tab,
    Strong,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventPhase {
    Capture,
    Target,
    Bubble,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetEvent {
    PointerMove(Point),
    PointerDown(Point),
    PointerUp(Point),
    Scroll(i32),
    Key {
        key: char,
        modifiers: u8,
        scancode: u8,
    },
    Focus(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EventResult {
    pub handled: bool,
    pub request_focus: bool,
    pub capture_pointer: bool,
    pub release_pointer: bool,
    pub needs_redraw: bool,
}

#[derive(Debug, Clone)]
pub struct WidgetSpec {
    pub widget_id: WidgetId,
    pub bounds: Rect,
    pub focus_policy: FocusPolicy,
    pub children: Vec<WidgetId>,
}

#[derive(Debug, Clone)]
pub struct SemanticsNode {
    pub node_id: u64,
    pub bounds: Rect,
    pub role: AccessRole,
    pub label: String,
    pub value: String,
    pub state: AccessState,
}

#[derive(Debug, Clone)]
pub struct RenderBox {
    pub element_id: ElementId,
    pub widget_id: WidgetId,
    pub bounds: Rect,
    pub z_index: u32,
    pub focus_policy: FocusPolicy,
    pub render_objects: Vec<RenderObject>,
    pub semantics: Vec<SemanticsNode>,
    pub children: Vec<ElementId>,
}

#[derive(Debug, Clone)]
pub struct ElementNode {
    pub element_id: ElementId,
    pub parent: Option<ElementId>,
    pub widget: WidgetSpec,
    pub render_box: RenderBox,
}

pub fn draw_render_objects(fb: &mut Framebuffer, bounds: Rect, objects: &[RenderObject]) {
    let mut text_system = TextSystem::new();
    render_object_list(
        fb,
        crate::gui::protocol::Rect::new(
            bounds.x,
            bounds.y,
            bounds.width.max(0) as u32,
            bounds.height.max(0) as u32,
        ),
        0,
        0,
        objects,
        &mut text_system,
    );
}

pub fn protocol_rect(rect: Rect) -> crate::gui::protocol::Rect {
    crate::gui::protocol::Rect::new(
        rect.x,
        rect.y,
        rect.width.max(0) as u32,
        rect.height.max(0) as u32,
    )
}

pub fn solid_rect_object(
    object_id: u64,
    rect: Rect,
    color: u32,
    lane: DamageLane,
    z_index: u32,
) -> RenderObject {
    RenderObject {
        object_id,
        bounds: protocol_rect(rect),
        clip: None,
        z_index,
        opacity: u8::MAX,
        lane,
        kind: RenderObjectKind::SolidRect {
            color,
            corner_radius: 0,
        },
    }
}

pub fn border_rect_objects(
    base_object_id: u64,
    rect: Rect,
    color: u32,
    lane: DamageLane,
    z_index: u32,
) -> Vec<RenderObject> {
    if rect.width <= 0 || rect.height <= 0 {
        return Vec::new();
    }

    let top = Rect::new(rect.x, rect.y, rect.width, 1);
    let bottom = Rect::new(rect.x, rect.y + rect.height - 1, rect.width, 1);
    let left_height = rect.height.saturating_sub(2);
    let left = Rect::new(rect.x, rect.y + 1, 1, left_height);
    let right = Rect::new(rect.x + rect.width - 1, rect.y + 1, 1, left_height);

    let mut objects = vec![
        solid_rect_object(base_object_id, top, color, lane, z_index),
        solid_rect_object(base_object_id ^ 1, bottom, color, lane, z_index),
    ];
    if left_height > 0 {
        objects.push(solid_rect_object(
            base_object_id ^ 2,
            left,
            color,
            lane,
            z_index,
        ));
        objects.push(solid_rect_object(
            base_object_id ^ 3,
            right,
            color,
            lane,
            z_index,
        ));
    }
    objects
}

pub fn raster_object(
    object_id: u64,
    rect: Rect,
    pixels: Vec<u32>,
    lane: DamageLane,
    z_index: u32,
) -> RenderObject {
    RenderObject {
        object_id,
        bounds: protocol_rect(rect),
        clip: None,
        z_index,
        opacity: u8::MAX,
        lane,
        kind: RenderObjectKind::Raster {
            width: rect.width.max(1) as u32,
            height: rect.height.max(1) as u32,
            pixels,
        },
    }
}

pub fn text_render_object(x: usize, y: usize, text: &str, color: u32, mono: bool) -> RenderObject {
    text_render_object_with_width(
        ((x as u64) << 32) ^ ((y as u64) << 8) ^ text.len() as u64,
        Rect::new(
            x as i32,
            y as i32,
            (text.chars().count().max(1) as i32).saturating_mul(8),
            18,
        ),
        text,
        color,
        mono,
        DamageLane::Text,
        0,
    )
}

pub fn text_render_object_with_width(
    object_id: u64,
    rect: Rect,
    text: &str,
    color: u32,
    mono: bool,
    lane: DamageLane,
    z_index: u32,
) -> RenderObject {
    RenderObject {
        object_id,
        bounds: crate::gui::protocol::Rect::new(
            rect.x,
            rect.y,
            rect.width.max(1) as u32,
            rect.height.max(1) as u32,
        ),
        clip: None,
        z_index,
        opacity: u8::MAX,
        lane,
        kind: RenderObjectKind::TextRun {
            blob_id: 0,
            text: String::from(text),
            color,
            style: if mono {
                TextRunStyle::Mono
            } else {
                TextRunStyle::Ui
            },
            max_width: rect.width.max(1) as u32,
        },
    }
}

pub fn draw_text_run(fb: &mut Framebuffer, x: usize, y: usize, text: &str, color: u32) {
    let object = text_render_object(x, y, text, color, false);
    draw_render_objects(
        fb,
        Rect::new(
            object.bounds.x,
            object.bounds.y,
            object.bounds.width as i32,
            object.bounds.height as i32,
        ),
        &[object],
    );
}

pub fn draw_char_run(fb: &mut Framebuffer, x: usize, y: usize, c: char, color: u32) {
    let mut encoded = [0u8; 4];
    let text = c.encode_utf8(&mut encoded);
    draw_text_run(fb, x, y, text, color);
}

#[derive(Default)]
pub struct CompiledWidgetTree {
    pub root: ElementId,
    pub elements: Vec<ElementNode>,
    pub focus_tree: FocusTree,
    pub gesture_arena: GestureArena,
    router: InputRouter,
}

impl CompiledWidgetTree {
    pub fn rebuild(&mut self, widgets: &[&dyn Widget]) {
        self.root = 0;
        self.elements.clear();

        for (index, widget) in widgets.iter().enumerate() {
            let widget_id = (index + 1) as WidgetId;
            let element_id = widget_id as ElementId;
            let spec = widget.widget_spec(widget_id);
            let render_box = widget.render_box(widget_id, element_id);
            self.elements.push(ElementNode {
                element_id,
                parent: None,
                widget: spec,
                render_box,
            });
        }

        let mut widget_to_element = Vec::with_capacity(self.elements.len());
        for element in self.elements.iter() {
            widget_to_element.push((element.widget.widget_id, element.element_id));
        }

        for index in 0..self.elements.len() {
            let child_widget_ids = self.elements[index].widget.children.clone();
            let mut children = Vec::new();
            let parent_element_id = self.elements[index].element_id;
            for child_widget_id in child_widget_ids {
                if let Some((_, child_element_id)) = widget_to_element
                    .iter()
                    .find(|(widget_id, _)| *widget_id == child_widget_id)
                {
                    children.push(*child_element_id);
                    if let Some(child) = self
                        .elements
                        .iter_mut()
                        .find(|candidate| candidate.element_id == *child_element_id)
                    {
                        child.parent = Some(parent_element_id);
                    }
                }
            }
            self.elements[index].render_box.children = children;
        }

        let render_boxes = self.render_boxes();
        self.focus_tree.rebuild(&render_boxes);
    }

    pub fn render_boxes(&self) -> Vec<RenderBox> {
        self.elements
            .iter()
            .map(|element| element.render_box.clone())
            .collect()
    }

    pub fn semantics(&self) -> Vec<SemanticsNode> {
        let mut nodes = Vec::new();
        for element in self.elements.iter() {
            nodes.extend(element.render_box.semantics.clone());
        }
        nodes
    }

    pub fn dispatch_pointer(
        &mut self,
        widgets: &mut [&mut dyn Widget],
        point: Point,
        event: WidgetEvent,
    ) -> EventResult {
        let render_boxes = self.render_boxes();
        let route = self.router.route_pointer(&render_boxes, point);
        let result = self.router.dispatch(&render_boxes, point, event, |element_id, phase, evt| {
            let index = element_id.saturating_sub(1) as usize;
            widgets
                .get_mut(index)
                .map(|widget| (*widget).event(phase, evt))
                .unwrap_or_default()
        });

        if result.request_focus {
            let previous = self.focus_tree.focused();
            if previous != route.target {
                if let Some(old) = previous {
                    let old_index = old.saturating_sub(1) as usize;
                    if let Some(widget) = widgets.get_mut(old_index) {
                        let _ = (*widget).event(EventPhase::Target, WidgetEvent::Focus(false));
                    }
                }
                if let Some(new_focus) = route.target {
                    let new_index = new_focus.saturating_sub(1) as usize;
                    if let Some(widget) = widgets.get_mut(new_index) {
                        let _ = (*widget).event(EventPhase::Target, WidgetEvent::Focus(true));
                    }
                }
                self.focus_tree.set_focused(route.target);
            }
        }

        match event {
            WidgetEvent::PointerDown(_) => {
                if result.capture_pointer {
                    if let Some(target) = route.target {
                        let _ = self.gesture_arena.claim(target, GestureKind::Drag);
                    }
                }
            }
            WidgetEvent::PointerUp(_) => {
                self.gesture_arena.clear();
            }
            WidgetEvent::Scroll(_) => {
                if let Some(target) = route.target {
                    let _ = self.gesture_arena.claim(target, GestureKind::Scroll);
                }
            }
            _ => {}
        }

        result
    }

    pub fn dispatch_key_to_focus(
        &mut self,
        widgets: &mut [&mut dyn Widget],
        event: WidgetEvent,
    ) -> EventResult {
        let Some(focused) = self.focus_tree.focused() else {
            return EventResult::default();
        };
        let index = focused.saturating_sub(1) as usize;
        widgets
            .get_mut(index)
            .map(|widget| (*widget).event(EventPhase::Target, event))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    struct HarnessWidget {
        bounds: Rect,
        label: &'static str,
        children: Vec<WidgetId>,
        focusable: bool,
        focused: bool,
        pressed: bool,
    }

    impl HarnessWidget {
        fn new(bounds: Rect, label: &'static str) -> Self {
            Self {
                bounds,
                label,
                children: Vec::new(),
                focusable: false,
                focused: false,
                pressed: false,
            }
        }

        fn with_children(mut self, children: &[WidgetId]) -> Self {
            self.children = children.to_vec();
            self
        }

        fn focusable(mut self) -> Self {
            self.focusable = true;
            self
        }
    }

    impl Widget for HarnessWidget {
        fn draw(&self, _fb: &mut Framebuffer) {}

        fn on_click(&mut self, x: i32, y: i32) -> bool {
            if self.bounds.contains(x, y) {
                self.pressed = true;
                true
            } else {
                false
            }
        }

        fn bounds(&self) -> Rect {
            self.bounds
        }

        fn can_focus(&self) -> bool {
            self.focusable
        }

        fn set_focus(&mut self, focused: bool) {
            self.focused = focused;
        }

        fn widget_spec(&self, widget_id: WidgetId) -> WidgetSpec {
            WidgetSpec {
                widget_id,
                bounds: self.bounds,
                focus_policy: self.focus_policy(),
                children: self.children.clone(),
            }
        }

        fn accessibility_info(&self) -> AccessibilityInfo<'_> {
            AccessibilityInfo {
                role: AccessRole::Button,
                label: self.label,
                value: "",
                state: if self.focused {
                    AccessState::empty().with(AccessState::FOCUSED)
                } else {
                    AccessState::empty()
                },
            }
        }
    }

    #[test]
    fn compiled_tree_links_children_and_semantics() {
        let parent = HarnessWidget::new(Rect::new(0, 0, 100, 40), "parent").with_children(&[2]);
        let child = HarnessWidget::new(Rect::new(8, 8, 24, 24), "child").focusable();
        let widgets: [&dyn Widget; 2] = [&parent, &child];

        let mut tree = CompiledWidgetTree::default();
        tree.rebuild(&widgets);

        assert_eq!(tree.elements.len(), 2);
        assert_eq!(tree.elements[0].render_box.children, vec![2]);
        assert_eq!(tree.elements[1].parent, Some(1));
        assert_eq!(tree.semantics().len(), 2);
    }

    #[test]
    fn compiled_tree_dispatch_updates_focus_and_gesture() {
        let mut first = HarnessWidget::new(Rect::new(0, 0, 32, 32), "first").focusable();
        let mut second = HarnessWidget::new(Rect::new(40, 0, 32, 32), "second").focusable();
        let widgets_ref: [&dyn Widget; 2] = [&first, &second];

        let mut tree = CompiledWidgetTree::default();
        tree.rebuild(&widgets_ref);

        let mut widgets_mut: [&mut dyn Widget; 2] = [&mut first, &mut second];
        let result = tree.dispatch_pointer(
            &mut widgets_mut,
            Point::new(44, 8),
            WidgetEvent::PointerDown(Point::new(44, 8)),
        );

        assert!(result.handled);
        assert_eq!(tree.focus_tree.focused(), Some(2));
        assert_eq!(tree.gesture_arena.active(), Some((2, GestureKind::Drag)));
    }
}

/// Tüm widget'ların implement etmesi gereken trait.
///
/// `Send` bound'u: widget'ların thread'ler arasında güvenle taşınabilmesini
/// sağlar. `no_std` ortamında async runtime olmasa da bu güvenlik garantisi
/// önemlidir. Trait object (`dyn Widget`) kullanımı için `Sized` olmayan
/// türlere de uygulanabilirlik sağlanır.
pub trait Widget: Send {
    /// Widget'ı framebuffer'a çizer.
    fn draw(&self, fb: &mut Framebuffer);

    /// Mouse click event'ini işler. True dönerse event yakalandı demektir.
    fn on_click(&mut self, x: i32, y: i32) -> bool;

    /// Klavye event'ini işler. True dönerse event yakalandı demektir.
    fn on_key(&mut self, _key: char, _modifiers: u8, _scancode: u8) -> bool {
        false
    }

    /// Mouse hover event'ini işler.
    fn on_hover(&mut self, _x: i32, _y: i32) -> bool {
        false
    }

    /// Mouse drag event'ini işler.
    fn on_drag(&mut self, _dx: i32, _dy: i32) -> bool {
        false
    }

    /// Mouse scroll event'ini işler.
    fn on_scroll(&mut self, _delta: i32) -> bool {
        false
    }

    /// Widget'ın sınır kutusunu döndürür.
    fn bounds(&self) -> Rect;

    /// Widget durumunu günceller (animasyonlar için).
    fn update(&mut self) {}

    /// Widget odaklı mı?
    fn is_focused(&self) -> bool {
        false
    }

    /// Widget odak durumunu ayarlar.
    fn set_focus(&mut self, _focused: bool) {}

    /// Widget odaklanabilir mi?
    fn can_focus(&self) -> bool {
        false
    }

    /// Verilen noktanın bu widget üzerine düşüp düşmediğini test eder.
    ///
    /// Varsayılan implementasyon `bounds().contains()` kullanır.
    /// Yuvarlak köşeli veya düzensiz şekilli widget'lar override edebilir.
    fn hit_test(&self, x: i32, y: i32) -> bool {
        self.bounds().contains(x, y)
    }

    /// Üst kap tarafından sunulan kısıtlamalar içinde widget boyutunu hesaplar.
    ///
    /// `constraints`: üst kap tarafından verilen mevcut alan (x, y, max_width, max_height).
    /// Varsayılan implementasyon mevcut bounds'u korur (absolute positioning).
    /// Layout engine kullanan widget'lar override eder.
    fn layout(&mut self, _constraints: LayoutConstraints) {}

    /// Erişilebilirlik bilgisi döndürür.
    ///
    /// Ekran okuyucular ve yardımcı teknolojiler için widget rolü, etiketi
    /// ve durumunu bildirir. Varsayılan: Generic rol, boş etiket.
    fn accessibility_info(&self) -> AccessibilityInfo<'_> {
        AccessibilityInfo {
            role: AccessRole::Generic,
            label: "",
            value: "",
            state: AccessState::empty(),
        }
    }

    fn screen_reader_snapshot(&self) -> Option<AccessibilityInfo<'_>> {
        let info = self.accessibility_info();
        if info.label.is_empty() && info.value.is_empty() {
            None
        } else {
            Some(info)
        }
    }

    /// Retained-mode render object üretimi.
    fn render_objects(&self) -> Vec<RenderObject> {
        Vec::new()
    }

    fn widget_spec(&self, widget_id: WidgetId) -> WidgetSpec {
        WidgetSpec {
            widget_id,
            bounds: self.bounds(),
            focus_policy: self.focus_policy(),
            children: Vec::new(),
        }
    }

    fn render_box(&self, widget_id: WidgetId, element_id: ElementId) -> RenderBox {
        let semantics = self
            .screen_reader_snapshot()
            .map(|info| {
                vec![SemanticsNode {
                    node_id: element_id,
                    bounds: self.bounds(),
                    role: info.role,
                    label: String::from(info.label),
                    value: String::from(info.value),
                    state: info.state,
                }]
            })
            .unwrap_or_default();
        RenderBox {
            element_id,
            widget_id,
            bounds: self.bounds(),
            z_index: widget_id as u32,
            focus_policy: self.focus_policy(),
            render_objects: self.render_objects(),
            semantics,
            children: Vec::new(),
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        if self.can_focus() {
            FocusPolicy::Click
        } else {
            FocusPolicy::None
        }
    }

    fn event(&mut self, phase: EventPhase, event: WidgetEvent) -> EventResult {
        if phase != EventPhase::Target {
            return EventResult::default();
        }

        match event {
            WidgetEvent::PointerMove(point) => EventResult {
                handled: self.on_hover(point.x, point.y),
                needs_redraw: true,
                ..EventResult::default()
            },
            WidgetEvent::PointerDown(point) => {
                let handled = self.on_click(point.x, point.y);
                EventResult {
                    handled,
                    request_focus: handled && self.can_focus(),
                    capture_pointer: handled,
                    needs_redraw: handled,
                    ..EventResult::default()
                }
            }
            WidgetEvent::PointerUp(_) => EventResult {
                release_pointer: true,
                ..EventResult::default()
            },
            WidgetEvent::Scroll(delta) => EventResult {
                handled: self.on_scroll(delta),
                needs_redraw: true,
                ..EventResult::default()
            },
            WidgetEvent::Key {
                key,
                modifiers,
                scancode,
            } => EventResult {
                handled: self.on_key(key, modifiers, scancode),
                needs_redraw: true,
                ..EventResult::default()
            },
            WidgetEvent::Focus(focused) => {
                self.set_focus(focused);
                EventResult {
                    handled: true,
                    needs_redraw: true,
                    ..EventResult::default()
                }
            }
        }
    }

    /// Erişilebilirlik ağacı için protokol düğümleri üretir.
    fn semantic_nodes(&self, app_id: AppId, node_id: u64) -> Vec<AccessibilityNode> {
        let Some(info) = self.screen_reader_snapshot() else {
            return Vec::new();
        };
        vec![AccessibilityNode {
            id: node_id,
            app_id,
            role: match info.role {
                AccessRole::Button => AccessibilityRole::Button,
                AccessRole::Dialog => AccessibilityRole::Dialog,
                AccessRole::Label => AccessibilityRole::Text,
                AccessRole::List => AccessibilityRole::List,
                AccessRole::ListItem => AccessibilityRole::ListItem,
                AccessRole::TextInput => AccessibilityRole::Input,
                _ => AccessibilityRole::Window,
            },
            label: String::from(info.label),
            description: String::from(info.value),
            focused: info.state.has(AccessState::FOCUSED),
            bounds: crate::gui::protocol::Rect::new(
                self.bounds().x,
                self.bounds().y,
                self.bounds().width.max(0) as u32,
                self.bounds().height.max(0) as u32,
            ),
        }]
    }
}

/// Button widget
pub mod button;
/// Checkbox and RadioButton widgets
pub mod checkbox;
/// Container widgets (Panel, TabControl, Splitter)
pub mod container;
/// Dialog widgets (Dialog, MessageBox, FileDialog)
pub mod dialog;
/// Label widget (text display)
pub mod label;
/// ListView and TreeView widgets
pub mod list;
/// Matrix animasyon widget (Matrix filmi efekti)
pub mod matrix;
/// Menu widgets (Menu, ContextMenu, MenuItem)
pub mod menu;
/// ProgressBar and Spinner widgets
pub mod progress;
/// ScrollBar and Slider widgets
pub mod scroll;
/// Text input widget (TextBox, TextArea)
pub mod text_input;
