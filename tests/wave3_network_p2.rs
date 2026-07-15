//! # Dalga 3 (P2) — Network Stack Build & Verify Suite
//!
//! Bu suite, Dalga 3 kapanisi icin eksik kalan ileri seviye ag yuzeylerini
//! dogrular:
//! - AF_XDP zero-copy socket cekirdegi
//! - TCP_INFO ve timestamping getsockopt yuzeyi
//! - `/proc/net/tcp` ve `/proc/net/sockstat` gorunumleri
//! - SNMP agent counter export'u
//! - WSAEventSelect / WSAPoll benzeri olay bildirimi
//! - AFD-benzeri overlapped operation tamamlama yuzeyi

#![cfg(not(target_os = "none"))]

use std::sync::Arc;

use spin::Mutex;

use ech_os::net::hw_timestamping::TsFlags;
use ech_os::net::netdev::LoopbackInterface;
use ech_os::net::snmp_agent;
use ech_os::net::socket::{
    self, AddressFamily, OverlappedStatus, PollFd, Protocol, SocketAddr, SocketType, FD_CONNECT,
    FD_WRITE,
};
use ech_os::net::tcp_info;
use ech_os::net::{self, Ipv4Addr, Port};

fn ensure_loopback() {
    if net::get_interface("lo").is_none() {
        net::register_interface(Arc::new(Mutex::new(LoopbackInterface::new())));
    }
}

#[test]
fn proc_net_tcp_renders_listen_socket() {
    ensure_loopback();
    let socket_id = socket::socket(AddressFamily::IPV4, SocketType::STREAM, Protocol::TCP)
        .expect("tcp socket");
    let addr = SocketAddr::new(Ipv4Addr::new(127, 0, 0, 1), Port(41030));
    socket::bind(socket_id, addr).expect("bind");
    socket::listen(socket_id, 4).expect("listen");

    let rendered = net::render_proc_net_tcp();
    assert!(rendered.contains("0100007F:A046"));
    assert!(rendered.contains("0A"));

    let tcp_info = socket::getsockopt_tcp_info(socket_id).expect("tcp info");
    assert_eq!(tcp_info.tcpi_state, tcp_info::TCP_LISTEN);

    socket::close(socket_id).expect("close");
}

#[test]
fn sockstat_counts_udp_and_raw_usage() {
    ensure_loopback();
    let udp_sock = socket::socket(AddressFamily::IPV4, SocketType::DGRAM, Protocol::UDP)
        .expect("udp socket");
    let raw_sock = socket::socket(AddressFamily::IPV4, SocketType::RAW, Protocol::ICMP)
        .expect("raw socket");

    let snapshot = net::get_sockstat_snapshot();
    assert!(snapshot.udp_inuse >= 1);
    assert!(snapshot.raw_inuse >= 1);
    assert!(snapshot.sockets_used >= snapshot.udp_inuse + snapshot.raw_inuse);

    let rendered = net::render_sockstat();
    assert!(rendered.contains("UDP: inuse"));
    assert!(rendered.contains("RAW: inuse"));

    socket::close(udp_sock).expect("close udp");
    socket::close(raw_sock).expect("close raw");
}

#[test]
fn snmp_agent_exports_core_counters() {
    let rendered = snmp_agent::render_agent_snapshot();
    assert!(rendered.contains("1.3.6.1.2.1.4.3"));
    assert!(rendered.contains("ipInReceives"));
    assert!(rendered.contains("tcpActiveOpens"));
    assert!(rendered.contains("udpOutDatagrams"));
}

#[test]
fn wsa_event_select_and_wsapoll_surface_real_readiness() {
    ensure_loopback();
    let udp_sock = socket::socket(AddressFamily::IPV4, SocketType::DGRAM, Protocol::UDP)
        .expect("udp socket");
    socket::wsa_event_select(udp_sock, FD_WRITE | FD_CONNECT).expect("register events");

    let events = socket::wsa_enum_network_events(udp_sock).expect("enum events");
    assert_ne!(events.mask & FD_WRITE, 0);

    let mut poll_fds = [PollFd::new(udp_sock as i32, socket::POLLOUT)];
    let ready = socket::wsapoll(&mut poll_fds, 0).expect("wsapoll");
    assert_eq!(ready, 1);
    assert_ne!(poll_fds[0].revents & socket::POLLOUT, 0);

    socket::close(udp_sock).expect("close udp");
}

#[test]
fn overlapped_operations_complete_with_real_socket_results() {
    ensure_loopback();
    let raw_sock = socket::socket(AddressFamily::IPV4, SocketType::RAW, Protocol::ICMP)
        .expect("raw socket");
    let addr = SocketAddr::new(Ipv4Addr::new(1, 1, 1, 1), Port(0));

    let connect_op = socket::submit_overlapped_connect(raw_sock, addr).expect("submit connect");
    let connect_status = socket::poll_overlapped_completion(connect_op).expect("completion");
    assert_eq!(connect_status, OverlappedStatus::Completed(Ok(0)));

    let close_op = socket::submit_overlapped_close(raw_sock).expect("submit close");
    let close_status = socket::poll_overlapped_completion(close_op).expect("completion");
    assert_eq!(close_status, OverlappedStatus::Completed(Ok(0)));
}

#[test]
fn timestamping_and_afxdp_surfaces_are_live() {
    ensure_loopback();
    let udp_sock = socket::socket(AddressFamily::IPV4, SocketType::DGRAM, Protocol::UDP)
        .expect("udp socket");
    let ts_flags = socket::getsockopt_timestamping(udp_sock).expect("timestamping");
    assert_ne!(ts_flags.bits() & TsFlags::SOFTWARE.bits(), 0);

    let sock_id = net::zero_copy::create_afxdp_socket(0).expect("afxdp socket");
    let buf_id = net::zero_copy::afxdp_umem_alloc(sock_id).expect("umem alloc");
    net::zero_copy::afxdp_post_fill(sock_id, buf_id).expect("post fill");
    let (rx, tx) = net::zero_copy::afxdp_process(sock_id, 1).expect("process");
    assert_eq!(tx, 0);
    assert!(rx <= 1);

    socket::close(udp_sock).expect("close udp");
}
