//! # Güç Yönetimi Modülü
//!
//! CPU güç durumları (C-state/P-state), frekans ölçeklendirme ve enerji tasarrufu.
//! Linux cpufreq ile eşdeğer Tier-1 OS düzeyinde yetenekler sunar.

use crate::memory_barriers::{smp_mb, smp_rmb, smp_wmb};
use crate::preempt::{preempt_enabled, PreemptDisableGuard};
use crate::rcu::{synchronize_rcu, RcuPtr};
use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

/// CPU power states (Linux C-states ile uyumlu)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum CpuState {
    /// C0: Active state
    C0 = 0,
    /// C1: Basic idle state
    C1 = 1,
    /// C2: Deeper idle state
    C2 = 2,
    /// C3: Deep idle state
    C3 = 3,
    /// C6: Very deep idle state
    C6 = 4,
    /// C7: Deepest idle state
    C7 = 5,
}

/// CPU frequency states (P-states)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuFrequency {
    /// Frequency in MHz
    pub frequency_mhz: u32,
    /// Voltage in millivolts
    pub voltage_mv: u32,
    /// Power consumption in milliwatts
    pub power_mw: u32,
    /// Whether this is the turbo frequency
    pub is_turbo: bool,
}

impl CpuFrequency {
    pub fn new(frequency_mhz: u32, voltage_mv: u32, power_mw: u32) -> Self {
        Self {
            frequency_mhz,
            voltage_mv,
            power_mw,
            is_turbo: false,
        }
    }

    pub fn turbo(frequency_mhz: u32, voltage_mv: u32, power_mw: u32) -> Self {
        Self {
            frequency_mhz,
            voltage_mv,
            power_mw,
            is_turbo: true,
        }
    }
}

/// CPU idle state descriptor
#[derive(Debug)]
pub struct CpuIdleState {
    /// State identifier (C1, C2, etc.)
    pub state: CpuState,
    /// Exit latency in microseconds
    pub exit_latency_us: u32,
    /// Power consumption in milliwatts
    pub power_mw: u32,
    /// Target residency in microseconds
    pub target_residency_us: u32,
    /// Whether this state disables caches
    pub disables_cache: bool,
    /// Whether this state flushes TLB
    pub flushes_tlb: bool,
    /// Usage count
    pub usage_count: AtomicU64,
    /// Total time spent in this state (ticks)
    pub total_time: AtomicU64,
}

impl CpuIdleState {
    pub fn new(
        state: CpuState,
        exit_latency_us: u32,
        power_mw: u32,
        target_residency_us: u32,
    ) -> Self {
        Self {
            state,
            exit_latency_us,
            power_mw,
            target_residency_us,
            disables_cache: state == CpuState::C3 || state == CpuState::C6 || state == CpuState::C7,
            flushes_tlb: state == CpuState::C3 || state == CpuState::C6 || state == CpuState::C7,
            usage_count: AtomicU64::new(0),
            total_time: AtomicU64::new(0),
        }
    }

    /// Check if this state is better than another for given idle time
    pub fn is_better_than(&self, other: &CpuIdleState, idle_time_us: u32) -> bool {
        // Prefer deeper states if idle time is sufficient
        if idle_time_us >= self.target_residency_us && idle_time_us >= other.target_residency_us {
            // Deeper state (higher enum value) is better
            (self.state as u32) > (other.state as u32)
        } else if idle_time_us >= self.target_residency_us {
            true
        } else {
            false
        }
    }

    /// Record entry into this idle state
    pub fn enter(&self) {
        self.usage_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record exit from this idle state
    pub fn exit(&self, duration_ticks: u64) {
        self.total_time.fetch_add(duration_ticks, Ordering::Relaxed);
    }

    /// Get usage statistics
    pub fn get_stats(&self) -> (u64, u64) {
        (
            self.usage_count.load(Ordering::Relaxed),
            self.total_time.load(Ordering::Relaxed),
        )
    }
}

/// CPU frequency governor types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum FreqGovernor {
    /// Performance governor (always max frequency)
    Performance = 0,
    /// Powersave governor (always min frequency)
    Powersave = 1,
    /// Userspace governor (user-controlled)
    Userspace = 2,
    /// On-demand governor (dynamic)
    OnDemand = 3,
    /// Conservative governor (gradual changes)
    Conservative = 4,
    /// Schedutil governor (scheduler-driven)
    Schedutil = 5,
}

