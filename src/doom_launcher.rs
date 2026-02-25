//! # echOS Doom Downloader & Launcher
//!
//! Downloads Doom shareware WAD and launches the game

use alloc::vec::Vec;
use alloc::vec;
use alloc::string::String;
use alloc::string::ToString;
use alloc::boxed::Box;
use spin::Mutex;

// ============================================================================
// DOOM URLs
// ============================================================================

/// Doom shareware WAD URL (DOOM1.WAD)
const DOOM_SHAREWARE_URL: &str = "http://distro.ibiblio.org/slitaz/sources/packages/d/doom1.wad";

/// Alternative Doom WAD mirror
const DOOM_MIRROR_URL: &str = "http://ftp.gwdg.de/pub/misc/idsoftware/idstuff/doom/doom1.wad";

/// Doom WAD file name
const DOOM_WAD_FILENAME: &str = "doom1.wad";

/// Expected WAD size (shareware)
const DOOM_SHAREWARE_SIZE: usize = 4_196_020;

// ============================================================================
// WAD HEADER
// ============================================================================

/// WAD file header
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct WadHeader {
    pub identification: [u8; 4],  // "IWAD" or "PWAD"
    pub num_lumps: u32,
    pub info_table_offset: u32,
}

impl WadHeader {
    /// Check if valid WAD
    pub fn is_valid(&self) -> bool {
        self.identification == *b"IWAD" || self.identification == *b"PWAD"
    }
    
    /// Check if is IWAD (official)
    pub fn is_iwad(&self) -> bool {
        self.identification == *b"IWAD"
    }
    
    /// Check if is PWAD (patch)
    pub fn is_pwad(&self) -> bool {
        self.identification == *b"PWAD"
    }
}

/// WAD lump entry
#[repr(C, packed)]
#[derive(Clone, Copy, Debug)]
pub struct WadLumpEntry {
    pub offset: u32,
    pub size: u32,
    pub name: [u8; 8],
}

impl WadLumpEntry {
    /// Get lump name as string
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
// WAD LOADER
// ============================================================================

/// Loaded WAD file
#[derive(Clone, Debug)]
pub struct WadFile {
    pub data: Vec<u8>,
    pub header: WadHeader,
    pub lumps: Vec<WadLumpEntry>,
    pub filename: String,
}

impl WadFile {
    /// Parse WAD from data
    pub fn parse(data: Vec<u8>, filename: &str) -> Option<Self> {
        if data.len() < core::mem::size_of::<WadHeader>() {
            return None;
        }
        
        // Parse header
        let header = unsafe {
            core::ptr::read(data.as_ptr() as *const WadHeader)
        };
        
        if !header.is_valid() {
            crate::serial_println!("[WAD] Invalid WAD header");
            return None;
        }
        
        // Parse lump table
        let lump_table_offset = header.info_table_offset as usize;
        let lump_size = core::mem::size_of::<WadLumpEntry>();
        let num_lumps = header.num_lumps as usize;
        
        let mut lumps = Vec::with_capacity(num_lumps);
        
        for i in 0..num_lumps {
            let offset = lump_table_offset + i * lump_size;
            if offset + lump_size > data.len() {
                break;
            }
            
            let entry = unsafe {
                core::ptr::read(data.as_ptr().add(offset) as *const WadLumpEntry)
            };
            
            lumps.push(entry);
        }
        
        crate::serial_println!("[WAD] Loaded {} lumps from {}", num_lumps, filename);
        
        Some(WadFile {
            data,
            header,
            lumps,
            filename: filename.to_string(),
        })
    }
    
    /// Get lump by name
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
    
    /// Get lump data by index
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
    
    /// Find lump index by name
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
// DOOM LAUNCHER
// ============================================================================

/// Doom launcher state
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DoomLauncherState {
    Idle,
    Downloading,
    DownloadComplete,
    Loading,
    Running,
    Error(String),
}

/// Doom launcher
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
    
    /// Check if WAD exists locally
    pub fn check_local_wad(&mut self) -> bool {
        // Try to load from filesystem
        // In real implementation, would check /games/doom/doom1.wad
        false
    }
    
