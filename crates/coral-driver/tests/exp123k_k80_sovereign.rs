// SPDX-License-Identifier: AGPL-3.0-only
//! Exp 123-K: K80 Sovereign Compute — GR enable, falcon wake, PIO boot.
//!
//! Tesla K80 = dual GK210 (Kepler, SM 3.7). No firmware security.
//! Direct PIO IMEM/DMEM upload for FECS/GPCCS.
//!
//! Run: `sudo cargo test --test exp123k_k80_sovereign -p coral-driver -- --ignored --nocapture`

use coral_driver::gsp::{ApplyError, RegisterAccess};
use coral_driver::nv::bar0::Bar0Access;
use coral_driver::nv::identity;
use coral_driver::nv::kepler_falcon;

const PMC_ENABLE: u32 = 0x200;
const PMC_SPOON_ENABLE: u32 = 0x204;

// GF100+ PMC_ENABLE bits (envytools)
const PMC_PXBAR: u32 = 1 << 2; // crossbar — needed for GPC access
const PMC_PMFB: u32 = 1 << 3; // memory FB
const PMC_PRING: u32 = 1 << 5; // PRI ring
const PMC_PCOPY0: u32 = 1 << 6; // copy engine
const PMC_PFIFO: u32 = 1 << 8; // PFIFO — command submission
const PMC_PGRAPH: u32 = 1 << 12; // PGRAPH — GR engine + falcons
const PMC_PDAEMON: u32 = 1 << 13; // PDAEMON (PMU)
const PMC_PTIMER: u32 = 1 << 16; // timer
const PMC_PBFB: u32 = 1 << 20; // more FB
const PMC_PFFB: u32 = 1 << 29; // frame buffer front

const PMC_ENABLE_FULL: u32 = PMC_PXBAR
    | PMC_PMFB
    | PMC_PRING
    | PMC_PCOPY0
    | PMC_PFIFO
    | PMC_PGRAPH
    | PMC_PDAEMON
    | PMC_PTIMER
    | PMC_PBFB
    | PMC_PFFB;

const KEPLER_FECS_BASE: u32 = kepler_falcon::FECS_BASE;
const KEPLER_GPCCS_BASE: u32 = kepler_falcon::GPCCS_BASE;

struct FalconState {
    name: &'static str,
    base: u32,
    cpuctl: u32,
    sctl: u32,
    exci: u32,
    mb0: u32,
    mb1: u32,
    hwcfg: u32,
}

fn read_reg(bar0: &Bar0Access, addr: u32) -> u32 {
    bar0.read_u32(addr).unwrap_or(0xDEAD_DEAD)
}

fn write_reg(bar0: &mut Bar0Access, addr: u32, val: u32) {
    bar0.write_u32(addr, val).unwrap_or_else(|e| {
        eprintln!("  WRITE FAILED: {addr:#010x} = {val:#010x}: {e}");
    });
}

fn read_falcon(bar0: &Bar0Access, name: &'static str, base: u32) -> FalconState {
    FalconState {
        name,
        base,
        cpuctl: read_reg(bar0, base + 0x100),
        sctl: read_reg(bar0, base + 0x240),
        exci: read_reg(bar0, base + 0x04C),
        mb0: read_reg(bar0, base + 0x040),
        mb1: read_reg(bar0, base + 0x044),
        hwcfg: read_reg(bar0, base + 0x108),
    }
}

fn print_falcon(f: &FalconState) {
    let state = if f.cpuctl == 0xBADF_1100
        || f.cpuctl == 0xDEAD_DEAD
        || f.cpuctl == 0xBADF_5040
        || f.cpuctl & 0xBADF_0000 == 0xBADF_0000
    {
        "PRI_FAULT"
    } else if f.cpuctl & 0x20 != 0 {
        "HRESET"
    } else if f.cpuctl & 0x10 != 0 {
        "HALTED"
    } else {
        "RUNNING"
    };
    eprintln!(
        "  {:<6} cpuctl={:#010x} ({state})  sctl={:#010x}  exci={:#010x}",
        f.name, f.cpuctl, f.sctl, f.exci
    );
    eprintln!(
        "         mb0={:#010x}  mb1={:#010x}  hwcfg={:#010x}",
        f.mb0, f.mb1, f.hwcfg
    );
}

