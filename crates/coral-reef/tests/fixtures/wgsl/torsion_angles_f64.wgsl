// SPDX-License-Identifier: AGPL-3.0-or-later
//
// torsion_angles_f64.wgsl — Torsion angle prediction (Structure Module)
//
// Predicts side-chain torsion angles from single representation via:
//   Linear → ResNet → ResNet → Linear → unit circle normalization
//
// Each output pair (sin, cos) is normalized to ||·|| = 1, representing
// an angle on the unit circle. 7 angles per residue: phi, psi, omega,
// and 4 chi angles.
//
// One thread per residue. Three-zone core streaming: f64 buffer I/O,
// df64 compute within layers, f64 output. Private working memory uses
// f32 (df64 hi component) for inter-layer activations.
//
// Layouts:
//   single:  [N, C_s] — input single representation
//   weights: flat concatenation of all weight matrices and biases
//   output:  [N, 14] — 7 × (sin, cos) pairs
//
// Absorption target: barracuda::ops::torsion_angles_f64
// Requires: df64_core.wgsl + df64_transcendentals.wgsl (prepended via compile_shader_f64)

struct Params {
    n_res:     u32,
    c_single:  u32,
    c_hidden:  u32,
    _pad:      u32,
}

@group(0) @binding(0) var<storage, read>       single:  array<f64>;
@group(0) @binding(1) var<storage, read>       weights: array<f64>;
@group(0) @binding(2) var<storage, read_write> output:  array<f64>;
@group(0) @binding(3) var<uniform>             params:  Params;

var<private> h: array<f32, 64>;
var<private> h_skip: array<f32, 64>;
var<private> h_tmp: array<f32, 64>;

fn linear_fwd(
    input_ptr: u32, input_dim: u32,
    w_off: u32, b_off: u32, out_dim: u32,
    use_h_as_input: bool,
) -> u32 {
    for (var o = 0u; o < out_dim; o++) {
        var acc = df64_zero();
        for (var i = 0u; i < input_dim; i++) {
            var x_df: Df64;
            if use_h_as_input {
                x_df = df64_from_f32(h[i]);
            } else {
                x_df = df64_from_f64(single[input_ptr + i]);
            }
            let w_df = df64_from_f64(weights[w_off + i * out_dim + o]);
            acc = df64_add(acc, df64_mul(x_df, w_df));
        }
        let bias_df = df64_from_f64(weights[b_off + o]);
        let result = df64_add(acc, bias_df);
        h_tmp[o] = result.hi;
    }
    for (var o = 0u; o < out_dim; o++) {
        h[o] = h_tmp[o];
    }
    return b_off + out_dim;
}

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let res_idx = gid.x;
    if res_idx >= params.n_res { return; }

    let cs = params.c_single;
    let ch = params.c_hidden;
    let hh = ch * ch;
    let input_base = res_idx * cs;

    var w_off = 0u;

    // 1. proj_in: [C_s, C_h], bias [C_h] (Zone 1: f64 input)
    let pi_w = w_off; w_off += cs * ch;
    let pi_b = w_off; w_off += ch;
    linear_fwd(input_base, cs, pi_w, pi_b, ch, false);

    // 2. ResNet block 1
    for (var o = 0u; o < ch; o++) { h_skip[o] = h[o]; }
    let r1w1 = w_off; w_off += hh;
    let r1b1 = w_off; w_off += ch;
    linear_fwd(0u, ch, r1w1, r1b1, ch, true);
    for (var o = 0u; o < ch; o++) { h[o] = max(h[o], 0.0); }
    let r1w2 = w_off; w_off += hh;
    let r1b2 = w_off; w_off += ch;
    linear_fwd(0u, ch, r1w2, r1b2, ch, true);
    for (var o = 0u; o < ch; o++) { h[o] = h[o] + h_skip[o]; }

    // 3. ResNet block 2
    for (var o = 0u; o < ch; o++) { h_skip[o] = h[o]; }
    let r2w1 = w_off; w_off += hh;
    let r2b1 = w_off; w_off += ch;
    linear_fwd(0u, ch, r2w1, r2b1, ch, true);
    for (var o = 0u; o < ch; o++) { h[o] = max(h[o], 0.0); }
    let r2w2 = w_off; w_off += hh;
    let r2b2 = w_off; w_off += ch;
    linear_fwd(0u, ch, r2w2, r2b2, ch, true);
    for (var o = 0u; o < ch; o++) { h[o] = h[o] + h_skip[o]; }

    // 4. proj_out: [C_h, 14], bias [14]
    let po_w = w_off; w_off += ch * 14u;
    let po_b = w_off;
    for (var o = 0u; o < 14u; o++) {
        var acc = df64_zero();
        for (var i = 0u; i < ch; i++) {
            let x_df = df64_from_f32(h[i]);
            let w_df = df64_from_f64(weights[po_w + i * 14u + o]);
            acc = df64_add(acc, df64_mul(x_df, w_df));
        }
        let bias_df = df64_from_f64(weights[po_b + o]);
        h_tmp[o] = df64_add(acc, bias_df).hi;
    }

    // 5. Unit circle normalization with sqrt_df64 (Zone 3: → f64)
    let out_base = res_idx * 14u;
    for (var a = 0u; a < 7u; a++) {
        let s = df64_from_f32(h_tmp[a * 2u]);
        let c = df64_from_f32(h_tmp[a * 2u + 1u]);
        let norm_sq = df64_add(df64_mul(s, s), df64_mul(c, c));
        let norm = sqrt_df64(norm_sq);
        let eps = df64_from_f32(1e-12);
        var safe_norm = eps;
        if df64_gt(norm, eps) { safe_norm = norm; }
        let inv_norm = df64_div(df64_from_f32(1.0), safe_norm);
        output[out_base + a * 2u] = df64_to_f64(df64_mul(s, inv_norm));
        output[out_base + a * 2u + 1u] = df64_to_f64(df64_mul(c, inv_norm));
    }
}
