// SPDX-License-Identifier: AGPL-3.0-only

use crate::helpers::{init_tracing, open_vfio};
use std::collections::HashSet;

#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn vfio_acr_firmware_inventory() {
    init_tracing();

    eprintln!("\n=== ACR Firmware Inventory ===\n");

    match coral_driver::nv::vfio_compute::acr_boot::AcrFirmwareSet::load("gv100") {
        Ok(fw) => {
            eprintln!("{}", fw.summary());
            eprintln!("\nACR BL:\n{}", fw.acr_bl_parsed);
            eprintln!("\nACR ucode:\n{}", fw.acr_ucode_parsed);

            // Dump sec2 desc.bin header for analysis
            eprintln!("\nSEC2 desc.bin ({} bytes):", fw.sec2_desc.len());
            let hex: String = fw
                .sec2_desc
                .iter()
                .take(48)
                .enumerate()
                .map(|(i, b)| {
                    if i > 0 && i % 16 == 0 {
                        format!("\n  {b:02x}")
                    } else if i > 0 && i % 4 == 0 {
                        format!("  {b:02x}")
                    } else {
                        format!("{b:02x}")
                    }
                })
                .collect();
            eprintln!("  {hex}");
        }
        Err(e) => eprintln!("Failed to load firmware: {e}"),
    }

    eprintln!("\n=== End Firmware Inventory ===");
}

/// Read SEC2 falcon registers via SysfsBar0 — works regardless of driver.
/// Use this to capture Nouveau-warm state after a driver swap.
#[test]
#[ignore = "reads BAR0 via sysfs — run with appropriate BDF"]
fn sysfs_sec2_register_dump() {
    let bdf = std::env::var("CORALREEF_VFIO_BDF").unwrap_or_else(|_| "0000:03:00.0".to_string());

    eprintln!("\n=== SEC2 Register Dump via SysfsBar0 ({bdf}) ===\n");

    let bar0 =
        coral_driver::vfio::sysfs_bar0::SysfsBar0::open(&bdf, 0x100_0000).expect("SysfsBar0::open");

    let driver_path = format!("/sys/bus/pci/devices/{bdf}/driver");
    let driver = std::fs::read_link(&driver_path)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "none".to_string());
    eprintln!("Current driver: {driver}");

    let sec2: usize = 0x87000;
    let fecs: usize = 0x409800;
    let gpccs: usize = 0x41A800;

    for (name, base) in [("SEC2", sec2), ("FECS", fecs), ("GPCCS", gpccs)] {
        let cpuctl = bar0.read_u32(base + 0x100);
        let sctl = bar0.read_u32(base + 0x240);
        let hwcfg = bar0.read_u32(base + 0x108);
        let bootvec = bar0.read_u32(base + 0x104);
        let mb0 = bar0.read_u32(base + 0x040);
        let mb1 = bar0.read_u32(base + 0x044);
        let dmactl = bar0.read_u32(base + 0x10C);
        let tracepc = bar0.read_u32(base + 0x030);
        let exci = bar0.read_u32(base + 0x148);

        eprintln!("{name} @ {base:#08x}:");
        eprintln!("  cpuctl={cpuctl:#010x} sctl={sctl:#010x} hwcfg={hwcfg:#010x}");
        eprintln!("  bootvec={bootvec:#010x} tracepc={tracepc:#010x} exci={exci:#010x}");
        eprintln!("  mb0={mb0:#010x} mb1={mb1:#010x} dmactl={dmactl:#010x}");
        eprintln!(
            "  halted={} hreset={} hs_mode={}",
            cpuctl & 0x20 != 0,
            cpuctl & 0x10 != 0,
            sctl & 0x3000 != 0
        );

        if name == "SEC2" {
            let bind_inst = bar0.read_u32(base + 0x668);
            let fbif_624 = bar0.read_u32(base + 0x624);
            let dma_base = bar0.read_u32(base + 0x110);
            let dma_moffs = bar0.read_u32(base + 0x114);
            let dma_cmd = bar0.read_u32(base + 0x118);
            let dma_fboffs = bar0.read_u32(base + 0x11C);
            eprintln!("  0x668={bind_inst:#010x} 0x624={fbif_624:#010x}");
            eprintln!(
                "  dma_base={dma_base:#010x} dma_moffs={dma_moffs:#010x} dma_cmd={dma_cmd:#010x} dma_fboffs={dma_fboffs:#010x}"
            );

            // Also read some additional SEC2-specific registers
            for off in [0x480, 0x484, 0x488, 0x48C, 0x490, 0x494] {
                let v = bar0.read_u32(base + off);
                if v != 0 {
                    eprintln!("  +{off:#05x}={v:#010x}");
                }
            }
        }
        eprintln!();
    }

    // Check PMC
    let pmc_enable = bar0.read_u32(0x200);
    let sec2_bit = 22;
    eprintln!(
        "PMC_ENABLE={pmc_enable:#010x} SEC2_enabled={}",
        pmc_enable & (1 << sec2_bit) != 0
    );
    eprintln!("\n=== End SEC2 Register Dump ===");
}

