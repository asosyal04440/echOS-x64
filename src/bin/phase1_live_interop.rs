#![cfg(not(target_os = "none"))]

use ech_os::net::dns::DnsRecordType;
use ech_os::net::doh::DohClient;
use ech_os::net::dot::DotClient;
use ech_os::net::ebpf::{
    EbpfLoader, BPF_ALU, BPF_EXIT, BPF_JMP, BPF_K, BPF_MOV, BPF_PROG_TYPE_SOCKET_FILTER,
};
use ech_os::net::grpc::ProtoMessage;
use ech_os::net::http2::{connection_preface, HpackEncoder, Http2Connection, Http2Frame};
use ech_os::net::ipv6::{
    process_packet as process_ipv6_packet, select_next_hop, Ipv6Addr, Ipv6Header, Ipv6Packet,
};
use ech_os::net::{
    cni::{CniConfig, CniManager, CNI_VERSION, DEFAULT_BRIDGE_NAME, DEFAULT_SUBNET},
    MacAddr,
};
use std::collections::BTreeMap;
use std::env;
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddrV4, TcpListener, TcpStream};
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

const POWERSHELL_EXE: &str = r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe";

fn main() {
    println!("phase1:live:start");
    if stage_enabled("grpc") {
        println!("phase1:live:grpc");
        smoke_grpc_loopback().expect("grpc");
    }
    if stage_enabled("ebpf") {
        println!("phase1:live:ebpf");
        smoke_ebpf_jit().expect("ebpf");
    }
    if stage_enabled("doh") {
        println!("phase1:live:doh");
        smoke_doh_matrix().expect("doh");
    }
    if stage_enabled("dot") {
        println!("phase1:live:dot");
        smoke_dot_matrix().expect("dot");
    }
    if stage_enabled("trust") {
        println!("phase1:live:trust");
        smoke_trust_matrix().expect("trust");
    }
    if stage_enabled("ops") {
        println!("phase1:live:ops");
        smoke_ops_matrix().expect("ops");
    }
    if stage_enabled("ipv6") {
        println!("phase1:live:ipv6");
        smoke_ipv6_control_plane().expect("ipv6");
    }
    if stage_enabled("cni") {
        println!("phase1:live:cni");
        smoke_cni_lifecycle().expect("cni");
    }
    if stage_enabled("http3") {
        println!("phase1:live:http3");
        smoke_http3().expect("http3");
    }
    println!("phase1:live:ok");
}

fn stage_enabled(name: &str) -> bool {
    let key = format!("PHASE1_SKIP_{}", name.to_ascii_uppercase());
    env::var_os(&key).is_none()
}

