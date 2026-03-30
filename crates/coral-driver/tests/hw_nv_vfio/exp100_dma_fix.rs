// SPDX-License-Identifier: AGPL-3.0-only

//! Exp 100: ACR Boot with Full IOMMU Coverage
//!
//! Root cause from Exp 098: SEC2 DMA trap (EXCI=0x201F) during WPR→FECS/GPCCS
//! copy. IOMMU fault log revealed IO_PAGE_FAULT at 0x26000-0x28000 — addresses
//! within our falcon page table range but without IOMMU backing.
//!
//! Fix: LOW_CATCH DMA buffer (0x0..0x40000) + HIGH_CATCH (WPR_end..2MiB)
//! fills all IOVA gaps so every VA in the identity-mapped PT has valid IOMMU.
//!
//! Expected outcome: SEC2 completes WPR→FECS/GPCCS DMA without trap, FECS
//! starts running, GPCCS bootstraps. Sovereign compute pipeline unlocked.

use crate::ember_client;
use crate::helpers::{init_tracing, vfio_bdf};

const SEC2_BASE: usize = 0x087000;
const FECS_BASE: usize = 0x409000;
const GPCCS_BASE: usize = 0x41a000;

mod freg {
    pub const CPUCTL: usize = 0x100;
    pub const HWCFG: usize = 0x108;
    pub const SCTL: usize = 0x240;
    pub const PC: usize = 0x030;
    pub const EXCI: usize = 0x148;
    pub const MAILBOX0: usize = 0x040;
    pub const MAILBOX1: usize = 0x044;
    pub const BOOTVEC: usize = 0x104;
    pub const IMEMC: usize = 0x180;
    pub const IMEMD: usize = 0x184;

    pub const CPUCTL_HALTED: u32 = 1 << 4;
    pub const CPUCTL_STOPPED: u32 = 1 << 5;

    pub fn imem_size(hwcfg: u32) -> usize {
        ((hwcfg & 0x1FF) as usize) << 8
    }
}

