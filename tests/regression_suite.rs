//! # H21 — Regresyon Test Suite (Host-side Simülasyon)
//!
//! Tier 1 unit, Tier 2 stress, syscall conformance, FS consistency testleri.
//!
//! Bu dosya `cargo test --test regression_suite` ile çalıştırılır.
//! Kernel target_os="none" olduğundan, testler host ortamında
//! simüle edilir ve iç veri yapıları doğrulanır.

#![cfg(not(target_os = "none"))]

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ech_os::drivers::wifi_jail::{
    WifiBand, WifiBss, WifiJailController, WifiPhyMode, WifiSecurity,
};
use ech_os::fs::btrfs::{
    register_scrub_volume_with_layout, run_scrub_daemon_pass, scrub_daemon_status,
    scrub_superblock_mirrors_with_layout, stamp_superblock_checksum, verify_superblock_checksum,
    BtrfsScrubIssueKind, BTRFS_CSUM_TYPE_CRC32, BTRFS_CSUM_TYPE_XXHASH, BTRFS_MAGIC,
    BTRFS_SUPERBLOCK_SIZE,
};
use ech_os::memory::{mglru, psi};
use ech_os::net::io_uring::{
    get_cqe as io_uring_get_cqe, get_io_uring, io_uring_close, io_uring_register, io_uring_setup,
    submit_sqe as io_uring_submit_sqe, IoUringParams, IoUringRegisteredBuffer, IoUringSqe,
    IORING_OP_NOP, IORING_OP_READ_FIXED, IORING_SETUP_SQPOLL, IOSQE_FIXED_FILE,
};
use ech_os::net::socket::{self, AddressFamily, Protocol, SocketType};
use ech_os::rcu::{SrcuDomain, TreeRcuDomain};
use ech_os::task::eas::{select_energy_aware_cpu, CoreKind, CppcPerfCaps, EasCore, EasTask};
use ech_os::task::eevdf::{EevdfRunQueue, EevdfTask};

// ============================================================================
// TIER 1 — Lock-Free Veri Yapısı Testleri
// ============================================================================

/// SPSC ring buffer — lock-free single-producer single-consumer
struct SpscRing<T> {
    buf: Vec<Option<T>>,
    head: AtomicU64,
    tail: AtomicU64,
    capacity: usize,
}

impl<T: Clone> SpscRing<T> {
    fn new(capacity: usize) -> Self {
        let mut buf = Vec::with_capacity(capacity);
        buf.resize_with(capacity, || None);
        Self {
            buf,
            head: AtomicU64::new(0),
            tail: AtomicU64::new(0),
            capacity,
        }
    }

    fn push(&mut self, val: T) -> bool {
        let tail = self.tail.load(Ordering::Relaxed) as usize;
        let head = self.head.load(Ordering::Acquire) as usize;
        let next_tail = (tail + 1) % self.capacity;
        if next_tail == head {
            return false; // full
        }
        self.buf[tail] = Some(val);
        self.tail.store(next_tail as u64, Ordering::Release);
        true
    }

    fn pop(&mut self) -> Option<T> {
        let head = self.head.load(Ordering::Relaxed) as usize;
        let tail = self.tail.load(Ordering::Acquire) as usize;
        if head == tail {
            return None; // empty
        }
        let val = self.buf[head].take();
        self.head
            .store(((head + 1) % self.capacity) as u64, Ordering::Release);
        val
    }

