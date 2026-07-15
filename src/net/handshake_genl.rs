use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, AtomicBool, Ordering};
use spin::Mutex;

use crate::net::netlink::{NlMsgHdr, NlAttr, NETLINK_MANAGER};

pub const HANDSHAKE_GENL_ID: u16 = 47;
pub const HANDSHAKE_GENL_VERSION: u8 = 1;

pub const HANDSHAKE_CMD_READY: u8 = 0;
pub const HANDSHAKE_CMD_ACCEPT: u8 = 1;
pub const HANDSHAKE_CMD_DONE: u8 = 2;

pub const HANDSHAKE_ATTR_SOCKFD: u16 = 1;
pub const HANDSHAKE_ATTR_HANDLER_CLASS: u16 = 2;
pub const HANDSHAKE_ATTR_MESSAGE_TYPE: u16 = 3;
pub const HANDSHAKE_ATTR_TIMEOUT: u16 = 4;
pub const HANDSHAKE_ATTR_AUTH_MODE: u16 = 5;
pub const HANDSHAKE_ATTR_PEER_IDENTITY: u16 = 6;
pub const HANDSHAKE_ATTR_CERTIFICATE: u16 = 7;
pub const HANDSHAKE_ATTR_PEERNAME: u16 = 8;
pub const HANDSHAKE_ATTR_KEYRING: u16 = 9;
pub const HANDSHAKE_ATTR_STATUS: u16 = 10;

pub const HANDSHAKE_ATTR_X509_CERT: u16 = 1;
pub const HANDSHAKE_ATTR_X509_PRIVKEY: u16 = 2;

pub const HANDSHAKE_HANDLER_CLASS_NONE: u32 = 0;
pub const HANDSHAKE_HANDLER_CLASS_TLSHD: u32 = 1;
pub const HANDSHAKE_HANDLER_CLASS_MAX: u32 = 2;

pub const HANDSHAKE_MSG_TYPE_UNSPEC: u32 = 0;
pub const HANDSHAKE_MSG_TYPE_CLIENT_HELLO: u32 = 1;
pub const HANDSHAKE_MSG_TYPE_SERVER_HELLO: u32 = 2;

pub const HANDSHAKE_AUTH_UNSPEC: u32 = 0;
pub const HANDSHAKE_AUTH_UNAUTH: u32 = 1;
pub const HANDSHAKE_AUTH_PSK: u32 = 2;
pub const HANDSHAKE_AUTH_X509: u32 = 3;

static HANDSHAKE_AGENT_REGISTERED: AtomicBool = AtomicBool::new(false);

lazy_static::lazy_static! {
    static ref PENDING_HANDSHAKES: Arc<Mutex<BTreeMap<u64, PendingHandshake>>> = {
        Arc::new(Mutex::new(BTreeMap::new()))
    };
    static ref NEXT_REQ_ID: AtomicU64 = AtomicU64::new(1);
}

struct PendingHandshake {
    sockfd: i32,
    handler_class: u32,
    message_type: u32,
    timeout: u32,
    auth_mode: u32,
    peer_identity: Vec<u32>,
    certificates: Vec<X509Entry>,
    peername: Option<alloc::string::String>,
    keyring: u32,
}

struct X509Entry {
    cert: i32,
    privkey: i32,
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
struct GenlMsgHdr {
    cmd: u8,
    version: u8,
    reserved: u16,
}

impl GenlMsgHdr {
    fn new(cmd: u8) -> Self {
        GenlMsgHdr { cmd, version: HANDSHAKE_GENL_VERSION, reserved: 0 }
    }

    fn as_bytes(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self as *const Self as *const u8, 4) }
    }
}

fn find_attr_u32(payload: &[u8], attr_type: u16) -> Option<u32> {
    let mut pos = 0;
    while pos + 4 <= payload.len() {
        let len = u16::from_le_bytes([payload[pos], payload[pos + 1]]) as usize;
        let typ = u16::from_le_bytes([payload[pos + 2], payload[pos + 3]]);
        if len < 4 { break; }
        let d = pos + 4;
        let e = pos + len;
        if typ == attr_type && e <= payload.len() && e - d >= 4 {
            return Some(u32::from_ne_bytes([payload[d], payload[d+1], payload[d+2], payload[d+3]]));
        }
        if len == 0 { break; }
        pos += len;
    }
    None
}

fn find_attr_s32(payload: &[u8], attr_type: u16) -> Option<i32> {
    find_attr_u32(payload, attr_type).map(|v| v as i32)
}

fn find_attr_string<'a>(payload: &'a [u8], attr_type: u16) -> Option<&'a [u8]> {
    let mut pos = 0;
    while pos + 4 <= payload.len() {
        let len = u16::from_le_bytes([payload[pos], payload[pos + 1]]) as usize;
        let typ = u16::from_le_bytes([payload[pos + 2], payload[pos + 3]]);
        if len < 4 { break; }
        let d = pos + 4;
        let e = pos + len;
        if typ == attr_type && e <= payload.len() {
            return Some(&payload[d..e]);
        }
        if len == 0 { break; }
        pos += len;
    }
    None
}

