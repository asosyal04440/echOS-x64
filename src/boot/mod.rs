//! # echOS Boot Bilgisi
//! 
//! UEFI boot sürecinden kernel'e aktarılan bilgiler.
//! Bellek haritası ve fiziksel offset içerir.

use uefi::boot::MemoryDescriptor;
use alloc::vec::Vec;

/// UEFI'den kernel'e aktarılan boot bilgileri
pub struct BootInfo {
    /// UEFI bellek haritası (tüm kullanılabilir/ayrılmış bölgeler)
    pub memory_map: Vec<MemoryDescriptor>,
    /// Fiziksel belleğin sanal adres offset'i (genellikle 0)
    pub physical_memory_offset: u64,
}

/// Bellek haritasının toplam boyutunu hesaplar (bytes)
pub fn get_memory_map_size(map: &[MemoryDescriptor]) -> usize {
    map.len() * core::mem::size_of::<MemoryDescriptor>()
}
