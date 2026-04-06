// SPDX-License-Identifier: AGPL-3.0-only
//! Exp 144: Warm ACR Boot — NO GR Reset — MMIO Gateway Architecture
//!
//! ALL GPU register operations route through ember RPCs.
//! No VFIO fd sharing, no local BAR0 mapping, no direct hardware access.
//!
//! HYPOTHESIS: The PRI ring fault in Exp 121 is caused by the PMC GR reset
//! (bit 12 toggle) which makes GPCCS inaccessible. When ACR firmware then
//! tries to copy authenticated firmware to GPCCS, it faults the PRI ring.
//!
//! This test skips the GR PMC reset entirely. On a warm GPU (after nouveau),
//! FECS/GPCCS should remain accessible. SEC2 is reset, loaded with ACR, and
//! started — all via ember RPCs.
//!
//! ```text
//! Run:
//! CORALREEF_VFIO_BDF=0000:03:00.0 RUST_LOG=info cargo test -p coral-driver \
//!   --features vfio --test hw_nv_vfio exp144 -- --ignored --nocapture --test-threads=1
//! ```

use crate::helpers::init_tracing;
use coral_driver::nv::vfio_compute::acr_boot::{
    AcrFirmwareSet, FalconBootvecOffsets, ParsedAcrFirmware,
    build_bl_dmem_desc, build_wpr, patch_acr_desc,
};
use std::io::Write;

const TRACE_PATH: &str = "/var/lib/coralreef/traces/exp144_trace.log";

fn trace(msg: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let line = format!("[{ts}] {msg}\n");
    eprintln!("  TRACE: {msg}");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(TRACE_PATH)
    {
        let _ = f.write_all(line.as_bytes());
        let _ = f.sync_all();
    }
}

mod r144 {
    pub const PMC_ENABLE: usize = 0x000200;
    pub const SEC2_BASE: usize = 0x087000;
    pub const FECS_BASE: usize = 0x409000;
    pub const GPCCS_BASE: usize = 0x41A000;
    pub const PMU_BASE: usize = 0x10A000;
    pub const CPUCTL: usize = 0x100;
    pub const SCTL: usize = 0x240;
    pub const PC: usize = 0x030;
    pub const EXCI: usize = 0x148;
    pub const HWCFG: usize = 0x108;
    pub const MAILBOX0: usize = 0x040;
    pub const MAILBOX1: usize = 0x044;
    pub const BOOTVEC: usize = 0x104;
    pub const FBIF_TRANSCFG: usize = 0x624;
    pub const PRIV_RING_COMMAND: usize = 0x12004C;
    pub const PRIV_RING_CMD_ACK: u32 = 0x02;
    pub const VRAM_ACR: u32 = 0x0005_0000;
    pub const VRAM_SHADOW: u64 = 0x0006_0000;
    pub const VRAM_WPR: u32 = 0x0008_0000;
}

fn ember_read(bdf: &str, offset: usize, label: &str) -> u32 {
    match crate::ember_client::mmio_read(bdf, offset) {
        Ok(val) => val,
        Err(e) => {
            trace(&format!("MMIO READ {label} @ {offset:#x} FAILED: {e}"));
            0xDEAD_DEAD
        }
    }
}

fn is_pri_fault(val: u32) -> bool {
    val & 0xFFF0_0000 == 0xBAD0_0000 || val & 0xFFF0_0000 == 0xBADF_0000
}

fn falcon_state_via_ember(bdf: &str, name: &str, base: usize) {
    let ops = vec![
        ("r", base + r144::CPUCTL, 0u32),
        ("r", base + r144::SCTL, 0),
        ("r", base + r144::PC, 0),
        ("r", base + r144::EXCI, 0),
        ("r", base + r144::MAILBOX0, 0),
    ];
    match crate::ember_client::mmio_batch(bdf, &ops) {
        Ok(results) => {
            let cpuctl = results.first().and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
            let sctl = results.get(1).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
            let pc = results.get(2).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
            let exci = results.get(3).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
            let mb0 = results.get(4).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
            let pri_err = is_pri_fault(cpuctl);
            let mode = match sctl { 0x3000 => "LS", 0x3002 => "HS", 0x7021 => "FW", _ => "??" };
            eprintln!(
                "  {name:6}: cpuctl={cpuctl:#010x} SCTL={sctl:#06x}({mode}) \
                 PC={pc:#06x} EXCI={exci:#010x} MB0={mb0:#010x}{}",
                if pri_err { " [PRI FAULT]" } else { "" }
            );
        }
        Err(e) => {
            eprintln!("  {name:6}: ember.mmio.batch failed: {e}");
        }
    }
}

