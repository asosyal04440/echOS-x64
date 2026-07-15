use super::NetError;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

pub const TCP_ESTABLISHED: u8 = 1;
pub const TCP_SYN_SENT: u8 = 2;
pub const TCP_SYN_RECV: u8 = 3;
pub const TCP_FIN_WAIT1: u8 = 4;
pub const TCP_FIN_WAIT2: u8 = 5;
pub const TCP_TIME_WAIT: u8 = 6;
pub const TCP_CLOSE: u8 = 7;
pub const TCP_CLOSE_WAIT: u8 = 8;
pub const TCP_LAST_ACK: u8 = 9;
pub const TCP_LISTEN: u8 = 10;
pub const TCP_CLOSING: u8 = 11;

#[allow(non_upper_case_globals)]
pub mod ca {
    pub const TCP_CA_Open: u8 = 0;
    pub const TCP_CA_Disorder: u8 = 1;
    pub const TCP_CA_CWR: u8 = 2;
    pub const TCP_CA_Recovery: u8 = 3;
    pub const TCP_CA_Loss: u8 = 4;
    pub const TCP_CA_Unknown: u8 = 0xFF;
}
pub use ca::*;

pub const TCP_INFO: i32 = 11;
pub const IP_TOS: i32 = 1;
pub const IPV6_TCLASS: i32 = 36;
pub const SO_ZEROCOPY: i32 = 60;

pub const TCP_INFO_SIZE: usize = 192;

const TCPI_OPT_SYN_SEEN: u8 = 1;

#[derive(Clone, Copy, Debug, Default)]
#[repr(C)]
pub struct TcpInfo {
    pub tcpi_state: u8,
    pub tcpi_ca_state: u8,
    pub tcpi_retransmits: u8,
    pub tcpi_probes: u8,
    pub tcpi_fack_out: u8,
    pub tcpi_options: u8,
    pub tcpi_snd_wscale: u8,
    pub tcpi_rcv_wscale: u8,
    pub tcpi_rto: u32,
    pub tcpi_ato: u32,
    pub tcpi_snd_mss: u16,
    pub tcpi_rcv_mss: u16,
    pub tcpi_unacked: u32,
    pub tcpi_sacked: u32,
    pub tcpi_lost: u32,
    pub tcpi_retrans: u32,
    pub tcpi_fackets: u32,
    pub tcpi_last_data_sent: u32,
    pub tcpi_last_ack_sent: u32,
    pub tcpi_last_data_recv: u32,
    pub tcpi_last_ack_recv: u32,
    pub tcpi_pmtu: u32,
    pub tcpi_rcv_ssthresh: u32,
    pub tcpi_rtt: u32,
    pub tcpi_rttvar: u32,
    pub tcpi_snd_ssthresh: u32,
    pub tcpi_snd_cwnd: u32,
    pub tcpi_advmss: u16,
    pub tcpi_reordering: u16,
    pub tcpi_rcv_wnd: u32,
    pub tcpi_snd_wnd: u32,
    pub tcpi_bytes_acked: u64,
    pub tcpi_bytes_received: u64,
    pub tcpi_segs_out: u32,
    pub tcpi_segs_in: u32,
    pub tcpi_busy_time: u64,
    pub tcpi_rwnd_limited: u64,
    pub tcpi_snd_wnd_limited: u64,
}

impl TcpInfo {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_connection(conn: &super::tcp::TcpConnection) -> Self {
        let state = tcp_state_to_linux(conn.state);
        let sack_count = conn.sack_scoreboard.blocks.len() as u32;

        let mut options: u8 = 0;
        if conn.sack_permitted || !conn.sack_scoreboard.blocks.is_empty() {
            options |= TCPI_OPT_SYN_SEEN;
        }

        let bytes_acked = (conn.snd_nxt.wrapping_sub(conn.snd_una)) as u64;
        let bytes_received = conn.rx_buffer.len() as u64;
        let reordering = if conn.sack_permitted { 3 } else { 3 };

