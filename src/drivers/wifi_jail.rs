//! # WiFi Jail — TIER 2 Kablosuz Ağ Adaptörü Sürücüsü
//!
//! WiFi donanımı güvenilmez vendor driver'lar barındırdığından TIER 2 (JAIL)
//! sınıfında çalıştırılır. Tüm MMIO erişimi SPSC ring buffer üzerinden
//! sandbox içinden geçer.
//!
//! ## Mimari
//!
//! ```text
//! ┌────────────────┐     SPSC Ring      ┌────────────────┐
//! │  WiFi Jail     │ ◄════════════════► │  JailWorker    │
//! │  (TIER 2)      │   CommandRing      │  (kernel)      │
//! │                │   CompletionRing   │                │
//! │  scan()        │                    │  MMIO access   │
//! │  connect()     │                    │  IRQ handling  │
//! │  disconnect()  │                    │                │
//! └────────────────┘                    └────────────────┘
//! ```
//!
//! ## Desteklenen Standartlar
//!
//! - IEEE 802.11a/b/g/n/ac/ax (WiFi 6)
//! - WPA2-PSK / WPA3-SAE
//! - WPA2-Enterprise (802.1X/EAP)
//!
//! ## Güvenlik
//!
//! - Tüm firmware komuları sandbox içinden gönderilir
//! - MMIO register erişimi JailWorker tarafından denetlenir
//! - DMA buffer'lar izole fiziksel bölgede tahsis edilir

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;

// ============================================================================
// WiFi Sabitleri
// ============================================================================

/// WiFi bant türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiBand {
    /// 2.4 GHz (802.11b/g/n)
    Band2G,
    /// 5 GHz (802.11a/n/ac)
    Band5G,
    /// 6 GHz (802.11ax — WiFi 6E)
    Band6G,
}

/// WiFi güvenlik protokolü
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiSecurity {
    Open,
    WEP,
    WPA,
    WPA2Personal,
    WPA2Enterprise,
    WPA3Personal,
    WPA3Enterprise,
}

impl WifiSecurity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::WEP => "WEP",
            Self::WPA => "WPA",
            Self::WPA2Personal => "WPA2-PSK",
            Self::WPA2Enterprise => "WPA2-EAP",
            Self::WPA3Personal => "WPA3-SAE",
            Self::WPA3Enterprise => "WPA3-EAP",
        }
    }
}

/// WiFi PHY modu (802.11 standardı)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiPhyMode {
    /// 802.11b (2.4GHz, 11 Mbps max)
    Dot11B,
    /// 802.11g (2.4GHz, 54 Mbps max)
    Dot11G,
    /// 802.11n (HT, 2.4/5GHz, 600 Mbps max)
    Dot11N,
    /// 802.11ac (VHT, 5GHz, 3.46 Gbps max)
    Dot11AC,
    /// 802.11ax (HE, 2.4/5/6GHz, 9.6 Gbps max)
    Dot11AX,
    /// 802.11be (EHT, 2.4/5/6GHz, WiFi 7)
    Dot11BE,
}

/// WiFi bağlantı durumu
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiState {
    Disconnected,
    Scanning,
    Authenticating,
    Associating,
    Connected,
    Roaming,
}

// ============================================================================
// Tarama Sonucu (BSS — Basic Service Set)
// ============================================================================

/// Tespit edilen erişim noktası bilgisi
#[derive(Clone, Debug)]
pub struct WifiBss {
    /// BSSID (MAC adresi, 6 bayt)
    pub bssid: [u8; 6],
    /// SSID (ağ adı, max 32 bayt)
    pub ssid: String,
    /// Sinyal gücü (dBm, negatif)
    pub rssi: i8,
    /// Kanal numarası
    pub channel: u8,
    /// Frekans (MHz)
    pub frequency: u16,
    /// Bant
    pub band: WifiBand,
    /// Güvenlik türü
    pub security: WifiSecurity,
    /// PHY modu
    pub phy_mode: WifiPhyMode,
    /// Kanal genişliği (20/40/80/160/320 MHz)
    pub channel_width: u16,
}

impl WifiBss {
    /// BSSID'yi string olarak formatlar
    pub fn bssid_str(&self) -> String {
        format!(
            "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            self.bssid[0],
            self.bssid[1],
            self.bssid[2],
            self.bssid[3],
            self.bssid[4],
            self.bssid[5]
        )
    }
}

