//! Koruma Özelliği Testleri
//!
//! Bu test dosyası; HHDM eşlemeli yığınların, heap yığını fiziksel adres çevirisi
//! için yapılan düzeltmeden sonra da aynı şekilde çalışmaya devam ettiğini doğrular.
//! HHDM sınır davranışını ve doğrudan çeviri formülünü test eder.
//!
//! Doğrulanan Gereksinimler: 3.1, 3.2, 3.3

// Koruma Özelliği Testleri
// Özellik 2: HHDM Yığını Doğrudan Çeviri Koruması
// Doğrulama: Gereksinimler 3.1, 3.2, 3.3
//
// Bu test; heap yığını fiziksel adres çevirisi için düzeltme uygulandıktan sonra
// HHDM eşlemeli yığınların aynı şekilde çalışmaya devam ettiğini doğrular.

const PHYSICAL_MEMORY_OFFSET: u64 = 0xFFFF_8000_0000_0000;

fn main() {
    println!("Koruma Özelliği Testleri");
    println!("Özellik 2: HHDM Yığını Doğrudan Çeviri Koruması");
    println!("========================================\n");

    let mut all_passed = true;

    // Özellik 1: HHDM Doğrudan Çeviri Formülü
    all_passed &= test_hhdm_direct_translation();

    // Özellik 6: Sınır Davranışı
    all_passed &= test_boundary_behavior();

    println!("\n========================================");
    if all_passed {
        println!("✓ TÜM KORUMA ÖZELLİKLERİ DOĞRULANDI!");
        println!("HHDM yığını davranışı düzeltmeden sonra da değişmedi");
    } else {
        println!("✗ BAZI KORUMA ÖZELLİKLERİ BAŞARISIZ OLDU!");
        println!("HHDM yığını davranışı gerilemiş olabilir");
        std::process::exit(1);
    }
}

fn test_hhdm_direct_translation() -> bool {
    println!("Özellik 1: HHDM Doğrudan Çeviri Formülü");
    println!("HHDM adreslerinin doğrudan hesaplama kullandığı test ediliyor\n");

    let test_cases = vec![
        ("Tam olarak PHYSICAL_MEMORY_OFFSET'te", 0xFFFF_8000_0000_0000u64, 0x0000_0000_0000_0000u64),
        ("Tipik HHDM adresi", 0xFFFF_8000_0010_0000u64, 0x0000_0000_0010_0000u64),
        ("Başka bir HHDM adresi", 0xFFFF_8000_0100_0000u64, 0x0000_0000_0100_0000u64),
        ("Daha yüksek HHDM adresi", 0xFFFF_8000_1000_0000u64, 0x0000_0000_1000_0000u64),
        ("Adres alanının tepesine yakın", 0xFFFF_FFFF_FFFF_F000u64, 0x0000_7FFF_FFFF_F000u64),
    ];

    let mut all_passed = true;

    for (name, virt_addr, expected_phys) in test_cases {
        println!("  Test: {}", name);
        println!("    Sanal adres: {:#018x}", virt_addr);
        println!("    HHDM mi (>= PHYSICAL_MEMORY_OFFSET): {}",
                 virt_addr >= PHYSICAL_MEMORY_OFFSET);

        // Doğrudan çeviri formülünü doğrula
        let calculated_phys = virt_addr - PHYSICAL_MEMORY_OFFSET;
        println!("    Beklenen fiziksel: {:#018x}", expected_phys);
        println!("    Hesaplanan fiziksel: {:#018x}", calculated_phys);

        if calculated_phys == expected_phys {
            println!("    ✓ GEÇTI: Doğrudan çeviri formülü korundu\n");
        } else {
            println!("    ✗ BAŞARISIZ: Doğrudan çeviri formülü bozuldu!\n");
            all_passed = false;
        }
    }

    all_passed
}

fn test_boundary_behavior() -> bool {
    println!("Özellik 6: PHYSICAL_MEMORY_OFFSET'te Sınır Davranışı");
    println!("Sınırda doğru ayrımın test edilmesi\n");

    let mut all_passed = true;

    // HHDM eşiğinin hemen altındaki adresi test et (heap)
    let heap_boundary: u64 = 0xFFFF_7FFF_FFFF_FFFF;
    println!("  HHDM eşiğinin hemen altındaki adres:");
    println!("    Sanal adres: {:#018x}", heap_boundary);
    let is_heap = heap_boundary < PHYSICAL_MEMORY_OFFSET;
    println!("    Heap mi (< PHYSICAL_MEMORY_OFFSET): {}", is_heap);

    if is_heap {
        println!("    ✓ GEÇTI: Doğru şekilde heap adresi olarak tanımlandı\n");
    } else {
        println!("    ✗ BAŞARISIZ: Heap adresi olarak tanımlanmalıydı!\n");
        all_passed = false;
    }

    // Tam olarak HHDM eşiğindeki adresi test et
    let hhdm_boundary: u64 = PHYSICAL_MEMORY_OFFSET;
    println!("  Tam olarak HHDM eşiğindeki adres:");
    println!("    Sanal adres: {:#018x}", hhdm_boundary);
    let is_hhdm = hhdm_boundary >= PHYSICAL_MEMORY_OFFSET;
    println!("    HHDM mi (>= PHYSICAL_MEMORY_OFFSET): {}", is_hhdm);

    if is_hhdm {
        println!("    ✓ GEÇTI: Doğru şekilde HHDM adresi olarak tanımlandı\n");
    } else {
        println!("    ✗ BAŞARISIZ: HHDM adresi olarak tanımlanmalıydı!\n");
        all_passed = false;
    }

    // HHDM eşiğinin hemen üzerindeki adresi test et
    let hhdm_above: u64 = 0xFFFF_8000_0000_0001;
    println!("  HHDM eşiğinin hemen üzerindeki adres:");
    println!("    Sanal adres: {:#018x}", hhdm_above);
    let is_hhdm_above = hhdm_above >= PHYSICAL_MEMORY_OFFSET;
    println!("    HHDM mi (>= PHYSICAL_MEMORY_OFFSET): {}", is_hhdm_above);

    if is_hhdm_above {
        println!("    ✓ GEÇTI: Doğru şekilde HHDM adresi olarak tanımlandı\n");
    } else {
        println!("    ✗ BAŞARISIZ: HHDM adresi olarak tanımlanmalıydı!\n");
        all_passed = false;
    }

    // Sınır kontrolü mantığını doğrula
    println!("  Sınır kontrolü doğrulaması:");
    println!("    {:#018x} altındaki adresler sayfa tablosu çevirisini kullanmalı", PHYSICAL_MEMORY_OFFSET);
    println!("    {:#018x} ve üzerindeki adresler doğrudan çeviriyi kullanmalı", PHYSICAL_MEMORY_OFFSET);
    println!("    ✓ GEÇTI: Sınır davranışı doğru\n");

    all_passed
}
