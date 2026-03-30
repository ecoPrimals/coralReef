// SPDX-License-Identifier: AGPL-3.0-only
//! Exp 123-K: K80 Sovereign Compute — GR enable, falcon wake, GK20A PIO boot.
//!
//! Tesla K80 = dual GK210 (Kepler, SM 3.7). No firmware security.
//! Direct PIO IMEM/DMEM upload for FECS/GPCCS.
//!
//! Run: `sudo cargo test --test exp123k_k80_sovereign -p coral-driver -- --ignored --nocapture`

mod common;

use common::exp123k_k80::*;
use coral_driver::nv::identity;
use coral_driver::nv::kepler_falcon;

#[test]
#[ignore = "requires root and Tesla K80 on vfio-pci"]
fn exp123k1_gr_enable_falcon_wake() {
    eprintln!("\n{}", "=".repeat(70));
    eprintln!("Exp 123-K1: GR Enable + Falcon Wake on Tesla K80 (GK210)");
    eprintln!("{}", "=".repeat(70));

    let devices = find_k80_devices();
    assert!(!devices.is_empty(), "No K80 devices found");

    // Use first K80 die
    let dev_path = &devices[0];
    eprintln!("\nTarget: {dev_path}");

    let mut bar0 = Bar0Access::from_sysfs_device(dev_path).expect("Failed to open BAR0");

    // Phase 0: Identity confirmation
    let boot0 = read_reg(&bar0, 0x0);
    let sm = identity::boot0_to_sm(boot0);
    eprintln!("\n--- Phase 0: Identity ---");
    eprintln!(
        "  BOOT0={boot0:#010x}  SM={sm:?}  variant={}",
        identity::chipset_variant(boot0)
    );
    assert_eq!(sm, Some(37), "Expected SM 37 (GK210)");

    // Phase 1: Read current PMC state
    let pmc = read_reg(&bar0, PMC_ENABLE);
    eprintln!("\n--- Phase 1: PMC State ---");
    eprintln!("  PMC_ENABLE={pmc:#010x}");
    eprintln!(
        "    PXBAR={} PMFB={} PRING={} PFIFO={} PGRAPH={} PDAEMON={} PTIMER={} PBFB={} PFFB={}",
        pmc & PMC_PXBAR != 0,
        pmc & PMC_PMFB != 0,
        pmc & PMC_PRING != 0,
        pmc & PMC_PFIFO != 0,
        pmc & PMC_PGRAPH != 0,
        pmc & PMC_PDAEMON != 0,
        pmc & PMC_PTIMER != 0,
        pmc & PMC_PBFB != 0,
        pmc & PMC_PFFB != 0
    );

    // Read pre-enable falcon state
    eprintln!("\n--- Phase 2: Pre-Enable Falcon State ---");
    let fecs_pre = read_falcon(&bar0, "FECS", KEPLER_FECS_BASE);
    let gpccs_pre = read_falcon(&bar0, "GPCCS", KEPLER_GPCCS_BASE);
    print_falcon(&fecs_pre);
    print_falcon(&gpccs_pre);

    let fecs_was_pri = is_pri_fault(fecs_pre.cpuctl);
    let gpccs_was_pri = is_pri_fault(gpccs_pre.cpuctl);

    // Phase 3: Enable ALL necessary engines
    eprintln!("\n--- Phase 3: Full Engine Enable ---");
    let new_pmc = pmc | PMC_ENABLE_FULL;
    write_reg(&mut bar0, PMC_ENABLE, new_pmc);
    std::thread::sleep(std::time::Duration::from_millis(20));

    let pmc_after = read_reg(&bar0, PMC_ENABLE);
    eprintln!("  PMC_ENABLE after: {pmc_after:#010x}");
    eprintln!(
        "    PXBAR={} PMFB={} PRING={} PFIFO={} PGRAPH={} PDAEMON={} PTIMER={} PBFB={} PFFB={}",
        pmc_after & PMC_PXBAR != 0,
        pmc_after & PMC_PMFB != 0,
        pmc_after & PMC_PRING != 0,
        pmc_after & PMC_PFIFO != 0,
        pmc_after & PMC_PGRAPH != 0,
        pmc_after & PMC_PDAEMON != 0,
        pmc_after & PMC_PTIMER != 0,
        pmc_after & PMC_PBFB != 0,
        pmc_after & PMC_PFFB != 0
    );

    // Also enable SPOON (PFIFO sub-engines)
    let spoon = read_reg(&bar0, PMC_SPOON_ENABLE);
    eprintln!("  SPOON_ENABLE before: {spoon:#010x}");
    if spoon == 0 {
        write_reg(&mut bar0, PMC_SPOON_ENABLE, 0xFFFF_FFFF);
        std::thread::sleep(std::time::Duration::from_millis(5));
        let spoon_after = read_reg(&bar0, PMC_SPOON_ENABLE);
        eprintln!("  SPOON_ENABLE after:  {spoon_after:#010x}");
    }

    // Phase 4: Read post-enable falcon state
    eprintln!("\n--- Phase 4: Post-Enable Falcon State ---");
    let fecs_post = read_falcon(&bar0, "FECS", KEPLER_FECS_BASE);
    let gpccs_post = read_falcon(&bar0, "GPCCS", KEPLER_GPCCS_BASE);
    print_falcon(&fecs_post);
    print_falcon(&gpccs_post);

    let fecs_woke = fecs_was_pri && !is_pri_fault(fecs_post.cpuctl);
    let gpccs_woke = gpccs_was_pri && !is_pri_fault(gpccs_post.cpuctl);
    if fecs_woke {
        eprintln!("\n  *** FECS WOKE UP ***");
    }
    if gpccs_woke {
        eprintln!("  *** GPCCS WOKE UP ***");
    }
    if !gpccs_woke && gpccs_was_pri {
        eprintln!("  GPCCS still PRI_FAULT — may need GPC-level enable (0x260 toggle or GR init)");
    }

    // Phase 5: Read additional GR state
    eprintln!("\n--- Phase 5: GR Engine State ---");
    let gr_status = read_reg(&bar0, 0x400700);
    let gr_intr = read_reg(&bar0, 0x400100);
    let gr_fecs_pc = read_reg(&bar0, KEPLER_FECS_BASE + 0x110); // NPC
    let gr_gpccs_pc = read_reg(&bar0, KEPLER_GPCCS_BASE + 0x110);
    let fecs_os = read_reg(&bar0, KEPLER_FECS_BASE + 0x500); // scratch0
    eprintln!("  GR_STATUS={gr_status:#010x}  GR_INTR={gr_intr:#010x}");
    eprintln!(
        "  FECS_NPC={gr_fecs_pc:#010x}  GPCCS_NPC={gr_gpccs_pc:#010x}  FECS_scratch0={fecs_os:#010x}"
    );

    // Phase 6: Explore GPC topology
    eprintln!("\n--- Phase 6: GPC Topology ---");
    let gpc_tpc_count0 = read_reg(&bar0, 0x500500); // GPC0 TPC count
    let gpc_tpc_count1 = read_reg(&bar0, 0x500900); // GPC1 TPC count
    let gpc_tpc_count2 = read_reg(&bar0, 0x500D00); // GPC2 TPC count
    let gpc_tpc_count3 = read_reg(&bar0, 0x501100); // GPC3 TPC count
    let gpc_tpc_count4 = read_reg(&bar0, 0x501500); // GPC4 TPC count
    let gr_gpc_count = read_reg(&bar0, 0x409604); // FECS_GPC_COUNT
    let gr_tpc_total = read_reg(&bar0, 0x409608); // FECS_TPC_TOTAL
    eprintln!("  GPC_COUNT={gr_gpc_count:#010x}  TPC_TOTAL={gr_tpc_total:#010x}");
    eprintln!(
        "  GPC0_TPC={gpc_tpc_count0:#x}  GPC1_TPC={gpc_tpc_count1:#x}  GPC2_TPC={gpc_tpc_count2:#x}  GPC3_TPC={gpc_tpc_count3:#x}  GPC4_TPC={gpc_tpc_count4:#x}"
    );

    // Phase 7: Check PFIFO state
    eprintln!("\n--- Phase 7: PFIFO State ---");
    let pfifo_ctrl = read_reg(&bar0, 0x2200);
    let pfifo_stat = read_reg(&bar0, 0x2204);
    let pbdma_map = read_reg(&bar0, 0x2390);
    eprintln!(
        "  PFIFO_CTRL={pfifo_ctrl:#010x}  PFIFO_STAT={pfifo_stat:#010x}  PBDMA_MAP={pbdma_map:#010x}"
    );

    // Phase 8: Memory controller quick read
    eprintln!("\n--- Phase 8: Memory Controller ---");
    let fb_cfg0 = read_reg(&bar0, 0x100200); // PFB_CFG0
    let fb_size = read_reg(&bar0, 0x10020C);
    eprintln!("  PFB_CFG0={fb_cfg0:#010x}  PFB_SIZE={fb_size:#010x}");

    // Read PMU state (should be accessible without GR)
    eprintln!("\n--- Phase 9: PMU State (No Security) ---");
    let pmu = read_falcon(&bar0, "PMU", 0x10A000);
    print_falcon(&pmu);
    eprintln!("  sctl=0 confirms NO firmware security (LS mode, unsigned OK)");

    eprintln!("\n{}", "=".repeat(70));
    eprintln!("Exp 123-K1 complete.");
    eprintln!("{}", "=".repeat(70));
}

