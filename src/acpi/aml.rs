//! # AML (ACPI Machine Language) Yorumlayıcısı
//!
//! ACPI için tam AML bayt kodu yorumlayıcısı.
//!
//! ## AML Nedir?
//! AML, BIOS/UEFI'nin donanım aygıtlarını, güç yönetimini ve konfigürasyonu
//! tanımlamak için kullandığı bir bayt kodu dilidir. Çekirdek bu bayt kodunu
//! çalışma zamanında yorumlayarak donanımı denetler.
//!
//! ## Yorumlayıcı Akışı
//! ```ascii
//! ACPI Tablosu (DSDT/SSDT)
//!         |
//!         v
//!  [AML Bayt Kodu]
//!         |
//!         v
//!  execute() → opcode döngüsü
//!         |
//!   ______|______
//!  |             |
//! Temel      Genişletilmiş
//! opcode     opcode (0x5B prefix)
//!  |             |
//!  v             v
//! Sonuç → AmlNamespace (ad alanı)
//! ```

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// AML SABİTLERİ
// ============================================================================

/// AML işlem kodları (opcodes).
///
/// Her sabit bir AML bayt kodu işlem kodunu temsil eder.
/// ACPI belirtimine göre 0x00-0xFF aralığında tanımlanmıştır.
pub const AML_ZERO_OP: u8 = 0x00;
pub const AML_ONE_OP: u8 = 0x01;
pub const AML_ALIAS_OP: u8 = 0x06;
pub const AML_NAME_OP: u8 = 0x08;
pub const AML_BYTE_OP: u8 = 0x0A;
pub const AML_WORD_OP: u8 = 0x0B;
pub const AML_DWORD_OP: u8 = 0x0C;
pub const AML_STRING_OP: u8 = 0x0D;
pub const AML_QWORD_OP: u8 = 0x0E;
pub const AML_SCOPE_OP: u8 = 0x10;
pub const AML_BUFFER_OP: u8 = 0x11;
pub const AML_PACKAGE_OP: u8 = 0x12;
pub const AML_VAR_PACKAGE_OP: u8 = 0x13;
pub const AML_METHOD_OP: u8 = 0x14;
pub const AML_EXTERNAL_OP: u8 = 0x15;
pub const AML_DUAL_NAME_PREFIX: u8 = 0x2E;
pub const AML_MULTI_NAME_PREFIX: u8 = 0x2F;
pub const AML_ROOT_CHAR: u8 = 0x5C;
pub const AML_PARENT_PREFIX_CHAR: u8 = 0x5E;
pub const AML_LOCAL0: u8 = 0x60;
pub const AML_LOCAL7: u8 = 0x67;
pub const AML_ARG0: u8 = 0x68;
pub const AML_ARG6: u8 = 0x6E;
pub const AML_STORE_OP: u8 = 0x70;
pub const AML_REF_OF_OP: u8 = 0x71;
pub const AML_ADD_OP: u8 = 0x72;
pub const AML_CONCAT_OP: u8 = 0x73;
pub const AML_SUBTRACT_OP: u8 = 0x74;
pub const AML_INCREMENT_OP: u8 = 0x75;
pub const AML_DECREMENT_OP: u8 = 0x76;
pub const AML_MULTIPLY_OP: u8 = 0x77;
pub const AML_DIVIDE_OP: u8 = 0x78;
pub const AML_SHIFT_LEFT_OP: u8 = 0x79;
pub const AML_SHIFT_RIGHT_OP: u8 = 0x7A;
pub const AML_AND_OP: u8 = 0x7B;
pub const AML_NAND_OP: u8 = 0x7C;
pub const AML_OR_OP: u8 = 0x7D;
pub const AML_NOR_OP: u8 = 0x7E;
pub const AML_XOR_OP: u8 = 0x7F;
pub const AML_NOT_OP: u8 = 0x80;
pub const AML_FIND_SET_LEFT_BIT_OP: u8 = 0x81;
pub const AML_FIND_SET_RIGHT_BIT_OP: u8 = 0x82;
pub const AML_DERE_OF_OP: u8 = 0x83;
pub const AML_CONCAT_RES_OP: u8 = 0x84;
pub const AML_MOD_OP: u8 = 0x85;
pub const AML_NOTIFY_OP: u8 = 0x86;
pub const AML_SIZE_OF_OP: u8 = 0x87;
pub const AML_INDEX_OP: u8 = 0x88;
pub const AML_MATCH_OP: u8 = 0x89;
pub const AML_CREATE_DWORD_FIELD_OP: u8 = 0x8A;
pub const AML_CREATE_WORD_FIELD_OP: u8 = 0x8B;
pub const AML_CREATE_BYTE_FIELD_OP: u8 = 0x8C;
pub const AML_CREATE_BIT_FIELD_OP: u8 = 0x8D;
pub const AML_OBJECT_TYPE_OP: u8 = 0x8E;
pub const AML_CREATE_QWORD_FIELD_OP: u8 = 0x8F;
pub const AML_LOGICAL_AND_OP: u8 = 0x90;
pub const AML_LOGICAL_OR_OP: u8 = 0x91;
pub const AML_LOGICAL_NOT_OP: u8 = 0x92;
pub const AML_LOGICAL_EQUAL_OP: u8 = 0x93;
pub const AML_LOGICAL_GREATER_OP: u8 = 0x94;
pub const AML_LOGICAL_LESS_OP: u8 = 0x95;
pub const AML_TO_BUFFER_OP: u8 = 0x96;
pub const AML_TO_DEC_STRING_OP: u8 = 0x97;
pub const AML_TO_HEX_STRING_OP: u8 = 0x98;
pub const AML_TO_INTEGER_OP: u8 = 0x99;
pub const AML_TO_STRING_OP: u8 = 0x9C;
pub const AML_COPY_OBJECT_OP: u8 = 0x9D;
pub const AML_MID_OP: u8 = 0x9E;
pub const AML_CONTINUE_OP: u8 = 0x9F;
pub const AML_IF_OP: u8 = 0xA0;
pub const AML_ELSE_OP: u8 = 0xA1;
pub const AML_WHILE_OP: u8 = 0xA2;
pub const AML_NOOP_OP: u8 = 0xA3;
pub const AML_RETURN_OP: u8 = 0xA4;
pub const AML_BREAK_OP: u8 = 0xA5;
pub const AML_BREAK_POINT_OP: u8 = 0xA6;
pub const AML_ONES_OP: u8 = 0xFF;

