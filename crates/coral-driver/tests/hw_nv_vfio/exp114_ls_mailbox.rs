// SPDX-License-Identifier: AGPL-3.0-only

//! Exp 114: LS-Mode FECS/GPCCS Activation (Path Y)
//!
//! Exp 113 proved the HS ACR chain is blocked by PMU dependency. The alternative
//! is the LS-mode mailbox path, which already loaded FECS/GPCCS in Exp 087/091.
//!
//! Pipeline:
//! 1. Nouveau cycle → clean state
//! 2. Sysmem ACR boot (correct upper PDEs, full init) → SEC2 runs in LS idle
//! 3. BOOTSTRAP_FALCON(FECS+GPCCS) via mailbox → ACR loads targets
//! 4. BOOTVEC fix → GPCCS=0x3400, FECS=0x7E00
//! 5. STARTCPU → check if FECS/GPCCS execute
//! 6. GR init writes if FECS is alive
//!
//! Run:
//! ```sh
//! cargo test -p coral-driver --features vfio --test hw_nv_vfio \
//!   exp114 -- --ignored --nocapture --test-threads=1
//! ```

use crate::ember_client;
use crate::glowplug_client::GlowPlugClient;
use crate::helpers::init_tracing;
use coral_driver::nv::vfio_compute::acr_boot::{
    AcrFirmwareSet, FalconBootvecOffsets, attempt_acr_mailbox_command, attempt_sysmem_acr_boot_full,
};

mod freg114 {
    pub const SEC2_BASE: usize = 0x087000;
    pub const FECS_BASE: usize = 0x409000;
    pub const GPCCS_BASE: usize = 0x41a000;
    pub const CPUCTL: usize = 0x100;
    pub const SCTL: usize = 0x240;
    pub const PC: usize = 0x030;
    pub const EXCI: usize = 0x148;
    pub const MAILBOX0: usize = 0x040;
    pub const MTHD_STATUS: usize = 0xC18;

    pub const CPUCTL_HRESET: u32 = 1 << 4;
    pub const CPUCTL_HALTED: u32 = 1 << 5;
}

fn discover_bdf() -> String {
    if let Ok(bdf) = std::env::var("CORALREEF_VFIO_BDF") {
        return bdf;
    }
    let driver_path = "/sys/bus/pci/drivers/vfio-pci";
    if let Ok(entries) = std::fs::read_dir(driver_path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.contains(':') && name.contains('.') {
                let vendor_path = format!("{driver_path}/{name}/vendor");
                if let Ok(vendor) = std::fs::read_to_string(&vendor_path) {
                    if vendor.trim() == "0x10de" {
                        return name;
                    }
                }
            }
        }
    }
    panic!("No VFIO GPU found.");
}

fn nouveau_cycle(bdf: &str) {
    let mut gp = GlowPlugClient::connect().expect("GlowPlug connection");
    gp.swap(bdf, "nouveau").expect("swap→nouveau");
    std::thread::sleep(std::time::Duration::from_secs(3));
    gp.swap(bdf, "vfio-pci").expect("swap→vfio-pci");
    std::thread::sleep(std::time::Duration::from_millis(500));
}

fn falcon_state(bar0: &coral_driver::vfio::device::MappedBar, name: &str, base: usize) {
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let cpuctl = r(freg114::CPUCTL);
    let sctl = r(freg114::SCTL);
    let pc = r(freg114::PC);
    let exci = r(freg114::EXCI);
    let mb0 = r(freg114::MAILBOX0);
    let hreset = cpuctl & freg114::CPUCTL_HRESET != 0;
    let halted = cpuctl & freg114::CPUCTL_HALTED != 0;
    let hs = sctl & 0x02 != 0;
    eprintln!(
        "  {name:6}: cpuctl={cpuctl:#010x} HRESET={hreset:<5} HALTED={halted:<5} HS={hs:<5} \
         PC={pc:#06x} EXCI={exci:#010x} MB0={mb0:#010x}"
    );
}

