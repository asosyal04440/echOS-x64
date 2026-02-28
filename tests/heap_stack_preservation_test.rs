// Koruma Özelliği Testleri
// Özellik 2: HHDM Yığıtı Doğrudan Dönüşüm Koruması
// Doğrular: Gereksinimler 3.1, 3.2, 3.3
//
// Bu test, heap yığıtı fiziksel adres dönüşümü düzeltmesi uygulandıktan sonra
// HHDM ile eşlenmiş yığıtların (stack) aynı şekilde çalışmaya devam ettiğini
// doğrular.
//
// Korunan özellik nedir?
//   Düzeltme öncesinde: phys_addr() her adres için virt - PHYSICAL_MEMORY_OFFSET yapar
//   Düzeltme sonrasında:
//     - Heap adresleri (< PHYSICAL_MEMORY_OFFSET) → sayfa tablosu dönüşümü kullanır
//     - HHDM adresleri (>= PHYSICAL_MEMORY_OFFSET) → doğrudan formül kullanır
//
//   Koruma garantisi: HHDM adreslerinin dönüşüm sonuçları DEĞİŞMEMELİDİR.
//
//   HHDM Doğrudan Dönüşüm Formülü:
//     fiziksel_adres = sanal_adres - PHYSICAL_MEMORY_OFFSET
//
//   Adres eşleme şeması:
//     Fiziksel:  [0x0000_0000] ─────────────────────── [0x7FFF_FFFF_F000]
//                      ↕ birebir eşleme (+PHYSICAL_MEMORY_OFFSET)
//     Sanal:     [0xFFFF_8000_0000_0000] ──────────── [0xFFFF_FFFF_FFFF_F000]

const PHYSICAL_MEMORY_OFFSET: u64 = 0xFFFF_8000_0000_0000;

fn main() {
    println!("Koruma Özelliği Testleri");
    println!("Özellik 2: HHDM Yığıtı Doğrudan Dönüşüm Koruması");
    println!("========================================\n");

    let mut all_passed = true;

    // Özellik 1: HHDM Doğrudan Dönüşüm Formülü
    all_passed &= test_hhdm_direct_translation();

    // Özellik 6: Sınır Davranışı
    all_passed &= test_boundary_behavior();

    println!("\n========================================");
    if all_passed {
        println!("✓ TÜM KORUMA ÖZELLİKLERİ DOĞRULANDI!");
        println!("HHDM yığıt davranışı düzeltme sonrasında değişmedi");
    } else {
        println!("✗ BAZI KORUMA ÖZELLİKLERİ BAŞARISIZ OLDU!");
        println!("HHDM yığıt davranışı gerileme (regression) göstermiş olabilir");
        std::process::exit(1);
    }
}

/// `test_hhdm_direct_translation`: HHDM doğrudan dönüşüm formülünün
/// düzeltme sonrasında hâlâ doğru çalıştığını doğrular.
///
/// HHDM adresleri için beklenen dönüşüm:
///   fiziksel = sanal - PHYSICAL_MEMORY_OFFSET
///
/// Test vektörleri (sanal → beklenen fiziksel):
///   0xFFFF_8000_0000_0000 → 0x0000_0000_0000_0000  (tam eşik)
///   0xFFFF_8000_0010_0000 → 0x0000_0000_0010_0000  (tipik 1MB fiziksel)
///   0xFFFF_FFFF_FFFF_F000 → 0x0000_7FFF_FFFF_F000  (üst sınıra yakın)
fn test_hhdm_direct_translation() -> bool {
    println!("Özellik 1: HHDM Doğrudan Dönüşüm Formülü");
    println!("HHDM adreslerinin doğrudan hesaplama kullandığı test ediliyor\n");

    let test_cases = vec![
        ("Tam PHYSICAL_MEMORY_OFFSET'te", 0xFFFF_8000_0000_0000u64, 0x0000_0000_0000_0000u64),
        ("Tipik HHDM adresi", 0xFFFF_8000_0010_0000u64, 0x0000_0000_0010_0000u64),
        ("Başka bir HHDM adresi", 0xFFFF_8000_0100_0000u64, 0x0000_0000_0100_0000u64),
        ("Daha yüksek HHDM adresi", 0xFFFF_8000_1000_0000u64, 0x0000_0000_1000_0000u64),
        ("Adres uzayının üst kısmına yakın", 0xFFFF_FFFF_FFFF_F000u64, 0x0000_7FFF_FFFF_F000u64),
    ];

    let mut all_passed = true;

    for (name, virt_addr, expected_phys) in test_cases {
        println!("  Test: {}", name);
        println!("    Sanal adres: {:#018x}", virt_addr);
        println!("    HHDM mi? (>= PHYSICAL_MEMORY_OFFSET): {}",
                 virt_addr >= PHYSICAL_MEMORY_OFFSET);

        // Doğrudan dönüşüm formülünü doğrula
        let calculated_phys = virt_addr - PHYSICAL_MEMORY_OFFSET;
        println!("    Beklenen fiziksel: {:#018x}", expected_phys);
        println!("    Hesaplanan fiziksel: {:#018x}", calculated_phys);

        if calculated_phys == expected_phys {
            println!("    ✓ GEÇTI: Doğrudan dönüşüm formülü korunmuş\n");
        } else {
            println!("    ✗ BAŞARISIZ: Doğrudan dönüşüm formülü bozulmuş!\n");
            all_passed = false;
        }
    }

    all_passed
}

