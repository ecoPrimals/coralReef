// SPDX-License-Identifier: AGPL-3.0-only

//! Exp 119: Cold-Boot WPR2 — FWSEC State on Direct VFIO Bind
//!
//! HYPOTHESIS: On a clean boot where vfio-pci grabs the GPU BEFORE nouveau,
//! FWSEC's boot-time WPR2 setup is never destroyed. WPR2 should be VALID
//! and falcons should be in SCTL=0x7021 (FWSEC-authenticated mode).
//!
//! If WPR2 is valid, we run ACR boot — the firmware should see valid WPR2
//! boundaries and PROCESS the WPR for the first time (ending the Exp 114-118
//! stall chain).
//!
//! CRITICAL: This test must run IMMEDIATELY after a cold reboot with NO
//! prior nouveau cycles. Do NOT run other GPU tests first.
//!
//! Run:
//! ```sh
//! cargo test -p coral-driver --features vfio --test hw_nv_vfio \
//!   exp119 -- --ignored --nocapture --test-threads=1
//! ```

use crate::helpers::{init_tracing, open_vfio};
use coral_driver::gsp::RegisterAccess;
use coral_driver::nv::vfio_compute::acr_boot::{
    AcrFirmwareSet, BootConfig, FalconBootvecOffsets, attempt_acr_mailbox_command,
    attempt_sysmem_acr_boot_with_config,
};
use coral_driver::vfio::device::MappedBar;

mod r119 {
    pub const SEC2_BASE: usize = 0x087000;
    pub const FECS_BASE: usize = 0x409000;
    pub const GPCCS_BASE: usize = 0x41a000;
    pub const PMU_BASE: usize = 0x10a000;

    pub const CPUCTL: usize = 0x100;
    pub const SCTL: usize = 0x240;
    pub const PC: usize = 0x030;
    pub const EXCI: usize = 0x148;
    pub const MAILBOX0: usize = 0x040;
    pub const HWCFG: usize = 0x108;

    pub const CPUCTL_HRESET: u32 = 1 << 4;
    pub const CPUCTL_HALTED: u32 = 1 << 5;
    pub const INDEXED_WPR: usize = 0x100CD4;

    pub const PMC_BOOT0: usize = 0x000000;
    pub const PMC_ENABLE: usize = 0x000200;
}

fn read_wpr2(bar0: &MappedBar) -> (u64, u64, bool) {
    let _ = bar0.write_u32(r119::INDEXED_WPR, 2);
    let raw_s = bar0.read_u32(r119::INDEXED_WPR).unwrap_or(0);
    let _ = bar0.write_u32(r119::INDEXED_WPR, 3);
    let raw_e = bar0.read_u32(r119::INDEXED_WPR).unwrap_or(0);
    let start = ((raw_s as u64) & 0xFFFF_FF00) << 8;
    let end = ((raw_e as u64) & 0xFFFF_FF00) << 8;
    let valid = start > 0 && end > start && (end - start) > 0x1000;
    (start, end, valid)
}

fn falcon_state(bar0: &MappedBar, name: &str, base: usize) {
    let cpuctl = bar0.read_u32(base + r119::CPUCTL).unwrap_or(0xDEAD);
    let sctl = bar0.read_u32(base + r119::SCTL).unwrap_or(0xDEAD);
    let pc = bar0.read_u32(base + r119::PC).unwrap_or(0xDEAD);
    let exci = bar0.read_u32(base + r119::EXCI).unwrap_or(0xDEAD);
    let mb0 = bar0.read_u32(base + r119::MAILBOX0).unwrap_or(0xDEAD);
    let hwcfg = bar0.read_u32(base + r119::HWCFG).unwrap_or(0xDEAD);
    let hreset = cpuctl & r119::CPUCTL_HRESET != 0;
    let halted = cpuctl & r119::CPUCTL_HALTED != 0;
    let alive = !hreset && !halted && cpuctl != 0xDEAD;
    let sctl_mode = match sctl {
        0x3000 => "LS (fuse-enforced)",
        0x3002 => "HS (authenticated)",
        0x7021 => "FWSEC-authenticated",
        _ => "UNKNOWN",
    };
    eprintln!(
        "  {name:6}: cpuctl={cpuctl:#010x} SCTL={sctl:#06x}={sctl_mode} \
         PC={pc:#06x} EXCI={exci:#010x} MB0={mb0:#010x} HWCFG={hwcfg:#010x} \
         alive={alive} hreset={hreset}"
    );
}

