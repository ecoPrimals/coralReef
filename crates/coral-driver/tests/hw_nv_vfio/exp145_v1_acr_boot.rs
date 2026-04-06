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

    // Check true PRI ring health using BOOT0 (always accessible) and
    // PRI_MASTER_INTR_STATUS (0x100), NOT SEC2_CPUCTL which returns
    // 0xbad0xxxx when the engine is simply disabled in PMC_ENABLE.
    let boot0_poison = boot0 & 0xFFF0_0000 == 0xBAD0_0000;
    let pri_master_intr = ember_read(&bdf, 0x100, "PRI_MASTER_INTR");
    let sec2_enabled = pmc & (1 << 20) != 0;
    eprintln!(
        "  GPU state: {} SEC2_enabled={sec2_enabled} PRI_INTR={pri_master_intr:#010x}",
        if warm { "WARM (fabric alive)" } else { "COLD" },
    );

    if boot0_poison {
        trace("PHASE_A: BOOT0 PRI POISONED — truly corrupted, aborting");
        eprintln!("  *** BOOT0 returns PRI error — GPU fabric dead. Reboot required. ***");
        return;
    }
    if !warm {
        eprintln!("  *** GPU is cold. Swap to nvidia driver to warm, then back to vfio. ***");
    }
    if !sec2_enabled {
        eprintln!("  SEC2 not in PMC_ENABLE — sec2_prepare will enable it");
    }

    trace("PHASE_A: pre-reset falcon state reads");
    eprintln!("\n  ── Pre-reset Falcon State ──");
    falcon_state_via_ember(&bdf, "SEC2", r145::SEC2_BASE);
    falcon_state_via_ember(&bdf, "FECS", r145::FECS_BASE);
    falcon_state_via_ember(&bdf, "GPCCS", r145::GPCCS_BASE);
    trace("PHASE_A: complete");

    // ── Phase B: Load firmware + Write to VRAM (BEFORE any PMC reset) ──
    //
    // PRAMIN-FIRST ORDERING: All bulk VRAM writes MUST happen before any
    // engine reset. PMC reset → PRAMIN write is the proven crash sequence.
    // By writing firmware to VRAM while the GPU is in its pristine post-
    // nouveau state, we avoid PCIe flow-control stalls entirely.
    trace("PHASE_B: loading ACR firmware from disk");
    eprintln!("\n  PHASE B: Load ACR Firmware + Write to VRAM (PRAMIN-first)");

    let fw = AcrFirmwareSet::load("gv100").expect("load firmware");
    eprintln!("  {}", fw.summary());

    let parsed = ParsedAcrFirmware::parse(&fw).expect("parse firmware");
    let data_off = parsed.load_header.data_dma_base as usize;
    eprintln!(
        "  ACR payload: {}B data_off={data_off:#x} data_size={:#x}",
        parsed.acr_payload.len(),
        parsed.load_header.data_size
    );

    // ── Phase B1: Build and write WPR via ember PRAMIN gateway ──
    let wpr_data = build_wpr(&fw, r145::VRAM_WPR as u64);
    let wpr_end = r145::VRAM_WPR as u64 + wpr_data.len() as u64;
    eprintln!(
        "  WPR: {}B at VRAM {:#x}..{:#x} (256KB-aligned)",
        wpr_data.len(),
        r145::VRAM_WPR,
        wpr_end
    );
    assert!(r145::VRAM_WPR % 0x40000 == 0, "WPR must be 256KB-aligned");

    trace("PHASE_B1: writing WPR to VRAM via ember.pramin.write (pre-PMC-reset)");
    match crate::ember_client::pramin_write(&bdf, r145::VRAM_WPR, &wpr_data) {
        Ok(n) => {
            trace(&format!("PHASE_B1: WPR written: {n} bytes"));
            eprintln!("  WPR written: {n} bytes via ember.pramin.write");
        }
        Err(e) => {
            trace(&format!("PHASE_B1: WPR write FAILED: {e}"));
            eprintln!("  *** WPR write failed: {e} ***");
            return;
        }
    }

    trace("PHASE_B1: writing shadow copy");
    match crate::ember_client::pramin_write(&bdf, r145::VRAM_SHADOW as u32, &wpr_data) {
        Ok(n) => {
            trace(&format!("PHASE_B1: shadow written: {n} bytes"));
        }
        Err(e) => {
            trace(&format!("PHASE_B1: shadow write FAILED: {e}"));
            eprintln!("  *** Shadow write failed: {e} ***");
            return;
        }
    }
    eprintln!("  WPR + shadow written to VRAM");

    // ── Phase B2: Patch ACR descriptor ──
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

    trace("PHASE_B2: writing ACR payload to VRAM via ember.pramin.write");
    match crate::ember_client::pramin_write(&bdf, r145::VRAM_ACR, &payload) {
        Ok(n) => {
            trace(&format!("PHASE_B2: ACR payload written: {n} bytes"));
            eprintln!("  ACR payload: {n}B → VRAM {:#x}", r145::VRAM_ACR);
        }
        Err(e) => {
            trace(&format!("PHASE_B2: ACR payload write FAILED: {e}"));
            eprintln!("  *** ACR payload write failed: {e} ***");
            return;
        }
    }
    // ── Phase B3: Build falcon instance block + page tables in VRAM ──
    // Nouveau's gm200_flcn_fw_load binds an instance block for virtual DMA
    // when a separate BL exists. Build the page table chain here so we can
    // bind it after the PMC reset in Phase C.
    trace("PHASE_B3: building instance block + page tables in VRAM");
    {
        use coral_driver::nv::vfio_compute::acr_boot::{
            FALCON_INST_VRAM, FALCON_PD3_VRAM, FALCON_PD2_VRAM,
            FALCON_PD1_VRAM, FALCON_PD0_VRAM, FALCON_PT0_VRAM,
            encode_vram_pde, encode_vram_pte,
        };

        let mut buf = vec![0u8; 0x6000]; // 6 pages: INST+PD3+PD2+PD1+PD0+PT0

        let w64 = |buf: &mut Vec<u8>, off: usize, val: u64| {
            buf[off..off + 8].copy_from_slice(&val.to_le_bytes());
        };
        let w32 = |buf: &mut Vec<u8>, off: usize, val: u32| {
            buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
        };

        // PDEs: each directory entry uses 16B dual PDE, value in upper 8 bytes
        w64(&mut buf, 0x1008, encode_vram_pde(FALCON_PD2_VRAM as u64)); // PD3→PD2
        w64(&mut buf, 0x2008, encode_vram_pde(FALCON_PD1_VRAM as u64)); // PD2→PD1
        w64(&mut buf, 0x3008, encode_vram_pde(FALCON_PD0_VRAM as u64)); // PD1→PD0
        w64(&mut buf, 0x4008, encode_vram_pde(FALCON_PT0_VRAM as u64)); // PD0→PT0

        // PTEs: identity-map 512 pages (2MB) of VRAM
        for i in 0u64..512 {
            w64(&mut buf, 0x5000 + (i as usize) * 8, encode_vram_pte(i * 4096));
        }

        // Instance block: PAGE_DIR_BASE at RAMIN offset 0x200
        let pdb_lo = ((FALCON_PD3_VRAM >> 12) << 12) | (1 << 11) | (1 << 10);
        w32(&mut buf, 0x0200, pdb_lo);
        w32(&mut buf, 0x0204, 0);
        w32(&mut buf, 0x0208, 0xFFFF_FFFF);
        w32(&mut buf, 0x020C, 0x0001_FFFF);

        match crate::ember_client::pramin_write(&bdf, FALCON_INST_VRAM, &buf) {
            Ok(n) => {
                trace(&format!("PHASE_B3: instance block written: {n} bytes"));
                eprintln!("  Instance block + page tables: {n}B → VRAM {FALCON_INST_VRAM:#x}");
            }
            Err(e) => {
                trace(&format!("PHASE_B3: instance block write FAILED: {e}"));
                eprintln!("  *** Instance block write failed: {e} ***");
                return;
            }
        }
    }

    trace("PHASE_B: PRAMIN writes complete (all VRAM written before PMC reset)");

    // ── Phase C: SEC2 reset + DMA setup (via ember) ──
    //
    // Now that all VRAM data is in place, it's safe to do the engine reset.
    // The PMC reset only affects SEC2's clock — VRAM contents persist.
    let pmc_pre_c = ember_read(&bdf, r145::PMC_ENABLE, "PMC_PRE_PHASE_C");
    trace(&format!(
        "PHASE_C: calling ember.sec2.prepare_physical (PMC={pmc_pre_c:#010x})"
    ));
    eprintln!("\n  PHASE C: SEC2 Reset (via ember.sec2.prepare_physical)");
    eprintln!("    PMC_ENABLE before = {pmc_pre_c:#010x}");

    let (prepare_ok, prepare_notes) = match crate::ember_client::sec2_prepare_physical(&bdf) {
        Ok((ok, notes)) => (ok, notes),
        Err(e) => {
            trace(&format!("PHASE_C: ember.sec2.prepare_physical FAILED: {e}"));
            eprintln!("  *** ember.sec2.prepare_physical FAILED: {e} ***");
            return;
        }
    };

    trace(&format!(
        "PHASE_C: sec2_prepare returned ok={prepare_ok} notes={}",
        prepare_notes.len()
    ));
    for (i, note) in prepare_notes.iter().enumerate() {
        trace(&format!("PHASE_C_NOTE[{i}]: {note}"));
        eprintln!("    {note}");
    }
    eprintln!("  Prepare result: ok={prepare_ok}");
    if !prepare_ok {
        trace("PHASE_C: FAILED — aborting");
        eprintln!("  *** SEC2 prepare failed — aborting ***");
        return;
    }
    trace("PHASE_C: complete");

    // ── Phase D: Bind instance block + Load BL to IMEM (via ember) ──
    trace("PHASE_D: instance block bind + BL upload via ember");
    eprintln!("\n  PHASE D: Instance Bind + BL Upload (via ember)");

    let base = r145::SEC2_BASE;
    let hwcfg = ember_read(&bdf, base + r145::HWCFG, "HWCFG");
    let code_limit = (hwcfg & 0x1FF) * 256;
    let boot_size =
        ((parsed.bl_desc.bl_code_off + parsed.bl_desc.bl_code_size + 0xFF) & !0xFF) as u32;
    let imem_addr = code_limit.saturating_sub(boot_size);
    let start_tag = parsed.bl_desc.bl_start_tag;
    let boot_addr = start_tag << 8;

    // Write BOOT0 to falcon register 0x084 (gm200_flcn_enable).
    let boot0 = ember_read(&bdf, 0x000, "BOOT0_CHIP_ID");
    eprintln!("  Writing BOOT0={boot0:#010x} to falcon reg 0x084");
    let _ = crate::ember_client::mmio_batch(&bdf, &[("w", base + 0x084, boot0)]);

    // ── Bind instance block (matching gm200_flcn_fw_load + gm200_flcn_bind_inst) ──
    // 1. Enable ITFEN ACCESS_EN (bit 0)
    trace("PHASE_D: binding instance block for virtual DMA");
    let itfen_cur = ember_read(&bdf, base + 0x048, "ITFEN_PRE_BIND");
    let _ = crate::ember_client::mmio_batch(&bdf, &[
        ("w", base + 0x048, itfen_cur | 0x01),  // ACCESS_EN
    ]);

    // 2. gm200_flcn_bind_inst: DMAIDX clear → bind_inst → mask triggers
    let inst_addr: u32 = 0x10000; // FALCON_INST_VRAM
    let bind_val = (1u32 << 30) | (0u32 << 28) | (inst_addr >> 12); // enable | VRAM | addr

    // nvkm_falcon_mask(0x604, 0x07, 0x00) — clear DMAIDX to VIRT
    let dmaidx_cur = ember_read(&bdf, base + 0x604, "DMAIDX_PRE");
    let dmaidx_new = (dmaidx_cur & !0x07u32) | 0x00; // clear bits[2:0]

    let _ = crate::ember_client::mmio_batch(&bdf, &[
        ("w", base + 0x604, dmaidx_new),
        ("w", base + 0x054, bind_val),
    ]);
    eprintln!("  bind_inst={bind_val:#010x}  DMAIDX: {dmaidx_cur:#x} → {dmaidx_new:#x}");

    // nvkm_falcon_mask(0x090, 0x00010000, 0x00010000) — set bit 16
    let unk090 = ember_read(&bdf, base + 0x090, "UNK090_PRE");
    let unk090_new = (unk090 & !0x00010000u32) | 0x00010000u32;
    let _ = crate::ember_client::mmio_batch(&bdf, &[("w", base + 0x090, unk090_new)]);

    // nvkm_falcon_mask(0x0a4, 0x00000008, 0x00000008) — set bit 3
    let eng_ctl = ember_read(&bdf, base + 0x0a4, "ENG_CTL_PRE");
    let eng_ctl_new = (eng_ctl & !0x00000008u32) | 0x00000008u32;
    let _ = crate::ember_client::mmio_batch(&bdf, &[("w", base + 0x0a4, eng_ctl_new)]);

    // 3. Wait for bind_stat == 5 (bits[14:12] of 0x0DC)
    trace("PHASE_D: waiting for bind_stat=5");
    let mut bind_ok = false;
    for _attempt in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let stat_raw = ember_read(&bdf, base + 0x0DC, "BIND_STAT");
        let bind_stat = (stat_raw >> 12) & 0x7;
        if bind_stat == 5 {
            eprintln!("  Bind state reached 5 (0x0DC={stat_raw:#010x})");
            bind_ok = true;
            break;
        }
    }
    if !bind_ok {
        let stat_final = ember_read(&bdf, base + 0x0DC, "BIND_STAT_FINAL");
        let bind_state = (stat_final >> 12) & 7;
        eprintln!("  *** Bind timeout: state={bind_state} (expected 5) ***");
        trace(&format!("PHASE_D: bind TIMEOUT stat={stat_final:#x}"));
    }

    // 4. Clear bind interrupt (bit 3 of INTR register 0x004)
    let _ = crate::ember_client::mmio_batch(&bdf, &[
        ("w", base + 0x004, 0x00000008u32),
    ]);

    // 5. Channel trigger LOAD: nvkm_falcon_mask(0x058, 2, 2)
    let ch_trig = ember_read(&bdf, base + 0x058, "CH_TRIG_PRE");
    let ch_trig_new = (ch_trig & !0x02u32) | 0x02u32;
    let _ = crate::ember_client::mmio_batch(&bdf, &[("w", base + 0x058, ch_trig_new)]);

    // 6. Wait for bind_stat == 0
    trace("PHASE_D: waiting for bind_stat=0");
    for _attempt in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let stat_raw = ember_read(&bdf, base + 0x0DC, "BIND_STAT_FINAL");
        let bind_stat = (stat_raw >> 12) & 0x7;
        if bind_stat == 0 {
            eprintln!("  Bind finalized: state=0");
            break;
        }
    }

    // ── Load BL to IMEM (gm200_flcn_fw_load: secure=false, BL only) ──
    trace("PHASE_D: BL code → IMEM (secure=false)");
    match crate::ember_client::falcon_upload_imem(
        &bdf, base, imem_addr, &parsed.bl_code, start_tag, false,
    ) {
        Ok(()) => eprintln!(
            "  BL code: {}B → IMEM@{imem_addr:#x} tag={start_tag:#x} boot={boot_addr:#x} secure=false",
            parsed.bl_code.len()
        ),
        Err(e) => {
            trace(&format!("PHASE_D: BL IMEM upload FAILED: {e}"));
            eprintln!("  *** BL IMEM upload failed: {e} ***");
            return;
        }
    }

    // ── Build BL descriptor with VIRTUAL DMA addresses ──
    // The instance block identity-maps first 2MB of VRAM, so virtual addr
    // equals physical VRAM addr for the ACR payload.
    let code_dma_base = r145::VRAM_ACR as u64; // Virtual = physical (identity-mapped)
    let data_dma_base = r145::VRAM_ACR as u64 + data_off as u64;
    let bl_desc = build_bl_dmem_desc(code_dma_base, data_dma_base, &parsed);

    // Keep ctx_dma=VIRT (1) — matching nouveau's gm200_acr_hsfw_load_bld.
    // The default from build_bl_dmem_desc is VIRT(1), no need to patch.
    let ctx_dma = u32::from_le_bytes(bl_desc[32..36].try_into().unwrap());
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

    // Verify IMEM at boot address — confirm BL code survived upload
    trace("PHASE_D: IMEM readback verification");
    let imemc_read_ops = vec![
        ("w", base + 0x180, (1u32 << 25) | imem_addr), // IMEMC: read mode + addr
        ("r", base + 0x184, 0u32), // IMEMD word 0
        ("r", base + 0x184, 0),    // IMEMD word 1
        ("r", base + 0x184, 0),    // IMEMD word 2
        ("r", base + 0x184, 0),    // IMEMD word 3
    ];
    if let Ok(results) = crate::ember_client::mmio_batch(&bdf, &imemc_read_ops) {
        let w0 = results.get(1).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let w1 = results.get(2).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let w2 = results.get(3).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let w3 = results.get(4).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let expected_w0 = if parsed.bl_code.len() >= 4 {
            u32::from_le_bytes(parsed.bl_code[0..4].try_into().unwrap())
        } else { 0 };
        let match_ok = w0 == expected_w0;
        eprintln!(
            "  IMEM@{imem_addr:#x} readback: [{w0:#010x} {w1:#010x} {w2:#010x} {w3:#010x}] expected[0]={expected_w0:#010x} match={}",
            if match_ok { "YES" } else { "NO" }
        );
        trace(&format!("IMEM verify: [{w0:#010x} {w1:#010x} {w2:#010x} {w3:#010x}] match={match_ok}"));
    }

    // Verify DMEM BL descriptor — confirm ctx_dma patch
    let dmemc_read_ops = vec![
        ("w", base + 0x1C0, (1u32 << 25) | 0u32), // DMEMC: read mode + addr=0
        ("r", base + 0x1C4, 0u32), // DMEMD word 0 (reserved[0])
        ("r", base + 0x1C4, 0),    // word 1
        ("r", base + 0x1C4, 0),    // word 2
        ("r", base + 0x1C4, 0),    // word 3
        ("r", base + 0x1C4, 0),    // word 4 (reserved[3])
        ("r", base + 0x1C4, 0),    // word 5 (signature[0])
        ("r", base + 0x1C4, 0),    // word 6 (signature[1])
        ("r", base + 0x1C4, 0),    // word 7 (signature[2..3])
        ("r", base + 0x1C4, 0),    // word 8 = ctx_dma (offset 32)
        ("r", base + 0x1C4, 0),    // word 9 = code_dma_base low (offset 36)
        ("r", base + 0x1C4, 0),    // word 10 = code_dma_base high (offset 40)
    ];
    if let Ok(results) = crate::ember_client::mmio_batch(&bdf, &dmemc_read_ops) {
        let ctx_dma = results.get(9).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let code_lo = results.get(10).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let code_hi = results.get(11).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        eprintln!(
            "  DMEM BL desc readback: ctx_dma={ctx_dma} code_dma={code_hi:#x}:{code_lo:#x}"
        );
        trace(&format!("DMEM verify: ctx_dma={ctx_dma} code_dma={code_hi:#x}:{code_lo:#x}"));
    }

    // Read BROM-related registers (may or may not exist on GV100)
    trace("PHASE_D: probing BROM registers");
    let brom_probe_ops = vec![
        ("r", base + 0x1180, 0u32), // FalconModSel
        ("r", base + 0x1198, 0u32), // FalconBromCurrUcodeId
        ("r", base + 0x119C, 0u32), // FalconBromEngidmask
        ("r", base + 0x1210, 0u32), // FalconBromParaaddr0
        ("r", base + 0x12C, 0u32),  // FalconHwcfg1 (for security model)
    ];
    if let Ok(results) = crate::ember_client::mmio_batch(&bdf, &brom_probe_ops) {
        let modsel = results.get(0).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let ucode_id = results.get(1).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let engid = results.get(2).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let paraaddr = results.get(3).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let hwcfg1 = results.get(4).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let sec_model = (hwcfg1 >> 4) & 0x3;
        eprintln!("  BROM regs: ModSel={modsel:#x} UcodeId={ucode_id:#x} EngIdMask={engid:#x} ParaAddr0={paraaddr:#x} HWCFG1={hwcfg1:#x} sec_model={sec_model}");
        trace(&format!("BROM: modsel={modsel:#x} uid={ucode_id:#x} engid={engid:#x} para={paraaddr:#x} hwcfg1={hwcfg1:#x} sec={sec_model}"));
    }

    // Read HS header fields for diagnostics
    eprintln!("  HS header: sig_prod_off={:#x} sig_prod_size={} patch_loc={:#x} patch_sig={:#x}",
        parsed.hs_header.sig_prod_offset, parsed.hs_header.sig_prod_size,
        parsed.hs_header.patch_loc, parsed.hs_header.patch_sig);

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

    // Set MAILBOX0 + BOOTVEC via batch (safe register writes, no DMA risk).
    // STARTCPU goes through ember.falcon.start_cpu which is FORK-ISOLATED —
    // if the falcon's DMA locks the PCIe bus, the forked child dies instead
    // of ember or the system.
    trace("PHASE_E: writing MAILBOX0 + BOOTVEC via ember.mmio.batch");
    let pre_start_ops = vec![
        ("w", base + r145::MAILBOX0, 0xcafe_beef_u32), // Nouveau sentinel
        ("w", base + r145::BOOTVEC, boot_addr),
    ];
    if let Err(e) = crate::ember_client::mmio_batch(&bdf, &pre_start_ops) {
        trace(&format!("PHASE_E: pre-start register writes FAILED: {e}"));
        eprintln!("  *** pre-start register writes failed: {e} ***");
        return;
    }

    // STARTCPU via fork-isolated ember.falcon.start_cpu — NOT mmio_batch.
    // mmio_batch only has thread-level watchdog protection. falcon.start_cpu
    // forks a child process: if the falcon DMA causes a PCIe hang, the child
    // is killed and the bus is reset. Ember survives.
    trace("PHASE_E: STARTCPU via ember.falcon.start_cpu (fork-isolated)");
    match crate::ember_client::falcon_start_cpu(&bdf, base) {
        Ok(result) => {
            let pc = result.get("pc").and_then(|v| v.as_u64()).unwrap_or(0xDEAD);
            let cpuctl = result.get("cpuctl").and_then(|v| v.as_u64()).unwrap_or(0xDEAD);
            let exci = result.get("exci").and_then(|v| v.as_u64()).unwrap_or(0xDEAD);
            eprintln!(
                "  falcon.start_cpu: pc={pc:#06x} cpuctl={cpuctl:#010x} exci={exci:#010x}"
            );
            trace(&format!("PHASE_E: start_cpu OK pc={pc:#x} cpuctl={cpuctl:#x} exci={exci:#x}"));
        }
        Err(e) => {
            trace(&format!("PHASE_E: falcon.start_cpu FAILED: {e}"));
            eprintln!("  *** falcon.start_cpu FAILED (fork-isolated, ember survived): {e} ***");
            eprintln!("  GPU may be faulted — use ember.device.recover to attempt recovery.");
            return;
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Read state after boot attempts
    let post_ops = vec![
        ("r", base + r145::CPUCTL, 0u32),
        ("r", base + r145::PC, 0),
        ("r", base + r145::EXCI, 0),
        ("r", base + r145::SCTL, 0),
        ("r", base + r145::MAILBOX0, 0),
        ("r", base + r145::BOOTVEC, 0),
    ];
    if let Ok(results) = crate::ember_client::mmio_batch(&bdf, &post_ops) {
        let cpuctl = results.first().and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let pc = results.get(1).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let exci = results.get(2).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let sctl = results.get(3).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let mb0 = results.get(4).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let bv = results.get(5).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let halted = cpuctl & 0x10 != 0;
        let hs = sctl & 0x02 != 0;
        trace(&format!(
            "PHASE_E: cpuctl={cpuctl:#x} pc={pc:#x} exci={exci:#x} sctl={sctl:#x} mb0={mb0:#x} bv={bv:#x}"
        ));
        eprintln!(
            "  Boot result: cpuctl={cpuctl:#010x} pc={pc:#06x} exci={exci:#010x} sctl={sctl:#010x} mb0={mb0:#010x} bv={bv:#x} halted={halted} hs={hs}"
        );
    }
    // Post-STARTCPU: read TRACEPC buffer (execution history)
    trace("PHASE_E: reading TRACEPC buffer");
    let tracepc_idx_ops = vec![("r", base + r145::EXCI, 0u32)];
    if let Ok(results) = crate::ember_client::mmio_batch(&bdf, &tracepc_idx_ops) {
        let exci_val = results.first().and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let trace_count = ((exci_val >> 16) & 0xFF).min(16);
        if trace_count > 0 {
            let mut tp_ops: Vec<(&str, usize, u32)> = Vec::new();
            for i in 0..trace_count {
                tp_ops.push(("w", base + r145::EXCI, i));
                tp_ops.push(("r", base + 0x14C, 0)); // TRACEPC
            }
            if let Ok(tp_results) = crate::ember_client::mmio_batch(&bdf, &tp_ops) {
                let traces: Vec<String> = (0..trace_count as usize)
                    .filter_map(|i| {
                        tp_results.get(i * 2 + 1).and_then(|v| v.as_u64())
                    })
                    .map(|pc| format!("{pc:#06x}"))
                    .collect();
                eprintln!("  TRACEPC ({trace_count}): [{}]", traces.join(" → "));
                trace(&format!("TRACEPC: [{}]", traces.join(",")));
            }
        }
    }

    // Post-STARTCPU: re-read IMEM to see if ROM scrubbed it
    trace("PHASE_E: IMEM readback post-STARTCPU");
    let imemc_post_ops = vec![
        ("w", base + 0x180, (1u32 << 25) | imem_addr),
        ("r", base + 0x184, 0u32),
        ("r", base + 0x184, 0),
        ("r", base + 0x184, 0),
        ("r", base + 0x184, 0),
    ];
    if let Ok(results) = crate::ember_client::mmio_batch(&bdf, &imemc_post_ops) {
        let w0 = results.get(1).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let w1 = results.get(2).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let w2 = results.get(3).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let w3 = results.get(4).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let expected_w0 = if parsed.bl_code.len() >= 4 {
            u32::from_le_bytes(parsed.bl_code[0..4].try_into().unwrap())
        } else { 0 };
        let survived = w0 == expected_w0;
        eprintln!(
            "  Post-boot IMEM@{imem_addr:#x}: [{w0:#010x} {w1:#010x} {w2:#010x} {w3:#010x}] survived={}",
            if survived { "YES" } else { "SCRUBBED" }
        );
        trace(&format!("Post-boot IMEM: [{w0:#010x} {w1:#010x}] survived={survived}"));
    }

    // Check secure IMEM section (at 0x100) — was it scrubbed by BROM?
    let sec_imem_check_ops = vec![
        ("w", base + 0x180, (1u32 << 25) | 0x100u32), // IMEMC: read mode + addr=0x100
        ("r", base + 0x184, 0u32),
        ("r", base + 0x184, 0),
        ("r", base + 0x184, 0),
        ("r", base + 0x184, 0),
    ];
    if let Ok(results) = crate::ember_client::mmio_batch(&bdf, &sec_imem_check_ops) {
        let w0 = results.get(1).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let w1 = results.get(2).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let w2 = results.get(3).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let w3 = results.get(4).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let scrubbed = w0 == 0xdead5ec1;
        eprintln!(
            "  Post-boot SEC IMEM@0x100: [{w0:#010x} {w1:#010x} {w2:#010x} {w3:#010x}] scrubbed={scrubbed}"
        );
        trace(&format!("Post-boot SEC IMEM: [{w0:#010x} {w1:#010x} {w2:#010x} {w3:#010x}] scrubbed={scrubbed}"));
    }

    // Also check BROM registers post-boot
    let brom_post_ops = vec![
        ("r", base + 0x1180, 0u32), // FalconModSel
        ("r", base + 0x1210, 0u32), // FalconBromParaaddr0
    ];
    if let Ok(results) = crate::ember_client::mmio_batch(&bdf, &brom_post_ops) {
        let modsel = results.get(0).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let para = results.get(1).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        eprintln!("  Post-boot BROM: ModSel={modsel:#x} ParaAddr0={para:#x}");
    }

    // Also check FBIF/DMACTL after boot — did the ROM reset them?
    let post_boot_dma_ops = vec![
        ("r", base + r145::FBIF_TRANSCFG, 0u32),
        ("r", base + 0x048, 0), // ITFEN
        ("r", base + 0x10C, 0), // DMACTL
    ];
    if let Ok(results) = crate::ember_client::mmio_batch(&bdf, &post_boot_dma_ops) {
        let fbif = results.first().and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let itfen = results.get(1).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        let dmactl = results.get(2).and_then(|v| v.as_u64()).unwrap_or(0xDEAD) as u32;
        eprintln!(
            "  Post-boot DMA: FBIF={fbif:#x} ITFEN={itfen:#x} DMACTL={dmactl:#x}"
        );
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
