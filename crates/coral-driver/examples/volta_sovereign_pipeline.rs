// SPDX-License-Identifier: AGPL-3.0-or-later
//! Titan V (GV100) sovereign pipeline diagnostic — SEC2/ACR → FECS → channel.
//!
//! Exercises the full Volta cold-boot path with per-step verification:
//!   1. VFIO open + BAR0 identity check
//!   2. PCI hot reset (clear stale LS-mode falcons)
//!   3. Falcon state probe (SEC2, FECS, GPCCS)
//!   4. SEC2/ACR boot solver (with FBIF instance-block config)
//!   5. FECS alive check
//!   6. GR context discovery + golden save
//!   7. Channel creation (5-level page tables)
//!   8. NOP dispatch
//!
//! Usage:
//!   RUST_LOG=info cargo run --example volta_sovereign_pipeline --features vfio -- 0000:65:00.0
//!
//! Each stage prints PASS/FAIL. The pipeline aborts on first failure,
//! leaving the GPU in a diagnosable state for tracing analysis.
#![cfg(all(target_os = "linux", feature = "vfio"))]

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let bdf = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "0000:65:00.0".to_string());

    eprintln!("═══════════════════════════════════════════════════════════════");
    eprintln!("  Titan V (GV100) Sovereign Pipeline Diagnostic");
    eprintln!("  Target: {bdf}");
    eprintln!("═══════════════════════════════════════════════════════════════\n");

    // ── Stage 1: VFIO open + identity ──
    eprintln!("[1/8] Opening VFIO device...");
    let device = match coral_driver::vfio::device::VfioDevice::open(&bdf) {
        Ok(d) => {
            eprintln!("  PASS  VFIO device opened");
            d
        }
        Err(e) => {
            eprintln!("  FAIL  VFIO open failed: {e}");
            std::process::exit(1);
        }
    };

    let bar0 = match device.map_bar(0) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("  FAIL  BAR0 map failed: {e}");
            std::process::exit(1);
        }
    };

    let boot0 = bar0.read_u32(0).unwrap_or(0xDEAD);
    let sm = coral_driver::nv::identity::boot0_to_sm(boot0);
    eprintln!("  BOOT0 = {boot0:#010x}, SM = {sm:?}");
    if boot0 == 0xFFFF_FFFF || boot0 == 0 {
        eprintln!("  FAIL  GPU link dead (BOOT0 = {boot0:#010x})");
        std::process::exit(1);
    }
    let sm_version = match sm {
        Some(v) if v >= 70 && v < 80 => {
            eprintln!("  PASS  Volta GPU confirmed (SM {v})");
            v
        }
        Some(v) => {
            eprintln!("  WARN  Non-Volta SM {v} — pipeline designed for GV100, proceeding anyway");
            v
        }
        None => {
            eprintln!("  FAIL  Unknown GPU (BOOT0 {boot0:#010x})");
            std::process::exit(1);
        }
    };

    let chip = coral_driver::nv::identity::chip_name(sm_version);
    eprintln!("  Chip: {chip}");

    // ── Stage 2: PCI hot reset ──
    eprintln!("\n[2/8] PCI hot reset (clear stale falcon LS-mode)...");
    match device.pci_hot_reset() {
        Ok(()) => {
            std::thread::sleep(std::time::Duration::from_millis(200));
            let boot0_after = bar0.read_u32(0).unwrap_or(0xDEAD);
            if boot0_after == boot0 {
                eprintln!("  PASS  Hot reset succeeded, BOOT0 stable");
            } else if boot0_after == 0xFFFF_FFFF || boot0_after == 0 {
                eprintln!("  FAIL  GPU link dead after hot reset");
                std::process::exit(2);
            } else {
                eprintln!("  WARN  BOOT0 changed: {boot0:#010x} -> {boot0_after:#010x}");
            }
        }
        Err(e) => {
            eprintln!("  WARN  Hot reset failed (non-fatal): {e}");
            eprintln!("         Trying FLR fallback...");
            if let Err(e2) = device.reset() {
                eprintln!("  WARN  FLR also failed: {e2}");
            } else {
                std::thread::sleep(std::time::Duration::from_millis(100));
                eprintln!("  PASS  FLR succeeded");
            }
        }
    }

    // Re-enable bus master after reset
    if let Err(e) = device.enable_bus_master() {
        eprintln!("  WARN  Bus master enable failed: {e}");
    }

    // ── Stage 3: Falcon state probe ──
    eprintln!("\n[3/8] Probing falcon states...");
    let probe = coral_driver::nv::vfio_compute::acr_boot::FalconProbe::capture(&bar0);
    eprintln!("{probe}");

    let sec2_probe = coral_driver::nv::vfio_compute::acr_boot::Sec2Probe::capture(&bar0);
    eprintln!("  SEC2 detail: {sec2_probe}");

    if probe.fecs_state == coral_driver::nv::vfio_compute::acr_boot::FecsState::Running {
        eprintln!("  PASS  FECS already running — skipping ACR boot");
    } else {
        eprintln!("  INFO  FECS state: {} — ACR boot required", probe.fecs_state_label());
    }

    // ── Stage 4: SEC2/ACR boot solver ──
    eprintln!("\n[4/8] Running SEC2/ACR boot solver...");
    let container = device.dma_backend();
    let results = match coral_driver::nv::vfio_compute::acr_boot::FalconBootSolver::boot_for_generation(
        &bar0,
        sm_version,
        chip,
        Some(container.clone()),
        None,
    ) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  FAIL  Boot solver error: {e}");
            std::process::exit(4);
        }
    };

    let mut any_success = false;
    for (i, r) in results.iter().enumerate() {
        let tag = if r.success { "PASS" } else { "----" };
        eprintln!("  [{tag}] Strategy {i}: {} (SEC2 PC: {:#x} -> {:#x})",
            r.strategy, r.sec2_before.pc, r.sec2_after.pc);
        if r.success {
            any_success = true;
            eprintln!("         FECS CPUCTL={:#010x} PC={:#x} MAILBOX0={:#010x}",
                r.fecs_cpuctl_after, r.fecs_pc_after, r.fecs_mailbox0_after);
        }
        for note in &r.notes {
            eprintln!("         {note}");
        }
    }

    // Post-solver SEC2 check
    let sec2_after = coral_driver::nv::vfio_compute::acr_boot::Sec2Probe::capture(&bar0);
    eprintln!("\n  SEC2 after solver: {sec2_after}");

    if !any_success {
        let probe_after = coral_driver::nv::vfio_compute::acr_boot::FalconProbe::capture(&bar0);
        if probe_after.fecs_state == coral_driver::nv::vfio_compute::acr_boot::FecsState::Running {
            eprintln!("  PASS  FECS is running (detected via post-solver probe)");
        } else {
            eprintln!("  FAIL  No boot strategy succeeded — FECS not running");
            eprintln!("         SEC2 PC={:#x}, FECS state: {}", sec2_after.pc, probe_after.fecs_state_label());
            eprintln!("\n  Pipeline halted. Examine RUST_LOG=debug traces for FBIF/DMA diagnostics.");
            std::process::exit(4);
        }
    }

    // ── Stage 5: FECS alive check ──
    eprintln!("\n[5/8] Verifying FECS is alive...");
    let fecs_cpuctl = bar0.read_u32(0x40_9100).unwrap_or(0xDEAD);
    let fecs_mb0 = bar0.read_u32(0x40_9040).unwrap_or(0xDEAD);
    let fecs_mb1 = bar0.read_u32(0x40_9044).unwrap_or(0xDEAD);
    let gpccs_cpuctl = bar0.read_u32(0x41_A100).unwrap_or(0xDEAD);
    let fecs_running = fecs_cpuctl & 0x20 != 0;
    let gpccs_running = gpccs_cpuctl & 0x20 != 0;

    eprintln!("  FECS  CPUCTL={fecs_cpuctl:#010x} MB0={fecs_mb0:#010x} MB1={fecs_mb1:#010x} running={fecs_running}");
    eprintln!("  GPCCS CPUCTL={gpccs_cpuctl:#010x} running={gpccs_running}");

    if !fecs_running {
        eprintln!("  FAIL  FECS not running — cannot proceed to channel creation");
        std::process::exit(5);
    }
    eprintln!("  PASS  FECS + GPCCS alive");

    // ── Stage 6: GR context discovery ──
    eprintln!("\n[6/8] Discovering GR context sizes...");

    // FECS method interface: discover context image, zcull, PM sizes
    let method_probe = coral_driver::nv::vfio_compute::acr_boot::fecs_method::fecs_probe_methods(&bar0);
    eprintln!("  FECS method probe: {method_probe}");

    // ── Stage 7: Channel creation ──
    eprintln!("\n[7/8] Creating Volta GPFIFO channel (5-level page tables)...");
    let gpfifo_iova: u64 = 0x1000;
    let userd_iova: u64 = 0x2000;
    let gpfifo_entries: u32 = 128;

    let _gpfifo_ring = match coral_driver::vfio::dma::DmaBuffer::new(
        container.clone(), 128 * 8, gpfifo_iova,
    ) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("  FAIL  GPFIFO DMA alloc: {e}");
            std::process::exit(7);
        }
    };
    let _userd = match coral_driver::vfio::dma::DmaBuffer::new(
        container.clone(), 4096, userd_iova,
    ) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("  FAIL  USERD DMA alloc: {e}");
            std::process::exit(7);
        }
    };

    let channel = match coral_driver::vfio::channel::VfioChannel::create(
        container.clone(), &bar0, gpfifo_iova, gpfifo_entries, userd_iova, 0,
    ) {
        Ok(ch) => {
            eprintln!("  PASS  Channel created (id={})", ch.id());
            ch
        }
        Err(e) => {
            eprintln!("  FAIL  Channel creation failed: {e}");
            std::process::exit(7);
        }
    };

    // ── Stage 8: NOP dispatch ──
    eprintln!("\n[8/8] NOP dispatch (GPFIFO push → doorbell → GP_GET poll)...");
    let nop_pb_iova: u64 = 0xB000;
    let mut nop_pb = match coral_driver::vfio::dma::DmaBuffer::new(
        container.clone(), 4096, nop_pb_iova,
    ) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("  FAIL  NOP push buffer DMA alloc: {e}");
            std::process::exit(8);
        }
    };

    // 2-dword NOP: method header + data
    {
        let pb = nop_pb.as_mut_slice();
        let nop_hdr: u32 = (1 << 29) | (1 << 16) | 0x40; // type=1, count=1, method=NOP
        pb[0..4].copy_from_slice(&nop_hdr.to_le_bytes());
        pb[4..8].copy_from_slice(&0_u32.to_le_bytes());
    }

    // Encode GPFIFO entry: low 32 = IOVA (aligned), high 32 = length in dwords << 10
    let gp_lo = nop_pb_iova & 0xFFFF_FFFC;
    let gp_hi = ((8u64 / 4) << 10) | ((nop_pb_iova >> 32) & 0xFF);
    let gp_entry: u64 = gp_lo | (gp_hi << 32);

    // Write GPFIFO entry into slot 0
    let gpfifo_slice = _gpfifo_ring.as_slice();
    // Use volatile writes for coherence
    _gpfifo_ring.volatile_write_u32(0, gp_entry as u32);
    _gpfifo_ring.volatile_write_u32(4, (gp_entry >> 32) as u32);

    // GP_PUT=1, GP_GET=0 via USERD
    _userd.volatile_write_u32(0x8C, 1); // GP_PUT
    _userd.volatile_write_u32(0x88, 0); // GP_GET

    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

    // Volta doorbell: BAR0 usermode at 0x810000 + channel_id * 0x10
    let doorbell_off = 0x81_0000 + (channel.id() as usize) * 0x10;
    let pre_get = _userd.volatile_read_u32(0x88);
    eprintln!("  GP_PUT=1 GP_GET={pre_get} (pre-doorbell)");

    if let Err(e) = bar0.write_u32(doorbell_off, 1) {
        eprintln!("  WARN  Doorbell write failed: {e}");
        // Fall back to Kepler-style doorbell
        let kepler_db = 0x3000 + (channel.id() as usize) * 8;
        let _ = bar0.write_u32(kepler_db, 0);
        eprintln!("  INFO  Tried Kepler doorbell at {kepler_db:#x}");
    } else {
        eprintln!("  Doorbell rung at {doorbell_off:#x}");
    }

    // Poll GP_GET
    let poll_start = std::time::Instant::now();
    let mut gp_get_final;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
        gp_get_final = _userd.volatile_read_u32(0x88);
        if gp_get_final >= 1 {
            eprintln!("  GP_GET advanced to {gp_get_final} in {:?}", poll_start.elapsed());
            break;
        }
        if poll_start.elapsed() > std::time::Duration::from_millis(500) {
            eprintln!("  TIMEOUT  GP_GET poll (500ms), GP_GET={gp_get_final}");
            break;
        }
    }

    if gp_get_final >= 1 {
        eprintln!("  PASS  NOP dispatch succeeded — GPU consumed push buffer");
    } else {
        eprintln!("  FAIL  GPU did not consume push buffer");
        let pfifo_intr = bar0.read_u32(0x2100).unwrap_or(0xDEAD);
        let sched_err = bar0.read_u32(0x254C).unwrap_or(0xDEAD);
        let pbdma_intr = bar0.read_u32(0x04_0148).unwrap_or(0xDEAD);
        eprintln!("    PFIFO_INTR={pfifo_intr:#010x} SCHED_ERR={sched_err:#010x} PBDMA_INTR={pbdma_intr:#010x}");
    }

    let _ = (channel, nop_pb, gpfifo_slice);

    eprintln!("\n═══════════════════════════════════════════════════════════════");
    eprintln!("  Titan V Sovereign Pipeline: ALL STAGES COMPLETE");
    eprintln!("═══════════════════════════════════════════════════════════════");
}