    /// Download Doom shareware WAD
    pub fn download_wad(&mut self) -> Result<(), String> {
        self.state = DoomLauncherState::Downloading;
        self.download_size = DOOM_SHAREWARE_SIZE;
        self.download_progress = 0;
        
        crate::serial_println!("[DOOM] Downloading WAD from {}", DOOM_SHAREWARE_URL);
        
        // Use HTTP client to download
        let client = crate::net::http::HttpClient::new();
        
        match client.download(DOOM_SHAREWARE_URL) {
            Ok(data) => {
                crate::serial_println!("[DOOM] Downloaded {} bytes", data.len());
                
                // Parse WAD
                if let Some(wad) = WadFile::parse(data, DOOM_WAD_FILENAME) {
                    self.wad = Some(wad);
                    self.state = DoomLauncherState::DownloadComplete;
                    Ok(())
                } else {
                    self.state = DoomLauncherState::Error("Invalid WAD file".to_string());
                    Err("Invalid WAD file".to_string())
                }
            }
            Err(e) => {
                let err_msg = alloc::format!("Download failed: {:?}", e);
                crate::serial_println!("[DOOM] {}", err_msg);
                self.state = DoomLauncherState::Error(err_msg.clone());
                Err(err_msg)
            }
        }
    }
    
    /// Load WAD from filesystem
    pub fn load_wad(&mut self, path: &str) -> Result<(), String> {
        self.state = DoomLauncherState::Loading;
        
        // In real implementation, would load from filesystem
        // For now, just return error
        let err = "Filesystem not available".to_string();
        self.state = DoomLauncherState::Error(err.clone());
        Err(err)
    }
    
    /// Launch Doom
    pub fn launch(&mut self) -> Result<(), String> {
        if self.wad.is_none() {
            return Err("No WAD loaded".to_string());
        }
        
        self.state = DoomLauncherState::Running;
        
        // Initialize Doom engine
        if !crate::doom::init_doom() {
            self.state = DoomLauncherState::Error("Failed to initialize Doom".to_string());
            return Err("Failed to initialize Doom".to_string());
        }
        
        crate::serial_println!("[DOOM] Game launched");
        Ok(())
    }
    
    /// Stop Doom
    pub fn stop(&mut self) {
        crate::doom::shutdown_doom();
        self.state = DoomLauncherState::Idle;
    }
    
    /// Get state
    pub fn state(&self) -> &DoomLauncherState {
        &self.state
    }
    
    /// Get download progress (0-100)
    pub fn download_progress(&self) -> usize {
        if self.download_size == 0 {
            return 0;
        }
        (self.download_progress * 100) / self.download_size
    }
    
    /// Get WAD
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
// GLOBAL LAUNCHER
// ============================================================================

static DOOM_LAUNCHER: Mutex<DoomLauncher> = Mutex::new(DoomLauncher {
    state: DoomLauncherState::Idle,
    wad: None,
    download_progress: 0,
    download_size: 0,
});

/// Initialize launcher
pub fn init() {
    crate::serial_println!("[DOOM] Launcher initialized");
}

/// Download and launch Doom
pub fn download_and_launch() -> Result<(), String> {
    let mut launcher = DOOM_LAUNCHER.lock();
    
    // Check if already downloaded
    if launcher.wad.is_some() {
        return launcher.launch();
    }
    
    // Download WAD
    launcher.download_wad()?;
    
    // Launch
    launcher.launch()
}

/// Get launcher state
pub fn get_state() -> DoomLauncherState {
    DOOM_LAUNCHER.lock().state.clone()
}

/// Stop game
pub fn stop() {
    DOOM_LAUNCHER.lock().stop();
}

/// Get WAD lump
pub fn get_wad_lump(name: &str) -> Option<Vec<u8>> {
    let launcher = DOOM_LAUNCHER.lock();
    if let Some(wad) = &launcher.wad {
        wad.get_lump(name).map(|s| s.to_vec())
    } else {
        None
    }
}

// ============================================================================
// CLI COMMAND
// ============================================================================

/// CLI command: doom
pub fn cmd_doom(args: &[&str]) -> String {
    if args.is_empty() {
        return "Usage: doom [download|launch|stop|status]\n".to_string();
    }
    
    match args[0] {
        "download" => {
            match download_and_launch() {
                Ok(()) => "Doom downloaded and launched!\n".to_string(),
                Err(e) => alloc::format!("Error: {}\n", e),
            }
        }
        "launch" => {
            let mut launcher = DOOM_LAUNCHER.lock();
            match launcher.launch() {
                Ok(()) => "Doom launched!\n".to_string(),
                Err(e) => alloc::format!("Error: {}\n", e),
            }
        }
        "stop" => {
            stop();
            "Doom stopped.\n".to_string()
        }
        "status" => {
            let state = get_state();
            alloc::format!("Doom status: {:?}\n", state)
        }
        _ => "Unknown command. Use: download, launch, stop, status\n".to_string(),
    }
}
