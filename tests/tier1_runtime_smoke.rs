#![cfg(not(target_os = "none"))]

use ech_os::drivers::async_traits::{AsyncBlockDevice, DmaBuffer};
use ech_os::drivers::drm::{
    AtomicKmsTransaction, DmaReservationUsage, DrmConnector, DrmConnectorStatus, DrmCrtc,
    DrmDevice, DrmMode, DrmPlane, DrmPlaneType, GPUBufferHandle, PlaneCandidate,
};
use ech_os::drivers::iommu::{IommuUnit, IommuVendor};
use ech_os::drivers::nic_native::NicNativeDevice;
use ech_os::drivers::nvme::NvmeAsyncBlockDevice;
use ech_os::gui::protocol::{DisplayPresentMode, Rect};
use ech_os::net::io_uring::{
    IoUring, IoUringParams, IoUringRegisteredBuffer, IoUringSqe, IORING_OP_NOP, IORING_SETUP_SQPOLL,
};
use ech_os::task::ghost::{
    active_policy, commit_policy, note_policy_dispatch, policy_snapshot, register_agent,
};
use std::sync::Arc;
use x86_64::VirtAddr;

fn scheduler_policy_smoke() {
    println!("smoke:scheduler:start");
    let agent = register_agent(7, VirtAddr::new(0x1000), VirtAddr::new(0x2000));
    agent.lock().register_task(41);

    assert!(commit_policy(7, 0, 41, 99, 128, 32));
    let decision = active_policy(0, 0).expect("policy should be active");
    assert_eq!(decision.task_id, 41);
    assert!(note_policy_dispatch(0, 41, decision.generation));

    let snapshot = policy_snapshot(0).expect("policy snapshot");
    assert_eq!(snapshot.committed, 1);
    assert_eq!(snapshot.dispatched, 1);
    println!("smoke:scheduler:ok");
}

fn iommu_smoke() {
    println!("smoke:iommu:start");
    let mut mmio = vec![0u64; 1024].into_boxed_slice();
    mmio[(ech_os::drivers::iommu::VTD_ECAP_REG / 8) as usize] = 1 << 15;
    let mmio_ptr = Box::leak(mmio).as_mut_ptr() as u64;
    let mut unit = IommuUnit::new(0, IommuVendor::Intel, mmio_ptr);
    unit.bind_verification_mmio(mmio_ptr);
    println!("smoke:iommu:init");
    unit.init_verification_ats().expect("iommu ats");
    println!("smoke:iommu:ats");
    let domain_id = unit.create_domain();
    let domain = unit.get_domain(domain_id).expect("domain");

    domain
        .bind_process_address_space(100, 77, 0xAA55, 0x2000)
        .expect("bind pasid");
    println!("smoke:iommu:bind");
    domain
        .attach_device_with_pasid(0, 0x0100, 77)
        .expect("attach device");
    domain
        .register_sva_window(100, 77, 0x1_0000_0000, 0x20_0000)
        .expect("sva window");
    domain
        .map_gpu_virtual_address(77, 0x4000_0000, 0x8000, 0x2000, true, true, 0x4000)
        .expect("gpuva");
    assert!(domain.try_consume_pri_budget(4));
    domain.record_fault_replay(0x0100, 77, 0x4000_0000);
    domain.release_pri_budget(4);
    let request_id = domain
        .queue_page_request(0x0100, 77, 0x4000_0100, 0x1000, true)
        .expect("queue page request");
    println!("smoke:iommu:queued:{request_id}");
    let queued = domain
        .pending_page_request_snapshot()
        .into_iter()
        .next()
        .expect("pending snapshot request");
    unit.inject_intel_page_request(&queued).expect("inject prq");
    println!("smoke:iommu:injected");
    let replay = domain.process_page_requests(4);
    println!("smoke:iommu:serviced:{}", replay.len());
    assert_eq!(replay.len(), 1);
    assert_eq!(replay[0].request_id, request_id);
    assert!(replay[0].replayed);
    let qsnap = unit.page_request_queue_snapshot().expect("prq snapshot");
    assert_eq!(qsnap.head, 0);
    assert_eq!(qsnap.tail, 1);
    assert_eq!(qsnap.completed_responses, 0);

    let snapshot = domain.shared_va_snapshot();
    assert_eq!(snapshot.pasid_bindings, 1);
    assert_eq!(snapshot.sva_windows, 1);
    assert_eq!(snapshot.gpuva_ranges, 1);
    assert_eq!(snapshot.device_bindings, 1);
    assert_eq!(snapshot.pending_page_requests, 0);
    assert_eq!(snapshot.completed_page_replays, 1);
    assert_eq!(snapshot.invalidation_records, 1);
    assert_eq!(
        domain
            .translate_gpuva(77, 0x4000_0100)
            .expect("gpuva lookup")
            .phys_addr,
        0x8000
    );
    println!("smoke:iommu:ok");
}

