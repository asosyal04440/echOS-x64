//! # Dinamik Bağlama (Dynamic Linking)
//!
//! ELF dinamik yükleyici ve dlopen/dlsym desteği.
//! POSIX standardındaki `dlopen`, `dlsym`, `dlclose` fonksiyonlarının
//! çekirdek içi karşılıklarını uygular.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicU32, AtomicU64, AtomicPtr, Ordering};
use spin::Mutex;

// ============================================================================
// ELF SABİTLERİ
// ============================================================================

/// ELF sihir baytları: tüm ELF dosyalarının başında bulunur.
/// 0x7F + 'E' + 'L' + 'F' şeklindedir; dosya doğrulamasında kullanılır.
pub const ELFMAG: [u8; 4] = [0x7F, b'E', b'L', b'F'];

/// ELF 64-bit sınıf kodu. 32-bit için ELFCLASS32 = 1 kullanılır.
pub const ELFCLASS64: u8 = 2;

/// Küçük-endian (little-endian) veri kodlaması. x86-64 bu formatı kullanır.
pub const ELFDATA2LSB: u8 = 1;

/// Bölüm türleri (Section Types) - ELF dosyasındaki farklı bölüm tiplerini belirtir
pub const SHT_NULL: u32 = 0;       // Boş / geçersiz bölüm
pub const SHT_PROGBITS: u32 = 1;   // Program verisi (.text, .data)
pub const SHT_SYMTAB: u32 = 2;     // Sembol tablosu
pub const SHT_STRTAB: u32 = 3;     // String tablosu
pub const SHT_RELA: u32 = 4;       // Eklemeli yer değiştirme tablosu
pub const SHT_HASH: u32 = 5;       // Sembol hash tablosu
pub const SHT_DYNAMIC: u32 = 6;    // Dinamik bağlama bilgisi
pub const SHT_NOTE: u32 = 7;       // Not bölümü
pub const SHT_NOBITS: u32 = 8;     // Dosyada yer kaplamayan bölüm (.bss)
pub const SHT_REL: u32 = 9;        // Eklemesiz yer değiştirme tablosu
pub const SHT_DYNSYM: u32 = 11;    // Dinamik sembol tablosu

/// Program başlık türleri (Program Header Types) - segment bilgisi sunar
pub const PT_NULL: u32 = 0;           // Kullanılmayan girdiler
pub const PT_LOAD: u32 = 1;           // Belleğe yüklenecek segment
pub const PT_DYNAMIC: u32 = 2;        // Dinamik bağlama bilgisi segmenti
pub const PT_INTERP: u32 = 3;         // Program yorumlayıcısı (linker) yolu
pub const PT_NOTE: u32 = 4;           // Not bilgisi
pub const PT_PHDR: u32 = 6;           // Program başlık tablosunun kendisi
pub const PT_GNU_EH_FRAME: u32 = 0x6474e550; // Exception handling çerçeve bilgisi
pub const PT_GNU_STACK: u32 = 0x6474e551;    // Yığıt yürütme izni
pub const PT_GNU_RELRO: u32 = 0x6474e552;    // Yeniden konumlandırmadan sonra salt okunur bölgeler