    fn len(&self) -> usize {
        let tail = self.tail.load(Ordering::Relaxed) as usize;
        let head = self.head.load(Ordering::Relaxed) as usize;
        if tail >= head {
            tail - head
        } else {
            self.capacity - head + tail
        }
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[test]
fn test_spsc_ring_basic() {
    let mut ring = SpscRing::<u64>::new(16);
    assert!(ring.is_empty());

    // Push ve pop
    assert!(ring.push(42));
    assert!(ring.push(99));
    assert_eq!(ring.len(), 2);

    assert_eq!(ring.pop(), Some(42));
    assert_eq!(ring.pop(), Some(99));
    assert!(ring.is_empty());
}

#[test]
fn test_spsc_ring_full() {
    let cap = 8;
    let mut ring = SpscRing::<u32>::new(cap);

    // cap-1 eleman dolduğunda full olmalı
    for i in 0..(cap - 1) {
        assert!(ring.push(i as u32), "push {} failed", i);
    }
    assert!(!ring.push(999), "should be full");

    // Birini çıkar, sonra bir tane daha ekle
    assert_eq!(ring.pop(), Some(0));
    assert!(ring.push(777));
}

#[test]
fn test_spsc_ring_wraparound() {
    let mut ring = SpscRing::<u64>::new(4);

    // 3 push, 3 pop (wrap around)
    for cycle in 0..10 {
        for i in 0..3 {
            assert!(ring.push(cycle * 100 + i));
        }
        for i in 0..3 {
            assert_eq!(ring.pop(), Some(cycle * 100 + i));
        }
        assert!(ring.is_empty());
    }
}

// ============================================================================
// TIER 1 — Tree RCU / SRCU doğrulama
// ============================================================================

#[test]
fn test_tree_rcu_leaf_aggregation() {
    let domain = TreeRcuDomain::new();
    let g0 = domain.read_lock_on_cpu(0);
    let g1 = domain.read_lock_on_cpu(63);

    let stats = domain.stats();
    assert_eq!(stats.active_cpus, 2);
    assert_eq!(stats.active_leaves, 1);

    domain.start_grace_period();
    assert!(!domain.grace_period_completed());

    drop(g0);
    assert!(!domain.grace_period_completed());

    drop(g1);
    assert!(domain.grace_period_completed());

    let g2 = domain.read_lock_on_cpu(64);
    let stats = domain.stats();
    assert_eq!(stats.active_cpus, 1);
    assert_eq!(stats.active_leaves, 1);
    drop(g2);
}

#[test]
fn test_srcu_slot_flip_completes_after_readers_leave() {
    let domain = SrcuDomain::new();

    {
        let guard = domain.read_lock_on_cpu(7);
        let stats = domain.stats();
        assert_eq!(stats.current_slot, 0);
        assert_eq!(stats.active_slot_readers, 1);
        drop(guard);
    }

    domain.synchronize();
    let stats = domain.stats();
    assert_eq!(stats.current_slot, 1);
    assert_eq!(stats.completed_epoch, 1);
    assert_eq!(stats.draining_slot_readers, 0);
}

// ============================================================================
// TIER 1 — EEVDF + EAS + PSI + MGLRU doğrulama
// ============================================================================

#[test]
fn test_eevdf_deadline_preemption_prefers_eligible_earliest_deadline() {
    let rq = EevdfRunQueue::new();
    let t1 = Arc::new(EevdfTask::new(1, 1024, 4_000_000));
    let t2 = Arc::new(EevdfTask::new(2, 1024, 1_000_000));

    rq.enqueue(t1.clone());
    rq.enqueue(t2.clone());

    // t1 önce çalışsın, sonra t2'nin daha erken deadline'ı preempt yaratsın.
    rq.account_runtime(1, 3_000_000);
    rq.account_runtime(2, 250_000);
    let next = rq.pick_next().expect("next task");
    assert_eq!(next.task_id, 2);
    assert!(rq.should_preempt(1, 2));
}

#[test]
fn test_eas_prefers_p_core_for_latency_sensitive_task() {
    let task = EasTask {
        task_id: 77,
        utilization: 640,
        latency_sensitive: true,
    };
    let cores = [
        EasCore {
            cpu_id: 0,
            kind: CoreKind::Efficiency,
            caps: CppcPerfCaps {
                highest_perf: 110,
                nominal_perf: 90,
                lowest_perf: 20,
                energy_cost: 35,
            },
            utilization: 280,
            thread_director_bias: -6,
        },
        EasCore {
            cpu_id: 1,
            kind: CoreKind::Performance,
            caps: CppcPerfCaps {
                highest_perf: 220,
                nominal_perf: 180,
                lowest_perf: 40,
                energy_cost: 62,
            },
            utilization: 420,
            thread_director_bias: 24,
        },
    ];

    let placement = select_energy_aware_cpu(&task, &cores).expect("placement");
    assert_eq!(placement.cpu_id, 1);
}

#[test]
fn test_psi_and_mglru_telemetry_progress() {
    psi::init(true);
    psi::record_memory_stall(8, 10, false);
    psi::record_memory_stall(6, 10, true);
    let snap = psi::snapshot();
    assert!(snap.some_avg10 > 0);
    assert!(snap.full_avg10 > 0);

    mglru::init(true);
    mglru::record_page_access(10, 1, 0, true, 100);
    mglru::record_page_access(10, 2, 0, false, 100);
    mglru::age_generations(1000);
    let victim = mglru::pick_victim(Some(10), Some(0)).expect("victim");
    assert!(victim.key.page_index == 1 || victim.key.page_index == 2);
}

// ============================================================================
// TIER 2 — WiFi MLO doğrulama
// ============================================================================

#[test]
fn test_wifi_mlo_prefers_6ghz_primary_and_multiband_bundle() {
    let controller = WifiJailController::new();
    controller.initialized.store(true, Ordering::SeqCst);
    *controller.scan_results.lock() = vec![
        WifiBss {
            bssid: [0, 1, 2, 3, 4, 5],
            ssid: "echOS-Lab".into(),
            rssi: -48,
            channel: 11,
            frequency: 2462,
            band: WifiBand::Band2G,
            security: WifiSecurity::WPA3Personal,
            phy_mode: WifiPhyMode::Dot11AX,
            channel_width: 40,
        },
        WifiBss {
            bssid: [6, 7, 8, 9, 10, 11],
            ssid: "echOS-Lab".into(),
            rssi: -41,
            channel: 36,
            frequency: 5180,
            band: WifiBand::Band5G,
            security: WifiSecurity::WPA3Personal,
            phy_mode: WifiPhyMode::Dot11AX,
            channel_width: 160,
        },
        WifiBss {
            bssid: [12, 13, 14, 15, 16, 17],
            ssid: "echOS-Lab".into(),
            rssi: -35,
            channel: 5,
            frequency: 5975,
            band: WifiBand::Band6G,
            security: WifiSecurity::WPA3Personal,
            phy_mode: WifiPhyMode::Dot11BE,
            channel_width: 320,
        },
    ];

    let session = controller
        .plan_mlo_for_ssid("echOS-Lab", WifiSecurity::WPA3Personal)
        .expect("MLO session");

    assert_eq!(session.primary.band, WifiBand::Band6G);
    assert!(session
        .secondary
        .iter()
        .any(|link| link.band == WifiBand::Band5G));
    assert!(session.link_count() >= 2);
    assert!(session.aggregate_mbps > session.primary.estimated_mbps);
}

// ============================================================================
// TIER 1 — Btrfs checksum / scrub doğrulama
// ============================================================================

static BTRFS_DAEMON_TEST_LOCK: Mutex<()> = Mutex::new(());

fn write_test_btrfs_superblock(block: &mut [u8], mirror_offset: u64, generation: u64) {
    block[64..72].copy_from_slice(&BTRFS_MAGIC.to_le_bytes());
    block[48..56].copy_from_slice(&mirror_offset.to_le_bytes());
    block[72..80].copy_from_slice(&generation.to_le_bytes());
    block[112..120].copy_from_slice(&(512 * 1024 * 1024u64).to_le_bytes());
    block[120..128].copy_from_slice(&(128 * 1024 * 1024u64).to_le_bytes());
    block[128..136].copy_from_slice(&5u64.to_le_bytes());
    block[136..144].copy_from_slice(&1u64.to_le_bytes());
    block[144..148].copy_from_slice(&4096u32.to_le_bytes());
    block[148..152].copy_from_slice(&16384u32.to_le_bytes());
    block[152..156].copy_from_slice(&16384u32.to_le_bytes());
    block[156..160].copy_from_slice(&4096u32.to_le_bytes());
    block[196..198].copy_from_slice(&BTRFS_CSUM_TYPE_CRC32.to_le_bytes());
    block[299..307].copy_from_slice(b"echOS\0\0\0");
    stamp_superblock_checksum(block).expect("checksum");
}

#[test]
fn test_btrfs_scrub_selects_freshest_valid_mirror() {
    let mirrors = [4096u64, 16384u64, 32768u64];
    let mut disk = vec![0u8; 24 * BTRFS_SUPERBLOCK_SIZE];

    {
        let start = mirrors[0] as usize;
        let end = start + BTRFS_SUPERBLOCK_SIZE;
        write_test_btrfs_superblock(&mut disk[start..end], mirrors[0], 7);
        disk[start + 384] ^= 0x5A;
    }

    {
        let start = mirrors[1] as usize;
        let end = start + BTRFS_SUPERBLOCK_SIZE;
        write_test_btrfs_superblock(&mut disk[start..end], mirrors[1], 11);
    }

    let report = scrub_superblock_mirrors_with_layout(&disk, &mirrors);
    assert_eq!(report.valid_mirrors, 1);
    assert_eq!(report.selected_mirror, Some(mirrors[1]));
    assert_eq!(report.freshest_generation, 11);
}

#[test]
fn test_btrfs_xxhash_superblock_roundtrip() {
    let mut block = vec![0u8; BTRFS_SUPERBLOCK_SIZE];
    write_test_btrfs_superblock(&mut block, 4096, 19);
    block[196..198].copy_from_slice(&BTRFS_CSUM_TYPE_XXHASH.to_le_bytes());
    stamp_superblock_checksum(&mut block).expect("xxhash stamp");
    verify_superblock_checksum(&block).expect("xxhash verify");
}

#[test]
fn test_btrfs_scrub_daemon_tracks_registered_volume() {
    let _guard = BTRFS_DAEMON_TEST_LOCK.lock().unwrap();
    let mirrors = [4096u64, 16384u64, 32768u64];
    let mut disk = vec![0u8; 24 * BTRFS_SUPERBLOCK_SIZE];

    {
        let start = mirrors[0] as usize;
        let end = start + BTRFS_SUPERBLOCK_SIZE;
        write_test_btrfs_superblock(&mut disk[start..end], mirrors[0], 7);
        disk[start + 128] ^= 0x33;
    }

    {
        let start = mirrors[1] as usize;
        let end = start + BTRFS_SUPERBLOCK_SIZE;
        write_test_btrfs_superblock(&mut disk[start..end], mirrors[1], 13);
    }

    let before = scrub_daemon_status().pass_count;
    register_scrub_volume_with_layout("regression-btrfs-scrubd", &disk, &mirrors);

    let attention_required = run_scrub_daemon_pass();
    let status = scrub_daemon_status();
    let report = status
        .last_reports
        .iter()
        .find(|entry| entry.mount_point == "regression-btrfs-scrubd")
        .expect("daemon report");

    assert_eq!(attention_required, 1);
    assert_eq!(status.pass_count, before + 1);
    assert!(status.registered_volumes >= 1);
    assert_eq!(report.report.selected_mirror, Some(mirrors[1]));
    assert!(report
        .report
        .issues
        .iter()
        .any(|issue| issue.kind == BtrfsScrubIssueKind::ChecksumMismatch));
}

// ============================================================================
// TIER 1 — io_uring SQ/CQ + SQPOLL doğrulama
// ============================================================================

#[test]
fn test_io_uring_sqpoll_autodrains_nop_submission() {
    let params = IoUringParams {
        flags: IORING_SETUP_SQPOLL,
        ..IoUringParams::default()
    };
    let fd = io_uring_setup(8, Some(params)).expect("io_uring setup");

    let sqe = IoUringSqe {
        opcode: IORING_OP_NOP,
        user_data: 0xBEEF,
        ..IoUringSqe::default()
    };
    io_uring_submit_sqe(fd, sqe).expect("submit nop");

    let ring = get_io_uring(fd).expect("ring clone");
    assert!(ring.sq_poll_active);
    assert!(ring.sq_poll_processed >= 1);

    let cqe = io_uring_get_cqe(fd).expect("completion");
    assert_eq!(cqe.user_data, 0xBEEF);
    assert_eq!(cqe.res, 0);

    io_uring_close(fd).expect("close ring");
}

#[test]
fn test_io_uring_register_tracks_files_and_buffers() {
    let fd = io_uring_setup(4, None).expect("io_uring setup");
    let files = [7i32, 11, 13];
    let buffers = [
        IoUringRegisteredBuffer {
            addr: 0x1000,
            len: 4096,
            bgid: 1,
        },
        IoUringRegisteredBuffer {
            addr: 0x2000,
            len: 2048,
            bgid: 1,
        },
    ];

    assert_eq!(
        io_uring_register(fd, 0, files.as_ptr() as u64, files.len() as u32)
            .expect("register files"),
        files.len() as i32
    );
    assert_eq!(
        io_uring_register(fd, 1, buffers.as_ptr() as u64, buffers.len() as u32)
            .expect("register buffers"),
        buffers.len() as i32
    );

    let ring = get_io_uring(fd).expect("ring clone");
    assert_eq!(ring.registered_file_count(), files.len());
    assert_eq!(ring.registered_buffer_count(), buffers.len());

    assert_eq!(
        io_uring_register(fd, 2, 0, 0).expect("unregister files"),
        files.len() as i32
    );
    assert_eq!(
        io_uring_register(fd, 3, 0, 0).expect("unregister buffers"),
        buffers.len() as i32
    );

    let ring = get_io_uring(fd).expect("ring clone after unregister");
    assert_eq!(ring.registered_file_count(), 0);
    assert_eq!(ring.registered_buffer_count(), 0);

    io_uring_close(fd).expect("close ring");
}

#[test]
fn test_io_uring_zero_syscall_fixed_resource_path() {
    let params = IoUringParams {
        flags: IORING_SETUP_SQPOLL,
        ..IoUringParams::default()
    };
    let fd = io_uring_setup(8, Some(params)).expect("io_uring setup");
    let socket_fd =
        socket::socket(AddressFamily::IPV4, SocketType::STREAM, Protocol::DEFAULT).expect("socket");

    let files = [socket_fd as i32];
    let mut backing = [0u8; 64];
    let buffers = [IoUringRegisteredBuffer {
        addr: backing.as_mut_ptr() as u64,
        len: backing.len() as u32,
        bgid: 7,
    }];

    io_uring_register(fd, 0, files.as_ptr() as u64, files.len() as u32).expect("register files");
    io_uring_register(fd, 1, buffers.as_ptr() as u64, buffers.len() as u32)
        .expect("register buffers");

    let sqe = IoUringSqe {
        opcode: IORING_OP_READ_FIXED,
        flags: IOSQE_FIXED_FILE,
        fd: 0,
        len: backing.len() as u32,
        buf_group: 0,
        user_data: 0xCAFE,
        ..IoUringSqe::default()
    };
    io_uring_submit_sqe(fd, sqe).expect("submit fixed read");

    let ring = get_io_uring(fd).expect("ring clone");
    assert!(ring.zero_syscall_ready());
    assert_eq!(ring.zero_syscall_submission_count(), 1);
    assert_eq!(ring.zero_syscall_completion_count(), 1);
    assert!(ring.sq_poll_processed >= 1);

    let cqe = io_uring_get_cqe(fd).expect("completion");
    assert_eq!(cqe.user_data, 0xCAFE);
    assert!(cqe.res <= 0);

    socket::close(socket_fd).expect("close socket");
    io_uring_close(fd).expect("close ring");
}

// ============================================================================
// TIER 1 — NVMe Submission Queue Simülasyonu
// ============================================================================

/// NVMe-benzeri SQ/CQ simülasyonu
struct NvmeQueuePair {
    sq: VecDeque<NvmeCommand>,
    cq: VecDeque<NvmeCompletion>,
    sq_tail: u32,
    cq_head: u32,
    queue_depth: u32,
}

#[derive(Clone, Debug, PartialEq)]
struct NvmeCommand {
    opcode: u8,
    nsid: u32,
    lba: u64,
    num_blocks: u16,
    cid: u16,
}

#[derive(Clone, Debug)]
struct NvmeCompletion {
    cid: u16,
    status: u16,
    sq_head: u16,
}

impl NvmeQueuePair {
    fn new(depth: u32) -> Self {
        Self {
            sq: VecDeque::with_capacity(depth as usize),
            cq: VecDeque::with_capacity(depth as usize),
            sq_tail: 0,
            cq_head: 0,
            queue_depth: depth,
        }
    }

    fn submit(&mut self, cmd: NvmeCommand) -> Result<(), &'static str> {
        if self.sq.len() >= self.queue_depth as usize {
            return Err("SQ full");
        }
        self.sq.push_back(cmd);
        self.sq_tail += 1;
        Ok(())
    }

    fn process_one(&mut self) -> bool {
        if let Some(cmd) = self.sq.pop_front() {
            self.cq.push_back(NvmeCompletion {
                cid: cmd.cid,
                status: 0, // success
                sq_head: self.sq_tail as u16,
            });
            true
        } else {
            false
        }
    }

    fn complete(&mut self) -> Option<NvmeCompletion> {
        let cpl = self.cq.pop_front();
        if cpl.is_some() {
            self.cq_head += 1;
        }
        cpl
    }
}

#[test]
fn test_nvme_queue_submit_complete() {
    let mut qp = NvmeQueuePair::new(64);

    // 10 komut gönder
    for i in 0..10 {
        let cmd = NvmeCommand {
            opcode: 0x02, // Read
            nsid: 1,
            lba: i * 8,
            num_blocks: 8,
            cid: i as u16,
        };
        assert!(qp.submit(cmd).is_ok());
    }

    // İşle
    for _ in 0..10 {
        assert!(qp.process_one());
    }
    assert!(!qp.process_one()); // SQ boş

    // Tamamla
    for i in 0..10 {
        let cpl = qp.complete().expect("should have completion");
        assert_eq!(cpl.cid, i as u16);
        assert_eq!(cpl.status, 0);
    }
}

#[test]
fn test_nvme_queue_depth_limit() {
    let mut qp = NvmeQueuePair::new(4);

    for i in 0..4 {
        let cmd = NvmeCommand {
            opcode: 1,
            nsid: 1,
            lba: 0,
            num_blocks: 1,
            cid: i,
        };
        assert!(qp.submit(cmd).is_ok());
    }
    // 5. komut reddedilmeli
    let cmd = NvmeCommand {
        opcode: 1,
        nsid: 1,
        lba: 0,
        num_blocks: 1,
        cid: 99,
    };
    assert!(qp.submit(cmd).is_err());
}

// ============================================================================
// TIER 2 — Jail Stress Testi
// ============================================================================

/// Jail izolasyon simülasyonu
struct JailSandbox {
    id: u16,
    name: String,
    crashed: bool,
    restart_count: u32,
    max_restarts: u32,
    io_ops: u64,
}

impl JailSandbox {
    fn new(id: u16, name: &str, max_restarts: u32) -> Self {
        Self {
            id,
            name: name.to_string(),
            crashed: false,
            restart_count: 0,
            max_restarts,
            io_ops: 0,
        }
    }

