use crate::gui::protocol::{
    DamageLane, Rect, RenderObjectKind, SceneNodeId, SceneUpdate, WindowId,
};
use crate::gui::scene::SceneGraph;
use crate::gui::text::{TextStyle, TextSystem};
use alloc::vec::Vec;

pub fn raster_surface_scene(
    window_id: WindowId,
    width: usize,
    height: usize,
    pixels: Vec<u32>,
    lane: DamageLane,
) -> SceneUpdate {
    let bounds = Rect::new(0, 0, width as u32, height as u32);
    let mut graph = SceneGraph::new(bounds);
    if let Some(clip) = graph.push_clip(graph.root(), bounds, bounds) {
        let _ = graph.push_render_object(
            clip,
            bounds,
            lane,
            RenderObjectKind::Raster {
                width: width as u32,
                height: height as u32,
                pixels,
            },
        );
    }
    graph.snapshot(window_id)
}

pub fn push_scene_rect(scene: &mut SceneGraph, parent: SceneNodeId, bounds: Rect, color: u32) {
    push_scene_round_rect(scene, parent, bounds, color, 0);
}

pub fn push_scene_round_rect(
    scene: &mut SceneGraph,
    parent: SceneNodeId,
    bounds: Rect,
    color: u32,
    corner_radius: u16,
) {
    let _ = scene.push_render_object(
        parent,
        bounds,
        DamageLane::Shell,
        RenderObjectKind::SolidRect {
            color,
            corner_radius,
        },
    );
}

pub fn push_scene_panel(
    scene: &mut SceneGraph,
    parent: SceneNodeId,
    bounds: Rect,
    fill: u32,
    border: u32,
    corner_radius: u16,
    top_accent: Option<u32>,
) {
    push_scene_round_rect(scene, parent, bounds, fill, corner_radius);
    push_scene_outline(scene, parent, bounds, border);
    if let Some(accent) = top_accent {
        let inset = (corner_radius as i32 / 2).max(0);
        let width = bounds
            .width
            .saturating_sub((inset as u32).saturating_mul(2))
            .max(1);
        push_scene_round_rect(
            scene,
            parent,
            Rect::new(bounds.x + inset, bounds.y, width, 1),
            accent,
            1,
        );
    }
}

pub fn push_scene_outline(scene: &mut SceneGraph, parent: SceneNodeId, bounds: Rect, color: u32) {
    if bounds.width == 0 || bounds.height == 0 {
        return;
    }
    push_scene_rect(
        scene,
        parent,
        Rect::new(bounds.x, bounds.y, bounds.width, 1),
        color,
    );
    push_scene_rect(
        scene,
        parent,
        Rect::new(bounds.x, bounds.bottom().saturating_sub(1), bounds.width, 1),
        color,
    );
    push_scene_rect(
        scene,
        parent,
        Rect::new(bounds.x, bounds.y, 1, bounds.height),
        color,
    );
    push_scene_rect(
        scene,
        parent,
        Rect::new(bounds.right().saturating_sub(1), bounds.y, 1, bounds.height),
        color,
    );
}

pub fn push_scene_text(
    scene: &mut SceneGraph,
    text_system: &mut TextSystem,
    parent: SceneNodeId,
    x: i32,
    y: i32,
    max_width: u32,
    text: &str,
    color: u32,
) {
    let blob = text_system.layout_text_with_style(text, max_width.max(1), TextStyle::ui(), color);
    let _ = scene.push_render_object(
        parent,
        Rect::new(x, y, blob.width_px.max(1), blob.height_px.max(1)),
        DamageLane::Text,
        RenderObjectKind::Raster {
            width: blob.width_px.max(1),
            height: blob.height_px.max(1),
            pixels: blob.pixels,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn raster_surface_scene_wraps_pixels_in_scene_update() {
        let snapshot = raster_surface_scene(41, 8, 4, vec![0xFF112233; 32], DamageLane::Shell);
        assert_eq!(snapshot.root_id, 41);
        assert_eq!(snapshot.render_objects.len(), 1);
        assert_eq!(snapshot.damage_hint, vec![Rect::new(0, 0, 8, 4)]);
        let object = &snapshot.render_objects[0].kind;
        assert!(matches!(object, RenderObjectKind::Raster { .. }));
        if let RenderObjectKind::Raster {
            width,
            height,
            pixels,
        } = object
        {
            assert_eq!((*width, *height), (8, 4));
            assert_eq!(pixels.len(), 32);
        }
    }
}
