//! # Argon2 Şifre Özetleme
//!
//! Argon2id bellek-yoğun şifre özet fonksiyonu.
//! Şifreleri kaba-kuvvet saldırılarına karşı korumak için RAM kullanımını zorlaştırır.
//!
//! ## Argon2 Neden Özeldir?
//!
//! Klasik hash fonksiyonları (SHA-256 gibi) ASIC/GPU ile çok hızlı hesaplanabileceğinden
//! kaba-kuvvet saldırılarına karşı yetersiz kalır. Argon2 bunu üç araçla engeller:
//!
//! 1. **Zaman maliyeti** (time_cost): İterasyon sayısı — CPU süresini artırır
//! 2. **Bellek maliyeti** (memory_cost): RAM kullanımı — GPU/ASIC saldırısını zorlaştırır
//! 3. **Paralellik** (parallelism): Şerit sayısı — birden fazla çekirdeği kullanır
//!
//! ## Argon2 Bellek Yapısı
//!
//! ```text
//!  memory_cost = 64MB, parallelism = 4 şerit, time_cost = 3 geçiş
//!
//!  Şerit 0: [B0,0][B0,1][B0,2]...[B0,L-1]
//!  Şerit 1: [B1,0][B1,1][B1,2]...[B1,L-1]
//!  Şerit 2: [B2,0][B2,1][B2,2]...[B2,L-1]
//!  Şerit 3: [B3,0][B3,1][B3,2]...[B3,L-1]
//!            ─────────── segment ──────────
//!             seg0  seg1  seg2  seg3
//!
//!  Her blok = 1024 bayt = 128 × 64-bit kelime
//!  Her geçişte her blok, diğer bloklardan seçilen
//!  iki referans bloğun G() fonksiyonuyla karışımıdır.
//!
//!  Sonlandırma:
//!  Tüm şeritlerin son bloğu XOR → son blok → Hash → çıktı
//! ```
//!
//! ## Argon2 Varyantları
//!
//! ```text
//!  Argon2d  : Veri bağımlı adresleme  → GPU saldırısına karşı güçlü
//!  Argon2i  : Veri bağımsız adresleme → yan kanal saldırısına karşı güçlü
//!  Argon2id : Her ikisinin karışımı   → önerilen genel amaçlı seçim
//! ```

use alloc::vec::Vec;

/// Her geçişte kaç senkronizasyon noktası olduğu (segment sayısı)
const ARGON2_SYNC_POINTS: usize = 4;
/// Her bloğun bayt cinsinden boyutu (1 KiB)
const ARGON2_BLOCK_SIZE: usize = 1024;

/// Argon2 varyantı: d (veri bağımlı), i (veri bağımsız), id (her ikisi — önerilen)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Argon2Variant {
    Argon2d = 0,
    Argon2i = 1,
    Argon2id = 2,
}

/// Argon2 protokol sürümü (V13 güncel standarttır)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Argon2Version {
    V10 = 0x10,
    V13 = 0x13,
}

/// Argon2 yapılandırma parametreleri — zaman/bellek/paralellik dengesini ayarlar
#[derive(Clone, Debug)]
pub struct Argon2Config {
    pub variant: Argon2Variant,
    pub version: Argon2Version,
    pub time_cost: u32,   // İterasyon sayısı — daha yüksek = daha yavaş hesaplama
    pub memory_cost: u32, // Bellek kullanımı KiB cinsinden
    pub parallelism: u32, // Paralel şerit (lane) sayısı
    pub hash_len: usize,  // Çıktı hash uzunluğu (bayt)
}

impl Default for Argon2Config {
    fn default() -> Self {
        Argon2Config {
            variant: Argon2Variant::Argon2id,
            version: Argon2Version::V13,
            time_cost: 3,
            memory_cost: 65536, // 64 MB bellek kullanımı
            parallelism: 4,
            hash_len: 32,
        }
    }
}

/// Argon2 bağlamı — bellek ve yapılandırma durumunu tutar
pub struct Argon2 {
    config: Argon2Config,
    memory: Vec<[u64; 128]>, // Blok başına 1024 bayt (128 × 64-bit kelime)
    segment_len: usize,
    lane_len: usize,
}

impl Argon2 {
    /// Yeni bir Argon2 örneği oluşturur ve bellek alanını başlatır.
    ///
    /// Bellek düzeni: parallelism × lane_len blok
    /// lane_len = ARGON2_SYNC_POINTS × segment_len
    pub fn new(config: Argon2Config) -> Self {
        let memory_blocks = config.memory_cost as usize;
        let segment_len = memory_blocks / (config.parallelism as usize * ARGON2_SYNC_POINTS);
        let lane_len = segment_len * ARGON2_SYNC_POINTS;

        let mut memory = Vec::with_capacity(memory_blocks);
        memory.resize(memory_blocks, [0u64; 128]);

        Argon2 {
            config,
            memory,
            segment_len,
            lane_len,
        }
    }

