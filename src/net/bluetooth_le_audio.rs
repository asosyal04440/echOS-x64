//! Bluetooth 5.2 LE Audio implementation.
//!
//! Implements ISO channels, LC3 configuration validation, unicast and broadcast
//! stream lifecycle, and a stateful in-kernel loopback transport for host-side
//! validation.

#![no_std]
#![allow(unused)]

use alloc::{boxed::Box, collections::BTreeMap, sync::Arc, vec::Vec};
use core::{
    fmt::Debug,
    sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering},
};
use spin::{Mutex, Once};

// ============================================================================
// Bluetooth LE Audio constants
// ============================================================================

pub const BT_UUID_ASCS: u16 = 0x184E;
pub const BT_UUID_BASS: u16 = 0x184F;
pub const BT_UUID_PACS: u16 = 0x1850;
pub const BT_UUID_CSIS: u16 = 0x1853;

pub const ISO_TYPE_CONNECTED: u8 = 0x01;
pub const ISO_TYPE_BROADCAST: u8 = 0x02;

pub const LC3_MIN_BITRATE: u32 = 16_000;
pub const LC3_MAX_BITRATE: u32 = 320_000;
pub const LC3_FRAME_DURATION_7_5_MS: u8 = 0x00;
pub const LC3_FRAME_DURATION_10_MS: u8 = 0x01;

pub const ASE_STATE_IDLE: u8 = 0x00;
pub const ASE_STATE_CONFIGURED: u8 = 0x01;
pub const ASE_STATE_QOS_CONFIGURED: u8 = 0x02;
pub const ASE_STATE_ENABLING: u8 = 0x03;
pub const ASE_STATE_STREAMING: u8 = 0x04;
pub const ASE_STATE_DISABLING: u8 = 0x05;
pub const ASE_STATE_RELEASING: u8 = 0x06;

// ============================================================================
// Core types
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeAudioError {
    InvalidParameter,
    NoMemory,
    Busy,
    Timeout,
    Disconnected,
    CodecError,
    QosError,
}

type LeAudioTransportFn = fn(&LeAudioManager, u16, &[u8]) -> Result<(), LeAudioError>;

#[derive(Clone, Copy)]
struct LeAudioTransportBackend {
    transmit: LeAudioTransportFn,
}

#[derive(Clone, Copy, Debug)]
pub struct Lc3Config {
    pub sampling_frequency: u32,
    pub frame_duration: u8,
    pub octets_per_frame: u16,
    pub bitrate: u32,
}

impl Lc3Config {
    pub fn new(freq: u32, duration: u8, octets: u16) -> Self {
        let bits_per_frame = octets as u32 * 8;
        let bitrate = match duration {
            LC3_FRAME_DURATION_7_5_MS => bits_per_frame.saturating_mul(400) / 3,
            LC3_FRAME_DURATION_10_MS => bits_per_frame.saturating_mul(100),
            _ => 0,
        };
        Self {
            sampling_frequency: freq,
            frame_duration: duration,
            octets_per_frame: octets,
            bitrate,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.bitrate >= LC3_MIN_BITRATE
            && self.bitrate <= LC3_MAX_BITRATE
            && matches!(
                self.frame_duration,
                LC3_FRAME_DURATION_7_5_MS | LC3_FRAME_DURATION_10_MS
            )
    }
}

pub struct AudioStreamEndpoint {
    pub id: u8,
    pub state: AtomicU8,
    pub cis_handle: u16,
    pub codec_config: Mutex<Option<Lc3Config>>,
    pub qos_config: Mutex<Option<QosConfig>>,
    pub metadata: Mutex<Vec<u8>>,
    pub data_callbacks: Mutex<Vec<Box<dyn Fn(&[u8]) + Send>>>,
}

impl AudioStreamEndpoint {
    pub fn new(id: u8) -> Self {
        Self {
            id,
            state: AtomicU8::new(ASE_STATE_IDLE),
            cis_handle: 0,
            codec_config: Mutex::new(None),
            qos_config: Mutex::new(None),
            metadata: Mutex::new(Vec::new()),
            data_callbacks: Mutex::new(Vec::new()),
        }
    }

