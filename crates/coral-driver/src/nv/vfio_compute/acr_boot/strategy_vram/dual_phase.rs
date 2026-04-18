// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::vfio::channel::registers::{falcon, misc};
use crate::vfio::device::MappedBar;
use crate::vfio::memory::{MemoryRegion, PraminRegion};

use super::super::boot_result::{AcrBootResult, make_fail_result};
use super::super::firmware::{AcrFirmwareSet, ParsedAcrFirmware};
use super::super::instance_block::{
    FALCON_INST_VRAM, FALCON_PD0_VRAM, FALCON_PD1_VRAM, FALCON_PD2_VRAM, FALCON_PD3_VRAM,
    FALCON_PT0_VRAM, encode_bind_inst, encode_vram_pde, encode_vram_pte, falcon_bind_context,
};
use super::super::sec2_hal::{
    Sec2Probe, falcon_dmem_upload, falcon_imem_upload_nouveau, falcon_start_cpu, find_sec2_pmc_bit,
    pmc_enable_sec2, sec2_emem_write,
};
use super::super::wpr::{build_bl_dmem_desc, build_wpr, patch_acr_desc};
use super::pramin_write::write_to_vram;

// ── Exp 112: Dual-Phase Boot ───────────────────────────────────────────

/// Build VRAM page tables with legacy PDEs (lower 8-byte slot).
/// Same as `build_vram_falcon_inst_block` but PDEs go in the WRONG slot
/// to trigger MMU physical fallback → HS authentication.
fn build_vram_legacy_pde_tables(bar0: &MappedBar) -> bool {
    let wv = |vram_addr: u32, offset: usize, val: u32| -> bool {
        match PraminRegion::new(bar0, vram_addr, offset + 4) {
            Ok(mut region) => region.write_u32(offset, val).is_ok(),
            Err(_) => false,
        }
    };
    let wv64 = |vram_addr: u32, offset: usize, val: u64| -> bool {
        let lo = (val & 0xFFFF_FFFF) as u32;
        let hi = (val >> 32) as u32;
        wv(vram_addr, offset, lo) && wv(vram_addr, offset + 4, hi)
    };

    // Zero all pages first
    for &page in &[
        FALCON_INST_VRAM,
        FALCON_PD3_VRAM,
        FALCON_PD2_VRAM,
        FALCON_PD1_VRAM,
        FALCON_PD0_VRAM,
        FALCON_PT0_VRAM,
    ] {
        for off in (0..0x1000).step_by(4) {
            if !wv(page, off, 0) {
                return false;
            }
        }
    }

    // Legacy PDE format: pointer in LOWER 8 bytes, upper 8 bytes = 0
    // This triggers MMU physical fallback → HS authentication
    if !wv64(FALCON_PD3_VRAM, 0, encode_vram_pde(FALCON_PD2_VRAM as u64)) {
        return false;
    }
    if !wv64(FALCON_PD3_VRAM, 8, 0) {
        return false;
    }

    if !wv64(FALCON_PD2_VRAM, 0, encode_vram_pde(FALCON_PD1_VRAM as u64)) {
        return false;
    }
    if !wv64(FALCON_PD2_VRAM, 8, 0) {
        return false;
    }

    if !wv64(FALCON_PD1_VRAM, 0, encode_vram_pde(FALCON_PD0_VRAM as u64)) {
        return false;
    }
    if !wv64(FALCON_PD1_VRAM, 8, 0) {
        return false;
    }

    if !wv64(FALCON_PD0_VRAM, 0, encode_vram_pde(FALCON_PT0_VRAM as u64)) {
        return false;
    }
    if !wv64(FALCON_PD0_VRAM, 8, 0) {
        return false;
    }

    // PT0: identity-map 512 small pages (2 MiB)
    for i in 0u64..512 {
        let phys = i * 4096;
        if !wv64(FALCON_PT0_VRAM, (i as usize) * 8, encode_vram_pte(phys)) {
            return false;
        }
    }

    // Instance block: PDB at RAMIN offset 0x200
    // GP100+ format: addr | VALID | (target << 1), target=0 for VRAM
    let pdb_lo: u32 = FALCON_PD3_VRAM | 1;
    if !wv(FALCON_INST_VRAM, 0x200, pdb_lo) {
        return false;
    }
    if !wv(FALCON_INST_VRAM, 0x204, 0) {
        return false;
    }
    if !wv(FALCON_INST_VRAM, 0x208, 0xFFFF_FFFF) {
        return false;
    }
    if !wv(FALCON_INST_VRAM, 0x20C, 0x0001_FFFF) {
        return false;
    }

    true
}