    /// Şifreyi özet değerine dönüştürür.
    ///
    /// Üç aşama:
    /// 1. `initial_hash`: H0 başlangıç vektörünü oluştur
    /// 2. `fill_memory_blocks`: Tüm bellek bloklarını doldur (zaman/bellek yoğun)
    /// 3. `finalize`: Son bloğu özetle ve çıktıyı üret
    pub fn hash(&mut self, password: &[u8], salt: &[u8], secret: &[u8], ad: &[u8]) -> Vec<u8> {
        // Başlangıç özetleme — H0 başlangıç vektörünü oluşturur
        let h0 = self.initial_hash(password, salt, secret, ad);

        // Bellek bloklarını doldur — hafızayı yoğun biçimde kullanır
        self.fill_memory_blocks(&h0);

        // Sonlandır — son bloğu özetle
        self.finalize()
    }

    /// Varsayılan yapılandırmayla şifreyi özetler (kullanım kolaylığı için)
    pub fn hash_password(password: &[u8], salt: &[u8]) -> Vec<u8> {
        let config = Argon2Config::default();
        let mut argon2 = Argon2::new(config);
        argon2.hash(password, salt, &[], &[])
    }

    /// Şifreyi mevcut özet değeriyle yan kanal güvenli biçimde doğrular.
    ///
    /// Sabit zamanlı karşılaştırma kullanılır; erken çıkış yoktur.
    /// Bu, `computed[i] != expected[i]` durumunu zamanlama farkından gizler.
    pub fn verify(password: &[u8], salt: &[u8], expected_hash: &[u8]) -> bool {
        let config = Argon2Config {
            hash_len: expected_hash.len(),
            ..Default::default()
        };
        let mut argon2 = Argon2::new(config);
        let computed = argon2.hash(password, salt, &[], &[]);

        crate::crypto::constant_time_eq(&computed, expected_hash)
    }

    fn initial_hash(&self, password: &[u8], salt: &[u8], secret: &[u8], ad: &[u8]) -> [u8; 64] {
        // H0 = H(|p| || p || |s| || s || |k| || k || |X| || X ||
        //         |A| || A || v || y || t || m || p || L || K || X)
        // Tüm giriş parametrelerini birleştirerek başlangıç özet vektörü oluşturulur

        let mut hasher = crate::crypto::Sha3::sha3_512();

        // Şifre uzunluğu ve şifre verisi
        hasher.update(&(password.len() as u32).to_le_bytes());
        hasher.update(password);

        // Tuz (salt) uzunluğu ve tuz verisi
        hasher.update(&(salt.len() as u32).to_le_bytes());
        hasher.update(salt);

        // Gizli anahtar uzunluğu ve gizli anahtar
        hasher.update(&(secret.len() as u32).to_le_bytes());
        hasher.update(secret);

        // İlişkili veri (associated data) uzunluğu ve verisi
        hasher.update(&(ad.len() as u32).to_le_bytes());
        hasher.update(ad);

        // Yapılandırma parametreleri
        hasher.update(&[self.config.variant as u8]);
        hasher.update(&[self.config.version as u8]);
        hasher.update(&self.config.time_cost.to_le_bytes());
        hasher.update(&self.config.memory_cost.to_le_bytes());
        hasher.update(&self.config.parallelism.to_le_bytes());
        hasher.update(&(self.config.hash_len as u32).to_le_bytes());

        let result = hasher.finalize();
        let mut h0 = [0u8; 64];
        h0.copy_from_slice(&result[..64]);
        h0
    }

    fn fill_memory_blocks(&mut self, h0: &[u8; 64]) {
        // Her şerit için ilk iki bloğu oluştur (H0 + şerit/sayaç karışımı)
        for lane in 0..self.config.parallelism as usize {
            // Her şerit için ilk blok B[0]
            let j0 = lane * self.lane_len;
            self.generate_block(j0, h0, lane, 0);

            // Her şerit için ikinci blok B[1]
            let j1 = lane * self.lane_len + 1;
            self.generate_block(j1, h0, lane, 1);
        }

        // Kalan blokları doldur (time_cost kadar geçiş, her geçişte tüm segmentler)
        for pass in 0..self.config.time_cost as usize {
            for slice in 0..ARGON2_SYNC_POINTS {
                for lane in 0..self.config.parallelism as usize {
                    for offset in 0..self.segment_len {
                        let segment_start = slice * self.segment_len;
                        let j = lane * self.lane_len + segment_start + offset;

                        if j < 2 {
                            continue; // İlk iki bloğu atla (zaten oluşturuldu)
                        }

                        // Her bloğu iki referans bloğun G() fonksiyonuyla hesapla
                        self.compute_block(j, pass, slice, lane, offset);
                    }
                }
            }
        }
    }

    fn generate_block(&mut self, block_idx: usize, h0: &[u8; 64], lane: usize, counter: usize) {
        // G fonksiyonu kullanarak başlangıç bloğu oluştur
        let mut input = [0u8; 72];
        input[..64].copy_from_slice(h0);
        input[64..68].copy_from_slice(&(lane as u32).to_le_bytes());
        input[68..72].copy_from_slice(&(counter as u32).to_le_bytes());

        // Girişi özetle ve bloğu doldur
        let mut hasher = crate::crypto::Sha3::sha3_512();
        hasher.update(&input);
        let hash = hasher.finalize();

        // Hash sonucundan bloğu doldur
        for i in 0..64 {
            let val = u64::from_le_bytes([
                hash[i * 8],
                hash[i * 8 + 1],
                hash[i * 8 + 2],
                hash[i * 8 + 3],
                hash[i * 8 + 4],
                hash[i * 8 + 5],
                hash[i * 8 + 6],
                hash[i * 8 + 7],
            ]);
            self.memory[block_idx][i] = val;
        }
    }

