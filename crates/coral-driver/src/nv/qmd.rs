// SPDX-License-Identifier: AGPL-3.0-or-later
//! QMD (Queue Management Descriptor) construction for NVIDIA compute dispatch.
//!
//! Supports multiple QMD versions:
//! - v2.1 (256-byte, 64-word): Pascal/Volta (SM < 70)
//! - v2.2 (256-byte, 64-word): Volta/Turing (SM70-SM79)
//! - v3.0 (256-byte, 64-word): Ampere (SM80-SM99)
//! - v5.0 (384-byte, 96-word): Blackwell (SM100+)
//!
//! Includes constant buffer binding, GPR count from compiler, shared
//! memory sizing, and dispatch grid/workgroup dimensions.
//!
//! Field layout derived from Mesa NVK (`nvk_compute.c`) and the NVIDIA
//! open GPU headers.

use crate::DispatchDims;

/// QMD size in u32 words for pre-Hopper (256 bytes = 64 words).
pub const QMD_SIZE_WORDS: usize = 64;

/// QMD size in u32 words for Hopper+ / Blackwell (384 bytes = 96 words).
pub const QMD_V4_PLUS_SIZE_WORDS: usize = 96;

/// Maximum constant buffers per dispatch.
pub const MAX_CBUFS: usize = 8;

/// A constant buffer binding for the QMD.
#[derive(Debug, Clone, Copy)]
pub struct CbufBinding {
    /// CBUF slot index (0–7).
    pub index: u32,
    /// GPU virtual address of the buffer.
    pub addr: u64,
    /// Buffer size in bytes.
    pub size: u32,
}

/// Parameters for QMD construction.
///
/// All fields the compiler and driver need to pass into the QMD.
#[derive(Debug, Clone)]
pub struct QmdParams {
    /// GPU virtual address of the compiled shader binary.
    pub shader_va: u64,
    /// Dispatch grid dimensions (number of CTAs).
    pub grid: DispatchDims,
    /// Workgroup (CTA) thread dimensions.
    pub workgroup: [u32; 3],
    /// General-purpose register count (from compiler compilation info).
    pub gpr_count: u32,
    /// Shared memory size in bytes (from compiler analysis).
    pub shared_mem_bytes: u32,
    /// Barrier count used by the shader.
    pub barrier_count: u32,
    /// Per-thread local memory size in bytes (from compiler analysis).
    pub local_mem_low_bytes: u32,
    /// Constant buffer bindings (storage buffers, uniforms).
    pub cbufs: Vec<CbufBinding>,
}

impl QmdParams {
    /// Create minimal params for a simple compute dispatch.
    #[must_use]
    pub fn simple(shader_va: u64, grid: DispatchDims, gpr_count: u32) -> Self {
        Self {
            shader_va,
            grid,
            workgroup: [64, 1, 1],
            gpr_count: gpr_count.max(4),
            shared_mem_bytes: 0,
            barrier_count: 0,
            local_mem_low_bytes: 0,
            cbufs: Vec::new(),
        }
    }
}

/// Helper: set a bitfield within the QMD word array.
///
/// `bit_start` is the starting bit (0-indexed from LSB of word 0),
/// `width` is the field width in bits, `value` is the value to set.
#[expect(
    clippy::cast_possible_truncation,
    reason = "GPU QMD fields are always ≤32 bits"
)]
const fn qmd_set_field(q: &mut [u32; QMD_SIZE_WORDS], bit_start: usize, width: usize, value: u64) {
    let word_idx = bit_start / 32;
    let bit_off = bit_start % 32;

    if bit_off + width <= 32 {
        let mask = if width >= 32 {
            u32::MAX
        } else {
            (1u32 << width) - 1
        };
        q[word_idx] &= !(mask << bit_off);
        q[word_idx] |= ((value as u32) & mask) << bit_off;
    } else {
        let lo_bits = 32 - bit_off;
        let lo_mask = u32::MAX << bit_off;
        q[word_idx] = (q[word_idx] & !lo_mask) | ((value as u32) << bit_off);

        let hi_bits = width - lo_bits;
        let hi_mask = if hi_bits >= 32 {
            u32::MAX
        } else {
            (1u32 << hi_bits) - 1
        };
        q[word_idx + 1] = (q[word_idx + 1] & !hi_mask) | (((value >> lo_bits) as u32) & hi_mask);
    }
}

