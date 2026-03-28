//! Surface registry for desktop composition.

use crate::gui::protocol::{
    AppId, DamageEpoch, FenceId, GpuBufferHandle, Rect, SceneUpdate, SharedSurfaceDescriptor,
    SurfaceId, WindowBufferMode,
};
use crate::gui::surface_memory::{
    publish_data_plane_surface, revoke_data_plane_surface, SharedSurfaceMemory,
};
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;

const MAX_SURFACE_DIMENSION: u32 = 8192;

#[derive(Clone, Debug)]
pub struct SurfaceRecord {
    pub id: SurfaceId,
    pub app_id: AppId,
    pub rect: Rect,
    pub visible: bool,
    pub z_index: u32,
    pub pixels: Vec<u32>,
    pub shared: Option<Arc<SharedSurfaceMemory>>,
    pub dirty: bool,
    pub gpu_buffer_handle: GpuBufferHandle,
    pub damage_epoch: DamageEpoch,
    pub fence_id: FenceId,
    pub scene_update: Option<SceneUpdate>,
}

#[derive(Clone, Debug)]
pub struct SurfaceInfo {
    pub id: SurfaceId,
    pub app_id: AppId,
    pub rect: Rect,
    pub visible: bool,
    pub z_index: u32,
    pub shared_mapped: bool,
    pub dirty: bool,
    pub gpu_buffer_handle: GpuBufferHandle,
    pub damage_epoch: DamageEpoch,
    pub fence_id: FenceId,
    pub scene_root: Option<u64>,
    pub buffer_mode: WindowBufferMode,
}

#[derive(Clone, Debug)]
pub enum SurfaceError {
    InvalidSize,
    SurfaceNotFound,
    BufferSizeMismatch,
    OutOfMemory,
    SharedSurfaceUnavailable,
}

pub struct SurfaceManager {
    next_id: SurfaceId,
    surfaces: BTreeMap<SurfaceId, SurfaceRecord>,
}

impl SurfaceManager {
    fn effective_object_bounds(object: &crate::gui::protocol::RenderObject) -> Rect {
        match object.clip {
            Some(clip) => object.bounds.intersection(&clip).unwrap_or_default(),
            None => object.bounds,
        }
    }

    fn scene_damage(
        previous: Option<&SceneUpdate>,
        next: &SceneUpdate,
        fallback: Rect,
    ) -> Vec<Rect> {
        let mut damage = Vec::new();
        if let Some(previous) = previous {
            let previous_map = previous
                .render_objects
                .iter()
                .map(|object| (object.object_id, object))
                .collect::<BTreeMap<_, _>>();
            let next_map = next
                .render_objects
                .iter()
                .map(|object| (object.object_id, object))
                .collect::<BTreeMap<_, _>>();

            for object_id in previous_map
                .keys()
                .chain(next_map.keys())
                .copied()
                .collect::<alloc::collections::BTreeSet<_>>()
            {
                let previous_object = previous_map.get(&object_id).copied();
                let next_object = next_map.get(&object_id).copied();
                let rect = match (previous_object, next_object) {
                    (Some(old), Some(new)) if old == new => None,
                    (Some(old), Some(new)) => {
                        let old_bounds = Self::effective_object_bounds(old);
                        let new_bounds = Self::effective_object_bounds(new);
                        Some(old_bounds.union(&new_bounds))
                    }
                    (Some(old), None) => Some(Self::effective_object_bounds(old)),
                    (None, Some(new)) => Some(Self::effective_object_bounds(new)),
                    (None, None) => None,
                };
                if let Some(rect) = rect {
                    if rect.is_empty() || damage.iter().any(|existing| *existing == rect) {
                        continue;
                    }
                    damage.push(rect);
                }
            }
        }
        for rect in next.damage_hint.iter().copied() {
            if rect.is_empty() || damage.iter().any(|existing| *existing == rect) {
                continue;
            }
            damage.push(rect);
        }
        if damage.is_empty() {
            damage.push(fallback);
        }
        damage
    }