#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn exp100_full_iommu_acr_boot() {
    init_tracing();

    eprintln!("\n=== Exp 100: ACR Boot with Full IOMMU Coverage ===\n");

    let bdf = vfio_bdf();

    // Phase 1: Clean state via nouveau cycle
    eprintln!("── Phase 1: Nouveau Cycle (clean FLR) ──");
    {
        let mut gp =
            crate::glowplug_client::GlowPlugClient::connect().expect("GlowPlug connection");

        gp.swap(&bdf, "nouveau").expect("swap→nouveau");
        eprintln!("  nouveau bound, sleeping 3s for full init...");
        std::thread::sleep(std::time::Duration::from_secs(3));

        gp.swap(&bdf, "vfio-pci").expect("swap→vfio-pci");
        eprintln!("  vfio-pci bound, sleeping 500ms...");
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    let fds = ember_client::request_fds(&bdf).expect("ember fds");
    let vfio_dev = coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");

    // Phase 2: Pre-boot falcon state
    eprintln!("\n── Phase 2: Pre-Boot State ──");
    for (name, base) in [
        ("SEC2", SEC2_BASE),
        ("FECS", FECS_BASE),
        ("GPCCS", GPCCS_BASE),
    ] {
        let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD_DEAD);
        let cpuctl = r(freg::CPUCTL);
        let sctl = r(freg::SCTL);
        let pc = r(freg::PC);
        let exci = r(freg::EXCI);
        let halted = cpuctl & freg::CPUCTL_HALTED != 0;
        eprintln!(
            "  {name}: cpuctl={cpuctl:#010x} HALTED={halted} sctl={sctl:#010x} PC={pc:#06x} EXCI={exci:#010x}"
        );
    }

    // Phase 3: SysMem ACR boot with VRAM WPR mirror
    eprintln!("\n── Phase 3: SysMem ACR Boot + VRAM WPR Mirror ──");
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

    // Phase 4: Post-boot state
    eprintln!("\n── Phase 4: Post-Boot State ──");
    for (name, base) in [
        ("SEC2", SEC2_BASE),
        ("FECS", FECS_BASE),
        ("GPCCS", GPCCS_BASE),
    ] {
        let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD_DEAD);
        let cpuctl = r(freg::CPUCTL);
        let sctl = r(freg::SCTL);
        let pc = r(freg::PC);
        let exci = r(freg::EXCI);
        let mb0 = r(freg::MAILBOX0);
        let mb1 = r(freg::MAILBOX1);
        let hwcfg = r(freg::HWCFG);
        let halted = cpuctl & freg::CPUCTL_HALTED != 0;
        let stopped = cpuctl & freg::CPUCTL_STOPPED != 0;
        let hs = sctl == 0x3002;
        eprintln!("  {name}: cpuctl={cpuctl:#010x} HALTED={halted} STOPPED={stopped} HS={hs}");
        eprintln!("    PC={pc:#06x} EXCI={exci:#010x} mb0={mb0:#010x} mb1={mb1:#010x}");

        // IMEM probe
        let w = |off: usize, val: u32| {
            let _ = bar0.write_u32(base + off, val);
        };
        w(freg::IMEMC, (1u32 << 25) | 0);
        let imem: Vec<u32> = (0..32).map(|_| r(freg::IMEMD)).collect();
        let nz = imem.iter().filter(|&&w| w != 0 && w != 0xDEAD_DEAD).count();
        eprintln!("    IMEM[0..128]: {nz}/32 non-zero");
        if nz > 0 {
            let first: Vec<String> = imem
                .iter()
                .enumerate()
                .filter(|&(_, &w)| w != 0 && w != 0xDEAD_DEAD)
                .take(4)
                .map(|(i, w)| format!("[{:#05x}]={w:#010x}", i * 4))
                .collect();
            eprintln!("    First: {}", first.join(" "));
        }
    }

    // Phase 5: If FECS is not in HRESET, test GR readiness
    let fecs_cpuctl = bar0.read_u32(FECS_BASE + freg::CPUCTL).unwrap_or(0xDEAD);
    let fecs_pc = bar0.read_u32(FECS_BASE + freg::PC).unwrap_or(0xDEAD);
    let fecs_exci = bar0.read_u32(FECS_BASE + freg::EXCI).unwrap_or(0xDEAD);

    if fecs_cpuctl & freg::CPUCTL_HALTED == 0 && fecs_exci == 0 {
        eprintln!("\n── Phase 5: FECS ALIVE — Testing GR Readiness ──");
        let pgraph = bar0.read_u32(0x400700).unwrap_or(0xDEAD);
        let gr_class = bar0.read_u32(0x410004).unwrap_or(0xDEAD);
        let fecs_mb0 = bar0.read_u32(FECS_BASE + freg::MAILBOX0).unwrap_or(0xDEAD);
        eprintln!(
            "  PGRAPH_STATUS={pgraph:#010x} GR_CLASS={gr_class:#010x} FECS_MB0={fecs_mb0:#010x}"
        );

        let gpccs_cpuctl = bar0.read_u32(GPCCS_BASE + freg::CPUCTL).unwrap_or(0xDEAD);
        let gpccs_pc = bar0.read_u32(GPCCS_BASE + freg::PC).unwrap_or(0xDEAD);
        let gpccs_exci = bar0.read_u32(GPCCS_BASE + freg::EXCI).unwrap_or(0xDEAD);
        eprintln!(
            "  GPCCS: cpuctl={gpccs_cpuctl:#010x} PC={gpccs_pc:#06x} EXCI={gpccs_exci:#010x}"
        );

        eprintln!("\n  *** SOVEREIGN COMPUTE PIPELINE POTENTIALLY UNLOCKED ***");
    } else {
        eprintln!("\n── Phase 5: FECS still halted/faulted ──");
        eprintln!("  FECS: cpuctl={fecs_cpuctl:#010x} PC={fecs_pc:#06x} EXCI={fecs_exci:#010x}");

        // Check SEC2 state more carefully
        let sec2_cpuctl = bar0.read_u32(SEC2_BASE + freg::CPUCTL).unwrap_or(0xDEAD);
        let sec2_exci = bar0.read_u32(SEC2_BASE + freg::EXCI).unwrap_or(0xDEAD);
        let sec2_sctl = bar0.read_u32(SEC2_BASE + freg::SCTL).unwrap_or(0xDEAD);
        eprintln!(
            "  SEC2: cpuctl={sec2_cpuctl:#010x} EXCI={sec2_exci:#010x} SCTL={sec2_sctl:#010x}"
        );

        if sec2_exci == 0 && sec2_cpuctl & freg::CPUCTL_STOPPED == 0 {
            eprintln!("  SEC2 completed WITHOUT DMA trap — progress! Check WPR state.");
        }
    }

    eprintln!("\n=== Exp 100 Complete ===");
}
