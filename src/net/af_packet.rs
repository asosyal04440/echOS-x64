use super::{get_interface, MacAddr, NetError};
use alloc::collections::{BTreeMap, VecDeque};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

/// ETH_P_ALL — captures all Ethernet protocols (0x0003)
pub const ETH_P_ALL: u16 = 0x0003;

/// Linux AF_PACKET family value
pub const AF_PACKET_VALUE: u16 = 17;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PacketMode {
    Raw,
    Dgram,
}

struct PacketSocketState {
    id: u32,
    mode: PacketMode,
    protocol: u16,
    bound_iface: Option<Arc<str>>,
    rx_queue: VecDeque<(MacAddr, Vec<u8>)>,
}

static PACKET_SOCKETS: Mutex<BTreeMap<u32, PacketSocketState>> = Mutex::new(BTreeMap::new());
static NEXT_PACKET_ID: AtomicU32 = AtomicU32::new(1);

pub fn create_packet_socket(mode: PacketMode, protocol: u16) -> u32 {
    let id = NEXT_PACKET_ID.fetch_add(1, Ordering::Relaxed);
    PACKET_SOCKETS.lock().insert(
        id,
        PacketSocketState {
            id,
            mode,
            protocol,
            bound_iface: None,
            rx_queue: VecDeque::new(),
        },
    );
    id
}

pub fn has_packet_socket(socket_id: u32) -> bool {
    PACKET_SOCKETS.lock().contains_key(&socket_id)
}

pub fn packet_bind(socket_id: u32, ifname: &str) -> Result<(), NetError> {
    let mut sockets = PACKET_SOCKETS.lock();
    let sock = sockets.get_mut(&socket_id).ok_or(NetError::InvalidFd)?;
    get_interface(ifname).ok_or(NetError::NoInterface)?;
    sock.bound_iface = Some(ifname.into());
    Ok(())
}

pub fn packet_send_to(socket_id: u32, data: &[u8], ifname: Option<&str>) -> Result<usize, NetError> {
    let bound_iface;
    let mode;
    {
        let sockets = PACKET_SOCKETS.lock();
        let sock = sockets.get(&socket_id).ok_or(NetError::InvalidFd)?;
        mode = sock.mode;
        bound_iface = sock.bound_iface.clone();
    }
    let iface_name = ifname.or(bound_iface.as_deref()).ok_or(NetError::NoInterface)?;
    let iface = get_interface(iface_name).ok_or(NetError::NoInterface)?;
    let frame = match mode {
        PacketMode::Raw => data.to_vec(),
        PacketMode::Dgram => {
            let dst_mac = MacAddr::BROADCAST;
            let iface_guard = iface.lock();
            let src_mac = iface_guard.mac();
            drop(iface_guard);
            let proto_num = if data.len() >= 1 {
                match data[0] >> 4 {
                    4 => 0x0800u16,
                    6 => 0x86DDu16,
                    _ => 0x0800u16,
                }
            } else {
                0x0800u16
            };
            let mut frame = Vec::with_capacity(14 + data.len());
            frame.extend_from_slice(&dst_mac.0);
            frame.extend_from_slice(&src_mac.0);
            frame.extend_from_slice(&proto_num.to_be_bytes());
            frame.extend_from_slice(data);
            frame
        }
    };
    let mut iface_guard = iface.lock();
    iface_guard.send(&frame)?;
    Ok(frame.len())
}

pub fn packet_recv_from(
    socket_id: u32,
    buf: &mut [u8],
) -> Result<(usize, MacAddr), NetError> {
    let mut sockets = PACKET_SOCKETS.lock();
    let sock = sockets.get_mut(&socket_id).ok_or(NetError::InvalidFd)?;
    let (src_mac, frame) = sock.rx_queue.pop_front().ok_or(NetError::WouldBlock)?;
    let data = match sock.mode {
        PacketMode::Raw => frame,
        PacketMode::Dgram => {
            if frame.len() <= 14 {
                return Err(NetError::InvalidPacket);
            }
            frame[14..].to_vec()
        }
    };
    let len = data.len().min(buf.len());
    buf[..len].copy_from_slice(&data[..len]);
    Ok((len, src_mac))
}

pub fn packet_close(socket_id: u32) -> bool {
    PACKET_SOCKETS.lock().remove(&socket_id).is_some()
}

pub fn packet_socket_count() -> usize {
    PACKET_SOCKETS.lock().len()
}

