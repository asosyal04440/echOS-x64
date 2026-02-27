use alloc::string::String;
use core::ffi::{c_char, c_void, CStr};
use core::fmt::Write;
use core::hash::{BuildHasherDefault, Hasher};
use core::ptr::NonNull;
use hashbrown::HashMap;
use lazy_static::lazy_static;
use spin::Mutex;
use virtio_drivers::{BufferDirection, Hal, PhysAddr};

type ShimIrqHandler = unsafe extern "C" fn(u32, *mut c_void) -> i32;
static IRQ_SHIM_HANDLERS: Mutex<[Option<ShimIrqHandler>; 256]> = Mutex::new([None; 256]);
static IRQ_SHIM_DEVS: Mutex<[usize; 256]> = Mutex::new([0; 256]);

fn irq_to_vector(irq: u32) -> Option<u8> {
    if irq > u8::MAX as u32 {
        return None;
    }
    if irq < 32 {
        Some((irq as u8).wrapping_add(32))
    } else {
        Some(irq as u8)
    }
}

fn shim_irq_trampoline(vector: u8) {
    let handler = { IRQ_SHIM_HANDLERS.lock()[vector as usize] };
    if let Some(func) = handler {
        let dev = IRQ_SHIM_DEVS.lock()[vector as usize] as *mut c_void;
        unsafe {
            func(vector as u32, dev);
        }
    }
}

fn c_str_to_str(ptr: *const c_char) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe { CStr::from_ptr(ptr).to_str().unwrap_or("") }
}

macro_rules! build_message {
    ($prefix:expr, $fmt:expr, $args:ident) => {{
        let mut out = String::new();
        if !$prefix.is_empty() {
            out.push_str($prefix);
        }
        let fmt_str = c_str_to_str($fmt);
        let bytes = fmt_str.as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if bytes[i] != b'%' {
                out.push(bytes[i] as char);
                i += 1;
                continue;
            }
            if i + 1 < bytes.len() && bytes[i + 1] == b'%' {
                out.push('%');
                i += 2;
                continue;
            }
            i += 1;
            let mut long = false;
            let mut longlong = false;
            if i < bytes.len() && bytes[i] == b'l' {
                long = true;
                i += 1;
                if i < bytes.len() && bytes[i] == b'l' {
                    longlong = true;
                    i += 1;
                }
            }
            if i >= bytes.len() {
                break;
            }
            match bytes[i] as char {
                's' => {
                    let s_ptr: *const c_char = $args.arg();
                    let s = c_str_to_str(s_ptr);
                    out.push_str(s);
                }
                'd' | 'i' => {
                    if longlong {
                        let v: i64 = $args.arg();
                        write!(out, "{}", v).unwrap();
                    } else if long {
                        let v: i64 = $args.arg();
                        write!(out, "{}", v).unwrap();
                    } else {
                        let v: i32 = $args.arg();
                        write!(out, "{}", v).unwrap();
                    }
                }
                'u' => {
                    if longlong {
                        let v: u64 = $args.arg();
                        write!(out, "{}", v).unwrap();
                    } else if long {
                        let v: u64 = $args.arg();
                        write!(out, "{}", v).unwrap();
                    } else {
                        let v: u32 = $args.arg();
                        write!(out, "{}", v).unwrap();
                    }
                }
                'x' => {
                    if longlong {
                        let v: u64 = $args.arg();
                        write!(out, "{:x}", v).unwrap();
                    } else if long {
                        let v: u64 = $args.arg();
                        write!(out, "{:x}", v).unwrap();
                    } else {
                        let v: u32 = $args.arg();
                        write!(out, "{:x}", v).unwrap();
                    }
                }
                'X' => {
                    if longlong {
                        let v: u64 = $args.arg();
                        write!(out, "{:X}", v).unwrap();
                    } else if long {
                        let v: u64 = $args.arg();
                        write!(out, "{:X}", v).unwrap();
                    } else {
                        let v: u32 = $args.arg();
                        write!(out, "{:X}", v).unwrap();
                    }
                }
                'p' => {
                    let v: *const c_void = $args.arg();
                    write!(out, "{:p}", v).unwrap();
                }
                'c' => {
                    let v: i32 = $args.arg();
                    let ch = core::char::from_u32(v as u32).unwrap_or('?');
                    out.push(ch);
                }
                _ => {
                    out.push('%');
                    out.push(bytes[i] as char);
                }
            }
            i += 1;
        }
        out
    }};
}