/// Hot-swap PDEs from legacy (lower 8-byte) to correct (upper 8-byte) format.
/// Called after HS authentication to enable correct virtual DMA.
fn hotswap_pdes_to_correct(bar0: &MappedBar) -> bool {
    let wv64 = |vram_addr: u32, offset: usize, val: u64| -> bool {
        let lo = (val & 0xFFFF_FFFF) as u32;
        let hi = (val >> 32) as u32;
        match PraminRegion::new(bar0, vram_addr, offset + 8) {
            Ok(mut region) => {
                region.write_u32(offset, lo).is_ok() && region.write_u32(offset + 4, hi).is_ok()
            }
            Err(_) => false,
        }
    };

    // Write correct PDEs (upper 8 bytes) and zero legacy (lower 8 bytes)
    let ok = wv64(FALCON_PD3_VRAM, 0, 0)
        && wv64(FALCON_PD3_VRAM, 8, encode_vram_pde(FALCON_PD2_VRAM as u64))
        && wv64(FALCON_PD2_VRAM, 0, 0)
        && wv64(FALCON_PD2_VRAM, 8, encode_vram_pde(FALCON_PD1_VRAM as u64))
        && wv64(FALCON_PD1_VRAM, 0, 0)
        && wv64(FALCON_PD1_VRAM, 8, encode_vram_pde(FALCON_PD0_VRAM as u64))
        && wv64(FALCON_PD0_VRAM, 0, 0)
        && wv64(FALCON_PD0_VRAM, 8, encode_vram_pde(FALCON_PT0_VRAM as u64));

    // TLB invalidate
    if ok {
        let pdb_addr = FALCON_INST_VRAM as u64;
        let pdb_inv = ((pdb_addr >> 12) << 4) as u32;
        let _ = bar0.write_u32(0x100CB8, pdb_inv);
        let _ = bar0.write_u32(0x100CEC, 0);
        let _ = bar0.write_u32(0x100CBC, 0x8000_0005);
    }

    ok
}

/// Configuration for dual-phase boot experiments.
#[derive(Default)]
pub struct DualPhaseConfig {
    /// If true, skip the PDE hot-swap (stay on legacy PDEs throughout).
    pub skip_hotswap: bool,
    /// If true, set blob_size=0 in the ACR descriptor (skip blob DMA).
    pub skip_blob_dma: bool,
    /// Microseconds to wait before hot-swapping PDEs (0 = immediate).
    pub hotswap_delay_us: u64,
    /// If true, attempt to set WPR2 hardware boundaries before boot.
    pub set_wpr2: bool,
}

impl std::fmt::Display for DualPhaseConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "hotswap={} blob={} delay={}µs wpr2={}",
            if self.skip_hotswap { "OFF" } else { "ON" },
            if self.skip_blob_dma { "skip" } else { "full" },
            self.hotswap_delay_us,
            if self.set_wpr2 { "SET" } else { "off" },
        )
    }
}

/// Exp 112+: Dual-phase boot — legacy PDEs for HS auth, hot-swap for DMA.
///
/// Phase 1: Build VRAM page tables with legacy PDEs (lower 8-byte slot)
///          → MMU physical fallback → HS authentication succeeds
/// Phase 2: Hot-swap PDEs to correct format (upper 8-byte) via PRAMIN
///          → Post-auth DMA uses correct virtual path through VRAM PTs
pub fn attempt_dual_phase_boot(bar0: &MappedBar, fw: &AcrFirmwareSet) -> AcrBootResult {
    attempt_dual_phase_boot_cfg(bar0, fw, &DualPhaseConfig::default())
}

