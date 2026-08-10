// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::error::CompileError;
use crate::gemm::{GemmPrecision, GemmShape};
use super::{emit_c_store_f32, emit_header, emit_thread_identity};

/// f16/f16f32 GEMM — m16n8k16 MMA shape.
///
/// Fragment layout (per thread, lane t in warp):
///   groupID = t >> 2  (0..7)
///   tid     = t & 3   (0..3)
///
/// A (row-major, f16): 4 regs (8 f16 values packed as 4 b32)
///   reg0 = {A[gid,     tid*2],   A[gid,     tid*2+1]}   k=0..7 half
///   reg1 = {A[gid,     tid*2+8], A[gid,     tid*2+9]}   k=8..15 half
///   reg2 = {A[gid+8,   tid*2],   A[gid+8,   tid*2+1]}
///   reg3 = {A[gid+8,   tid*2+8], A[gid+8,   tid*2+9]}
///
/// B (col-major, f16): 2 regs (4 f16 values packed as 2 b32)
///   reg0 = {B[tid*2,   gid], B[tid*2+1, gid]}
///   reg1 = {B[tid*2+8, gid], B[tid*2+9, gid]}
///
/// C (row-major, f32): 4 regs (f16f32) or 2 regs (f16)
///   f32 layout: reg0=C[gid*2, tid*2], reg1=C[gid*2, tid*2+1],
///               reg2=C[gid*2+1, tid*2], reg3=C[gid*2+1, tid*2+1]
pub(super) fn emit_f16_gemm(
    shape: GemmShape,
    precision: GemmPrecision,
    sm: u8,
) -> Result<String, CompileError> {
    let tile_k: u32 = 16;
    let k_iters = shape.k / tile_k;
    let f32_acc = precision == GemmPrecision::F16F32;
    let (src_type, dst_type, acc_type) = if f32_acc {
        ("f16", "f32", "f32")
    } else {
        ("f16", "f16", "f16")
    };
    let acc_regs: u32 = if f32_acc { 4 } else { 2 };
    let c_elem_bytes: u32 = if f32_acc { 4 } else { 2 };
    let k_times_2 = shape.k * 2; // A/B stride in bytes (f16 = 2 bytes)

    let mut ptx = String::with_capacity(8192);

    emit_header(&mut ptx, &shape, sm, src_type, acc_type, "m16n8k16", k_iters);

    writeln_ptx!(ptx, ".visible .entry gemm_kernel(");
    writeln_ptx!(ptx, "    .param .u64 param_A,");
    writeln_ptx!(ptx, "    .param .u64 param_B,");
    writeln_ptx!(ptx, "    .param .u64 param_C");
    writeln_ptx!(ptx, ")");
    writeln_ptx!(ptx, ".reqntid 32");
    writeln_ptx!(ptx, "{{");

    writeln_ptx!(ptx, "    .reg .b64 %rd<16>;");
    writeln_ptx!(ptx, "    .reg .b32 %r<48>;");
    writeln_ptx!(ptx, "    .reg .pred %p<4>;");
    writeln_ptx!(ptx);

    emit_thread_identity(&mut ptx);

    writeln_ptx!(ptx, "    // Per-thread fragment lane decomposition");
    writeln_ptx!(ptx, "    and.u32 %r13, %r10, 31;");
    writeln_ptx!(ptx, "    shr.u32 %r14, %r13, 2;      // group_id (0..7)");
    writeln_ptx!(ptx, "    and.u32 %r15, %r13, 3;       // tid_in_group (0..3)");
    writeln_ptx!(ptx);

    writeln_ptx!(ptx, "    // Warp tile position in output matrix");
    writeln_ptx!(ptx, "    mul.lo.u32 %r16, %r12, 16;   // m_start = ctaid.y * 16");
    writeln_ptx!(ptx, "    mul.lo.u32 %r17, %r11, 8;    // n_start = ctaid.x * 8");
    writeln_ptx!(ptx);

    // Zero accumulators
    writeln_ptx!(
        ptx,
        "    // Zero accumulator registers ({acc_regs} x {acc_type})"
    );
    for i in 0..acc_regs {
        writeln_ptx!(ptx, "    mov.b32 %r{i}, 0;");
    }
    writeln_ptx!(ptx);

    // Precompute A row base addresses (constant across K loop)
    // a_row0 = m_start + group_id, a_row1 = a_row0 + 8
    // a_row_base = A + row * K * 2 + tid_in_group * 2 * 2
    writeln_ptx!(ptx, "    // Precompute A fragment row base addresses");
    writeln_ptx!(ptx, "    add.u32 %r18, %r16, %r14;    // a_row0 = m_start + group_id");
    writeln_ptx!(
        ptx,
        "    mul.lo.u32 %r19, %r18, {k_times_2}; // a_row0 * K * sizeof(f16)"
    );
    writeln_ptx!(ptx, "    shl.b32 %r20, %r15, 2;       // tid_in_group * 4 (byte offset for 2 f16)");
    writeln_ptx!(ptx, "    add.u32 %r21, %r19, %r20;    // a_row0_byte_offset");
    writeln_ptx!(ptx, "    cvt.u64.u32 %rd3, %r21;");
    writeln_ptx!(ptx, "    add.u64 %rd3, %rd0, %rd3;    // a_row0_base ptr");
    writeln_ptx!(ptx);
    writeln_ptx!(ptx, "    add.u32 %r22, %r18, 8;       // a_row1 = a_row0 + 8");
    writeln_ptx!(
        ptx,
        "    mul.lo.u32 %r23, %r22, {k_times_2};"
    );
    writeln_ptx!(ptx, "    add.u32 %r24, %r23, %r20;");
    writeln_ptx!(ptx, "    cvt.u64.u32 %rd4, %r24;");
    writeln_ptx!(ptx, "    add.u64 %rd4, %rd0, %rd4;    // a_row1_base ptr");
    writeln_ptx!(ptx);

    // Precompute B column base address
    // B is col-major K×N: B[k, n] = B + (n * K + k) * 2
    // b_col = n_start + group_id
    // b_col_base = B + b_col * K * 2 + tid_in_group * 2 * 2
    writeln_ptx!(ptx, "    // Precompute B fragment column base address");
    writeln_ptx!(ptx, "    add.u32 %r25, %r17, %r14;    // b_col = n_start + group_id");
    writeln_ptx!(
        ptx,
        "    mul.lo.u32 %r26, %r25, {k_times_2}; // b_col * K * sizeof(f16)"
    );
    writeln_ptx!(ptx, "    add.u32 %r27, %r26, %r20;    // b_col_byte_offset");
    writeln_ptx!(ptx, "    cvt.u64.u32 %rd5, %r27;");
    writeln_ptx!(ptx, "    add.u64 %rd5, %rd1, %rd5;    // b_col_base ptr");
    writeln_ptx!(ptx);

    // K-loop (unrolled)
    writeln_ptx!(
        ptx,
        "    // K-loop: {k_iters} iterations of mma.sync.aligned.m16n8k16"
    );
    for ki in 0..k_iters {
        let k_byte_off = ki * tile_k * 2; // byte offset for this K slice

        writeln_ptx!(ptx, "    // --- K iteration {ki} (k_base={kb}) ---", kb = ki * tile_k);
        writeln_ptx!(
            ptx,
            "    ld.global.b32 %r4, [%rd3+{k_byte_off}];"
        );
        writeln_ptx!(
            ptx,
            "    ld.global.b32 %r5, [%rd3+{off}];",
            off = k_byte_off + 16
        );
        writeln_ptx!(
            ptx,
            "    ld.global.b32 %r6, [%rd4+{k_byte_off}];"
        );
        writeln_ptx!(
            ptx,
            "    ld.global.b32 %r7, [%rd4+{off}];",
            off = k_byte_off + 16
        );

        writeln_ptx!(
            ptx,
            "    ld.global.b32 %r8, [%rd5+{k_byte_off}];"
        );
        writeln_ptx!(
            ptx,
            "    ld.global.b32 %r9, [%rd5+{off}];",
            off = k_byte_off + 16
        );

        if f32_acc {
            writeln_ptx!(
                ptx,
                "    mma.sync.aligned.m16n8k16.row.col.{dst_type}.{src_type}.{src_type}.{acc_type}"
            );
            writeln_ptx!(
                ptx,
                "        {{%r0, %r1, %r2, %r3}}, {{%r4, %r5, %r6, %r7}}, {{%r8, %r9}}, {{%r0, %r1, %r2, %r3}};"
            );
        } else {
            writeln_ptx!(
                ptx,
                "    mma.sync.aligned.m16n8k16.row.col.{dst_type}.{src_type}.{src_type}.{acc_type}"
            );
            writeln_ptx!(
                ptx,
                "        {{%r0, %r1}}, {{%r4, %r5, %r6, %r7}}, {{%r8, %r9}}, {{%r0, %r1}};"
            );
        }
    }

    writeln_ptx!(ptx);

    // Store C fragment
    // C is row-major M×N: C[m, n] = C + (m * N + n) * c_elem_bytes
    // c_row0 = m_start + group_id * 2
    // c_col0 = n_start + tid_in_group * 2
    let n_val = shape.n;
    let n_row_bytes = n_val * c_elem_bytes; // bytes per row of C

    writeln_ptx!(ptx, "    // Store C fragment to global memory");
    writeln_ptx!(ptx, "    shl.b32 %r28, %r14, 1;       // group_id * 2");
    writeln_ptx!(ptx, "    add.u32 %r28, %r16, %r28;    // c_row0 = m_start + group_id * 2");
    writeln_ptx!(ptx, "    shl.b32 %r29, %r15, 1;       // tid_in_group * 2");
    writeln_ptx!(ptx, "    add.u32 %r29, %r17, %r29;    // c_col0 = n_start + tid_in_group * 2");
    writeln_ptx!(
        ptx,
        "    mul.lo.u32 %r30, %r28, {n_val};    // c_row0 * N"
    );
    writeln_ptx!(ptx, "    add.u32 %r30, %r30, %r29;    // c_row0 * N + c_col0");
    writeln_ptx!(
        ptx,
        "    shl.b32 %r30, %r30, {shift};        // * sizeof(element)",
        shift = if f32_acc { 2 } else { 1 }
    );
    writeln_ptx!(ptx, "    cvt.u64.u32 %rd6, %r30;");
    writeln_ptx!(ptx, "    add.u64 %rd6, %rd2, %rd6;    // C base addr for this thread");
    writeln_ptx!(ptx);

    if f32_acc {
        writeln_ptx!(ptx, "    st.global.f32 [%rd6], %r0;              // C[row0, col0]");
        writeln_ptx!(ptx, "    st.global.f32 [%rd6+4], %r1;            // C[row0, col0+1]");
        writeln_ptx!(
            ptx,
            "    st.global.f32 [%rd6+{off}], %r2;   // C[row0+1, col0]",
            off = n_row_bytes
        );
        writeln_ptx!(
            ptx,
            "    st.global.f32 [%rd6+{off}], %r3;   // C[row0+1, col0+1]",
            off = n_row_bytes + 4
        );
    } else {
        // f16 accumulate: 2 regs, each a packed f16x2
        writeln_ptx!(ptx, "    st.global.b32 [%rd6], %r0;              // C[row0, col0:col0+1]");
        writeln_ptx!(
            ptx,
            "    st.global.b32 [%rd6+{off}], %r1;   // C[row0+1, col0:col0+1]",
            off = n_row_bytes
        );
    }

    writeln_ptx!(ptx);
    writeln_ptx!(ptx, "    ret;");
    writeln_ptx!(ptx, "}}");

    Ok(ptx)
}