#[test]
#[ignore = "requires VFIO-bound GPU hardware + GlowPlug + Ember"]
fn exp114_ls_mailbox_pipeline() {
    init_tracing();

    let banner = "#".repeat(60);
    eprintln!("\n{banner}");
    eprintln!("#  Exp 114: LS-Mode FECS/GPCCS Activation (Path Y)         #");
    eprintln!("{banner}\n");

    let bdf = discover_bdf();
    eprintln!("Target BDF: {bdf}");

    // ── Phase 1: Nouveau cycle ──
    eprintln!("\n── Phase 1: Nouveau Cycle ──");
    nouveau_cycle(&bdf);

    let fds = ember_client::request_fds(&bdf).expect("ember fds");
    let vfio_dev = coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");

    eprintln!("\n── Phase 1b: Post-Nouveau State ──");
    falcon_state(&bar0, "SEC2", freg114::SEC2_BASE);
    falcon_state(&bar0, "FECS", freg114::FECS_BASE);
    falcon_state(&bar0, "GPCCS", freg114::GPCCS_BASE);

    // ── Phase 2: Sysmem ACR boot (correct PDEs, LS mode, full init) ──
    eprintln!("\n── Phase 2: Sysmem ACR Boot (correct PDEs, full init) ──");
    let fw = AcrFirmwareSet::load("gv100").expect("firmware load");
    let container = vfio_dev.dma_backend();

    let acr_result = attempt_sysmem_acr_boot_full(&bar0, &fw, container);
    eprintln!("  Strategy: {}", acr_result.strategy);
    eprintln!("  Success: {}", acr_result.success);
    for note in &acr_result.notes {
        eprintln!("  | {note}");
    }

    // Check SEC2 state after ACR boot
    eprintln!("\n── Phase 2b: Post-ACR State ──");
    falcon_state(&bar0, "SEC2", freg114::SEC2_BASE);
    falcon_state(&bar0, "FECS", freg114::FECS_BASE);
    falcon_state(&bar0, "GPCCS", freg114::GPCCS_BASE);

    let sec2_pc = bar0.read_u32(freg114::SEC2_BASE + freg114::PC).unwrap_or(0);
    let sec2_cpuctl = bar0
        .read_u32(freg114::SEC2_BASE + freg114::CPUCTL)
        .unwrap_or(0);
    let sec2_alive = sec2_pc > 0x100
        && sec2_cpuctl & freg114::CPUCTL_HRESET == 0
        && sec2_cpuctl & freg114::CPUCTL_HALTED == 0;

    if !sec2_alive {
        let sec2_halted = sec2_cpuctl & freg114::CPUCTL_HALTED != 0;
        eprintln!("\n  SEC2 not in idle loop (PC={sec2_pc:#x}, halted={sec2_halted}).");
        eprintln!("  ACR boot may have completed and halted (normal for LS mode).");
        eprintln!("  Proceeding to mailbox command anyway...");
    }

    // ── Phase 3: BOOTSTRAP_FALCON via mailbox ──
    eprintln!("\n── Phase 3: BOOTSTRAP_FALCON (FECS+GPCCS) ──");

    let bootvec = FalconBootvecOffsets {
        gpccs: fw.gpccs_bl.bl_imem_off(),
        fecs: fw.fecs_bl.bl_imem_off(),
    };
    eprintln!(
        "  BOOTVEC offsets: GPCCS={:#06x} FECS={:#06x}",
        bootvec.gpccs, bootvec.fecs
    );

    let mailbox_result = attempt_acr_mailbox_command(&bar0, &bootvec);
    eprintln!("\n  Mailbox result:");
    eprintln!("  Strategy: {}", mailbox_result.strategy);
    eprintln!("  Success: {}", mailbox_result.success);
    for note in &mailbox_result.notes {
        eprintln!("  | {note}");
    }

    // ── Phase 4: Final state ──
    eprintln!("\n── Phase 4: Final State ──");
    falcon_state(&bar0, "SEC2", freg114::SEC2_BASE);
    falcon_state(&bar0, "FECS", freg114::FECS_BASE);
    falcon_state(&bar0, "GPCCS", freg114::GPCCS_BASE);

    let fecs_cpuctl = bar0
        .read_u32(freg114::FECS_BASE + freg114::CPUCTL)
        .unwrap_or(0xDEAD);
    let fecs_pc = bar0.read_u32(freg114::FECS_BASE + freg114::PC).unwrap_or(0);
    let fecs_exci = bar0
        .read_u32(freg114::FECS_BASE + freg114::EXCI)
        .unwrap_or(0);
    let gpccs_cpuctl = bar0
        .read_u32(freg114::GPCCS_BASE + freg114::CPUCTL)
        .unwrap_or(0xDEAD);
    let gpccs_pc = bar0
        .read_u32(freg114::GPCCS_BASE + freg114::PC)
        .unwrap_or(0);
    let gpccs_exci = bar0
        .read_u32(freg114::GPCCS_BASE + freg114::EXCI)
        .unwrap_or(0);

    let fecs_running = fecs_cpuctl & (freg114::CPUCTL_HRESET | freg114::CPUCTL_HALTED) == 0;
    let gpccs_running = gpccs_cpuctl & (freg114::CPUCTL_HRESET | freg114::CPUCTL_HALTED) == 0;
    let fecs_no_exci = fecs_exci == 0 || (fecs_exci >> 24) < 0x10;
    let gpccs_no_exci = gpccs_exci == 0 || (gpccs_exci >> 24) < 0x10;

    let fecs_mthd = bar0
        .read_u32(freg114::FECS_BASE + freg114::MTHD_STATUS)
        .unwrap_or(0);

    // ── Summary ──
    let sep = "=".repeat(70);
    eprintln!("\n{sep}");
    eprintln!("  Exp 114 RESULTS");
    eprintln!("{sep}");
    eprintln!(
        "  FECS:  running={fecs_running} PC={fecs_pc:#06x} EXCI={fecs_exci:#010x} MTHD={fecs_mthd:#010x}"
    );
    eprintln!("  GPCCS: running={gpccs_running} PC={gpccs_pc:#06x} EXCI={gpccs_exci:#010x}");
    eprintln!("{sep}");

    if fecs_running && fecs_no_exci {
        eprintln!("\n  *** FECS IS RUNNING! ***");
        if fecs_mthd & 1 != 0 {
            eprintln!("  *** FECS MTHD_STATUS READY — accepts GR commands! ***");
        }
    }

    if gpccs_running && gpccs_no_exci {
        eprintln!("  *** GPCCS IS RUNNING! ***");
    } else if gpccs_exci != 0 {
        let cause = gpccs_exci >> 24;
        let fault_pc = gpccs_exci & 0xFFFF;
        eprintln!("  GPCCS EXCEPTION: cause={cause:#04x} fault_pc={fault_pc:#06x}");
    }

    if fecs_running && gpccs_running && fecs_no_exci && gpccs_no_exci {
        eprintln!("\n  *** BOTH FECS AND GPCCS ALIVE — L10 SOLVED! ***");
        eprintln!("  Next: L11 — GR context init + shader dispatch");
    }

    eprintln!("\n=== Exp 114 Complete ===");
}