#[no_mangle]
pub unsafe extern "C" fn kmalloc(size: usize, _flags: u32) -> *mut c_void {
    crate::allocator::heap_alloc(size) as *mut c_void
}

#[no_mangle]
pub unsafe extern "C" fn kfree(ptr: *mut c_void) {
    crate::allocator::heap_free(ptr as *mut u8)
}

#[no_mangle]
pub unsafe extern "C" fn ioremap(phys_addr: u64, size: usize) -> *mut c_void {
    crate::memory::map_mmio(phys_addr, size) as *mut c_void
}

#[no_mangle]
pub unsafe extern "C" fn printk(fmt: *const c_char, mut args: ...) -> i32 {
    let out = build_message!("", fmt, args);
    crate::serial_print!("{}", out);
    out.len() as i32
}

#[no_mangle]
pub unsafe extern "C" fn dev_info(
    _dev: *mut crate::linux_glue::Device,
    fmt: *const c_char,
    mut args: ...
) -> i32 {
    let out = build_message!("INFO: ", fmt, args);
    crate::serial_print!("{}", out);
    out.len() as i32
}

#[no_mangle]
pub unsafe extern "C" fn dev_err(
    _dev: *mut crate::linux_glue::Device,
    fmt: *const c_char,
    mut args: ...
) -> i32 {
    let out = build_message!("ERROR: ", fmt, args);
    crate::serial_print!("{}", out);
    out.len() as i32
}

#[no_mangle]
pub unsafe extern "C" fn pci_enable_device(dev: *mut crate::linux_glue::PciDev) -> i32 {
    unsafe { crate::drivers::pci::enable_bus_master(dev) }
}

#[no_mangle]
pub unsafe extern "C" fn pci_request_regions(
    dev: *mut crate::linux_glue::PciDev,
    _name: *const c_char,
) -> i32 {
    unsafe { crate::drivers::pci::request_regions(dev) }
}

#[no_mangle]
pub unsafe extern "C" fn request_irq(
    irq: u32,
    handler: *const c_void,
    flags: u64,
    _name: *const c_char,
    dev: *mut c_void,
) -> i32 {
    if handler.is_null() {
        return -22;
    }
    let vector = match irq_to_vector(irq) {
        Some(vector) => vector,
        None => return -22,
    };
    let shim_handler: ShimIrqHandler = core::mem::transmute(handler);
    IRQ_SHIM_HANDLERS.lock()[vector as usize] = Some(shim_handler);
    IRQ_SHIM_DEVS.lock()[vector as usize] = dev as usize;
    if crate::interrupts::request_irq(vector, shim_irq_trampoline, flags) {
        0
    } else {
        -1
    }
}

#[no_mangle]
pub unsafe extern "C" fn free_irq(irq: u32, _dev: *mut c_void) {
    let vector = match irq_to_vector(irq) {
        Some(vector) => vector,
        None => return,
    };
    IRQ_SHIM_HANDLERS.lock()[vector as usize] = None;
    IRQ_SHIM_DEVS.lock()[vector as usize] = 0;
    crate::interrupts::free_irq(vector);
}

pub struct VirtioHal;

unsafe impl Hal for VirtioHal {
    fn dma_alloc(pages: usize, _direction: BufferDirection) -> (PhysAddr, NonNull<u8>) {
        let domain = crate::cpu::smp::current_dma_domain();
        match crate::memory::dma_alloc_for_domain(domain, pages) {
            Some((paddr, vaddr)) => (paddr, vaddr),
            None => (0, NonNull::dangling()),
        }
    }

    unsafe fn dma_dealloc(paddr: PhysAddr, vaddr: NonNull<u8>, pages: usize) -> i32 {
        if paddr == 0 || pages == 0 {
            return -1;
        }
        let domain = crate::cpu::smp::current_dma_domain();
        crate::memory::dma_dealloc_for_domain(domain, paddr, pages);
        let _ = vaddr;
        0
    }