        TcpInfo {
            tcpi_state: state,
            tcpi_ca_state: TCP_CA_Open,
            tcpi_retransmits: conn.retransmit_count,
            tcpi_probes: 0,
            tcpi_fack_out: conn.fast_retx.dup_ack_count as u8,
            tcpi_options: options,
            tcpi_snd_wscale: conn.ws_scale,
            tcpi_rcv_wscale: conn.peer_ws_scale,
            tcpi_rto: conn.rto,
            tcpi_ato: conn.rtt / 2,
            tcpi_snd_mss: conn.mss,
            tcpi_rcv_mss: conn.mss,
            tcpi_unacked: conn.snd_nxt.wrapping_sub(conn.snd_una),
            tcpi_sacked: conn.sack_scoreboard.sacked_bytes,
            tcpi_lost: 0,
            tcpi_retrans: conn.retransmit_count as u32,
            tcpi_fackets: sack_count,
            tcpi_last_data_sent: 0,
            tcpi_last_ack_sent: 0,
            tcpi_last_data_recv: 0,
            tcpi_last_ack_recv: 0,
            tcpi_pmtu: 1500,
            tcpi_rcv_ssthresh: 65535,
            tcpi_rtt: conn.rtt,
            tcpi_rttvar: conn.rtt_var,
            tcpi_snd_ssthresh: conn.ssthresh,
            tcpi_snd_cwnd: conn.cwnd,
            tcpi_advmss: conn.mss,
            tcpi_reordering: reordering,
            tcpi_rcv_wnd: (conn.window_size as u32) << (conn.peer_ws_scale as u32),
            tcpi_snd_wnd: conn.snd_wnd,
            tcpi_bytes_acked: bytes_acked,
            tcpi_bytes_received: bytes_received,
            tcpi_segs_out: 0,
            tcpi_segs_in: 0,
            tcpi_busy_time: 0,
            tcpi_rwnd_limited: 0,
            tcpi_snd_wnd_limited: 0,
        }
    }

    pub fn serialize(&self) -> [u8; TCP_INFO_SIZE] {
        let mut buf = [0u8; TCP_INFO_SIZE];
        let mut off = 0;

        buf[off] = self.tcpi_state;
        off += 1;
        buf[off] = self.tcpi_ca_state;
        off += 1;
        buf[off] = self.tcpi_retransmits;
        off += 1;
        buf[off] = self.tcpi_probes;
        off += 1;
        buf[off] = self.tcpi_fack_out;
        off += 1;
        buf[off] = self.tcpi_options;
        off += 1;
        buf[off] = self.tcpi_snd_wscale;
        off += 1;
        buf[off] = self.tcpi_rcv_wscale;
        off += 1;

        buf[off..off + 4].copy_from_slice(&self.tcpi_rto.to_ne_bytes());
        off += 4;
        buf[off..off + 4].copy_from_slice(&self.tcpi_ato.to_ne_bytes());
        off += 4;
        buf[off..off + 2].copy_from_slice(&self.tcpi_snd_mss.to_ne_bytes());
        off += 2;
        buf[off..off + 2].copy_from_slice(&self.tcpi_rcv_mss.to_ne_bytes());
        off += 2;

        buf[off..off + 4].copy_from_slice(&self.tcpi_unacked.to_ne_bytes());
        off += 4;
        buf[off..off + 4].copy_from_slice(&self.tcpi_sacked.to_ne_bytes());
        off += 4;
        buf[off..off + 4].copy_from_slice(&self.tcpi_lost.to_ne_bytes());
        off += 4;
        buf[off..off + 4].copy_from_slice(&self.tcpi_retrans.to_ne_bytes());
        off += 4;
        buf[off..off + 4].copy_from_slice(&self.tcpi_fackets.to_ne_bytes());
        off += 4;

        buf[off..off + 4].copy_from_slice(&self.tcpi_last_data_sent.to_ne_bytes());
        off += 4;
        buf[off..off + 4].copy_from_slice(&self.tcpi_last_ack_sent.to_ne_bytes());
        off += 4;
        buf[off..off + 4].copy_from_slice(&self.tcpi_last_data_recv.to_ne_bytes());
        off += 4;
        buf[off..off + 4].copy_from_slice(&self.tcpi_last_ack_recv.to_ne_bytes());
        off += 4;

        buf[off..off + 4].copy_from_slice(&self.tcpi_pmtu.to_ne_bytes());
        off += 4;
        buf[off..off + 4].copy_from_slice(&self.tcpi_rcv_ssthresh.to_ne_bytes());
        off += 4;
        buf[off..off + 4].copy_from_slice(&self.tcpi_rtt.to_ne_bytes());
        off += 4;
        buf[off..off + 4].copy_from_slice(&self.tcpi_rttvar.to_ne_bytes());
        off += 4;
        buf[off..off + 4].copy_from_slice(&self.tcpi_snd_ssthresh.to_ne_bytes());
        off += 4;
        buf[off..off + 4].copy_from_slice(&self.tcpi_snd_cwnd.to_ne_bytes());
        off += 4;
        buf[off..off + 2].copy_from_slice(&self.tcpi_advmss.to_ne_bytes());
        off += 2;
        buf[off..off + 2].copy_from_slice(&self.tcpi_reordering.to_ne_bytes());
        off += 2;

        buf[off..off + 4].copy_from_slice(&self.tcpi_rcv_wnd.to_ne_bytes());
        off += 4;
        buf[off..off + 4].copy_from_slice(&self.tcpi_snd_wnd.to_ne_bytes());
        off += 4;
        buf[off..off + 8].copy_from_slice(&self.tcpi_bytes_acked.to_ne_bytes());
        off += 8;
        buf[off..off + 8].copy_from_slice(&self.tcpi_bytes_received.to_ne_bytes());
        off += 8;
        buf[off..off + 4].copy_from_slice(&self.tcpi_segs_out.to_ne_bytes());
        off += 4;
        buf[off..off + 4].copy_from_slice(&self.tcpi_segs_in.to_ne_bytes());
        off += 4;

        buf[off..off + 8].copy_from_slice(&self.tcpi_busy_time.to_ne_bytes());
        off += 8;
        buf[off..off + 8].copy_from_slice(&self.tcpi_rwnd_limited.to_ne_bytes());
        off += 8;
        buf[off..off + 8].copy_from_slice(&self.tcpi_snd_wnd_limited.to_ne_bytes());

        buf
    }

