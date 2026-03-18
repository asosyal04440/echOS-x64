use crate::gui::protocol::{DamageLane, Rect, RenderObjectKind, SceneUpdate, WindowId};
use crate::gui::scene::SceneGraph;
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
