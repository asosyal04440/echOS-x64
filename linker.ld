OUTPUT_FORMAT("elf64-x86-64")
OUTPUT_ARCH(i386:x86-64)
ENTRY(_start)

KERNEL_VMA = 0xFFFFFFFF80000000;
KASLR_OFFSET = 0;

SECTIONS
{
    . = 0x100000;
    boot_start = .;
    .boot : AT(0x100000)
    {
        KEEP(*(.boot.multiboot2))
        . = ALIGN(8);
        *(.boot .boot.*)
        . = ALIGN(16);
        stack_bottom = .;
        . = . + 0x8000;
        stack_top = .;
    }
    boot_lma_end = ALIGN(LOADADDR(.boot) + SIZEOF(.boot), 0x200000);

    . = KERNEL_VMA + KASLR_OFFSET;
    kernel_start = .;
    .text : AT(ADDR(.text) - KERNEL_VMA - KASLR_OFFSET + boot_lma_end)
    {
        *(.text .text.*)
    }
    .rodata : AT(ADDR(.rodata) - KERNEL_VMA - KASLR_OFFSET + boot_lma_end)
    {
        *(.rodata .rodata.*)
    }
    .data : AT(ADDR(.data) - KERNEL_VMA - KASLR_OFFSET + boot_lma_end)
    {
        *(.data .data.*)
    }
    .bss (NOLOAD) : AT(ADDR(.bss) - KERNEL_VMA - KASLR_OFFSET + boot_lma_end)
    {
        *(COMMON)
        *(.bss .bss.*)
    }
    kernel_end = .;

    /DISCARD/ :
    {
        *(.eh_frame)
        *(.eh_frame.*)
        *(.eh_frame_hdr)
    }
}
