//! # echOS Heap Allocator Tipi
//!
//! echOS için global heap allocator seçimini yönetir.
//! Şimdilik TLSF (Two-Level Segregated Fit) kullanılıyor.
//!
//! ## Allocator Karşılaştırması
//!
//! ```
//! ┌─────────────────┬──────────┬────────────┬───────────────────────────┐
//! │ Allocator       │ Hız      │ Bellek Kul.│ Kullanım Amacı            │
//! ├─────────────────┼──────────┼────────────┼───────────────────────────┤
//! │ BumpAllocator   │ En hızlı │ Verimli    │ Başlatma (deallocate yok) │
//! │ LockedHeap      │ Orta     │ Orta       │ Basit çekirdekler         │
//! │ TLSF            │ Hızlı    │ Çok verimli│ Genel amaçlı çekirdek     │
//! └─────────────────┴──────────┴────────────┴───────────────────────────┘
//! ```
//!
//! ## TLSF Nedir?
//! Two-Level Segregated Fit — serbest blokları boyutlarına göre iki
//! seviyeli bir tabloya yerleştirir. O(1) zamanda tahsis ve serbest
//! bırakma sağlar; gerçek zamanlı sistemler için uygundur.

// İleride allocator değiştirmek istersek burası merkezi kontrol noktasıdır.
// Yeni bir allocator eklemek için:
//   1. Cargo.toml'a bağımlılığı ekle
//   2. global_allocator! makrosunu güncelle
//   3. Heap başlatma adresini/boyutunu ayarla
// Şimdilik tüm yapılandırma mod.rs üzerinden yönetiliyor.
