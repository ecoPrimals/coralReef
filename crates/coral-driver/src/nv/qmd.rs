// SPDX-License-Identifier: AGPL-3.0-or-later
//! QMD (Queue Management Descriptor) construction for NVIDIA compute dispatch.
//!
//! Full 256-byte (64-word) QMD for Volta v2.1 and Ampere v3.0.
//! Includes constant buffer binding, GPR count from compiler, shared
//! memory sizing, and dispatch grid/workgroup dimensions.
//!
//! Field layout derived from Mesa NVK (`nvk_compute.c`) and the NVIDIA
//! open GPU headers.

use crate::DispatchDims;

/// QMD size in u32 words (256 bytes = 64 words).
pub const QMD_SIZE_WORDS: usize = 64;

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

/// Build a QMD v3.0 (Ampere SM86+) for compute dispatch.
///
/// Same field layout as v2.1/v2.2 but with `QMD_MAJOR_VERSION`=3, `QMD_VERSION`=0.
#[must_use]
pub fn build_qmd_v30(params: &QmdParams) -> [u32; QMD_SIZE_WORDS] {
    let mut q = build_qmd_v21(params);
    q[0] &= !0xFF;
    qmd_set_field(&mut q, 0, 4, 3);
    qmd_set_field(&mut q, 4, 4, 0);
    q
}

/// Select the appropriate QMD builder for a given SM architecture.
#[must_use]
pub fn build_qmd_for_sm(sm: u32, params: &QmdParams) -> [u32; QMD_SIZE_WORDS] {
    match sm {
        0..=69 => build_qmd_v21(params),
        70..=79 => build_qmd_v22(params),
        _ => build_qmd_v30(params),
    }
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

    fn get_field(q: &[u32; QMD_SIZE_WORDS], bit_start: usize, width: usize) -> u64 {
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
        assert_eq!(get_field(&q, 0, 4), 3, "major version");
        assert_eq!(get_field(&q, 4, 4), 0, "minor version");
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
        assert_eq!(get_field(&q, 224, 32), 64, "CTA_RASTER_WIDTH");
        assert_eq!(get_field(&q, 256, 16), 1, "CTA_RASTER_HEIGHT");
        assert_eq!(get_field(&q, 272, 16), 1, "CTA_RASTER_DEPTH");
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
        let q21 = build_qmd_v21(&params);
        let q30 = build_qmd_v30(&params);
        // Grid, shader addr, etc. should be identical
        assert_eq!(
            get_field(&q21, 224, 32),
            get_field(&q30, 224, 32),
            "grid width"
        );
        assert_eq!(
            get_field(&q21, 832, 32),
            get_field(&q30, 832, 32),
            "shader addr lo"
        );
        assert_eq!(
            get_field(&q21, 864, 32),
            get_field(&q30, 864, 32),
            "shader addr hi"
        );
        // But version should differ
        assert_ne!(q21[0] & 0xF, q30[0] & 0xF, "major version");
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
        // SM 0..=69 → v2.1
        let q_69 = build_qmd_for_sm(69, &params);
        assert_eq!(get_field(&q_69, 0, 4), 2);
        assert_eq!(get_field(&q_69, 4, 4), 1);
        // SM 70..=79 → v2.2
        let q_70 = build_qmd_for_sm(70, &params);
        assert_eq!(get_field(&q_70, 0, 4), 2);
        assert_eq!(get_field(&q_70, 4, 4), 2);
        let q_75 = build_qmd_for_sm(75, &params);
        assert_eq!(get_field(&q_75, 4, 4), 2);
        // SM 80+ → v3.0
        let q_86 = build_qmd_for_sm(86, &params);
        assert_eq!(get_field(&q_86, 0, 4), 3);
        assert_eq!(get_field(&q_86, 4, 4), 0);
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
        assert_eq!(get_field(&q_79, 0, 4), 2, "SM 79 = v2.x");
        assert_eq!(get_field(&q_80, 0, 4), 3, "SM 80+ = v3.0");
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
}
