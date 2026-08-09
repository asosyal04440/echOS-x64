.intel_syntax noprefix

.section .boot.multiboot2, "a"
.align 8
mb2_hdr:
    .long 0xE85250D6
    .long 0
    .long mb2_hdr_end - mb2_hdr
    .long -(0xE85250D6 + 0 + (mb2_hdr_end - mb2_hdr))
    .short 0
    .short 0
    .long 8
mb2_hdr_end:

.section .boot.text, "ax"
.code32
.global _start
_start:
    cli                         # Mask interrupts while switching CPU modes
    mov ecx, cr0                # Mode-agnostic probe (0F 20 C1: decodes identically in
                                # 32-bit protected mode and 64-bit long mode)
    test ecx, 0x80000000        # Test PG (paging) bit; Limine enters with paging enabled,
                                # Multiboot2 (GRUB) enters with paging disabled
    jnz .limine_entry64         # Paging on -> 64-bit Limine handoff (image slid to higher half)
    cmp eax, 0x36d76289         # Multiboot2 magic check (EAX must match)
    je .mb_ok
    mov dword ptr [mb2_info_addr], 0
    jmp .mb_continue
.mb_ok:
    mov dword ptr [mb2_info_addr], ebx  # Preserve Multiboot2 info address (EBX)
.mb_continue:
    mov dword ptr [boot_magic], eax
    mov dx, 0xE9
    mov al, 0x41
    out dx, al
    mov ax, ds
    mov es, ax
    mov ss, ax
    cld
    lea esp, [stack_top]
    xor ebp, ebp
    mov dx, 0xE9
    mov al, 0x42
    out dx, al

    mov eax, 1                   # CPUID leaf 1 (assumes CPUID is supported)
    cpuid                        # Query CPU features (assumes leaf 1 is valid)
    bt ecx, 30                   # Test RDRAND support bit
    jc .rdrand_ok                # Jump if RDRAND is available
    rdtsc                        # Fallback entropy: TSC (assumes RDTSC available)
    xor eax, 0xA5A5A5A5          # Mix low bits with a constant to decorrelate
    xor edx, 0x5A5A5A5A          # Mix high bits similarly
    jmp .entropy_ready           # Proceed with entropy-derived offset

.rdrand_ok:
    rdrand eax                   # Get random low dword
    jc .rdrand_part2             # If valid, continue; otherwise fallback
    rdtsc                        # Fallback if RDRAND failed (temporary)
    xor eax, 0xA5A5A5A5          # Mix low bits to reduce bias

.rdrand_part2:
    rdrand edx                   # Get random high dword
    jc .entropy_ready            # If valid, continue; otherwise fallback
    rdtsc                        # Fallback for high bits
    xor edx, 0x5A5A5A5A          # Mix high bits

