
; echOS AP (Application Processor) Başlatma Trampoleni
;
; Bu dosya, çok işlemcili sistemlerde (SMP) ek CPU çekirdeklerini (AP'leri) uyandırmak için
; kullanılan düşük bellekli assembly kodunu içerir.
;
; Trampolin Nedir?
;   x86 CPU'ları RESET sonrasında Real Mod'da (16-bit) başlar; AP'ler uyandırıldığında
;   da 16-bit Real Mod'da yürütmeye başlarlar. Ancak çekirdek 64-bit Long Mod'da çalışır.
;   Bu "trampolin" kodu, AP'yi adım adım aşağıdaki modlara taşır:
;
;   ┌─────────────────────────────────────────────────────────────────────────┐
;   │  Real Mod (16-bit)  →  Protected Mod (32-bit)  →  Long Mod (64-bit)    │
;   │       0x1000                 protected_mode              long_mode      │
;   │  GDT'yi yükle          CR0.PE=1, segment kayıtları    CR0.PG=1, RIP→Rust│
;   │  CR0.PE=1 yap          PAE etkin, PML4 yükle           ap_entry() çağır │
;   └─────────────────────────────────────────────────────────────────────────┘
;
; Bellek Yerleşimi:
;   Trampolin 0x1000 fiziksel adresine kopyalanır.
;   Bu adres 1. MB içinde olduğundan Real Mod'da erişilebilirdir.
;   ap_startup_data yapısı bu sayfanın sonunda başlar ve BSP tarafından doldurulur.

.intel_syntax noprefix
.section .text.ap_trampoline, "ax"
.code16

.global ap_startup_begin
.global ap_startup_end
.global ap_startup_data

; GDT ve atlama hedefi için ofset hesapları (derleme zamanında çözülür)
.set ap_startup_base, 0x1000
.set gdt_offset, gdt - ap_startup_begin
.set gdt_ptr_offset, gdt_ptr - ap_startup_begin
.set protected_mode_offset, protected_mode - ap_startup_begin
.set protected_mode_target, ap_startup_base + protected_mode_offset
.set ap_startup_data_offset, ap_startup_data - ap_startup_begin
.set long_mode_offset, long_mode - ap_startup_begin
.set far_ptr_offset, far_ptr_scratch - ap_startup_begin

.align 4096
ap_startup_begin:
    jmp start                               ; Başlangıç koduna atla

    ; Uzak atlamalar (far jump) için çalışma zamanında doldurulan veri alanı.
    ; GDT girişleriyle üst üste gelmemesi için jmp'nin hemen ardına yerleştirilir.
    .align 4
far_ptr_scratch:
    .long 0   # offset alanı (çalışma zamanında doldurulur)
    .word 0   # seçici alanı (çalışma zamanında doldurulur)

    ; ── Global Descriptor Table (GDT) ──
    ; Protected Mod ve Long Mod için segment tanımlayıcıları
    .align 8
gdt:
    # Boş tanımlayıcı (Null Descriptor) — GDT'nin 0. girişi her zaman sıfır olmalıdır
    .quad 0x0000000000000000
    # 0x08: 32-bit Kod Segmenti — taban=0, limit=4GB, 32-bit, çalıştırılabilir, okunabilir
    .quad 0x00CF9A000000FFFF
    # 0x10: 32-bit Veri Segmenti — taban=0, limit=4GB, 32-bit, yazılabilir
    .quad 0x00CF92000000FFFF
    # 0x18: 64-bit Kod Segmenti — Long Mod kod segmenti (L biti=1)
    .quad 0x00AF9A000000FFFF
    # 0x20: 64-bit Veri Segmenti — Long Mod veri segmenti
    .quad 0x00CF92000000FFFF
gdt_end:

; GDT İşaretçisi (Pointer) — `lgdt` komutu bu yapıyı okur
gdt_ptr:
    .word gdt_end - gdt - 1                 ; Limit: GDT boyutunun bir eksiği
    .long ap_startup_base + gdt_offset      ; Taban: GDT'nin çalışma zamanı fiziksel adresi

; ── Real Mod Giriş Noktası ──
start:
    cli                                     ; Kesmeleri kapat — başlatma tamamlanana kadar
    cld                                     ; Yön bayrağını temizle (artan bellek erişimi)
    mov al, 0x41                            ; 'A' → QEMU debug port 0xE9'a yaz (hata ayıklama izleme)
    out 0xE9, al

    ; Segment kayıtlarını sıfırla — Real Mod'da segmentler 0 taban alırsın
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00                          ; Stack işaretçisini geçici yığın alanına ayarla

    ; GDT taban adresini çalışma zamanı konumuna göre güncelle (fixup)
    ; Derleme zamanındaki adresler geçersiz — çalışma zamanında 0x1000+offset hesaplanmalı
    mov ebx, ap_startup_base
    lea ebx, [ebx + gdt_offset]

    ; GDT işaretçisi (gdt_ptr) yapısına erişim için SI kaydını kullan
    mov si, ap_startup_base
    lea si, [si + gdt_ptr_offset]

    ; GDT işaretçisinin 32-bit taban alanını hesaplanan fiziksel adresle güncelle
    mov [si+2], ebx

    ; GDT'yi yükle — bu noktadan sonra segment kayıtları anlamlı seçiciler kullanabilir
    lgdt [si]

    mov al, 0x42                            ; 'B' → GDT yüklendi
    out 0xE9, al

    ; ── Real Mod'dan Protected Mod'a Geçiş ──
    ; CR0'ın PE (Protection Enable) bitini 1 yap → Protected Mod aktif
    mov eax, cr0
    or eax, 1
    mov cr0, eax

    mov al, 0x43                            ; 'C' → CR0.PE=1, Protected Mod etkin
    out 0xE9, al

    ; Protected Mod için uzak atlamayı (far jump) scratch alanında hazırla
    ; Bu yaklaşım kodu üzerine YAZMAYI önler (kod modifikasyonunuz önlemi)
    mov ebx, ap_startup_base
    lea ebx, [ebx + protected_mode_offset]

    ; Far pointer yapısını scratch alana yaz (GDT girişlerinin üstüne yazmaktan kaçın)
    mov si, ap_startup_base
    lea si, [si + far_ptr_offset]

    ; 32-bit offset değerini yaz
    mov [si], ebx
    ; Kod seçicisi 0x08 yaz (32-bit Kod Segmenti — GDT'deki 1. giriş)
    mov word ptr [si + 4], 0x08

    ; m16:32 formatında uzak atlama yap (segment:offset)
    ; 0x66 prefix → 32-bit operand (Real Mod'da default 16-bit'ti)
    ; 0xFF 0x2C  → JMP m16:32 [SI] — SI'nın gösterdiği adresten atlama hedefini oku
    .byte 0x66   # Operand boyutu geçersizleme: 32-bit offset kullan
    .byte 0xFF   # JMP m16:32 komutu
    .byte 0x2C   # ModRM: [SI] — SI adresindeki far pointer'ı kullan

    ; ── 32-bit Protected Mod'da Devam ──
    .code32
protected_mode:
    mov al, 0x44                            ; 'D' → Protected Mod'a başarıyla geçildi
    out 0xE9, al

    ; Veri segmenti seçicilerini ayarla: 0x10 = 32-bit Veri Segmenti (GDT'deki 2. giriş)
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov fs, ax
    mov gs, ax

    mov ebx, ap_startup_base

    ; ── Long Mod (64-bit) Geçişi Hazırlığı ──

    ; CR4.PAE (bit 5) = 1: Fiziksel Adres Uzantısı etkinleştir
    ; PAE 4 KB sayfalar yerine 2 MB sayfa desteği sağlar ve Long Mod için ZORUNLUDur
    mov eax, cr4
    or eax, (1 << 5)
    mov cr4, eax

    ; CR3'e PML4 (Page Map Level 4) sayfasının fiziksel adresini yaz
    ; PML4, 64-bit sayfalama hiyerarşisinin 4. ve en üst seviyesidir:
    ;   PML4 → PDPT → PD → PT → Fiziksel Sayfa
    mov eax, dword ptr [ebx + ap_startup_data_offset]
    test eax, eax
    jz pml4_error                           ; PML4 adresi NULL ise hata yolu
    mov cr3, eax

    ; EFER MSR (Extended Feature Enable Register) üzerinden Long Mod ve NX etkinleştir
    ; MSR 0xC0000080 = IA32_EFER
    ;   Bit 8 (LME): Long Mode Enable — bu bit page table aktif olunca LMA'ya dönüşür
    ;   Bit 11 (NXE): No-Execute Enable — çalıştırma koruması için sayfa biti
    mov ecx, 0xC0000080
    rdmsr
    or eax, 0x900  ; (1 << 8) = LME | (1 << 11) = NXE
    wrmsr

    ; CR0.PG (bit 31) = 1: Sayfalama etkinleştir → Long Mod Etkin (LMA biti set edilir)
    ; CR0.PE (bit 0) = 1 zaten set edilmişti; PG=1 eklenerek 64-bit geçişi tamamlanır
    mov eax, cr0
    or eax, (1 << 31)
    mov cr0, eax

    mov al, 0x45                            ; 'E' → Sayfalama etkin, Long Mod aktif
    out 0xE9, al

    ; 64-bit kod segmentine uzak atlamayla geç
    ; CS=0x18 (Long Mod Kod Segmenti) ve EIP=long_mode_offset kombinasyonu
    ; retf komutu yığından CS:EIP çifti alarak uzak dönüş yapar (far return = far jump)
    push 0x18
    lea eax, [ebx + long_mode_offset]
    push eax
    retf

pml4_error:
    mov al, 0x58  ; 'X' — PML4 adresi NULL, hata durumu
    out 0xE9, al
    hlt                                     ; CPU'yu durdur — kurtarma yok

    ; ── 64-bit Long Mod'da Devam ──
    .code64
long_mode:
    mov al, 0x46                            ; 'F' → Long Mod'a başarıyla geçildi
    out 0xE9, al

    ; 64-bit veri segmenti seçicilerini ayarla: 0x20 = 64-bit Veri Segmenti
    ; Long Mod'da CS dışındaki segment kayıtları büyük ölçüde görmezden gelinir;
    ; yine de uyumlu değerler yüklenir
    mov ax, 0x20
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov fs, ax
    mov gs, ax

    ; ap_startup_data yapısından yığın (stack) üst adresini oku ve RSP'ye yükle
    ; ap_startup_data düzeni:
    ;   +0:  pml4_phys  (u64) — PML4 sayfasının fiziksel adresi
    ;   +8:  entry      (u64) — ap_entry() Rust fonksiyonunun sanal adresi
    ;   +16: stack_top  (u64) — Bu AP için ayrılmış çekirdek yığınının üst adresi
    ;   +24: cpu_data   (u64) — CpuData yapısına işaretçi
    mov rbx, ap_startup_base
    mov rsp, [rbx + ap_startup_data_offset + 16]  ; RSP = stack_top

    ; Yığın adresinin geçerli olduğunu doğrula (NULL kontrolü)
    test rsp, rsp
    jz stack_error                          ; Yığın adresi NULL ise hata yolu

    mov al, 0x47                            ; 'G' → Yığın hazır
    out 0xE9, al

    ; ap_entry(cpu_data: &'static mut CpuData) için argümanı hazırla
    ; SysV x86-64 ABI: ilk integer/pointer argüman RDI kaydında geçirilir.
    ; "extern sysv64" bildirimi sayesinde Windows ve Linux'ta aynı çağrı kuralı kullanılır.
    mov rdi, [rbx + ap_startup_data_offset + 24]    ; RDI = cpu_data işaretçisi

    ; Giriş noktası adresini RAX'e yükle ('G' karakteri yazdıktan SONRA — AL'yi bozmamak için)
    mov rax, [rbx + ap_startup_data_offset + 8]     ; RAX = ap_entry() fonksiyon adresi

    ; Rust AP giriş noktasını çağır — bu noktadan sonra Rust kodu yürütülür
    call rax

    ; ap_entry() hiçbir zaman dönmemelidir (! dönüş türü)
    ; Eğer bir şekilde dönerse CPU'yu güvenli şekilde durdur
    cli                                     ; Kesmeleri kapat
    hlt                                     ; CPU'yu durdur

stack_error:
    mov al, 0x59  ; 'Y' — Yığın adresi NULL, hata durumu
    out 0xE9, al
    cli                                     ; Kesmeleri kapat
    hlt                                     ; CPU'yu durdur

; ── AP Başlatma Veri Yapısı ──
; BSP bu alanı trampoline kopyalanmadan önce veya hemen ardından doldurur.
; Her alan 8 bayt (u64) genişliğinde ve 16 bayta hizalanmıştır.
.align 16
ap_startup_data:
    .quad 0 ; pml4_phys  — Kernelin PML4 tablo fiziksel adresi
    .quad 0 ; entry      — ap_entry() Rust fonksiyonunun sanal adresi
    .quad 0 ; stack_top  — Bu AP'ye özel çekirdek yığınının üst adresi
    .quad 0 ; cpu_data   — CpuData yapısına işaretçi (Rust tarafından doldurulur)

; Trampolin sayfayı tam 4096 bayta tamamla (geri kalanı sıfırla)
.fill 4096 - (. - ap_startup_begin), 1, 0
ap_startup_end:
; Önyükleme imzası — bazı araçlar için referans noktası
.word 0xAA55