fn nic_vendor_doorbell_smoke() {
    println!("smoke:nic:start");
    let mmio = vec![0u32; 0x4000 / 4].into_boxed_slice();
    let mmio_ptr = Box::leak(mmio).as_mut_ptr() as u64;
    let nic = NicNativeDevice::new_intel_8254x("e1000-smoke", [0; 6], mmio_ptr);
    let dma = DmaBuffer {
        vaddr: 0x1000,
        paddr: 0x2000,
        size: 2048,
    };
    let _ = ech_os::drivers::async_traits::AsyncNetDevice::submit_tx(&nic, &dma, 512)
        .expect("tx submit");
    let snapshot = nic.doorbell_snapshot();
    assert_eq!(snapshot.tx_tail, 1);
    assert_eq!(snapshot.irq_mask, u32::MAX);
    println!("smoke:nic:ok");
}

fn mk_drm_device() -> DrmDevice {
    let device = DrmDevice::new(1, "card-smoke");
    let crtc = Arc::new(DrmCrtc::new(1, 0));
    let connector = Arc::new(DrmConnector::new(1, 1));
    *connector.connection.lock() = DrmConnectorStatus::Connected;
    connector.add_mode(DrmMode {
        clock: 148500,
        hdisplay: 1920,
        hsync_start: 2008,
        hsync_end: 2052,
        htotal: 2200,
        hskew: 0,
        vdisplay: 1080,
        vsync_start: 1084,
        vsync_end: 1089,
        vtotal: 1125,
        vscan: 0,
        vrefresh: 60,
        flags: 0,
        type_: 0,
        name: [0; 32],
    });
    device.add_crtc(crtc);
    device.add_connector(connector);
    device.add_plane(Arc::new(DrmPlane::new_with_type(1, DrmPlaneType::Primary)));
    device
}

fn drm_smoke() {
    println!("smoke:drm:start");
    let device = mk_drm_device();
    let exported = device.gem_create(4096);
    let exported_fd = device
        .export_dma_buf_handle(exported.handle)
        .expect("dma-buf export")
        .fd;
    let imported = device
        .import_dma_buf_fd_for_process(exported_fd, 55)
        .expect("dma-buf import");
    let consumer = device.gem_create(4096);
    let leaf = device.gem_create(4096);
    device
        .link_cross_process_reservation_with_usage(
            imported.handle,
            consumer.handle,
            55,
            DmaReservationUsage::Write,
        )
        .expect("link graph");
    device
        .link_cross_process_reservation_with_usage(
            consumer.handle,
            leaf.handle,
            55,
            DmaReservationUsage::Read,
        )
        .expect("link graph");

    let txn = AtomicKmsTransaction {
        frame_id: 100,
        commit_id: 55,
        crtc_id: 1,
        connector_id: 1,
        mode: None,
        planes: vec![PlaneCandidate {
            surface_id: exported.handle as u64,
            plane_type: DrmPlaneType::Primary,
            z: 0,
            src: Rect::new(0, 0, 64, 64),
            dst: Rect::new(0, 0, 64, 64),
            opaque: true,
            format: 0,
            buffer: GPUBufferHandle {
                handle: exported.handle as u64,
                paddr: 0x1000,
                width: 64,
                height: 64,
                stride: 256,
                format: 0,
            },
        }],
        damage_regions: vec![],
        target_refresh_hz: 60,
        present_mode: DisplayPresentMode::VblankFifo,
    };

    let result = device.commit_transaction(&txn).expect("atomic commit");
    assert_eq!(
        device
            .reservation_snapshot(consumer.handle)
            .expect("consumer reservation")
            .fence
            .target_value,
        55
    );
    assert_eq!(device.reservation_graph_snapshot(imported.handle).len(), 2);
    assert!(device.report_flip_complete(100, 55, result.vblank_seq, result.timestamp_ns));
    assert_eq!(
        device
            .reservation_snapshot(consumer.handle)
            .expect("consumer completion")
            .fence
            .current_value,
        55
    );
    assert_eq!(
        device
            .reservation_snapshot(leaf.handle)
            .expect("leaf completion")
            .fence
            .current_value,
        55
    );
    println!("smoke:drm:ok");
}

