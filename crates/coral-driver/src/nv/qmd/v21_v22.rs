// SPDX-License-Identifier: AGPL-3.0-or-later
//! QMD v2.1 / v2.2 (256-byte legacy layout for Kepler–Turing).

use super::field::qmd_set_field;
use super::types::{MAX_CBUFS, QMD_SIZE_WORDS, QmdParams};

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