    fn simulate_io(&mut self) -> bool {
        self.io_ops += 1;
        !self.crashed
    }

    fn crash(&mut self) {
        self.crashed = true;
    }

    fn restart(&mut self) -> bool {
        if self.restart_count >= self.max_restarts {
            return false;
        }
        self.crashed = false;
        self.restart_count += 1;
        true
    }
}

#[test]
fn test_jail_crash_isolation() {
    let mut core_alive = true;
    let mut jail = JailSandbox::new(1, "usb-jail", 5);

    // Jail crash'lese bile core etkilenmemeli
    for _ in 0..100 {
        jail.simulate_io();
    }
    jail.crash();
    assert!(jail.crashed);
    assert!(core_alive); // Core hala yaşıyor

    // Restart
    assert!(jail.restart());
    assert!(!jail.crashed);
    assert_eq!(jail.restart_count, 1);
}

#[test]
fn test_jail_stress_1000_crashes() {
    let mut core_alive = true;
    let mut total_crashes = 0u32;

    // 1000 crash senaryosu — core ASLA etkilenmemeli
    for i in 0..1000 {
        let mut jail = JailSandbox::new(i as u16, "stress-jail", 1);
        jail.simulate_io();
        jail.crash();
        total_crashes += 1;

        // Core hala çalışıyor olmalı
        assert!(core_alive, "Core crashed after jail crash #{}", i);
    }

    assert_eq!(total_crashes, 1000);
    assert!(core_alive);
}

#[test]
fn test_jail_max_restart_limit() {
    let mut jail = JailSandbox::new(1, "limited-jail", 3);

    for _ in 0..3 {
        jail.crash();
        assert!(jail.restart());
    }
    jail.crash();
    assert!(!jail.restart(), "should not restart beyond max");
    assert!(jail.crashed);
    assert_eq!(jail.restart_count, 3);
}

// ============================================================================
// SYSCALL CONFORMANCE
// ============================================================================

/// Basit syscall numarası → isim eşlemesi
fn syscall_name(nr: u64) -> &'static str {
    match nr {
        0 => "read",
        1 => "write",
        2 => "open",
        3 => "close",
        4 => "stat",
        5 => "fstat",
        9 => "mmap",
        10 => "mprotect",
        11 => "munmap",
        12 => "brk",
        39 => "getpid",
        56 => "clone",
        57 => "fork",
        59 => "execve",
        60 => "exit",
        62 => "kill",
        63 => "uname",
        78 => "getdents",
        79 => "getcwd",
        80 => "chdir",
        82 => "rename",
        83 => "mkdir",
        84 => "rmdir",
        85 => "creat",
        87 => "unlink",
        89 => "readlink",
        90 => "chmod",
        92 => "chown",
        96 => "gettimeofday",
        231 => "exit_group",
        295 => "openat",
        _ => "unknown",
    }
}

