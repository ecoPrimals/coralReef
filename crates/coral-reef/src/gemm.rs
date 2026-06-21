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

/// Minimum SM version for HMMA tensor-core GEMM (Ampere).
const MIN_HMMA_SM: u32 = 80;

/// MMA tile-K depth for f16/f16→f32 precision (16×8×16 MMA shape).
const TILE_K_F16: u32 = 16;

/// MMA tile-K depth for TF32 precision (16×8×8 MMA shape).
const TILE_K_TF32: u32 = 8;

/// MMA tile rows (M dimension per warp).
const MMA_TILE_ROWS: u32 = 16;

/// MMA tile columns (N dimension per warp).
const MMA_TILE_COLS: u32 = 8;

/// Threads per warp (NVIDIA architecture constant).
const THREADS_PER_WARP: u32 = 32;

/// Maximum workgroup size for GEMM kernels.
const MAX_WORKGROUP_SIZE: u32 = 256;

/// Default GPR estimate for GEMM metadata (conservative).
const GEMM_DEFAULT_GPR_COUNT: u32 = 32;

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
    if nv.sm() < MIN_HMMA_SM {
        return Err(CompileError::UnsupportedArch(
            format!(
                "tensor-core GEMM requires SM{MIN_HMMA_SM}+, got SM{}",
                nv.sm()
            )
            .into(),
        ));
    }
    if shape.m == 0 || shape.n == 0 || shape.k == 0 {
        return Err(CompileError::InvalidInput(
            "GEMM dimensions must be non-zero".into(),
        ));
    }

    let tile_k: u32 = match precision {
        GemmPrecision::F16 | GemmPrecision::F16F32 => TILE_K_F16,
        GemmPrecision::Tf32 => TILE_K_TF32,
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
    let warps_along_m = shape.m.div_ceil(MMA_TILE_ROWS);
    let warps_along_n = shape.n.div_ceil(MMA_TILE_COLS);
    let threads = warps_along_m * warps_along_n * THREADS_PER_WARP;

    Ok(backend::CompiledBinary {
        binary: ptx.into_bytes(),
        info: backend::CompilationInfo {
            gpr_count: GEMM_DEFAULT_GPR_COUNT,
            instr_count: 0,
            shared_mem_bytes: 0,
            barrier_count: 0,
            local_size: [threads.min(MAX_WORKGROUP_SIZE), 1, 1],
            local_mem_bytes: 0,
        },
        format: backend::BinaryFormat::Ptx,
    })
}
