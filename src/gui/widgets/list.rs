//! # echOS ListView ve TreeView Widget'ları
//!
//! Liste ve ağaç yapısında seçim widget'ları.
//!
//! ## ListView
//!
//! Sabit yükseklikte satırlar halinde öğe listesi gösterir. Sanal kaydırma
//! (virtual scrolling) ile yalnızca görünür satırlar çizilir; performans için
//! tüm öğeler değil, yalnızca `scroll_offset..scroll_offset+visible` aralığı işlenir.
//!
//! ## TreeView
//!
//! Ağaç yapısındaki hiyerarşik veriyi girintili satırlarla gösterir. `TreeNode`
//! özyinelemeli (recursive) bir veri yapısıdır: her düğüm alt düğümler listesi tutar.
//! `flattened` vektörü ağacın görünür düğümlerini düz listeye "açar"; bu sayede
//! `ListView` ile aynı satır bazlı çizim mantığı kullanılabilir.
//!
//! ## Sanal Kaydırma (Virtual Scrolling)
//!
//! `scroll_offset` görünümün başlangıç indeksini tutar. `visible_items()` kaç
//! satırın ekranda sığdığını hesaplar. Yalnızca bu aralıktaki öğeler çizilir;
//! bu yaklaşım binlerce öğeyi verimli göstermeye olanak tanır.

use super::{
    border_rect_objects, draw_render_objects, solid_rect_object, text_render_object_with_width,
    AccessRole, AccessState, AccessibilityInfo, Rect, Widget,
};
use crate::gop::framebuffer::Framebuffer;
use crate::gui::protocol::{DamageLane, RenderObject};
use crate::gui::theme::Theme;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

/// Listede görüntülenen tek bir satır öğesi.
///
/// `#[derive(Clone)]` sayesinde öğeler kopyalanabilir; `TreeNode.flatten()`
/// gibi yerlerde vektöre push edilirken kopyalama kullanılır.
/// `icon: Option<u8>` ikon indeksini isteğe bağlı tutar; `None` ise ikon yok.
#[derive(Clone)]
pub struct ListItem {
    pub text: String,
    pub id: usize,
    pub selected: bool,
    pub icon: Option<u8>, // Icon index (optional)
}

impl ListItem {
    /// Yeni liste öğesi oluşturur; seçilmemiş, ikonsuz başlar.
    pub fn new(id: usize, text: &str) -> Self {
        Self {
            text: String::from(text),
            id,
            selected: false,
            icon: None,
        }
    }

    /// Builder: ikon indeksi ekler.
    pub fn with_icon(mut self, icon: u8) -> Self {
        self.icon = Some(icon);
        self
    }
}

/// Liste görünümü widget'ı; tek veya çok sütunlu öğe listesi.
///
/// `scroll_offset: usize` görünümün kaçıncı öğeyle başladığını tutar.
/// `item_height: usize` her satırın piksel yüksekliği; değiştirilemez (sabit).
/// `multi_select: bool` çoklu seçimi etkinleştirip etkinleştirmediği.
/// `hovered_index: Option<usize>` fare imlecinin üzerinde olduğu öğeyi tutar.
/// `on_select` ve `on_double_click` öğe seçim callback'leridir.
pub struct ListView {
    rect: Rect,
    items: Vec<ListItem>,
    selected_index: Option<usize>,
    scroll_offset: usize,
    item_height: usize,
    multi_select: bool,
    hovered_index: Option<usize>,
    on_select: Option<fn(usize)>,
    on_double_click: Option<fn(usize)>,
}