/// CPU power management descriptor
#[repr(C, align(64))]
pub struct CpuPowerDesc {
    /// CPU ID
    pub cpu_id: u32,
    /// Current C-state
    pub current_cstate: AtomicU32, // CpuState as u32
    /// Target C-state for idle
    pub target_cstate: AtomicU32,
    /// Current P-state (frequency index)
    pub current_pstate: AtomicU32,
    /// Target P-state
    pub target_pstate: AtomicU32,
    /// Available idle states
    pub idle_states: Vec<CpuIdleState>,
    /// Available frequencies
    pub frequencies: Vec<CpuFrequency>,
    /// Current frequency governor
    pub governor: AtomicU32, // FreqGovernor as u32
    /// Minimum frequency index
    pub min_freq_idx: u32,
    /// Maximum frequency index
    pub max_freq_idx: u32,
    /// Turbo frequency index (if available)
    pub turbo_freq_idx: Option<u32>,
    /// Current load (0-100%)
    pub current_load: AtomicU32,
    /// Average load (for on-demand governor)
    pub avg_load: AtomicU32,
    /// Time in each C-state
    pub cstate_time: [AtomicU64; 6], // C0-C7
    /// C-state entry count
    pub cstate_count: [AtomicU64; 6],
    /// Frequency transitions count
    pub freq_transitions: AtomicU64,
    /// Last frequency change timestamp
    pub last_freq_change: AtomicU64,
    /// Power management enabled
    pub pm_enabled: AtomicBool,
    /// Idle loop running
    pub in_idle: AtomicBool,
    /// Padding to avoid false sharing
    _padding: [u8; 0],
}

impl CpuPowerDesc {
    /// Create new CPU power descriptor
    pub fn new(cpu_id: u32) -> Self {
        let mut idle_states = Vec::new();
        let mut frequencies = Vec::new();

        // Default idle states (typical x86 values)
        idle_states.push(CpuIdleState::new(CpuState::C1, 1, 100, 2));
        idle_states.push(CpuIdleState::new(CpuState::C2, 10, 50, 10));
        idle_states.push(CpuIdleState::new(CpuState::C3, 50, 20, 100));
        idle_states.push(CpuIdleState::new(CpuState::C6, 100, 10, 200));
        idle_states.push(CpuIdleState::new(CpuState::C7, 150, 5, 300));

        // Default frequencies (typical desktop CPU)
        frequencies.push(CpuFrequency::new(800, 800, 5000)); // Min
        frequencies.push(CpuFrequency::new(1200, 900, 8000));
        frequencies.push(CpuFrequency::new(1600, 1000, 12000));
        frequencies.push(CpuFrequency::new(2000, 1100, 17000));
        frequencies.push(CpuFrequency::new(2400, 1200, 23000));
        frequencies.push(CpuFrequency::new(2800, 1300, 30000));
        frequencies.push(CpuFrequency::new(3200, 1400, 38000));
        frequencies.push(CpuFrequency::turbo(3600, 1500, 47000)); // Turbo

        Self {
            cpu_id,
            current_cstate: AtomicU32::new(CpuState::C0 as u32),
            target_cstate: AtomicU32::new(CpuState::C1 as u32),
            current_pstate: AtomicU32::new(3), // Start at middle frequency
            target_pstate: AtomicU32::new(3),
            idle_states,
            frequencies,
            governor: AtomicU32::new(FreqGovernor::OnDemand as u32),
            min_freq_idx: 0,
            max_freq_idx: 7,
            turbo_freq_idx: Some(7),
            current_load: AtomicU32::new(0),
            avg_load: AtomicU32::new(0),
            cstate_time: [const { AtomicU64::new(0) }; 6],
            cstate_count: [const { AtomicU64::new(0) }; 6],
            freq_transitions: AtomicU64::new(0),
            last_freq_change: AtomicU64::new(0),
            pm_enabled: AtomicBool::new(true),
            in_idle: AtomicBool::new(false),
            _padding: [0; 0],
        }
    }

    /// Get current C-state
    pub fn get_current_cstate(&self) -> CpuState {
        match self.current_cstate.load(Ordering::Acquire) {
            0 => CpuState::C0,
            1 => CpuState::C1,
            2 => CpuState::C2,
            3 => CpuState::C3,
            4 => CpuState::C6,
            5 => CpuState::C7,
            _ => CpuState::C0,
        }
    }

    /// Set current C-state
    pub fn set_current_cstate(&self, state: CpuState) {
        self.current_cstate.store(state as u32, Ordering::Release);
        smp_wmb();
    }

    /// Get current frequency
    pub fn get_current_frequency(&self) -> Option<CpuFrequency> {
        let idx = self.current_pstate.load(Ordering::Acquire) as usize;
        self.frequencies.get(idx).copied()
    }

    /// Set frequency
    pub fn set_frequency(&self, freq_idx: u32) -> Result<(), PowerError> {
        if freq_idx > self.max_freq_idx {
            return Err(PowerError::InvalidFrequency);
        }

        // Check turbo availability
        if let Some(turbo_idx) = self.turbo_freq_idx {
            if freq_idx == turbo_idx && !self.can_use_turbo() {
                return Err(PowerError::TurboUnavailable);
            }
        }

        let old_idx = self.current_pstate.load(Ordering::Acquire);
        if old_idx != freq_idx {
            // IA32_PERF_CTL MSR'a (0x199) yeni P-state değeri yaz
            let freq = &self.frequencies[freq_idx as usize];
            let perf_ctl_value = (freq_idx as u64) << 8; // P-state indeks (bit 15:8)
            unsafe {
                // MSR 0x199 = IA32_PERF_CTL
                // wrmsr: ECX=MSR index, EAX=low 32, EDX=high 32
                let low = perf_ctl_value as u32;
                let high = (perf_ctl_value >> 32) as u32;
                core::arch::asm!(
                    "wrmsr",
                    in("ecx") 0x199u32,
                    in("eax") low,
                    in("edx") high,
                    options(nomem, nostack)
                );
            }

            self.current_pstate.store(freq_idx, Ordering::Release);
            self.freq_transitions.fetch_add(1, Ordering::Relaxed);
            self.last_freq_change.store(
                crate::task::scheduler::get_ticks() as u64,
                Ordering::Relaxed,
            );
            smp_mb();

            crate::serial_println!(
                "Power: CPU {} frequency changed to {} MHz (MSR 0x199 = {:#x})",
                self.cpu_id,
                freq.frequency_mhz,
                perf_ctl_value
            );
        }

        Ok(())
    }