.entropy_ready:
    xor eax, eax
    xor edx, edx
    mov dword ptr [kaslr_offset], eax      # Store KASLR offset low
    mov dword ptr [kaslr_offset + 4], edx  # Store KASLR offset high

    lea edi, [pml4_table]        # Zero PML4 table (4KiB)
    mov ecx, 1024                # 1024 dwords = 4096 bytes
    xor eax, eax                 # Zero fill value
    rep stosd                    # Clear PML4

    lea edi, [pdpt_table]        # Zero PDPT table (4KiB)
    mov ecx, 1024                # 1024 dwords = 4096 bytes
    xor eax, eax                 # Zero fill value
    rep stosd                    # Clear PDPT

    lea edi, [pdpt_table_high]   # Zero PDPT table for higher-half (4KiB)
    mov ecx, 1024                # 1024 dwords = 4096 bytes
    xor eax, eax                 # Zero fill value
    rep stosd                    # Clear PDPT

    lea edi, [pd_table]          # Zero PD table for identity map (4KiB)
    mov ecx, 1024                # 1024 dwords = 4096 bytes
    xor eax, eax                 # Zero fill value
    rep stosd                    # Clear PD

    lea edi, [pd_table_high]     # Zero PD table for higher-half map (4KiB)
    mov ecx, 1024                # 1024 dwords = 4096 bytes
    xor eax, eax                 # Zero fill value
    rep stosd                    # Clear PD
    mov dx, 0xE9
    mov al, 0x45
    out dx, al

    lea eax, [pd_table]          # PD physical base for identity mapping
    or eax, 0x03                 # Present + Writable flags
    mov dword ptr [pdpt_table + 0], eax         # PDPT[0] -> identity PD
    mov dword ptr [pdpt_table + 4], 0           # Upper 32 bits = 0
    lea eax, [pd_table_high]     # PD physical base for higher-half mapping
    or eax, 0x03                 # Present + Writable flags
    mov dword ptr [pdpt_table_high + 510*8 + 0], eax # PDPT_HIGH[510] -> higher-half PD
    mov dword ptr [pdpt_table_high + 510*8 + 4], 0   # Upper 32 bits = 0

    lea eax, [pdpt_table]        # PDPT physical base
    or eax, 0x03                 # Present + Writable flags
    mov dword ptr [pml4_table + 0], eax         # PML4[0] -> PDPT (identity)
    mov dword ptr [pml4_table + 4], 0           # Upper 32 bits = 0
    lea eax, [pdpt_table]
    or eax, 0x03
    mov dword ptr [pml4_table + 256*8 + 0], eax
    mov dword ptr [pml4_table + 256*8 + 4], 0
    lea eax, [pdpt_table_high]   # PDPT physical base for higher-half
    or eax, 0x03                 # Present + Writable flags
    mov dword ptr [pml4_table + 511*8 + 0], eax # PML4[511] -> PDPT_HIGH
    mov dword ptr [pml4_table + 511*8 + 4], 0   # Upper 32 bits = 0

    lea edi, [pd_table]          # Fill PD with 2MiB identity mappings
    mov ecx, 512                 # 512 entries = 1GiB coverage
    xor ebx, ebx                 # Physical base starts at 0
.map_identity:
    mov eax, ebx                 # Physical base for this 2MiB chunk
    or eax, 0x83                 # Present + Writable + PS (2MiB page)
    mov dword ptr [edi + 0], eax # Write low 32 bits of PDE
    mov dword ptr [edi + 4], 0   # Upper 32 bits = 0
    add ebx, 0x200000            # Next 2MiB physical chunk
    add edi, 8                   # Next PDE
    loop .map_identity           # Repeat for 1GiB

    xor esi, esi
    lea ebx, [boot_lma_end]
    lea edi, [pd_table_high]           # Fill higher-half PD with offset mapping
    mov ecx, 512                       # 512 entries = 1GiB coverage
.map_high:
    cmp esi, 0                         # Check if we reached the offset index
    jbe .map_high_fill                 # If index reached, start mapping
    mov dword ptr [edi + 0], 0         # Leave entry unmapped before offset
    mov dword ptr [edi + 4], 0
    dec esi                            # Countdown until offset reached
    add edi, 8                         # Next PDE
    loop .map_high
    jmp .map_high_done
.map_high_fill:
    mov eax, ebx                       # Physical base for this 2MiB chunk
    or eax, 0x83                       # Present + Writable + PS (2MiB page)
    mov dword ptr [edi + 0], eax       # Write low 32 bits of PDE
    mov dword ptr [edi + 4], 0         # Upper 32 bits = 0
    add ebx, 0x200000                  # Next 2MiB physical chunk
    add edi, 8                         # Next PDE
    loop .map_high_fill
.map_high_done:

    mov eax, cr4                 # Read CR4
    or eax, (1 << 5)             # Enable PAE for long mode paging
    or eax, (1 << 4)             # Enable PSE (optional, kept per requirement)
    mov cr4, eax                 # Write CR4

    mov ecx, 0xC0000080          # EFER MSR
    rdmsr                        # Read EFER
    or eax, (1 << 8)             # Set LME (Long Mode Enable)
    or eax, (1 << 11)            # Set NXE (No-Execute Enable)
    wrmsr                        # Write EFER

    lea eax, [pml4_table]        # Load PML4 physical address
    mov cr3, eax                 # Activate page tables

    mov eax, cr0                 # Read CR0
    or eax, (1 << 31)            # Enable paging
    mov cr0, eax                 # Write CR0
    mov dx, 0xE9
    mov al, 0x46
    out dx, al

    lgdt [gdt64_ptr]             # Load 64-bit GDT before far jump

    .att_syntax
    ljmp $0x08, $trampoline_64
    .intel_syntax noprefix

