// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{BM, BN, CTA_THREADS, emit_smem_header};
use crate::error::CompileError;
use crate::gemm::{GemmPrecision, GemmShape};

/// Phase 2 f16/f16f32 GEMM with shared memory tiling.
///
/// Block tile: BM×BN = 64×16. Each CTA has 4 warps (128 threads).
/// Warps are stacked vertically: warp `w` covers rows `[w*16, (w+1)*16)`.
/// Each warp computes 2 MMA tiles along N (16×8 × 2 = 16×16).
///
/// K-loop pipeline:
///   1. Cooperative load A[BM×BK] and B[BK×BN] from global → shared memory
///   2. `bar.sync 0` — all threads must finish writing smem
///   3. `ldmatrix.sync` — warp-cooperative fragment load from smem
///   4. `mma.sync.aligned` × 2 per warp
///   5. `bar.sync 0` — safe to overwrite smem in next iteration
pub(super) fn emit_f16_gemm_smem(
    shape: GemmShape,
    precision: GemmPrecision,
    sm: u8,
) -> Result<(String, u32), CompileError> {
    let bk: u32 = 16; // MMA tile-K for f16
    let k_iters = shape.k / bk;
    let f32_acc = precision == GemmPrecision::F16F32;
    let (src_type, dst_type, acc_type) = if f32_acc {
        ("f16", "f32", "f32")
    } else {
        ("f16", "f16", "f16")
    };
    let acc_regs: u32 = if f32_acc { 4 } else { 2 };
    let c_elem_bytes: u32 = if f32_acc { 4 } else { 2 };

    // Shared memory layout (f16 = 2 bytes per element)
    let smem_a_bytes = BM * bk * 2; // 64 * 16 * 2 = 2048
    let smem_b_bytes = bk * BN * 2; // 16 * 16 * 2 = 512
    let total_smem = smem_a_bytes + smem_b_bytes;

    let mut ptx = String::with_capacity(16384);

    emit_smem_header(
        &mut ptx, &shape, sm, src_type, acc_type, "m16n8k16", k_iters,
    );

    writeln_ptx!(ptx, ".visible .entry gemm_kernel(");
    writeln_ptx!(ptx, "    .param .u64 param_A,");
    writeln_ptx!(ptx, "    .param .u64 param_B,");
    writeln_ptx!(ptx, "    .param .u64 param_C");
    writeln_ptx!(ptx, ")");
    writeln_ptx!(ptx, ".reqntid {CTA_THREADS}");
    writeln_ptx!(ptx, "{{");

    // Register declarations
    writeln_ptx!(ptx, "    .reg .b64 %rd<24>;");
    writeln_ptx!(ptx, "    .reg .b32 %r<64>;");
    writeln_ptx!(ptx, "    .reg .pred %p<4>;");
    writeln_ptx!(ptx);

    // Shared memory declarations
    writeln_ptx!(ptx, "    .shared .align 16 .b8 smem_A[{smem_a_bytes}];");
    writeln_ptx!(ptx, "    .shared .align 16 .b8 smem_B[{smem_b_bytes}];");
    writeln_ptx!(ptx);

    // Load params and identity
    writeln_ptx!(ptx, "    ld.param.u64 %rd0, [param_A];");
    writeln_ptx!(ptx, "    ld.param.u64 %rd1, [param_B];");
    writeln_ptx!(ptx, "    ld.param.u64 %rd2, [param_C];");
    writeln_ptx!(ptx);
    writeln_ptx!(ptx, "    mov.u32 %r10, %tid.x;");
    writeln_ptx!(ptx, "    mov.u32 %r11, %ctaid.x;");
    writeln_ptx!(ptx, "    mov.u32 %r12, %ctaid.y;");
    writeln_ptx!(ptx);

    // Warp and lane identity
    writeln_ptx!(ptx, "    // Warp and lane decomposition");
    writeln_ptx!(
        ptx,
        "    shr.u32 %r40, %r10, 5;          // warp_id = tid / 32"
    );
    writeln_ptx!(
        ptx,
        "    and.u32 %r41, %r10, 31;         // lane_id = tid & 31"
    );
    writeln_ptx!(
        ptx,
        "    shr.u32 %r14, %r41, 2;          // group_id = lane / 4"
    );
    writeln_ptx!(
        ptx,
        "    and.u32 %r15, %r41, 3;          // tid_in_group = lane & 3"
    );
    writeln_ptx!(ptx);

    // Block tile position in output matrix
    writeln_ptx!(ptx, "    // Block tile position");
    let bm_val = BM;
    let bn_val = BN;
    writeln_ptx!(
        ptx,
        "    mul.lo.u32 %r16, %r12, {bm_val};  // m_block = ctaid.y * BM"
    );
    writeln_ptx!(
        ptx,
        "    mul.lo.u32 %r17, %r11, {bn_val};  // n_block = ctaid.x * BN"
    );
    writeln_ptx!(ptx);

    // Warp's M offset within block tile
    writeln_ptx!(ptx, "    // Warp M offset within block tile");
    writeln_ptx!(
        ptx,
        "    shl.b32 %r42, %r40, 4;          // warp_m_off = warp_id * 16"
    );
    writeln_ptx!(ptx);

    // Zero accumulators (2 MMA tiles per warp along N)
    writeln_ptx!(
        ptx,
        "    // Zero accumulator registers (2 MMA tiles x {acc_regs} regs)"
    );
    for tile_n in 0..2u32 {
        for r in 0..acc_regs {
            let reg_idx = tile_n * acc_regs + r;
            writeln_ptx!(ptx, "    mov.b32 %r{reg_idx}, 0;");
        }
    }
    writeln_ptx!(ptx);

    // Cooperative load helpers: each of 128 threads loads one element
    // A[BM][BK] = 64*16 = 1024 f16 elements → 8 elements per thread (1024/128)
    // B[BK][BN] = 16*16 = 256 f16 elements → 2 elements per thread (256/128)
    let a_elems_per_thread: u32 = (BM * bk) / CTA_THREADS;
    let b_elems_per_thread: u32 = (bk * BN) / CTA_THREADS;
    let k_times_2 = shape.k * 2;

    // Precompute cooperative load row/col for this thread
    // A: thread `t` loads A[row, col] where linear = t * elems_per_thread + elem_idx
    //    row = linear / BK, col = linear % BK, byte = row * K * 2 + col * 2
    writeln_ptx!(ptx, "    // === K-loop with shared memory ===");

    for ki in 0..k_iters {
        let k_base = ki * bk;

        writeln_ptx!(ptx, "    // --- K iteration {ki} (k_base={k_base}) ---");
        writeln_ptx!(ptx);

        // Cooperative load A[BM][BK] → smem_A
        writeln_ptx!(ptx, "    // Cooperative load A tile to shared memory");
        for elem in 0..a_elems_per_thread {
            let linear_base = elem * CTA_THREADS;

            // At runtime: linear = {linear_base} + %r10
            // row = linear / BK = linear / 16 = linear >> 4
            // col = linear % BK = linear & 15
            // global addr = A + (m_block + row) * K * 2 + (k_base + col) * 2
            // smem addr = smem_A + linear * 2
            writeln_ptx!(
                ptx,
                "    add.u32 %r50, %r10, {linear_base};  // linear = tid + {linear_base}"
            );
            writeln_ptx!(
                ptx,
                "    shr.u32 %r51, %r50, 4;          // a_row = linear / 16"
            );
            writeln_ptx!(
                ptx,
                "    and.u32 %r52, %r50, 15;         // a_col = linear & 15"
            );
            writeln_ptx!(
                ptx,
                "    add.u32 %r51, %r51, %r16;       // global_row = m_block + a_row"
            );
            writeln_ptx!(
                ptx,
                "    add.u32 %r52, %r52, {k_base};       // global_col = k_base + a_col"
            );
            writeln_ptx!(
                ptx,
                "    mul.lo.u32 %r53, %r51, {k_times_2}; // global_row * K * 2"
            );
            writeln_ptx!(ptx, "    shl.b32 %r54, %r52, 1;          // global_col * 2");
            writeln_ptx!(ptx, "    add.u32 %r53, %r53, %r54;");
            writeln_ptx!(ptx, "    cvt.u64.u32 %rd10, %r53;");
            writeln_ptx!(ptx, "    add.u64 %rd10, %rd0, %rd10;");
            writeln_ptx!(ptx, "    ld.global.u16 %r55, [%rd10];");
            writeln_ptx!(
                ptx,
                "    shl.b32 %r56, %r50, 1;          // smem offset = linear * 2"
            );
            writeln_ptx!(ptx, "    mov.u32 %r57, smem_A;");
            writeln_ptx!(ptx, "    add.u32 %r57, %r57, %r56;");
            writeln_ptx!(ptx, "    st.shared.u16 [%r57], %r55;");
        }
        writeln_ptx!(ptx);

        // Cooperative load B[BK][BN] → smem_B
        writeln_ptx!(ptx, "    // Cooperative load B tile to shared memory");
        for elem in 0..b_elems_per_thread {
            let linear_base = elem * CTA_THREADS;

            // B is col-major K×N: B[k, n] = B + (n * K + k) * 2
            // linear = {linear_base} + tid
            // b_row = linear / BN = linear / 16
            // b_col = linear % BN = linear & 15
            // global: B + (n_block + b_col) * K * 2 + (k_base + b_row) * 2
            // smem: smem_B + linear * 2
            writeln_ptx!(ptx, "    add.u32 %r50, %r10, {linear_base};");
            writeln_ptx!(
                ptx,
                "    shr.u32 %r51, %r50, 4;          // b_row = linear / 16"
            );
            writeln_ptx!(
                ptx,
                "    and.u32 %r52, %r50, 15;         // b_col = linear & 15"
            );
            writeln_ptx!(
                ptx,
                "    add.u32 %r52, %r52, %r17;       // global_n = n_block + b_col"
            );
            writeln_ptx!(
                ptx,
                "    add.u32 %r51, %r51, {k_base};       // global_k = k_base + b_row"
            );
            writeln_ptx!(
                ptx,
                "    mul.lo.u32 %r53, %r52, {k_times_2}; // global_n * K * 2"
            );
            writeln_ptx!(ptx, "    shl.b32 %r54, %r51, 1;          // global_k * 2");
            writeln_ptx!(ptx, "    add.u32 %r53, %r53, %r54;");
            writeln_ptx!(ptx, "    cvt.u64.u32 %rd10, %r53;");
            writeln_ptx!(ptx, "    add.u64 %rd10, %rd1, %rd10;");
            writeln_ptx!(ptx, "    ld.global.u16 %r55, [%rd10];");
            writeln_ptx!(ptx, "    shl.b32 %r56, %r50, 1;");
            writeln_ptx!(ptx, "    mov.u32 %r57, smem_B;");
            writeln_ptx!(ptx, "    add.u32 %r57, %r57, %r56;");
            writeln_ptx!(ptx, "    st.shared.u16 [%r57], %r55;");
        }
        writeln_ptx!(ptx);

        // Synchronize — all threads finished writing to shared memory
        writeln_ptx!(ptx, "    bar.sync 0;");
        writeln_ptx!(ptx);

        // ldmatrix loads for this warp's MMA fragments
        // smem_A layout: row-major BM×BK (64×16), each row = 16 f16 = 32 bytes
        // Warp w reads rows [w*16 .. (w+1)*16)
        //
        // ldmatrix.sync.aligned.m8n8.x4 loads 4 matrix fragments (4 regs)
        // Each thread provides an address; the 32 threads collectively load
        // 8×8 matrix tiles. For m16n8k16, we need x4 (4 fragments):
        //   rows 0..7 from A[warp_m_off .. warp_m_off+8, k]
        //   rows 8..15 from A[warp_m_off+8 .. warp_m_off+16, k]
        //
        // Thread lane `l` provides address for row `l % 8` (within the 8-row chunk)
        // ldmatrix distributes packed f16 pairs across registers.
        //
        // A fragment address: smem_A + (warp_m_off + row) * BK * 2 + col_pair * 4
        // where row = lane % 16, col_pair = (lane / 16) * 8 → selects k-half

        let bk_stride = bk * 2; // bytes per row of smem_A
        writeln_ptx!(ptx, "    // ldmatrix: load A fragment from shared memory");
        writeln_ptx!(
            ptx,
            "    and.u32 %r58, %r41, 15;         // row_in_tile = lane & 15"
        );
        writeln_ptx!(
            ptx,
            "    shr.u32 %r59, %r41, 4;          // k_half = lane >> 4 (0 or 1)"
        );
        writeln_ptx!(
            ptx,
            "    add.u32 %r58, %r58, %r42;       // smem_row = warp_m_off + row_in_tile"
        );
        writeln_ptx!(
            ptx,
            "    mul.lo.u32 %r58, %r58, {bk_stride}; // smem_row * BK * 2"
        );
        writeln_ptx!(
            ptx,
            "    shl.b32 %r59, %r59, 4;          // k_half * 16 (byte offset for k-half)"
        );
        writeln_ptx!(ptx, "    add.u32 %r58, %r58, %r59;");
        writeln_ptx!(ptx, "    mov.u32 %r60, smem_A;");
        writeln_ptx!(
            ptx,
            "    add.u32 %r60, %r60, %r58;       // addr for ldmatrix A"
        );
        writeln_ptx!(
            ptx,
            "    ldmatrix.sync.aligned.m8n8.x4.shared.b16 {{%r{a0}, %r{a1}, %r{a2}, %r{a3}}}, [%r60];",
            a0 = 20,
            a1 = 21,
            a2 = 22,
            a3 = 23
        );
        writeln_ptx!(ptx);

        // B fragment: smem_B is row-major BK×BN (16×16)
        // For m16n8k16, B fragment needs 2 regs (ldmatrix.x2)
        // We need 2 MMA tiles along N (cols 0..8 and 8..16)
        //
        // B fragment address for N-tile `t`:
        //   smem_B + (lane % 16) * BN * 2 + t * 8 * 2
        //   where row = lane % 16, t selects the 8-column slice

        let bn_stride = BN * 2; // bytes per row of smem_B
        for tile_n in 0..2u32 {
            let n_byte_off = tile_n * 8 * 2; // 8 cols * 2 bytes per f16
            let b0 = 24 + tile_n * 2;
            let b1 = b0 + 1;

            writeln_ptx!(
                ptx,
                "    // ldmatrix: load B fragment (N-tile {tile_n}) from shared memory"
            );
            writeln_ptx!(ptx, "    and.u32 %r58, %r41, 15;");
            writeln_ptx!(ptx, "    mul.lo.u32 %r58, %r58, {bn_stride};");
            writeln_ptx!(ptx, "    add.u32 %r58, %r58, {n_byte_off};");
            writeln_ptx!(ptx, "    mov.u32 %r60, smem_B;");
            writeln_ptx!(
                ptx,
                "    add.u32 %r60, %r60, %r58;       // addr for ldmatrix B"
            );
            writeln_ptx!(
                ptx,
                "    ldmatrix.sync.aligned.m8n8.x2.trans.shared.b16 {{%r{b0}, %r{b1}}}, [%r60];"
            );
            writeln_ptx!(ptx);

            // MMA for this N-tile
            let c_base = tile_n * acc_regs;
            if f32_acc {
                writeln_ptx!(
                    ptx,
                    "    mma.sync.aligned.m16n8k16.row.col.{dst_type}.{src_type}.{src_type}.{acc_type}"
                );
                writeln_ptx!(
                    ptx,
                    "        {{%r{c0}, %r{c1}, %r{c2}, %r{c3}}}, {{%r20, %r21, %r22, %r23}}, {{%r{b0}, %r{b1}}}, {{%r{c0}, %r{c1}, %r{c2}, %r{c3}}};",
                    c0 = c_base,
                    c1 = c_base + 1,
                    c2 = c_base + 2,
                    c3 = c_base + 3
                );
            } else {
                writeln_ptx!(
                    ptx,
                    "    mma.sync.aligned.m16n8k16.row.col.{dst_type}.{src_type}.{src_type}.{acc_type}"
                );
                writeln_ptx!(
                    ptx,
                    "        {{%r{c0}, %r{c1}}}, {{%r20, %r21, %r22, %r23}}, {{%r{b0}, %r{b1}}}, {{%r{c0}, %r{c1}}};",
                    c0 = c_base,
                    c1 = c_base + 1
                );
            }
            writeln_ptx!(ptx);
        }

        // Synchronize before next K iteration overwrites smem
        if ki + 1 < k_iters {
            writeln_ptx!(ptx, "    bar.sync 0;");
            writeln_ptx!(ptx);
        }
    }

    writeln_ptx!(ptx);

    // Store C fragments (2 MMA tiles per warp along N)
    // Each warp owns rows [m_block + warp_id*16, m_block + (warp_id+1)*16)
    // and columns [n_block, n_block + 16) via 2 MMA tiles of 8 cols each
    let n_val = shape.n;
    let n_row_bytes = n_val * c_elem_bytes;

    writeln_ptx!(ptx, "    // Store C fragments (2 MMA tiles per warp)");
    writeln_ptx!(
        ptx,
        "    add.u32 %r42, %r42, %r16;       // warp_m_abs = m_block + warp_m_off"
    );
    writeln_ptx!(ptx);

    for tile_n in 0..2u32 {
        let c_base = tile_n * acc_regs;
        let n_off = tile_n * 8;

        writeln_ptx!(ptx, "    // Store C tile (N-tile {tile_n})");
        writeln_ptx!(ptx, "    shl.b32 %r28, %r14, 1;       // group_id * 2");
        writeln_ptx!(
            ptx,
            "    add.u32 %r28, %r42, %r28;    // c_row = warp_m_abs + group_id * 2"
        );
        writeln_ptx!(ptx, "    shl.b32 %r29, %r15, 1;       // tid_in_group * 2");
        writeln_ptx!(
            ptx,
            "    add.u32 %r29, %r17, %r29;    // c_col = n_block + tid_in_group * 2"
        );
        if n_off > 0 {
            writeln_ptx!(
                ptx,
                "    add.u32 %r29, %r29, {n_off};    // + N-tile offset"
            );
        }
        writeln_ptx!(ptx, "    mul.lo.u32 %r30, %r28, {n_val};");
        writeln_ptx!(ptx, "    add.u32 %r30, %r30, %r29;");
        writeln_ptx!(
            ptx,
            "    shl.b32 %r30, %r30, {shift};",
            shift = if f32_acc { 2 } else { 1 }
        );
        writeln_ptx!(ptx, "    cvt.u64.u32 %rd6, %r30;");
        writeln_ptx!(ptx, "    add.u64 %rd6, %rd2, %rd6;");
        writeln_ptx!(ptx);

        if f32_acc {
            writeln_ptx!(ptx, "    st.global.f32 [%rd6], %r{c};", c = c_base);
            writeln_ptx!(ptx, "    st.global.f32 [%rd6+4], %r{c};", c = c_base + 1);
            writeln_ptx!(
                ptx,
                "    st.global.f32 [%rd6+{n_row_bytes}], %r{c};",
                c = c_base + 2
            );
            writeln_ptx!(
                ptx,
                "    st.global.f32 [%rd6+{off}], %r{c};",
                off = n_row_bytes + 4,
                c = c_base + 3
            );
        } else {
            writeln_ptx!(ptx, "    st.global.b32 [%rd6], %r{c};", c = c_base);
            writeln_ptx!(
                ptx,
                "    st.global.b32 [%rd6+{n_row_bytes}], %r{c};",
                c = c_base + 1
            );
        }
        writeln_ptx!(ptx);
    }

    writeln_ptx!(ptx, "    ret;");
    writeln_ptx!(ptx, "}}");

    Ok((ptx, total_smem))
}