impl ListView {
    /// Yeni liste görünümü oluşturur; boş, tek seçimli.
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            rect: Rect::new(x, y, width, height),
            items: Vec::new(),
            selected_index: None,
            scroll_offset: 0,
            item_height: 24,
            multi_select: false,
            hovered_index: None,
            on_select: None,
            on_double_click: None,
        }
    }

    /// Builder: çoklu seçimi etkinleştirir/devre dışı bırakır.
    pub fn with_multi_select(mut self, enabled: bool) -> Self {
        self.multi_select = enabled;
        self
    }

    /// Listeye yeni öğe ekler.
    pub fn add_item(&mut self, item: ListItem) {
        self.items.push(item);
    }

    /// Tüm öğeleri, seçimi ve kaydırmayı sıfırlar.
    pub fn clear(&mut self) {
        self.items.clear();
        self.selected_index = None;
        self.scroll_offset = 0;
    }

    /// Tüm öğelere salt okunur referans döndürür.
    pub fn items(&self) -> &Vec<ListItem> {
        &self.items
    }

    /// Seçili öğenin indeksini döndürür; seçim yoksa None.
    pub fn selected_index(&self) -> Option<usize> {
        self.selected_index
    }

    /// Seçili öğeye referans döndürür; seçim yoksa None.
    ///
    /// `and_then`: `selected_index` `Some(i)` ise `items.get(i)` çağrılır,
    /// `None` ise zincirleme de None döner. Bu, iç içe Option kontrolü yerine
    /// daha temiz Option zincirleme deyimidir.
    pub fn selected_item(&self) -> Option<&ListItem> {
        self.selected_index.and_then(|i| self.items.get(i))
    }

    /// Ekranda kaç öğenin göründüğünü hesaplar.
    ///
    /// `(height - 4)`: 2 piksel üst ve alt iç boşluk için çıkartılır.
    /// Tamsayı bölmesi `(h - 4) / item_height` satır sayısını verir.
    fn visible_items(&self) -> usize {
        (self.rect.height as usize - 4) / self.item_height
    }

    /// Verilen y koordinatındaki öğenin indeksini döndürür.
    ///
    /// `relative_y`: y koordinatından liste başlangıcı ve iç boşluk çıkarılır.
    /// `relative_y / item_height`: hangi satırda olduğunu hesaplar.
    /// `scroll_offset` eklenerek gerçek liste indeksine çevrilir.
    fn item_at(&self, y: i32) -> Option<usize> {
        let relative_y = y - self.rect.y - 2;
        if relative_y < 0 {
            return None;
        }
        let index = self.scroll_offset + (relative_y as usize / self.item_height);
        if index < self.items.len() {
            Some(index)
        } else {
            None
        }
    }

    /// Belirtilen indeksteki öğeyi seçer; kaydırma görünümünü günceller.
    ///
    /// Tek seçim modunda önce tüm seçimler temizlenir ("deselect all").
    /// "Scroll to visible" mantığı: seçili öğe görünür alanın dışındaysa,
    /// görünümü öğeyi gösterecek şekilde kaydırır. Bu, programatik seçimde
    /// kullanıcının öğeyi görmesini sağlar.
    fn select(&mut self, index: usize) {
        // Çok seçimli değilse önceki seçimi temizle
        if !self.multi_select {
            for item in &mut self.items {
                item.selected = false;
            }
        }

        if index < self.items.len() {
            self.items[index].selected = true;
            self.selected_index = Some(index);

            // Seçili öğeyi görünür alana kaydır
            let visible = self.visible_items();
            if index < self.scroll_offset {
                self.scroll_offset = index;
            } else if index >= self.scroll_offset + visible {
                self.scroll_offset = index - visible + 1;
            }

            if let Some(handler) = self.on_select {
                handler(index);
            }
        }
    }

    fn render_primitives(&self) -> Vec<RenderObject> {
        let mut objects = Vec::new();
        let base_id = ((self.rect.x as u64) << 32) ^ (self.rect.y as u64);
        let visible = self.visible_items();
        let item_y_start = self.rect.y + 2;

        objects.push(solid_rect_object(
            base_id,
            self.rect,
            Theme::WINDOW_BG.to_u32(),
            DamageLane::Window,
            0,
        ));
        objects.extend(border_rect_objects(
            base_id ^ 0x10,
            self.rect,
            Theme::BORDER.to_u32(),
            DamageLane::Window,
            1,
        ));

        for i in 0..visible {
            let item_index = self.scroll_offset + i;
            if item_index >= self.items.len() {
                break;
            }

            let item = &self.items[item_index];
            let item_y = item_y_start + (i * self.item_height) as i32;
            let row_rect = Rect::new(
                self.rect.x + 1,
                item_y,
                self.rect.width - 2,
                self.item_height as i32,
            );
            if item.selected {
                objects.push(solid_rect_object(
                    base_id ^ 0x1000 ^ item_index as u64,
                    row_rect,
                    Theme::ACCENT_PRIMARY.to_u32(),
                    DamageLane::Window,
                    2,
                ));
            } else if self.hovered_index == Some(item_index) {
                objects.push(solid_rect_object(
                    base_id ^ 0x2000 ^ item_index as u64,
                    row_rect,
                    Theme::BUTTON_HOVER.to_u32(),
                    DamageLane::Window,
                    2,
                ));
            }

            let mut text_x = self.rect.x + 4;
            if item.icon.is_some() {
                objects.push(solid_rect_object(
                    base_id ^ 0x3000 ^ item_index as u64,
                    Rect::new(text_x, item_y + 4, 16, 16),
                    Theme::TEXT_SECONDARY.to_u32(),
                    DamageLane::Window,
                    3,
                ));
                text_x += 20;
            }

            let text_color = if item.selected {
                Theme::DESKTOP_BG.to_u32()
            } else {
                Theme::TEXT_PRIMARY.to_u32()
            };
            objects.push(text_render_object_with_width(
                base_id ^ 0x4000 ^ item_index as u64,
                Rect::new(
                    text_x,
                    item_y + ((self.item_height as i32 - 16) / 2),
                    (self.rect.width - (text_x - self.rect.x) - 12).max(1),
                    18,
                ),
                &item.text,
                text_color,
                false,
                DamageLane::Text,
                4,
            ));
        }

        if self.items.len() > visible {
            let h = self.rect.height.max(1) as usize;
            let scroll_bar_height = (h * visible / self.items.len()).max(20) as i32;
            let scroll_bar_y = self.rect.y + (h * self.scroll_offset / self.items.len()) as i32;
            objects.push(solid_rect_object(
                base_id ^ 0x5000,
                Rect::new(
                    self.rect.x + self.rect.width - 8,
                    scroll_bar_y,
                    6,
                    scroll_bar_height,
                ),
                Theme::BUTTON_BG.to_u32(),
                DamageLane::Window,
                5,
            ));
        }

        objects
    }
}