fn find_attr_nested(payload: &[u8], attr_type: u16) -> Option<Vec<(u16, Vec<u8>)>> {
    let mut pos = 0;
    while pos + 4 <= payload.len() {
        let len = u16::from_le_bytes([payload[pos], payload[pos + 1]]) as usize;
        let typ = u16::from_le_bytes([payload[pos + 2], payload[pos + 3]]);
        if len < 4 { break; }
        let d = pos + 4;
        let e = pos + len;
        if typ == attr_type && e <= payload.len() {
            let mut nested = Vec::new();
            let mut np = d;
            while np + 4 <= e {
                let nlen = u16::from_le_bytes([payload[np], payload[np + 1]]) as usize;
                let ntyp = u16::from_le_bytes([payload[np + 2], payload[np + 3]]);
                if nlen < 4 { break; }
                let nd = np + 4;
                let ne = np + nlen;
                if ne > e { break; }
                nested.push((ntyp, payload[nd..ne].to_vec()));
                if nlen == 0 { break; }
                np += nlen;
            }
            return Some(nested);
        }
        if len == 0 { break; }
        pos += len;
    }
    None
}

fn find_attr_multi_u32(payload: &[u8], attr_type: u16) -> Vec<u32> {
    let mut result = Vec::new();
    let mut pos = 0;
    while pos + 4 <= payload.len() {
        let len = u16::from_le_bytes([payload[pos], payload[pos + 1]]) as usize;
        let typ = u16::from_le_bytes([payload[pos + 2], payload[pos + 3]]);
        if len < 4 { break; }
        let d = pos + 4;
        let e = pos + len;
        if typ == attr_type && e <= payload.len() && e - d >= 4 {
            result.push(u32::from_ne_bytes([payload[d], payload[d+1], payload[d+2], payload[d+3]]));
        }
        if len == 0 { break; }
        pos += len;
    }
    result
}

fn handle_accept(src_pid: u32, seq: u32, attr_payload: &[u8]) {
    let handler_class = find_attr_u32(attr_payload, HANDSHAKE_ATTR_HANDLER_CLASS).unwrap_or(1);

    HANDSHAKE_AGENT_REGISTERED.store(true, Ordering::Relaxed);

    let mut queue = PENDING_HANDSHAKES.lock();
    let (req_id, pending) = if let Some(next) = queue.iter().next() {
        (next.0, next.1)
    } else {
        let empty_payload = Vec::new();
        send_response(src_pid, seq, HANDSHAKE_CMD_ACCEPT, &empty_payload, true);
        return;
    };

    let req_id = *req_id;
    let sockfd = pending.sockfd;
    let msg_type = pending.message_type;
    let timeout = pending.timeout;
    let auth_mode = pending.auth_mode;
    let peername = pending.peername.clone().unwrap_or_default();

    let mut reply = Vec::new();
    reply.extend_from_slice(&NlAttr::new(HANDSHAKE_ATTR_SOCKFD, &sockfd.to_ne_bytes()));
    reply.extend_from_slice(&NlAttr::new(HANDSHAKE_ATTR_MESSAGE_TYPE, &msg_type.to_ne_bytes()));
    reply.extend_from_slice(&NlAttr::new(HANDSHAKE_ATTR_TIMEOUT, &timeout.to_ne_bytes()));
    reply.extend_from_slice(&NlAttr::new(HANDSHAKE_ATTR_AUTH_MODE, &auth_mode.to_ne_bytes()));
    if !peername.is_empty() {
        reply.extend_from_slice(&NlAttr::new(HANDSHAKE_ATTR_PEERNAME, peername.as_bytes()));
    }
    reply.extend_from_slice(&NlAttr::new(HANDSHAKE_ATTR_KEYRING, &pending.keyring.to_ne_bytes()));

    send_response(src_pid, seq, HANDSHAKE_CMD_ACCEPT, &reply, false);
    queue.remove(&req_id);
}

fn handle_done(src_pid: u32, seq: u32, attr_payload: &[u8]) {
    let _status = find_attr_u32(attr_payload, HANDSHAKE_ATTR_STATUS).unwrap_or(0);
    let _sockfd = find_attr_s32(attr_payload, HANDSHAKE_ATTR_SOCKFD).unwrap_or(-1);

    send_response(src_pid, seq, HANDSHAKE_CMD_DONE, &[], true);
}

fn handle_ready(src_pid: u32, seq: u32, _attr_payload: &[u8]) {
    send_response(src_pid, seq, HANDSHAKE_CMD_READY, &[], true);
}

