
# echOS AP (Application Processor) Baslatma Trampoleni
#
# Bu dosya, cok islemcili sistemlerde (SMP) ek CPU cekirdeklerini (AP'leri) uyandirmak icin
# kullanilan dusuk bellekli assembly kodunu icerir.
#
# Trampolin Nedir?
#   x86 CPU'lari RESET sonrasinda Real Mod'da (16-bit) baslar; AP'ler uyandirildiginda
#   da 16-bit Real Mod'da yurutmeye baslarlar. Ancak cekirdek 64-bit Long Mod'da calisir.
#   Bu "trampolin" kodu, AP'yi adim adim asagidaki modlara tasir:
#
#   +-------------------------------------------------------------------------+
#   |  Real Mod (16-bit)  ->  Protected Mod (32-bit)  ->  Long Mod (64-bit)    |
#   |       0x1000                 protected_mode              long_mode      |
#   |  GDT'yi yukle          CR0.PE=1, segment kayitlari    CR0.PG=1, RIP->Rust|
#   |  CR0.PE=1 yap          PAE etkin, PML4 yukle           ap_entry() cagir |
#   +-------------------------------------------------------------------------+
#
# Bellek Yerlesimi:
#   Trampolin 0x1000 fiziksel adresine kopyalanir.
#   Bu adres 1. MB icinde oldugundan Real Mod'da erisilebilirdir.
#   ap_startup_data yapisi bu sayfanin sonunda baslar ve BSP tarafindan doldurulur.

.intel_syntax noprefix
.section .text.ap_trampoline, "ax"
.code16

.global ap_startup_begin
.global ap_startup_end
.global ap_startup_data

# GDT ve atlama hedefi icin ofset hesaplari (derleme zamaninda cozulur)
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
    jmp start                               # Baslangic koduna atla

    # Uzak atlamalar (far jump) icin calisma zamaninda doldurulan veri alani.
    # GDT girisleriyle ust uste gelmemesi icin jmp'nin hemen ardina yerlestirilir.
    .align 4
far_ptr_scratch:
    .long 0   # offset alani (calisma zamaninda doldurulur)
    .word 0   # secici alani (calisma zamaninda doldurulur)

    # -- Global Descriptor Table (GDT) --
    # Protected Mod ve Long Mod icin segment tanimlayicilari
    .align 8
gdt:
    # Bos tanimlayici (Null Descriptor) ? GDT'nin 0. girisi her zaman sifir olmalidir
    .quad 0x0000000000000000
    # 0x08: 32-bit Kod Segmenti ? taban=0, limit=4GB, 32-bit, calistirilabilir, okunabilir
    .quad 0x00CF9A000000FFFF
    # 0x10: 32-bit Veri Segmenti ? taban=0, limit=4GB, 32-bit, yazilabilir
    .quad 0x00CF92000000FFFF
    # 0x18: 64-bit Kod Segmenti ? Long Mod kod segmenti (L biti=1)
    .quad 0x00AF9A000000FFFF
    # 0x20: 64-bit Veri Segmenti ? Long Mod veri segmenti
    .quad 0x00CF92000000FFFF
gdt_end:

# GDT Isaretcisi (Pointer) ? `lgdt` komutu bu yapiyi okur
gdt_ptr:
    .word gdt_end - gdt - 1                 # Limit: GDT boyutunun bir eksigi
    .long ap_startup_base + gdt_offset      # Taban: GDT'nin calisma zamani fiziksel adresi

