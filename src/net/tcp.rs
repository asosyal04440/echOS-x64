//! # TCP Protokolü (Transmission Control Protocol)
//!
//! TCP durum makinesi ve bağlantı yönetimi, SACK desteği ile birlikte.
//!
//! ## TCP Nedir?
//! TCP, güvenilir, sıralı ve hata denetimli veri iletimi sağlayan bir protokoldür.
//! IP (Internet Protocol) üzerinde çalışır ve iletişim kurmak için üç yönlü el sıkışma
//! (three-way handshake) kullanır.
//!
//! ## TCP El Sıkışma Diyagramı (Three-Way Handshake)
//!
//! ```
//!  İstemci                        Sunucu
//!     |                              |
//!     |-------- SYN (seq=x) -------->|   1. İstemci bağlantı isteği gönderir
//!     |                              |
//!     |<----- SYN-ACK (seq=y) -------|   2. Sunucu kabul eder ve kendi seq'ini gönderir
//!     |        (ack=x+1)             |
//!     |                              |
//!     |-------- ACK (ack=y+1) ------>|   3. İstemci onayla, bağlantı KURULDU
//!     |                              |
//!     |====== VERİ TRANSFERİ ========|   Artık veri gönderilebilir
//! ```
//!
//! ## TCP Kapatma Diyagramı (Four-Way Termination)
//!
//! ```
//!  İstemci                        Sunucu
//!     |                              |
//!     |-------- FIN ----------------->|   1. Aktif kapat isteği
//!     |<-------- ACK -----------------|   2. Sunucu onaylar
//!     |<-------- FIN -----------------|   3. Sunucu da kapatar
//!     |-------- ACK ----------------->|   4. İstemci son onayı
//!     |  (2*MSL bekleme: TIME_WAIT)   |
//! ```

use super::ip::{IpProtocol, Ipv4Packet};
use super::ipv6::{Ipv6NextHeader, Ipv6Packet};
use super::socket::AddressFamily;
use super::{allocate_socket_id, IpAddr, Ipv4Addr, NetError, Port, SocketAddr};
use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use spin::Mutex;

// ============================================================================
// TCP SEÇENEKLER (OPTIONS)
// ============================================================================
// TCP başlığına eklenebilen isteğe bağlı uzantılar.
// Her seçenek: [tür(1B)] [uzunluk(1B)] [veri(değişken)] formatındadır.
// Toplam seçenek alanı 40 baytı geçemez (min başlık 20B, max 60B).

/// TCP seçenek türleri - RFC 793 ve uzantıları
pub const TCPOPT_EOL: u8 = 0; // End of Option List - seçenekler bitti
pub const TCPOPT_NOP: u8 = 1; // No Operation - hizalama dolgusu
pub const TCPOPT_MSS: u8 = 2; // Maximum Segment Size - maks. segment boyutu
pub const TCPOPT_WINDOW_SCALE: u8 = 3; // Window Scale - pencere ölçekleme (RFC 7323)
pub const TCPOPT_SACK_PERMITTED: u8 = 4; // SACK izni (RFC 2018)
pub const TCPOPT_SACK: u8 = 5; // Selective ACK verisi
pub const TCPOPT_TIMESTAMP: u8 = 8; // Zaman damgası (RTT ölçümü için)

// ============================================================================
// TCP SACK (Seçici Onaylama - Selective Acknowledgment)
// ============================================================================
// SACK, alıcının hangi veri bloklarını aldığını gönderene bildirmesine izin verir.
// Bu sayede gönderen sadece kayıp olan segmentleri yeniden iletir.
//
// SACK olmadan (Kümülatif ACK):
//   Gönderilen: [1][2][3][4][5][6][7][8]
//   Kayıp:          [2]   [4][5]
//   ACK sadece: ack=2 (2'nin önüne kadar)
//   Gönderen 2'den itibaren HEPSİNİ yeniden gönderir
//
// SACK ile:
//   Gönderilen: [1][2][3][4][5][6][7][8]
//   Kayıp:          [2]   [4][5]
//   ACK: ack=2, SACK=[3..4, 6..9]
//   Gönderen sadece [2] ve [4][5]'i yeniden gönderir

/// SACK bloğu: [başlangıç, bitiş) aralığındaki sıra numaraları
/// Not: Bitiş nokta dahil DEĞİLDİR (yarı açık aralık)
#[derive(Clone, Copy, Debug, Default)]
pub struct SackBlock {
    pub start: u32,
    pub end: u32,
}

impl SackBlock {
    pub fn new(start: u32, end: u32) -> Self {
        SackBlock { start, end }
    }

    /// Verilen sıra numarasının bu blok içinde olup olmadığını kontrol eder.
    /// Dikkat: 32-bit sıra numaraları taşabilir (wraparound), bu yüzden
    /// basit karşılaştırma yerine imzalı aritmetik kullanılır.
    pub fn contains(&self, seq: u32) -> bool {
        // Taşma (wraparound) durumu ele alınır
        if self.start <= self.end {
            seq >= self.start && seq < self.end
        } else {
            seq >= self.start || seq < self.end
        }
    }

    /// Blok uzunluğunu bayt cinsinden döndürür.
    /// wrapping_sub kullanılır çünkü sıra numaraları taşabilir.
    pub fn len(&self) -> u32 {
        self.end.wrapping_sub(self.start)
    }
}

/// SACK panosu (scoreboard) - seçici yeniden iletim için alınan segmentleri izler.
/// RFC 2018'e göre en fazla 4 SACK bloğu gönderilebilir (başlık sınırlaması).
///
/// ```
/// SND.UNA    SACK blokları          SND.NXT
///   |    GAP1  |===|  GAP2  |===|    |
///   +----------+---+--------+---+----+
///              ^               ^
///           SACK[0]         SACK[1]
/// GAP1 ve GAP2 yeniden iletilmesi gereken aralıklardır.
/// ```
#[derive(Clone, Debug)]
pub struct SackScoreboard {
    /// Alınan SACK blokları (RFC 2018'e göre maks. 4)
    pub blocks: Vec<SackBlock>,
    /// Saklayabileceğimiz maks. blok sayısı
    pub max_blocks: usize,
    /// En yüksek SACK'lı sıra numarası
    pub high_sack: u32,
    /// SACK'lı toplam bayt sayısı
    pub sacked_bytes: u32,
}

impl Default for SackScoreboard {
    fn default() -> Self {
        Self::new()
    }
}

impl SackScoreboard {
    pub fn new() -> Self {
        SackScoreboard {
            blocks: Vec::with_capacity(4),
            max_blocks: 4,
            high_sack: 0,
            sacked_bytes: 0,
        }
    }

    /// Yeni bir SACK bloğu ekler, örtüşen blokları birleştirir.
    /// Bloklar her zaman başlangıç sıra numarasına göre sıralı tutulur.
    pub fn add_block(&mut self, block: SackBlock) {
        // Mevcut bloklarla örtüşme/bitişiklik kontrolü ve birleştirme
        let mut merged = false;
        for existing in &mut self.blocks {
            // Blokların örtüşüp örtüşmediğini veya bitişik olup olmadığını kontrol et
            if Self::blocks_overlap_or_adjacent(existing, &block) {
                // Birleştir: mevcut bloğu genişlet
                existing.start = existing.start.min(block.start);
                existing.end = existing.end.max(block.end);
                merged = true;
                break;
            }
        }

        if !merged {
            // Yer varsa yeni blok ekle
            if self.blocks.len() < self.max_blocks {
                self.blocks.push(block);
            } else {
                // En eski bloğu çıkar ve yenisini ekle (FIFO politikası)
                self.blocks.remove(0);
                self.blocks.push(block);
            }
        }

        // Blokları başlangıç sıra numarasına göre sırala
        self.blocks.sort_by_key(|b| b.start);

        // high_sack güncelle
        for block in &self.blocks {
            if block.end.wrapping_sub(self.high_sack) as i32 > 0 {
                self.high_sack = block.end;
            }
        }

        // SACK'lı toplam bayt sayısını yeniden hesapla
        self.sacked_bytes = self.blocks.iter().map(|b| b.len()).sum();
    }

    /// İki bloğun örtüşüp örtüşmediğini veya bitişik olup olmadığını kontrol eder.
    /// 32-bit taşma gözetilir (wrapping aritmetik).
    fn blocks_overlap_or_adjacent(a: &SackBlock, b: &SackBlock) -> bool {
        // Taşma durumunu ele al
        let a_before_b = a.end.wrapping_sub(b.start) as i32 >= 0;
        let b_before_a = b.end.wrapping_sub(a.start) as i32 >= 0;

        // Bitişik: a.end == b.start veya b.end == a.start
        let adjacent = a.end.wrapping_sub(b.start) == 0 || b.end.wrapping_sub(a.start) == 0;

        // Örtüşme: aralıklar kesişiyor
        let overlap =
            (a.start <= b.start && a.end > b.start) || (b.start <= a.start && b.end > a.start);

        overlap || adjacent
    }

    /// Verilen sıra numarası aralığının SACK blokları tarafından kapsanıp
    /// kapsanmadığını kontrol eder. Yeniden iletim kararında kullanılır.
    pub fn is_sacked(&self, start: u32, end: u32) -> bool {
        for block in &self.blocks {
            if block.start <= start && block.end >= end {
                return true;
            }
        }
        false
    }

    /// Alınmamış veri aralıklarını (gaps/boşlukları) döndürür.
    /// Bu boşluklar yeniden iletilmesi gereken segmentleri gösterir.
    ///
    /// Örnek:
    /// SND.UNA=100, SACK=[150..200, 250..300], SND.NXT=350
    /// Boşluklar: [100..150], [200..250], [300..350]
    pub fn get_gaps(&self, snd_una: u32, snd_nxt: u32) -> Vec<SackBlock> {
        let mut gaps = Vec::new();

        if self.blocks.is_empty() {
            // SACK bilgisi yok, tüm pencere boşluk sayılır
            gaps.push(SackBlock::new(snd_una, snd_nxt));
            return gaps;
        }

        // SND.UNA'dan başla
        let mut current = snd_una;

        for block in &self.blocks {
            if current < block.start {
                // Mevcut konum ile bloğun başlangıcı arasında boşluk var
                gaps.push(SackBlock::new(current, block.start));
            }
            current = current.max(block.end);
        }

        // Son bloktan SND.NXT'ye kadar boşluk
        if current < snd_nxt {
            gaps.push(SackBlock::new(current, snd_nxt));
        }

        gaps
    }

    /// Panoyu temizler (bağlantı sıfırlandığında veya yeni bağlantıda)
    pub fn clear(&mut self) {
        self.blocks.clear();
        self.high_sack = 0;
        self.sacked_bytes = 0;
    }

    /// SACK bloklarını TCP seçeneği formatına dönüştürür.
    /// Format: [tür=5][uzunluk=2+8n][blok1_start][blok1_end]...[blokN_start][blokN_end]
    /// Her blok 8 bayt (2x uint32 big-endian), 4 bayta hizalanır.
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::new();

        // Seçenek türü
        data.push(TCPOPT_SACK);
        // Uzunluk: 2 (başlık) + 8 * blok_sayısı
        let len = 2 + 8 * self.blocks.len();
        data.push(len as u8);

        // Her blok: 4 bayt başlangıç + 4 bayt bitiş (big-endian)
        for block in &self.blocks {
            data.extend_from_slice(&block.start.to_be_bytes());
            data.extend_from_slice(&block.end.to_be_bytes());
        }

        // 4 baytlık sınıra hizala (NOP ile doldur)
        while data.len() % 4 != 0 {
            data.push(TCPOPT_NOP);
        }

        data
    }

    /// TCP seçeneği verisinden SACK bloklarını ayrıştırır.
    /// Hatalı format durumunda None döner.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 2 || data[0] != TCPOPT_SACK {
            return None;
        }

        let len = data[1] as usize;
        if len < 2 || (len - 2) % 8 != 0 {
            return None;
        }

        let num_blocks = (len - 2) / 8;
        let mut scoreboard = SackScoreboard::new();

        for i in 0..num_blocks {
            let offset = 2 + i * 8;
            if offset + 8 > data.len() {
                break;
            }

            let start = u32::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]);
            let end = u32::from_be_bytes([
                data[offset + 4],
                data[offset + 5],
                data[offset + 6],
                data[offset + 7],
            ]);

            scoreboard.add_block(SackBlock::new(start, end));
        }

        Some(scoreboard)
    }
}

/// SYN paketleri için SACK izin seçeneği.
/// Bağlantı kurulumunda her iki taraf da bu seçeneği göndererek
/// SACK kullanmak istediğini bildirir.
#[derive(Clone, Copy, Debug)]
pub struct SackPermitted;

impl SackPermitted {
    pub fn serialize() -> [u8; 2] {
        [TCPOPT_SACK_PERMITTED, 2]
    }
}

// ============================================================================
// TCP HIZLI YENİDEN İLETİM (FAST RETRANSMIT)
// ============================================================================
// Paket kaybı genellikle iki yolla tespit edilir:
// 1. RTO (Retransmission Timeout) zaman aşımı - yavaş ama güvenilir
// 2. Tekrarlanan ACK (Duplicate ACK) - daha hızlı
//
// Hızlı Yeniden İletim Algoritması:
// - Alıcı, sıra dışı segment alınca ACK gönderir ama mevcut beklenen
//   sıra numarasını tekrarlar (Duplicate ACK)
// - Gönderen 3 aynı duplicate ACK alırsa paketi kayıp sayar
//   ve RTO dolmadan yeniden iletir
//
// ```
// Gönderici          Alıcı
//   |-- [1] -------->|
//   |-- [2] -------->|  [3] KAYIP
//   |-- [4] -------->|   -> ack=3 (dup ACK 1)
//   |-- [5] -------->|   -> ack=3 (dup ACK 2)
//   |-- [6] -------->|   -> ack=3 (dup ACK 3) --> Hızlı iletim tetiklenir!
//   |-- [3] -------->|   Yeniden gönder
//               ack=7 (tüm alındı)
// ```

/// Hızlı yeniden iletim durumu.
/// Duplicate ACK sayacı ve kurtarma noktası burada tutulur.
#[derive(Clone, Debug)]
pub struct FastRetransmitState {
    /// Duplicate ACK sayacı (3'e ulaşınca hızlı iletim)
    pub dup_ack_count: u32,
    /// Son alınan ACK numarası
    pub last_ack: u32,
    /// Son hızlı iletim sırasındaki sıra numarası (kurtarma noktası)
    pub recover: u32,
    /// Şu anda hızlı kurtarma (fast recovery) aşamasında mı?
    pub in_recovery: bool,
    /// Hızlı iletim eşiği (RFC 5681: varsayılan 3)
    pub threshold: u32,
}

impl Default for FastRetransmitState {
    fn default() -> Self {
        Self::new()
    }
}

impl FastRetransmitState {
    pub fn new() -> Self {
        FastRetransmitState {
            dup_ack_count: 0,
            last_ack: 0,
            recover: 0,
            in_recovery: false,
            threshold: 3,
        }
    }