fn send_response(src_pid: u32, seq: u32, cmd: u8, payload: &[u8], is_done: bool) {
    let mut inner = Vec::new();
    let ghdr_cmd = if is_done { 0u8 } else { cmd };
    let ghdr = GenlMsgHdr::new(ghdr_cmd);
    inner.extend_from_slice(ghdr.as_bytes());
    inner.extend_from_slice(payload);

    let msg_type = if is_done { 3u16 } else { HANDSHAKE_GENL_ID };
    let total_len = (core::mem::size_of::<NlMsgHdr>() + inner.len()) as u32;
    let reply_hdr = NlMsgHdr::new(
        total_len,
        msg_type,
        if is_done { 0 } else { 2u16 },
        seq,
        0,
    );
    let reply_msg = crate::net::netlink::NetlinkMessage {
        header: reply_hdr,
        payload: inner,
    };

    if src_pid != 0 {
        if let Some(sock) = NETLINK_MANAGER.get_socket(src_pid) {
            sock.rx_buf.lock().push(reply_msg);
        }
    }
}

pub fn enqueue_handshake(
    sockfd: i32,
    handler_class: u32,
    message_type: u32,
    timeout: u32,
    auth_mode: u32,
    peername: Option<&str>,
    keyring: u32,
) -> u64 {
    let id = NEXT_REQ_ID.fetch_add(1, Ordering::Relaxed);
    let pending = PendingHandshake {
        sockfd,
        handler_class,
        message_type,
        timeout,
        auth_mode,
        peer_identity: Vec::new(),
        certificates: Vec::new(),
        peername: peername.map(|s| alloc::string::String::from(s)),
        keyring,
    };
    PENDING_HANDSHAKES.lock().insert(id, pending);
    id
}

pub fn handle_handshake_genl_request(
    src_pid: u32,
    seq: u32,
    payload: &[u8],
) {
    if payload.len() < 4 { return; }

    let hdr = unsafe { &*(payload.as_ptr() as *const GenlMsgHdr) };
    let cmd = hdr.cmd;
    let attr_payload = &payload[4..];

    match cmd {
        HANDSHAKE_CMD_READY => handle_ready(src_pid, seq, attr_payload),
        HANDSHAKE_CMD_ACCEPT => handle_accept(src_pid, seq, attr_payload),
        HANDSHAKE_CMD_DONE => handle_done(src_pid, seq, attr_payload),
        _ => send_response(src_pid, seq, 0, &[], true),
    }
}

pub fn is_agent_registered() -> bool {
    HANDSHAKE_AGENT_REGISTERED.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handshake_constants() {
        assert_eq!(HANDSHAKE_GENL_ID, 47);
        assert_eq!(HANDSHAKE_GENL_VERSION, 1);
        assert_eq!(HANDSHAKE_CMD_READY, 0);
        assert_eq!(HANDSHAKE_CMD_ACCEPT, 1);
        assert_eq!(HANDSHAKE_CMD_DONE, 2);
        assert_eq!(HANDSHAKE_HANDLER_CLASS_NONE, 0);
        assert_eq!(HANDSHAKE_HANDLER_CLASS_TLSHD, 1);
        assert_eq!(HANDSHAKE_HANDLER_CLASS_MAX, 2);
        assert_eq!(HANDSHAKE_MSG_TYPE_UNSPEC, 0);
        assert_eq!(HANDSHAKE_MSG_TYPE_CLIENT_HELLO, 1);
        assert_eq!(HANDSHAKE_MSG_TYPE_SERVER_HELLO, 2);
        assert_eq!(HANDSHAKE_AUTH_UNSPEC, 0);
        assert_eq!(HANDSHAKE_AUTH_UNAUTH, 1);
        assert_eq!(HANDSHAKE_AUTH_PSK, 2);
        assert_eq!(HANDSHAKE_AUTH_X509, 3);
    }

    #[test]
    fn test_find_attr_u32() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&NlAttr::new(5, &42u32.to_ne_bytes()));
        assert_eq!(find_attr_u32(&buf, 5), Some(42));
        assert_eq!(find_attr_u32(&buf, 99), None);
    }

    #[test]
    fn test_find_attr_s32() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&NlAttr::new(1, &(-7i32).to_ne_bytes()));
        assert_eq!(find_attr_s32(&buf, 1), Some(-7));
    }

    #[test]
    fn test_find_attr_string() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&NlAttr::new(8, b"example.com"));
        assert_eq!(find_attr_string(&buf, 8), Some(&b"example.com"[..]));
    }

    #[test]
    fn test_enqueue_dequeue() {
        let id = enqueue_handshake(3, 1, 1, 5000, 3, Some("example.com"), 0);
        assert!(id > 0);

        let queue = PENDING_HANDSHAKES.lock();
        assert!(queue.contains_key(&id));
        let entry = queue.get(&id).unwrap();
        assert_eq!(entry.sockfd, 3);
        assert_eq!(entry.handler_class, 1);
        assert_eq!(entry.message_type, 1);
        assert_eq!(entry.timeout, 5000);
        assert_eq!(entry.auth_mode, 3);
        assert_eq!(entry.peername.as_deref(), Some("example.com"));
    }
}
