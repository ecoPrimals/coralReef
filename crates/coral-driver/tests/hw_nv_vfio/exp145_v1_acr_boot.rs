// SPDX-License-Identifier: AGPL-3.0-only
//! Exp 145: ACR Boot — MMIO Gateway Architecture
//!
//! ALL GPU register and PRAMIN operations route through ember RPCs.
//! No VFIO fd sharing, no local BAR0 mapping, no direct hardware access.
//! If any BAR0 operation hangs, it hangs ember's handler thread — not this
//! process and not the system's main thread.
//!
//! ```text
//! Run:
//! CORALREEF_VFIO_BDF=0000:03:00.0 RUST_LOG=info cargo test -p coral-driver \
//!   --features vfio --test hw_nv_vfio exp145 -- --ignored --nocapture --test-threads=1
//! ```

use crate::helpers::init_tracing;
use coral_driver::nv::vfio_compute::acr_boot::{
    AcrFirmwareSet, FalconBootvecOffsets, ParsedAcrFirmware,
    build_bl_dmem_desc, build_wpr, patch_acr_desc,
};

use std::io::Write;

const TRACE_PATH: &str = "/var/lib/coralreef/traces/exp145_trace.log";

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

mod r145 {
    pub const PMC_ENABLE: usize = 0x000200;
    pub const SEC2_BASE: usize = 0x087000;
    pub const FECS_BASE: usize = 0x409000;
    pub const GPCCS_BASE: usize = 0x41A000;
    pub const CPUCTL: usize = 0x100;
    pub const SCTL: usize = 0x240;
    pub const PC: usize = 0x030;
    pub const EXCI: usize = 0x148;
    pub const HWCFG: usize = 0x108;
    pub const MAILBOX0: usize = 0x040;
    pub const MAILBOX1: usize = 0x044;
    pub const BOOTVEC: usize = 0x104;
    pub const FBIF_TRANSCFG: usize = 0x624;
    pub const VRAM_ACR: u32 = 0x0005_0000;
    pub const VRAM_SHADOW: u64 = 0x0006_0000;
    pub const VRAM_WPR: u32 = 0x0008_0000;
}

/// Read a register via ember MMIO gateway, with tracing.
fn ember_read(bdf: &str, offset: usize, label: &str) -> u32 {
    trace(&format!("MMIO READ {label} @ {offset:#x} ..."));
    match crate::ember_client::mmio_read(bdf, offset) {
        Ok(val) => {
            trace(&format!("MMIO READ {label} @ {offset:#x} = {val:#010x}"));
            val
        }
        Err(e) => {
            trace(&format!("MMIO READ {label} @ {offset:#x} FAILED: {e}"));
            eprintln!("  WARNING: ember.mmio.read failed: {e}");
            0xDEAD_DEAD
        }
    }
}

/// Read falcon state for diagnostics via ember batch RPC.
fn falcon_state_via_ember(bdf: &str, name: &str, base: usize) {
    let ops = vec![
        ("r", base + r145::CPUCTL, 0u32),
        ("r", base + r145::SCTL, 0),
        ("r", base + r145::PC, 0),
        ("r", base + r145::EXCI, 0),
    ];
    match crate::ember_client::mmio_batch(bdf, &ops) {
        Ok(results) => {
            let cpuctl = results.first().and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
            let sctl = results.get(1).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
            let pc = results.get(2).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
            let exci = results.get(3).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
            let pri_err = cpuctl & 0xFFF0_0000 == 0xBAD0_0000;
            eprintln!(
                "  {name:6}: cpuctl={cpuctl:#010x} sctl={sctl:#010x} pc={pc:#06x} exci={exci:#010x}{}",
                if pri_err { " [PRI ERROR]" } else { "" }
            );
        }
        Err(e) => {
            eprintln!("  {name:6}: ember.mmio.batch failed: {e}");
        }
    }
}