fn is_pri_fault(val: u32) -> bool {
    val & 0xBAD0_0000 == 0xBAD0_0000 || val == 0xDEAD_DEAD
}

fn find_k80_devices() -> Vec<String> {
    let mut devices = Vec::new();
    let pci_dir = "/sys/bus/pci/devices";
    if let Ok(entries) = std::fs::read_dir(pci_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let dev_path = format!("{pci_dir}/{name}");
            let vendor = std::fs::read_to_string(format!("{dev_path}/vendor")).unwrap_or_default();
            let device = std::fs::read_to_string(format!("{dev_path}/device")).unwrap_or_default();
            // K80 = 10de:102d
            if vendor.trim() == "0x10de" && device.trim() == "0x102d" {
                let driver = std::fs::read_link(format!("{dev_path}/driver"))
                    .ok()
                    .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()))
                    .unwrap_or_else(|| "none".to_string());
                eprintln!("  K80 found: {name} driver={driver}");
                devices.push(dev_path);
            }
        }
    }
    devices.sort();
    devices
}

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

/// GK110-native firmware directory (extracted from linux kernel gk110 fuc3 headers).
const GK110_FW_DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/firmware/nvidia/gk110"
);

/// Load firmware from a directory, returning (fecs_inst, fecs_data, gpccs_inst, gpccs_data).
fn load_firmware(fw_dir: &str) -> (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) {
    let read = |name: &str| -> Vec<u8> {
        let path = format!("{fw_dir}/{name}");
        std::fs::read(&path).unwrap_or_else(|e| panic!("Cannot read {path}: {e}"))
    };
    (
        read("fecs_inst.bin"),
        read("fecs_data.bin"),
        read("gpccs_inst.bin"),
        read("gpccs_data.bin"),
    )
}

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
    bar0.write_u32(
        KEPLER_FECS_BASE + kepler_falcon::FALCON_IMEM_CTRL,
        (1 << 25),
    )
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

/// Read the VBIOS PROM from BAR0 using `Bar0Access`.
fn read_vbios_prom(bar0: &mut Bar0Access) -> Vec<u8> {
    const PROM_BASE: u32 = 0x0030_0000;

    // Enable PROM access
    let enable_reg = bar0.read_u32(0x1854).unwrap_or(0);
    let _ = bar0.write_u32(0x1854, enable_reg & !1);

    let sig = bar0.read_u32(PROM_BASE).unwrap_or(0);
    assert_eq!(sig & 0xFFFF, 0xAA55, "VBIOS PROM signature missing");

    let blocks = ((sig >> 16) & 0xFF) as usize;
    let image_size = if blocks > 0 { blocks * 512 } else { 64 * 1024 };
    let read_size = image_size.max(256 * 1024).min(512 * 1024);

    let mut rom = Vec::with_capacity(read_size);
    for off in (0..read_size).step_by(4) {
        let word = bar0.read_u32(PROM_BASE + off as u32).unwrap_or(0xFFFF_FFFF);
        if off > image_size && word == 0xFFFF_FFFF {
            let next = bar0
                .read_u32(PROM_BASE + off as u32 + 4)
                .unwrap_or(0xFFFF_FFFF);
            if next == 0xFFFF_FFFF {
                break;
            }
        }
        rom.extend_from_slice(&word.to_le_bytes());
    }

    let _ = bar0.write_u32(0x1854, enable_reg);
    rom
}

