// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tensor-core GEMM PTX generation — tiled kernel.
//!
//! Emits a complete PTX kernel using `mma.sync.aligned` instructions for
//! NVIDIA SM80+ (Ampere, Ada, Blackwell). The kernel performs:
//!   C[M×N] = A[M×K] * B[K×N]
//!
//! Two modes:
//!
//! **Phase 1** (global memory): Each CTA is one warp (32 threads) handling a
//! single MMA output tile (16×8). Grid covers full M×N output.
//!
//! **Phase 2** (shared memory): Each CTA is 4 warps (128 threads) handling a
//! 64×16 output tile. Uses shared memory tile buffers, `ldmatrix.sync` for
//! warp-cooperative fragment loads, and `bar.sync` for pipeline synchronization.
//! Grid covers full M×N output: gridDim = (N/BN, M/BM, 1).
//!
//! Layout: A row-major M×K, B column-major K×N, C row-major M×N.

use crate::error::CompileError;
use crate::gemm::{GemmPrecision, GemmShape};

/// Block tile M dimension (4 warps × 16 MMA rows).
const BM: u32 = 64;

/// Block tile N dimension (2 MMA tiles × 8 MMA cols).
const BN: u32 = 16;

/// Number of warps per CTA in shared-memory mode.
const WARPS_PER_CTA: u32 = 4;

/// Threads per CTA in shared-memory mode.
const CTA_THREADS: u32 = WARPS_PER_CTA * 32;

/// Emit a Phase 1 tiled PTX GEMM kernel (global memory loads).
pub fn emit_gemm_ptx(
    shape: GemmShape,
    precision: GemmPrecision,
    sm: u8,
) -> Result<String, CompileError> {
    match precision {
        GemmPrecision::F16 | GemmPrecision::F16F32 => phase1::emit_f16_gemm(shape, precision, sm),
        GemmPrecision::Tf32 => phase1::emit_tf32_gemm(shape, sm),
    }
}

/// Emit a Phase 2 tiled PTX GEMM kernel (shared memory + `ldmatrix` + `bar.sync`).
///
/// Returns `(ptx_source, shared_mem_bytes)` so the caller can report shared
/// memory usage in `CompilationInfo`.
pub fn emit_gemm_ptx_smem(
    shape: GemmShape,
    precision: GemmPrecision,
    sm: u8,
) -> Result<(String, u32), CompileError> {
    match precision {
        GemmPrecision::F16 | GemmPrecision::F16F32 => {
            phase2::emit_f16_gemm_smem(shape, precision, sm)
        }
        GemmPrecision::Tf32 => phase2::emit_tf32_gemm_smem(shape, sm),
    }
}

mod phase1;
mod phase2;

fn emit_header(
    ptx: &mut String,
    shape: &GemmShape,
    sm: u8,
    src_type: &str,
    acc_type: &str,
    mma_shape: &str,
    k_iters: u32,
) {
    writeln_ptx!(ptx, ".version 8.7");
    writeln_ptx!(ptx, ".target sm_{sm}");
    writeln_ptx!(ptx, ".address_size 64");
    writeln_ptx!(ptx);
    writeln_ptx!(
        ptx,
        "// GEMM: C[{m}x{n}] = A[{m}x{k}] * B[{k}x{n}]",
        m = shape.m,
        n = shape.n,
        k = shape.k
    );
    writeln_ptx!(
        ptx,
        "// Precision: {src_type} inputs, {acc_type} accumulate"
    );
    writeln_ptx!(ptx, "// MMA tile: {mma_shape}, K iterations: {k_iters}");
    writeln_ptx!(
        ptx,
        "// Grid: ({gx}, {gy}, 1) — 1 warp per CTA, 32 threads",
        gx = shape.n / 8,
        gy = shape.m / 16
    );
    writeln_ptx!(ptx);
}

