// SPDX-License-Identifier: AGPL-3.0-or-later
//! QMD v5.0 (Blackwell SM100+, 384-byte `clcec0qmd` layout).

use super::field::qmd_set_field_dyn;
use super::sm_config::gv100_sm_config_smem_size;
use super::types::{MAX_CBUFS, QMD_V4_PLUS_SIZE_WORDS, QmdParams};

/// Build a QMD v5.0 (Blackwell SM120+) for compute dispatch.
///
/// QMD v5.0 is 384 bytes (96 words = 3072 bits), a completely different
/// layout from v3.0.  Field positions from the official NVIDIA open header
/// `clcec0qmd.h` (NVCEC0_QMDV05_00):
///
/// - MW(153:151): `QMD_TYPE` = GRID_CTA (2)
/// - MW(455:448): `SASS_VERSION` (8 bits)
/// - MW(456:456): `API_VISIBLE_CALL_LIMIT` = NO_CHECK (1)
/// - MW(467:464): `QMD_MINOR_VERSION` = 0
/// - MW(471:468): `QMD_MAJOR_VERSION` = 5
/// - MW(472:477): Cache invalidation flags (6 × 1-bit)
/// - MW(1055:1024): `PROGRAM_ADDRESS_LOWER_SHIFTED4` (32 bits)
/// - MW(1076:1056): `PROGRAM_ADDRESS_UPPER_SHIFTED4` (21 bits)
/// - MW(1103:1088): `CTA_THREAD_DIMENSION0` (16 bits)
/// - MW(1119:1104): `CTA_THREAD_DIMENSION1` (16 bits)
/// - MW(1127:1120): `CTA_THREAD_DIMENSION2` (8 bits)
/// - MW(1136:1128): `REGISTER_COUNT` (9 bits)
/// - MW(1141:1137): `BARRIER_COUNT` (5 bits)
/// - MW(1162:1152): `SHARED_MEMORY_SIZE_SHIFTED7` (11 bits)
/// - MW(1199:1184): `SHADER_LOCAL_MEMORY_LOW_SIZE_SHIFTED4` (16 bits)
/// - MW(1279:1248): `GRID_WIDTH` (32 bits)
/// - MW(1295:1280): `GRID_HEIGHT` (16 bits)
/// - MW(1327:1312): `GRID_DEPTH` (16 bits)
/// - Per-CBUF(i):
///   - `ADDR_LOWER_SHIFTED6(i)`: MW((1375+i*64):(1344+i*64)) — 32 bits
///   - `ADDR_UPPER_SHIFTED6(i)`: MW((1394+i*64):(1376+i*64)) — 19 bits
///   - `SIZE_SHIFTED4(i)`:       MW((1407+i*64):(1395+i*64)) — 13 bits
///   - `VALID(i)`:               MW((1856+i*4):(1856+i*4))   — 1 bit (separate)
///   - `INVALIDATE(i)`:          MW((1859+i*4):(1859+i*4))   — 1 bit (separate)
#[must_use]
pub fn build_qmd_v50(params: &QmdParams) -> Vec<u32> {
    let mut q = vec![0u32; QMD_V4_PLUS_SIZE_WORDS];

    // QMD_GROUP_ID MW(149:144) = 0x1f (required by SKED, per NVK)
    qmd_set_field_dyn(&mut q, 144, 6, 0x1f);

    // QMD_TYPE MW(153:151) = GRID_CTA (2)
    qmd_set_field_dyn(&mut q, 151, 3, 2);

    // SASS_VERSION MW(455:448) — set to 0 (driver-level, not ISA version)
    // API_VISIBLE_CALL_LIMIT MW(456:456) = NO_CHECK (1)
    qmd_set_field_dyn(&mut q, 456, 1, 1);
    // SAMPLER_INDEX MW(457:457) = INDEPENDENTLY (0) — default

    // QMD_MINOR_VERSION MW(467:464) = 0, QMD_MAJOR_VERSION MW(471:468) = 5
    qmd_set_field_dyn(&mut q, 464, 4, 0);
    qmd_set_field_dyn(&mut q, 468, 4, 5);

    // Cache invalidation flags MW(472:477) — all TRUE for first dispatch
    qmd_set_field_dyn(&mut q, 472, 1, 1); // INVALIDATE_TEXTURE_HEADER_CACHE
    qmd_set_field_dyn(&mut q, 473, 1, 1); // INVALIDATE_TEXTURE_SAMPLER_CACHE
    qmd_set_field_dyn(&mut q, 474, 1, 1); // INVALIDATE_TEXTURE_DATA_CACHE
    qmd_set_field_dyn(&mut q, 475, 1, 1); // INVALIDATE_SHADER_DATA_CACHE
    qmd_set_field_dyn(&mut q, 476, 1, 1); // INVALIDATE_INSTRUCTION_CACHE
    qmd_set_field_dyn(&mut q, 477, 1, 1); // INVALIDATE_SHADER_CONSTANT_CACHE

    // PROGRAM_ADDRESS_LOWER_SHIFTED4 MW(1055:1024) — 32 bits
    // PROGRAM_ADDRESS_UPPER_SHIFTED4 MW(1076:1056) — 21 bits
    let addr_shifted4 = params.shader_va >> 4;
    qmd_set_field_dyn(&mut q, 1024, 32, addr_shifted4 & 0xFFFF_FFFF);
    qmd_set_field_dyn(&mut q, 1056, 21, addr_shifted4 >> 32);

    // CTA_THREAD_DIMENSION0 MW(1103:1088) — 16 bits
    // CTA_THREAD_DIMENSION1 MW(1119:1104) — 16 bits
    // CTA_THREAD_DIMENSION2 MW(1127:1120) — 8 bits
    qmd_set_field_dyn(&mut q, 1088, 16, u64::from(params.workgroup[0]));
    qmd_set_field_dyn(&mut q, 1104, 16, u64::from(params.workgroup[1]));
    qmd_set_field_dyn(&mut q, 1120, 8, u64::from(params.workgroup[2]));

    // REGISTER_COUNT MW(1136:1128) — 9 bits
    let reg_count = params.gpr_count.min(511);
    qmd_set_field_dyn(&mut q, 1128, 9, u64::from(reg_count));

    // BARRIER_COUNT MW(1141:1137) — 5 bits
    qmd_set_field_dyn(&mut q, 1137, 5, u64::from(params.barrier_count));

    // SHARED_MEMORY_SIZE_SHIFTED7 MW(1162:1152) — 11 bits
    let shared_aligned = (params.shared_mem_bytes + 127) & !127;
    qmd_set_field_dyn(&mut q, 1152, 11, u64::from(shared_aligned >> 7));

    // MIN/MAX/TARGET_SM_CONFIG_SHARED_MEM_SIZE (6 bits each)
    // Use the same gv100 encoding as v3.0: round up to a power-of-2
    // multiple of 4KB, then encode as `size/4096 + 1`.
    let smem_cfg = gv100_sm_config_smem_size(params.shared_mem_bytes);
    // MIN_SM_CONFIG_SHARED_MEM_SIZE MW(1168:1163) — smallest partition
    qmd_set_field_dyn(&mut q, 1163, 6, smem_cfg);
    // MAX_SM_CONFIG_SHARED_MEM_SIZE MW(1174:1169) — largest available
    qmd_set_field_dyn(&mut q, 1169, 6, gv100_sm_config_smem_size(96 * 1024));
    // TARGET_SM_CONFIG_SHARED_MEM_SIZE MW(1180:1175)
    qmd_set_field_dyn(&mut q, 1175, 6, smem_cfg);

    // SHADER_LOCAL_MEMORY_LOW_SIZE_SHIFTED4 MW(1199:1184) — 16 bits
    qmd_set_field_dyn(&mut q, 1184, 16, u64::from(params.local_mem_low_bytes >> 4));

    // GRID_WIDTH MW(1279:1248) — 32 bits
    // GRID_HEIGHT MW(1295:1280) — 16 bits
    // GRID_DEPTH MW(1327:1312) — 16 bits
    qmd_set_field_dyn(&mut q, 1248, 32, u64::from(params.grid.x));
    qmd_set_field_dyn(&mut q, 1280, 16, u64::from(params.grid.y));
    qmd_set_field_dyn(&mut q, 1312, 16, u64::from(params.grid.z));

    // Constant buffer bindings — v5.0 layout per clcec0qmd.h
    for cb in &params.cbufs {
        let idx = cb.index as usize;
        if idx < MAX_CBUFS {
            let addr_shifted6 = cb.addr >> 6;

            // ADDR_LOWER_SHIFTED6(i): MW((1375+i*64):(1344+i*64)) — 32 bits
            let addr_base = 1344 + idx * 64;
            qmd_set_field_dyn(&mut q, addr_base, 32, addr_shifted6 & 0xFFFF_FFFF);
            // ADDR_UPPER_SHIFTED6(i): MW((1394+i*64):(1376+i*64)) — 19 bits
            qmd_set_field_dyn(&mut q, addr_base + 32, 19, addr_shifted6 >> 32);
            // SIZE_SHIFTED4(i): MW((1407+i*64):(1395+i*64)) — 13 bits
            qmd_set_field_dyn(&mut q, addr_base + 51, 13, u64::from(cb.size >> 4));

            // VALID(i): MW((1856+i*4):(1856+i*4)) — 1 bit (separate section)
            qmd_set_field_dyn(&mut q, 1856 + idx * 4, 1, 1);
            // NVK does NOT set per-CBUF INVALIDATE; the global
            // INVALIDATE_SHADER_CONSTANT_CACHE (MW 477) suffices.
        }
    }

    q
}
