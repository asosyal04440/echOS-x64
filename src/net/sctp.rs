//! Stream Control Transmission Protocol (SCTP) core state machine.

use super::{IpAddr, Ipv4Addr, NetError, Port, SocketAddr};
use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

const CHUNK_DATA: u8 = 0;
const CHUNK_INIT: u8 = 1;
const CHUNK_INIT_ACK: u8 = 2;
const CHUNK_SACK: u8 = 3;
const CHUNK_HEARTBEAT: u8 = 4;
const CHUNK_HEARTBEAT_ACK: u8 = 5;
const CHUNK_ABORT: u8 = 6;
const CHUNK_COOKIE_ECHO: u8 = 10;
const CHUNK_COOKIE_ACK: u8 = 11;
const CHUNK_SHUTDOWN: u8 = 7;
const CHUNK_SHUTDOWN_ACK: u8 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SctpState {
    Closed,
    CookieWait,
    CookieEchoed,
    Established,
    ShutdownPending,
    ShutdownSent,
    ShutdownReceived,
    ShutdownAckSent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SctpMessage {
    pub stream_id: u16,
    pub stream_seq: u16,
    pub ppid: u32,
    pub unordered: bool,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SctpAssociation {
    pub id: u32,
    pub local_addrs: Vec<IpAddr>,
    pub peer_addrs: Vec<IpAddr>,
    pub local_port: Port,
    pub peer_port: Port,
    pub state: SctpState,
    pub verification_tag: u32,
    pub peer_verification_tag: u32,
    pub next_tsn: u32,
    pub expected_tsn: u32,
    pub outbound_streams: u16,
    pub inbound_streams: u16,
    pub reassembly: BTreeMap<u32, Vec<u8>>,
    pub partial_messages: BTreeMap<(u16, u16), Vec<u8>>,
    pub outbound_stream_seq: BTreeMap<u16, u16>,
    pub inbound: VecDeque<SctpMessage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SctpPacket {
    pub src_port: Port,
    pub dst_port: Port,
    pub verification_tag: u32,
    pub chunks: Vec<SctpChunk>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SctpChunk {
    Init {
        initiate_tag: u32,
        a_rwnd: u32,
        outbound_streams: u16,
        inbound_streams: u16,
        initial_tsn: u32,
        addrs: Vec<IpAddr>,
    },
    InitAck {
        initiate_tag: u32,
        a_rwnd: u32,
        outbound_streams: u16,
        inbound_streams: u16,
        initial_tsn: u32,
        cookie: Vec<u8>,
        addrs: Vec<IpAddr>,
    },
    CookieEcho(Vec<u8>),
    CookieAck,
    Sack {
        cumulative_tsn_ack: u32,
    },
    Data {
        tsn: u32,
        stream_id: u16,
        stream_seq: u16,
        ppid: u32,
        begin: bool,
        end: bool,
        unordered: bool,
        payload: Vec<u8>,
    },
    Shutdown {
        cumulative_tsn_ack: u32,
    },
    ShutdownAck,
    Heartbeat(Vec<u8>),
    HeartbeatAck(Vec<u8>),
    Abort(Vec<u8>),
}

impl SctpPacket {
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.src_port.0.to_be_bytes());
        out.extend_from_slice(&self.dst_port.0.to_be_bytes());
        out.extend_from_slice(&self.verification_tag.to_be_bytes());
        out.extend_from_slice(&0u32.to_be_bytes());
        for chunk in &self.chunks {
            serialize_chunk(chunk, &mut out);
        }
        let crc = crc32c(&out);
        out[8..12].copy_from_slice(&crc.to_be_bytes());
        out
    }

    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 12 {
            return None;
        }
        let expected_crc = u32::from_be_bytes(data[8..12].try_into().ok()?);
        let mut tmp = data.to_vec();
        tmp[8..12].copy_from_slice(&0u32.to_be_bytes());
        if crc32c(&tmp) != expected_crc {
            return None;
        }
        let mut pos = 12;
        let mut chunks = Vec::new();
        while pos + 4 <= data.len() {
            let kind = data[pos];
            let flags = data[pos + 1];
            let len = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
            if len < 4 || pos + len > data.len() {
                return None;
            }
            let body = &data[pos + 4..pos + len];
            chunks.push(parse_chunk(kind, flags, body)?);
            pos += (len + 3) & !3;
        }
        Some(Self {
            src_port: Port(u16::from_be_bytes([data[0], data[1]])),
            dst_port: Port(u16::from_be_bytes([data[2], data[3]])),
            verification_tag: u32::from_be_bytes(data[4..8].try_into().ok()?),
            chunks,
        })
    }
}

fn serialize_addr(addrs: &[IpAddr], out: &mut Vec<u8>) {
    for addr in addrs {
        match addr {
            IpAddr::V4(ip) => {
                out.extend_from_slice(&5u16.to_be_bytes());
                out.extend_from_slice(&8u16.to_be_bytes());
                out.extend_from_slice(&ip.0);
            }
            IpAddr::V6(ip) => {
                out.extend_from_slice(&6u16.to_be_bytes());
                out.extend_from_slice(&20u16.to_be_bytes());
                for seg in ip.segments() {
                    out.extend_from_slice(&seg.to_be_bytes());
                }
            }
        }
    }
}

fn parse_addrs(mut body: &[u8]) -> Vec<IpAddr> {
    let mut out = Vec::new();
    while body.len() >= 4 {
        let kind = u16::from_be_bytes([body[0], body[1]]);
        let len = u16::from_be_bytes([body[2], body[3]]) as usize;
        if len < 4 || len > body.len() {
            break;
        }
        let payload = &body[4..len];
        match kind {
            5 if payload.len() >= 4 => out.push(IpAddr::V4(Ipv4Addr([payload[0], payload[1], payload[2], payload[3]]))),
            6 if payload.len() >= 16 => {
                let mut seg = [0u16; 8];
                for (i, slot) in seg.iter_mut().enumerate() {
                    *slot = u16::from_be_bytes([payload[i * 2], payload[i * 2 + 1]]);
                }
                out.push(IpAddr::V6(super::ipv6::Ipv6Addr::from_segments(seg)));
            }
            _ => {}
        }
        body = &body[(len + 3) & !3..];
    }
    out
}

fn serialize_chunk(chunk: &SctpChunk, out: &mut Vec<u8>) {
    let start = out.len();
    out.resize(start + 4, 0);
    match chunk {
        SctpChunk::Init {
            initiate_tag,
            a_rwnd,
            outbound_streams,
            inbound_streams,
            initial_tsn,
            addrs,
        }
        | SctpChunk::InitAck {
            initiate_tag,
            a_rwnd,
            outbound_streams,
            inbound_streams,
            initial_tsn,
            addrs,
            ..
        } => {
            out.extend_from_slice(&initiate_tag.to_be_bytes());
            out.extend_from_slice(&a_rwnd.to_be_bytes());
            out.extend_from_slice(&outbound_streams.to_be_bytes());
            out.extend_from_slice(&inbound_streams.to_be_bytes());
            out.extend_from_slice(&initial_tsn.to_be_bytes());
            if let SctpChunk::InitAck { cookie, .. } = chunk {
                out.extend_from_slice(&7u16.to_be_bytes());
                out.extend_from_slice(&((cookie.len() + 4) as u16).to_be_bytes());
                out.extend_from_slice(cookie);
                while out.len() % 4 != 0 {
                    out.push(0);
                }
            }
            serialize_addr(addrs, out);
        }
        SctpChunk::CookieEcho(cookie) => out.extend_from_slice(cookie),
        SctpChunk::CookieAck => {}
        SctpChunk::Sack { cumulative_tsn_ack } | SctpChunk::Shutdown { cumulative_tsn_ack } => {
            out.extend_from_slice(&cumulative_tsn_ack.to_be_bytes());
            if matches!(chunk, SctpChunk::Sack { .. }) {
                out.extend_from_slice(&0u32.to_be_bytes());
                out.extend_from_slice(&0u16.to_be_bytes());
                out.extend_from_slice(&0u16.to_be_bytes());
            }
        }
        SctpChunk::ShutdownAck => {}
        SctpChunk::Heartbeat(info) | SctpChunk::HeartbeatAck(info) | SctpChunk::Abort(info) => {
            out.extend_from_slice(info);
        }
        SctpChunk::Data {
            tsn,
            stream_id,
            stream_seq,
            ppid,
            begin,
            end,
            unordered,
            payload,
        } => {
            out.extend_from_slice(&tsn.to_be_bytes());
            out.extend_from_slice(&stream_id.to_be_bytes());
            out.extend_from_slice(&stream_seq.to_be_bytes());
            out.extend_from_slice(&ppid.to_be_bytes());
            out.extend_from_slice(payload);
            out[start + 1] = (if *unordered { 0x04 } else { 0 })
                | (if *begin { 0x02 } else { 0 })
                | (if *end { 0x01 } else { 0 });
        }
    }

    out[start] = match chunk {
        SctpChunk::Init { .. } => CHUNK_INIT,
        SctpChunk::InitAck { .. } => CHUNK_INIT_ACK,
        SctpChunk::CookieEcho(_) => CHUNK_COOKIE_ECHO,
        SctpChunk::CookieAck => CHUNK_COOKIE_ACK,
        SctpChunk::Sack { .. } => CHUNK_SACK,
        SctpChunk::Data { .. } => CHUNK_DATA,
        SctpChunk::Shutdown { .. } => CHUNK_SHUTDOWN,
        SctpChunk::ShutdownAck => CHUNK_SHUTDOWN_ACK,
        SctpChunk::Heartbeat(_) => CHUNK_HEARTBEAT,
        SctpChunk::HeartbeatAck(_) => CHUNK_HEARTBEAT_ACK,
        SctpChunk::Abort(_) => CHUNK_ABORT,
    };

    let len = (out.len() - start) as u16;
    out[start + 2..start + 4].copy_from_slice(&len.to_be_bytes());
    while out.len() % 4 != 0 {
        out.push(0);
    }
}

fn parse_chunk(kind: u8, flags: u8, body: &[u8]) -> Option<SctpChunk> {
    match kind {
        CHUNK_INIT | CHUNK_INIT_ACK if body.len() >= 16 => {
            let initiate_tag = u32::from_be_bytes(body[0..4].try_into().ok()?);
            let a_rwnd = u32::from_be_bytes(body[4..8].try_into().ok()?);
            let outbound_streams = u16::from_be_bytes(body[8..10].try_into().ok()?);
            let inbound_streams = u16::from_be_bytes(body[10..12].try_into().ok()?);
            let initial_tsn = u32::from_be_bytes(body[12..16].try_into().ok()?);
            let mut rest = &body[16..];
            let mut cookie = Vec::new();
            if kind == CHUNK_INIT_ACK && rest.len() >= 4 && u16::from_be_bytes([rest[0], rest[1]]) == 7 {
                let len = u16::from_be_bytes([rest[2], rest[3]]) as usize;
                cookie.extend_from_slice(&rest[4..len]);
                rest = &rest[(len + 3) & !3..];
            }
            let addrs = parse_addrs(rest);
            Some(if kind == CHUNK_INIT {
                SctpChunk::Init {
                    initiate_tag,
                    a_rwnd,
                    outbound_streams,
                    inbound_streams,
                    initial_tsn,
                    addrs,
                }
            } else {
                SctpChunk::InitAck {
                    initiate_tag,
                    a_rwnd,
                    outbound_streams,
                    inbound_streams,
                    initial_tsn,
                    cookie,
                    addrs,
                }
            })
        }
        CHUNK_COOKIE_ECHO => Some(SctpChunk::CookieEcho(body.to_vec())),
        CHUNK_COOKIE_ACK => Some(SctpChunk::CookieAck),
        CHUNK_SACK if body.len() >= 4 => Some(SctpChunk::Sack {
            cumulative_tsn_ack: u32::from_be_bytes(body[0..4].try_into().ok()?),
        }),
        CHUNK_SHUTDOWN if body.len() >= 4 => Some(SctpChunk::Shutdown {
            cumulative_tsn_ack: u32::from_be_bytes(body[0..4].try_into().ok()?),
        }),
        CHUNK_SHUTDOWN_ACK => Some(SctpChunk::ShutdownAck),
        CHUNK_HEARTBEAT => Some(SctpChunk::Heartbeat(body.to_vec())),
        CHUNK_HEARTBEAT_ACK => Some(SctpChunk::HeartbeatAck(body.to_vec())),
        CHUNK_ABORT => Some(SctpChunk::Abort(body.to_vec())),
        CHUNK_DATA if body.len() >= 12 => Some(SctpChunk::Data {
            tsn: u32::from_be_bytes(body[0..4].try_into().ok()?),
            stream_id: u16::from_be_bytes(body[4..6].try_into().ok()?),
            stream_seq: u16::from_be_bytes(body[6..8].try_into().ok()?),
            ppid: u32::from_be_bytes(body[8..12].try_into().ok()?),
            begin: flags & 0x02 != 0,
            end: flags & 0x01 != 0,
            unordered: flags & 0x04 != 0,
            payload: body[12..].to_vec(),
        }),
        _ => None,
    }
}

fn crc32c(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0x82f63b78 & mask);
        }
    }
    !crc
}