/// Phase 2 TF32 GEMM with shared memory tiling.
///
/// Block tile: BM×BN = 64×16, BK=8. Each CTA has 4 warps (128 threads).
/// Each warp computes 2 MMA tiles (m16n8k8) along N.
pub(super) fn emit_tf32_gemm_smem(shape: GemmShape, sm: u8) -> Result<(String, u32), CompileError> {
    let bk: u32 = 8;
    let k_iters = shape.k / bk;

    // Shared memory layout (f32/tf32 = 4 bytes per element)
    let smem_a_bytes = BM * bk * 4; // 64 * 8 * 4 = 2048
    let smem_b_bytes = bk * BN * 4; // 8 * 16 * 4 = 512
    let total_smem = smem_a_bytes + smem_b_bytes;

    let mut ptx = String::with_capacity(16384);

    emit_smem_header(&mut ptx, &shape, sm, "tf32", "f32", "m16n8k8", k_iters);

    writeln_ptx!(ptx, ".visible .entry gemm_kernel(");
    writeln_ptx!(ptx, "    .param .u64 param_A,");
    writeln_ptx!(ptx, "    .param .u64 param_B,");
    writeln_ptx!(ptx, "    .param .u64 param_C");
    writeln_ptx!(ptx, ")");
    writeln_ptx!(ptx, ".reqntid {CTA_THREADS}");
    writeln_ptx!(ptx, "{{");

    writeln_ptx!(ptx, "    .reg .b64 %rd<24>;");
    writeln_ptx!(ptx, "    .reg .b32 %r<64>;");
    writeln_ptx!(ptx, "    .reg .pred %p<4>;");
    writeln_ptx!(ptx);

    writeln_ptx!(ptx, "    .shared .align 16 .b8 smem_A[{smem_a_bytes}];");
    writeln_ptx!(ptx, "    .shared .align 16 .b8 smem_B[{smem_b_bytes}];");
    writeln_ptx!(ptx);

    writeln_ptx!(ptx, "    ld.param.u64 %rd0, [param_A];");
    writeln_ptx!(ptx, "    ld.param.u64 %rd1, [param_B];");
    writeln_ptx!(ptx, "    ld.param.u64 %rd2, [param_C];");
    writeln_ptx!(ptx);
    writeln_ptx!(ptx, "    mov.u32 %r10, %tid.x;");
    writeln_ptx!(ptx, "    mov.u32 %r11, %ctaid.x;");
    writeln_ptx!(ptx, "    mov.u32 %r12, %ctaid.y;");
    writeln_ptx!(ptx);

    writeln_ptx!(ptx, "    shr.u32 %r40, %r10, 5;          // warp_id");
    writeln_ptx!(ptx, "    and.u32 %r41, %r10, 31;         // lane_id");
    writeln_ptx!(ptx, "    shr.u32 %r14, %r41, 2;          // group_id");
    writeln_ptx!(ptx, "    and.u32 %r15, %r41, 3;          // tid_in_group");
    writeln_ptx!(ptx);

    let bm_val = BM;
    let bn_val = BN;
    writeln_ptx!(ptx, "    mul.lo.u32 %r16, %r12, {bm_val};");
    writeln_ptx!(ptx, "    mul.lo.u32 %r17, %r11, {bn_val};");
    writeln_ptx!(ptx);

    writeln_ptx!(
        ptx,
        "    shl.b32 %r42, %r40, 4;          // warp_m_off = warp_id * 16"
    );
    writeln_ptx!(ptx);

    // Zero 2 × 4 accumulator regs
    for tile_n in 0..2u32 {
        for r in 0..4u32 {
            writeln_ptx!(ptx, "    mov.b32 %r{}, 0;", tile_n * 4 + r);
        }
    }
    writeln_ptx!(ptx);

    let a_total_elems = BM * bk; // 64 * 8 = 512
    let b_total_elems = bk * BN; // 8 * 16 = 128
    let a_elems_per_thread = a_total_elems / CTA_THREADS; // 4
    let b_elems_per_thread = b_total_elems / CTA_THREADS; // 1
    let k_times_4 = shape.k * 4;

    writeln_ptx!(ptx, "    // === K-loop with shared memory (tf32) ===");
    for ki in 0..k_iters {
        let k_base = ki * bk;

        writeln_ptx!(ptx, "    // --- K iteration {ki} ---");
        writeln_ptx!(ptx);

        // Cooperative load A
        writeln_ptx!(ptx, "    // Cooperative load A tile (tf32)");
        for elem in 0..a_elems_per_thread {
            let linear_base = elem * CTA_THREADS;
            writeln_ptx!(ptx, "    add.u32 %r50, %r10, {linear_base};");
            writeln_ptx!(
                ptx,
                "    shr.u32 %r51, %r50, 3;          // a_row = linear / BK(8)"
            );
            writeln_ptx!(
                ptx,
                "    and.u32 %r52, %r50, 7;          // a_col = linear & 7"
            );
            writeln_ptx!(ptx, "    add.u32 %r51, %r51, %r16;");
            writeln_ptx!(ptx, "    add.u32 %r52, %r52, {k_base};");
            writeln_ptx!(ptx, "    mul.lo.u32 %r53, %r51, {k_times_4};");
            writeln_ptx!(ptx, "    shl.b32 %r54, %r52, 2;          // * 4 bytes");
            writeln_ptx!(ptx, "    add.u32 %r53, %r53, %r54;");
            writeln_ptx!(ptx, "    cvt.u64.u32 %rd10, %r53;");
            writeln_ptx!(ptx, "    add.u64 %rd10, %rd0, %rd10;");
            writeln_ptx!(ptx, "    ld.global.b32 %r55, [%rd10];");
            writeln_ptx!(
                ptx,
                "    shl.b32 %r56, %r50, 2;          // smem offset * 4"
            );
            writeln_ptx!(ptx, "    mov.u32 %r57, smem_A;");
            writeln_ptx!(ptx, "    add.u32 %r57, %r57, %r56;");
            writeln_ptx!(ptx, "    st.shared.b32 [%r57], %r55;");
        }
        writeln_ptx!(ptx);

        // Cooperative load B
        writeln_ptx!(ptx, "    // Cooperative load B tile (tf32)");
        for elem in 0..b_elems_per_thread {
            let linear_base = elem * CTA_THREADS;
            writeln_ptx!(ptx, "    add.u32 %r50, %r10, {linear_base};");
            writeln_ptx!(
                ptx,
                "    shr.u32 %r51, %r50, 4;          // b_row = linear / BN(16)"
            );
            writeln_ptx!(
                ptx,
                "    and.u32 %r52, %r50, 15;         // b_col = linear & 15"
            );
            writeln_ptx!(ptx, "    add.u32 %r52, %r52, %r17;");
            writeln_ptx!(ptx, "    add.u32 %r51, %r51, {k_base};");
            writeln_ptx!(ptx, "    mul.lo.u32 %r53, %r52, {k_times_4};");
            writeln_ptx!(ptx, "    shl.b32 %r54, %r51, 2;");
            writeln_ptx!(ptx, "    add.u32 %r53, %r53, %r54;");
            writeln_ptx!(ptx, "    cvt.u64.u32 %rd10, %r53;");
            writeln_ptx!(ptx, "    add.u64 %rd10, %rd1, %rd10;");
            writeln_ptx!(ptx, "    ld.global.b32 %r55, [%rd10];");
            writeln_ptx!(ptx, "    shl.b32 %r56, %r50, 2;");
            writeln_ptx!(ptx, "    mov.u32 %r57, smem_B;");
            writeln_ptx!(ptx, "    add.u32 %r57, %r57, %r56;");
            writeln_ptx!(ptx, "    st.shared.b32 [%r57], %r55;");
        }
        writeln_ptx!(ptx);

        writeln_ptx!(ptx, "    bar.sync 0;");
        writeln_ptx!(ptx);

        // ldmatrix A for TF32: x4, each reg = 1 f32
        let bk_stride_tf32 = bk * 4; // bytes per row in smem_A
        writeln_ptx!(ptx, "    // ldmatrix A (tf32)");
        writeln_ptx!(ptx, "    and.u32 %r58, %r41, 15;");
        writeln_ptx!(ptx, "    shr.u32 %r59, %r41, 4;");
        writeln_ptx!(ptx, "    add.u32 %r58, %r58, %r42;");
        writeln_ptx!(ptx, "    mul.lo.u32 %r58, %r58, {bk_stride_tf32};");
        writeln_ptx!(ptx, "    shl.b32 %r59, %r59, 4;");
        writeln_ptx!(ptx, "    add.u32 %r58, %r58, %r59;");
        writeln_ptx!(ptx, "    mov.u32 %r60, smem_A;");
        writeln_ptx!(ptx, "    add.u32 %r60, %r60, %r58;");
        writeln_ptx!(
            ptx,
            "    ldmatrix.sync.aligned.m8n8.x4.shared.b16 {{%r20, %r21, %r22, %r23}}, [%r60];"
        );
        writeln_ptx!(ptx);

        // ldmatrix B + MMA for each N-tile
        let bn_stride_tf32 = BN * 4;
        for tile_n in 0..2u32 {
            let n_byte_off = tile_n * 8 * 4;
            let b0 = 24 + tile_n * 2;
            let b1 = b0 + 1;

            writeln_ptx!(ptx, "    // ldmatrix B + MMA (N-tile {tile_n})");
            writeln_ptx!(ptx, "    and.u32 %r58, %r41, 15;");
            writeln_ptx!(ptx, "    mul.lo.u32 %r58, %r58, {bn_stride_tf32};");
            writeln_ptx!(ptx, "    add.u32 %r58, %r58, {n_byte_off};");
            writeln_ptx!(ptx, "    mov.u32 %r60, smem_B;");
            writeln_ptx!(ptx, "    add.u32 %r60, %r60, %r58;");
            writeln_ptx!(
                ptx,
                "    ldmatrix.sync.aligned.m8n8.x2.trans.shared.b16 {{%r{b0}, %r{b1}}}, [%r60];"
            );

            let c_base = tile_n * 4;
            writeln_ptx!(
                ptx,
                "    mma.sync.aligned.m16n8k8.row.col.f32.tf32.tf32.f32"
            );
            writeln_ptx!(
                ptx,
                "        {{%r{c0}, %r{c1}, %r{c2}, %r{c3}}}, {{%r20, %r21, %r22, %r23}}, {{%r{b0}, %r{b1}}}, {{%r{c0}, %r{c1}, %r{c2}, %r{c3}}};",
                c0 = c_base,
                c1 = c_base + 1,
                c2 = c_base + 2,
                c3 = c_base + 3
            );
            writeln_ptx!(ptx);
        }

        if ki + 1 < k_iters {
            writeln_ptx!(ptx, "    bar.sync 0;");
            writeln_ptx!(ptx);
        }
    }

    writeln_ptx!(ptx);

    // Store C (identical pattern to f16 f32-acc path)
    let n_val = shape.n;
    let n_row_bytes = n_val * 4;

    writeln_ptx!(ptx, "    // Store C fragments");
    writeln_ptx!(ptx, "    add.u32 %r42, %r42, %r16;");
    writeln_ptx!(ptx);

    for tile_n in 0..2u32 {
        let c_base = tile_n * 4;
        let n_off = tile_n * 8;

        writeln_ptx!(ptx, "    shl.b32 %r28, %r14, 1;");
        writeln_ptx!(ptx, "    add.u32 %r28, %r42, %r28;");
        writeln_ptx!(ptx, "    shl.b32 %r29, %r15, 1;");
        writeln_ptx!(ptx, "    add.u32 %r29, %r17, %r29;");
        if n_off > 0 {
            writeln_ptx!(ptx, "    add.u32 %r29, %r29, {n_off};");
        }
        writeln_ptx!(ptx, "    mul.lo.u32 %r30, %r28, {n_val};");
        writeln_ptx!(ptx, "    add.u32 %r30, %r30, %r29;");
        writeln_ptx!(ptx, "    shl.b32 %r30, %r30, 2;");
        writeln_ptx!(ptx, "    cvt.u64.u32 %rd6, %r30;");
        writeln_ptx!(ptx, "    add.u64 %rd6, %rd2, %rd6;");
        writeln_ptx!(ptx, "    st.global.f32 [%rd6], %r{c};", c = c_base);
        writeln_ptx!(ptx, "    st.global.f32 [%rd6+4], %r{c};", c = c_base + 1);
        writeln_ptx!(
            ptx,
            "    st.global.f32 [%rd6+{n_row_bytes}], %r{c};",
            c = c_base + 2
        );
        writeln_ptx!(
            ptx,
            "    st.global.f32 [%rd6+{off}], %r{c};",
            off = n_row_bytes + 4,
            c = c_base + 3
        );
        writeln_ptx!(ptx);
    }

    writeln_ptx!(ptx, "    ret;");
    writeln_ptx!(ptx, "}}");

    Ok((ptx, total_smem))
}