/// Configurable dual-phase boot (Exp 113 variants).
pub fn attempt_dual_phase_boot_cfg(
    bar0: &MappedBar,
    fw: &AcrFirmwareSet,
    cfg: &DualPhaseConfig,
) -> AcrBootResult {
    let mut notes = Vec::new();
    let sec2_before = Sec2Probe::capture(bar0);
    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };

    notes.push(format!("Dual-Phase Boot [{cfg}]"));

    // ── Step 0: VRAM check ──
    let vram_ok = match PraminRegion::new(bar0, 0x5_0000, 8) {
        Ok(mut rgn) => {
            let s = 0xACB0_112A_u32;
            let _ = rgn.write_u32(0, s);
            rgn.read_u32(0).unwrap_or(0) == s
        }
        Err(_) => false,
    };
    if !vram_ok {
        return make_fail_result("Dual-phase: VRAM inaccessible", sec2_before, bar0, notes);
    }
    notes.push("VRAM: accessible".to_string());

    // ── Step 1: Parse firmware ──
    let parsed = match ParsedAcrFirmware::parse(fw) {
        Ok(p) => p,
        Err(e) => {
            notes.push(format!("Firmware parse failed: {e}"));
            return make_fail_result("Dual-phase: parse failed", sec2_before, bar0, notes);
        }
    };
    let data_off = parsed.load_header.data_dma_base as usize;

    // ── Step 2: Write payload + WPR to VRAM ──
    let vram_acr: u32 = 0x0005_0000;
    let vram_wpr: u32 = 0x0007_0000;
    let vram_shadow: u64 = 0x0006_0000;

    let mut payload_patched = parsed.acr_payload.to_vec();
    let wpr_data = build_wpr(fw, vram_wpr as u64);
    let wpr_end = vram_wpr as u64 + wpr_data.len() as u64;

    patch_acr_desc(
        &mut payload_patched,
        data_off,
        vram_wpr as u64,
        wpr_end,
        vram_shadow,
    );
    if cfg.skip_blob_dma {
        if data_off + 0x268 <= payload_patched.len() {
            payload_patched[data_off + 0x258..data_off + 0x25C]
                .copy_from_slice(&0u32.to_le_bytes());
            payload_patched[data_off + 0x260..data_off + 0x268]
                .copy_from_slice(&0u64.to_le_bytes());
            notes.push("blob_size=0 (skip blob DMA)".to_string());
        }
    } else if data_off + 0x268 <= payload_patched.len() {
        let blob_size = u32::from_le_bytes(
            payload_patched[data_off + 0x258..data_off + 0x25C]
                .try_into()
                .unwrap_or([0; 4]),
        );
        notes.push(format!(
            "blob_size={blob_size:#x} (preserved for full init)"
        ));
    }

    if !write_to_vram(bar0, vram_acr, &payload_patched, &mut notes) {
        return make_fail_result("Dual-phase: payload write failed", sec2_before, bar0, notes);
    }
    if !write_to_vram(bar0, vram_wpr, &wpr_data, &mut notes) {
        return make_fail_result("Dual-phase: WPR write failed", sec2_before, bar0, notes);
    }
    if !write_to_vram(bar0, vram_shadow as u32, &wpr_data, &mut notes) {
        return make_fail_result("Dual-phase: shadow write failed", sec2_before, bar0, notes);
    }
    notes.push(format!(
        "VRAM data: ACR@{vram_acr:#x} WPR@{vram_wpr:#x} shadow@{vram_shadow:#x}"
    ));

    // ── Step 3: Build VRAM page tables with LEGACY PDEs ──
    let pt_ok = build_vram_legacy_pde_tables(bar0);
    notes.push(format!("Legacy PDE page tables: built={pt_ok}"));

    // Verify: PDE should be in LOWER 8 bytes, upper should be 0
    let rv = |vram_addr: u32, offset: usize| -> u32 {
        PraminRegion::new(bar0, vram_addr, offset + 4)
            .ok()
            .and_then(|r| r.read_u32(offset).ok())
            .unwrap_or(0xDEAD)
    };
    let pd3_lo = rv(FALCON_PD3_VRAM, 0);
    let pd3_hi = rv(FALCON_PD3_VRAM, 8);
    notes.push(format!(
        "PD3 verify: lower={pd3_lo:#010x} upper={pd3_hi:#010x} (expect: lower=PDE, upper=0)"
    ));

    // ── Step 3b: WPR2 hardware boundaries ──
    {
        let wpr1_beg = bar0.read_u32(0x100CE4).unwrap_or(0xDEAD);
        let wpr1_end = bar0.read_u32(0x100CE8).unwrap_or(0xDEAD);
        let wpr2_beg = bar0.read_u32(0x100CEC).unwrap_or(0xDEAD);
        let wpr2_end = bar0.read_u32(0x100CF0).unwrap_or(0xDEAD);
        notes.push(format!(
            "WPR hw: WPR1=[{wpr1_beg:#010x}..{wpr1_end:#010x}] WPR2=[{wpr2_beg:#010x}..{wpr2_end:#010x}]"
        ));

        // GM200 indexed register for WPR boundaries
        let _ = bar0.write_u32(0x100CD4, 2);
        let idx_lo = bar0.read_u32(0x100CD4).unwrap_or(0xDEAD);
        let _ = bar0.write_u32(0x100CD4, 3);
        let idx_hi = bar0.read_u32(0x100CD4).unwrap_or(0xDEAD);
        notes.push(format!(
            "WPR GM200 indexed: lo={idx_lo:#010x} hi={idx_hi:#010x}"
        ));

        if cfg.set_wpr2 {
            // Attempt to set WPR2 boundaries to cover our WPR region.
            // The ACR BL may validate these before copying authenticated images.
            let wpr_beg_val = vram_wpr;
            let wpr_end_val = wpr_end as u32;

            // Direct PFB WPR registers
            let _ = bar0.write_u32(0x100CE4, wpr_beg_val);
            let _ = bar0.write_u32(0x100CE8, wpr_end_val);
            let rb1_beg = bar0.read_u32(0x100CE4).unwrap_or(0xDEAD);
            let rb1_end = bar0.read_u32(0x100CE8).unwrap_or(0xDEAD);
            notes.push(format!(
                "WPR1 set: {wpr_beg_val:#010x}→{rb1_beg:#010x} {wpr_end_val:#010x}→{rb1_end:#010x}"
            ));

            // 0x100CEC/CF0 may be TLB invalidation registers, NOT WPR2.
            // Try writing WPR bounds anyway and check readback.
            let _ = bar0.write_u32(0x100CEC, wpr_beg_val);
            let _ = bar0.write_u32(0x100CF0, wpr_end_val);
            let rb2_beg = bar0.read_u32(0x100CEC).unwrap_or(0xDEAD);
            let rb2_end = bar0.read_u32(0x100CF0).unwrap_or(0xDEAD);
            notes.push(format!(
                "WPR2 set: {wpr_beg_val:#010x}→{rb2_beg:#010x} {wpr_end_val:#010x}→{rb2_end:#010x}"
            ));

            // GM200 indexed: (addr >> 8) | enable_bit
            let gm200_lo = (wpr_beg_val >> 8) | 0x01;
            let gm200_hi = wpr_end_val >> 8;
            let _ = bar0.write_u32(0x100CD4, gm200_lo);
            std::thread::sleep(std::time::Duration::from_micros(10));
            let rb_lo = bar0.read_u32(0x100CD4).unwrap_or(0xDEAD);
            let _ = bar0.write_u32(0x100CD4, gm200_hi);
            std::thread::sleep(std::time::Duration::from_micros(10));
            let rb_hi = bar0.read_u32(0x100CD4).unwrap_or(0xDEAD);
            notes.push(format!(
                "GM200 indexed set: lo={gm200_lo:#010x}→{rb_lo:#010x} hi={gm200_hi:#010x}→{rb_hi:#010x}"
            ));
        }
    }

    // ── Step 4: SEC2 reset ──
    w(falcon::ITFEN, r(falcon::ITFEN) & !0x03);
    w(falcon::IRQMCLR, 0xFFFF_FFFF);
    {
        let sec2_bit = find_sec2_pmc_bit(bar0).unwrap_or(22);
        let sec2_mask = 1u32 << sec2_bit;
        let val = bar0.read_u32(misc::PMC_ENABLE).unwrap_or(0);
        if val & sec2_mask != 0 {
            let _ = bar0.write_u32(misc::PMC_ENABLE, val & !sec2_mask);
            let _ = bar0.read_u32(misc::PMC_ENABLE);
            std::thread::sleep(std::time::Duration::from_micros(20));
        }
    }
    w(falcon::ENGCTL, 0x01);
    std::thread::sleep(std::time::Duration::from_micros(10));
    w(falcon::ENGCTL, 0x00);
    if let Err(e) = pmc_enable_sec2(bar0) {
        notes.push(format!("PMC enable failed: {e}"));
    }
    let scrub_start = std::time::Instant::now();
    loop {
        let scrub = r(falcon::DMACTL);
        if scrub & 0x06 == 0 {
            break;
        }
        if scrub_start.elapsed() > std::time::Duration::from_millis(100) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }
    let boot0 = bar0.read_u32(misc::BOOT0).unwrap_or(0);
    w(0x084, boot0);
    notes.push(format!(
        "Post-reset: cpuctl={:#010x} sctl={:#010x}",
        r(falcon::CPUCTL),
        r(falcon::SCTL)
    ));

    // ── Step 5: Bind VRAM instance block ──
    w(falcon::ITFEN, r(falcon::ITFEN) | 0x01);
    let bind_val = encode_bind_inst(FALCON_INST_VRAM as u64, 0);
    let (bind_ok, bind_notes) =
        falcon_bind_context(&|off| r(off), &|off, val| w(off, val), bind_val);
    for n in &bind_notes {
        notes.push(n.clone());
    }
    notes.push(format!(
        "Bind: {} val={bind_val:#010x}",
        if bind_ok { "OK" } else { "TIMEOUT" }
    ));

    // ── Step 6: Upload BL + data ──
    let hwcfg = r(falcon::HWCFG);
    let code_limit = falcon::imem_size_bytes(hwcfg);
    let boot_size =
        ((parsed.bl_desc.bl_code_off + parsed.bl_desc.bl_code_size + 0xFF) & !0xFF) as u32;
    let imem_addr = code_limit.saturating_sub(boot_size);
    let start_tag = parsed.bl_desc.bl_start_tag;
    let boot_addr = start_tag << 8;

    falcon_imem_upload_nouveau(bar0, base, imem_addr, &parsed.bl_code, start_tag);

    let bl_payload = fw.acr_bl_parsed.payload(&fw.acr_bl_raw);
    let bl_data_off = parsed.bl_desc.bl_data_off as usize;
    let bl_data_size = parsed.bl_desc.bl_data_size as usize;
    let bl_data_end = (bl_data_off + bl_data_size).min(bl_payload.len());
    if bl_data_off < bl_payload.len() && bl_data_size > 0 {
        sec2_emem_write(bar0, 0, &bl_payload[bl_data_off..bl_data_end]);
    }

    let code_dma_base = vram_acr as u64;
    let data_dma_base = vram_acr as u64 + data_off as u64;
    let data_section = &payload_patched[data_off..];
    falcon_dmem_upload(bar0, base, 0, data_section);

    let bl_desc = build_bl_dmem_desc(code_dma_base, data_dma_base, &parsed);
    falcon_dmem_upload(bar0, base, 0, &bl_desc);
    notes.push(format!(
        "BL: IMEM@{imem_addr:#x} boot={boot_addr:#x} code_dma={code_dma_base:#x} ctx_dma=VIRT"
    ));

    // ── Step 7: Start falcon + immediately hot-swap PDEs ──
    w(falcon::EXCI, 0);
    w(falcon::MAILBOX0, 0xdead_a5a5_u32);
    w(falcon::MAILBOX1, 0);
    w(falcon::BOOTVEC, boot_addr);

    notes.push("Phase 1: Starting falcon with legacy PDEs...".to_string());
    falcon_start_cpu(bar0, base);

    if cfg.skip_hotswap {
        notes.push("Phase 2: SKIPPED (legacy PDEs throughout)".to_string());
    } else {
        if cfg.hotswap_delay_us > 0 {
            std::thread::sleep(std::time::Duration::from_micros(cfg.hotswap_delay_us));
            let delay_sctl = r(falcon::SCTL);
            let delay_pc = r(falcon::PC);
            notes.push(format!(
                "Hot-swap delay: {}µs SCTL={delay_sctl:#010x} PC={delay_pc:#06x}",
                cfg.hotswap_delay_us
            ));
        }
        let swap_ok = hotswap_pdes_to_correct(bar0);
        let swap_sctl = r(falcon::SCTL);
        let swap_pc = r(falcon::PC);
        notes.push(format!(
            "Phase 2: PDEs hot-swapped={swap_ok} SCTL={swap_sctl:#010x} PC={swap_pc:#06x}"
        ));
        std::thread::sleep(std::time::Duration::from_micros(10));
        let _ = hotswap_pdes_to_correct(bar0);
    }

    // ── Step 8: Poll with PC sampling ──
    let timeout = std::time::Duration::from_secs(5);
    let start_time = std::time::Instant::now();
    let mut all_pcs: Vec<u32> = Vec::new();
    for _ in 0..500 {
        let pc = r(falcon::PC);
        if all_pcs.last() != Some(&pc) {
            all_pcs.push(pc);
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
        if start_time.elapsed().as_millis() > 50 {
            break;
        }
    }

    let mut settled_count = 0u32;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let cpuctl = r(falcon::CPUCTL);
        let mb0 = r(falcon::MAILBOX0);
        let pc = r(falcon::PC);
        let halted = cpuctl & falcon::CPUCTL_HALTED != 0;
        let hreset = cpuctl & falcon::CPUCTL_HRESET != 0;

        if all_pcs.last() != Some(&pc) {
            all_pcs.push(pc);
            settled_count = 0;
        } else {
            settled_count += 1;
        }

        if mb0 != 0 || halted || hreset {
            notes.push(format!(
                "SEC2: cpuctl={cpuctl:#010x} mb0={mb0:#010x} pc={pc:#06x} ({}ms)",
                start_time.elapsed().as_millis()
            ));
            break;
        }
        if settled_count > 200 || start_time.elapsed() > timeout {
            notes.push(format!(
                "SEC2 settled/timeout: cpuctl={cpuctl:#010x} mb0={mb0:#010x} pc={pc:#06x}",
            ));
            break;
        }
    }

    if !all_pcs.is_empty() {
        let trace: Vec<String> = all_pcs.iter().map(|p| format!("{p:#06x}")).collect();
        notes.push(format!("PC trace: [{}]", trace.join(" → ")));
    }

    // ── Step 9: Diagnostics ──
    super::super::boot_diagnostics::capture_post_boot_diagnostics(bar0, base, &mut notes);

    // Check WPR status
    if let Ok(region) = PraminRegion::new(bar0, vram_wpr, 64) {
        let fecs_status = region.read_u32(20).unwrap_or(0);
        let gpccs_status = region.read_u32(44).unwrap_or(0);
        notes.push(format!(
            "WPR: FECS status={fecs_status} GPCCS status={gpccs_status}"
        ));
    }

    let sctl = r(falcon::SCTL);
    let hs = sctl & 0x02 != 0;
    notes.push(format!("*** SCTL={sctl:#010x} HS={hs} ***"));

    let sec2_after = Sec2Probe::capture(bar0);
    let post = super::super::boot_result::PostBootCapture::capture(bar0);
    notes.push(format!(
        "Final: FECS cpuctl={:#010x} exci={:#010x} GPCCS cpuctl={:#010x} exci={:#010x}",
        post.fecs_cpuctl, post.fecs_exci, post.gpccs_cpuctl, post.gpccs_exci
    ));

    post.into_result("Dual-phase boot (Exp 112)", sec2_before, sec2_after, notes)
}