static NEXT_ASSOC_ID: AtomicU32 = AtomicU32::new(1);
static ASSOCIATIONS: Mutex<BTreeMap<u32, SctpAssociation>> = Mutex::new(BTreeMap::new());

pub fn open(local_addrs: Vec<IpAddr>, local_port: Port, outbound_streams: u16, inbound_streams: u16) -> u32 {
    let id = NEXT_ASSOC_ID.fetch_add(1, Ordering::Relaxed);
    let assoc = SctpAssociation {
        id,
        local_addrs,
        peer_addrs: Vec::new(),
        local_port,
        peer_port: Port(0),
        state: SctpState::Closed,
        verification_tag: 0xECA00000u32.wrapping_add(id),
        peer_verification_tag: 0,
        next_tsn: 1,
        expected_tsn: 1,
        outbound_streams,
        inbound_streams,
        reassembly: BTreeMap::new(),
        partial_messages: BTreeMap::new(),
        outbound_stream_seq: BTreeMap::new(),
        inbound: VecDeque::new(),
    };
    ASSOCIATIONS.lock().insert(id, assoc);
    id
}

pub fn bindx(assoc_id: u32, addrs: &[IpAddr], add: bool) -> Result<(), NetError> {
    let mut assocs = ASSOCIATIONS.lock();
    let assoc = assocs.get_mut(&assoc_id).ok_or(NetError::InvalidFd)?;
    if add {
        for addr in addrs {
            if !assoc.local_addrs.iter().any(|a| a == addr) {
                assoc.local_addrs.push(*addr);
            }
        }
    } else {
        assoc.local_addrs.retain(|addr| !addrs.iter().any(|a| a == addr));
        if assoc.local_addrs.is_empty() {
            return Err(NetError::InvalidParam);
        }
    }
    Ok(())
}

