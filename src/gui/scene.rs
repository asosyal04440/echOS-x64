use crate::gui::protocol::{
    DamageLane, Rect, RenderObject, RenderObjectKind, SceneNodeId, SceneRevision, SceneRootId,
    SceneUpdate,
};
use alloc::vec;
use alloc::vec::Vec;

const ROOT_INDEX: usize = 0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SceneNodeKind {
    Group,
    Clip { rect: Rect },
    Render(RenderObject),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneNode {
    pub id: SceneNodeId,
    pub parent: Option<SceneNodeId>,
    pub bounds: Rect,
    pub dirty_generation: SceneRevision,
    pub children: Vec<SceneNodeId>,
    pub kind: SceneNodeKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SceneSlot {
    generation: u32,
    node: Option<SceneNode>,
}

pub struct SceneGraph {
    root: SceneNodeId,
    next_object_id: u64,
    revision: SceneRevision,
    semantic_root: Option<u64>,
    dirty_bounds: Vec<Rect>,
    slots: Vec<SceneSlot>,
    free_list: Vec<usize>,
    flat_render_cache: Vec<RenderObject>,
    cached_revision: SceneRevision,
}

impl SceneGraph {
    pub fn new(bounds: Rect) -> Self {
        let root_generation = 1;
        let root = make_node_id(ROOT_INDEX, root_generation);
        Self {
            root,
            next_object_id: 1,
            revision: 1,
            semantic_root: None,
            dirty_bounds: vec![bounds],
            slots: vec![SceneSlot {
                generation: root_generation,
                node: Some(SceneNode {
                    id: root,
                    parent: None,
                    bounds,
                    dirty_generation: 1,
                    children: Vec::new(),
                    kind: SceneNodeKind::Group,
                }),
            }],
            free_list: Vec::new(),
            flat_render_cache: Vec::new(),
            cached_revision: 0,
        }
    }

    pub fn root(&self) -> SceneNodeId {
        self.root
    }

    pub fn set_semantic_root(&mut self, semantic_root: Option<u64>) {
        self.semantic_root = semantic_root;
    }

    pub fn push_group(&mut self, parent: SceneNodeId, bounds: Rect) -> Option<SceneNodeId> {
        self.push_node(parent, bounds, SceneNodeKind::Group)
    }

    pub fn push_clip(
        &mut self,
        parent: SceneNodeId,
        clip_rect: Rect,
        bounds: Rect,
    ) -> Option<SceneNodeId> {
        self.push_node(parent, bounds, SceneNodeKind::Clip { rect: clip_rect })
    }

    pub fn push_render_object(
        &mut self,
        parent: SceneNodeId,
        bounds: Rect,
        lane: DamageLane,
        kind: RenderObjectKind,
    ) -> Option<SceneNodeId> {
        let object_id = self.alloc_object_id();
        self.push_node(
            parent,
            bounds,
            SceneNodeKind::Render(RenderObject {
                object_id,
                bounds,
                clip: None,
                z_index: object_id as u32,
                opacity: 255,
                lane,
                kind,
            }),
        )
    }

    pub fn replace_root_children(&mut self, children: Vec<SceneNodeId>) {
        let generation = self.bump_revision();
        let root_bounds = match self.get_node_mut(self.root) {
            Some(root) => {
                root.children = children;
                root.dirty_generation = generation;
                root.bounds
            }
            None => return,
        };
        self.cached_revision = 0;
        self.dirty_bounds.push(root_bounds);
    }

    pub fn snapshot(&mut self, root_id: SceneRootId) -> SceneUpdate {
        self.rebuild_flat_render_cache();
        let mut damage_hint = core::mem::take(&mut self.dirty_bounds);
        damage_hint.retain(|rect| !rect.is_empty());
        damage_hint.sort_by_key(|rect| (rect.y, rect.x, rect.height, rect.width));
        damage_hint.dedup();

        let mut snapshot = SceneUpdate {
            root_id,
            revision: self.revision,
            render_objects: self.flat_render_cache.clone(),
            damage_hint,
            semantic_root: self.semantic_root,
        };
        snapshot.canonicalize();
        snapshot
    }

    fn push_node(
        &mut self,
        parent: SceneNodeId,
        bounds: Rect,
        kind: SceneNodeKind,
    ) -> Option<SceneNodeId> {
        if self.get_node(parent).is_none() {
            return None;
        }

        let id = self.alloc_node_id();
        let generation = self.bump_revision();
        let node = SceneNode {
            id,
            parent: Some(parent),
            bounds,
            dirty_generation: generation,
            children: Vec::new(),
            kind,
        };

        let node_index = node_index(id);
        self.ensure_slot(node_index, node_generation(id));
        self.slots[node_index].node = Some(node);

        if let Some(parent_node) = self.get_node_mut(parent) {
            parent_node.children.push(id);
            parent_node.dirty_generation = generation;
        }
        self.cached_revision = 0;
        self.dirty_bounds.push(bounds);
        Some(id)
    }

    fn rebuild_flat_render_cache(&mut self) {
        if self.cached_revision == self.revision {
            return;
        }

        self.flat_render_cache.clear();
        let root = self.root;
        self.collect_render_objects(root, None);
        self.flat_render_cache
            .sort_by_key(|object| (object.z_index, object.object_id));
        self.cached_revision = self.revision;
    }

    fn collect_render_objects(&mut self, node_id: SceneNodeId, inherited_clip: Option<Rect>) {
        let Some(node) = self.get_node(node_id).cloned() else {
            return;
        };

        let next_clip = match node.kind {
            SceneNodeKind::Group => inherited_clip,
            SceneNodeKind::Clip { rect } => {
                let combined = match inherited_clip {
                    Some(current) => current.intersection(&rect),
                    None => Some(rect),
                };
                let Some(combined) = combined else {
                    return;
                };
                Some(combined)
            }
            SceneNodeKind::Render(mut object) => {
                object.clip = match (object.clip, inherited_clip) {
                    (Some(local), Some(parent)) => parent.intersection(&local),
                    (Some(local), None) => Some(local),
                    (None, Some(parent)) => Some(parent),
                    (None, None) => None,
                };
                self.flat_render_cache.push(object);
                inherited_clip
            }
        };

        for child in node.children.iter().copied() {
            self.collect_render_objects(child, next_clip);
        }
    }

    fn alloc_object_id(&mut self) -> u64 {
        let id = self.next_object_id;
        self.next_object_id = self.next_object_id.saturating_add(1);
        id
    }

    fn alloc_node_id(&mut self) -> SceneNodeId {
        if let Some(index) = self.free_list.pop() {
            let generation = self.slots[index].generation.saturating_add(1).max(1);
            self.slots[index].generation = generation;
            return make_node_id(index, generation);
        }

        let index = self.slots.len();
        self.slots.push(SceneSlot {
            generation: 1,
            node: None,
        });
        make_node_id(index, 1)
    }

    fn ensure_slot(&mut self, index: usize, generation: u32) {
        while self.slots.len() <= index {
            self.slots.push(SceneSlot {
                generation: 0,
                node: None,
            });
        }
        self.slots[index].generation = generation;
    }

    fn get_node(&self, node_id: SceneNodeId) -> Option<&SceneNode> {
        let slot = self.slots.get(node_index(node_id))?;
        if slot.generation != node_generation(node_id) {
            return None;
        }
        slot.node.as_ref()
    }

    fn get_node_mut(&mut self, node_id: SceneNodeId) -> Option<&mut SceneNode> {
        let slot = self.slots.get_mut(node_index(node_id))?;
        if slot.generation != node_generation(node_id) {
            return None;
        }
        slot.node.as_mut()
    }

    fn bump_revision(&mut self) -> SceneRevision {
        self.revision = self.revision.saturating_add(1);
        self.revision
    }
}

fn make_node_id(index: usize, generation: u32) -> SceneNodeId {
    ((generation as u64) << 32) | index as u64
}

fn node_index(node_id: SceneNodeId) -> usize {
    (node_id & 0xFFFF_FFFF) as usize
}

fn node_generation(node_id: SceneNodeId) -> u32 {
    (node_id >> 32) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_nodes_bound_descendant_render_objects() {
        let mut graph = SceneGraph::new(Rect::new(0, 0, 200, 120));
        let clip = graph
            .push_clip(
                graph.root(),
                Rect::new(20, 10, 60, 30),
                Rect::new(20, 10, 60, 30),
            )
            .unwrap();
        let _ = graph.push_render_object(
            clip,
            Rect::new(0, 0, 100, 40),
            DamageLane::Window,
            RenderObjectKind::SolidRect {
                color: 0xFF00FF00,
                corner_radius: 0,
            },
        );

        let snapshot = graph.snapshot(7);
        assert_eq!(snapshot.root_id, 7);
        assert_eq!(snapshot.render_objects.len(), 1);
        assert_eq!(
            snapshot.render_objects[0].clip,
            Some(Rect::new(20, 10, 60, 30))
        );
    }

    #[test]
    fn snapshot_reuses_flattened_cache_until_graph_changes() {
        let mut graph = SceneGraph::new(Rect::new(0, 0, 80, 40));
        let _ = graph.push_render_object(
            graph.root(),
            Rect::new(0, 0, 12, 8),
            DamageLane::Shell,
            RenderObjectKind::SolidRect {
                color: 0xFF001122,
                corner_radius: 0,
            },
        );

        let first = graph.snapshot(1);
        assert_eq!(first.damage_hint, vec![Rect::new(0, 0, 80, 40), Rect::new(0, 0, 12, 8)]);

        let second = graph.snapshot(1);
        assert!(second.damage_hint.is_empty());
        assert_eq!(first.render_objects, second.render_objects);

        let _ = graph.push_render_object(
            graph.root(),
            Rect::new(16, 0, 12, 8),
            DamageLane::Shell,
            RenderObjectKind::SolidRect {
                color: 0xFF334455,
                corner_radius: 0,
            },
        );
        let third = graph.snapshot(1);
        assert_eq!(third.render_objects.len(), 2);
        assert_eq!(third.revision, first.revision + 1);
    }
}