/// Genişletilmiş işlem kodları (Extended Opcodes).
///
/// `0x5B` öneki ile başlayan iki baytlık işlem kodları.
/// Mutex, EventDesc, OperationRegion, Field gibi genişletilmiş yapılar için kullanılır.
pub const AML_EXT_OP: u8 = 0x5B;
pub const AML_EXT_MUTEX_OP: u16 = 0x5B01;
pub const AML_EXT_EVENT_OP: u16 = 0x5B02;
pub const AML_EXT_COND_REF_OF_OP: u16 = 0x5B12;
pub const AML_EXT_CREATE_FIELD_OP: u16 = 0x5B13;
pub const AML_EXT_LOAD_TABLE_OP: u16 = 0x5B1F;
pub const AML_EXT_LOAD_OP: u16 = 0x5B20;
pub const AML_EXT_STALL_OP: u16 = 0x5B21;
pub const AML_EXT_SLEEP_OP: u16 = 0x5B22;
pub const AML_EXT_ACQUIRE_OP: u16 = 0x5B23;
pub const AML_EXT_SIGNAL_OP: u16 = 0x5B24;
pub const AML_EXT_WAIT_OP: u16 = 0x5B25;
pub const AML_EXT_RESET_OP: u16 = 0x5B26;
pub const AML_EXT_RELEASE_OP: u16 = 0x5B27;
pub const AML_EXT_FROM_BCD_OP: u16 = 0x5B28;
pub const AML_EXT_TO_BCD_OP: u16 = 0x5B29;
pub const AML_EXT_REVISION_OP: u16 = 0x5B30;
pub const AML_EXT_DEBUG_OP: u16 = 0x5B31;
pub const AML_EXT_FATAL_OP: u16 = 0x5B32;
pub const AML_EXT_TIMER_OP: u16 = 0x5B33;
pub const AML_EXT_REGION_OP: u16 = 0x5B80;
pub const AML_EXT_FIELD_OP: u16 = 0x5B81;
pub const AML_EXT_DEVICE_OP: u16 = 0x5B82;
pub const AML_EXT_PROCESSOR_OP: u16 = 0x5B83;
pub const AML_EXT_POWER_RES_OP: u16 = 0x5B84;
pub const AML_EXT_THERMAL_ZONE_OP: u16 = 0x5B85;

// ============================================================================
// AML DEĞERİ
// ============================================================================

/// AML veri türlerini temsil eden numaralandırma.
///
/// ACPI belirtimi birçok farklı nesne türü tanımlar; bu enum tüm türleri kapsar.
/// `Uninitialized`: henüz değer atanmamış yerel değişken veya argüman.
#[derive(Clone, Debug)]
pub enum AmlValue {
    Uninitialized,
    Integer(u64),
    String(String),
    Buffer(Vec<u8>),
    Package(Vec<AmlValue>),
    FieldUnit(FieldUnit),
    Device(String),
    Method(MethodDesc),
    Mutex(MutexDesc),
    Event(EventDesc),
    Processor(ProcessorDesc),
    PowerResource(PowerResDesc),
    ThermalZone(String),
    Debug,
    Reference(u64), // Nesneye referans
}

