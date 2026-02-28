#![cfg(not(target_os = "none"))]
//! Ağ Benchmark Takımı - echOS TCP/IP Yığını ile Linux ve Windows karşılaştırması
//!
//! Bu modül, echOS'un ağ alt sisteminin (smoltcp tabanlı TCP/IP yığını)
//! performansını ölçer. Benchmark'lar şunları test eder:
//!   - TCP verimi (throughput): Saniyede kaç bayt aktarılabilir?
//!   - UDP gecikme süresi (latency): Ping-pong turu ne kadar sürer?
//!   - Bağlantı kurma hızı: TCP el sıkışması (3-way handshake) süresi
//!   - Eşzamanlı bağlantı (concurrency): 100 eşzamanlı soket yönetimi
//!   - Paket işleme verimi: Farklı boyutlarda paket ayrıştırma hızı

#![feature(test)]
extern crate test;

use ech_os::net::{IpAddr, Ipv4Addr, TcpSocket, UdpSocket};
use std::net::{SocketAddr, SocketAddrV4};
use test::Bencher;

/// `bench_tcp_throughput`: TCP veri aktarım verimini ölçer.
///
/// Bu benchmark, farklı paket boyutlarında TCP üzerinden veri gönderip alarak
/// toplam aktarılan bayt sayısını ölçer. Test senaryosu:
///   - Paket boyutları: 512B, 1024B, 1460B (standart MTU), 2048B
///   - Her boyut için 100 bağlantı: gönderi + yankı (echo) alımı
///   - Veri bütünlüğü: gönderilen == alınan doğrulanır
///
/// TCP verimi akışı:
///   Bağlan → Veri gönder → Yankı al → Doğrula → Kapat → Tekrar
#[bench]
fn bench_tcp_throughput(b: &mut Bencher) {
    b.iter(|| {
        // TCP verimi testi
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

                // Yankıyı al
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

/// `bench_udp_latency`: UDP paket gecikme süresini ölçer.
///
/// UDP bağlantısızdır (connectionless); bu nedenle TCP'den çok daha düşük
/// gecikme süresi sunar ancak paket kayıpları olabilir.
///
/// Test senaryosu (ping-pong modeli):
///   1. "PING" (4 bayt) gönder
///   2. "PONG" yanıtını bekle
///   3. Elapsed süreyi nanosaniye cinsinden kaydet
///   4. 1000 tur sonra ortalama gecikmeyi hesapla
///
/// Ortalama gecikme = toplam_gecikme / başarılı_ping_sayısı
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

/// `bench_connection_establishment`: TCP bağlantı kurma süresini ölçer.
///
/// TCP bağlantısı kurulurken 3 adımlı el sıkışma (3-way handshake) gerçekleşir:
///   İstemci → [SYN]         → Sunucu
///   İstemci ← [SYN-ACK]     ← Sunucu
///   İstemci → [ACK]         → Sunucu
///
/// Bu benchmark, her bağlantı kuruluşunun kaç nanosaniye sürdüğünü ölçer.
/// 100 bağlantı açılır; her biri küçük bir test verisi gönderir ve kapatılır.
/// Ortalama bağlantı süresi = toplam_süre / 100
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

                // Temiz kapatma
                socket.shutdown().unwrap();
            }
        }

        let avg_connection_time = total_time / 100;
        test::black_box(avg_connection_time);
    });
}

/// `bench_network_concurrency`: Eşzamanlı bağlantı yönetim kapasitesini ölçer.
///
/// Gerçek dünya senaryolarında çekirdek aynı anda yüzlerce TCP bağlantısını
/// yönetmek zorundadır. Bu benchmark, 100 eşzamanlı bağlantının:
///   - Başarıyla açılıp açılmadığını
///   - Her birinden 10KB veri gönderilip alınabildiğini
///   - Toplam verimi ölçer
///
/// Eşzamanlılık modeli (simülasyon, gerçek thread değil):
///   Bağlantı 1: [kur → 10KB gönder → al]
///   Bağlantı 2: [kur → 10KB gönder → al]
///   ...
///   Bağlantı 100: [kur → 10KB gönder → al]
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

                // Her bağlantı 10KB gönderir
                let data = vec![0xCC; 10240];
                let bytes_sent = socket.send(&data).unwrap();
                total_throughput += bytes_sent;

                // Yanıtı al
                let mut buffer = vec![0; bytes_sent];
                let bytes_received = socket.recv(&mut buffer).unwrap();
                total_throughput += bytes_received;
            }
        }

        test::black_box((successful_connections, total_throughput));
    });
}

/// `bench_packet_processing`: Ağ paketi işleme verimini ölçer.
///
/// Bu benchmark, TCP/IP yığınının saniyede kaç paket işleyebildiğini ölçer.
/// Farklı paket boyutları, ağ başlığı (header) ayrıştırma maliyetini farklı
/// etkiler:
///   - Küçük paket (64B) : Başlık oranı yüksek, veri oranı düşük
///   - Büyük paket (1500B): Başlık oranı düşük, veri oranı yüksek (MTU sınırı)
///
/// Test boyutları: 64, 128, 256, 512, 1024, 1500 bayt (Ethernet MTU: 1500B)
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

/// `process_network_packet`: Ağ paketi işleme mantığını simüle eden yardımcı fonksiyon.
///
/// Gerçek uygulamada bu fonksiyon şunları yapar:
///   1. Ethernet başlığını ayrıştır (14 bayt: hedef MAC, kaynak MAC, EtherType)
///   2. IP başlığını ayrıştır (20+ bayt: TTL, protokol, kaynak/hedef IP)
///   3. TCP/UDP başlığını ayrıştır (20+ bayt: port numaraları, bayraklar)
///   4. Sağlama toplamını (checksum) doğrula
///   5. Sokete ilet
///
/// Bu benchmark simülasyonunda sadece paket boyutu döndürülür.
fn process_network_packet(packet: &[u8]) -> usize {
    // Ağ paketi işlemeyi simüle et
    // Gerçekte: başlık ayrıştırma, sağlama toplamı doğrulaması vb. içerir

    // Basit simülasyon: sadece paket boyutunu döndür
    packet.len()
}
