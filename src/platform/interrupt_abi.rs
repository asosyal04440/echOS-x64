#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KernelTrapFrameView {
    pub instruction_pointer: u64,
    pub stack_pointer: u64,
    pub cpu_flags: u64,
    pub code_segment: u64,
    pub stack_segment: u64,
}

impl KernelTrapFrameView {
    pub const fn host_inert() -> Self {
        Self {
            instruction_pointer: 0,
            stack_pointer: 0,
            cpu_flags: 0,
            code_segment: 0,
            stack_segment: 0,
        }
    }
}

#[cfg(any(target_os = "none", target_os = "uefi"))]
impl From<&x86_64::structures::idt::InterruptStackFrame> for KernelTrapFrameView {
    fn from(frame: &x86_64::structures::idt::InterruptStackFrame) -> Self {
        Self {
            instruction_pointer: frame.instruction_pointer.as_u64(),
            stack_pointer: frame.stack_pointer.as_u64(),
            cpu_flags: frame.cpu_flags,
            code_segment: frame.code_segment,
            stack_segment: frame.stack_segment,
        }
    }
}
