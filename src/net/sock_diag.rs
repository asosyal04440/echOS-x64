//! # SOCK_DIAG — Socket Monitoring via NETLINK_SOCK_DIAG
//!
//! Implements the `ss`-compatible socket monitoring protocol
//! (`NETLINK_SOCK_DIAG`). Listens for `SOCK_DIAG_BY_FAMILY` requests
//! and replies with `inet_diag_msg` structures for every matching socket.
//!
//! ## Wire format (Linux ABI)
//!
//! Request:
//!   NlMsgHdr (16B) | inet_diag_req (48B)
//!
//! Response (multi-part):
//!   NlMsgHdr (16B, NLM_F_MULTI) | inet_diag_msg (72B) | [INET_DIAG_INFO attr]
//!   NlMsgHdr (16B, NLM_F_MULTI) | inet_diag_msg (72B) | [INET_DIAG_INFO attr]
//!   ...
//!   NLMSG_DONE

use alloc::vec::Vec;
use alloc::vec;
use core::mem::size_of;

// ============================================================================
// CONSTANTS
// ============================================================================

/// SOCK_DIAG request type: dump sockets by family
pub const SOCK_DIAG_BY_FAMILY: u16 = 20;

use crate::net::netlink::{NLMSG_DONE, NLMSG_ERROR};

/// INET_DIAG attribute types
const INET_DIAG_INFO: u16 = 2;
const INET_DIAG_CONG: u16 = 4;

// ============================================================================
// STRUCTURES (Linux ABI, repr(C))
// ============================================================================

/// `struct inet_diag_sockid` — 24 bytes
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct InetDiagSockId {
    pub sport: u16,
    pub dport: u16,
    pub ifindex: u32,
    pub cookie: [u32; 2],
    pub src: [u32; 4],
    pub dst: [u32; 4],
}

/// `struct inet_diag_msg` — 72 bytes
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct InetDiagMsg {
    pub family: u8,
    pub state: u8,
    pub timer: u8,
    pub retrans: u8,
    pub id: InetDiagSockId,
    pub expires: u32,
    pub rqueue: u32,
    pub wqueue: u32,
    pub uid: u32,
    pub inode: u32,
}

/// `struct inet_diag_req` — query filter
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct InetDiagReq {
    pub family: u8,
    pub src_len: u8,
    pub dst_len: u8,
    pub ext: u8,
    pub id: InetDiagSockId,
    pub states: u32,
    pub dbs: u32,
}

// ============================================================================
// HELPERS
// ============================================================================

const TCP_ESTABLISHED: u8 = 1;
const TCP_SYN_SENT: u8 = 2;
const TCP_SYN_RECV: u8 = 3;
const TCP_FIN_WAIT1: u8 = 4;
const TCP_FIN_WAIT2: u8 = 5;
const TCP_TIME_WAIT: u8 = 6;
const TCP_CLOSE: u8 = 7;
const TCP_CLOSE_WAIT: u8 = 8;
const TCP_LAST_ACK: u8 = 9;
const TCP_LISTEN: u8 = 10;
const TCP_CLOSING: u8 = 11;

fn tcp_state_to_diag(state: &crate::net::tcp::TcpState) -> u8 {
    use crate::net::tcp::TcpState::*;
    match state {
        Closed => TCP_CLOSE,
        Listen => TCP_LISTEN,
        SynSent => TCP_SYN_SENT,
        SynReceived => TCP_SYN_RECV,
        Established => TCP_ESTABLISHED,
        FinWait1 => TCP_FIN_WAIT1,
        FinWait2 => TCP_FIN_WAIT2,
        CloseWait => TCP_CLOSE_WAIT,
        Closing => TCP_CLOSING,
        LastAck => TCP_LAST_ACK,
        TimeWait => TCP_TIME_WAIT,
    }
}

fn af_to_diag(af: &crate::net::socket::AddressFamily) -> u8 {
    use crate::net::socket::AddressFamily::*;
    match af {
        UNSPEC => 0,
        IPV4 => 2,
        IPV6 => 10,
        PACKET => 17,
    }
}

fn ip_to_diag_bytes(ip: &crate::net::IpAddr) -> [u32; 4] {
    match ip {
        crate::net::IpAddr::V4(v4) => [u32::from_ne_bytes(v4.0), 0, 0, 0],
        crate::net::IpAddr::V6(v6) => {
            let b = v6.0;
            [
                u32::from_be_bytes([b[0], b[1], b[2], b[3]]),
                u32::from_be_bytes([b[4], b[5], b[6], b[7]]),
                u32::from_be_bytes([b[8], b[9], b[10], b[11]]),
                u32::from_be_bytes([b[12], b[13], b[14], b[15]]),
            ]
        }
    }
}