    /// Gelen ACK'i işler. Hızlı yeniden iletim gerekiyorsa true döner.
    /// Üç duplicate ACK alındığında hızlı iletim tetiklenir.
    pub fn on_ack(&mut self, ack: u32, sack_blocks: &[SackBlock], snd_una: u32) -> bool {
        if ack == self.last_ack && ack != snd_una {
            // Yinelenen ACK (Duplicate ACK)
            // Yeni SACK bilgisi taşıyor mu kontrol et
            let has_new_sack = !sack_blocks.is_empty();

            self.dup_ack_count += 1;

            // Eşiğe ulaşıldı ve henüz kurtarma modunda değilsek hızlı iletim
            if self.dup_ack_count >= self.threshold && !self.in_recovery {
                self.in_recovery = true;
                self.recover = snd_una;
                return true;
            }
        } else if ack > self.last_ack {
            // Yeni ACK - duplicate sayacını sıfırla
            self.dup_ack_count = 0;
            self.last_ack = ack;

            // Kurtarma noktasına ulaşıldıysa hızlı kurtarmadan çık
            if self.in_recovery && ack >= self.recover {
                self.in_recovery = false;
            }
        }

        false
    }

    /// Durumu sıfırla (bağlantı kurulumu veya zaman aşımı sonrası)
    pub fn reset(&mut self) {
        self.dup_ack_count = 0;
        self.in_recovery = false;
    }
}

// ============================================================================
// TCP FAST OPEN (TFO) - Hızlı Açılış
// ============================================================================
// TFO, TCP el sıkışması ile birlikte veri göndererek gecikmeyi azaltır.
// Normal TCP'de ilk veri ACK sonrası gönderilebilir (1 RTT gecikme).
// TFO ile SYN paketine veri eklenerek 0 ekstra RTT sağlanır.
//
// TFO Akışı:
// ```
// İlk bağlantı (cookie alınır):
//   İstemci -> SYN + TFO-Cookie-Request -> Sunucu
//   İstemci <- SYN-ACK + TFO-Cookie    <- Sunucu
//   İstemci -> ACK                      -> Sunucu
//
// Sonraki bağlantılar (0-RTT veri):
//   İstemci -> SYN + TFO-Cookie + VERİ -> Sunucu
//   Sunucu hemen veriyi işler (ACK beklenmez)
//   İstemci <- SYN-ACK + yanıt          <- Sunucu
// ```

/// TFO çerezi (cookie) - 8 bayt rastgele değer.
/// Sunucu tarafından oluşturulur, istemci sonraki bağlantılarda kullanır.
#[derive(Clone, Copy, Debug, Default)]
pub struct TfoCookie(pub [u8; 8]);

type HmacSha256 = Hmac<Sha256>;

static TCP_TIMESTAMP_COUNTER: AtomicU32 = AtomicU32::new(0);
static TCP_TFO_COOKIE_EPOCH: AtomicU32 = AtomicU32::new(0x5a17_c0de);

impl TfoCookie {
    pub fn new() -> Self {
        let mut cookie = [0u8; 8];
        for i in 0..8 {
            cookie[i] = crate::random::next_u32() as u8;
        }
        TfoCookie(cookie)
    }

    pub fn generate(server_ip: Ipv4Addr, time_ms: u64) -> Self {
        let mut mac = HmacSha256::new_from_slice(&tfo_secret(server_ip))
            .expect("TCP TFO HMAC key length is fixed");
        mac.update(&server_ip.0);
        mac.update(&time_ms.to_be_bytes());
        let digest = mac.finalize().into_bytes();
        let mut cookie = [0u8; 8];
        cookie.copy_from_slice(&digest[..8]);
        TfoCookie(cookie)
    }

    pub fn verify(&self, server_ip: Ipv4Addr, time_window: u64) -> bool {
        let expected = TfoCookie::generate(server_ip, time_window);
        self.0 == expected.0
    }
}

fn tfo_secret(server_ip: Ipv4Addr) -> [u8; 16] {
    let epoch = TCP_TFO_COOKIE_EPOCH.load(Ordering::Relaxed);
    let mut secret = [0u8; 16];
    secret[..4].copy_from_slice(&server_ip.0);
    secret[4..8].copy_from_slice(&epoch.to_be_bytes());
    secret[8..12].copy_from_slice(&(epoch.rotate_left(7) ^ 0xa5a5_5a5a).to_be_bytes());
    secret[12..16].copy_from_slice(&(epoch.wrapping_mul(0x9e37_79b9)).to_be_bytes());
    secret
}

fn next_tcp_timestamp() -> u32 {
    TCP_TIMESTAMP_COUNTER
        .fetch_add(1000, Ordering::Relaxed)
        .wrapping_add(1000)
}

/// TFO bağlantı durumu - çerezler ve bekleyen SYN verisi
#[derive(Clone, Debug)]
pub struct TfoState {
    /// Sunucu IP'sine göre çerezler (her sunucu için ayrı çerez)
    pub cookies: BTreeMap<u32, TfoCookie>,
    /// SYN paketinde gönderilecek bekleyen veri
    pub pending_data: Vec<u8>,
    /// TFO etkin mi?
    pub enabled: bool,
    /// TFO çerez isteği gönderildi mi?
    pub cookie_requested: bool,
    /// SYN'de veri gönderildi mi?
    pub data_in_syn: bool,
}

impl Default for TfoState {
    fn default() -> Self {
        Self::new()
    }
}

impl TfoState {
    pub fn new() -> Self {
        TfoState {
            cookies: BTreeMap::new(),
            pending_data: Vec::new(),
            enabled: true,
            cookie_requested: false,
            data_in_syn: false,
        }
    }

    /// Belirli sunucu için kayıtlı çerezi al
    pub fn get_cookie(&self, server_ip: Ipv4Addr) -> Option<TfoCookie> {
        let ip_key = u32::from_be_bytes(server_ip.0);
        self.cookies.get(&ip_key).copied()
    }

    /// Sunucudan gelen çerezi kaydet (sonraki bağlantılarda kullanmak için)
    pub fn store_cookie(&mut self, server_ip: Ipv4Addr, cookie: TfoCookie) {
        let ip_key = u32::from_be_bytes(server_ip.0);
        self.cookies.insert(ip_key, cookie);
    }

    /// TFO çerezini TCP seçeneği olarak serileştir
    pub fn serialize_cookie_option(cookie: &TfoCookie) -> Vec<u8> {
        let mut data = Vec::new();
        data.push(TCPOPT_FAST_OPEN); // 34
        data.push(10); // Uzunluk: 2 (başlık) + 8 (çerez)
        data.extend_from_slice(&cookie.0);
        data
    }
}

/// TFO seçenek türü (RFC 7413)
pub const TCPOPT_FAST_OPEN: u8 = 34;

// ============================================================================
// TCP PENCERE ÖLÇEKLEMESİ (WINDOW SCALING)
// ============================================================================
// TCP başlığındaki pencere alanı 16 bit olduğundan maks. 65535 bayt pencere
// boyutuna izin verir. Yüksek bant genişlikli ağlarda bu yetersizdir!
//
// BDP (Bandwidth-Delay Product) = bant_genişliği × RTT
// 1 Gbps, 100ms RTT => BDP = 12.5 MB -- 64KB'dan çok daha büyük!
//
// Çözüm: Window Scale seçeneği (RFC 7323)
// Gerçek pencere = başlık_penceresi << ölçek_faktörü
// Ölçek 0-14 arasında olabilir => maks. pencere ~1 GB

/// Pencere ölçekleme seçeneği - yüksek bant genişliğinde gerekli
#[derive(Clone, Copy, Debug)]
pub struct WindowScaleOption {
    pub scale: u8,
}

impl WindowScaleOption {
    pub fn new(scale: u8) -> Self {
        WindowScaleOption {
            scale: scale.min(14),
        } // RFC 7323: maks. 14
    }

    /// Pencere ölçekleme seçeneğini serileştir: [tür][uzunluk=3][ölçek]
    pub fn serialize(&self) -> [u8; 3] {
        [TCPOPT_WINDOW_SCALE, 3, self.scale]
    }

    /// Seçenek verisinden ayrıştır
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 3 || data[0] != TCPOPT_WINDOW_SCALE {
            return None;
        }
        Some(WindowScaleOption { scale: data[2] })
    }

    /// Gerçek pencere boyutunu hesapla: temel_pencere << ölçek
    pub fn effective_window(base: u16, scale: u8) -> u32 {
        (base as u32) << (scale as u32)
    }
}

// ============================================================================
// TCP ZAMAN DAMGALARI (TIMESTAMPS)
// ============================================================================
// Zaman damgaları iki amaç için kullanılır:
// 1. RTT Ölçümü: ts_val gönderilir, ts_ecr ile geri alınır
//    RTT = şimdiki_zaman - ts_ecr
// 2. PAWS (Protection Against Wrapped Sequences): Sıra numarası taşmasına
//    karşı koruma sağlar (RFC 7323)
//
// Format: [tür=8][uzunluk=10][ts_val(4B)][ts_ecr(4B)]

/// TCP Zaman Damgası seçeneği - RTT ölçümü ve PAWS koruması
#[derive(Clone, Copy, Debug, Default)]
pub struct TimestampOption {
    pub ts_val: u32, // Gönderenin zaman damgası değeri
    pub ts_ecr: u32, // Geri yankılanan zaman damgası (echo reply)
}

impl TimestampOption {
    pub fn new(ts_val: u32, ts_ecr: u32) -> Self {
        TimestampOption { ts_val, ts_ecr }
    }

    /// Şimdiki zamanla oluştur (ts_ecr karşı taraftan gelen değer)
    pub fn now(ts_ecr: u32) -> Self {
        let ts_val = next_tcp_timestamp();
        TimestampOption { ts_val, ts_ecr }
    }

    /// Zaman damgası seçeneğini serileştir: [8][10][ts_val][ts_ecr]
    pub fn serialize(&self) -> [u8; 10] {
        let mut data = [0u8; 10];
        data[0] = TCPOPT_TIMESTAMP;
        data[1] = 10; // Uzunluk
        data[2..6].copy_from_slice(&self.ts_val.to_be_bytes());
        data[6..10].copy_from_slice(&self.ts_ecr.to_be_bytes());
        data
    }

    /// Seçenek verisinden ayrıştır
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 10 || data[0] != TCPOPT_TIMESTAMP {
            return None;
        }
        let ts_val = u32::from_be_bytes([data[2], data[3], data[4], data[5]]);
        let ts_ecr = u32::from_be_bytes([data[6], data[7], data[8], data[9]]);
        Some(TimestampOption { ts_val, ts_ecr })
    }

    /// Zaman damgasından RTT hesapla: şimdiki_zaman - ts_ecr
    pub fn calculate_rtt(&self) -> u32 {
        let now = TCP_TIMESTAMP_COUNTER.load(Ordering::Relaxed);
        now.wrapping_sub(self.ts_ecr)
    }
}

// ============================================================================
// TCP SEÇENEKLERİ AYRIŞTIRICISI
// ============================================================================
// TCP başlığı 20 bayt sabittir (veri_offset=5).
// Seçenekler 20-60 bayt arasında yer alır (maks. 40 bayt).
// Seçenekler EOL ile sonlanır veya başlık sonuna kadar devam eder.

/// Ayrıştırılmış TCP seçenekleri - tüm seçenekler burada toplanır
#[derive(Clone, Debug, Default)]
pub struct TcpOptions {
    pub mss: Option<u16>,                        // Maksimum Segment Boyutu
    pub window_scale: Option<WindowScaleOption>, // Pencere ölçekleme faktörü
    pub sack_permitted: bool,                    // SACK kullanımına izin var mı
    pub sack_blocks: Vec<SackBlock>,             // Alınan SACK blokları
    pub timestamps: Option<TimestampOption>,     // Zaman damgaları
    pub tfo_cookie: Option<TfoCookie>,           // TFO çerezi
}

impl TcpOptions {
    /// TCP başlığından seçenekleri ayrıştır.
    /// Seçenekler header_data[20..header_len] aralığında yer alır.
    pub fn parse(header_data: &[u8], header_len: usize) -> Self {
        let mut options = TcpOptions::default();

        if header_len <= 20 {
            return options;
        }

        let opts_data = &header_data[20..header_len];
        let mut i = 0;

        while i < opts_data.len() {
            let opt_kind = opts_data[i];

            match opt_kind {
                TCPOPT_EOL => break, // Seçenekler listesi sonu
                TCPOPT_NOP => {
                    i += 1; // Hizalama dolgusu, atla
                    continue;
                }
                _ => {
                    if i + 1 >= opts_data.len() {
                        break;
                    }
                    let opt_len = opts_data[i + 1] as usize;
                    if opt_len < 2 || i + opt_len > opts_data.len() {
                        break;
                    }

                    let opt_data = &opts_data[i..i + opt_len];

                    match opt_kind {
                        TCPOPT_MSS => {
                            if opt_len >= 4 {
                                options.mss = Some(u16::from_be_bytes([opt_data[2], opt_data[3]]));
                            }
                        }
                        TCPOPT_WINDOW_SCALE => {
                            if let Some(ws) = WindowScaleOption::parse(opt_data) {
                                options.window_scale = Some(ws);
                            }
                        }
                        TCPOPT_SACK_PERMITTED => {
                            options.sack_permitted = true;
                        }
                        TCPOPT_SACK => {
                            if let Some(scoreboard) = SackScoreboard::parse(opt_data) {
                                options.sack_blocks = scoreboard.blocks;
                            }
                        }
                        TCPOPT_TIMESTAMP => {
                            options.timestamps = TimestampOption::parse(opt_data);
                        }
                        TCPOPT_FAST_OPEN => {
                            if opt_len >= 10 {
                                let mut cookie = [0u8; 8];
                                cookie.copy_from_slice(&opt_data[2..10]);
                                options.tfo_cookie = Some(TfoCookie(cookie));
                            }
                        }
                        _ => {}
                    }

                    i += opt_len;
                }
            }
        }

        options
    }

    /// SYN paketi için seçenekler oluştur.
    /// SYN'de MSS, pencere ölçekleme, SACK izni ve zaman damgaları müzakere edilir.
    pub fn build_syn_options(
        mss: u16,
        ws_scale: u8,
        enable_sack: bool,
        enable_tfo: bool,
    ) -> Vec<u8> {
        let mut opts = Vec::new();

        // MSS seçeneği (Maksimum Segment Boyutu bildirimi)
        opts.push(TCPOPT_MSS);
        opts.push(4);
        opts.extend_from_slice(&mss.to_be_bytes());

        // Pencere ölçekleme
        opts.extend_from_slice(&WindowScaleOption::new(ws_scale).serialize());

        // SACK izni
        if enable_sack {
            opts.extend_from_slice(&SackPermitted::serialize());
        }

        // Zaman damgaları
        let ts = TimestampOption::now(0);
        opts.extend_from_slice(&ts.serialize());

        // 4 bayta hizala
        while opts.len() % 4 != 0 {
            opts.push(TCPOPT_NOP);
        }

        opts
    }

