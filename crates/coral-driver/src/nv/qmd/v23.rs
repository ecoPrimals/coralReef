// SPDX-License-Identifier: AGPL-3.0-or-later
//! QMD v2.3 (Ampere SM80–89).

use super::field::qmd_set_field;
use super::sm_config::gv100_sm_config_smem_size;
use super::types::{MAX_CBUFS, QMD_SIZE_WORDS, QmdParams};

/// Build a QMD v2.3 (Ampere SM80-89) for compute dispatch.
///
/// NVK and CUDA use v2.3 for Ampere — not v3.0. The CWD on Ampere hardware
/// may not correctly process v3.0 CBUF descriptors.
///
/// Most field positions are shared with v3.0, but these differ:
/// - MW(579:576): `QMD_VERSION`=3, MW(583:580): `QMD_MAJOR_VERSION`=2
/// - MW(951:928): `SHADER_LOCAL_MEMORY_LOW_SIZE` (24 bits)
/// - MW(959:955): `BARRIER_COUNT` (5 bits)
#[must_use]
pub fn build_qmd_v23(params: &QmdParams) -> [u32; QMD_SIZE_WORDS] {
    let mut q = [0u32; QMD_SIZE_WORDS];

    // QMD_VERSION MW(579:576) = 3, QMD_MAJOR_VERSION MW(583:580) = 2
    qmd_set_field(&mut q, 576, 4, 3);
    qmd_set_field(&mut q, 580, 4, 2);

    // SM_GLOBAL_CACHING_ENABLE [134] = 1
    qmd_set_field(&mut q, 134, 1, 1);

    // CTA raster dimensions (grid) — same as v3.0
    qmd_set_field(&mut q, 384, 32, u64::from(params.grid.x));
    qmd_set_field(&mut q, 416, 16, u64::from(params.grid.y));
    qmd_set_field(&mut q, 448, 16, u64::from(params.grid.z));

    // CTA thread dimensions (workgroup) — same as v3.0
    qmd_set_field(&mut q, 592, 16, u64::from(params.workgroup[0]));
    qmd_set_field(&mut q, 608, 16, u64::from(params.workgroup[1]));
    qmd_set_field(&mut q, 624, 16, u64::from(params.workgroup[2]));

    // REGISTER_COUNT_V [656:648] (9 bits) — same as v3.0
    let reg_count = params.gpr_count.min(511);
    qmd_set_field(&mut q, 648, 9, u64::from(reg_count));

    // API_VISIBLE_CALL_LIMIT MW(378:378) = NO_CHECK (1)
    qmd_set_field(&mut q, 378, 1, 1);

    // SHARED_MEMORY_SIZE [561:544] (18 bits) — same as v3.0
    let shared_aligned = (params.shared_mem_bytes + 255) & !255;
    qmd_set_field(&mut q, 544, 18, u64::from(shared_aligned));

    // SM config shared memory partition sizes (Volta+ SKED requirement).
    let smem_cfg = gv100_sm_config_smem_size(params.shared_mem_bytes);
    qmd_set_field(&mut q, 562, 6, smem_cfg);
    qmd_set_field(&mut q, 569, 6, gv100_sm_config_smem_size(96 * 1024));
    qmd_set_field(&mut q, 657, 6, smem_cfg);

    // SHADER_LOCAL_MEMORY_LOW_SIZE [951:928] (24 bits) — v2.3 position
    qmd_set_field(&mut q, 928, 24, u64::from(params.local_mem_low_bytes));

    // BARRIER_COUNT [959:955] (5 bits) — v2.3 position
    qmd_set_field(&mut q, 955, 5, u64::from(params.barrier_count));

    // PROGRAM_ADDRESS — same as v3.0
    qmd_set_field(&mut q, 1536, 32, params.shader_va & 0xFFFF_FFFF);
    qmd_set_field(&mut q, 1568, 17, params.shader_va >> 32);

    // Constant buffer bindings — same positions as v3.0
    //
    // Per-CBUF fields (clc7c0qmd.h QMDV02_03):
    //   ADDR_LOWER(i):     MW((1055+i*64):(1024+i*64)) — 32 bits
    //   ADDR_UPPER(i):     MW((1072+i*64):(1056+i*64)) — 17 bits
    //   PREFETCH_POST(i):  MW((1073+i*64):(1073+i*64)) — 1 bit
    //   INVALIDATE(i):     MW((1074+i*64):(1074+i*64)) — 1 bit
    //   SIZE_SHIFTED4(i):  MW((1087+i*64):(1075+i*64)) — 13 bits
    for cb in &params.cbufs {
        let idx = cb.index as usize;
        if idx < MAX_CBUFS {
            qmd_set_field(&mut q, 640 + idx, 1, 1);
            let base = 1024 + idx * 64;
            qmd_set_field(&mut q, base, 32, cb.addr & 0xFFFF_FFFF);
            qmd_set_field(&mut q, base + 32, 17, cb.addr >> 32);
            qmd_set_field(&mut q, base + 50, 1, 1); // INVALIDATE = TRUE
            qmd_set_field(&mut q, base + 51, 13, u64::from(cb.size >> 4));
        }
    }

    q
}