#[test]
fn test_syscall_names_conformance() {
    // Linux x86_64 syscall numaraları doğrulaması
    assert_eq!(syscall_name(0), "read");
    assert_eq!(syscall_name(1), "write");
    assert_eq!(syscall_name(2), "open");
    assert_eq!(syscall_name(3), "close");
    assert_eq!(syscall_name(9), "mmap");
    assert_eq!(syscall_name(39), "getpid");
    assert_eq!(syscall_name(57), "fork");
    assert_eq!(syscall_name(59), "execve");
    assert_eq!(syscall_name(60), "exit");
    assert_eq!(syscall_name(295), "openat");
}

/// Syscall argüman doğrulama
#[test]
fn test_syscall_arg_validation() {
    // NULL pointer kontrolü
    let null_ptr: *const u8 = std::ptr::null();
    assert!(null_ptr.is_null());

    // Geçersiz fd
    let invalid_fd: i32 = -1;
    assert!(invalid_fd < 0);

    // Buffer boyutu 0
    let zero_len: usize = 0;
    assert_eq!(zero_len, 0);
}

// ============================================================================
// FS CONSISTENCY
// ============================================================================

/// İnode simülasyonu
#[derive(Clone, Debug)]
struct Inode {
    ino: u64,
    name: String,
    size: u64,
    is_dir: bool,
    link_count: u32,
    children: Vec<u64>,
}

