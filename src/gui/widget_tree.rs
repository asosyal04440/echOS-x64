//! # Retained Mode Widget Tree
//!
//! Efficient widget management with dirty tracking
//! Only re-renders widgets that have changed

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
// WIDGET ID
// ============================================================================

/// Unique widget identifier
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
// WIDGET NODE
// ============================================================================

/// Node in the widget tree
pub struct WidgetNode {
    /// Unique ID
    pub id: WidgetId,
    /// Parent ID (None for root)
    pub parent_id: Option<WidgetId>,
    /// Children IDs
    pub children: Vec<WidgetId>,
    /// The actual widget
    pub widget: Box<dyn Widget>,
    /// Cached bounding rectangle
    pub cached_rect: Rect,
    /// Content hash for change detection
    pub content_hash: u64,
    /// Is this node visible
    pub visible: bool,
    /// Is this node enabled
    pub enabled: bool,
    /// Z-index for rendering order
    pub z_index: i32,
    /// Needs layout
    pub needs_layout: bool,
    /// Needs render
    pub needs_render: bool,
    /// Clip children to bounds
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
    
    /// Compute content hash for change detection
    pub fn compute_hash(&mut self) {
        // Simple manual hash based on bounds/state
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
    
    /// Check if content has changed
    pub fn is_dirty(&self, old_hash: u64) -> bool {
        self.content_hash != old_hash || self.needs_render
    }
}

// ============================================================================
// WIDGET TREE
// ============================================================================

/// The main widget tree structure
pub struct WidgetTree {
    /// All widget nodes
    nodes: BTreeMap<WidgetId, WidgetNode>,
    /// Root widget ID
    root_id: WidgetId,
    /// Next widget ID
    next_id: u64,
    /// Set of dirty widget IDs
    dirty_widgets: BTreeSet<WidgetId>,
    /// Layout queue (widgets needing layout)
    layout_queue: VecDeque<WidgetId>,
    /// Render queue (widgets needing render, sorted by z-index)
    render_queue: Vec<WidgetId>,
    /// Focus chain (tab order)
    focus_chain: Vec<WidgetId>,
    /// Currently focused widget
    focused_widget: Option<WidgetId>,
    /// Hovered widget
    hovered_widget: Option<WidgetId>,
    /// Widget under mouse (for hit testing cache)
    hover_cache: BTreeMap<(i32, i32), WidgetId>,
    /// Frame counter
    frame: u64,
}

impl WidgetTree {
    /// Create a new empty widget tree
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
        
        // Create root node (invisible container)
        // Root widget is a dummy that covers the whole screen
        tree
    }
    
    /// Create a new widget tree with a root widget
    pub fn with_root(root: Box<dyn Widget>, width: i32, height: i32) -> Self {
        let mut tree = Self::new();
        
        let mut node = WidgetNode::new(WidgetId::ROOT, root);
        node.cached_rect = Rect::new(0, 0, width, height);
        node.needs_layout = false;
        node.compute_hash();
        
        tree.nodes.insert(WidgetId::ROOT, node);
        tree
    }
    
    /// Generate a new unique widget ID
    fn generate_id(&mut self) -> WidgetId {
        let id = WidgetId::new(self.next_id);
        self.next_id += 1;
        id
    }
    
    /// Add a widget as child of parent
    pub fn add_widget(&mut self, parent_id: WidgetId, widget: Box<dyn Widget>) -> WidgetId {
        let id = self.generate_id();
        
        let mut node = WidgetNode::new(id, widget);
        node.parent_id = Some(parent_id);
        node.z_index = self.nodes.len() as i32;
        
        // Add to parent's children
        if let Some(parent) = self.nodes.get_mut(&parent_id) {
            parent.children.push(id);
        }
        
        // Insert node
        self.nodes.insert(id, node);
        
        // Mark dirty
        self.mark_dirty(id);
        self.mark_needs_layout(id);
        
        // Add to focus chain if focusable
        // (would check widget.is_focusable() if trait had it)
        self.focus_chain.push(id);
        
        id
    }
    
    /// Add widget to root
    pub fn add_to_root(&mut self, widget: Box<dyn Widget>) -> WidgetId {
        self.add_widget(WidgetId::ROOT, widget)
    }
    
