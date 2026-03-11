//! # echOS Doom İndirici ve Başlatıcı
//!
//! Doom shareware WAD dosyasını indirir ve oyunu başlatır.
//!
//! ## WAD Dosyası Nedir?
//! WAD (Where's All the Data?), Doom'un tüm oyun verilerini
//! (haritalar, sesler, görseller) depolayan özel dosya formatıdır.
//! IWAD (Internal WAD): Resmi id Software dosyası.
//! PWAD (Patch WAD): Kullanıcı tarafından oluşturulan ek içerik.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

// ============================================================================
// DOOM URL'LERİ
// ============================================================================

/// Doom shareware WAD indirme URL'si (DOOM1.WAD)
const DOOM_SHAREWARE_URL: &str = "http://distro.ibiblio.org/slitaz/sources/packages/d/doom1.wad";

/// Alternatif Doom WAD aynası (sunucu erişilemez olduğunda kullanılır)
const DOOM_MIRROR_URL: &str = "http://ftp.gwdg.de/pub/misc/idsoftware/idstuff/doom/doom1.wad";

/// Doom WAD dosya adı
const DOOM_WAD_FILENAME: &str = "doom1.wad";

/// Shareware sürümünün beklenen dosya boyutu (bayt cinsinden)
const DOOM_SHAREWARE_SIZE: usize = 4_196_020;

// ============================================================================
// WAD BAŞLIĞI
// ============================================================================

/// WAD dosya başlığı.
/// packed repr: C derleyicisi gibi bellek hizalaması — dosyadaki ham baytlarla
/// doğrudan eşleme için zorunludur.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct WadHeader {
    pub identification: [u8; 4], // "IWAD" veya "PWAD" — dosya türünü belirler
    pub num_lumps: u32,
    pub info_table_offset: u32,
}

impl WadHeader {
    /// WAD başlığının geçerli olup olmadığını kontrol eder.
    /// Geçerli bir WAD ya IWAD ya da PWAD olmalıdır.
    pub fn is_valid(&self) -> bool {
        self.identification == *b"IWAD" || self.identification == *b"PWAD"
    }

    /// Resmi (official) IWAD olup olmadığını kontrol eder.
    pub fn is_iwad(&self) -> bool {
        self.identification == *b"IWAD"
    }

    /// Yama (patch) PWAD olup olmadığını kontrol eder.
    pub fn is_pwad(&self) -> bool {
        self.identification == *b"PWAD"
    }
}

/// WAD lump (veri parçası) dizin girişi.
/// Her lump; ofset, boyut ve isim bilgisi içerir.
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct WadLumpEntry {
    pub offset: u32,
    pub size: u32,
    pub name: [u8; 8],
}

impl WadLumpEntry {
    /// Lump adını null-sonlandırmalı bayt dizisinden String'e dönüştürür.
    pub fn name_as_string(&self) -> String {
        let mut name = String::new();
        for &b in &self.name {
            if b == 0 {
                break;
            }
            name.push(b as char);
        }
        name
    }
}

// ============================================================================
// WAD YÜKLEYİCİ
// ============================================================================

/// Belleğe yüklenmiş WAD dosyası.
/// Başlık, lump dizini ve ham veri tamponunu bir arada tutar.
#[derive(Clone, Debug)]
pub struct WadFile {
    pub data: Vec<u8>,
    pub header: WadHeader,
    pub lumps: Vec<WadLumpEntry>,
    pub filename: String,
}

impl WadFile {
    /// Ham veri dizisini WAD olarak ayrıştırır (parse eder).
    ///
    /// Ayrıştırma akışı:
    /// ```text
    /// Ham veri tamponu
    ///   ├── Başlık oku (ilk 12 bayt)
    ///   │     ├── Geçerlilik kontrolü ("IWAD"/"PWAD")
    ///   │     └── Lump tablosu ofsetini al
    ///   └── Lump tablosunu oku
    ///         ├── Her giriş için ofset, boyut, isim oku
    ///         └── Vec<WadLumpEntry> olarak sakla
    /// ```
    pub fn parse(data: Vec<u8>, filename: &str) -> Option<Self> {
        if data.len() < core::mem::size_of::<WadHeader>() {
            return None;
        }

        // Başlığı ayrıştır — unsafe: C-benzeri ham bellek okuma
        let header = unsafe { core::ptr::read(data.as_ptr() as *const WadHeader) };

        if !header.is_valid() {
            crate::serial_println!("[WAD] Geçersiz WAD başlığı");
            return None;
        }

        // Lump tablosunu ayrıştır
        let lump_table_offset = header.info_table_offset as usize;
        let lump_size = core::mem::size_of::<WadLumpEntry>();
        let num_lumps = header.num_lumps as usize;

        let mut lumps = Vec::with_capacity(num_lumps);

        for i in 0..num_lumps {
            let offset = lump_table_offset + i * lump_size;
            if offset + lump_size > data.len() {
                break;
            }

            let entry =
                unsafe { core::ptr::read(data.as_ptr().add(offset) as *const WadLumpEntry) };

            lumps.push(entry);
        }

        crate::serial_println!("[WAD] {}'dan {} lump yüklendi", filename, num_lumps);

        Some(WadFile {
            data,
            header,
            lumps,
            filename: filename.to_string(),
        })
    }