    pub fn set_state(&self, state: u8) {
        self.state.store(state, Ordering::Release);
    }

    pub fn get_state(&self) -> u8 {
        self.state.load(Ordering::Acquire)
    }

    pub fn add_data_callback<F>(&self, callback: F)
    where
        F: Fn(&[u8]) + Send + 'static,
    {
        self.data_callbacks.lock().push(Box::new(callback));
    }
}

impl Debug for AudioStreamEndpoint {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AudioStreamEndpoint")
            .field("id", &self.id)
            .field("state", &self.get_state())
            .field("cis_handle", &self.cis_handle)
            .finish()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct QosConfig {
    pub cig_id: u8,
    pub cis_id: u8,
    pub sdu_interval: u32,
    pub framing: bool,
    pub phy: u8,
    pub max_sdu: u16,
    pub retransmission_number: u8,
    pub max_transport_latency: u16,
}

#[derive(Clone, Copy, Debug)]
pub struct BroadcastQosConfig {
    pub interval: u32,
    pub framing: bool,
    pub encryption: bool,
    pub broadcast_code: [u8; 16],
    pub max_sdu: u16,
    pub max_transport_latency: u16,
    pub rtn: u8,
    pub phy: u8,
    pub bis_count: u8,
}

#[derive(Debug)]
pub struct BroadcastAudioStream {
    pub big_handle: u8,
    pub bis_handles: Vec<u16>,
    pub codec_config: Lc3Config,
    pub qos_config: BroadcastQosConfig,
    pub subgroup_count: u8,
    pub bis_count: u8,
    pub metadata: Vec<u8>,
}

#[derive(Debug)]
pub struct LeAudioDevice {
    pub address: [u8; 6],
    pub connected: AtomicBool,
    pub ases: Mutex<BTreeMap<u8, Arc<AudioStreamEndpoint>>>,
    pub pac_sink: Vec<Lc3Config>,
    pub pac_source: Vec<Lc3Config>,
    pub csip_set_members: Vec<[u8; 6]>,
}

impl LeAudioDevice {
    pub fn new(address: [u8; 6]) -> Self {
        Self {
            address,
            connected: AtomicBool::new(false),
            ases: Mutex::new(BTreeMap::new()),
            pac_sink: Vec::new(),
            pac_source: Vec::new(),
            csip_set_members: Vec::new(),
        }
    }

    pub fn add_ase(&self, ase: Arc<AudioStreamEndpoint>) {
        self.ases.lock().insert(ase.id, ase);
    }

    pub fn get_ase(&self, id: u8) -> Option<Arc<AudioStreamEndpoint>> {
        self.ases.lock().get(&id).cloned()
    }
}

// ============================================================================
// Manager
// ============================================================================

static LE_AUDIO_MANAGER: Once<Mutex<LeAudioManager>> = Once::new();

pub struct LeAudioManager {
    pub devices: Mutex<BTreeMap<[u8; 6], Arc<LeAudioDevice>>>,
    pub broadcast_streams: Mutex<BTreeMap<u8, Arc<BroadcastAudioStream>>>,
    transport_backends: Mutex<BTreeMap<u8, LeAudioTransportBackend>>,
    iso_tx_frames: Mutex<BTreeMap<u16, Vec<Vec<u8>>>>,
    iso_rx_frames: Mutex<BTreeMap<u16, Vec<Vec<u8>>>>,
    pub next_cig_id: AtomicU8,
    pub next_big_id: AtomicU8,
    pub initialized: AtomicBool,
}

impl LeAudioManager {
    pub fn new() -> Self {
        Self {
            devices: Mutex::new(BTreeMap::new()),
            broadcast_streams: Mutex::new(BTreeMap::new()),
            transport_backends: Mutex::new(BTreeMap::new()),
            iso_tx_frames: Mutex::new(BTreeMap::new()),
            iso_rx_frames: Mutex::new(BTreeMap::new()),
            next_cig_id: AtomicU8::new(0),
            next_big_id: AtomicU8::new(0),
            initialized: AtomicBool::new(false),
        }
    }