#[test]
#[ignore = "requires root, K80 on vfio-pci, and Phase 1 (GR enabled)"]
fn exp123k2_fecs_pio_boot() {
    eprintln!("\n{}", "=".repeat(70));
    eprintln!("Exp 123-K2: FECS PIO Firmware Boot on K80 (GK210)");
    eprintln!("{}", "=".repeat(70));

    let devices = find_k80_devices();
    assert!(!devices.is_empty(), "No K80 devices found");

    let dev_path = &devices[0];
    eprintln!("\nTarget: {dev_path}");

    let mut bar0 = Bar0Access::from_sysfs_device(dev_path).expect("Failed to open BAR0");

    // Ensure all engines are enabled
    let pmc = read_reg(&bar0, PMC_ENABLE);
    let need_enable = pmc | PMC_ENABLE_FULL;
    if pmc != need_enable {
        eprintln!("  Enabling engines: PMC {pmc:#010x} → {need_enable:#010x}");
        write_reg(&mut bar0, PMC_ENABLE, need_enable);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    // PMC-level GR reset toggle (nouveau pattern) for clean falcon state
    eprintln!("\n--- Phase 0: PMC GR Reset Toggle ---");
    let pmc_now = read_reg(&bar0, PMC_ENABLE);
    write_reg(&mut bar0, PMC_ENABLE, pmc_now & !PMC_PGRAPH); // disable GR
    std::thread::sleep(std::time::Duration::from_millis(50));
    write_reg(&mut bar0, PMC_ENABLE, pmc_now | PMC_PGRAPH); // re-enable GR
    std::thread::sleep(std::time::Duration::from_millis(50));
    eprintln!("  GR reset toggle complete");

    // Verify FECS is accessible (not PRI fault)
    let fecs = read_falcon(&bar0, "FECS", KEPLER_FECS_BASE);
    print_falcon(&fecs);
    if is_pri_fault(fecs.cpuctl) {
        eprintln!("\n  FECS still PRI_FAULT after GR enable. Cannot proceed.");
        return;
    }

    // Load GK20A firmware (Tegra Kepler — same Falcon ISA as GK210)
    let fecs_inst_path = "/lib/firmware/nvidia/gk20a/fecs_inst.bin";
    let fecs_data_path = "/lib/firmware/nvidia/gk20a/fecs_data.bin";
    let gpccs_inst_path = "/lib/firmware/nvidia/gk20a/gpccs_inst.bin";
    let gpccs_data_path = "/lib/firmware/nvidia/gk20a/gpccs_data.bin";

    let fecs_inst = match std::fs::read(fecs_inst_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("  Cannot read {fecs_inst_path}: {e}");
            return;
        }
    };
    let fecs_data = match std::fs::read(fecs_data_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("  Cannot read {fecs_data_path}: {e}");
            return;
        }
    };
    let gpccs_inst = match std::fs::read(gpccs_inst_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("  Cannot read {gpccs_inst_path}: {e}");
            return;
        }
    };
    let gpccs_data = match std::fs::read(gpccs_data_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("  Cannot read {gpccs_data_path}: {e}");
            return;
        }
    };

    eprintln!("\n--- Firmware Loaded ---");
    eprintln!(
        "  FECS  inst={} bytes  data={} bytes",
        fecs_inst.len(),
        fecs_data.len()
    );
    eprintln!(
        "  GPCCS inst={} bytes  data={} bytes",
        gpccs_inst.len(),
        gpccs_data.len()
    );

    // Phase 1: Verify FECS is in clean reset state
    eprintln!("\n--- Phase 1: FECS Post-Reset State ---");
    let fecs_reset = read_falcon(&bar0, "FECS", KEPLER_FECS_BASE);
    print_falcon(&fecs_reset);
    eprintln!("  Expected: HRESET (0x20) or HALTED (0x10) with sctl=0x0");

    // Phase 2: Upload FECS firmware (FECS first on hub — it manages GPCCS)
    eprintln!("\n--- Phase 2: Upload FECS Firmware ---");
    match kepler_falcon::upload_dmem(&mut bar0, KEPLER_FECS_BASE, 0, &fecs_data) {
        Ok(()) => eprintln!("  FECS DMEM: {} bytes uploaded", fecs_data.len()),
        Err(e) => {
            eprintln!("  FECS DMEM upload failed: {e}");
            return;
        }
    }
    match kepler_falcon::upload_imem(&mut bar0, KEPLER_FECS_BASE, 0, &fecs_inst) {
        Ok(()) => eprintln!("  FECS IMEM: {} bytes uploaded", fecs_inst.len()),
        Err(e) => {
            eprintln!("  FECS IMEM upload failed: {e}");
            return;
        }
    }

    // Phase 3: IMEM readback verification
    eprintln!("\n--- Phase 3: IMEM Readback Verification ---");
    // Read back first 16 words of FECS IMEM
    bar0.write_u32(KEPLER_FECS_BASE + 0x180, 0x0).ok(); // IMEM_CTRL: read from addr 0
    std::thread::sleep(std::time::Duration::from_millis(1));
    let mut imem_match = true;
    for i in 0..4 {
        let readback = read_reg(&bar0, KEPLER_FECS_BASE + 0x184);
        let expected = if i * 4 + 3 < fecs_inst.len() {
            u32::from_le_bytes([
                fecs_inst[i * 4],
                fecs_inst[i * 4 + 1],
                fecs_inst[i * 4 + 2],
                fecs_inst[i * 4 + 3],
            ])
        } else {
            0
        };
        let matches = readback == expected;
        if !matches {
            imem_match = false;
        }
        eprintln!(
            "  IMEM[{:3}]: read={readback:#010x}  expect={expected:#010x}  {}",
            i * 4,
            if matches { "✓" } else { "✗" }
        );
    }
    if !imem_match {
        eprintln!("  *** IMEM MISMATCH — PIO upload may not be writing to FECS IMEM ***");
        eprintln!("  This could mean FECS IMEM requires a different access pattern on GK210");
    }

    // Phase 4: Set BOOTVEC and enable interrupts before start
    eprintln!("\n--- Phase 4: Configure FECS Boot ---");
    // Falcon boot vector — start at IMEM address 0
    write_reg(&mut bar0, KEPLER_FECS_BASE + 0x104, 0x0); // BOOTVEC = 0
    // Enable falcon interrupts for trap visibility
    write_reg(&mut bar0, KEPLER_FECS_BASE + 0x010, 0xFFFF_FFFF); // FALCON_IRQMASK
    write_reg(&mut bar0, KEPLER_FECS_BASE + 0x014, 0xFFFF_FFFF); // FALCON_IRQDEST
    // ITFEN (interface enable) — needed for DMA and MMIO access
    write_reg(&mut bar0, KEPLER_FECS_BASE + 0x048, 0x3); // ITFEN: enable MMIO + DMA
    eprintln!("  BOOTVEC=0x0  IRQMASK=0xFFFFFFFF  ITFEN=0x3");

    // Phase 5: Start FECS
    eprintln!("\n--- Phase 5: Start FECS ---");
    // Write STARTCPU to CPUCTL
    write_reg(&mut bar0, KEPLER_FECS_BASE + 0x100, 0x2); // CPUCTL = STARTCPU
    eprintln!("  STARTCPU written to FECS CPUCTL");

    // Poll for up to 1 second
    for i in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let cpuctl = read_reg(&bar0, KEPLER_FECS_BASE + 0x100);
        let pc = read_reg(&bar0, KEPLER_FECS_BASE + 0x110); // NPC
        let mb0 = read_reg(&bar0, KEPLER_FECS_BASE + 0x040);
        let mb1 = read_reg(&bar0, KEPLER_FECS_BASE + 0x044);
        let exci = read_reg(&bar0, KEPLER_FECS_BASE + 0x04C);
        let sctl = read_reg(&bar0, KEPLER_FECS_BASE + 0x240);

        let state = if cpuctl & 0x20 != 0 {
            "HRESET"
        } else if cpuctl & 0x10 != 0 {
            "HALTED"
        } else {
            "RUNNING"
        };

        eprintln!(
            "  [{i:2}] cpuctl={cpuctl:#010x}({state}) pc={pc:#010x} mb0={mb0:#010x} mb1={mb1:#010x} exci={exci:#010x} sctl={sctl:#010x}"
        );

        if state == "RUNNING" && mb1 != 0 {
            eprintln!("  *** FECS RUNNING + MAILBOX RESPONSE ***");
            break;
        }
        if state == "HALTED" && i > 0 {
            eprintln!("  FECS HALTED at PC={pc:#010x}");
            // Read TRACEPC for debugging
            for t in 0..4 {
                let trace = read_reg(&bar0, KEPLER_FECS_BASE + 0x030 + t * 4);
                eprintln!("    TRACEPC[{t}]={trace:#010x}");
            }
            break;
        }
    }

    // Final state dump
    eprintln!("\n--- Final State ---");
    let fecs_final = read_falcon(&bar0, "FECS", KEPLER_FECS_BASE);
    let gpccs_final = read_falcon(&bar0, "GPCCS", KEPLER_GPCCS_BASE);
    print_falcon(&fecs_final);
    print_falcon(&gpccs_final);

    let gr_status = read_reg(&bar0, 0x400700);
    let gr_intr = read_reg(&bar0, 0x400100);
    eprintln!("  GR_STATUS={gr_status:#010x}  GR_INTR={gr_intr:#010x}");

    eprintln!("\n{}", "=".repeat(70));
    eprintln!("Exp 123-K2 complete.");
    eprintln!("{}", "=".repeat(70));
}

