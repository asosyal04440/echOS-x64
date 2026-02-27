//! # TCP Protokolü
//!
//! SACK desteği ile TCP durum makinesi ve bağlantı yönetimi.
//! TCP (Transmission Control Protocol), güvenilir, sıralı ve hata denetimli
//! veri iletimi sağlayan bağlantı yönelimli bir taşıma katmanı protokolüdür.

use super::{Ipv4Addr, Port, SocketAddr, NetError, allocate_socket_id};
use super::ip::{IpProtocol, Ipv4Packet};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use alloc::vec;
use alloc::boxed::Box;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;

// ============================================================================
// TCP SEÇENEKLERİ
// ============================================================================

/// TCP seçenek türleri - RFC 793 ve uzantılarında tanımlanan başlık seçenekleri
pub const TCPOPT_EOL: u8 = 0;
pub const TCPOPT_NOP: u8 = 1;
pub const TCPOPT_MSS: u8 = 2;
pub const TCPOPT_WINDOW_SCALE: u8 = 3;
pub const TCPOPT_SACK_PERMITTED: u8 = 4;
pub const TCPOPT_SACK: u8 = 5;
pub const TCPOPT_TIMESTAMP: u8 = 8;

// ============================================================================
// TCP SACK (Seçici Onaylama - Selective Acknowledgment)
// ============================================================================

/// SACK bloğu: [başlangıç, son) sıra numarası aralığı.
/// SACK, alıcının sadece eksik segmentlerin yeniden iletilmesini talep
/// etmesine olanak tanıyarak ağ verimliliğini artırır (RFC 2018).
#[derive(Clone, Copy, Debug, Default)]
pub struct SackBlock {
    pub start: u32,
    pub end: u32,
}

impl SackBlock {
    pub fn new(start: u32, end: u32) -> Self {
        SackBlock { start, end }
    }
    
    /// Verilen sıra numarasının bu SACK bloğu içinde olup olmadığını kontrol eder
    pub fn contains(&self, seq: u32) -> bool {
        // Sıra numarası taşması (wraparound) durumunu ele al
        if self.start <= self.end {
            seq >= self.start && seq < self.end
        } else {
            seq >= self.start || seq < self.end
        }
    }
    
    /// Bloğun bayt cinsinden uzunluğunu döndürür
    pub fn len(&self) -> u32 {
        self.end.wrapping_sub(self.start)
    }
}

/// SACK puan tablosu - seçici yeniden iletim için alınan segmentleri izler.
/// Puan tablosu, hangi veri bloklarının alındığını takip ederek
/// yalnızca kayıp olan segmentlerin yeniden gönderilmesini sağlar.
#[derive(Clone, Debug)]
pub struct SackScoreboard {
    /// Alınan SACK blokları (RFC 2018'e göre maksimum 4 blok)
    pub blocks: Vec<SackBlock>,
    /// Saklanabilecek maksimum blok sayısı
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
    
    /// Yeni bir SACK bloğu ekler; çakışan blokları birleştirir.
    /// RFC 2018: Alıcı en fazla 4 SACK bloğu bildirebilir.
    pub fn add_block(&mut self, block: SackBlock) {
        // Mevcut bloklarla çakışmayı kontrol et ve gerekirse birleştir
        let mut merged = false;
        for existing in &mut self.blocks {
            // Blokların çakıştığını veya bitişik olduğunu kontrol et
            if Self::blocks_overlap_or_adjacent(existing, &block) {
                // Birleştir: mevcut bloğu genişlet
                existing.start = existing.start.min(block.start);
                existing.end = existing.end.max(block.end);
                merged = true;
                break;
            }
        }
        
        if !merged {
            // Yer varsa yeni bloğu ekle
            if self.blocks.len() < self.max_blocks {
                self.blocks.push(block);
            } else {
                // En eski bloğu sil ve yeni bloğu ekle
                self.blocks.remove(0);
                self.blocks.push(block);
            }
        }
        
        // Blokları başlangıç sıra numarasına göre sırala
        self.blocks.sort_by_key(|b| b.start);
        
        // high_sack değerini güncelle
        for block in &self.blocks {
            if block.end.wrapping_sub(self.high_sack) as i32 > 0 {
                self.high_sack = block.end;
            }
        }
        
        // SACK'lı bayt sayısını yeniden hesapla
        self.sacked_bytes = self.blocks.iter().map(|b| b.len()).sum();
    }
    
    /// İki bloğun çakıştığını veya bitişik olduğunu kontrol eder.
    /// Sıra numarası taşması (32-bit döngüsellik) göz önünde bulundurulur.
    fn blocks_overlap_or_adjacent(a: &SackBlock, b: &SackBlock) -> bool {
        // Sıra numarası taşmasını ele al
        let a_before_b = a.end.wrapping_sub(b.start) as i32 >= 0;
        let b_before_a = b.end.wrapping_sub(a.start) as i32 >= 0;
        
        // Bitişik: a.end == b.start veya b.end == a.start
        let adjacent = a.end.wrapping_sub(b.start) == 0 || b.end.wrapping_sub(a.start) == 0;
        
        // Çakışma: aralıklar kesişiyor
        let overlap = (a.start <= b.start && a.end > b.start) ||
                      (b.start <= a.start && b.end > a.start);
        
        overlap || adjacent
    }
    
    /// Verilen sıra numarası aralığının SACK blokları tarafından kapsanıp kapsanmadığını kontrol eder
    pub fn is_sacked(&self, start: u32, end: u32) -> bool {
        for block in &self.blocks {
            if block.start <= start && block.end >= end {
                return true;
            }
        }
        false
    }
    
    /// Alınan verideki boşlukları döndürür (yeniden iletim için).
    /// SND.UNA ile SND.NXT arasındaki onaylanmamış aralıklarda
    /// SACK bloklarının kapsamadığı bölgeler yeniden iletilmelidir.
    pub fn get_gaps(&self, snd_una: u32, snd_nxt: u32) -> Vec<SackBlock> {
        let mut gaps = Vec::new();
        
        if self.blocks.is_empty() {
            // SACK bilgisi yok, tüm pencere boşluk olarak değerlendiriliyor
            gaps.push(SackBlock::new(snd_una, snd_nxt));
            return gaps;
        }
        
        // SND.UNA noktasından başla
        let mut current = snd_una;
        
        for block in &self.blocks {
            if current < block.start {
                // current ile block.start arasındaki boşluk
                gaps.push(SackBlock::new(current, block.start));
            }
            current = current.max(block.end);
        }
        
        // Son bloktan SND.NXT'ye kadar olan boşluk
        if current < snd_nxt {
            gaps.push(SackBlock::new(current, snd_nxt));
        }
        
        gaps
    }
    
    /// Puan tablosunu sıfırlar
    pub fn clear(&mut self) {
        self.blocks.clear();
        self.high_sack = 0;
        self.sacked_bytes = 0;
    }
    
    /// SACK bloklarını TCP seçenek formatına serileştirir.
    /// Her blok 8 bayt: 4 bayt başlangıç + 4 bayt bitiş sıra numarası.
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::new();
        
        // Seçenek türü
        data.push(TCPOPT_SACK);
        // Uzunluk: 2 (başlık) + 8 * blok_sayısı
        let len = 2 + 8 * self.blocks.len();
        data.push(len as u8);
        