fn emit_smem_header(
    ptx: &mut String,
    shape: &GemmShape,
    sm: u8,
    src_type: &str,
    acc_type: &str,
    mma_shape: &str,
    k_iters: u32,
) {
    writeln_ptx!(ptx, ".version 8.7");
    writeln_ptx!(ptx, ".target sm_{sm}");
    writeln_ptx!(ptx, ".address_size 64");
    writeln_ptx!(ptx);
    writeln_ptx!(
        ptx,
        "// GEMM (smem): C[{m}x{n}] = A[{m}x{k}] * B[{k}x{n}]",
        m = shape.m,
        n = shape.n,
        k = shape.k
    );
    writeln_ptx!(
        ptx,
        "// Precision: {src_type} inputs, {acc_type} accumulate"
    );
    writeln_ptx!(ptx, "// MMA tile: {mma_shape}, K iterations: {k_iters}");
    writeln_ptx!(
        ptx,
        "// Block tile: {BM}x{BN}, {WARPS_PER_CTA} warps ({CTA_THREADS} threads) per CTA"
    );
    writeln_ptx!(
        ptx,
        "// Grid: ({gx}, {gy}, 1)",
        gx = shape.n / BN,
        gy = shape.m / BM
    );
    writeln_ptx!(ptx);
}

fn emit_thread_identity(ptx: &mut String) {
    writeln_ptx!(ptx, "    // Load matrix base pointers");
    writeln_ptx!(ptx, "    ld.param.u64 %rd0, [param_A];");
    writeln_ptx!(ptx, "    ld.param.u64 %rd1, [param_B];");
    writeln_ptx!(ptx, "    ld.param.u64 %rd2, [param_C];");
    writeln_ptx!(ptx);
    writeln_ptx!(ptx, "    // Thread and block identity");
    writeln_ptx!(ptx, "    mov.u32 %r10, %tid.x;        // thread ID in CTA");
    writeln_ptx!(ptx, "    mov.u32 %r11, %ctaid.x;      // block along N");
    writeln_ptx!(ptx, "    mov.u32 %r12, %ctaid.y;      // block along M");
    writeln_ptx!(ptx);
}