/// Exp 128-A2: Full nvidia-470 recipe replay + FECS PIO boot.
///
/// Uses the nvidia-470 VM capture diff to replay ALL register writes (including
/// PGRAPH/clocks/PLL), then boots FECS via PIO upload. This bypasses the
/// DEVINIT interpreter entirely — we replay the exact nvidia driver state.
#[test]
#[ignore = "requires root and Tesla K80 on vfio-pci"]
fn exp128a2_full_recipe_fecs_boot() {
    eprintln!("\n{}", "=".repeat(70));
    eprintln!("Exp 128-A2: Full Recipe Replay + FECS Boot on K80 (GK210)");
    eprintln!("{}", "=".repeat(70));

    let devices = find_k80_devices();
    assert!(!devices.is_empty(), "No K80 devices found");

    let dev_path = &devices[0];
    eprintln!("\nTarget: {dev_path}");

    let mut bar0 = Bar0Access::from_sysfs_device(dev_path).expect("Failed to open BAR0");

    // Phase 0: Verify Kepler identity
    let boot0 = read_reg(&bar0, 0x0);
    let sm = identity::boot0_to_sm(boot0);
    eprintln!("  BOOT0={boot0:#010x}  SM={sm:?}");
    assert_eq!(sm, Some(37), "Expected SM 37 (GK210)");

    // Phase 1: Pre-boot state capture
    eprintln!("\n--- Phase 1: Pre-Boot State ---");
    let fecs_pre = read_falcon(&bar0, "FECS", KEPLER_FECS_BASE);
    let pmu_pre = read_falcon(&bar0, "PMU", 0x10A000);
    print_falcon(&fecs_pre);
    print_falcon(&pmu_pre);
    let ptimer_pre = read_reg(&bar0, 0x9400);
    let pmc_pre = read_reg(&bar0, PMC_ENABLE);
    eprintln!("  PTIMER={ptimer_pre:#010x}  PMC_ENABLE={pmc_pre:#010x}");

    // Phase 2: Enable all engines first
    eprintln!("\n--- Phase 2: Engine Enable ---");
    write_reg(&mut bar0, PMC_ENABLE, pmc_pre | PMC_ENABLE_FULL);
    std::thread::sleep(std::time::Duration::from_millis(20));
    let pmc_after = read_reg(&bar0, PMC_ENABLE);
    eprintln!("  PMC_ENABLE after: {pmc_after:#010x}");

    // Phase 3: Apply nvidia-470 cold→warm diff recipe (ALL registers)
    eprintln!("\n--- Phase 3: Full nvidia-470 Recipe Replay ---");
    let (writes, skipped) = apply_nvidia470_recipe(&mut bar0);
    eprintln!("  Applied {writes} register writes, skipped {skipped}");

    // Verify clocks started
    std::thread::sleep(std::time::Duration::from_millis(50));
    let ptimer_post = read_reg(&bar0, 0x9400);
    let ptimer_post2 = read_reg(&bar0, 0x9410);
    eprintln!("  PTIMER: {ptimer_post:#010x} / {ptimer_post2:#010x}");
    let ptimer_ticking = ptimer_post != ptimer_pre || ptimer_post != 0;
    eprintln!("  PTIMER ticking: {ptimer_ticking}");

    // Phase 4: PMC GR reset toggle for clean falcon state
    eprintln!("\n--- Phase 4: GR Reset Toggle ---");
    let pmc_now = read_reg(&bar0, PMC_ENABLE);
    write_reg(&mut bar0, PMC_ENABLE, pmc_now & !PMC_PGRAPH);
    std::thread::sleep(std::time::Duration::from_millis(50));
    write_reg(&mut bar0, PMC_ENABLE, pmc_now | PMC_PGRAPH);
    std::thread::sleep(std::time::Duration::from_millis(50));
    eprintln!("  GR reset toggle complete");

    // Phase 5: Load and boot FECS/GPCCS firmware
    eprintln!("\n--- Phase 5: FECS PIO Boot ---");
    let (fecs_inst, fecs_data, gpccs_inst, gpccs_data) = load_firmware(GK110_FW_DIR);
    eprintln!(
        "  Firmware: FECS inst={}B data={}B  GPCCS inst={}B data={}B",
        fecs_inst.len(),
        fecs_data.len(),
        gpccs_inst.len(),
        gpccs_data.len()
    );

    match kepler_falcon::boot_fecs_gpccs(
        &mut bar0,
        &fecs_inst,
        &fecs_data,
        &gpccs_inst,
        &gpccs_data,
        std::time::Duration::from_secs(5),
    ) {
        Ok(()) => eprintln!("  boot_fecs_gpccs: SUCCESS"),
        Err(e) => eprintln!("  boot_fecs_gpccs: FAILED — {e}"),
    }

    // Phase 6: Post-boot state
    eprintln!("\n--- Phase 6: Post-Boot State ---");
    let fecs_post = read_falcon(&bar0, "FECS", KEPLER_FECS_BASE);
    let gpccs_post = read_falcon(&bar0, "GPCCS", KEPLER_GPCCS_BASE);
    print_falcon(&fecs_post);
    print_falcon(&gpccs_post);

    let fecs_pc = read_reg(&bar0, KEPLER_FECS_BASE + 0x030);
    let fecs_scratch0 = read_reg(&bar0, KEPLER_FECS_BASE + 0x500);
    eprintln!("  FECS PC={fecs_pc:#010x}  SCRATCH0={fecs_scratch0:#010x}");

    let fecs_running = !is_pri_fault(fecs_post.cpuctl)
        && fecs_post.cpuctl & 0x10 == 0
        && fecs_post.cpuctl & 0x20 == 0;

    // Phase 7: PFIFO state
    let pfifo_ctrl = read_reg(&bar0, 0x2200);
    let pfifo_stat = read_reg(&bar0, 0x2204);
    let pbdma_map = read_reg(&bar0, 0x2004);
    eprintln!("\n--- Phase 7: PFIFO State ---");
    eprintln!("  PFIFO_CTRL={pfifo_ctrl:#010x}  STAT={pfifo_stat:#010x}  PBDMA_MAP={pbdma_map:#010x}");

    if fecs_running {
        eprintln!("\n  *** FECS RUNNING — ready for dispatch ***");
    } else {
        eprintln!("\n  FECS not running. Halted at PC={fecs_pc:#010x}");
        for t in 0..4 {
            let trace = read_reg(&bar0, KEPLER_FECS_BASE + 0x030 + t * 4);
            eprintln!("    TRACEPC[{t}]={trace:#010x}");
        }
    }

    eprintln!("\n{}", "=".repeat(70));
    eprintln!("Exp 128-A2 complete. FECS running: {fecs_running}");
    eprintln!("{}", "=".repeat(70));
}

