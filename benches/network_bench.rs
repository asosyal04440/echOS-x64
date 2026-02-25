#![cfg(not(target_os = "none"))]
//! Network Benchmark Suite - echOS TCP/IP Stack vs Linux vs Windows

#![feature(test)]
extern crate test;

use ech_os::net::{IpAddr, Ipv4Addr, TcpSocket, UdpSocket};
use std::net::{SocketAddr, SocketAddrV4};
use test::Bencher;

#[bench]
fn bench_tcp_throughput(b: &mut Bencher) {
    b.iter(|| {
        // TCP throughput test
        let server_addr = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 8080);

        let mut total_bytes = 0;

        // Simulate network traffic
        for packet_size in &[512, 1024, 1460, 2048] {
            for _ in 0..100 {
                let mut socket = TcpSocket::new();
                socket.connect(server_addr.into()).unwrap();

                // Send data
                let data = vec![0xAA; *packet_size];
                let bytes_sent = socket.send(&data).unwrap();
                total_bytes += bytes_sent;

                // Receive echo
                let mut buffer = vec![0; *packet_size];
                let bytes_received = socket.recv(&mut buffer).unwrap();
                total_bytes += bytes_received;

                // Verify data integrity
                assert_eq!(&data[..], &buffer[..bytes_received]);
            }
        }

        test::black_box(total_bytes);
    });
}

#[bench]
fn bench_udp_latency(b: &mut Bencher) {
    b.iter(|| {
        // UDP latency test
        let server_addr = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 9090);

        let mut total_latency = 0;
        let mut successful_pings = 0;

        for _ in 0..1000 {
            let socket = UdpSocket::bind("0.0.0.0:0").unwrap();

            let start = std::time::Instant::now();

            // Send ping
            let ping_data = b"PING";
            socket.send_to(ping_data, server_addr.into()).unwrap();

            // Receive pong
            let mut buffer = [0; 4];
            if let Ok((bytes_received, _)) = socket.recv_from(&mut buffer) {
                if bytes_received == 4 && &buffer == b"PONG" {
                    let latency = start.elapsed().as_nanos();
                    total_latency += latency;
                    successful_pings += 1;
                }
            }
        }

        let avg_latency = if successful_pings > 0 {
            total_latency / successful_pings as u128
        } else {
            0
        };

        test::black_box(avg_latency);
    });
}

#[bench]
fn bench_connection_establishment(b: &mut Bencher) {
    b.iter(|| {
        // TCP connection establishment time
        let server_addr = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 8080);

        let mut total_time = 0;

        for _ in 0..100 {
            let start = std::time::Instant::now();

            let mut socket = TcpSocket::new();
            if socket.connect(server_addr.into()).is_ok() {
                total_time += start.elapsed().as_nanos();

                // Send small data to verify connection
                socket.send(b"TEST").unwrap();

                // Clean close
                socket.shutdown().unwrap();
            }
        }

        let avg_connection_time = total_time / 100;
        test::black_box(avg_connection_time);
    });
}

#[bench]
fn bench_network_concurrency(b: &mut Bencher) {
    b.iter(|| {
        // Concurrent connections test
        let server_addr = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 8080);

        let mut successful_connections = 0;
        let mut total_throughput = 0;

        // Simulate 100 concurrent connections
        for _ in 0..100 {
            let mut socket = TcpSocket::new();
            if socket.connect(server_addr.into()).is_ok() {
                successful_connections += 1;

                // Each connection sends 10KB
                let data = vec![0xCC; 10240];
                let bytes_sent = socket.send(&data).unwrap();
                total_throughput += bytes_sent;

                // Receive response
                let mut buffer = vec![0; bytes_sent];
                let bytes_received = socket.recv(&mut buffer).unwrap();
                total_throughput += bytes_received;
            }
        }

        test::black_box((successful_connections, total_throughput));
    });
}

#[bench]
fn bench_packet_processing(b: &mut Bencher) {
    b.iter(|| {
        // Packet processing throughput
        let mut total_packets = 0;
        let mut total_bytes = 0;

        // Process different packet sizes
        for packet_size in &[64, 128, 256, 512, 1024, 1500] {
            for _ in 0..100 {
                // Simulate packet reception
                let packet_data = vec![0x55; *packet_size];

                // Process packet (simulate network stack)
                let processed_bytes = process_network_packet(&packet_data);

                total_packets += 1;
                total_bytes += processed_bytes;
            }
        }

        test::black_box((total_packets, total_bytes));
    });
}

fn process_network_packet(packet: &[u8]) -> usize {
    // Simulate network packet processing
    // This would include parsing headers, checksum verification, etc.

    // Simple simulation: just return the packet size
    packet.len()
}
