// SPDX-License-Identifier: AGPL-3.0-only
//! QMD (Queue Management Descriptor) construction for NVIDIA compute dispatch.
//!
//! QMD v3.0 (SM86 Ampere) describes the compute shader to the GPU:
//! - Shader program address
//! - Register counts (GPR, UGPR)
//! - Workgroup dimensions
//! - Shared memory size
//! - Barrier counts

use crate::DispatchDims;

/// QMD v3.0 size in u32 words (64 bytes = 16 words).
pub const QMD_SIZE_WORDS: usize = 16;

/// Build a compute QMD for dispatch.
///
/// Returns the QMD as fixed-size array suitable for pushbuf submission.
pub fn build_compute_qmd(
    shader_va: u64,
    dims: DispatchDims,
    _code_size: u32,
) -> [u32; QMD_SIZE_WORDS] {
    let mut qmd = [0u32; QMD_SIZE_WORDS];

    // QMD word 0: version (3.0), invalidate texture data cache
    qmd[0] = 0x0300_0000;

    // QMD word 2: CTA dimensions
    qmd[2] = dims.x;
    qmd[3] = dims.y;
    qmd[4] = dims.z;

    // QMD word 8: shader address (low 32 bits, 256-byte aligned)
    qmd[8] = (shader_va >> 8) as u32;
    // QMD word 9: shader address (high 32 bits)
    qmd[9] = (shader_va >> 40) as u32;

    // QMD word 10: GPR allocation (16 GPRs, 16 UGPRs)
    let gpr_alloc = 16_u32;
    let ugpr_alloc = 16_u32;
    qmd[10] = gpr_alloc | (ugpr_alloc << 16);

    qmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qmd_dimensions() {
        let qmd = build_compute_qmd(0x1_0000_0000, DispatchDims::new(64, 1, 1), 256);
        assert_eq!(qmd[2], 64);
        assert_eq!(qmd[3], 1);
        assert_eq!(qmd[4], 1);
    }

    #[test]
    fn qmd_shader_address() {
        let va = 0x0001_0000_0000_u64;
        let qmd = build_compute_qmd(va, DispatchDims::linear(1), 256);
        assert_eq!(qmd[8], (va >> 8) as u32);
    }

    #[test]
    fn qmd_version() {
        let qmd = build_compute_qmd(0, DispatchDims::linear(1), 0);
        assert_eq!(qmd[0] >> 24, 3);
    }
}