    /// Check if turbo can be used
    pub fn can_use_turbo(&self) -> bool {
        // Simple turbo availability check
        // In real implementation, this would check thermal limits, power budget, etc.
        let load = self.current_load.load(Ordering::Acquire);
        load < 80 // Only use turbo if load is not too high
    }

    /// Enter idle state
    pub fn enter_idle(&self, idle_time_us: u32) -> CpuState {
        if !self.pm_enabled.load(Ordering::Acquire) {
            return CpuState::C0;
        }

        // Find best idle state for given time
        let mut best_state = &self.idle_states[0]; // C1 as default

        for state in &self.idle_states {
            if state.is_better_than(best_state, idle_time_us) {
                best_state = state;
            }
        }

        // Enter selected state
        best_state.enter();
        self.set_current_cstate(best_state.state);
        self.in_idle.store(true, Ordering::Release);

        best_state.state
    }

    /// Exit idle state
    pub fn exit_idle(&self, duration_ticks: u64) {
        let current_state = self.get_current_cstate();

        // Update statistics for current state
        if let Some(state) = self.idle_states.iter().find(|s| s.state == current_state) {
            state.exit(duration_ticks);
        }

        // Update C-state statistics
        let state_idx = current_state as usize;
        if state_idx < 6 {
            self.cstate_time[state_idx].fetch_add(duration_ticks, Ordering::Relaxed);
            self.cstate_count[state_idx].fetch_add(1, Ordering::Relaxed);
        }

        // Return to C0
        self.set_current_cstate(CpuState::C0);
        self.in_idle.store(false, Ordering::Release);
        smp_mb();
    }

    /// Update CPU load
    pub fn update_load(&self, load: u32) {
        self.current_load.store(load, Ordering::Release);

        // Update average load (exponential moving average)
        let current_avg = self.avg_load.load(Ordering::Acquire);
        let new_avg = (current_avg * 3 + load) / 4; // 0.75 weight to old value
        self.avg_load.store(new_avg, Ordering::Release);

        // Apply frequency governor
        self.apply_governor();
    }

    /// Apply frequency governor
    fn apply_governor(&self) {
        let governor = self.get_governor();
        let load = self.avg_load.load(Ordering::Acquire);

        match governor {
            FreqGovernor::Performance => {
                self.set_frequency(self.max_freq_idx);
            }
            FreqGovernor::Powersave => {
                self.set_frequency(self.min_freq_idx);
            }
            FreqGovernor::OnDemand => {
                self.apply_ondemand_governor(load);
            }
            FreqGovernor::Conservative => {
                self.apply_conservative_governor(load);
            }
            FreqGovernor::Schedutil => {
                self.apply_schedutil_governor(load);
            }
            FreqGovernor::Userspace => {
                // User-controlled, no automatic changes
            }
        }
    }

    /// Apply on-demand governor
    fn apply_ondemand_governor(&self, load: u32) {
        let current_idx = self.current_pstate.load(Ordering::Acquire);

        if load > 80 && current_idx < self.max_freq_idx {
            // Increase frequency
            self.set_frequency(current_idx + 1);
        } else if load < 20 && current_idx > self.min_freq_idx {
            // Decrease frequency
            self.set_frequency(current_idx - 1);
        }
    }

    /// Apply conservative governor
    fn apply_conservative_governor(&self, load: u32) {
        let current_idx = self.current_pstate.load(Ordering::Acquire);

        // More gradual changes than on-demand
        if load > 90 && current_idx < self.max_freq_idx {
            self.set_frequency(current_idx + 1);
        } else if load < 10 && current_idx > self.min_freq_idx {
            self.set_frequency(current_idx - 1);
        }
    }

    /// Apply schedutil governor
    fn apply_schedutil_governor(&self, load: u32) {
        // Scheduler-driven frequency selection
        // Use load to directly map to frequency
        let freq_range = self.max_freq_idx - self.min_freq_idx;
        let target_idx = self.min_freq_idx + (load * freq_range / 100);

        self.set_frequency(target_idx);
    }

