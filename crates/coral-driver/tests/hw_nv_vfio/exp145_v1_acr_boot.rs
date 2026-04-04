// SPDX-License-Identifier: AGPL-3.0-only
//! Exp 145: ACR Boot with Correct Falcon v1 Register Interface
//!
//! ROOT CAUSE FIX: All prior ACR experiments (121, 144) used the gm200-era
//! falcon binding registers (0x054, 0x604) which are WRONG for GV100 SEC2.
//!
//! Nouveau's GP102+/GV100 SEC2 uses the falcon v1 interface:
//!   - Instance block at register 0x480 (not 0x054)
//!   - ITFEN bits [5:4] for DMA control (not DMAIDX at 0x604)
//!   - SEC2 debug register at 0x408 (not 0x084)
//!   - Reset: MC disable → ENGCTL toggle → MC enable (not ENGCTL → PMC reset)
//!
//! WPR is placed at 0x80000 (256KB-aligned, matching nouveau's 0x40000 alignment).
//!
//! ```text
//! Run:
//! CORALREEF_VFIO_BDF=0000:03:00.0 RUST_LOG=info cargo test -p coral-driver \
//!   --features vfio --test hw_nv_vfio exp145 -- --ignored --nocapture --test-threads=1
//! ```

use crate::helpers::init_tracing;
use coral_driver::nv::vfio_compute::acr_boot::{
    AcrFirmwareSet, FalconBootvecOffsets, ParsedAcrFirmware,
    attempt_acr_mailbox_command, build_bl_dmem_desc, build_wpr,
    falcon_dmem_upload, falcon_imem_upload_nouveau, falcon_start_cpu,
    patch_acr_desc, sec2_prepare_v1,
};
use coral_driver::vfio::device::{MappedBar, VfioDevice};
use coral_driver::vfio::memory::{MemoryRegion, PraminRegion};

use std::io::Write;

const TRACE_PATH: &str = "/tmp/exp145_trace.log";

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

fn trace_read(bar0: &MappedBar, offset: usize, label: &str) -> u32 {
    trace(&format!("BAR0 READ {label} @ {offset:#x} ..."));
    let val = bar0.read_u32(offset).unwrap_or(0xDEAD_DEAD);
    trace(&format!("BAR0 READ {label} @ {offset:#x} = {val:#010x}"));
    val
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
    pub const DMACTL: usize = 0x10C;
    pub const MAILBOX0: usize = 0x040;
    pub const MAILBOX1: usize = 0x044;
    pub const BOOTVEC: usize = 0x104;
    pub const ITFEN: usize = 0x048;
    pub const CPUCTL_HALTED: u32 = 1 << 4;
    pub const CPUCTL_HRESET: u32 = 1 << 4;
    pub const FBIF_TRANSCFG: usize = 0x624;
    pub const VRAM_ACR: u32 = 0x0005_0000;
    pub const VRAM_SHADOW: u64 = 0x0006_0000;
    // 256KB-aligned WPR (fixes prior 0x70000 misalignment)
    pub const VRAM_WPR: u32 = 0x0008_0000;
}

fn falcon_state(bar0: &MappedBar, name: &str, base: usize) {
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xBAD0_DEAD);
    let cpuctl = r(r145::CPUCTL);
    let sctl = r(r145::SCTL);
    let pc = r(r145::PC);
    let exci = r(r145::EXCI);
    let pri_err = cpuctl & 0xFFF0_0000 == 0xBAD0_0000;
    eprintln!(
        "  {name:6}: cpuctl={cpuctl:#010x} sctl={sctl:#010x} pc={pc:#06x} exci={exci:#010x}{}",
        if pri_err { " [PRI ERROR]" } else { "" }
    );
}

fn write_to_vram(bar0: &MappedBar, vram_addr: u32, data: &[u8]) -> bool {
    for (chunk_idx, chunk) in data.chunks(4096).enumerate() {
        let offset = chunk_idx * 4096;
        match PraminRegion::new(bar0, vram_addr + offset as u32, chunk.len()) {
            Ok(mut rgn) => {
                for (i, word) in chunk.chunks(4).enumerate() {
                    let val = match word.len() {
                        4 => u32::from_le_bytes([word[0], word[1], word[2], word[3]]),
                        3 => u32::from_le_bytes([word[0], word[1], word[2], 0]),
                        2 => u32::from_le_bytes([word[0], word[1], 0, 0]),
                        1 => u32::from_le_bytes([word[0], 0, 0, 0]),
                        _ => 0,
                    };
                    if rgn.write_u32(i * 4, val).is_err() {
                        return false;
                    }
                }
            }
            Err(_) => return false,
        }
    }
    true
}

