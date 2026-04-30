// SPDX-License-Identifier: AGPL-3.0-or-later
//! QMD v3.0 (Ada, Hopper — `cl_c6c0qmd`/`cl_c7c0qmd` layout).

use super::field::qmd_set_field;
use super::sm_config::gv100_sm_config_smem_size;
use super::types::{MAX_CBUFS, QMD_SIZE_WORDS, QmdParams};

/// Build a QMD v3.0 (Ampere SM80+) for compute dispatch.
///
/// QMD v3.0 has a **completely different** field layout from v2.1/v2.2.
/// Field positions from `cl_c6c0qmd.h` / `cl_c7c0qmd.h` (NVIDIA open headers):
///
/// - MW(134:134): `SM_GLOBAL_CACHING_ENABLE` (1 = ENABLE)
/// - MW(415:384): `CTA_RASTER_WIDTH` (32 bits)
/// - MW(431:416): `CTA_RASTER_HEIGHT` (16 bits)
/// - MW(463:448): `CTA_RASTER_DEPTH` (16 bits)
/// - MW(561:544): `SHARED_MEMORY_SIZE` (18 bits, 256-byte aligned)
/// - MW(579:576): `QMD_VERSION`=0
/// - MW(583:580): `QMD_MAJOR_VERSION`=3
/// - MW(607:592): `CTA_THREAD_DIMENSION0` (16 bits)
/// - MW(623:608): `CTA_THREAD_DIMENSION1` (16 bits)
/// - MW(639:624): `CTA_THREAD_DIMENSION2` (16 bits)
/// - MW((640+i):(640+i)): `CONSTANT_BUFFER_VALID(i)` (1 bit)
/// - MW(656:648): `REGISTER_COUNT_V` (9 bits)
/// - MW(759:736): `SHADER_LOCAL_MEMORY_LOW_SIZE` (24 bits)
/// - MW(767:763): `BARRIER_COUNT` (5 bits)
/// - MW((1055+i*64):(1024+i*64)): `CONSTANT_BUFFER_ADDR_LOWER(i)` (32 bits)
/// - MW((1072+i*64):(1056+i*64)): `CONSTANT_BUFFER_ADDR_UPPER(i)` (17 bits)
/// - MW((1087+i*64):(1075+i*64)): `CONSTANT_BUFFER_SIZE_SHIFTED4(i)` (13 bits)
/// - MW(1567:1536): `PROGRAM_ADDRESS_LOWER` (32 bits)
/// - MW(1584:1568): `PROGRAM_ADDRESS_UPPER` (17 bits)
#[must_use]
pub fn build_qmd_v30(params: &QmdParams) -> [u32; QMD_SIZE_WORDS] {
    let mut q = [0u32; QMD_SIZE_WORDS];

    // QMD_VERSION MW(579:576) = 0, QMD_MAJOR_VERSION MW(583:580) = 3
    qmd_set_field(&mut q, 576, 4, 0);
    qmd_set_field(&mut q, 580, 4, 3);

    // SM_GLOBAL_CACHING_ENABLE [134] = 1
    qmd_set_field(&mut q, 134, 1, 1);

    // API_VISIBLE_CALL_LIMIT MW(378:378) = NO_CHECK (1)
    qmd_set_field(&mut q, 378, 1, 1);

    // SAMPLER_INDEX MW(382:382) = INDEPENDENTLY (0) — default, explicit for clarity

    // CTA raster dimensions (grid)
    qmd_set_field(&mut q, 384, 32, u64::from(params.grid.x));
    qmd_set_field(&mut q, 416, 16, u64::from(params.grid.y));
    qmd_set_field(&mut q, 448, 16, u64::from(params.grid.z));

    // CTA thread dimensions (workgroup)
    qmd_set_field(&mut q, 592, 16, u64::from(params.workgroup[0]));
    qmd_set_field(&mut q, 608, 16, u64::from(params.workgroup[1]));
    qmd_set_field(&mut q, 624, 16, u64::from(params.workgroup[2]));

    // REGISTER_COUNT_V [656:648] (9 bits)
    let reg_count = params.gpr_count.min(511);
    qmd_set_field(&mut q, 648, 9, u64::from(reg_count));

    // BARRIER_COUNT [767:763] (5 bits)
    qmd_set_field(&mut q, 763, 5, u64::from(params.barrier_count));

    // SHARED_MEMORY_SIZE [561:544] (18 bits, 256-byte aligned)
    let shared_aligned = (params.shared_mem_bytes + 255) & !255;
    qmd_set_field(&mut q, 544, 18, u64::from(shared_aligned));

    // SM config shared memory partition sizes (Volta+ SKED requirement).
    // The hardware uses these to determine the shared memory partition
    // per SM, which affects CTA scheduling. Without them, the SKED may
    // refuse to schedule CTAs even with 0 shared memory.
    //
    // Encoding: partition_bytes = (value - 1) * 4096; value 1 = 0KB, 3 = 8KB.
    let smem_cfg = gv100_sm_config_smem_size(params.shared_mem_bytes);
    // MIN_SM_CONFIG_SHARED_MEM_SIZE MW(567:562) — 6 bits
    qmd_set_field(&mut q, 562, 6, smem_cfg);
    // MAX_SM_CONFIG_SHARED_MEM_SIZE MW(574:569) — 6 bits (use max partition)
    qmd_set_field(&mut q, 569, 6, gv100_sm_config_smem_size(96 * 1024));
    // TARGET_SM_CONFIG_SHARED_MEM_SIZE MW(662:657) — 6 bits
    qmd_set_field(&mut q, 657, 6, smem_cfg);

    // SHADER_LOCAL_MEMORY_LOW_SIZE [759:736] (24 bits, per-thread bytes)
    qmd_set_field(&mut q, 736, 24, u64::from(params.local_mem_low_bytes));

    // PROGRAM_ADDRESS [1584:1536] — 49-bit VA, lower 32 + upper 17
    qmd_set_field(&mut q, 1536, 32, params.shader_va & 0xFFFF_FFFF);
    qmd_set_field(&mut q, 1568, 17, params.shader_va >> 32);

    // Constant buffer bindings: v3.0 layout (same CBUF positions as v2.3)
    //   VALID(i):          bit 640+i
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