    /// Get current governor
    pub fn get_governor(&self) -> FreqGovernor {
        match self.governor.load(Ordering::Acquire) {
            0 => FreqGovernor::Performance,
            1 => FreqGovernor::Powersave,
            2 => FreqGovernor::Userspace,
            3 => FreqGovernor::OnDemand,
            4 => FreqGovernor::Conservative,
            5 => FreqGovernor::Schedutil,
            _ => FreqGovernor::OnDemand,
        }
    }

    /// Set frequency governor
    pub fn set_governor(&self, governor: FreqGovernor) {
        self.governor.store(governor as u32, Ordering::Release);
        smp_wmb();

        // Apply new governor immediately
        self.apply_governor();

        crate::serial_println!(
            "Power: CPU {} governor changed to {:?}",
            self.cpu_id,
            governor
        );
    }

    /// Enable/disable power management
    pub fn set_pm_enabled(&self, enabled: bool) {
        self.pm_enabled.store(enabled, Ordering::Release);
        smp_wmb();

        if !enabled {
            // Return to C0 and max frequency when disabled
            self.set_current_cstate(CpuState::C0);
            self.set_frequency(self.max_freq_idx);
        }
    }

    /// Get power statistics
    pub fn get_power_stats(&self) -> PowerStats {
        let mut cstate_times = [0u64; 6];
        let mut cstate_counts = [0u64; 6];

        for i in 0..6 {
            cstate_times[i] = self.cstate_time[i].load(Ordering::Relaxed);
            cstate_counts[i] = self.cstate_count[i].load(Ordering::Relaxed);
        }

        PowerStats {
            current_frequency: self.get_current_frequency(),
            current_load: self.current_load.load(Ordering::Relaxed),
            avg_load: self.avg_load.load(Ordering::Relaxed),
            freq_transitions: self.freq_transitions.load(Ordering::Relaxed),
            cstate_times,
            cstate_counts,
            idle_state_stats: self.idle_states.iter().map(|s| s.get_stats()).collect(),
        }
    }
}

/// Power statistics
#[derive(Debug, Clone)]
pub struct PowerStats {
    pub current_frequency: Option<CpuFrequency>,
    pub current_load: u32,
    pub avg_load: u32,
    pub freq_transitions: u64,
    pub cstate_times: [u64; 6],
    pub cstate_counts: [u64; 6],
    pub idle_state_stats: Vec<(u64, u64)>,
}

/// Power management manager
pub struct PowerManager {
    /// Maximum number of CPUs
    max_cpus: u32,
    /// CPU power descriptors
    cpu_descs: Vec<RcuPtr<CpuPowerDesc>>,
    /// Global power management enabled
    pm_enabled: AtomicBool,
    /// Global power policy
    global_policy: AtomicU32, // FreqGovernor as u32
    /// Power statistics
    stats: PowerManagerStats,
}

/// Power manager statistics
#[derive(Debug)]
pub struct PowerManagerStats {
    pub total_idle_transitions: AtomicU64,
    pub total_freq_changes: AtomicU64,
    pub total_energy_saved: AtomicU64, // In milliwatt-hours (approximate)
}

impl PowerManagerStats {
    pub const fn new() -> Self {
        Self {
            total_idle_transitions: AtomicU64::new(0),
            total_freq_changes: AtomicU64::new(0),
            total_energy_saved: AtomicU64::new(0),
        }
    }