    /// Veri paketi için seçenekler oluştur.
    /// Veri paketlerinde zaman damgası ve SACK bilgisi yer alır.
    pub fn build_data_options(ts_echo: u32, sack_blocks: &[SackBlock]) -> Vec<u8> {
        let mut opts = Vec::new();

        // Zaman damgaları (RTT ölçümü için ts_echo karşı tarafın ts_val'ı)
        let ts = TimestampOption::now(ts_echo);
        opts.extend_from_slice(&ts.serialize());

        // Varsa SACK blokları (alıcı hangi segmentleri aldığını bildirir)
        if !sack_blocks.is_empty() {
            let scoreboard = SackScoreboard {
                blocks: sack_blocks.to_vec(),
                max_blocks: 4,
                high_sack: 0,
                sacked_bytes: 0,
            };
            opts.extend_from_slice(&scoreboard.serialize());
        }

        // 4 bayta hizala
        while opts.len() % 4 != 0 {
            opts.push(TCPOPT_NOP);
        }

        opts
    }
}

/// TCP başlığı - minimum 20 bayt sabit alan.
/// data_offset alanı başlık uzunluğunu 32-bit kelimeler cinsinden gösterir.
///
/// ```
/// 0                   1                   2                   3
/// 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |          Kaynak Port          |         Hedef Port            |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                        Sıra Numarası                         |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                     Onaylama Numarası                        |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |  Veri | Rsv |U|A|P|R|S|F|         Pencere Boyutu            |
/// | Ofset |     |R|C|S|S|Y|I|                                    |
/// |       |     |G|K|H|T|N|N|                                    |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |           Sağlama Toplamı     |         Acil İşaretçi        |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// |                    Seçenekler (değişken)                      |
/// +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
/// ```
#[derive(Clone, Copy, Debug)]
pub struct TcpHeader {
    pub src_port: Port,
    pub dst_port: Port,
    pub seq_num: u32,
    pub ack_num: u32,
    pub data_offset: u8, // 4 bit, başlık uzunluğu 32-bit kelimeler cinsinden
    pub flags: TcpFlags,
    pub window_size: u16,
    pub checksum: u16,
    pub urgent_ptr: u16,
}

/// TCP bayrakları - her biri 1 bit
#[derive(Clone, Copy, Debug, Default)]
pub struct TcpFlags {
    pub fin: bool, // FIN: Bağlantıyı sonlandır
    pub syn: bool, // SYN: Sıra numarasını senkronize et (bağlantı kuruluşu)
    pub rst: bool, // RST: Bağlantıyı sıfırla (hata durumunda)
    pub psh: bool, // PSH: Veriyi hemen üst katmana ilet (tampon bekletme)
    pub ack: bool, // ACK: Onaylama numarası geçerli
    pub urg: bool, // URG: Acil veri göstericisi geçerli
}

impl TcpFlags {
    pub fn new() -> Self {
        TcpFlags::default()
    }

    pub fn syn() -> Self {
        TcpFlags {
            syn: true,
            ..Default::default()
        }
    }

    pub fn syn_ack() -> Self {
        TcpFlags {
            syn: true,
            ack: true,
            ..Default::default()
        }
    }

    pub fn ack() -> Self {
        TcpFlags {
            ack: true,
            ..Default::default()
        }
    }

    pub fn fin() -> Self {
        TcpFlags {
            fin: true,
            ..Default::default()
        }
    }

    pub fn fin_ack() -> Self {
        TcpFlags {
            fin: true,
            ack: true,
            ..Default::default()
        }
    }

    pub fn rst() -> Self {
        TcpFlags {
            rst: true,
            ..Default::default()
        }
    }

    pub fn to_u8(self) -> u8 {
        let mut val = 0u8;
        if self.fin {
            val |= 0x01;
        }
        if self.syn {
            val |= 0x02;
        }
        if self.rst {
            val |= 0x04;
        }
        if self.psh {
            val |= 0x08;
        }
        if self.ack {
            val |= 0x10;
        }
        if self.urg {
            val |= 0x20;
        }
        val
    }

    pub fn from_u8(val: u8) -> Self {
        TcpFlags {
            fin: val & 0x01 != 0,
            syn: val & 0x02 != 0,
            rst: val & 0x04 != 0,
            psh: val & 0x08 != 0,
            ack: val & 0x10 != 0,
            urg: val & 0x20 != 0,
        }
    }
}

impl TcpHeader {
    pub const MIN_SIZE: usize = 20;

    /// TCP başlığını ham baytlardan ayrıştır.
    /// data_offset * 4 = başlık toplam uzunluğu (bayt)
    pub fn parse(data: &[u8]) -> Result<Self, NetError> {
        if data.len() < Self::MIN_SIZE {
            return Err(NetError::InvalidPacket);
        }

        let src_port = Port(u16::from_be_bytes([data[0], data[1]]));
        let dst_port = Port(u16::from_be_bytes([data[2], data[3]]));
        let seq_num = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let ack_num = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
        let data_offset = (data[12] >> 4) & 0x0F;
        if data_offset < 5 {
            return Err(NetError::InvalidPacket);
        }
        let Some(header_len) = (data_offset as usize).checked_mul(4) else {
            return Err(NetError::InvalidPacket);
        };
        if header_len > data.len() {
            return Err(NetError::InvalidPacket);
        }
        let flags = TcpFlags::from_u8(data[13]);
        let window_size = u16::from_be_bytes([data[14], data[15]]);
        let checksum = u16::from_be_bytes([data[16], data[17]]);
        let urgent_ptr = u16::from_be_bytes([data[18], data[19]]);

        Ok(TcpHeader {
            src_port,
            dst_port,
            seq_num,
            ack_num,
            data_offset,
            flags,
            window_size,
            checksum,
            urgent_ptr,
        })
    }

    /// Başlığı bayt dizisine yaz (big-endian ağ byte düzeni)
    pub fn serialize(&self, buf: &mut [u8]) -> Result<(), NetError> {
        if buf.len() < Self::MIN_SIZE {
            return Err(NetError::BufferFull);
        }

        buf[0..2].copy_from_slice(&self.src_port.0.to_be_bytes());
        buf[2..4].copy_from_slice(&self.dst_port.0.to_be_bytes());
        buf[4..8].copy_from_slice(&self.seq_num.to_be_bytes());
        buf[8..12].copy_from_slice(&self.ack_num.to_be_bytes());
        buf[12] = (self.data_offset << 4) | 0x00; // Reserved bits
        buf[13] = self.flags.to_u8();
        buf[14..16].copy_from_slice(&self.window_size.to_be_bytes());
        buf[16..18].copy_from_slice(&self.checksum.to_be_bytes());
        buf[18..20].copy_from_slice(&self.urgent_ptr.to_be_bytes());

        Ok(())
    }

    /// Başlık uzunluğunu bayt cinsinden döndür (data_offset * 4)
    pub fn header_len(&self) -> usize {
        (self.data_offset as usize).saturating_mul(4)
    }
}

/// TCP sağlama toplamı hesapla.
/// TCP sağlama toplamı, sahte başlık (pseudo-header) + TCP segmentinin
/// 16-bit tek tümleyen toplamıdır.
///
/// Sahte Başlık (Pseudo-Header) Formatı:
/// ```
/// [Kaynak IP (4B)][Hedef IP (4B)][Sıfır (1B)][Protokol=6 (1B)][TCP Uzunluğu (2B)]
/// ```
/// Bu yapı, IP katmanı bilgilerini sağlama toplamına dahil ederek
/// IP spoofing'e karşı ek koruma sağlar.
pub fn compute_checksum(src_ip: Ipv4Addr, dst_ip: Ipv4Addr, segment: &[u8]) -> u16 {
    let mut sum: u32 = 0;

    // Sahte başlık (pseudo-header) katkısı
    sum += u16::from_be_bytes([src_ip.0[0], src_ip.0[1]]) as u32;
    sum += u16::from_be_bytes([src_ip.0[2], src_ip.0[3]]) as u32;
    sum += u16::from_be_bytes([dst_ip.0[0], dst_ip.0[1]]) as u32;
    sum += u16::from_be_bytes([dst_ip.0[2], dst_ip.0[3]]) as u32;
    sum += 6u32; // TCP protokol numarası
    sum += segment.len() as u32;

    // TCP segmenti baytları (2'şer 2'şer topla)
    let mut i = 0;
    while i + 1 < segment.len() {
        sum += u16::from_be_bytes([segment[i], segment[i + 1]]) as u32;
        i += 2;
    }

    // Tek kalan bayt (üst bayt olarak eklenir)
    if i < segment.len() {
        sum += (segment[i] as u32) << 8;
    }

    // Taşmaları katla (32-bit -> 16-bit)
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }

    // Tek tümleyen (one's complement)
    !(sum as u16)
}

/// TCP sağlama toplamını doğrula.
/// Sağlama toplamı doğruysa sonuç 0 olmalıdır
/// (compute_checksum'a sağlama toplamı dahil segment verilirse).
pub fn verify_checksum(src_ip: Ipv4Addr, dst_ip: Ipv4Addr, segment: &[u8]) -> bool {
    compute_checksum(src_ip, dst_ip, segment) == 0
}

pub fn compute_checksum_v6(
    src_ip: super::ipv6::Ipv6Addr,
    dst_ip: super::ipv6::Ipv6Addr,
    segment: &[u8],
) -> u16 {
    let mut sum: u32 = 0;

    for chunk in src_ip.0.chunks(2) {
        sum += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
    }
    for chunk in dst_ip.0.chunks(2) {
        sum += u16::from_be_bytes([chunk[0], chunk[1]]) as u32;
    }

    let len = segment.len() as u32;
    sum += (len >> 16) & 0xFFFF;
    sum += len & 0xFFFF;
    sum += Ipv6NextHeader::Tcp as u32;

    let mut i = 0;
    while i + 1 < segment.len() {
        sum += u16::from_be_bytes([segment[i], segment[i + 1]]) as u32;
        i += 2;
    }
    if i < segment.len() {
        sum += (segment[i] as u32) << 8;
    }

    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }

    !(sum as u16)
}

pub fn verify_checksum_v6(
    src_ip: super::ipv6::Ipv6Addr,
    dst_ip: super::ipv6::Ipv6Addr,
    segment: &[u8],
) -> bool {
    compute_checksum_v6(src_ip, dst_ip, segment) == 0
}

/// TCP bağlantı durumu makinesi.
/// Her durum, hangi paketlerin kabul edileceğini ve
/// hangi geçişlerin yapılabileceğini belirler.
///
/// ```
/// CLOSED -> LISTEN (listen çağrısı)
/// CLOSED -> SYN_SENT (connect çağrısı, SYN gönderildi)
/// LISTEN -> SYN_RECEIVED (SYN alındı, SYN-ACK gönderildi)
/// SYN_SENT -> ESTABLISHED (SYN-ACK alındı, ACK gönderildi)
/// SYN_RECEIVED -> ESTABLISHED (ACK alındı)
/// ESTABLISHED -> FIN_WAIT_1 (close çağrısı, FIN gönderildi)
/// ESTABLISHED -> CLOSE_WAIT (FIN alındı, ACK gönderildi)
/// FIN_WAIT_1 -> FIN_WAIT_2 (ACK alındı)
/// FIN_WAIT_2 -> TIME_WAIT (FIN alındı, ACK gönderildi)
/// CLOSE_WAIT -> LAST_ACK (close çağrısı, FIN gönderildi)
/// LAST_ACK -> CLOSED (ACK alındı)
/// TIME_WAIT -> CLOSED (2*MSL süre geçti)
/// ```

/// TIME_WAIT süresi: 2×MSL = 2×60 saniye (tick cinsinden)
const TIME_WAIT_DURATION: u64 = 120;
/// Nagle gecikme süresi (tick cinsinden, ~200ms — RFC 896).
/// Tick = 10ms kabul edilerek 200/10 = 20 hesaplandı.
const NAGLE_DELAY_TICKS: u64 = 20;
/// SYN yeniden iletim zaman aşımı (tick cinsinden, ~1 saniye = 100 tick).
const SYN_TIMEOUT_TICKS: u64 = 100;
/// SYN max yeniden iletim sayısı (RFC 6298 varsayılan: 3).
const SYN_MAX_RETRANSMIT: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TcpState {
    Closed,      // Bağlantı yok
    Listen,      // Bağlantı bekliyor
    SynSent,     // SYN gönderildi, SYN-ACK bekleniyor
    SynReceived, // SYN alındı, SYN-ACK gönderildi
    Established, // Bağlantı kuruldu, veri transferi
    FinWait1,    // FIN gönderildi, onay bekleniyor
    FinWait2,    // FIN onaylandı, karşı tarafın FIN'i bekleniyor
    CloseWait,   // Karşı taraf kapattı, uygulama kapanmayı bekliyor
    Closing,     // Her iki taraf eş zamanlı kapanıyor
    LastAck,     // Son ACK bekleniyor
    TimeWait,    // 2*MSL süre bekleniyor (gecikmiş segmentlere karşı)
}

/// TCP bağlantısı - tüm durum bilgilerini tutar.
/// Her bağlantı benzersiz bir ID'ye sahiptir ve bağlantı tablosunda saklanır.
#[derive(Clone, Debug)]
pub struct TcpConnection {
    pub id: u32,
    pub family: AddressFamily,
    pub local: SocketAddr,
    pub remote: SocketAddr,
    pub state: TcpState,
    pub seq_num: u32,
    pub ack_num: u32,
    pub window_size: u16,
    pub rx_buffer: Vec<u8>,
    pub tx_buffer: Vec<u8>,
    pub listen_backlog: usize,
    // Tıkanıklık kontrolü değişkenleri
    pub cwnd: u32,            // Tıkanıklık penceresi (Congestion Window)
    pub ssthresh: u32,        // Yavaş başlangıç eşiği (Slow Start Threshold)
    pub rtt: u32,             // Tahmin edilen gidiş-dönüş süresi (ms)
    pub rtt_var: u32,         // RTT varyansı (Jacobson algoritması)
    pub rto: u32,             // Yeniden iletim zaman aşımı = RTT + 4*RTTVAR (ms)
    pub retransmit_count: u8, // Yeniden iletim sayacı
    // Pencere ölçekleme
    pub ws_scale: u8,      // Bizim pencere ölçekleme faktörümüz
    pub peer_ws_scale: u8, // Karşı tarafın pencere ölçekleme faktörü
    // SACK desteği
    pub sack_permitted: bool,            // SACK müzakere edildi mi?
    pub sack_scoreboard: SackScoreboard, // Alınan SACK blokları panosu
    pub rx_sack_blocks: Vec<SackBlock>,  // Göndereceğimiz SACK blokları
    // Hızlı yeniden iletim
    pub fast_retx: FastRetransmitState,
    // Gönderme durumu (RFC 793 değişkenleri)
    pub snd_una: u32, // En eski onaylanmamış sıra numarası
    pub snd_nxt: u32, // Gönderilecek bir sonraki sıra numarası
    pub snd_wnd: u32, // Gönderme penceresi (karşı tarafın alım kapasitesi)
    // Zaman damgaları
    pub ts_recent: u32, // Karşı tarafın son zaman damgası
    pub ts_echo: u32,   // Geri yankılayacağımız zaman damgası
    pub ts_val: u32,    // Bizim zaman damgası değerimiz
    // Tıkanıklık kontrol durumu (CcState dispatch)
    pub cc: CcState,
    // TIME_WAIT zamanlayıcısı (tick cinsinden)
    pub time_wait_start: u64,
    // Yeniden iletim zamanlayıcısı (tick cinsinden)
    pub last_send_time: u64,
    // TCP Keepalive (RFC 1122 Section 4.2.3.6)
    pub keepalive_enabled: bool,
    pub keepalive_idle: u32,   // tcp_keepalive_time (varsayılan 7200s)
    pub keepalive_intvl: u32,  // tcp_keepalive_intvl (varsayılan 75s)
    pub keepalive_probes: u32, // tcp_keepalive_probes (varsayılan 9)
    pub keepalive_last_ping: u64,
    pub keepalive_probe_count: u32,
    // Nagle algoritması (RFC 896)
    pub nagle_enabled: bool,   // TCP_NODELAY=false ise Nagle aktif
    pub nagle_buffer: Vec<u8>, // Nagle bekleyen veri
    pub nagle_last_send: u64,  // Son gönderim zamanı
    // MSS (TCP Maximum Segment Size)
    pub mss: u16,              // Negotiated MSS (varsayılan 536 RFC 2069)
    // SYN yeniden iletim
    pub syn_retransmit_count: u8,
    pub syn_first_send_time: u64,
}

