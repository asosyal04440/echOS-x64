//! # echOS Heap Allocator Tipi
//! 
//! echOS için global allocator seçimi.
//! Şimdilik TLSF kullanılıyor ancak LockedHeap veya BumpAllocator'a geçilebilir.

// İleride allocator değiştirmek istersek burayı kullanabiliriz.
// Şimdilik sadece mod.rs üzerinden yönetiliyor.