    pub fn record_idle_transition(&self) {
        self.total_idle_transitions.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_freq_change(&self) {
        self.total_freq_changes.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_energy_saved(&self, energy_mwh: u64) {
        self.total_energy_saved
            .fetch_add(energy_mwh, Ordering::Relaxed);
    }

    pub fn get_stats(&self) -> (u64, u64, u64) {
        (
            self.total_idle_transitions.load(Ordering::Relaxed),
            self.total_freq_changes.load(Ordering::Relaxed),
            self.total_energy_saved.load(Ordering::Relaxed),
        )
    }
}

impl PowerManager {
    /// Create new power manager
    pub fn new(max_cpus: u32) -> Self {
        let mut cpu_descs = Vec::with_capacity(max_cpus as usize);

        // Initialize CPU power descriptors
        for cpu_id in 0..max_cpus {
            let desc = Box::new(CpuPowerDesc::new(cpu_id));
            cpu_descs.push(RcuPtr::new(Box::into_raw(desc)));
        }

        Self {
            max_cpus,
            cpu_descs,
            pm_enabled: AtomicBool::new(true),
            global_policy: AtomicU32::new(FreqGovernor::OnDemand as u32),
            stats: PowerManagerStats::new(),
        }
    }

    /// Get CPU power descriptor
    pub fn get_cpu_desc(&self, cpu_id: u32) -> Option<RcuPtr<CpuPowerDesc>> {
        if cpu_id >= self.max_cpus {
            return None;
        }

        Some(self.cpu_descs[cpu_id as usize].clone())
    }

    /// Enter idle state for CPU
    pub fn cpu_idle_enter(&self, cpu_id: u32, idle_time_us: u32) -> Result<CpuState, PowerError> {
        if !self.pm_enabled.load(Ordering::Acquire) {
            return Ok(CpuState::C0);
        }

        let desc = match self.get_cpu_desc(cpu_id) {
            Some(desc) => desc,
            None => return Err(PowerError::InvalidCpuId),
        };

        let state = desc.read().enter_idle(idle_time_us);
        self.stats.record_idle_transition();

        Ok(state)
    }

    /// Exit idle state for CPU
    pub fn cpu_idle_exit(&self, cpu_id: u32, duration_ticks: u64) -> Result<(), PowerError> {
        let desc = match self.get_cpu_desc(cpu_id) {
            Some(desc) => desc,
            None => return Err(PowerError::InvalidCpuId),
        };

        desc.read().exit_idle(duration_ticks);
        Ok(())
    }

    /// Update CPU load
    pub fn update_cpu_load(&self, cpu_id: u32, load: u32) -> Result<(), PowerError> {
        let desc = match self.get_cpu_desc(cpu_id) {
            Some(desc) => desc,
            None => return Err(PowerError::InvalidCpuId),
        };

        desc.read().update_load(load);
        Ok(())
    }

    /// Set CPU frequency governor
    pub fn set_cpu_governor(&self, cpu_id: u32, governor: FreqGovernor) -> Result<(), PowerError> {
        let desc = match self.get_cpu_desc(cpu_id) {
            Some(desc) => desc,
            None => return Err(PowerError::InvalidCpuId),
        };

        desc.read().set_governor(governor);
        Ok(())
    }

    /// Set global frequency governor
    pub fn set_global_governor(&self, governor: FreqGovernor) {
        self.global_policy.store(governor as u32, Ordering::Release);
        smp_wmb();

        // Apply to all CPUs
        for cpu_id in 0..self.max_cpus {
            if let Some(desc) = self.get_cpu_desc(cpu_id) {
                let _ = self.set_cpu_governor(cpu_id, governor);
            }
        }

        crate::serial_println!("Power: Global governor changed to {:?}", governor);
    }

    /// Enable/disable power management
    pub fn set_pm_enabled(&self, enabled: bool) {
        self.pm_enabled.store(enabled, Ordering::Release);
        smp_wmb();

        // Apply to all CPUs
        for cpu_id in 0..self.max_cpus {
            if let Some(desc) = self.get_cpu_desc(cpu_id) {
                desc.read().set_pm_enabled(enabled);
            }
        }

        crate::serial_println!(
            "Power: Power management {}",
            if enabled { "enabled" } else { "disabled" }
        );
    }

    /// Get power statistics for CPU
    pub fn get_cpu_stats(&self, cpu_id: u32) -> Result<PowerStats, PowerError> {
        let desc = match self.get_cpu_desc(cpu_id) {
            Some(desc) => desc,
            None => return Err(PowerError::InvalidCpuId),
        };

        Ok(desc.read().get_power_stats())
    }

    /// Get global power statistics
    pub fn get_global_stats(&self) -> (u64, u64, u64) {
        self.stats.get_stats()
    }

    /// Suspend system — tüm cihazları hazırla, cache flush, ACPI S3'e gir
    pub fn system_suspend(&self) -> Result<(), PowerError> {
        crate::serial_println!("Power: Preparing system suspend...");

        // 1. Flush CPU caches (WBINVD)
        #[cfg(not(feature = "simics"))]
        unsafe {
            core::arch::asm!("wbinvd", options(nostack, preserves_flags));
        }

        // 2. Tüm CPU'ları derin uykuya al
        for cpu_id in 0..self.max_cpus {
            if let Some(desc) = self.get_cpu_desc(cpu_id) {
                let desc_guard = desc.read();
                desc_guard.set_pm_enabled(false);
                desc_guard.set_current_cstate(CpuState::C7);
            }
        }

        // 3. Gerçek ACPI S3 durumuna gir (drivers::power üzerinden)
        let _ =
            crate::drivers::power::PM_MANAGER.enter_sleep(crate::drivers::power::SleepState::S3);

        crate::serial_println!("Power: System suspended");
        Ok(())
    }

    /// Resume system — cihaz durumlarını geri yükle, CPU'ları aktifleştir
    pub fn system_resume(&self) -> Result<(), PowerError> {
        crate::serial_println!("Power: Resuming system...");

        // 1. BSP cache'lerini invalidate et
        #[cfg(not(feature = "simics"))]
        unsafe {
            core::arch::asm!("wbinvd", options(nostack, preserves_flags));
        }

        // 2. Tüm CPU'ları aktif duruma getir
        for cpu_id in 0..self.max_cpus {
            if let Some(desc) = self.get_cpu_desc(cpu_id) {
                let desc_guard = desc.read();
                desc_guard.set_pm_enabled(true);
                desc_guard.set_current_cstate(CpuState::C0);
                desc_guard.set_frequency(desc_guard.max_freq_idx);
            }
        }

        // 3. AP'leri uyandır (INIT-SIPI via LAPIC ICR)
        #[cfg(not(feature = "simics"))]
        {
            // ICR low register (offset 0x300): INIT IPI, all excluding self
            crate::apic::lapic::write_reg(0x300, 0x000C4500);
            // Kısa bekleme
            for _ in 0..10000 {
                core::hint::spin_loop();
            }
            // SIPI — all excluding self, vector 0
            crate::apic::lapic::write_reg(0x300, 0x000C4600);
            crate::serial_println!("Power: AP wake-up INIT-SIPI sent");
        }

        crate::serial_println!("Power: System resumed");
        Ok(())
    }
}

/// Power management errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerError {
    /// Invalid CPU ID
    InvalidCpuId,
    /// Invalid frequency
    InvalidFrequency,
    /// Turbo mode unavailable
    TurboUnavailable,
    /// Power management disabled
    PowerManagementDisabled,
    /// Invalid state transition
    InvalidStateTransition,
}

/// Global power manager instance
static mut POWER_MANAGER: Option<PowerManager> = None;
static POWER_INIT: AtomicBool = AtomicBool::new(false);

/// Initialize power management subsystem
pub fn init(max_cpus: u32) {
    if POWER_INIT.load(Ordering::Acquire) {
        return;
    }

    crate::serial_println!("Power: Initializing power management for {} CPUs", max_cpus);

    let manager = PowerManager::new(max_cpus);

    unsafe {
        POWER_MANAGER = Some(manager);
    }

    POWER_INIT.store(true, Ordering::Release);
    smp_mb();

    crate::serial_println!("Power: Power management initialized");
}

/// Get power manager
pub fn get_manager() -> Option<&'static PowerManager> {
    if !POWER_INIT.load(Ordering::Acquire) {
        return None;
    }