pub fn deliver_frame(data: &[u8]) {
    if data.len() < 14 {
        return;
    }
    let ethertype = u16::from_be_bytes([data[12], data[13]]);
    let src_mac = MacAddr::new([data[6], data[7], data[8], data[9], data[10], data[11]]);
    let mut sockets = PACKET_SOCKETS.lock();
    for sock in sockets.values_mut() {
        if sock.protocol != ETH_P_ALL && sock.protocol != ethertype {
            continue;
        }
        if let Some(ref iface) = sock.bound_iface {
            if !interface_name_matches(&iface, data) {
                continue;
            }
        }
        let frame = data.to_vec();
        if sock.rx_queue.len() >= 1024 {
            sock.rx_queue.pop_front();
        }
        sock.rx_queue.push_back((src_mac, frame));
    }
}

fn interface_name_matches(_name: &str, _data: &[u8]) -> bool {
    true
}

pub fn capture_start(ethertype: u16) -> u32 {
    create_packet_socket(PacketMode::Raw, ethertype)
}

pub fn capture_stop(id: u32) -> u32 {
    let count = packet_snapshot_count(id);
    packet_close(id);
    count
}

pub fn capture_read(id: u32, buf: &mut [u8]) -> Option<usize> {
    let mut bk = [0u8; 2048];
    match packet_recv_from(id, &mut bk) {
        Ok((len, _)) => {
            let cp_len = len.min(buf.len());
            buf[..cp_len].copy_from_slice(&bk[..cp_len]);
            Some(cp_len)
        }
        Err(_) => None,
    }
}

fn packet_snapshot_count(id: u32) -> u32 {
    let sockets = PACKET_SOCKETS.lock();
    sockets.get(&id).map_or(0, |s| s.rx_queue.len() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_eth_frame(ethertype: u16, payload: &[u8]) -> Vec<u8> {
        let mut frame = Vec::with_capacity(14 + payload.len());
        frame.extend_from_slice(&[0x00u8; 6]);
        frame.extend_from_slice(&[0x01u8; 6]);
        frame.extend_from_slice(&ethertype.to_be_bytes());
        frame.extend_from_slice(payload);
        frame
    }

    #[test]
    fn packet_socket_create_close() {
        let id = create_packet_socket(PacketMode::Raw, ETH_P_ALL);
        assert!(has_packet_socket(id));
        assert!(packet_close(id));
        assert!(!has_packet_socket(id));
    }

    #[test]
    fn packet_socket_deliver_raw_frame() {
        let id = create_packet_socket(PacketMode::Raw, ETH_P_ALL);
        let frame = make_eth_frame(0x0800, b"hello");
        deliver_frame(&frame);
        let mut buf = [0u8; 64];
        let (len, mac) = packet_recv_from(id, &mut buf).unwrap();
        assert_eq!(len, frame.len());
        assert_eq!(&buf[..len], &frame);
        assert_eq!(mac, MacAddr::new([0x01; 6]));
        packet_close(id);
    }

    #[test]
    fn packet_socket_filter_by_ethertype_ip() {
        let id = create_packet_socket(PacketMode::Raw, 0x0800);
        let arp_frame = make_eth_frame(0x0806, b"arp");
        let ip_frame = make_eth_frame(0x0800, b"ip");
        deliver_frame(&arp_frame);
        deliver_frame(&ip_frame);
        let mut buf = [0u8; 64];
        let result = packet_recv_from(id, &mut buf);
        assert!(result.is_ok());
        let (len, _) = result.unwrap();
        assert_eq!(&buf[..len], &ip_frame);
        packet_close(id);
    }

    #[test]
    fn packet_socket_dgram_strips_ethernet_header() {
        let id = create_packet_socket(PacketMode::Dgram, ETH_P_ALL);
        let frame = make_eth_frame(0x0800, b"hello");
        deliver_frame(&frame);
        let mut buf = [0u8; 64];
        let (len, _) = packet_recv_from(id, &mut buf).unwrap();
        assert_eq!(len, 5);
        assert_eq!(&buf[..len], b"hello");
        packet_close(id);
    }

    #[test]
    fn packet_socket_queue_does_not_exceed_limit() {
        let id = create_packet_socket(PacketMode::Raw, ETH_P_ALL);
        for _ in 0..1100 {
            deliver_frame(&make_eth_frame(0x0800, b"x"));
        }
        {
            let sockets = PACKET_SOCKETS.lock();
            let sock = sockets.get(&id).unwrap();
            assert!(sock.rx_queue.len() <= 1024);
        }
        let mut buf = [0u8; 64];
        let result = packet_recv_from(id, &mut buf);
        assert!(result.is_ok());
        packet_close(id);
    }

    #[test]
    fn packet_socket_send_raw_and_dgram() {
        let raw_id = create_packet_socket(PacketMode::Raw, ETH_P_ALL);
        let result = packet_send_to(raw_id, &[0u8; 60], None);
        assert!(result.is_err());
        packet_close(raw_id);
    }
}