        // Her blok: 4 bayt başlangıç + 4 bayt bitiş
        for block in &self.blocks {
            data.extend_from_slice(&block.start.to_be_bytes());
            data.extend_from_slice(&block.end.to_be_bytes());
        }
        
        // 4 baytlık hizaya kadar doldur
        while data.len() % 4 != 0 {
            data.push(TCPOPT_NOP);
        }
        
        data
    }
    
    /// TCP seçenek verisinden SACK bloklarını ayrıştırır
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
            
            let start = u32::from_be_bytes([data[offset], data[offset+1], data[offset+2], data[offset+3]]);
            let end = u32::from_be_bytes([data[offset+4], data[offset+5], data[offset+6], data[offset+7]]);
            
            scoreboard.add_block(SackBlock::new(start, end));
        }
        
        Some(scoreboard)
    }
}

/// SYN paketlerinde SACK kullanımına izin verildiğini bildiren seçenek.
/// Bağlantı kurulumu sırasında her iki taraf da bu seçeneği göndererek
/// SACK özelliğini müzakere eder.
#[derive(Clone, Copy, Debug)]
pub struct SackPermitted;

impl SackPermitted {
    pub fn serialize() -> [u8; 2] {
        [TCPOPT_SACK_PERMITTED, 2]
    }
}

// ============================================================================
// TCP HİZLI YENİDEN İLETİM (FAST RETRANSMIT)
// ============================================================================

/// Hızlı yeniden iletim durumu.
/// Üç veya daha fazla yinelenen ACK alındığında zaman aşımı beklenmeden
/// kayıp segment hemen yeniden iletilir (RFC 5681).
#[derive(Clone, Debug)]
pub struct FastRetransmitState {
    /// Yinelenen ACK sayıcısı
    pub dup_ack_count: u32,
    /// Son alınan ACK numarası
    pub last_ack: u32,
    /// Son hızlı yeniden iletimde ki sıra numarası (kurtarma noktası)
    pub recover: u32,
    /// Hızlı kurtarma devam ediyor mu?
    pub in_recovery: bool,
    /// Hızlı yeniden iletim eşiği (genellikle 3)
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
    
    /// Gelen ACK'yi işler; hızlı yeniden iletim gerekiyorsa true döndürür.
    /// Yinelenen ACK: aynı ACK numarasının tekrarı = segment kaybına işaret eder.
    pub fn on_ack(&mut self, ack: u32, sack_blocks: &[SackBlock], snd_una: u32) -> bool {
        if ack == self.last_ack && ack != snd_una {
            // Yinelenen ACK
            // Yeni SACK bilgisi taşıyıp taşımadığını kontrol et
            let has_new_sack = !sack_blocks.is_empty();
            
            self.dup_ack_count += 1;
            
            // Yinelenen ACK sayısı eşiğe ulaşınca hızlı yeniden iletim başlat
            if self.dup_ack_count >= self.threshold && !self.in_recovery {
                self.in_recovery = true;
                self.recover = snd_una;
                return true;
            }
        } else if ack > self.last_ack {
            // Yeni ACK - yineleme sayıcısını sıfırla
            self.dup_ack_count = 0;
            self.last_ack = ack;
            
            // ACK kurtarma noktasını geçiyorsa hızlı kurtarmadan çık
            if self.in_recovery && ack >= self.recover {
                self.in_recovery = false;
            }
        }
        
        false
    }
    
    /// Durumu sıfırlar
    pub fn reset(&mut self) {
        self.dup_ack_count = 0;
        self.in_recovery = false;
    }
}

// ============================================================================
// TCP HİZLI AÇILMA (TFO - TCP Fast Open)
// ============================================================================

/// TFO Çerezi (8 bayt).
/// TCP Fast Open, üçlü el sıkışması tamamlanmadan SYN paketi içinde
/// veri gönderilmesine olanak tanıyarak bağlantı kurulum gecikmesini azaltır.
#[derive(Clone, Copy, Debug, Default)]
pub struct TfoCookie(pub [u8; 8]);

impl TfoCookie {
    pub fn new() -> Self {
        let mut cookie = [0u8; 8];
        for i in 0..8 {
            cookie[i] = crate::random::next_u32() as u8;
        }
        TfoCookie(cookie)
    }
    
    pub fn generate(server_ip: Ipv4Addr, time_ms: u64) -> Self {
        // Basit çerez: IP + zaman damgası özeti
        let mut cookie = [0u8; 8];
        cookie[..4].copy_from_slice(&server_ip.0);
        cookie[4..8].copy_from_slice(&(time_ms as u32).to_be_bytes());
        
        // Rastgele değerle XOR ile güvenlik katmanı ekle
        let rand = crate::random::next_u32();
        for i in 0..4 {
            cookie[i] ^= (rand >> (i * 8)) as u8;
        }
        
        TfoCookie(cookie)
    }
    
    pub fn verify(&self, server_ip: Ipv4Addr, time_window: u64) -> bool {
        // Basitleştirilmiş doğrulama - üretim ortamında kriptografik yöntemler kullanılırdı
        let expected = TfoCookie::generate(server_ip, time_window);
        self.0 == expected.0
    }
}

/// Bağlantı için TFO durumu
#[derive(Clone, Debug)]
pub struct TfoState {
    /// Sunucu IP adresine göre saklı çerezler
    pub cookies: BTreeMap<u32, TfoCookie>,
    /// SYN ile gönderilecek bekleyen veri
    pub pending_data: Vec<u8>,
    /// TFO etkin mi
    pub enabled: bool,
    /// TFO çerez isteği gönderildi mi
    pub cookie_requested: bool,
    /// SYN içinde veri gönderildi mi
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
    
    /// Sunucuya ait çerezi getirir
    pub fn get_cookie(&self, server_ip: Ipv4Addr) -> Option<TfoCookie> {
        let ip_key = u32::from_be_bytes(server_ip.0);
        self.cookies.get(&ip_key).copied()
    }
    
    /// Sunucudan gelen çerezi saklar
    pub fn store_cookie(&mut self, server_ip: Ipv4Addr, cookie: TfoCookie) {
        let ip_key = u32::from_be_bytes(server_ip.0);
        self.cookies.insert(ip_key, cookie);
    }
    
    /// TFO çerez seçeneğini serileştirir
    pub fn serialize_cookie_option(cookie: &TfoCookie) -> Vec<u8> {
        let mut data = Vec::new();
        data.push(TCPOPT_FAST_OPEN); // 34 - TFO seçenek kodu
        data.push(10); // Uzunluk: 2 + 8
        data.extend_from_slice(&cookie.0);
        data
    }
}

/// TFO seçenek kodu (IANA tarafından tahsis edilmiş)
pub const TCPOPT_FAST_OPEN: u8 = 34;

// ============================================================================
// TCP PENCERE ÖLÇEKLENDİRME (WINDOW SCALING)
// ============================================================================

/// Pencere ölçeklendirme seçeneği.
/// RFC 7323: TCP başlığındaki 16-bit pencere alanını genişleterek
/// 1 GB'a kadar alıcı penceresi bildirmeye olanak tanır.
#[derive(Clone, Copy, Debug)]
pub struct WindowScaleOption {
    pub scale: u8,
}