/// Build a QMD v2.1 (Pascal/Volta SM70) for compute dispatch.
///
/// Returns the full 64-word QMD suitable for `SEND_PCAS_A/B` submission.
///
/// Field positions are from NVIDIA open headers (`cl_c3c0qmd.h`), using
/// **bit offsets** within the 256-byte (2048-bit) QMD structure:
///
/// - Bits 0..4: `QMD_MAJOR_VERSION`=2.
/// - Bits 4..8: `QMD_VERSION`=1.
/// - Bits 224..256: `CTA_RASTER_WIDTH` (word 7).
/// - Bits 256..272: `CTA_RASTER_HEIGHT` (word 8, bits 0-15).
/// - Bits 272..288: `CTA_RASTER_DEPTH` (word 8, bits 16-31).
/// - Bits 544..560: `CTA_THREAD_DIMENSION0` (word 17, bits 0-15).
/// - Bits 560..576: `CTA_THREAD_DIMENSION1` (word 17, bits 16-31).
/// - Bits 576..592: `CTA_THREAD_DIMENSION2` (word 18, bits 0-15).
/// - Bits 592..597: `BARRIER_COUNT` (word 18, bits 16-20).
/// - Bits 608..616: `REGISTER_COUNT` (word 19, bits 0-7).
/// - Bits 640..658: `SHARED_MEMORY_SIZE` (word 20, bits 0-17).
/// - Bits 832..864: `PROGRAM_ADDRESS_LOWER` (word 26).
/// - Bits 864..896: `PROGRAM_ADDRESS_UPPER` (word 27).
/// - Per-CBUF(i): 64-bit stride starting at bit 1536+i*64.
#[must_use]
pub fn build_qmd_v21(params: &QmdParams) -> [u32; QMD_SIZE_WORDS] {
    let mut q = [0u32; QMD_SIZE_WORDS];

    // QMD_MAJOR_VERSION [3:0] = 2, QMD_VERSION [7:4] = 1
    qmd_set_field(&mut q, 0, 4, 2);
    qmd_set_field(&mut q, 4, 4, 1);
    // SAMPLER_INDEX [11:9] = INDEPENDENTLY (0)

    // CTA raster dimensions (grid)
    qmd_set_field(&mut q, 224, 32, u64::from(params.grid.x));
    qmd_set_field(&mut q, 256, 16, u64::from(params.grid.y));
    qmd_set_field(&mut q, 272, 16, u64::from(params.grid.z));

    // CTA thread dimensions (workgroup)
    qmd_set_field(&mut q, 544, 16, u64::from(params.workgroup[0]));
    qmd_set_field(&mut q, 560, 16, u64::from(params.workgroup[1]));
    qmd_set_field(&mut q, 576, 16, u64::from(params.workgroup[2]));

    // BARRIER_COUNT [596:592] (5 bits)
    qmd_set_field(&mut q, 592, 5, u64::from(params.barrier_count));

    // REGISTER_COUNT [615:608] (8 bits)
    let reg_count = params.gpr_count.min(255);
    qmd_set_field(&mut q, 608, 8, u64::from(reg_count));

    // SHARED_MEMORY_SIZE [657:640] (18 bits, 256-byte aligned)
    let shared_aligned = (params.shared_mem_bytes + 255) & !255;
    qmd_set_field(&mut q, 640, 18, u64::from(shared_aligned));

    // PROGRAM_ADDRESS_LOWER [863:832] (32 bits)
    qmd_set_field(&mut q, 832, 32, params.shader_va & 0xFFFF_FFFF);
    // PROGRAM_ADDRESS_UPPER [895:864] (32 bits)
    qmd_set_field(&mut q, 864, 32, params.shader_va >> 32);

    // Constant buffer bindings: each CBUF(i) at bit 1536 + i*64
    for cb in &params.cbufs {
        let idx = cb.index as usize;
        if idx < MAX_CBUFS {
            let base = 1536 + idx * 64;
            // ADDR_LOWER [31:0]
            qmd_set_field(&mut q, base, 32, cb.addr & 0xFFFF_FFFF);
            // ADDR_UPPER [39:32] (8 bits)
            qmd_set_field(&mut q, base + 32, 8, cb.addr >> 32);
            // SIZE_SHIFTED4 [56:40] (17 bits)
            qmd_set_field(&mut q, base + 40, 17, u64::from(cb.size >> 4));
            // VALID [57] (1 bit)
            qmd_set_field(&mut q, base + 57, 1, 1);
        }
    }

    q
}

