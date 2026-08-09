.intel_syntax noprefix

/* Native Limine entry.  Selected only for the higher-half linker image. */
.section .boot.text, "ax"
.code64
.global _start
_start:
    cli
    mov dx, 0xE9
    mov al, 0x4C
    out dx, al
    lea rsp, [rip + stack_top]
    xor rbp, rbp
    cld

    lea rax, [rip + gdt64]
    mov qword ptr [rip + gdt64_ptr_runtime + 2], rax
    lgdt [rip + gdt64_ptr_runtime]
    lea rax, [rip + .limine_cs_ok]
    push 0x08
    push rax
    retfq

.limine_cs_ok:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    lea rax, [rip + tss64]
    mov word ptr [rip + gdt64 + 0x18], 0x0067
    mov word ptr [rip + gdt64 + 0x1A], ax
    shr rax, 16
    mov byte ptr [rip + gdt64 + 0x1C], al
    mov byte ptr [rip + gdt64 + 0x1D], 0x89
    mov byte ptr [rip + gdt64 + 0x1E], 0
    mov byte ptr [rip + gdt64 + 0x1F], ah
    shr rax, 16
    mov dword ptr [rip + gdt64 + 0x20], eax
    mov dword ptr [rip + gdt64 + 0x24], 0
    lea rax, [rip + stack_top]
    mov qword ptr [rip + tss64 + 0x04], rax
    lea rax, [rip + ist_stack_top]
    mov qword ptr [rip + tss64 + 0x20], rax
    mov word ptr [rip + tss64 + 0x5E], 0x0068
    mov ax, 0x18
    ltr ax

    lea rdi, [rip + idt64]
    mov rcx, 256
    lea rax, [rip + isr_default]
.fill_idt_limine:
    call set_gate
    add rdi, 16
    loop .fill_idt_limine
    lea rax, [rip + isr_ud]
    lea rdi, [rip + idt64 + 16*6]
    call set_gate
    lea rax, [rip + isr_df]
    lea rdi, [rip + idt64 + 16*8]
    mov cl, 1
    call set_gate_ist
    lea rax, [rip + isr_gp]
    lea rdi, [rip + idt64 + 16*13]
    call set_gate
    lea rax, [rip + isr_pf]
    lea rdi, [rip + idt64 + 16*14]
    mov cl, 1
    call set_gate_ist
    lea rax, [rip + idt64]
    mov qword ptr [rip + idt64_ptr_runtime + 2], rax
    lidt [rip + idt64_ptr_runtime]

    mov dx, 0xE9
    mov al, 0x6C
    out dx, al
    lea rax, [rip + kernel_start]
    mov rbx, qword ptr [rip + link_base_sentinel]
    sub rax, rbx
    xor edi, edi
    mov rsi, rax
    xor edx, edx
    lea rax, [rip + kernel_entry]
    jmp rax

.halt64:
    hlt
    jmp .halt64

.align 16
set_gate:
    mov word ptr [rdi], ax
    mov word ptr [rdi + 2], 0x08
    mov byte ptr [rdi + 4], 0
    mov byte ptr [rdi + 5], 0x8E
    mov rdx, rax
    shr rdx, 16
    mov word ptr [rdi + 6], dx
    mov rdx, rax
    shr rdx, 32
    mov dword ptr [rdi + 8], edx
    mov dword ptr [rdi + 12], 0
    ret

.align 16
set_gate_ist:
    mov word ptr [rdi], ax
    mov word ptr [rdi + 2], 0x08
    mov byte ptr [rdi + 4], cl
    mov byte ptr [rdi + 5], 0x8E
    mov rdx, rax
    shr rdx, 16
    mov word ptr [rdi + 6], dx
    mov rdx, rax
    shr rdx, 32
    mov dword ptr [rdi + 8], edx
    mov dword ptr [rdi + 12], 0
    ret

.align 16
print_hex64:
    mov rcx, 16
    mov r8, rbx
.hex_loop:
    mov rax, r8
    shr rax, 60
    and al, 0x0F
    lea rsi, [rip + hex_digits]
    mov al, byte ptr [rsi + rax]
    mov dx, 0xE9
    out dx, al
    shl r8, 4
    loop .hex_loop
    ret

.align 16
isr_default:
    mov dx, 0xE9
    mov al, 0x58
    out dx, al
.isr_default_halt:
    hlt
    jmp .isr_default_halt

.align 16
isr_gp:
    mov dx, 0xE9
    mov al, 0x51
    out dx, al
.isr_gp_halt:
    hlt
    jmp .isr_gp_halt

.align 16
isr_pf:
    mov dx, 0xE9
    mov al, 0x50
    out dx, al
    mov rbx, cr2
    call print_hex64
.isr_pf_halt:
    hlt
    jmp .isr_pf_halt

.align 16
isr_ud:
    mov dx, 0xE9
    mov al, 0x55
    out dx, al
.isr_ud_halt:
    hlt
    jmp .isr_ud_halt

.align 16
isr_df:
    mov dx, 0xE9
    mov al, 0x44
    out dx, al
.isr_df_halt:
    hlt
    jmp .isr_df_halt

.section .boot.data, "aw"
.align 8
gdt64_ptr_runtime:
    .word (gdt64_end - gdt64 - 1)
    .quad 0
.align 8
idt64_ptr_runtime:
    .word (idt64_end - idt64 - 1)
    .quad 0
.align 8
link_base_sentinel:
    .quad kernel_start

.section .boot.bss, "aw", @nobits
.align 16
idt64:
    .skip 4096
idt64_end:
.align 16
tss64:
    .skip 104
.align 16
boot_stack:
    # Native Limine enters directly into the shared boot pipeline.  The
    # bounded canonical map and PMM handover need headroom before heap use.
    .skip 4194304
stack_top:
.align 16
ist_stack:
    .skip 4096
ist_stack_top:

.section .boot.rodata, "a"
.align 8
hex_digits:
    .ascii "0123456789ABCDEF"

.section .boot.data, "aw"
.align 8
gdt64:
    .quad 0
    .quad 0x00AF9A000000FFFF
    .quad 0x00AF92000000FFFF
    .quad 0
    .quad 0
gdt64_ptr:
    .word (gdt64_end - gdt64 - 1)
    .quad gdt64
gdt64_end:
