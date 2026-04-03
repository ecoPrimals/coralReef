// SPDX-License-Identifier: AGPL-3.0-only
//! Exp 123-K: VBIOS DEVINIT and nvidia-470 recipe tests on K80.
//!
//! Run: `cargo test --test hw_nv_vfio -p coral-driver --features vfio -- exp123k_k80_sovereign_devinit --ignored --nocapture`

use crate::exp123k_common::*;
use coral_driver::nv::identity;
use coral_driver::nv::kepler_falcon;

#[test]
#[ignore = "requires K80 on vfio-pci — VBIOS DEVINIT + falcon boot"]
fn exp123k3_devinit_then_fecs_boot() {
    eprintln!("\n{}", "=".repeat(70));
    eprintln!("Exp 123-K3: VBIOS DEVINIT + FECS/GPCCS Boot on K80 (GK210)");
    eprintln!("{}", "=".repeat(70));

    let devices = find_k80_devices();
    assert!(!devices.is_empty(), "No K80 devices found");

    let dev_path = &devices[0];
    eprintln!("\nTarget: {dev_path}");

    let mut bar0 = K80Bar0::open(dev_path);

    let boot0 = read_reg(&bar0, 0x0);
    eprintln!(
        "  BOOT0={boot0:#010x}  SM={:?}  variant={}",
        identity::boot0_to_sm(boot0),
        identity::chipset_variant(boot0)
    );

    // Phase 0: Read VBIOS from PROM
    eprintln!("\n--- Phase 0: Read VBIOS ---");
    let rom = read_vbios_prom(&mut bar0);
    eprintln!("  VBIOS: {} bytes ({} KB)", rom.len(), rom.len() / 1024);

    // Check GR status before DEVINIT
    let gr_status_before = read_reg(&bar0, 0x400700);
    let hub_pll_before = read_reg(&bar0, 0x137020);
    eprintln!(
        "\n  Pre-DEVINIT: GR_STATUS={gr_status_before:#010x}  HUB_PLL={hub_pll_before:#010x}"
    );

    // Phase 1: Execute VBIOS DEVINIT scripts
    eprintln!("\n--- Phase 1: Execute DEVINIT ---");
    let (scripts, ops, writes) = interpret_vbios_scripts(&mut bar0, &rom);
    eprintln!("  DEVINIT complete: {scripts} scripts, {ops} ops, {writes} register writes");

    // Allow settling time
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Phase 2: Check post-DEVINIT state
    eprintln!("\n--- Phase 2: Post-DEVINIT State ---");
    let gr_status = read_reg(&bar0, 0x400700);
    let gr_intr = read_reg(&bar0, 0x400100);
    let hub_pll = read_reg(&bar0, 0x137020);
    let hub_coef = read_reg(&bar0, 0x137024);
    let clk_src = read_reg(&bar0, 0x137100);
    let pmc_enable = read_reg(&bar0, PMC_ENABLE);
    eprintln!("  PMC_ENABLE={pmc_enable:#010x}");
    eprintln!("  GR_STATUS={gr_status:#010x}  GR_INTR={gr_intr:#010x}");
    eprintln!("  HUB_PLL={hub_pll:#010x}  HUB_COEF={hub_coef:#010x}  CLK_SRC={clk_src:#010x}");
    eprintln!("  GR accessible: {}", !is_pri_fault(gr_status));

    // Ensure engines are enabled
    let pmc = read_reg(&bar0, PMC_ENABLE);
    write_reg(&mut bar0, PMC_ENABLE, pmc | PMC_ENABLE_FULL);
    std::thread::sleep(std::time::Duration::from_millis(20));

    // PMC GR reset toggle
    let pmc_now = read_reg(&bar0, PMC_ENABLE);
    write_reg(&mut bar0, PMC_ENABLE, pmc_now & !PMC_PGRAPH);
    std::thread::sleep(std::time::Duration::from_millis(50));
    write_reg(&mut bar0, PMC_ENABLE, pmc_now | PMC_PGRAPH);
    std::thread::sleep(std::time::Duration::from_millis(50));

    let fecs = read_falcon(&bar0, "FECS", KEPLER_FECS_BASE);
    print_falcon(&fecs);

    if is_pri_fault(fecs.cpuctl) {
        eprintln!("  FECS still PRI_FAULT after DEVINIT + GR enable. Cannot proceed.");
        return;
    }

    // Phase 3: Load GK110 firmware and attempt boot
    let (fecs_inst, fecs_data, gpccs_inst, gpccs_data) = load_firmware(GK110_FW_DIR);
    eprintln!("\n--- Phase 3: Load + Boot FECS ---");
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

    kepler_falcon::pmc_unk260(&mut bar0, false).ok();
    kepler_falcon::upload_dmem(&mut bar0, KEPLER_FECS_BASE, 0, &fecs_data).expect("FECS DMEM");
    kepler_falcon::upload_imem(&mut bar0, KEPLER_FECS_BASE, 0, &fecs_inst).expect("FECS IMEM");

    // Check GPCCS accessibility
    let gpccs = read_falcon(&bar0, "GPCCS", KEPLER_GPCCS_BASE);
    print_falcon(&gpccs);
    let gpccs_ok = !is_pri_fault(gpccs.cpuctl);
    if gpccs_ok {
        kepler_falcon::upload_dmem(&mut bar0, KEPLER_GPCCS_BASE, 0, &gpccs_data)
            .expect("GPCCS DMEM");
        kepler_falcon::upload_imem(&mut bar0, KEPLER_GPCCS_BASE, 0, &gpccs_inst)
            .expect("GPCCS IMEM");
        eprintln!("  GPCCS firmware loaded");
        write_reg(&mut bar0, KEPLER_GPCCS_BASE + 0x104, 0x0);
        write_reg(&mut bar0, KEPLER_GPCCS_BASE + 0x010, 0xFFFF_FFFF);
        write_reg(&mut bar0, KEPLER_GPCCS_BASE + 0x014, 0xFFFF_FFFF);
        write_reg(&mut bar0, KEPLER_GPCCS_BASE + 0x048, 0x3);
        kepler_falcon::start_falcon(&mut bar0, KEPLER_GPCCS_BASE).expect("GPCCS start");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    write_reg(&mut bar0, KEPLER_FECS_BASE + 0x104, 0x0);
    write_reg(&mut bar0, KEPLER_FECS_BASE + 0x010, 0xFFFF_FFFF);
    write_reg(&mut bar0, KEPLER_FECS_BASE + 0x014, 0xFFFF_FFFF);
    write_reg(&mut bar0, KEPLER_FECS_BASE + 0x048, 0x3);
    kepler_falcon::pmc_unk260(&mut bar0, true).ok();
    kepler_falcon::start_falcon(&mut bar0, KEPLER_FECS_BASE).expect("FECS start");

    eprintln!("\n--- Phase 4: Polling FECS ---");
    let mut booted = false;
    for i in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let cpuctl = read_reg(&bar0, KEPLER_FECS_BASE + 0x100);
        let pc = read_reg(&bar0, KEPLER_FECS_BASE + 0x110);
        let mb0 = read_reg(&bar0, KEPLER_FECS_BASE + 0x040);
        let mb1 = read_reg(&bar0, KEPLER_FECS_BASE + 0x044);
        let scratch0 = read_reg(&bar0, kepler_falcon::FECS_SCRATCH0);

        let state = if cpuctl & 0x20 != 0 {
            "HRESET"
        } else if cpuctl & 0x10 != 0 {
            "HALTED"
        } else {
            "RUNNING"
        };

        if i < 5 || i % 5 == 0 || state != "RUNNING" {
            eprintln!(
                "  [{i:2}] cpuctl={cpuctl:#010x}({state}) pc={pc:#010x} mb0={mb0:#010x} mb1={mb1:#010x} scratch0={scratch0:#010x}"
            );
        }

        if state == "RUNNING" {
            if mb1 != 0 || scratch0 != 0 {
                eprintln!("  *** FECS RUNNING + RESPONSE ***");
                booted = true;
            }
            break;
        }
        if state == "HALTED" && i > 0 {
            eprintln!("  FECS HALTED at PC={pc:#010x}");
            for t in 0..4 {
                let trace = read_reg(&bar0, KEPLER_FECS_BASE + 0x030 + t * 4);
                eprintln!("    TRACEPC[{t}]={trace:#010x}");
            }
            break;
        }
    }

    eprintln!("\n--- Final State ---");
    let fecs_f = read_falcon(&bar0, "FECS", KEPLER_FECS_BASE);
    let gpccs_f = read_falcon(&bar0, "GPCCS", KEPLER_GPCCS_BASE);
    print_falcon(&fecs_f);
    print_falcon(&gpccs_f);
    eprintln!(
        "  GR_STATUS={:#010x}  GR_INTR={:#010x}",
        read_reg(&bar0, 0x400700),
        read_reg(&bar0, 0x400100)
    );

    if booted {
        eprintln!("\n  *** FECS BOOT SUCCESS — sovereign compute unlocked on K80 ***");
    }

    eprintln!("\n{}", "=".repeat(70));
    eprintln!("Exp 123-K3 complete.");
    eprintln!("{}", "=".repeat(70));
}

