// SPDX-License-Identifier: AGPL-3.0-only
//! Exp 123-K1 / K2: GR enable + FECS PIO boot.

use coral_driver::gsp::RegisterAccess;
use coral_driver::nv::bar0::Bar0Access;
use coral_driver::nv::identity;
use coral_driver::nv::kepler_falcon;

use super::helpers::*;

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