impl WindowScaleOption {
    pub fn new(scale: u8) -> Self {
        WindowScaleOption { scale: scale.min(14) }
    }
    
    /// Pencere ölçeklendirme seçeneğini serileştirir
    pub fn serialize(&self) -> [u8; 3] {
        [TCPOPT_WINDOW_SCALE, 3, self.scale]
    }
    
    /// Seçenek verisinden pencere boyutu ölçeğini ayrıştırır
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 3 || data[0] != TCPOPT_WINDOW_SCALE {
            return None;
        }
        Some(WindowScaleOption { scale: data[2] })
    }
    
    /// Gerçek pencere boyutunu hesaplar: base_window << scale_factor
    pub fn effective_window(base: u16, scale: u8) -> u32 {
        (base as u32) << (scale as u32)
    }
}

// ============================================================================
// TCP ZAMAN DAMGALARI (TIMESTAMPS)
// ============================================================================

/// TCP Zaman Damgası seçeneği (RFC 7323).
/// RTT ölçümü ve PAWS (Eski Segmentlere Karşı Koruma) için kullanılır.
#[derive(Clone, Copy, Debug, Default)]
pub struct TimestampOption {
    pub ts_val: u32,  // Gönderenin zaman damgası değeri
    pub ts_ecr: u32,  // Zaman damgası yankısı (echo reply)
}

impl TimestampOption {
    pub fn new(ts_val: u32, ts_ecr: u32) -> Self {
        TimestampOption { ts_val, ts_ecr }
    }
    
    /// Geçerli zamanla zaman damgası oluşturur
    pub fn now(ts_ecr: u32) -> Self {
        // Şu an için rastgele sayi sözde-zaman damgası olarak kullanılıyor
        let ts_val = crate::random::next_u32();
        TimestampOption { ts_val, ts_ecr }
    }
    
    /// Zaman damgası seçeneğini baytlara serileştirir
    pub fn serialize(&self) -> [u8; 10] {
        let mut data = [0u8; 10];
        data[0] = TCPOPT_TIMESTAMP;
        data[1] = 10; // Uzunluk
        data[2..6].copy_from_slice(&self.ts_val.to_be_bytes());
        data[6..10].copy_from_slice(&self.ts_ecr.to_be_bytes());
        data
    }
    
    /// Seçenek verisinden zaman damgasını ayrıştırır
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 10 || data[0] != TCPOPT_TIMESTAMP {
            return None;
        }
        let ts_val = u32::from_be_bytes([data[2], data[3], data[4], data[5]]);
        let ts_ecr = u32::from_be_bytes([data[6], data[7], data[8], data[9]]);
        Some(TimestampOption { ts_val, ts_ecr })
    }
    
    /// Zaman damgasından RTT'yi hesaplar.
    /// Gönderim anının yankısı kullanılarak hassas RTT ölçümü yapılır.
    pub fn calculate_rtt(&self) -> u32 {
        // Basitleştirilmiş RTT hesaplaması
        let now = crate::random::next_u32();
        now.wrapping_sub(self.ts_ecr)
    }
}

// ============================================================================
// TCP SEÇENEKLERİ AYRICI (OPTIONS PARSER)
// ============================================================================

/// Ayrıştırılmış TCP seçenekleri
#[derive(Clone, Debug, Default)]
pub struct TcpOptions {
    pub mss: Option<u16>,
    pub window_scale: Option<WindowScaleOption>,
    pub sack_permitted: bool,
    pub sack_blocks: Vec<SackBlock>,
    pub timestamps: Option<TimestampOption>,
    pub tfo_cookie: Option<TfoCookie>,
}

impl TcpOptions {
    /// TCP başlığındaki seçenekleri ayrıştırır
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
                TCPOPT_EOL => break,
                TCPOPT_NOP => {
                    i += 1;
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
    
    /// SYN paketi için seçenek bytesı oluşturur.
    /// Bağlantı kurulurken MSS, pencere ölçeği, SACK ve TFO müzakere edilir.
    pub fn build_syn_options(mss: u16, ws_scale: u8, enable_sack: bool, enable_tfo: bool) -> Vec<u8> {
        let mut opts = Vec::new();
        
        // MSS seçeneği - Maksimum Segment Boyutu
        opts.push(TCPOPT_MSS);
        opts.push(4);
        opts.extend_from_slice(&mss.to_be_bytes());
        
        // Pencere ölçeği
        opts.extend_from_slice(&WindowScaleOption::new(ws_scale).serialize());
        
        // SACK destekleniyor
        if enable_sack {
            opts.extend_from_slice(&SackPermitted::serialize());
        }
        
        // Zaman damgaları
        let ts = TimestampOption::now(0);
        opts.extend_from_slice(&ts.serialize());
        
        // 4 baytlık hizaya kadar doldur
        while opts.len() % 4 != 0 {
            opts.push(TCPOPT_NOP);
        }
        
        opts
    }
    
    /// Veri paketi için seçenek bytesı oluşturur
    pub fn build_data_options(ts_echo: u32, sack_blocks: &[SackBlock]) -> Vec<u8> {
        let mut opts = Vec::new();
        
        // Zaman damgaları
        let ts = TimestampOption::now(ts_echo);
        opts.extend_from_slice(&ts.serialize());
        
        // Varsa SACK blokları
        if !sack_blocks.is_empty() {
            let scoreboard = SackScoreboard {
                blocks: sack_blocks.to_vec(),
                max_blocks: 4,
                high_sack: 0,
                sacked_bytes: 0,
            };
            opts.extend_from_slice(&scoreboard.serialize());
        }
        
        // 4 baytlık hizaya kadar doldur
        while opts.len() % 4 != 0 {
            opts.push(TCPOPT_NOP);
        }
        
        opts
    }
}

/// TCP başlığı (minimum 20 bayt).
/// Her TCP segmenti bu yapı ile başlar; kaynak/hedef port,
/// sıra/onay numaraları ve kontrol bayraklarını taşır.
#[derive(Clone, Copy, Debug)]
pub struct TcpHeader {
    pub src_port: Port,
    pub dst_port: Port,
    pub seq_num: u32,
    pub ack_num: u32,
    pub data_offset: u8,        // 4 bit; başlık uzunluğu 32-bit sözcük cinsinden
    pub flags: TcpFlags,
    pub window_size: u16,
    pub checksum: u16,
    pub urgent_ptr: u16,
}

/// TCP kontrol bayrakları.
/// Her bayrak bağlantı yönetiminde farklı anlam taşır:
/// SYN=bağlantı başlat, ACK=onayla, FIN=bitir, RST=sıfırla, PSH=hemen ilet, URG=acil.
#[derive(Clone, Copy, Debug, Default)]
pub struct TcpFlags {
    pub fin: bool,
    pub syn: bool,
    pub rst: bool,
    pub psh: bool,
    pub ack: bool,
    pub urg: bool,
}

impl TcpFlags {
    pub fn new() -> Self {
        TcpFlags::default()
    }
    
    pub fn syn() -> Self {
        TcpFlags { syn: true, ..Default::default() }
    }
    
    pub fn syn_ack() -> Self {
        TcpFlags { syn: true, ack: true, ..Default::default() }
    }
    
    pub fn ack() -> Self {
        TcpFlags { ack: true, ..Default::default() }
    }
    