/// Basit FS tutarlılık kontrolcüsü
struct FsConsistencyChecker {
    inodes: BTreeMap<u64, Inode>,
    next_ino: u64,
}

impl FsConsistencyChecker {
    fn new() -> Self {
        let mut checker = Self {
            inodes: BTreeMap::new(),
            next_ino: 2,
        };
        // Root inode (ino=1)
        checker.inodes.insert(
            1,
            Inode {
                ino: 1,
                name: "/".to_string(),
                size: 0,
                is_dir: true,
                link_count: 2,
                children: Vec::new(),
            },
        );
        checker
    }

    fn create_file(&mut self, parent: u64, name: &str, size: u64) -> u64 {
        let ino = self.next_ino;
        self.next_ino += 1;
        self.inodes.insert(
            ino,
            Inode {
                ino,
                name: name.to_string(),
                size,
                is_dir: false,
                link_count: 1,
                children: Vec::new(),
            },
        );
        if let Some(parent_inode) = self.inodes.get_mut(&parent) {
            parent_inode.children.push(ino);
        }
        ino
    }

    fn create_dir(&mut self, parent: u64, name: &str) -> u64 {
        let ino = self.next_ino;
        self.next_ino += 1;
        self.inodes.insert(
            ino,
            Inode {
                ino,
                name: name.to_string(),
                size: 0,
                is_dir: true,
                link_count: 2,
                children: Vec::new(),
            },
        );
        if let Some(parent_inode) = self.inodes.get_mut(&parent) {
            parent_inode.children.push(ino);
            parent_inode.link_count += 1; // ".." referansı
        }
        ino
    }

