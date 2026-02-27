ENTRY(_start)

SECTIONS
{
    . = 0x100000;

    kernel_start = .;

    .boot.multiboot2 : {
        *(.boot.multiboot2)
    }

    .boot.text : {
        *(.boot.text)
    }

    .boot.rodata : {
        *(.boot.rodata)
    }

    .boot.data : {
        *(.boot.data)
    }

    boot_lma_end = .;

    .boot.bss (NOLOAD) : {
        *(.boot.bss)
    }

    .text : {
        *(.text .text.*)
    }

    .rodata : {
        *(.rodata .rodata.*)
    }

    .data : {
        *(.data .data.*)
    }

    .bss (NOLOAD) : {
        *(.bss .bss.*)
    }

    kernel_end = .;
}