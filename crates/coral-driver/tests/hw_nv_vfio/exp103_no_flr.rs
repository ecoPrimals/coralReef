// SPDX-License-Identifier: AGPL-3.0-only

//! Exp 103: No-FLR ACR Boot
//!
//! Hypothesis: The persistent DMA trap at TRACEPC=0x0500 in HS mode is caused
//! by GPU subsystem state lost during FLR — not DMA configuration. We have
//! exhaustively verified VRAM, page tables, FBIF, ctx_dma, and binding.
//!
//! Approach: Disable PCI reset before swapping from nouveau to vfio-pci.
//! This preserves nouveau's GPU initialization (devinit, fb, memory training).
//! Then attempt ACR boot on the fully-initialized GPU.

use crate::ember_client;
use crate::helpers::{init_tracing, vfio_bdf};

const SEC2_BASE: usize = 0x087000;
const FECS_BASE: usize = 0x409000;
const GPCCS_BASE: usize = 0x41a000;

mod freg103 {
    pub const CPUCTL: usize = 0x100;
    pub const SCTL: usize = 0x240;
    pub const PC: usize = 0x030;
    pub const EXCI: usize = 0x148;
    pub const MAILBOX0: usize = 0x040;
    pub const MAILBOX1: usize = 0x044;
    pub const CPUCTL_HALTED: u32 = 1 << 4;
    pub const CPUCTL_STOPPED: u32 = 1 << 5;
}

fn disable_pci_reset(bdf: &str) -> Result<(), String> {
    let path = format!("/sys/bus/pci/devices/{bdf}/reset_method");
    std::fs::write(&path, "").map_err(|e| format!("write {path}: {e}"))
}

fn restore_pci_reset(bdf: &str) {
    let path = format!("/sys/bus/pci/devices/{bdf}/reset_method");
    let _ = std::fs::write(&path, "flr\n");
}