    /// İsme göre lump verisini döndürür.
    pub fn get_lump(&self, name: &str) -> Option<&[u8]> {
        for lump in &self.lumps {
            if lump.name_as_string() == name {
                let start = lump.offset as usize;
                let end = start + lump.size as usize;
                if end <= self.data.len() {
                    return Some(&self.data[start..end]);
                }
            }
        }
        None
    }

    /// Dizin sırasına göre lump verisini döndürür.
    pub fn get_lump_by_index(&self, index: usize) -> Option<&[u8]> {
        if index >= self.lumps.len() {
            return None;
        }

        let lump = &self.lumps[index];
        let start = lump.offset as usize;
        let end = start + lump.size as usize;

        if end <= self.data.len() {
            Some(&self.data[start..end])
        } else {
            None
        }
    }

    /// İsme göre lump dizin numarasını bulur.
    pub fn find_lump(&self, name: &str) -> Option<usize> {
        for (i, lump) in self.lumps.iter().enumerate() {
            if lump.name_as_string() == name {
                return Some(i);
            }
        }
        None
    }
}

// ============================================================================
// DOOM BAŞLATICI
// ============================================================================

/// Doom başlatıcı durum makinesi.
/// İndirme → Yükleme → Çalışma akışını yönetir.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DoomLauncherState {
    Idle,
    Downloading,
    DownloadComplete,
    Loading,
    Running,
    Error(String),
}

/// Doom başlatıcı.
/// WAD indirme, yükleme ve oyun başlatma işlemlerini yönetir.
pub struct DoomLauncher {
    state: DoomLauncherState,
    wad: Option<WadFile>,
    download_progress: usize,
    download_size: usize,
}

impl DoomLauncher {
    pub fn new() -> Self {
        DoomLauncher {
            state: DoomLauncherState::Idle,
            wad: None,
            download_progress: 0,
            download_size: 0,
        }
    }

    /// Yerel dosya sisteminde WAD dosyasının bulunup bulunmadığını kontrol eder.
    pub fn check_local_wad(&mut self) -> bool {
        // Gerçek uygulamada /games/doom/doom1.wad kontrol edilir
        false
    }

    /// Doom shareware WAD dosyasını HTTP üzerinden indirir.
    pub fn download_wad(&mut self) -> Result<(), String> {
        self.state = DoomLauncherState::Downloading;
        self.download_size = DOOM_SHAREWARE_SIZE;
        self.download_progress = 0;

        crate::serial_println!("[DOOM] WAD indiriliyor: {}", DOOM_SHAREWARE_URL);

        // HTTP istemcisi ile indir
        let client = crate::net::http::HttpClient::new();

        match client.download(DOOM_SHAREWARE_URL) {
            Ok(data) => {
                crate::serial_println!("[DOOM] {} bayt indirildi", data.len());

                // WAD'ı ayrıştır ve doğrula
                if let Some(wad) = WadFile::parse(data, DOOM_WAD_FILENAME) {
                    self.wad = Some(wad);
                    self.state = DoomLauncherState::DownloadComplete;
                    Ok(())
                } else {
                    self.state = DoomLauncherState::Error("Geçersiz WAD dosyası".to_string());
                    Err("Geçersiz WAD dosyası".to_string())
                }
            }
            Err(e) => {
                let err_msg = alloc::format!("İndirme başarısız: {:?}", e);
                crate::serial_println!("[DOOM] {}", err_msg);
                self.state = DoomLauncherState::Error(err_msg.clone());
                Err(err_msg)
            }
        }
    }