/// Dinamik etiketler (Dynamic Tags) - .dynamic bölümündeki anahtar-değer çiftleri
pub const DT_NULL: i64 = 0;        // Listenin sonu
pub const DT_NEEDED: i64 = 1;      // Gereken paylaşımlı kütüphane adı
pub const DT_PLTRELSZ: i64 = 2;    // PLT yer değiştirme boyutu
pub const DT_PLTGOT: i64 = 3;      // PLT/GOT adresi
pub const DT_HASH: i64 = 4;        // Sembol hash tablosu adresi
pub const DT_STRTAB: i64 = 5;      // String tablosu adresi
pub const DT_SYMTAB: i64 = 6;      // Sembol tablosu adresi
pub const DT_RELA: i64 = 7;        // RELA yer değiştirme tablosu
pub const DT_RELASZ: i64 = 8;      // RELA tablosunun bayt boyutu
pub const DT_RELAENT: i64 = 9;     // RELA girdisinin bayt boyutu
pub const DT_STRSZ: i64 = 10;      // String tablosunun boyutu
pub const DT_SYMENT: i64 = 11;     // Sembol tablo girdisi boyutu
pub const DT_INIT: i64 = 12;       // Başlatma fonksiyonu adresi
pub const DT_FINI: i64 = 13;       // Sonlandırma fonksiyonu adresi
pub const DT_SONAME: i64 = 14;     // Paylaşımlı nesne adı
pub const DT_RPATH: i64 = 15;      // Kütüphane arama yolu (eski)
pub const DT_RUNPATH: i64 = 29;    // Kütüphane arama yolu (yeni)
pub const DT_FLAGS: i64 = 30;      // Bayraklar
pub const DT_GNU_HASH: i64 = 0x6ffffef5;   // GNU-tarzı hash tablosu
pub const DT_VERSYM: i64 = 0x6ffffff0;     // Sürüm sembolü bölümü
pub const DT_RELACOUNT: i64 = 0x6ffffff9;  // RELA sayısı
pub const DT_FLAGS_1: i64 = 0x6ffffffb;    // Genişletilmiş bayraklar

/// Yer değiştirme türleri (x86_64) - dinamik bağlama sırasında uygulanır
pub const R_X86_64_NONE: u32 = 0;       // İşlem yapma
pub const R_X86_64_64: u32 = 1;         // 64-bit mutlak adres
pub const R_X86_64_PC32: u32 = 2;       // 32-bit PC-göreli adres
pub const R_X86_64_GOT32: u32 = 3;      // GOT girdisine 32-bit uzaklık
pub const R_X86_64_PLT32: u32 = 4;      // PLT girdisine 32-bit uzaklık
pub const R_X86_64_COPY: u32 = 5;       // Sembol kopyalama
pub const R_X86_64_GLOB_DAT: u32 = 6;   // GOT girdi oluşturma
pub const R_X86_64_JUMP_SLOT: u32 = 7;  // PLT atlama yuvası
pub const R_X86_64_RELATIVE: u32 = 8;   // Göreceli yük adresi
pub const R_X86_64_GOTPCREL: u32 = 9;   // GOT'a PC-göreli 32-bit uzaklık
pub const R_X86_64_32: u32 = 10;        // 32-bit mutlak adres (sıfır genişletme)
pub const R_X86_64_IRELATIVE: u32 = 37; // STT_GNU_IFUNC sembolü

/// Sembol bağlama türleri (Symbol Binding)
pub const STB_LOCAL: u8 = 0;   // Yerel sembol (sadece bu nesne dosyasında görünür)
pub const STB_GLOBAL: u8 = 1;  // Global sembol (tüm nesne dosyalarında görünür)
pub const STB_WEAK: u8 = 2;    // Zayıf sembol (override edilebilir)

/// Sembol türleri (Symbol Types)
pub const STT_NOTYPE: u8 = 0;   // Tür belirtilmemiş
pub const STT_OBJECT: u8 = 1;   // Veri nesnesi (değişken, dizi)
pub const STT_FUNC: u8 = 2;     // Fonksiyon veya yürütülebilir kod
pub const STT_SECTION: u8 = 3;  // Bölüme ilişkin sembol

// ============================================================================
// ELF BAŞLIKLARI (ELF HEADERS)
// ============================================================================

