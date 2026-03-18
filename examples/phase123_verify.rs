#![cfg(not(target_os = "none"))]

use std::process::ExitCode;
use std::sync::atomic::Ordering;

use ech_os::drivers::wifi_jail::{
    WifiBand, WifiBss, WifiJailController, WifiPhyMode, WifiSecurity,
};
use ech_os::fs::btrfs::{
    register_scrub_volume_with_layout, run_scrub_daemon_pass, scrub_daemon_status,
    scrub_superblock_mirrors_with_layout, stamp_superblock_checksum, verify_superblock_checksum,
    BTRFS_CSUM_TYPE_CRC32, BTRFS_CSUM_TYPE_XXHASH, BTRFS_MAGIC, BTRFS_SUPERBLOCK_SIZE,
};
use ech_os::net::io_uring::{
    get_cqe as io_uring_get_cqe, get_io_uring, io_uring_close, io_uring_register, io_uring_setup,
    submit_sqe as io_uring_submit_sqe, IoUringParams, IoUringRegisteredBuffer, IoUringSqe,
    IORING_OP_NOP, IORING_OP_READ_FIXED, IORING_SETUP_SQPOLL, IOSQE_FIXED_FILE,
};
use ech_os::net::socket::{self, AddressFamily, Protocol, SocketType};
use ech_os::rcu::{SrcuDomain, TreeRcuDomain};

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

fn verify_tree_rcu() {
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
}

fn verify_srcu() {
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

fn verify_wifi_mlo() {
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

fn verify_btrfs() {
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

    let mut xxhash_block = vec![0u8; BTRFS_SUPERBLOCK_SIZE];
    write_test_btrfs_superblock(&mut xxhash_block, 4096, 19);
    xxhash_block[196..198].copy_from_slice(&BTRFS_CSUM_TYPE_XXHASH.to_le_bytes());
    stamp_superblock_checksum(&mut xxhash_block).expect("xxhash stamp");
    verify_superblock_checksum(&xxhash_block).expect("xxhash verify");

    let before = scrub_daemon_status().pass_count;
    register_scrub_volume_with_layout("phase123-verify", &disk, &mirrors);
    let attention_required = run_scrub_daemon_pass();
    let status = scrub_daemon_status();
    let report = status
        .last_reports
        .iter()
        .find(|entry| entry.mount_point == "phase123-verify")
        .expect("scrub daemon report");

    assert_eq!(attention_required, 1);
    assert_eq!(status.pass_count, before + 1);
    assert_eq!(report.report.selected_mirror, Some(mirrors[1]));
}

fn verify_io_uring() {
    let params = IoUringParams {
        flags: IORING_SETUP_SQPOLL,
        ..IoUringParams::default()
    };
    let fd = io_uring_setup(8, Some(params)).expect("io_uring setup");

    let sqe = IoUringSqe {
        opcode: IORING_OP_NOP,
        user_data: 0xCAFE,
        ..IoUringSqe::default()
    };
    io_uring_submit_sqe(fd, sqe).expect("submit nop");

    let ring = get_io_uring(fd).expect("ring clone");
    assert!(ring.sq_poll_active);
    assert!(ring.sq_poll_processed >= 1);
    let cqe = io_uring_get_cqe(fd).expect("completion");
    assert_eq!(cqe.user_data, 0xCAFE);
    assert_eq!(cqe.res, 0);

    let files = [3i32, 4, 5];
    let buffers = [IoUringRegisteredBuffer {
        addr: 0x1000,
        len: 4096,
        bgid: 1,
    }];
    io_uring_register(fd, 0, files.as_ptr() as u64, files.len() as u32).expect("register files");
    io_uring_register(fd, 1, buffers.as_ptr() as u64, buffers.len() as u32)
        .expect("register buffers");

    let ring = get_io_uring(fd).expect("ring clone after register");
    assert_eq!(ring.registered_file_count(), files.len());
    assert_eq!(ring.registered_buffer_count(), buffers.len());

    let socket_fd =
        socket::socket(AddressFamily::IPV4, SocketType::STREAM, Protocol::DEFAULT).expect("socket");
    let fixed_files = [socket_fd as i32];
    let mut fast_path_buf = [0u8; 64];
    let fixed_buffers = [IoUringRegisteredBuffer {
        addr: fast_path_buf.as_mut_ptr() as u64,
        len: fast_path_buf.len() as u32,
        bgid: 9,
    }];
    io_uring_register(fd, 0, fixed_files.as_ptr() as u64, fixed_files.len() as u32)
        .expect("register fast-path files");
    io_uring_register(
        fd,
        1,
        fixed_buffers.as_ptr() as u64,
        fixed_buffers.len() as u32,
    )
    .expect("register fast-path buffers");

    let fixed_sqe = IoUringSqe {
        opcode: IORING_OP_READ_FIXED,
        flags: IOSQE_FIXED_FILE,
        fd: 0,
        len: fast_path_buf.len() as u32,
        buf_group: 0,
        user_data: 0xD15C0,
        ..IoUringSqe::default()
    };
    io_uring_submit_sqe(fd, fixed_sqe).expect("submit fixed sqe");

    let ring = get_io_uring(fd).expect("ring clone for zero-syscall");
    assert!(ring.zero_syscall_ready());
    assert!(ring.zero_syscall_submission_count() >= 1);
    assert!(ring.zero_syscall_completion_count() >= 1);

    let fixed_cqe = io_uring_get_cqe(fd).expect("fixed completion");
    assert_eq!(fixed_cqe.user_data, 0xD15C0);
    assert!(fixed_cqe.res <= 0);

    socket::close(socket_fd).expect("close socket");

    io_uring_close(fd).expect("close ring");
}

fn main() -> ExitCode {
    verify_tree_rcu();
    verify_srcu();
    verify_wifi_mlo();
    verify_btrfs();
    verify_io_uring();
    println!("phase123 verification ok");
    ExitCode::SUCCESS
}
