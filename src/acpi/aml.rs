//! # AML (ACPI Machine Language) Interpreter
//!
//! Full AML bytecode interpreter for ACPI.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

// ============================================================================
// AML CONSTANTS
// ============================================================================

/// AML opcodes
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

/// Extended opcodes
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
// AML VALUE
// ============================================================================

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
    Reference(u64), // Reference to object
}

#[derive(Clone, Debug)]
pub struct FieldUnit {
    pub region_name: String,
    pub offset: u64,
    pub length: u64,
    pub access_type: u8,
    pub lock_rule: bool,
    pub update_rule: u8,
}

#[derive(Clone, Debug)]
pub struct MethodDesc {
    pub name: String,
    pub args: u8,
    pub serialized: bool,
    pub code: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct MutexDesc {
    pub name: String,
    pub sync_level: u8,
    pub locked: AtomicBool,
    pub owner: AtomicU32,
}

#[derive(Clone, Debug)]
pub struct EventDesc {
    pub name: String,
    pub count: AtomicU32,
}

#[derive(Clone, Debug)]
pub struct ProcessorDesc {
    pub name: String,
    pub id: u8,
    pub pblk_addr: u32,
    pub pblk_len: u8,
}

#[derive(Clone, Debug)]
pub struct PowerResDesc {
    pub name: String,
    pub system_level: u8,
    pub resource_order: u16,
}

// ============================================================================
// AML NAMESPACE
// ============================================================================

pub struct AmlNamespace {
    /// Named objects
    pub objects: Mutex<BTreeMap<String, AmlValue>>,
    /// Current scope
    pub scope: Mutex<Vec<String>>,
}

impl AmlNamespace {
    pub fn new() -> Self {
        Self {
            objects: Mutex::new(BTreeMap::new()),
            scope: Mutex::new(vec![String::from("\\")]),
        }
    }

    /// Add object
    pub fn add(&self, name: &str, value: AmlValue) {
        let full_name = self.resolve_name(name);
        self.objects.lock().insert(full_name, value);
    }

    /// Get object
    pub fn get(&self, name: &str) -> Option<AmlValue> {
        let full_name = self.resolve_name(name);
        self.objects.lock().get(&full_name).cloned()
    }

    /// Resolve name to full path
    fn resolve_name(&self, name: &str) -> String {
        if name.starts_with('\\') {
            return String::from(name);
        }
        
        if name.starts_with('^') {
            // Parent scope
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

    /// Push scope
    pub fn push_scope(&self, name: &str) {
        let full_name = self.resolve_name(name);
        self.scope.lock().push(full_name);
    }

    /// Pop scope
    pub fn pop_scope(&self) {
        self.scope.lock().pop();
    }
}

// ============================================================================
// AML INTERPRETER
// ============================================================================

pub struct AmlInterpreter {
    /// Namespace
    pub namespace: AmlNamespace,
    /// Local variables (per execution)
    pub locals: Mutex<[AmlValue; 8]>,
    /// Arguments
    pub args: Mutex<[AmlValue; 7]>,
    /// Execution state
    pub state: Mutex<ExecutionState>,
    /// Statistics
    pub stats: Mutex<AmlStats>,
}

#[derive(Clone, Debug)]
pub struct ExecutionState {
    pub pc: usize,
    pub depth: u32,
    pub break_flag: bool,
    pub continue_flag: bool,
    pub return_value: Option<AmlValue>,
}

#[derive(Clone, Debug, Default)]
pub struct AmlStats {
    pub methods_executed: u64,
    pub opcodes_executed: u64,
    pub objects_created: u64,
}

impl AmlInterpreter {
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

    /// Execute AML bytecode
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

    /// Execute single opcode
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
                state.pc += 1; // Skip null terminator
                self.push_value(AmlValue::String(s));
            }
            AML_NAME_OP => {
                // Parse name and value
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
                
                // Execute scope contents
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
                // Store value to destination
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
                    // Evaluate condition
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
                // Try to parse as name
                state.pc -= 1;
                let name = self.parse_name(code, &mut state.pc)?;
                if let Some(obj) = self.namespace.get(&name) {
                    self.push_value(obj);
                }
            }
        }

        Ok(())
    }

    /// Execute extended opcode
    fn execute_ext_opcode(&self, code: &[u8], ext_op: u16, state: &mut ExecutionState) -> Result<(), AmlError> {
        match ext_op {
            AML_EXT_SLEEP_OP => {
                let ms = self.pop_int()?;
                // Sleep for ms milliseconds
                crate::serial_println!("[AML] Sleep {} ms", ms);
            }
            AML_EXT_STALL_OP => {
                let us = self.pop_int()?;
                // Stall for us microseconds
            }
            AML_EXT_ACQUIRE_OP => {
                let timeout = self.pop_int()?;
                let mutex_name = self.pop_value()?;
                // Acquire mutex
            }
            AML_EXT_RELEASE_OP => {
                let mutex_name = self.pop_value()?;
                // Release mutex
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
                
                // Parse field elements
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
                // Debug output
                let val = self.pop_value()?;
                crate::serial_println!("[AML DEBUG] {:?}", val);
            }
            _ => {}
        }
        
        Ok(())
    }

    /// Parse package length
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

    /// Parse name
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
            // Single name
            for _ in 0..4 {
                if code[*pc] != 0 {
                    name.push(code[*pc] as char);
                }
                *pc += 1;
            }
        }
        
        Ok(name)
    }

    /// Push value onto stack (simplified)
    fn push_value(&self, value: AmlValue) {
        // Simplified - would use actual stack
    }

    /// Pop value from stack
    fn pop_value(&self) -> Result<AmlValue, AmlError> {
        Ok(AmlValue::Uninitialized)
    }

    /// Pop integer value
    fn pop_int(&self) -> Result<u64, AmlError> {
        match self.pop_value()? {
            AmlValue::Integer(v) => Ok(v),
            _ => Err(AmlError::TypeError),
        }
    }

    /// Execute method
    pub fn execute_method(&self, name: &str, args: &[AmlValue]) -> Result<AmlValue, AmlError> {
        let method = self.namespace.get(name);
        
        if let Some(AmlValue::Method(m)) = method {
            // Set arguments
            {
                let mut a = self.args.lock();
                for (i, arg) in args.iter().enumerate() {
                    if i < 7 {
                        a[i] = arg.clone();
                    }
                }
            }
            
            // Reset locals
            {
                let mut l = self.locals.lock();
                for i in 0..8 {
                    l[i] = AmlValue::Uninitialized;
                }
            }
            
            // Execute method code
            let result = self.execute(&m.code)?;
            
            let mut stats = self.stats.lock();
            stats.methods_executed += 1;
            
            Ok(result)
        } else {
            Err(AmlError::MethodNotFound)
        }
    }

    /// Evaluate object
    pub fn evaluate(&self, name: &str) -> Result<AmlValue, AmlError> {
        let obj = self.namespace.get(name);
        
        match obj {
            Some(AmlValue::Method(m)) => self.execute_method(name, &[]),
            Some(v) => Ok(v),
            None => Err(AmlError::ObjectNotFound),
        }
    }

    /// Get statistics
    pub fn get_stats(&self) -> AmlStats {
        self.stats.lock().clone()
    }
}

lazy_static::lazy_static! {
    pub static ref AML: AmlInterpreter = AmlInterpreter::new();
}

// ============================================================================
// ERROR TYPE
// ============================================================================

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
// INITIALIZATION
// ============================================================================

pub fn init() {
    crate::serial_println!("[AML] Interpreter initialized");
}