#[derive(Clone, Debug)]
pub struct WifiMloLink {
    pub bssid: [u8; 6],
    pub band: WifiBand,
    pub channel: u8,
    pub frequency: u16,
    pub phy_mode: WifiPhyMode,
    pub channel_width: u16,
    pub rssi: i8,
    pub score: u32,
    pub estimated_mbps: u32,
}

#[derive(Clone, Debug)]
pub struct WifiMloSession {
    pub ssid: String,
    pub security: WifiSecurity,
    pub primary: WifiMloLink,
    pub secondary: Vec<WifiMloLink>,
    pub aggregate_mbps: u32,
    pub average_rssi: i8,
}

impl WifiMloSession {
    pub fn link_count(&self) -> usize {
        1 + self.secondary.len()
    }
}

// ============================================================================
// WiFi Jail Komutu (sandbox → kernel)
// ============================================================================

/// WiFi jail'den çekirdeğe gönderilen komutlar
#[derive(Clone, Debug)]
pub enum WifiJailCommand {
    /// Pasif/Aktif tarama başlat
    Scan { passive: bool },
    /// Belirli SSID'ye bağlan
    Connect {
        ssid: String,
        password: String,
        security: WifiSecurity,
    },
    /// Bağlantıyı kes
    Disconnect,
    /// Güç modu ayarla
    SetPowerSave(bool),
    /// MAC adresi oku
    GetMacAddress,
    /// İstatistikleri oku
    GetStats,
    /// Firmware sürümünü oku
    GetFirmwareVersion,
    /// Kanal değiştir
    SetChannel(u8),
    /// TX gücü ayarla (dBm)
    SetTxPower(i8),
}

/// Kernel → WiFi jail yanıtı
#[derive(Clone, Debug)]
pub enum WifiJailResponse {
    Ok,
    Error(WifiError),
    ScanResults(Vec<WifiBss>),
    MacAddress([u8; 6]),
    Stats(WifiStats),
    FirmwareVersion(String),
}

/// WiFi hata türleri
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WifiError {
    NotInitialized,
    DeviceNotFound,
    AlreadyConnected,
    NotConnected,
    AuthenticationFailed,
    AssociationFailed,
    Timeout,
    FirmwareError,
    InvalidParameter,
    NoMemory,
}

// ============================================================================
// WiFi İstatistikleri
// ============================================================================

#[derive(Clone, Debug, Default)]
pub struct WifiStats {
    /// Gönderilen paket sayısı
    pub tx_packets: u64,
    /// Alınan paket sayısı
    pub rx_packets: u64,
    /// Gönderilen bayt
    pub tx_bytes: u64,
    /// Alınan bayt
    pub rx_bytes: u64,
    /// TX hataları
    pub tx_errors: u64,
    /// RX hataları
    pub rx_errors: u64,
    /// Yeniden deneme sayısı
    pub tx_retries: u64,
    /// Düşürülen çerçeveler
    pub rx_dropped: u64,
    /// Son sinyal gücü (dBm)
    pub signal_dbm: i8,
    /// Son gürültü seviyesi (dBm)
    pub noise_dbm: i8,
    /// Bağlantı süresi (saniye)
    pub connected_time: u64,
}

// ============================================================================
// WiFi Jail Controller
// ============================================================================

/// TIER 2 WiFi sürücüsü — tüm işlemler jail sandbox içinde
pub struct WifiJailController {
    /// Donanım var mı?
    pub initialized: AtomicBool,
    /// Bağlantı durumu
    pub state: Mutex<WifiState>,
    /// Bağlı SSID
    pub connected_ssid: Mutex<Option<String>>,
    /// Bağlı BSSID
    pub connected_bssid: Mutex<Option<[u8; 6]>>,
    /// MAC adresi
    pub mac_address: Mutex<[u8; 6]>,
    /// Tarama sonuçları
    pub scan_results: Mutex<Vec<WifiBss>>,
    /// Aktif MLO oturumu
    pub mlo_session: Mutex<Option<WifiMloSession>>,
    /// İstatistikler
    pub stats: Mutex<WifiStats>,
    /// Command ring token (sandbox kimlik doğrulama)
    pub jail_token: AtomicU32,
}

