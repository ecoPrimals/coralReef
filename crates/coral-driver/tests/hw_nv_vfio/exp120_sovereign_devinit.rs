// SPDX-License-Identifier: AGPL-3.0-only

//! Exp 120: Sovereign DEVINIT + ACR Boot
//!
//! The sovereign path: run DEVINIT ourselves via BAR0 MMIO, then check if
//! WPR2 becomes valid, then run ACR. No nouveau needed.
//!
//! Flow:
//!   1. Read VBIOS from PROM (BAR0 + 0x300000)
//!   2. Parse BIT table → find PMU DEVINIT firmware
//!   3. Upload DEVINIT to PMU falcon → execute
//!   4. Check WPR2 boundaries → if valid, run ACR boot
//!   5. BOOTSTRAP_FALCON → FECS + GPCCS
//!
//! Run:
//! ```sh
//! CORALREEF_VFIO_BDF=0000:03:00.0 RUST_LOG=debug cargo test -p coral-driver \
//!   --features vfio --test hw_nv_vfio exp120 -- --ignored --nocapture --test-threads=1
//! ```

use crate::helpers::{init_tracing, open_vfio};
use coral_driver::nv::vfio_compute::acr_boot::{
    AcrFirmwareSet, BootConfig, FalconBootvecOffsets, attempt_acr_mailbox_command,
    attempt_sysmem_acr_boot_with_config,
};
use coral_driver::vfio::channel::devinit::{DevinitStatus, execute_devinit_with_diagnostics};
use coral_driver::vfio::device::MappedBar;

mod r120 {
    pub const SEC2_BASE: usize = 0x087000;
    pub const FECS_BASE: usize = 0x409000;
    pub const GPCCS_BASE: usize = 0x41a000;
    pub const PMU_BASE: usize = 0x10a000;

    pub const CPUCTL: usize = 0x100;
    pub const SCTL: usize = 0x240;
    pub const PC: usize = 0x030;
    pub const EXCI: usize = 0x148;
    pub const MAILBOX0: usize = 0x040;
    pub const _HWCFG: usize = 0x108;

    pub const CPUCTL_HRESET: u32 = 1 << 4;
    pub const CPUCTL_HALTED: u32 = 1 << 5;
    pub const INDEXED_WPR: usize = 0x100CD4;

    pub const _PMC_BOOT0: usize = 0x000000;
    pub const _PMC_ENABLE: usize = 0x000200;
}

fn read_wpr2(bar0: &MappedBar) -> (u64, u64, bool) {
    let _ = bar0.write_u32(r120::INDEXED_WPR, 2);
    let raw_s = bar0.read_u32(r120::INDEXED_WPR).unwrap_or(0);
    let _ = bar0.write_u32(r120::INDEXED_WPR, 3);
    let raw_e = bar0.read_u32(r120::INDEXED_WPR).unwrap_or(0);
    let start = ((raw_s as u64) & 0xFFFF_FF00) << 8;
    let end = ((raw_e as u64) & 0xFFFF_FF00) << 8;
    let valid = start > 0 && end > start && (end - start) > 0x1000;
    eprintln!("  WPR2 raw: idx2={raw_s:#010x} idx3={raw_e:#010x}");
    (start, end, valid)
}

fn falcon_state(bar0: &MappedBar, name: &str, base: usize) {
    let cpuctl = bar0.read_u32(base + r120::CPUCTL).unwrap_or(0xDEAD);
    let sctl = bar0.read_u32(base + r120::SCTL).unwrap_or(0xDEAD);
    let pc = bar0.read_u32(base + r120::PC).unwrap_or(0xDEAD);
    let exci = bar0.read_u32(base + r120::EXCI).unwrap_or(0xDEAD);
    let mb0 = bar0.read_u32(base + r120::MAILBOX0).unwrap_or(0xDEAD);
    let hreset = cpuctl & r120::CPUCTL_HRESET != 0;
    let halted = cpuctl & r120::CPUCTL_HALTED != 0;
    let sctl_mode = match sctl {
        0x3000 => "LS",
        0x3002 => "HS",
        0x7021 => "FWSEC",
        _ => "??",
    };
    eprintln!(
        "  {name:6}: cpuctl={cpuctl:#010x} SCTL={sctl:#06x}({sctl_mode}) \
         PC={pc:#06x} EXCI={exci:#010x} MB0={mb0:#010x} hreset={hreset} halted={halted}"
    );
}

