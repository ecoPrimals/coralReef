// SPDX-License-Identifier: AGPL-3.0-only

//! Exp 144: Warm ACR Boot — NO GR Reset
//!
//! HYPOTHESIS: The PRI ring fault in Exp 121 is caused by the PMC GR reset
//! (bit 12 toggle) which makes GPCCS inaccessible. When ACR firmware then
//! tries to copy authenticated firmware to GPCCS, it faults the PRI ring.
//!
//! This test is identical to Exp 121 BUT skips the GR PMC reset entirely.
//! On a warm GPU (after nouveau), FECS/GPCCS should remain accessible.
//! SEC2 is reset normally (PMC bit 5), loaded with ACR, and started.
//!
//! Prerequisites: GPU must be warm (swap to nouveau, wait, swap back to vfio).
//!
//! Run:
//! ```sh
//! CORALREEF_VFIO_BDF=0000:03:00.0 RUST_LOG=info cargo test -p coral-driver \
//!   --features vfio --test hw_nv_vfio exp144 -- --ignored --nocapture --test-threads=1
//! ```

use crate::helpers::init_tracing;
use coral_driver::nv::vfio_compute::acr_boot::{
    AcrFirmwareSet, BootConfig, FalconBootvecOffsets, attempt_acr_mailbox_command,
    attempt_sysmem_acr_boot_with_config, attempt_vram_native_acr_boot,
};
use coral_driver::vfio::device::{MappedBar, VfioDevice};

mod r144 {
    pub const SEC2_BASE: usize = 0x087000;
    pub const FECS_BASE: usize = 0x409000;
    pub const GPCCS_BASE: usize = 0x41a000;
    pub const PMU_BASE: usize = 0x10a000;

    pub const CPUCTL: usize = 0x100;
    pub const SCTL: usize = 0x240;
    pub const PC: usize = 0x030;
    pub const EXCI: usize = 0x148;
    pub const MAILBOX0: usize = 0x040;

    pub const PMC_ENABLE: usize = 0x000200;
    pub const PMC_INTR: usize = 0x000100;

    pub const PRIV_RING_INTR_STATUS: usize = 0x120058;
    pub const PRIV_RING_COMMAND: usize = 0x12004C;
    pub const PRIV_RING_CMD_ACK: u32 = 0x02;

    pub const GR_ENGINE_BIT: u32 = 1 << 12;
}

fn falcon_state(bar0: &MappedBar, name: &str, base: usize) {
    let cpuctl = bar0.read_u32(base + r144::CPUCTL).unwrap_or(0xDEAD);
    let sctl = bar0.read_u32(base + r144::SCTL).unwrap_or(0xDEAD);
    let pc = bar0.read_u32(base + r144::PC).unwrap_or(0xDEAD);
    let exci = bar0.read_u32(base + r144::EXCI).unwrap_or(0xDEAD);
    let mb0 = bar0.read_u32(base + r144::MAILBOX0).unwrap_or(0xDEAD);
    let hreset = cpuctl & 0x10 != 0;
    let halted = cpuctl & 0x20 != 0;
    let mode = match sctl {
        0x3000 => "LS",
        0x3002 => "HS",
        0x7021 => "FW",
        _ => "??",
    };
    eprintln!(
        "  {name:6}: cpuctl={cpuctl:#010x} SCTL={sctl:#06x}({mode}) \
         PC={pc:#06x} EXCI={exci:#010x} MB0={mb0:#010x} hreset={hreset} halted={halted}"
    );
}

fn clear_pri_faults(bar0: &MappedBar) {
    let status = bar0.read_u32(r144::PRIV_RING_INTR_STATUS).unwrap_or(0);
    eprintln!("  PRI ring status: {status:#010x}");
    if status != 0 {
        for i in 0..10 {
            let _ = bar0.write_u32(r144::PRIV_RING_COMMAND, r144::PRIV_RING_CMD_ACK);
            std::thread::sleep(std::time::Duration::from_millis(20));
            let s = bar0.read_u32(r144::PRIV_RING_INTR_STATUS).unwrap_or(0);
            if s == 0 {
                eprintln!("  PRI faults cleared (attempt {i})");
                return;
            }
        }
        let s = bar0.read_u32(r144::PRIV_RING_INTR_STATUS).unwrap_or(0);
        eprintln!("  PRI faults still pending: {s:#010x}");
    } else {
        eprintln!("  PRI ring clean — no faults");
    }
}

fn verify_gpccs_accessible(bar0: &MappedBar) -> bool {
    let cpuctl = bar0.read_u32(r144::GPCCS_BASE + r144::CPUCTL).unwrap_or(0xDEAD);
    let is_pri_err = cpuctl == 0xBADF1000 || cpuctl == 0xBAD00100
        || (cpuctl & 0xFFF00000) == 0xBAD00000 || (cpuctl & 0xFFF00000) == 0xBADF0000;
    if is_pri_err {
        eprintln!("  *** GPCCS INACCESSIBLE: cpuctl={cpuctl:#010x} — GR fabric broken ***");
    } else {
        eprintln!("  GPCCS accessible: cpuctl={cpuctl:#010x}");
    }
    !is_pri_err
}

