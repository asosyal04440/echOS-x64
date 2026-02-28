/*
 * echOS - Bağlayıcı (Linker) Betiği
 *
 * Bu dosya, GNU ld veya lld bağlayıcısına çekirdek ikili dosyasının
 * bellekte nasıl düzenleneceğini söyler. İşletim sistemi çekirdeği için
 * kritik öneme sahiptir çünkü:
 *   - Hangi adrese yükleneceğini belirler
 *   - Kod, veri ve BSS bölümlerinin sırasını tanımlar
 *   - Çekirdek başlangıç/bitiş adreslerini semboller olarak dışa aktarır
 *
 * Bellek Düzeni Özeti:
 *
 *   0x100000 (1MB) ──────────────────────────────────────
 *   kernel_start     │  .boot.multiboot2  │  Multiboot2 başlığı
 *                    ├────────────────────┤
 *                    │  .boot.text        │  32-bit boot kodu
 *                    ├────────────────────┤
 *                    │  .boot.rodata      │  Boot salt-okunur verisi
 *                    ├────────────────────┤
 *                    │  .boot.data        │  Boot yazılabilir verisi
 *   boot_lma_end     ├────────────────────┤
 *                    │  .boot.bss (NOLOAD)│  Boot sıfırlanmış veri alanı
 *                    ├────────────────────┤
 *                    │  .text             │  64-bit çekirdek kodu
 *                    ├────────────────────┤
 *                    │  .rodata           │  Salt-okunur sabitler/dizeler
 *                    ├────────────────────┤
 *                    │  .data             │  Başlatılmış global değişkenler
 *                    ├────────────────────┤
 *                    │  .bss (NOLOAD)     │  Başlatılmamış global değişkenler
 *   kernel_end       ──────────────────────────────────────
 */

/*
 * ENTRY: Çekirdeğin giriş noktasını belirtir.
 * Önyükleyici çekirdeği yüklediğinde kontrolü `_start` sembolüne iletir.
 * Bu, çekirdek main() fonksiyonundan önce çalışan assembly kodudur.
 */
ENTRY(_start)

SECTIONS
{
    /*
     * . = 0x100000: Konum sayacını 1MB adresine ayarla.
     * Neden 1MB? x86 gerçek mod (real mode) adresleme alanı <1MB'dir.
     * 1MB'nin altı BIOS, VGA tampon belleği ve diğer donanım için ayrılmıştır.
     * Çekirdek 1MB'den itibaren güvenle yerleşebilir.
     */
    . = 0x100000;

    /*
     * kernel_start: Çekirdeğin bellekteki başlangıç adresini işaret eden sembol.
     * Rust kodu bu sembolü `extern "C" { static kernel_start: u8; }` şeklinde
     * okuyabilir; bu sayede çekirdek yükleme adresi çalışma zamanında bilinir.
     */
    kernel_start = .;

    /*
     * .boot.multiboot2: Multiboot2 başlık bölümü.
     * Multiboot2 uyumlu önyükleyiciler (GRUB vb.), çekirdeği yüklemeden önce
     * bu başlığı arar. Başlığın dosyanın başında (8KB içinde) bulunması zorunludur.
     * Bölümün adı `boot` prefix'i taşır çünkü bu kod 32-bit protected mode'da çalışır.
     */
    .boot.multiboot2 : {
        *(.boot.multiboot2)
    }

    /*
     * .boot.text: 32-bit önyükleme kodu bölümü.
     * Önyükleyici çekirdeği 32-bit protected mode'da başlatır.
     * Bu bölüm, 64-bit long mode'a geçiş yapan assembly kodunu içerir:
     *   1. GDT (Global Descriptor Table) kurulumu
     *   2. Sayfalama (paging) etkinleştirme
     *   3. EFER.LME biti ayarlama
     *   4. .text bölümündeki 64-bit _start'a atlama
     */
    .boot.text : {
        *(.boot.text)
    }

    /*
     * .boot.rodata: Boot aşamasına ait salt-okunur veri.
     * Önyükleme kodunun kullandığı sabit dizeler, tablo verileri vb.
     * Yazma korumalıdır; yanlışlıkla üzerine yazılması engellenir.
     */
    .boot.rodata : {
        *(.boot.rodata)
    }

    /*
     * .boot.data: Boot aşamasına ait yazılabilir veri.
     * Önyükleme kodunun değiştirebildiği global değişkenleri içerir.
     * Örneğin: geçici sayfa tabloları, stack pointer'lar.
     */
    .boot.data : {
        *(.boot.data)
    }

    /*
     * boot_lma_end: Yükleme adresi sonu (Load Memory Address).
     * Bu sembol, boot bölümlerinin disk üzerindeki son adresini işaret eder.
     * BSS bölümü dosyada yer kaplamaz (NOLOAD), bu nedenle ayrıca işaretlenir.
     */
    boot_lma_end = .;

    /*
     * .boot.bss (NOLOAD): Boot aşamasına ait başlatılmamış veri.
     * NOLOAD direktifi: Bu bölüm DOSYAYA YAZILMAz; önyükleyici yüklerken
     * bu bellek alanını sıfırlar (zero-fill). Disk boyutunu küçültür.
     * Örneğin: boot stack alanı, geçici tampon bellek.
     */
    .boot.bss (NOLOAD) : {
        *(.boot.bss)
    }

    /*
     * .text: 64-bit çekirdek kodu bölümü.
     * Çekirdeğin tüm Rust ve assembly fonksiyonları burada yer alır.
     * `.text.*` joker ifadesi, Rust'ın LTO (Link Time Optimization) için
     * oluşturduğu tüm alt bölümleri de kapsar (örn: .text.mymodule::myfn).
     */
    .text : {
        *(.text .text.*)
    }

    /*
     * .rodata: Salt-okunur veri bölümü.
     * Rust'ta `const`, `static` (değişmez) ve string literalleri içerir.
     * Donanım tarafından yazma korumalı sayfa olarak işaretlenebilir,
     * bu sayede yanlışlıkla üzerine yazma önlenir.
     */
    .rodata : {
        *(.rodata .rodata.*)
    }

    /*
     * .data: Başlatılmış değişken veri bölümü.
     * `static mut` gibi başlangıç değeri olan Rust global değişkenleri burada saklanır.
     * Bu bölüm hem disk görüntüsünde hem de bellekte yer kaplar.
     */
    .data : {
        *(.data .data.*)
    }

    /*
     * .bss (NOLOAD): Başlatılmamış değişken veri bölümü.
     * Rust'ta başlangıç değeri verilmemiş `static mut` değişkenler buraya düşer.
     * NOLOAD: Disk görüntüsünde yer kaplamaz; önyükleyici/çekirdek başlangıcında
     * bu alan sıfırlanır (bellek eşleme). Büyük tampon bellekler için idealdir.
     */
    .bss (NOLOAD) : {
        *(.bss .bss.*)
    }

    /*
     * kernel_end: Çekirdeğin bellekteki bitiş adresini işaret eden sembol.
     * Çekirdek belleğini korumak veya dinamik bellek yöneticisini
     * kernel_end'den itibaren başlatmak için kullanılır:
     *   heap_start = kernel_end (sayfa hizalamasına yuvarlanarak)
     */
    kernel_end = .;
}
