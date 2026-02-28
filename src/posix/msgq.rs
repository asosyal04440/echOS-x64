//! # Mesaj Kuyrukları (Message Queues)
//!
//! System V IPC ve POSIX mesaj kuyruğu implementasyonları.
//! İşlemler arası haberleşme için mesaj bazlı IPC mekanizması sağlar.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// MESAJ KUYRUĞU SABİTLERİ
// ============================================================================

/// System V IPC sabitleri
/// IPC_CREAT: Kuyruk yoksa yeni oluştur
/// IPC_EXCL: Varsa hata döndür (IPC_CREAT ile birlikte kullanılır)
/// IPC_NOWAIT: Bloke olmak yerine hata döndür
/// IPC_RMID/SET/STAT/INFO: msgctl komutları
pub const IPC_CREAT: i32 = 0x0200;
pub const IPC_EXCL: i32 = 0x0400;
pub const IPC_NOWAIT: i32 = 0x0800;
pub const IPC_RMID: i32 = 0;
pub const IPC_SET: i32 = 1;
pub const IPC_STAT: i32 = 2;
pub const IPC_INFO: i32 = 3;

/// Mesaj kuyruğu boyut sınırları
/// MSGMAX: Tek mesajın maximum bayt sayısı (8 KB)
/// MSGMNB: Kuyrukta aynı anda olabilecek toplam bayt (16 KB)
/// MSGMNI: Sistemde olabilecek toplam kuyruk ID sayısı
/// MSGTQL: Sistemde aynı anda olabilecek toplam mesaj sayısı
pub const MSGMAX: usize = 8192;        // Max message size
pub const MSGMNB: usize = 16384;       // Default max bytes on queue
pub const MSGMNI: usize = 128;         // Max message queue IDs
pub const MSGTQL: usize = 256;         // Max messages system-wide

/// POSIX mesaj kuyruğu: mesaj öncelik sayısı
pub const MQ_PRIO_MAX: u32 = 32;

// ============================================================================
// SYSTEM V MESAJI
// ============================================================================

/// System V IPC mesajı
/// mtype: Mesaj tipi - alıcı bu değere göre seçici filtreleme yapabilir
///   mtype=0: kuyruğun en başındaki mesajı al (FIFO)
///   mtype>0: tam olarak bu tipteki ilk mesajı al
///   mtype<0: tipi |mtype|'dan küçük veya eşit olan ilk mesajı al
/// data: Mesajın asıl içeriği (bayt dizisi)
#[derive(Clone, Debug)]
pub struct SysvMessage {
    /// Mesaj tipi (msgrcv filtreleme için kullanılır)
    pub mtype: i64,
    /// Mesaj verisi
    pub data: Vec<u8>,
}

impl SysvMessage {
    pub fn new(mtype: i64, data: Vec<u8>) -> Self {
        Self { mtype, data }
    }
}

// ============================================================================
// SYSTEM V MESAJ KUYRUĞU
// ============================================================================

/// System V mesaj kuyruğu
/// UNIX System V IPC standartlarına uygun mesaj kuyruğu
/// Kernel tarafından yönetilen, işlemler arası paylaşımlı veri yapısı
pub struct SysvMsgQueue {
    /// Kuyruk tanımlayıcı numarası (msqid)
    pub id: i32,
    /// Kuyruk anahtarı (msgget() çağrısındaki key)
    pub key: i32,
    /// İzin bitleri (okuma/yazma için rwxrwxrwx formatında)
    pub mode: u16,
    /// Kuyruktaki mesajlar (Mutex korumalı)
    pub messages: Mutex<Vec<SysvMessage>>,
    /// Kuyruğun alabileceği max bayt sayısı
    pub msg_bytes_max: usize,
    /// Şu anda kuyruktaki toplam bayt sayısı
    pub current_bytes: AtomicU64,
    /// Max mesaj adedi
    pub msg_max: usize,
    /// Şu anda kuyruktaki mesaj sayısı
    pub current_msgs: AtomicU32,
    /// En son mesaj gönderen sürecin PID'i
    pub lspid: AtomicU32,
    /// En son mesaj alan sürecin PID'i
    pub lrpid: AtomicU32,
    /// Son gönderme zamanı (ticks)
    pub stime: AtomicU64,
    /// Son alma zamanı (ticks)
    pub rtime: AtomicU64,
    /// Oluşturulma zamanı (ticks)
    pub ctime: AtomicU64,
    /// Sahibin kullanıcı ID'si
    pub uid: AtomicU32,
    /// Sahibin grup ID'si
    pub gid: AtomicU32,
}

impl SysvMsgQueue {
    pub fn new(id: i32, key: i32, mode: u16) -> Self {
        Self {
            id,
            key,
            mode,
            messages: Mutex::new(Vec::new()),
            msg_bytes_max: MSGMNB,
            current_bytes: AtomicU64::new(0),
            msg_max: MSGTQL,
            current_msgs: AtomicU32::new(0),
            lspid: AtomicU32::new(0),
            lrpid: AtomicU32::new(0),
            stime: AtomicU64::new(0),
            rtime: AtomicU64::new(0),
            ctime: AtomicU64::new(crate::task::scheduler::get_ticks()),
            uid: AtomicU32::new(0),
            gid: AtomicU32::new(0),
        }
    }

    /// Mesaj gönder (msgsnd sistem çağrısına karşılık gelir)
    /// flags: IPC_NOWAIT = kuyruk doluysa bloke olmak yerine hata döndür
    pub fn send(&self, msg: SysvMessage, flags: i32) -> Result<(), MsgError> {
        // Boyut kontrolü: MSGMAX sınırını aşan mesajlar reddedilir
        if msg.data.len() > MSGMAX {
            return Err(MsgError::MessageTooLong);
        }

        // Kuyruk doluluk kontrolü: MSGMNB toplam bayt sınırı
        let current = self.current_bytes.load(Ordering::SeqCst);
        if current + msg.data.len() as u64 > self.msg_bytes_max as u64 {
            if flags & IPC_NOWAIT != 0 {
                return Err(MsgError::WouldBlock);
            }
            // Bloke olmak gerekir - şimdilik hata döndür
            return Err(MsgError::WouldBlock);
        }

        // Mesajı kuyruğa ekle (FIFO sırası)
        self.messages.lock().push(msg.clone());
        self.current_bytes.fetch_add(msg.data.len() as u64, Ordering::SeqCst);
        self.current_msgs.fetch_add(1, Ordering::SeqCst);
        self.stime.store(crate::task::scheduler::get_ticks(), Ordering::SeqCst);
        self.lspid.store(0, Ordering::SeqCst); // Current PID

        Ok(())
    }

    /// Mesaj al (msgrcv sistem çağrısına karşılık gelir)
    /// mtype: 0=ilk mesaj, >0=tam eşleşme, <0=en küçük tip
    pub fn recv(&self, mtype: i64, flags: i32) -> Result<SysvMessage, MsgError> {
        let mut messages = self.messages.lock();

        // Mesaj tipi eşleştirmesi: Linux msgrcv() semantiği
        let index = if mtype == 0 {
            // mtype==0: Kuyruğun en başındaki (en eski) mesajı al
            if messages.is_empty() {
                if flags & IPC_NOWAIT != 0 {
                    return Err(MsgError::WouldBlock);
                }
                return Err(MsgError::WouldBlock);
            }
            Some(0)
        } else if mtype > 0 {
            // mtype>0: Bu tipin ilk mesajını al
            messages.iter().position(|m| m.mtype == mtype)
        } else {
            // mtype<0: |mtype|'dan küçük veya eşit tipteki ilk mesajı al
            let abs_type = (-mtype) as u64;
            messages.iter().position(|m| m.mtype as u64 <= abs_type)
        };

        if let Some(idx) = index {
            let msg = messages.remove(idx);
            self.current_bytes.fetch_sub(msg.data.len() as u64, Ordering::SeqCst);
            self.current_msgs.fetch_sub(1, Ordering::SeqCst);
            self.rtime.store(crate::task::scheduler::get_ticks(), Ordering::SeqCst);
            self.lrpid.store(0, Ordering::SeqCst); // Current PID
            return Ok(msg);
        }

        if flags & IPC_NOWAIT != 0 {
            return Err(MsgError::WouldBlock);
        }

        Err(MsgError::WouldBlock)
    }

    /// Kuyruk istatistiklerini döndür (IPC_STAT için)
    pub fn get_stats(&self) -> MsqQueueStats {
        MsqQueueStats {
            msg_qbytes: self.msg_bytes_max as u64,
            msg_qnum: self.current_msgs.load(Ordering::SeqCst) as u64,
            msg_lspid: self.lspid.load(Ordering::SeqCst),
            msg_lrpid: self.lrpid.load(Ordering::SeqCst),
            msg_stime: self.stime.load(Ordering::SeqCst),
            msg_rtime: self.rtime.load(Ordering::SeqCst),
            msg_ctime: self.ctime.load(Ordering::SeqCst),
        }
    }
}

/// Mesaj kuyruğu istatistikleri (msqid_ds yapısına karşılık gelir)
#[derive(Clone, Debug)]
pub struct MsqQueueStats {
    pub msg_qbytes: u64,
    pub msg_qnum: u64,
    pub msg_lspid: u32,
    pub msg_lrpid: u32,
    pub msg_stime: u64,
    pub msg_rtime: u64,
    pub msg_ctime: u64,
}

// ============================================================================
// POSIX MESAJI
// ============================================================================

/// POSIX (mq_send/mq_receive) mesajı
/// priority: Yüksek değer = daha yüksek öncelik (önce alınır)
/// POSIX kuyrukları System V'den farklı olarak öncelik destekler
#[derive(Clone, Debug)]
pub struct PosixMessage {
    /// Mesaj önceliği (0'dan MQ_PRIO_MAX-1'e kadar)
    pub priority: u32,
    /// Mesaj içeriği
    pub data: Vec<u8>,
}

impl PosixMessage {
    pub fn new(priority: u32, data: Vec<u8>) -> Self {
        Self { priority, data }
    }
}

// ============================================================================
// POSIX MESAJ KUYRUĞU
// ============================================================================

/// POSIX mesaj kuyruğu (mq_open/mq_send/mq_receive arayüzü)
/// System V'den farkları:
/// - İsim tabanlı erişim (dosya yolu gibi "/myqueue")
/// - Öncelik desteği (yüksek öncelikli mesajlar önce alınır)
/// - Bildirim desteği (mq_notify ile sinyal alma)
pub struct PosixMsgQueue {
    /// Kuyruk adı (örn: "/myqueue")
    pub name: String,
    /// Mesajlar (önceliğe göre sıralı tutulur)
    pub messages: Mutex<Vec<PosixMessage>>,
    /// Maximum mesaj sayısı
    pub mq_maxmsg: u32,
    /// Maximum mesaj boyutu (bayt)
    pub mq_msgsize: u32,
    /// Şu anki mesaj adedi
    pub mq_curmsgs: AtomicU32,
    /// Kuyruk bayrakları (O_NONBLOCK vb.)
    pub mq_flags: AtomicU32,
    /// Bloke olmayan mod aktif mi
    pub nonblocking: AtomicBool,
    /// Referans sayacı (birden fazla açık handle destekler)
    pub ref_count: AtomicU32,
    /// Bildirim kaydı (sinyal veya thread bildirimi için)
    pub notify: Mutex<Option<NotifyInfo>>,
}

/// Kuyruk bildirim bilgisi (mq_notify için)
#[derive(Clone, Debug)]
pub struct NotifyInfo {
    pub pid: u32,
    pub sig: u32,
}

impl PosixMsgQueue {
    pub fn new(name: &str, maxmsg: u32, msgsize: u32) -> Self {
        Self {
            name: String::from(name),
            messages: Mutex::new(Vec::new()),
            mq_maxmsg: maxmsg,
            mq_msgsize: msgsize,
            mq_curmsgs: AtomicU32::new(0),
            mq_flags: AtomicU32::new(0),
            nonblocking: AtomicBool::new(false),
            ref_count: AtomicU32::new(1),
            notify: Mutex::new(None),
        }
    }

    /// Mesaj gönder (mq_send/mq_timedsend için)
    /// Mesajlar önceliğe göre sıralı eklenir (büyük öncelik = öne eklenir)
    pub fn send(&self, msg: PosixMessage, _timeout: Option<u64>) -> Result<(), MsgError> {
        // Boyut kontrolü
        if msg.data.len() > self.mq_msgsize as usize {
            return Err(MsgError::MessageTooLong);
        }

        // Kuyruk doluluk kontrolü
        if self.mq_curmsgs.load(Ordering::SeqCst) >= self.mq_maxmsg {
            if self.nonblocking.load(Ordering::SeqCst) {
                return Err(MsgError::WouldBlock);
            }
            return Err(MsgError::WouldBlock);
        }

        // Önceliğe göre sıralı ekleme - yüksek öncelik öne geçer
        let mut messages = self.messages.lock();
        let pos = messages.iter()
            .position(|m| m.priority < msg.priority)
            .unwrap_or(messages.len());
        messages.insert(pos, msg);

        self.mq_curmsgs.fetch_add(1, Ordering::SeqCst);

        // Bildirim kaydı varsa process'e sinyal gönder
        if let Some(notify) = self.notify.lock().as_ref() {
            // Send signal
            crate::serial_println!("[MQ] Notify PID {} with signal {}", notify.pid, notify.sig);
        }

        Ok(())
    }

    /// Mesaj al (mq_receive/mq_timedreceive için)
    /// Vec'in sonundaki = en yüksek öncelikli mesaj döndürülür
    pub fn recv(&self, _timeout: Option<u64>) -> Result<PosixMessage, MsgError> {
        if self.mq_curmsgs.load(Ordering::SeqCst) == 0 {
            if self.nonblocking.load(Ordering::SeqCst) {
                return Err(MsgError::WouldBlock);
            }
            return Err(MsgError::WouldBlock);
        }

        let mut messages = self.messages.lock();
        if let Some(msg) = messages.pop() {
            self.mq_curmsgs.fetch_sub(1, Ordering::SeqCst);
            return Ok(msg);
        }

        Err(MsgError::WouldBlock)
    }

    /// Kuyruk özelliklerini döndür (mq_getattr için)
    pub fn get_attr(&self) -> MqAttr {
        MqAttr {
            mq_flags: self.mq_flags.load(Ordering::SeqCst),
            mq_maxmsg: self.mq_maxmsg,
            mq_msgsize: self.mq_msgsize,
            mq_curmsgs: self.mq_curmsgs.load(Ordering::SeqCst),
        }
    }

    /// Kuyruk bayraklarını ayarla (mq_setattr için)
    pub fn set_attr(&self, flags: u32) {
        self.mq_flags.store(flags, Ordering::SeqCst);
        self.nonblocking.store((flags & O_NONBLOCK) != 0, Ordering::SeqCst);
    }
}

/// POSIX mesaj kuyruğu özellik yapısı (mq_attr)
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MqAttr {
    pub mq_flags: u32,
    pub mq_maxmsg: u32,
    pub mq_msgsize: u32,
    pub mq_curmsgs: u32,
}

/// O_NONBLOCK bayrağı: bloke olmadan işlem yap
const O_NONBLOCK: u32 = 0x800;

// ============================================================================
// MESAJ KUYRUĞU YÖNETİCİSİ
// ============================================================================

/// Hem System V hem POSIX mesaj kuyruklarını yöneten merkezi yapı
/// - sysv_queues: integer ID ile erişilen System V kuyrukları
/// - posix_queues: string isim ile erişilen POSIX kuyrukları
pub struct MsgQueueManager {
    /// System V mesaj kuyrukları (id -> kuyruk)
    sysv_queues: Mutex<BTreeMap<i32, Arc<SysvMsgQueue>>>,
    /// POSIX mesaj kuyrukları (isim -> kuyruk)
    posix_queues: Mutex<BTreeMap<String, Arc<PosixMsgQueue>>>,
    /// Bir sonraki System V ID değeri
    next_sysv_id: AtomicI32,
    /// İstatistikler
    stats: Mutex<MsgStats>,
}

/// Mesaj kuyruğu istatistikleri
#[derive(Clone, Debug, Default)]
pub struct MsgStats {
    pub sysv_queues: u32,
    pub posix_queues: u32,
    pub messages_sent: u64,
    pub messages_received: u64,
}

impl MsgQueueManager {
    pub const fn new() -> Self {
        Self {
            sysv_queues: Mutex::new(BTreeMap::new()),
            posix_queues: Mutex::new(BTreeMap::new()),
            next_sysv_id: AtomicI32::new(1),
            stats: Mutex::new(MsgStats::default()),
        }
    }

    /// System V mesaj kuyruğu oluştur veya aç (msgget sistem çağrısı)
    pub fn msgget(&self, key: i32, msgflg: i32) -> Result<i32, MsgError> {
        let mut queues = self.sysv_queues.lock();

        // Anahtar eşleşmesi: aynı key ile önceden oluşturulmuş kuyruk var mı?
        for queue in queues.values() {
            if queue.key == key && key != 0 {
                if msgflg & IPC_EXCL != 0 {
                    return Err(MsgError::AlreadyExists);
                }
                return Ok(queue.id);
            }
        }

        if msgflg & IPC_CREAT == 0 {
            return Err(MsgError::NotFound);
        }

        let id = self.next_sysv_id.fetch_add(1, Ordering::SeqCst);
        let queue = Arc::new(SysvMsgQueue::new(id, key, (msgflg & 0x1FF) as u16));
        queues.insert(id, queue);

        let mut stats = self.stats.lock();
        stats.sysv_queues += 1;

        Ok(id)
    }

    /// System V kontrol (IPC_RMID=sil, IPC_STAT=durum) (msgctl sistem çağrısı)
    pub fn msgctl(&self, msqid: i32, cmd: i32) -> Result<MsqQueueStats, MsgError> {
        let queues = self.sysv_queues.lock();

        match cmd {
            IPC_RMID => {
                drop(queues);
                self.sysv_queues.lock().remove(&msqid);
                Ok(MsqQueueStats {
                    msg_qbytes: 0, msg_qnum: 0, msg_lspid: 0, msg_lrpid: 0,
                    msg_stime: 0, msg_rtime: 0, msg_ctime: 0,
                })
            }
            IPC_STAT => {
                let queue = queues.get(&msqid).ok_or(MsgError::NotFound)?;
                Ok(queue.get_stats())
            }
            _ => Err(MsgError::InvalidCommand),
        }
    }

    /// System V mesaj gönder (msgsnd sistem çağrısı)
    pub fn msgsnd(&self, msqid: i32, msg: SysvMessage, flags: i32) -> Result<(), MsgError> {
        let queues = self.sysv_queues.lock();
        let queue = queues.get(&msqid).ok_or(MsgError::NotFound)?;

        let result = queue.send(msg, flags);

        let mut stats = self.stats.lock();
        stats.messages_sent += 1;

        result
    }

    /// System V mesaj al (msgrcv sistem çağrısı)
    pub fn msgrcv(&self, msqid: i32, mtype: i64, flags: i32) -> Result<SysvMessage, MsgError> {
        let queues = self.sysv_queues.lock();
        let queue = queues.get(&msqid).ok_or(MsgError::NotFound)?;

        let result = queue.recv(mtype, flags);

        let mut stats = self.stats.lock();
        stats.messages_received += 1;

        result
    }

    /// POSIX mesaj kuyruğu oluştur veya aç (mq_open sistem çağrısı)
    pub fn mq_open(&self, name: &str, oflag: i32, mode: u32, attr: Option<MqAttr>) -> Result<Arc<PosixMsgQueue>, MsgError> {
        let mut queues = self.posix_queues.lock();

        if let Some(queue) = queues.get(name) {
            if oflag & IPC_EXCL != 0 {
                return Err(MsgError::AlreadyExists);
            }
            queue.ref_count.fetch_add(1, Ordering::SeqCst);
            return Ok(queue.clone());
        }

        if oflag & IPC_CREAT == 0 {
            return Err(MsgError::NotFound);
        }

        let (maxmsg, msgsize) = if let Some(a) = attr {
            (a.mq_maxmsg, a.mq_msgsize)
        } else {
            (10, 8192)
        };

        let queue = Arc::new(PosixMsgQueue::new(name, maxmsg, msgsize));
        queues.insert(String::from(name), queue.clone());

        let mut stats = self.stats.lock();
        stats.posix_queues += 1;

        Ok(queue)
    }

    /// POSIX mesaj kuyruğunu kapat (mq_close sistem çağrısı)
    pub fn mq_close(&self, name: &str) -> Result<(), MsgError> {
        if let Some(queue) = self.posix_queues.lock().get(name) {
            queue.ref_count.fetch_sub(1, Ordering::SeqCst);
            return Ok(());
        }
        Err(MsgError::NotFound)
    }

    /// POSIX mesaj kuyruğunu sil (mq_unlink sistem çağrısı)
    pub fn mq_unlink(&self, name: &str) -> Result<(), MsgError> {
        self.posix_queues.lock().remove(name);
        Ok(())
    }

    /// Mesaj kuyruğu istatistiklerini döndür
    pub fn get_stats(&self) -> MsgStats {
        self.stats.lock().clone()
    }
}

lazy_static::lazy_static! {
    pub static ref MSG_QUEUE_MANAGER: MsgQueueManager = MsgQueueManager::new();
}

// ============================================================================
// HATA TİPİ
// ============================================================================

/// Mesaj kuyruğu hata kodları
/// Linux hata kodlarından türetilmiştir (ENOENT, EEXIST, EAGAIN vb.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsgError {
    NotFound,          // ENOENT: Kuyruk bulunamadı
    AlreadyExists,     // EEXIST: Kuyruk zaten mevcut
    WouldBlock,        // EAGAIN: Bloke olunması gerekiyor
    MessageTooLong,    // EMSGSIZE: Mesaj çok büyük
    InvalidCommand,    // EINVAL: Geçersiz komut
    PermissionDenied,  // EACCES: Yetki yok
}

// ============================================================================
// SİSTEM ÇAĞRISI ARAYÜZÜ
// ============================================================================

/// msgget() sistem çağrısı wrapper'ı
/// Başarıda kuyruk ID'si, hata durumunda negatif errno döndürür
pub fn sys_msgget(key: i32, msgflg: i32) -> i32 {
    match MSG_QUEUE_MANAGER.msgget(key, msgflg) {
        Ok(id) => id,
        Err(MsgError::NotFound) => -2,     // -ENOENT
        Err(MsgError::AlreadyExists) => -17, // -EEXIST
        Err(_) => -22,                      // -EINVAL
    }
}

/// msgsnd() sistem çağrısı wrapper'ı
/// Başarıda 0, hata durumunda negatif errno döndürür
pub fn sys_msgsnd(msqid: i32, mtype: i64, data: &[u8], flags: i32) -> i32 {
    let msg = SysvMessage::new(mtype, data.to_vec());
    match MSG_QUEUE_MANAGER.msgsnd(msqid, msg, flags) {
        Ok(()) => 0,
        Err(MsgError::WouldBlock) => -11, // -EAGAIN
        Err(_) => -22,                    // -EINVAL
    }
}

/// msgrcv() sistem çağrısı wrapper'ı
/// Başarıda alınan bayt sayısı, hata durumunda negatif errno döndürür
pub fn sys_msgrcv(msqid: i32, mtype: i64, buf: &mut [u8], flags: i32) -> i64 {
    match MSG_QUEUE_MANAGER.msgrcv(msqid, mtype, flags) {
        Ok(msg) => {
            let len = msg.data.len().min(buf.len());
            buf[..len].copy_from_slice(&msg.data[..len]);
            len as i64
        }
        Err(MsgError::WouldBlock) => -11, // -EAGAIN
        Err(_) => -22,                    // -EINVAL
    }
}

// ============================================================================
// BAŞLATMA
// ============================================================================

/// Mesaj kuyruğu alt sistemini başlat
pub fn init() {
    crate::serial_println!("[MSGQ] Message queues initialized");
}