fn smoke_grpc_loopback() -> Result<(), String> {
    let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
        .map_err(|err| format!("grpc bind failed: {err}"))?;
    let port = listener.local_addr().map_err(|err| err.to_string())?.port();

    let server = thread::spawn(move || -> Result<(), String> {
        let (mut stream, _) = listener.accept().map_err(|err| err.to_string())?;
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .map_err(|err| err.to_string())?;
        let mut request_wire = Vec::new();
        let mut recv_buf = [0u8; 4096];
        while request_wire.len() < connection_preface().len() {
            let read_len = stream.read(&mut recv_buf).map_err(|err| err.to_string())?;
            if read_len == 0 {
                break;
            }
            request_wire.extend_from_slice(&recv_buf[..read_len]);
        }
        if request_wire.len() < connection_preface().len()
            || &request_wire[..connection_preface().len()] != connection_preface()
        {
            return Err(String::from("grpc preface missing"));
        }
        let mut request_conn = Http2Connection::new();
        let request_stream_id = request_conn.create_stream();
        let mut frame_wire = request_wire.split_off(connection_preface().len());
        loop {
            let mut consumed = 0usize;
            while consumed < frame_wire.len() {
                let Some((frame, used)) = Http2Frame::decode(&frame_wire[consumed..]) else {
                    break;
                };
                request_conn
                    .process_frame(&frame)
                    .map_err(|err| format!("{err:?}"))?;
                consumed += used;
            }
            if consumed > 0 {
                frame_wire.drain(..consumed);
            }
            if request_conn
                .get_stream(request_stream_id)
                .is_some_and(|state| state.end_stream && state.data.len() >= 5)
            {
                break;
            }
            let read_len = match stream.read(&mut recv_buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(err) if err.kind() == std::io::ErrorKind::TimedOut => break,
                Err(err) => return Err(err.to_string()),
            };
            frame_wire.extend_from_slice(&recv_buf[..read_len]);
        }
        let request_state = request_conn
            .get_stream(request_stream_id)
            .ok_or_else(|| String::from("grpc request stream missing"))?;
        if !request_state.end_stream || request_state.data.len() < 5 {
            return Err(String::from("grpc request incomplete"));
        }

        let mut response_headers = BTreeMap::new();
        response_headers.insert(String::from(":status"), String::from("200"));
        response_headers.insert(
            String::from("content-type"),
            String::from("application/grpc"),
        );
        response_headers.insert(String::from("grpc-status"), String::from("0"));
        let mut encoder = HpackEncoder::new(4096);
        let headers_frame =
            Http2Frame::headers(1, encoder.encode(&response_headers), false).encode();

        let mut reply = ProtoMessage::new();
        reply.add_string(1, "Hello, loopback!");
        let payload = reply.serialize();
        let mut grpc_body = Vec::with_capacity(5 + payload.len());
        grpc_body.push(0);
        grpc_body.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        grpc_body.extend_from_slice(&payload);
        let data_frame = Http2Frame::data(1, grpc_body, true).encode();
        let settings = Http2Frame::settings(&ech_os::net::http2::Http2Settings::default()).encode();

        stream.write_all(&settings).map_err(|err| err.to_string())?;
        stream
            .write_all(&headers_frame)
            .map_err(|err| err.to_string())?;
        stream
            .write_all(&data_frame)
            .map_err(|err| err.to_string())?;
        stream.flush().map_err(|err| err.to_string())?;
        let _ = stream.shutdown(Shutdown::Write);
        Ok(())
    });

    thread::sleep(Duration::from_millis(50));

    let mut stream = TcpStream::connect(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
        .map_err(|err| format!("grpc connect failed: {err}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|err| err.to_string())?;
    stream
        .write_all(connection_preface())
        .map_err(|err| err.to_string())?;

    let mut conn = Http2Connection::new();
    let settings = Http2Frame::settings(&conn.settings).encode();
    stream.write_all(&settings).map_err(|err| err.to_string())?;

    let stream_id = conn.create_stream();
    let mut headers = BTreeMap::new();
    headers.insert(String::from(":method"), String::from("POST"));
    headers.insert(String::from(":path"), String::from("/Greeter/SayHello"));
    headers.insert(String::from(":authority"), format!("127.0.0.1:{port}"));
    headers.insert(
        String::from("content-type"),
        String::from("application/grpc"),
    );
    headers.insert(String::from("te"), String::from("trailers"));
    let headers_frame =
        Http2Frame::headers(stream_id, conn.encoder.encode(&headers), false).encode();
    stream
        .write_all(&headers_frame)
        .map_err(|err| err.to_string())?;

    let mut request = ProtoMessage::new();
    request.add_string(1, "loopback");
    let request_payload = request.serialize();
    let mut request_body = Vec::with_capacity(5 + request_payload.len());
    request_body.push(0);
    request_body.extend_from_slice(&(request_payload.len() as u32).to_be_bytes());
    request_body.extend_from_slice(&request_payload);
    let data_frame = Http2Frame::data(stream_id, request_body, true).encode();
    stream
        .write_all(&data_frame)
        .map_err(|err| err.to_string())?;

    let mut wire = Vec::new();
    let mut recv_buf = [0u8; 4096];
    loop {
        match stream.read(&mut recv_buf) {
            Ok(0) => break,
            Ok(n) => {
                wire.extend_from_slice(&recv_buf[..n]);
                let mut consumed = 0usize;
                while consumed < wire.len() {
                    let Some((frame, used)) = Http2Frame::decode(&wire[consumed..]) else {
                        break;
                    };
                    conn.process_frame(&frame)
                        .map_err(|err| format!("{err:?}"))?;
                    consumed += used;
                }
                if consumed > 0 {
                    wire.drain(..consumed);
                }
                if conn
                    .get_stream(stream_id)
                    .is_some_and(|stream| stream.end_stream)
                {
                    break;
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(err) if err.kind() == std::io::ErrorKind::TimedOut => break,
            Err(err) => return Err(err.to_string()),
        }
    }

    server.join().map_err(|_| String::from("grpc panic"))??;
    let state = conn
        .get_stream(stream_id)
        .ok_or_else(|| String::from("grpc stream missing"))?;
    if !state.end_stream || state.data.len() < 5 {
        return Err(String::from("grpc response incomplete"));
    }
    let msg_len =
        u32::from_be_bytes([state.data[1], state.data[2], state.data[3], state.data[4]]) as usize;
    let message =
        ProtoMessage::deserialize(&state.data[5..5 + msg_len]).map_err(|err| format!("{err:?}"))?;
    if message.get_string(1).as_deref() != Some("Hello, loopback!") {
        return Err(String::from("grpc reply mismatch"));
    }
    println!("smoke:grpc:ok");
    Ok(())
}

fn smoke_ebpf_jit() -> Result<(), String> {
    let mut loader = EbpfLoader::new();
    let program = vec![
        ((BPF_ALU | BPF_MOV | BPF_K) as u64) << 56 | (1u64 << 32) | 1u64,
        ((BPF_JMP | BPF_EXIT) as u64) << 56,
    ];
    let translated = [
        ech_os::ebpf::BpfInsn::new(BPF_ALU | BPF_MOV | BPF_K, 0, 0, 0, 1),
        ech_os::ebpf::BpfInsn::new(BPF_JMP | BPF_EXIT, 0, 0, 0, 0),
    ];
    if env::var_os("PHASE1_DEBUG_EBPF").is_some() || env::var_os("PHASE1_SKIP_EBPF_RUN").is_some() {
        eprintln!("probe:before-compile");
        append_probe("before-compile");
        eprintln!("probe:after-probe-write");
        let skip_exec = env::var_os("PHASE1_SKIP_EBPF_RUN").is_some();
        let bytes = if skip_exec {
            ech_os::ebpf_jit::compile_bytes(&translated)
                .map_err(|err| format!("jit codegen debug failed: {err:?}"))?
        } else {
            ech_os::ebpf_jit::JitCompiler::compile(&translated)
                .map_err(|err| format!("jit compile debug failed: {err:?}"))?
                .code_bytes()
                .to_vec()
        };
        eprintln!("probe:after-compile");
        append_probe("after-compile");
        let bytes = bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>()
            .join(" ");
        eprintln!("probe:after-bytes");
        append_probe("after-bytes");
        println!("smoke:ebpf:jit:{bytes}");
        if skip_exec {
            eprintln!("probe:before-return");
            append_probe("before-return");
            return Ok(());
        }
    }
    loader
        .load_program("allow_all", program, BPF_PROG_TYPE_SOCKET_FILTER)
        .map_err(|err| format!("{err:?}"))?;
    loader
        .jit_compile("allow_all")
        .map_err(|err| format!("{err:?}"))?;
    loader
        .attach_socket_filter("net:ingress", "allow_all")
        .map_err(|err| format!("{err:?}"))?;
    let verdict = loader
        .run_socket_filter("net:ingress", &[0xde, 0xad, 0xbe, 0xef])
        .map_err(|err| format!("{err:?}"))?;
    if verdict != 1 {
        return Err(format!("ebpf verdict mismatch: {verdict}"));
    }
    println!("smoke:ebpf:ok");
    Ok(())
}

fn smoke_doh_matrix() -> Result<(), String> {
    let query = DohClient::build_query("example.com", DnsRecordType::A);
    let encoded = base64url_encode(&query);
    let query_b64 = encode_b64(&query);
    let mut successes = 0usize;
    let mut failures = Vec::new();
    for (provider, url) in [
        ("cloudflare", "https://cloudflare-dns.com/dns-query"),
        ("google", "https://dns.google/dns-query"),
        ("quad9", "https://dns.quad9.net/dns-query"),
    ] {
        let script = format!(
            "$ProgressPreference='SilentlyContinue'; Add-Type -AssemblyName System.Net.Http; \
             $client=[System.Net.Http.HttpClient]::new(); \
             $request=[System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Get, '{url}?dns={encoded}'); \
             $request.Headers.Accept.ParseAdd('application/dns-message'); \
             $response=$client.SendAsync($request).GetAwaiter().GetResult(); \
             if(-not $response.IsSuccessStatusCode) {{ \
               $post=[System.Net.Http.HttpRequestMessage]::new([System.Net.Http.HttpMethod]::Post, '{url}'); \
               $post.Headers.Accept.ParseAdd('application/dns-message'); \
               $payload=[Convert]::FromBase64String('{query_b64}'); \
               $post.Content=[System.Net.Http.ByteArrayContent]::new($payload); \
               $post.Content.Headers.ContentType=[System.Net.Http.Headers.MediaTypeHeaderValue]::Parse('application/dns-message'); \
               $response=$client.SendAsync($post).GetAwaiter().GetResult(); \
             }}; \
             if(-not $response.IsSuccessStatusCode) {{ throw 'http-status:' + [int]$response.StatusCode }}; \
             $bytes=$response.Content.ReadAsByteArrayAsync().GetAwaiter().GetResult(); \
             [Console]::Out.Write([Convert]::ToBase64String($bytes));"
        );
        match run_powershell(&script)
            .and_then(|output| decode_b64(&output))
            .and_then(|bytes| {
                let parsed = DohClient::parse_response(&bytes)
                    .map_err(|err| format!("{provider}:{err:?}"))?;
                parsed
                    .get_a()
                    .ok_or_else(|| format!("doh A answer missing: {provider}"))
            }) {
            Ok(answer) => {
                successes += 1;
                println!("smoke:doh:{provider}:{answer}");
            }
            Err(err) => {
                println!("smoke:doh:{provider}:err:{err}");
                failures.push(format!("{provider}:{err}"));
            }
        }
    }
    if successes < 2 {
        return Err(format!(
            "doh provider quorum failed: successes={successes} failures={}",
            failures.join(" | ")
        ));
    }
    Ok(())
}

fn smoke_dot_matrix() -> Result<(), String> {
    let query = DotClient::build_query("example.com", DnsRecordType::A);
    let query_b64 = encode_b64(&query);
    for (provider, ip, name) in [
        ("cloudflare", "1.1.1.1", "cloudflare-dns.com"),
        ("google", "8.8.8.8", "dns.google"),
        ("quad9", "9.9.9.9", "dns.quad9.net"),
    ] {
        let output = run_powershell(&make_dot_script(&query_b64, ip, name))?;
        let bytes = decode_b64(&output)?;
        let parsed =
            DotClient::parse_response(&bytes).map_err(|err| format!("{provider}:{err:?}"))?;
        let answer = parsed
            .get_a()
            .ok_or_else(|| format!("dot A answer missing: {provider}"))?;
        println!("smoke:dot:{provider}:{answer}");
    }
    Ok(())
}

fn smoke_trust_matrix() -> Result<(), String> {
    let script = r#"
$ProgressPreference='SilentlyContinue'
Add-Type -AssemblyName System.Net.Http
$sites = @(
  @{ name='example'; url='https://example.com/' },
  @{ name='expired'; url='https://expired.badssl.com/' },
  @{ name='wrong_host'; url='https://wrong.host.badssl.com/' },
  @{ name='revoked'; url='https://revoked.badssl.com/' }
)
$handler = New-Object System.Net.Http.HttpClientHandler
$handler.CheckCertificateRevocationList = $true
$client = [System.Net.Http.HttpClient]::new($handler)
$lines = New-Object System.Collections.Generic.List[string]
foreach($site in $sites) {
  try {
    $resp = $client.GetAsync($site.url).GetAwaiter().GetResult()
    $lines.Add($site.name + ':OK:' + [int]$resp.StatusCode)
  } catch {
    $message = $_.Exception.GetBaseException().Message.Replace("`r",' ').Replace("`n",' ')
    $lines.Add($site.name + ':ERR:' + $message)
  }
}
[Console]::Out.Write(($lines -join [Environment]::NewLine))
"#;
    let output = run_powershell(script)?;
    let mut example_ok = false;
    let mut rejected = 0usize;
    for line in output.lines() {
        if line.starts_with("example:OK:") {
            example_ok = true;
        }
        if line.starts_with("expired:ERR:")
            || line.starts_with("wrong_host:ERR:")
            || line.starts_with("revoked:ERR:")
        {
            rejected += 1;
        }
        println!("smoke:trust:{line}");
    }
    if !example_ok || rejected < 2 {
        return Err(format!(
            "trust matrix insufficient: example_ok={example_ok} rejected={rejected}"
        ));
    }
    Ok(())
}

fn smoke_ops_matrix() -> Result<(), String> {
    let ok_listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
        .map_err(|err| format!("ops bind failed: {err}"))?;
    let ok_port = ok_listener
        .local_addr()
        .map_err(|err| format!("ops local_addr failed: {err}"))?
        .port();
    let ok_server = thread::spawn(move || -> Result<(), String> {
        let (mut stream, _) = ok_listener.accept().map_err(|err| err.to_string())?;
        let mut req_buf = [0u8; 1024];
        let read_len = stream.read(&mut req_buf).map_err(|err| err.to_string())?;
        if read_len == 0 {
            return Err(String::from("ops http request missing"));
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
            .map_err(|err| err.to_string())?;
        stream.flush().map_err(|err| err.to_string())?;
        Ok(())
    });

    let mut http_client = TcpStream::connect(SocketAddrV4::new(Ipv4Addr::LOCALHOST, ok_port))
        .map_err(|err| format!("ops http connect failed: {err}"))?;
    http_client
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .map_err(|err| format!("ops http write failed: {err}"))?;
    let mut response = String::new();
    http_client
        .read_to_string(&mut response)
        .map_err(|err| format!("ops http read failed: {err}"))?;
    if !response.starts_with("HTTP/1.1 200 OK") {
        return Err(format!("ops http unexpected response: {response}"));
    }
    ok_server
        .join()
        .map_err(|_| String::from("ops http server panic"))??;
    println!("smoke:ops:http:ok");

    let timeout_listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
        .map_err(|err| format!("ops timeout bind failed: {err}"))?;
    let timeout_port = timeout_listener
        .local_addr()
        .map_err(|err| format!("ops timeout local_addr failed: {err}"))?
        .port();
    let timeout_server = thread::spawn(move || -> Result<(), String> {
        let (_stream, _) = timeout_listener.accept().map_err(|err| err.to_string())?;
        thread::sleep(Duration::from_millis(600));
        Ok(())
    });
    let mut timeout_client =
        TcpStream::connect(SocketAddrV4::new(Ipv4Addr::LOCALHOST, timeout_port))
            .map_err(|err| format!("ops timeout connect failed: {err}"))?;
    timeout_client
        .set_read_timeout(Some(Duration::from_millis(150)))
        .map_err(|err| format!("ops timeout set_read_timeout failed: {err}"))?;
    let mut timeout_buf = [0u8; 16];
    match timeout_client.read(&mut timeout_buf) {
        Ok(0) => return Err(String::from("ops timeout expected read timeout, got eof")),
        Ok(n) => return Err(format!("ops timeout expected failure, got {n} bytes")),
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) => {}
        Err(err) => return Err(format!("ops timeout unexpected error: {err}")),
    }
    timeout_server
        .join()
        .map_err(|_| String::from("ops timeout server panic"))??;
    println!("smoke:ops:http-timeout:ok");

    let closed_listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
        .map_err(|err| format!("ops closed bind failed: {err}"))?;
    let closed_port = closed_listener
        .local_addr()
        .map_err(|err| format!("ops closed local_addr failed: {err}"))?
        .port();
    drop(closed_listener);
    match TcpStream::connect(SocketAddrV4::new(Ipv4Addr::LOCALHOST, closed_port)) {
        Ok(_) => return Err(String::from("ops closed-port expected connect failure")),
        Err(_) => println!("smoke:ops:tcp-closed-port:ok"),
    }

    let ping_output = run_powershell(
        "$ok=Test-Connection -Quiet -Count 1 127.0.0.1; if($ok){[Console]::Out.Write('ping:ok')} else { throw 'ping failed' }",
    )?;
    if ping_output.trim() != "ping:ok" {
        return Err(format!("ops ping unexpected output: {ping_output}"));
    }
    println!("smoke:ops:ping:ok");
    Ok(())
}

fn smoke_ipv6_control_plane() -> Result<(), String> {
    let router_addr = Ipv6Addr::from_segments([0xfe80, 0, 0, 0, 0, 0, 0, 1]);
    let dest_addr = Ipv6Addr::from_segments([0xff02, 0, 0, 0, 0, 0, 0, 1]);
    let router_mac = [0x02, 0xaa, 0xbb, 0xcc, 0xdd, 0x01];
    let payload = vec![
        134,
        0,
        0,
        0,
        64,
        0,
        0,
        120,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        1,
        1,
        router_mac[0],
        router_mac[1],
        router_mac[2],
        router_mac[3],
        router_mac[4],
        router_mac[5],
    ];
    let packet = Ipv6Packet::new(
        Ipv6Header::new(router_addr, dest_addr, 58, payload.len() as u16),
        &payload,
    );
    process_ipv6_packet(&packet.serialize())
        .map_err(|err| format!("ipv6 RA process failed: {err:?}"))?;

    let remote = Ipv6Addr::from_segments([0x2606, 0x4700, 0, 0, 0, 0, 0, 0x1111]);
    let (next_hop, mac) = select_next_hop(&remote, 0)
        .ok_or_else(|| String::from("ipv6 next hop missing after RA"))?;
    if next_hop != router_addr || mac != MacAddr::new(router_mac) {
        return Err(format!(
            "ipv6 next hop mismatch: hop={:?} mac={:?}",
            next_hop, mac
        ));
    }
    println!("smoke:ipv6:ra-next-hop:ok");
    Ok(())
}

fn smoke_cni_lifecycle() -> Result<(), String> {
    let manager = CniManager::new();
    let config = CniConfig {
        cni_version: CNI_VERSION.to_string(),
        container_name: String::from("phase1-live"),
        container_id: String::from("phase1-live-interop"),
        netns: String::from("/var/run/netns/phase1-live"),
        bridge: String::from(DEFAULT_BRIDGE_NAME),
        ip_address: String::new(),
        gateway: String::from("10.244.0.1"),
        subnet: String::from(DEFAULT_SUBNET),
        dns_servers: vec![String::from("1.1.1.1"), String::from("8.8.8.8")],
        mtu: 1500,
        args: BTreeMap::new(),
    };
    let add = manager
        .run_command("ADD", &config)
        .map_err(|err| format!("cni add failed: {err:?}"))?;
    manager
        .run_check(&config)
        .map_err(|err| format!("cni check failed: {err:?}"))?;
    manager
        .run_delete(&config)
        .map_err(|err| format!("cni del failed: {err:?}"))?;
    if manager.run_check(&config).is_ok() {
        return Err(String::from("cni check should fail after delete"));
    }
    println!("smoke:cni:add:{}", add.to_json());
    println!("smoke:cni:lifecycle:ok");
    Ok(())
}

fn smoke_http3() -> Result<(), String> {
    let script = r#"
$ProgressPreference='SilentlyContinue'
$edgeCandidates = @(
  'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe',
  'C:\Program Files\Microsoft\Edge\Application\msedge.exe'
)
$edge = $edgeCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $edge) {
  Write-Output 'http3:ERR:edge-not-found'
  exit 1
}
$log = Join-Path $env:TEMP ('echos-http3-' + [Guid]::NewGuid().ToString() + '.json')
try {
  & $edge --headless=new --disable-gpu --enable-quic --origin-to-force-quic-on=cloudflare-quic.com:443 --log-net-log=$log --net-log-capture-mode=Everything --dump-dom https://cloudflare-quic.com/ | Out-Null
  $raw = if (Test-Path $log) { Get-Content $log -Raw } else { '' }
  if ($raw -match 'HTTP3_SESSION' -or $raw -match 'QUIC_SESSION' -or $raw -match '"next_proto":"h3"' -or $raw -match 'h3') {
    Write-Output 'http3:OK:edge-quic'
  } else {
    Write-Output 'http3:ERR:quic-netlog-marker-missing'
    exit 1
  }
} finally {
  if (Test-Path $log) { Remove-Item $log -Force }
}
"#;
    let output = run_powershell(script)?;
    let mut saw_ok = false;
    for line in output.lines() {
        println!("{line}");
        if line.starts_with("http3:OK:") {
            saw_ok = true;
        }
    }
    if !saw_ok {
        return Err(String::from("http3 smoke did not confirm QUIC transport"));
    }
    Ok(())
}

fn run_powershell(script: &str) -> Result<String, String> {
    let output = Command::new(POWERSHELL_EXE)
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|err| format!("powershell spawn failed: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "powershell exit {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn make_dot_script(query_b64: &str, ip: &str, name: &str) -> String {
    format!(
        "$ProgressPreference='SilentlyContinue'; \
         $query=[byte[]][Convert]::FromBase64String('{query_b64}'); \
         $tcp=[System.Net.Sockets.TcpClient]::new(); \
         $tcp.ReceiveTimeout=5000; \
         $tcp.SendTimeout=5000; \
         $tcp.Connect('{ip}', 853); \
         $ssl=[System.Net.Security.SslStream]::new($tcp.GetStream(), $false); \
         $ssl.ReadTimeout=5000; \
         $ssl.WriteTimeout=5000; \
         $ssl.AuthenticateAsClient('{name}'); \
         $queryLen=[int]$query.Length; \
         $prefix=[byte[]]@((($queryLen -shr 8) -band 0xff), ($queryLen -band 0xff)); \
         $ssl.Write($prefix, 0, 2); \
         $ssl.Write($query, 0, $queryLen); \
         $ssl.Flush(); \
         $lenBuf=New-Object byte[] 2; \
         $read=0; \
         while($read -lt 2) {{ \
           $n=$ssl.Read($lenBuf, $read, 2-$read); \
           if($n -le 0) {{ throw 'dot-length-eof' }}; \
           $read += $n \
         }}; \
         $len=([int]$lenBuf[0] -shl 8) -bor [int]$lenBuf[1]; \
         $resp=New-Object byte[] $len; \
         $read=0; \
         while($read -lt $len) {{ \
           $n=$ssl.Read($resp, $read, $len-$read); \
           if($n -le 0) {{ throw 'dot-body-eof' }}; \
           $read += $n \
         }}; \
         [Console]::Out.Write([Convert]::ToBase64String($resp));"
    )
}

fn append_probe(stage: &str) {
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(Path::new("phase1_ebpf_probe.log"))
        .and_then(|mut file| writeln!(file, "{stage}"));
}

fn encode_b64(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0usize;
    while i + 3 <= data.len() {
        let chunk = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | data[i + 2] as u32;
        out.push(TABLE[((chunk >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((chunk >> 12) & 0x3f) as usize] as char);
        out.push(TABLE[((chunk >> 6) & 0x3f) as usize] as char);
        out.push(TABLE[(chunk & 0x3f) as usize] as char);
        i += 3;
    }
    match data.len() - i {
        1 => {
            let chunk = (data[i] as u32) << 16;
            out.push(TABLE[((chunk >> 18) & 0x3f) as usize] as char);
            out.push(TABLE[((chunk >> 12) & 0x3f) as usize] as char);
            out.push('=');
            out.push('=');
        }
        2 => {
            let chunk = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8);
            out.push(TABLE[((chunk >> 18) & 0x3f) as usize] as char);
            out.push(TABLE[((chunk >> 12) & 0x3f) as usize] as char);
            out.push(TABLE[((chunk >> 6) & 0x3f) as usize] as char);
            out.push('=');
        }
        _ => {}
    }
    out
}

fn decode_b64(data: &str) -> Result<Vec<u8>, String> {
    fn value(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            b'=' => Some(64),
            _ => None,
        }
    }

    let clean: Vec<u8> = data
        .as_bytes()
        .iter()
        .copied()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    if clean.len() % 4 != 0 {
        return Err(String::from("invalid base64 length"));
    }

    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    for chunk in clean.chunks_exact(4) {
        let a = value(chunk[0]).ok_or_else(|| String::from("invalid base64 digit"))?;
        let b = value(chunk[1]).ok_or_else(|| String::from("invalid base64 digit"))?;
        let c = value(chunk[2]).ok_or_else(|| String::from("invalid base64 digit"))?;
        let d = value(chunk[3]).ok_or_else(|| String::from("invalid base64 digit"))?;
        if a == 64 || b == 64 {
            return Err(String::from("invalid base64 padding"));
        }
        let word = ((a as u32) << 18)
            | ((b as u32) << 12)
            | (((c & 0x3f) as u32) << 6)
            | ((d & 0x3f) as u32);
        out.push(((word >> 16) & 0xff) as u8);
        if c != 64 {
            out.push(((word >> 8) & 0xff) as u8);
        }
        if d != 64 {
            out.push((word & 0xff) as u8);
        }
    }
    Ok(out)
}

fn base64url_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    let mut i = 0usize;
    while i + 3 <= data.len() {
        let chunk = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8) | data[i + 2] as u32;
        out.push(TABLE[((chunk >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((chunk >> 12) & 0x3f) as usize] as char);
        out.push(TABLE[((chunk >> 6) & 0x3f) as usize] as char);
        out.push(TABLE[(chunk & 0x3f) as usize] as char);
        i += 3;
    }
    match data.len() - i {
        1 => {
            let chunk = (data[i] as u32) << 16;
            out.push(TABLE[((chunk >> 18) & 0x3f) as usize] as char);
            out.push(TABLE[((chunk >> 12) & 0x3f) as usize] as char);
        }
        2 => {
            let chunk = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8);
            out.push(TABLE[((chunk >> 18) & 0x3f) as usize] as char);
            out.push(TABLE[((chunk >> 12) & 0x3f) as usize] as char);
            out.push(TABLE[((chunk >> 6) & 0x3f) as usize] as char);
        }
        _ => {}
    }
    out
}