/// Build a QMD v2.2 (Volta SM70/Turing SM75) for compute dispatch.
///
/// Same field layout as v2.1 but with `QMD_VERSION`=2.
#[must_use]
pub fn build_qmd_v22(params: &QmdParams) -> [u32; QMD_SIZE_WORDS] {
    let mut q = build_qmd_v21(params);
    qmd_set_field(&mut q, 4, 4, 2);
    q
}

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

    // SHARED_MEMORY_SIZE [561:544] (18 bits) — same as v3.0
    let shared_aligned = (params.shared_mem_bytes + 255) & !255;
    qmd_set_field(&mut q, 544, 18, u64::from(shared_aligned));

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
    // HW encoding: (size_kb / 4) + 1, where 1 means 0KB.
    // Per NVK, even shaders with 0 shared memory need a valid config.
    let smem_kb = shared_aligned / 1024;
    let smem_hw = u64::from((smem_kb / 4) + 1).min(63);
    // MIN_SM_CONFIG_SHARED_MEM_SIZE MW(1168:1163) — smallest partition
    qmd_set_field_dyn(&mut q, 1163, 6, smem_hw);
    // MAX_SM_CONFIG_SHARED_MEM_SIZE MW(1174:1169) — largest available (0x3f = 248KB)
    qmd_set_field_dyn(&mut q, 1169, 6, 0x3f);
    // TARGET_SM_CONFIG_SHARED_MEM_SIZE MW(1180:1175)
    qmd_set_field_dyn(&mut q, 1175, 6, smem_hw);

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

/// Dynamic-size variant of `qmd_set_field` for Vec-backed QMDs.
#[expect(
    clippy::cast_possible_truncation,
    reason = "GPU QMD fields are always ≤32 bits"
)]
fn qmd_set_field_dyn(q: &mut [u32], bit_start: usize, width: usize, value: u64) {
    let word_idx = bit_start / 32;
    let bit_off = bit_start % 32;
    if word_idx >= q.len() {
        return;
    }

    if bit_off + width <= 32 {
        let mask = if width >= 32 {
            u32::MAX
        } else {
            (1u32 << width) - 1
        };
        q[word_idx] &= !(mask << bit_off);
        q[word_idx] |= ((value as u32) & mask) << bit_off;
    } else {
        let lo_bits = 32 - bit_off;
        let lo_mask = u32::MAX << bit_off;
        q[word_idx] = (q[word_idx] & !lo_mask) | ((value as u32) << bit_off);

        if word_idx + 1 < q.len() {
            let hi_bits = width - lo_bits;
            let hi_mask = if hi_bits >= 32 {
                u32::MAX
            } else {
                (1u32 << hi_bits) - 1
            };
            q[word_idx + 1] =
                (q[word_idx + 1] & !hi_mask) | (((value >> lo_bits) as u32) & hi_mask);
        }
    }
}

/// Select the appropriate QMD builder for a given SM architecture.
///
/// Returns a `Vec<u32>` — 64 words for SM < 100, 96 words for SM >= 100.
///
/// Blackwell (SM 100+) requires QMD v5.0 per NVK/Mesa (`clcec0qmd.h`).
/// QMD v3.0 is silently accepted by Blackwell hardware but does not
/// actually launch compute threads — stores never materialise.
#[must_use]
pub fn build_qmd_for_sm(sm: u32, params: &QmdParams) -> Vec<u32> {
    match sm {
        0..=69 => build_qmd_v21(params).to_vec(),
        70..=79 => build_qmd_v22(params).to_vec(),
        80..=99 => build_qmd_v30(params).to_vec(),
        // SM 100+ (Blackwell) requires QMD v5.0.
        _ => build_qmd_v50_with_sm(params, sm),
    }
}

/// Encode the SM version as a SASS_VERSION byte for QMD v5.0.
///
/// NVIDIA uses `(major << 4) | minor` — e.g. SM 8.9 = 0x89, SM 12.0 = 0xC0.
/// Our internal SM numbering is `major * 10 + minor`, so SM 120 → 12.0.
#[must_use]
const fn sm_to_sass_version(sm: u32) -> u64 {
    let major = sm / 10;
    let minor = sm % 10;
    ((major << 4) | minor) as u64
}

/// Build QMD v5.0 with the correct SASS_VERSION for the target SM.
fn build_qmd_v50_with_sm(params: &QmdParams, sm: u32) -> Vec<u32> {
    let mut q = build_qmd_v50(params);
    // SASS_VERSION MW(455:448) — 8 bits
    qmd_set_field_dyn(&mut q, 448, 8, sm_to_sass_version(sm));
    q
}

