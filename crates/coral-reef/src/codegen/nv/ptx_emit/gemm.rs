// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tensor-core GEMM PTX generation.
//!
//! Emits a complete PTX kernel using `mma.sync.aligned` instructions for
//! NVIDIA SM80+ (Ampere, Ada, Blackwell). The kernel performs:
//!   C[M×N] = A[M×K] * B[K×N] + C[M×N]
//!
//! Uses warp-level MMA tiles (16×8×16 for f16, 16×8×8 for TF32).

use std::fmt::Write as _;

use crate::error::CompileError;
use crate::gemm::{GemmPrecision, GemmShape};

/// Emit a complete PTX GEMM kernel for tensor-core execution.
pub fn emit_gemm_ptx(
    shape: GemmShape,
    precision: GemmPrecision,
    sm: u8,
) -> Result<String, CompileError> {
    let (mma_shape, src_type, dst_type, tile_k) = match precision {
        GemmPrecision::F16 => ("m16n8k16", "f16", "f16", 16u32),
        GemmPrecision::F16F32 => ("m16n8k16", "f16", "f32", 16u32),
        GemmPrecision::Tf32 => ("m16n8k8", "tf32", "f32", 8u32),
    };

    let k_iters = shape.k / tile_k;
    let acc_type = if precision == GemmPrecision::F16 {
        "f16"
    } else {
        "f32"
    };

    let mut ptx = String::with_capacity(4096);

    writeln!(ptx, ".version 8.7").expect("write to String");
    writeln!(ptx, ".target sm_{sm}").expect("write to String");
    writeln!(ptx, ".address_size 64").expect("write to String");
    writeln!(ptx).expect("write to String");

    writeln!(
        ptx,
        "// GEMM: C[{m}x{n}] = A[{m}x{k}] * B[{k}x{n}] + C[{m}x{n}]",
        m = shape.m,
        n = shape.n,
        k = shape.k
    )
    .expect("write to String");
    writeln!(
        ptx,
        "// Precision: {src_type} inputs, {acc_type} accumulate"
    )
    .expect("write to String");
    writeln!(ptx, "// MMA tile: {mma_shape}, K iterations: {k_iters}").expect("write to String");
    writeln!(ptx).expect("write to String");

    writeln!(ptx, ".visible .entry gemm_kernel(").expect("write to String");
    writeln!(ptx, "    .param .u64 param_A,").expect("write to String");
    writeln!(ptx, "    .param .u64 param_B,").expect("write to String");
    writeln!(ptx, "    .param .u64 param_C").expect("write to String");
    writeln!(ptx, ")").expect("write to String");
    writeln!(ptx, "{{").expect("write to String");

    writeln!(ptx, "    .reg .b64 %rd<8>;").expect("write to String");
    writeln!(ptx, "    .reg .b32 %r<32>;").expect("write to String");
    writeln!(ptx, "    .reg .pred %p<4>;").expect("write to String");
    writeln!(ptx).expect("write to String");

    writeln!(ptx, "    // Load matrix pointers").expect("write to String");
    writeln!(ptx, "    ld.param.u64 %rd0, [param_A];").expect("write to String");
    writeln!(ptx, "    ld.param.u64 %rd1, [param_B];").expect("write to String");
    writeln!(ptx, "    ld.param.u64 %rd2, [param_C];").expect("write to String");
    writeln!(ptx).expect("write to String");

    writeln!(ptx, "    // Zero accumulator registers (4 x {acc_type})").expect("write to String");
    for i in 0..4u32 {
        writeln!(ptx, "    mov.b32 %r{i}, 0;").expect("write to String");
    }
    writeln!(ptx).expect("write to String");

    writeln!(
        ptx,
        "    // K-loop: {k_iters} iterations of mma.sync.aligned.{mma_shape}"
    )
    .expect("write to String");
    for iter in 0..k_iters {
        let a_offset = iter * tile_k * 2;
        let b_offset = iter * tile_k * 2;

        writeln!(ptx, "    // --- K iteration {iter} ---").expect("write to String");
        writeln!(ptx, "    // Load A fragment (4 x f16 packed as 2 x b32)")
            .expect("write to String");
        writeln!(ptx, "    ld.global.v2.b32 {{%r4, %r5}}, [%rd0+{a_offset}];")
            .expect("write to String");
        writeln!(
            ptx,
            "    ld.global.v2.b32 {{%r6, %r7}}, [%rd0+{off}];",
            off = a_offset + 8
        )
        .expect("write to String");

        writeln!(ptx, "    // Load B fragment (2 x f16 packed as 1 x b32)")
            .expect("write to String");
        writeln!(ptx, "    ld.global.v2.b32 {{%r8, %r9}}, [%rd1+{b_offset}];")
            .expect("write to String");

        writeln!(
            ptx,
            "    mma.sync.aligned.{mma_shape}.row.col.{dst_type}.{src_type}.{src_type}.{acc_type}"
        )
        .expect("write to String");
        writeln!(
            ptx,
            "        {{%r0, %r1, %r2, %r3}}, {{%r4, %r5, %r6, %r7}}, {{%r8, %r9}}, {{%r0, %r1, %r2, %r3}};"
        ).expect("write to String");
    }

    writeln!(ptx).expect("write to String");
    writeln!(ptx, "    // Store C fragment").expect("write to String");
    writeln!(ptx, "    st.global.v4.b32 [%rd2], {{%r0, %r1, %r2, %r3}};").expect("write to String");
    writeln!(ptx).expect("write to String");
    writeln!(ptx, "    ret;").expect("write to String");
    writeln!(ptx, "}}").expect("write to String");

    Ok(ptx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gemm_ptx_f16f32_basic() {
        let shape = GemmShape { m: 16, n: 8, k: 16 };
        let ptx = emit_gemm_ptx(shape, GemmPrecision::F16F32, 80).unwrap();
        assert!(ptx.contains(".target sm_80"));
        assert!(ptx.contains("mma.sync.aligned.m16n8k16.row.col.f32.f16.f16.f32"));
        assert!(ptx.contains("gemm_kernel"));
    }

    #[test]
    fn gemm_ptx_tf32_basic() {
        let shape = GemmShape { m: 16, n: 8, k: 32 };
        let ptx = emit_gemm_ptx(shape, GemmPrecision::Tf32, 80).unwrap();
        assert!(ptx.contains("mma.sync.aligned.m16n8k8.row.col.f32.tf32.tf32.f32"));
    }

    #[test]
    fn gemm_ptx_f16_accumulate() {
        let shape = GemmShape { m: 16, n: 8, k: 16 };
        let ptx = emit_gemm_ptx(shape, GemmPrecision::F16, 89).unwrap();
        assert!(ptx.contains("mma.sync.aligned.m16n8k16.row.col.f16.f16.f16.f16"));
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
}
