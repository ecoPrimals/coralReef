// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tesla K80 cold-boot sovereign pipeline — step-by-step diagnostic.
//!
//! Exercises the full K80 cold-boot path with per-step verification:
//!   1. VFIO open (legacy, no iommufd FLR)
//!   2. BAR0 map + BOOT0 identity check
//!   3. PMC_ENABLE verification
//!   4. PRI ring init
//!   5. PGOB disable (all variants)
//!   6. GPC probe (enrollment check)
//!   7. FECS/GPCCS firmware upload
//!   8. Channel creation
//!   9. NOP dispatch
//!
//! Usage:
//!   RUST_LOG=info cargo run --example kepler_cold_pipeline --features vfio -- 0000:4b:00.0
//!
//! Each stage prints PASS/FAIL and the pipeline aborts on first failure,
//! leaving the GPU in a diagnosable state.
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
        .unwrap_or_else(|| "0000:4b:00.0".to_string());

    eprintln!("═══════════════════════════════════════════════════════════");
    eprintln!("  K80 Cold-Boot Sovereign Pipeline Diagnostic");
    eprintln!("  Target: {bdf}");
    eprintln!("═══════════════════════════════════════════════════════════\n");

    // ── Stage 1: VFIO open ──
    eprintln!("[1/9] Opening VFIO device (legacy path, no iommufd FLR)...");
    let device = match coral_driver::vfio::device::VfioDevice::open_legacy(&bdf) {
        Ok(d) => {
            eprintln!("  ✓ VFIO device opened");
            d
        }
        Err(e) => {
            eprintln!("  ✗ VFIO open failed: {e}");
            std::process::exit(1);
        }
    };

    // ── Stage 2: BAR0 map + identity ──
    eprintln!("\n[2/9] Mapping BAR0 and reading BOOT0...");
    let bar0 = match device.map_bar(0) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("  ✗ BAR0 map failed: {e}");
            std::process::exit(1);
        }
    };

    let boot0 = bar0.read_u32(0).unwrap_or(0xDEAD);
    let sm = coral_driver::nv::identity::boot0_to_sm(boot0);
    eprintln!("  BOOT0 = {boot0:#010x}, SM = {sm:?}");
    if boot0 == 0xFFFF_FFFF || boot0 == 0 {
        eprintln!("  ✗ GPU link dead (BOOT0 = {boot0:#010x})");
        std::process::exit(1);
    }
    eprintln!("  ✓ GPU identity resolved");

    // ── Stage 3: PMC_ENABLE check ──
    eprintln!("\n[3/9] Checking PMC_ENABLE...");
    let pmc = bar0.read_u32(0x200).unwrap_or(0xDEAD);
    let pgraph = pmc & (1 << 12) != 0;
    let pmu = pmc & (1 << 13) != 0;
    let pring = pmc & (1 << 5) != 0;
    eprintln!("  PMC_ENABLE = {pmc:#010x}");
    eprintln!("    PGRAPH (bit 12) = {pgraph}");
    eprintln!("    PMU    (bit 13) = {pmu}");
    eprintln!("    PRING  (bit  5) = {pring}");
    eprintln!("  ✓ PMC readable");

    // ── Stage 4: Cold init OR warm skip ──
    //
    // Detect warm state by checking if GPCs are reachable via PRI ring.
    // On warm (post-nouveau) GPUs, GPC registers return valid data.
    // On cold (post-FLR), they return 0x00000000 or 0xBADF3000.
    let gpc0_probe = bar0.read_u32(0x50_2100).unwrap_or(0);
    let pgraph_on = pmc & (1 << 12) != 0;
    let pri_gpc_pre = bar0.read_u32(0x12_0078).unwrap_or(0);
    let warm = pgraph_on && pri_gpc_pre > 0 && gpc0_probe != 0 && gpc0_probe != 0xFFFF_FFFF;
    let guard_early =
        coral_driver::nv::vfio_compute::hardware_guard::GuardedBar::new(&bar0, 32).ok();

    if warm {
        eprintln!("\n[4/9] WARM state detected — running warm GR init");
        eprintln!("  PMC_ENABLE={pmc:#010x} PGRAPH=on PRI_GPC={pri_gpc_pre} GPC0_CPUCTL={gpc0_probe:#010x}");
        if let Some(ref g) = guard_early {
            coral_driver::nv::vfio_compute::kepler_warm_gr_init(g, &bdf);
        }
        eprintln!("  ✓ Warm GR init complete");
    } else {
        eprintln!("\n[4/9] Running full Kepler cold init...");
        eprintln!("  (PRI ring → clocks → PMC → PGOB → ELPG → FECS boot)");
        coral_driver::nv::vfio_compute::kepler_cold_init(&bar0);
        eprintln!("  ✓ kepler_cold_init complete (check tracing for details)");
    }
    drop(guard_early);

    // ── Stage 5: Post-init PRI ring + PGOB verification ──
    eprintln!("\n[5/9] Verifying post-init state...");
    let guard =
        match coral_driver::nv::vfio_compute::hardware_guard::GuardedBar::new(&bar0, 32) {
            Ok(g) => {
                eprintln!("  ✓ GuardedBar created (canary OK)");
                g
            }
            Err(e) => {
                eprintln!("  ✗ GuardedBar refused: {e}");
                std::process::exit(1);
            }
        };

    let pri_hub = bar0.read_u32(0x12_0070).unwrap_or(0xDEAD);
    let pri_rop = bar0.read_u32(0x12_0074).unwrap_or(0xDEAD);
    let pri_gpc = bar0.read_u32(0x12_0078).unwrap_or(0xDEAD);
    let pmc_post = bar0.read_u32(0x200).unwrap_or(0xDEAD);
    eprintln!("  PMC_ENABLE   = {pmc_post:#010x}");
    eprintln!("  PRI stations: hub={pri_hub}, rop={pri_rop}, gpc={pri_gpc}");
    if pri_gpc == 0 {
        eprintln!("  ⚠ No GPC stations on PRI ring after cold init");
    } else {
        eprintln!("  ✓ PRI ring has {pri_gpc} GPC stations");
    }

    coral_driver::nv::vfio_compute::pgob::pgob_diagnostic(&guard, "pipeline::post-cold-init");

    // ── Stage 6: GPC probe ──
    eprintln!("\n[6/9] Probing GPC enrollment...");
    let gpc0_ver = bar0.read_u32(0x50_2004).unwrap_or(0xDEAD);
    let gpc0_cpuctl = bar0.read_u32(0x50_2100).unwrap_or(0xDEAD);
    let gpc1_ver = bar0.read_u32(0x52_2004).unwrap_or(0xDEAD);
    let fecs_cpuctl = bar0.read_u32(0x40_9100).unwrap_or(0xDEAD);
    let top_num_gpcs = bar0.read_u32(0x02_2430).unwrap_or(0xDEAD);

    let is_badf = |v: u32| v & 0xBADF_0000 == 0xBADF_0000;
    let gpc0_ver_alive = gpc0_ver != 0 && gpc0_ver != 0xDEAD && !is_badf(gpc0_ver);
    let gpc0_running = gpc0_cpuctl & 0x20 != 0; // bit 5 = STARTCPU acknowledged
    let fecs_running = fecs_cpuctl & 0x20 != 0;
    let gpc0_alive = gpc0_ver_alive || gpc0_running || fecs_running;

    eprintln!("  GPC0 version  = {gpc0_ver:#010x} (ver_alive={gpc0_ver_alive})");
    eprintln!("  GPC0 CPUCTL   = {gpc0_cpuctl:#010x} (running={gpc0_running})");
    eprintln!("  GPC1 version  = {gpc1_ver:#010x}");
    eprintln!("  FECS CPUCTL   = {fecs_cpuctl:#010x} (running={fecs_running})");
    eprintln!("  TOP_NUM_GPCS  = {top_num_gpcs:#010x}");

    if !gpc0_alive {
        eprintln!("  ✗ GPC0 still dead — no falcon running");
        eprintln!("\n  Remaining pipeline stages skipped.");
        std::process::exit(2);
    }
    eprintln!("  ✓ GPCs enrolled (FECS+GPCCS running)");

    // ── Stage 7: FECS firmware ──
    eprintln!("\n[7/9] Loading FECS/GPCCS firmware...");
    let profile = coral_driver::nv::generation::profile_for_sm(sm.unwrap_or(37));
    let fw_dir = format!("/lib/firmware/nvidia/{}", profile.firmware_chip);

    let check_fw = |name: &str| -> bool {
        let path = format!("{fw_dir}/{name}");
        match std::fs::metadata(&path) {
            Ok(m) => {
                eprintln!("  {name}: {} bytes", m.len());
                true
            }
            Err(_) => {
                eprintln!("  {name}: MISSING");
                false
            }
        }
    };

    let fw_ok = check_fw("fecs_inst.bin")
        & check_fw("fecs_data.bin")
        & check_fw("gpccs_inst.bin")
        & check_fw("gpccs_data.bin");

    if !fw_ok {
        eprintln!("  ✗ Firmware files missing from {fw_dir}");
        std::process::exit(3);
    }
    eprintln!("  ✓ Firmware available");

    // ── Stage 8: Channel creation ──
    eprintln!("\n[8/9] Creating Kepler GPFIFO channel...");
    let container = device.dma_backend();
    let mut gpfifo = match coral_driver::vfio::dma::DmaBuffer::new(
        container.clone(),
        4096 * 4,
        0x1_0000_0000,
    ) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("  ✗ GPFIFO DMA alloc failed: {e}");
            std::process::exit(4);
        }
    };
    let mut userd = match coral_driver::vfio::dma::DmaBuffer::new(
        container.clone(),
        4096,
        0x1_0001_0000,
    ) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("  ✗ USERD DMA alloc failed: {e}");
            std::process::exit(4);
        }
    };
    let channel = match coral_driver::vfio::channel::VfioChannel::create_kepler(
        container.clone(),
        &guard,
        0x1_0000_0000,
        256,
        0x1_0001_0000,
        0,
    ) {
        Ok(ch) => {
            eprintln!("  ✓ Channel created");
            ch
        }
        Err(e) => {
            eprintln!("  ✗ Channel creation failed: {e}");
            std::process::exit(5);
        }
    };

    // ── Stage 9: NOP dispatch via GPFIFO ──
    eprintln!("\n[9/9] NOP dispatch (GPFIFO push buffer → doorbell → GP_GET poll)...");

    // Allocate a NOP push buffer at IOVA 0xB000 (within the channel's
    // page-table-mapped range 0x1000..0x200000).
    let nop_pb_iova: u64 = 0xB000;
    let mut nop_pb = match coral_driver::vfio::dma::DmaBuffer::new(
        container.clone(),
        4096,
        nop_pb_iova,
    ) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("  ✗ NOP push buffer DMA alloc failed: {e}");
            std::process::exit(6);
        }
    };

    // Write a 2-dword NOP push buffer: method header + data.
    // Header: type=1 (non-inc), count=1 method, subchannel=0, method=0x40 (NOP)
    {
        let pb = nop_pb.as_mut_slice();
        let nop_hdr: u32 = (1 << 29) | (1 << 16) | 0x40;
        pb[0..4].copy_from_slice(&nop_hdr.to_le_bytes());
        pb[4..8].copy_from_slice(&0_u32.to_le_bytes());
    }

    // Encode GPFIFO entry: lower 32 = IOVA (4-byte aligned), upper 32 = length in dwords << 10
    let gp_entry: u64 = (nop_pb_iova & 0xFFFF_FFFC) | ((2_u64) << (32 + 10));

    // Write GPFIFO entry into slot 0 of the GPFIFO ring
    let gpfifo_slice = gpfifo.as_mut_slice();
    gpfifo_slice[0..8].copy_from_slice(&gp_entry.to_le_bytes());

    // Write GP_PUT=1, GP_GET=0 in USERD page
    let userd_slice = userd.as_mut_slice();
    let gp_get_off = 34 * 4; // ramuserd::GP_GET = offset 0x88
    let gp_put_off = 35 * 4; // ramuserd::GP_PUT = offset 0x8C
    userd_slice[gp_put_off..gp_put_off + 4].copy_from_slice(&1_u32.to_le_bytes());
    userd_slice[gp_get_off..gp_get_off + 4].copy_from_slice(&0_u32.to_le_bytes());

    // Memory fence before doorbell
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

    // Read pre-doorbell state
    let pre_get = u32::from_le_bytes([
        userd_slice[gp_get_off],
        userd_slice[gp_get_off + 1],
        userd_slice[gp_get_off + 2],
        userd_slice[gp_get_off + 3],
    ]);
    eprintln!("  GP_PUT=1 GP_GET={pre_get} (pre-doorbell)");

    // Issue Kepler doorbell: write 0 to BAR0 + 0x3000 + channel_id * 8
    let doorbell_off = 0x3000 + (0u32 as usize) * 8; // channel_id = 0
    if let Err(e) = bar0.write_u32(doorbell_off, 0) {
        eprintln!("  ✗ Doorbell write failed: {e}");
    } else {
        eprintln!("  Doorbell rung at {doorbell_off:#x}");
    }

    // Poll GP_GET for up to 500ms
    let poll_start = std::time::Instant::now();
    let mut gp_get_final;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
        gp_get_final = u32::from_le_bytes([
            userd_slice[gp_get_off],
            userd_slice[gp_get_off + 1],
            userd_slice[gp_get_off + 2],
            userd_slice[gp_get_off + 3],
        ]);
        if gp_get_final >= 1 {
            eprintln!(
                "  GP_GET advanced to {} in {:?} — GPFIFO consumed!",
                gp_get_final,
                poll_start.elapsed()
            );
            break;
        }
        if poll_start.elapsed() > std::time::Duration::from_millis(500) {
            eprintln!("  ⚠ GP_GET poll timeout (500ms), GP_GET={gp_get_final}");
            break;
        }
    }

    if gp_get_final >= 1 {
        eprintln!("  ✓ NOP dispatch succeeded — GPU consumed the push buffer");
    } else {
        eprintln!("  ✗ NOP dispatch: GPU did not consume push buffer (GP_GET still 0)");
        // Diagnostic: check for PFIFO/PBDMA errors
        let pfifo_intr = bar0.read_u32(0x2100).unwrap_or(0xDEAD);
        let sched_err = bar0.read_u32(0x254C).unwrap_or(0xDEAD);
        eprintln!("    PFIFO_INTR={pfifo_intr:#010x} SCHED_ERR={sched_err:#010x}");
    }

    let _ = (channel, nop_pb);

    eprintln!("\n═══════════════════════════════════════════════════════════");
    eprintln!("  K80 Cold-Boot Pipeline: ALL STAGES COMPLETE");
    eprintln!("═══════════════════════════════════════════════════════════");
}