    unsafe fn mmio_phys_to_virt(paddr: PhysAddr, size: usize) -> NonNull<u8> {
        if !crate::ironshim_bridge::is_mmio_allowed(paddr as usize, size) {
            crate::serial_println!(
                "[IronShim/MMIO] map denied: base={:#x} size={}",
                paddr,
                size
            );
            return NonNull::dangling();
        }
        let ptr = crate::memory::map_mmio(paddr as u64, size);
        NonNull::new(ptr).unwrap()
    }

    unsafe fn share(buffer: NonNull<[u8]>, _direction: BufferDirection) -> PhysAddr {
        let domain = crate::cpu::smp::current_dma_domain();
        crate::memory::dma_share_for_domain(domain, buffer).unwrap_or(0)
    }

    unsafe fn unshare(_paddr: PhysAddr, buffer: NonNull<[u8]>, _direction: BufferDirection) {
        let domain = crate::cpu::smp::current_dma_domain();
        crate::memory::dma_unshare_for_domain(domain, buffer);
    }
}

pub struct StrHasher(u64);

impl Default for StrHasher {
    fn default() -> Self {
        Self(0xcbf29ce484222325)
    }
}

impl Hasher for StrHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        let mut hash = self.0;
        for &b in bytes {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        self.0 = hash;
    }
}

type LinuxHashMap = HashMap<&'static str, usize, BuildHasherDefault<StrHasher>>;

lazy_static! {
    pub static ref LINUX_KERNEL_SYMBOLS: Mutex<LinuxHashMap> = {
        let mut map: LinuxHashMap = HashMap::with_hasher(BuildHasherDefault::default());
        map.insert("printk", printk as *const () as usize);
        map.insert("kmalloc", kmalloc as *const () as usize);
        map.insert("kfree", kfree as *const () as usize);
        map.insert("ioremap", ioremap as *const () as usize);
        map.insert("dev_info", dev_info as *const () as usize);
        map.insert("dev_err", dev_err as *const () as usize);
        map.insert("pci_enable_device", pci_enable_device as *const () as usize);
        map.insert(
            "pci_request_regions",
            pci_request_regions as *const () as usize,
        );
        map.insert(
            "pci_register_driver",
            crate::linux_glue::pci_register_driver as *const () as usize,
        );
        map.insert(
            "pci_unregister_driver",
            crate::linux_glue::pci_unregister_driver as *const () as usize,
        );
        map.insert("request_irq", request_irq as *const () as usize);
        map.insert("free_irq", free_irq as *const () as usize);
        Mutex::new(map)
    };
}

pub fn resolve_symbol(name: &str) -> Option<usize> {
    LINUX_KERNEL_SYMBOLS.lock().get(name).copied()
}

// ============================================================================
// IRONSHIM-RS ENTEGRASYONU — Capability-Based Sürücü İzolasyonu
// ============================================================================
//
// IronShim-rs, sürücülerin donanıma erişimini Capability (Yetki) bazlı kontrol
// eder. Bir sürücü sadece Manifest'inde tanımlı DMA/MMIO/Port I/O bölgelerine
// erişebilir. Aksi halde kernel anında reddeder.

use ironshim_rs::{
    // DMA
    DmaAllocator, DmaHandle, PhysAddr as ShimPhysAddr,
    // Interrupt
    InterruptHandler, InterruptRegistry, InterruptBudget, InterruptMetrics,
    // Resource / PCI
    KernelPciBridge, PciConfigAccess, PciTopology, PciAddress, PortIo,
    // Policy & Audit
    SyscallPolicy, SyscallRequest, AuditSink, AuditEvent,
    // Errors
    Error as ShimError,
};

// ---------------------------------------------------------------------------
// 1. DMA Allocator — echOS Bellek Yöneticisine Bağlantı
// ---------------------------------------------------------------------------

/// echOS'un fiziksel bellek yöneticisini IronShim DMA arayüzüne bağlar.
pub struct EchOsDmaAllocator;

