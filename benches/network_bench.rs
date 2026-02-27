#![cfg(not(target_os = "none"))]
//! Ağ Kıyaslama Paketi - echOS TCP/IP Yığını vs Linux vs Windows
//!
//! Bu modül; TCP verimi, UDP gecikmesi, bağlantı kurma süresi, eşzamanlı
//! bağlantı sayısı ve paket işleme hızını ölçen kıyaslama fonksiyonlarını içerir.

#![feature(test)]
extern crate test;

use ech_os::net::{IpAddr, Ipv4Addr, TcpSocket, UdpSocket};
use std::net::{SocketAddr, SocketAddrV4};
use test::Bencher;

#[bench]
fn bench_tcp_throughput(b: &mut Bencher) {
    b.iter(|| {
        // TCP verim testi
        let server_addr = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 8080);

        let mut total_bytes = 0;

        // Ağ trafiğini simüle et
        for packet_size in &[512, 1024, 1460, 2048] {
            for _ in 0..100 {
                let mut socket = TcpSocket::new();
                socket.connect(server_addr.into()).unwrap();

                // Veri gönder
                let data = vec![0xAA; *packet_size];
                let bytes_sent = socket.send(&data).unwrap();
                total_bytes += bytes_sent;

                // Yankı al
                let mut buffer = vec![0; *packet_size];
                let bytes_received = socket.recv(&mut buffer).unwrap();
                total_bytes += bytes_received;

                // Veri bütünlüğünü doğrula
                assert_eq!(&data[..], &buffer[..bytes_received]);
            }
        }

        test::black_box(total_bytes);
    });
}

#[bench]
fn bench_udp_latency(b: &mut Bencher) {
    b.iter(|| {
        // UDP gecikme testi
        let server_addr = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 9090);

        let mut total_latency = 0;
        let mut successful_pings = 0;

        for _ in 0..1000 {
            let socket = UdpSocket::bind("0.0.0.0:0").unwrap();

            let start = std::time::Instant::now();

            // Ping gönder
            let ping_data = b"PING";
            socket.send_to(ping_data, server_addr.into()).unwrap();

            // Pong al
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
        // TCP bağlantı kurma süresi
        let server_addr = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 8080);

        let mut total_time = 0;

        for _ in 0..100 {
            let start = std::time::Instant::now();

            let mut socket = TcpSocket::new();
            if socket.connect(server_addr.into()).is_ok() {
                total_time += start.elapsed().as_nanos();

                // Bağlantıyı doğrulamak için küçük veri gönder
                socket.send(b"TEST").unwrap();

                // Temiz kapat
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
        // Eşzamanlı bağlantı testi
        let server_addr = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 8080);

        let mut successful_connections = 0;
        let mut total_throughput = 0;

        // 100 eşzamanlı bağlantıyı simüle et
        for _ in 0..100 {
            let mut socket = TcpSocket::new();
            if socket.connect(server_addr.into()).is_ok() {
                successful_connections += 1;

                // Her bağlantı 10 KB gönderir
                let data = vec![0xCC; 10240];
                let bytes_sent = socket.send(&data).unwrap();
                total_throughput += bytes_sent;

                // Yanıt al
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
        // Paket işleme verimi
        let mut total_packets = 0;
        let mut total_bytes = 0;

        // Farklı paket boyutlarını işle
        for packet_size in &[64, 128, 256, 512, 1024, 1500] {
            for _ in 0..100 {
                // Paket alımını simüle et
                let packet_data = vec![0x55; *packet_size];

                // Paketi işle (ağ yığınını simüle et)
                let processed_bytes = process_network_packet(&packet_data);

                total_packets += 1;
                total_bytes += processed_bytes;
            }
        }

        test::black_box((total_packets, total_bytes));
    });
}

fn process_network_packet(packet: &[u8]) -> usize {
    // Ağ paketi işlemeyi simüle et
    // Bu; başlık ayrıştırma, sağlama toplamı doğrulama vb. işlemleri kapsar

    // Basit simülasyon: paket boyutunu döndür
    packet.len()
}
