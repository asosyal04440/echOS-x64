use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};

pub struct DebugconWriter {
    buffer: [u8; 4096],
    pos: AtomicU32,
    dropped: AtomicU64,
    last_flush: AtomicU64,
}

impl DebugconWriter {
    pub const fn new() -> Self {
        Self {
            buffer: [0u8; 4096],
            pos: AtomicU32::new(0),
            dropped: AtomicU64::new(0),
            last_flush: AtomicU64::new(0),
        }
    }

    pub fn write(&self, data: &[u8]) {
        let current_pos = self.pos.load(Ordering::Relaxed);
        if current_pos as usize + data.len() > self.buffer.len() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        unsafe {
            let buf = &self.buffer as *const u8 as *mut u8;
            for (i, &b) in data.iter().enumerate() {
                core::ptr::write(buf.add(current_pos as usize + i), b);
            }
        }
        self.pos.store(current_pos + data.len() as u32, Ordering::Release);
        self.try_flush();
    }

    fn try_flush(&self) {
        let current_pos = self.pos.load(Ordering::Acquire);
        if current_pos == 0 {
            return;
        }
        let now = self.timestamp_ms();
        let last = self.last_flush.load(Ordering::Relaxed);
        let buffer_full = current_pos as usize > self.buffer.len() * 80 / 100;
        if now - last >= 100 || buffer_full {
            unsafe {
                for i in 0..current_pos as usize {
                    core::arch::asm!(
                        "mov dx, 0xe9",
                        "out dx, al",
                        in("al") self.buffer[i],
                    );
                }
            }
            self.pos.store(0, Ordering::Release);
            self.last_flush.store(now, Ordering::Release);
        }
    }

    pub fn flush(&self) {
        let current_pos = self.pos.load(Ordering::Acquire);
        if current_pos == 0 {
            return;
        }
        unsafe {
            for i in 0..current_pos as usize {
                core::arch::asm!(
                    "mov dx, 0xe9",
                    "out dx, al",
                    in("al") self.buffer[i],
                );
            }
        }
        self.pos.store(0, Ordering::Release);
        self.last_flush.store(self.timestamp_ms(), Ordering::Release);
    }

    fn timestamp_ms(&self) -> u64 {
        unsafe { core::arch::x86_64::_rdtsc() / 3_000_000 }
    }
}

pub static DEBUGCON: DebugconWriter = DebugconWriter::new();

pub fn write_fmt(args: core::fmt::Arguments) {
    use core::fmt::Write;
    struct DebugconAdapter;
    impl Write for DebugconAdapter {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            DEBUGCON.write(s.as_bytes());
            Ok(())
        }
    }
    let _ = DebugconAdapter.write_fmt(args);
    let _ = DebugconAdapter.write_str("\n");
}
