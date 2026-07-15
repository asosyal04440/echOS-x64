use ech_os::net::http_sys;
use ech_os::net::ipv6_transition;
use ech_os::net::port_knock;
use ech_os::net::sctp;
use ech_os::net::smb;
use ech_os::net::{IpAddr, Ipv4Addr, Port};
use std::collections::BTreeMap;

#[test]
fn sctp_association_transfers_fragmented_message() {
    let server = sctp::open(vec![IpAddr::V4(Ipv4Addr([10, 1, 0, 1]))], Port(9000), 16, 16);
    let client = sctp::open(vec![IpAddr::V4(Ipv4Addr([10, 1, 0, 2]))], Port(9001), 16, 16);
    let init = sctp::initiate(client, vec![IpAddr::V4(Ipv4Addr([10, 1, 0, 1]))], Port(9000)).unwrap();
    let init_ack = sctp::handle_packet(server, &init).unwrap().unwrap();
    let cookie_echo = sctp::handle_packet(client, &init_ack).unwrap().unwrap();
    let cookie_ack = sctp::handle_packet(server, &cookie_echo).unwrap().unwrap();
    sctp::handle_packet(client, &cookie_ack).unwrap();

    let payload = vec![0xAB; 256];
    for packet in sctp::sendmsg(client, 3, 99, true, &payload, 96).unwrap() {
        let sack = sctp::handle_packet(server, &packet).unwrap().unwrap();
        let _ = sctp::handle_packet(client, &sack).unwrap();
    }
    let msg = sctp::recvmsg(server).unwrap();
    assert_eq!(msg.ppid, 99);
    assert_eq!(msg.stream_seq, 0);
    assert!(msg.unordered);
    assert_eq!(msg.payload, payload);

    let heartbeat = sctp::heartbeat(client).unwrap();
    let heartbeat_ack = sctp::handle_packet(server, &heartbeat).unwrap().unwrap();
    assert!(matches!(heartbeat_ack.chunks.first(), Some(sctp::SctpChunk::HeartbeatAck(_))));

    let abort = sctp::abort(client, b"done").unwrap();
    let _ = sctp::handle_packet(server, &abort).unwrap();
    assert_eq!(sctp::get_association(server).unwrap().state, sctp::SctpState::Closed);
    sctp::close(client).unwrap();
    sctp::close(server).unwrap();
}

#[test]
fn ipv6_transition_mechanisms_roundtrip() {
    let six_to_four = ipv6_transition::SixToFourAddr::from_ipv4(Ipv4Addr([192, 0, 2, 44]));
    let addr = six_to_four.to_ipv6(0xBEEF, [0, 0, 0, 0x44]);
    assert_eq!(
        ipv6_transition::SixToFourAddr::extract_ipv4(addr),
        Some(Ipv4Addr([192, 0, 2, 44]))
    );

    let teredo = ipv6_transition::TeredoAddr {
        server_ipv4: Ipv4Addr([65, 54, 227, 120]),
        client_ipv4: Ipv4Addr([203, 0, 113, 55]),
        flags: 0x8000,
        udp_port: 40000,
    };
    assert_eq!(ipv6_transition::TeredoAddr::decode(teredo.encode()), Some(teredo));

    let isatap = ipv6_transition::IsatapAddr::new([0x2001, 0xdb8, 0, 5], Ipv4Addr([10, 0, 0, 9])).to_ipv6();
    assert_eq!(
        ipv6_transition::IsatapAddr::extract_ipv4(isatap),
        Some(Ipv4Addr([10, 0, 0, 9]))
    );
}