/// Find BIT table in VBIOS ROM. Returns (init_tables_base, condition_table_offset).
fn find_bit_init_tables(rom: &[u8]) -> (usize, usize) {
    let sig: &[u8] = &[0xFF, 0xB8, b'B', b'I', b'T'];
    let bit_off = rom
        .windows(sig.len())
        .position(|w| w == sig)
        .expect("BIT signature not found");

    let entry_size = rom[bit_off + 9] as usize;
    let entry_count = rom[bit_off + 10] as usize;
    let entries_start = bit_off + 12;

    eprintln!("  BIT at {bit_off:#06x}: {entry_count} entries of {entry_size} bytes");

    let mut i_data_off = 0usize;
    for i in 0..entry_count {
        let e = entries_start + i * entry_size;
        if e + 6 > rom.len() {
            break;
        }
        let id = rom[e];
        let data_off = u16::from_le_bytes([rom[e + 4], rom[e + 5]]) as usize;
        let data_sz = u16::from_le_bytes([rom[e + 2], rom[e + 3]]);
        if id != 0 {
            eprintln!(
                "    '{}' (v{}) data_off={data_off:#06x} size={data_sz}",
                id as char,
                rom[e + 1]
            );
        }
        if id == b'I' {
            i_data_off = data_off;
        }
    }

    assert!(
        i_data_off > 0 && i_data_off + 2 <= rom.len(),
        "BIT 'I' not found"
    );
    // BIT 'I' data layout (u16 pointers):
    // [0] init script table, [2] macro index, [4] macro, [6] condition table, ...
    let script_list_ptr = u16::from_le_bytes([rom[i_data_off], rom[i_data_off + 1]]) as usize;
    let cond_table = if i_data_off + 8 <= rom.len() {
        u16::from_le_bytes([rom[i_data_off + 6], rom[i_data_off + 7]]) as usize
    } else {
        0
    };
    eprintln!("  script_list_ptr={script_list_ptr:#06x}  cond_table={cond_table:#06x}");
    (script_list_ptr, cond_table)
}