#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn exp103_no_flr_acr_boot() {
    init_tracing();
    let bdf = vfio_bdf();

    eprintln!("\n=== Exp 103: No-FLR ACR Boot ===\n");

    // Phase 1: nouveau cycle WITHOUT FLR on return
    eprintln!("── Phase 1: Nouveau Init → No-FLR swap back ──");
    {
        let mut gp =
            crate::glowplug_client::GlowPlugClient::connect().expect("GlowPlug connection");

        gp.swap(&bdf, "nouveau").expect("swap→nouveau");
        eprintln!("  nouveau bound, sleeping 3s for full init...");
        std::thread::sleep(std::time::Duration::from_secs(3));

        // Disable PCI reset BEFORE swapping to vfio-pci
        match disable_pci_reset(&bdf) {
            Ok(()) => eprintln!("  PCI reset disabled for {bdf}"),
            Err(e) => eprintln!("  WARNING: Could not disable reset: {e}"),
        }

        gp.swap(&bdf, "vfio-pci").expect("swap→vfio-pci");
        eprintln!("  vfio-pci bound (no FLR), sleeping 500ms...");
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Restore reset method for future use
        restore_pci_reset(&bdf);
    }

    let fds = ember_client::request_fds(&bdf).expect("ember fds");
    let vfio_dev = coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");

    // Phase 2: Check what state nouveau left
    eprintln!("\n── Phase 2: Post-Nouveau (No-FLR) State ──");
    let mut sec2_already_hs = false;
    for (name, base) in [
        ("SEC2", SEC2_BASE),
        ("FECS", FECS_BASE),
        ("GPCCS", GPCCS_BASE),
    ] {
        let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD_DEAD);
        let cpuctl = r(freg103::CPUCTL);
        let sctl = r(freg103::SCTL);
        let pc = r(freg103::PC);
        let exci = r(freg103::EXCI);
        let mb0 = r(freg103::MAILBOX0);
        let halted = cpuctl & freg103::CPUCTL_HALTED != 0;
        let stopped = cpuctl & freg103::CPUCTL_STOPPED != 0;
        let hs = sctl & 0x02 != 0;
        eprintln!(
            "  {name}: cpuctl={cpuctl:#010x} HALTED={halted} STOPPED={stopped} HS={hs} sctl={sctl:#010x}"
        );
        eprintln!("    PC={pc:#06x} EXCI={exci:#010x} mb0={mb0:#010x}");

        if name == "SEC2" && hs && !stopped && !halted {
            eprintln!("  *** SEC2 is alive in HS mode from nouveau! ***");
            sec2_already_hs = true;
        }
        if name == "SEC2" && hs && (stopped || exci != 0) {
            eprintln!("  SEC2 in HS but stopped/faulted — will need fresh boot");
        }
    }

    // MC / FBHUB state comparison (no FLR means nouveau's init should persist)
    {
        let mc_boot = bar0.read_u32(0x100000).unwrap_or(0xDEAD);
        let mc_cfg = bar0.read_u32(0x100004).unwrap_or(0xDEAD);
        let fbhub0 = bar0.read_u32(0x100800).unwrap_or(0xDEAD);
        let fbhub4 = bar0.read_u32(0x100804).unwrap_or(0xDEAD);
        let fbhub8 = bar0.read_u32(0x100808).unwrap_or(0xDEAD);
        let pmc = bar0.read_u32(0x000200).unwrap_or(0xDEAD);
        let mmu_ctrl = bar0.read_u32(0x100C80).unwrap_or(0xDEAD);
        eprintln!(
            "  MC: PFB[0]={mc_boot:#010x} [4]={mc_cfg:#010x} FBHUB={fbhub0:#010x}/{fbhub4:#010x}/{fbhub8:#010x}"
        );
        eprintln!("  MC: PMC_EN={pmc:#010x} MMU_CTRL={mmu_ctrl:#010x}");
    }

    if sec2_already_hs {
        // Phase 3A: SEC2 survived — try sending BOOTSTRAP_FALCON commands
        eprintln!("\n── Phase 3A: SEC2 Alive — Try Mailbox Commands ──");

        let bootvec = coral_driver::nv::vfio_compute::acr_boot::FalconBootvecOffsets {
            gpccs: 0x3400,
            fecs: 0x7E00,
        };
        let result =
            coral_driver::nv::vfio_compute::acr_boot::attempt_acr_mailbox_command(&bar0, &bootvec);
        eprintln!("  Strategy: {}", result.strategy);
        eprintln!("  Success: {}", result.success);
        for note in &result.notes {
            eprintln!("  | {note}");
        }
    } else {
        // Phase 3B: SEC2 not running — try fresh ACR boot on initialized GPU
        eprintln!("\n── Phase 3B: Fresh ACR Boot on Initialized GPU ──");

        let fw = coral_driver::nv::vfio_compute::acr_boot::AcrFirmwareSet::load("gv100")
            .expect("firmware load");
        let container = vfio_dev.dma_backend();

        let result = coral_driver::nv::vfio_compute::acr_boot::attempt_sysmem_acr_boot_full(
            &bar0, &fw, container,
        );
        eprintln!("  Strategy: {}", result.strategy);
        eprintln!("  Success: {}", result.success);
        for note in &result.notes {
            eprintln!("  | {note}");
        }
    }

    // Phase 4: Final state
    eprintln!("\n── Phase 4: Final State ──");
    for (name, base) in [
        ("SEC2", SEC2_BASE),
        ("FECS", FECS_BASE),
        ("GPCCS", GPCCS_BASE),
    ] {
        let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD_DEAD);
        let cpuctl = r(freg103::CPUCTL);
        let sctl = r(freg103::SCTL);
        let pc = r(freg103::PC);
        let exci = r(freg103::EXCI);
        let mb0 = r(freg103::MAILBOX0);
        let halted = cpuctl & freg103::CPUCTL_HALTED != 0;
        let hs = sctl & 0x02 != 0;
        eprintln!(
            "  {name}: cpuctl={cpuctl:#010x} HALTED={halted} HS={hs} PC={pc:#06x} EXCI={exci:#010x} mb0={mb0:#010x}"
        );
    }

    eprintln!("\n=== Exp 103 Complete ===");
}