impl Widget for ListView {
    fn draw(&self, fb: &mut Framebuffer) {
        let objects = self.render_primitives();
        draw_render_objects(fb, self.rect, &objects);
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if self.rect.contains(x, y) {
            if let Some(index) = self.item_at(y) {
                self.select(index);
            }
            true
        } else {
            false
        }
    }

    /// Hover durumunu günceller; hangi öğenin üzerinde olunduğunu takip eder.
    fn on_hover(&mut self, x: i32, y: i32) -> bool {
        let old_hovered = self.hovered_index;
        self.hovered_index = if self.rect.contains(x, y) {
            self.item_at(y)
        } else {
            None
        };
        old_hovered != self.hovered_index
    }

    /// Kaydırma çarkı ile listeyi kaydırır.
    ///
    /// `saturating_sub(visible)`: alttan taşmayı önler; `max_scroll` negatif olamaz.
    /// `delta > 0`: yukarı kaydırma (önceki öğeye), `delta < 0`: aşağı kaydırma.
    fn on_scroll(&mut self, delta: i32) -> bool {
        let visible = self.visible_items();
        let max_scroll = self.items.len().saturating_sub(visible);

        if delta > 0 && self.scroll_offset > 0 {
            self.scroll_offset -= 1;
            return true;
        } else if delta < 0 && self.scroll_offset < max_scroll {
            self.scroll_offset += 1;
            return true;
        }
        false
    }

    fn bounds(&self) -> Rect {
        self.rect
    }

    fn can_focus(&self) -> bool {
        true
    }

    fn accessibility_info(&self) -> AccessibilityInfo<'_> {
        let mut state = AccessState::empty();
        if self.selected_index.is_some() {
            state = state.with(AccessState::SELECTED);
        }
        AccessibilityInfo {
            role: AccessRole::List,
            label: "list",
            value: "",
            state,
        }
    }

    fn render_objects(&self) -> Vec<RenderObject> {
        self.render_primitives()
    }
}

/// Ağaç düğümü; alt düğümleri olan hiyerarşik veri birimi.
///
/// `#[derive(Clone)]` sayesinde düğümler kopyalanabilir; `flatten()` metodunda
/// alt düğümler tekrar tekrar kopyalanarak düz listeye eklenebilir.
///
/// `expanded: bool` alt düğümlerin görünüp görünmediğini kontrol eder.
/// `level: usize` girintileme için kullanılır; kök düğüm level=0'dır.
#[derive(Clone)]
pub struct TreeNode {
    pub text: String,
    pub id: usize,
    pub children: Vec<TreeNode>,
    pub expanded: bool,
    pub selected: bool,
    pub level: usize,
}

impl TreeNode {
    /// Yeni ağaç düğümü oluşturur; daraltılmış, seçilmemiş, kök seviyede.
    pub fn new(id: usize, text: &str) -> Self {
        Self {
            text: String::from(text),
            id,
            children: Vec::new(),
            expanded: false,
            selected: false,
            level: 0,
        }
    }