pub fn initiate(assoc_id: u32, peer_addrs: Vec<IpAddr>, peer_port: Port) -> Result<SctpPacket, NetError> {
    let mut assocs = ASSOCIATIONS.lock();
    let assoc = assocs.get_mut(&assoc_id).ok_or(NetError::InvalidFd)?;
    assoc.peer_addrs = peer_addrs.clone();
    assoc.peer_port = peer_port;
    assoc.state = SctpState::CookieWait;
    Ok(SctpPacket {
        src_port: assoc.local_port,
        dst_port: peer_port,
        verification_tag: 0,
        chunks: vec![SctpChunk::Init {
            initiate_tag: assoc.verification_tag,
            a_rwnd: 65_535,
            outbound_streams: assoc.outbound_streams,
            inbound_streams: assoc.inbound_streams,
            initial_tsn: assoc.next_tsn,
            addrs: assoc.local_addrs.clone(),
        }],
    })
}

pub fn handle_packet(assoc_id: u32, packet: &SctpPacket) -> Result<Option<SctpPacket>, NetError> {
    let mut assocs = ASSOCIATIONS.lock();
    let assoc = assocs.get_mut(&assoc_id).ok_or(NetError::InvalidFd)?;
    let mut reply_chunks = Vec::new();
    let init_only = packet.chunks.iter().all(|chunk| matches!(chunk, SctpChunk::Init { .. }));
    if !init_only && packet.verification_tag != assoc.verification_tag {
        return Err(NetError::InvalidPacket);
    }

    for chunk in &packet.chunks {
        match chunk {
            SctpChunk::Init {
                initiate_tag,
                outbound_streams,
                inbound_streams,
                initial_tsn,
                addrs,
                ..
            } => {
                assoc.peer_verification_tag = *initiate_tag;
                assoc.peer_addrs = addrs.clone();
                assoc.peer_port = packet.src_port;
                assoc.expected_tsn = *initial_tsn;
                let cookie = build_cookie(assoc.id, *initiate_tag, *initial_tsn);
                reply_chunks.push(SctpChunk::InitAck {
                    initiate_tag: assoc.verification_tag,
                    a_rwnd: 65_535,
                    outbound_streams: assoc.outbound_streams.max(*inbound_streams),
                    inbound_streams: assoc.inbound_streams.max(*outbound_streams),
                    initial_tsn: assoc.next_tsn,
                    cookie,
                    addrs: assoc.local_addrs.clone(),
                });
            }
            SctpChunk::InitAck {
                initiate_tag,
                initial_tsn,
                cookie,
                addrs,
                ..
            } => {
                assoc.peer_verification_tag = *initiate_tag;
                assoc.expected_tsn = *initial_tsn;
                assoc.peer_addrs = addrs.clone();
                assoc.state = SctpState::CookieEchoed;
                reply_chunks.push(SctpChunk::CookieEcho(cookie.clone()));
            }
            SctpChunk::CookieEcho(cookie) => {
                let parsed = parse_cookie(cookie).ok_or(NetError::InvalidPacket)?;
                if parsed.0 != assoc.id || parsed.1 != assoc.peer_verification_tag || parsed.2 != assoc.expected_tsn {
                    return Err(NetError::InvalidPacket);
                }
                assoc.peer_verification_tag = parsed.1;
                assoc.expected_tsn = parsed.2;
                assoc.state = SctpState::Established;
                reply_chunks.push(SctpChunk::CookieAck);
            }
            SctpChunk::CookieAck => {
                assoc.state = SctpState::Established;
            }
            SctpChunk::Data {
                tsn,
                stream_id,
                stream_seq,
                ppid,
                begin,
                end,
                unordered,
                payload,
                ..
            } => {
                let entry = assoc.reassembly.entry(*tsn).or_insert_with(Vec::new);
                entry.extend_from_slice(payload);
                let key = (*stream_id, *stream_seq);
                if *begin {
                    assoc.partial_messages.insert(key, Vec::new());
                }
                assoc.partial_messages
                    .entry(key)
                    .or_insert_with(Vec::new)
                    .extend_from_slice(payload);
                if *tsn == assoc.expected_tsn {
                    assoc.expected_tsn = assoc.expected_tsn.wrapping_add(1);
                }
                if *end {
                    assoc.inbound.push_back(SctpMessage {
                        stream_id: *stream_id,
                        stream_seq: *stream_seq,
                        ppid: *ppid,
                        unordered: *unordered,
                        payload: assoc.partial_messages.remove(&key).unwrap_or_else(|| entry.clone()),
                    });
                }
                assoc.reassembly.remove(tsn);
                reply_chunks.push(SctpChunk::Sack {
                    cumulative_tsn_ack: assoc.expected_tsn.saturating_sub(1),
                });
            }
            SctpChunk::Shutdown { cumulative_tsn_ack } => {
                assoc.expected_tsn = cumulative_tsn_ack.wrapping_add(1);
                assoc.state = SctpState::ShutdownReceived;
                reply_chunks.push(SctpChunk::ShutdownAck);
            }
            SctpChunk::ShutdownAck => {
                assoc.state = SctpState::Closed;
            }
            SctpChunk::Heartbeat(info) => {
                reply_chunks.push(SctpChunk::HeartbeatAck(info.clone()));
            }
            SctpChunk::HeartbeatAck(_) => {}
            SctpChunk::Abort(_) => {
                assoc.state = SctpState::Closed;
                assoc.inbound.clear();
                assoc.partial_messages.clear();
                assoc.reassembly.clear();
            }
            SctpChunk::Sack { .. } => {}
        }
    }

    if reply_chunks.is_empty() {
        Ok(None)
    } else {
        Ok(Some(SctpPacket {
            src_port: assoc.local_port,
            dst_port: packet.src_port,
            verification_tag: assoc.peer_verification_tag,
            chunks: reply_chunks,
        }))
    }
}