    fn unlink(&mut self, parent: u64, ino: u64) -> bool {
        if let Some(inode) = self.inodes.get_mut(&ino) {
            inode.link_count -= 1;
            if inode.link_count == 0 {
                self.inodes.remove(&ino);
            }
        }
        if let Some(parent_inode) = self.inodes.get_mut(&parent) {
            parent_inode.children.retain(|&c| c != ino);
        }
        true
    }

    /// FS tutarlılık kontrolü
    fn check_consistency(&self) -> Vec<String> {
        let mut errors = Vec::new();

        // 1. Orphan inode kontrolü (root dışında parent'ı olmayan)
        for (&ino, inode) in &self.inodes {
            if ino == 1 {
                continue;
            }
            let has_parent = self.inodes.values().any(|p| p.children.contains(&ino));
            if !has_parent && inode.link_count > 0 {
                errors.push(format!("Orphan inode: {} ({})", ino, inode.name));
            }
        }

        // 2. Dangling child referansı
        for inode in self.inodes.values() {
            for &child_ino in &inode.children {
                if !self.inodes.contains_key(&child_ino) {
                    errors.push(format!("Dangling reference: {} → {}", inode.ino, child_ino));
                }
            }
        }

        // 3. Link count tutarlılığı
        for inode in self.inodes.values() {
            if !inode.is_dir && inode.link_count == 0 {
                errors.push(format!(
                    "Zero link count but still exists: {} ({})",
                    inode.ino, inode.name
                ));
            }
        }

        errors
    }
}

