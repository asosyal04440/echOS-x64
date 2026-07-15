//! Stateful port knocking and single-packet authorization.

use super::Ipv4Addr;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp::min;
use spin::Mutex;

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum KnockProto {
    Tcp,
    Udp,
    Icmp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KnockEvent {
    pub proto: KnockProto,
    pub port: u16,
    pub ts_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtectedService {
    pub name: String,
    pub protected_port: u16,
    pub sequence: Vec<(KnockProto, u16)>,
    pub open_window_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpaPacket {
    pub service: String,
    pub requested_port: u16,
    pub ts_ms: u64,
    pub nonce: u64,
    pub tag: [u8; 32],
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct PeerState {
    knocks: VecDeque<KnockEvent>,
    authorized_until_ms: BTreeMap<String, u64>,
    used_spa_nonces: BTreeMap<(String, u64), u64>,
}

static SERVICES: Mutex<BTreeMap<String, ProtectedService>> = Mutex::new(BTreeMap::new());
static PEERS: Mutex<BTreeMap<Ipv4Addr, PeerState>> = Mutex::new(BTreeMap::new());

fn compute_tag(secret: &[u8], service: &str, ip: Ipv4Addr, port: u16, ts_ms: u64, nonce: u64) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(secret).expect("hmac");
    mac.update(service.as_bytes());
    mac.update(&ip.0);
    mac.update(&port.to_be_bytes());
    mac.update(&ts_ms.to_be_bytes());
    mac.update(&nonce.to_be_bytes());
    let bytes = mac.finalize().into_bytes();
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes[..32]);
    out
}

pub fn register_service(service: ProtectedService) {
    SERVICES.lock().insert(service.name.clone(), service);
}

pub fn build_spa_packet(
    secret: &[u8],
    service: &str,
    ip: Ipv4Addr,
    requested_port: u16,
    ts_ms: u64,
    nonce: u64,
) -> SpaPacket {
    SpaPacket {
        service: String::from(service),
        requested_port,
        ts_ms,
        nonce,
        tag: compute_tag(secret, service, ip, requested_port, ts_ms, nonce),
    }
}

pub fn observe_knock(peer: Ipv4Addr, event: KnockEvent) {
    let mut peers = PEERS.lock();
    let state = peers.entry(peer).or_default();
    state.knocks.push_back(event);
    while state.knocks.len() > 16 {
        state.knocks.pop_front();
    }

    let services = SERVICES.lock();
    for service in services.values() {
        let seq_len = service.sequence.len();
        if state.knocks.len() < seq_len {
            continue;
        }
        let start = state.knocks.len() - seq_len;
        let knocks: Vec<_> = state.knocks.iter().skip(start).copied().collect();
        if knocks
            .iter()
            .zip(service.sequence.iter())
            .all(|(ev, expected)| (ev.proto, ev.port) == *expected)
        {
            let first_ts = knocks.first().map(|k| k.ts_ms).unwrap_or(0);
            let last_ts = knocks.last().map(|k| k.ts_ms).unwrap_or(0);
            if last_ts.saturating_sub(first_ts) <= service.open_window_ms {
                state
                    .authorized_until_ms
                    .insert(service.name.clone(), last_ts + service.open_window_ms);
            }
        }
    }
}

pub fn authorize_spa(
    secret: &[u8],
    peer: Ipv4Addr,
    packet: &SpaPacket,
    now_ms: u64,
    replay_window_ms: u64,
) -> bool {
    if now_ms.abs_diff(packet.ts_ms) > replay_window_ms {
        return false;
    }
    let tag = compute_tag(
        secret,
        &packet.service,
        peer,
        packet.requested_port,
        packet.ts_ms,
        packet.nonce,
    );
    if tag != packet.tag {
        return false;
    }
    let services = SERVICES.lock();
    let Some(service) = services.get(&packet.service) else {
        return false;
    };
    if packet.requested_port != service.protected_port {
        return false;
    }
    let mut peers = PEERS.lock();
    let state = peers.entry(peer).or_default();
    state
        .used_spa_nonces
        .retain(|_, ts| now_ms.saturating_sub(*ts) <= replay_window_ms);
    let nonce_key = (packet.service.clone(), packet.nonce);
    if state.used_spa_nonces.contains_key(&nonce_key) {
        return false;
    }
    state.used_spa_nonces.insert(nonce_key, packet.ts_ms);
    state
        .authorized_until_ms
        .insert(packet.service.clone(), now_ms + service.open_window_ms);
    true
}

pub fn is_authorized(peer: Ipv4Addr, service: &str, now_ms: u64) -> bool {
    let mut peers = PEERS.lock();
    let Some(state) = peers.get_mut(&peer) else {
        return false;
    };
    state.authorized_until_ms.retain(|_, until| *until >= now_ms);
    state
        .authorized_until_ms
        .get(service)
        .map(|until| *until >= now_ms)
        .unwrap_or(false)
}

pub fn revoke(peer: Ipv4Addr, service: &str) {
    if let Some(state) = PEERS.lock().get_mut(&peer) {
        state.authorized_until_ms.remove(service);
    }
}

pub fn cleanup_peer_log(peer: Ipv4Addr, keep_last: usize) {
    if let Some(state) = PEERS.lock().get_mut(&peer) {
        while state.knocks.len() > keep_last {
            state.knocks.pop_front();
        }
        let floor_ts = state.knocks.back().map(|event| event.ts_ms).unwrap_or(0);
        state.authorized_until_ms.retain(|_, until| *until >= floor_ts);
        state.used_spa_nonces.retain(|_, ts| *ts >= floor_ts.saturating_sub(60_000));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn knock_sequence_opens_service() {
        register_service(ProtectedService {
            name: String::from("ssh"),
            protected_port: 22,
            sequence: vec![(KnockProto::Tcp, 1000), (KnockProto::Udp, 2000), (KnockProto::Tcp, 3000)],
            open_window_ms: 3000,
        });
        let ip = Ipv4Addr([203, 0, 113, 1]);
        observe_knock(ip, KnockEvent { proto: KnockProto::Tcp, port: 1000, ts_ms: 100 });
        observe_knock(ip, KnockEvent { proto: KnockProto::Udp, port: 2000, ts_ms: 200 });
        observe_knock(ip, KnockEvent { proto: KnockProto::Tcp, port: 3000, ts_ms: 300 });
        assert!(is_authorized(ip, "ssh", 1000));
    }

    #[test]
    fn spa_packet_authorizes_service() {
        register_service(ProtectedService {
            name: String::from("admin"),
            protected_port: 8443,
            sequence: vec![],
            open_window_ms: 5000,
        });
        let secret = b"echos-wave4";
        let ip = Ipv4Addr([198, 51, 100, 9]);
        let pkt = build_spa_packet(secret, "admin", ip, 8443, 1_000, 77);
        assert!(authorize_spa(secret, ip, &pkt, 1_500, 2_000));
        assert!(is_authorized(ip, "admin", 2_000));
    }

    #[test]
    fn spa_replay_is_rejected() {
        register_service(ProtectedService {
            name: String::from("ops"),
            protected_port: 9443,
            sequence: vec![],
            open_window_ms: 3_000,
        });
        let secret = b"echos-wave4";
        let ip = Ipv4Addr([198, 51, 100, 10]);
        let pkt = build_spa_packet(secret, "ops", ip, 9443, 5_000, 99);
        assert!(authorize_spa(secret, ip, &pkt, 5_100, 1_000));
        assert!(!authorize_spa(secret, ip, &pkt, 5_150, 1_000));
    }
}
