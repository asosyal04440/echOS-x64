
.intel_syntax noprefix
.section .text.ap_trampoline, "ax"
.code16

.global ap_startup_begin
.global ap_startup_end
.global ap_startup_data

.set ap_startup_base, 0x1000

# Calculate offsets for GDT and jump target
.set gdt_offset, gdt - ap_startup_begin
.set gdt_ptr_offset, gdt_ptr - ap_startup_begin
.set protected_mode_offset, protected_mode - ap_startup_begin
.set protected_mode_target, ap_startup_base + protected_mode_offset
.set ap_startup_data_offset, ap_startup_data - ap_startup_begin
.set long_mode_offset, long_mode - ap_startup_begin
.set far_ptr_offset, far_ptr_scratch - ap_startup_begin

.align 4096
ap_startup_begin:
    jmp start

    # Scratch area for far pointer (placed after jmp, before GDT)
    .align 4
far_ptr_scratch:
    .long 0   # offset (filled at runtime)
    .word 0   # selector (filled at runtime)

    .align 8
gdt:
    # Null descriptor (entry 0)
    .quad 0x0000000000000000
    # 0x08: Code32 — base=0, limit=4GB, 32-bit, executable, readable
    .quad 0x00CF9A000000FFFF
    # 0x10: Data32 — base=0, limit=4GB, 32-bit, writable
    .quad 0x00CF92000000FFFF
    # 0x18: Code64 — long mode code segment
    .quad 0x00AF9A000000FFFF
    # 0x20: Data64 — long mode data segment
    .quad 0x00CF92000000FFFF
gdt_end:

gdt_ptr:
    .word gdt_end - gdt - 1
    .long ap_startup_base + gdt_offset

start:
    cli
    cld
    mov al, 0x41
    out 0xE9, al
    
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00
    
    # Fixup GDT base address for runtime
    mov ebx, ap_startup_base
    lea ebx, [ebx + gdt_offset]
    
    # Point SI to GDT pointer
    mov si, ap_startup_base
    lea si, [si + gdt_ptr_offset]
    
    # Write GDT base into GDT pointer
    mov [si+2], ebx
    
    # Load GDT
    lgdt [si]
    
    mov al, 0x42
    out 0xE9, al

    # Enable Protected Mode (PE bit in CR0)
    mov eax, cr0
    or eax, 1
    mov cr0, eax
    
    mov al, 0x43
    out 0xE9, al

    # Prepare far jump target in scratch area (NOT overwriting code!)
    mov ebx, ap_startup_base
    lea ebx, [ebx + protected_mode_offset]

    # Store far pointer at scratch area
    mov si, ap_startup_base
    lea si, [si + far_ptr_offset]
    
    # Write offset (32-bit)
    mov [si], ebx
    # Write selector 0x08 (Code32)
    mov word ptr [si + 4], 0x08

    # Far jump to protected mode using m16:32
    .byte 0x66   # operand size override (32-bit offset)
    .byte 0xFF   # JMP m16:32
    .byte 0x2C   # ModRM: [SI]

    .code32
protected_mode:
    mov al, 0x44
    out 0xE9, al
    
    # Load data segment selectors
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov fs, ax
    mov gs, ax

    mov ebx, ap_startup_base
    
    # Enable PAE (bit 5 of CR4)
    mov eax, cr4
    or eax, (1 << 5)
    mov cr4, eax

    # Load PML4 page table
    mov eax, dword ptr [ebx + ap_startup_data_offset]
    test eax, eax
    jz pml4_error
    mov cr3, eax

    # Enable Long Mode AND NX (No-Execute) in EFER MSR
    mov ecx, 0xC0000080
    rdmsr
    or eax, 0x900  # (1 << 8) | (1 << 11)
    wrmsr

    # Enable Paging (bit 31 of CR0)
    mov eax, cr0
    or eax, (1 << 31)
    mov cr0, eax

    mov al, 0x45
    out 0xE9, al

    # Far jump to 64-bit long mode
    # Push CS=0x18 (Code64) and EIP, then retf
    push 0x18
    lea eax, [ebx + long_mode_offset]
    push eax
    retf

pml4_error:
    mov al, 0x58  # 'X' — PML4 null error
    out 0xE9, al
    hlt

    .code64
long_mode:
    mov al, 0x46
    out 0xE9, al
    
    # Load 64-bit data segment selectors
    mov ax, 0x20
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov fs, ax
    mov gs, ax

    # Load stack pointer from startup data
    mov rbx, ap_startup_base
    mov rsp, [rbx + ap_startup_data_offset + 16]  # stack_top
    
    # Verify stack is not null
    test rsp, rsp
    jz stack_error

    mov al, 0x47
    out 0xE9, al

    # Prepare argument for ap_entry(cpu_data: &'static mut CpuData)
    # The cpu_data pointer is passed via ap_startup_data structure directly!
    # By using extern "sysv64" in Rust, the first arg goes to RDI safely regardless of target OS.
    mov rdi, [rbx + ap_startup_data_offset + 24]

    # Load entry point into rax (do this AFTER printing 'G' to avoid overwriting AL)
    mov rax, [rbx + ap_startup_data_offset + 8]   # entry point

    # Call the Rust AP entry point
    call rax
    
    cli
    hlt

stack_error:
    mov al, 0x59  # 'Y' — stack null error
    out 0xE9, al
    cli
    hlt

.align 16
ap_startup_data:
    .quad 0 # pml4_phys
    .quad 0 # entry
    .quad 0 # stack_top
    .quad 0 # cpu_data

.fill 4096 - (. - ap_startup_begin), 1, 0
ap_startup_end:
.word 0xAA55
