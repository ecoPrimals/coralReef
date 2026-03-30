// SPDX-License-Identifier: AGPL-3.0-only
//! Exp 123-K2b: GK110-native firmware FECS/GPCCS boot on K80.
//!
//! Run: `sudo cargo test --test exp123k_k80_sovereign_gk110_boot -p coral-driver -- --ignored --nocapture`

mod common;

use common::exp123k_k80::*;
use coral_driver::nv::identity;
use coral_driver::nv::kepler_falcon;

#[test]
#[ignore = "requires K80 on vfio-pci — GK110-native firmware boot"]
fn exp123k2b_gk110_native_fecs_boot() {
    eprintln!("\n{}", "=".repeat(70));
    eprintln!("Exp 123-K2b: FECS/GPCCS PIO Boot with GK110-native firmware");
    eprintln!("{}", "=".repeat(70));

    let devices = find_k80_devices();
    assert!(!devices.is_empty(), "No K80 devices found");

    let dev_path = &devices[0];
    eprintln!("\nTarget: {dev_path}");

    let mut bar0 = Bar0Access::from_sysfs_device(dev_path).expect("Failed to open BAR0");

    // Identity check
    let boot0 = read_reg(&bar0, 0x0);
    let sm = identity::boot0_to_sm(boot0);
    eprintln!(
        "  BOOT0={boot0:#010x}  SM={sm:?}  variant={}",
        identity::chipset_variant(boot0)
    );
    assert_eq!(sm, Some(37), "Expected SM 37 (GK210)");

    // Ensure all engines are enabled
    let pmc = read_reg(&bar0, PMC_ENABLE);
    let need = pmc | PMC_ENABLE_FULL;
    if pmc != need {
        write_reg(&mut bar0, PMC_ENABLE, need);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    // PMC GR reset toggle for clean falcon state
    eprintln!("\n--- Phase 0: PMC GR Reset + unk260 ---");
    let pmc_now = read_reg(&bar0, PMC_ENABLE);
    write_reg(&mut bar0, PMC_ENABLE, pmc_now & !PMC_PGRAPH);
    std::thread::sleep(std::time::Duration::from_millis(50));
    write_reg(&mut bar0, PMC_ENABLE, pmc_now | PMC_PGRAPH);
    std::thread::sleep(std::time::Duration::from_millis(50));

    // pmc_unk260 = 0 (nouveau does this before falcon load for clock gating)
    kepler_falcon::pmc_unk260(&mut bar0, false).expect("pmc_unk260(0)");
    eprintln!("  GR reset + unk260=0 complete");

    let fecs = read_falcon(&bar0, "FECS", KEPLER_FECS_BASE);
    print_falcon(&fecs);
    assert!(!is_pri_fault(fecs.cpuctl), "FECS PRI_FAULT after GR enable");

    // Load GK110-native firmware
    let (fecs_inst, fecs_data, gpccs_inst, gpccs_data) = load_firmware(GK110_FW_DIR);
    eprintln!("\n--- GK110-native Firmware Loaded ---");
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

    // Phase 1: Upload GPCCS first (FECS manages GPCCS, but GPCCS must be loaded first)
    eprintln!("\n--- Phase 1: Upload + Start GPCCS ---");

    // GPCCS may be in PRI_FAULT because GPC engines are not on the PRI ring yet.
    // On Kepler, GPCCS at 0x41A000 sits inside GPC0 (0x500000 region).
    // Check if GPCCS responds to PRI reads:
    let gpccs = read_falcon(&bar0, "GPCCS", KEPLER_GPCCS_BASE);
    print_falcon(&gpccs);

    if is_pri_fault(gpccs.cpuctl) {
        eprintln!("  GPCCS in PRI_FAULT — attempting GR_FECS reset sequence");
        // On GK110, after PMC GR enable, the GPC falcon base (0x41A000) goes through
        // the GR PRI hub. We need FECS alive first to route PRI to GPCs.
        // Alternative: use the broadcast GPC register at 0x41A000 may already work
        // if PGRAPH is enabled. Let's verify with a different approach: toggle the
        // GR engine specifically.

        // PGRAPH_INTR_EN → disable all GR interrupts during init
        write_reg(&mut bar0, 0x400138, 0);
        // GR SRC_CLEAR
        write_reg(&mut bar0, 0x40032C, 0xFFFF_FFFF);
        std::thread::sleep(std::time::Duration::from_millis(10));

        let gpccs2 = read_falcon(&bar0, "GPCCS", KEPLER_GPCCS_BASE);
        print_falcon(&gpccs2);
        if is_pri_fault(gpccs2.cpuctl) {
            eprintln!("  GPCCS still PRI_FAULT after SRC_CLEAR.");
            eprintln!("  Will proceed with FECS-only boot (FECS will boot GPCCS internally).");
        }
    }

    let gpccs_accessible = !is_pri_fault(read_reg(&bar0, KEPLER_GPCCS_BASE + 0x100));

    if gpccs_accessible {
        kepler_falcon::upload_dmem(&mut bar0, KEPLER_GPCCS_BASE, 0, &gpccs_data)
            .expect("GPCCS DMEM upload");
        kepler_falcon::upload_imem(&mut bar0, KEPLER_GPCCS_BASE, 0, &gpccs_inst)
            .expect("GPCCS IMEM upload");
        eprintln!(
            "  GPCCS firmware uploaded ({} + {} bytes)",
            gpccs_inst.len(),
            gpccs_data.len()
        );
    }

    // Phase 2: Upload FECS
    eprintln!("\n--- Phase 2: Upload + Start FECS ---");
    kepler_falcon::upload_dmem(&mut bar0, KEPLER_FECS_BASE, 0, &fecs_data)
        .expect("FECS DMEM upload");
    kepler_falcon::upload_imem(&mut bar0, KEPLER_FECS_BASE, 0, &fecs_inst)
        .expect("FECS IMEM upload");
    eprintln!(
        "  FECS firmware uploaded ({} + {} bytes)",
        fecs_inst.len(),
        fecs_data.len()
    );

    // IMEM readback with auto-increment to verify upload
    eprintln!("\n--- Phase 2a: IMEM Readback (auto-increment) ---");
    // Set IMEM_CTRL to read from addr 0 with auto-increment (bit 25)
    bar0.write_u32(KEPLER_FECS_BASE + kepler_falcon::FALCON_IMEM_CTRL, 1 << 25)
        .ok();
    std::thread::sleep(std::time::Duration::from_millis(1));
    let mut ok_count = 0;
    let check_words = 8.min(fecs_inst.len() / 4);
    for i in 0..check_words {
        let readback = read_reg(&bar0, KEPLER_FECS_BASE + kepler_falcon::FALCON_IMEM_DATA);
        let expected = u32::from_le_bytes([
            fecs_inst[i * 4],
            fecs_inst[i * 4 + 1],
            fecs_inst[i * 4 + 2],
            fecs_inst[i * 4 + 3],
        ]);
        let matches = readback == expected;
        if matches {
            ok_count += 1;
        }
        eprintln!(
            "  IMEM[{:3}]: read={readback:#010x}  expect={expected:#010x}  {}",
            i * 4,
            if matches { "✓" } else { "✗" }
        );
    }
    eprintln!("  {ok_count}/{check_words} words match");

    // Phase 3: Configure and start FECS
    eprintln!("\n--- Phase 3: Configure + Start FECS ---");
    write_reg(&mut bar0, KEPLER_FECS_BASE + 0x104, 0x0); // BOOTVEC = 0
    write_reg(&mut bar0, KEPLER_FECS_BASE + 0x010, 0xFFFF_FFFF); // IRQMASK
    write_reg(&mut bar0, KEPLER_FECS_BASE + 0x014, 0xFFFF_FFFF); // IRQDEST
    write_reg(&mut bar0, KEPLER_FECS_BASE + 0x048, 0x3); // ITFEN

    if gpccs_accessible {
        // Start GPCCS first
        write_reg(&mut bar0, KEPLER_GPCCS_BASE + 0x104, 0x0); // BOOTVEC
        write_reg(&mut bar0, KEPLER_GPCCS_BASE + 0x010, 0xFFFF_FFFF);
        write_reg(&mut bar0, KEPLER_GPCCS_BASE + 0x014, 0xFFFF_FFFF);
        write_reg(&mut bar0, KEPLER_GPCCS_BASE + 0x048, 0x3);
        kepler_falcon::start_falcon(&mut bar0, KEPLER_GPCCS_BASE).expect("GPCCS start");
        eprintln!("  GPCCS started");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    // pmc_unk260 = 1 (re-enable after falcon load, per nouveau)
    kepler_falcon::pmc_unk260(&mut bar0, true).expect("pmc_unk260(1)");

    kepler_falcon::start_falcon(&mut bar0, KEPLER_FECS_BASE).expect("FECS start");
    eprintln!("  FECS started, unk260=1");

    // Phase 4: Poll for boot
    eprintln!("\n--- Phase 4: Polling FECS ---");
    let mut booted = false;
    for i in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let cpuctl = read_reg(&bar0, KEPLER_FECS_BASE + 0x100);
        let pc = read_reg(&bar0, KEPLER_FECS_BASE + 0x110);
        let mb0 = read_reg(&bar0, KEPLER_FECS_BASE + 0x040);
        let mb1 = read_reg(&bar0, KEPLER_FECS_BASE + 0x044);
        let exci = read_reg(&bar0, KEPLER_FECS_BASE + 0x04C);
        let sctl = read_reg(&bar0, KEPLER_FECS_BASE + 0x240);
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
                "  [{i:2}] cpuctl={cpuctl:#010x}({state}) pc={pc:#010x} mb0={mb0:#010x} mb1={mb1:#010x} exci={exci:#010x} sctl={sctl:#010x} scratch0={scratch0:#010x}"
            );
        }

        if state == "RUNNING" && (mb1 != 0 || scratch0 != 0) {
            eprintln!(
                "  *** FECS RUNNING + RESPONSE: mb0={mb0:#x} mb1={mb1:#x} scratch0={scratch0:#x} ***"
            );
            booted = true;
            break;
        }
        if state == "HALTED" {
            eprintln!("  FECS HALTED at PC={pc:#010x}  exci={exci:#010x}");
            for t in 0..4 {
                let trace = read_reg(&bar0, KEPLER_FECS_BASE + 0x030 + t * 4);
                eprintln!("    TRACEPC[{t}]={trace:#010x}");
            }
            // Check if FECS trapped — read exception cause
            let exc_cause = read_reg(&bar0, KEPLER_FECS_BASE + 0x028);
            eprintln!("    EXCP_CAUSE={exc_cause:#010x}");
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

    if booted {
        eprintln!("\n  *** FECS BOOT SUCCESS — GK110 firmware running on GK210 ***");
    } else {
        eprintln!("\n  FECS did NOT boot. Check firmware compatibility.");
    }

    eprintln!("\n{}", "=".repeat(70));
    eprintln!("Exp 123-K2b complete.");
    eprintln!("{}", "=".repeat(70));
}