    /// Remove a widget and all its children
    pub fn remove_widget(&mut self, id: WidgetId) {
        // Remove from parent's children
        if let Some(node) = self.nodes.get(&id) {
            if let Some(parent_id) = node.parent_id {
                if let Some(parent) = self.nodes.get_mut(&parent_id) {
                    parent.children.retain(|&c| c != id);
                }
            }
        }
        
        // Collect all descendants
        let mut to_remove = Vec::new();
        self.collect_descendants(id, &mut to_remove);
        to_remove.push(id);
        
        // Remove all
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
        
        // Clear hover cache (might have changed)
        self.hover_cache.clear();
    }
    
    /// Collect all descendants of a widget
    fn collect_descendants(&self, id: WidgetId, result: &mut Vec<WidgetId>) {
        if let Some(node) = self.nodes.get(&id) {
            for &child_id in &node.children {
                result.push(child_id);
                self.collect_descendants(child_id, result);
            }
        }
    }
    
    /// Mark a widget as dirty (needs re-render)
    pub fn mark_dirty(&mut self, id: WidgetId) {
        self.dirty_widgets.insert(id);
        
        // Also mark for render
        if let Some(node) = self.nodes.get_mut(&id) {
            node.needs_render = true;
        }
        
        // Add to render queue if not already
        if !self.render_queue.contains(&id) {
            self.render_queue.push(id);
        }
    }
    