/// Exp 089: Layer 10 — FECS method probe after falcon boot.
///
/// After Exp 088 proved FECS transitions to RUNNING, this test probes
/// the FECS method interface (0x409500/0x409504) to discover context
/// image sizes. If FECS responds, the path to golden context generation
/// and shader dispatch is open.
#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn vfio_fecs_method_probe() {
    init_tracing();
    let dev = open_vfio();

    eprintln!("\n=== Exp 089: FECS Method Probe (Layer 10) ===\n");

    // Phase 1: Boot FECS via the solver
    eprintln!("Phase 1: Running falcon boot solver...");
    let results = dev.falcon_boot_solver(None).expect("solver");
    let any_success = results.iter().any(|r| r.success);
    for (i, r) in results.iter().enumerate() {
        let tag = if r.success { "SUCCESS" } else { "FAILED" };
        eprintln!("  Strategy {}: {} — {tag}", i + 1, r.strategy);
        for note in &r.notes {
            eprintln!("    {note}");
        }
    }
    if !any_success {
        eprintln!("  No strategy succeeded — FECS not running, aborting method probe.");
        eprintln!("\n=== End Exp 089 ===");
        return;
    }

    let probe_pre = dev.falcon_probe();
    eprintln!("\nPost-boot falcon state:\n{probe_pre}");

    // Phase 2: FECS diagnostic and GR register survival check
    eprintln!("\nPhase 2: FECS execution diagnostics...");
    {
        const FECS: usize = 0x409000;
        const GPCCS: usize = 0x41A000;
        let bar0 = dev.bar0_ref();
        let r = |addr: usize| bar0.read_u32(addr).unwrap_or(0xDEAD);

        let cpuctl = r(FECS + 0x100);
        let pc = r(FECS + 0x030);
        let exci = r(FECS + 0x148);
        let dmactl = r(FECS + 0x10C);
        let sctl = r(FECS + 0x240);
        let status800 = r(0x409800);
        let status804 = r(0x409804);

        eprintln!("  cpuctl={cpuctl:#010x} pc={pc:#010x} exci={exci:#010x}");
        eprintln!("  dmactl={dmactl:#010x} sctl={sctl:#010x}");
        eprintln!("  0x409800={status800:#010x} 0x409804={status804:#010x}");

        // Key GR registers — check if boot solver corrupted them
        eprintln!("\n  Key GR register survival check:");
        eprintln!("  0x400500 (GR_ENABLE)     = {:#010x}", r(0x400500));
        eprintln!("  0x000260 (PMC_UNK260)    = {:#010x}", r(0x000260));
        eprintln!("  0x409c24 (FECS_EXCEPT)   = {:#010x}", r(0x409c24));
        eprintln!("  0x418880 (GPC_MMU_CFG)   = {:#010x}", r(0x418880));
        eprintln!("  0x4188ac (NUM_LTCS)      = {:#010x}", r(0x4188ac));
        eprintln!("  0x4188b4 (FB_MMU_WR)     = {:#010x}", r(0x4188b4));
        eprintln!("  0x4188b8 (FB_MMU_RD)     = {:#010x}", r(0x4188b8));
        eprintln!("  0x400100 (PGRAPH_INTR)   = {:#010x}", r(0x400100));
        eprintln!("  0x40013c (PGRAPH_INTR_EN)= {:#010x}", r(0x40013c));
        eprintln!("  0x40802c (SCC_INIT)      = {:#010x}", r(0x40802c));
        eprintln!("  0x408850 (ROP_ACTIVE_FBP)= {:#010x}", r(0x408850));

        // Sample FECS PC
        let mut pcs = Vec::new();
        for _ in 0..5 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            pcs.push(r(FECS + 0x030));
        }
        let unique: HashSet<_> = pcs.iter().collect();
        eprintln!(
            "\n  PC samples: {} unique — {}",
            unique.len(),
            if unique.len() > 1 {
                "EXECUTING"
            } else {
                "STUCK"
            }
        );
        eprintln!(
            "  PCs: {:?}",
            pcs.iter().map(|p| format!("{p:#06x}")).collect::<Vec<_>>()
        );
        // GPCCS full diagnostic
        let gpccs_cpuctl = r(GPCCS + 0x100);
        let gpccs_pc = r(GPCCS + 0x030);
        let gpccs_exci = r(GPCCS + 0x148);
        let gpccs_sctl = r(GPCCS + 0x240);
        let gpccs_bootvec = r(GPCCS + 0x104);
        let gpccs_dmactl = r(GPCCS + 0x10C);
        let gpccs_intr_en = r(GPCCS + 0x00c);
        let gpccs_itfen = r(GPCCS + 0x048);
        let gpccs_hwcfg = r(GPCCS + 0x008);
        eprintln!("\n  GPCCS full state:");
        eprintln!("    cpuctl={gpccs_cpuctl:#010x} pc={gpccs_pc:#010x} exci={gpccs_exci:#010x}");
        eprintln!(
            "    sctl={gpccs_sctl:#010x} bootvec={gpccs_bootvec:#010x} dmactl={gpccs_dmactl:#010x}"
        );
        eprintln!(
            "    intr_en={gpccs_intr_en:#010x} itfen={gpccs_itfen:#010x} hwcfg={gpccs_hwcfg:#010x}"
        );

        // Try reading GPCCS IMEM (first 4 words) to check if firmware loaded
        let _ = bar0.write_u32(GPCCS + 0x180, 0x0200_0000); // IMEMC: read, addr=0
        let imem0 = r(GPCCS + 0x184); // IMEMD
        let imem1 = r(GPCCS + 0x184);
        let imem2 = r(GPCCS + 0x184);
        let imem3 = r(GPCCS + 0x184);
        eprintln!("    IMEM[0..16]: {imem0:#010x} {imem1:#010x} {imem2:#010x} {imem3:#010x}");
        if imem0 == 0 && imem1 == 0 && imem2 == 0 && imem3 == 0 {
            eprintln!("    ** GPCCS IMEM appears EMPTY — firmware NOT loaded **");
        }

        // FECS SCTL and BOOTVEC for comparison
        eprintln!("\n  FECS security state:");
        eprintln!(
            "    sctl={:#010x} bootvec={:#010x}",
            r(FECS + 0x240),
            r(FECS + 0x104)
        );
    }

    // Phase 3: Re-apply dynamic GR init AFTER boot solver
    // (the solver's engine resets may have corrupted key registers)
    eprintln!("\nPhase 3: Re-applying GR init after boot solver...");
    {
        let bar0 = dev.bar0_ref();
        let r = |addr: usize| bar0.read_u32(addr).unwrap_or(0);

        // FECS/GPCCS interrupt enables — critical for firmware init
        let _ = bar0.write_u32(0x40900c, 0x0000_fc24); // FECS INTR_ENABLE
        let _ = bar0.write_u32(0x41a00c, 0x0000_fc24); // GPCCS INTR_ENABLE
        let _ = bar0.write_u32(0x409048, 0x0000_0004); // FECS ITFEN
        let _ = bar0.write_u32(0x41a048, 0x0000_0004); // GPCCS ITFEN
        // Clock-gate restore
        let _ = bar0.write_u32(0x000260, 1);
        // GR enable
        let _ = bar0.write_u32(0x400500, 0x0001_0001);
        // FECS exceptions
        let _ = bar0.write_u32(0x409c24, 0x000e_0002);
        // SCC init
        let _ = bar0.write_u32(0x40802c, 1);
        // GPC MMU
        let _ = bar0.write_u32(0x418880, r(0x100c80) & 0xf000_1fff);
        let _ = bar0.write_u32(0x418890, 0);
        let _ = bar0.write_u32(0x418894, 0);
        let _ = bar0.write_u32(0x4188b4, r(0x100cc8));
        let _ = bar0.write_u32(0x4188b8, r(0x100ccc));
        let _ = bar0.write_u32(0x4188b0, r(0x100cc4));
        // LTC / FBP
        let _ = bar0.write_u32(0x4188ac, r(0x100800));
        let _ = bar0.write_u32(0x41833c, r(0x100804));
        let fbp = r(0x12006c) & 0xf;
        let _ = bar0.write_u32(0x408850, (r(0x408850) & !0xf) | fbp);
        let _ = bar0.write_u32(0x408958, (r(0x408958) & !0xf) | fbp);
        // Interrupt enables
        let _ = bar0.write_u32(0x400100, 0xffff_ffff);
        let _ = bar0.write_u32(0x40013c, 0xffff_ffff);
        let _ = bar0.write_u32(0x400124, 0x0000_0002);
        // Trap enables
        for &addr in &[
            0x404000usize,
            0x404600,
            0x408030,
            0x406018,
            0x404490,
            0x405840,
            0x405848,
            0x407020,
        ] {
            let _ = bar0.write_u32(addr, 0xc000_0000);
        }
        let _ = bar0.write_u32(0x405844, 0x00ff_ffff);
        let _ = bar0.write_u32(0x400108, 0xffff_ffff);
        let _ = bar0.write_u32(0x400138, 0xffff_ffff);
        let _ = bar0.write_u32(0x400118, 0xffff_ffff);
        let _ = bar0.write_u32(0x400130, 0xffff_ffff);
        let _ = bar0.write_u32(0x40011c, 0xffff_ffff);
        let _ = bar0.write_u32(0x400134, 0xffff_ffff);

        // Readback verification for key registers
        eprintln!("  Readback verification:");
        eprintln!("    FECS_INTR_EN  (0x40900c) = {:#010x}", r(0x40900c));
        eprintln!("    GPCCS_INTR_EN (0x41a00c) = {:#010x}", r(0x41a00c));
        eprintln!("    FECS_ITFEN    (0x409048) = {:#010x}", r(0x409048));
        eprintln!("    GPCCS_ITFEN   (0x41a048) = {:#010x}", r(0x41a048));
        eprintln!("    GR_ENABLE     (0x400500) = {:#010x}", r(0x400500));
        eprintln!("    FECS_EXCEPT   (0x409c24) = {:#010x}", r(0x409c24));
        eprintln!("    SCC_INIT      (0x40802c) = {:#010x}", r(0x40802c));

        eprintln!("  Re-applied. Waiting 200ms for FECS to advance...");
        std::thread::sleep(std::time::Duration::from_millis(200));

        let pc = bar0.read_u32(0x409030).unwrap_or(0xDEAD);
        let status = bar0.read_u32(0x409800).unwrap_or(0xDEAD);
        let cpuctl = bar0.read_u32(0x409100).unwrap_or(0xDEAD);
        let gpccs_cpuctl = bar0.read_u32(0x41a100).unwrap_or(0xDEAD);
        let gpccs_pc = bar0.read_u32(0x41a030).unwrap_or(0xDEAD);
        eprintln!("  Post-reapply: pc={pc:#010x} status={status:#010x} cpuctl={cpuctl:#010x}");
        eprintln!("  GPCCS: cpuctl={gpccs_cpuctl:#010x} pc={gpccs_pc:#010x}");

        // Phase 3b: Try GPCCS hard-reset + re-start with explicit BOOTVEC
        let gpccs_pc_before = bar0.read_u32(0x41a030).unwrap_or(0xDEAD);
        eprintln!("  GPCCS PC before reset attempt: {gpccs_pc_before:#010x}");
        if gpccs_pc_before == 0 {
            eprintln!("  Attempting GPCCS HRESET + STARTCPU...");
            let _ = bar0.write_u32(0x41a100, 0x20); // CPUCTL = HRESET
            std::thread::sleep(std::time::Duration::from_millis(10));
            let _ = bar0.write_u32(0x41a104, 0); // BOOTVEC = 0
            let _ = bar0.write_u32(0x41a00c, 0x0000_fc24); // INTR_ENABLE
            let _ = bar0.write_u32(0x41a048, 0x0000_0004); // ITFEN
            let _ = bar0.write_u32(0x41a100, 0x02); // CPUCTL = STARTCPU
            std::thread::sleep(std::time::Duration::from_millis(50));
            let gpccs_post = bar0.read_u32(0x41a100).unwrap_or(0xDEAD);
            let gpccs_pc_after = bar0.read_u32(0x41a030).unwrap_or(0xDEAD);
            let gpccs_exci = bar0.read_u32(0x41a148).unwrap_or(0xDEAD);
            eprintln!(
                "  GPCCS after reset+start: cpuctl={gpccs_post:#010x} pc={gpccs_pc_after:#010x} exci={gpccs_exci:#010x}"
            );
        }

        // Extended PC sampling after re-apply
        let mut pcs2 = Vec::new();
        for _ in 0..10 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            pcs2.push(bar0.read_u32(0x409030).unwrap_or(0xDEAD));
        }
        let unique2: HashSet<_> = pcs2.iter().collect();
        eprintln!(
            "  Post-reapply FECS PC samples: {} unique — {}",
            unique2.len(),
            if unique2.len() > 1 {
                "EXECUTING"
            } else {
                "STILL STUCK"
            }
        );
        eprintln!(
            "  PCs: {:?}",
            pcs2.iter().map(|p| format!("{p:#06x}")).collect::<Vec<_>>()
        );
    }

    // Phase 4: Apply FECS exception configuration (again, in case cleared)
    eprintln!("\nPhase 4: FECS exception config...");
    dev.fecs_init_exceptions();

    // Phase 5: Probe FECS methods
    eprintln!("\nPhase 5: Probing FECS method interface...");
    let method_probe = dev.fecs_method_probe();
    eprintln!("\n{method_probe}");

    let all_ok = method_probe.ctx_size.is_ok()
        && method_probe.zcull_size.is_ok()
        && method_probe.pm_size.is_ok()
        && method_probe.watchdog.is_ok();

    if all_ok {
        eprintln!("****************************************************");
        eprintln!("*  FECS METHOD INTERFACE WORKING!                   *");
        eprintln!("*  Context sizes discovered — golden context next.  *");
        eprintln!("****************************************************");
    } else {
        eprintln!("FECS method interface NOT fully responding.");
        eprintln!("FECS may need GR MMIO init packs applied first.");
    }

    let probe_post = dev.falcon_probe();
    eprintln!("\nPost-method falcon state:\n{probe_post}");

    eprintln!("\n=== End Exp 089 ===");
}
