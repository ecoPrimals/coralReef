// SPDX-License-Identifier: AGPL-3.0-only

//! WPR (Write Protected Region) construction and ACR descriptor patching.

use crate::vfio::dma::DmaBuffer;

use super::firmware::dma_idx;
use super::firmware::{AcrFirmwareSet, ParsedAcrFirmware};

/// IOVA for ACR firmware DMA buffer — must be within the channel's 2MB
/// identity-mapped page table range (PT0 covers IOVAs 0x1000..0x1FF000).
pub(crate) const ACR_IOVA_BASE: u64 = 0x18_0000;

// ── WPR construction (080b) ──────────────────────────────────────────

/// Falcon ID constants used in WPR LSF (Lazy Secure Falcon) descriptors.
/// From Nouveau's `nvkm_acr_lsf_id` enum.
pub mod falcon_id {
    /// WPR LSF / ACR falcon ID for PMU.
    pub const PMU: u32 = 0;
    /// WPR LSF / ACR falcon ID for FECS (front-end / scheduling falcon).
    pub const FECS: u32 = 2;
    /// WPR LSF / ACR falcon ID for GPCCS (GPC context-switch falcon).
    pub const GPCCS: u32 = 3;
    /// WPR LSF / ACR falcon ID for SEC2 (secure / ACR orchestration falcon).
    pub const SEC2: u32 = 7;
    /// Sentinel: no valid falcon (`nvkm_acr_lsf_id` style).
    pub const INVALID: u32 = 0xFFFF_FFFF;
}

/// DMA buffers allocated for the ACR boot chain.
pub struct AcrDmaContext {
    /// DMA-backed buffer holding the ACR ucode image for SEC2/HS load.
    pub acr_ucode: DmaBuffer,
}

/// Build flcn_bl_dmem_desc_v1 for SEC2 BL — tells BL where to find ACR firmware.
pub(crate) fn build_bl_dmem_desc(
    code_dma_base: u64,
    data_dma_base: u64,
    parsed: &ParsedAcrFirmware,
) -> Vec<u8> {
    let mut desc = vec![0u8; 76];
    let w32 = |buf: &mut [u8], off: usize, val: u32| {
        buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
    };
    let w64 = |buf: &mut [u8], off: usize, val: u64| {
        buf[off..off + 8].copy_from_slice(&val.to_le_bytes());
    };

    // reserved[4] at 0..16: zeroes
    // signature[4] at 16..32: zeroes
    // ctx_dma at 32: SEC2 uses DMA index 6 (instance-block-translated),
    // NOT index 0 (physical DMA). This is critical — wrong index causes BL
    // to read from VRAM instead of system memory.
    w32(&mut desc, 32, dma_idx::VIRT);
    // code_dma_base at 36 (u64, packed)
    w64(&mut desc, 36, code_dma_base);
    // non_sec_code_off at 44
    w32(&mut desc, 44, parsed.load_header.non_sec_code_off);
    // non_sec_code_size at 48
    w32(&mut desc, 48, parsed.load_header.non_sec_code_size);
    // sec_code_off at 52 (first app code offset)
    let sec_off = parsed.load_header.apps.first().map(|a| a.0).unwrap_or(0);
    w32(&mut desc, 52, sec_off);
    // sec_code_size at 56 (first app code size)
    let sec_size = parsed.load_header.apps.first().map(|a| a.1).unwrap_or(0);
    w32(&mut desc, 56, sec_size);
    // code_entry_point at 60
    w32(&mut desc, 60, 0);
    // data_dma_base at 64 (u64, packed)
    w64(&mut desc, 64, data_dma_base);
    // data_size at 72
    w32(&mut desc, 72, parsed.load_header.data_size);

    desc
}