    /// Builder: alt düğüm ekler; child'ın seviyesi otomatik ayarlanır.
    ///
    /// `child.level = self.level + 1`: her alt düğüm bir seviye daha derin.
    /// Builder pattern: `node.add_child(child1).add_child(child2)` zincirleme
    /// ile ağaç yapısı oluşturulabilir.
    pub fn add_child(mut self, child: TreeNode) -> Self {
        let mut child = child;
        child.level = self.level + 1;
        self.children.push(child);
        self
    }

    /// Düğümü ve genişletilmiş alt düğümlerini sonuç vektörüne düz olarak ekler.
    ///
    /// Özyinelemeli (recursive) yöntem: self eklenip sonra expanded ise
    /// her alt düğüm için `child.flatten(&mut result)` çağrılır.
    /// Bu DFS (Depth-First Search) sıralamasını üretir; ağaç görünümü
    /// için doğal sıralama budur.
    fn flatten(&self, result: &mut Vec<(usize, String, bool, bool, usize)>) {
        result.push((
            self.id,
            self.text.clone(),
            self.expanded,
            self.selected,
            self.level,
        ));
        if self.expanded {
            for child in &self.children {
                child.flatten(result);
            }
        }
    }
}

/// Ağaç görünümü widget'ı; hiyerarşik veriyi girintili liste olarak gösterir.
///
/// `root_nodes: Vec<TreeNode>` ağaç yapısını tutar (gerçek veri).
/// `flattened` sadece görünür düğümlerin düz listesidir; çizim ve hit-testing
/// için kullanılır. Bu "view model" ayrımı ağaç verisini görüntü verisiyle
/// gevşek (loose) bağlar.
///
/// Tuple formatı `(id, text, expanded, selected, level)`:
/// `id` düğüm kimliği, `level` girintileme miktarını belirler.
pub struct TreeView {
    rect: Rect,
    root_nodes: Vec<TreeNode>,
    flattened: Vec<(usize, String, bool, bool, usize)>, // id, text, expanded, selected, level
    selected_id: Option<usize>,
    scroll_offset: usize,
    item_height: usize,
    hovered_index: Option<usize>,
    /// Odağlanma durumu; true iken klavye olayları işlenir
    focused: bool,
}