    pub fn new() -> Self {
        Self {
            next_id: 1,
            surfaces: BTreeMap::new(),
        }
    }

    pub fn create_surface(
        &mut self,
        app_id: AppId,
        width: u32,
        height: u32,
    ) -> Result<SurfaceId, SurfaceError> {
        Self::validate_dimensions(width, height)?;
        let len = Self::pixel_len(width, height)?;

        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        let record = SurfaceRecord {
            id,
            app_id,
            rect: Rect::new(0, 0, width, height),
            visible: true,
            z_index: id as u32,
            pixels: vec![0; len],
            shared: Some(SharedSurfaceMemory::new(width, height)),
            dirty: true,
            gpu_buffer_handle: id,
            damage_epoch: 1,
            fence_id: 0,
            scene_update: None,
        };
        self.surfaces.insert(id, record);
        Ok(id)
    }

    pub fn destroy_surface(&mut self, surface_id: SurfaceId) -> bool {
        if let Some(surface) = self.surfaces.remove(&surface_id) {
            revoke_data_plane_surface(surface.app_id, surface_id);
            true
        } else {
            false
        }
    }

    pub fn destroy_surfaces_for_app(&mut self, app_id: AppId) {
        let revoked = self
            .surfaces
            .iter()
            .filter_map(|(surface_id, surface)| (surface.app_id == app_id).then_some(*surface_id))
            .collect::<Vec<_>>();
        self.surfaces.retain(|_, s| s.app_id != app_id);
        for surface_id in revoked {
            revoke_data_plane_surface(app_id, surface_id);
        }
    }