fn build_cookie(assoc_id: u32, peer_tag: u32, initial_tsn: u32) -> Vec<u8> {
    let mut cookie = Vec::with_capacity(12);
    cookie.extend_from_slice(&assoc_id.to_be_bytes());
    cookie.extend_from_slice(&peer_tag.to_be_bytes());
    cookie.extend_from_slice(&initial_tsn.to_be_bytes());
    cookie
}

fn parse_cookie(cookie: &[u8]) -> Option<(u32, u32, u32)> {
    if cookie.len() < 12 {
        return None;
    }
    Some((
        u32::from_be_bytes(cookie[0..4].try_into().ok()?),
        u32::from_be_bytes(cookie[4..8].try_into().ok()?),
        u32::from_be_bytes(cookie[8..12].try_into().ok()?),
    ))
}

pub fn sendmsg(
    assoc_id: u32,
    stream_id: u16,
    ppid: u32,
    unordered: bool,
    payload: &[u8],
    mtu: usize,
) -> Result<Vec<SctpPacket>, NetError> {
    let mut assocs = ASSOCIATIONS.lock();
    let assoc = assocs.get_mut(&assoc_id).ok_or(NetError::InvalidFd)?;
    if assoc.state != SctpState::Established {
        return Err(NetError::NotConnected);
    }
    let chunk_payload = mtu.saturating_sub(28).max(1);
    let mut packets = Vec::new();
    let mut offset = 0usize;
    let stream_seq = {
        let seq = assoc.outbound_stream_seq.entry(stream_id).or_insert(0);
        let current = *seq;
        *seq = seq.wrapping_add(1);
        current
    };
    while offset < payload.len() {
        let end = core::cmp::min(offset + chunk_payload, payload.len());
        let packet = SctpPacket {
            src_port: assoc.local_port,
            dst_port: assoc.peer_port,
            verification_tag: assoc.peer_verification_tag,
            chunks: vec![SctpChunk::Data {
                tsn: assoc.next_tsn,
                stream_id,
                stream_seq,
                ppid,
                begin: offset == 0,
                end: end == payload.len(),
                unordered,
                payload: payload[offset..end].to_vec(),
            }],
        };
        assoc.next_tsn = assoc.next_tsn.wrapping_add(1);
        offset = end;
        packets.push(packet);
    }
    Ok(packets)
}