    pub fn init() -> Result<(), LeAudioError> {
        let manager = LE_AUDIO_MANAGER.call_once(|| Mutex::new(LeAudioManager::new()));
        let mut manager = manager.lock();
        manager.initialized.store(true, Ordering::Release);
        manager
            .transport_backends
            .lock()
            .entry(ISO_TYPE_CONNECTED)
            .or_insert(LeAudioTransportBackend {
                transmit: LeAudioManager::connected_loopback_transport,
            });

        crate::serial_println!("[BT] LE Audio Manager initialized");
        Ok(())
    }

    pub fn get() -> Option<&'static Mutex<LeAudioManager>> {
        LE_AUDIO_MANAGER.get()
    }

    pub fn register_transport_backend(iso_type: u8, transmit: LeAudioTransportFn) {
        if let Some(manager) = Self::get() {
            manager
                .lock()
                .transport_backends
                .lock()
                .insert(iso_type, LeAudioTransportBackend { transmit });
        }
    }

    pub fn add_device(&self, address: [u8; 6]) -> Arc<LeAudioDevice> {
        let device = Arc::new(LeAudioDevice::new(address));
        self.devices.lock().insert(address, device.clone());
        device
    }

    pub fn find_device(&self, address: &[u8; 6]) -> Option<Arc<LeAudioDevice>> {
        self.devices.lock().get(address).cloned()
    }

    fn collect_ases_for_handle(&self, cis_handle: u16) -> Vec<Arc<AudioStreamEndpoint>> {
        let devices = self.devices.lock();
        let mut matches = Vec::new();
        for device in devices.values() {
            let ases = device.ases.lock();
            for ase in ases.values() {
                if ase.cis_handle == cis_handle {
                    matches.push(ase.clone());
                }
            }
        }
        matches
    }

    fn record_tx_frame(&self, cis_handle: u16, data: &[u8]) {
        self.iso_tx_frames
            .lock()
            .entry(cis_handle)
            .or_insert_with(Vec::new)
            .push(data.to_vec());
    }

    fn record_rx_frame(&self, cis_handle: u16, data: &[u8]) {
        self.iso_rx_frames
            .lock()
            .entry(cis_handle)
            .or_insert_with(Vec::new)
            .push(data.to_vec());
    }

    fn connected_loopback_transport(
        manager: &LeAudioManager,
        cis_handle: u16,
        data: &[u8],
    ) -> Result<(), LeAudioError> {
        manager.receive_audio_data(cis_handle, data)
    }

    pub fn tx_frames(&self, cis_handle: u16) -> Vec<Vec<u8>> {
        self.iso_tx_frames
            .lock()
            .get(&cis_handle)
            .cloned()
            .unwrap_or_default()
    }

    pub fn rx_frames(&self, cis_handle: u16) -> Vec<Vec<u8>> {
        self.iso_rx_frames
            .lock()
            .get(&cis_handle)
            .cloned()
            .unwrap_or_default()
    }

    pub fn validate_lc3_config(&self, config: &Lc3Config) -> Result<(), LeAudioError> {
        if !config.is_valid() {
            return Err(LeAudioError::InvalidParameter);
        }
        Ok(())
    }

    pub fn start_unicast_stream(
        &self,
        device_addr: &[u8; 6],
        ase_id: u8,
        codec_config: Lc3Config,
        qos_config: QosConfig,
    ) -> Result<u16, LeAudioError> {
        let device = self
            .find_device(device_addr)
            .ok_or(LeAudioError::Disconnected)?;

        self.validate_lc3_config(&codec_config)?;
        let ase = device
            .get_ase(ase_id)
            .ok_or(LeAudioError::InvalidParameter)?;

        ase.set_state(ASE_STATE_CONFIGURED);
        *ase.codec_config.lock() = Some(codec_config);
        *ase.qos_config.lock() = Some(qos_config);
        ase.set_state(ASE_STATE_STREAMING);
        Ok(ase.cis_handle)
    }

