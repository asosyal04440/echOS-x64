//! Shared zero-copy surface memory with damage generation tracking.

use crate::allocator::doctrine::{
    alloc_surface_pixels, DoctrineError, SurfacePixelBuffer, SurfacePixelFormat,
};
use crate::gui::protocol::{ClientId, Rect, SharedSurfaceDescriptor, SurfaceId};
use alloc::collections::BTreeMap;
use alloc::sync::{Arc, Weak};
use alloc::vec;
use alloc::vec::Vec;
use core::cmp::min;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use lazy_static::lazy_static;
use spin::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct DataPlaneKey {
    client_id: ClientId,
    surface_id: SurfaceId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataPlaneResolveError {
    SurfaceUnavailable,
    DescriptorMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceAllocError {
    OutOfMemory,
    PolicyViolation,
}

#[derive(Clone)]
pub struct SurfaceSnapshot {
    pub width: u32,
    pub height: u32,
    pub generation: u64,
    pub pixels: Vec<u32>,
}

fn map_doctrine_error(error: DoctrineError) -> SurfaceAllocError {
    crate::serial_println!("[SURFACE] doctrine allocation error: {:?}", error);
    match error {
        DoctrineError::OutOfMemory => SurfaceAllocError::OutOfMemory,
        DoctrineError::PolicyViolation(_)
        | DoctrineError::ExceptionDenied(_)
        | DoctrineError::InvalidRegistry(_) => SurfaceAllocError::PolicyViolation,
    }
}

#[derive(Debug)]
pub struct SharedSurfaceMemory {
    width: AtomicU32,
    height: AtomicU32,
    generation: AtomicU64,
    damage: Mutex<Vec<Rect>>,
    pixels: Mutex<SurfacePixelBuffer>,
}

lazy_static! {
    static ref DATA_PLANE_REGISTRY: Mutex<BTreeMap<DataPlaneKey, Weak<SharedSurfaceMemory>>> =
        Mutex::new(BTreeMap::new());
}

impl SharedSurfaceMemory {
    pub fn new(width: u32, height: u32) -> Result<Arc<Self>, SurfaceAllocError> {
        crate::serial_println!(
            "[SURFACE] allocate shared surface {}x{} ({} bytes)",
            width,
            height,
            (width as usize)
                .saturating_mul(height as usize)
                .saturating_mul(core::mem::size_of::<u32>())
        );
        Ok(Arc::new(Self {
            width: AtomicU32::new(width),
            height: AtomicU32::new(height),
            generation: AtomicU64::new(1),
            damage: Mutex::new(vec![Rect::new(0, 0, width, height)]),
            pixels: Mutex::new(
                alloc_surface_pixels(
                    width as usize,
                    height as usize,
                    SurfacePixelFormat::Argb8888,
                )
                .map_err(map_doctrine_error)?,
            ),
        }))
    }

    pub fn resize(&self, width: u32, height: u32) -> Result<(), SurfaceAllocError> {
        crate::serial_println!(
            "[SURFACE] resize shared surface {}x{} ({} bytes)",
            width,
            height,
            (width as usize)
                .saturating_mul(height as usize)
                .saturating_mul(core::mem::size_of::<u32>())
        );
        self.width.store(width, Ordering::Release);
        self.height.store(height, Ordering::Release);
        let len = (width as usize).saturating_mul(height as usize);
        self.pixels
            .lock()
            .resize_zeroed(len)
            .map_err(map_doctrine_error)?;
        self.submit_damage(Rect::new(0, 0, width, height));
        Ok(())
    }

    pub fn write_full(&self, pixels: &[u32]) -> Result<(), ()> {
        let width = self.width.load(Ordering::Acquire) as usize;
        let height = self.height.load(Ordering::Acquire) as usize;
        let expected = width.saturating_mul(height);
        if pixels.len() != expected {
            return Err(());
        }
        self.pixels.lock().copy_from_slice(pixels);
        self.submit_damage(Rect::new(0, 0, width as u32, height as u32));
        Ok(())
    }

    pub fn snapshot(&self) -> Result<SurfaceSnapshot, SurfaceAllocError> {
        Ok(SurfaceSnapshot {
            width: self.width.load(Ordering::Acquire),
            height: self.height.load(Ordering::Acquire),
            generation: self.generation.load(Ordering::Acquire),
            pixels: self
                .pixels
                .lock()
                .snapshot_vec("gui.shared_surface.snapshot")
                .map_err(map_doctrine_error)?,
        })
    }

    pub fn with_pixels<R>(&self, f: impl FnOnce(&[u32]) -> R) -> R {
        let pixels = self.pixels.lock();
        f(pixels.as_slice())
    }

    pub fn dimensions(&self) -> (u32, u32) {
        (
            self.width.load(Ordering::Acquire),
            self.height.load(Ordering::Acquire),
        )
    }

    pub fn submit_damage(&self, rect: Rect) {
        self.damage.lock().push(rect);
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    pub fn take_damage(&self) -> Vec<Rect> {
        core::mem::take(&mut *self.damage.lock())
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
}

pub fn publish_data_plane_surface(
    descriptor: SharedSurfaceDescriptor,
    surface: &Arc<SharedSurfaceMemory>,
) {
    let key = DataPlaneKey {
        client_id: descriptor.client_id,
        surface_id: descriptor.surface_id,
    };
    DATA_PLANE_REGISTRY
        .lock()
        .insert(key, Arc::downgrade(surface));
}

pub fn revoke_data_plane_surface(client_id: ClientId, surface_id: SurfaceId) {
    DATA_PLANE_REGISTRY.lock().remove(&DataPlaneKey {
        client_id,
        surface_id,
    });
}

pub fn resolve_data_plane_surface(
    descriptor: SharedSurfaceDescriptor,
) -> Result<Arc<SharedSurfaceMemory>, DataPlaneResolveError> {
    let key = DataPlaneKey {
        client_id: descriptor.client_id,
        surface_id: descriptor.surface_id,
    };
    let surface = DATA_PLANE_REGISTRY
        .lock()
        .get(&key)
        .and_then(Weak::upgrade)
        .ok_or(DataPlaneResolveError::SurfaceUnavailable)?;

    let (width, height) = surface.dimensions();
    if width != descriptor.width || height != descriptor.height {
        return Err(DataPlaneResolveError::DescriptorMismatch);
    }

    Ok(surface)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_resolves_live_surface_from_descriptor() {
        let descriptor = SharedSurfaceDescriptor {
            client_id: 7,
            surface_id: 9,
            width: 64,
            height: 32,
            pixel_stride: 64,
            generation: 1,
        };
        let surface = SharedSurfaceMemory::new(64, 32).expect("surface alloc");
        publish_data_plane_surface(descriptor, &surface);

        let resolved = resolve_data_plane_surface(descriptor).expect("surface must resolve");
        assert!(Arc::ptr_eq(&surface, &resolved));

        revoke_data_plane_surface(descriptor.client_id, descriptor.surface_id);
    }

    #[test]
    fn registry_rejects_descriptor_with_stale_geometry() {
        let descriptor = SharedSurfaceDescriptor {
            client_id: 11,
            surface_id: 17,
            width: 64,
            height: 32,
            pixel_stride: 64,
            generation: 1,
        };
        let surface = SharedSurfaceMemory::new(64, 32).expect("surface alloc");
        publish_data_plane_surface(descriptor, &surface);
        surface.resize(80, 32).expect("resize");

        assert!(matches!(
            resolve_data_plane_surface(descriptor),
            Err(DataPlaneResolveError::DescriptorMismatch)
        ));

        revoke_data_plane_surface(descriptor.client_id, descriptor.surface_id);
    }

    #[test]
    fn registry_revoke_makes_surface_unavailable() {
        let descriptor = SharedSurfaceDescriptor {
            client_id: 21,
            surface_id: 5,
            width: 16,
            height: 16,
            pixel_stride: 16,
            generation: 1,
        };
        let surface = SharedSurfaceMemory::new(16, 16).expect("surface alloc");
        publish_data_plane_surface(descriptor, &surface);
        revoke_data_plane_surface(descriptor.client_id, descriptor.surface_id);

        assert!(matches!(
            resolve_data_plane_surface(descriptor),
            Err(DataPlaneResolveError::SurfaceUnavailable)
        ));
    }
}