    pub fn deserialize(data: &[u8]) -> Result<Self, NetError> {
        if data.len() < TCP_INFO_SIZE {
            return Err(NetError::InvalidParam);
        }

        let mut off = 0;
        let tcpi_state = data[off];
        off += 1;
        let tcpi_ca_state = data[off];
        off += 1;
        let tcpi_retransmits = data[off];
        off += 1;
        let tcpi_probes = data[off];
        off += 1;
        let tcpi_fack_out = data[off];
        off += 1;
        let tcpi_options = data[off];
        off += 1;
        let tcpi_snd_wscale = data[off];
        off += 1;
        let tcpi_rcv_wscale = data[off];
        off += 1;

        let tcpi_rto = u32::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        off += 4;
        let tcpi_ato = u32::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        off += 4;
        let tcpi_snd_mss =
            u16::from_ne_bytes([data[off], data[off + 1]]);
        off += 2;
        let tcpi_rcv_mss =
            u16::from_ne_bytes([data[off], data[off + 1]]);
        off += 2;

        let tcpi_unacked = u32::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        off += 4;
        let tcpi_sacked = u32::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        off += 4;
        let tcpi_lost = u32::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        off += 4;
        let tcpi_retrans = u32::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        off += 4;
        let tcpi_fackets = u32::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        off += 4;

        let tcpi_last_data_sent = u32::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        off += 4;
        let tcpi_last_ack_sent = u32::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        off += 4;
        let tcpi_last_data_recv = u32::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        off += 4;
        let tcpi_last_ack_recv = u32::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        off += 4;

        let tcpi_pmtu = u32::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        off += 4;
        let tcpi_rcv_ssthresh = u32::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        off += 4;
        let tcpi_rtt = u32::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        off += 4;
        let tcpi_rttvar = u32::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        off += 4;
        let tcpi_snd_ssthresh = u32::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        off += 4;
        let tcpi_snd_cwnd = u32::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        off += 4;
        let tcpi_advmss = u16::from_ne_bytes([data[off], data[off + 1]]);
        off += 2;
        let tcpi_reordering = u16::from_ne_bytes([data[off], data[off + 1]]);
        off += 2;

        let tcpi_rcv_wnd = u32::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        off += 4;
        let tcpi_snd_wnd = u32::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        off += 4;
        let tcpi_bytes_acked =
            u64::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3], data[off + 4], data[off + 5], data[off + 6], data[off + 7]]);
        off += 8;
        let tcpi_bytes_received =
            u64::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3], data[off + 4], data[off + 5], data[off + 6], data[off + 7]]);
        off += 8;
        let tcpi_segs_out = u32::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        off += 4;
        let tcpi_segs_in = u32::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
        off += 4;

        let tcpi_busy_time =
            u64::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3], data[off + 4], data[off + 5], data[off + 6], data[off + 7]]);
        off += 8;
        let tcpi_rwnd_limited =
            u64::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3], data[off + 4], data[off + 5], data[off + 6], data[off + 7]]);
        off += 8;
        let tcpi_snd_wnd_limited =
            u64::from_ne_bytes([data[off], data[off + 1], data[off + 2], data[off + 3], data[off + 4], data[off + 5], data[off + 6], data[off + 7]]);

        Ok(TcpInfo {
            tcpi_state,
            tcpi_ca_state,
            tcpi_retransmits,
            tcpi_probes,
            tcpi_fack_out,
            tcpi_options,
            tcpi_snd_wscale,
            tcpi_rcv_wscale,
            tcpi_rto,
            tcpi_ato,
            tcpi_snd_mss,
            tcpi_rcv_mss,
            tcpi_unacked,
            tcpi_sacked,
            tcpi_lost,
            tcpi_retrans,
            tcpi_fackets,
            tcpi_last_data_sent,
            tcpi_last_ack_sent,
            tcpi_last_data_recv,
            tcpi_last_ack_recv,
            tcpi_pmtu,
            tcpi_rcv_ssthresh,
            tcpi_rtt,
            tcpi_rttvar,
            tcpi_snd_ssthresh,
            tcpi_snd_cwnd,
            tcpi_advmss,
            tcpi_reordering,
            tcpi_rcv_wnd,
            tcpi_snd_wnd,
            tcpi_bytes_acked,
            tcpi_bytes_received,
            tcpi_segs_out,
            tcpi_segs_in,
            tcpi_busy_time,
            tcpi_rwnd_limited,
            tcpi_snd_wnd_limited,
        })
    }
}