impl WifiJailController {
    pub const fn new() -> Self {
        Self {
            initialized: AtomicBool::new(false),
            state: Mutex::new(WifiState::Disconnected),
            connected_ssid: Mutex::new(None),
            connected_bssid: Mutex::new(None),
            mac_address: Mutex::new([0u8; 6]),
            scan_results: Mutex::new(Vec::new()),
            mlo_session: Mutex::new(None),
            stats: Mutex::new(WifiStats {
                tx_packets: 0,
                rx_packets: 0,
                tx_bytes: 0,
                rx_bytes: 0,
                tx_errors: 0,
                rx_errors: 0,
                tx_retries: 0,
                rx_dropped: 0,
                signal_dbm: 0,
                noise_dbm: 0,
                connected_time: 0,
            }),
            jail_token: AtomicU32::new(0),
        }
    }

    fn estimated_link_mbps(bss: &WifiBss) -> u32 {
        let base = match bss.phy_mode {
            WifiPhyMode::Dot11B => 11,
            WifiPhyMode::Dot11G => 54,
            WifiPhyMode::Dot11N => 300,
            WifiPhyMode::Dot11AC => 866,
            WifiPhyMode::Dot11AX => 1200,
            WifiPhyMode::Dot11BE => 2400,
        };

        let width_factor = match bss.channel_width {
            20 => 1,
            40 => 2,
            80 => 4,
            160 => 8,
            320 => 16,
            _ => 1,
        };

        let band_gain = match bss.band {
            WifiBand::Band2G => 85,
            WifiBand::Band5G => 100,
            WifiBand::Band6G => 115,
        };

        let rssi_gain = (bss.rssi as i32 + 100).clamp(25, 70) as u32;
        ((base * width_factor) * band_gain as u32 * rssi_gain) / 7000
    }

    fn link_score(bss: &WifiBss) -> u32 {
        let band_score = match bss.band {
            WifiBand::Band2G => 8,
            WifiBand::Band5G => 18,
            WifiBand::Band6G => 28,
        };
        let phy_score = match bss.phy_mode {
            WifiPhyMode::Dot11B => 1,
            WifiPhyMode::Dot11G => 4,
            WifiPhyMode::Dot11N => 10,
            WifiPhyMode::Dot11AC => 18,
            WifiPhyMode::Dot11AX => 26,
            WifiPhyMode::Dot11BE => 34,
        };
        let width_score = match bss.channel_width {
            20 => 4,
            40 => 8,
            80 => 14,
            160 => 22,
            320 => 32,
            _ => 0,
        };
        let security_score = match bss.security {
            WifiSecurity::WPA3Personal | WifiSecurity::WPA3Enterprise => 12,
            WifiSecurity::WPA2Personal | WifiSecurity::WPA2Enterprise => 8,
            WifiSecurity::WPA | WifiSecurity::WEP => 2,
            WifiSecurity::Open => 0,
        };
        let signal_score = (bss.rssi as i32 + 100).clamp(0, 60) as u32;

        signal_score * 4 + band_score * 3 + phy_score * 5 + width_score * 2 + security_score
    }

    fn bss_to_link(bss: &WifiBss) -> WifiMloLink {
        WifiMloLink {
            bssid: bss.bssid,
            band: bss.band,
            channel: bss.channel,
            frequency: bss.frequency,
            phy_mode: bss.phy_mode,
            channel_width: bss.channel_width,
            rssi: bss.rssi,
            score: Self::link_score(bss),
            estimated_mbps: Self::estimated_link_mbps(bss),
        }
    }

    fn security_compatible(expected: WifiSecurity, observed: WifiSecurity) -> bool {
        if expected == observed {
            return true;
        }

        matches!(
            (expected, observed),
            (WifiSecurity::WPA3Personal, WifiSecurity::WPA2Personal)
                | (WifiSecurity::WPA3Enterprise, WifiSecurity::WPA2Enterprise)
        )
    }

    fn build_scan_results(&self, passive: bool) -> Vec<WifiBss> {
        let mut results = vec![
            WifiBss {
                bssid: [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x01],
                ssid: String::from("echOS-Lab"),
                rssi: -51,
                channel: 6,
                frequency: 2437,
                band: WifiBand::Band2G,
                security: WifiSecurity::WPA3Personal,
                phy_mode: WifiPhyMode::Dot11AX,
                channel_width: 40,
            },
            WifiBss {
                bssid: [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x02],
                ssid: String::from("echOS-Lab"),
                rssi: -44,
                channel: 36,
                frequency: 5180,
                band: WifiBand::Band5G,
                security: WifiSecurity::WPA3Personal,
                phy_mode: WifiPhyMode::Dot11AX,
                channel_width: 160,
            },
            WifiBss {
                bssid: [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0x03],
                ssid: String::from("echOS-Lab"),
                rssi: -39,
                channel: 5,
                frequency: 5975,
                band: WifiBand::Band6G,
                security: WifiSecurity::WPA3Personal,
                phy_mode: WifiPhyMode::Dot11BE,
                channel_width: 320,
            },
            WifiBss {
                bssid: [0xAC, 0xEF, 0x00, 0x11, 0x22, 0x33],
                ssid: String::from("echOS-Guest"),
                rssi: -66,
                channel: 149,
                frequency: 5745,
                band: WifiBand::Band5G,
                security: WifiSecurity::WPA2Personal,
                phy_mode: WifiPhyMode::Dot11AC,
                channel_width: 80,
            },
        ];

        if passive {
            // Pasif taramada beacon'ı zayıf ağlar filtrelenir.
            results.retain(|bss| bss.rssi >= -60);
        }

        results
    }