/// Patch the ACR descriptor within the ACR payload's data section.
///
/// The data section of `ucode_load.bin` contains a `flcn_acr_desc_v1` that
/// must be patched with WPR region addresses before loading. For GP102/GV100:
///
/// `flcn_acr_desc_v1` layout (from Nouveau `nvfw/acr.h`):
///   0x000: reserved_dmem[0x200]  (512 bytes)
///   0x200: signatures[4]          (16 bytes)
///   0x210: wpr_region_id          (u32)
///   0x214: wpr_offset             (u32)
///   0x218: mmu_memory_range       (u32)
///   0x21C: regions.no_regions     (u32)
///   0x220: region_props[0].start_addr  (u32, addr >> 8)
///   0x224: region_props[0].end_addr    (u32, addr >> 8)
///   0x228: region_props[0].region_id   (u32)
///   0x22C: region_props[0].read_mask   (u32)
///   0x230: region_props[0].write_mask  (u32)
///   0x234: region_props[0].client_mask (u32)
///   0x238: region_props[0].shadow_mem_start_addr (u32, addr >> 8)
///   0x23C: region_props[1]  (28 bytes, left zeroed)
///   0x258: ucode_blob_size  (u32)
///   0x260: ucode_blob_base  (u64, 8-byte aligned)
pub(crate) fn patch_acr_desc(payload: &mut [u8], data_off: usize, wpr_start: u64, wpr_end: u64) {
    let needed = data_off + 0x268;
    if needed > payload.len() {
        tracing::warn!(
            data_off,
            payload_len = payload.len(),
            needed,
            "ACR data section too small for v1 descriptor patch"
        );
        return;
    }
    let w32 = |buf: &mut [u8], off: usize, val: u32| {
        buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
    };
    let w64 = |buf: &mut [u8], off: usize, val: u64| {
        buf[off..off + 8].copy_from_slice(&val.to_le_bytes());
    };
    let base = data_off;

    w32(payload, base + 0x210, 1); // wpr_region_id
    w32(payload, base + 0x21C, 2); // no_regions
    w32(payload, base + 0x220, (wpr_start >> 8) as u32); // region[0].start_addr
    w32(payload, base + 0x224, (wpr_end >> 8) as u32); // region[0].end_addr
    w32(payload, base + 0x228, 1); // region[0].region_id
    w32(payload, base + 0x22C, 0xF); // region[0].read_mask
    w32(payload, base + 0x230, 0xC); // region[0].write_mask
    w32(payload, base + 0x234, 0x2); // region[0].client_mask
    w32(payload, base + 0x238, (wpr_start >> 8) as u32); // region[0].shadow_mem_start

    // ucode_blob_base/size: point ACR at the entire WPR region
    let wpr_size = wpr_end - wpr_start;
    w32(payload, base + 0x258, wpr_size as u32); // ucode_blob_size
    w64(payload, base + 0x260, wpr_start); // ucode_blob_base
}