impl TreeView {
    /// Yeni ağaç görünümü oluşturur; boş, kaydırılmamış.
    pub fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            rect: Rect::new(x, y, width, height),
            root_nodes: Vec::new(),
            flattened: Vec::new(),
            selected_id: None,
            scroll_offset: 0,
            item_height: 22,
            hovered_index: None,
            focused: false,
        }
    }

    /// Kök düğüm ekler ve düz listeyi yeniden oluşturur.
    pub fn add_root(&mut self, node: TreeNode) {
        self.root_nodes.push(node);
        self.rebuild_flattened();
    }

    /// Tüm düğümleri, seçimleri ve kaydırmayı sıfırlar.
    pub fn clear(&mut self) {
        self.root_nodes.clear();
        self.flattened.clear();
        self.selected_id = None;
        self.scroll_offset = 0;
    }

    /// Düz listeyi tüm kök düğümlerden yeniden oluşturur.
    ///
    /// `TreeNode::flatten` özyinelemeli olarak çağrılır; genişletilmiş
    /// alt düğümler de dahil edilir. Daraltılan/genişletilen her değişimde
    /// bu yöntem çağrılarak düz liste güncellenir.
    fn rebuild_flattened(&mut self) {
        self.flattened.clear();
        for node in &self.root_nodes {
            node.flatten(&mut self.flattened);
        }
    }

    /// Belirtilen indeksteki görünür düğümün genişleme durumunu tersine çevirir.
    ///
    /// Düğüm ID'si bulunur, `root_nodes` ağacında özyinelemeli arana
    /// (`toggle_node_recursive_static`), durumu güncellenir, sonra düz liste
    /// yeniden oluşturulur. Bu "mutate then rebuild" kalıbı basit ama
    /// büyük ağaçlarda O(n) yeniden inşa nedeniyle pahalı olabilir.
    fn toggle_expand(&mut self, index: usize) {
        // Görünür liste indeksinden düğüm ID'sini bul ve durumu çevir
        if index < self.flattened.len() {
            let id = self.flattened[index].0;
            let expanded = self.flattened[index].2;
            // Kök düğümler listesinde özyinelemeli ara ve güncelle
            Self::toggle_node_recursive_static(&mut self.root_nodes, id, !expanded);
            self.rebuild_flattened();
        }
    }

    /// Ağaçta ID ile düğümü bulup genişleme durumunu günceller (özyinelemeli).
    ///
    /// `-> bool` dönüş değeri: düğüm bulunduğunda `true` döner; üst çağrılar
    /// bunu arama erken sonlandırma (short-circuit) işareti olarak kullanır.
    /// Bu "early return recursion" desenidir, gereksiz alt ağaç aramasını engeller.
    fn toggle_node_recursive_static(
        nodes: &mut Vec<TreeNode>,
        id: usize,
        new_expanded: bool,
    ) -> bool {
        for node in nodes {
            if node.id == id {
                node.expanded = new_expanded;
                return true;
            }
            if Self::toggle_node_recursive_static(&mut node.children, id, new_expanded) {
                return true;
            }
        }
        false
    }

    /// Ekranda kaç düğümün göründüğünü hesaplar.
    fn visible_items(&self) -> usize {
        (self.rect.height as usize - 4) / self.item_height
    }

    /// Verilen y koordinatındaki görünür düğümün indeksini döndürür.
    fn item_at(&self, y: i32) -> Option<usize> {
        let relative_y = y - self.rect.y - 2;
        if relative_y < 0 {
            return None;
        }
        let index = self.scroll_offset + (relative_y as usize / self.item_height);
        if index < self.flattened.len() {
            Some(index)
        } else {
            None
        }
    }

    fn render_primitives(&self) -> Vec<RenderObject> {
        let mut objects = Vec::new();
        let base_id = ((self.rect.x as u64) << 32) ^ (self.rect.y as u64) ^ 0x7000_0000;
        let visible = self.visible_items();
        let item_y_start = self.rect.y + 2;

        objects.push(solid_rect_object(
            base_id,
            self.rect,
            Theme::WINDOW_BG.to_u32(),
            DamageLane::Window,
            0,
        ));
        objects.extend(border_rect_objects(
            base_id ^ 0x10,
            self.rect,
            Theme::BORDER.to_u32(),
            DamageLane::Window,
            1,
        ));

        for i in 0..visible {
            let item_index = self.scroll_offset + i;
            if item_index >= self.flattened.len() {
                break;
            }

            let (_, text, expanded, selected, level) = &self.flattened[item_index];
            let item_y = item_y_start + (i * self.item_height) as i32;
            let row_rect = Rect::new(
                self.rect.x + 1,
                item_y,
                self.rect.width - 2,
                self.item_height as i32,
            );
            if *selected {
                objects.push(solid_rect_object(
                    base_id ^ 0x1000 ^ item_index as u64,
                    row_rect,
                    Theme::ACCENT_PRIMARY.to_u32(),
                    DamageLane::Window,
                    2,
                ));
            } else if self.hovered_index == Some(item_index) {
                objects.push(solid_rect_object(
                    base_id ^ 0x2000 ^ item_index as u64,
                    row_rect,
                    Theme::BUTTON_HOVER.to_u32(),
                    DamageLane::Window,
                    2,
                ));
            }

            let indent = (*level as i32) * 16;
            let text_x = self.rect.x + 4 + indent;
            let has_children = if item_index + 1 < self.flattened.len() {
                self.flattened[item_index + 1].4 > *level
            } else {
                false
            };
            if has_children {
                let indicator = if *expanded { "-" } else { "+" };
                objects.push(text_render_object_with_width(
                    base_id ^ 0x3000 ^ item_index as u64,
                    Rect::new(text_x, item_y + 3, 10, 18),
                    indicator,
                    Theme::TEXT_SECONDARY.to_u32(),
                    false,
                    DamageLane::Text,
                    3,
                ));
            }

            let text_color = if *selected {
                Theme::DESKTOP_BG.to_u32()
            } else {
                Theme::TEXT_PRIMARY.to_u32()
            };
            objects.push(text_render_object_with_width(
                base_id ^ 0x4000 ^ item_index as u64,
                Rect::new(
                    text_x + 12,
                    item_y + ((self.item_height as i32 - 16) / 2),
                    (self.rect.width - indent - 24).max(1),
                    18,
                ),
                text,
                text_color,
                false,
                DamageLane::Text,
                4,
            ));
        }

        objects
    }
}

