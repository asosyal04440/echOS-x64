use core::sync::atomic::{AtomicU64, Ordering};
use x86_64::structures::paging::{PhysFrame, Size4KiB};
use x86_64::PhysAddr;

use super::frame_ownership::{self};
use super::migration::migrate_page_alloc;
use super::rmap;
use super::{allocate_contiguous_frames, PAGE_SIZE};

const MAX_MIGRATE_PER_COMPACT: usize = 256;
const PAGEBLOCK_PAGES: usize = 512;

static COMPACT_SCAN_CURSOR: AtomicU64 = AtomicU64::new(0);

fn is_frame_allocated(phys: u64) -> bool {
    frame_ownership::frame_flags(phys).bits() != 0
        || frame_ownership::frame_refcount(phys) > 0
}

fn is_migratable(phys: u64) -> bool {
    let entries = rmap::rmap_lookup(phys);
    if entries.is_empty() {
        return false;
    }
    let refcount = frame_ownership::frame_refcount(phys);
    if refcount == 0 && frame_ownership::frame_flags(phys).bits() == 0 {
        return false;
    }
    for entry in &entries {
        if entry.pml4 == 0 || entry.space_id == 0 {
            return false;
        }
    }
    true
}

pub fn compact_contiguous(target_pages: usize) -> Option<PhysFrame<Size4KiB>> {
    let total_frames = super::global_memory_manager()
        .map(|m| m.total_frames())
        .unwrap_or(0);
    if total_frames == 0 || target_pages == 0 || target_pages > 512 {
        return None;
    }

    let total_blocks = total_frames / PAGEBLOCK_PAGES;
    if total_blocks == 0 {
        return None;
    }

    let start_block = (COMPACT_SCAN_CURSOR.load(Ordering::Relaxed) as usize % total_blocks) as u64;
    let mut migrated = 0usize;

    for block_offset in 0..total_blocks.min(128) {
        let block_idx = (start_block as usize + block_offset) % total_blocks;
        let block_pfn = block_idx * PAGEBLOCK_PAGES;
        let block_phys = (block_pfn as u64) * PAGE_SIZE as u64;

        let mut block_migrated = 0usize;
        for page_in_block in 0..PAGEBLOCK_PAGES {
            if migrated >= MAX_MIGRATE_PER_COMPACT {
                break;
            }

            let pfn = block_pfn + page_in_block;
            if pfn >= total_frames {
                break;
            }
            let phys = (pfn as u64) * PAGE_SIZE as u64;

            if !is_frame_allocated(phys) || !is_migratable(phys) {
                continue;
            }

            if migrate_page_alloc(phys).is_some() {
                migrated += 1;
                block_migrated += 1;
            }
        }

        if block_migrated > 0 || migrated >= target_pages {
            if migrated >= target_pages {
                if let Some(frame) = allocate_contiguous_frames(target_pages) {
                    COMPACT_SCAN_CURSOR.store(
                        ((block_idx + 1) * PAGEBLOCK_PAGES) as u64,
                        Ordering::Relaxed,
                    );
                    crate::serial_println!(
                        "[COMPACT] success: migrated {} pages, target {} @ {:#x}",
                        migrated,
                        target_pages,
                        frame.start_address().as_u64()
                    );
                    return Some(frame);
                }
            }
        }
    }

    COMPACT_SCAN_CURSOR.store(
        ((start_block as usize + 128) * PAGEBLOCK_PAGES) as u64 % (total_frames as u64),
        Ordering::Relaxed,
    );

    if let Some(frame) = allocate_contiguous_frames(target_pages) {
        crate::serial_println!(
            "[COMPACT] success (post-scan): target {} @ {:#x}",
            target_pages,
            frame.start_address().as_u64()
        );
        return Some(frame);
    }

    None
}
