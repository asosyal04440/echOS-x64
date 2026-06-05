/*
 * echOS Ring 3 Shell — User-Mode Linker Script
 *
 * Bu script, echshell binary'sinin user-mode ELF olarak
 * derlenmesini sağlar. Kernel'in aksine sabit bir adrese
 * yüklenir (0x400000 - Linux/ELF convention).
 *
 * Bellek Düzeni:
 *   0x400000 (4MB) ──────────────────────────────
 *   _start          │  .text         │  Shell kodu
 *                   ├────────────────┤
 *                   │  .rodata       │  Sabit veri
 *                   ├────────────────┤
 *                   │  .data         │  Başlatılmış değişkenler
 *                   ├────────────────┤
 *                   │  .bss          │  Başlatılmamış değişkenler
 *   _end            ──────────────────────────────
 */

ENTRY(_start)

SECTIONS
{
    . = 0x400000;

    _start_offset = .;

    .text : {
        *(.text .text.*)
    }

    . = ALIGN(0x1000);

    .rodata : {
        *(.rodata .rodata.*)
    }

    . = ALIGN(0x1000);

    .got : {
        *(.data.rel.ro .data.rel.ro.*)
        *(.got .got.*)
        *(.got.plt .got.plt.*)
    }

    . = ALIGN(0x1000);

    .data : {
        *(.data .data.*)
    }

    . = ALIGN(0x1000);

    .bss (NOLOAD) : {
        *(.bss .bss.*)
        *(COMMON)
    }

    _end = .;
}
