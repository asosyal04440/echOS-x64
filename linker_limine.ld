/*
 * Native Limine image layout.
 *
 * The ordinary linker.ld is intentionally identity-linked at 0x100000 for
 * the Multiboot2/GRUB entry. Native Limine requires non-relocatable ELF
 * PT_LOAD virtual addresses in the higher half; this script keeps the
 * physical load image at the 1 MiB offset while assigning the image a
 * KERNEL_SPACE_START-based VMA.
 */

ENTRY(_start)

KERNEL_VMA = 0xffffffff80000000;
KERNEL_LMA = 0x00100000;
KERNEL_DELTA = KERNEL_VMA + KERNEL_LMA;

SECTIONS
{
    . = KERNEL_DELTA;
    kernel_start = .;

    .boot.multiboot2 : AT(KERNEL_LMA + (ADDR(.boot.multiboot2) - KERNEL_DELTA)) {
        KEEP(*(.boot.multiboot2))
    }

    . = ALIGN(0x1000);
    .boot.text : AT(KERNEL_LMA + (ADDR(.boot.text) - KERNEL_DELTA)) {
        *(.boot.text)
    }

    . = ALIGN(0x1000);
    .boot.rodata : AT(KERNEL_LMA + (ADDR(.boot.rodata) - KERNEL_DELTA)) {
        *(.boot.rodata)
    }

    . = ALIGN(0x1000);
    .got : AT(KERNEL_LMA + (ADDR(.got) - KERNEL_DELTA)) {
        *(.got .got.*)
    }

    . = ALIGN(0x1000);
    .boot.data : AT(KERNEL_LMA + (ADDR(.boot.data) - KERNEL_DELTA)) {
        *(.boot.data)
    }

    boot_lma_end = LOADADDR(.boot.data) + SIZEOF(.boot.data);

    .boot.bss (NOLOAD) : AT(KERNEL_LMA + (ADDR(.boot.bss) - KERNEL_DELTA)) {
        *(.boot.bss)
    }

    . = ALIGN(0x1000);
    .text : AT(KERNEL_LMA + (ADDR(.text) - KERNEL_DELTA)) {
        *(.text .text.*)
    }

    . = ALIGN(0x1000);
    .rodata : AT(KERNEL_LMA + (ADDR(.rodata) - KERNEL_DELTA)) {
        *(.rodata .rodata.*)
    }

    . = ALIGN(0x1000);
    .data : AT(KERNEL_LMA + (ADDR(.data) - KERNEL_DELTA)) {
        *(.data .data.*)
    }

    . = ALIGN(0x1000);
    .limine_req_start : AT(KERNEL_LMA + (ADDR(.limine_req_start) - KERNEL_DELTA)) {
        KEEP(*(.limine_req_start))
    }

    . = ALIGN(0x1000);
    .limine_reqs : AT(KERNEL_LMA + (ADDR(.limine_reqs) - KERNEL_DELTA)) {
        KEEP(*(.limine_reqs))
    }

    . = ALIGN(0x1000);
    .limine_req_end : AT(KERNEL_LMA + (ADDR(.limine_req_end) - KERNEL_DELTA)) {
        KEEP(*(.limine_req_end))
    }

    . = ALIGN(0x1000);
    .bss (NOLOAD) : AT(KERNEL_LMA + (ADDR(.bss) - KERNEL_DELTA)) {
        *(.bss .bss.*)
    }

    kernel_phys_start = LOADADDR(.boot.multiboot2);
    kernel_phys_end = LOADADDR(.bss) + SIZEOF(.bss);
    kernel_end = .;
}
