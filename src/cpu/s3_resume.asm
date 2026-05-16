
# echOS ACPI S3 Resume Trampoline
#
# ACPI FACS FirmwareWakingVector enters below 1 MiB in real mode on PC-class
# platforms. This page moves the BSP back to long mode with the live kernel
# PML4, then either resumes the original PM1 caller stack or falls back to the
# Rust S3 resume entry.

.intel_syntax noprefix
.section .text.s3_resume_trampoline, "ax"
.code16

.global s3_resume_begin
.global s3_resume_end
.global s3_resume_data
.global s3_enter_pm1_and_wait

.set s3_resume_base, 0x8000
.set s3_resume_magic, 0x5343485245533345
.set gdt_offset, s3_gdt - s3_resume_begin
.set gdt_ptr_offset, s3_gdt_ptr - s3_resume_begin
.set protected_mode_offset, s3_protected_mode - s3_resume_begin
.set data_offset, s3_resume_data - s3_resume_begin
.set long_mode_offset, s3_long_mode - s3_resume_begin
.set far_ptr_offset, s3_far_ptr_scratch - s3_resume_begin

.align 4096
s3_resume_begin:
    jmp s3_start

    .align 4
s3_far_ptr_scratch:
    .long 0
    .word 0

    .align 8
s3_gdt:
    .quad 0x0000000000000000
    .quad 0x00CF9A000000FFFF
    .quad 0x00CF92000000FFFF
    .quad 0x00AF9A000000FFFF
    .quad 0x00CF92000000FFFF
s3_gdt_end:

s3_gdt_ptr:
    .word s3_gdt_end - s3_gdt - 1
    .long s3_resume_base + gdt_offset

s3_start:
    cli
    cld
    mov al, 0x53
    out 0xE9, al

    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00

    mov ebx, s3_resume_base
    lea ebx, [ebx + gdt_offset]
    mov si, s3_resume_base
    lea si, [si + gdt_ptr_offset]
    mov [si+2], ebx
    lgdt [si]

    mov eax, cr0
    or eax, 1
    mov cr0, eax

    mov ebx, s3_resume_base
    lea ebx, [ebx + protected_mode_offset]
    mov si, s3_resume_base
    lea si, [si + far_ptr_offset]
    mov [si], ebx
    mov word ptr [si + 4], 0x08
    .byte 0x66
    .byte 0xFF
    .byte 0x2C

    .code32
s3_protected_mode:
    mov al, 0x33
    out 0xE9, al

    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov fs, ax
    mov gs, ax

    mov ebx, s3_resume_base
    mov eax, cr4
    or eax, (1 << 5)
    mov cr4, eax

    mov eax, dword ptr [ebx + data_offset]
    test eax, eax
    jz s3_pml4_error
    mov cr3, eax

    mov ecx, 0xC0000080
    rdmsr
    or eax, 0x900
    wrmsr

    mov eax, cr0
    or eax, (1 << 31)
    mov cr0, eax

    push 0x18
    lea eax, [ebx + long_mode_offset]
    push eax
    retf

s3_pml4_error:
    mov al, 0x70
    out 0xE9, al
    cli
    hlt

    .code64
s3_long_mode:
    mov al, 0x52
    out 0xE9, al

    mov ax, 0x20
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov fs, ax
    mov gs, ax

    mov rbx, s3_resume_base

    mov rax, [rbx + data_offset + 40]
    mov rcx, s3_resume_magic
    cmp rax, rcx
    jne s3_call_rust_entry

    mov rsp, [rbx + data_offset + 24]
    test rsp, rsp
    jz s3_stack_error
    mov rax, [rbx + data_offset + 32]
    test rax, rax
    jz s3_entry_error
    mov qword ptr [rbx + data_offset + 40], 0
    jmp rax

s3_call_rust_entry:
    mov rsp, [rbx + data_offset + 16]
    test rsp, rsp
    jz s3_stack_error
    and rsp, -16
    xor rbp, rbp

    mov rax, [rbx + data_offset + 8]
    test rax, rax
    jz s3_entry_error
    call rax

s3_halt:
    cli
    hlt
    jmp s3_halt

s3_stack_error:
    mov al, 0x71
    out 0xE9, al
    jmp s3_halt

s3_entry_error:
    mov al, 0x72
    out 0xE9, al
    jmp s3_halt

.align 16
s3_resume_data:
    .quad 0
    .quad 0
    .quad 0
    .quad 0
    .quad 0
    .quad 0

.fill 4096 - (. - s3_resume_begin), 1, 0
s3_resume_end:

.section .text.s3_pm1_entry, "ax"
.code64
s3_enter_pm1_and_wait:
    mov r10, [rsp + 40]
    test r10, r10
    jz s3_enter_pm1_failed

    mov [r10 + 24], rsp
    lea rax, [rip + s3_pm1_resume_continuation]
    mov [r10 + 32], rax
    mov rax, s3_resume_magic
    mov [r10 + 40], rax

    mov r11d, edx
    mov dx, cx
    mov ax, r8w
    out dx, ax
    test r11w, r11w
    jz s3_pm1_wait_loop
    mov dx, r11w
    mov ax, r9w
    out dx, ax

s3_pm1_wait_loop:
    hlt
    jmp s3_pm1_wait_loop

s3_pm1_resume_continuation:
    mov rax, s3_resume_magic
    ret

s3_enter_pm1_failed:
    xor rax, rax
    ret