#[test]
#[ignore]
fn exp119_cold_boot_wpr2() {
    init_tracing();
    let eq = "=".repeat(70);

    eprintln!("{eq}");
    eprintln!("#  Exp 119: Cold-Boot WPR2 — Direct VFIO Without Nouveau Cycle");
    eprintln!("#  HYPOTHESIS: FWSEC boot-time WPR2 survives if nouveau never ran");
    eprintln!("{eq}");

    // ── Phase A: Read cold-boot state ──────────────────────────
    eprintln!("\n{eq}");
    eprintln!("  PHASE A: Open VFIO device directly (NO nouveau cycle)");
    eprintln!("{eq}");

    let dev = open_vfio();
    let bar0 = dev.bar0_ref();

    let boot0 = bar0.read_u32(r119::PMC_BOOT0).unwrap_or(0);
    let pmc_en = bar0.read_u32(r119::PMC_ENABLE).unwrap_or(0);
    eprintln!("  BOOT0 = {boot0:#010x}  PMC_ENABLE = {pmc_en:#010x}");

    eprintln!("\n  ── Falcon State (cold boot) ──");
    falcon_state(bar0, "SEC2", r119::SEC2_BASE);
    falcon_state(bar0, "FECS", r119::FECS_BASE);
    falcon_state(bar0, "GPCCS", r119::GPCCS_BASE);
    falcon_state(bar0, "PMU", r119::PMU_BASE);

    eprintln!("\n  ── WPR2 Hardware Boundaries ──");
    let (wpr2_start, wpr2_end, wpr2_valid) = read_wpr2(bar0);
    let wpr2_size = if wpr2_end > wpr2_start {
        wpr2_end - wpr2_start
    } else {
        0
    };
    eprintln!(
        "  WPR2: start={wpr2_start:#012x} end={wpr2_end:#012x} \
         size={wpr2_size:#x} ({} KiB) valid={wpr2_valid}",
        wpr2_size / 1024
    );

    if !wpr2_valid {
        eprintln!("\n  *** WPR2 IS INVALID — FWSEC state was destroyed ***");
        eprintln!("  Possible causes:");
        eprintln!("    - GlowPlug did a nouveau cycle on startup");
        eprintln!("    - vfio-pci bind triggered a device reset");
        eprintln!("    - FWSEC doesn't run on this GPU/BIOS combo");
        eprintln!("    - System didn't do a full cold reboot");
        eprintln!("\n  *** EXPERIMENT ENDS HERE — reboot required ***");
        eprintln!("{eq}");
        return;
    }

    eprintln!("\n  *** WPR2 IS VALID! FWSEC boot state survived! ***");
    eprintln!("  This is the breakthrough — attempting ACR boot with live WPR2...\n");

    // ── Phase B: ACR boot with valid WPR2 ──────────────────────
    eprintln!("{eq}");
    eprintln!("  PHASE B: ACR Boot — blob_size=0, correct PDEs, valid WPR2");
    eprintln!("{eq}");

    let fw = AcrFirmwareSet::load("gv100").expect("load firmware from /lib/firmware/nvidia/gv100/");
    eprintln!("  Firmware loaded OK");

    let config = BootConfig {
        pde_upper: true,
        acr_vram_pte: false,
        blob_size_zero: true,
        bind_vram: false,
        imem_preload: false,
        tlb_invalidate: true,
    };
    eprintln!("  Config: pde_upper=true, blob_size=0, TLB flush");
    eprintln!("  WPR2 addresses: start={wpr2_start:#x} end={wpr2_end:#x}");

    let result = attempt_sysmem_acr_boot_with_config(bar0, &fw, dev.dma_backend(), &config);

    eprintln!("\n  ── ACR Boot Result ──");
    eprintln!("  {result}");
    for note in &result.notes {
        eprintln!("    {note}");
    }

    eprintln!("\n  ── Post-ACR Falcon State ──");
    falcon_state(bar0, "SEC2", r119::SEC2_BASE);
    falcon_state(bar0, "FECS", r119::FECS_BASE);
    falcon_state(bar0, "GPCCS", r119::GPCCS_BASE);

    // ── Phase C: BOOTSTRAP_FALCON ──────────────────────────────
    eprintln!("\n{eq}");
    eprintln!("  PHASE C: BOOTSTRAP_FALCON — Load FECS + GPCCS");
    eprintln!("{eq}");

    let bootvecs = FalconBootvecOffsets {
        fecs: 0x7E00,
        gpccs: 0x3400,
    };

    let mb_result = attempt_acr_mailbox_command(bar0, &bootvecs);
    eprintln!("\n  ── Mailbox Command Result ──");
    eprintln!("  {mb_result}");
    for note in &mb_result.notes {
        eprintln!("    {note}");
    }

    eprintln!("\n  ── Post-BOOTSTRAP Falcon State ──");
    falcon_state(bar0, "SEC2", r119::SEC2_BASE);
    falcon_state(bar0, "FECS", r119::FECS_BASE);
    falcon_state(bar0, "GPCCS", r119::GPCCS_BASE);

    // Check WPR copy status
    eprintln!("\n  ── WPR Copy Status (MB0) ──");
    let sec2_mb0 = bar0
        .read_u32(r119::SEC2_BASE + r119::MAILBOX0)
        .unwrap_or(0xDEAD);
    let fecs_mb0 = bar0
        .read_u32(r119::FECS_BASE + r119::MAILBOX0)
        .unwrap_or(0xDEAD);
    let gpccs_mb0 = bar0
        .read_u32(r119::GPCCS_BASE + r119::MAILBOX0)
        .unwrap_or(0xDEAD);
    eprintln!("  SEC2  MB0: {sec2_mb0:#010x}");
    eprintln!("  FECS  MB0: {fecs_mb0:#010x}");
    eprintln!("  GPCCS MB0: {gpccs_mb0:#010x}");

    // Check WPR2 again (did ACR modify it?)
    eprintln!("\n  ── WPR2 After ACR ──");
    let (wpr2_s2, wpr2_e2, wpr2_v2) = read_wpr2(bar0);
    eprintln!("  WPR2: start={wpr2_s2:#012x} end={wpr2_e2:#012x} valid={wpr2_v2}");
    if wpr2_s2 != wpr2_start || wpr2_e2 != wpr2_end {
        eprintln!("  *** WPR2 CHANGED during ACR boot! ***");
    }

    eprintln!("\n{eq}");
    eprintln!("#  Exp 119 COMPLETE");
    eprintln!("{eq}");
}
