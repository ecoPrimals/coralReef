// SPDX-License-Identifier: AGPL-3.0-or-later
//! QMD v2.x / v3.0 builders (256-byte / 64-word layout).

use super::{MAX_CBUFS, QMD_SIZE_WORDS, QmdParams, qmd_set_field};

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
