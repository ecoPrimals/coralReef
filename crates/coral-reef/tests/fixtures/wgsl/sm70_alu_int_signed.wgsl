// SPDX-License-Identifier: AGPL-3.0-only
// sm70_encode/alu/int: signed integer ops, IMAD, IADD3, shifts
// Exercises: signed multiply-add, overflow handling, shift encoding

@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> input_i: array<i32>;
@group(0) @binding(2) var<storage, read> input_u: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let ia = input_i[idx];
    let ib = input_i[(idx + 1u) % 64u];
    let ic = input_i[(idx + 2u) % 64u];
    let ua = input_u[idx];
    let ub = input_u[(idx + 1u) % 64u];

    let s_add = ia + ib;
    let s_sub = ia - ib;
    let s_mul = ia * ib;
    let s_mad = ia * ib + ic;

    let shift_amt = ub & 31u;
    let shl = ia << shift_amt;
    let shr_u = ua >> shift_amt;
    let shr_s = ia >> shift_amt;

    let bw_and = ia & ib;
    let bw_or = ia | ib;
    let bw_xor = ia ^ ib;

    let cmp_lt = ia < ib;
    let cmp_le = ia <= ib;
    let cmp_gt = ia > ib;
    let cmp_ge = ia >= ib;

    var r: f32 = f32(s_add + s_sub);
    r = r + f32(s_mul) * 0.001;
    r = r + f32(s_mad) * 0.0001;
    r = r + f32(shl) * 0.00001;
    r = r + f32(shr_u) * 0.000001;
    r = r + f32(shr_s) * 0.0000001;
    r = r + f32(bw_and + bw_or + bw_xor) * 0.00000001;
    if cmp_lt { r = r + 1.0; }
    if cmp_le { r = r + 2.0; }
    if cmp_gt { r = r + 3.0; }
    if cmp_ge { r = r + 4.0; }

    out[idx] = r;
}
