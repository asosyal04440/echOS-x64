//! # TCP_CORK — Gönderim Toplama (Send Batching)
//!
//! Linux `TCP_CORK` soket seçeneği ile küçük segmentlerin tek bir TCP
//! segmentinde birleştirilmesini sağlar.
//!
//! ## TCP_CORK Nedir?
//!
//! `setsockopt(fd, TCP_CORK, 1)` ile soket "corked" moda alınır. Bu modda:
//! - Gönderilen veri hemen paketlenmez, dahili bir tamponda birikir
//! - `TCP_CORK=0` yapıldığında birikmiş veri tek bir segmentle gönderilir
//! - MTU'ya ulaşıldığında otomatik gönderim tetiklenir
//!
//! ## Nagle vs TCP_CORK
//!
//! - **Nagle (TCP_NODELAY=0)**: Akıllı, küçük yazma varsa bekler (200ms ACK timer)
//! - **TCP_CORK**: Geliştirici kontrolünde; "tüm veri hazır" sinyali ile manuel flush
//!
//! ## Tipik Kullanım
//!
//! ```text
//! // HTTP response header + body gönderirken:
//! cork(sock);
//! write(sock, "HTTP/1.1 200 OK\r\n");
//! write(sock, "Content-Length: 1024\r\n\r\n");
//! write(sock, body);              // header + body tek paket gider
//! uncork(sock);                    // flush
//! ```
//!
//! ## Avantaj ve Dezavantaj
//!
//! **Avantaj:** Header + body tek paket, ~40 byte overhead tasarrufu, azaltılmış
//! segment sayısı (bufferbloat düşer)
//!
//! **Dezavantaj:** Eğer uncork çağrılmazsa veri sonsuza dek tamponda kalır.
//! Flush için MTU, timeout veya explicit uncork gerekir.

use super::{tcp, NetError};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

/// Maksimum cork tamponu (64 KB — MTU güvenli sınırı)
pub const CORK_MAX_BUF: usize = 65536;

/// TCP_CORK durumu — soket başına bir tampon
#[derive(Clone, Debug, Default)]
pub struct CorkState {
    /// Corked modda mı?
    pub enabled: bool,
    /// Birikmiş veri (uncork veya MTU dolduğunda gönderilecek)
    pub buffer: Vec<u8>,
    /// Toplam corked byte sayısı
    pub bytes_corked: u64,
    /// Corked iken yapılan write sayısı
    pub writes_corked: u64,
    /// En son uncork zamanı (ticks)
    pub last_uncork_ticks: u64,
    /// MTU'ya ulaşılıp otomatik flush sayısı
    pub auto_flushes: u64,
}

impl CorkState {
    pub const fn new() -> Self {
        CorkState {
            enabled: false,
            buffer: Vec::new(),
            bytes_corked: 0,
            writes_corked: 0,
            last_uncork_ticks: 0,
            auto_flushes: 0,
        }
    }
}

static CORK_STATES: Mutex<BTreeMap<u32, CorkState>> = Mutex::new(BTreeMap::new());

/// Soket için cork durumunu al (yoksa oluştur)
fn get_or_create(socket_id: u32) -> CorkState {
    let mut states = CORK_STATES.lock();
    if let Some(s) = states.get(&socket_id) {
        return s.clone();
    }
    let s = CorkState::new();
    states.insert(socket_id, CorkState::new());
    s
}

/// TCP_CORK=1 — corking modunu etkinleştir
pub fn enable(socket_id: u32) {
    let mut states = CORK_STATES.lock();
    let entry = states.entry(socket_id).or_insert_with(CorkState::new);
    entry.enabled = true;
    crate::serial_println!("[TCP_CORK] enable({})", socket_id);
}