impl Widget for TreeView {
    fn draw(&self, fb: &mut Framebuffer) {
        let objects = self.render_primitives();
        draw_render_objects(fb, self.rect, &objects);
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        if self.rect.contains(x, y) {
            if let Some(index) = self.item_at(y) {
                // Tıklanan alanın genişlet/daralt göstergesinde mi olduğunu kontrol et.
                // Girintiye göre gösterge x konumu hesaplanır.
                let (_, _, _, _, level) = self.flattened[index];
                let indent = level * 16;
                let indicator_x = self.rect.x + 4 + indent as i32;

                if x >= indicator_x && x < indicator_x + 12 {
                    // Genişlet/daralt göstergesine tıklandı
                    self.toggle_expand(index);
                } else {
                    // Düğüm metnine tıklandı: seçimi güncelle
                    for item in &mut self.flattened {
                        item.3 = false;
                    }
                    self.flattened[index].3 = true;
                    self.selected_id = Some(self.flattened[index].0);
                }
            }
            true
        } else {
            false
        }
    }

    /// Hover durumunu günceller.
    fn on_hover(&mut self, x: i32, y: i32) -> bool {
        let old_hovered = self.hovered_index;
        self.hovered_index = if self.rect.contains(x, y) {
            self.item_at(y)
        } else {
            None
        };
        old_hovered != self.hovered_index
    }

    /// Kaydırma çarkı ile ağaç görünümünü kaydırır.
    ///
    /// `flattened.len()` görünür tüm düğüm sayısı; `max_scroll` son sayfa başlangıcı.
    fn on_scroll(&mut self, delta: i32) -> bool {
        let visible = self.visible_items();
        let max_scroll = self.flattened.len().saturating_sub(visible);

        if delta > 0 && self.scroll_offset > 0 {
            self.scroll_offset -= 1;
            return true;
        } else if delta < 0 && self.scroll_offset < max_scroll {
            self.scroll_offset += 1;
            return true;
        }
        false
    }

    /// Klavye ile ağaç görünümünde gezinme.
    /// Yukarı/Aşağı: seçimi taşır. Enter/Space: genişlet/daralt.
    /// Sağ ok: genişlet. Sol ok: daralt.
    fn on_key(&mut self, _key: char, _modifiers: u8, scancode: u8) -> bool {
        if !self.focused || self.flattened.is_empty() {
            return false;
        }

        // Mevcut seçili öğenin indeksini bul
        let current_idx = self
            .selected_id
            .and_then(|id| self.flattened.iter().position(|f| f.0 == id))
            .unwrap_or(0);

        match scancode {
            0x48 => {
                // Up arrow
                if current_idx > 0 {
                    let new_idx = current_idx - 1;
                    for item in &mut self.flattened {
                        item.3 = false;
                    }
                    self.flattened[new_idx].3 = true;
                    self.selected_id = Some(self.flattened[new_idx].0);
                    // Kaydırmayı ayarla
                    if new_idx < self.scroll_offset {
                        self.scroll_offset = new_idx;
                    }
                }
                true
            }
            0x50 => {
                // Down arrow
                if current_idx + 1 < self.flattened.len() {
                    let new_idx = current_idx + 1;
                    for item in &mut self.flattened {
                        item.3 = false;
                    }
                    self.flattened[new_idx].3 = true;
                    self.selected_id = Some(self.flattened[new_idx].0);
                    // Kaydırmayı ayarla
                    let visible = self.visible_items();
                    if new_idx >= self.scroll_offset + visible {
                        self.scroll_offset = new_idx - visible + 1;
                    }
                }
                true
            }
            0x1C | 0x39 => {
                // Enter veya Space: genişlet/daralt
                self.toggle_expand(current_idx);
                true
            }
            0x4D => {
                // Right arrow: genişlet
                if !self.flattened[current_idx].2 {
                    self.toggle_expand(current_idx);
                }
                true
            }
            0x4B => {
                // Left arrow: daralt
                if self.flattened[current_idx].2 {
                    self.toggle_expand(current_idx);
                }
                true
            }
            _ => false,
        }
    }

    fn is_focused(&self) -> bool {
        self.focused
    }

    fn set_focus(&mut self, focused: bool) {
        self.focused = focused;
    }

    fn bounds(&self) -> Rect {
        self.rect
    }

    fn can_focus(&self) -> bool {
        true
    }

    fn accessibility_info(&self) -> AccessibilityInfo<'_> {
        let mut state = AccessState::empty();
        if self.selected_id.is_some() {
            state = state.with(AccessState::SELECTED);
        }
        AccessibilityInfo {
            role: AccessRole::List,
            label: "tree",
            value: "",
            state,
        }
    }

    fn render_objects(&self) -> Vec<RenderObject> {
        self.render_primitives()
    }
}