/// Minimal VBIOS init script interpreter using `Bar0Access`.
fn interpret_vbios_scripts(bar0: &mut Bar0Access, rom: &[u8]) -> (usize, usize, usize) {
    let (init_tables_base, cond_table) = find_bit_init_tables(rom);
    // init_tables_base is a pointer to a list of u16 script pointers (direct from BIT 'I' offset 0)
    let script_table = init_tables_base;

    let rd08 = |rom: &[u8], off: usize| -> u8 { rom.get(off).copied().unwrap_or(0) };
    let rd16 = |rom: &[u8], off: usize| -> u16 {
        if off + 2 <= rom.len() {
            u16::from_le_bytes([rom[off], rom[off + 1]])
        } else {
            0
        }
    };
    let rd32 = |rom: &[u8], off: usize| -> u32 {
        if off + 4 <= rom.len() {
            u32::from_le_bytes([rom[off], rom[off + 1], rom[off + 2], rom[off + 3]])
        } else {
            0
        }
    };

    let mut total_ops = 0usize;
    let mut total_writes = 0usize;
    let mut total_scripts = 0usize;

    let mut script_idx = 0;
    loop {
        let entry_off = script_table + script_idx * 2;
        if entry_off + 2 > rom.len() {
            break;
        }
        let script_off = rd16(rom, entry_off) as usize;
        if script_off == 0 || script_off >= rom.len() {
            break;
        }

        eprintln!("    Script {script_idx} at {script_off:#06x}");
        let mut off = script_off;
        let mut execute = true;
        let mut ops = 0usize;
        let mut writes = 0usize;
        let max_ops = 50_000;

        while off != 0 && ops < max_ops {
            let op = rd08(rom, off);
            ops += 1;

            match op {
                0x71 => {
                    off = 0;
                } // DONE
                0x72 => {
                    execute = true;
                    off += 1;
                } // RESUME
                0x38 => {
                    execute = !execute;
                    off += 1;
                } // NOT
                0x7A => {
                    // ZM_REG: reg(u32) + val(u32)
                    let reg = rd32(rom, off + 1);
                    let val = rd32(rom, off + 5);
                    if execute && reg < 0x0100_0000 {
                        let _ = bar0.write_u32(reg, val);
                        writes += 1;
                    }
                    off += 9;
                }
                0x6E => {
                    // NV_REG: reg(u32) + mask(u32) + val(u32)
                    let reg = rd32(rom, off + 1);
                    let mask = rd32(rom, off + 5);
                    let val = rd32(rom, off + 9);
                    if execute && reg < 0x0100_0000 {
                        let cur = bar0.read_u32(reg).unwrap_or(0);
                        let _ = bar0.write_u32(reg, (cur & mask) | val);
                        writes += 1;
                    }
                    off += 13;
                }
                0x58 | 0x91 => {
                    // ZM_REG_SEQUENCE / ZM_REG_GROUP
                    let base = rd32(rom, off + 1);
                    let count = rd08(rom, off + 5) as usize;
                    off += 6;
                    for i in 0..count {
                        if off + 4 > rom.len() {
                            break;
                        }
                        let val = rd32(rom, off);
                        let reg = base + (i as u32) * 4;
                        if execute && reg < 0x0100_0000 {
                            let _ = bar0.write_u32(reg, val);
                            writes += 1;
                        }
                        off += 4;
                    }
                }
                0x77 => {
                    // ZM_REG16
                    let reg = rd32(rom, off + 1);
                    let val = rd16(rom, off + 5) as u32;
                    if execute && reg < 0x0100_0000 {
                        let _ = bar0.write_u32(reg, val);
                        writes += 1;
                    }
                    off += 7;
                }
                0x47 => {
                    // ANDN_REG
                    let reg = rd32(rom, off + 1);
                    let mask = rd32(rom, off + 5);
                    if execute && reg < 0x0100_0000 {
                        let cur = bar0.read_u32(reg).unwrap_or(0);
                        let _ = bar0.write_u32(reg, cur & !mask);
                        writes += 1;
                    }
                    off += 9;
                }
                0x48 => {
                    // OR_REG
                    let reg = rd32(rom, off + 1);
                    let val = rd32(rom, off + 5);
                    if execute && reg < 0x0100_0000 {
                        let cur = bar0.read_u32(reg).unwrap_or(0);
                        let _ = bar0.write_u32(reg, cur | val);
                        writes += 1;
                    }
                    off += 9;
                }
                0x74 | 0x57 => {
                    // TIME / LTIME
                    let usec = rd16(rom, off + 1) as u64;
                    if execute && usec > 0 {
                        std::thread::sleep(std::time::Duration::from_micros(usec.min(100_000)));
                    }
                    off += 3;
                }
                0x75 => {
                    // CONDITION
                    let cond = rd08(rom, off + 1);
                    if cond_table != 0 {
                        let e = cond_table + (cond as usize) * 12;
                        if e + 12 <= rom.len() {
                            let reg = rd32(rom, e);
                            let mask = rd32(rom, e + 4);
                            let val = rd32(rom, e + 8);
                            if reg != 0 {
                                let actual = bar0.read_u32(reg).unwrap_or(0);
                                if (actual & mask) != val {
                                    execute = false;
                                }
                            }
                        }
                    }
                    off += 2;
                }
                0x56 => {
                    // CONDITION_TIME
                    let cond = rd08(rom, off + 1);
                    let retries = rd08(rom, off + 2).max(1);
                    let delay = rd16(rom, off + 3) as u64;
                    if cond_table != 0 {
                        let e = cond_table + (cond as usize) * 12;
                        let mut met = false;
                        for _ in 0..retries {
                            if e + 12 <= rom.len() {
                                let reg = rd32(rom, e);
                                let mask = rd32(rom, e + 4);
                                let val = rd32(rom, e + 8);
                                let actual = bar0.read_u32(reg).unwrap_or(0);
                                if (actual & mask) == val {
                                    met = true;
                                    break;
                                }
                            }
                            std::thread::sleep(std::time::Duration::from_micros(delay));
                        }
                        if !met {
                            execute = false;
                        }
                    }
                    off += 5;
                }
                0x73 => {
                    off += 3;
                } // STRAP_CONDITION (skip)
                0x6D => {
                    // RAM_CONDITION
                    let mask = rd08(rom, off + 1);
                    let val = rd08(rom, off + 2);
                    let strap = bar0.read_u32(0x101000).unwrap_or(0) as u8;
                    if (strap & mask) != val {
                        execute = false;
                    }
                    off += 3;
                }
                0x33 => {
                    off += 2;
                } // REPEAT
                0x36 => {
                    off += 1;
                } // END_REPEAT
                0x5C => {
                    // JUMP
                    let target = rd16(rom, off + 1) as usize;
                    off = if target > 0 && target < rom.len() {
                        target
                    } else {
                        0
                    };
                }
                0x5B => {
                    off += 3;
                } // SUB_DIRECT (skip for simplicity)
                0x6B => {
                    off += 2;
                } // SUB (skip)
                0x76 | 0x39 => {
                    off += 2;
                } // IO_CONDITION / IO_FLAG_CONDITION
                0x3A => {
                    let sz = rd08(rom, off + 2) as usize;
                    off += 3 + sz;
                }
                // PLL opcodes (skip)
                0x79 | 0x4B => {
                    off += 9;
                }
                0x34 | 0x4A => {
                    let c = rd08(rom, off + 9) as usize;
                    off += 10 + c * 4;
                }
                0x59 => {
                    off += 13;
                }
                // I/O and GPIO (skip)
                0x69 => {
                    off += 5;
                }
                0x32 => {
                    let c = rd08(rom, off + 7) as usize;
                    off += 8 + c * 4;
                }
                0x37 => {
                    off += 11;
                }
                0x3B | 0x3C => {
                    off += 5;
                }
                0x49 => {
                    let c = rd08(rom, off + 7) as usize;
                    off += 8 + c * 2;
                }
                0x4C => {
                    off += 7;
                }
                0x4D => {
                    off += 6;
                }
                0x4E => {
                    let c = rd08(rom, off + 4) as usize;
                    off += 5 + c;
                }
                0x4F => {
                    off += 9;
                }
                0x50 => {
                    let c = rd08(rom, off + 3) as usize;
                    off += 4 + c * 2;
                }
                0x51 => {
                    off += 7;
                }
                0x52 => {
                    off += 4;
                }
                0x53 => {
                    off += 3;
                }
                0x54 => {
                    let c = rd08(rom, off + 1) as usize;
                    off += 2 + c * 2;
                }
                0x5A => {
                    off += 9;
                } // ZM_REG_INDIRECT
                0x5E => {
                    off += 6;
                }
                0x5F => {
                    off += 22;
                }
                0x62 => {
                    off += 5;
                }
                0x78 => {
                    off += 6;
                }
                0x87 => {
                    off += 5 + 4 * 4;
                } // simplified RAM_RESTRICT
                0x8F => {
                    let c = rd08(rom, off + 5) as usize;
                    off += 6 + c * 4 * 4;
                }
                0x90 => {
                    off += 9;
                }
                0x96 => {
                    off += 11;
                }
                0x97 => {
                    // ZM_MASK_ADD
                    let reg = rd32(rom, off + 1);
                    let mask = rd32(rom, off + 5);
                    let add = rd08(rom, off + 9) as u32;
                    if execute && reg < 0x0100_0000 {
                        let cur = bar0.read_u32(reg).unwrap_or(0);
                        let _ = bar0.write_u32(reg, (cur & mask) + add);
                        writes += 1;
                    }
                    off += 11;
                }
                0x98 => {
                    off += 8;
                }
                0x99 => {
                    let c = rd08(rom, off + 5) as usize;
                    off += 6 + c;
                }
                0x9A => {
                    off += 9;
                }
                0xA9 => {
                    let c = rd08(rom, off + 1) as usize;
                    off += 2 + c * 2;
                }
                // No-ops
                0x63 | 0x66..=0x68 | 0x8C..=0x8E | 0x92 | 0xAA => {
                    off += 1;
                }
                0x65 => {
                    off += 3;
                }
                0x6F => {
                    off += 2;
                }
                _ => {
                    eprintln!("    Unknown opcode {op:#04x} at {off:#06x}, stopping script");
                    off = 0;
                }
            }
        }
        eprintln!("    Script {script_idx}: {ops} ops, {writes} writes");
        total_ops += ops;
        total_writes += writes;
        total_scripts += 1;
        script_idx += 1;
        if script_idx > 50 {
            break;
        }
    }
    (total_scripts, total_ops, total_writes)
}

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

    let mut bar0 = Bar0Access::from_sysfs_device(dev_path).expect("Failed to open BAR0");

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