    unsafe { POWER_MANAGER.as_ref() }
}

/// Convenience functions for common operations
pub fn cpu_idle_enter(cpu_id: u32, idle_time_us: u32) -> Result<CpuState, PowerError> {
    let manager = get_manager().ok_or(PowerError::PowerManagementDisabled)?;
    manager.cpu_idle_enter(cpu_id, idle_time_us)
}

pub fn cpu_idle_exit(cpu_id: u32, duration_ticks: u64) -> Result<(), PowerError> {
    let manager = get_manager().ok_or(PowerError::PowerManagementDisabled)?;
    manager.cpu_idle_exit(cpu_id, duration_ticks)
}

pub fn update_cpu_load(cpu_id: u32, load: u32) -> Result<(), PowerError> {
    let manager = get_manager().ok_or(PowerError::PowerManagementDisabled)?;
    manager.update_cpu_load(cpu_id, load)
}

pub fn set_cpu_governor(cpu_id: u32, governor: FreqGovernor) -> Result<(), PowerError> {
    let manager = get_manager().ok_or(PowerError::PowerManagementDisabled)?;
    manager.set_cpu_governor(cpu_id, governor)
}

pub fn set_global_governor(governor: FreqGovernor) {
    if let Some(manager) = get_manager() {
        manager.set_global_governor(governor);
    }
}

pub fn system_suspend() -> Result<(), PowerError> {
    let manager = get_manager().ok_or(PowerError::PowerManagementDisabled)?;
    manager.system_suspend()
}

pub fn system_resume() -> Result<(), PowerError> {
    let manager = get_manager().ok_or(PowerError::PowerManagementDisabled)?;
    manager.system_resume()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpu_power_desc() {
        let desc = CpuPowerDesc::new(0);
        assert_eq!(desc.get_current_cstate(), CpuState::C0);
        assert!(desc.pm_enabled.load(Ordering::Acquire));

        desc.set_governor(FreqGovernor::Performance);
        assert_eq!(desc.get_governor(), FreqGovernor::Performance);
    }

    #[test]
    fn test_idle_states() {
        let c1 = CpuIdleState::new(CpuState::C1, 1, 100, 2);
        let c2 = CpuIdleState::new(CpuState::C2, 10, 50, 10);

        assert!(c2.is_better_than(&c1, 15)); // 15us > C2 target residency
        assert!(!c2.is_better_than(&c1, 5)); // 5us < C2 target residency
    }

    #[test]
    fn test_power_manager() {
        let manager = PowerManager::new(4);
        assert!(manager.pm_enabled.load(Ordering::Acquire));

        assert!(manager.cpu_idle_enter(0, 10).is_ok());
        assert!(manager.cpu_idle_exit(0, 100).is_ok());
    }
}

// ============================================================================
// ACPI S-STATES (Sleep States)
// ============================================================================

/// ACPI sleep states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SleepState {
    /// S1: Power On Suspend - CPU stopped, RAM preserved
    S1,
    /// S2: CPU powered off, context saved to RAM
    S2,
    /// S3: Suspend to RAM (STR) - Low power, fast resume
    S3,
    /// S4: Suspend to Disk (Hibernate) - Lowest power
    S4,
}

impl SleepState {
    pub fn to_acpi(&self) -> u8 {
        match self {
            SleepState::S1 => 1,
            SleepState::S2 => 2,
            SleepState::S3 => 3,
            SleepState::S4 => 4,
        }
    }
}

// ============================================================================
// BATTERY MANAGEMENT
// ============================================================================