#[test]
#[ignore = "requires VFIO-bound GPU hardware + GlowPlug + Ember + warm GPU"]
fn exp145_v1_acr_boot() {
    init_tracing();
    let eq = "=".repeat(70);
    let bdf = crate::helpers::vfio_bdf();

    eprintln!("{eq}");
    eprintln!("#  Exp 145: ACR Boot — Falcon v1 Register Interface");
    eprintln!("#  ROOT CAUSE FIX: v1 binding (0x480) + ITFEN [5:4] + aligned WPR");
    eprintln!("{eq}");

    // Clear previous trace
    let _ = std::fs::remove_file(TRACE_PATH);
    trace("exp145 STARTED");

    // ── Experiment lifecycle: pause glowplug health probes ──
    // ExperimentGuard auto-calls experiment_end on drop (even on early abort/panic).
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

    // ── Phase A: Open VFIO device ──
    trace("PHASE_A: VFIO open — requesting fds from ember");
    eprintln!("\n  PHASE A: VFIO open");

    let fds = crate::ember_client::request_fds(&bdf).expect("ember fds");
    trace("PHASE_A: ember fds received");
    eprintln!("  ember: received VFIO fds for {bdf}");

    let device = VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    trace("PHASE_A: VfioDevice constructed (bus_master OFF)");
    eprintln!("  bus_master OFF (opt-in — will enable before SEC2 DMA)");

    let bar0 = device.map_bar(0).expect("map BAR0");
    trace("PHASE_A: BAR0 mapped — first reads next");

    let boot0 = trace_read(&bar0, 0, "BOOT0");
    let pmc = trace_read(&bar0, r145::PMC_ENABLE, "PMC_ENABLE");
    eprintln!("  BOOT0={boot0:#010x}  PMC_ENABLE={pmc:#010x}");

    let warm = pmc > 0x1000_0000;
    let sec2_probe = trace_read(&bar0, r145::SEC2_BASE + r145::CPUCTL, "SEC2_CPUCTL");
    let pri_poisoned = sec2_probe & 0xFFF0_0000 == 0xBAD0_0000;
    eprintln!(
        "  GPU state: {}{}",
        if warm { "WARM (fabric alive)" } else { "COLD" },
        if pri_poisoned { " [PRI RING POISONED — reboot required]" } else { "" },
    );
    if pri_poisoned {
        trace("PHASE_A: PRI POISONED — aborting");
        eprintln!("  *** PRI ring corrupted. Reboot, then swap to nvidia to warm GPU. ***");
        eprintln!("  *** DO NOT enable memory engines (PMC bits 16-28) on cold GPU! ***");
        return;
    }
    if !warm {
        eprintln!("  *** GPU is cold. Swap to nvidia driver to warm, then back to vfio. ***");
    }

    trace("PHASE_A: pre-reset falcon state reads");
    eprintln!("\n  ── Pre-reset Falcon State ──");
    trace("PHASE_A: reading SEC2 falcon state");
    falcon_state(&bar0, "SEC2", r145::SEC2_BASE);
    trace("PHASE_A: reading FECS falcon state");
    falcon_state(&bar0, "FECS", r145::FECS_BASE);
    trace("PHASE_A: reading GPCCS falcon state");
    falcon_state(&bar0, "GPCCS", r145::GPCCS_BASE);
    trace("PHASE_A: complete");

    // ── Phase B: SEC2 reset + v1 bind ──
    trace("PHASE_B: SEC2 reset — sec2_prepare_v1");
    eprintln!("\n  PHASE B: SEC2 Reset (v1 interface — matching nouveau exactly)");

    let (prepare_ok, prepare_notes) = sec2_prepare_v1(&bar0);
    for note in &prepare_notes {
        eprintln!("    {note}");
    }
    eprintln!("  Prepare result: ok={prepare_ok}");
    if !prepare_ok {
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

    // ── Phase C1: Build and write WPR (256KB-aligned at 0x80000) ──
    let wpr_data = build_wpr(&fw, r145::VRAM_WPR as u64);
    let wpr_end = r145::VRAM_WPR as u64 + wpr_data.len() as u64;
    eprintln!(
        "  WPR: {}B at VRAM {:#x}..{:#x} (256KB-aligned)",
        wpr_data.len(),
        r145::VRAM_WPR,
        wpr_end
    );
    assert!(
        r145::VRAM_WPR % 0x40000 == 0,
        "WPR must be 256KB-aligned"
    );

    // Write WPR to VRAM
    trace("PHASE_C1: writing WPR to VRAM via PRAMIN");
    assert!(write_to_vram(&bar0, r145::VRAM_WPR, &wpr_data), "WPR write failed");
    // Write shadow copy
    assert!(
        write_to_vram(&bar0, r145::VRAM_SHADOW as u32, &wpr_data),
        "Shadow write failed"
    );
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
    // Set blob_size=0 to skip ACR blob DMA (WPR already in VRAM)
    if data_off + 0x268 <= payload.len() {
        payload[data_off + 0x258..data_off + 0x25C]
            .copy_from_slice(&0u32.to_le_bytes());
        payload[data_off + 0x260..data_off + 0x268]
            .copy_from_slice(&0u64.to_le_bytes());
    }
    eprintln!("  ACR descriptor patched (blob_size=0, WPR at {:#x})", r145::VRAM_WPR);

    // Write ACR payload to VRAM
    trace("PHASE_C2: writing ACR payload to VRAM via PRAMIN");
    assert!(
        write_to_vram(&bar0, r145::VRAM_ACR, &payload),
        "ACR payload write failed"
    );
    eprintln!("  ACR payload: {}B → VRAM {:#x}", payload.len(), r145::VRAM_ACR);
    trace("PHASE_C: complete");

    // ── Phase D: Load BL to IMEM + descriptor to DMEM ──
    trace("PHASE_D: BL upload to IMEM/DMEM");
    eprintln!("\n  PHASE D: BL Upload");

    let base = r145::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };

    let hwcfg = r(r145::HWCFG);
    let code_limit = (hwcfg & 0x1FF) * 256; // imem_size_bytes
    let boot_size =
        ((parsed.bl_desc.bl_code_off + parsed.bl_desc.bl_code_size + 0xFF) & !0xFF) as u32;
    let imem_addr = code_limit.saturating_sub(boot_size);
    let start_tag = parsed.bl_desc.bl_start_tag;
    let boot_addr = start_tag << 8;

    trace("PHASE_D: IMEM upload");
    falcon_imem_upload_nouveau(&bar0, base, imem_addr, &parsed.bl_code, start_tag);
    eprintln!(
        "  BL code: {}B → IMEM@{imem_addr:#x} tag={start_tag:#x} boot={boot_addr:#x}",
        parsed.bl_code.len()
    );

    // BL descriptor: virtual addresses (identity-mapped through PT0)
    let code_dma_base = r145::VRAM_ACR as u64;
    let data_dma_base = r145::VRAM_ACR as u64 + data_off as u64;
    let bl_desc = build_bl_dmem_desc(code_dma_base, data_dma_base, &parsed);
    let ctx_dma = u32::from_le_bytes(bl_desc[32..36].try_into().unwrap_or([1, 0, 0, 0]));
    eprintln!(
        "  BL desc: code={code_dma_base:#x} data={data_dma_base:#x} ctx_dma={ctx_dma} (VIRT)"
    );

    // Load full data section to DMEM first (BL descriptor overwrites start)
    trace("PHASE_D: DMEM data section upload");
    let data_section = &payload[data_off..];
    falcon_dmem_upload(&bar0, base, 0, data_section);
    eprintln!("  Data section: {}B → DMEM@0", data_section.len());

    // Overwrite DMEM@0 with BL descriptor
    trace("PHASE_D: DMEM BL descriptor upload");
    falcon_dmem_upload(&bar0, base, 0, &bl_desc);
    eprintln!("  BL descriptor: {}B → DMEM@0", bl_desc.len());
    trace("PHASE_D: complete");

    // ── Phase E: Boot SEC2 ──
    eprintln!("\n{eq}");
    eprintln!("  PHASE E: Boot SEC2 — v1 interface, 256KB-aligned WPR");
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
            eprintln!("  *** Cannot safely enable DMA — aborting ***");
            return;
        }
    }

    trace("PHASE_E: writing SEC2 boot registers");
    w(r145::EXCI, 0);
    w(r145::MAILBOX0, 0xdead_a5a5_u32);
    w(r145::MAILBOX1, 0);
    w(r145::BOOTVEC, boot_addr);

    let fbif_pre = r(r145::FBIF_TRANSCFG);
    let itfen_pre = r(r145::ITFEN);
    let dmactl_pre = r(r145::DMACTL);
    eprintln!(
        "  Pre-boot: BOOTVEC={boot_addr:#x} FBIF={fbif_pre:#x} ITFEN={itfen_pre:#x} DMACTL={dmactl_pre:#x}"
    );

    trace("PHASE_E: falcon_start_cpu");
    falcon_start_cpu(&bar0, base);
    trace("PHASE_E: SEC2 started — entering poll");

    // ── Phase F: Poll ──
    trace("PHASE_F: polling SEC2");
    eprintln!("\n  PHASE F: Polling SEC2");

    let timeout = std::time::Duration::from_secs(5);
    let start_time = std::time::Instant::now();
    let mut pc_trace: Vec<u32> = Vec::new();

    // Fast PC sampling
    for _ in 0..500 {
        let pc = r(r145::PC);
        if pc_trace.last() != Some(&pc) {
            pc_trace.push(pc);
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
        if start_time.elapsed().as_millis() > 50 {
            break;
        }
    }

    // Slower polling for completion
    let mut settled = 0u32;
    let mut last_pc = pc_trace.last().copied().unwrap_or(0);
    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let cpuctl = r(r145::CPUCTL);
        let mb0 = r(r145::MAILBOX0);
        let mb1 = r(r145::MAILBOX1);
        let pc = r(r145::PC);
        let sctl = r(r145::SCTL);

        if pc != last_pc {
            pc_trace.push(pc);
            last_pc = pc;
            settled = 0;
        } else {
            settled += 1;
        }

        let halted = cpuctl & r145::CPUCTL_HALTED != 0;
        let hreset = cpuctl & r145::CPUCTL_HRESET != 0;

        if mb0 != 0xdead_a5a5 || halted || hreset {
            eprintln!(
                "  SEC2 response: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} \
                 pc={pc:#06x} sctl={sctl:#010x} ({}ms)",
                start_time.elapsed().as_millis()
            );
            break;
        }
        if settled > 200 || start_time.elapsed() > timeout {
            eprintln!(
                "  SEC2 settled/timeout: cpuctl={cpuctl:#010x} mb0={mb0:#010x} \
                 pc={pc:#06x} sctl={sctl:#010x} ({}ms)",
                start_time.elapsed().as_millis()
            );
            break;
        }
    }

    let trace_str: Vec<String> = pc_trace.iter().map(|p| format!("{p:#06x}")).collect();
    eprintln!(
        "  PC trace ({} entries): [{}]",
        pc_trace.len(),
        trace_str.join(" → ")
    );

    trace("PHASE_F: poll complete");

    // ── Phase G: Post-boot diagnostics ──
    trace("PHASE_G: post-boot diagnostics");
    eprintln!("\n  PHASE G: Post-boot Diagnostics");

    let sctl = r(r145::SCTL);
    let hs = sctl & 0x02 != 0;
    let exci = r(r145::EXCI);
    let cpuctl_final = r(r145::CPUCTL);
    let mb0_final = r(r145::MAILBOX0);
    let mb1_final = r(r145::MAILBOX1);
    let dmactl_post = r(r145::DMACTL);
    let itfen_post = r(r145::ITFEN);

    eprintln!("  *** SCTL={sctl:#010x} HS={hs} ***");
    eprintln!("  EXCI={exci:#010x} CPUCTL={cpuctl_final:#010x}");
    eprintln!("  MB0={mb0_final:#010x} MB1={mb1_final:#010x}");
    eprintln!("  DMACTL={dmactl_post:#010x} ITFEN={itfen_post:#010x}");
    // NOTE: PRI_RING_INTR_STATUS (0x120058) read SKIPPED — poisonous post-nouveau.
    eprintln!("  PRI_RING_INTR: (skipped — 0x120058 is poisonous post-nouveau)");

    eprintln!("\n  ── Post-ACR Falcon State ──");
    falcon_state(&bar0, "SEC2", r145::SEC2_BASE);
    falcon_state(&bar0, "FECS", r145::FECS_BASE);
    falcon_state(&bar0, "GPCCS", r145::GPCCS_BASE);

    // ── Phase H: BOOTSTRAP_FALCON (if ACR succeeded) ──
    if hs && mb0_final == 0 {
        eprintln!("\n  PHASE H: BOOTSTRAP_FALCON");

        let bootvecs = FalconBootvecOffsets {
            fecs: 0x7E00,
            gpccs: 0x3400,
        };
        let mb_result = attempt_acr_mailbox_command(&bar0, &bootvecs);
        eprintln!("  {mb_result}");
        for note in &mb_result.notes {
            eprintln!("    {note}");
        }

        eprintln!("\n  ── Final State ──");
        falcon_state(&bar0, "SEC2", r145::SEC2_BASE);
        falcon_state(&bar0, "FECS", r145::FECS_BASE);
        falcon_state(&bar0, "GPCCS", r145::GPCCS_BASE);
    } else {
        eprintln!("\n  Skipping BOOTSTRAP_FALCON (ACR not in HS mode or errors present)");
    }

    // Cleanup through ember — disable bus_master, restore AER masks.
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

    trace("exp145 COMPLETE");
    // ExperimentGuard::drop auto-calls experiment_end here.
    eprintln!("\n{eq}");
    eprintln!(
        "#  Exp 145 COMPLETE — HS={hs} SCTL={sctl:#010x}",
    );
    eprintln!("{eq}");
}