#[test]
#[ignore = "requires VFIO-bound GPU hardware + GlowPlug + Ember + warm GPU"]
fn exp144_warm_acr_no_gr_reset() {
    init_tracing();
    let eq = "=".repeat(70);
    let bdf = crate::helpers::vfio_bdf();

    eprintln!("{eq}");
    eprintln!("#  Exp 144: Warm ACR Boot — NO GR Reset");
    eprintln!("#  HYPOTHESIS: GR reset kills GPCCS → PRI ring fault during ACR");
    eprintln!("#  FIX: Skip GR reset on warm GPU, let ACR access intact GPCCS");
    eprintln!("{eq}");

    // ── Phase A: Minimal VFIO open (just BAR0 + IOMMU) ────────
    eprintln!("\n  PHASE A: Minimal VFIO open (BAR0 + DMA only)");

    let fds = crate::ember_client::request_fds(&bdf).expect("ember fds");
    eprintln!("  ember: received VFIO fds for {bdf}");

    let device = VfioDevice::from_received(&bdf, fds).expect("VfioDevice::from_received");
    device.enable_bus_master().expect("enable bus_master");
    let dma = device.dma_backend();
    let bar0 = device.map_bar(0).expect("map BAR0");

    let boot0 = bar0.read_u32(0).unwrap_or(0);
    let pmc = bar0.read_u32(r144::PMC_ENABLE).unwrap_or(0);
    eprintln!("  BOOT0={boot0:#010x}  PMC_ENABLE={pmc:#010x}");

    let warm = pmc > 0x1000_0000;
    eprintln!("  GPU state: {}", if warm { "WARM (fabric alive)" } else { "COLD — this test needs warm GPU!" });
    if !warm {
        eprintln!("  *** ABORTING: GPU is cold. Swap to nouveau first, then swap back. ***");
        return;
    }

    eprintln!("\n  ── Raw Falcon State (no reset performed) ──");
    falcon_state(&bar0, "PMU", r144::PMU_BASE);
    falcon_state(&bar0, "SEC2", r144::SEC2_BASE);
    falcon_state(&bar0, "FECS", r144::FECS_BASE);
    falcon_state(&bar0, "GPCCS", r144::GPCCS_BASE);

    // ── Phase B: Clear PRI faults ONLY — NO GR reset ──────────
    eprintln!("\n  PHASE B: Clear PRI faults (NO GR reset — preserving GPCCS)");

    clear_pri_faults(&bar0);

    eprintln!("\n  Verifying GPCCS is accessible (GR not reset)...");
    let gpccs_ok = verify_gpccs_accessible(&bar0);
    if !gpccs_ok {
        eprintln!("  *** GPCCS already broken before ACR — something else damaged GR ***");
    }

    eprintln!("\n  Verifying FECS is accessible...");
    let fecs_cpu = bar0.read_u32(r144::FECS_BASE + r144::CPUCTL).unwrap_or(0xDEAD);
    let fecs_ok = fecs_cpu & 0xFFF00000 != 0xBAD00000 && fecs_cpu & 0xFFF00000 != 0xBADF0000;
    eprintln!("  FECS cpuctl={fecs_cpu:#010x} accessible={fecs_ok}");

    // ── Phase C: ACR Boot (VRAM-native — no GR reset, warm path) ────
    eprintln!("\n{eq}");
    eprintln!("  PHASE C: ACR Boot — VRAM-NATIVE (NO GR RESET — warm path)");
    eprintln!("  Hypothesis: sysmem DMA fails because ACR needs VRAM addresses.");
    eprintln!("  VRAM-native places EVERYTHING in VRAM, matching nouveau's approach.");
    eprintln!("{eq}");

    let fw = AcrFirmwareSet::load("gv100").expect("load firmware");

    let result = attempt_vram_native_acr_boot(&bar0, &fw, true);
    eprintln!("\n  {result}");
    for note in &result.notes {
        eprintln!("    {note}");
    }

    // ── Phase D: Check PRI + falcon state after ACR ────────────
    eprintln!("\n  PHASE D: Post-ACR diagnostics");

    let pri_status = bar0.read_u32(r144::PRIV_RING_INTR_STATUS).unwrap_or(0xDEAD);
    let pmc_intr = bar0.read_u32(r144::PMC_INTR).unwrap_or(0xDEAD);
    eprintln!("  PRI_RING_INTR_STATUS: {pri_status:#010x}");
    eprintln!("  PMC_INTR: {pmc_intr:#010x}");

    let gpccs_post_ok = verify_gpccs_accessible(&bar0);
    eprintln!("  GPCCS survived ACR: {gpccs_post_ok}");

    eprintln!("\n  ── Post-ACR Falcon State ──");
    falcon_state(&bar0, "SEC2", r144::SEC2_BASE);
    falcon_state(&bar0, "FECS", r144::FECS_BASE);
    falcon_state(&bar0, "GPCCS", r144::GPCCS_BASE);

    // ── Phase E: BOOTSTRAP_FALCON ─────────────────────────────
    eprintln!("\n  PHASE E: BOOTSTRAP_FALCON");

    let bootvecs = FalconBootvecOffsets {
        fecs: 0x7E00,
        gpccs: 0x3400,
    };
    let mb_result = attempt_acr_mailbox_command(&bar0, &bootvecs);
    eprintln!("\n  {mb_result}");
    for note in &mb_result.notes {
        eprintln!("    {note}");
    }

    eprintln!("\n  ── Final State ──");
    falcon_state(&bar0, "SEC2", r144::SEC2_BASE);
    falcon_state(&bar0, "FECS", r144::FECS_BASE);
    falcon_state(&bar0, "GPCCS", r144::GPCCS_BASE);

    let pri_final = bar0.read_u32(r144::PRIV_RING_INTR_STATUS).unwrap_or(0xDEAD);
    eprintln!("  PRI_RING final: {pri_final:#010x}");

    eprintln!("\n{eq}");
    eprintln!("#  Exp 144 COMPLETE — GR untouched, GPCCS survived={gpccs_post_ok}");
    eprintln!("{eq}");
}