fn build_tcp_info_attr(conn: &crate::net::tcp::TcpConnection) -> Vec<u8> {
    use crate::net::tcp_info::TcpInfo;
    let info = TcpInfo::from_connection(conn);
    let serialized = info.serialize();
    let mut attr = Vec::with_capacity(4 + serialized.len());
    let total_len = (4 + serialized.len()) as u16;
    attr.extend_from_slice(&total_len.to_le_bytes());
    attr.extend_from_slice(&INET_DIAG_INFO.to_le_bytes());
    attr.extend_from_slice(&serialized);
    while attr.len() % 4 != 0 {
        attr.push(0);
    }
    attr
}

// ============================================================================
// MAIN ENTRY POINT
// ============================================================================

/// Handle a SOCK_DIAG request and return the response payload(s).
///
/// `request` is the raw netlink message payload (after NlMsgHdr).
/// Returns a vector of (nlmsg_type, payload) tuples to be sent as
/// multi-part messages.
pub fn handle_diag_request(request: &[u8]) -> Vec<(u16, Vec<u8>)> {
    if request.len() < size_of::<InetDiagReq>() {
        return vec![(NLMSG_ERROR, vec![])];
    }

    let req = unsafe { &*(request.as_ptr() as *const InetDiagReq) };

    if req.family != 2 && req.family != 10 {
        return vec![(NLMSG_DONE, vec![])];
    }

    let mut responses: Vec<(u16, Vec<u8>)> = Vec::new();

    let tcp_conns = crate::net::tcp::TCP_CONNECTIONS.lock();
    for (_id, conn) in tcp_conns.iter() {
        let family_byte = af_to_diag(&conn.family);
        if family_byte != req.family && req.family != 0 {
            continue;
        }

        let state_byte = tcp_state_to_diag(&conn.state);
        if (req.states & (1u32 << state_byte)) == 0 && req.states != 0 && req.states != !0u32 {
            continue;
        }

        let msg = InetDiagMsg {
            family: family_byte,
            state: state_byte,
            timer: 0,
            retrans: conn.retransmit_count,
            id: InetDiagSockId {
                sport: conn.local.port.0.to_be(),
                dport: conn.remote.port.0.to_be(),
                ifindex: 1,
                cookie: [0, 0],
                src: ip_to_diag_bytes(&conn.local.ip),
                dst: ip_to_diag_bytes(&conn.remote.ip),
            },
            expires: 0,
            rqueue: conn.rx_buffer.len() as u32,
            wqueue: conn.tx_buffer.len() as u32,
            uid: 0,
            inode: conn.id,
        };

        let msg_bytes = unsafe {
            core::slice::from_raw_parts(
                &msg as *const InetDiagMsg as *const u8,
                size_of::<InetDiagMsg>(),
            )
        };

        let mut payload = msg_bytes.to_vec();

        if req.ext & 1 != 0 {
            let tcp_info_bytes = build_tcp_info_attr(conn);
            payload.extend_from_slice(&tcp_info_bytes);
        }

        responses.push((SOCK_DIAG_BY_FAMILY, payload));
    }
    drop(tcp_conns);

    let udp_socks = crate::net::udp::UDP_SOCKETS.lock();
    for (_id, sock) in udp_socks.iter() {
        let family_byte = af_to_diag(&sock.family);
        if family_byte != req.family && req.family != 0 {
            continue;
        }

        let msg = InetDiagMsg {
            family: family_byte,
            state: 1,
            timer: 0,
            retrans: 0,
            id: InetDiagSockId {
                sport: sock.local.port.0.to_be(),
                dport: 0,
                ifindex: 1,
                cookie: [0, 0],
                src: ip_to_diag_bytes(&sock.local.ip),
                dst: [0; 4],
            },
            expires: 0,
            rqueue: sock.rx_buffer.len() as u32,
            wqueue: 0,
            uid: 0,
            inode: sock.id,
        };

        let msg_bytes = unsafe {
            core::slice::from_raw_parts(
                &msg as *const InetDiagMsg as *const u8,
                size_of::<InetDiagMsg>(),
            )
        };

        responses.push((SOCK_DIAG_BY_FAMILY, msg_bytes.to_vec()));
    }
    drop(udp_socks);

    responses.push((NLMSG_DONE, vec![]));
    responses
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::{offset_of, size_of};

    #[test]
    fn inet_diag_sockid_size() {
        assert_eq!(size_of::<InetDiagSockId>(), 48);
    }

    #[test]
    fn inet_diag_msg_size() {
        assert_eq!(size_of::<InetDiagMsg>(), 72);
    }

    #[test]
    fn inet_diag_req_size() {
        assert_eq!(size_of::<InetDiagReq>(), 60);
    }

    #[test]
    fn inet_diag_sockid_offsets() {
        assert_eq!(offset_of!(InetDiagSockId, sport), 0);
        assert_eq!(offset_of!(InetDiagSockId, dport), 2);
        assert_eq!(offset_of!(InetDiagSockId, ifindex), 4);
        assert_eq!(offset_of!(InetDiagSockId, cookie), 8);
        assert_eq!(offset_of!(InetDiagSockId, src), 16);
        assert_eq!(offset_of!(InetDiagSockId, dst), 32);
    }

    #[test]
    fn inet_diag_msg_offsets() {
        assert_eq!(offset_of!(InetDiagMsg, family), 0);
        assert_eq!(offset_of!(InetDiagMsg, state), 1);
        assert_eq!(offset_of!(InetDiagMsg, timer), 2);
        assert_eq!(offset_of!(InetDiagMsg, retrans), 3);
        assert_eq!(offset_of!(InetDiagMsg, id), 4);
        assert_eq!(offset_of!(InetDiagMsg, expires), 52);
        assert_eq!(offset_of!(InetDiagMsg, rqueue), 56);
        assert_eq!(offset_of!(InetDiagMsg, wqueue), 60);
        assert_eq!(offset_of!(InetDiagMsg, uid), 64);
        assert_eq!(offset_of!(InetDiagMsg, inode), 68);
    }

    #[test]
    fn empty_request_returns_error() {
        let result = handle_diag_request(&[]);
        assert!(!result.is_empty());
        assert_eq!(result[0].0, NLMSG_ERROR);
    }

    #[test]
    fn short_request_returns_error() {
        let result = handle_diag_request(&[1, 2, 3]);
        assert_eq!(result[0].0, NLMSG_ERROR);
    }

    #[test]
    fn full_request_empty_tables_returns_done() {
        let req = InetDiagReq {
            family: 2,
            src_len: 0,
            dst_len: 0,
            ext: 0,
            id: InetDiagSockId {
                sport: 0,
                dport: 0,
                ifindex: 0,
                cookie: [0; 2],
                src: [0; 4],
                dst: [0; 4],
            },
            states: !0u32,
            dbs: 0,
        };
        let req_bytes = unsafe {
            core::slice::from_raw_parts(
                &req as *const InetDiagReq as *const u8,
                size_of::<InetDiagReq>(),
            )
        };
        let result = handle_diag_request(req_bytes);
        assert!(!result.is_empty());
        let last = result.last().unwrap();
        assert_eq!(last.0, NLMSG_DONE);
    }

    #[test]
    fn tcp_state_conversion() {
        assert_eq!(tcp_state_to_diag(&crate::net::tcp::TcpState::Established), TCP_ESTABLISHED);
        assert_eq!(tcp_state_to_diag(&crate::net::tcp::TcpState::Listen), TCP_LISTEN);
        assert_eq!(tcp_state_to_diag(&crate::net::tcp::TcpState::Closed), TCP_CLOSE);
        assert_eq!(tcp_state_to_diag(&crate::net::tcp::TcpState::TimeWait), TCP_TIME_WAIT);
    }

    #[test]
    fn af_conversion() {
        assert_eq!(af_to_diag(&crate::net::socket::AddressFamily::IPV4), 2);
        assert_eq!(af_to_diag(&crate::net::socket::AddressFamily::IPV6), 10);
    }

    #[test]
    fn ipv4_to_diag_bytes_conversion() {
        let ip = crate::net::IpAddr::V4(crate::net::Ipv4Addr([192, 168, 1, 1]));
        let b = ip_to_diag_bytes(&ip);
        assert_eq!(b[0], u32::from_ne_bytes([192, 168, 1, 1]));
        assert_eq!(b[1], 0);
        assert_eq!(b[2], 0);
        assert_eq!(b[3], 0);
    }

    #[test]
    fn unused_family_returns_empty_done() {
        let req = InetDiagReq {
            family: 99,
            src_len: 0,
            dst_len: 0,
            ext: 0,
            id: InetDiagSockId {
                sport: 0, dport: 0, ifindex: 0,
                cookie: [0; 2], src: [0; 4], dst: [0; 4],
            },
            states: !0u32,
            dbs: 0,
        };
        let req_bytes = unsafe {
            core::slice::from_raw_parts(&req as *const _ as *const u8, size_of::<InetDiagReq>())
        };
        let result = handle_diag_request(req_bytes);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, NLMSG_DONE);
    }
}