pub fn recvmsg(assoc_id: u32) -> Result<SctpMessage, NetError> {
    let mut assocs = ASSOCIATIONS.lock();
    let assoc = assocs.get_mut(&assoc_id).ok_or(NetError::InvalidFd)?;
    assoc.inbound.pop_front().ok_or(NetError::WouldBlock)
}

pub fn shutdown(assoc_id: u32) -> Result<SctpPacket, NetError> {
    let mut assocs = ASSOCIATIONS.lock();
    let assoc = assocs.get_mut(&assoc_id).ok_or(NetError::InvalidFd)?;
    assoc.state = SctpState::ShutdownSent;
    Ok(SctpPacket {
        src_port: assoc.local_port,
        dst_port: assoc.peer_port,
        verification_tag: assoc.peer_verification_tag,
        chunks: vec![SctpChunk::Shutdown {
            cumulative_tsn_ack: assoc.expected_tsn.saturating_sub(1),
        }],
    })
}

pub fn heartbeat(assoc_id: u32) -> Result<SctpPacket, NetError> {
    let mut assocs = ASSOCIATIONS.lock();
    let assoc = assocs.get_mut(&assoc_id).ok_or(NetError::InvalidFd)?;
    if assoc.state != SctpState::Established {
        return Err(NetError::NotConnected);
    }
    let mut info = Vec::new();
    if let Some(addr) = assoc.peer_addrs.first() {
        match addr {
            IpAddr::V4(ip) => info.extend_from_slice(&ip.0),
            IpAddr::V6(ip) => info.extend_from_slice(ip.as_bytes()),
        }
    }
    info.extend_from_slice(&assoc.expected_tsn.to_be_bytes());
    Ok(SctpPacket {
        src_port: assoc.local_port,
        dst_port: assoc.peer_port,
        verification_tag: assoc.peer_verification_tag,
        chunks: vec![SctpChunk::Heartbeat(info)],
    })
}