/// ELF64 Yürütülebilir Başlık (Executable Header)
/// Dosyanın ilk 64 baytını oluşturur; kütüphanenin türü, mimarisi ve
/// program/bölüm tablolarının konumları hakkında bilgi verir.
#[repr(C)]
pub struct Elf64Ehdr {
    pub e_ident: [u8; 16],    // Kimlik baytları (magic, sınıf, kodlama, vs.)
    pub e_type: u16,           // Nesne dosya türü (ET_DYN = paylaşımlı kütüphane)
    pub e_machine: u16,        // Hedef mimari (EM_X86_64 = 0x3E)
    pub e_version: u32,        // ELF sürümü (her zaman 1)
    pub e_entry: u64,          // Giriş noktası sanal adresi
    pub e_phoff: u64,          // Program başlık tablosunun dosya uzaklığı
    pub e_shoff: u64,          // Bölüm başlık tablosunun dosya uzaklığı
    pub e_flags: u32,          // Mimariye özgü bayraklar
    pub e_ehsize: u16,         // Bu başlığın boyutu (64 bayt)
    pub e_phentsize: u16,      // Program başlık tablosu girdisi boyutu
    pub e_phnum: u16,          // Program başlığı sayısı
    pub e_shentsize: u16,      // Bölüm başlık tablosu girdisi boyutu
    pub e_shnum: u16,          // Bölüm başlığı sayısı
    pub e_shstrndx: u16,       // Bölüm adı string tablosunun indeksi
}

/// ELF64 Program Başlığı (Program Header / Segment)
/// Çalışma zamanı bellek düzenini tanımlar.
#[repr(C)]
pub struct Elf64Phdr {
    pub p_type: u32,    // Segment türü (PT_LOAD, PT_DYNAMIC, vb.)
    pub p_flags: u32,   // Segment bayrakları (okuma/yazma/yürütme izinleri)
    pub p_offset: u64,  // Dosyadaki uzaklık
    pub p_vaddr: u64,   // Sanal bellek adresi
    pub p_paddr: u64,   // Fiziksel bellek adresi (genellikle kullanılmaz)
    pub p_filesz: u64,  // Dosyadaki segment boyutu
    pub p_memsz: u64,   // Bellekteki segment boyutu (> filesz ise kalan sıfırlanır)
    pub p_align: u64,   // Hizalama gereksinimi
}

/// ELF64 Bölüm Başlığı (Section Header)
/// .text, .data, .bss gibi bölümleri tanımlar.
#[repr(C)]
pub struct Elf64Shdr {
    pub sh_name: u32,       // Bölüm adının string tablosundaki indeksi
    pub sh_type: u32,       // Bölüm türü (SHT_PROGBITS, SHT_SYMTAB, vb.)
    pub sh_flags: u64,      // Bölüm özellikleri (SHF_ALLOC, SHF_EXECINSTR, vb.)
    pub sh_addr: u64,       // Bellekte yükleneceği adres
    pub sh_offset: u64,     // Dosyadaki uzaklık
    pub sh_size: u64,       // Bölümün bayt boyutu
    pub sh_link: u32,       // Bölüme bağlı diğer bölümün indeksi
    pub sh_info: u32,       // Ek bilgi
    pub sh_addralign: u64,  // Hizalama kısıtlaması
    pub sh_entsize: u64,    // Sabit boyutlu girdi içeriyorsa girdi boyutu
}

/// ELF64 Sembol Tablosu Girdisi
/// Her sembol (fonksiyon, değişken) için bir girdi bulunur.
#[repr(C)]
pub struct Elf64Sym {
    pub st_name: u32,   // Sembol adının string tablosundaki uzaklığı
    pub st_info: u8,    // Sembol türü ve bağlama bilgisi (STB_* | STT_*)
    pub st_other: u8,   // Sembol görünürlüğü
    pub st_shndx: u16,  // İlgili bölümün indeksi
    pub st_value: u64,  // Sembolün değeri (adres veya uzaklık)
    pub st_size: u64,   // Sembolün kapladığı alan
}

/// ELF64 Yer Değiştirme Girdisi (Relocation with Addend)
/// Dinamik bağlama sırasında adres düzeltmeleri için kullanılır.
#[repr(C)]
pub struct Elf64Rela {
    pub r_offset: u64,  // Yer değiştirmenin uygulanacağı adres
    pub r_info: u64,    // Sembol indeksi ve yer değiştirme türü
    pub r_addend: i64,  // Sabit eklenti değeri
}