fn io_uring_smoke() {
    println!("smoke:io_uring:start");
    let params = IoUringParams {
        flags: IORING_SETUP_SQPOLL,
        ..IoUringParams::default()
    };
    let mut ring = IoUring::new(8, Some(params)).expect("io_uring");
    ring.registered_files.push(3);
    ring.registered_buffers.push(IoUringRegisteredBuffer {
        addr: 0x1000,
        len: 64,
        bgid: 1,
    });

    let sqes = [
        IoUringSqe {
            opcode: IORING_OP_NOP,
            user_data: 1,
            ..IoUringSqe::default()
        },
        IoUringSqe {
            opcode: IORING_OP_NOP,
            user_data: 2,
            ..IoUringSqe::default()
        },
    ];

    let submitted = ring.submit_sqes(&sqes).expect("submit batch");
    assert_eq!(submitted, 2);
    let processed = ring.process_pending_budgeted(2);
    assert!(processed == 2 || processed == 0);
    let cqe0 = ring.get_cqe().expect("cqe0");
    let cqe1 = ring.get_cqe().expect("cqe1");
    assert_eq!(cqe0.res, 0);
    assert_eq!(cqe1.res, 0);
    assert_eq!(ring.batching_snapshot(), (1, 1, 2, 2));
    println!("smoke:io_uring:ok");
}

fn nvme_async_smoke() {
    println!("smoke:nvme:start");
    let mmio = vec![0u32; 2].into_boxed_slice();
    let sq = vec![ech_os::drivers::nvme::NvmeCommand::new(0, 0, 0); 8].into_boxed_slice();
    let cq = vec![
        ech_os::drivers::nvme::NvmeCompletion {
            cid: 0,
            p: 0,
            sqid: 0,
            status: 1,
            cdw0: 0,
            cdw1: 0,
        };
        8
    ]
    .into_boxed_slice();

    let mmio_ptr = Box::leak(mmio).as_mut_ptr() as u64;
    let sq_ptr = Box::leak(sq).as_mut_ptr() as u64;
    let cq_ptr = Box::leak(cq).as_mut_ptr() as u64;
    let nvme =
        NvmeAsyncBlockDevice::from_raw_queue(1, 4096, 1024, mmio_ptr, sq_ptr, cq_ptr, 8, 0, 4);
    let dma = DmaBuffer {
        vaddr: 0x1000,
        paddr: 0x2000,
        size: 4096,
    };

    let token = nvme.submit_read(8, 1, &dma).expect("submit read");
    unsafe {
        nvme.inject_verification_completion(1, 1, 4096);
    }
    let completion = nvme.poll_completion().expect("poll completion");
    assert_eq!(completion.token, token);
    assert_eq!(completion.result, 0);
    assert_eq!(completion.data_len, 4096);
    println!("smoke:nvme:ok");
}

fn nvme_controller_smoke() {
    println!("smoke:nvme_ctrl:start");
    let mut mmio = vec![0u32; 1024].into_boxed_slice();
    mmio[0x1c / 4] = 1; // CSTS_RDY = 1
    let mut ctrl = ech_os::drivers::nvme::NvmeController::new(0, 0, 0);
    ctrl.mmio_base = mmio.as_mut_ptr() as u64;
    ctrl.timeout_ms = 1; // Hızlı timeout
    ctrl.capabilities.nvm_subsystem_reset = true;
    
    // reset expects RDY to drop, but our mock MMIO keeps it at 1, so it should TIMEOUT
    let res = ctrl.controller_reset();
    assert_eq!(res, Err(ech_os::drivers::nvme::NvmeError::Timeout));
    println!("smoke:nvme_ctrl:ok");
}

fn main() {
    scheduler_policy_smoke();
    iommu_smoke();
    drm_smoke();
    io_uring_smoke();
    nvme_async_smoke();
    nvme_controller_smoke();
    nic_vendor_doorbell_smoke();
    println!("tier1_runtime_smoke: OK");
}