    /// Mark a widget as needing layout
    pub fn mark_needs_layout(&mut self, id: WidgetId) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.needs_layout = true;
        }
        
        // Add to layout queue
        if !self.layout_queue.contains(&id) {
            self.layout_queue.push_back(id);
        }
    }
    
    /// Propagate dirty state up the tree
    pub fn propagate_dirty(&mut self, id: WidgetId) {
        let mut current = Some(id);
        
        while let Some(node_id) = current {
            self.mark_dirty(node_id);
            
            current = self.nodes.get(&node_id).and_then(|n| n.parent_id);
        }
    }
    
    /// Get a widget node by ID
    pub fn get(&self, id: WidgetId) -> Option<&WidgetNode> {
        self.nodes.get(&id)
    }
    
    /// Get a mutable widget node by ID
    pub fn get_mut(&mut self, id: WidgetId) -> Option<&mut WidgetNode> {
        self.nodes.get_mut(&id)
    }
    
    /// Get widget by ID
    pub fn get_widget(&self, id: WidgetId) -> Option<&dyn Widget> {
        self.nodes.get(&id).map(|n| n.widget.as_ref())
    }
    
    /// Get mutable widget by ID
    pub fn get_widget_mut<'a>(&'a mut self, id: WidgetId) -> Option<&'a mut dyn Widget> {
        let node = self.nodes.get_mut(&id)?;
        Some(node.widget.as_mut())
    }
    
    /// Find widget at position (hit testing)
    pub fn hit_test(&self, x: i32, y: i32) -> Option<WidgetId> {
        // Check cache first
        if let Some(&id) = self.hover_cache.get(&(x, y)) {
            if let Some(node) = self.nodes.get(&id) {
                if node.visible && node.cached_rect.contains(x, y) {
                    return Some(id);
                }
            }
        }
        
        // Walk tree in reverse z-order (top to bottom)
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
    
    /// Update hover state
    pub fn update_hover(&mut self, x: i32, y: i32) -> bool {
        let new_hover = self.hit_test(x, y);
        
        if new_hover != self.hovered_widget {
            // Leave old widget
            if let Some(old_id) = self.hovered_widget {
                if let Some(node) = self.nodes.get_mut(&old_id) {
                    // widget.on_mouse_leave() would be called here
                    node.needs_render = true;
                }
            }
            
            // Enter new widget
            if let Some(new_id) = new_hover {
                if let Some(node) = self.nodes.get_mut(&new_id) {
                    // widget.on_mouse_enter() would be called here
                    node.needs_render = true;
                }
            }
            
            self.hovered_widget = new_hover;
            
            // Update cache
            self.hover_cache.insert((x, y), new_hover.unwrap_or(WidgetId::ROOT));
            
            true
        } else {
            false
        }
    }
    
    /// Process layout queue
    pub fn process_layout(&mut self) {
        while let Some(id) = self.layout_queue.pop_front() {
            if let Some(node) = self.nodes.get_mut(&id) {
                // Layout this widget
                // (would call widget.measure() and widget.arrange())
                node.needs_layout = false;
                
                // Mark for render after layout
                node.needs_render = true;
                self.dirty_widgets.insert(id);
            }
        }
    }
    
    /// Render all dirty widgets
    pub fn render(&mut self, fb: &mut Framebuffer) -> usize {
        self.frame += 1;
        let mut rendered = 0;
        
        // Process layout first
        self.process_layout();
        
        // Sort render queue by z-index (back to front)
        self.render_queue.sort_by_key(|&id| {
            self.nodes.get(&id).map(|n| n.z_index).unwrap_or(0)
        });
        
        // Render each dirty widget
        for &id in &self.render_queue {
            if let Some(node) = self.nodes.get_mut(&id) {
                if node.visible && node.needs_render {
                    // Render widget
                    node.widget.draw(fb);
                    node.needs_render = false;
                    rendered += 1;
                }
            }
        }
        
        // Clear render queue
        self.render_queue.clear();
        
        // Update hashes
        for &id in &self.dirty_widgets {
            if let Some(node) = self.nodes.get_mut(&id) {
                node.compute_hash();
            }
        }
        
        // Clear dirty set
        self.dirty_widgets.clear();
        
        rendered
    }
    
    /// Get focused widget
    pub fn focused(&self) -> Option<WidgetId> {
        self.focused_widget
    }
    
    /// Set focus to a widget
    pub fn set_focus(&mut self, id: WidgetId) {
        // Remove focus from old
        if let Some(old_id) = self.focused_widget {
            if let Some(node) = self.nodes.get_mut(&old_id) {
                // widget.on_blur()
                node.needs_render = true;
            }
        }
        
        // Set new focus
        if self.nodes.contains_key(&id) {
            if let Some(node) = self.nodes.get_mut(&id) {
                // widget.on_focus()
                node.needs_render = true;
            }
            self.focused_widget = Some(id);
        }
    }
    
    /// Focus next widget in tab order
    pub fn focus_next(&mut self) {
        if let Some(current) = self.focused_widget {
            if let Some(idx) = self.focus_chain.iter().position(|&id| id == current) {
                let next_idx = (idx + 1) % self.focus_chain.len();
                self.set_focus(self.focus_chain[next_idx]);
                return;
            }
        }
        
        // Focus first
        if let Some(&first) = self.focus_chain.first() {
            self.set_focus(first);
        }
    }
    
    /// Focus previous widget in tab order
    pub fn focus_prev(&mut self) {
        if let Some(current) = self.focused_widget {
            if let Some(idx) = self.focus_chain.iter().position(|&id| id == current) {
                let prev_idx = if idx == 0 { self.focus_chain.len() - 1 } else { idx - 1 };
                self.set_focus(self.focus_chain[prev_idx]);
                return;
            }
        }
        
        // Focus last
        if let Some(&last) = self.focus_chain.last() {
            self.set_focus(last);
        }
    }
    
    /// Get children of a widget
    pub fn children(&self, id: WidgetId) -> Option<&Vec<WidgetId>> {
        self.nodes.get(&id).map(|n| &n.children)
    }
    
    /// Get parent of a widget
    pub fn parent(&self, id: WidgetId) -> Option<WidgetId> {
        self.nodes.get(&id).and_then(|n| n.parent_id)
    }
    
    /// Get widget count
    pub fn count(&self) -> usize {
        self.nodes.len()
    }
    
    /// Get dirty widget count
    pub fn dirty_count(&self) -> usize {
        self.dirty_widgets.len()
    }
    
    /// Check if tree has dirty widgets
    pub fn is_dirty(&self) -> bool {
        !self.dirty_widgets.is_empty() || !self.render_queue.is_empty()
    }
    
    /// Clear all dirty state
    pub fn clear_dirty(&mut self) {
        self.dirty_widgets.clear();
        self.render_queue.clear();
        self.layout_queue.clear();
        
        for node in self.nodes.values_mut() {
            node.needs_render = false;
            node.needs_layout = false;
        }
    }
    
    /// Set widget visibility
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
    
    /// Set widget enabled state
    pub fn set_enabled(&mut self, id: WidgetId, enabled: bool) {
        if let Some(node) = self.nodes.get_mut(&id) {
            if node.enabled != enabled {
                node.enabled = enabled;
                self.mark_dirty(id);
            }
        }
    }
    
    /// Set widget z-index
    pub fn set_z_index(&mut self, id: WidgetId, z_index: i32) {
        if let Some(node) = self.nodes.get_mut(&id) {
            if node.z_index != z_index {
                node.z_index = z_index;
                self.mark_dirty(id);
            }
        }
    }
    
    /// Bring widget to front
    pub fn bring_to_front(&mut self, id: WidgetId) {
        let max_z = self.nodes.values().map(|n| n.z_index).max().unwrap_or(0);
        self.set_z_index(id, max_z + 1);
    }
    
    /// Send widget to back
    pub fn send_to_back(&mut self, id: WidgetId) {
        let min_z = self.nodes.values().map(|n| n.z_index).min().unwrap_or(0);
        self.set_z_index(id, min_z - 1);
    }
    
    /// Get frame count
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
// WIDGET TREE BUILDER
// ============================================================================

/// Builder for creating widget trees
pub struct WidgetTreeBuilder {
    tree: WidgetTree,
    current_parent: WidgetId,
}

impl WidgetTreeBuilder {
    pub fn new(width: i32, height: i32) -> Self {
        // Create root with dummy widget
        let root_widget = Box::new(DummyWidget::new(width, height));
        WidgetTreeBuilder {
            tree: WidgetTree::with_root(root_widget, width, height),
            current_parent: WidgetId::ROOT,
        }
    }
    
    /// Add a widget to current parent
    pub fn add(mut self, widget: Box<dyn Widget>) -> Self {
        self.tree.add_widget(self.current_parent, widget);
        self
    }
    
    /// Add widget and make it the new parent
    pub fn add_container(mut self, widget: Box<dyn Widget>) -> Self {
        let id = self.tree.add_widget(self.current_parent, widget);
        self.current_parent = id;
        self
    }
    
    /// Go up to parent
    pub fn end_container(mut self) -> Self {
        if let Some(parent) = self.tree.parent(self.current_parent) {
            self.current_parent = parent;
        }
        self
    }
    
    /// Build the tree
    pub fn build(self) -> WidgetTree {
        self.tree
    }
}

// ============================================================================
// DUMMY WIDGET (for root)
// ============================================================================

/// Dummy widget for root container
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
        // Root doesn't draw anything
    }
    
    fn on_click(&mut self, _x: i32, _y: i32) -> bool {
        false
    }
    
    fn bounds(&self) -> Rect {
        self.rect
    }
}

// ============================================================================
// GLOBAL WIDGET TREE
// ============================================================================

lazy_static::lazy_static! {
    static ref WIDGET_TREE: Mutex<WidgetTree> = Mutex::new(WidgetTree::new());
}

/// Get global widget tree
pub fn get_tree() -> &'static Mutex<WidgetTree> {
    &WIDGET_TREE
}

/// Add widget to global tree
pub fn add_widget(widget: Box<dyn Widget>) -> WidgetId {
    WIDGET_TREE.lock().add_to_root(widget)
}

/// Render global tree
pub fn render_tree(fb: &mut Framebuffer) -> usize {
    WIDGET_TREE.lock().render(fb)
}

/// Hit test in global tree
pub fn hit_test(x: i32, y: i32) -> Option<WidgetId> {
    WIDGET_TREE.lock().hit_test(x, y)
}

/// Initialize widget tree with screen size
pub fn init(width: i32, height: i32) {
    *WIDGET_TREE.lock() = WidgetTree::with_root(
        Box::new(DummyWidget::new(width, height)),
        width, height
    );
    crate::serial_println!("[GUI] Widget tree initialized ({}x{})", width, height);
}