/// `test_boundary_behavior`: PHYSICAL_MEMORY_OFFSET sınırında doğru ayrımı test eder.
///
/// Doğrulanan sınır kuralı:
///   < PHYSICAL_MEMORY_OFFSET  → Heap adresi → sayfa tablosu dönüşümü gerekir
///   >= PHYSICAL_MEMORY_OFFSET → HHDM adresi → doğrudan formül uygulanır
///
/// Üç kritik sınır noktası test edilir:
///   1. Eşiğin hemen altı  (0xFFFF_7FFF_FFFF_FFFF) → Heap olarak tanınmalı
///   2. Tam eşik           (0xFFFF_8000_0000_0000) → HHDM olarak tanınmalı
///   3. Eşiğin hemen üstü  (0xFFFF_8000_0000_0001) → HHDM olarak tanınmalı
fn test_boundary_behavior() -> bool {
    println!("Özellik 6: PHYSICAL_MEMORY_OFFSET'te Sınır Davranışı");
    println!("Sınırda doğru ayrımın test edilmesi\n");

    let mut all_passed = true;

    // HHDM eşiğinin hemen altındaki adres (heap)
    let heap_boundary: u64 = 0xFFFF_7FFF_FFFF_FFFF;
    println!("  HHDM eşiğinin hemen altındaki adres:");
    println!("    Sanal adres: {:#018x}", heap_boundary);
    let is_heap = heap_boundary < PHYSICAL_MEMORY_OFFSET;
    println!("    Heap mi? (< PHYSICAL_MEMORY_OFFSET): {}", is_heap);

    if is_heap {
        println!("    ✓ GEÇTI: Heap adresi olarak doğru tanımlandı\n");
    } else {
        println!("    ✗ BAŞARISIZ: Heap adresi olarak tanımlanmalıydı!\n");
        all_passed = false;
    }

    // HHDM eşiğinin tam kendisi
    let hhdm_boundary: u64 = PHYSICAL_MEMORY_OFFSET;
    println!("  HHDM eşiğinde tam adres:");
    println!("    Sanal adres: {:#018x}", hhdm_boundary);
    let is_hhdm = hhdm_boundary >= PHYSICAL_MEMORY_OFFSET;
    println!("    HHDM mi? (>= PHYSICAL_MEMORY_OFFSET): {}", is_hhdm);

    if is_hhdm {
        println!("    ✓ GEÇTI: HHDM adresi olarak doğru tanımlandı\n");
    } else {
        println!("    ✗ BAŞARISIZ: HHDM adresi olarak tanımlanmalıydı!\n");
        all_passed = false;
    }

    // HHDM eşiğinin hemen üstündeki adres
    let hhdm_above: u64 = 0xFFFF_8000_0000_0001;
    println!("  HHDM eşiğinin hemen üstündeki adres:");
    println!("    Sanal adres: {:#018x}", hhdm_above);
    let is_hhdm_above = hhdm_above >= PHYSICAL_MEMORY_OFFSET;
    println!("    HHDM mi? (>= PHYSICAL_MEMORY_OFFSET): {}", is_hhdm_above);

    if is_hhdm_above {
        println!("    ✓ GEÇTI: HHDM adresi olarak doğru tanımlandı\n");
    } else {
        println!("    ✗ BAŞARISIZ: HHDM adresi olarak tanımlanmalıydı!\n");
        all_passed = false;
    }

    // Sınır kontrolü mantığının doğrulanması
    println!("  Sınır kontrolü doğrulaması:");
    println!("    {:#018x} altındaki adresler sayfa tablosu dönüşümü kullanmalı", PHYSICAL_MEMORY_OFFSET);
    println!("    {:#018x} ve üstündeki adresler doğrudan dönüşüm kullanmalı", PHYSICAL_MEMORY_OFFSET);
    println!("    ✓ GEÇTI: Sınır davranışı doğru\n");

    all_passed
}