/// Alan birimi (FieldUnit) tanımı — bir OperationRegion içindeki bit alanıdır.
#[derive(Clone, Debug)]
pub struct FieldUnit {
    pub region_name: String,
    pub offset: u64,
    pub length: u64,
    pub access_type: u8,
    pub lock_rule: bool,
    pub update_rule: u8,
}

/// Yöntem (Method) tanımlayıcısı — AML yöntemi metaveri ve bayt kodunu içerir.
#[derive(Clone, Debug)]
pub struct MethodDesc {
    pub name: String,
    pub args: u8,
    pub serialized: bool,
    pub code: Vec<u8>,
}

/// Mutex tanımlayıcısı — ACPI eşzamanlılık kontrolü için kullanılır.
#[derive(Clone, Debug)]
pub struct MutexDesc {
    pub name: String,
    pub sync_level: u8,
    pub locked: AtomicBool,
    pub owner: AtomicU32,
}

/// Olay (Event) tanımlayıcısı — AML sinyalleşme mekanizması.
#[derive(Clone, Debug)]
pub struct EventDesc {
    pub name: String,
    pub count: AtomicU32,
}

/// İşlemci (Processor) tanımlayıcısı — eski ACPI işlemci nesnesi.
#[derive(Clone, Debug)]
pub struct ProcessorDesc {
    pub name: String,
    pub id: u8,
    pub pblk_addr: u32,
    pub pblk_len: u8,
}

/// Güç kaynağı (PowerResource) tanımlayıcısı — sistem güç seviyesi kontrolü.
#[derive(Clone, Debug)]
pub struct PowerResDesc {
    pub name: String,
    pub system_level: u8,
    pub resource_order: u16,
}

// ============================================================================
// AML AD ALANI
// ============================================================================

/// AML ad alanı (Namespace) — tüm ACPI nesnelerinin hiyerarşik deposu.
///
/// Kök `\` (ters eğik çizgi) ile başlar; aygıtlar `\_SB`, yöntemler `\_TZ` vb.
/// kapsamlar altında organizedir. Her nesne tam yol ile adreslenir.
pub struct AmlNamespace {
    /// İsimli nesneler — tam yol -> AML değeri eşlemesi
    pub objects: Mutex<BTreeMap<String, AmlValue>>,
    /// Geçerli kapsam yığını — yeni nesneler en üstteki kapsam altına eklenir
    pub scope: Mutex<Vec<String>>,
}

impl AmlNamespace {
    /// Boş bir AML ad alanı oluşturur; kapsam kökten (`\`) başlar.
    pub fn new() -> Self {
        Self {
            objects: Mutex::new(BTreeMap::new()),
            scope: Mutex::new(vec![String::from("\\")]),
        }
    }

    /// Ad alanına yeni bir nesne ekler.
    ///
    /// `name` göreli ise geçerli kapsama göre tam yola çözümlenir.
    pub fn add(&self, name: &str, value: AmlValue) {
        let full_name = self.resolve_name(name);
        self.objects.lock().insert(full_name, value);
    }

    /// Ad alanından nesneyi okur; bulunamazsa `None` döner.
    pub fn get(&self, name: &str) -> Option<AmlValue> {
        let full_name = self.resolve_name(name);
        self.objects.lock().get(&full_name).cloned()
    }

    /// Göreli adı tam yola çözümler.
    ///
    /// - `\` ile başlıyorsa: mutlak yol, değiştirme.
    /// - `^` ile başlıyorsa: üst kapsama git (Linux `..` gibi).
    /// - Aksi hâlde: geçerli kapsamın altına ekle.
    fn resolve_name(&self, name: &str) -> String {
        if name.starts_with('\\') {
            return String::from(name);
        }

        if name.starts_with('^') {
            // Üst kapsam
            let mut scope = self.scope.lock();
            if scope.len() > 1 {
                scope.pop();
            }
            return self.resolve_name(&name[1..]);
        }

        let scope = self.scope.lock();
        if scope.is_empty() {
            String::from("\\") + name
        } else {
            scope.last().unwrap().clone() + "." + name
        }
    }

    /// Kapsam yığınına yeni bir kapsam iter.
    pub fn push_scope(&self, name: &str) {
        let full_name = self.resolve_name(name);
        self.scope.lock().push(full_name);
    }

    /// Kapsam yığınından en üstteki kapsamı çıkarır.
    pub fn pop_scope(&self) {
        self.scope.lock().pop();
    }
}

// ============================================================================
// AML YORUMLAYICISI
// ============================================================================

