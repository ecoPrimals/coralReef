// SPDX-License-Identifier: AGPL-3.0-only
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

/// Build a QMD v2.1 (Volta SM70) for compute dispatch.
///
/// Returns the full 64-word QMD suitable for `SEND_PCAS_A/B` submission.
///
/// Field layout (word offsets, from Mesa `cl_c3c0qmd.h`):
///
/// - Word 0: `QMD_VERSION`=2, `API_VISIBLE_CALL_LIMIT`, `SAMPLER_INDEX`.
/// - Words 1–3: `CTA_RASTER_WIDTH`/`HEIGHT`/`DEPTH` (grid dimensions).
/// - Word 6: `CTA_THREAD_DIMENSION0` bits 15:0, `CTA_THREAD_DIMENSION1` bits 31:16.
/// - Word 7: `CTA_THREAD_DIMENSION2` bits 15:0, `REGISTER_COUNT` bits 23:16.
/// - Word 10: `BARRIER_COUNT` bits 4:0.
/// - Word 11: `SHARED_MEMORY_SIZE` (256-byte aligned).
/// - Words 17–18: `PROGRAM_ADDRESS_LOWER`/`UPPER`.
/// - Word 20: `CONSTANT_BUFFER_VALID` bitmask bits 7:0.
/// - Words 22–37: `CONSTANT_BUFFER_ADDR` pairs (8 slots x 2 words each).
/// - Words 38–45: `CONSTANT_BUFFER_SIZE_SHIFTED4` (8 slots).
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "QMD register fields are 32-bit by spec"
)]
pub fn build_qmd_v21(params: &QmdParams) -> [u32; QMD_SIZE_WORDS] {
    let mut q = [0u32; QMD_SIZE_WORDS];

    // Word 0: QMD_VERSION=2, API_VISIBLE_CALL_LIMIT=NO_CHECK(0),
    //         SAMPLER_INDEX=INDEPENDENTLY(1 << 12)
    q[0] = 0x02 | (1 << 12);

    // Words 1-3: CTA raster dimensions (grid)
    q[1] = params.grid.x;
    q[2] = params.grid.y;
    q[3] = params.grid.z;

    // Word 6: CTA thread dimensions (workgroup size)
    q[6] = (params.workgroup[0] & 0xFFFF) | ((params.workgroup[1] & 0xFFFF) << 16);

    // Word 7: CTA_THREAD_DIMENSION2 [15:0], REGISTER_COUNT [23:16]
    let reg_count = params.gpr_count.min(255);
    q[7] = (params.workgroup[2] & 0xFFFF) | (reg_count << 16);

    // Word 10: BARRIER_COUNT [4:0]
    q[10] = params.barrier_count & 0x1F;

    // Word 11: SHARED_MEMORY_SIZE (aligned to 256 bytes)
    let shared_aligned = (params.shared_mem_bytes + 255) & !255;
    q[11] = shared_aligned;

    // Word 17-18: PROGRAM_ADDRESS (256-byte aligned)
    q[17] = params.shader_va as u32;
    q[18] = (params.shader_va >> 32) as u32 & 0x0001_FFFF;

    // Word 20: CONSTANT_BUFFER_VALID bitmask
    let mut cbuf_valid: u32 = 0;
    for cb in &params.cbufs {
        if cb.index < MAX_CBUFS as u32 {
            cbuf_valid |= 1 << cb.index;
        }
    }
    q[20] = cbuf_valid;

    // Words 22-37: CBUF address pairs (lower, upper) for slots 0-7
    for cb in &params.cbufs {
        let idx = cb.index as usize;
        if idx < MAX_CBUFS {
            let base = 22 + idx * 2;
            q[base] = cb.addr as u32;
            q[base + 1] = (cb.addr >> 32) as u32 & 0x0001_FFFF;
        }
    }

    // Words 38-45: CBUF sizes (shifted right by 4)
    for cb in &params.cbufs {
        let idx = cb.index as usize;
        if idx < MAX_CBUFS {
            q[38 + idx] = cb.size >> 4;
        }
    }

    q
}