fn verify_gpccs_accessible(bdf: &str) -> bool {
    let cpuctl = ember_read(bdf, r144::GPCCS_BASE + r144::CPUCTL, "GPCCS_CPUCTL");
    let is_err = is_pri_fault(cpuctl);
    if is_err {
        eprintln!("  *** GPCCS INACCESSIBLE: cpuctl={cpuctl:#010x} — GR fabric broken ***");
    } else {
        eprintln!("  GPCCS accessible: cpuctl={cpuctl:#010x}");
    }
    !is_err
}

#[test]
#[ignore = "requires VFIO-bound GPU hardware + GlowPlug + Ember + warm GPU"]
fn exp144_warm_acr_no_gr_reset() {
    init_tracing();
    let eq = "=".repeat(70);
    let bdf = crate::helpers::vfio_bdf();

    eprintln!("{eq}");
    eprintln!("#  Exp 144: Warm ACR Boot — NO GR Reset (MMIO Gateway)");
    eprintln!("#  ALL operations route through ember. No direct BAR0 access.");
    eprintln!("{eq}");

    let _ = std::fs::remove_file(TRACE_PATH);
    trace("exp144 STARTED (MMIO gateway mode)");

    // Experiment lifecycle
    struct ExperimentGuard { bdf: String }
    impl Drop for ExperimentGuard {
        fn drop(&mut self) {
            trace("LIFECYCLE: experiment_end");
            if let Ok(mut c) = crate::glowplug_client::GlowPlugClient::connect() {
                let _ = c.experiment_end(&self.bdf);
            }
        }
    }
    let _exp_guard = ExperimentGuard { bdf: bdf.clone() };

    if let Ok(mut gp) = crate::glowplug_client::GlowPlugClient::connect() {
        match gp.experiment_start(&bdf, "exp144_warm_acr", 120) {
            Ok(_) => eprintln!("  glowplug: health probes paused"),
            Err(e) => eprintln!("  WARNING: experiment_start failed: {e}"),
        }
    }

    // ── Phase A: GPU State ──
    eprintln!("\n  PHASE A: GPU State (via ember)");

    let boot0 = ember_read(&bdf, 0, "BOOT0");
    let pmc = ember_read(&bdf, r144::PMC_ENABLE, "PMC_ENABLE");
    eprintln!("  BOOT0={boot0:#010x}  PMC_ENABLE={pmc:#010x}");

    let warm = pmc > 0x1000_0000;
    eprintln!(
        "  GPU state: {}",
        if warm { "WARM (fabric alive)" } else { "COLD — needs warm GPU!" }
    );
    if !warm {
        trace("PHASE_A: GPU cold — aborting");
        eprintln!("  *** ABORTING: GPU is cold. Swap to nouveau first. ***");
        return;
    }

    eprintln!("\n  ── Raw Falcon State ──");
    falcon_state_via_ember(&bdf, "PMU", r144::PMU_BASE);
    falcon_state_via_ember(&bdf, "SEC2", r144::SEC2_BASE);
    falcon_state_via_ember(&bdf, "FECS", r144::FECS_BASE);
    falcon_state_via_ember(&bdf, "GPCCS", r144::GPCCS_BASE);

    // ── Phase B: Clear PRI faults — blind ACK only (NO 0x120058 read) ──
    eprintln!("\n  PHASE B: Clear PRI faults (NO GR reset, NO 0x120058 read)");
    trace("PHASE_B: blind PRI ACK via ember (skipping poisonous 0x120058)");

    // Blind-ACK PRI ring faults via write-only register
    if let Err(e) = crate::ember_client::mmio_write(&bdf, r144::PRIV_RING_COMMAND, r144::PRIV_RING_CMD_ACK) {
        eprintln!("  PRI ACK write failed: {e}");
    }
    std::thread::sleep(std::time::Duration::from_millis(20));
    eprintln!("  PRI ring: blind ACK sent (0x120058 read deliberately skipped — poisonous)");

    eprintln!("\n  Verifying GPCCS is accessible (GR not reset)...");
    let gpccs_ok = verify_gpccs_accessible(&bdf);
    if !gpccs_ok {
        eprintln!("  *** GPCCS broken before ACR — something else damaged GR ***");
    }

    let fecs_cpu = ember_read(&bdf, r144::FECS_BASE + r144::CPUCTL, "FECS_CPUCTL");
    let fecs_ok = !is_pri_fault(fecs_cpu);
    eprintln!("  FECS cpuctl={fecs_cpu:#010x} accessible={fecs_ok}");

    // ── Phase C: ACR Boot via ember MMIO gateway (VRAM-native) ──
    eprintln!("\n{eq}");
    eprintln!("  PHASE C: ACR Boot — VRAM-NATIVE via ember (NO GR RESET)");
    eprintln!("{eq}");
    trace("PHASE_C: ACR boot via ember MMIO gateway");

    // SEC2 prepare (reset + physical DMA)
    let (prepare_ok, prepare_notes) = match crate::ember_client::sec2_prepare_physical(&bdf) {
        Ok((ok, notes)) => (ok, notes),
        Err(e) => {
            trace(&format!("PHASE_C: sec2_prepare FAILED: {e}"));
            eprintln!("  *** sec2_prepare failed: {e} ***");
            return;
        }
    };
    for note in &prepare_notes {
        eprintln!("    {note}");
    }
    if !prepare_ok {
        trace("PHASE_C: sec2_prepare returned false");
        eprintln!("  *** SEC2 prepare failed ***");
        return;
    }

    // Load firmware
    let fw = AcrFirmwareSet::load("gv100").expect("load firmware");
    let parsed = ParsedAcrFirmware::parse(&fw).expect("parse firmware");
    let data_off = parsed.load_header.data_dma_base as usize;

    // Build and write WPR
    let wpr_data = build_wpr(&fw, r144::VRAM_WPR as u64);
    if let Err(e) = crate::ember_client::pramin_write(&bdf, r144::VRAM_WPR, &wpr_data) {
        trace(&format!("PHASE_C: WPR write failed: {e}"));
        eprintln!("  *** WPR write failed: {e} ***");
        return;
    }
    eprintln!("  WPR: {}B → VRAM {:#x}", wpr_data.len(), r144::VRAM_WPR);

    // Shadow copy
    if let Err(e) = crate::ember_client::pramin_write(&bdf, r144::VRAM_SHADOW as u32, &wpr_data) {
        eprintln!("  WARNING: shadow write failed: {e}");
    }

    // Patch and write ACR payload
    let mut payload = parsed.acr_payload.clone();
    let wpr_end = r144::VRAM_WPR as u64 + wpr_data.len() as u64;
    patch_acr_desc(&mut payload, data_off, r144::VRAM_WPR as u64, wpr_end, r144::VRAM_SHADOW);
    if data_off + 0x268 <= payload.len() {
        payload[data_off + 0x258..data_off + 0x25C].copy_from_slice(&0u32.to_le_bytes());
        payload[data_off + 0x260..data_off + 0x268].copy_from_slice(&0u64.to_le_bytes());
    }

    if let Err(e) = crate::ember_client::pramin_write(&bdf, r144::VRAM_ACR, &payload) {
        trace(&format!("PHASE_C: ACR payload write failed: {e}"));
        eprintln!("  *** ACR payload write failed: {e} ***");
        return;
    }
    eprintln!("  ACR payload: {}B → VRAM {:#x}", payload.len(), r144::VRAM_ACR);

    // Upload BL code + descriptor
    let base = r144::SEC2_BASE;
    let hwcfg = ember_read(&bdf, base + r144::HWCFG, "HWCFG");
    let code_limit = (hwcfg & 0x1FF) * 256;
    let boot_size = ((parsed.bl_desc.bl_code_off + parsed.bl_desc.bl_code_size + 0xFF) & !0xFF) as u32;
    let imem_addr = code_limit.saturating_sub(boot_size);
    let start_tag = parsed.bl_desc.bl_start_tag;
    let boot_addr = start_tag << 8;

    if let Err(e) = crate::ember_client::falcon_upload_imem(&bdf, base, imem_addr, &parsed.bl_code, start_tag, true) {
        trace(&format!("PHASE_C: IMEM upload failed: {e}"));
        eprintln!("  *** IMEM upload failed: {e} ***");
        return;
    }

    let code_dma_base = r144::VRAM_ACR as u64;
    let data_dma_base = r144::VRAM_ACR as u64 + data_off as u64;
    let bl_desc = build_bl_dmem_desc(code_dma_base, data_dma_base, &parsed);

    let data_section = &payload[data_off..];
    if let Err(e) = crate::ember_client::falcon_upload_dmem(&bdf, base, 0, data_section) {
        eprintln!("  *** DMEM data upload failed: {e} ***");
        return;
    }
    if let Err(e) = crate::ember_client::falcon_upload_dmem(&bdf, base, 0, &bl_desc) {
        eprintln!("  *** DMEM BL desc upload failed: {e} ***");
        return;
    }

    // Prepare DMA + boot
    if let Err(e) = crate::ember_client::prepare_dma(&bdf) {
        eprintln!("  *** prepare_dma failed: {e} ***");
        return;
    }

    let boot_ops = vec![
        ("w", base + r144::EXCI, 0u32),
        ("w", base + r144::MAILBOX0, 0xdead_a5a5_u32),
        ("w", base + r144::MAILBOX1, 0u32),
        ("w", base + r144::BOOTVEC, boot_addr),
    ];
    if let Err(e) = crate::ember_client::mmio_batch(&bdf, &boot_ops) {
        eprintln!("  *** boot register writes failed: {e} ***");
        return;
    }

    if let Err(e) = crate::ember_client::falcon_start_cpu(&bdf, base) {
        eprintln!("  *** falcon_start_cpu failed: {e} ***");
        return;
    }

    // Poll
    let poll_result = match crate::ember_client::falcon_poll(&bdf, base, 5000, 0xdead_a5a5) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("  *** falcon_poll failed: {e} ***");
            return;
        }
    };

    if let Some(snapshots) = poll_result.get("snapshots").and_then(|v| v.as_array()) {
        for snap in snapshots {
            let cpuctl = snap.get("cpuctl").and_then(|v| v.as_u64()).unwrap_or(0);
            let mb0 = snap.get("mailbox0").and_then(|v| v.as_u64()).unwrap_or(0);
            let pc = snap.get("pc").and_then(|v| v.as_u64()).unwrap_or(0);
            let sctl = snap.get("sctl").and_then(|v| v.as_u64()).unwrap_or(0);
            let elapsed = snap.get("elapsed_ms").and_then(|v| v.as_u64()).unwrap_or(0);
            let reason = snap.get("reason").and_then(|v| v.as_str()).unwrap_or("?");
            eprintln!(
                "  SEC2 {reason}: cpuctl={cpuctl:#010x} mb0={mb0:#010x} pc={pc:#06x} sctl={sctl:#010x} ({elapsed}ms)"
            );
        }
    }

    // ── Phase D: Post-ACR diagnostics ──
    eprintln!("\n  PHASE D: Post-ACR diagnostics");
    // NOTE: PRI_RING_INTR_STATUS (0x120058) deliberately NOT read — it's poisonous
    eprintln!("  PRI_RING_INTR: SKIPPED (0x120058 is poisonous)");

    let gpccs_post_ok = verify_gpccs_accessible(&bdf);
    eprintln!("  GPCCS survived ACR: {gpccs_post_ok}");

    eprintln!("\n  ── Post-ACR Falcon State ──");
    falcon_state_via_ember(&bdf, "SEC2", r144::SEC2_BASE);
    falcon_state_via_ember(&bdf, "FECS", r144::FECS_BASE);
    falcon_state_via_ember(&bdf, "GPCCS", r144::GPCCS_BASE);

    // ── Phase E: BOOTSTRAP_FALCON ──
    if let Some(final_state) = poll_result.get("final") {
        let sctl = final_state.get("sctl").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let hs = sctl & 0x02 != 0;
        let mb0 = final_state.get("mailbox0").and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;

        if hs && mb0 == 0 {
            eprintln!("\n  PHASE E: BOOTSTRAP_FALCON");
            let _bootvecs = FalconBootvecOffsets { fecs: 0x7E00, gpccs: 0x3400 };

            let bootstrap_ops = vec![
                ("w", r144::SEC2_BASE + r144::MAILBOX0, 0u32),
                ("w", r144::SEC2_BASE + r144::MAILBOX1, 0u32),
            ];
            if let Err(e) = crate::ember_client::mmio_batch(&bdf, &bootstrap_ops) {
                eprintln!("  BOOTSTRAP_FALCON mailbox clear failed: {e}");
            }

            eprintln!("\n  ── Final State ──");
            falcon_state_via_ember(&bdf, "SEC2", r144::SEC2_BASE);
            falcon_state_via_ember(&bdf, "FECS", r144::FECS_BASE);
            falcon_state_via_ember(&bdf, "GPCCS", r144::GPCCS_BASE);
        } else {
            eprintln!("\n  Skipping BOOTSTRAP_FALCON (ACR not in HS or errors)");
        }
    }

    // Cleanup
    match crate::ember_client::cleanup_dma(&bdf) {
        Ok(()) => eprintln!("  cleanup_dma: OK"),
        Err(e) => eprintln!("  WARNING: cleanup_dma failed: {e}"),
    }

    trace("exp144 COMPLETE (MMIO gateway mode)");
    eprintln!("\n{eq}");
    eprintln!("#  Exp 144 COMPLETE — GR untouched, GPCCS survived={gpccs_post_ok}");
    eprintln!("{eq}");
}