#[test]
fn http_sys_request_queue_routes_and_serializes_response() {
    let session = http_sys::create_server_session("kernel-http");
    let group = http_sys::create_url_group(session).unwrap();
    let queue = http_sys::create_request_queue();
    http_sys::bind_url_group_to_queue(group, queue).unwrap();
    http_sys::add_url_to_group(group, "http://localhost/api").unwrap();

    let req_id = http_sys::inject_request(
        "POST",
        "http://localhost/api/items",
        Ipv4Addr([127, 0, 0, 1]),
        Port(54000),
        BTreeMap::new(),
        br#"{"ok":true}"#,
    )
    .unwrap();
    let request = http_sys::receive_request(queue).unwrap();
    assert_eq!(request.request_id, req_id);
    assert_eq!(request.path, "/api/items");
    assert_eq!(http_sys::query_request_queue(queue).unwrap().pending_requests, 0);

    let response = http_sys::HttpSysResponse::ok(b"accepted", "text/plain");
    let bytes = response.serialize();
    assert!(bytes.starts_with(b"HTTP/1.1 200 OK\r\n"));
    http_sys::send_response(req_id, response).unwrap();
    assert_eq!(http_sys::get_response(req_id).unwrap().body, b"accepted");
    assert_eq!(http_sys::take_response(req_id).unwrap().body, b"accepted");
    http_sys::remove_url_from_group(group, "http://localhost/api").unwrap();
    http_sys::close_request_queue(queue);
    http_sys::close_server_session(session);
}

#[test]
fn smb_share_supports_negotiate_session_tree_and_file_ops() {
    smb::register_memory_share("public");
    let session = smb::session_setup("bahadir", true);
    assert_eq!(session.dialect, smb::SmbDialect::Smb2);

    let tree = smb::tree_connect(session.session_id, "public").unwrap();
    smb::mkdir(tree.tree_id, "/docs").unwrap();
    let handle = smb::create(tree.tree_id, "/docs/readme.txt").unwrap();
    smb::write(handle.file_id, b"echos smb").unwrap();
    smb::seek(handle.file_id, 0).unwrap();
    assert_eq!(smb::read(handle.file_id, 32).unwrap(), b"echos smb");
    smb::rename(tree.tree_id, "/docs/readme.txt", "/docs/guide.txt").unwrap();
    assert_eq!(
        smb::list_dir(tree.tree_id, "/docs").unwrap(),
        vec![String::from("/docs/guide.txt")]
    );
    smb::close(handle.file_id);
    smb::unlink(tree.tree_id, "/docs/guide.txt").unwrap();
    smb::rmdir(tree.tree_id, "/docs").unwrap();
    smb::tree_disconnect(tree.tree_id);
    smb::logoff(session.session_id);
}

#[test]
fn spa_and_knock_authorization_open_protected_service() {
    port_knock::register_service(port_knock::ProtectedService {
        name: String::from("ssh"),
        protected_port: 22,
        sequence: vec![
            (port_knock::KnockProto::Tcp, 1111),
            (port_knock::KnockProto::Udp, 2222),
            (port_knock::KnockProto::Tcp, 3333),
        ],
        open_window_ms: 5_000,
    });
    let ip = Ipv4Addr([198, 51, 100, 77]);
    port_knock::observe_knock(
        ip,
        port_knock::KnockEvent {
            proto: port_knock::KnockProto::Tcp,
            port: 1111,
            ts_ms: 1_000,
        },
    );
    port_knock::observe_knock(
        ip,
        port_knock::KnockEvent {
            proto: port_knock::KnockProto::Udp,
            port: 2222,
            ts_ms: 1_200,
        },
    );
    port_knock::observe_knock(
        ip,
        port_knock::KnockEvent {
            proto: port_knock::KnockProto::Tcp,
            port: 3333,
            ts_ms: 1_350,
        },
    );
    assert!(port_knock::is_authorized(ip, "ssh", 2_000));

    port_knock::register_service(port_knock::ProtectedService {
        name: String::from("admin"),
        protected_port: 8443,
        sequence: vec![],
        open_window_ms: 5_000,
    });
    let secret = b"wave4-spa";
    let spa = port_knock::build_spa_packet(secret, "admin", ip, 8443, 4_000, 9);
    assert!(port_knock::authorize_spa(secret, ip, &spa, 4_200, 2_000));
    assert!(!port_knock::authorize_spa(secret, ip, &spa, 4_250, 2_000));
    assert!(port_knock::is_authorized(ip, "admin", 4_500));
}
