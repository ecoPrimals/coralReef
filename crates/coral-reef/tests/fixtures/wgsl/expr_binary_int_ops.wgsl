// SPDX-License-Identifier: AGPL-3.0-only
// expr_binary: integer arithmetic +, -, *, /, % on i32 and u32
// Bitwise: &, |, ^, <<, >>
// Comparisons: <, <=, >, >=, ==, != for int and float

@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> input_u: array<u32>;
@group(0) @binding(2) var<storage, read> input_i: array<i32>;
@group(0) @binding(3) var<storage, read> input_f: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let ua = input_u[idx % 64u];
    let ub = select(1u, input_u[(idx + 1u) % 64u], input_u[(idx + 1u) % 64u] != 0u);
    let ia = input_i[idx % 64u];
    let ib = select(1, input_i[(idx + 1u) % 64u], input_i[(idx + 1u) % 64u] != 0);
    let fa = input_f[idx % 64u];
    let fb = input_f[(idx + 1u) % 64u];

    // u32 arithmetic
    let u_add = ua + ub;
    let u_sub = ua - ub;
    let u_mul = ua * ub;
    let u_div = ua / ub;
    let u_mod = ua % ub;

    // i32 arithmetic
    let i_add = ia + ib;
    let i_sub = ia - ib;
    let i_mul = ia * ib;
    let i_div = ia / ib;
    let i_mod = ia % ib;

    // Bitwise u32
    let bw_and = ua & ub;
    let bw_or = ua | ub;
    let bw_xor = ua ^ ub;
    let bw_shl = ua << (ub & 31u);
    let bw_shr = ua >> (ub & 31u);

    // Float comparisons (use in select to avoid complex CFG)
    let fc_lt = select(0.0, 1.0, fa < fb);
    let fc_le = select(0.0, 1.0, fa <= fb);
    let fc_gt = select(0.0, 1.0, fa > fb);
    let fc_ge = select(0.0, 1.0, fa >= fb);
    let fc_eq = select(0.0, 1.0, fa == fb);
    let fc_ne = select(0.0, 1.0, fa != fb);

    // Int comparisons
    let ic_lt = select(0.0, 1.0, ia < ib);
    let ic_le = select(0.0, 1.0, ia <= ib);
    let ic_gt = select(0.0, 1.0, ia > ib);
    let ic_ge = select(0.0, 1.0, ia >= ib);
    let ic_eq = select(0.0, 1.0, ia == ib);
    let ic_ne = select(0.0, 1.0, ia != ib);

    var result: f32 = 0.0;
    result = result + f32(u_add + u_sub + u_mul + u_div + u_mod);
    result = result + f32(i_add) + f32(i_sub) + f32(i_mul) + f32(i_div) + f32(i_mod);
    result = result + f32(bw_and + bw_or + bw_xor + bw_shl + bw_shr);
    result = result + fc_lt * 0.1 + fc_le * 0.2 + fc_gt * 0.3 + fc_ge * 0.4 + fc_eq * 0.5 + fc_ne * 0.6;
    result = result + ic_lt * 0.01 + ic_le * 0.02 + ic_gt * 0.03 + ic_ge * 0.04 + ic_eq * 0.05 + ic_ne * 0.06;

    out[idx] = result;
}