    pub fn set_geometry(
        &mut self,
        surface_id: SurfaceId,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<(), SurfaceError> {
        Self::validate_dimensions(width, height)?;
        let len = Self::pixel_len(width, height)?;

        let surface = self
            .surfaces
            .get_mut(&surface_id)
            .ok_or(SurfaceError::SurfaceNotFound)?;
        surface.rect = Rect::new(x, y, width, height);
        if surface.pixels.len() != len {
            surface.pixels.resize(len, 0);
        }
        if let Some(shared) = surface.shared.as_ref() {
            shared.resize(width, height);
        }
        surface.dirty = true;
        surface.damage_epoch = surface.damage_epoch.saturating_add(1);
        Ok(())
    }

    pub fn set_visible(
        &mut self,
        surface_id: SurfaceId,
        visible: bool,
    ) -> Result<(), SurfaceError> {
        let surface = self
            .surfaces
            .get_mut(&surface_id)
            .ok_or(SurfaceError::SurfaceNotFound)?;
        surface.visible = visible;
        surface.dirty = true;
        surface.damage_epoch = surface.damage_epoch.saturating_add(1);
        Ok(())
    }

    pub fn set_z_index(&mut self, surface_id: SurfaceId, z_index: u32) -> Result<(), SurfaceError> {
        let surface = self
            .surfaces
            .get_mut(&surface_id)
            .ok_or(SurfaceError::SurfaceNotFound)?;
        surface.z_index = z_index;
        surface.dirty = true;
        surface.damage_epoch = surface.damage_epoch.saturating_add(1);
        Ok(())
    }

    pub fn commit_buffer(
        &mut self,
        surface_id: SurfaceId,
        pixels: &[u32],
    ) -> Result<(), SurfaceError> {
        let surface = self
            .surfaces
            .get_mut(&surface_id)
            .ok_or(SurfaceError::SurfaceNotFound)?;
        let expected = Self::pixel_len(surface.rect.width, surface.rect.height)?;
        if pixels.len() != expected {
            return Err(SurfaceError::BufferSizeMismatch);
        }
        surface.pixels.copy_from_slice(pixels);
        if let Some(shared) = surface.shared.as_ref() {
            let _ = shared.write_full(pixels);
        }
        surface.scene_update = None;
        surface.dirty = true;
        surface.damage_epoch = surface.damage_epoch.saturating_add(1);
        Ok(())
    }

    pub fn commit_scene(
        &mut self,
        surface_id: SurfaceId,
        mut scene: SceneUpdate,
    ) -> Result<Vec<Rect>, SurfaceError> {
        let surface = self
            .surfaces
            .get_mut(&surface_id)
            .ok_or(SurfaceError::SurfaceNotFound)?;
        scene.canonicalize();
        scene.damage_hint = Self::scene_damage(
            surface.scene_update.as_ref(),
            &scene,
            Rect::new(0, 0, surface.rect.width, surface.rect.height),
        );
        let damage_hint = scene.damage_hint.clone();
        surface.scene_update = Some(scene);
        surface.dirty = true;
        surface.damage_epoch = surface.damage_epoch.saturating_add(1);
        Ok(damage_hint)
    }

    pub fn map_shared_surface(
        &mut self,
        surface_id: SurfaceId,
    ) -> Result<SharedSurfaceDescriptor, SurfaceError> {
        let surface = self
            .surfaces
            .get_mut(&surface_id)
            .ok_or(SurfaceError::SurfaceNotFound)?;
        let shared = surface
            .shared
            .get_or_insert_with(|| {
                SharedSurfaceMemory::new(surface.rect.width, surface.rect.height)
            })
            .clone();
        Ok(SharedSurfaceDescriptor {
            client_id: surface.app_id,
            surface_id,
            width: surface.rect.width,
            height: surface.rect.height,
            pixel_stride: surface.rect.width,
            generation: shared.generation(),
        })
        .map(|descriptor| {
            publish_data_plane_surface(descriptor, &shared);
            descriptor
        })
    }

    pub fn shared_surface(&self, surface_id: SurfaceId) -> Option<Arc<SharedSurfaceMemory>> {
        self.surfaces
            .get(&surface_id)
            .and_then(|surface| surface.shared.as_ref().cloned())
    }

    pub fn submit_shared_damage(
        &mut self,
        surface_id: SurfaceId,
        rect: Rect,
        generation: u64,
    ) -> Result<(), SurfaceError> {
        let surface = self
            .surfaces
            .get_mut(&surface_id)
            .ok_or(SurfaceError::SurfaceNotFound)?;
        let Some(shared) = surface.shared.as_ref() else {
            return Err(SurfaceError::SharedSurfaceUnavailable);
        };
        if generation < shared.generation() {
            return Err(SurfaceError::SharedSurfaceUnavailable);
        }
        shared.submit_damage(rect);
        surface.dirty = true;
        surface.damage_epoch = surface.damage_epoch.saturating_add(1);
        Ok(())
    }

    pub fn mark_present_fence(
        &mut self,
        surface_id: SurfaceId,
        fence_id: FenceId,
    ) -> Result<(), SurfaceError> {
        let surface = self
            .surfaces
            .get_mut(&surface_id)
            .ok_or(SurfaceError::SurfaceNotFound)?;
        surface.fence_id = fence_id;
        Ok(())
    }

    pub fn snapshot(&self, surface_id: SurfaceId) -> Option<SurfaceRecord> {
        self.surfaces.get(&surface_id).map(|surface| {
            let mut cloned = surface.clone();
            if let Some(shared) = surface.shared.as_ref() {
                cloned.pixels = shared.snapshot().pixels;
            }
            cloned
        })
    }

    pub fn list_surfaces(&self) -> Vec<SurfaceInfo> {
        self.surfaces
            .values()
            .map(|s| SurfaceInfo {
                id: s.id,
                app_id: s.app_id,
                rect: s.rect,
                visible: s.visible,
                z_index: s.z_index,
                shared_mapped: s.shared.is_some(),
                dirty: s.dirty,
                gpu_buffer_handle: s.gpu_buffer_handle,
                damage_epoch: s.damage_epoch,
                fence_id: s.fence_id,
                scene_root: s.scene_update.as_ref().map(|scene| scene.root_id),
                buffer_mode: if s.scene_update.is_some() {
                    WindowBufferMode::Scene
                } else {
                    WindowBufferMode::Pixels
                },
            })
            .collect()
    }

    pub fn has_dirty_surface(&self) -> bool {
        self.surfaces.values().any(|s| s.visible && s.dirty)
    }

    pub fn clear_dirty(&mut self) {
        for surface in self.surfaces.values_mut() {
            surface.dirty = false;
        }
    }

    fn validate_dimensions(width: u32, height: u32) -> Result<(), SurfaceError> {
        if width == 0 || height == 0 {
            return Err(SurfaceError::InvalidSize);
        }
        if width > MAX_SURFACE_DIMENSION || height > MAX_SURFACE_DIMENSION {
            return Err(SurfaceError::InvalidSize);
        }
        Ok(())
    }

    fn pixel_len(width: u32, height: u32) -> Result<usize, SurfaceError> {
        let len_u64 = (width as u64).saturating_mul(height as u64);
        if len_u64 > usize::MAX as u64 {
            return Err(SurfaceError::OutOfMemory);
        }
        Ok(len_u64 as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::protocol::{DamageLane, RenderObject, RenderObjectKind, SceneUpdate};

    fn solid(id: u64, rect: Rect) -> RenderObject {
        RenderObject {
            object_id: id,
            bounds: rect,
            clip: None,
            z_index: id as u32,
            opacity: u8::MAX,
            lane: DamageLane::Shell,
            kind: RenderObjectKind::SolidRect {
                color: 0xFF00FF00,
                corner_radius: 0,
            },
        }
    }

    #[test]
    fn scene_damage_only_marks_changed_objects() {
        let previous = SceneUpdate {
            root_id: 1,
            revision: 1,
            render_objects: vec![
                solid(1, Rect::new(0, 0, 40, 20)),
                solid(2, Rect::new(50, 0, 30, 20)),
            ],
            damage_hint: Vec::new(),
            semantic_root: None,
        };
        let next = SceneUpdate {
            root_id: 1,
            revision: 2,
            render_objects: vec![
                solid(1, Rect::new(0, 0, 40, 20)),
                solid(2, Rect::new(56, 0, 30, 20)),
            ],
            damage_hint: Vec::new(),
            semantic_root: None,
        };

        let damage = SurfaceManager::scene_damage(Some(&previous), &next, Rect::new(0, 0, 96, 48));
        assert_eq!(damage, vec![Rect::new(50, 0, 36, 20)]);
    }

    #[test]
    fn commit_scene_returns_precise_damage_hint() {
        let mut surfaces = SurfaceManager::new();
        let surface_id = surfaces.create_surface(7, 96, 48).unwrap();
        let first = SceneUpdate {
            root_id: 3,
            revision: 1,
            render_objects: vec![
                solid(1, Rect::new(0, 0, 20, 20)),
                solid(2, Rect::new(24, 0, 20, 20)),
            ],
            damage_hint: Vec::new(),
            semantic_root: None,
        };
        let second = SceneUpdate {
            root_id: 3,
            revision: 2,
            render_objects: vec![
                solid(1, Rect::new(0, 0, 20, 20)),
                solid(2, Rect::new(30, 0, 20, 20)),
            ],
            damage_hint: Vec::new(),
            semantic_root: None,
        };

        let first_damage = surfaces.commit_scene(surface_id, first).unwrap();
        assert_eq!(first_damage, vec![Rect::new(0, 0, 96, 48)]);

        let second_damage = surfaces.commit_scene(surface_id, second).unwrap();
        assert_eq!(second_damage, vec![Rect::new(24, 0, 26, 20)]);
    }
}