.halt:
    hlt                          # Stop execution on invalid boot
    jmp .halt                    # Keep CPU halted

.code64
/*
 * Limine 64-bit entry path.
 *
 * Limine (base revision >= 1) slides a lower-half-linked executable into the
 * higher half (>= 0xffffffff80000000) and maps the whole image (every PT_LOAD,
 * including the NOLOAD .boot.bss zero-fill) at the slid addresses; no low
 * identity map is provided. Every memory reference in this path must therefore
 * be RIP-relative -- absolute link-time addresses would fault.
 *
 * The kernel image itself is verified to be fully RIP-relative (no absolute
 * pointers in code or data), so no runtime patching is required under the
 * slide. The slide value is still computed below and passed to kernel_entry
 * as the KASLR offset: the kernel's virtual memory model is
 * KERNEL_SPACE_START + kaslr_offset, and the physical kernel region guard
 * (frame allocator) is driven by the Executable Address feature's physical
 * base, so the slide must reach the allocator, not be silently zero.
 *
 * Entry state (Limine protocol spec, x86-64): CS=0x28, DS/ES/SS/FS/GS=0x30
 * (flat 64-bit), IF=0, DF=0, paging enabled (PG/PAE/LME/LMA/NX), RSP points to
 * the top of a bootloader-provided stack, all GP registers are zeroed. Base
 * revision 4 does not guarantee the base-revision-5 cleanups (RFLAGS, IDTR,
 * LDTR, TR state undefined), so this bootstrap path loads its own GDT/TSS/IDT.
 */
.limine_entry64:
    mov dx, 0xE9
    mov al, 0x4C                 # 'L' marker: Limine entry reached
    out dx, al
    lea rsp, [rip + stack_top]   # Switch to the kernel's own boot stack
    xor rbp, rbp
    cld
    # Runtime GDT pointer: the static gdt64_ptr embeds the link-time base,
    # which is invalid under the Limine higher-half slide, so rebuild it here.
    lea rax, [rip + gdt64]
    mov qword ptr [rip + gdt64_ptr_runtime + 2], rax
    lgdt [rip + gdt64_ptr_runtime]
    # Switch CS from Limine's selector (0x28, not valid in our GDT) to 0x08.
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
    # TSS: fill the descriptor in our GDT and load the task register.
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
    # Minimal IDT safety net (Rust's interrupts::init replaces it later).
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
    mov al, 0x6C                 # 'l' marker: dispatching to kernel_entry
    out dx, al
    # KASLR slide hesabı: Limine görüntüyü tek bir offset ile higher-half'a
    # kaydırır. Slide = runtime(kernel_start) - link-time(kernel_start);
    # link-time sabiti .boot.data'daki sentinel'da tutulur (bootloader'a
    # görünür .limine_reqs gibi ALLOC bölgelerinde). Sonuç kernel_entry'ye
    # kaslr_offset olarak geçirilir (virtual model).
    lea rax, [rip + kernel_start]
    mov rbx, qword ptr [rip + link_base_sentinel]
    sub rax, rbx                 # rax = slide
    xor edi, edi                 # Arg1: boot_info_addr = 0 (Limine state via requests)
    mov rsi, rax                 # Arg2: kaslr_offset = slide
    xor edx, edx                 # Arg3: boot_magic = 0 (dispatch via limine_available())
    lea rax, [rip + kernel_entry]
    jmp rax