/// TF32 GEMM — m16n8k8 MMA shape, f32-sized elements.
///
/// Fragment layout differs from f16: each register holds 1 tf32 (as f32),
/// not 2 packed f16.
///
/// A (row-major, f32): 4 regs
///   reg0 = A[gid, tid],   reg1 = A[gid, tid+4]
///   reg2 = A[gid+8, tid], reg3 = A[gid+8, tid+4]
///
/// B (col-major, f32): 2 regs
///   reg0 = B[tid, gid], reg1 = B[tid+4, gid]
pub(super) fn emit_tf32_gemm(shape: GemmShape, sm: u8) -> Result<String, CompileError> {
    let tile_k: u32 = 8;
    let k_iters = shape.k / tile_k;
    let k_times_4 = shape.k * 4; // stride in bytes (f32 = 4 bytes)

    let mut ptx = String::with_capacity(8192);

    emit_header(&mut ptx, &shape, sm, "tf32", "f32", "m16n8k8", k_iters);

    writeln_ptx!(ptx, ".visible .entry gemm_kernel(");
    writeln_ptx!(ptx, "    .param .u64 param_A,");
    writeln_ptx!(ptx, "    .param .u64 param_B,");
    writeln_ptx!(ptx, "    .param .u64 param_C");
    writeln_ptx!(ptx, ")");
    writeln_ptx!(ptx, ".reqntid 32");
    writeln_ptx!(ptx, "{{");

    writeln_ptx!(ptx, "    .reg .b64 %rd<16>;");
    writeln_ptx!(ptx, "    .reg .b32 %r<48>;");
    writeln_ptx!(ptx, "    .reg .pred %p<4>;");
    writeln_ptx!(ptx);

    emit_thread_identity(&mut ptx);

    writeln_ptx!(ptx, "    // Per-thread fragment lane decomposition");
    writeln_ptx!(ptx, "    and.u32 %r13, %r10, 31;");
    writeln_ptx!(ptx, "    shr.u32 %r14, %r13, 2;      // group_id (0..7)");
    writeln_ptx!(ptx, "    and.u32 %r15, %r13, 3;       // tid_in_group (0..3)");
    writeln_ptx!(ptx);

    writeln_ptx!(ptx, "    // Warp tile position in output matrix");
    writeln_ptx!(ptx, "    mul.lo.u32 %r16, %r12, 16;   // m_start = ctaid.y * 16");
    writeln_ptx!(ptx, "    mul.lo.u32 %r17, %r11, 8;    // n_start = ctaid.x * 8");
    writeln_ptx!(ptx);

    writeln_ptx!(ptx, "    // Zero accumulator registers (4 x f32)");
    for i in 0..4u32 {
        writeln_ptx!(ptx, "    mov.b32 %r{i}, 0;");
    }
    writeln_ptx!(ptx);

    // A base addresses: A + (a_row * K + tid_in_group) * 4
    writeln_ptx!(ptx, "    // Precompute A fragment row base addresses (tf32, 4 bytes/elem)");
    writeln_ptx!(ptx, "    add.u32 %r18, %r16, %r14;    // a_row0 = m_start + group_id");
    writeln_ptx!(
        ptx,
        "    mul.lo.u32 %r19, %r18, {k_times_4}; // a_row0 * K * sizeof(f32)"
    );
    writeln_ptx!(ptx, "    shl.b32 %r20, %r15, 2;       // tid_in_group * 4 (1 f32 element)");
    writeln_ptx!(ptx, "    add.u32 %r21, %r19, %r20;");
    writeln_ptx!(ptx, "    cvt.u64.u32 %rd3, %r21;");
    writeln_ptx!(ptx, "    add.u64 %rd3, %rd0, %rd3;    // a_row0_base");
    writeln_ptx!(ptx);
    writeln_ptx!(ptx, "    add.u32 %r22, %r18, 8;       // a_row1 = a_row0 + 8");
    writeln_ptx!(
        ptx,
        "    mul.lo.u32 %r23, %r22, {k_times_4};"
    );
    writeln_ptx!(ptx, "    add.u32 %r24, %r23, %r20;");
    writeln_ptx!(ptx, "    cvt.u64.u32 %rd4, %r24;");
    writeln_ptx!(ptx, "    add.u64 %rd4, %rd0, %rd4;    // a_row1_base");
    writeln_ptx!(ptx);

    // B base addresses: B + (b_col * K + tid_in_group) * 4
    writeln_ptx!(ptx, "    // Precompute B fragment column base address (tf32)");
    writeln_ptx!(ptx, "    add.u32 %r25, %r17, %r14;    // b_col = n_start + group_id");
    writeln_ptx!(
        ptx,
        "    mul.lo.u32 %r26, %r25, {k_times_4};"
    );
    writeln_ptx!(ptx, "    add.u32 %r27, %r26, %r20;");
    writeln_ptx!(ptx, "    cvt.u64.u32 %rd5, %r27;");
    writeln_ptx!(ptx, "    add.u64 %rd5, %rd1, %rd5;    // b_col_base");
    writeln_ptx!(ptx);

    // K-loop
    writeln_ptx!(
        ptx,
        "    // K-loop: {k_iters} iterations of mma.sync.aligned.m16n8k8"
    );
    for ki in 0..k_iters {
        let k_byte_off = ki * tile_k * 4; // f32 elements, 4 bytes each

        writeln_ptx!(ptx, "    // --- K iteration {ki} ---");
        // A: reg0 = A[row0, tid], reg1 = A[row0, tid+4] → offset +16 bytes (4 elements * 4 bytes)
        writeln_ptx!(
            ptx,
            "    ld.global.b32 %r4, [%rd3+{k_byte_off}];"
        );
        writeln_ptx!(
            ptx,
            "    ld.global.b32 %r5, [%rd3+{off}];",
            off = k_byte_off + 16
        );
        writeln_ptx!(
            ptx,
            "    ld.global.b32 %r6, [%rd4+{k_byte_off}];"
        );
        writeln_ptx!(
            ptx,
            "    ld.global.b32 %r7, [%rd4+{off}];",
            off = k_byte_off + 16
        );

        writeln_ptx!(
            ptx,
            "    ld.global.b32 %r8, [%rd5+{k_byte_off}];"
        );
        writeln_ptx!(
            ptx,
            "    ld.global.b32 %r9, [%rd5+{off}];",
            off = k_byte_off + 16
        );

        writeln_ptx!(
            ptx,
            "    mma.sync.aligned.m16n8k8.row.col.f32.tf32.tf32.f32"
        );
        writeln_ptx!(
            ptx,
            "        {{%r0, %r1, %r2, %r3}}, {{%r4, %r5, %r6, %r7}}, {{%r8, %r9}}, {{%r0, %r1, %r2, %r3}};"
        );
    }

    writeln_ptx!(ptx);

    // C store — identical to f16f32 (f32 accumulator, row-major)
    let n_val = shape.n;
    let n_row_bytes = n_val * 4;
    emit_c_store_f32(&mut ptx, n_val, n_row_bytes);

    writeln_ptx!(ptx);
    writeln_ptx!(ptx, "    ret;");
    writeln_ptx!(ptx, "}}");

    Ok(ptx)
}