    pub fn fin() -> Self {
        TcpFlags { fin: true, ..Default::default() }
    }
    
    pub fn fin_ack() -> Self {
        TcpFlags { fin: true, ack: true, ..Default::default() }
    }
    
    pub fn rst() -> Self {
        TcpFlags { rst: true, ..Default::default() }
    }
    
    pub fn to_u8(self) -> u8 {
        let mut val = 0u8;
        if self.fin { val |= 0x01; }
        if self.syn { val |= 0x02; }
        if self.rst { val |= 0x04; }
        if self.psh { val |= 0x08; }
        if self.ack { val |= 0x10; }
        if self.urg { val |= 0x20; }
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
    
    /// TCP başlığını baytlardan ayrıştırır
    pub fn parse(data: &[u8]) -> Result<Self, NetError> {
        if data.len() < Self::MIN_SIZE {
            return Err(NetError::InvalidPacket);
        }
        
        let src_port = Port(u16::from_be_bytes([data[0], data[1]]));
        let dst_port = Port(u16::from_be_bytes([data[2], data[3]]));
        let seq_num = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
        let ack_num = u32::from_be_bytes([data[8], data[9], data[10], data[11]]);
        let data_offset = (data[12] >> 4) & 0x0F;
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
    
    /// Başlığı baytlara serileştirir
    pub fn serialize(&self, buf: &mut [u8]) -> Result<(), NetError> {
        if buf.len() < Self::MIN_SIZE {
            return Err(NetError::BufferFull);
        }
        
        buf[0..2].copy_from_slice(&self.src_port.0.to_be_bytes());
        buf[2..4].copy_from_slice(&self.dst_port.0.to_be_bytes());
        buf[4..8].copy_from_slice(&self.seq_num.to_be_bytes());
        buf[8..12].copy_from_slice(&self.ack_num.to_be_bytes());
        buf[12] = (self.data_offset << 4) | 0x00; // Ayrılmış bitler (rezerved)
        buf[13] = self.flags.to_u8();
        buf[14..16].copy_from_slice(&self.window_size.to_be_bytes());
        buf[16..18].copy_from_slice(&self.checksum.to_be_bytes());
        buf[18..20].copy_from_slice(&self.urgent_ptr.to_be_bytes());
        
        Ok(())
    }
    
    /// Başlık uzunluğunu bayt cinsinden döndürür
    pub fn header_len(&self) -> usize {
        (self.data_offset as usize) * 4
    }
}

/// TCP sınamasını hesaplar.
/// TCP, doğrulamak için IP sözde-başlığı (sahte başlık) dahil
/// Internet checksum algoritmasını kullanır.
pub fn compute_checksum(src_ip: Ipv4Addr, dst_ip: Ipv4Addr, segment: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    
    // Sahte-başlık (pseudo-header): kaynak IP, hedef IP, protokol, uzunluk
    sum += u16::from_be_bytes([src_ip.0[0], src_ip.0[1]]) as u32;
    sum += u16::from_be_bytes([src_ip.0[2], src_ip.0[3]]) as u32;
    sum += u16::from_be_bytes([dst_ip.0[0], dst_ip.0[1]]) as u32;
    sum += u16::from_be_bytes([dst_ip.0[2], dst_ip.0[3]]) as u32;
    sum += 6u32; // TCP protokol numarası (RFC 793)
    sum += segment.len() as u32;
    
    // TCP segment verisi
    let mut i = 0;
    while i + 1 < segment.len() {
        sum += u16::from_be_bytes([segment[i], segment[i + 1]]) as u32;
        i += 2;
    }
    
    // Tek kalan bayt (eğer segment uzunluğu tek sayıysa)
    if i < segment.len() {
        sum += (segment[i] as u32) << 8;
    }
    
    // Elde bitlerini kat
    while (sum >> 16) != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    
    // Birlerin tamamlayıcısı
    !(sum as u16)
}

/// TCP sınamasını doğrular; hesaplanan değer sıfır ise geçerlidir
pub fn verify_checksum(src_ip: Ipv4Addr, dst_ip: Ipv4Addr, segment: &[u8]) -> bool {
    compute_checksum(src_ip, dst_ip, segment) == 0
}

/// TCP bağlantı durumu (RFC 793 durum makinesi).
/// Her durum, bağlantının yaşam döngüsündeki bir aşamayı temsil eder:
/// Closed → SynSent → Established → FinWait1 → TimeWait → Closed
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TcpState {
    Closed,
    Listen,
    SynSent,
    SynReceived,
    Established,
    FinWait1,
    FinWait2,
    CloseWait,
    Closing,
    LastAck,
    TimeWait,
}

/// TCP bağlantısı.
/// Her bağlantı, durum makinesi, tamponlar ve tıkanma kontrolcü durumunu içerir.
#[derive(Clone, Debug)]
pub struct TcpConnection {
    pub id: u32,
    pub local: SocketAddr,
    pub remote: SocketAddr,
    pub state: TcpState,
    pub seq_num: u32,
    pub ack_num: u32,
    pub window_size: u16,
    pub rx_buffer: Vec<u8>,
    pub tx_buffer: Vec<u8>,
    pub listen_backlog: usize,
    // Tıkanma kontrolcüsü
    pub cwnd: u32,           // Tıkanma penceresi (Congestion Window)
    pub ssthresh: u32,       // Yavaş başlama eşiği (Slow Start Threshold)
    pub rtt: u32,            // Gidiş-dönüş süresi - ms (Round-Trip Time)
    pub rtt_var: u32,        // RTT varyansı
    pub rto: u32,            // Yeniden iletim zaman aşımı - ms (Retransmission Timeout)
    pub retransmit_count: u8, // Yeniden iletim sayıcısı
    // Pencere ölçeklendirme
    pub ws_scale: u8,        // Pencere ölçek katsayısı
    pub peer_ws_scale: u8,   // Karşı tarafın pencere ölçeği
    // SACK desteği
    pub sack_permitted: bool,     // SACK müzakere edildi mi
    pub sack_scoreboard: SackScoreboard,  // Alınan SACK blokları
    pub rx_sack_blocks: Vec<SackBlock>,   // Gönderilecek SACK blokları
    // Hızlı yeniden iletim
    pub fast_retx: FastRetransmitState,
    // Gönderme durumu (RFC 793 değişkenleri)
    pub snd_una: u32,        // En eski onaylanmamış sıra numarası (SND.UNA)
    pub snd_nxt: u32,        // Gönderilecek sıradaki sıra numarası (SND.NXT)
    pub snd_wnd: u32,        // Gönderme penceresi (SND.WND)
    // Zaman damgaları
    pub ts_recent: u32,      // Karşı taraftan gelen son zaman damgası
    pub ts_echo: u32,        // Yankılanacak zaman damgası
    pub ts_val: u32,         // Kendi zaman damgası değerimiz
}

impl TcpConnection {
    pub fn new(local: SocketAddr) -> Self {
        TcpConnection {
            id: allocate_socket_id(),
            local,
            remote: SocketAddr::default(),
            state: TcpState::Closed,
            seq_num: 0,
            ack_num: 0,
            window_size: 65535,
            rx_buffer: Vec::new(),
            tx_buffer: Vec::new(),
            listen_backlog: 0,
            // Tıkanma kontrolcüsü varsayılanları
            cwnd: 10 * 1460,        // Başlangıç penceresi (10 MSS)
            ssthresh: 65535,        // Yüksek başlangıç eşiği
            rtt: 100,               // Başlangıç RTT tahmini (100ms)
            rtt_var: 50,            // Başlangıç RTT varyansı
            rto: 200,               // Başlangıç RTO (200ms)
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
        }
    }
    
    pub fn connect(&mut self, remote: SocketAddr) -> Result<(), NetError> {
        if self.state != TcpState::Closed {
            return Err(NetError::ProtocolError);
        }
        
        self.remote = remote;
        self.seq_num = crate::random::rand_u64() as u32;
        self.state = TcpState::SynSent;
        
        // Üçlü el sıkışmasını başlatmak için SYN gönder
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
        
        // Bekleyen bağlantıları kontrol et
        // TODO: Kabul kuyruğunu (accept queue) uygula
        
        Err(NetError::WouldBlock)
    }
    
    pub fn send(&mut self, data: &[u8]) -> Result<usize, NetError> {
        if self.state != TcpState::Established {
            return Err(NetError::ConnectionClosed);
        }
        
        self.send_packet(TcpFlags::ack(), data)?;
        self.seq_num = self.seq_num.wrapping_add(data.len() as u32);
        
        Ok(data.len())
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
        let mut header = TcpHeader {
            src_port: self.local.port,
            dst_port: self.remote.port,
            seq_num: self.seq_num,
            ack_num: self.ack_num,
            data_offset: 5,
            flags,
            window_size: self.window_size,
            checksum: 0,
            urgent_ptr: 0,
        };
        
        // TCP segmentini oluştur
        let mut segment = vec![0u8; TcpHeader::MIN_SIZE + data.len()];
        header.serialize(&mut segment)?;
        segment[TcpHeader::MIN_SIZE..].copy_from_slice(data);
        
        // Sahte başlıkla sınama hesapla
        let src_ip = super::local_ip();
        header.checksum = compute_checksum(src_ip, self.remote.ip, &segment);
        header.serialize(&mut segment)?;
        
        // IP katmanı üzerinden gönder
        let mut ip_buf = vec![0u8; 1500];
        let len = super::ip::build_packet(
            self.remote.ip,
            IpProtocol::TCP,
            &segment,
            &mut ip_buf,
        )?;
        
        // Ethernet çerçevesini oluştur ve gönder
        super::send_packet(&ip_buf[..len])?;
        
        Ok(())
    }
    
    fn on_packet(&mut self, header: &TcpHeader, data: &[u8]) -> Result<(), NetError> {
        // ACK numarasını güncelle
        if header.flags.ack {
            self.ack_num = header.ack_num;
        }
        
        // Durum makinesi: gelen pakete göre durum geçişi yap
            TcpState::Listen => {
                if header.flags.syn {
                    // Yeni bağlantı girişimi
                    self.remote = SocketAddr::new(
                        // IP, IP katmanından gelecek
                        Ipv4Addr::UNSPECIFIED,
                        header.src_port,
                    );
                    self.seq_num = crate::random::rand_u64() as u32;
                    self.ack_num = header.seq_num.wrapping_add(1);
                    self.state = TcpState::SynReceived;
                    
                    // SYN-ACK gönder
                    self.send_packet(TcpFlags::syn_ack(), &[])?;
                }
            }
            TcpState::SynSent => {
                if header.flags.syn && header.flags.ack {
                    // SYN-ACK alındı - üçlü el sıkışmasının ikinci adımı
                    self.ack_num = header.seq_num.wrapping_add(1);
                    self.state = TcpState::Established;
                    
                    // ACK gönder - üçlü el sıkışması tamamlandı
                    self.send_packet(TcpFlags::ack(), &[])?;
                }
            }
            TcpState::SynReceived => {
                if header.flags.ack {
                    // Bağlantı kuruldu
                    self.state = TcpState::Established;
                }
            }
            TcpState::Established => {
                // Veri al
                if !data.is_empty() {
                    self.rx_buffer.extend_from_slice(data);
                    self.ack_num = self.ack_num.wrapping_add(data.len() as u32);
                    
                    // ACK gönder
                    self.send_packet(TcpFlags::ack(), &[])?;
                }
                
                // FIN alındı - karşı taraf bağlantıyı kapatmak istiyor
                if header.flags.fin {
                    self.state = TcpState::CloseWait;
                    self.ack_num = self.ack_num.wrapping_add(1);
                    self.send_packet(TcpFlags::ack(), &[])?;
                }
            }
            TcpState::FinWait1 => {
                if header.flags.ack {
                    self.state = TcpState::FinWait2;
                }
            }
            TcpState::FinWait2 => {
                if header.flags.fin {
                    self.ack_num = self.ack_num.wrapping_add(1);
                    self.send_packet(TcpFlags::ack(), &[])?;
                    self.state = TcpState::TimeWait;
                }
            }
            TcpState::LastAck => {
                if header.flags.ack {
                    self.state = TcpState::Closed;
                }
            }
            _ => {}
        }
        
        Ok(())
    }
    
    /// RTT tahminini günceller (Jacobson/Karels algoritması).
    /// Bu algoritma, kayan ortalama ile RTT varyansını takip ederek
    /// RTO'yu (Yeniden İletim Zaman Aşımı) dinamik olarak ayarlar.
    pub fn update_rtt(&mut self, measured_rtt: u32) {
        let delta = if measured_rtt > self.rtt {
            measured_rtt - self.rtt
        } else {
            self.rtt - measured_rtt
        };
        
        self.rtt_var = (3 * self.rtt_var + delta) / 4;
        self.rtt = (7 * self.rtt + measured_rtt) / 8;
        self.rto = self.rtt + 4 * self.rtt_var;
        
        // RTO'yu sınırlandır: çok küçük (<200ms) veya çok büyük (>60s) olmasını önle
        if self.rto < 200 { self.rto = 200; }
        if self.rto > 60000 { self.rto = 60000; }
    }
}

// ============================================================================
// TCP CUBIC TIKANMA KONTROLÜ
// ============================================================================

/// CUBIC tıkanma kontrol durumu.
/// CUBIC, Linux'un varsayılan TCP tıkanma kontrol algoritmasıdır.
/// Küpsel bir fonksiyon kullanarak bant genişliğine hızlı erişir,
/// ağ dostu davranış için TCP-dostu mod da içerir (RFC 8312).
#[derive(Clone, Debug)]
pub struct CubicState {
    /// CUBIC beta sabiti = 0.7 (beta_cubic) - çoğaltmalı azaltma oranı
    pub beta: f64,
    /// CUBIC C sabiti = 0.4 - pencere büyüme katsayısı
    pub c: f64,
    /// Son tıkanma olayından önce ulaşılan maksimum pencere boyutu (W_max)
    pub w_max: f64,
    /// Son tıkanma olayının zamanı (ms cinsinden)
    pub t_last: u64,
    /// Bayt cinsinden mevcut tıkanma penceresi (cwnd)
    pub cwnd: u32,
    /// Yavaş başlama eşiği
    pub ssthresh: u32,
    /// Gözlenen minimum RTT
    pub min_rtt: u32,
    /// TCP-dostu pencere tahmini
    pub tcp_cwnd: f64,
}

impl CubicState {
    /// CUBIC beta = 0.7 (çoğaltmalı azaltma katsayısı; Reno'nun 0.5'ine göre daha yumuşak)
    const BETA: f64 = 0.7;
    /// CUBIC C = 0.4 (pencere büyüme hızını belirleyen sabit)
    const C: f64 = 0.4;
    /// Hesaplamalar için MSS (Maksimum Segment Boyutu)
    const MSS: u32 = 1460;
    