impl TcpConnection {
    pub fn new(local: SocketAddr, family: AddressFamily) -> Self {
        TcpConnection {
            id: allocate_socket_id(),
            family,
            local,
            remote: SocketAddr::default(),
            state: TcpState::Closed,
            seq_num: 0,
            ack_num: 0,
            window_size: 65535,
            rx_buffer: Vec::new(),
            tx_buffer: Vec::new(),
            listen_backlog: 0,
            // Tıkanıklık kontrolü başlangıç değerleri
            cwnd: 10 * 1460, // Başlangıç penceresi (10 MSS - RFC 6928)
            ssthresh: 65535, // Yüksek başlangıç eşiği
            rtt: 100,        // Başlangıç RTT tahmini (100ms)
            rtt_var: 50,     // Başlangıç RTT varyansı
            rto: 200,        // Başlangıç RTO (200ms)
            retransmit_count: 0,
            ws_scale: 0,
            peer_ws_scale: 0,
            // SACK
            sack_permitted: false,
            sack_scoreboard: SackScoreboard::new(),
            rx_sack_blocks: Vec::new(),
            // Hızlı yeniden iletim
            fast_retx: FastRetransmitState::new(),
            // Gönderme durumu
            snd_una: 0,
            snd_nxt: 0,
            snd_wnd: 65535,
            // Zaman damgaları
            ts_recent: 0,
            ts_echo: 0,
            ts_val: 0,
            // Tıkanıklık kontrol durumu
            cc: CcState::new(CcAlgorithm::Cubic),
            // TIME_WAIT zamanlayıcısı
            time_wait_start: 0,
            // Yeniden iletim zamanlayıcısı
            last_send_time: 0,
            // TCP Keepalive
            keepalive_enabled: false,
            keepalive_idle: 7200,   // 2 saat (saniye)
            keepalive_intvl: 75,    // 75 saniye
            keepalive_probes: 9,    // 9 başarısız ping
            keepalive_last_ping: 0,
            keepalive_probe_count: 0,
            // Nagle algoritması
            nagle_enabled: true,    // Varsayılan: Nagle aktif (TCP_NODELAY=false)
            nagle_buffer: Vec::new(),
            nagle_last_send: 0,
            // MSS (varsayılan RFC 2069 minimum MSS)
            mss: 536,
            // SYN yeniden iletim
            syn_retransmit_count: 0,
            syn_first_send_time: 0,
        }
    }

    pub fn connect(&mut self, remote: SocketAddr) -> Result<(), NetError> {
        if self.state != TcpState::Closed {
            return Err(NetError::ProtocolError);
        }

        self.remote = remote;
        self.seq_num = crate::random::rand_u64() as u32;
        self.state = TcpState::SynSent;

        // SYN yeniden iletim zamanlayıcısını başlat
        self.syn_first_send_time = crate::task::scheduler::get_ticks() as u64;
        self.syn_retransmit_count = 0;

        // SYN gönder - üç yönlü el sıkışmanın ilk adımı
        self.send_packet(TcpFlags::syn(), &[])?;

        Ok(())
    }

    pub fn listen(&mut self, backlog: usize) -> Result<(), NetError> {
        if self.state != TcpState::Closed {
            return Err(NetError::ProtocolError);
        }

        self.state = TcpState::Listen;
        self.listen_backlog = backlog;

        Ok(())
    }

    pub fn accept(&mut self) -> Result<SocketAddr, NetError> {
        if self.state != TcpState::Listen {
            return Err(NetError::ProtocolError);
        }

        // Kabul kuyruğundan bağlantı al
        let mut queue = ACCEPT_QUEUE.lock();
        if let Some(child_ids) = queue.get_mut(&self.id) {
            if !child_ids.is_empty() {
                let _child_id = child_ids.remove(0);
                return Ok(self.remote);
            }
        }

        Err(NetError::WouldBlock)
    }

    pub fn send(&mut self, data: &[u8]) -> Result<usize, NetError> {
        if self.state != TcpState::Established {
            return Err(NetError::ConnectionClosed);
        }

        // Nagle algoritması (RFC 896):
        // 1. TCP_NODELAY=true ise Nagle devre dışı → doğrudan gönder
        if !self.nagle_enabled {
            self.send_packet(TcpFlags::ack(), data)?;
            self.seq_num = self.seq_num.wrapping_add(data.len() as u32);
            self.snd_nxt = self.seq_num;
            self.last_send_time = crate::task::scheduler::get_ticks() as u64;
            return Ok(data.len());
        }

        // 2. Henüz onaylanmamış veri varsa (snd_una != snd_nxt) → tampona ekle
        let has_unacked = self.snd_una != self.snd_nxt;
        if has_unacked {
            self.nagle_buffer.extend_from_slice(data);
            return Ok(data.len());
        }

        // 3. Gönderilecek veri MSS'den küçükse ve buffer boşsa → tampona ekle, timer başlat
        let mss_usize = self.mss as usize;
        if data.len() < mss_usize && self.nagle_buffer.is_empty() {
            self.nagle_buffer.extend_from_slice(data);
            self.nagle_last_send = crate::task::scheduler::get_ticks() as u64;
            return Ok(data.len());
        }

        // 4. Veri MSS'ye eşit veya daha büyükse → hemen gönder
        // 5. Tampon doluysa (MSS+) → tamponu + yeni veriyi gönder
        if !self.nagle_buffer.is_empty() {
            let mut combined = core::mem::take(&mut self.nagle_buffer);
            combined.extend_from_slice(data);
            self.send_packet(TcpFlags::ack(), &combined)?;
            self.seq_num = self.seq_num.wrapping_add(combined.len() as u32);
            self.snd_nxt = self.seq_num;
            self.last_send_time = crate::task::scheduler::get_ticks() as u64;
            return Ok(data.len());
        }

        self.send_packet(TcpFlags::ack(), data)?;
        self.seq_num = self.seq_num.wrapping_add(data.len() as u32);
        self.snd_nxt = self.seq_num;
        self.last_send_time = crate::task::scheduler::get_ticks() as u64;

        Ok(data.len())
    }

    /// Nagle zamanlayıcısını kontrol et — süresi dolmuşsa tampondaki veriyi gönder.
    /// Periyodik olarak çağrılmalıdır (TCP timer thread tarafından).
    pub fn flush_nagle(&mut self) -> Result<(), NetError> {
        if !self.nagle_enabled || self.nagle_buffer.is_empty() {
            return Ok(());
        }

        // Tüm onaylanmış veriler gönderildiyse veya timer dolduysa flush et
        let has_unacked = self.snd_una != self.snd_nxt;
        let now = crate::task::scheduler::get_ticks() as u64;
        let timer_expired = now.wrapping_sub(self.nagle_last_send) >= NAGLE_DELAY_TICKS;

        if !has_unacked || timer_expired {
            let data = core::mem::take(&mut self.nagle_buffer);
            self.send_packet(TcpFlags::ack(), &data)?;
            self.seq_num = self.seq_num.wrapping_add(data.len() as u32);
            self.snd_nxt = self.seq_num;
            self.last_send_time = now;
        }

        Ok(())
    }

    pub fn recv(&mut self, buf: &mut [u8]) -> Result<usize, NetError> {
        if self.rx_buffer.is_empty() {
            if self.state == TcpState::CloseWait || self.state == TcpState::Closed {
                return Err(NetError::ConnectionClosed);
            }
            return Err(NetError::WouldBlock);
        }

        let len = buf.len().min(self.rx_buffer.len());
        buf[..len].copy_from_slice(&self.rx_buffer[..len]);
        self.rx_buffer.drain(..len);

        Ok(len)
    }

    pub fn close(&mut self) -> Result<(), NetError> {
        match self.state {
            TcpState::Established => {
                self.state = TcpState::FinWait1;
                self.send_packet(TcpFlags::fin_ack(), &[])?;
            }
            TcpState::CloseWait => {
                self.state = TcpState::LastAck;
                self.send_packet(TcpFlags::fin_ack(), &[])?;
            }
            _ => {
                self.state = TcpState::Closed;
            }
        }

        Ok(())
    }

    fn send_packet(&mut self, flags: TcpFlags, data: &[u8]) -> Result<(), NetError> {
        // SYN/SYN-ACK paketleri için TCP seçeneklerini oluştur
        let mut options_buf: Vec<u8> = Vec::new();
        if flags.syn {
            // MSS seçeneği (4 bayt: kind=2, len=4, value=u16)
            options_buf.push(TCPOPT_MSS);
            options_buf.push(4);
            let mss_bytes = self.mss.to_be_bytes();
            options_buf.push(mss_bytes[0]);
            options_buf.push(mss_bytes[1]);
            // Window Scale seçeneği (3 bayt: kind=3, len=3, shift=8)
            options_buf.push(TCPOPT_WINDOW_SCALE);
            options_buf.push(3);
            options_buf.push(8);
            // SACK Permitted seçeneği (2 bayt: kind=4, len=2)
            options_buf.push(TCPOPT_SACK_PERMITTED);
            options_buf.push(2);
        }

        // data_offset: 5 (20 bayt) + seçeneklerin 32-bit kelime sayısı
        let opts_len_words = (options_buf.len() + 3) / 4; // yukarı yuvarla
        let data_offset = 5 + opts_len_words as u8;

        let mut header = TcpHeader {
            src_port: self.local.port,
            dst_port: self.remote.port,
            seq_num: self.seq_num,
            ack_num: self.ack_num,
            data_offset,
            flags,
            window_size: self.window_size,
            checksum: 0,
            urgent_ptr: 0,
        };

        // TCP segmentini oluştur: başlık + seçenekler + veri
        let mut segment = vec![0u8; TcpHeader::MIN_SIZE + options_buf.len() + data.len()];
        header.serialize(&mut segment)?;
        // Seçenekleri başlıktan hemen sonraya yerleştir
        segment[TcpHeader::MIN_SIZE..TcpHeader::MIN_SIZE + options_buf.len()]
            .copy_from_slice(&options_buf);
        // Veriyi seçeneklerden sonra yerleştir
        segment[TcpHeader::MIN_SIZE + options_buf.len()..]
            .copy_from_slice(data);

        // Sağlama toplamını sahte başlık ile hesapla
        match (self.local.ip, self.remote.ip) {
            (IpAddr::V4(mut src_ip), IpAddr::V4(dst_ip)) => {
                if src_ip.is_unspecified() {
                    src_ip = super::local_ip();
                }
                header.checksum = compute_checksum(src_ip, dst_ip, &segment);
                header.serialize(&mut segment)?;

                let mut ip_buf = vec![0u8; 1500];
                let len = super::ip::build_packet(dst_ip, IpProtocol::TCP, &segment, &mut ip_buf)?;
                super::send_packet(&ip_buf[..len])?;
            }
            (IpAddr::V6(mut src_ip), IpAddr::V6(dst_ip)) => {
                if src_ip.is_unspecified() {
                    src_ip = super::ipv6::local_ipv6();
                }
                header.checksum = compute_checksum_v6(src_ip, dst_ip, &segment);
                header.serialize(&mut segment)?;

                let packet = super::ipv6::Ipv6Packet::new(
                    super::ipv6::Ipv6Header::new(
                        src_ip,
                        dst_ip,
                        Ipv6NextHeader::Tcp as u8,
                        segment.len() as u16,
                    ),
                    &segment,
                );
                let serialized = packet.serialize();
                super::send_packet(&serialized)?;
            }
            _ => return Err(NetError::InvalidParam),
        }

        Ok(())
    }