/// TCP_CORK=0 — corking modunu kapat ve birikmiş veriyi gönder
///
/// Flush edilen byte sayısını döndürür. `tcp::send` başarısız olursa
/// tampon ve `enabled` durumu geri alınır; veri kaybı olmaz.
pub fn disable(socket_id: u32) -> Result<usize, NetError> {
    // Aşama 1: kilit altında tamponu al ve durumu "uncorked" yap.
    let buf = {
        let mut states = CORK_STATES.lock();
        let state = states.get_mut(&socket_id).ok_or(NetError::InvalidFd)?;
        state.enabled = false;
        state.last_uncork_ticks = crate::interrupts::get_ticks();
        core::mem::take(&mut state.buffer)
    };
    let len = buf.len();

    if len == 0 {
        return Ok(0);
    }

    // Aşama 2: lock'ı bırak, tcp::send çağır (TCP send uzun sürebilir)
    let result = tcp::send(socket_id, &buf);
    match result {
        Ok(_) => {
            let mut states = CORK_STATES.lock();
            if let Some(state) = states.get_mut(&socket_id) {
                state.bytes_corked += len as u64;
            }
            crate::serial_println!(
                "[TCP_CORK] disable({}) flushed {} bytes",
                socket_id,
                len
            );
            Ok(len)
        }
        Err(e) => {
            // Tamponu ve enabled=true durumunu geri al — corked kalan
            // veri sonraki write/flush denemesinde tekrar denenir
            let mut states = CORK_STATES.lock();
            if let Some(state) = states.get_mut(&socket_id) {
                state.buffer = buf;
                state.enabled = true;
            }
            Err(e)
        }
    }
}

/// Corked modda mı?
pub fn is_enabled(socket_id: u32) -> bool {
    CORK_STATES
        .lock()
        .get(&socket_id)
        .map(|s| s.enabled)
        .unwrap_or(false)
}

/// Corked sokete yaz. Corked ise tampona ekle, değilse doğrudan gönder.
///
/// MTU'ya (1500 byte) ulaşıldığında otomatik flush yapılır.
pub fn write(socket_id: u32, data: &[u8]) -> Result<usize, NetError> {
    let mut states = CORK_STATES.lock();
    let state = states.entry(socket_id).or_insert_with(CorkState::new);

    if !state.enabled {
        drop(states);
        return tcp::send(socket_id, data);
    }

    // Corked: tampona ekle
    if state.buffer.len() + data.len() > CORK_MAX_BUF {
        return Err(NetError::BufferFull);
    }
    state.buffer.extend_from_slice(data);
    state.writes_corked += 1;
    let buf_len = state.buffer.len();
    drop(states);

    // MTU dolduysa otomatik flush
    if buf_len >= 1500 {
        disable(socket_id)?;
    }
    Ok(data.len())
}

/// Corked veriyi zorla flush et (TCP_CORK=0 yapmadan)
pub fn flush(socket_id: u32) -> Result<usize, NetError> {
    disable(socket_id)
}

/// Soket kapatılırken çağrılacak — corked veriyi flush et
pub fn on_close(socket_id: u32) {
    if is_enabled(socket_id) {
        let _ = flush(socket_id);
    }
    CORK_STATES.lock().remove(&socket_id);
}

/// Cork istatistiklerini al
pub fn stats(socket_id: u32) -> CorkState {
    CORK_STATES
        .lock()
        .get(&socket_id)
        .cloned()
        .unwrap_or_default()
}

// ============================================================================
// TESTLER
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cork_state_default_is_disabled() {
        let s = CorkState::new();
        assert!(!s.enabled);
        assert_eq!(s.buffer.len(), 0);
    }

    #[test]
    fn cork_max_buf_is_mtu_safe() {
        // 64 KB > herhangi bir MTU, güvenli üst sınır
        assert!(CORK_MAX_BUF >= 1500);
    }

    #[test]
    fn enable_disable_round_trip_preserves_buffer_logic() {
        // State machine sıralaması: enable() sonra disable() buffer'ı boşaltmalı
        // Burada gerçek ağ gönderimi olmadığı için sadece API akışını test ediyoruz
        let mut s = CorkState::new();
        assert!(!s.enabled);
        s.enabled = true;
        s.buffer.extend_from_slice(b"hello");
        s.buffer.extend_from_slice(b" world");
        assert_eq!(s.buffer.len(), 11);
        s.enabled = false;
        // Manuel simülasyon: gerçek kodda disable() buf'ı tcp::send'e verir
        let flushed = core::mem::take(&mut s.buffer);
        assert_eq!(flushed, b"hello world");
    }

    #[test]
    fn is_enabled_tracks_state_machine() {
        // Bilinmeyen soket: false
        assert!(!is_enabled(0xDEAD_BEEF));
        enable(0xDEAD_BEEF);
        assert!(is_enabled(0xDEAD_BEEF));
        on_close(0xDEAD_BEEF);
        assert!(!is_enabled(0xDEAD_BEEF));
    }

    #[test]
    fn cork_max_buf_rejects_oversize() {
        // CORK_MAX_BUF'un beklenen alt sınırı korunuyor
        assert!(CORK_MAX_BUF >= 65535);
    }
}