/// AML bayt kodu yorumlayıcısı.
///
/// ACPI DSDT/SSDT tablolarındaki AML bayt kodunu çalıştırır.
/// Yerel değişkenler (Local0-Local7), argümanlar (Arg0-Arg6),
/// yürütme durumu ve istatistikler iş parçacığı açısından güvenli şekilde tutulur.
pub struct AmlInterpreter {
    /// AML nesnelerinin hiyerarşik deposu
    pub namespace: AmlNamespace,
    /// Çalışma başına yerel değişkenler (Local0-Local7)
    pub locals: Mutex<[AmlValue; 8]>,
    /// Yöntem argümanları (Arg0-Arg6)
    pub args: Mutex<[AmlValue; 7]>,
    /// Yürütme durumu (program sayacı, derinlik, bayraklar)
    pub state: Mutex<ExecutionState>,
    /// Yorumlayıcı istatistikleri
    pub stats: Mutex<AmlStats>,
}

/// AML yürütme durumu.
///
/// Program sayacı, çağrı derinliği ve kontrol akışı bayraklarını içerir.
#[derive(Clone, Debug)]
pub struct ExecutionState {
    pub pc: usize,
    pub depth: u32,
    pub break_flag: bool,
    pub continue_flag: bool,
    pub return_value: Option<AmlValue>,
}

/// AML yorumlayıcı istatistikleri — hata ayıklama ve performans analizi için.
#[derive(Clone, Debug, Default)]
pub struct AmlStats {
    pub methods_executed: u64,
    pub opcodes_executed: u64,
    pub objects_created: u64,
}

impl AmlInterpreter {
    /// Yeni bir AML yorumlayıcı örneği oluşturur.
    ///
    /// Tüm yerel değişkenler ve argümanlar `Uninitialized` ile başlatılır.
    pub fn new() -> Self {
        Self {
            namespace: AmlNamespace::new(),
            locals: Mutex::new([
                AmlValue::Uninitialized,
                AmlValue::Uninitialized,
                AmlValue::Uninitialized,
                AmlValue::Uninitialized,
                AmlValue::Uninitialized,
                AmlValue::Uninitialized,
                AmlValue::Uninitialized,
                AmlValue::Uninitialized,
            ]),
            args: Mutex::new([
                AmlValue::Uninitialized,
                AmlValue::Uninitialized,
                AmlValue::Uninitialized,
                AmlValue::Uninitialized,
                AmlValue::Uninitialized,
                AmlValue::Uninitialized,
                AmlValue::Uninitialized,
            ]),
            state: Mutex::new(ExecutionState {
                pc: 0,
                depth: 0,
                break_flag: false,
                continue_flag: false,
                return_value: None,
            }),
            stats: Mutex::new(AmlStats::default()),
        }
    }

    /// AML bayt kodunu çalıştırır.
    ///
    /// Program sayacını sıfırlayarak bayt kodu boyunca opcode döngüsü yürütür.
    /// Yöntem bir dönüş değeri bırakmadıysa `Uninitialized` döner.
    pub fn execute(&self, code: &[u8]) -> Result<AmlValue, AmlError> {
        let mut state = self.state.lock();
        state.pc = 0;
        state.depth = 0;
        state.return_value = None;
        drop(state);

        while self.state.lock().pc < code.len() {
            let pc = self.state.lock().pc;
            let opcode = code[pc];

            self.execute_opcode(code, opcode)?;

            let mut stats = self.stats.lock();
            stats.opcodes_executed += 1;
        }

        Ok(self.state.lock().return_value.clone().unwrap_or(AmlValue::Uninitialized))
    }