    pub fn new() -> Self {
        CubicState {
            beta: Self::BETA,
            c: Self::C,
            w_max: 0.0,
            t_last: 0,
            cwnd: Self::MSS * 10, // Başlangıç penceresi: 10 MSS
            ssthresh: Self::MSS * 100,
            min_rtt: 1000,
            tcp_cwnd: Self::MSS as f64 * 10.0,
        }
    }
    
    /// Küp kökü yaklaşımı (Newton-Raphson yöntemi).
    /// CUBIC pencere hesaplamasında K = cbrt(W_max * beta / C) için kullanılır.
    fn cbrt(x: f64) -> f64 {
        if x <= 0.0 {
            return 0.0;
        }
        // Başlangıç tahminini bit manipülasyonu ile hesapla
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
    
    /// f64 için tam sayı üsseli kuvvet hesaplaması
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
    
    /// T anında CUBIC pencere boyutunu hesaplar:
    /// W(t) = C*(t-K)^3 + W_max
    /// burada K = cubic_root(W_max*beta/C)
    /// T anı, son tıkanma olayından bu yana geçen süredir.
    pub fn cubic_window(&self, t_ms: u64) -> f64 {
        if self.w_max == 0.0 {
            return self.cwnd as f64;
        }
        
        // K = cubic_root(W_max * beta / C)
        let k = Self::cbrt(self.w_max * self.beta / self.c);
        
        // Son tıkanma olayından bu yana geçen süre
        let t = (t_ms as f64 - self.t_last as f64) / 1000.0; // Saniyeye çevir
        
        // W(t) = C * (t - K)^3 + W_max
        let w = self.c * Self::powi(t - k, 3) + self.w_max;
        
        w.max(Self::MSS as f64)
    }
    
    /// TCP-dostu pencere boyutunu hesaplar (TCP arkadaşlığı için):
    /// W_tcp(t) = W_max * (1 - beta) + 3 * beta / (2 - beta) * (t / RTT)
    /// Bu formül, CUBIC'in klasik TCP Reno ile aynı hızda büyümesini sağlar.
    pub fn tcp_friendly_window(&self, t_ms: u64, rtt_ms: u32) -> f64 {
        let t = (t_ms as f64 - self.t_last as f64) / 1000.0;
        let rtt = rtt_ms as f64 / 1000.0;
        
        if rtt == 0.0 {
            return self.tcp_cwnd;
        }
        
        // TCP-dostu artış oranı
        let alpha = 3.0 * self.beta / (2.0 - self.beta);
        let w_tcp = self.w_max * (1.0 - self.beta) + alpha * t / rtt;
        
        w_tcp.max(Self::MSS as f64)
    }
    
    /// ACK alındığında cwnd'yi günceller (CUBIC algoritması).
    /// Yavaş başlama aflamasında üssel, tıkanma kaçınma aflamasında küpsel büyüme.
    pub fn on_ack(&mut self, acked_bytes: u32, current_time_ms: u64, rtt_ms: u32) {
        // Minimum RTT'yi güncelle
        if rtt_ms < self.min_rtt && rtt_ms > 0 {
            self.min_rtt = rtt_ms;
        }
        
        // Yavaş başlama aflaması: her ACK'de bir MSS ekle
        if self.cwnd < self.ssthresh {
            self.cwnd += acked_bytes;
            return;
        }
        
        // CUBIC tıkanma kaçınma aflaması
        let cubic_w = self.cubic_window(current_time_ms);
        let tcp_w = self.tcp_friendly_window(current_time_ms, rtt_ms);
        
        // CUBIC ve TCP-dostu değerlerin maksimumunu al
        let target_w = cubic_w.max(tcp_w);
        
        // Bayta çevir ve güncelle
        let current_w = self.cwnd as f64;
        let new_w = current_w + (target_w - current_w) * (acked_bytes as f64 / self.cwnd as f64);
        self.cwnd = new_w.max(Self::MSS as f64) as u32;
    }
    
    /// Tıkanma olayını (paket kaybı) ele alır.
    /// CUBIC, Reno'dan daha yumuşak bir azaltma uygular (beta=0.7 vs 0.5).
    pub fn on_loss(&mut self, current_time_ms: u64) {
        // Mevcut pencereyi W_max olarak kaydet
        self.w_max = self.cwnd as f64;
        
        // Çoğaltmalı azaltma: W_max = W_max * beta
        self.w_max *= self.beta;
        
        // Yeni cwnd'yi ayarla
        self.cwnd = (self.cwnd as f64 * self.beta).max(Self::MSS as f64) as u32;
        self.ssthresh = self.cwnd;
        
        // Tıkanma olayı zamanını kaydet
        self.t_last = current_time_ms;
    }
    
    /// Zaman aşımını ele alır (Retransmission Timeout - RTO).
    /// TCP Tahoe davranışı: Zaman aşımında pencere 1 MSS'e sıfırlanır.
    pub fn on_timeout(&mut self, current_time_ms: u64) {
        // Mevcut pencereyi kaydet
        self.w_max = self.cwnd as f64;
        
        // 1 MSS'e sıfırla
        self.cwnd = Self::MSS;
        self.ssthresh = Self::MSS * 2;
        
        // Zamanı kaydet
        self.t_last = current_time_ms;
    }
}

// ============================================================================
// TCP BBR TIKANMA KONTROLÜ
// ============================================================================

/// BBR tıkanma kontrolcüsü durumu.
/// BBR (Bottleneck Bandwidth and Round-trip propagation time),
/// Google tarafından geliştirilmiş, bant genişliği ve minimum
/// RTT ölçümüne dayanan model-tabanlı bir tıkanma kontrol algoritmasıdır.
#[derive(Clone, Debug, Default)]
pub struct BbrState {
    /// BBR çalışma modu
    pub mode: BbrMode,
    /// Tahmini bant genişliği (bayt/saniye)
    pub bw: u64,
    /// Gözlenen minimum RTT (mikrosaniye cinsinden)
    pub min_rtt: u64,
    /// Gidiş-dönüş yayılım süresi sayacı (RTprop)
    pub rtprop: u64,
    /// Son RTprop güncellemesinin zamanı
    pub rtprop_stamp: u64,
    /// Veri gönderim hızı (bayt/saniye)
    pub pacing_rate: u64,
    /// Gönderim kuantası (bayt)
    pub send_quantum: u32,
    /// Cwnd kazanç katsayısı
    pub cwnd_gain: f64,
    /// Pacing (hız düzeltme) kazanç katsayısı
    pub pacing_gain: f64,
    /// BBR tur sayıcısı
    pub round_count: u64,
    /// Sonraki turun başlangıç sınırı
    pub next_round_delivered: u64,
    /// Bant genişliği filtresi
    pub bw_filter: BbrBwFilter,
    /// RTT filtresi
    pub rtt_filter: BbrRttFilter,
    /// ProbeRTT tamamlandı mı
    pub probe_rtt_done: bool,
    /// ProbeRTT tur damgası
    pub probe_rtt_round_stamp: u64,
}

/// BBR çalışma modları.
/// Startup: Ağ kapasitesini keşfet. Drain: Kuyruğu boşalt.
/// ProbeBW: Bant genişliğini sürekli ölç. ProbeRTT: Minimum RTT'yi güncelle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BbrMode {
    #[default]
    Startup,
    Drain,
    ProbeBW,
    ProbeRTT,
}

/// BBR bant genişliği filtresi (son 10 turda maksimum bant genişliği).
/// Geçici düşüşleri eleyerek gerçek bant genişliği tahmini yapar.
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

/// BBR RTT filtresi (10 saniyelik pencerede minimum RTT).
/// PAWS benzeri mekanizma ile eski RTT ölçümlerini temizler.
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
        // 10 saniyelik zaman penceresi
        if now - self.stamp > 10_000_000 {
            self.min_rtt = rtt;
            self.stamp = now;
        } else if rtt < self.min_rtt {
            self.min_rtt = rtt;
        }
    }
}

