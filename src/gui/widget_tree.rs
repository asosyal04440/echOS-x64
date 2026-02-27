//! # Tutulumlu Mod Widget Ağacı
//!
//! Kirlilik takibiyle verimli widget yönetimi.
//! Yalnızca değişen widget'ları yeniden çizer.
//!
//! ## Mimari
//! - `WidgetId`: Her widget için benzersiz 64-bit tanımlayıcı
//! - `WidgetNode`: Ağaçtaki tek widget düğümü (ebeveyn, çocuklar, önbellek, z-index)
//! - `WidgetTree`: Ana ağaç yapısı; kirli küme, yerleşim kuyruğu, render kuyruğu, odak zinciri
//! - `WidgetTreeBuilder`: Widget ağacını hiyerarşik olarak oluşturmak için inşaatçı deseni
//! - `DummyWidget`: Kök kapsayıcı için boş widget (tüm ekranı kaplar)
//!
//! ## Kirlilik Takibi Algoritması
//! 1. Widget değiştiğinde `mark_dirty(id)` çağrılır → kirli kümesine eklenir
//! 2. `render()` yalnızca kirli widget'ları z-index sırasına göre render eder
//! 3. Render sonrası içerik karması (`compute_hash`) güncellenir
//! 4. Bir son kez render kuyruğu ve kirli küme temizlenir

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use core::hash::Hash;
use spin::Mutex;

use super::widgets::{Widget, Rect};
use crate::gop::framebuffer::Framebuffer;

// ============================================================================
// WIDGET KİMLİĞİ
// ============================================================================

/// Benzersiz widget tanımlayıcısı
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WidgetId(pub u64);

impl WidgetId {
    pub const ROOT: WidgetId = WidgetId(0);

    pub fn new(id: u64) -> Self {
        WidgetId(id)
    }

    pub fn is_root(&self) -> bool {
        self.0 == 0
    }
}

// ============================================================================
// WIDGET DÜĞÜMÜ
// ============================================================================

/// Widget ağacındaki düğüm
pub struct WidgetNode {
    /// Benzersiz kimlik
    pub id: WidgetId,
    /// Üst kimlik (kök için None)
    pub parent_id: Option<WidgetId>,
    /// Çocuk kimlikler
    pub children: Vec<WidgetId>,
    /// Gerçek widget
    pub widget: Box<dyn Widget>,
    /// Önbelleğe alınmış sınırlayıcı dikdörtgen
    pub cached_rect: Rect,
    /// Değişim tespiti için içerik karması
    pub content_hash: u64,
    /// Bu düğüm görünür mü
    pub visible: bool,
    /// Bu düğüm etkin mi
    pub enabled: bool,
    /// Render sırası için z-index
    pub z_index: i32,
    /// Yerleşim gerekli mi
    pub needs_layout: bool,
    /// Render gerekli mi
    pub needs_render: bool,
    /// Çocukları sınırlara kırp
    pub clip_children: bool,
}

impl WidgetNode {
    pub fn new(id: WidgetId, widget: Box<dyn Widget>) -> Self {
        WidgetNode {
            id,
            parent_id: None,
            children: Vec::new(),
            widget,
            cached_rect: Rect::new(0, 0, 0, 0),
            content_hash: 0,
            visible: true,
            enabled: true,
            z_index: 0,
            needs_layout: true,
            needs_render: true,
            clip_children: true,
        }
    }

    /// Değişim tespiti için içerik karmasını hesapla
    pub fn compute_hash(&mut self) {
        // Sınırları/durumu temel alan basit manual karma
        let mut hash = 0u64;
        hash = hash.wrapping_mul(31).wrapping_add(self.cached_rect.x as u64);
        hash = hash.wrapping_mul(31).wrapping_add(self.cached_rect.y as u64);
        hash = hash.wrapping_mul(31).wrapping_add(self.cached_rect.width as u64);
        hash = hash.wrapping_mul(31).wrapping_add(self.cached_rect.height as u64);
        hash = hash.wrapping_mul(31).wrapping_add(self.visible as u64);
        hash = hash.wrapping_mul(31).wrapping_add(self.enabled as u64);
        hash = hash.wrapping_mul(31).wrapping_add(self.z_index as u64);
        self.content_hash = hash;
    }

    /// İçeriğin değişip değişmediğini kontrol et
    pub fn is_dirty(&self, old_hash: u64) -> bool {
        self.content_hash != old_hash || self.needs_render
    }
}

// ============================================================================
// WIDGET AĞACI
// ============================================================================