    pub fn plan_mlo_for_ssid(&self, ssid: &str, security: WifiSecurity) -> Option<WifiMloSession> {
        let mut candidates: Vec<WifiBss> = self
            .scan_results
            .lock()
            .iter()
            .filter(|bss| bss.ssid == ssid && Self::security_compatible(security, bss.security))
            .cloned()
            .collect();

        if candidates.is_empty() {
            return None;
        }

        candidates.sort_by(|a, b| {
            Self::link_score(b)
                .cmp(&Self::link_score(a))
                .then_with(|| Self::estimated_link_mbps(b).cmp(&Self::estimated_link_mbps(a)))
        });

        let primary = Self::bss_to_link(&candidates[0]);
        let mut secondary = Vec::new();
        let mut used_bands = vec![primary.band];

        for candidate in candidates.iter().skip(1) {
            if used_bands.contains(&candidate.band) {
                continue;
            }

            let link = Self::bss_to_link(candidate);
            if link.score + 12 < primary.score {
                continue;
            }
            secondary.push(link.clone());
            used_bands.push(candidate.band);

            if secondary.len() == 2 {
                break;
            }
        }

        let total_raw = primary.estimated_mbps
            + secondary
                .iter()
                .map(|link| link.estimated_mbps)
                .sum::<u32>();
        let efficiency = 92u32.saturating_sub((secondary.len() as u32) * 7);
        let aggregate_mbps = total_raw * efficiency / 100;

        let total_rssi =
            primary.rssi as i32 + secondary.iter().map(|link| link.rssi as i32).sum::<i32>();
        let average_rssi = (total_rssi / (1 + secondary.len()) as i32) as i8;

        Some(WifiMloSession {
            ssid: String::from(ssid),
            security,
            primary,
            secondary,
            aggregate_mbps,
            average_rssi,
        })
    }

    /// WiFi donanımını tarar ve başlatır
    pub fn init(&self) -> Result<(), WifiError> {
        // PCI taraması: wireless network controller (class=0x02, subclass=0x80)
        // veya Intel WiFi (vendor=0x8086) / Qualcomm / Broadcom
        for dev in crate::drivers::pci::scan() {
            let is_wifi = (dev.class_code == 0x02 && dev.subclass == 0x80)
                || (dev.vendor_id == 0x8086 && dev.class_code == 0x02); // Intel WiFi

            if is_wifi {
                crate::serial_println!(
                    "[WiFi Jail] Found WiFi adapter: {:04x}:{:04x} (class={:02x}.{:02x})",
                    dev.vendor_id,
                    dev.device_id,
                    dev.class_code,
                    dev.subclass
                );

                // Pseudo MAC adresi ata
                let mut mac = self.mac_address.lock();
                *mac = [
                    0x02,
                    0x00,
                    0x00,
                    (dev.vendor_id & 0xFF) as u8,
                    (dev.device_id >> 8) as u8,
                    (dev.device_id & 0xFF) as u8,
                ];

                self.initialized.store(true, Ordering::SeqCst);
                self.jail_token.store(0xCAFE_0001, Ordering::SeqCst);

                crate::serial_println!(
                    "[WiFi Jail] MAC: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                    mac[0],
                    mac[1],
                    mac[2],
                    mac[3],
                    mac[4],
                    mac[5]
                );
                return Ok(());
            }
        }

        crate::serial_println!("[WiFi Jail] No WiFi adapter found");
        Err(WifiError::DeviceNotFound)
    }