/// Build a complete WPR (Write-Protected Region) containing FECS and GPCCS
/// firmware for ACR verification and bootstrap.
///
/// Layout matches Nouveau's `gp102_acr_wpr_layout` + `gp102_acr_wpr_build`:
///   [0..264]     wpr_header_v1 array (11 max entries × 24B)
///   [264..512]   padding (ALIGN to 256)
///   [512..768]   shared sub-WPR headers (0x100 bytes, zeros)
///   [768..]      per-falcon: LSB header (240B) → image (4K-aligned) → BLD (256B)
///
/// Returns the serialized WPR bytes.
pub fn build_wpr(fw: &AcrFirmwareSet, wpr_vram_base: u64) -> Vec<u8> {
    let align = |v: usize, a: usize| (v + a - 1) & !(a - 1);
    let w32 = |buf: &mut Vec<u8>, off: usize, val: u32| {
        if off + 4 > buf.len() {
            buf.resize(off + 4, 0);
        }
        buf[off..off + 4].copy_from_slice(&val.to_le_bytes());
    };

    // Build per-falcon images: bl_bytes + inst_bytes + data_bytes
    let fecs_img = [
        fw.fecs_bl.as_slice(),
        fw.fecs_inst.as_slice(),
        fw.fecs_data.as_slice(),
    ]
    .concat();
    let gpccs_img = [
        fw.gpccs_bl.as_slice(),
        fw.gpccs_inst.as_slice(),
        fw.gpccs_data.as_slice(),
    ]
    .concat();

    // Phase 1: compute layout offsets (gp102_acr_wpr_layout)
    let mut wpr: usize = 0;

    // Header table: 11 entries × 24 bytes, aligned to 256
    wpr += 11 * 24;
    wpr = align(wpr, 256);
    // Shared sub-WPR headers
    wpr += 0x100;

    // FECS
    wpr = align(wpr, 256);
    let fecs_lsb_off = wpr;
    wpr += 240; // sizeof(lsb_header_v1)

    wpr = align(wpr, 4096);
    let fecs_img_off = wpr;
    wpr += fecs_img.len();

    wpr = align(wpr, 256);
    let fecs_bld_off = wpr;
    wpr += 256; // ALIGN(sizeof(flcn_bl_dmem_desc_v2), 256)

    // GPCCS
    wpr = align(wpr, 256);
    let gpccs_lsb_off = wpr;
    wpr += 240;

    wpr = align(wpr, 4096);
    let gpccs_img_off = wpr;
    wpr += gpccs_img.len();

    wpr = align(wpr, 256);
    let gpccs_bld_off = wpr;
    wpr += 256;

    let wpr_total = wpr;
    let mut buf = vec![0u8; wpr_total];

    // Phase 2: write WPR headers (gp102_acr_wpr_build)
    // FECS header at offset 0
    w32(&mut buf, 0, falcon_id::FECS); // falcon_id
    w32(&mut buf, 4, fecs_lsb_off as u32); // lsb_offset
    w32(&mut buf, 8, falcon_id::SEC2); // bootstrap_owner
    w32(&mut buf, 12, 0); // lazy_bootstrap = FALSE (auto-boot)
    w32(&mut buf, 16, 0); // bin_version
    w32(&mut buf, 20, 1); // status = WPR_HEADER_V1_STATUS_COPY

    // GPCCS header at offset 24
    w32(&mut buf, 24, falcon_id::GPCCS);
    w32(&mut buf, 28, gpccs_lsb_off as u32);
    w32(&mut buf, 32, falcon_id::SEC2);
    w32(&mut buf, 36, 0); // lazy_bootstrap = FALSE
    w32(&mut buf, 40, 0); // bin_version
    w32(&mut buf, 44, 1); // status = COPY

    // Sentinel at offset 48
    w32(&mut buf, 48, falcon_id::INVALID);

    // Phase 3: write LSB headers + images + BLDs
    // Helper: write LSB header for a falcon
    let write_lsb = |buf: &mut Vec<u8>,
                     lsb_off: usize,
                     img_off: usize,
                     bld_off: usize,
                     sig: &[u8],
                     bl_size: usize,
                     inst_size: usize,
                     data_size: usize,
                     fid: u32| {
        // Copy signature (up to 192 bytes from sig file)
        let sig_len = sig.len().min(192);
        buf[lsb_off..lsb_off + sig_len].copy_from_slice(&sig[..sig_len]);
        // Populate lsf_ucode_desc_v1 metadata fields within the signature area.
        // The sig file only contains crypto keys (bytes 0-63). Fields at 64+
        // must be filled by the host (Nouveau does this in build_bld_desc).
        if sig_len < 112 {
            buf.resize(buf.len().max(lsb_off + 112), 0);
        }
        let s = lsb_off;
        // b_prd_present (offset 64) — already set in sig file
        // b_dbg_present (offset 68) — already set in sig file
        buf[s + 72..s + 76].copy_from_slice(&fid.to_le_bytes()); // falcon_id
        buf[s + 76..s + 80].copy_from_slice(&1u32.to_le_bytes()); // bsupported = 1
        // status (offset 80) — keep from sig file
        // elf_section_names_idx (offset 84) — 0
        buf[s + 88..s + 92].copy_from_slice(&(bl_size as u32).to_le_bytes()); // app_resident_code_off
        buf[s + 92..s + 96].copy_from_slice(&(inst_size as u32).to_le_bytes()); // app_resident_code_size
        buf[s + 96..s + 100].copy_from_slice(&((bl_size + inst_size) as u32).to_le_bytes()); // app_resident_data_off
        buf[s + 100..s + 104].copy_from_slice(&(data_size as u32).to_le_bytes()); // app_resident_data_size

        // LSB tail starts at lsb_off + 192
        let t = lsb_off + 192;
        let img_size = bl_size + inst_size + data_size;
        // ucode_off: offset of image relative to WPR base
        buf[t..t + 4].copy_from_slice(&(img_off as u32).to_le_bytes());
        // ucode_size
        buf[t + 4..t + 8].copy_from_slice(&(img_size as u32).to_le_bytes());
        // data_size
        buf[t + 8..t + 12].copy_from_slice(&(data_size as u32).to_le_bytes());
        // bl_code_size
        buf[t + 12..t + 16].copy_from_slice(&(bl_size as u32).to_le_bytes());
        // bl_imem_off = 0 (BL loads at IMEM 0)
        buf[t + 16..t + 20].copy_from_slice(&0u32.to_le_bytes());
        // bl_data_off = bld_off relative to WPR
        buf[t + 20..t + 24].copy_from_slice(&(bld_off as u32).to_le_bytes());
        // bl_data_size = 256
        buf[t + 24..t + 28].copy_from_slice(&256u32.to_le_bytes());
        // app_code_off = bl_size (app code follows BL in image)
        buf[t + 28..t + 32].copy_from_slice(&(bl_size as u32).to_le_bytes());
        // app_code_size = inst_size
        buf[t + 32..t + 36].copy_from_slice(&(inst_size as u32).to_le_bytes());
        // app_data_off = bl_size + inst_size
        buf[t + 36..t + 40].copy_from_slice(&((bl_size + inst_size) as u32).to_le_bytes());
        // app_data_size = data_size
        buf[t + 40..t + 44].copy_from_slice(&(data_size as u32).to_le_bytes());
        // flags = 0
        buf[t + 44..t + 48].copy_from_slice(&0u32.to_le_bytes());
    };

    // Write FECS LSB + image + BLD
    write_lsb(
        &mut buf,
        fecs_lsb_off,
        fecs_img_off,
        fecs_bld_off,
        &fw.fecs_sig,
        fw.fecs_bl.len(),
        fw.fecs_inst.len(),
        fw.fecs_data.len(),
        falcon_id::FECS,
    );
    buf[fecs_img_off..fecs_img_off + fecs_img.len()].copy_from_slice(&fecs_img);

    // FECS BLD (flcn_bl_dmem_desc_v2): point code/data at WPR-relative addresses
    let fecs_code_dma = wpr_vram_base + fecs_img_off as u64;
    let fecs_data_dma =
        wpr_vram_base + fecs_img_off as u64 + fw.fecs_bl.len() as u64 + fw.fecs_inst.len() as u64;
    write_bl_dmem_desc_v2(
        &mut buf,
        fecs_bld_off,
        fecs_code_dma,
        fecs_data_dma,
        0,
        fw.fecs_bl.len() as u32,
        fw.fecs_bl.len() as u32,
        fw.fecs_inst.len() as u32,
        0,
        fw.fecs_data.len() as u32,
    );

    // Write GPCCS LSB + image + BLD
    write_lsb(
        &mut buf,
        gpccs_lsb_off,
        gpccs_img_off,
        gpccs_bld_off,
        &fw.gpccs_sig,
        fw.gpccs_bl.len(),
        fw.gpccs_inst.len(),
        fw.gpccs_data.len(),
        falcon_id::GPCCS,
    );
    buf[gpccs_img_off..gpccs_img_off + gpccs_img.len()].copy_from_slice(&gpccs_img);

    let gpccs_code_dma = wpr_vram_base + gpccs_img_off as u64;
    let gpccs_data_dma = wpr_vram_base
        + gpccs_img_off as u64
        + fw.gpccs_bl.len() as u64
        + fw.gpccs_inst.len() as u64;
    write_bl_dmem_desc_v2(
        &mut buf,
        gpccs_bld_off,
        gpccs_code_dma,
        gpccs_data_dma,
        0,
        fw.gpccs_bl.len() as u32,
        fw.gpccs_bl.len() as u32,
        fw.gpccs_inst.len() as u32,
        0,
        fw.gpccs_data.len() as u32,
    );

    tracing::info!(
        wpr_size = wpr_total,
        fecs_lsb = fecs_lsb_off,
        fecs_img = fecs_img_off,
        fecs_img_size = fecs_img.len(),
        fecs_bld = fecs_bld_off,
        gpccs_lsb = gpccs_lsb_off,
        gpccs_img = gpccs_img_off,
        gpccs_img_size = gpccs_img.len(),
        gpccs_bld = gpccs_bld_off,
        "WPR layout"
    );

    buf
}
/// Write a `flcn_bl_dmem_desc_v2` (84 bytes packed, padded to 256) into `buf` at `off`.
#[allow(clippy::too_many_arguments)]
fn write_bl_dmem_desc_v2(
    buf: &mut [u8],
    off: usize,
    code_dma_base: u64,
    data_dma_base: u64,
    non_sec_code_off: u32,
    non_sec_code_size: u32,
    sec_code_off: u32,
    sec_code_size: u32,
    code_entry_point: u32,
    data_size: u32,
) {
    let w32 = |buf: &mut [u8], o: usize, v: u32| buf[o..o + 4].copy_from_slice(&v.to_le_bytes());
    let w64 = |buf: &mut [u8], o: usize, v: u64| buf[o..o + 8].copy_from_slice(&v.to_le_bytes());

    // reserved[4] at 0..16: zeros
    // signature[4] at 16..32: zeros
    w32(buf, off + 32, 0); // ctx_dma = 0
    w64(buf, off + 36, code_dma_base);
    w32(buf, off + 44, non_sec_code_off);
    w32(buf, off + 48, non_sec_code_size);
    w32(buf, off + 52, sec_code_off);
    w32(buf, off + 56, sec_code_size);
    w32(buf, off + 60, code_entry_point);
    w64(buf, off + 64, data_dma_base);
    w32(buf, off + 72, data_size);
    // argc=0, argv=0 at 76, 80
}