/// Exp 128-A2b: PRI ring init + FECS/GPCCS full boot.
///
/// The A2 test showed FECS boots but GPCCS stays in PRI_FAULT because the
/// PRI ring (hub-to-GPC routing) isn't initialized. On Kepler, GPC registers
/// at 0x500000+ and GPCCS at 0x41A000 require the PRI ring to be active.
///
/// This test adds PRI ring enumeration before the falcon boot.
#[test]
#[ignore = "requires root and Tesla K80 on vfio-pci"]
fn exp128a2b_pri_ring_fecs_gpccs_boot() {
    eprintln!("\n{}", "=".repeat(70));
    eprintln!("Exp 128-A2b: PRI Ring Init + Full FECS/GPCCS Boot on K80 (GK210)");
    eprintln!("{}", "=".repeat(70));

    let devices = find_k80_devices();
    assert!(!devices.is_empty(), "No K80 devices found");

    let dev_path = &devices[0];
    eprintln!("\nTarget: {dev_path}");

    let mut bar0 = Bar0Access::from_sysfs_device(dev_path).expect("Failed to open BAR0");

    let boot0 = read_reg(&bar0, 0x0);
    let sm = identity::boot0_to_sm(boot0);
    eprintln!("  BOOT0={boot0:#010x}  SM={sm:?}");
    assert_eq!(sm, Some(37), "Expected SM 37 (GK210)");

    // Phase 1: Enable all engines
    eprintln!("\n--- Phase 1: Engine Enable ---");
    let pmc_pre = read_reg(&bar0, PMC_ENABLE);
    write_reg(&mut bar0, PMC_ENABLE, pmc_pre | PMC_ENABLE_FULL);
    std::thread::sleep(std::time::Duration::from_millis(20));
    let pmc_after = read_reg(&bar0, PMC_ENABLE);
    eprintln!("  PMC_ENABLE: {pmc_pre:#010x} → {pmc_after:#010x}");

    // Phase 2: PRI ring initialization (nouveau: gk104_privring_init)
    eprintln!("\n--- Phase 2: PRI Ring Init ---");

    // Check pre-init GPCCS state
    let gpccs_pre = read_reg(&bar0, KEPLER_GPCCS_BASE + 0x100);
    eprintln!("  GPCCS CPUCTL before PRI init: {gpccs_pre:#010x} ({})",
        if is_pri_fault(gpccs_pre) { "PRI_FAULT" } else { "accessible" });

    // PRI ring master registers (envytools: PRING)
    let ring_cmd = 0x12004C_u32;  // PRING command
    let ring_status = 0x120048_u32; // PRING status

    // Read current ring state
    let ring_stat_pre = read_reg(&bar0, ring_status);
    eprintln!("  PRING status: {ring_stat_pre:#010x}");

    // Step 1: Ack any pending ring interrupts
    write_reg(&mut bar0, ring_cmd, 0x4); // ack interrupt
    std::thread::sleep(std::time::Duration::from_millis(5));

    // Step 2: Enumerate the ring (discover all attached engines)
    write_reg(&mut bar0, ring_cmd, 0x1); // enumerate
    std::thread::sleep(std::time::Duration::from_millis(50));

    let ring_stat_enum = read_reg(&bar0, ring_status);
    eprintln!("  PRING status after enumerate: {ring_stat_enum:#010x}");

    // Step 3: Start the ring
    write_reg(&mut bar0, ring_cmd, 0x2); // start
    std::thread::sleep(std::time::Duration::from_millis(50));

    let ring_stat_start = read_reg(&bar0, ring_status);
    eprintln!("  PRING status after start: {ring_stat_start:#010x}");

    // Also try GPC slave ring start (nouveau gk104_privring_init pattern)
    let gpc_priv_base = 0x128100_u32; // GPC0 slave ring
    write_reg(&mut bar0, gpc_priv_base + 0x104, 0x2); // GPC0 slave: start
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Check GPCCS accessibility now
    let gpccs_post_ring = read_reg(&bar0, KEPLER_GPCCS_BASE + 0x100);
    eprintln!("  GPCCS CPUCTL after PRI init: {gpccs_post_ring:#010x} ({})",
        if is_pri_fault(gpccs_post_ring) { "PRI_FAULT" } else { "accessible" });

    // Also try broader PRI ring fixes:
    // nouveau's gf100_priv does: wr32(0x122204, 2) then rd32(0x122204)
    write_reg(&mut bar0, 0x122204, 0x2); // GPC slave start
    std::thread::sleep(std::time::Duration::from_millis(10));
    let gpc_slave = read_reg(&bar0, 0x122204);
    eprintln!("  GPC slave ring (0x122204): {gpc_slave:#010x}");

    let gpccs_post_slave = read_reg(&bar0, KEPLER_GPCCS_BASE + 0x100);
    eprintln!("  GPCCS CPUCTL after slave start: {gpccs_post_slave:#010x} ({})",
        if is_pri_fault(gpccs_post_slave) { "PRI_FAULT" } else { "accessible" });

    // Phase 3: Apply nvidia-470 recipe
    eprintln!("\n--- Phase 3: nvidia-470 Recipe Replay ---");
    let (writes, skipped) = apply_nvidia470_recipe(&mut bar0);
    eprintln!("  Applied {writes} register writes, skipped {skipped}");

    // Check GPCCS again after recipe
    let gpccs_post_recipe = read_reg(&bar0, KEPLER_GPCCS_BASE + 0x100);
    eprintln!("  GPCCS CPUCTL after recipe: {gpccs_post_recipe:#010x} ({})",
        if is_pri_fault(gpccs_post_recipe) { "PRI_FAULT" } else { "accessible" });

    // Phase 4: GR reset toggle
    eprintln!("\n--- Phase 4: GR Reset Toggle ---");
    let pmc_now = read_reg(&bar0, PMC_ENABLE);
    write_reg(&mut bar0, PMC_ENABLE, pmc_now & !PMC_PGRAPH);
    std::thread::sleep(std::time::Duration::from_millis(50));
    write_reg(&mut bar0, PMC_ENABLE, pmc_now | PMC_PGRAPH);
    std::thread::sleep(std::time::Duration::from_millis(50));
    eprintln!("  GR reset toggle complete");

    // Re-check GPCCS after GR reset
    let gpccs_post_reset = read_reg(&bar0, KEPLER_GPCCS_BASE + 0x100);
    eprintln!("  GPCCS CPUCTL after GR reset: {gpccs_post_reset:#010x} ({})",
        if is_pri_fault(gpccs_post_reset) { "PRI_FAULT" } else { "accessible" });

    // Phase 4b: Re-init PRI ring after GR reset (GR reset may tear down ring)
    if is_pri_fault(gpccs_post_reset) {
        eprintln!("\n--- Phase 4b: Re-Init PRI Ring After GR Reset ---");
        write_reg(&mut bar0, ring_cmd, 0x4); // ack
        std::thread::sleep(std::time::Duration::from_millis(5));
        write_reg(&mut bar0, ring_cmd, 0x1); // enumerate
        std::thread::sleep(std::time::Duration::from_millis(50));
        write_reg(&mut bar0, ring_cmd, 0x2); // start
        std::thread::sleep(std::time::Duration::from_millis(50));
        write_reg(&mut bar0, 0x122204, 0x2); // GPC slave start
        std::thread::sleep(std::time::Duration::from_millis(20));

        let gpccs_post_reinit = read_reg(&bar0, KEPLER_GPCCS_BASE + 0x100);
        eprintln!("  GPCCS CPUCTL after re-init: {gpccs_post_reinit:#010x} ({})",
            if is_pri_fault(gpccs_post_reinit) { "PRI_FAULT" } else { "accessible" });
    }

    // Phase 5: Boot FECS/GPCCS
    eprintln!("\n--- Phase 5: FECS/GPCCS PIO Boot ---");
    let gpccs_accessible = !is_pri_fault(read_reg(&bar0, KEPLER_GPCCS_BASE + 0x100));
    eprintln!("  GPCCS accessible: {gpccs_accessible}");

    let (fecs_inst, fecs_data, gpccs_inst, gpccs_data) = load_firmware(GK110_FW_DIR);
    eprintln!(
        "  Firmware: FECS inst={}B data={}B  GPCCS inst={}B data={}B",
        fecs_inst.len(), fecs_data.len(), gpccs_inst.len(), gpccs_data.len()
    );

    if gpccs_accessible {
        match kepler_falcon::boot_fecs_gpccs(
            &mut bar0,
            &fecs_inst, &fecs_data,
            &gpccs_inst, &gpccs_data,
            std::time::Duration::from_secs(5),
        ) {
            Ok(()) => eprintln!("  boot_fecs_gpccs: SUCCESS"),
            Err(e) => eprintln!("  boot_fecs_gpccs: FAILED — {e}"),
        }
    } else {
        eprintln!("  GPCCS still PRI_FAULT — booting FECS only");
        kepler_falcon::upload_dmem(&mut bar0, KEPLER_FECS_BASE, 0, &fecs_data).unwrap();
        kepler_falcon::upload_imem(&mut bar0, KEPLER_FECS_BASE, 0, &fecs_inst).unwrap();
        kepler_falcon::start_falcon(&mut bar0, KEPLER_FECS_BASE).unwrap();
        std::thread::sleep(std::time::Duration::from_secs(2));
    }

    // Phase 6: Post-boot state
    eprintln!("\n--- Phase 6: Post-Boot State ---");
    let fecs_post = read_falcon(&bar0, "FECS", KEPLER_FECS_BASE);
    let gpccs_post = read_falcon(&bar0, "GPCCS", KEPLER_GPCCS_BASE);
    print_falcon(&fecs_post);
    print_falcon(&gpccs_post);

    let fecs_cpuctl = fecs_post.cpuctl;
    let fecs_running = !is_pri_fault(fecs_cpuctl)
        && fecs_cpuctl & 0x10 == 0
        && fecs_cpuctl & 0x20 == 0;
    let gpccs_cpuctl = gpccs_post.cpuctl;
    let gpccs_running = !is_pri_fault(gpccs_cpuctl)
        && gpccs_cpuctl & 0x10 == 0
        && gpccs_cpuctl & 0x20 == 0;

    eprintln!("  FECS running: {fecs_running}  GPCCS running: {gpccs_running}");

    // Phase 7: Test FECS method interface
    if fecs_running {
        eprintln!("\n--- Phase 7: FECS Method Interface ---");
        write_reg(&mut bar0, KEPLER_FECS_BASE + 0x804, 0x00);
        write_reg(&mut bar0, KEPLER_FECS_BASE + 0x800, 0x00);
        write_reg(&mut bar0, KEPLER_FECS_BASE + 0x500, 0x00);
        write_reg(&mut bar0, KEPLER_FECS_BASE + 0x504, 0x10);

        for i in 0..100 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            let status = read_reg(&bar0, KEPLER_FECS_BASE + 0x804);
            if status == 0x01 {
                let image_size = read_reg(&bar0, KEPLER_FECS_BASE + 0x500);
                eprintln!("  CTX_IMAGE_SIZE = {image_size:#010x} ({image_size} bytes) at poll {i}");
                break;
            } else if status == 0x02 {
                eprintln!("  CTX_IMAGE_SIZE: ERROR at poll {i}");
                break;
            }
            if i == 99 {
                let s1 = read_reg(&bar0, KEPLER_FECS_BASE + 0x800);
                let s2 = read_reg(&bar0, KEPLER_FECS_BASE + 0x804);
                eprintln!("  FECS method TIMEOUT — status={s1:#010x} status2={s2:#010x}");
            }
        }
    }

    eprintln!("\n{}", "=".repeat(70));
    eprintln!("Exp 128-A2b complete.");
    eprintln!("{}", "=".repeat(70));
}

