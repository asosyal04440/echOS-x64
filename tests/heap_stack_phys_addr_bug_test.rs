//! Hata Koşulu Araştırma Testi
//!
//! Bu test dosyası; heap üzerinde ayrılan adreslerde `phys_addr()` çağrıldığında
//! oluşan tamsayı taşması (integer underflow) hatasını simüle eder ve
//! HHDM eşiğinin altındaki adreslerin fiziksel adres hesaplamasında
//! neden başarısız olduğunu belgeler.
//!
//! Doğrulanan Gereksinimler: 2.1, 2.2, 2.3

// Hata Koşulu Araştırma Testi
// Özellik 1: Heap Yığını Fiziksel Adres Çevirisi
// Doğrulama: Gereksinimler 2.1, 2.2, 2.3
//
// Bu test; heap üzerinde ayrılmış adreslerde phys_addr() çağrıldığında
// oluşan hata koşulunu simüle ederek gösterir.

const PHYSICAL_MEMORY_OFFSET: u64 = 0xFFFF_8000_0000_0000;

fn main() {
    println!("Hata Koşulu Araştırma Testi");
    println!("Özellik 1: Heap Yığını Fiziksel Adres Çevirisi");
    println!("========================================\n");

    // Test Durumu 1: Tipik heap adresi (gerçek hatada gözlemlenen)
    println!("Test Durumu 1: Tipik heap adresi");
    let typical_heap_addr: u64 = 0x0000_4444_4447_8A90;
    println!("  Sanal adres: {:#018x}", typical_heap_addr);
    println!("  Heap adresi mi (< PHYSICAL_MEMORY_OFFSET): {}",
             typical_heap_addr < PHYSICAL_MEMORY_OFFSET);

    // Bu; düzeltilmemiş phys_addr() fonksiyonunun yaptığını simüle eder
    match typical_heap_addr.checked_sub(PHYSICAL_MEMORY_OFFSET) {
        Some(phys) => {
            println!("  HATA: Çıkarma beklenmedik şekilde başardı: {:#018x}", phys);
            println!("  Bu, hatanın mevcut olmadığını veya testin yanlış olduğunu gösterir\n");
        }
        None => {
            println!("  HATA ONAYLANDI: Tamsayı alt taşması tespit edildi!");
            println!("  Heap adresinden PHYSICAL_MEMORY_OFFSET çıkarmaya çalışılıyor");
            println!("  hata ayıklama modunda panik oluşturur\n");
        }
    }

    // Test Durumu 2: Düşük adres
    println!("Test Durumu 2: Düşük adres yığını");
    let low_addr: u64 = 0x0000_0000_1000_0000;
    println!("  Sanal adres: {:#018x}", low_addr);
    println!("  Heap adresi mi (< PHYSICAL_MEMORY_OFFSET): {}",
             low_addr < PHYSICAL_MEMORY_OFFSET);

    match low_addr.checked_sub(PHYSICAL_MEMORY_OFFSET) {
        Some(phys) => {
            println!("  HATA: Çıkarma beklenmedik şekilde başardı: {:#018x}", phys);
        }
        None => {
            println!("  HATA ONAYLANDI: Tamsayı alt taşması tespit edildi!\n");
        }
    }

    // Test Durumu 3: Sınır adresi (HHDM eşiğinin hemen altı)
    println!("Test Durumu 3: Sınır adresi (HHDM eşiğinin hemen altı)");
    let boundary_addr: u64 = 0xFFFF_7FFF_FFFF_FFFF;
    println!("  Sanal adres: {:#018x}", boundary_addr);
    println!("  Heap adresi mi (< PHYSICAL_MEMORY_OFFSET): {}",
             boundary_addr < PHYSICAL_MEMORY_OFFSET);

    match boundary_addr.checked_sub(PHYSICAL_MEMORY_OFFSET) {
        Some(phys) => {
            println!("  HATA: Çıkarma beklenmedik şekilde başardı: {:#018x}", phys);
        }
        None => {
            println!("  HATA ONAYLANDI: Tamsayı alt taşması tespit edildi!\n");
        }
    }

    // Test Durumu 4: HHDM adresi (doğru çalışması beklenen)
    println!("Test Durumu 4: HHDM adresi (kontrol - çalışması beklenir)");
    let hhdm_addr: u64 = 0xFFFF_8000_0010_0000;
    println!("  Sanal adres: {:#018x}", hhdm_addr);
    println!("  HHDM adresi mi (>= PHYSICAL_MEMORY_OFFSET): {}",
             hhdm_addr >= PHYSICAL_MEMORY_OFFSET);

    match hhdm_addr.checked_sub(PHYSICAL_MEMORY_OFFSET) {
        Some(phys) => {
            println!("  BAŞARILI: Fiziksel adres hesaplandı: {:#018x}", phys);
            println!("  HHDM adresleri için beklenen davranış bu\n");
        }
        None => {
            println!("  HATA: HHDM adresi için beklenmedik alt taşma!\n");
        }
    }

    println!("========================================");
    println!("Kök Neden Analizi:");
    println!("  Düzeltilmemiş KernelStack::phys_addr() metodu koşulsuz olarak");
    println!("  şunu uygular: virt_addr - PHYSICAL_MEMORY_OFFSET");
    println!("  Bu; virt_addr < PHYSICAL_MEMORY_OFFSET olan heap adreslerinde");
    println!("  tamsayı alt taşmasına neden olur");
    println!("\nBulunan karşı örnekler:");
    println!("  - Heap adresleri (< {:#018x}) alt taşmaya neden olur", PHYSICAL_MEMORY_OFFSET);
    println!("  - Düşük adresler başarısız olur");
    println!("  - HHDM eşiğinin hemen altındaki sınır adresleri başarısız olur");
    println!("  - Yalnızca HHDM adresleri (>= {:#018x}) doğru çalışır", PHYSICAL_MEMORY_OFFSET);
}