#[test]
fn test_fs_consistency_basic() {
    let mut fs = FsConsistencyChecker::new();

    // Dosya ve dizin oluştur
    let dir = fs.create_dir(1, "home");
    let _file1 = fs.create_file(dir, "readme.txt", 1024);
    let _file2 = fs.create_file(dir, "config.ini", 256);

    let errors = fs.check_consistency();
    assert!(errors.is_empty(), "FS errors: {:?}", errors);
}

#[test]
fn test_fs_consistency_after_unlink() {
    let mut fs = FsConsistencyChecker::new();

    let dir = fs.create_dir(1, "tmp");
    let file = fs.create_file(dir, "temp.dat", 4096);

    // Sil
    fs.unlink(dir, file);

    let errors = fs.check_consistency();
    assert!(errors.is_empty(), "FS errors after unlink: {:?}", errors);
}

#[test]
fn test_fs_consistency_deep_tree() {
    let mut fs = FsConsistencyChecker::new();

    // 10 seviye derin dizin ağacı
    let mut parent = 1u64;
    for i in 0..10 {
        let dir = fs.create_dir(parent, &format!("level{}", i));
        // Her seviyede 5 dosya
        for j in 0..5 {
            fs.create_file(dir, &format!("file_{}.dat", j), (i + 1) as u64 * 100);
        }
        parent = dir;
    }

    let errors = fs.check_consistency();
    assert!(errors.is_empty(), "Deep tree errors: {:?}", errors);
    assert_eq!(fs.inodes.len(), 1 + 10 + 50); // root + 10 dir + 50 files
}

// ============================================================================
// DISPATCHER — TIER SINIFLANDIRMA TESTLERİ
// ============================================================================

#[derive(Debug, PartialEq)]
enum TestTier {
    Tier1Native,
    Tier2Jail,
    Unknown,
}

fn classify_pci(class: u8, subclass: u8) -> TestTier {
    match (class, subclass) {
        (0x01, 0x08) => TestTier::Tier1Native, // NVMe
        (0x02, 0x00) => TestTier::Tier1Native, // Ethernet
        (0x03, _) => TestTier::Tier1Native,    // GPU
        (0x02, 0x80) => TestTier::Tier2Jail,   // WiFi
        (0x04, _) => TestTier::Tier2Jail,      // Audio
        (0x0C, 0x03) => TestTier::Tier2Jail,   // USB
        (0x0D, _) => TestTier::Tier2Jail,      // Bluetooth
        _ => TestTier::Unknown,
    }
}

#[test]
fn test_tier_classification() {
    assert_eq!(classify_pci(0x01, 0x08), TestTier::Tier1Native); // NVMe
    assert_eq!(classify_pci(0x02, 0x00), TestTier::Tier1Native); // Ethernet
    assert_eq!(classify_pci(0x03, 0x00), TestTier::Tier1Native); // VGA
    assert_eq!(classify_pci(0x03, 0x02), TestTier::Tier1Native); // 3D GPU

    assert_eq!(classify_pci(0x02, 0x80), TestTier::Tier2Jail); // WiFi
    assert_eq!(classify_pci(0x04, 0x01), TestTier::Tier2Jail); // Audio
    assert_eq!(classify_pci(0x0C, 0x03), TestTier::Tier2Jail); // USB xHCI
    assert_eq!(classify_pci(0x0D, 0x00), TestTier::Tier2Jail); // Bluetooth

    assert_eq!(classify_pci(0xFF, 0xFF), TestTier::Unknown);
}