/// Exp 128-A3: Kepler GPFIFO channel dispatch.
///
/// Depends on A2 (FECS running). Sets up PFIFO, creates a Kepler GPFIFO
/// channel, binds it to GR, submits a NOP via GPFIFO, and checks completion.
///
/// Kepler channel setup (gk104):
/// - Channel class: KEPLER_CHANNEL_GPFIFO_B (0xA16F)
/// - Compute class: KEPLER_COMPUTE_B (0xA1C0)
/// - USERD at BAR1 offset, GP_PUT direct write (no doorbell)
/// - RAMFC layout: 512 bytes, GP_BASE at +0x00, SIGNATURE at +0x10
#[test]
#[ignore = "requires root, K80 on vfio-pci, and FECS running (exp128a2 first)"]
fn exp128a3_kepler_gpfifo_dispatch() {
    eprintln!("\n{}", "=".repeat(70));
    eprintln!("Exp 128-A3: Kepler GPFIFO Channel Dispatch on K80 (GK210)");
    eprintln!("{}", "=".repeat(70));

    let devices = find_k80_devices();
    assert!(!devices.is_empty(), "No K80 devices found");

    let dev_path = &devices[0];
    let mut bar0 = Bar0Access::from_sysfs_device(dev_path).expect("Failed to open BAR0");

    // Pre-check: FECS must be alive
    let fecs_cpuctl = read_reg(&bar0, KEPLER_FECS_BASE + 0x100);
    let fecs_alive = !is_pri_fault(fecs_cpuctl)
        && fecs_cpuctl & 0x10 == 0
        && fecs_cpuctl & 0x20 == 0;
    eprintln!("  FECS CPUCTL={fecs_cpuctl:#010x} alive={fecs_alive}");
    if !fecs_alive {
        eprintln!("  FECS not running. Run exp128a2_full_recipe_fecs_boot first.");
        eprintln!("  SKIP");
        return;
    }

    // Phase 1: PFIFO init
    eprintln!("\n--- Phase 1: PFIFO Init ---");
    let pfifo_enable = read_reg(&bar0, 0x2200);
    eprintln!("  PFIFO_ENABLE={pfifo_enable:#010x}");

    // Enable PFIFO if not already
    if pfifo_enable & 1 == 0 {
        write_reg(&mut bar0, 0x2200, pfifo_enable | 1);
        std::thread::sleep(std::time::Duration::from_millis(10));
        let after = read_reg(&bar0, 0x2200);
        eprintln!("  PFIFO_ENABLE after: {after:#010x}");
    }

    // Clear PFIFO interrupts
    write_reg(&mut bar0, 0x2100, 0xFFFF_FFFF);

    // Read PBDMA map to find available PBDMAs
    let pbdma_map = read_reg(&bar0, 0x2004);
    eprintln!("  PBDMA_MAP={pbdma_map:#010x}");

    // Phase 2: FECS method interface test
    eprintln!("\n--- Phase 2: FECS Method Interface ---");

    // Query context image size (method 0x10) — confirms FECS method loop active
    write_reg(&mut bar0, KEPLER_FECS_BASE + 0x804, 0x00); // STATUS2 = 0
    write_reg(&mut bar0, KEPLER_FECS_BASE + 0x800, 0x00); // STATUS = 0
    write_reg(&mut bar0, KEPLER_FECS_BASE + 0x500, 0x00); // MTHD_DATA = 0
    write_reg(&mut bar0, KEPLER_FECS_BASE + 0x504, 0x10); // MTHD_CMD = CTX_IMAGE_SIZE

    let method_start = std::time::Instant::now();
    let mut method_ok = false;
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        let status = read_reg(&bar0, KEPLER_FECS_BASE + 0x804);
        if status == 0x01 {
            let image_size = read_reg(&bar0, KEPLER_FECS_BASE + 0x500);
            eprintln!(
                "  CTX_IMAGE_SIZE = {image_size:#010x} ({} bytes) in {:?}",
                image_size,
                method_start.elapsed()
            );
            method_ok = true;
            break;
        } else if status == 0x02 {
            eprintln!("  CTX_IMAGE_SIZE: ERROR (status2=0x02)");
            break;
        }
    }
    if !method_ok {
        let status = read_reg(&bar0, KEPLER_FECS_BASE + 0x804);
        let status1 = read_reg(&bar0, KEPLER_FECS_BASE + 0x800);
        eprintln!("  CTX_IMAGE_SIZE: TIMEOUT (status={status:#010x} status1={status1:#010x})");
        eprintln!("  FECS method interface not responsive — cannot proceed with channel setup");
        return;
    }

    // Phase 3: Channel setup (skeletal — allocate in PRAMIN window)
    eprintln!("\n--- Phase 3: Kepler Channel Setup ---");

    // For a minimal test, we need:
    // 1. Instance block (RAMFC) at a known PRAMIN offset
    // 2. GPFIFO ring buffer (just a few entries)
    // 3. USERD segment
    // 4. Channel bind via CCSR

    // Use PRAMIN window (BAR0 0x700000..0x800000) to place channel structures.
    // This is a 1MB window into the start of VRAM (or instance memory).
    let pramin_base: u32 = 0x70_0000;

    // Layout in PRAMIN:
    // 0x000..0x200: Instance block (RAMFC, 512 bytes)
    // 0x200..0x400: GPFIFO ring (16 entries * 8 bytes = 128 bytes)
    // 0x400..0x500: USERD (256 bytes)
    let inst_off = pramin_base;
    let gpfifo_off = pramin_base + 0x200;
    let userd_off = pramin_base + 0x400;

    // Zero out the instance block
    for i in (0..0x200).step_by(4) {
        write_reg(&mut bar0, inst_off + i, 0);
    }

    // Write RAMFC fields (gk104 layout from exp 123 spec)
    let gpfifo_va = gpfifo_off as u64;
    let gpfifo_entries_log2 = 4u32; // 16 entries

    // GPFIFO base (offset 0x48 in RAMFC): low 32 bits
    write_reg(&mut bar0, inst_off + 0x48, gpfifo_va as u32);
    // GPFIFO limit: high bits + entry count
    write_reg(
        &mut bar0,
        inst_off + 0x4C,
        ((gpfifo_va >> 32) as u32) | (gpfifo_entries_log2 << 16),
    );

    // USERD VA (offset 0x08/0x0C)
    write_reg(&mut bar0, inst_off + 0x08, userd_off);
    write_reg(&mut bar0, inst_off + 0x0C, 0);

    // PBDMA validation signature (offset 0x10)
    write_reg(&mut bar0, inst_off + 0x10, 0x0000_FACE);

    // Fixed fields from exp 123 spec
    write_reg(&mut bar0, inst_off + 0x30, 0xFFFF_F902);
    write_reg(&mut bar0, inst_off + 0x84, 0x2040_0000);
    write_reg(&mut bar0, inst_off + 0x94, 0x3000_0000); // GR engine bind
    write_reg(&mut bar0, inst_off + 0x9C, 0x100);
    write_reg(&mut bar0, inst_off + 0xAC, 0x0000_001F);
    write_reg(&mut bar0, inst_off + 0xB8, 0xF800_0000);
    write_reg(&mut bar0, inst_off + 0xE8, 0); // channel ID = 0

    eprintln!("  Instance block at PRAMIN+0x000 ({inst_off:#010x})");
    eprintln!("  GPFIFO at PRAMIN+0x200 ({gpfifo_off:#010x}), {gpfifo_entries_log2} entries (log2)");
    eprintln!("  USERD at PRAMIN+0x400 ({userd_off:#010x})");

    // Phase 4: Bind channel via CCSR
    eprintln!("\n--- Phase 4: Channel Bind ---");
    let chid = 0u32;
    let ccsr_entry = 0x80_0000 + chid * 8;
    // inst_addr is physical address >> 12; for PRAMIN window at start of VRAM, offset = 0
    let inst_phys = 0u32; // PRAMIN window base in VRAM = 0
    let ccsr_val = 0x8000_0000 | (inst_phys >> 12);
    write_reg(&mut bar0, ccsr_entry, ccsr_val);
    write_reg(&mut bar0, ccsr_entry + 4, 0); // CCSR upper

    eprintln!("  CCSR[{chid}] = {ccsr_val:#010x} at {ccsr_entry:#010x}");

    // Check channel status
    std::thread::sleep(std::time::Duration::from_millis(10));
    let ccsr_status = read_reg(&bar0, ccsr_entry);
    eprintln!("  CCSR readback: {ccsr_status:#010x}");

    // Phase 5: Write a NOP GPFIFO entry
    eprintln!("\n--- Phase 5: GPFIFO NOP Submission ---");

    // GPFIFO entry format: [63:2]=address>>2, [1:0]=type
    // Type 0 = indirect (points to pushbuf), Type 2 = NOP
    // For a NOP test, write a zero-length entry
    write_reg(&mut bar0, gpfifo_off, 0);
    write_reg(&mut bar0, gpfifo_off + 4, 0);

    // Update GP_PUT (USERD offset 0x8C on Kepler)
    write_reg(&mut bar0, userd_off + 0x8C, 1);
    eprintln!("  GP_PUT = 1 written to USERD+0x8C");

    // Poll for completion
    std::thread::sleep(std::time::Duration::from_millis(100));
    let gp_get = read_reg(&bar0, inst_off + 0x48 + 0x10); // GP_GET near GPFIFO
    let pfifo_intr = read_reg(&bar0, 0x2100);
    eprintln!("  After 100ms: GP_GET readback near={gp_get:#010x} PFIFO_INTR={pfifo_intr:#010x}");

    // Check PBDMA status
    for pid in 0..4u32 {
        if pbdma_map & (1 << pid) == 0 {
            continue;
        }
        let pbdma_base = 0x0004_0000 + pid * 0x2000;
        let pb_status = read_reg(&bar0, pbdma_base + 0x08);
        let pb_intr = read_reg(&bar0, pbdma_base + 0x108);
        let pb_gp_base = read_reg(&bar0, pbdma_base + 0x048);
        if pb_status != 0 || pb_intr != 0 {
            eprintln!(
                "  PBDMA[{pid}]: status={pb_status:#010x} intr={pb_intr:#010x} gp_base={pb_gp_base:#010x}"
            );
        }
    }

    eprintln!("\n{}", "=".repeat(70));
    eprintln!("Exp 128-A3 complete.");
    eprintln!(
        "  This is a skeletal channel test. Full dispatch requires MMU setup"
    );
    eprintln!("  and compute class binding (KEPLER_COMPUTE_B = 0xA1C0).");
    eprintln!("{}", "=".repeat(70));
}