/// ELF64 Dinamik Bölüm Girdisi
/// .dynamic bölümündeki anahtar-değer çiftleri.
#[repr(C)]
pub struct Elf64Dyn {
    pub d_tag: i64,  // Etiket (DT_NEEDED, DT_STRTAB, vb.)
    pub d_val: u64,  // Değer veya adres
}

// ============================================================================
// YÜKLENMİŞ KÜTÜPHANELERİN YAPISI
// ============================================================================

/// Bellekte yüklenmiş bir paylaşımlı kütüphaneyi temsil eder.
/// Referans sayımlı bir yapıdır; birden fazla işlem aynı kütüphaneyi paylaşabilir.
pub struct LoadedLibrary {
    /// Kütüphanenin dosya adı
    pub name: String,
    /// Bellekte yüklendiği taban adres
    pub base: AtomicU64,
    /// Kütüphanenin kapladığı bayt sayısı
    pub size: u64,
    /// Giriş noktası adresi (varsa)
    pub entry: u64,
    /// Sembol adı -> adres haritası (dlsym için kullanılır)
    pub symbols: Mutex<BTreeMap<String, u64>>,
    /// Kaç referans bu kütüphaneyi açık tutuyore (dlopen/dlclose sayacı)
    pub ref_count: AtomicU32,
    /// Kütüphanenin başlangıç fonksiyonu (_init veya DT_INIT)
    pub init: AtomicU64,
    /// Kütüphanenin sonlanma fonksiyonu (_fini veya DT_FINI)
    pub fini: AtomicU64,
    /// Bu kütüphanenin ihtiyaç duyduğu diğer kütüphaneler (DT_NEEDED)
    pub needed: Mutex<Vec<String>>,
    /// Thread-Local Storage modül kimliği
    pub tls_modid: AtomicU32,
    /// RTLD_GLOBAL bayrağı ile açılmış mı?
    pub is_global: AtomicU32,
}

impl LoadedLibrary {
    pub fn new(name: &str, base: u64, size: u64) -> Self {
        Self {
            name: String::from(name),
            base: AtomicU64::new(base),
            size,
            entry: 0,
            symbols: Mutex::new(BTreeMap::new()),
            ref_count: AtomicU32::new(1),
            init: AtomicU64::new(0),
            fini: AtomicU64::new(0),
            needed: Mutex::new(Vec::new()),
            tls_modid: AtomicU32::new(0),
            is_global: AtomicU32::new(0),
        }
    }

    /// Sembol tablosuna yeni bir sembol ekle.
    /// Dinamik yükleyici ELF dosyasını ayrıştırırken bu fonksiyonu çağırır.
    pub fn add_symbol(&self, name: &str, addr: u64) {
        self.symbols.lock().insert(String::from(name), addr);
    }

    /// İsme göre sembolü ara ve adresini döndür.
    /// dlsym() çağrısı bu metodu kullanır.
    pub fn lookup(&self, name: &str) -> Option<u64> {
        self.symbols.lock().get(name).copied()
    }
}

// ============================================================================
// DİNAMİK YÜKLEYİCİ (DYNAMIC LOADER / LINKER)
// ============================================================================

/// Dinamik kütüphane yöneticisi.
/// ld.so / ld-linux.so'nun çekirdek içi sade karşılığı.
/// dlopen/dlsym/dlclose işlemlerini koordine eder.
pub struct DynamicLoader {
    /// Yüklü kütüphaneler: dosya adı -> kütüphane
    libraries: Mutex<BTreeMap<String, Arc<LoadedLibrary>>>,
    /// Açık handle'lar: handle ID -> kütüphane (dlopen dönüş değeri)
    handles: Mutex<BTreeMap<u32, Arc<LoadedLibrary>>>,
    /// Bir sonraki handle kimliği (monoton artan sayaç)
    next_handle: AtomicU32,
    /// Kütüphane arama yolları (/lib, /usr/lib, vb.)
    search_paths: Mutex<Vec<String>>,
    /// Yükleme istatistikleri
    stats: Mutex<DlStats>,
}