/// Battery status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatteryStatus {
    Discharging,
    Charging,
    Critical,
    Full,
    Unknown,
}

/// Battery information
#[derive(Debug, Clone)]
pub struct BatteryInfo {
    pub id: u32,
    pub present: bool,
    pub status: BatteryStatus,
    pub capacity_percent: u32,
    pub voltage_mv: u32,
    pub current_ma: i32,
    pub remaining_capacity_mwh: u32,
    pub full_capacity_mwh: u32,
    pub design_capacity_mwh: u32,
    pub time_to_empty_sec: u32,
    pub time_to_full_sec: u32,
    pub temperature_celsius: i32,
    pub manufacturer: alloc::string::String,
    pub model: alloc::string::String,
}

impl BatteryInfo {
    pub fn new(id: u32) -> Self {
        BatteryInfo {
            id,
            present: false,
            status: BatteryStatus::Unknown,
            capacity_percent: 0,
            voltage_mv: 0,
            current_ma: 0,
            remaining_capacity_mwh: 0,
            full_capacity_mwh: 0,
            design_capacity_mwh: 0,
            time_to_empty_sec: 0,
            time_to_full_sec: 0,
            temperature_celsius: 25,
            manufacturer: alloc::string::String::new(),
            model: alloc::string::String::new(),
        }
    }

    pub fn is_low(&self) -> bool {
        self.capacity_percent < 20
    }

    pub fn is_critical(&self) -> bool {
        self.capacity_percent < 5 || self.status == BatteryStatus::Critical
    }

    pub fn health_percent(&self) -> u32 {
        if self.design_capacity_mwh > 0 {
            (self.full_capacity_mwh * 100) / self.design_capacity_mwh
        } else {
            100
        }
    }
}

// ============================================================================
// THERMAL MANAGEMENT
// ============================================================================

/// Thermal trip type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThermalTripType {
    Active,
    Passive,
    Critical,
    Hot,
}

/// Thermal trip point
#[derive(Debug, Clone)]
pub struct ThermalTripPoint {
    pub trip_type: ThermalTripType,
    pub temperature_celsius: i32,
    pub hysteresis_celsius: i32,
}

impl ThermalTripPoint {
    pub fn new(trip_type: ThermalTripType, temp: i32) -> Self {
        ThermalTripPoint {
            trip_type,
            temperature_celsius: temp,
            hysteresis_celsius: 2,
        }
    }
}

/// Thermal zone information
#[derive(Debug, Clone)]
pub struct ThermalZoneInfo {
    pub id: u32,
    pub name: alloc::string::String,
    pub temperature_celsius: i32,
    pub trip_points: Vec<ThermalTripPoint>,
    pub passive_temp: i32,
    pub critical_temp: i32,
}

impl ThermalZoneInfo {
    pub fn new(id: u32, name: &str) -> Self {
        ThermalZoneInfo {
            id,
            name: alloc::string::String::from(name),
            temperature_celsius: 25,
            trip_points: Vec::new(),
            passive_temp: 80,
            critical_temp: 95,
        }
    }

    pub fn is_overheating(&self) -> bool {
        self.temperature_celsius >= self.critical_temp
    }

    pub fn needs_cooling(&self) -> bool {
        self.temperature_celsius >= self.passive_temp
    }

    pub fn add_trip_point(&mut self, trip: ThermalTripPoint) {
        match trip.trip_type {
            ThermalTripType::Passive => self.passive_temp = trip.temperature_celsius,
            ThermalTripType::Critical => self.critical_temp = trip.temperature_celsius,
            _ => {}
        }
        self.trip_points.push(trip);
    }
}

/// Cooling device type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoolingType {
    Fan,
    Processor,
    Lcd,
    Gpu,
}

/// Cooling device information
#[derive(Debug, Clone)]
pub struct CoolingDeviceInfo {
    pub id: u32,
    pub cooling_type: CoolingType,
    pub name: alloc::string::String,
    pub state: u32,
    pub max_state: u32,
    pub min_state: u32,
}

impl CoolingDeviceInfo {
    pub fn new(id: u32, cooling_type: CoolingType, name: &str) -> Self {
        CoolingDeviceInfo {
            id,
            cooling_type,
            name: alloc::string::String::from(name),
            state: 0,
            max_state: 10,
            min_state: 0,
        }
    }

    pub fn set_state(&mut self, state: u32) {
        self.state = state.clamp(self.min_state, self.max_state);
    }

    pub fn increase(&mut self) {
        if self.state < self.max_state {
            self.state += 1;
        }
    }

    pub fn decrease(&mut self) {
        if self.state > self.min_state {
            self.state -= 1;
        }
    }

    pub fn percent(&self) -> u32 {
        if self.max_state > self.min_state {
            ((self.state - self.min_state) * 100) / (self.max_state - self.min_state)
        } else {
            0
        }
    }
}

// ============================================================================
// GLOBAL POWER STATE
// ============================================================================

use alloc::collections::BTreeMap;
use spin::Mutex;