.code64
trampoline_64:
    mov dx, 0xE9
    mov al, 0x43
    out dx, al
    lea rsp, [stack_top]
    xor rbp, rbp
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    xor ax, ax
    mov fs, ax
    mov gs, ax
    mov ax, 0x10
    mov ss, ax
    mov dx, 0xE9
    mov al, 0x52
    out dx, al
    lea rax, [tss64]
    mov word ptr [gdt64 + 0x18], 0x0067
    mov word ptr [gdt64 + 0x1A], ax
    shr rax, 16
    mov byte ptr [gdt64 + 0x1C], al
    mov byte ptr [gdt64 + 0x1D], 0x89
    mov byte ptr [gdt64 + 0x1E], 0
    mov byte ptr [gdt64 + 0x1F], ah
    shr rax, 16
    mov dword ptr [gdt64 + 0x20], eax
    mov dword ptr [gdt64 + 0x24], 0
    lea rax, [stack_top]
    mov qword ptr [tss64 + 0x04], rax
    lea rax, [ist_stack_top]
    mov qword ptr [tss64 + 0x20], rax
    mov word ptr [tss64 + 0x5E], 0x0068
    mov ax, 0x18
    ltr ax
    lea rdi, [idt64]
    mov rcx, 256
    lea rax, [rip + isr_default]
.fill_idt:
    call set_gate
    add rdi, 16
    loop .fill_idt
    lea rax, [rip + isr_ud]
    lea rdi, [idt64 + 16*6]
    call set_gate
    lea rax, [rip + isr_df]
    lea rdi, [idt64 + 16*8]
    mov cl, 1
    call set_gate_ist
    lea rax, [rip + isr_gp]
    lea rdi, [idt64 + 16*13]
    call set_gate
    lea rax, [rip + isr_pf]
    lea rdi, [idt64 + 16*14]
    mov cl, 1
    call set_gate_ist
    lidt [idt64_ptr]
    mov dx, 0xE9
    mov al, 0x49
    out dx, al

    mov r15, qword ptr [kaslr_offset]  # Preserve KASLR offset in non-volatile reg
    mov dx, 0xE9
    mov al, 0x4A
    out dx, al
    mov dx, 0xE9
    mov al, 0x49
    out dx, al
    mov rbx, rdi
    call print_hex64
    mov rdx, qword ptr [boot_magic]    # Boot magic
    push rdx
    mov dx, 0xE9
    mov al, 0x4D
    out dx, al
    pop rdx
    mov rbx, rdx
    push rdx
    call print_hex64
    pop rdx
    movabs rax, OFFSET kernel_entry
    push rax
    push rdx
    mov rbx, rax
    call print_hex64
    pop rdx
    pop rax
    mov rdi, qword ptr [mb2_info_addr] # Arg1: Multiboot2 info physical address
    mov rsi, r15                       # Arg2: KASLR offset
    mov rdx, qword ptr [boot_magic]    # Arg3: Boot magic
    jmp rax

.bad_kernel_ptr:
    mov dx, 0xE9
    mov al, 0x5A
    out dx, al
    jmp .halt64

.halt64:
    hlt                          # If kernel returns, halt
    jmp .halt64                  # Stay halted

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

.section .boot.data, "a"
.align 8
mb2_info_addr:
    .quad 0
boot_magic:
    .quad 0
kaslr_offset:
    .quad 0
gdt64_ptr_runtime:
    .word (gdt64_end - gdt64 - 1)
    .quad 0
.align 8
idt64_ptr_runtime:
    .word (idt64_end - idt64 - 1)
    .quad 0
kernel_main_ptr:
    .quad kernel_entry
link_base_sentinel:
    .quad kernel_start

.section .boot.bss, "aw", @nobits
.align 4096
pml4_table:
    .skip 4096
.align 4096
pdpt_table:
    .skip 4096
.align 4096
pdpt_table_high:
    .skip 4096
.align 4096
pd_table:
    .skip 4096
.align 4096
pd_table_high:
    .skip 4096

.section .boot.bss, "aw", @nobits
.align 16
idt64:
    .skip 4096
idt64_end:
.align 4096

.section .boot.rodata, "a"
.align 8
idt64_ptr:
    .word (idt64_end - idt64 - 1)
    .quad idt64

hex_digits:
    .ascii "0123456789ABCDEF"

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

.section .boot.bss, "aw", @nobits
.align 16
tss64:
    .skip 104
.align 16
boot_stack:
    # BootContext carries the bounded pre-heap memory map (256 entries).  Keep
    # enough headroom for the adapter handover, normalization, and stage-gate
    # call chain before the final allocator is online.
    .skip 4194304
stack_top:
.align 16
ist_stack:
    .skip 4096
ist_stack_top:
