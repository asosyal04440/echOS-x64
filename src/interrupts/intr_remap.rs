//! # Kesme Yeniden Yönlendirme
//!
//! Intel VT-d ve AMD-Vi kesme yeniden yönlendirme desteği.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// KES. YENİDEN YÖNL. SABİTLERİ
// ============================================================================

/// Intel VT-d yazmaçları
pub const VTD_VER_REG: u32 = 0x00;
pub const VTD_CAP_REG: u32 = 0x08;
pub const VTD_ECAP_REG: u32 = 0x10;
pub const VTD_GCMD_REG: u32 = 0x18;
pub const VTD_GSTS_REG: u32 = 0x1C;
pub const VTD_RTADDR_REG: u32 = 0x20;
pub const VTD_CCMD_REG: u32 = 0x28;
pub const VTD_FSTS_REG: u32 = 0x34;
pub const VTD_FECTL_REG: u32 = 0x38;
pub const VTD_FEDATA_REG: u32 = 0x3C;
pub const VTD_FEADDR_REG: u32 = 0x40;
pub const VTD_FEUADDR_REG: u32 = 0x44;
pub const VTD_AFLOG_REG: u32 = 0x58;
pub const VTD_IVA_REG: u32 = 0x60;
pub const VTD_IRTA_REG: u32 = 0xB8;

/// VT-d yetenek bayrakları
pub const VTD_CAP_RWBF: u64 = 1 << 4;      // Zorunlu Yazma-Buffer Temizleme
pub const VTD_CAP_AFL: u64 = 1 << 3;        // Gelişmiş Hata Günlüğü
pub const VTD_CAP_MGAW_MASK: u64 = 0x3F << 16; // Maksimum Misafir Adres Genişliği
pub const VTD_CAP_SAGAW_MASK: u64 = 0x1F << 8;  // Desteklenen Ayarlanmış Misafir Adres Genişliği

/// VT-d genişletilmiş yetenek bayrakları
pub const VTD_ECAP_IR: u64 = 1 << 3;        // Kesme Yeniden Yönlendirme
pub const VTD_ECAP_EIM: u64 = 1 << 4;       // Genişletilmiş Kesme Modu
pub const VTD_ECAP_DT: u64 = 1 << 2;        // Aygıt-TLB'leri

/// Global komut yazmacı bitleri
pub const VTD_GCMD_TE: u32 = 1 << 31;       // Çeviri Etkinleştirme
pub const VTD_GCMD_SRTP: u32 = 1 << 30;     // Kök Tablo İşaretçisi Ayarla
pub const VTD_GCMD_SFL: u32 = 1 << 29;     // Hata Günlüğü Ayarla
pub const VTD_GCMD_EAFL: u32 = 1 << 28;    // Gelişmiş Hata Günlüğünü Etkinleştir
pub const VTD_GCMD_WBF: u32 = 1 << 27;     // Yazma Buffer Temizleme
pub const VTD_GCMD_IRE: u32 = 1 << 25;     // Kesme Yeniden Yönlendirme Etkinleştir
pub const VTD_GCMD_SIRTP: u32 = 1 << 24;  // Kesme Yeniden Yönl. Tablo İşaretçisi Ayarla

/// Global durum yazmacı bitleri
pub const VTD_GSTS_TES: u32 = 1 << 31;
pub const VTD_GSTS_RTPS: u32 = 1 << 30;
pub const VTD_GSTS_FLS: u32 = 1 << 29;
pub const VTD_GSTS_AFLS: u32 = 1 << 28;
pub const VTD_GSTS_WBFS: u32 = 1 << 27;
pub const VTD_GSTS_IRES: u32 = 1 << 25;
pub const VTD_GSTS_IRTPS: u32 = 1 << 24;

/// IRTE (Kesme Yeniden Yönl. Tablo Girdisi) boyutu
pub const IRTE_SIZE: usize = 16;

/// IRTE bayrakları
pub const IRTE_P: u64 = 1 << 0;             // Mevcut
pub const IRTE_FPD: u64 = 1 << 1;           // Hata İşleme Devre Dışı
pub const IRTE_DM: u64 = 1 << 2;            // Teslim Modu
pub const IRTE_TM: u64 = 1 << 4;            // Tetikleyici Modu
pub const IRTE_RH: u64 = 1 << 6;            // Yönlendirme İpucu

/// Kaynak doğrulama türleri
pub const SVT_NONE: u8 = 0;
pub const SVT_RID: u8 = 1;
pub const SVT_BUS: u8 = 2;

// ============================================================================
// IRTE (Kesme Yeniden Yönl. Tablo Girdisi)
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Irte {
    pub low: u64,
    pub high: u64,
}

impl Irte {
    pub fn new() -> Self {
        Self { low: 0, high: 0 }
    }

    /// Mevcut ayarla
    pub fn set_present(&mut self, present: bool) {
        if present {
            self.low |= IRTE_P;
        } else {
            self.low &= !IRTE_P;
        }
    }

    /// Vektör ayarla
    pub fn set_vector(&mut self, vector: u8) {
        self.low = (self.low & !0xFF00) | ((vector as u64) << 8);
    }

    /// Teslim modunu ayarla
    pub fn set_delivery_mode(&mut self, mode: u8) {
        self.low = (self.low & !(0x7 << 5)) | ((mode as u64) << 5);
    }

    /// Tetikleyici modunu ayarla (0=kenar, 1=seviye)
    pub fn set_trigger_mode(&mut self, level: bool) {
        if level {
            self.low |= IRTE_TM;
        } else {
            self.low &= !IRTE_TM;
        }
    }

    /// Hedef kimliğini ayarla
    pub fn set_dest_id(&mut self, dest: u32) {
        self.high = (self.high & !0xFFFF) | (dest as u64);
    }

    /// Kaynak doğrulamasını ayarla
    pub fn set_source(&mut self, svt: u8, sid: u16, sq: u8) {
        let val = ((svt as u64) << 18) | ((sid as u64) << 32) | ((sq as u64) << 17);
        self.high = (self.high & !(0x3FFFF << 17)) | val;
    }

    /// Kesme yeniden yönl. tanıtıcısını ayarla (yayınlanan kesmeler için)
    pub fn set_ir_handle(&mut self, handle: u64) {
        self.high = (self.high & !0xFFFF0000) | (handle << 16);
    }
}

// ============================================================================
// KES. YENİDEN YÖNL. TABLOSU
// ============================================================================

pub struct IntrRemapTable {
    /// Tablo girdileri
    pub entries: Mutex<Vec<Irte>>,
    /// Fiziksel adres
    pub phys_addr: AtomicU64,
    /// Boyut (girdi sayısı)
    pub size: usize,
}

impl IntrRemapTable {
    pub fn new(size: usize) -> Self {
        let mut entries = Vec::with_capacity(size);
        for _ in 0..size {
            entries.push(Irte::new());
        }

        Self {
            entries: Mutex::new(entries),
            phys_addr: AtomicU64::new(0),
            size,
        }
    }

    /// Girdi al
    pub fn get_entry(&self, index: usize) -> Option<Irte> {
        if index < self.size {
            self.entries.lock().get(index).copied()
        } else {
            None
        }
    }

    /// Girdi ayarla
    pub fn set_entry(&self, index: usize, entry: Irte) -> Result<(), IrError> {
        if index >= self.size {
            return Err(IrError::InvalidIndex);
        }

        self.entries.lock()[index] = entry;
        Ok(())
    }

    /// Boş girdi tahsis et
    pub fn allocate_entry(&self) -> Option<usize> {
        let mut entries = self.entries.lock();

        for (i, entry) in entries.iter().enumerate() {
            if entry.low & IRTE_P == 0 {
                return Some(i);
            }
        }

        None
    }
}

// ============================================================================
// KES. YENİDEN YÖNL. BİRİMİ
// ============================================================================

pub struct IntrRemapUnit {
    /// Birim kimliği
    pub id: u32,
    /// Baz adresi (MMIO)
    pub base_addr: u64,
    /// Yetenek yazmacı
    pub cap: AtomicU64,
    /// Genişletilmiş yetenek yazmacı
    pub ecap: AtomicU64,
    /// Kesme yeniden yönlendirme tablosu
    pub irt: Mutex<Option<Arc<IntrRemapTable>>>,
    /// Etkin mi
    pub enabled: AtomicBool,
    /// Hata kuyruğu
    pub fault_queue: Mutex<Vec<FaultRecord>>,
}

#[derive(Clone, Debug)]
pub struct FaultRecord {
    pub fault_reason: u8,
    pub source_id: u16,
    pub domain_id: u16,
    pub address: u64,
    pub timestamp: u64,
}

impl IntrRemapUnit {
    pub fn new(id: u32, base_addr: u64) -> Self {
        Self {
            id,
            base_addr,
            cap: AtomicU64::new(0),
            ecap: AtomicU64::new(0),
            irt: Mutex::new(None),
            enabled: AtomicBool::new(false),
            fault_queue: Mutex::new(Vec::new()),
        }
    }

    /// Birimi başlat
    pub fn init(&self) -> Result<(), IrError> {
        // Yetenekleri oku
        let cap = self.read_reg(VTD_CAP_REG);
        let ecap = self.read_reg(VTD_ECAP_REG);

        self.cap.store(cap, Ordering::SeqCst);
        self.ecap.store(ecap, Ordering::SeqCst);

        // Kesme yeniden yönlendirmenin desteklenip desteklenmediğini kontrol et
        if ecap & VTD_ECAP_IR == 0 {
            return Err(IrError::NotSupported);
        }

        crate::serial_println!("[IR] Unit {} initialized at {:#x}", self.id, self.base_addr);

        Ok(())
    }

    /// Kesme yeniden yönlendirme tablosu oluştur
    pub fn create_irt(&self, size: usize) -> Arc<IntrRemapTable> {
        let irt = Arc::new(IntrRemapTable::new(size));

        // Tablo işaretçisi ayarla
        let addr = irt.phys_addr.load(Ordering::SeqCst);
        let irta = addr | (size.trailing_zeros() as u64);

        self.write_reg(VTD_IRTA_REG, irta);

        *self.irt.lock() = Some(irt.clone());

        irt
    }

    /// Kesme yeniden yönlendirmeyi etkinleştir
    pub fn enable(&self) -> Result<(), IrError> {
        // Önce kesme yeniden yönl. tablo işaretçisini ayarla
        self.write_reg(VTD_GCMD_REG, VTD_GCMD_SIRTP);

        // Tamamlanmayı bekle
        self.wait_status(VTD_GSTS_IRTPS);

        // Kesme yeniden yönlendirmeyi etkinleştir
        self.write_reg(VTD_GCMD_REG, VTD_GCMD_IRE);

        // Etkinleştirmeyi bekle
        self.wait_status(VTD_GSTS_IRES);

        self.enabled.store(true, Ordering::SeqCst);

        crate::serial_println!("[IR] Interrupt remapping enabled for unit {}", self.id);

        Ok(())
    }

    /// Kesme yeniden yönlendirmeyi devre dışı bırak
    pub fn disable(&self) {
        self.write_reg(VTD_GCMD_REG, 0);
        self.enabled.store(false, Ordering::SeqCst);
    }

    /// Kesmeyi programla
    pub fn program_interrupt(&self, index: usize, vector: u8, dest: u32, trigger: bool) -> Result<(), IrError> {
        let irt = self.irt.lock();
        let table = irt.as_ref().ok_or(IrError::TableNotSet)?;

        let mut entry = Irte::new();
        entry.set_present(true);
        entry.set_vector(vector);
        entry.set_trigger_mode(trigger);
        entry.set_dest_id(dest);
        entry.set_source(SVT_NONE, 0, 0);

        table.set_entry(index, entry)?;

        Ok(())
    }

    /// Hatayı işle
    pub fn handle_fault(&self) -> Option<FaultRecord> {
        let fsts = self.read_reg(VTD_FSTS_REG) as u32;

        if fsts & 0x80000000 != 0 {
            // Hata bekliyor
            let reason = ((fsts >> 1) & 0xFF) as u8;
            let source = ((fsts >> 9) & 0xFFFF) as u16;

            let record = FaultRecord {
                fault_reason: reason,
                source_id: source,
                domain_id: 0,
                address: 0,
                timestamp: crate::task::scheduler::get_ticks(),
            };

            // Hatayı temizle
            self.write_reg(VTD_FSTS_REG, 0xFFFFFFFF);

            self.fault_queue.lock().push(record.clone());

            return Some(record);
        }

        None
    }

    /// Yazmaç oku
    fn read_reg(&self, offset: u32) -> u64 {
        // unsafe {
        //     core::ptr::read_volatile((self.base_addr + offset as u64) as *const u64)
        // }
        0
    }

    /// Yazmaç yaz
    fn write_reg(&self, offset: u32, value: u64) {
        // unsafe {
        //     core::ptr::write_volatile((self.base_addr + offset as u64) as *mut u64, value);
        // }
    }

    /// Durum biti bekle
    fn wait_status(&self, bit: u32) {
        // for _ in 0..1000 {
        //     let status = self.read_reg(VTD_GSTS_REG) as u32;
        //     if status & bit != 0 {
        //         return;
        //     }
        // }
    }
}

// ============================================================================
// KES. YENİDEN YÖNL. YÖNETİCİSİ
// ============================================================================

pub struct IntrRemapManager {
    /// Yeniden yönlendirme birimleri
    pub units: Mutex<BTreeMap<u32, Arc<IntrRemapUnit>>>,
    /// Global kesme indeks tahsis edici
    pub next_index: AtomicU32,
    /// İstatistikler
    pub stats: Mutex<IrStats>,
}

#[derive(Clone, Debug, Default)]
pub struct IrStats {
    pub interrupts_mapped: u64,
    pub faults_handled: u64,
}

impl IntrRemapManager {
    pub const fn new() -> Self {
        Self {
            units: Mutex::new(BTreeMap::new()),
            next_index: AtomicU32::new(0),
            stats: Mutex::new(IrStats::default()),
        }
    }

    /// Birim kaydet
    pub fn register_unit(&self, id: u32, base_addr: u64) -> Result<Arc<IntrRemapUnit>, IrError> {
        let unit = Arc::new(IntrRemapUnit::new(id, base_addr));
        unit.init()?;

        self.units.lock().insert(id, unit.clone());

        Ok(unit)
    }

    /// Birim al
    pub fn get_unit(&self, id: u32) -> Option<Arc<IntrRemapUnit>> {
        self.units.lock().get(&id).cloned()
    }

    /// Kesme indeksi tahsis et
    pub fn allocate_index(&self) -> u32 {
        self.next_index.fetch_add(1, Ordering::SeqCst)
    }

    /// Kesme eşle
    pub fn map_interrupt(&self, unit_id: u32, vector: u8, dest: u32, trigger: bool) -> Result<u32, IrError> {
        let unit = self.get_unit(unit_id).ok_or(IrError::UnitNotFound)?;

        let index = self.allocate_index();
        unit.program_interrupt(index as usize, vector, dest, trigger)?;

        let mut stats = self.stats.lock();
        stats.interrupts_mapped += 1;

        Ok(index)
    }

    /// Hataları işle
    pub fn handle_faults(&self) {
        for unit in self.units.lock().values() {
            while let Some(_fault) = unit.handle_fault() {
                let mut stats = self.stats.lock();
                stats.faults_handled += 1;
            }
        }
    }

    /// İstatistikleri al
    pub fn get_stats(&self) -> IrStats {
        self.stats.lock().clone()
    }
}

lazy_static::lazy_static! {
    pub static ref INTR_REMAP: IntrRemapManager = IntrRemapManager::new();
}

// ============================================================================
// HATA TİPİ
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrError {
    NotSupported,
    UnitNotFound,
    TableNotSet,
    InvalidIndex,
    TableFull,
}

// ============================================================================
// BAŞLATMA
// ============================================================================

pub fn init() {
    crate::serial_println!("[IR] Interrupt remapping manager initialized");
}