/// Ana widget ağacı yapısı
pub struct WidgetTree {
    /// Tüm widget düğümleri
    nodes: BTreeMap<WidgetId, WidgetNode>,
    /// Kök widget kimliği
    root_id: WidgetId,
    /// Sonraki widget kimliği
    next_id: u64,
    /// Kirli widget kimlik kümesi
    dirty_widgets: BTreeSet<WidgetId>,
    /// Yerleşim kuyruğu (yerleşim gerektiren widget'lar)
    layout_queue: VecDeque<WidgetId>,
    /// Render kuyruğu (render gerektiren widget'lar, z-index sıralı)
    render_queue: Vec<WidgetId>,
    /// Odak zinciri (sekme sırası)
    focus_chain: Vec<WidgetId>,
    /// Mevcut odaklanmış widget
    focused_widget: Option<WidgetId>,
    /// Üzerine gelinmiş widget
    hovered_widget: Option<WidgetId>,
    /// Fare altındaki widget (hit test önbelleği)
    hover_cache: BTreeMap<(i32, i32), WidgetId>,
    /// Kare sayacı
    frame: u64,
}

impl WidgetTree {
    /// Boş yeni widget ağacı oluştur
    pub fn new() -> Self {
        let mut tree = WidgetTree {
            nodes: BTreeMap::new(),
            root_id: WidgetId::ROOT,
            next_id: 1,
            dirty_widgets: BTreeSet::new(),
            layout_queue: VecDeque::new(),
            render_queue: Vec::new(),
            focus_chain: Vec::new(),
            focused_widget: None,
            hovered_widget: None,
            hover_cache: BTreeMap::new(),
            frame: 0,
        };

        // Kök düğüm oluştur (görünmez kapsayıcı)
        // Kök widget tüm ekranı kaplayan bir kukla widget'tır
        tree
    }

    /// Kök widget ile yeni widget ağacı oluştur
    pub fn with_root(root: Box<dyn Widget>, width: i32, height: i32) -> Self {
        let mut tree = Self::new();

        let mut node = WidgetNode::new(WidgetId::ROOT, root);
        node.cached_rect = Rect::new(0, 0, width, height);
        node.needs_layout = false;
        node.compute_hash();

        tree.nodes.insert(WidgetId::ROOT, node);
        tree
    }

    /// Yeni benzersiz widget kimliği üret
    fn generate_id(&mut self) -> WidgetId {
        let id = WidgetId::new(self.next_id);
        self.next_id += 1;
        id
    }

    /// Üst widget'a çocuk olarak widget ekle
    pub fn add_widget(&mut self, parent_id: WidgetId, widget: Box<dyn Widget>) -> WidgetId {
        let id = self.generate_id();

        let mut node = WidgetNode::new(id, widget);
        node.parent_id = Some(parent_id);
        node.z_index = self.nodes.len() as i32;

        // Üst widget'ın çocuklarına ekle
        if let Some(parent) = self.nodes.get_mut(&parent_id) {
            parent.children.push(id);
        }

        // Düğümü ekle
        self.nodes.insert(id, node);

        // Kirli olarak işaretle
        self.mark_dirty(id);
        self.mark_needs_layout(id);

        // Odaklanabilirse odak zincirine ekle
        // (trait widget.is_focusable() metodu olsaydı kontrol edilirdi)
        self.focus_chain.push(id);

        id
    }

    /// Köke widget ekle
    pub fn add_to_root(&mut self, widget: Box<dyn Widget>) -> WidgetId {
        self.add_widget(WidgetId::ROOT, widget)
    }

    /// Bir widget ve tüm çocuklarını kaldır
    pub fn remove_widget(&mut self, id: WidgetId) {
        // Üst widget'ın çocuklarından kaldır
        if let Some(node) = self.nodes.get(&id) {
            if let Some(parent_id) = node.parent_id {
                if let Some(parent) = self.nodes.get_mut(&parent_id) {
                    parent.children.retain(|&c| c != id);
                }
            }
        }

        // Tüm torun widget'ları topla
        let mut to_remove = Vec::new();
        self.collect_descendants(id, &mut to_remove);
        to_remove.push(id);

        // Hepsini kaldır
        for remove_id in to_remove {
            self.nodes.remove(&remove_id);
            self.dirty_widgets.remove(&remove_id);
            self.focus_chain.retain(|&fid| fid != remove_id);

            if self.focused_widget == Some(remove_id) {
                self.focused_widget = None;
            }
            if self.hovered_widget == Some(remove_id) {
                self.hovered_widget = None;
            }
        }

        // Üzerine gelme önbelleğini temizle (değişmiş olabilir)
        self.hover_cache.clear();
    }