fn emit_c_store_f32(ptx: &mut String, n_val: u32, n_row_bytes: u32) {
    writeln_ptx!(
        ptx,
        "    // Store C fragment (f32 accumulator, row-major M*N)"
    );
    writeln_ptx!(ptx, "    shl.b32 %r28, %r14, 1;       // group_id * 2");
    writeln_ptx!(
        ptx,
        "    add.u32 %r28, %r16, %r28;    // c_row0 = m_start + group_id * 2"
    );
    writeln_ptx!(ptx, "    shl.b32 %r29, %r15, 1;       // tid_in_group * 2");
    writeln_ptx!(
        ptx,
        "    add.u32 %r29, %r17, %r29;    // c_col0 = n_start + tid_in_group * 2"
    );
    writeln_ptx!(ptx, "    mul.lo.u32 %r30, %r28, {n_val};    // c_row0 * N");
    writeln_ptx!(
        ptx,
        "    add.u32 %r30, %r30, %r29;    // c_row0 * N + c_col0"
    );
    writeln_ptx!(ptx, "    shl.b32 %r30, %r30, 2;       // * sizeof(f32)");
    writeln_ptx!(ptx, "    cvt.u64.u32 %rd6, %r30;");
    writeln_ptx!(
        ptx,
        "    add.u64 %rd6, %rd2, %rd6;    // C addr for this thread's tile"
    );
    writeln_ptx!(ptx);
    writeln_ptx!(
        ptx,
        "    st.global.f32 [%rd6], %r0;              // C[row0, col0]"
    );
    writeln_ptx!(
        ptx,
        "    st.global.f32 [%rd6+4], %r1;            // C[row0, col0+1]"
    );
    writeln_ptx!(
        ptx,
        "    st.global.f32 [%rd6+{n_row_bytes}], %r2;   // C[row0+1, col0]"
    );
    writeln_ptx!(
        ptx,
        "    st.global.f32 [%rd6+{off}], %r3;   // C[row0+1, col0+1]",
        off = n_row_bytes + 4
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gemm_ptx_f16f32_has_thread_mapping() {
        let shape = GemmShape { m: 16, n: 8, k: 16 };
        let ptx = emit_gemm_ptx(shape, GemmPrecision::F16F32, 80).unwrap();
        assert!(ptx.contains(".target sm_80"));
        assert!(ptx.contains("mma.sync.aligned.m16n8k16.row.col.f32.f16.f16.f32"));
        assert!(ptx.contains("gemm_kernel"));
        assert!(ptx.contains(".reqntid 32"));
        assert!(ptx.contains("%tid.x"));
        assert!(ptx.contains("%ctaid.x"));
        assert!(ptx.contains("%ctaid.y"));
    }

    #[test]
    fn gemm_ptx_f16f32_correct_grid_comment() {
        let shape = GemmShape {
            m: 128,
            n: 64,
            k: 32,
        };
        let ptx = emit_gemm_ptx(shape, GemmPrecision::F16F32, 80).unwrap();
        assert!(ptx.contains("Grid: (8, 8, 1)"));
        assert!(ptx.contains("K iterations: 2"));
    }

    #[test]
    fn gemm_ptx_tf32_basic() {
        let shape = GemmShape { m: 16, n: 8, k: 32 };
        let ptx = emit_gemm_ptx(shape, GemmPrecision::Tf32, 80).unwrap();
        assert!(ptx.contains("mma.sync.aligned.m16n8k8.row.col.f32.tf32.tf32.f32"));
        assert!(ptx.contains(".reqntid 32"));
        assert!(ptx.contains("%ctaid.y"));
    }

    #[test]
    fn gemm_ptx_f16_accumulate() {
        let shape = GemmShape { m: 16, n: 8, k: 16 };
        let ptx = emit_gemm_ptx(shape, GemmPrecision::F16, 89).unwrap();
        assert!(ptx.contains("mma.sync.aligned.m16n8k16.row.col.f16.f16.f16.f16"));
        assert!(
            ptx.contains("{%r0, %r1}"),
            "f16 accumulate uses 2 regs, not 4"
        );
    }

    #[test]
    fn gemm_ptx_multi_k_iterations() {
        let shape = GemmShape { m: 16, n: 8, k: 64 };
        let ptx = emit_gemm_ptx(shape, GemmPrecision::F16F32, 120).unwrap();
        assert!(ptx.contains("K iteration 0"));
        assert!(ptx.contains("K iteration 3"));
        let mma_count = ptx
            .lines()
            .filter(|l| l.trim_start().starts_with("mma.sync.aligned"))
            .count();
        assert_eq!(mma_count, 4, "should have 4 MMA instructions for K=64/16");
    }

    #[test]
    fn gemm_ptx_sm120_blackwell() {
        let shape = GemmShape { m: 16, n: 8, k: 16 };
        let ptx = emit_gemm_ptx(shape, GemmPrecision::F16F32, 120).unwrap();
        assert!(ptx.contains(".target sm_120"));
    }

    #[test]
    fn gemm_ptx_c_store_uses_stride() {
        let shape = GemmShape {
            m: 32,
            n: 64,
            k: 16,
        };
        let ptx = emit_gemm_ptx(shape, GemmPrecision::F16F32, 86).unwrap();
        assert!(
            ptx.contains("mul.lo.u32 %r30, %r28, 64"),
            "C store should multiply by N=64"
        );
        assert!(
            ptx.contains("[%rd6+256]"),
            "next row offset = N * sizeof(f32) = 64 * 4 = 256"
        );
    }

    // --- Phase 2 (shared memory) tests ---

    #[test]
    fn smem_f16f32_has_shared_memory() {
        let shape = GemmShape {
            m: 64,
            n: 16,
            k: 16,
        };
        let (ptx, smem) = emit_gemm_ptx_smem(shape, GemmPrecision::F16F32, 80).unwrap();
        assert!(ptx.contains("smem_A"), "should declare smem_A");
        assert!(ptx.contains("smem_B"), "should declare smem_B");
        assert!(
            ptx.contains(".shared .align 16"),
            "shared mem must be aligned"
        );
        assert!(smem > 0, "shared mem bytes must be non-zero");
    }

    #[test]
    fn smem_f16f32_has_bar_sync() {
        let shape = GemmShape {
            m: 64,
            n: 16,
            k: 16,
        };
        let (ptx, _) = emit_gemm_ptx_smem(shape, GemmPrecision::F16F32, 80).unwrap();
        assert!(ptx.contains("bar.sync 0"), "must have barrier sync");
    }

    #[test]
    fn smem_f16f32_has_ldmatrix() {
        let shape = GemmShape {
            m: 64,
            n: 16,
            k: 16,
        };
        let (ptx, _) = emit_gemm_ptx_smem(shape, GemmPrecision::F16F32, 80).unwrap();
        assert!(
            ptx.contains("ldmatrix.sync.aligned"),
            "must use ldmatrix for fragment loads"
        );
    }

    #[test]
    fn smem_f16f32_correct_cta_threads() {
        let shape = GemmShape {
            m: 64,
            n: 16,
            k: 16,
        };
        let (ptx, _) = emit_gemm_ptx_smem(shape, GemmPrecision::F16F32, 80).unwrap();
        assert!(
            ptx.contains(".reqntid 128"),
            "Phase 2 should use 128 threads (4 warps)"
        );
    }

    #[test]
    fn smem_f16f32_has_mma() {
        let shape = GemmShape {
            m: 64,
            n: 16,
            k: 16,
        };
        let (ptx, _) = emit_gemm_ptx_smem(shape, GemmPrecision::F16F32, 80).unwrap();
        assert!(ptx.contains("mma.sync.aligned.m16n8k16.row.col.f32.f16.f16.f32"));
    }

    #[test]
    fn smem_tf32_basic() {
        let shape = GemmShape { m: 64, n: 16, k: 8 };
        let (ptx, smem) = emit_gemm_ptx_smem(shape, GemmPrecision::Tf32, 80).unwrap();
        assert!(ptx.contains("mma.sync.aligned.m16n8k8.row.col.f32.tf32.tf32.f32"));
        assert!(ptx.contains("smem_A"));
        assert!(ptx.contains("bar.sync 0"));
        assert!(smem > 0);
    }

    #[test]
    fn smem_shared_mem_size_f16() {
        let shape = GemmShape {
            m: 64,
            n: 16,
            k: 16,
        };
        let (_, smem) = emit_gemm_ptx_smem(shape, GemmPrecision::F16F32, 80).unwrap();
        let expected_a = 64 * 16 * 2; // BM * BK * sizeof(f16) = 2048
        let expected_b = 16 * 16 * 2; // BK * BN * sizeof(f16) = 512
        assert_eq!(smem, expected_a + expected_b, "A(2048) + B(512) = 2560");
    }

    #[test]
    fn smem_shared_mem_size_tf32() {
        let shape = GemmShape { m: 64, n: 16, k: 8 };
        let (_, smem) = emit_gemm_ptx_smem(shape, GemmPrecision::Tf32, 80).unwrap();
        let expected_a = 64 * 8 * 4; // BM * BK * sizeof(f32) = 2048
        let expected_b = 8 * 16 * 4; // BK * BN * sizeof(f32) = 512
        assert_eq!(smem, expected_a + expected_b, "A(2048) + B(512) = 2560");
    }

    #[test]
    fn smem_multi_k_iterations() {
        let shape = GemmShape {
            m: 64,
            n: 16,
            k: 64,
        };
        let (ptx, _) = emit_gemm_ptx_smem(shape, GemmPrecision::F16F32, 86).unwrap();
        assert!(ptx.contains("K iteration 0"));
        assert!(ptx.contains("K iteration 3"));
        let mma_count = ptx
            .lines()
            .filter(|l| l.trim_start().starts_with("mma.sync.aligned"))
            .count();
        assert_eq!(
            mma_count,
            4 * 2,
            "4 K iterations × 2 N-tiles = 8 MMA instructions"
        );
    }

    #[test]
    fn smem_grid_comment() {
        let shape = GemmShape {
            m: 128,
            n: 32,
            k: 16,
        };
        let (ptx, _) = emit_gemm_ptx_smem(shape, GemmPrecision::F16F32, 80).unwrap();
        assert!(
            ptx.contains("Grid: (2, 2, 1)"),
            "Grid should be (N/BN, M/BM) = (32/16, 128/64)"
        );
    }
}
