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

#[path = "qmd_v02.rs"]
mod qmd_v02;
#[path = "qmd_v50.rs"]
mod qmd_v50;

pub use qmd_v02::{build_qmd_v21, build_qmd_v22, build_qmd_v23, build_qmd_v30};
pub use qmd_v50::build_qmd_v50;

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
pub(super) const fn qmd_set_field(
    q: &mut [u32; QMD_SIZE_WORDS],
    bit_start: usize,
    width: usize,
    value: u64,
) {
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

/// Dynamic-size variant of `qmd_set_field` for Vec-backed QMDs.
#[expect(
    clippy::cast_possible_truncation,
    reason = "GPU QMD fields are always ≤32 bits"
)]
pub(super) fn qmd_set_field_dyn(q: &mut [u32], bit_start: usize, width: usize, value: u64) {
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
        _ => qmd_v50::build_qmd_v50_with_sm(params, sm),
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
        assert_eq!(
            q_120.len(),
            QMD_V4_PLUS_SIZE_WORDS,
            "Blackwell QMD = 96 words"
        );
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
        assert_eq!(
            get_field(&q, 456, 1),
            1,
            "API_VISIBLE_CALL_LIMIT = NO_CHECK"
        );
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
        assert_eq!(
            get_field(&q, 1152, 11),
            1024 >> 7,
            "SHARED_MEMORY_SIZE_SHIFTED7"
        );
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
        assert_eq!(
            get_field(&q, 1395, 13),
            u64::from(4096_u32 >> 4),
            "CBUF 0 size"
        );
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
        assert_eq!(
            get_field(&q, 1459, 13),
            u64::from(8192_u32 >> 4),
            "CBUF 1 size"
        );
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
