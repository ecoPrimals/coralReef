// SPDX-License-Identifier: AGPL-3.0-only

//! Exp 121: Minimal ACR Boot — No PFIFO, No GR Init, No Channel
//!
//! HYPOTHESIS: The `open_vfio()` full device init (PFIFO, GR BAR0 writes,
//! FECS channel methods) poisons GPU state with PRI faults that prevent
//! ACR's copy-to-target from working.
//!
//! This test uses only VfioDevice (BAR0 + IOMMU DMA) with NO engine init.
//! Just clear PRI faults → PMC GR reset → ACR boot.
//!
//! Run:
//! ```sh
//! CORALREEF_VFIO_BDF=0000:03:00.0 RUST_LOG=info cargo test -p coral-driver \
//!   --features vfio --test hw_nv_vfio exp121 -- --ignored --nocapture --test-threads=1
//! ```

use crate::helpers::init_tracing;
use coral_driver::nv::vfio_compute::acr_boot::{
    AcrFirmwareSet, BootConfig, FalconBootvecOffsets, attempt_acr_mailbox_command,
    attempt_sysmem_acr_boot_with_config,
};
use coral_driver::vfio::device::{MappedBar, VfioDevice};

mod r121 {
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
    let cpuctl = bar0.read_u32(base + r121::CPUCTL).unwrap_or(0xDEAD);
    let sctl = bar0.read_u32(base + r121::SCTL).unwrap_or(0xDEAD);
    let pc = bar0.read_u32(base + r121::PC).unwrap_or(0xDEAD);
    let exci = bar0.read_u32(base + r121::EXCI).unwrap_or(0xDEAD);
    let mb0 = bar0.read_u32(base + r121::MAILBOX0).unwrap_or(0xDEAD);
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
    let status = bar0.read_u32(r121::PRIV_RING_INTR_STATUS).unwrap_or(0);
    eprintln!("  PRI ring status: {status:#010x}");
    if status != 0 {
        for i in 0..10 {
            let _ = bar0.write_u32(r121::PRIV_RING_COMMAND, r121::PRIV_RING_CMD_ACK);
            std::thread::sleep(std::time::Duration::from_millis(20));
            let s = bar0.read_u32(r121::PRIV_RING_INTR_STATUS).unwrap_or(0);
            if s == 0 {
                eprintln!("  PRI faults cleared (attempt {i})");
                return;
            }
        }
        let s = bar0.read_u32(r121::PRIV_RING_INTR_STATUS).unwrap_or(0);
        eprintln!("  PRI faults still pending: {s:#010x}");
    } else {
        eprintln!("  PRI ring clean — no faults");
    }
}

fn pmc_gr_reset(bar0: &MappedBar) {
    let pmc = bar0.read_u32(r121::PMC_ENABLE).unwrap_or(0);
    eprintln!(
        "  PMC_ENABLE before: {pmc:#010x} (GR bit={:?})",
        pmc & r121::GR_ENGINE_BIT != 0
    );

    let _ = bar0.write_u32(r121::PMC_ENABLE, pmc & !r121::GR_ENGINE_BIT);
    std::thread::sleep(std::time::Duration::from_millis(10));
    let _ = bar0.write_u32(r121::PMC_ENABLE, pmc | r121::GR_ENGINE_BIT);
    std::thread::sleep(std::time::Duration::from_millis(10));

    let pmc2 = bar0.read_u32(r121::PMC_ENABLE).unwrap_or(0);
    eprintln!("  PMC_ENABLE after reset: {pmc2:#010x}");
}

#[test]
#[ignore = "requires VFIO-bound GPU hardware + GlowPlug + Ember"]
fn exp121_minimal_acr() {
    init_tracing();
    let eq = "=".repeat(70);
    let bdf = crate::helpers::vfio_bdf();

    eprintln!("{eq}");
    eprintln!("#  Exp 121: Minimal ACR Boot — No PFIFO, No GR Init");
    eprintln!("#  HYPOTHESIS: Full device init poisons PRI ring / GR fabric");
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
    eprintln!("  BOOT0={boot0:#010x}");

    eprintln!("\n  ── Raw Falcon State (zero init) ──");
    falcon_state(&bar0, "PMU", r121::PMU_BASE);
    falcon_state(&bar0, "SEC2", r121::SEC2_BASE);
    falcon_state(&bar0, "FECS", r121::FECS_BASE);
    falcon_state(&bar0, "GPCCS", r121::GPCCS_BASE);

    // ── Phase B: Clear PRI faults + PMC GR reset ──────────────
    eprintln!("\n  PHASE B: Clear PRI faults + PMC GR reset");

    clear_pri_faults(&bar0);

    eprintln!("\n  Performing PMC GR engine reset...");
    pmc_gr_reset(&bar0);

    eprintln!("\n  Re-checking PRI after GR reset...");
    clear_pri_faults(&bar0);

    eprintln!("\n  ── Post-Reset Falcon State ──");
    falcon_state(&bar0, "PMU", r121::PMU_BASE);
    falcon_state(&bar0, "SEC2", r121::SEC2_BASE);
    falcon_state(&bar0, "FECS", r121::FECS_BASE);
    falcon_state(&bar0, "GPCCS", r121::GPCCS_BASE);

    // ── Phase C: ACR Boot (minimal — no PFIFO, no channel) ────
    eprintln!("\n{eq}");
    eprintln!("  PHASE C: ACR Boot (minimal path)");
    eprintln!("{eq}");

    let fw = AcrFirmwareSet::load("gv100").expect("load firmware");

    let config = BootConfig {
        pde_upper: true,
        acr_vram_pte: false,
        blob_size_zero: true,
        bind_vram: false,
        imem_preload: false,
        tlb_invalidate: true,
    };

    let result = attempt_sysmem_acr_boot_with_config(&bar0, &fw, dma.clone(), &config);
    eprintln!("\n  {result}");
    for note in &result.notes {
        eprintln!("    {note}");
    }

    // ── Phase D: Check FBHUB + PRI state after ACR ────────────
    eprintln!("\n  PHASE D: Post-ACR diagnostics");

    let pri_status = bar0.read_u32(r121::PRIV_RING_INTR_STATUS).unwrap_or(0xDEAD);
    let pmc_intr = bar0.read_u32(r121::PMC_INTR).unwrap_or(0xDEAD);
    eprintln!("  PRI_RING_INTR_STATUS: {pri_status:#010x}");
    eprintln!("  PMC_INTR: {pmc_intr:#010x}");

    eprintln!("\n  ── Post-ACR Falcon State ──");
    falcon_state(&bar0, "SEC2", r121::SEC2_BASE);
    falcon_state(&bar0, "FECS", r121::FECS_BASE);
    falcon_state(&bar0, "GPCCS", r121::GPCCS_BASE);

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
    falcon_state(&bar0, "SEC2", r121::SEC2_BASE);
    falcon_state(&bar0, "FECS", r121::FECS_BASE);
    falcon_state(&bar0, "GPCCS", r121::GPCCS_BASE);

    let pri_final = bar0.read_u32(r121::PRIV_RING_INTR_STATUS).unwrap_or(0xDEAD);
    eprintln!("  PRI_RING final: {pri_final:#010x}");

    eprintln!("\n{eq}");
    eprintln!("#  Exp 121 COMPLETE");
    eprintln!("{eq}");
}