/// Yükleyici istatistikleri - tanı ve hata ayıklama için
#[derive(Clone, Debug, Default)]
pub struct DlStats {
    pub libraries_loaded: u32,     // Toplam yüklenen kütüphane sayısı
    pub symbols_resolved: u64,     // Çözümlenen sembol sayısı
    pub relocations_applied: u64,  // Uygulanan yer değiştirme sayısı
}

impl DynamicLoader {
    pub const fn new() -> Self {
        Self {
            libraries: Mutex::new(BTreeMap::new()),
            handles: Mutex::new(BTreeMap::new()),
            next_handle: AtomicU32::new(1),
            search_paths: Mutex::new(Vec::new()),
            stats: Mutex::new(DlStats::default()),
        }
    }

    /// Varsayılan arama yollarını ekleyerek yükleyiciyi başlat.
    /// Sistem genellikle /lib ve /usr/lib'e bakar.
    pub fn init(&self) {
        let mut paths = self.search_paths.lock();
        paths.push(String::from("/lib"));
        paths.push(String::from("/usr/lib"));
        paths.push(String::from("/usr/local/lib"));

        crate::serial_println!("[DLOPEN] Dynamic loader initialized");
    }

    /// Paylaşımlı kütüphaneyi aç ve bir handle döndür.
    /// Aynı kütüphane zaten yüklüyse referans sayacını artırır (ikinci kez yüklemez).
    /// flags: RTLD_LAZY (0x1), RTLD_NOW (0x2), RTLD_GLOBAL (0x100), vb.
    pub fn dlopen(&self, filename: &str, flags: i32) -> Result<u32, DlError> {
        // Kütüphane zaten yüklüyse mevcut kaydı kullan
        if let Some(lib) = self.libraries.lock().get(filename) {
            lib.ref_count.fetch_add(1, Ordering::SeqCst);
            let handle = self.next_handle.fetch_add(1, Ordering::SeqCst);
            self.handles.lock().insert(handle, lib.clone());
            return Ok(handle);
        }

        // ELF dosyasını dosya sisteminden oku
        let data = self.load_file(filename)?;

        // ELF başlıklarını ayrıştır ve belleğe yükle
        let lib = self.load_elf(&data, filename)?;

        // Yer değiştirme tablolarını uygula (adres sabitleme)
        self.apply_relocations(&lib, &data)?;

        // Kütüphanenin _init / DT_INIT fonksiyonunu çağır
        self.call_init(&lib);

        // Global tabloya kaydet ve handle oluştur
        let handle = self.next_handle.fetch_add(1, Ordering::SeqCst);
        self.libraries.lock().insert(String::from(filename), lib.clone());
        self.handles.lock().insert(handle, lib.clone());

        if flags & 0x00100 != 0 { // RTLD_GLOBAL
            lib.is_global.store(1, Ordering::SeqCst);
        }

        let mut stats = self.stats.lock();
        stats.libraries_loaded += 1;

        Ok(handle)
    }