impl DmaAllocator for EchOsDmaAllocator {
    fn alloc<T>(&self, count: usize) -> Result<DmaHandle<'_, T, Self>, ShimError>
    where
        Self: Sized,
    {
        let size = core::mem::size_of::<T>().checked_mul(count).ok_or(ShimError::OutOfMemory)?;
        let pages = (size + 4095) / 4096;
        let domain = crate::cpu::smp::current_dma_domain();
        match crate::memory::dma_alloc_for_domain(domain, pages) {
            Some((paddr, vaddr)) => {
                DmaHandle::from_raw(self, vaddr.as_ptr() as *mut T, paddr, count)
            }
            None => Err(ShimError::OutOfMemory),
        }
    }

    fn free<T>(&self, phys: ShimPhysAddr, count: usize) {
        let size = core::mem::size_of::<T>().saturating_mul(count);
        let pages = (size + 4095) / 4096;
        let domain = crate::cpu::smp::current_dma_domain();
        crate::memory::dma_dealloc_for_domain(domain, phys, pages);
    }
}

// ---------------------------------------------------------------------------
// 2. Interrupt Registry — echOS IRQ Sistemine Bağlantı
// ---------------------------------------------------------------------------

/// echOS'un kesme (interrupt) yöneticisini IronShim'e bağlar.
pub struct EchOsInterruptRegistry {
    budgets: Mutex<[InterruptBudget; 256]>,
    calls: Mutex<[u32; 256]>,
    quarantined: Mutex<[bool; 256]>,
    handlers: Mutex<[Option<*mut dyn InterruptHandler>; 256]>,
}

// Safety: handlers array is accessed under lock
unsafe impl Send for EchOsInterruptRegistry {}
unsafe impl Sync for EchOsInterruptRegistry {}

impl EchOsInterruptRegistry {
    pub const fn new() -> Self {
        const UNLIMITED: InterruptBudget = InterruptBudget::unlimited();
        Self {
            budgets: Mutex::new([UNLIMITED; 256]),
            calls: Mutex::new([0u32; 256]),
            quarantined: Mutex::new([false; 256]),
            handlers: Mutex::new([const { None }; 256]),
        }
    }
}

impl InterruptRegistry for EchOsInterruptRegistry {
    fn register(&self, irq: u32, handler: &'static mut dyn InterruptHandler) -> Result<(), ShimError> {
        self.register_with_budget(irq, handler, InterruptBudget::unlimited())
    }

    fn register_with_budget(
        &self,
        irq: u32,
        handler: &'static mut dyn InterruptHandler,
        budget: InterruptBudget,
    ) -> Result<(), ShimError> {
        if irq as usize >= 256 {
            return Err(ShimError::InvalidAddress);
        }
        let idx = irq as usize;
        let mut handlers = self.handlers.lock();
        if handlers[idx].is_some() {
            return Err(ShimError::InterruptInUse);
        }
        handlers[idx] = Some(handler as *mut dyn InterruptHandler);
        self.budgets.lock()[idx] = budget;
        self.calls.lock()[idx] = 0;
        self.quarantined.lock()[idx] = false;
        crate::serial_println!(
            "[IronShim] IRQ {} registered (budget: {} calls max)",
            irq,
            budget.max_calls
        );
        Ok(())
    }

    fn unregister(&self, irq: u32) -> Result<(), ShimError> {
        if irq as usize >= 256 {
            return Err(ShimError::InvalidAddress);
        }
        self.handlers.lock()[irq as usize] = None;
        crate::serial_println!("[IronShim] IRQ {} unregistered", irq);
        Ok(())
    }

    fn trigger(&self, irq: u32) -> Result<(), ShimError> {
        self.trigger_with_budget(irq, 1)
    }

    fn trigger_with_budget(&self, irq: u32, elapsed_ticks: u32) -> Result<(), ShimError> {
        let idx = irq as usize;
        if idx >= 256 {
            return Err(ShimError::InvalidAddress);
        }
        if self.quarantined.lock()[idx] {
            return Err(ShimError::Quarantined);
        }
        let budget = self.budgets.lock()[idx];
        let count = {
            let mut c = self.calls.lock();
            c[idx] = c[idx].saturating_add(1);
            c[idx]
        };
        if count > budget.max_calls {
            self.quarantined.lock()[idx] = true;
            crate::serial_println!("[IronShim] IRQ {} QUARANTINED (budget exceeded)", irq);
            return Err(ShimError::BudgetExceeded);
        }
        let mut handlers = self.handlers.lock();
        if let Some(handler_ptr) = handlers[idx] {
            unsafe { (*handler_ptr).handle(irq) }
        } else {
            Err(ShimError::ResourceNotGranted)
        }
    }