fn tcp_state_to_linux(state: super::tcp::TcpState) -> u8 {
    match state {
        super::tcp::TcpState::Closed => TCP_CLOSE,
        super::tcp::TcpState::Listen => TCP_LISTEN,
        super::tcp::TcpState::SynSent => TCP_SYN_SENT,
        super::tcp::TcpState::SynReceived => TCP_SYN_RECV,
        super::tcp::TcpState::Established => TCP_ESTABLISHED,
        super::tcp::TcpState::FinWait1 => TCP_FIN_WAIT1,
        super::tcp::TcpState::FinWait2 => TCP_FIN_WAIT2,
        super::tcp::TcpState::CloseWait => TCP_CLOSE_WAIT,
        super::tcp::TcpState::Closing => TCP_CLOSING,
        super::tcp::TcpState::LastAck => TCP_LAST_ACK,
        super::tcp::TcpState::TimeWait => TCP_TIME_WAIT,
    }
}

fn tcp_state_to_str(state: u8) -> &'static str {
    match state {
        TCP_ESTABLISHED => "01",
        TCP_SYN_SENT => "02",
        TCP_SYN_RECV => "03",
        TCP_FIN_WAIT1 => "04",
        TCP_FIN_WAIT2 => "05",
        TCP_TIME_WAIT => "06",
        TCP_CLOSE => "07",
        TCP_CLOSE_WAIT => "08",
        TCP_LAST_ACK => "09",
        TCP_LISTEN => "0A",
        TCP_CLOSING => "0B",
        _ => "FF",
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ProcNetTcpEntry {
    pub sl: u32,
    pub local_addr: u32,
    pub local_port: u16,
    pub rem_addr: u32,
    pub rem_port: u16,
    pub st: u8,
    pub tx_queue: u32,
    pub rx_queue: u32,
    pub tr: u32,
    pub tm_when: u32,
    pub retrnsmt: u32,
    pub uid: u32,
    pub timeout: u32,
    pub inode: u64,
}

fn write_hex_u32(buf: &mut Vec<u8>, val: u32) {
    let bytes = val.to_be_bytes();
    for &b in &bytes {
        let hi = b >> 4;
        let lo = b & 0x0f;
        buf.push(if hi < 10 { b'0' + hi } else { b'a' + hi - 10 });
        buf.push(if lo < 10 { b'0' + lo } else { b'a' + lo - 10 });
    }
}

fn write_hex_u16_port(buf: &mut Vec<u8>, val: u16) {
    let bytes = val.to_be_bytes();
    for &b in &bytes {
        let hi = b >> 4;
        let lo = b & 0x0f;
        buf.push(if hi < 10 { b'0' + hi } else { b'a' + hi - 10 });
        buf.push(if lo < 10 { b'0' + lo } else { b'a' + lo - 10 });
    }
}

fn write_hex_u64(buf: &mut Vec<u8>, val: u64) {
    let bytes = val.to_be_bytes();
    for &b in &bytes {
        let hi = b >> 4;
        let lo = b & 0x0f;
        buf.push(if hi < 10 { b'0' + hi } else { b'a' + hi - 10 });
        buf.push(if lo < 10 { b'0' + lo } else { b'a' + lo - 10 });
    }
}

fn write_hex_u32_dec(buf: &mut Vec<u8>, val: u32) {
    let s = format!("{:x}", val);
    buf.extend_from_slice(s.as_bytes());
}

fn write_hex_u64_dec(buf: &mut Vec<u8>, val: u64) {
    let s = format!("{:x}", val);
    buf.extend_from_slice(s.as_bytes());
}

fn write_dec_u32(buf: &mut Vec<u8>, val: u32) {
    let s = format!("{}", val);
    buf.extend_from_slice(s.as_bytes());
}

pub fn format_proc_net_tcp(entries: &[ProcNetTcpEntry]) -> Vec<u8> {
    let mut out = Vec::new();

    let header = b"  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n";
    out.extend_from_slice(header);

    for entry in entries {
        out.extend_from_slice(b"  ");
        write_dec_u32(&mut out, entry.sl);

        out.extend_from_slice(b" ");
        write_hex_u32(&mut out, entry.local_addr);
        out.push(b':');
        write_hex_u16_port(&mut out, entry.local_port);

        out.extend_from_slice(b" ");
        write_hex_u32(&mut out, entry.rem_addr);
        out.push(b':');
        write_hex_u16_port(&mut out, entry.rem_port);

        out.extend_from_slice(b"  ");
        out.extend_from_slice(tcp_state_to_str(entry.st).as_bytes());

        out.extend_from_slice(b" ");
        write_hex_u32_dec(&mut out, entry.tx_queue);
        out.extend_from_slice(b":");
        write_hex_u32_dec(&mut out, entry.rx_queue);

        out.extend_from_slice(b"  ");
        write_hex_u32_dec(&mut out, entry.tr);
        out.extend_from_slice(b":");
        write_hex_u32_dec(&mut out, entry.tm_when);

        out.extend_from_slice(b"  ");
        write_hex_u32_dec(&mut out, entry.retrnsmt);

        out.extend_from_slice(b" ");
        write_dec_u32(&mut out, entry.uid);

        out.extend_from_slice(b" ");
        write_dec_u32(&mut out, entry.timeout);

        out.extend_from_slice(b" ");
        write_hex_u64_dec(&mut out, entry.inode);

        out.push(b'\n');
    }

    out
}

pub fn proc_net_tcp_entry_from_connection(
    conn: &super::tcp::TcpConnection,
    sl: u32,
    inode: u64,
) -> ProcNetTcpEntry {
    let local_ip = match conn.local.ip {
        super::IpAddr::V4(ip) => u32::from_be_bytes(ip.0),
        _ => 0,
    };
    let remote_ip = match conn.remote.ip {
        super::IpAddr::V4(ip) => u32::from_be_bytes(ip.0),
        _ => 0,
    };

    ProcNetTcpEntry {
        sl,
        local_addr: local_ip,
        local_port: conn.local.port.0,
        rem_addr: remote_ip,
        rem_port: conn.remote.port.0,
        st: tcp_state_to_linux(conn.state),
        tx_queue: conn.tx_buffer.len() as u32,
        rx_queue: conn.rx_buffer.len() as u32,
        tr: 0,
        tm_when: 0,
        retrnsmt: conn.retransmit_count as u32,
        uid: 0,
        timeout: 0,
        inode,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tcp_info_default_all_zeros() {
        let info = TcpInfo::default();
        assert_eq!(info.tcpi_state, 0);
        assert_eq!(info.tcpi_ca_state, 0);
        assert_eq!(info.tcpi_retransmits, 0);
        assert_eq!(info.tcpi_probes, 0);
        assert_eq!(info.tcpi_fack_out, 0);
        assert_eq!(info.tcpi_options, 0);
        assert_eq!(info.tcpi_snd_wscale, 0);
        assert_eq!(info.tcpi_rcv_wscale, 0);
        assert_eq!(info.tcpi_rto, 0);
        assert_eq!(info.tcpi_ato, 0);
        assert_eq!(info.tcpi_snd_mss, 0);
        assert_eq!(info.tcpi_rcv_mss, 0);
        assert_eq!(info.tcpi_unacked, 0);
        assert_eq!(info.tcpi_sacked, 0);
        assert_eq!(info.tcpi_lost, 0);
        assert_eq!(info.tcpi_retrans, 0);
        assert_eq!(info.tcpi_fackets, 0);
        assert_eq!(info.tcpi_last_data_sent, 0);
        assert_eq!(info.tcpi_last_ack_sent, 0);
        assert_eq!(info.tcpi_last_data_recv, 0);
        assert_eq!(info.tcpi_last_ack_recv, 0);
        assert_eq!(info.tcpi_pmtu, 0);
        assert_eq!(info.tcpi_rcv_ssthresh, 0);
        assert_eq!(info.tcpi_rtt, 0);
        assert_eq!(info.tcpi_rttvar, 0);
        assert_eq!(info.tcpi_snd_ssthresh, 0);
        assert_eq!(info.tcpi_snd_cwnd, 0);
        assert_eq!(info.tcpi_advmss, 0);
        assert_eq!(info.tcpi_reordering, 0);
        assert_eq!(info.tcpi_rcv_wnd, 0);
        assert_eq!(info.tcpi_snd_wnd, 0);
        assert_eq!(info.tcpi_bytes_acked, 0);
        assert_eq!(info.tcpi_bytes_received, 0);
        assert_eq!(info.tcpi_segs_out, 0);
        assert_eq!(info.tcpi_segs_in, 0);
        assert_eq!(info.tcpi_busy_time, 0);
        assert_eq!(info.tcpi_rwnd_limited, 0);
        assert_eq!(info.tcpi_snd_wnd_limited, 0);
    }

    #[test]
    fn tcp_info_serialize_size() {
        let info = TcpInfo::default();
        let buf = info.serialize();
        assert_eq!(buf.len(), TCP_INFO_SIZE);
    }

    #[test]
    fn tcp_info_serialize_deserialize_roundtrip() {
        let mut info = TcpInfo::default();
        info.tcpi_state = TCP_ESTABLISHED;
        info.tcpi_ca_state = TCP_CA_Recovery;
        info.tcpi_retransmits = 5;
        info.tcpi_probes = 2;
        info.tcpi_fack_out = 3;
        info.tcpi_options = 7;
        info.tcpi_snd_wscale = 7;
        info.tcpi_rcv_wscale = 8;
        info.tcpi_rto = 300;
        info.tcpi_ato = 50;
        info.tcpi_snd_mss = 1460;
        info.tcpi_rcv_mss = 1440;
        info.tcpi_unacked = 4380;
        info.tcpi_sacked = 2920;
        info.tcpi_lost = 1460;
        info.tcpi_retrans = 2;
        info.tcpi_fackets = 4;
        info.tcpi_last_data_sent = 1000;
        info.tcpi_last_ack_sent = 999;
        info.tcpi_last_data_recv = 500;
        info.tcpi_last_ack_recv = 499;
        info.tcpi_pmtu = 1500;
        info.tcpi_rcv_ssthresh = 65535;
        info.tcpi_rtt = 25;
        info.tcpi_rttvar = 10;
        info.tcpi_snd_ssthresh = 4294967295;
        info.tcpi_snd_cwnd = 20;
        info.tcpi_advmss = 1460;
        info.tcpi_reordering = 3;
        info.tcpi_rcv_wnd = 131070;
        info.tcpi_snd_wnd = 65535;
        info.tcpi_bytes_acked = 123456789;
        info.tcpi_bytes_received = 987654321;
        info.tcpi_segs_out = 1000;
        info.tcpi_segs_in = 999;
        info.tcpi_busy_time = 50000;
        info.tcpi_rwnd_limited = 10000;
        info.tcpi_snd_wnd_limited = 5000;

        let buf = info.serialize();
        assert_eq!(buf.len(), TCP_INFO_SIZE);

        let restored = TcpInfo::deserialize(&buf).unwrap();
        assert_eq!(restored.tcpi_state, TCP_ESTABLISHED);
        assert_eq!(restored.tcpi_ca_state, TCP_CA_Recovery);
        assert_eq!(restored.tcpi_retransmits, 5);
        assert_eq!(restored.tcpi_probes, 2);
        assert_eq!(restored.tcpi_fack_out, 3);
        assert_eq!(restored.tcpi_options, 7);
        assert_eq!(restored.tcpi_snd_wscale, 7);
        assert_eq!(restored.tcpi_rcv_wscale, 8);
        assert_eq!(restored.tcpi_rto, 300);
        assert_eq!(restored.tcpi_ato, 50);
        assert_eq!(restored.tcpi_snd_mss, 1460);
        assert_eq!(restored.tcpi_rcv_mss, 1440);
        assert_eq!(restored.tcpi_unacked, 4380);
        assert_eq!(restored.tcpi_sacked, 2920);
        assert_eq!(restored.tcpi_lost, 1460);
        assert_eq!(restored.tcpi_retrans, 2);
        assert_eq!(restored.tcpi_fackets, 4);
        assert_eq!(restored.tcpi_last_data_sent, 1000);
        assert_eq!(restored.tcpi_last_ack_sent, 999);
        assert_eq!(restored.tcpi_last_data_recv, 500);
        assert_eq!(restored.tcpi_last_ack_recv, 499);
        assert_eq!(restored.tcpi_pmtu, 1500);
        assert_eq!(restored.tcpi_rcv_ssthresh, 65535);
        assert_eq!(restored.tcpi_rtt, 25);
        assert_eq!(restored.tcpi_rttvar, 10);
        assert_eq!(restored.tcpi_snd_ssthresh, 4294967295);
        assert_eq!(restored.tcpi_snd_cwnd, 20);
        assert_eq!(restored.tcpi_advmss, 1460);
        assert_eq!(restored.tcpi_reordering, 3);
        assert_eq!(restored.tcpi_rcv_wnd, 131070);
        assert_eq!(restored.tcpi_snd_wnd, 65535);
        assert_eq!(restored.tcpi_bytes_acked, 123456789);
        assert_eq!(restored.tcpi_bytes_received, 987654321);
        assert_eq!(restored.tcpi_segs_out, 1000);
        assert_eq!(restored.tcpi_segs_in, 999);
        assert_eq!(restored.tcpi_busy_time, 50000);
        assert_eq!(restored.tcpi_rwnd_limited, 10000);
        assert_eq!(restored.tcpi_snd_wnd_limited, 5000);
    }

    #[test]
    fn tcp_info_deserialize_short_buffer() {
        let buf = [0u8; 10];
        assert!(TcpInfo::deserialize(&buf).is_err());
    }

    #[test]
    fn tcp_state_constants() {
        assert_eq!(TCP_ESTABLISHED, 1);
        assert_eq!(TCP_SYN_SENT, 2);
        assert_eq!(TCP_SYN_RECV, 3);
        assert_eq!(TCP_FIN_WAIT1, 4);
        assert_eq!(TCP_FIN_WAIT2, 5);
        assert_eq!(TCP_TIME_WAIT, 6);
        assert_eq!(TCP_CLOSE, 7);
        assert_eq!(TCP_CLOSE_WAIT, 8);
        assert_eq!(TCP_LAST_ACK, 9);
        assert_eq!(TCP_LISTEN, 10);
        assert_eq!(TCP_CLOSING, 11);
    }

    #[test]
    fn tcp_ca_state_constants() {
        assert_eq!(TCP_CA_Open, 0);
        assert_eq!(TCP_CA_Disorder, 1);
        assert_eq!(TCP_CA_CWR, 2);
        assert_eq!(TCP_CA_Recovery, 3);
        assert_eq!(TCP_CA_Loss, 4);
    }

    #[test]
    fn socket_option_constants() {
        assert_eq!(TCP_INFO, 11);
        assert_eq!(IP_TOS, 1);
        assert_eq!(IPV6_TCLASS, 36);
        assert_eq!(SO_ZEROCOPY, 60);
    }

    #[test]
    fn proc_net_tcp_single_entry() {
        let entry = ProcNetTcpEntry {
            sl: 0,
            local_addr: 0x0100007F,
            local_port: 80,
            rem_addr: 0xC0A80101,
            rem_port: 12345,
            st: TCP_ESTABLISHED,
            tx_queue: 0,
            rx_queue: 0,
            tr: 0,
            tm_when: 0,
            retrnsmt: 0,
            uid: 0,
            timeout: 0,
            inode: 12345,
        };

        let output = format_proc_net_tcp(&[entry]);
        let text = core::str::from_utf8(&output).unwrap();

        assert!(text.starts_with("  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n"));
        assert!(text.contains("0100007f:0050"));
        assert!(text.contains("c0a80101:3039"));
        assert!(text.contains(" 01 "));
        assert!(text.contains(" 0 "));
        assert!(text.contains(" 3039"));
    }

    #[test]
    fn proc_net_tcp_multiple_entries() {
        let entries = [
            ProcNetTcpEntry {
                sl: 0,
                local_addr: 0x00000000,
                local_port: 80,
                rem_addr: 0x00000000,
                rem_port: 0,
                st: TCP_LISTEN,
                tx_queue: 0,
                rx_queue: 0,
                tr: 0,
                tm_when: 0,
                retrnsmt: 0,
                uid: 0,
                timeout: 0,
                inode: 100,
            },
            ProcNetTcpEntry {
                sl: 1,
                local_addr: 0x0100007F,
                local_port: 443,
                rem_addr: 0x0A000001,
                rem_port: 54321,
                st: TCP_ESTABLISHED,
                tx_queue: 10,
                rx_queue: 20,
                tr: 0,
                tm_when: 0,
                retrnsmt: 0,
                uid: 1000,
                timeout: 0,
                inode: 200,
            },
            ProcNetTcpEntry {
                sl: 2,
                local_addr: 0x0100007F,
                local_port: 8080,
                rem_addr: 0x0A000002,
                rem_port: 9999,
                st: TCP_TIME_WAIT,
                tx_queue: 0,
                rx_queue: 0,
                tr: 0,
                tm_when: 0,
                retrnsmt: 0,
                uid: 0,
                timeout: 120,
                inode: 300,
            },
        ];

        let output = format_proc_net_tcp(&entries);
        let text = core::str::from_utf8(&output).unwrap();
        let lines: Vec<&str> = text.lines().collect();

        assert_eq!(lines.len(), 4);
        assert!(lines[0].contains("sl"));
        assert!(lines[1].contains("00000000:0050"));
        assert!(lines[1].contains(" 0A "));
        assert!(lines[2].contains("0100007f:01bb"));
        assert!(lines[2].contains("0a000001:d431"));
        assert!(lines[2].contains(" 01 "));
        assert!(lines[2].contains(" 1000 "));
        assert!(lines[3].contains("0100007f:1f90"));
        assert!(lines[3].contains(" 06 "));
        assert!(lines[3].contains(" 120 "));
    }

    #[test]
    fn hex_encoding_addresses() {
        let mut buf = Vec::new();
        write_hex_u32(&mut buf, 0x7F000001);
        assert_eq!(&buf, b"7f000001");

        buf.clear();
        write_hex_u16_port(&mut buf, 80);
        assert_eq!(&buf, b"0050");

        buf.clear();
        write_hex_u16_port(&mut buf, 443);
        assert_eq!(&buf, b"01bb");

        buf.clear();
        write_hex_u16_port(&mut buf, 65535);
        assert_eq!(&buf, b"ffff");
    }

    #[test]
    fn proc_net_tcp_state_mapping() {
        assert_eq!(tcp_state_to_str(TCP_ESTABLISHED), "01");
        assert_eq!(tcp_state_to_str(TCP_SYN_SENT), "02");
        assert_eq!(tcp_state_to_str(TCP_SYN_RECV), "03");
        assert_eq!(tcp_state_to_str(TCP_FIN_WAIT1), "04");
        assert_eq!(tcp_state_to_str(TCP_FIN_WAIT2), "05");
        assert_eq!(tcp_state_to_str(TCP_TIME_WAIT), "06");
        assert_eq!(tcp_state_to_str(TCP_CLOSE), "07");
        assert_eq!(tcp_state_to_str(TCP_CLOSE_WAIT), "08");
        assert_eq!(tcp_state_to_str(TCP_LAST_ACK), "09");
        assert_eq!(tcp_state_to_str(TCP_LISTEN), "0A");
        assert_eq!(tcp_state_to_str(TCP_CLOSING), "0B");
        assert_eq!(tcp_state_to_str(0xFF), "FF");
    }

    #[test]
    fn tcp_info_zero_roundtrip() {
        let info = TcpInfo::default();
        let buf = info.serialize();
        let restored = TcpInfo::deserialize(&buf).unwrap();
        assert_eq!(restored.tcpi_state, 0);
        assert_eq!(restored.tcpi_rto, 0);
        assert_eq!(restored.tcpi_bytes_acked, 0);
        assert_eq!(restored.tcpi_snd_wnd_limited, 0);
    }
}