#[test]
#[ignore = "requires VFIO-bound GPU hardware + GlowPlug + Ember"]
fn exp120_sovereign_devinit() {
    init_tracing();
    let eq = "=".repeat(70);

    eprintln!("{eq}");
    eprintln!("#  Exp 120: Sovereign DEVINIT + ACR Boot");
    eprintln!("#  Run DEVINIT ourselves, then ACR — full sovereign path");
    eprintln!("{eq}");

    // ── Phase A: Pre-DEVINIT state ─────────────────────────────
    eprintln!("\n{eq}");
    eprintln!("  PHASE A: Pre-DEVINIT GPU State");
    eprintln!("{eq}");

    let dev = open_vfio();
    let bar0 = dev.bar0_ref();
    let bdf = crate::helpers::vfio_bdf();

    eprintln!("\n  ── Falcon State (pre-DEVINIT) ──");
    falcon_state(bar0, "PMU", r120::PMU_BASE);
    falcon_state(bar0, "SEC2", r120::SEC2_BASE);
    falcon_state(bar0, "FECS", r120::FECS_BASE);
    falcon_state(bar0, "GPCCS", r120::GPCCS_BASE);

    eprintln!("\n  ── WPR2 (pre-DEVINIT) ──");
    let (w_s, w_e, w_v) = read_wpr2(bar0);
    eprintln!("  WPR2: start={w_s:#012x} end={w_e:#012x} valid={w_v}");

    let status = DevinitStatus::probe(bar0);
    eprintln!(
        "\n  needs_post={} devinit_reg={:#010x}",
        status.needs_post, status.devinit_reg
    );

    // ── Phase B: Execute DEVINIT ───────────────────────────────
    eprintln!("\n{eq}");
    eprintln!("  PHASE B: Execute Sovereign DEVINIT via PMU FALCON");
    eprintln!("{eq}");

    match execute_devinit_with_diagnostics(bar0, Some(&bdf)) {
        Ok(true) => eprintln!("\n  *** DEVINIT SUCCEEDED ***"),
        Ok(false) => eprintln!("\n  DEVINIT reports: not needed"),
        Err(e) => {
            eprintln!("\n  DEVINIT FAILED: {e}");
            eprintln!("  Continuing to check state anyway...");
        }
    }

    // ── Phase C: Post-DEVINIT state ────────────────────────────
    eprintln!("\n{eq}");
    eprintln!("  PHASE C: Post-DEVINIT GPU State");
    eprintln!("{eq}");

    eprintln!("\n  ── Falcon State (post-DEVINIT) ──");
    falcon_state(bar0, "PMU", r120::PMU_BASE);
    falcon_state(bar0, "SEC2", r120::SEC2_BASE);
    falcon_state(bar0, "FECS", r120::FECS_BASE);
    falcon_state(bar0, "GPCCS", r120::GPCCS_BASE);

    eprintln!("\n  ── WPR2 (post-DEVINIT) ──");
    let (w_s2, w_e2, w_v2) = read_wpr2(bar0);
    let w_sz2 = w_e2.saturating_sub(w_s2);
    eprintln!("  WPR2: start={w_s2:#012x} end={w_e2:#012x} size={w_sz2:#x} valid={w_v2}");

    let status2 = DevinitStatus::probe(bar0);
    eprintln!(
        "  needs_post={} devinit_reg={:#010x}",
        status2.needs_post, status2.devinit_reg
    );

    if !w_v2 {
        eprintln!("\n  WPR2 still invalid after DEVINIT.");
        eprintln!("  Proceeding to ACR boot anyway — DEVINIT IS complete, memory trained.");
        eprintln!("  ACR might establish WPR2 itself, or work without it.\n");
    } else {
        eprintln!("\n  *** WPR2 IS VALID! Proceeding to ACR boot... ***\n");
    }

    // ── Phase D: ACR Boot ──────────────────────────────────────
    eprintln!("{eq}");
    eprintln!("  PHASE D: ACR Boot with Valid WPR2");
    eprintln!("{eq}");

    let fw = AcrFirmwareSet::load("gv100").expect("load firmware from /lib/firmware/nvidia/gv100/");

    let config = BootConfig {
        pde_upper: true,
        acr_vram_pte: false,
        blob_size_zero: true,
        bind_vram: false,
        imem_preload: false,
        tlb_invalidate: true,
    };

    let result = attempt_sysmem_acr_boot_with_config(bar0, &fw, dev.dma_backend(), &config);
    eprintln!("\n  {result}");

    // ── Phase E: BOOTSTRAP_FALCON ──────────────────────────────
    eprintln!("\n{eq}");
    eprintln!("  PHASE E: BOOTSTRAP_FALCON");
    eprintln!("{eq}");

    let bootvecs = FalconBootvecOffsets {
        fecs: 0x7E00,
        gpccs: 0x3400,
    };
    let mb_result = attempt_acr_mailbox_command(bar0, &bootvecs);
    eprintln!("\n  {mb_result}");

    eprintln!("\n  ── Final Falcon State ──");
    falcon_state(bar0, "PMU", r120::PMU_BASE);
    falcon_state(bar0, "SEC2", r120::SEC2_BASE);
    falcon_state(bar0, "FECS", r120::FECS_BASE);
    falcon_state(bar0, "GPCCS", r120::GPCCS_BASE);

    eprintln!("\n  ── Final WPR2 ──");
    let (w_sf, w_ef, w_vf) = read_wpr2(bar0);
    eprintln!("  WPR2: start={w_sf:#012x} end={w_ef:#012x} valid={w_vf}");

    eprintln!("\n{eq}");
    eprintln!("#  Exp 120 COMPLETE");
    eprintln!("{eq}");
}