    fn unquarantine(&self, irq: u32) -> Result<(), ShimError> {
        let idx = irq as usize;
        if idx >= 256 {
            return Err(ShimError::InvalidAddress);
        }
        self.quarantined.lock()[idx] = false;
        self.calls.lock()[idx] = 0;
        Ok(())
    }

    fn metrics(&self, irq: u32) -> Result<InterruptMetrics, ShimError> {
        let idx = irq as usize;
        if idx >= 256 {
            return Err(ShimError::InvalidAddress);
        }
        Ok(InterruptMetrics {
            latency_ticks: 0,
            missed: 0,
            budget_violations: if self.quarantined.lock()[idx] { 1 } else { 0 },
        })
    }
}

// ---------------------------------------------------------------------------
// 3. PCI Config Access & Bridge — echOS PCI Tarama Sistemine Bağlantı
// ---------------------------------------------------------------------------

/// echOS'un PCI config space okuma kodunu IronShim PCI arayüzüne bağlar.
pub struct EchOsPciConfig;

impl PciConfigAccess for EchOsPciConfig {
    fn read_u32(&self, bus: u8, device: u8, function: u8, offset: u8) -> u32 {
        crate::drivers::pci::read_config_dword(bus, device, function, offset as u16)
    }
}

/// echOS PCI topoloji taraması (tüm bus/device/function).
pub struct EchOsPciTopology;

impl PciTopology for EchOsPciTopology {
    fn for_each_function(&self, f: &mut dyn FnMut(PciAddress)) {
        for bus in 0u8..=255 {
            for device in 0u8..32 {
                for function in 0u8..8 {
                    f(PciAddress { bus, device, function });
                }
            }
        }
    }
}

/// echOS PCI Bridge — config + topology birleşimi.
pub struct EchOsPciBridge;

impl KernelPciBridge for EchOsPciBridge {
    fn config(&self) -> &dyn PciConfigAccess {
        &EchOsPciConfig
    }

    fn topology(&self) -> &dyn PciTopology {
        &EchOsPciTopology
    }
}

// ---------------------------------------------------------------------------
// 4. Port I/O — echOS x86 Port Erişimine Bağlantı
// ---------------------------------------------------------------------------

/// echOS x86 port I/O işlemlerini IronShim'e bağlar.
pub struct EchOsPortIo;

impl PortIo for EchOsPortIo {
    fn inb(&self, port: u16) -> u8 {
        if !crate::ironshim_bridge::is_port_allowed(port) {
            crate::serial_println!("[IronShim/Port] inb denied: port={:#x}", port);
            return 0;
        }
        unsafe { x86_64::instructions::port::Port::new(port).read() }
    }
    fn inw(&self, port: u16) -> u16 {
        if !crate::ironshim_bridge::is_port_allowed(port) {
            crate::serial_println!("[IronShim/Port] inw denied: port={:#x}", port);
            return 0;
        }
        unsafe { x86_64::instructions::port::Port::new(port).read() }
    }
    fn inl(&self, port: u16) -> u32 {
        if !crate::ironshim_bridge::is_port_allowed(port) {
            crate::serial_println!("[IronShim/Port] inl denied: port={:#x}", port);
            return 0;
        }
        unsafe { x86_64::instructions::port::Port::new(port).read() }
    }
    fn outb(&self, port: u16, value: u8) {
        if !crate::ironshim_bridge::is_port_allowed(port) {
            crate::serial_println!("[IronShim/Port] outb denied: port={:#x}", port);
            return;
        }
        unsafe { x86_64::instructions::port::Port::new(port).write(value) }
    }
    fn outw(&self, port: u16, value: u16) {
        if !crate::ironshim_bridge::is_port_allowed(port) {
            crate::serial_println!("[IronShim/Port] outw denied: port={:#x}", port);
            return;
        }
        unsafe { x86_64::instructions::port::Port::new(port).write(value) }
    }
    fn outl(&self, port: u16, value: u32) {
        if !crate::ironshim_bridge::is_port_allowed(port) {
            crate::serial_println!("[IronShim/Port] outl denied: port={:#x}", port);
            return;
        }
        unsafe { x86_64::instructions::port::Port::new(port).write(value) }
    }
}