lazy_static::lazy_static! {
    static ref BATTERIES: Mutex<BTreeMap<u32, BatteryInfo>> = Mutex::new(BTreeMap::new());
    static ref THERMAL_ZONES: Mutex<BTreeMap<u32, ThermalZoneInfo>> = Mutex::new(BTreeMap::new());
    static ref COOLING_DEVICES: Mutex<BTreeMap<u32, CoolingDeviceInfo>> = Mutex::new(BTreeMap::new());
}

/// Initialize ACPI power management
pub fn init_acpi_power() {
    // Initialize default thermal zone for CPU
    let mut zone = ThermalZoneInfo::new(0, "CPU");
    zone.add_trip_point(ThermalTripPoint::new(ThermalTripType::Passive, 80));
    zone.add_trip_point(ThermalTripPoint::new(ThermalTripType::Critical, 95));
    THERMAL_ZONES.lock().insert(0, zone);

    // Initialize default cooling device
    let cooling = CoolingDeviceInfo::new(0, CoolingType::Processor, "CPU Cooling");
    COOLING_DEVICES.lock().insert(0, cooling);

    // Initialize default battery
    let battery = BatteryInfo::new(0);
    BATTERIES.lock().insert(0, battery);

    crate::serial_println!("[PWR] ACPI power management initialized");
}

/// Get battery info
pub fn get_battery(id: u32) -> Option<BatteryInfo> {
    BATTERIES.lock().get(&id).cloned()
}

/// Get all batteries
pub fn get_all_batteries() -> Vec<BatteryInfo> {
    BATTERIES.lock().values().cloned().collect()
}

/// Get average battery percent
pub fn get_battery_percent() -> u32 {
    let batteries = BATTERIES.lock();
    let batteries: Vec<_> = batteries.values().filter(|b| b.present).collect();
    if batteries.is_empty() {
        return 100;
    }
    batteries.iter().map(|b| b.capacity_percent).sum::<u32>() / batteries.len() as u32
}

/// Is battery low
pub fn is_battery_low() -> bool {
    get_battery_percent() < 20
}

/// Is battery critical
pub fn is_battery_critical() -> bool {
    get_battery_percent() < 5
}

/// Get thermal zone
pub fn get_thermal_zone(id: u32) -> Option<ThermalZoneInfo> {
    THERMAL_ZONES.lock().get(&id).cloned()
}

/// Get all thermal zones
pub fn get_all_thermal_zones() -> Vec<ThermalZoneInfo> {
    THERMAL_ZONES.lock().values().cloned().collect()
}

/// Get average temperature
pub fn get_average_temperature() -> i32 {
    let zones = THERMAL_ZONES.lock();
    let zones: Vec<_> = zones.values().collect();
    if zones.is_empty() {
        return 25;
    }
    zones.iter().map(|z| z.temperature_celsius).sum::<i32>() / zones.len() as i32
}

/// Is system overheating
pub fn is_overheating() -> bool {
    THERMAL_ZONES.lock().values().any(|z| z.is_overheating())
}

/// Update thermal zones and apply cooling
pub fn update_thermal() {
    let mut zones = THERMAL_ZONES.lock();
    let mut cooling = COOLING_DEVICES.lock();

    for zone in zones.values_mut() {
        if zone.needs_cooling() {
            for device in cooling.values_mut() {
                device.increase();
            }
        } else if zone.temperature_celsius < zone.passive_temp - 5 {
            for device in cooling.values_mut() {
                device.decrease();
            }
        }
    }
}

/// Get cooling device
pub fn get_cooling_device(id: u32) -> Option<CoolingDeviceInfo> {
    COOLING_DEVICES.lock().get(&id).cloned()
}

/// Get all cooling devices
pub fn get_all_cooling_devices() -> Vec<CoolingDeviceInfo> {
    COOLING_DEVICES.lock().values().cloned().collect()
}

/// Enter sleep state — ACPI PM1a_CNT üzerinden uyku durumuna geç
pub fn enter_sleep(state: SleepState) -> Result<(), PowerError> {
    crate::serial_println!("[PWR] Entering sleep state S{}", state.to_acpi());

    // SleepState dönüştürme: src/power.rs -> drivers/power.rs
    let driver_state = match state {
        SleepState::S1 => crate::drivers::power::SleepState::S1,
        SleepState::S2 => crate::drivers::power::SleepState::S1, // S2≈S1
        SleepState::S3 => crate::drivers::power::SleepState::S3,
        SleepState::S4 => crate::drivers::power::SleepState::S4,
    };

    crate::drivers::power::PM_MANAGER
        .enter_sleep(driver_state)
        .map_err(|_| PowerError::InvalidStateTransition)
}

/// Shutdown system — ACPI S5 + QEMU fallback
pub fn system_shutdown() -> Result<(), PowerError> {
    crate::serial_println!("[PWR] System shutdown requested");

    // drivers/power PM_MANAGER üzerinden S5 (power off) gerçekleştir
    crate::drivers::power::PM_MANAGER
        .power_off()
        .map_err(|_| PowerError::InvalidStateTransition)
}
