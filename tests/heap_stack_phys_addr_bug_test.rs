// Hata Durumu Keşif Testi
// Özellik 1: Heap Yığını Fiziksel Adres Dönüşümü
// Doğrular: Gereksinimler 2.1, 2.2, 2.3
//
// Bu test, phys_addr() fonksiyonu heap'te tahsis edilmiş adreslere
// çağrıldığında oluşan hata durumunu simüle ederek gösterir.
//
// Sorunun özeti:
//   HHDM (Higher Half Direct Map): Fiziksel belleğin yüksek sanal adres
//   uzayında birebir eşlenmiş kopyasıdır. HHDM adresleri PHYSICAL_MEMORY_OFFSET
//   değerinden büyük veya eşittir.
//
//   Heap adresleri ise çok daha düşük sanal adreslerde bulunur. Eğer
//   phys_addr() her adresi koşulsuz olarak PHYSICAL_MEMORY_OFFSET çıkararak
//   fiziksel adres hesaplamaya çalışırsa, heap adreslerinde tamsayı taşması
//   (integer underflow) meydana gelir.
//
//   Adres Uzayı Haritası:
//
//   0x0000_0000_0000_0000 ──────────────────────────────────────
//                          │  Heap adresleri (düşük yarı)       │
//                          │  Örn: 0x0000_4444_4447_8A90        │
//   0xFFFF_7FFF_FFFF_FFFF ──────────────────────────────────────
//                          │  (kanonik olmayan boşluk)          │
//   0xFFFF_8000_0000_0000 ──────────────────────────────────────
//   PHYSICAL_MEMORY_OFFSET │  HHDM adresleri (yüksek yarı)     │
//                          │  virt = phys + PHYSICAL_MEMORY_OFFSET
//   0xFFFF_FFFF_FFFF_FFFF ──────────────────────────────────────

const PHYSICAL_MEMORY_OFFSET: u64 = 0xFFFF_8000_0000_0000;

fn main() {
    println!("Hata Durumu Keşif Testi");
    println!("Özellik 1: Heap Yığını Fiziksel Adres Dönüşümü");
    println!("========================================\n");

    // Test Durumu 1: Tipik heap adresi (gerçek hatada gözlemlendi)
    println!("Test Durumu 1: Tipik heap adresi");
    let typical_heap_addr: u64 = 0x0000_4444_4447_8A90;
    println!("  Sanal adres: {:#018x}", typical_heap_addr);
    println!("  Heap adresi mi? (< PHYSICAL_MEMORY_OFFSET): {}",
             typical_heap_addr < PHYSICAL_MEMORY_OFFSET);

    // Düzeltilmemiş phys_addr() fonksiyonunun yaptığı işlemi simüle eder
    match typical_heap_addr.checked_sub(PHYSICAL_MEMORY_OFFSET) {
        Some(phys) => {
            println!("  HATA: Çıkarma işlemi beklenmedik şekilde başarılı oldu: {:#018x}", phys);
            println!("  Bu, hatanın mevcut olmadığını veya testin yanlış olduğunu gösterir\n");
        }
        None => {
            println!("  HATA DOĞRULANDI: Tamsayı taşması (underflow) tespit edildi!");
            println!("  PHYSICAL_MEMORY_OFFSET'i heap adresinden çıkarmaya çalışmak");
            println!("  hata ayıklama modunda panik'e neden olurdu\n");
        }
    }

    // Test Durumu 2: Düşük adres
    println!("Test Durumu 2: Düşük yığıt adresi");
    let low_addr: u64 = 0x0000_0000_1000_0000;
    println!("  Sanal adres: {:#018x}", low_addr);
    println!("  Heap adresi mi? (< PHYSICAL_MEMORY_OFFSET): {}",
             low_addr < PHYSICAL_MEMORY_OFFSET);

    match low_addr.checked_sub(PHYSICAL_MEMORY_OFFSET) {
        Some(phys) => {
            println!("  HATA: Çıkarma işlemi beklenmedik şekilde başarılı oldu: {:#018x}", phys);
        }
        None => {
            println!("  HATA DOĞRULANDI: Tamsayı taşması (underflow) tespit edildi!\n");
        }
    }

    // Test Durumu 3: Sınır adresi (HHDM eşiğinin hemen altı)
    println!("Test Durumu 3: Sınır adresi (HHDM eşiğinin hemen altında)");
    let boundary_addr: u64 = 0xFFFF_7FFF_FFFF_FFFF;
    println!("  Sanal adres: {:#018x}", boundary_addr);
    println!("  Heap adresi mi? (< PHYSICAL_MEMORY_OFFSET): {}",
             boundary_addr < PHYSICAL_MEMORY_OFFSET);

    match boundary_addr.checked_sub(PHYSICAL_MEMORY_OFFSET) {
        Some(phys) => {
            println!("  HATA: Çıkarma işlemi beklenmedik şekilde başarılı oldu: {:#018x}", phys);
        }
        None => {
            println!("  HATA DOĞRULANDI: Tamsayı taşması (underflow) tespit edildi!\n");
        }
    }

    // Test Durumu 4: HHDM adresi (kontrol - doğru çalışmalıdır)
    println!("Test Durumu 4: HHDM adresi (kontrol - doğru çalışmalı)");
    let hhdm_addr: u64 = 0xFFFF_8000_0010_0000;
    println!("  Sanal adres: {:#018x}", hhdm_addr);
    println!("  HHDM adresi mi? (>= PHYSICAL_MEMORY_OFFSET): {}",
             hhdm_addr >= PHYSICAL_MEMORY_OFFSET);

    match hhdm_addr.checked_sub(PHYSICAL_MEMORY_OFFSET) {
        Some(phys) => {
            println!("  BAŞARILI: Fiziksel adres hesaplandı: {:#018x}", phys);
            println!("  HHDM adresleri için beklenen davranış bu\n");
        }
        None => {
            println!("  HATA: HHDM adresi için beklenmedik taşma!\n");
        }
    }

    println!("========================================");
    println!("Kök Neden Analizi:");
    println!("  Düzeltilmemiş KernelStack::phys_addr() metodu koşulsuz olarak");
    println!("  şu işlemi yapar: virt_addr - PHYSICAL_MEMORY_OFFSET");
    println!("  Bu, heap adreslerinde tamsayı taşmasına neden olur çünkü");
    println!("  virt_addr < PHYSICAL_MEMORY_OFFSET koşulu gerçekleşir");
    println!("\nKarşı örnekler:");
    println!("  - Heap adresleri (< {:#018x}) taşmaya neden olur", PHYSICAL_MEMORY_OFFSET);
    println!("  - Düşük adresler başarısız olur");
    println!("  - HHDM eşiğinin hemen altındaki sınır adresleri başarısız olur");
    println!("  - Yalnızca HHDM adresleri (>= {:#018x}) doğru çalışır", PHYSICAL_MEMORY_OFFSET);
}