    pub fn start_broadcast_stream(
        &self,
        codec_config: Lc3Config,
        qos_config: BroadcastQosConfig,
    ) -> Result<u8, LeAudioError> {
        self.validate_lc3_config(&codec_config)?;

        let big_id = self.next_big_id.fetch_add(1, Ordering::Relaxed);
        let bis_handles = (0..qos_config.bis_count)
            .map(|i| 0x0100 + (big_id as u16) * 16 + i as u16)
            .collect();

        let stream = Arc::new(BroadcastAudioStream {
            big_handle: big_id,
            bis_handles,
            codec_config,
            qos_config,
            subgroup_count: 1,
            bis_count: qos_config.bis_count,
            metadata: Vec::new(),
        });

        self.broadcast_streams.lock().insert(big_id, stream);
        crate::serial_println!("[BT] Broadcast stream started with BIG ID {}", big_id);
        Ok(big_id)
    }

    pub fn send_audio_data(&self, cis_handle: u16, data: &[u8]) -> Result<(), LeAudioError> {
        if data.is_empty() {
            return Err(LeAudioError::InvalidParameter);
        }
        if self.collect_ases_for_handle(cis_handle).is_empty() {
            return Err(LeAudioError::Disconnected);
        }

        self.record_tx_frame(cis_handle, data);
        let backend = self
            .transport_backends
            .lock()
            .get(&ISO_TYPE_CONNECTED)
            .copied()
            .unwrap_or(LeAudioTransportBackend {
                transmit: LeAudioManager::connected_loopback_transport,
            });
        (backend.transmit)(self, cis_handle, data)
    }

    pub fn receive_audio_data(&self, cis_handle: u16, data: &[u8]) -> Result<(), LeAudioError> {
        let targets = self.collect_ases_for_handle(cis_handle);
        if targets.is_empty() {
            return Err(LeAudioError::Disconnected);
        }

        self.record_rx_frame(cis_handle, data);
        for ase in targets {
            let callbacks = ase.data_callbacks.lock();
            for callback in callbacks.iter() {
                callback(data);
            }
        }

        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lc3_config_validation() {
        let valid_config = Lc3Config::new(48_000, LC3_FRAME_DURATION_10_MS, 120);
        assert!(valid_config.is_valid());
        assert_eq!(valid_config.bitrate, 96_000);

        let invalid_config = Lc3Config::new(48_000, 0xFF, 10_000);
        assert!(!invalid_config.is_valid());
    }

    #[test]
    fn test_le_audio_manager() {
        let manager = LeAudioManager::new();
        let device_addr = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66];

        let device = manager.add_device(device_addr);
        assert!(manager.find_device(&device_addr).is_some());

        let ase = Arc::new(AudioStreamEndpoint::new(1));
        device.add_ase(ase);
        assert!(device.get_ase(1).is_some());
    }

    #[test]
    fn test_le_audio_iso_loopback_transport_is_stateful() {
        let manager = LeAudioManager::new();
        let device_addr = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
        let device = manager.add_device(device_addr);

        let mut endpoint = AudioStreamEndpoint::new(7);
        endpoint.cis_handle = 0x0042;
        let ase = Arc::new(endpoint);
        let delivered = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
        let delivered_sink = delivered.clone();
        ase.add_data_callback(move |data| {
            delivered_sink.lock().push(data.to_vec());
        });
        device.add_ase(ase);

        manager.send_audio_data(0x0042, b"frame-a").unwrap();
        manager.receive_audio_data(0x0042, b"frame-b").unwrap();

        let delivered = delivered.lock();
        assert_eq!(delivered.len(), 2);
        assert_eq!(delivered[0], b"frame-a");
        assert_eq!(delivered[1], b"frame-b");
        drop(delivered);

        assert_eq!(manager.tx_frames(0x0042).len(), 1);
        assert_eq!(manager.rx_frames(0x0042).len(), 2);
        assert_eq!(manager.tx_frames(0x0042)[0], b"frame-a");
        assert_eq!(manager.rx_frames(0x0042)[0], b"frame-a");
        assert_eq!(manager.rx_frames(0x0042)[1], b"frame-b");
    }
}