/// Build a QMD v3.0 (Ampere SM86+) for compute dispatch.
///
/// Same layout as v2.1 but with `QMD_VERSION`=3 and minor field differences.
#[must_use]
pub fn build_qmd_v30(params: &QmdParams) -> [u32; QMD_SIZE_WORDS] {
    let mut q = build_qmd_v21(params);
    // Overwrite version: v3.0
    q[0] = (q[0] & !0xFF) | 0x03;
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

    #[test]
    fn qmd_v21_version() {
        let params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        let q = build_qmd_v21(&params);
        assert_eq!(q[0] & 0xFF, 2);
    }

    #[test]
    fn qmd_v30_version() {
        let params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        let q = build_qmd_v30(&params);
        assert_eq!(q[0] & 0xFF, 3);
    }

    #[test]
    fn qmd_grid_dimensions() {
        let params = QmdParams::simple(0, DispatchDims::new(64, 8, 2), 32);
        let q = build_qmd_v21(&params);
        assert_eq!(q[1], 64);
        assert_eq!(q[2], 8);
        assert_eq!(q[3], 2);
    }

    #[test]
    fn qmd_gpr_count() {
        let params = QmdParams::simple(0, DispatchDims::linear(1), 48);
        let q = build_qmd_v21(&params);
        let reg_count = (q[7] >> 16) & 0xFF;
        assert_eq!(reg_count, 48);
    }

    #[test]
    fn qmd_shader_address() {
        let va = 0x0001_0000_0000_u64;
        let params = QmdParams::simple(va, DispatchDims::linear(1), 32);
        let q = build_qmd_v21(&params);
        let addr_lo = q[17];
        let addr_hi = q[18];
        let reconstructed = u64::from(addr_lo) | (u64::from(addr_hi) << 32);
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

        // CONSTANT_BUFFER_VALID should have bits 0 and 1 set
        assert_eq!(q[20] & 0x3, 0x3);

        // CBUF 0 address
        let cb0_lo = q[22];
        let cb0_hi = q[23];
        let cb0_addr = u64::from(cb0_lo) | (u64::from(cb0_hi) << 32);
        assert_eq!(cb0_addr, 0x2_0000_0000);

        // CBUF 0 size (shifted by 4)
        assert_eq!(q[38], 4096 >> 4);

        // CBUF 1 address
        let cb1_lo = q[24];
        let cb1_hi = q[25];
        let cb1_addr = u64::from(cb1_lo) | (u64::from(cb1_hi) << 32);
        assert_eq!(cb1_addr, 0x3_0000_0000);

        // CBUF 1 size (shifted by 4)
        assert_eq!(q[39], 8192 >> 4);
    }

    #[test]
    fn qmd_shared_memory_aligned() {
        let mut params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        params.shared_mem_bytes = 100;
        let q = build_qmd_v21(&params);
        // Should be aligned to 256
        assert_eq!(q[11], 256);
    }

    #[test]
    fn qmd_barrier_count() {
        let mut params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        params.barrier_count = 3;
        let q = build_qmd_v21(&params);
        assert_eq!(q[10] & 0x1F, 3);
    }

    #[test]
    fn qmd_workgroup_size() {
        let mut params = QmdParams::simple(0, DispatchDims::linear(1), 32);
        params.workgroup = [128, 4, 2];
        let q = build_qmd_v21(&params);
        let dim0 = q[6] & 0xFFFF;
        let dim1 = (q[6] >> 16) & 0xFFFF;
        let dim2 = q[7] & 0xFFFF;
        assert_eq!(dim0, 128);
        assert_eq!(dim1, 4);
        assert_eq!(dim2, 2);
    }

    #[test]
    fn legacy_build_compute_qmd_compat() {
        let q = build_compute_qmd(0x1_0000_0000, DispatchDims::new(64, 1, 1), 256);
        assert_eq!(q[1], 64);
        assert_eq!(q[2], 1);
        assert_eq!(q[3], 1);
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
        let reg_count = (q[7] >> 16) & 0xFF;
        assert_eq!(reg_count, 255);
    }
}