    /// Bir widget'ın tüm torunlarını topla
    fn collect_descendants(&self, id: WidgetId, result: &mut Vec<WidgetId>) {
        if let Some(node) = self.nodes.get(&id) {
            for &child_id in &node.children {
                result.push(child_id);
                self.collect_descendants(child_id, result);
            }
        }
    }

    /// Widget'ı kirli olarak işaretle (yeniden render gerekli)
    pub fn mark_dirty(&mut self, id: WidgetId) {
        self.dirty_widgets.insert(id);

        // Render gerekli olarak da işaretle
        if let Some(node) = self.nodes.get_mut(&id) {
            node.needs_render = true;
        }

        // Render kuyruğuna ekle (henüz yoksa)
        if !self.render_queue.contains(&id) {
            self.render_queue.push(id);
        }
    }

    /// Widget'ı yerleşim gerekli olarak işaretle
    pub fn mark_needs_layout(&mut self, id: WidgetId) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.needs_layout = true;
        }

        // Yerleşim kuyruğuna ekle
        if !self.layout_queue.contains(&id) {
            self.layout_queue.push_back(id);
        }
    }

    /// Kirlilik durumunu ağaçta yukarı yay
    pub fn propagate_dirty(&mut self, id: WidgetId) {
        let mut current = Some(id);

        while let Some(node_id) = current {
            self.mark_dirty(node_id);

            current = self.nodes.get(&node_id).and_then(|n| n.parent_id);
        }
    }

    /// Kimliğe göre widget düğümü al
    pub fn get(&self, id: WidgetId) -> Option<&WidgetNode> {
        self.nodes.get(&id)
    }

    /// Kimliğe göre değiştirilebilir widget düğümü al
    pub fn get_mut(&mut self, id: WidgetId) -> Option<&mut WidgetNode> {
        self.nodes.get_mut(&id)
    }

    /// Kimliğe göre widget al
    pub fn get_widget(&self, id: WidgetId) -> Option<&dyn Widget> {
        self.nodes.get(&id).map(|n| n.widget.as_ref())
    }

    /// Kimliğe göre değiştirilebilir widget al
    pub fn get_widget_mut<'a>(&'a mut self, id: WidgetId) -> Option<&'a mut dyn Widget> {
        let node = self.nodes.get_mut(&id)?;
        Some(node.widget.as_mut())
    }

    /// Konumdaki widget'ı bul (hit test)
    pub fn hit_test(&self, x: i32, y: i32) -> Option<WidgetId> {
        // Önce önbelleği kontrol et
        if let Some(&id) = self.hover_cache.get(&(x, y)) {
            if let Some(node) = self.nodes.get(&id) {
                if node.visible && node.cached_rect.contains(x, y) {
                    return Some(id);
                }
            }
        }

        // Ağacı ters z-sırasıyla gez (üstten alta)
        let mut hit: Option<WidgetId> = None;
        let mut max_z = i32::MIN;

        for (&id, node) in &self.nodes {
            if !node.visible {
                continue;
            }

            if node.cached_rect.contains(x, y) {
                if node.z_index > max_z {
                    max_z = node.z_index;
                    hit = Some(id);
                }
            }
        }

        hit
    }

    /// Üzerine gelme durumunu güncelle
    pub fn update_hover(&mut self, x: i32, y: i32) -> bool {
        let new_hover = self.hit_test(x, y);

        if new_hover != self.hovered_widget {
            // Eski widget'tan çık
            if let Some(old_id) = self.hovered_widget {
                if let Some(node) = self.nodes.get_mut(&old_id) {
                    // widget.on_mouse_leave() burada çağrılacak
                    node.needs_render = true;
                }
            }

            // Yeni widget'a gir
            if let Some(new_id) = new_hover {
                if let Some(node) = self.nodes.get_mut(&new_id) {
                    // widget.on_mouse_enter() burada çağrılacak
                    node.needs_render = true;
                }
            }

            self.hovered_widget = new_hover;

            // Önbelleği güncelle
            self.hover_cache.insert((x, y), new_hover.unwrap_or(WidgetId::ROOT));

            true
        } else {
            false
        }
    }

    /// Yerleşim kuyruğunu işle
    pub fn process_layout(&mut self) {
        while let Some(id) = self.layout_queue.pop_front() {
            if let Some(node) = self.nodes.get_mut(&id) {
                // Bu widget'ı yerleştir
                // (widget.measure() ve widget.arrange() çağrılacak)
                node.needs_layout = false;

                // Yerleşim sonrası render gerekli
                node.needs_render = true;
                self.dirty_widgets.insert(id);
            }
        }
    }

    /// Tüm kirli widget'ları render et
    pub fn render(&mut self, fb: &mut Framebuffer) -> usize {
        self.frame += 1;
        let mut rendered = 0;

        // Önce yerleşimi işle
        self.process_layout();

        // Render kuyruğunu z-index'e göre sırala (arkadan öne)
        self.render_queue.sort_by_key(|&id| {
            self.nodes.get(&id).map(|n| n.z_index).unwrap_or(0)
        });

        // Her kirli widget'ı render et
        for &id in &self.render_queue {
            if let Some(node) = self.nodes.get_mut(&id) {
                if node.visible && node.needs_render {
                    // Widget'ı render et
                    node.widget.draw(fb);
                    node.needs_render = false;
                    rendered += 1;
                }
            }
        }

        // Render kuyruğunu temizle
        self.render_queue.clear();

        // Karmaları güncelle
        for &id in &self.dirty_widgets {
            if let Some(node) = self.nodes.get_mut(&id) {
                node.compute_hash();
            }
        }

        // Kirli kümeyi temizle
        self.dirty_widgets.clear();

        rendered
    }

    /// Odaklanmış widget'ı al
    pub fn focused(&self) -> Option<WidgetId> {
        self.focused_widget
    }

    /// Odağı bir widget'a ayarla
    pub fn set_focus(&mut self, id: WidgetId) {
        // Eskiden odağı kaldır
        if let Some(old_id) = self.focused_widget {
            if let Some(node) = self.nodes.get_mut(&old_id) {
                // widget.on_blur() çağrılacak
                node.needs_render = true;
            }
        }

        // Yeni odağı ayarla
        if self.nodes.contains_key(&id) {
            if let Some(node) = self.nodes.get_mut(&id) {
                // widget.on_focus() çağrılacak
                node.needs_render = true;
            }
            self.focused_widget = Some(id);
        }
    }

    /// Sekme sırasında sonraki widget'a odaklan
    pub fn focus_next(&mut self) {
        if let Some(current) = self.focused_widget {
            if let Some(idx) = self.focus_chain.iter().position(|&id| id == current) {
                let next_idx = (idx + 1) % self.focus_chain.len();
                self.set_focus(self.focus_chain[next_idx]);
                return;
            }
        }

        // İlkine odaklan
        if let Some(&first) = self.focus_chain.first() {
            self.set_focus(first);
        }
    }

    /// Sekme sırasında önceki widget'a odaklan
    pub fn focus_prev(&mut self) {
        if let Some(current) = self.focused_widget {
            if let Some(idx) = self.focus_chain.iter().position(|&id| id == current) {
                let prev_idx = if idx == 0 { self.focus_chain.len() - 1 } else { idx - 1 };
                self.set_focus(self.focus_chain[prev_idx]);
                return;
            }
        }

        // Sonuncuya odaklan
        if let Some(&last) = self.focus_chain.last() {
            self.set_focus(last);
        }
    }

    /// Bir widget'ın çocuklarını al
    pub fn children(&self, id: WidgetId) -> Option<&Vec<WidgetId>> {
        self.nodes.get(&id).map(|n| &n.children)
    }

    /// Bir widget'ın üst widget'ını al
    pub fn parent(&self, id: WidgetId) -> Option<WidgetId> {
        self.nodes.get(&id).and_then(|n| n.parent_id)
    }

    /// Widget sayısını al
    pub fn count(&self) -> usize {
        self.nodes.len()
    }

    /// Kirli widget sayısını al
    pub fn dirty_count(&self) -> usize {
        self.dirty_widgets.len()
    }

    /// Ağacın kirli widget'ları olup olmadığını kontrol et
    pub fn is_dirty(&self) -> bool {
        !self.dirty_widgets.is_empty() || !self.render_queue.is_empty()
    }

    /// Tüm kirlilik durumunu temizle
    pub fn clear_dirty(&mut self) {
        self.dirty_widgets.clear();
        self.render_queue.clear();
        self.layout_queue.clear();

        for node in self.nodes.values_mut() {
            node.needs_render = false;
            node.needs_layout = false;
        }
    }

    /// Widget görünürlüğünü ayarla
    pub fn set_visible(&mut self, id: WidgetId, visible: bool) {
        let children = if let Some(node) = self.nodes.get_mut(&id) {
            if node.visible != visible {
                node.visible = visible;
                Some(node.children.clone())
            } else {
                None
            }
        } else {
            None
        };

        if let Some(children) = children {
            self.mark_dirty(id);
            for child_id in children {
                self.set_visible(child_id, visible);
            }
        }
    }

    /// Widget etkin durumunu ayarla
    pub fn set_enabled(&mut self, id: WidgetId, enabled: bool) {
        if let Some(node) = self.nodes.get_mut(&id) {
            if node.enabled != enabled {
                node.enabled = enabled;
                self.mark_dirty(id);
            }
        }
    }

    /// Widget z-index değerini ayarla
    pub fn set_z_index(&mut self, id: WidgetId, z_index: i32) {
        if let Some(node) = self.nodes.get_mut(&id) {
            if node.z_index != z_index {
                node.z_index = z_index;
                self.mark_dirty(id);
            }
        }
    }

    /// Widget'ı öne getir
    pub fn bring_to_front(&mut self, id: WidgetId) {
        let max_z = self.nodes.values().map(|n| n.z_index).max().unwrap_or(0);
        self.set_z_index(id, max_z + 1);
    }

    /// Widget'ı arkaya gönder
    pub fn send_to_back(&mut self, id: WidgetId) {
        let min_z = self.nodes.values().map(|n| n.z_index).min().unwrap_or(0);
        self.set_z_index(id, min_z - 1);
    }

    /// Kare sayısını al
    pub fn frame(&self) -> u64 {
        self.frame
    }
}