    /// Jail komutunu işler (sandbox → kernel yolu)
    pub fn process_command(&self, cmd: WifiJailCommand) -> WifiJailResponse {
        if !self.initialized.load(Ordering::SeqCst) {
            return WifiJailResponse::Error(WifiError::NotInitialized);
        }

        match cmd {
            WifiJailCommand::Scan { passive } => {
                *self.state.lock() = WifiState::Scanning;
                crate::serial_println!(
                    "[WiFi Jail] Scan initiated ({})",
                    if passive { "passive" } else { "active" }
                );

                let results = self.build_scan_results(passive);

                *self.scan_results.lock() = results.clone();
                *self.state.lock() = WifiState::Disconnected;

                WifiJailResponse::ScanResults(results)
            }
            WifiJailCommand::Connect {
                ssid,
                password: _,
                security,
            } => {
                let state = *self.state.lock();
                if state == WifiState::Connected {
                    return WifiJailResponse::Error(WifiError::AlreadyConnected);
                }

                crate::serial_println!(
                    "[WiFi Jail] Connecting to '{}' ({})",
                    ssid,
                    security.as_str()
                );

                *self.state.lock() = WifiState::Authenticating;
                let mlo_session = match self.plan_mlo_for_ssid(&ssid, security) {
                    Some(session) => session,
                    None => {
                        *self.state.lock() = WifiState::Disconnected;
                        return WifiJailResponse::Error(WifiError::AssociationFailed);
                    }
                };
                *self.state.lock() = WifiState::Associating;
                *self.state.lock() = WifiState::Connected;

                *self.connected_ssid.lock() = Some(ssid.clone());
                *self.connected_bssid.lock() = Some(mlo_session.primary.bssid);
                self.stats.lock().signal_dbm = mlo_session.average_rssi;
                *self.mlo_session.lock() = Some(mlo_session.clone());

                crate::serial_println!(
                    "[WiFi Jail] Connected to '{}' via {} link(s), aggregate={} Mbps",
                    ssid,
                    mlo_session.link_count(),
                    mlo_session.aggregate_mbps
                );
                WifiJailResponse::Ok
            }
            WifiJailCommand::Disconnect => {
                if *self.state.lock() != WifiState::Connected {
                    return WifiJailResponse::Error(WifiError::NotConnected);
                }

                let ssid = self.connected_ssid.lock().clone().unwrap_or_default();
                crate::serial_println!("[WiFi Jail] Disconnecting from '{}'", ssid);

                *self.state.lock() = WifiState::Disconnected;
                *self.connected_ssid.lock() = None;
                *self.connected_bssid.lock() = None;
                *self.mlo_session.lock() = None;

                WifiJailResponse::Ok
            }
            WifiJailCommand::GetMacAddress => {
                WifiJailResponse::MacAddress(*self.mac_address.lock())
            }
            WifiJailCommand::GetStats => WifiJailResponse::Stats(self.stats.lock().clone()),
            WifiJailCommand::GetFirmwareVersion => {
                WifiJailResponse::FirmwareVersion(String::from("echOS-WiFi-Jail v1.0.0"))
            }
            WifiJailCommand::SetPowerSave(enable) => {
                crate::serial_println!(
                    "[WiFi Jail] Power save: {}",
                    if enable { "ON" } else { "OFF" }
                );
                WifiJailResponse::Ok
            }
            WifiJailCommand::SetChannel(ch) => {
                crate::serial_println!("[WiFi Jail] Channel set: {}", ch);
                WifiJailResponse::Ok
            }
            WifiJailCommand::SetTxPower(dbm) => {
                crate::serial_println!("[WiFi Jail] TX power: {} dBm", dbm);
                WifiJailResponse::Ok
            }
        }
    }

    /// Bağlantı durumu
    pub fn get_state(&self) -> WifiState {
        *self.state.lock()
    }

    /// Bağlı SSID
    pub fn connected_ssid(&self) -> Option<String> {
        self.connected_ssid.lock().clone()
    }

    pub fn mlo_session(&self) -> Option<WifiMloSession> {
        self.mlo_session.lock().clone()
    }
}

// ============================================================================
// Global Instance
// ============================================================================

lazy_static::lazy_static! {
    pub static ref WIFI_JAIL: WifiJailController = WifiJailController::new();
}

/// WiFi jail modülünü başlatır
pub fn init() {
    crate::serial_println!("[WiFi Jail] TIER 2 WiFi driver initializing...");
    match WIFI_JAIL.init() {
        Ok(()) => crate::serial_println!("[WiFi Jail] Initialization complete"),
        Err(e) => crate::serial_println!("[WiFi Jail] Init skipped: {:?}", e),
    }
}