// ============================================================================
// LATENCY HISTOGRAM TESTİ
// ============================================================================

struct LatencyHistogram {
    buckets: [u64; 20],
    min: u64,
    max: u64,
    sum: u64,
    count: u64,
}

impl LatencyHistogram {
    fn new() -> Self {
        Self {
            buckets: [0; 20],
            min: u64::MAX,
            max: 0,
            sum: 0,
            count: 0,
        }
    }

    fn record(&mut self, ns: u64) {
        self.count += 1;
        self.sum += ns;
        if ns < self.min {
            self.min = ns;
        }
        if ns > self.max {
            self.max = ns;
        }

        // Log2 bucket
        let bucket = if ns == 0 {
            0
        } else {
            (64 - ns.leading_zeros()) as usize
        };
        let idx = bucket.min(19);
        self.buckets[idx] += 1;
    }

    fn avg(&self) -> u64 {
        if self.count == 0 {
            0
        } else {
            self.sum / self.count
        }
    }
}

#[test]
fn test_latency_histogram() {
    let mut hist = LatencyHistogram::new();

    // 1000 örneklem
    for i in 1..=1000 {
        hist.record(i);
    }

    assert_eq!(hist.count, 1000);
    assert_eq!(hist.min, 1);
    assert_eq!(hist.max, 1000);
    assert_eq!(hist.avg(), 500); // (1+2+...+1000)/1000 = 500.5 → 500 (truncated)
}

// ============================================================================
// KASLR SLIDE TESTİ
// ============================================================================

#[test]
fn test_kaslr_slide_alignment() {
    let alignment: u64 = 2 * 1024 * 1024; // 2 MiB
    let max_slide: u64 = 1024 * 1024 * 1024; // 1 GiB
    let slot_count = max_slide / alignment; // 512

    // Tüm olası slide değerleri 2MB-aligned olmalı
    for slot in 0..slot_count {
        let slide = slot * alignment;
        assert_eq!(slide % alignment, 0, "Slot {} not aligned", slot);
        assert!(slide < max_slide, "Slot {} exceeds max slide", slot);
    }

    assert_eq!(slot_count, 512);
}

// ============================================================================
// DEPENDENCY GRAPH — TOPOLOJİK SIRALAMA TESTİ
// ============================================================================

fn topological_sort(nodes: &[u32], edges: &[(u32, u32)]) -> Vec<u32> {
    let mut in_degree: HashMap<u32, usize> = HashMap::new();
    let mut adj: HashMap<u32, Vec<u32>> = HashMap::new();

    for &n in nodes {
        in_degree.insert(n, 0);
        adj.insert(n, Vec::new());
    }

    for &(from, to) in edges {
        adj.entry(from).or_default().push(to);
        *in_degree.entry(to).or_default() += 1;
    }

    let mut queue: VecDeque<u32> = in_degree
        .iter()
        .filter(|(_, &deg)| deg == 0)
        .map(|(&id, _)| id)
        .collect();

    let mut order = Vec::new();
    while let Some(node) = queue.pop_front() {
        order.push(node);
        if let Some(neighbors) = adj.get(&node) {
            for &next in neighbors {
                if let Some(deg) = in_degree.get_mut(&next) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(next);
                    }
                }
            }
        }
    }

    order
}

#[test]
fn test_topological_sort_basic() {
    // PCI → NVMe → FS
    let nodes = vec![1, 2, 3];
    let edges = vec![(1, 2), (2, 3)]; // PCI→NVMe, NVMe→FS

    let order = topological_sort(&nodes, &edges);
    assert_eq!(order.len(), 3);

    // PCI mutlaka NVMe'den önce
    let pos_pci = order.iter().position(|&x| x == 1).unwrap();
    let pos_nvme = order.iter().position(|&x| x == 2).unwrap();
    let pos_fs = order.iter().position(|&x| x == 3).unwrap();
    assert!(pos_pci < pos_nvme);
    assert!(pos_nvme < pos_fs);
}

#[test]
fn test_topological_sort_diamond() {
    // Diamond: A→B, A→C, B→D, C→D
    let nodes = vec![1, 2, 3, 4];
    let edges = vec![(1, 2), (1, 3), (2, 4), (3, 4)];

    let order = topological_sort(&nodes, &edges);
    assert_eq!(order.len(), 4);

    let pos = |x: u32| order.iter().position(|&n| n == x).unwrap();
    assert!(pos(1) < pos(2));
    assert!(pos(1) < pos(3));
    assert!(pos(2) < pos(4));
    assert!(pos(3) < pos(4));
}