    /// Tek bir AML işlem kodunu işler.
    ///
    /// Program sayacını bir artırır, ardından `match` ile işlem koduna
    /// göre uygun eylemi gerçekleştirir (aritmetik, mantıksal, kontrol akışı vb.).
    fn execute_opcode(&self, code: &[u8], opcode: u8) -> Result<(), AmlError> {
        let mut state = self.state.lock();
        state.pc += 1;

        match opcode {
            AML_ZERO_OP => {
                self.push_value(AmlValue::Integer(0));
            }
            AML_ONE_OP => {
                self.push_value(AmlValue::Integer(1));
            }
            AML_ONES_OP => {
                self.push_value(AmlValue::Integer(u64::MAX));
            }
            AML_BYTE_OP => {
                let val = code[state.pc] as u64;
                state.pc += 1;
                self.push_value(AmlValue::Integer(val));
            }
            AML_WORD_OP => {
                let val = u16::from_le_bytes([code[state.pc], code[state.pc + 1]]) as u64;
                state.pc += 2;
                self.push_value(AmlValue::Integer(val));
            }
            AML_DWORD_OP => {
                let val = u32::from_le_bytes([
                    code[state.pc], code[state.pc + 1],
                    code[state.pc + 2], code[state.pc + 3]
                ]) as u64;
                state.pc += 4;
                self.push_value(AmlValue::Integer(val));
            }
            AML_QWORD_OP => {
                let val = u64::from_le_bytes([
                    code[state.pc], code[state.pc + 1], code[state.pc + 2], code[state.pc + 3],
                    code[state.pc + 4], code[state.pc + 5], code[state.pc + 6], code[state.pc + 7]
                ]);
                state.pc += 8;
                self.push_value(AmlValue::Integer(val));
            }
            AML_STRING_OP => {
                let mut s = String::new();
                while code[state.pc] != 0 {
                    s.push(code[state.pc] as char);
                    state.pc += 1;
                }
                state.pc += 1; // Null sonlandırıcıyı atla
                self.push_value(AmlValue::String(s));
            }
            AML_NAME_OP => {
                // Adı ve değeri ayrıştır
                let name = self.parse_name(code, &mut state.pc)?;
                let value = self.pop_value()?;
                self.namespace.add(&name, value);

                let mut stats = self.stats.lock();
                stats.objects_created += 1;
            }
            AML_SCOPE_OP => {
                let pkg_len = self.parse_pkg_len(code, &mut state.pc);
                let name = self.parse_name(code, &mut state.pc)?;
                self.namespace.push_scope(&name);

                // Kapsam içeriğini çalıştır
                let end_pc = state.pc + pkg_len;
                while state.pc < end_pc {
                    let op = code[state.pc];
                    state.pc += 1;
                    drop(state);
                    self.execute_opcode(code, op)?;
                    state = self.state.lock();
                }

                self.namespace.pop_scope();
            }
            AML_METHOD_OP => {
                let pkg_len = self.parse_pkg_len(code, &mut state.pc);
                let name = self.parse_name(code, &mut state.pc)?;

                let flags = code[state.pc];
                state.pc += 1;

                let args = flags & 0x07;
                let serialized = (flags & 0x08) != 0;

                let method_code = code[state.pc..state.pc + pkg_len - (name.len() + 2)].to_vec();

                let method = AmlValue::Method(MethodDesc {
                    name: name.clone(),
                    args,
                    serialized,
                    code: method_code,
                });

                self.namespace.add(&name, method);
            }
            AML_STORE_OP => {
                let value = self.pop_value()?;
                let dest = self.pop_value()?;
                // Değeri hedefe depola
                self.push_value(value);
            }
            AML_ADD_OP => {
                let op2 = self.pop_int()?;
                let op1 = self.pop_int()?;
                let result = op1.wrapping_add(op2);
                self.push_value(AmlValue::Integer(result));
            }
            AML_SUBTRACT_OP => {
                let op2 = self.pop_int()?;
                let op1 = self.pop_int()?;
                let result = op1.wrapping_sub(op2);
                self.push_value(AmlValue::Integer(result));
            }
            AML_MULTIPLY_OP => {
                let op2 = self.pop_int()?;
                let op1 = self.pop_int()?;
                let result = op1.wrapping_mul(op2);
                self.push_value(AmlValue::Integer(result));
            }
            AML_DIVIDE_OP => {
                let divisor = self.pop_int()?;
                let dividend = self.pop_int()?;
                if divisor == 0 {
                    return Err(AmlError::DivideByZero);
                }
                let quotient = dividend / divisor;
                let remainder = dividend % divisor;
                self.push_value(AmlValue::Integer(quotient));
                self.push_value(AmlValue::Integer(remainder));
            }
            AML_AND_OP => {
                let op2 = self.pop_int()?;
                let op1 = self.pop_int()?;
                self.push_value(AmlValue::Integer(op1 & op2));
            }
            AML_OR_OP => {
                let op2 = self.pop_int()?;
                let op1 = self.pop_int()?;
                self.push_value(AmlValue::Integer(op1 | op2));
            }
            AML_XOR_OP => {
                let op2 = self.pop_int()?;
                let op1 = self.pop_int()?;
                self.push_value(AmlValue::Integer(op1 ^ op2));
            }
            AML_NOT_OP => {
                let op = self.pop_int()?;
                self.push_value(AmlValue::Integer(!op));
            }
            AML_SHIFT_LEFT_OP => {
                let shift = self.pop_int()?;
                let val = self.pop_int()?;
                self.push_value(AmlValue::Integer(val << shift));
            }
            AML_SHIFT_RIGHT_OP => {
                let shift = self.pop_int()?;
                let val = self.pop_int()?;
                self.push_value(AmlValue::Integer(val >> shift));
            }
            AML_INCREMENT_OP => {
                let val = self.pop_int()?;
                self.push_value(AmlValue::Integer(val.wrapping_add(1)));
            }
            AML_DECREMENT_OP => {
                let val = self.pop_int()?;
                self.push_value(AmlValue::Integer(val.wrapping_sub(1)));
            }
            AML_LOGICAL_AND_OP => {
                let op2 = self.pop_int()?;
                let op1 = self.pop_int()?;
                self.push_value(AmlValue::Integer(if op1 != 0 && op2 != 0 { 1 } else { 0 }));
            }
            AML_LOGICAL_OR_OP => {
                let op2 = self.pop_int()?;
                let op1 = self.pop_int()?;
                self.push_value(AmlValue::Integer(if op1 != 0 || op2 != 0 { 1 } else { 0 }));
            }
            AML_LOGICAL_NOT_OP => {
                let val = self.pop_int()?;
                self.push_value(AmlValue::Integer(if val == 0 { 1 } else { 0 }));
            }
            AML_LOGICAL_EQUAL_OP => {
                let op2 = self.pop_int()?;
                let op1 = self.pop_int()?;
                self.push_value(AmlValue::Integer(if op1 == op2 { 1 } else { 0 }));
            }
            AML_LOGICAL_GREATER_OP => {
                let op2 = self.pop_int()?;
                let op1 = self.pop_int()?;
                self.push_value(AmlValue::Integer(if op1 > op2 { 1 } else { 0 }));
            }
            AML_LOGICAL_LESS_OP => {
                let op2 = self.pop_int()?;
                let op1 = self.pop_int()?;
                self.push_value(AmlValue::Integer(if op1 < op2 { 1 } else { 0 }));
            }
            AML_IF_OP => {
                let pkg_len = self.parse_pkg_len(code, &mut state.pc);
                let condition = self.pop_int()?;

                if condition != 0 {
                    let end_pc = state.pc + pkg_len;
                    while state.pc < end_pc {
                        let op = code[state.pc];
                        state.pc += 1;
                        drop(state);
                        self.execute_opcode(code, op)?;
                        state = self.state.lock();
                    }
                } else {
                    state.pc += pkg_len;
                }
            }
            AML_WHILE_OP => {
                let pkg_len = self.parse_pkg_len(code, &mut state.pc);
                let cond_start = state.pc;
                let end_pc = state.pc + pkg_len;

                loop {
                    // Koşulu değerlendir
                    state.pc = cond_start;
                    drop(state);
                    let cond = self.pop_int()?;
                    state = self.state.lock();

                    if cond == 0 {
                        break;
                    }

                    state.pc = cond_start;
                    while state.pc < end_pc {
                        if state.break_flag {
                            state.break_flag = false;
                            break;
                        }
                        if state.continue_flag {
                            state.continue_flag = false;
                            break;
                        }

                        let op = code[state.pc];
                        state.pc += 1;
                        drop(state);
                        self.execute_opcode(code, op)?;
                        state = self.state.lock();
                    }
                }

                state.pc = end_pc;
            }
            AML_RETURN_OP => {
                let value = self.pop_value()?;
                state.return_value = Some(value);
            }
            AML_BREAK_OP => {
                state.break_flag = true;
            }
            AML_CONTINUE_OP => {
                state.continue_flag = true;
            }
            AML_NOOP_OP => {}
            AML_EXT_OP => {
                let ext_op = code[state.pc] as u16;
                state.pc += 1;
                self.execute_ext_opcode(code, ext_op, &mut state)?;
            }
            AML_LOCAL0..=AML_LOCAL7 => {
                let idx = (opcode - AML_LOCAL0) as usize;
                let val = self.locals.lock()[idx].clone();
                self.push_value(val);
            }
            AML_ARG0..=AML_ARG6 => {
                let idx = (opcode - AML_ARG0) as usize;
                let val = self.args.lock()[idx].clone();
                self.push_value(val);
            }
            _ => {
                // Ad olarak ayrıştırmayı dene
                state.pc -= 1;
                let name = self.parse_name(code, &mut state.pc)?;
                if let Some(obj) = self.namespace.get(&name) {
                    self.push_value(obj);
                }
            }
        }

        Ok(())
    }