/// Legacy builder — wraps `build_qmd_v30` with minimal params.
///
/// Preserved for backward compatibility with existing tests.
#[must_use]
pub fn build_compute_qmd(
    shader_va: u64,
    dims: DispatchDims,
    _code_size: u32,
) -> [u32; QMD_SIZE_WORDS] {
    let params = QmdParams::simple(shader_va, dims, 16);
    build_qmd_v30(&params)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get_field(q: &[u32], bit_start: usize, width: usize) -> u64 {
        let word_idx = bit_start / 32;
        let bit_off = bit_start % 32;
        if bit_off + width <= 32 {
            let mask = if width >= 32 {
                u32::MAX
            } else {
                (1u32 << width) - 1
            };
            u64::from((q[word_idx] >> bit_off) & mask)
        } else {
            let lo_bits = 32 - bit_off;
            let lo = u64::from(q[word_idx] >> bit_off);
            let hi_bits = width - lo_bits;
            let hi_mask = if hi_bits >= 32 {
                u32::MAX
            } else {
                (1u32 << hi_bits) - 1
            };
            let hi = u64::from(q[word_idx + 1] & hi_mask);
            lo | (hi << lo_bits)
        }
    }

    #[test]
    fn qmd_v21_version() {
        let params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        let q = build_qmd_v21(&params);
        assert_eq!(get_field(&q, 0, 4), 2, "major version");
        assert_eq!(get_field(&q, 4, 4), 1, "minor version");
    }

    #[test]
    fn qmd_v30_version() {
        let params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        let q = build_qmd_v30(&params);
        // MW(583:580) = QMD_MAJOR_VERSION = 3, MW(579:576) = QMD_VERSION = 0
        assert_eq!(get_field(&q, 580, 4), 3, "major version");
        assert_eq!(get_field(&q, 576, 4), 0, "minor version");
    }

    #[test]
    fn qmd_grid_dimensions() {
        let params = QmdParams::simple(0, DispatchDims::new(64, 8, 2), 32);
        let q = build_qmd_v21(&params);
        assert_eq!(get_field(&q, 224, 32), 64, "CTA_RASTER_WIDTH");
        assert_eq!(get_field(&q, 256, 16), 8, "CTA_RASTER_HEIGHT");
        assert_eq!(get_field(&q, 272, 16), 2, "CTA_RASTER_DEPTH");
    }

    #[test]
    fn qmd_gpr_count() {
        let params = QmdParams::simple(0, DispatchDims::linear(1), 48);
        let q = build_qmd_v21(&params);
        assert_eq!(get_field(&q, 608, 8), 48, "REGISTER_COUNT");
    }

    #[test]
    fn qmd_shader_address() {
        let va = 0x0001_0000_0000_u64;
        let params = QmdParams::simple(va, DispatchDims::linear(1), 32);
        let q = build_qmd_v21(&params);
        let addr_lo = get_field(&q, 832, 32);
        let addr_hi = get_field(&q, 864, 32);
        let reconstructed = addr_lo | (addr_hi << 32);
        assert_eq!(reconstructed, va);
    }

    #[test]
    fn qmd_cbuf_binding() {
        let mut params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        params.cbufs.push(CbufBinding {
            index: 0,
            addr: 0x2_0000_0000,
            size: 4096,
        });
        params.cbufs.push(CbufBinding {
            index: 1,
            addr: 0x3_0000_0000,
            size: 8192,
        });

        let q = build_qmd_v21(&params);

        // CBUF 0: valid, address, size
        assert_eq!(get_field(&q, 1536 + 57, 1), 1, "CBUF 0 valid");
        let cb0_lo = get_field(&q, 1536, 32);
        let cb0_hi = get_field(&q, 1536 + 32, 8);
        let cb0_addr = cb0_lo | (cb0_hi << 32);
        assert_eq!(cb0_addr, 0x2_0000_0000);
        assert_eq!(
            get_field(&q, 1536 + 40, 17),
            u64::from(4096_u32 >> 4),
            "CBUF 0 size"
        );

        // CBUF 1: valid, address, size
        assert_eq!(get_field(&q, 1600 + 57, 1), 1, "CBUF 1 valid");
        let cb1_lo = get_field(&q, 1600, 32);
        let cb1_hi = get_field(&q, 1600 + 32, 8);
        let cb1_addr = cb1_lo | (cb1_hi << 32);
        assert_eq!(cb1_addr, 0x3_0000_0000);
        assert_eq!(
            get_field(&q, 1600 + 40, 17),
            u64::from(8192_u32 >> 4),
            "CBUF 1 size"
        );
    }

    #[test]
    fn qmd_shared_memory_aligned() {
        let mut params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        params.shared_mem_bytes = 100;
        let q = build_qmd_v21(&params);
        assert_eq!(get_field(&q, 640, 18), 256, "SHARED_MEMORY_SIZE aligned");
    }

    #[test]
    fn qmd_barrier_count() {
        let mut params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        params.barrier_count = 3;
        let q = build_qmd_v21(&params);
        assert_eq!(get_field(&q, 592, 5), 3, "BARRIER_COUNT");
    }

    #[test]
    fn qmd_workgroup_size() {
        let mut params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        params.workgroup = [128, 4, 2];
        let q = build_qmd_v21(&params);
        assert_eq!(get_field(&q, 544, 16), 128, "CTA_THREAD_DIMENSION0");
        assert_eq!(get_field(&q, 560, 16), 4, "CTA_THREAD_DIMENSION1");
        assert_eq!(get_field(&q, 576, 16), 2, "CTA_THREAD_DIMENSION2");
    }

    #[test]
    fn legacy_build_compute_qmd_compat() {
        let q = build_compute_qmd(0x1_0000_0000, DispatchDims::new(64, 1, 1), 256);
        // legacy uses build_qmd_v30, so v3.0 field positions
        assert_eq!(get_field(&q, 384, 32), 64, "CTA_RASTER_WIDTH (v3.0)");
        assert_eq!(get_field(&q, 416, 16), 1, "CTA_RASTER_HEIGHT (v3.0)");
        assert_eq!(get_field(&q, 448, 16), 1, "CTA_RASTER_DEPTH (v3.0)");
    }

    #[test]
    fn qmd_size_is_64_words() {
        assert_eq!(QMD_SIZE_WORDS, 64);
        assert_eq!(QMD_SIZE_WORDS * 4, 256);
    }

    #[test]
    fn qmd_gpr_count_clamped() {
        let params = QmdParams::simple(0, DispatchDims::linear(1), 300);
        let q = build_qmd_v21(&params);
        assert_eq!(get_field(&q, 608, 8), 255, "REGISTER_COUNT clamped");
    }

    #[test]
    fn qmd_cbuf_index_above_max_ignored() {
        let mut params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        params.cbufs.push(CbufBinding {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "test value deliberately exceeds max"
            )]
            index: MAX_CBUFS as u32,
            addr: 0xDEAD_BEEF,
            size: 4096,
        });
        let q = build_qmd_v21(&params);
        // All 8 CBUF valid bits should be 0
        for i in 0..MAX_CBUFS {
            assert_eq!(
                get_field(&q, 1536 + i * 64 + 57, 1),
                0,
                "CBUF {i} should be invalid"
            );
        }
    }

    #[test]
    fn qmd_cbuf_index_7_valid() {
        let mut params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        params.cbufs.push(CbufBinding {
            index: 7,
            addr: 0x7_0000_0000,
            size: 1024,
        });
        let q = build_qmd_v21(&params);
        assert_eq!(get_field(&q, 1536 + 7 * 64 + 57, 1), 1, "CBUF 7 valid");
        assert_eq!(
            get_field(&q, 1536 + 7 * 64 + 40, 17),
            u64::from(1024_u32 >> 4)
        );
    }

    #[test]
    fn qmd_simple_workgroup_default() {
        let params = QmdParams::simple(0, DispatchDims::linear(1), 16);
        assert_eq!(params.workgroup, [64, 1, 1]);
    }

    #[test]
    fn qmd_v30_preserves_other_fields() {
        let params = QmdParams::simple(0x1_0000_0000, DispatchDims::new(8, 4, 2), 64);
        let q30 = build_qmd_v30(&params);
        // v3.0 has different field positions from v2.1 — verify the v3.0 layout
        assert_eq!(get_field(&q30, 384, 32), 8, "grid width (v3.0 at 384)");
        assert_eq!(get_field(&q30, 416, 16), 4, "grid height (v3.0 at 416)");
        assert_eq!(get_field(&q30, 448, 16), 2, "grid depth (v3.0 at 448)");
        assert_eq!(
            get_field(&q30, 1536, 32),
            0,
            "shader addr lo (v3.0 at 1536)"
        );
        assert_eq!(
            get_field(&q30, 1568, 17),
            1,
            "shader addr hi (v3.0 at 1568)"
        );
        assert_eq!(get_field(&q30, 580, 4), 3, "major version");
    }

    #[test]
    fn qmd_set_field_cross_word_boundary() {
        let mut q = [0u32; QMD_SIZE_WORDS];
        qmd_set_field(&mut q, 28, 8, 0xFF);
        assert_eq!(q[0] >> 28, 0xF);
        assert_eq!(q[1] & 0xF, 0xF);
    }

    #[test]
    fn build_qmd_for_sm_selects_version() {
        let params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        // SM 0..=69 → v2.1 (version at bits 0:3 / 4:7)
        let q_69 = build_qmd_for_sm(69, &params);
        assert_eq!(get_field(&q_69, 0, 4), 2);
        assert_eq!(get_field(&q_69, 4, 4), 1);
        // SM 70..=79 → v2.2
        let q_70 = build_qmd_for_sm(70, &params);
        assert_eq!(get_field(&q_70, 0, 4), 2);
        assert_eq!(get_field(&q_70, 4, 4), 2);
        let q_75 = build_qmd_for_sm(75, &params);
        assert_eq!(get_field(&q_75, 4, 4), 2);
        // SM 80..=99 → v3.0
        let q_86 = build_qmd_for_sm(86, &params);
        assert_eq!(get_field(&q_86, 580, 4), 3, "SM 86 major = 3 (v3.0)");
        assert_eq!(get_field(&q_86, 576, 4), 0, "SM 86 minor = 0 (v3.0)");
        let q_90 = build_qmd_for_sm(90, &params);
        assert_eq!(get_field(&q_90, 580, 4), 3, "SM 90 major = 3 (v3.0)");
        assert_eq!(get_field(&q_90, 576, 4), 0, "SM 90 minor = 0 (v3.0)");
        let q_99 = build_qmd_for_sm(99, &params);
        assert_eq!(q_99.len(), QMD_SIZE_WORDS, "SM 99 = 64-word QMD (v3.0)");
        // SM 100+ → v5.0 (Blackwell)
        let q_120 = build_qmd_for_sm(120, &params);
        assert_eq!(q_120.len(), QMD_V4_PLUS_SIZE_WORDS, "Blackwell QMD = 96 words");
        assert_eq!(get_field(&q_120, 468, 4), 5, "SM 120 major = 5 (v5.0)");
        assert_eq!(get_field(&q_120, 464, 4), 0, "SM 120 minor = 0 (v5.0)");
    }

    #[test]
    fn cbuf_binding_debug() {
        let cb = CbufBinding {
            index: 0,
            addr: 0x1_0000_0000,
            size: 4096,
        };
        let debug = format!("{cb:?}");
        assert!(debug.contains("CbufBinding"));
        assert!(debug.contains("4096"));
    }

    #[test]
    fn qmd_params_debug() {
        let params = QmdParams::simple(0x1000, DispatchDims::linear(8), 16);
        let debug = format!("{params:?}");
        assert!(debug.contains("QmdParams"));
        // shader_va may be formatted as decimal (4096) or hex
        assert!(
            debug.contains("4096")
                || debug.contains("0x1000")
                || debug.contains("16")
                || debug.contains("8"),
            "debug should contain struct field values: {debug}"
        );
    }

    #[test]
    fn qmd_simple_gpr_minimum() {
        let params = QmdParams::simple(0, DispatchDims::linear(1), 0);
        assert_eq!(params.gpr_count, 4, "simple() clamps gpr_count to min 4");
    }

    #[test]
    fn qmd_shared_memory_zero() {
        let mut params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        params.shared_mem_bytes = 0;
        let q = build_qmd_v21(&params);
        assert_eq!(get_field(&q, 640, 18), 0, "zero shared mem stays 0");
    }

    #[test]
    fn qmd_shared_memory_exact_256_aligned() {
        let mut params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        params.shared_mem_bytes = 256;
        let q = build_qmd_v21(&params);
        assert_eq!(get_field(&q, 640, 18), 256);
    }

    #[test]
    fn qmd_build_for_sm_boundary_70() {
        let params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        let q_69 = build_qmd_for_sm(69, &params);
        let q_70 = build_qmd_for_sm(70, &params);
        assert_ne!(
            get_field(&q_69, 4, 4),
            get_field(&q_70, 4, 4),
            "SM 69 vs 70 should differ in minor version"
        );
    }

    #[test]
    fn qmd_build_for_sm_boundary_80() {
        let params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        let q_79 = build_qmd_for_sm(79, &params);
        let q_80 = build_qmd_for_sm(80, &params);
        assert_eq!(
            get_field(&q_79, 0, 4),
            2,
            "SM 79 = v2.x (version at bits 0:3)"
        );
        assert_eq!(
            get_field(&q_80, 580, 4),
            3,
            "SM 80 = v3.0 (major at MW(583:580))"
        );
        assert_eq!(
            get_field(&q_80, 576, 4),
            0,
            "SM 80 = v3.0 (minor at MW(579:576))"
        );
    }

    #[test]
    fn qmd_build_for_sm_boundary_100() {
        let params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        let q_99 = build_qmd_for_sm(99, &params);
        let q_100 = build_qmd_for_sm(100, &params);
        assert_eq!(q_99.len(), QMD_SIZE_WORDS, "SM 99 = 64-word QMD");
        assert_eq!(q_100.len(), QMD_V4_PLUS_SIZE_WORDS, "SM 100 = 96-word QMD");
        assert_eq!(get_field(&q_100, 468, 4), 5, "SM 100 major = 5 (v5.0)");
    }

    #[test]
    fn qmd_grid_linear_single() {
        let params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        let q = build_qmd_v21(&params);
        assert_eq!(get_field(&q, 224, 32), 1);
        assert_eq!(get_field(&q, 256, 16), 1);
        assert_eq!(get_field(&q, 272, 16), 1);
    }

    #[test]
    fn qmd_v22_sets_minor_version_two() {
        let params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        let q = build_qmd_v22(&params);
        assert_eq!(get_field(&q, 0, 4), 2);
        assert_eq!(get_field(&q, 4, 4), 2);
    }

    #[test]
    fn qmd_set_field_width_32_fits_single_word() {
        let mut q = [0u32; QMD_SIZE_WORDS];
        qmd_set_field(&mut q, 0, 32, 0xDEAD_BEEF);
        assert_eq!(q[0], 0xDEAD_BEEF);
    }

    #[test]
    fn qmd_shared_memory_size_field_truncates_to_18_bits() {
        let mut params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        params.shared_mem_bytes = 262_144;
        let q = build_qmd_v21(&params);
        let aligned = (262_144_u32 + 255) & !255;
        assert_eq!(aligned, 262_144);
        let masked = u64::from(aligned) & ((1u64 << 18) - 1);
        assert_eq!(get_field(&q, 640, 18), masked);
    }

    #[test]
    fn qmd_duplicate_cbuf_slot_last_binding_wins() {
        let mut params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        params.cbufs.push(CbufBinding {
            index: 0,
            addr: 0x1_0000_0000,
            size: 1024,
        });
        params.cbufs.push(CbufBinding {
            index: 0,
            addr: 0x5_0000_0000,
            size: 2048,
        });
        let q = build_qmd_v21(&params);
        let lo = get_field(&q, 1536, 32);
        let hi = get_field(&q, 1536 + 32, 8);
        let addr = lo | (hi << 32);
        assert_eq!(addr, 0x5_0000_0000);
        assert_eq!(get_field(&q, 1536 + 40, 17), u64::from(2048_u32 >> 4));
    }

    #[test]
    fn qmd_v50_version() {
        let params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        let q = build_qmd_v50(&params);
        assert_eq!(q.len(), QMD_V4_PLUS_SIZE_WORDS);
        // MW(471:468) = QMD_MAJOR_VERSION = 5, MW(467:464) = QMD_MINOR_VERSION = 0
        assert_eq!(get_field(&q, 468, 4), 5, "major version");
        assert_eq!(get_field(&q, 464, 4), 0, "minor version");
    }

    #[test]
    fn qmd_v50_qmd_type_grid_cta() {
        let params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        let q = build_qmd_v50(&params);
        // MW(153:151) = QMD_TYPE = GRID_CTA (2)
        assert_eq!(get_field(&q, 151, 3), 2, "QMD_TYPE = GRID_CTA");
    }

    #[test]
    fn qmd_v50_api_visible_call_limit() {
        let params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        let q = build_qmd_v50(&params);
        // MW(456:456) = API_VISIBLE_CALL_LIMIT = NO_CHECK (1)
        assert_eq!(get_field(&q, 456, 1), 1, "API_VISIBLE_CALL_LIMIT = NO_CHECK");
    }

    #[test]
    fn qmd_v50_cache_invalidation_flags() {
        let params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        let q = build_qmd_v50(&params);
        assert_eq!(get_field(&q, 472, 1), 1, "INVALIDATE_TEXTURE_HEADER_CACHE");
        assert_eq!(get_field(&q, 473, 1), 1, "INVALIDATE_TEXTURE_SAMPLER_CACHE");
        assert_eq!(get_field(&q, 474, 1), 1, "INVALIDATE_TEXTURE_DATA_CACHE");
        assert_eq!(get_field(&q, 475, 1), 1, "INVALIDATE_SHADER_DATA_CACHE");
        assert_eq!(get_field(&q, 476, 1), 1, "INVALIDATE_INSTRUCTION_CACHE");
        assert_eq!(get_field(&q, 477, 1), 1, "INVALIDATE_SHADER_CONSTANT_CACHE");
    }

    #[test]
    fn qmd_v50_grid_dimensions() {
        let params = QmdParams::simple(0, DispatchDims::new(64, 8, 2), 32);
        let q = build_qmd_v50(&params);
        // MW(1279:1248) GRID_WIDTH, MW(1295:1280) GRID_HEIGHT, MW(1327:1312) GRID_DEPTH
        assert_eq!(get_field(&q, 1248, 32), 64, "GRID_WIDTH");
        assert_eq!(get_field(&q, 1280, 16), 8, "GRID_HEIGHT");
        assert_eq!(get_field(&q, 1312, 16), 2, "GRID_DEPTH");
    }

    #[test]
    fn qmd_v50_workgroup() {
        let mut params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        params.workgroup = [128, 4, 2];
        let q = build_qmd_v50(&params);
        // MW(1103:1088), MW(1119:1104), MW(1127:1120)
        assert_eq!(get_field(&q, 1088, 16), 128, "CTA_THREAD_DIMENSION0");
        assert_eq!(get_field(&q, 1104, 16), 4, "CTA_THREAD_DIMENSION1");
        assert_eq!(get_field(&q, 1120, 8), 2, "CTA_THREAD_DIMENSION2");
    }

    #[test]
    fn qmd_v50_shader_address_shifted() {
        let va = 0x0001_0000_0000_u64;
        let params = QmdParams::simple(va, DispatchDims::linear(1), 32);
        let q = build_qmd_v50(&params);
        // MW(1055:1024) lower 32 bits, MW(1076:1056) upper 21 bits
        let lo_shifted = get_field(&q, 1024, 32);
        let hi_shifted = get_field(&q, 1056, 21);
        let reconstructed = (lo_shifted | (hi_shifted << 32)) << 4;
        assert_eq!(reconstructed, va);
    }

    #[test]
    fn qmd_v50_register_count() {
        let params = QmdParams::simple(0, DispatchDims::linear(1), 48);
        let q = build_qmd_v50(&params);
        // MW(1136:1128) — 9 bits
        assert_eq!(get_field(&q, 1128, 9), 48, "REGISTER_COUNT");
    }

    #[test]
    fn qmd_v50_shared_memory_shifted7() {
        let mut params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        params.shared_mem_bytes = 1024;
        let q = build_qmd_v50(&params);
        // MW(1162:1152) — 11 bits
        assert_eq!(get_field(&q, 1152, 11), 1024 >> 7, "SHARED_MEMORY_SIZE_SHIFTED7");
    }

    #[test]
    fn qmd_v50_cbuf_correct_positions() {
        let mut params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        params.cbufs.push(CbufBinding {
            index: 0,
            addr: 0x2_0000_0040,
            size: 4096,
        });
        let q = build_qmd_v50(&params);

        // VALID(0) at MW(1856) — separate from CBUF descriptor
        assert_eq!(get_field(&q, 1856, 1), 1, "CBUF 0 valid");
        // INVALIDATE(0) at MW(1859)
        assert_eq!(get_field(&q, 1859, 1), 1, "CBUF 0 invalidate");

        // ADDR_LOWER_SHIFTED6(0): MW(1375:1344) — 32 bits
        let addr_lo = get_field(&q, 1344, 32);
        // ADDR_UPPER_SHIFTED6(0): MW(1394:1376) — 19 bits
        let addr_hi = get_field(&q, 1376, 19);
        let reconstructed = (addr_lo | (addr_hi << 32)) << 6;
        assert_eq!(reconstructed, 0x2_0000_0040, "CBUF 0 addr");

        // SIZE_SHIFTED4(0): MW(1407:1395) — 13 bits
        assert_eq!(get_field(&q, 1395, 13), u64::from(4096_u32 >> 4), "CBUF 0 size");
    }

    #[test]
    fn qmd_v50_cbuf_slot1() {
        let mut params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        params.cbufs.push(CbufBinding {
            index: 1,
            addr: 0x3_0000_0080,
            size: 8192,
        });
        let q = build_qmd_v50(&params);

        // VALID(1) at MW(1860)
        assert_eq!(get_field(&q, 1860, 1), 1, "CBUF 1 valid");

        // ADDR_LOWER_SHIFTED6(1): MW(1375+64:1344+64) = MW(1439:1408)
        let addr_lo = get_field(&q, 1408, 32);
        let addr_hi = get_field(&q, 1440, 19);
        let reconstructed = (addr_lo | (addr_hi << 32)) << 6;
        assert_eq!(reconstructed, 0x3_0000_0080, "CBUF 1 addr");

        // SIZE_SHIFTED4(1): MW(1407+64:1395+64) = MW(1471:1459)
        assert_eq!(get_field(&q, 1459, 13), u64::from(8192_u32 >> 4), "CBUF 1 size");
    }

    #[test]
    fn qmd_v50_cbuf_addr_high_bits() {
        let mut params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        params.cbufs.push(CbufBinding {
            index: 0,
            addr: 0x1_2000_F000,
            size: 256,
        });
        let q = build_qmd_v50(&params);
        let addr_lo = get_field(&q, 1344, 32);
        let addr_hi = get_field(&q, 1376, 19);
        let reconstructed = (addr_lo | (addr_hi << 32)) << 6;
        assert_eq!(
            reconstructed, 0x1_2000_F000,
            "CBUF addr must survive upper bits"
        );
    }
}