#[test]
#[ignore = "requires K80 on vfio-pci — DEVINIT + nvidia-470 recipe + FECS boot"]
fn exp123k4_devinit_nvidia470_recipe_fecs_boot() {
    eprintln!("\n{}", "=".repeat(70));
    eprintln!("Exp 123-K4: DEVINIT + nvidia-470 Recipe + FECS Boot");
    eprintln!("{}", "=".repeat(70));

    let devices = find_k80_devices();
    assert!(!devices.is_empty(), "No K80 devices found");

    let dev_path = &devices[0];
    eprintln!("\nTarget: {dev_path}");

    let mut bar0 = K80Bar0::open(dev_path);

    let boot0 = read_reg(&bar0, 0x0);
    eprintln!(
        "  BOOT0={boot0:#010x}  SM={:?}  variant={}",
        identity::boot0_to_sm(boot0),
        identity::chipset_variant(boot0)
    );

    // Phase 0: VBIOS DEVINIT (partial — configures enough for PRI access)
    eprintln!("\n--- Phase 0: VBIOS DEVINIT ---");
    let rom = read_vbios_prom(&mut bar0);
    eprintln!("  VBIOS: {} bytes", rom.len());
    let (scripts, ops, vbios_writes) = interpret_vbios_scripts(&mut bar0, &rom);
    eprintln!("  DEVINIT: {scripts} scripts, {ops} ops, {vbios_writes} writes");
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Ensure engines enabled after DEVINIT
    let pmc = read_reg(&bar0, PMC_ENABLE);
    write_reg(&mut bar0, PMC_ENABLE, pmc | PMC_ENABLE_FULL);
    std::thread::sleep(std::time::Duration::from_millis(20));

    // GR reset toggle to bring FECS out of PRI fault
    let pmc_now = read_reg(&bar0, PMC_ENABLE);
    write_reg(&mut bar0, PMC_ENABLE, pmc_now & !PMC_PGRAPH);
    std::thread::sleep(std::time::Duration::from_millis(50));
    write_reg(&mut bar0, PMC_ENABLE, pmc_now | PMC_PGRAPH);
    std::thread::sleep(std::time::Duration::from_millis(50));

    let fecs_post_devinit = read_falcon(&bar0, "FECS", KEPLER_FECS_BASE);
    eprintln!("\n  Post-DEVINIT + GR reset FECS:");
    print_falcon(&fecs_post_devinit);

    if is_pri_fault(fecs_post_devinit.cpuctl) {
        eprintln!("  FECS still PRI_FAULT after DEVINIT + GR reset. Cannot proceed.");
        return;
    }
    eprintln!("  *** FECS accessible after DEVINIT + GR reset! ***");

    // Phase 1: Apply nvidia-470 register recipe (after GR reset, so writes stick)
    eprintln!("\n--- Phase 1: Apply nvidia-470 Recipe ---");
    let clk_before = read_reg(&bar0, 0x137020);
    let clk_src_before = read_reg(&bar0, 0x130000);
    let (recipe_writes, recipe_skipped) = apply_nvidia470_recipe(&mut bar0);
    std::thread::sleep(std::time::Duration::from_millis(200));
    let clk_after = read_reg(&bar0, 0x137020);
    let clk_src_after = read_reg(&bar0, 0x130000);
    eprintln!("  Recipe applied: {recipe_writes} writes, {recipe_skipped} skipped");
    eprintln!("  HUB_PLL: {clk_before:#010x} → {clk_after:#010x}");
    eprintln!("  CLK_SRC: {clk_src_before:#010x} → {clk_src_after:#010x}");
    eprintln!(
        "  PMC_ENABLE={:#010x} (no extra GR reset — preserve clock writes)",
        read_reg(&bar0, PMC_ENABLE)
    );

    // Phase 3: Check post-recipe state
    eprintln!("\n--- Phase 3: Post-Recipe State ---");
    let fecs = read_falcon(&bar0, "FECS", KEPLER_FECS_BASE);
    let gpccs = read_falcon(&bar0, "GPCCS", KEPLER_GPCCS_BASE);
    let pmu = read_falcon(&bar0, "PMU", 0x10A000);
    print_falcon(&fecs);
    print_falcon(&gpccs);
    print_falcon(&pmu);

    let gr_status = read_reg(&bar0, 0x400700);
    let pfb_cfg = read_reg(&bar0, 0x100200);
    eprintln!("  GR_STATUS={gr_status:#010x}  PFB_CFG0={pfb_cfg:#010x}");
    eprintln!(
        "  FECS accessible: {}  GPCCS accessible: {}  GR accessible: {}",
        !is_pri_fault(fecs.cpuctl),
        !is_pri_fault(gpccs.cpuctl),
        !is_pri_fault(gr_status)
    );

    if is_pri_fault(fecs.cpuctl) {
        eprintln!("  FECS PRI_FAULT after recipe. Cannot boot firmware.");
        return;
    }

    // Phase 4: Load GK110-native firmware
    let (fecs_inst, fecs_data, gpccs_inst, gpccs_data) = load_firmware(GK110_FW_DIR);
    eprintln!("\n--- Phase 4: Firmware Load ---");
    eprintln!(
        "  FECS  inst={} data={}  GPCCS inst={} data={}",
        fecs_inst.len(),
        fecs_data.len(),
        gpccs_inst.len(),
        gpccs_data.len()
    );

    kepler_falcon::pmc_unk260(&mut bar0, false).ok();

    kepler_falcon::upload_dmem(&mut bar0, KEPLER_FECS_BASE, 0, &fecs_data).expect("FECS DMEM");
    kepler_falcon::upload_imem(&mut bar0, KEPLER_FECS_BASE, 0, &fecs_inst).expect("FECS IMEM");

    let gpccs_ok = !is_pri_fault(read_reg(&bar0, KEPLER_GPCCS_BASE + 0x100));
    if gpccs_ok {
        kepler_falcon::upload_dmem(&mut bar0, KEPLER_GPCCS_BASE, 0, &gpccs_data)
            .expect("GPCCS DMEM");
        kepler_falcon::upload_imem(&mut bar0, KEPLER_GPCCS_BASE, 0, &gpccs_inst)
            .expect("GPCCS IMEM");
        eprintln!("  GPCCS firmware loaded");
        write_reg(&mut bar0, KEPLER_GPCCS_BASE + 0x104, 0x0);
        write_reg(&mut bar0, KEPLER_GPCCS_BASE + 0x010, 0xFFFF_FFFF);
        write_reg(&mut bar0, KEPLER_GPCCS_BASE + 0x014, 0xFFFF_FFFF);
        write_reg(&mut bar0, KEPLER_GPCCS_BASE + 0x048, 0x3);
        kepler_falcon::start_falcon(&mut bar0, KEPLER_GPCCS_BASE).expect("GPCCS start");
        std::thread::sleep(std::time::Duration::from_millis(10));
    } else {
        eprintln!("  GPCCS PRI_FAULT — FECS-only boot");
    }

    // Phase 5: Boot FECS
    eprintln!("\n--- Phase 5: Boot FECS ---");
    write_reg(&mut bar0, KEPLER_FECS_BASE + 0x104, 0x0);
    write_reg(&mut bar0, KEPLER_FECS_BASE + 0x010, 0xFFFF_FFFF);
    write_reg(&mut bar0, KEPLER_FECS_BASE + 0x014, 0xFFFF_FFFF);
    write_reg(&mut bar0, KEPLER_FECS_BASE + 0x048, 0x3);
    kepler_falcon::pmc_unk260(&mut bar0, true).ok();
    kepler_falcon::start_falcon(&mut bar0, KEPLER_FECS_BASE).expect("FECS start");
    eprintln!("  FECS started");

    // Phase 6: Poll
    eprintln!("\n--- Phase 6: Polling FECS ---");
    let mut booted = false;
    for i in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let cpuctl = read_reg(&bar0, KEPLER_FECS_BASE + 0x100);
        let pc = read_reg(&bar0, KEPLER_FECS_BASE + 0x110);
        let mb0 = read_reg(&bar0, KEPLER_FECS_BASE + 0x040);
        let mb1 = read_reg(&bar0, KEPLER_FECS_BASE + 0x044);
        let exci = read_reg(&bar0, KEPLER_FECS_BASE + 0x04C);
        let scratch0 = read_reg(&bar0, kepler_falcon::FECS_SCRATCH0);

        let state = if cpuctl & 0x20 != 0 {
            "HRESET"
        } else if cpuctl & 0x10 != 0 {
            "HALTED"
        } else {
            "RUNNING"
        };

        if i < 10 || i % 5 == 0 || state != "RUNNING" {
            eprintln!(
                "  [{i:2}] cpuctl={cpuctl:#010x}({state}) pc={pc:#010x} mb0={mb0:#010x} mb1={mb1:#010x} exci={exci:#010x} s0={scratch0:#010x}"
            );
        }

        if state == "RUNNING" {
            if mb1 != 0 || scratch0 != 0 {
                eprintln!("  *** FECS RUNNING + RESPONSE ***");
                booted = true;
            }
            break;
        }
        if state == "HALTED" && i > 0 {
            eprintln!("  FECS HALTED at PC={pc:#010x}  exci={exci:#010x}");
            for t in 0..4 {
                let trace = read_reg(&bar0, KEPLER_FECS_BASE + 0x030 + t * 4);
                eprintln!("    TRACEPC[{t}]={trace:#010x}");
            }
            let exc_cause = read_reg(&bar0, KEPLER_FECS_BASE + 0x028);
            eprintln!("    EXCP_CAUSE={exc_cause:#010x}");
            break;
        }
    }

    // Final state
    eprintln!("\n--- Final State ---");
    let f = read_falcon(&bar0, "FECS", KEPLER_FECS_BASE);
    let g = read_falcon(&bar0, "GPCCS", KEPLER_GPCCS_BASE);
    print_falcon(&f);
    print_falcon(&g);
    let gs = read_reg(&bar0, 0x400700);
    let gi = read_reg(&bar0, 0x400100);
    eprintln!("  GR_STATUS={gs:#010x}  GR_INTR={gi:#010x}");

    if booted {
        eprintln!("\n  *** FECS BOOT SUCCESS — sovereign compute on K80 ***");
    }

    eprintln!("\n{}", "=".repeat(70));
    eprintln!("Exp 123-K4 complete.");
    eprintln!("{}", "=".repeat(70));
}