impl Default for WidgetTree {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// WIDGET AĞACI İNŞAATÇISI
// ============================================================================

/// Widget ağacı oluşturmak için inşaatçı
pub struct WidgetTreeBuilder {
    tree: WidgetTree,
    current_parent: WidgetId,
}

impl WidgetTreeBuilder {
    pub fn new(width: i32, height: i32) -> Self {
        // Kök widget olarak kukla widget oluştur
        let root_widget = Box::new(DummyWidget::new(width, height));
        WidgetTreeBuilder {
            tree: WidgetTree::with_root(root_widget, width, height),
            current_parent: WidgetId::ROOT,
        }
    }

    /// Mevcut üste widget ekle
    pub fn add(mut self, widget: Box<dyn Widget>) -> Self {
        self.tree.add_widget(self.current_parent, widget);
        self
    }

    /// Widget ekle ve yeni üst yap
    pub fn add_container(mut self, widget: Box<dyn Widget>) -> Self {
        let id = self.tree.add_widget(self.current_parent, widget);
        self.current_parent = id;
        self
    }

    /// Üste çık
    pub fn end_container(mut self) -> Self {
        if let Some(parent) = self.tree.parent(self.current_parent) {
            self.current_parent = parent;
        }
        self
    }

    /// Ağacı oluştur
    pub fn build(self) -> WidgetTree {
        self.tree
    }
}

// ============================================================================
// KUKLA WIDGET (kök için)
// ============================================================================

/// Kök kapsayıcı için kukla widget
struct DummyWidget {
    rect: Rect,
}

impl DummyWidget {
    fn new(width: i32, height: i32) -> Self {
        DummyWidget {
            rect: Rect::new(0, 0, width, height),
        }
    }
}

impl Widget for DummyWidget {
    fn draw(&self, _fb: &mut Framebuffer) {
        // Kök hiçbir şey çizmez
    }