    fn compute_block(
        &mut self,
        block_idx: usize,
        pass: usize,
        slice: usize,
        lane: usize,
        offset: usize,
    ) {
        // Basitleştirilmiş blok hesaplama.
        // Gerçek implementasyon doğru adresleme ve G fonksiyonu (Blake2b tabanlı) gerektirir.

        // Referans bloklarını al (sözde-rastgele seçim)
        let ref1 = self.get_ref_block(block_idx, pass, slice, lane, offset, 0);
        let ref2 = self.get_ref_block(block_idx, pass, slice, lane, offset, 1);

        // G fonksiyonunu uygula (basitleştirilmiş karıştırma)
        for i in 0..128 {
            self.memory[block_idx][i] = self.memory[ref1][i]
                .wrapping_add(self.memory[ref2][i])
                .rotate_left(24);
        }
    }

    fn get_ref_block(
        &self,
        block_idx: usize,
        pass: usize,
        slice: usize,
        lane: usize,
        offset: usize,
        _ref_num: usize,
    ) -> usize {
        // Basitleştirilmiş adresleme — gerçek implementasyon sözde-rastgele adresleme gerektirir
        let lane_len = self.lane_len;
        let segment_start = slice * self.segment_len;

        // Basit sözde-rastgele seçim (deterministik hash tabanlı)
        let mut hasher = crate::crypto::Sha3::sha3_256();
        hasher.update(&(pass as u32).to_le_bytes());
        hasher.update(&(slice as u32).to_le_bytes());
        hasher.update(&(lane as u32).to_le_bytes());
        hasher.update(&(offset as u32).to_le_bytes());
        let hash = hasher.finalize();

        let pseudo_val = u32::from_le_bytes([hash[0], hash[1], hash[2], hash[3]]) as usize;

        // Mevcut bloklar arasından seç
        if pass == 0 {
            pseudo_val % (segment_start + offset)
        } else {
            pseudo_val % (lane_len * self.config.parallelism as usize)
        }
    }

    fn finalize(&self) -> Vec<u8> {
        // Her şeridin son bloğunu XOR ile birleştir
        let mut result_block = [0u64; 128];

        for lane in 0..self.config.parallelism as usize {
            let last_block = (lane + 1) * self.lane_len - 1;
            for i in 0..128 {
                result_block[i] ^= self.memory[last_block][i];
            }
        }

        // Sonuç bloğunu özetle
        let mut hasher = crate::crypto::Sha3::sha3_256();
        for word in result_block.iter() {
            hasher.update(&word.to_le_bytes());
        }

        let hash = hasher.finalize();
        let mut output = Vec::with_capacity(self.config.hash_len);
        output.extend_from_slice(&hash[..self.config.hash_len.min(hash.len())]);
        output
    }
}

/// Tuz ve parametrelerle birlikte saklanan şifre özet yapısı.
/// Doğrulama için gerekli tüm bilgileri bir arada tutar.
#[derive(Clone, Debug)]
pub struct PasswordHash {
    pub hash: Vec<u8>,
    pub salt: Vec<u8>,
    pub time_cost: u32,
    pub memory_cost: u32,
    pub parallelism: u32,
}

impl PasswordHash {
    /// Yeni şifre özeti oluşturur — rastgele tuz üretir ve şifreyi özet değerine dönüştürür.
    /// RDRAND donanım rastgele sayı üreteci tuz için kullanılır.
    pub fn new(password: &[u8]) -> Self {
        // Rastgele tuz oluştur (donanım RNG ile)
        let mut salt = [0u8; 16];
        crate::crypto::rdrand_bytes(&mut salt);

        let config = Argon2Config::default();
        let mut argon2 = Argon2::new(config.clone());
        let hash = argon2.hash(password, &salt, &[], &[]);

        PasswordHash {
            hash,
            salt: salt.to_vec(),
            time_cost: config.time_cost,
            memory_cost: config.memory_cost,
            parallelism: config.parallelism,
        }
    }

    /// Şifreyi saklanan özet değeriyle doğrular.
    /// Saklanan parametrelerle aynı Argon2 hesabı yapılır, sabit zamanlı karşılaştırılır.
    pub fn verify(&self, password: &[u8]) -> bool {
        let config = Argon2Config {
            time_cost: self.time_cost,
            memory_cost: self.memory_cost,
            parallelism: self.parallelism,
            hash_len: self.hash.len(),
            ..Default::default()
        };

        let mut argon2 = Argon2::new(config);
        let computed = argon2.hash(password, &self.salt, &[], &[]);

        crate::crypto::constant_time_eq(&computed, &self.hash)
    }
}