    fn on_packet(&mut self, header: &TcpHeader, data: &[u8]) -> Result<(), NetError> {
        // RST işlenirse bağlantıyı hemen kapat
        if header.flags.rst {
            if self.state == TcpState::SynSent || self.state == TcpState::SynReceived {
                self.state = TcpState::Closed;
                return Ok(());
            }
            if self.state == TcpState::Established || self.state == TcpState::CloseWait {
                self.state = TcpState::Closed;
                return Ok(());
            }
            // Diğer durumlarda RST atla (zaten kapandı)
            return Ok(());
        }

        // ACK numarasını güncelle
        if header.flags.ack {
            self.ack_num = header.ack_num;
        }

        // TCP seçeneklerini ayrıştır (header'daki options alanından)
        let header_len = header.header_len();
        let raw_header: Vec<u8> = {
            let mut buf = vec![0u8; header_len];
            header.serialize(&mut buf)?;
            buf
        };
        let options = TcpOptions::parse(&raw_header, header_len);

        // TCP durum makinesi - gelen pakete göre durum geçişleri
        match self.state {
            TcpState::Listen => {
                if header.flags.syn {
                    // Yeni bağlantı girişimi - el sıkışmanın ilk adımı
                    self.remote = SocketAddr::new(
                        match self.family {
                            AddressFamily::IPV6 => IpAddr::V6(super::ipv6::Ipv6Addr::UNSPECIFIED),
                            _ => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                        },
                        header.src_port,
                    );
                    self.seq_num = crate::random::rand_u64() as u32;
                    self.ack_num = header.seq_num.wrapping_add(1);

                    // MSS müzakeresi — karşı tarafın MSS değerini al
                    if let Some(peer_mss) = options.mss {
                        if peer_mss > 0 && peer_mss < self.mss {
                            self.mss = peer_mss;
                        }
                    }

                    self.state = TcpState::SynReceived;

                    // SYN-ACK gönder - ikinci adım
                    self.send_packet(TcpFlags::syn_ack(), &[])?;
                }
            }
            TcpState::SynSent => {
                if header.flags.syn && header.flags.ack {
                    // SYN-ACK alındı - el sıkışmanın ikinci adımı
                    self.ack_num = header.seq_num.wrapping_add(1);

                    // MSS müzakeresi — karşı tarafın MSS değerini al
                    if let Some(peer_mss) = options.mss {
                        if peer_mss > 0 && peer_mss < self.mss {
                            self.mss = peer_mss;
                        }
                    }

                    // Pencere ölçekleme müzakeresi — karşı tarafın WS değerini al
                    if let Some(ws) = options.window_scale {
                        self.peer_ws_scale = ws.scale;
                    }

                    // SACK izni — karşı taraf SACK'e izin veriyor mu?
                    if options.sack_permitted {
                        self.sack_permitted = true;
                    }

                    self.state = TcpState::Established;
                    // SYN yeniden iletim sayacını sıfırla
                    self.syn_retransmit_count = 0;

                    // ACK gönder - üçüncü ve son adım
                    self.send_packet(TcpFlags::ack(), &[])?;
                } else if header.flags.syn {
                    // Simultaneous open — her iki taraf da SYN göndermiş
                    self.ack_num = header.seq_num.wrapping_add(1);
                    self.state = TcpState::SynReceived;
                    self.send_packet(TcpFlags::syn_ack(), &[])?;
                }
            }
            TcpState::SynReceived => {
                if header.flags.ack {
                    // ACK alındı - bağlantı tamamen kuruldu
                    self.state = TcpState::Established;
                }
            }
            TcpState::Established => {
                // ACK işleme - tıkanıklık kontrolü güncelle
                if header.flags.ack {
                    let acked = header.ack_num.wrapping_sub(self.snd_una);
                    if acked > 0 && acked < 0x8000_0000 {
                        let now = crate::task::scheduler::get_ticks() as u64;
                        self.cc.on_ack(acked, now, self.rtt);
                        self.cwnd = self.cc.cwnd();
                        self.ssthresh = match self.cc.algorithm {
                            CcAlgorithm::Reno => self.cc.reno.ssthresh,
                            CcAlgorithm::Cubic => self.cc.cubic.ssthresh,
                            CcAlgorithm::Bbr | CcAlgorithm::Bbrv3 => self.cc.bbr.target_cwnd(),
                        };
                        self.snd_una = header.ack_num;
                        // Keepalive zamanlayıcısını sıfırla (ACK alındı → bağlantı canlı)
                        self.keepalive_last_ping = now;
                        self.keepalive_probe_count = 0;
                    }
                }

                // Veri paketi al
                if !data.is_empty() {
                    self.rx_buffer.extend_from_slice(data);
                    self.ack_num = self.ack_num.wrapping_add(data.len() as u32);

                    // Veriyi onayla
                    self.send_packet(TcpFlags::ack(), &[])?;
                }

                // FIN alındı - karşı taraf kapanmak istiyor
                if header.flags.fin {
                    self.state = TcpState::CloseWait;
                    self.ack_num = self.ack_num.wrapping_add(1);
                    self.send_packet(TcpFlags::ack(), &[])?;
                }

                // Geçersiz ACK kontrolü — ACK_num snd_nxt'den büyükse veya
                // snd_una'dan küçükse RST gönder (RFC 793 Section 3.9)
                if header.flags.ack && !data.is_empty() {
                    if header.ack_num.wrapping_sub(self.snd_nxt) < 0x8000_0000
                        && header.ack_num != self.snd_nxt
                    {
                        self.send_packet(TcpFlags::rst(), &[])?;
                    }
                }
            }
            TcpState::FinWait1 => {
                if header.flags.ack {
                    self.state = TcpState::FinWait2;
                }
                // FIN+ACK alınırsa (simultaneous close)
                if header.flags.fin {
                    self.ack_num = self.ack_num.wrapping_add(1);
                    self.send_packet(TcpFlags::ack(), &[])?;
                    self.state = TcpState::TimeWait;
                    self.time_wait_start = crate::task::scheduler::get_ticks() as u64;
                }
            }
            TcpState::FinWait2 => {
                if header.flags.fin {
                    self.ack_num = self.ack_num.wrapping_add(1);
                    self.send_packet(TcpFlags::ack(), &[])?;
                    self.state = TcpState::TimeWait;
                    self.time_wait_start = crate::task::scheduler::get_ticks() as u64;
                }
            }
            TcpState::LastAck => {
                if header.flags.ack {
                    self.state = TcpState::Closed;
                }
            }
            TcpState::Closing => {
                if header.flags.ack {
                    self.state = TcpState::TimeWait;
                    self.time_wait_start = crate::task::scheduler::get_ticks() as u64;
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// RTT tahminini güncelle (Jacobson/Karels algoritması, RFC 6298).
    /// SRTT = 7/8 * SRTT + 1/8 * R   (üstel hareketli ortalama)
    /// RTTVAR = 3/4 * RTTVAR + 1/4 * |SRTT - R|   (varyans tahmini)
    /// RTO = SRTT + 4 * RTTVAR
    pub fn update_rtt(&mut self, measured_rtt: u32) {
        let delta = if measured_rtt > self.rtt {
            measured_rtt - self.rtt
        } else {
            self.rtt - measured_rtt
        };

        self.rtt_var = (3 * self.rtt_var + delta) / 4;
        self.rtt = (7 * self.rtt + measured_rtt) / 8;
        self.rto = self.rtt + 4 * self.rtt_var;

        // RTO sınırla (min 200ms, max 60s - RFC 6298)
        if self.rto < 200 {
            self.rto = 200;
        }
        if self.rto > 60000 {
            self.rto = 60000;
        }
    }

    /// Yeniden iletim zamanlayıcısını kontrol et.
    /// Onaylanmamış veri varsa ve RTO süresi geçmişse yeniden iletim yap.
    pub fn check_retransmit(&mut self) -> Result<(), NetError> {
        if self.state != TcpState::Established {
            return Ok(());
        }

        let now = crate::task::scheduler::get_ticks() as u64;
        let elapsed = now.wrapping_sub(self.last_send_time);

        // Onaylanmamış veri var mı ve RTO süresi geçti mi?
        let has_unacked = self.snd_nxt != self.snd_una;
        if has_unacked && elapsed > self.rto as u64 && self.last_send_time > 0 {
            crate::serial_println!("[TCP] RTO expired, retransmitting (rto={}ms)", self.rto);

            // Tıkanıklık kontrolüne zaman aşımı bildir
            let now_ms = crate::task::scheduler::get_ticks() as u64;
            self.cc.on_timeout(now_ms);
            self.cwnd = self.cc.cwnd();
            self.ssthresh = match self.cc.algorithm {
                CcAlgorithm::Reno => self.cc.reno.ssthresh,
                CcAlgorithm::Cubic => self.cc.cubic.ssthresh,
                CcAlgorithm::Bbr | CcAlgorithm::Bbrv3 => self.cc.bbr.target_cwnd(),
            };

            // Yeniden iletim sayacını artır
            self.retransmit_count = self.retransmit_count.saturating_add(1);

            // Üstel geri çekilme (exponential backoff)
            self.rto = (self.rto * 2).min(60000);

            // tx_buffer'daki veriyi yeniden gönder
            if !self.tx_buffer.is_empty() {
                let data = self.tx_buffer.clone();
                self.send_packet(TcpFlags::ack(), &data)?;
                self.last_send_time = crate::task::scheduler::get_ticks() as u64;
            }
        }

        Ok(())
    }

    /// TCP Keepalive kontrolü (RFC 1122 Section 4.2.3.6).
    /// Boşta geçen süre keepalive_time'e ulaşırsa ping gönder, her keepalive_intvl'de tekrar dene.
    /// keepalive_probes başarısız ping sonrası bağlantıyı kapat.
    pub fn check_keepalive(&mut self) -> Result<(), NetError> {
        if !self.keepalive_enabled || self.state != TcpState::Established {
            return Ok(());
        }

        let now = crate::task::scheduler::get_ticks() as u64;
        let idle_ticks = now.wrapping_sub(self.keepalive_last_ping);

        // İlk keepalive zamanı henüz dolmadıysa atla
        if self.keepalive_probe_count == 0 && idle_ticks < self.keepalive_idle as u64 {
            return Ok(());
        }

        // Ping zamanı geldi mi?
        let since_last = if self.keepalive_probe_count == 0 {
            idle_ticks
        } else {
            now.wrapping_sub(self.keepalive_last_ping)
        };

        if since_last >= self.keepalive_intvl as u64 {
            self.keepalive_probe_count += 1;
            self.keepalive_last_ping = now;

            crate::serial_println!(
                "[TCP] Keepalive probe #{}/{} sent to {}:{}",
                self.keepalive_probe_count,
                self.keepalive_probes,
                self.remote.ip,
                self.remote.port.0
            );

            // Boş bir veri segmenti gönder (seq = snd_nxt - 1) — RFC 1122
            self.send_packet(TcpFlags::ack(), &[])?;

            // Maksimum probe sayısına ulaşıldıysa bağlantıyı kapat
            if self.keepalive_probe_count >= self.keepalive_probes {
                crate::serial_println!("[TCP] Keepalive: connection dead, closing");
                self.state = TcpState::Closed;
            }
        }

        Ok(())
    }

    /// SYN yeniden iletim kontrolü — SynSent durumunda SYN zaman aşımı kontrolü.
    /// SYN_MAX_RETRANSMIT kez yeniden denediyse bağlantıyı kapat.
    pub fn check_syn_retransmit(&mut self) -> Result<(), NetError> {
        if self.state != TcpState::SynSent || self.syn_first_send_time == 0 {
            return Ok(());
        }

        let now = crate::task::scheduler::get_ticks() as u64;
        let elapsed = now.wrapping_sub(self.syn_first_send_time);

        if elapsed >= SYN_TIMEOUT_TICKS {
            if self.syn_retransmit_count >= SYN_MAX_RETRANSMIT {
                crate::serial_println!(
                    "[TCP] SYN retransmit limit reached, connection failed to {}:{}",
                    self.remote.ip,
                    self.remote.port.0
                );
                self.state = TcpState::Closed;
                return Ok(());
            }

            crate::serial_println!(
                "[TCP] SYN retransmit #{}/{} to {}:{}",
                self.syn_retransmit_count + 1,
                SYN_MAX_RETRANSMIT,
                self.remote.ip,
                self.remote.port.0
            );

            self.syn_retransmit_count += 1;
            self.send_packet(TcpFlags::syn(), &[])?;
            self.syn_first_send_time = now;

            // Üstel geri çekilme: RTO'yu iki katına çıkar
            self.rto = (self.rto * 2).min(60000);
        }

        Ok(())
    }
}

// ============================================================================
// TCP CUBIC TIKANIKLIK KONTROLÜ
// ============================================================================
// CUBIC, Linux çekirdeğinin varsayılan tıkanıklık kontrol algoritmasıdır (RFC 8312).
// Klasik Reno/NewReno'nun doğrusal büyümesi yerine küpsel (cubic) büyüme kullanır.
//
// CUBIC Pencere Büyüme Fonksiyonu:
//   W(t) = C * (t - K)^3 + W_max
//   K = küpkök(W_max * β / C)
//
// Özellikler:
// - Tıkanıklık sonrası W_max değerinin %70'ine (β=0.7) düşer
// - Küpsel fonksiyon sayesinde yüksek-BDP ağlarda daha iyi performans
// - TCP Uyumluluğu: TCP-friendly penceresine göre maksimum alır
//
// ```
// cwnd
//   ^
//   |    W_max
//   |   /        ..(CUBIC W(t))
//   |  .       /
//   | .      /
//   |.     /
//   +---K--t---------> zaman
//       ^
//       Tıkanıklık olayı noktası
// ```

/// CUBIC tıkanıklık kontrolü durumu
#[derive(Clone, Debug)]
pub struct CubicState {
    /// CUBIC pencere azaltma sabiti (β_cubic = 0.7)
    pub beta: f64,
    /// CUBIC pencere büyüme sabiti (C = 0.4)
    pub c: f64,
    /// Son tıkanıklık öncesi pencere boyutu (W_max)
    pub w_max: f64,
    /// Son tıkanıklık olayı zamanı (ms)
    pub t_last: u64,
    /// Şimdiki tıkanıklık penceresi (bayt)
    pub cwnd: u32,
    /// Yavaş başlangıç eşiği
    pub ssthresh: u32,
    /// Gözlemlenen minimum RTT
    pub min_rtt: u32,
    /// TCP-uyumlu pencere tahmini
    pub tcp_cwnd: f64,
}

impl CubicState {
    /// CUBIC β = 0.7 (çarpımsal azaltma faktörü) - tıkanıklıkta pencereyi %30 azaltır
    const BETA: f64 = 0.7;
    /// CUBIC C = 0.4 (pencere büyüme faktörü) - RFC 8312
    const C: f64 = 0.4;
    /// MSS (Maximum Segment Size) hesaplamalar için kullanılır
    const MSS: u32 = 1460;

    pub fn new() -> Self {
        CubicState {
            beta: Self::BETA,
            c: Self::C,
            w_max: 0.0,
            t_last: 0,
            cwnd: Self::MSS * 10, // Başlangıç: 10 MSS
            ssthresh: Self::MSS * 100,
            min_rtt: 1000,
            tcp_cwnd: Self::MSS as f64 * 10.0,
        }
    }

    /// Newton-Raphson yöntemi ile küpkök hesaplama.
    /// Alanın f64::cbrt() fonksiyonu yoksa bu kullanılır.
    fn cbrt(x: f64) -> f64 {
        if x <= 0.0 {
            return 0.0;
        }
        // Başlangıç tahmini ile iteratif yakınsama
        let mut y = x;
        for _ in 0..10 {
            let y3 = y * y * y;
            if y3 == 0.0 {
                break;
            }
            y = y - (y3 - x) / (3.0 * y * y);
        }
        y
    }

    /// Tamsayı üs ile f64 kuvvet hesaplama (no_std uyumlu)
    fn powi(base: f64, exp: i32) -> f64 {
        if exp == 0 {
            return 1.0;
        }
        let abs_exp = if exp < 0 { -exp } else { exp };
        let mut result = 1.0;
        for _ in 0..abs_exp {
            result *= base;
        }
        if exp < 0 {
            1.0 / result
        } else {
            result
        }
    }

    /// CUBIC pencere fonksiyonu: W(t) = C*(t-K)^3 + W_max
    /// t: tıkanıklık olayından bu yana geçen süre (ms)
    /// K: W_max noktasına ulaşmak için gereken süre
    pub fn cubic_window(&self, t_ms: u64) -> f64 {
        if self.w_max == 0.0 {
            return self.cwnd as f64;
        }

        // K = küpkök(W_max * β / C)
        let k = Self::cbrt(self.w_max * self.beta / self.c);

        // Tıkanıklık olayından bu yana geçen süre (saniye)
        let t = (t_ms as f64 - self.t_last as f64) / 1000.0;

        // W(t) = C * (t - K)^3 + W_max
        let w = self.c * Self::powi(t - k, 3) + self.w_max;

        w.max(Self::MSS as f64)
    }

    /// TCP-uyumlu pencere hesapla (TCP Friendliness için).
    /// CUBIC, TCP'nin ulaşacağından daha yavaş büyüyorsa TCP oranını kullanır.
    /// W_tcp(t) = W_max*(1-β) + 3β/(2-β) * t/RTT
    pub fn tcp_friendly_window(&self, t_ms: u64, rtt_ms: u32) -> f64 {
        let t = (t_ms as f64 - self.t_last as f64) / 1000.0;
        let rtt = rtt_ms as f64 / 1000.0;

        if rtt == 0.0 {
            return self.tcp_cwnd;
        }

        // TCP-uyumlu artış oranı
        let alpha = 3.0 * self.beta / (2.0 - self.beta);
        let w_tcp = self.w_max * (1.0 - self.beta) + alpha * t / rtt;

        w_tcp.max(Self::MSS as f64)
    }

    /// ACK alındığında pencereyi güncelle (CUBIC algoritması).
    /// Yavaş başlangıç (Slow Start): cwnd < ssthresh ise üstel büyü.
    /// Tıkanıklık önleme (Congestion Avoidance): CUBIC W(t) veya TCP dostu.
    pub fn on_ack(&mut self, acked_bytes: u32, current_time_ms: u64, rtt_ms: u32) {
        // Min RTT güncelle
        if rtt_ms < self.min_rtt && rtt_ms > 0 {
            self.min_rtt = rtt_ms;
        }

        // Yavaş başlangıç aşaması (cwnd < ssthresh)
        if self.cwnd < self.ssthresh {
            self.cwnd += acked_bytes;
            return;
        }

        // CUBIC tıkanıklık önleme aşaması
        let cubic_w = self.cubic_window(current_time_ms);
        let tcp_w = self.tcp_friendly_window(current_time_ms, rtt_ms);

        // CUBIC ve TCP-uyumlu pencerenin maksimumunu al
        let target_w = cubic_w.max(tcp_w);

        // Bayta çevir ve güncelle
        let current_w = self.cwnd as f64;
        let new_w = current_w + (target_w - current_w) * (acked_bytes as f64 / self.cwnd as f64);
        self.cwnd = new_w.max(Self::MSS as f64) as u32;
    }

    /// Tıkanıklık olayı (paket kaybı) işle.
    /// W_max = mevcut pencere, cwnd = cwnd * β, yeni ssthresh = cwnd
    pub fn on_loss(&mut self, current_time_ms: u64) {
        // Mevcut pencereyi W_max olarak kaydet
        self.w_max = self.cwnd as f64;

        // Çarpımsal azaltma: W_max = W_max * β
        self.w_max *= self.beta;

        // Yeni cwnd ayarla
        self.cwnd = (self.cwnd as f64 * self.beta).max(Self::MSS as f64) as u32;
        self.ssthresh = self.cwnd;

        // Tıkanıklık olayı zamanını kaydet
        self.t_last = current_time_ms;
    }

    /// Zaman aşımı (RTO) işle - cwnd 1 MSS'ye sıfırlanır
    pub fn on_timeout(&mut self, current_time_ms: u64) {
        // Mevcut pencereyi kaydet
        self.w_max = self.cwnd as f64;

        // 1 MSS'ye sıfırla (RFC 5681: RTO sonrası yavaş başlangıç)
        self.cwnd = Self::MSS;
        self.ssthresh = Self::MSS * 2;

        // Zamanı kaydet
        self.t_last = current_time_ms;
    }
}

// ============================================================================
// TCP BBR TIKANIKLIK KONTROLÜ
// ============================================================================
// BBR (Bottleneck Bandwidth and Round-trip propagation time), Google tarafından
// geliştirilen model tabanlı bir tıkanıklık kontrol algoritmasıdır (RFC 9102).
//
// BBR'nin temel prensibi:
// - Ağın bottleneck bant genişliğini (BtlBw) ve minimum RTT'yi ölç
// - BDP = BtlBw × RTTprop kadarlık veri gönder (ne fazla ne az)
//
// BBR Modları:
// ```
// STARTUP: Hızla bant genişliğini bul (üstel büyüme, gain=2.89)
//    |
//    v (Bant genişliği platoya ulaştı)
// DRAIN: Dolu kuyruğu boşalt (gain=0.35)
//    |
//    v (Kuyruk boş)
// PROBE_BW: Döngüsel kazanç ile bant genişliğini araştır
//    |          [1.25, 0.75, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0]
//    v (10 saniyede bir)
// PROBE_RTT: cwnd'i azalt, gerçek RTT'yi ölç (200ms)
//    |
//    v
// PROBE_BW (geri dön)
// ```

/// BBR tıkanıklık kontrolü durumu
#[derive(Clone, Debug, Default)]
pub struct BbrState {
    /// BBR modu (Startup/Drain/ProbeBW/ProbeRTT)
    pub mode: BbrMode,
    /// Tahmini bant genişliği (bayt/saniye)
    pub bw: u64,
    /// Gözlemlenen minimum RTT (mikrosaniye)
    pub min_rtt: u64,
    /// RTT yayılım süresi (RTprop)
    pub rtprop: u64,
    /// Son RTprop güncelleme zamanı
    pub rtprop_stamp: u64,
    /// Gönderim hızı (pacing rate, bayt/saniye)
    pub pacing_rate: u64,
    /// Gönderim birimi (send quantum, bayt)
    pub send_quantum: u32,
    /// Tıkanıklık penceresi kazancı
    pub cwnd_gain: f64,
    /// Gönderim hızı kazancı
    pub pacing_gain: f64,
    /// BBR tur sayacı
    pub round_count: u64,
    /// Sonraki tur sınırı
    pub next_round_delivered: u64,
    /// Bant genişliği filtresi (son 10 turdaki maksimum)
    pub bw_filter: BbrBwFilter,
    /// RTT filtresi (10 saniyelik minimum pencere)
    pub rtt_filter: BbrRttFilter,
    /// ProbeRTT döngüsü tamamlandı mı?
    pub probe_rtt_done: bool,
    /// ProbeRTT başlangıç turu
    pub probe_rtt_round_stamp: u64,
    pub bbrv3_enabled: bool,
    pub inflight_hi: u32,
    pub ecn_alpha: f64,
    pub full_bw: u64,
    pub full_bw_rounds: u8,
}

/// BBR modları
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BbrMode {
    #[default]
    Startup, // Başlangıç: bant genişliğini hızla bul
    Drain,    // Boşalt: kuyruğu azalt
    ProbeBW,  // Bant genişliğini araştır
    ProbeRTT, // RTT'yi ölç
}

/// BBR bant genişliği filtresi - son 10 turdaki maksimum bant genişliği
#[derive(Clone, Debug)]
pub struct BbrBwFilter {
    pub samples: [u64; 10],
    pub count: usize,
}

impl Default for BbrBwFilter {
    fn default() -> Self {
        BbrBwFilter {
            samples: [0; 10],
            count: 0,
        }
    }
}

impl BbrBwFilter {
    pub fn update(&mut self, bw: u64) {
        self.samples[self.count % 10] = bw;
        self.count += 1;
    }

    pub fn max(&self) -> u64 {
        self.samples.iter().max().copied().unwrap_or(0)
    }
}

/// BBR RTT filtresi - 10 saniyelik pencerede minimum RTT
#[derive(Clone, Debug)]
pub struct BbrRttFilter {
    pub min_rtt: u64,
    pub stamp: u64,
}

impl Default for BbrRttFilter {
    fn default() -> Self {
        BbrRttFilter {
            min_rtt: u64::MAX,
            stamp: 0,
        }
    }
}

impl BbrRttFilter {
    pub fn update(&mut self, rtt: u64, now: u64) {
        // 10 saniyelik pencere
        if now - self.stamp > 10_000_000 {
            self.min_rtt = rtt;
            self.stamp = now;
        } else if rtt < self.min_rtt {
            self.min_rtt = rtt;
        }
    }
}

const BBRV3_STARTUP_PACING_GAIN: f64 = 2.77;
const BBRV3_DRAIN_PACING_GAIN: f64 = 0.70;
const BBRV3_HEADROOM: f64 = 0.85;

impl BbrState {
    /// BBR sabitleri (RFC 9102)
    const BBR_HIGH_GAIN: f64 = 2.89; // 2/ln(2) - Startup için yüksek kazanç
    const BBR_DRAIN_GAIN: f64 = 0.35; // 1/2.89 - Drain için düşük kazanç
    const BBR_CWND_GAIN_TARGET: f64 = 2.0; // Hedef cwnd kazancı
    const BBR_PROBE_RTT_CWND_GAIN: f64 = 0.5; // ProbeRTT sırasında cwnd yarıya iner
    const BBR_PROBE_RTT_MODE_DURATION_MS: u64 = 200; // ProbeRTT süresi (ms)
    const BBR_MIN_RTT_WIN_SEC: u64 = 10; // RTprop güncelleme penceresi (saniye)

    pub fn new() -> Self {
        BbrState {
            mode: BbrMode::Startup,
            bw: 0,
            min_rtt: 0,
            rtprop: 0,
            rtprop_stamp: 0,
            pacing_rate: 0,
            send_quantum: 1460,
            cwnd_gain: Self::BBR_HIGH_GAIN,
            pacing_gain: Self::BBR_HIGH_GAIN,
            round_count: 0,
            next_round_delivered: 0,
            bw_filter: BbrBwFilter::default(),
            rtt_filter: BbrRttFilter::default(),
            probe_rtt_done: false,
            probe_rtt_round_stamp: 0,
            bbrv3_enabled: false,
            inflight_hi: 1460 * 100,
            ecn_alpha: 0.0,
            full_bw: 0,
            full_bw_rounds: 0,
        }
    }

    pub fn enable_v3(&mut self) {
        self.bbrv3_enabled = true;
        self.cwnd_gain = Self::BBR_CWND_GAIN_TARGET;
        self.pacing_gain = BBRV3_STARTUP_PACING_GAIN;
    }

    /// Gönderim hızını hesapla: pacing_rate = BtlBw * pacing_gain
    pub fn set_pacing_rate(&mut self) {
        let bw = self.bw as f64;
        let gain = self.pacing_gain;
        self.pacing_rate = (bw * gain) as u64;
    }

    /// Gönderim birimini hesapla: min(64KB, pacing_rate/1000)
    pub fn set_send_quantum(&mut self) {
        // Gönderim birimi = min(64KB, pacing_rate / 1000)
        let quantum = (self.pacing_rate / 1000).min(65536) as u32;
        self.send_quantum = quantum.max(1460);
    }

    /// Hedef cwnd hesapla: cwnd_gain * BDP
    /// BDP = BtlBw * RTTprop (Bandwidth-Delay Product)
    pub fn target_cwnd(&self) -> u32 {
        // BDP = bant_genişliği × min_rtt (bayt cinsinden)
        let bdp = if self.min_rtt > 0 {
            (self.bw * self.min_rtt / 1_000_000) as u32 // mikrosaniyeyi milisaniyeye çevir
        } else {
            1460
        };

        // target_cwnd = cwnd_gain * BDP
        let target = (self.cwnd_gain * bdp as f64) as u32;
        if self.bbrv3_enabled {
            target.min(self.inflight_hi.max(1460))
        } else {
            target.max(1460)
        }
    }

    /// ACK alındığında BBR durumunu güncelle.
    /// Bant genişliği ve RTT ölçümlerini günceller, mod geçişlerini yönetir.
    pub fn on_ack(&mut self, delivered: u32, rtt_us: u64, now_us: u64) {
        // Bant genişliği örneğini güncelle: teslim_edilen / RTT
        if rtt_us > 0 {
            let bw_sample = (delivered as u64 * 1_000_000) / rtt_us;
            self.bw_filter.update(bw_sample);
            self.bw = self.bw_filter.max();
        }

        // RTT tahminini güncelle
        self.rtt_filter.update(rtt_us, now_us);
        self.min_rtt = self.rtt_filter.min_rtt;

        // Tur sayacını artır
        self.round_count += 1;

        // Moda özgü işlemler
        match self.mode {
            BbrMode::Startup => {
                // Bottleneck bant genişliğine ulaşıldı mı kontrol et
                if self.is_full_bw_reached() {
                    self.mode = BbrMode::Drain;
                    self.pacing_gain = if self.bbrv3_enabled {
                        BBRV3_DRAIN_PACING_GAIN
                    } else {
                        Self::BBR_DRAIN_GAIN
                    };
                    self.cwnd_gain = Self::BBR_CWND_GAIN_TARGET;
                }
            }
            BbrMode::Drain => {
                // Kuyruk boşalmasını bekle
                if self.target_cwnd() <= 1460 {
                    self.mode = BbrMode::ProbeBW;
                    self.pacing_gain = 1.0;
                    self.cwnd_gain = Self::BBR_CWND_GAIN_TARGET;
                }
            }
            BbrMode::ProbeBW => {
                // Kazanç döngüsü: 1.25, 0.75, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0
                let cycle_idx = (self.round_count / 8) % 8;
                self.pacing_gain = match cycle_idx {
                    0 => 1.25, // Prob: daha hızlı gönder
                    1 => 0.75, // Boşalt: daha yavaş gönder
                    _ => 1.0,  // Normal hız
                };

                // RTT probu gerekiyor mu kontrol et (10 saniyede bir)
                if now_us - self.rtprop_stamp > Self::BBR_MIN_RTT_WIN_SEC * 1_000_000 {
                    self.mode = BbrMode::ProbeRTT;
                    self.cwnd_gain = Self::BBR_PROBE_RTT_CWND_GAIN;
                }
            }
            BbrMode::ProbeRTT => {
                // 200ms ProbeRTT modunda kal, gerçek RTT'yi ölç
                if now_us - self.rtprop_stamp > Self::BBR_PROBE_RTT_MODE_DURATION_MS * 1000 {
                    self.mode = BbrMode::ProbeBW;
                    self.cwnd_gain = Self::BBR_CWND_GAIN_TARGET;
                }
            }
        }

        // Gönderim hızını ve birimini güncelle
        if self.bbrv3_enabled {
            let bdp = if self.min_rtt > 0 {
                (self.bw * self.min_rtt / 1_000_000) as u32
            } else {
                1460
            };
            let ecn_penalty = (1.0 - (self.ecn_alpha * 0.5)).clamp(0.5, 1.0);
            self.inflight_hi = ((bdp as f64 * BBRV3_HEADROOM * ecn_penalty) as u32).max(1460);
        }
        self.set_pacing_rate();
        self.set_send_quantum();
    }

    /// Tam bant genişliğine ulaşıldı mı kontrol et (Startup aşaması bitiyor mu?).
    /// Gerçek BBR'de bant genişliği büyüme oranı izlenir.
    fn is_full_bw_reached(&mut self) -> bool {
        if self.bw == 0 {
            return false;
        }

        let bw_threshold = if self.full_bw == 0 {
            0
        } else {
            ((self.full_bw as u128 * 5) / 4) as u64
        };

        if self.full_bw == 0 || self.bw >= bw_threshold {
            self.full_bw = self.bw;
            self.full_bw_rounds = 0;
            return false;
        }

        self.full_bw_rounds = self.full_bw_rounds.saturating_add(1);
        self.full_bw_rounds >= 3
    }

    /// Kayıp olayını işle.
    /// BBR kaybı doğrudan tıkanıklık işareti olarak görmez,
    /// bant genişliği tahminine dayalı çalışır.
    pub fn on_loss(&mut self) {
        if self.bbrv3_enabled {
            self.inflight_hi = (self.inflight_hi.saturating_mul(9) / 10).max(1460);
            self.ecn_alpha = (self.ecn_alpha + 0.05).min(1.0);
        }
        // BBR kaybı doğrudan işlemez
        // Bant genişliği tahminini kullanır
    }

    /// Zaman aşımını işle - Startup moduna sıfırla
    pub fn on_timeout(&mut self, now_us: u64) {
        // Startup moduna sıfırla
        self.mode = BbrMode::Startup;
        self.cwnd_gain = Self::BBR_HIGH_GAIN;
        self.pacing_gain = if self.bbrv3_enabled {
            BBRV3_STARTUP_PACING_GAIN
        } else {
            Self::BBR_HIGH_GAIN
        };
        self.rtprop_stamp = now_us;
    }

    /// Mevcut cwnd hesapla (moda göre değişir)
    pub fn cwnd(&self) -> u32 {
        match self.mode {
            BbrMode::ProbeRTT => 1460, // ProbeRTT'de minimum (4 segment)
            _ => self.target_cwnd(),
        }
    }
}

// ============================================================================
// TIKANIKLIK KONTROL ALGORİTMASI SEÇİMİ
// ============================================================================
// Linux çekirdeğinde farklı algoritmalar mevcuttur:
// - Reno: Temel, 1990'lardan beri kullanılıyor. Basit ama WAN'da verimsiz.
// - CUBIC: Linux 2.6.19'dan varsayılan. Yüksek-BDP ağlarda daha iyi.
// - BBR: Google'ın 2016 algoritması. Model tabanlı, daha verimli.

/// Tıkanıklık kontrol algoritması seçimi
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CcAlgorithm {
    #[default]
    Reno, // Klasik TCP Reno
    Cubic, // CUBIC (modern standart)
    Bbr,   // BBR (Google)
    Bbrv3, // BBRv3
}

/// Tıkanıklık kontrol durumu - seçilen algoritmayı yönetir
#[derive(Clone, Debug)]
pub struct CcState {
    pub algorithm: CcAlgorithm,
    pub reno: RenoState,
    pub cubic: CubicState,
    pub bbr: BbrState,
}

/// Reno tıkanıklık kontrolü - temel TCP algoritması.
/// Slow Start + Congestion Avoidance + Fast Retransmit + Fast Recovery
#[derive(Clone, Debug)]
pub struct RenoState {
    pub cwnd: u32,     // Tıkanıklık penceresi
    pub ssthresh: u32, // Yavaş başlangıç eşiği
    pub rtt: u32,      // RTT tahmini (ms)
    pub rtt_var: u32,  // RTT varyansı
    pub rto: u32,      // Yeniden iletim zaman aşımı (ms)
}

impl Default for RenoState {
    fn default() -> Self {
        RenoState {
            cwnd: 1460 * 10,
            ssthresh: 1460 * 100,
            rtt: 1000,
            rtt_var: 500,
            rto: 1000,
        }
    }
}

impl CcState {
    pub fn new(algorithm: CcAlgorithm) -> Self {
        let mut bbr = BbrState::new();
        if algorithm == CcAlgorithm::Bbrv3 {
            bbr.enable_v3();
        }
        CcState {
            algorithm,
            reno: RenoState::default(),
            cubic: CubicState::new(),
            bbr,
        }
    }

    pub fn cwnd(&self) -> u32 {
        match self.algorithm {
            CcAlgorithm::Reno => self.reno.cwnd,
            CcAlgorithm::Cubic => self.cubic.cwnd,
            CcAlgorithm::Bbr | CcAlgorithm::Bbrv3 => self.bbr.cwnd(),
        }
    }

    pub fn on_ack(&mut self, acked_bytes: u32, current_time_ms: u64, rtt_ms: u32) {
        match self.algorithm {
            CcAlgorithm::Reno => {
                if self.reno.cwnd < self.reno.ssthresh {
                    self.reno.cwnd += acked_bytes;
                } else {
                    self.reno.cwnd += (1460 * acked_bytes) / self.reno.cwnd;
                }
            }
            CcAlgorithm::Cubic => {
                self.cubic.on_ack(acked_bytes, current_time_ms, rtt_ms);
            }
            CcAlgorithm::Bbr | CcAlgorithm::Bbrv3 => {
                self.bbr
                    .on_ack(acked_bytes, rtt_ms as u64 * 1000, current_time_ms * 1000);
            }
        }
    }

    pub fn on_loss(&mut self, current_time_ms: u64) {
        match self.algorithm {
            CcAlgorithm::Reno => {
                self.reno.ssthresh = self.reno.cwnd / 2;
                self.reno.cwnd = self.reno.ssthresh;
            }
            CcAlgorithm::Cubic => {
                self.cubic.on_loss(current_time_ms);
            }
            CcAlgorithm::Bbr | CcAlgorithm::Bbrv3 => {
                self.bbr.on_loss();
            }
        }
    }

    pub fn on_timeout(&mut self, current_time_ms: u64) {
        match self.algorithm {
            CcAlgorithm::Reno => {
                self.reno.ssthresh = self.reno.cwnd / 2;
                self.reno.cwnd = 1460;
            }
            CcAlgorithm::Cubic => {
                self.cubic.on_timeout(current_time_ms);
            }
            CcAlgorithm::Bbr | CcAlgorithm::Bbrv3 => {
                self.bbr.on_timeout(current_time_ms * 1000);
            }
        }
    }
}

// ============================================================================
// TCP YÖNETİCİSİ
// ============================================================================
// Tüm TCP bağlantıları ve dinleyiciler global tablolarda saklanır.
// spin::Mutex ile iş parçacığı güvenliği sağlanır.

pub static TCP_CONNECTIONS: Mutex<BTreeMap<u32, Box<TcpConnection>>> = Mutex::new(BTreeMap::new());
static TCP_LISTENERS: Mutex<BTreeMap<(AddressFamily, Port), u32>> = Mutex::new(BTreeMap::new());
/// Kabul kuyruğu: dinleyici soket ID'si -> bekleyen çocuk bağlantı ID'leri
static ACCEPT_QUEUE: Mutex<BTreeMap<u32, Vec<u32>>> = Mutex::new(BTreeMap::new());

/// TCP alt sistemini başlat
pub fn init() {
    crate::serial_println!("[TCP] Initialized");
}

/// Yeni TCP soketi oluştur ve bağlantı tablosuna ekle
pub fn create_socket(family: AddressFamily) -> u32 {
    let local = match family {
        AddressFamily::IPV6 => SocketAddr::unspecified_v6(Port(0)),
        _ => SocketAddr::default(),
    };
    let conn = TcpConnection::new(local, family);
    let id = conn.id;
    TCP_CONNECTIONS.lock().insert(id, Box::new(conn));
    id
}

/// TCP soketini yerel adrese bağla
pub fn bind(socket_id: u32, addr: SocketAddr) -> Result<(), NetError> {
    let mut conns = TCP_CONNECTIONS.lock();
    let conn = conns.get_mut(&socket_id).ok_or(NetError::ProtocolError)?;
    conn.local = addr;
    Ok(())
}

/// Uzak adrese TCP bağlantısı başlat (SYN gönderir)
pub fn connect(socket_id: u32, remote: SocketAddr) -> Result<(), NetError> {
    let mut conns = TCP_CONNECTIONS.lock();
    let conn = conns.get_mut(&socket_id).ok_or(NetError::ProtocolError)?;
    conn.connect(remote)
}

/// TCP soketini dinleme moduna al ve port kaydını yap
pub fn listen(socket_id: u32, backlog: usize) -> Result<(), NetError> {
    let mut conns = TCP_CONNECTIONS.lock();
    let conn = conns.get_mut(&socket_id).ok_or(NetError::ProtocolError)?;
    conn.listen(backlog)?;

    // Dinleyici olarak kaydet
    let mut listeners = TCP_LISTENERS.lock();
    listeners.insert((conn.family, conn.local.port), socket_id);

    Ok(())
}

/// Gelen bağlantıyı kabul et
pub fn accept(socket_id: u32) -> Result<(u32, SocketAddr), NetError> {
    // Dinleyici durumunu doğrula
    {
        let conns = TCP_CONNECTIONS.lock();
        let conn = conns.get(&socket_id).ok_or(NetError::ProtocolError)?;
        if conn.state != TcpState::Listen {
            return Err(NetError::ProtocolError);
        }
    }

    // Kabul kuyruğundan Established durumundaki bağlantıyı al
    let child_ids: Vec<u32> = {
        let queue = ACCEPT_QUEUE.lock();
        queue.get(&socket_id).cloned().unwrap_or_default()
    };

    for &child_id in &child_ids {
        let established = {
            let conns = TCP_CONNECTIONS.lock();
            conns
                .get(&child_id)
                .map_or(false, |c| c.state == TcpState::Established)
        };
        if established {
            let remote = {
                let conns = TCP_CONNECTIONS.lock();
                conns.get(&child_id).map(|c| c.remote).unwrap_or_default()
            };
            // Kabul kuyruğundan çıkar
            let mut queue = ACCEPT_QUEUE.lock();
            if let Some(ids) = queue.get_mut(&socket_id) {
                ids.retain(|&id| id != child_id);
            }
            return Ok((child_id, remote));
        }
    }

    Err(NetError::WouldBlock)
}

/// Bağlantı üzerinden veri gönder
pub fn send(socket_id: u32, data: &[u8]) -> Result<usize, NetError> {
    let mut conns = TCP_CONNECTIONS.lock();
    let conn = conns.get_mut(&socket_id).ok_or(NetError::ProtocolError)?;
    conn.send(data)
}

/// Bağlantıdan veri al
pub fn recv(socket_id: u32, buf: &mut [u8]) -> Result<usize, NetError> {
    let mut conns = TCP_CONNECTIONS.lock();
    let conn = conns.get_mut(&socket_id).ok_or(NetError::ProtocolError)?;
    conn.recv(buf)
}

/// Non-blocking send - tries to send without waiting
/// Returns WouldBlock if the connection is not ready for sending
pub fn try_send(socket_id: u32, data: &[u8]) -> Result<usize, NetError> {
    let mut conns = TCP_CONNECTIONS.lock();
    let conn = conns.get_mut(&socket_id).ok_or(NetError::ProtocolError)?;

    // Check connection state
    if conn.state != TcpState::Established {
        return Err(NetError::ConnectionClosed);
    }

    // Try to send - this is already non-blocking in our implementation
    conn.send(data)
}

/// Non-blocking receive - returns immediately if no data available
/// Returns WouldBlock if no data is available
pub fn try_recv(socket_id: u32, buf: &mut [u8]) -> Result<usize, NetError> {
    let mut conns = TCP_CONNECTIONS.lock();
    let conn = conns.get_mut(&socket_id).ok_or(NetError::ProtocolError)?;

    // Check if data is available
    if conn.rx_buffer.is_empty() {
        if conn.state == TcpState::CloseWait || conn.state == TcpState::Closed {
            return Err(NetError::ConnectionClosed);
        }
        return Err(NetError::WouldBlock);
    }

    conn.recv(buf)
}

/// Peek at data without consuming it from the buffer
/// Returns the data but leaves it in the receive buffer
pub fn peek(socket_id: u32, buf: &mut [u8]) -> Result<usize, NetError> {
    let conns = TCP_CONNECTIONS.lock();
    let conn = conns.get(&socket_id).ok_or(NetError::ProtocolError)?;

    // Check if data is available
    if conn.rx_buffer.is_empty() {
        if conn.state == TcpState::CloseWait || conn.state == TcpState::Closed {
            return Err(NetError::ConnectionClosed);
        }
        return Err(NetError::WouldBlock);
    }

    // Copy data without removing from buffer
    let len = buf.len().min(conn.rx_buffer.len());
    buf[..len].copy_from_slice(&conn.rx_buffer[..len]);

    Ok(len)
}

/// Receive exactly the requested amount of data (blocking)
/// Waits until buffer is full or connection is closed
pub fn recv_all(socket_id: u32, buf: &mut [u8]) -> Result<usize, NetError> {
    let mut total_read = 0;

    while total_read < buf.len() {
        let read = {
            let mut conns = TCP_CONNECTIONS.lock();
            let conn = conns.get_mut(&socket_id).ok_or(NetError::ProtocolError)?;

            // Check if connection is closed
            if conn.state == TcpState::CloseWait || conn.state == TcpState::Closed {
                if conn.rx_buffer.is_empty() {
                    break; // EOF - return what we have
                }
            }

            // Try to read what's available
            if !conn.rx_buffer.is_empty() {
                let remaining = &mut buf[total_read..];
                let len = remaining.len().min(conn.rx_buffer.len());
                remaining[..len].copy_from_slice(&conn.rx_buffer[..len]);
                conn.rx_buffer.drain(..len);
                len
            } else {
                0
            }
        };

        if read == 0 {
            // No data available - check if connection is still active
            let conns = TCP_CONNECTIONS.lock();
            let conn = conns.get(&socket_id).ok_or(NetError::ProtocolError)?;

            if conn.state == TcpState::CloseWait || conn.state == TcpState::Closed {
                break; // Connection closed, return what we have
            }

            // Yield CPU and wait for data
            drop(conns);
            crate::task::scheduler::schedule();
        } else {
            total_read += read;
        }
    }

    if total_read == 0 {
        Err(NetError::ConnectionClosed)
    } else {
        Ok(total_read)
    }
}

/// TCP soketini kapat (FIN gönderir)
pub fn close(socket_id: u32) -> Result<(), NetError> {
    let mut conns = TCP_CONNECTIONS.lock();
    if let Some(conn) = conns.get_mut(&socket_id) {
        conn.close()?;
    }
    Ok(())
}

/// Bağlantıyı ID'ye göre al (olay kontrolü için)
pub fn get_connection(socket_id: u32) -> Option<TcpConnection> {
    let conns = TCP_CONNECTIONS.lock();
    conns.get(&socket_id).map(|c| (**c).clone())
}

/// Tüm bağlantıları al (ss komutu için)
pub fn get_all_connections() -> Vec<TcpConnection> {
    let conns = TCP_CONNECTIONS.lock();
    conns.values().map(|c| (**c).clone()).collect()
}

/// Bağlantının yerel adresini al (getsockname için)
pub fn get_connection_local_addr(socket_id: u32) -> Result<SocketAddr, NetError> {
    let conns = TCP_CONNECTIONS.lock();
    conns
        .get(&socket_id)
        .map(|c| c.local.clone())
        .ok_or(NetError::InvalidFd)
}

/// Bağlantının uzak adresini al (getpeername için)
pub fn get_connection_remote_addr(socket_id: u32) -> Result<SocketAddr, NetError> {
    let conns = TCP_CONNECTIONS.lock();
    conns
        .get(&socket_id)
        .map(|c| c.remote.clone())
        .ok_or(NetError::InvalidFd)
}

/// Gelen TCP paketini işle.
/// Hedef porta göre mevcut bağlantı veya dinleyici aranır.
pub fn process_packet(ip_packet: &Ipv4Packet) -> Result<(), NetError> {
    if !verify_checksum(
        ip_packet.header.src,
        ip_packet.header.dst,
        ip_packet.payload,
    ) {
        crate::serial_println!("[TCP] IPv4 checksum verification failed, dropping packet");
        return Err(NetError::ChecksumError);
    }
    let tcp_header = TcpHeader::parse(ip_packet.payload)?;
    let data = &ip_packet.payload[tcp_header.header_len()..];

    // Bağlantı ara
    let conns = TCP_CONNECTIONS.lock();

    // Kurulu bağlantı ara (hem yerel hem uzak port eşleşmeli)
    let mut found_id = None;
    for (_, conn) in conns.iter() {
        if conn.family == AddressFamily::IPV4
            && conn.local.port == tcp_header.dst_port
            && conn.remote.port == tcp_header.src_port
        {
            found_id = Some(conn.id);
            break;
        }
    }
    drop(conns);

    if let Some(id) = found_id {
        let mut conns = TCP_CONNECTIONS.lock();
        if let Some(conn) = conns.get_mut(&id) {
            conn.remote.ip = IpAddr::V4(ip_packet.header.src);
            return conn.on_packet(&tcp_header, data);
        }
    }

    // Dinleyici ara (sadece hedef port kontrol edilir)
    let listeners = TCP_LISTENERS.lock();
    if let Some(&listener_id) = listeners.get(&(AddressFamily::IPV4, tcp_header.dst_port)) {
        drop(listeners);

        if tcp_header.flags.syn {
            // SYN alındı: yeni çocuk bağlantı oluştur ve kabul kuyruğuna ekle
            let local_addr = {
                let conns = TCP_CONNECTIONS.lock();
                conns.get(&listener_id).map(|c| c.local)
            };

            if let Some(local_addr) = local_addr {
                let mut child = TcpConnection::new(local_addr, AddressFamily::IPV4);
                child.remote = SocketAddr::new(ip_packet.header.src, tcp_header.src_port);
                child.seq_num = crate::random::rand_u64() as u32;
                child.ack_num = tcp_header.seq_num.wrapping_add(1);
                child.state = TcpState::SynReceived;
                let _ = child.send_packet(TcpFlags::syn_ack(), &[]);
                let child_id = child.id;

                TCP_CONNECTIONS.lock().insert(child_id, Box::new(child));
                ACCEPT_QUEUE
                    .lock()
                    .entry(listener_id)
                    .or_insert_with(Vec::new)
                    .push(child_id);
                crate::serial_println!(
                    "[TCP] SYN received, child connection {} created for listener {}",
                    child_id,
                    listener_id
                );
            }

            return Ok(());
        }

        // SYN olmayan paketler için dinleyiciyi kontrol et
        let mut conns = TCP_CONNECTIONS.lock();
        if let Some(conn) = conns.get_mut(&listener_id) {
            conn.remote.ip = IpAddr::V4(ip_packet.header.src);
            return conn.on_packet(&tcp_header, data);
        }
    }

    // Eşleşen bağlantı yok - RST gönderilebilir
    Ok(())
}

pub fn process_ipv6_packet(ip_packet: &Ipv6Packet) -> Result<(), NetError> {
    if !verify_checksum_v6(
        ip_packet.header.src,
        ip_packet.header.dst,
        &ip_packet.payload,
    ) {
        crate::serial_println!("[TCPv6] Checksum verification failed, dropping packet");
        return Err(NetError::ChecksumError);
    }

    let tcp_header = TcpHeader::parse(&ip_packet.payload)?;
    let data = &ip_packet.payload[tcp_header.header_len()..];

    let conns = TCP_CONNECTIONS.lock();
    let mut found_id = None;
    for (_, conn) in conns.iter() {
        if conn.family == AddressFamily::IPV6
            && conn.local.port == tcp_header.dst_port
            && conn.remote.port == tcp_header.src_port
        {
            found_id = Some(conn.id);
            break;
        }
    }
    drop(conns);

    if let Some(id) = found_id {
        let mut conns = TCP_CONNECTIONS.lock();
        if let Some(conn) = conns.get_mut(&id) {
            conn.remote.ip = IpAddr::V6(ip_packet.header.src);
            return conn.on_packet(&tcp_header, data);
        }
    }

    let listeners = TCP_LISTENERS.lock();
    if let Some(&listener_id) = listeners.get(&(AddressFamily::IPV6, tcp_header.dst_port)) {
        drop(listeners);

        if tcp_header.flags.syn {
            let local_addr = {
                let conns = TCP_CONNECTIONS.lock();
                conns.get(&listener_id).map(|c| c.local)
            };

            if let Some(local_addr) = local_addr {
                let mut child = TcpConnection::new(local_addr, AddressFamily::IPV6);
                child.remote = SocketAddr::new(ip_packet.header.src, tcp_header.src_port);
                child.seq_num = crate::random::rand_u64() as u32;
                child.ack_num = tcp_header.seq_num.wrapping_add(1);
                child.state = TcpState::SynReceived;
                let _ = child.send_packet(TcpFlags::syn_ack(), &[]);
                let child_id = child.id;

                TCP_CONNECTIONS.lock().insert(child_id, Box::new(child));
                ACCEPT_QUEUE
                    .lock()
                    .entry(listener_id)
                    .or_insert_with(Vec::new)
                    .push(child_id);
            }
            return Ok(());
        }

        let mut conns = TCP_CONNECTIONS.lock();
        if let Some(conn) = conns.get_mut(&listener_id) {
            conn.remote.ip = IpAddr::V6(ip_packet.header.src);
            return conn.on_packet(&tcp_header, data);
        }
    }

    Ok(())
}

// ============================================================================
// netstat desteği
// ============================================================================

/// netstat komutu için bağlantı özeti
#[derive(Clone, Debug)]
pub struct TcpConnInfo {
    pub local_ip: IpAddr,
    pub local_port: u16,
    pub remote_ip: IpAddr,
    pub remote_port: u16,
    pub state: TcpState,
}

/// Tüm TCP bağlantılarını listele (netstat için)
pub fn list_connections() -> Vec<TcpConnInfo> {
    let conns = TCP_CONNECTIONS.lock();
    conns
        .values()
        .map(|c| TcpConnInfo {
            local_ip: c.local.ip,
            local_port: c.local.port.0,
            remote_ip: c.remote.ip,
            remote_port: c.remote.port.0,
            state: c.state,
        })
        .collect()
}

/// TIME_WAIT durumundaki bağlantıları temizle (2×MSL zamanlayıcısı).
/// Periyodik olarak çağrılmalıdır (örn. zamanlayıcı kesmeleri içinden).
pub fn time_wait_gc() {
    let now = crate::task::scheduler::get_ticks() as u64;
    let mut conns = TCP_CONNECTIONS.lock();
    let expired_ids: Vec<u32> = conns
        .iter()
        .filter(|(_, c)| {
            c.state == TcpState::TimeWait
                && c.time_wait_start > 0
                && now.wrapping_sub(c.time_wait_start) >= TIME_WAIT_DURATION
        })
        .map(|(&id, _)| id)
        .collect();

    for id in &expired_ids {
        crate::serial_println!("[TCP] TIME_WAIT expired, removing connection {}", id);
        conns.remove(id);
    }
}

/// TCP zamanlayıcı tetikleme — tüm aktif bağlantılarda periyodik kontrolleri çalıştır.
/// Zamanlayıcı kesmesi veya idle döngü tarafından periyodik olarak çağrılmalıdır.
pub fn tcp_timer_tick() {
    let mut conns = TCP_CONNECTIONS.lock();
    let ids: Vec<u32> = conns.keys().copied().collect();
    let mut closed_ids: Vec<u32> = Vec::new();

    for &id in &ids {
        if let Some(conn) = conns.get_mut(&id) {
            // Nagle flush kontrolü
            let _ = conn.flush_nagle();
            // Yeniden iletim kontrolü
            let _ = conn.check_retransmit();
            // Keepalive kontrolü
            let _ = conn.check_keepalive();
            // SYN yeniden iletim kontrolü
            let _ = conn.check_syn_retransmit();
            // Kapanmış bağlantıları işaretle
            if conn.state == TcpState::Closed {
                closed_ids.push(id);
            }
        }
    }

    // TIME_WAIT GC
    let now = crate::task::scheduler::get_ticks() as u64;
    for (&id, conn) in conns.iter() {
        if conn.state == TcpState::TimeWait
            && conn.time_wait_start > 0
            && now.wrapping_sub(conn.time_wait_start) >= TIME_WAIT_DURATION
        {
            closed_ids.push(id);
        }
    }

    for id in closed_ids {
        conns.remove(&id);
    }
}

/// SACK retransmission entry — tek bir segment'in yeniden gönderim kaydı.
#[derive(Debug, Clone)]
pub struct RetransmitEntry {
    /// Segment başlangıç sequence numarası
    pub seq_start: u32,
    /// Segment bitiş sequence numarası
    pub seq_end: u32,
    /// Gönderim sayısı
    pub tx_count: u32,
    /// Son gönderim zamanı (TSC)
    pub last_sent_tsc: u64,
    /// Kayıp olarak işaretlendi mi
    pub marked_lost: bool,
    /// SACK edildi mi
    pub sacked: bool,
}

/// SACK congestion state — kayıp tabanlı tıkanıklık kontrolü.
#[derive(Debug, Clone)]
pub struct SackCongestionState {
    pub cwnd: u32,
    pub ssthresh: u32,
    pub pipe: u32,
    pub limited_transmit: u32,
}

impl SackCongestionState {
    pub fn new(initial_cwnd: u32) -> Self {
        Self {
            cwnd: initial_cwnd,
            ssthresh: u32::MAX,
            pipe: 0,
            limited_transmit: 0,
        }
    }

    /// RFC 6675 Pipe hesaplaması.
    pub fn update_pipe(&mut self, lost: usize, sacked: usize) {
        self.pipe = (lost + sacked) as u32;
    }

    /// Kayıp tespiti sonrası multiplicative decrease.
    pub fn on_loss(&mut self) {
        self.ssthresh = (self.cwnd / 2).max(2);
        self.cwnd = self.ssthresh;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tfo_cookie_generation_is_mac_scoped_to_ip_and_window() {
        TCP_TFO_COOKIE_EPOCH.store(0x1234_5678, Ordering::Relaxed);

        let server = Ipv4Addr::new(203, 0, 113, 10);
        let other = Ipv4Addr::new(203, 0, 113, 11);
        let cookie = TfoCookie::generate(server, 123_456);

        assert!(cookie.verify(server, 123_456));
        assert!(!cookie.verify(server, 123_457));
        assert!(!cookie.verify(other, 123_456));
    }

    #[test]
    fn timestamp_option_uses_monotonic_counter_for_rtt() {
        TCP_TIMESTAMP_COUNTER.store(0, Ordering::Relaxed);

        let first = TimestampOption::now(0);
        let second = TimestampOption::now(first.ts_val);

        assert_eq!(first.ts_val, 1000);
        assert_eq!(second.ts_val, 2000);
        assert_eq!(second.calculate_rtt(), 1000);
    }

    #[test]
    fn bbr_startup_requires_stalled_growth_before_drain() {
        let mut bbr = BbrState::new();

        bbr.on_ack(1460, 1_000, 1_000);
        assert_eq!(bbr.mode, BbrMode::Startup);

        bbr.on_ack(1825, 1_000, 2_000);
        assert_eq!(bbr.mode, BbrMode::Startup);

        bbr.on_ack(1830, 1_000, 3_000);
        assert_eq!(bbr.mode, BbrMode::Startup);

        bbr.on_ack(1832, 1_000, 4_000);
        assert_eq!(bbr.mode, BbrMode::Startup);

        bbr.on_ack(1831, 1_000, 5_000);
        assert_eq!(bbr.mode, BbrMode::Drain);
        assert!(bbr.full_bw >= 1_825_000);
        assert!(bbr.full_bw_rounds >= 3);
    }

    #[test]
    fn tcp_header_parse_rejects_data_offset_beyond_segment() {
        let mut segment = [0u8; TcpHeader::MIN_SIZE];
        segment[12] = 15 << 4;

        assert_eq!(
            TcpHeader::parse(&segment).unwrap_err(),
            NetError::InvalidPacket
        );
    }

    #[test]
    fn sack_block_len_tracks_wrapping_sequence_space() {
        let block = SackBlock::new(u32::MAX - 3, 4);

        assert_eq!(block.len(), 8);
        assert!(block.contains(u32::MAX - 1));
        assert!(block.contains(1));
        assert!(!block.contains(8));
    }

    #[test]
    fn tcp_ipv6_listener_demuxes_syn_into_child_queue() {
        let listener = create_socket(AddressFamily::IPV6);
        bind(listener, SocketAddr::unspecified_v6(Port(54000))).unwrap();
        listen(listener, 4).unwrap();

        let src_ip =
            super::super::ipv6::Ipv6Addr::from_segments([0x2001, 0xdb8, 0, 0, 0, 0, 0, 10]);
        let dst_ip =
            super::super::ipv6::Ipv6Addr::from_segments([0x2001, 0xdb8, 0, 0, 0, 0, 0, 20]);

        let mut header = TcpHeader {
            src_port: Port(41000),
            dst_port: Port(54000),
            seq_num: 1234,
            ack_num: 0,
            data_offset: 5,
            flags: TcpFlags::syn(),
            window_size: 65535,
            checksum: 0,
            urgent_ptr: 0,
        };
        let mut segment = vec![0u8; TcpHeader::MIN_SIZE];
        header.serialize(&mut segment).unwrap();
        header.checksum = compute_checksum_v6(src_ip, dst_ip, &segment);
        header.serialize(&mut segment).unwrap();

        let packet = Ipv6Packet::new(
            super::super::ipv6::Ipv6Header::new(
                src_ip,
                dst_ip,
                Ipv6NextHeader::Tcp as u8,
                segment.len() as u16,
            ),
            &segment,
        );

        process_ipv6_packet(&packet).unwrap();

        let child_ids = ACCEPT_QUEUE
            .lock()
            .get(&listener)
            .cloned()
            .unwrap_or_default();
        assert_eq!(child_ids.len(), 1);
        let child_id = child_ids[0];

        let conns = TCP_CONNECTIONS.lock();
        let child = conns.get(&child_id).unwrap();
        assert_eq!(child.family, AddressFamily::IPV6);
        assert_eq!(child.remote.ip, IpAddr::V6(src_ip));
        assert_eq!(child.remote.port, Port(41000));
        assert_eq!(child.state, TcpState::SynReceived);
    }
}