impl BbrState {
    /// BBR sabitleri
    const BBR_HIGH_GAIN: f64 = 2.89;      // 2/ln(2) - başlangıç aflaması için yüksek kazanç
    const BBR_DRAIN_GAIN: f64 = 0.35;     // 1/2.89 - boşaltma aflaması için düşük kazanç
    const BBR_CWND_GAIN_TARGET: f64 = 2.0;
    const BBR_PROBE_RTT_CWND_GAIN: f64 = 0.5;
    const BBR_PROBE_RTT_MODE_DURATION_MS: u64 = 200;
    const BBR_MIN_RTT_WIN_SEC: u64 = 10;
    
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
        }
    }
    
    /// Pacing hızını hesaplar ve ayarlar.
    /// Pacing, veri gönderimini düzgün dağıtır; ani burst'ler yerine
    /// sürekli akış sağlar.
    pub fn set_pacing_rate(&mut self) {
        let bw = self.bw as f64;
        let gain = self.pacing_gain;
        self.pacing_rate = (bw * gain) as u64;
    }
    
    /// Gönderim kuantasını hesaplar
    pub fn set_send_quantum(&mut self) {
        // Gönderim kuantası = min(64KB, pacing_rate / 1000)
        let quantum = (self.pacing_rate / 1000).min(65536) as u32;
        self.send_quantum = quantum.max(1460);
    }
    
    /// Hedef cwnd boyutunu hesaplar.
    /// BDP (Bant Genişliği-Gecikme Ürünü) = bant_genişliği * min_RTT
    pub fn target_cwnd(&self) -> u32 {
        // BDP = bw * min_rtt (bayt cinsinden)
        let bdp = if self.min_rtt > 0 {
            (self.bw * self.min_rtt / 1_000_000) as u32 // Mikrosaniyeden milisaniyeye çevir
        } else {
            1460
        };
        
        // hedef_cwnd = cwnd_gain * BDP
        let target = (self.cwnd_gain * bdp as f64) as u32;
        target.max(1460)
    }
    
    /// ACK işleme sırasında BBR durumunu günceller.
    /// Bant genişliği ve RTT ölçümleri sürekli güncellenir.
    pub fn on_ack(&mut self, delivered: u32, rtt_us: u64, now_us: u64) {
        // Bant genişliği tahminini güncelle
        if rtt_us > 0 {
            let bw_sample = (delivered as u64 * 1_000_000) / rtt_us;
            self.bw_filter.update(bw_sample);
            self.bw = self.bw_filter.max();
        }
        
        // RTT tahminini güncelle
        self.rtt_filter.update(rtt_us, now_us);
        self.min_rtt = self.rtt_filter.min_rtt;
        
        // Tur geçişini kontrol et
        self.round_count += 1;
        
        // Moda özgü işleme
        match self.mode {
            BbrMode::Startup => {
                // Darboğazın bulunup bulunmadığını kontrol et
                if self.is_full_bw_reached() {
                    self.mode = BbrMode::Drain;
                    self.pacing_gain = Self::BBR_DRAIN_GAIN;
                    self.cwnd_gain = Self::BBR_CWND_GAIN_TARGET;
                }
            }
            BbrMode::Drain => {
                // Kuyruğun boşalmasını bekle
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
                    0 => 1.25,
                    1 => 0.75,
                    _ => 1.0,
                };
                
                // RTT yoklaması gerekiyor mu kontrol et
                if now_us - self.rtprop_stamp > Self::BBR_MIN_RTT_WIN_SEC * 1_000_000 {
                    self.mode = BbrMode::ProbeRTT;
                    self.cwnd_gain = Self::BBR_PROBE_RTT_CWND_GAIN;
                }
            }
            BbrMode::ProbeRTT => {
                // 200ms boyunca ProbeRTT modunda kal
                if now_us - self.rtprop_stamp > Self::BBR_PROBE_RTT_MODE_DURATION_MS * 1000 {
                    self.mode = BbrMode::ProbeBW;
                    self.cwnd_gain = Self::BBR_CWND_GAIN_TARGET;
                }
            }
        }
        
        // Pacing hızını güncelle
        self.set_pacing_rate();
        self.set_send_quantum();
    }
    
    /// Tam bant genişliğine ulaşılıp ulaşılmadığını kontrol eder (startup aflaması).
    /// Gerçek BBR'de bant genişliği büyüme oranı izlenir.
    fn is_full_bw_reached(&self) -> bool {
        // Basitleştirilmiş: bant genişliği önemli ölçüde artmadıysa durdur
        // Gerçek BBR'de bant genişliği büyüme oranı takip edilir
        self.bw > 0 && self.round_count > 3
    }
    
    /// Paket kaybını ele alır.
    /// BBR, kayba doğrudan tepki vermez; bant genişliği ölçümüne güvenir.
    /// Bu, kayıp tabanlı tıkanma kontrolclerinden temel farkıdır.
    pub fn on_loss(&mut self) {
        // BBR kayıba doğrudan tepki vermez
        // Bant genişliği tahminine güvenir
    }
    
    /// Zaman aşımını ele alır.
    /// BBR zaman aşımında başlangıç moduna (Startup) geri döner.
    pub fn on_timeout(&mut self, now_us: u64) {
        // Başlangıç moduna sıfırla
        self.mode = BbrMode::Startup;
        self.cwnd_gain = Self::BBR_HIGH_GAIN;
        self.pacing_gain = Self::BBR_HIGH_GAIN;
        self.rtprop_stamp = now_us;
    }
    
    /// Mevcut tıkanma penceresini döndürür
    pub fn cwnd(&self) -> u32 {
        match self.mode {
            BbrMode::ProbeRTT => 1460, // ProbeRTT sırasında minimum 4 segment
            _ => self.target_cwnd(),
        }
    }
}