    /// Belirtilen yoldan WAD dosyasını yükler.
    pub fn load_wad(&mut self, path: &str) -> Result<(), String> {
        self.state = DoomLauncherState::Loading;

        // Gerçek uygulamada dosya sisteminden yüklenır.
        // Şimdilik hata döner
        let err = "Dosya sistemi kullanılamıyor".to_string();
        self.state = DoomLauncherState::Error(err.clone());
        Err(err)
    }

    /// Doom oyununu başlatır.
    /// WAD yüklü değilse hata döner.
    pub fn launch(&mut self) -> Result<(), String> {
        if self.wad.is_none() {
            return Err("WAD yüklenmedi".to_string());
        }

        self.state = DoomLauncherState::Running;

        // Doom motorunu başlat
        if !crate::doom::init_doom() {
            self.state = DoomLauncherState::Error("Doom başlatılamadı".to_string());
            return Err("Doom başlatılamadı".to_string());
        }

        crate::serial_println!("[DOOM] Oyun başlatıldı");
        Ok(())
    }

    /// Doom oyununu durdurur.
    pub fn stop(&mut self) {
        crate::doom::shutdown_doom();
        self.state = DoomLauncherState::Idle;
    }

    /// Mevcut başlatıcı durumunu döndürür.
    pub fn state(&self) -> &DoomLauncherState {
        &self.state
    }

    /// İndirme ilerleme yüzdesini (0-100) döndürür.
    pub fn download_progress(&self) -> usize {
        if self.download_size == 0 {
            return 0;
        }
        (self.download_progress * 100) / self.download_size
    }

    /// Yüklü WAD referansını döndürür.
    pub fn wad(&self) -> Option<&WadFile> {
        self.wad.as_ref()
    }
}

impl Default for DoomLauncher {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// GLOBAL BAŞLATICI
// ============================================================================

/// Global Doom başlatıcısı — Mutex ile güvenli çok iş parçacıklı erişim.
static DOOM_LAUNCHER: Mutex<DoomLauncher> = Mutex::new(DoomLauncher {
    state: DoomLauncherState::Idle,
    wad: None,
    download_progress: 0,
    download_size: 0,
});

/// Başlatıcıyı hazırlar.
pub fn init() {
    crate::serial_println!("[DOOM] Başlatıcı hazır");
}

/// WAD dosyasını indirir ve oyunu başlatır.
/// WAD zaten varsa doğrudan başlatır.
pub fn download_and_launch() -> Result<(), String> {
    let mut launcher = DOOM_LAUNCHER.lock();

    // Zaten indirilmiş mi kontrol et
    if launcher.wad.is_some() {
        return launcher.launch();
    }

    // WAD'ı indir
    launcher.download_wad()?;

    // Başlat
    launcher.launch()
}

/// Başlatıcı durumunu döndürür.
pub fn get_state() -> DoomLauncherState {
    DOOM_LAUNCHER.lock().state.clone()
}

/// Oyunu durdurur.
pub fn stop() {
    DOOM_LAUNCHER.lock().stop();
}

/// Belirtilen isimde WAD lump verisini döndürür.
pub fn get_wad_lump(name: &str) -> Option<Vec<u8>> {
    let launcher = DOOM_LAUNCHER.lock();
    if let Some(wad) = &launcher.wad {
        wad.get_lump(name).map(|s| s.to_vec())
    } else {
        None
    }
}

// ============================================================================
// KOMUTSATIRı KOMUTU
// ============================================================================

/// `doom` shell komutunu işler.
/// Kullanım: doom [download|launch|stop|status]
pub fn cmd_doom(args: &[&str]) -> String {
    if args.is_empty() {
        return "Kullanım: doom [download|launch|stop|status]\n".to_string();
    }

    match args[0] {
        "download" => match download_and_launch() {
            Ok(()) => "Doom indirildi ve başlatıldı!\n".to_string(),
            Err(e) => alloc::format!("Hata: {}\n", e),
        },
        "launch" => {
            let mut launcher = DOOM_LAUNCHER.lock();
            match launcher.launch() {
                Ok(()) => "Doom başlatıldı!\n".to_string(),
                Err(e) => alloc::format!("Hata: {}\n", e),
            }
        }
        "stop" => {
            stop();
            "Doom durduruldu.\n".to_string()
        }
        "status" => {
            let state = get_state();
            alloc::format!("Doom durumu: {:?}\n", state)
        }
        _ => "Bilinmeyen komut. Kullanın: download, launch, stop, status\n".to_string(),
    }
}
