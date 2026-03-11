//! Shared zero-copy surface memory with damage generation tracking.

use crate::gui::protocol::Rect;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

#[derive(Clone)]
pub struct SurfaceSnapshot {
    pub width: u32,
    pub height: u32,
    pub generation: u64,
    pub pixels: Vec<u32>,
}

#[derive(Debug)]
pub struct SharedSurfaceMemory {
    width: AtomicU32,
    height: AtomicU32,
    generation: AtomicU64,
    damage: Mutex<Vec<Rect>>,
    pixels: Mutex<Vec<u32>>,
}

impl SharedSurfaceMemory {
    pub fn new(width: u32, height: u32) -> Arc<Self> {
        let len = (width as usize).saturating_mul(height as usize);
        Arc::new(Self {
            width: AtomicU32::new(width),
            height: AtomicU32::new(height),
            generation: AtomicU64::new(1),
            damage: Mutex::new(vec![Rect::new(0, 0, width, height)]),
            pixels: Mutex::new(vec![0; len]),
        })
    }

    pub fn resize(&self, width: u32, height: u32) {
        self.width.store(width, Ordering::Release);
        self.height.store(height, Ordering::Release);
        let len = (width as usize).saturating_mul(height as usize);
        self.pixels.lock().resize(len, 0);
        self.submit_damage(Rect::new(0, 0, width, height));
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

    pub fn snapshot(&self) -> SurfaceSnapshot {
        SurfaceSnapshot {
            width: self.width.load(Ordering::Acquire),
            height: self.height.load(Ordering::Acquire),
            generation: self.generation.load(Ordering::Acquire),
            pixels: self.pixels.lock().clone(),
        }
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