pub fn abort(assoc_id: u32, cause: &[u8]) -> Result<SctpPacket, NetError> {
    let mut assocs = ASSOCIATIONS.lock();
    let assoc = assocs.get_mut(&assoc_id).ok_or(NetError::InvalidFd)?;
    assoc.state = SctpState::Closed;
    Ok(SctpPacket {
        src_port: assoc.local_port,
        dst_port: assoc.peer_port,
        verification_tag: assoc.peer_verification_tag,
        chunks: vec![SctpChunk::Abort(cause.to_vec())],
    })
}

pub fn close(assoc_id: u32) -> Result<(), NetError> {
    ASSOCIATIONS.lock().remove(&assoc_id).map(|_| ()).ok_or(NetError::InvalidFd)
}

pub fn get_association(assoc_id: u32) -> Option<SctpAssociation> {
    ASSOCIATIONS.lock().get(&assoc_id).cloned()
}

pub fn local_primary_addr(assoc_id: u32) -> Option<SocketAddr> {
    let assoc = ASSOCIATIONS.lock().get(&assoc_id).cloned()?;
    Some(SocketAddr::new(*assoc.local_addrs.first()?, assoc.local_port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sctp_handshake_and_message_flow() {
        let server = open(vec![IpAddr::V4(Ipv4Addr([10, 0, 0, 1]))], Port(5000), 8, 8);
        let client = open(vec![IpAddr::V4(Ipv4Addr([10, 0, 0, 2]))], Port(5001), 8, 8);

        let init = initiate(client, vec![IpAddr::V4(Ipv4Addr([10, 0, 0, 1]))], Port(5000)).unwrap();
        let init_ack = handle_packet(server, &init).unwrap().unwrap();
        let cookie_echo = handle_packet(client, &init_ack).unwrap().unwrap();
        let cookie_ack = handle_packet(server, &cookie_echo).unwrap().unwrap();
        handle_packet(client, &cookie_ack).unwrap();

        assert_eq!(get_association(client).unwrap().state, SctpState::Established);
        assert_eq!(get_association(server).unwrap().state, SctpState::Established);

        let data_packets = sendmsg(client, 1, 42, false, b"hello-sctp", 64).unwrap();
        for pkt in data_packets {
            let sack = handle_packet(server, &pkt).unwrap().unwrap();
            let _ = handle_packet(client, &sack).unwrap();
        }
        let msg = recvmsg(server).unwrap();
        assert_eq!(msg.stream_seq, 0);
        assert_eq!(msg.payload, b"hello-sctp");
    }
}