// ============================================================================
// TIKANMA KONTROLCÜŞÜ ALGORİTMA SEÇİMİ
// ============================================================================

/// Tıkanma kontrolcü algoritması seçimi.
/// Reno: Klasik TCP tıkanma kontrolcüsü (RFC 5681)
/// Cubic: Linux varsayılanı, yüksek hızlı ağlarda verimli (RFC 8312)
/// BBR: Google'dan model-tabanlı, bant genişliği ölçümü odaklı
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CcAlgorithm {
    #[default]
    Reno,
    Cubic,
    Bbr,
}

/// Tıkanma kontrolcü durumu - seçilen algoritmaya göre delőge
#[derive(Clone, Debug)]
pub struct CcState {
    pub algorithm: CcAlgorithm,
    pub reno: RenoState,
    pub cubic: CubicState,
    pub bbr: BbrState,
}

/// Reno durumu - temel TCP tıkanma kontrolcüsü.
/// Yavaş başlama ve tıkanma kaçınma aflaması ile paket kaybına
/// çoğaltmalı azaltmayla tepki verir.
#[derive(Clone, Debug)]
pub struct RenoState {
    pub cwnd: u32,
    pub ssthresh: u32,
    pub rtt: u32,
    pub rtt_var: u32,
    pub rto: u32,
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
        CcState {
            algorithm,
            reno: RenoState::default(),
            cubic: CubicState::new(),
            bbr: BbrState::new(),
        }
    }
    
    pub fn cwnd(&self) -> u32 {
        match self.algorithm {
            CcAlgorithm::Reno => self.reno.cwnd,
            CcAlgorithm::Cubic => self.cubic.cwnd,
            CcAlgorithm::Bbr => self.bbr.cwnd(),
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
            CcAlgorithm::Bbr => {
                self.bbr.on_ack(acked_bytes, rtt_ms as u64 * 1000, current_time_ms * 1000);
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
            CcAlgorithm::Bbr => {
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
            CcAlgorithm::Bbr => {
                self.bbr.on_timeout(current_time_ms * 1000);
            }
        }
    }
}

// ============================================================================
// TCP YÖNETİCİSİ (TCP MANAGER)
// ============================================================================

static TCP_CONNECTIONS: Mutex<BTreeMap<u32, Box<TcpConnection>>> = Mutex::new(BTreeMap::new());
static TCP_LISTENERS: Mutex<BTreeMap<Port, u32>> = Mutex::new(BTreeMap::new());

/// TCP alt sistemini başlatır
pub fn init() {
    crate::serial_println!("[TCP] Initialized");
}

/// TCP soketi oluşturur
pub fn create_socket() -> u32 {
    let conn = TcpConnection::new(SocketAddr::default());
    let id = conn.id;
    TCP_CONNECTIONS.lock().insert(id, Box::new(conn));
    id
}

/// TCP soketini bir adrese bağlar
pub fn bind(socket_id: u32, addr: SocketAddr) -> Result<(), NetError> {
    let mut conns = TCP_CONNECTIONS.lock();
    let conn = conns.get_mut(&socket_id).ok_or(NetError::ProtocolError)?;
    conn.local = addr;
    Ok(())
}

/// TCP sokuyle bağlantı kurar (aktif açılım)
pub fn connect(socket_id: u32, remote: SocketAddr) -> Result<(), NetError> {
    let mut conns = TCP_CONNECTIONS.lock();
    let conn = conns.get_mut(&socket_id).ok_or(NetError::ProtocolError)?;
    conn.connect(remote)
}

/// TCP soketini dinleme moduna alır (pasif açılım)
pub fn listen(socket_id: u32, backlog: usize) -> Result<(), NetError> {
    let mut conns = TCP_CONNECTIONS.lock();
    let conn = conns.get_mut(&socket_id).ok_or(NetError::ProtocolError)?;
    conn.listen(backlog)?;
    
    // Dinleyiciyi kayıt et
    let mut listeners = TCP_LISTENERS.lock();
    listeners.insert(conn.local.port, socket_id);
    
    Ok(())
}

/// Bağlantıyı kabul eder
pub fn accept(socket_id: u32) -> Result<(u32, SocketAddr), NetError> {
    let conns = TCP_CONNECTIONS.lock();
    let conn = conns.get(&socket_id).ok_or(NetError::ProtocolError)?;
    
    if conn.state != TcpState::Listen {
        return Err(NetError::ProtocolError);
    }
    
    // TODO: Kabul kuyruğunu kontrol et
    Err(NetError::WouldBlock)
}

/// Veri gönderir
pub fn send(socket_id: u32, data: &[u8]) -> Result<usize, NetError> {
    let mut conns = TCP_CONNECTIONS.lock();
    let conn = conns.get_mut(&socket_id).ok_or(NetError::ProtocolError)?;
    conn.send(data)
}

/// Veri alır
pub fn recv(socket_id: u32, buf: &mut [u8]) -> Result<usize, NetError> {
    let mut conns = TCP_CONNECTIONS.lock();
    let conn = conns.get_mut(&socket_id).ok_or(NetError::ProtocolError)?;
    conn.recv(buf)
}

/// Soketi kapatır (FIN el sıkışmasını başlatır)
pub fn close(socket_id: u32) -> Result<(), NetError> {
    let mut conns = TCP_CONNECTIONS.lock();
    if let Some(conn) = conns.get_mut(&socket_id) {
        conn.close()?;
    }
    Ok(())
}

/// Bağlantıyı ID ile getirir (olay denetimi için)
pub fn get_connection(socket_id: u32) -> Option<TcpConnection> {
    let conns = TCP_CONNECTIONS.lock();
    conns.get(&socket_id).map(|c| (**c).clone())
}

/// Tüm bağlantıları döndürür (ss/netstat yardımcı programı için)
pub fn get_all_connections() -> Vec<TcpConnection> {
    let conns = TCP_CONNECTIONS.lock();
    conns.values().map(|c| (**c).clone()).collect()
}

/// Gelen TCP paketini işler.
/// Kaynak/hedef port eşleşmesine göre ilgili bağlantıyı bulur
/// ve durum makinesini günceller.
pub fn process_packet(ip_packet: &Ipv4Packet) -> Result<(), NetError> {
    let tcp_header = TcpHeader::parse(ip_packet.payload)?;
    let data = &ip_packet.payload[tcp_header.header_len()..];
    
    // Bağlantıyı bul
    let conns = TCP_CONNECTIONS.lock();
    
    // Kurulmuş bağlantıyı ara
    let mut found_id = None;
    for (_, conn) in conns.iter() {
        if conn.local.port == tcp_header.dst_port && 
           conn.remote.port == tcp_header.src_port {
            found_id = Some(conn.id);
            break;
        }
    }
    drop(conns);
    
    if let Some(id) = found_id {
        let mut conns = TCP_CONNECTIONS.lock();
        if let Some(conn) = conns.get_mut(&id) {
            conn.remote.ip = ip_packet.header.src;
            return conn.on_packet(&tcp_header, data);
        }
    }
    
    // Dinleyici var mı kontrol et
    let listeners = TCP_LISTENERS.lock();
    if let Some(&_socket_id) = listeners.get(&tcp_header.dst_port) {
        drop(listeners);
        
        let mut conns = TCP_CONNECTIONS.lock();
        let port_as_key = tcp_header.dst_port.0 as u32;
        if let Some(conn) = conns.get_mut(&port_as_key) {
            conn.remote.ip = ip_packet.header.src;
            return conn.on_packet(&tcp_header, data);
        }
    }
    
    // Eşleşen bağlantı bulunamadı - RST gönderilmesi gerekir
    Ok(())
}
