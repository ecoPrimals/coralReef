// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tensor-core GEMM kernel generation.
//!
//! Generates PTX kernels using `mma.sync.aligned` (HMMA) instructions for
//! NVIDIA SM80+ (Ampere, Ada, Blackwell). Consumed by ecosystem benchmark
//! harnesses for DF64 NVK parity validation.

use crate::backend;
use crate::codegen;
use crate::error::CompileError;
use crate::gpu_arch::GpuTarget;

/// GEMM shape parameters for tensor-core kernel generation.
#[derive(Debug, Clone, Copy)]
pub struct GemmShape {
    /// Matrix rows (M dimension).
    pub m: u32,
    /// Matrix columns (N dimension).
    pub n: u32,
    /// Inner/reduction dimension (K dimension).
    pub k: u32,
}

/// Precision for GEMM operands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GemmPrecision {
    /// f16 inputs, f16 accumulate — fastest, least precise.
    F16,
    /// f16 inputs, f32 accumulate — standard mixed-precision.
    #[default]
    F16F32,
    /// f32 inputs via TF32, f32 accumulate (Ampere+ only).
    Tf32,
}

/// Generate a tensor-core GEMM kernel as PTX for NVIDIA SM80+ targets.
///
/// Produces a PTX kernel (`gemm_kernel`) that performs C = A * B + C using
/// `mma.sync.aligned` tensor-core instructions. The kernel tiles the GEMM
/// using warp-level MMA (16x8x16 for f16→f32, 16x8x8 for TF32).
///
/// The generated kernel assumes:
/// - Row-major A (M×K), column-major B (K×N), row-major C (M×N)
/// - Matrices passed as `.param .u64` pointers
/// - Workgroup size determined by tiling (typically 128 or 256 threads)
///
/// # Errors
///
/// Returns [`CompileError`] if the target is not NVIDIA SM80+ or if the
/// shape is not aligned to tensor-core tile boundaries.
pub fn compile_gemm(
    shape: GemmShape,
    precision: GemmPrecision,
    target: GpuTarget,
) -> Result<backend::CompiledBinary, CompileError> {
    let nv = target.as_nvidia().ok_or_else(|| {
        CompileError::UnsupportedArch("HMMA/tensor-core GEMM requires NVIDIA target".into())
    })?;
    if nv.sm() < 80 {
        return Err(CompileError::UnsupportedArch(
            format!("tensor-core GEMM requires SM80+, got SM{}", nv.sm()).into(),
        ));
    }
    if shape.m == 0 || shape.n == 0 || shape.k == 0 {
        return Err(CompileError::InvalidInput(
            "GEMM dimensions must be non-zero".into(),
        ));
    }

    let tile_k: u32 = match precision {
        GemmPrecision::F16 | GemmPrecision::F16F32 => 16,
        GemmPrecision::Tf32 => 8,
    };
    if shape.k % tile_k != 0 {
        return Err(CompileError::InvalidInput(
            format!(
                "K dimension ({}) must be aligned to tile_k ({tile_k})",
                shape.k
            )
            .into(),
        ));
    }

    tracing::info!(
        m = shape.m,
        n = shape.n,
        k = shape.k,
        ?precision,
        sm = nv.sm(),
        "coral-reef compile_gemm (tensor-core)"
    );

    let ptx = codegen::nv::ptx_emit::gemm::emit_gemm_ptx(shape, precision, nv.sm_version())?;
    let tile_rows = 16u32;
    let tile_cols = 8u32;
    let warps_along_m = shape.m.div_ceil(tile_rows);
    let warps_along_n = shape.n.div_ceil(tile_cols);
    let threads = warps_along_m * warps_along_n * 32;

    Ok(backend::CompiledBinary {
        binary: ptx.into_bytes(),
        info: backend::CompilationInfo {
            gpr_count: 32,
            instr_count: 0,
            shared_mem_bytes: 0,
            barrier_count: 0,
            local_size: [threads.min(256), 1, 1],
            local_mem_bytes: 0,
        },
        format: backend::BinaryFormat::Ptx,
    })
}