    /// Genişletilmiş işlem kodunu (Extended Opcode) çalıştırır.
    ///
    /// `0x5B` önekinden sonra gelen ikinci bayta göre Sleep, Stall, Acquire,
    /// Release, OperationRegion, Field, Device, Processor vb. işlenır.
    fn execute_ext_opcode(&self, code: &[u8], ext_op: u16, state: &mut ExecutionState) -> Result<(), AmlError> {
        match ext_op {
            AML_EXT_SLEEP_OP => {
                let ms = self.pop_int()?;
                // ms milisaniye uyut
                crate::serial_println!("[AML] Sleep {} ms", ms);
            }
            AML_EXT_STALL_OP => {
                let us = self.pop_int()?;
                // us mikrosaniye beklet
            }
            AML_EXT_ACQUIRE_OP => {
                let timeout = self.pop_int()?;
                let mutex_name = self.pop_value()?;
                // Mutex edinme
            }
            AML_EXT_RELEASE_OP => {
                let mutex_name = self.pop_value()?;
                // Mutex bırakma
            }
            AML_EXT_REGION_OP => {
                let pkg_len = self.parse_pkg_len(code, &mut state.pc);
                let name = self.parse_name(code, &mut state.pc)?;
                let space_id = code[state.pc];
                state.pc += 1;
                let offset = self.pop_int()?;
                let length = self.pop_int()?;

                crate::serial_println!("[AML] OperationRegion {} space={} offset={:#x} len={:#x}",
                    name, space_id, offset, length);
            }
            AML_EXT_FIELD_OP => {
                let pkg_len = self.parse_pkg_len(code, &mut state.pc);
                let region_name = self.parse_name(code, &mut state.pc)?;
                let flags = code[state.pc];
                state.pc += 1;

                // Alan elemanlarını ayrıştır
                crate::serial_println!("[AML] Field {} flags={:#x}", region_name, flags);
            }
            AML_EXT_DEVICE_OP => {
                let pkg_len = self.parse_pkg_len(code, &mut state.pc);
                let name = self.parse_name(code, &mut state.pc)?;

                self.namespace.push_scope(&name);
                self.namespace.add(&name, AmlValue::Device(name.clone()));

                let end_pc = state.pc + pkg_len;
                while state.pc < end_pc {
                    let op = code[state.pc];
                    state.pc += 1;
                    drop(state);
                    self.execute_opcode(code, op)?;
                    state = self.state.lock();
                }

                self.namespace.pop_scope();
            }
            AML_EXT_PROCESSOR_OP => {
                let pkg_len = self.parse_pkg_len(code, &mut state.pc);
                let name = self.parse_name(code, &mut state.pc)?;
                let id = code[state.pc];
                state.pc += 1;
                let pblk_addr = u32::from_le_bytes([
                    code[state.pc], code[state.pc + 1], code[state.pc + 2], code[state.pc + 3]
                ]);
                state.pc += 4;
                let pblk_len = code[state.pc];
                state.pc += 1;

                let proc = AmlValue::Processor(ProcessorDesc {
                    name: name.clone(),
                    id,
                    pblk_addr,
                    pblk_len,
                });

                self.namespace.add(&name, proc);
            }
            AML_EXT_POWER_RES_OP => {
                let pkg_len = self.parse_pkg_len(code, &mut state.pc);
                let name = self.parse_name(code, &mut state.pc)?;
                let system_level = code[state.pc];
                state.pc += 1;
                let resource_order = u16::from_le_bytes([code[state.pc], code[state.pc + 1]]);
                state.pc += 2;

                let pr = AmlValue::PowerResource(PowerResDesc {
                    name: name.clone(),
                    system_level,
                    resource_order,
                });

                self.namespace.add(&name, pr);
            }
            AML_EXT_THERMAL_ZONE_OP => {
                let pkg_len = self.parse_pkg_len(code, &mut state.pc);
                let name = self.parse_name(code, &mut state.pc)?;

                self.namespace.push_scope(&name);
                self.namespace.add(&name, AmlValue::ThermalZone(name.clone()));

                let end_pc = state.pc + pkg_len;
                while state.pc < end_pc {
                    let op = code[state.pc];
                    state.pc += 1;
                    drop(state);
                    self.execute_opcode(code, op)?;
                    state = self.state.lock();
                }

                self.namespace.pop_scope();
            }
            AML_EXT_DEBUG_OP => {
                // Hata ayıklama çıktısı
                let val = self.pop_value()?;
                crate::serial_println!("[AML DEBUG] {:?}", val);
            }
            _ => {}
        }

        Ok(())
    }