# -- Real Mod Giris Noktasi --
start:
    cli                                     # Kesmeleri kapat ? baslatma tamamlanana kadar
    cld                                     # Yon bayragini temizle (artan bellek erisimi)
    mov al, 0x41                            # 'A' -> QEMU debug port 0xE9'a yaz (hata ayiklama izleme)
    out 0xE9, al

    # Segment kayitlarini sifirla ? Real Mod'da segmentler 0 taban alirsin
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00                          # Stack isaretcisini gecici yigin alanina ayarla

    # GDT taban adresini calisma zamani konumuna gore guncelle (fixup)
    # Derleme zamanindaki adresler gecersiz ? calisma zamaninda 0x1000+offset hesaplanmali
    mov ebx, ap_startup_base
    lea ebx, [ebx + gdt_offset]

    # GDT isaretcisi (gdt_ptr) yapisina erisim icin SI kaydini kullan
    mov si, ap_startup_base
    lea si, [si + gdt_ptr_offset]

    # GDT isaretcisinin 32-bit taban alanini hesaplanan fiziksel adresle guncelle
    mov [si+2], ebx

    # GDT'yi yukle ? bu noktadan sonra segment kayitlari anlamli seciciler kullanabilir
    lgdt [si]

    mov al, 0x42                            # 'B' -> GDT yuklendi
    out 0xE9, al

    # -- Real Mod'dan Protected Mod'a Gecis --
    # CR0'in PE (Protection Enable) bitini 1 yap -> Protected Mod aktif
    mov eax, cr0
    or eax, 1
    mov cr0, eax

    mov al, 0x43                            # 'C' -> CR0.PE=1, Protected Mod etkin
    out 0xE9, al

    # Protected Mod icin uzak atlamayi (far jump) scratch alaninda hazirla
    # Bu yaklasim kodu uzerine YAZMAYI onler (kod modifikasyonunuz onlemi)
    mov ebx, ap_startup_base
    lea ebx, [ebx + protected_mode_offset]

    # Far pointer yapisini scratch alana yaz (GDT girislerinin ustune yazmaktan kacin)
    mov si, ap_startup_base
    lea si, [si + far_ptr_offset]

    # 32-bit offset degerini yaz
    mov [si], ebx
    # Kod secicisi 0x08 yaz (32-bit Kod Segmenti ? GDT'deki 1. giris)
    mov word ptr [si + 4], 0x08

    # m16:32 formatinda uzak atlama yap (segment:offset)
    # 0x66 prefix -> 32-bit operand (Real Mod'da default 16-bit'ti)
    # 0xFF 0x2C  -> JMP m16:32 [SI] ? SI'nin gosterdigi adresten atlama hedefini oku
    .byte 0x66   # Operand boyutu gecersizleme: 32-bit offset kullan
    .byte 0xFF   # JMP m16:32 komutu
    .byte 0x2C   # ModRM: [SI] - SI adresindeki far pointer'i kullan

    # -- 32-bit Protected Mod'da Devam --
    .code32
protected_mode:
    mov al, 0x44                            # 'D' -> Protected Mod'a basariyla gecildi
    out 0xE9, al

    # Veri segmenti secicilerini ayarla: 0x10 = 32-bit Veri Segmenti (GDT'deki 2. giris)
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov fs, ax
    mov gs, ax

    mov ebx, ap_startup_base

    # -- Long Mod (64-bit) Gecisi Hazirligi --

    # CR4.PAE (bit 5) = 1: Fiziksel Adres Uzantisi etkinlestir
    # PAE 4 KB sayfalar yerine 2 MB sayfa destegi saglar ve Long Mod icin ZORUNLUDur
    mov eax, cr4
    or eax, (1 << 5)
    mov cr4, eax

    # CR3'e PML4 (Page Map Level 4) sayfasinin fiziksel adresini yaz
    # PML4, 64-bit sayfalama hiyerarsisinin 4. ve en ust seviyesidir:
    #   PML4 -> PDPT -> PD -> PT -> Fiziksel Sayfa
    mov eax, dword ptr [ebx + ap_startup_data_offset]
    test eax, eax
    jz pml4_error                           # PML4 adresi NULL ise hata yolu
    mov cr3, eax

    # EFER MSR (Extended Feature Enable Register) uzerinden Long Mod ve NX etkinlestir
    # MSR 0xC0000080 = IA32_EFER
    #   Bit 8 (LME): Long Mode Enable ? bu bit page table aktif olunca LMA'ya donusur
    #   Bit 11 (NXE): No-Execute Enable ? calistirma korumasi icin sayfa biti
    mov ecx, 0xC0000080
    rdmsr
    or eax, 0x900  # (1 << 8) = LME | (1 << 11) = NXE
    wrmsr

    # CR0.PG (bit 31) = 1: Sayfalama etkinlestir -> Long Mod Etkin (LMA biti set edilir)
    # CR0.PE (bit 0) = 1 zaten set edilmisti; PG=1 eklenerek 64-bit gecisi tamamlanir
    mov eax, cr0
    or eax, (1 << 31)
    mov cr0, eax

    mov al, 0x45                            # 'E' -> Sayfalama etkin, Long Mod aktif
    out 0xE9, al

    # 64-bit kod segmentine uzak atlamayla gec
    # CS=0x18 (Long Mod Kod Segmenti) ve EIP=long_mode_offset kombinasyonu
    # retf komutu yigindan CS:EIP cifti alarak uzak donus yapar (far return = far jump)
    push 0x18
    lea eax, [ebx + long_mode_offset]
    push eax
    retf