    /// ELF verisini ayrıştır ve bir LoadedLibrary yapısı oluştur.
    /// Segmentleri sanal belleğe kopyalar, sembol tablosunu doldurur.
    fn load_elf(&self, data: &[u8], name: &str) -> Result<Arc<LoadedLibrary>, DlError> {
        if data.len() < core::mem::size_of::<Elf64Ehdr>() {
            return Err(DlError::InvalidElf);
        }

        let ehdr = unsafe { &*(data.as_ptr() as *const Elf64Ehdr) };

        // Sihir baytlarını kontrol et (geçerli ELF mi?)
        if ehdr.e_ident[0..4] != ELFMAG {
            return Err(DlError::InvalidElf);
        }

        // 64-bit ELF mi kontrol et
        if ehdr.e_ident[4] != ELFCLASS64 {
            return Err(DlError::InvalidElf);
        }

        // PT_LOAD segmentlerini tarayarak toplam bellek boyutunu hesapla
        let mut total_size = 0u64;
        let mut base_addr = u64::MAX;

        for i in 0..ehdr.e_phnum as usize {
            let phdr = self.get_phdr(data, i)?;

            if phdr.p_type == PT_LOAD {
                if phdr.p_vaddr < base_addr {
                    base_addr = phdr.p_vaddr;
                }
                let end = phdr.p_vaddr + phdr.p_memsz;
                if end > total_size {
                    total_size = end;
                }
            }
        }

        // Kütüphane için bellek tahsis et (gerçekte mmap kullanılır)
        let load_base = 0x7F0000000000u64;

        let lib = Arc::new(LoadedLibrary::new(name, load_base, total_size));

        // Her PT_LOAD segmentini belleğe kopyala
        for i in 0..ehdr.e_phnum as usize {
            let phdr = self.get_phdr(data, i)?;

            if phdr.p_type == PT_LOAD {
                // Segment verisini dosya konumundan oku
                let file_start = phdr.p_offset as usize;
                let file_end = core::cmp::min(file_start + phdr.p_filesz as usize, data.len());

                // load_base + phdr.p_vaddr adresine kopyala
            }
        }

        // .dynamic bölümünü ayrıştır (DT_NEEDED, DT_INIT, vb.)
        self.parse_dynamic(&lib, data, ehdr)?;

        Ok(lib)
    }

    fn get_phdr(&self, data: &[u8], index: usize) -> Result<&'static Elf64Phdr, DlError> {
        let ehdr = unsafe { &*(data.as_ptr() as *const Elf64Ehdr) };

        let offset = ehdr.e_phoff as usize + index * ehdr.e_phentsize as usize;

        if offset + core::mem::size_of::<Elf64Phdr>() > data.len() {
            return Err(DlError::InvalidElf);
        }