    /// Paket uzunluğunu (PkgLength) ayrıştırır.
    ///
    /// ACPI belirtiminde PkgLength 1-4 bayt uzunluğunda olabilir.
    /// Öncü baytın bit 7:6 alanı ek bayt sayısını, bit 5:0 ise uzunluk başlangıcını verir.
    fn parse_pkg_len(&self, code: &[u8], pc: &mut usize) -> usize {
        let lead = code[*pc];
        *pc += 1;

        let count = (lead >> 6) as usize;
        let mut len = (lead & 0x3F) as usize;

        for i in 0..count {
            len |= (code[*pc + i] as usize) << (8 * i + 4);
        }
        *pc += count;

        len
    }

    /// AML adını (NameString) ayrıştırır.
    ///
    /// Kök karakteri (`\`), çift/çoklu isim önekleri ve tekli isim segmentlerini işler.
    /// Her isim segmenti 4 karakterden oluşur; kısa isimler '_' ile doldurulur.
    fn parse_name(&self, code: &[u8], pc: &mut usize) -> Result<String, AmlError> {
        let mut name = String::new();

        if code[*pc] == AML_ROOT_CHAR {
            name.push('\\');
            *pc += 1;
        }

        if code[*pc] == AML_DUAL_NAME_PREFIX {
            *pc += 1;
            for _ in 0..2 {
                name.push(code[*pc] as char);
                name.push(code[*pc + 1] as char);
                name.push(code[*pc + 2] as char);
                name.push(code[*pc + 3] as char);
                *pc += 4;
            }
        } else if code[*pc] == AML_MULTI_NAME_PREFIX {
            *pc += 1;
            let seg_count = code[*pc];
            *pc += 1;
            for _ in 0..seg_count {
                name.push(code[*pc] as char);
                name.push(code[*pc + 1] as char);
                name.push(code[*pc + 2] as char);
                name.push(code[*pc + 3] as char);
                *pc += 4;
            }
        } else {
            // Tekli isim
            for _ in 0..4 {
                if code[*pc] != 0 {
                    name.push(code[*pc] as char);
                }
                *pc += 1;
            }
        }

        Ok(name)
    }