    fn on_click(&mut self, _x: i32, _y: i32) -> bool {
        false
    }

    fn bounds(&self) -> Rect {
        self.rect
    }
}

// ============================================================================
// GLOBAL WIDGET AĞACI
// ============================================================================

lazy_static::lazy_static! {
    static ref WIDGET_TREE: Mutex<WidgetTree> = Mutex::new(WidgetTree::new());
}

/// Global widget ağacını al
pub fn get_tree() -> &'static Mutex<WidgetTree> {
    &WIDGET_TREE
}

/// Global ağaca widget ekle
pub fn add_widget(widget: Box<dyn Widget>) -> WidgetId {
    WIDGET_TREE.lock().add_to_root(widget)
}

/// Global ağacı render et
pub fn render_tree(fb: &mut Framebuffer) -> usize {
    WIDGET_TREE.lock().render(fb)
}

/// Global ağaçta hit test yap
pub fn hit_test(x: i32, y: i32) -> Option<WidgetId> {
    WIDGET_TREE.lock().hit_test(x, y)
}

/// Widget ağacını ekran boyutuyla başlat
pub fn init(width: i32, height: i32) {
    *WIDGET_TREE.lock() = WidgetTree::with_root(
        Box::new(DummyWidget::new(width, height)),
        width, height
    );
    crate::serial_println!("[GUI] Widget ağacı başlatıldı ({}x{})", width, height);
}