pml4_error:
    mov al, 0x58  # 'X' ? PML4 adresi NULL, hata durumu
    out 0xE9, al
    hlt                                     # CPU'yu durdur ? kurtarma yok

    # -- 64-bit Long Mod'da Devam --
    .code64
long_mode:
    mov al, 0x46                            # 'F' -> Long Mod'a basariyla gecildi
    out 0xE9, al

    # 64-bit veri segmenti secicilerini ayarla: 0x20 = 64-bit Veri Segmenti
    # Long Mod'da CS disindaki segment kayitlari buyuk olcude gormezden gelinir;
    # yine de uyumlu degerler yuklenir
    mov ax, 0x20
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov fs, ax
    mov gs, ax

    # ap_startup_data yapisindan yigin (stack) ust adresini oku ve RSP'ye yukle
    # ap_startup_data duzeni:
    #   +0:  pml4_phys  (u64) ? PML4 sayfasinin fiziksel adresi
    #   +8:  entry      (u64) ? ap_entry() Rust fonksiyonunun sanal adresi
    #   +16: stack_top  (u64) ? Bu AP icin ayrilmis cekirdek yigininin ust adresi
    #   +24: cpu_data   (u64) ? CpuData yapisina isaretci
    mov rbx, ap_startup_base
    mov rsp, [rbx + ap_startup_data_offset + 16]  # RSP = stack_top

    # Yigin adresinin gecerli oldugunu dogrula (NULL kontrolu)
    test rsp, rsp
    jz stack_error                          # Yigin adresi NULL ise hata yolu

    mov al, 0x47                            # 'G' -> Yigin hazir
    out 0xE9, al

    # ap_entry(cpu_data: &'static mut CpuData) icin argumani hazirla
    # SysV x86-64 ABI: ilk integer/pointer arguman RDI kaydinda gecirilir.
    # "extern sysv64" bildirimi sayesinde Windows ve Linux'ta ayni cagri kurali kullanilir.
    mov rdi, [rbx + ap_startup_data_offset + 24]    # RDI = cpu_data isaretcisi

    # Giris noktasi adresini RAX'e yukle ('G' karakteri yazdiktan SONRA ? AL'yi bozmamak icin)
    mov rax, [rbx + ap_startup_data_offset + 8]     # RAX = ap_entry() fonksiyon adresi

    # Rust AP giris noktasini cagir ? bu noktadan sonra Rust kodu yurutulur
    call rax

    # ap_entry() hicbir zaman donmemelidir (! donus turu)
    # Eger bir sekilde donerse CPU'yu guvenli sekilde durdur
    cli                                     # Kesmeleri kapat
    hlt                                     # CPU'yu durdur

stack_error:
    mov al, 0x59  # 'Y' ? Yigin adresi NULL, hata durumu
    out 0xE9, al
    cli                                     # Kesmeleri kapat
    hlt                                     # CPU'yu durdur

# -- AP Baslatma Veri Yapisi --
# BSP bu alani trampoline kopyalanmadan once veya hemen ardindan doldurur.
# Her alan 8 bayt (u64) genisliginde ve 16 bayta hizalanmistir.
.align 16
ap_startup_data:
    .quad 0 # pml4_phys  - Kernelin PML4 tablo fiziksel adresi
    .quad 0 # entry      - ap_entry() Rust fonksiyonunun sanal adresi
    .quad 0 # stack_top  - Bu AP'ye ozel cekirdek yigininin ust adresi
    .quad 0 # cpu_data   - CpuData yapisina isaretci (Rust tarafindan doldurulur)

# Trampolin sayfayi tam 4096 bayta tamamla (geri kalani sifirla)
.fill 4096 - (. - ap_startup_begin), 1, 0
ap_startup_end:
# Onyukleme imzasi ? bazi araclar icin referans noktasi
.word 0xAA55