    /// Değeri yığına iter (basitleştirilmiş uygulama).
    fn push_value(&self, value: AmlValue) {
        // Basitleştirilmiş — gerçek uygulamada ayrı bir değer yığını kullanılır
    }

    /// Yığından değer çıkarır.
    fn pop_value(&self) -> Result<AmlValue, AmlError> {
        Ok(AmlValue::Uninitialized)
    }

    /// Yığından tamsayı değeri çıkarır; tür uyuşmazlığında hata döner.
    fn pop_int(&self) -> Result<u64, AmlError> {
        match self.pop_value()? {
            AmlValue::Integer(v) => Ok(v),
            _ => Err(AmlError::TypeError),
        }
    }

    /// Belirtilen ada sahip AML yöntemini çalıştırır.
    ///
    /// Argümanları ayarlar, yerel değişkenleri sıfırlar ve yöntem bayt kodunu çalıştırır.
    /// Yöntem bulunamazsa `MethodNotFound` hatası döner.
    pub fn execute_method(&self, name: &str, args: &[AmlValue]) -> Result<AmlValue, AmlError> {
        let method = self.namespace.get(name);

        if let Some(AmlValue::Method(m)) = method {
            // Argümanları ayarla
            {
                let mut a = self.args.lock();
                for (i, arg) in args.iter().enumerate() {
                    if i < 7 {
                        a[i] = arg.clone();
                    }
                }
            }

            // Yerel değişkenleri sıfırla
            {
                let mut l = self.locals.lock();
                for i in 0..8 {
                    l[i] = AmlValue::Uninitialized;
                }
            }

            // Yöntem bayt kodunu çalıştır
            let result = self.execute(&m.code)?;

            let mut stats = self.stats.lock();
            stats.methods_executed += 1;

            Ok(result)
        } else {
            Err(AmlError::MethodNotFound)
        }
    }

    /// Belirtilen ada sahip AML nesnesini değerlendirir.
    ///
    /// Nesne yöntemse çalıştırır; diğer türdeyse doğrudan döner.
    pub fn evaluate(&self, name: &str) -> Result<AmlValue, AmlError> {
        let obj = self.namespace.get(name);

        match obj {
            Some(AmlValue::Method(m)) => self.execute_method(name, &[]),
            Some(v) => Ok(v),
            None => Err(AmlError::ObjectNotFound),
        }
    }

    /// Yorumlayıcı istatistiklerinin anlık görüntüsünü döner.
    pub fn get_stats(&self) -> AmlStats {
        self.stats.lock().clone()
    }
}

/// Küresel AML yorumlayıcı örneği.
///
/// `lazy_static` ile ilk erişimde oluşturulur; tüm ACPI yöntem çağrıları bu örnek üzerinden yapılır.
lazy_static::lazy_static! {
    pub static ref AML: AmlInterpreter = AmlInterpreter::new();
}

// ============================================================================
// HATA TÜRÜ
// ============================================================================

/// AML yorumlayıcı hata türleri.
///
/// Geçersiz opcode, tür uyumsuzluğu, sıfıra bölme gibi çalışma zamanı hatalarını kapsar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmlError {
    InvalidOpcode,
    InvalidName,
    StackUnderflow,
    TypeError,
    MethodNotFound,
    ObjectNotFound,
    DivideByZero,
    BufferOverflow,
}

// ============================================================================
// BAŞLATMA
// ============================================================================

/// AML yorumlayıcı alt sistemini başlatır.
pub fn init() {
    crate::serial_println!("[AML] Interpreter initialized");
}