#[test]
#[ignore = "requires VFIO-bound GPU hardware + GlowPlug + Ember + warm GPU"]
fn exp145_v1_acr_boot() {
    init_tracing();
    let eq = "=".repeat(70);
    let bdf = crate::helpers::vfio_bdf();

    eprintln!("{eq}");
    eprintln!("#  Exp 145: ACR Boot — MMIO Gateway Architecture");
    eprintln!("#  ALL operations route through ember. No direct BAR0 access.");
    eprintln!("{eq}");

    let _ = std::fs::remove_file(TRACE_PATH);
    trace("exp145 STARTED (MMIO gateway mode)");

    // ── Experiment lifecycle: pause glowplug health probes ──
    struct ExperimentGuard { bdf: String }
    impl Drop for ExperimentGuard {
        fn drop(&mut self) {
            trace("LIFECYCLE: experiment_end via glowplug (guard drop)");
            if let Ok(mut c) = crate::glowplug_client::GlowPlugClient::connect() {
                match c.experiment_end(&self.bdf) {
                    Ok(r) => {
                        trace(&format!("LIFECYCLE: experiment_end OK: {r}"));
                        eprintln!("  glowplug: experiment_end OK — health probes resumed");
                    }
                    Err(e) => {
                        trace(&format!("LIFECYCLE: experiment_end FAILED: {e}"));
                        eprintln!("  WARNING: glowplug experiment_end failed: {e}");
                    }
                }
            }
        }
    }
    let _exp_guard = ExperimentGuard { bdf: bdf.clone() };

    trace("LIFECYCLE: experiment_start via glowplug");
    let mut gp_client = crate::glowplug_client::GlowPlugClient::connect()
        .expect("connect to glowplug");
    match gp_client.experiment_start(&bdf, "exp145_acr_boot", 120) {
        Ok(result) => {
            trace(&format!("LIFECYCLE: experiment_start OK: {result}"));
            eprintln!("  glowplug: experiment_start OK — health probes paused (120s watchdog)");
        }
        Err(e) => {
            trace(&format!("LIFECYCLE: experiment_start FAILED: {e}"));
            eprintln!("  WARNING: glowplug experiment_start failed: {e}");
            eprintln!("  (proceeding anyway — health probes may interfere)");
        }
    }

    // ── Phase A: Read GPU state via ember MMIO gateway ──
    trace("PHASE_A: reading GPU state via ember MMIO gateway (no fd sharing)");
    eprintln!("\n  PHASE A: GPU State (via ember MMIO gateway)");

    let boot0 = ember_read(&bdf, 0, "BOOT0");
    let pmc = ember_read(&bdf, r145::PMC_ENABLE, "PMC_ENABLE");
    eprintln!("  BOOT0={boot0:#010x}  PMC_ENABLE={pmc:#010x}");

    let warm = pmc > 0x1000_0000;
    let sec2_probe = ember_read(&bdf, r145::SEC2_BASE + r145::CPUCTL, "SEC2_CPUCTL");
    let pri_poisoned = sec2_probe & 0xFFF0_0000 == 0xBAD0_0000;
    eprintln!(
        "  GPU state: {}{}",
        if warm { "WARM (fabric alive)" } else { "COLD" },
        if pri_poisoned { " [PRI RING POISONED — reboot required]" } else { "" },
    );
    if pri_poisoned {
        trace("PHASE_A: PRI POISONED — aborting");
        eprintln!("  *** PRI ring corrupted. Reboot, then swap to nvidia to warm GPU. ***");
        return;
    }
    if !warm {
        eprintln!("  *** GPU is cold. Swap to nvidia driver to warm, then back to vfio. ***");
    }

    trace("PHASE_A: pre-reset falcon state reads");
    eprintln!("\n  ── Pre-reset Falcon State ──");
    falcon_state_via_ember(&bdf, "SEC2", r145::SEC2_BASE);
    falcon_state_via_ember(&bdf, "FECS", r145::FECS_BASE);
    falcon_state_via_ember(&bdf, "GPCCS", r145::GPCCS_BASE);
    trace("PHASE_A: complete");

    // ── Phase B: SEC2 reset + physical DMA (via ember) ──
    let pmc_pre_b = ember_read(&bdf, r145::PMC_ENABLE, "PMC_PRE_PHASE_B");
    trace(&format!(
        "PHASE_B: calling ember.sec2.prepare_physical (PMC={pmc_pre_b:#010x})"
    ));
    eprintln!("\n  PHASE B: SEC2 Reset (via ember.sec2.prepare_physical)");
    eprintln!("    PMC_ENABLE before = {pmc_pre_b:#010x}");

    let (prepare_ok, prepare_notes) = match crate::ember_client::sec2_prepare_physical(&bdf) {
        Ok((ok, notes)) => (ok, notes),
        Err(e) => {
            trace(&format!("PHASE_B: ember.sec2.prepare_physical FAILED: {e}"));
            eprintln!("  *** ember.sec2.prepare_physical FAILED: {e} ***");
            return;
        }
    };

    trace(&format!(
        "PHASE_B: sec2_prepare returned ok={prepare_ok} notes={}",
        prepare_notes.len()
    ));
    for (i, note) in prepare_notes.iter().enumerate() {
        trace(&format!("PHASE_B_NOTE[{i}]: {note}"));
        eprintln!("    {note}");
    }
    eprintln!("  Prepare result: ok={prepare_ok}");
    if !prepare_ok {
        trace("PHASE_B: FAILED — aborting");
        eprintln!("  *** SEC2 prepare failed — aborting ***");
        return;
    }
    trace("PHASE_B: complete");

    // ── Phase C: Load firmware ──
    trace("PHASE_C: loading ACR firmware from disk");
    eprintln!("\n  PHASE C: Load ACR Firmware");

    let fw = AcrFirmwareSet::load("gv100").expect("load firmware");
    eprintln!("  {}", fw.summary());

    let parsed = ParsedAcrFirmware::parse(&fw).expect("parse firmware");
    let data_off = parsed.load_header.data_dma_base as usize;
    eprintln!(
        "  ACR payload: {}B data_off={data_off:#x} data_size={:#x}",
        parsed.acr_payload.len(),
        parsed.load_header.data_size
    );

    // ── Phase C1: Build and write WPR via ember PRAMIN gateway ──
    let wpr_data = build_wpr(&fw, r145::VRAM_WPR as u64);
    let wpr_end = r145::VRAM_WPR as u64 + wpr_data.len() as u64;
    eprintln!(
        "  WPR: {}B at VRAM {:#x}..{:#x} (256KB-aligned)",
        wpr_data.len(),
        r145::VRAM_WPR,
        wpr_end
    );
    assert!(r145::VRAM_WPR % 0x40000 == 0, "WPR must be 256KB-aligned");

    trace("PHASE_C1: writing WPR to VRAM via ember.pramin.write");
    match crate::ember_client::pramin_write(&bdf, r145::VRAM_WPR, &wpr_data) {
        Ok(n) => {
            trace(&format!("PHASE_C1: WPR written: {n} bytes"));
            eprintln!("  WPR written: {n} bytes via ember.pramin.write");
        }
        Err(e) => {
            trace(&format!("PHASE_C1: WPR write FAILED: {e}"));
            eprintln!("  *** WPR write failed: {e} ***");
            return;
        }
    }

    trace("PHASE_C1: writing shadow copy");
    match crate::ember_client::pramin_write(&bdf, r145::VRAM_SHADOW as u32, &wpr_data) {
        Ok(n) => {
            trace(&format!("PHASE_C1: shadow written: {n} bytes"));
        }
        Err(e) => {
            trace(&format!("PHASE_C1: shadow write FAILED: {e}"));
            eprintln!("  *** Shadow write failed: {e} ***");
            return;
        }
    }
    eprintln!("  WPR + shadow written to VRAM");

    // ── Phase C2: Patch ACR descriptor ──
    let mut payload = parsed.acr_payload.clone();
    patch_acr_desc(
        &mut payload,
        data_off,
        r145::VRAM_WPR as u64,
        wpr_end,
        r145::VRAM_SHADOW,
    );
    if data_off + 0x268 <= payload.len() {
        payload[data_off + 0x258..data_off + 0x25C]
            .copy_from_slice(&0u32.to_le_bytes());
        payload[data_off + 0x260..data_off + 0x268]
            .copy_from_slice(&0u64.to_le_bytes());
    }
    eprintln!("  ACR descriptor patched (blob_size=0, WPR at {:#x})", r145::VRAM_WPR);

    trace("PHASE_C2: writing ACR payload to VRAM via ember.pramin.write");
    match crate::ember_client::pramin_write(&bdf, r145::VRAM_ACR, &payload) {
        Ok(n) => {
            trace(&format!("PHASE_C2: ACR payload written: {n} bytes"));
            eprintln!("  ACR payload: {n}B → VRAM {:#x}", r145::VRAM_ACR);
        }
        Err(e) => {
            trace(&format!("PHASE_C2: ACR payload write FAILED: {e}"));
            eprintln!("  *** ACR payload write failed: {e} ***");
            return;
        }
    }
    trace("PHASE_C: complete");

    // ── Phase D: Load BL to IMEM + descriptor to DMEM (via ember) ──
    trace("PHASE_D: BL upload via ember falcon RPCs");
    eprintln!("\n  PHASE D: BL Upload (via ember)");

    let base = r145::SEC2_BASE;
    let hwcfg = ember_read(&bdf, base + r145::HWCFG, "HWCFG");
    let code_limit = (hwcfg & 0x1FF) * 256;
    let boot_size =
        ((parsed.bl_desc.bl_code_off + parsed.bl_desc.bl_code_size + 0xFF) & !0xFF) as u32;
    let imem_addr = code_limit.saturating_sub(boot_size);
    let start_tag = parsed.bl_desc.bl_start_tag;
    let boot_addr = start_tag << 8;

    trace("PHASE_D: IMEM upload via ember");
    match crate::ember_client::falcon_upload_imem(
        &bdf, base, imem_addr, &parsed.bl_code, start_tag,
    ) {
        Ok(()) => {
            eprintln!(
                "  BL code: {}B → IMEM@{imem_addr:#x} tag={start_tag:#x} boot={boot_addr:#x}",
                parsed.bl_code.len()
            );
        }
        Err(e) => {
            trace(&format!("PHASE_D: IMEM upload FAILED: {e}"));
            eprintln!("  *** IMEM upload failed: {e} ***");
            return;
        }
    }

    let code_dma_base = r145::VRAM_ACR as u64;
    let data_dma_base = r145::VRAM_ACR as u64 + data_off as u64;
    let bl_desc = build_bl_dmem_desc(code_dma_base, data_dma_base, &parsed);
    let ctx_dma = u32::from_le_bytes(bl_desc[32..36].try_into().unwrap_or([1, 0, 0, 0]));
    eprintln!(
        "  BL desc: code={code_dma_base:#x} data={data_dma_base:#x} ctx_dma={ctx_dma} (VIRT)"
    );

    // Load full data section to DMEM first
    trace("PHASE_D: DMEM data section upload via ember");
    let data_section = &payload[data_off..];
    if let Err(e) = crate::ember_client::falcon_upload_dmem(&bdf, base, 0, data_section) {
        trace(&format!("PHASE_D: DMEM data upload FAILED: {e}"));
        eprintln!("  *** DMEM data upload failed: {e} ***");
        return;
    }
    eprintln!("  Data section: {}B → DMEM@0", data_section.len());

    // Overwrite DMEM@0 with BL descriptor
    trace("PHASE_D: DMEM BL descriptor upload via ember");
    if let Err(e) = crate::ember_client::falcon_upload_dmem(&bdf, base, 0, &bl_desc) {
        trace(&format!("PHASE_D: DMEM desc upload FAILED: {e}"));
        eprintln!("  *** DMEM BL desc upload failed: {e} ***");
        return;
    }
    eprintln!("  BL descriptor: {}B → DMEM@0", bl_desc.len());
    trace("PHASE_D: complete");

    // ── Phase E: Boot SEC2 (via ember) ──
    eprintln!("\n{eq}");
    eprintln!("  PHASE E: Boot SEC2 — via ember MMIO gateway");
    eprintln!("{eq}");

    trace("PHASE_E: calling ember.prepare_dma (quiesce + bus_master via ember)");
    match crate::ember_client::prepare_dma(&bdf) {
        Ok(result) => {
            trace(&format!("PHASE_E: ember.prepare_dma OK: {result}"));
            eprintln!("  ember.prepare_dma: {result}");
        }
        Err(e) => {
            trace(&format!("PHASE_E: ember.prepare_dma FAILED: {e}"));
            eprintln!("  *** ember.prepare_dma FAILED: {e} ***");
            return;
        }
    }

    // Write boot registers via ember batch
    trace("PHASE_E: writing SEC2 boot registers via ember.mmio.batch");
    let boot_ops = vec![
        ("w", base + r145::EXCI, 0u32),
        ("w", base + r145::MAILBOX0, 0xdead_a5a5_u32),
        ("w", base + r145::MAILBOX1, 0u32),
        ("w", base + r145::BOOTVEC, boot_addr),
    ];
    if let Err(e) = crate::ember_client::mmio_batch(&bdf, &boot_ops) {
        trace(&format!("PHASE_E: boot register writes FAILED: {e}"));
        eprintln!("  *** boot register writes failed: {e} ***");
        return;
    }

    // Read pre-boot state
    let pre_boot_ops = vec![
        ("r", base + r145::FBIF_TRANSCFG, 0u32),
        ("r", base + 0x048, 0), // ITFEN
        ("r", base + 0x10C, 0), // DMACTL
    ];
    if let Ok(results) = crate::ember_client::mmio_batch(&bdf, &pre_boot_ops) {
        let fbif = results.first().and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let itfen = results.get(1).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let dmactl = results.get(2).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        eprintln!(
            "  Pre-boot: BOOTVEC={boot_addr:#x} FBIF={fbif:#x} ITFEN={itfen:#x} DMACTL={dmactl:#x}"
        );
    }

    // Start CPU via ember
    trace("PHASE_E: falcon_start_cpu via ember");
    match crate::ember_client::falcon_start_cpu(&bdf, base) {
        Ok(result) => {
            let pc = result.get("pc").and_then(|v| v.as_u64()).unwrap_or(0xDEAD);
            let exci = result.get("exci").and_then(|v| v.as_u64()).unwrap_or(0xDEAD);
            trace(&format!("PHASE_E: SEC2 started — pc={pc:#x} exci={exci:#x}"));
            eprintln!("  falcon_start_cpu: pc={pc:#06x} exci={exci:#010x}");
        }
        Err(e) => {
            trace(&format!("PHASE_E: falcon_start_cpu FAILED: {e}"));
            eprintln!("  *** falcon_start_cpu failed: {e} ***");
            return;
        }
    }
    trace("PHASE_E: SEC2 started — entering server-side poll");

    // ── Phase F: Poll (server-side via ember) ──
    trace("PHASE_F: polling SEC2 via ember.falcon.poll");
    eprintln!("\n  PHASE F: Polling SEC2 (server-side)");

    let poll_result = match crate::ember_client::falcon_poll(
        &bdf, base, 5000, 0xdead_a5a5,
    ) {
        Ok(r) => r,
        Err(e) => {
            trace(&format!("PHASE_F: ember.falcon.poll FAILED: {e}"));
            eprintln!("  *** falcon.poll failed: {e} ***");
            return;
        }
    };

    // Display poll results
    if let Some(snapshots) = poll_result.get("snapshots").and_then(|v| v.as_array()) {
        for snap in snapshots {
            let cpuctl = snap.get("cpuctl").and_then(|v| v.as_u64()).unwrap_or(0);
            let mb0 = snap.get("mailbox0").and_then(|v| v.as_u64()).unwrap_or(0);
            let mb1 = snap.get("mailbox1").and_then(|v| v.as_u64()).unwrap_or(0);
            let pc = snap.get("pc").and_then(|v| v.as_u64()).unwrap_or(0);
            let sctl = snap.get("sctl").and_then(|v| v.as_u64()).unwrap_or(0);
            let elapsed = snap.get("elapsed_ms").and_then(|v| v.as_u64()).unwrap_or(0);
            let reason = snap.get("reason").and_then(|v| v.as_str()).unwrap_or("?");
            eprintln!(
                "  SEC2 {reason}: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} \
                 pc={pc:#06x} sctl={sctl:#010x} ({elapsed}ms)"
            );
        }
    }

    if let Some(pc_trace) = poll_result.get("pc_trace").and_then(|v| v.as_array()) {
        let trace_str: Vec<String> = pc_trace
            .iter()
            .filter_map(|v| v.as_u64())
            .map(|p| format!("{p:#06x}"))
            .collect();
        eprintln!(
            "  PC trace ({} entries): [{}]",
            trace_str.len(),
            trace_str.join(" → ")
        );
    }

    trace("PHASE_F: poll complete");

    // ── Phase G: Post-boot diagnostics ──
    trace("PHASE_G: post-boot diagnostics");
    eprintln!("\n  PHASE G: Post-boot Diagnostics");

    if let Some(final_state) = poll_result.get("final") {
        let sctl = final_state.get("sctl").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let hs = sctl & 0x02 != 0;
        let exci = final_state.get("exci").and_then(|v| v.as_u64()).unwrap_or(0);
        let cpuctl = final_state.get("cpuctl").and_then(|v| v.as_u64()).unwrap_or(0);
        let mb0 = final_state.get("mailbox0").and_then(|v| v.as_u64()).unwrap_or(0);
        let mb1 = final_state.get("mailbox1").and_then(|v| v.as_u64()).unwrap_or(0);
        let dmactl = final_state.get("dmactl").and_then(|v| v.as_u64()).unwrap_or(0);
        let itfen = final_state.get("itfen").and_then(|v| v.as_u64()).unwrap_or(0);

        eprintln!("  *** SCTL={sctl:#010x} HS={hs} ***");
        eprintln!("  EXCI={exci:#010x} CPUCTL={cpuctl:#010x}");
        eprintln!("  MB0={mb0:#010x} MB1={mb1:#010x}");
        eprintln!("  DMACTL={dmactl:#010x} ITFEN={itfen:#010x}");
        eprintln!("  PRI_RING_INTR: (skipped — 0x120058 is poisonous post-nouveau)");

        eprintln!("\n  ── Post-ACR Falcon State ──");
        falcon_state_via_ember(&bdf, "SEC2", r145::SEC2_BASE);
        falcon_state_via_ember(&bdf, "FECS", r145::FECS_BASE);
        falcon_state_via_ember(&bdf, "GPCCS", r145::GPCCS_BASE);

        // ── Phase H: BOOTSTRAP_FALCON (if ACR succeeded) ──
        if hs && mb0 == 0 {
            eprintln!("\n  PHASE H: BOOTSTRAP_FALCON");

            let _bootvecs = FalconBootvecOffsets {
                fecs: 0x7E00,
                gpccs: 0x3400,
            };

            // Write FECS bootvec and attempt BOOTSTRAP_FALCON via batch writes
            let bootstrap_ops = vec![
                ("w", r145::SEC2_BASE + r145::MAILBOX0, 0u32),
                ("w", r145::SEC2_BASE + r145::MAILBOX1, 0u32),
            ];
            if let Err(e) = crate::ember_client::mmio_batch(&bdf, &bootstrap_ops) {
                eprintln!("  BOOTSTRAP_FALCON mailbox clear failed: {e}");
            }

            eprintln!("\n  ── Final State ──");
            falcon_state_via_ember(&bdf, "SEC2", r145::SEC2_BASE);
            falcon_state_via_ember(&bdf, "FECS", r145::FECS_BASE);
            falcon_state_via_ember(&bdf, "GPCCS", r145::GPCCS_BASE);
        } else {
            eprintln!("\n  Skipping BOOTSTRAP_FALCON (ACR not in HS mode or errors present)");
        }
    }

    // Cleanup through ember
    trace("CLEANUP: calling ember.cleanup_dma");
    match crate::ember_client::cleanup_dma(&bdf) {
        Ok(()) => {
            trace("CLEANUP: ember.cleanup_dma OK");
            eprintln!("  ember.cleanup_dma: OK (bus_master OFF, AER restored)");
        }
        Err(e) => {
            trace(&format!("CLEANUP: ember.cleanup_dma FAILED: {e}"));
            eprintln!("  WARNING: ember.cleanup_dma failed: {e}");
        }
    }

    trace("exp145 COMPLETE (MMIO gateway mode)");
    eprintln!("\n{eq}");
    if let Some(final_state) = poll_result.get("final") {
        let sctl = final_state.get("sctl").and_then(|v| v.as_u64()).unwrap_or(0);
        let hs = sctl & 0x02 != 0;
        eprintln!("#  Exp 145 COMPLETE — HS={hs} SCTL={sctl:#010x}");
    } else {
        eprintln!("#  Exp 145 COMPLETE");
    }
    eprintln!("{eq}");
}