// ---------------------------------------------------------------------------
// 5. Syscall Policy — IronShim Syscall Filtreleme
// ---------------------------------------------------------------------------

/// IronShim'in syscall politikasını echOS Seccomp ile birleştirir.
pub struct EchOsSyscallPolicy;

impl SyscallPolicy for EchOsSyscallPolicy {
    fn check(&self, request: &SyscallRequest) -> Result<(), ShimError> {
        use crate::security::seccomp::{
            SeccompData, SECCOMP_MODE_DISABLED, SECCOMP_MODE_FILTER, SECCOMP_MODE_STRICT,
            SECCOMP_RET_ACTION, SECCOMP_RET_ALLOW, SECCOMP_RET_LOG, SECCOMP_RET_TRACE,
            SECCOMP_STRICT_ALLOWED,
        };

        let mode = crate::task::scheduler::get_current_seccomp_mode();
        if mode == SECCOMP_MODE_DISABLED {
            return Ok(());
        }

        if mode == SECCOMP_MODE_STRICT {
            if SECCOMP_STRICT_ALLOWED.contains(&(request.number as i32)) {
                return Ok(());
            }
            return Err(ShimError::AccessDenied);
        }

        if mode == SECCOMP_MODE_FILTER {
            let filter = crate::task::scheduler::get_current_seccomp_filter();
            let Some(filter) = filter else {
                return Err(ShimError::AccessDenied);
            };
            let args = [
                request.args[0] as u64,
                request.args[1] as u64,
                request.args[2] as u64,
                request.args[3] as u64,
                request.args[4] as u64,
                request.args[5] as u64,
            ];
            let data = SeccompData::new(request.number as i32, args);
            let action = filter.evaluate(&data) & SECCOMP_RET_ACTION;
            if action == SECCOMP_RET_ALLOW || action == SECCOMP_RET_LOG || action == SECCOMP_RET_TRACE {
                return Ok(());
            }
            return Err(ShimError::AccessDenied);
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 6. Audit Sink — IronShim Denetim Günlüğü
// ---------------------------------------------------------------------------

/// Tüm IronShim kararlarını serial console'a loglayan denetim havuzu.
pub struct EchOsAuditLog;

impl AuditSink for EchOsAuditLog {
    fn record(&self, event: AuditEvent) {
        match event {
            AuditEvent::ManifestValidated => {
                crate::serial_println!("[IronShim/Audit] Manifest validated ✅");
            }
            AuditEvent::ManifestRejected => {
                crate::serial_println!("[IronShim/Audit] Manifest REJECTED ❌");
            }
            AuditEvent::SyscallDenied(nr) => {
                crate::serial_println!("[IronShim/Audit] Syscall {} DENIED", nr);
            }
            AuditEvent::SyscallAllowed(_) => {} // Sessiz
            _ => {
                crate::serial_println!("[IronShim/Audit] Event: {:?}", event);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 7. Global IronShim Singleton'ları
// ---------------------------------------------------------------------------

/// Global DMA allocator
pub static IRONSHIM_DMA: EchOsDmaAllocator = EchOsDmaAllocator;
/// Global Interrupt registry (IRQ budget + quarantine yönetimi)
pub static IRONSHIM_IRQ: EchOsInterruptRegistry = EchOsInterruptRegistry::new();
/// Global PCI bridge (config + topology)
pub static IRONSHIM_PCI: EchOsPciBridge = EchOsPciBridge;
/// Global Port I/O
pub static IRONSHIM_PORT: EchOsPortIo = EchOsPortIo;
/// Global Syscall policy
pub static IRONSHIM_POLICY: EchOsSyscallPolicy = EchOsSyscallPolicy;
/// Global Audit log
pub static IRONSHIM_AUDIT: EchOsAuditLog = EchOsAuditLog;