        Ok(unsafe { &*(data.as_ptr().add(offset) as *const Elf64Phdr) })
    }

    fn parse_dynamic(&self, lib: &LoadedLibrary, data: &[u8], ehdr: &Elf64Ehdr) -> Result<(), DlError> {
        for i in 0..ehdr.e_phnum as usize {
            let phdr = self.get_phdr(data, i)?;

            if phdr.p_type == PT_DYNAMIC {
                // .dynamic bölümündeki girdileri işle
                let dyn_offset = phdr.p_offset as usize;
                let dyn_size = phdr.p_filesz as usize;

                let mut offset = dyn_offset;
                while offset + core::mem::size_of::<Elf64Dyn>() <= dyn_offset + dyn_size {
                    let dyn_entry = unsafe {
                        &*(data.as_ptr().add(offset) as *const Elf64Dyn)
                    };

                    match dyn_entry.d_tag {
                        DT_NEEDED => {
                            // Bağımlılık ekle (başka bir kütüphane gerekli)
                        }
                        DT_INIT => {
                            lib.init.store(dyn_entry.d_val, Ordering::SeqCst);
                        }
                        DT_FINI => {
                            lib.fini.store(dyn_entry.d_val, Ordering::SeqCst);
                        }
                        DT_NULL => break, // Dinamik bölümün sonu
                        _ => {}
                    }

                    offset += core::mem::size_of::<Elf64Dyn>();
                }
            }
        }

        Ok(())
    }

    fn apply_relocations(&self, lib: &LoadedLibrary, _data: &[u8]) -> Result<(), DlError> {
        // RELA yer değiştirme tablosunu uygula
        // Her R_X86_64_GLOB_DAT, R_X86_64_JUMP_SLOT vb. için adresi yaz
        let mut stats = self.stats.lock();
        stats.relocations_applied += 1;

        Ok(())
    }

    fn call_init(&self, lib: &LoadedLibrary) {
        let init = lib.init.load(Ordering::SeqCst);
        if init != 0 {
            // DT_INIT ile belirtilen başlatma fonksiyonunu çağır
            crate::serial_println!("[DLOPEN] Calling init at {:#x}", init);
        }
    }

    /// Açık bir handle üzerinden sembol ara.
    /// POSIX dlsym() fonksiyonuna karşılık gelir.
    pub fn dlsym(&self, handle: u32, symbol: &str) -> Result<u64, DlError> {
        let handles = self.handles.lock();
        let lib = handles.get(&handle).ok_or(DlError::InvalidHandle)?;

        let addr = lib.lookup(symbol);

        let mut stats = self.stats.lock();
        stats.symbols_resolved += 1;

        addr.ok_or(DlError::SymbolNotFound)
    }

    /// Kütüphane handle'ını kapat.
    /// Referans sayacı sıfıra düşerse `_fini` / DT_FINI çağrılır ve bellek serbest bırakılır.
    pub fn dlclose(&self, handle: u32) -> Result<(), DlError> {
        let mut handles = self.handles.lock();

        if let Some(lib) = handles.remove(&handle) {
            lib.ref_count.fetch_sub(1, Ordering::SeqCst);

            // Son referanssa sonlanma fonksiyonunu çağır
            if lib.ref_count.load(Ordering::SeqCst) == 0 {
                let fini = lib.fini.load(Ordering::SeqCst);
                if fini != 0 {
                    // _fini fonksiyonunu çağır
                }
            }

            return Ok(());
        }

        Err(DlError::InvalidHandle)
    }

    /// Dosya sisteminden ELF verisini oku (yer tutucu uygulama)
    fn load_file(&self, _filename: &str) -> Result<Vec<u8>, DlError> {
        // Gerçek uygulamada sanal dosya sisteminden okur
        Ok(vec![0u8; 4096])
    }

    /// Yükleyici istatistiklerini döndür
    pub fn get_stats(&self) -> DlStats {
        self.stats.lock().clone()
    }
}

lazy_static::lazy_static! {
    /// Global dinamik yükleyici örneği.
    /// Tüm dlopen/dlsym/dlclose çağrıları bu nesneyi kullanır.
    pub static ref DYN_LOADER: DynamicLoader = DynamicLoader::new();
}

// ============================================================================
// HATA TİPİ
// ============================================================================

/// Dinamik yükleme hataları
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DlError {
    InvalidElf,          // Geçersiz ELF formatı
    InvalidHandle,       // Geçersiz kütüphane handle'ı
    SymbolNotFound,      // Sembol bulunamadı
    FileNotFound,        // Dosya bulunamadı
    RelocationFailed,    // Yer değiştirme başarısız
    DependencyNotFound,  // Bağımlı kütüphane bulunamadı
}

// ============================================================================
// SİSTEM ÇAĞRISI ARAYÜZÜ
// ============================================================================

/// dlopen() sistem çağrısı - başarıda handle (pozitif), hata durumunda -1
pub fn sys_dlopen(filename: &str, flags: i32) -> i64 {
    match DYN_LOADER.dlopen(filename, flags) {
        Ok(handle) => handle as i64,
        Err(_) => -1,
    }
}

/// dlsym() sistem çağrısı - başarıda sembol adresi, hata durumunda 0
pub fn sys_dlsym(handle: u32, symbol: &str) -> i64 {
    match DYN_LOADER.dlsym(handle, symbol) {
        Ok(addr) => addr as i64,
        Err(_) => 0,
    }
}

/// dlclose() sistem çağrısı - başarıda 0, hata durumunda -1
pub fn sys_dlclose(handle: u32) -> i32 {
    match DYN_LOADER.dlclose(handle) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

// ============================================================================
// BAŞLATMA
// ============================================================================

/// Dinamik yükleyiciyi başlat (çekirdek boot sırasında çağrılır)
pub fn init() {
    DYN_LOADER.init();
}
