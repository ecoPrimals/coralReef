// SPDX-License-Identifier: AGPL-3.0-or-later
//
// outer_product_mean_f64.wgsl — MSA → pair representation via outer product mean
//
// For each residue pair (i, j), averages the outer product of projected
// MSA representations over all sequences:
//   output[i, j, ca*c_b + cb] = mean_s(a[s, i, ca] * b[s, j, cb])
//
// This converts evolutionary covariance (MSA) into structural contact
// information (pair representation) — the key bridge in AlphaFold2's
// Evoformer.
//
// Three-zone core streaming: f64 buffer I/O, df64 compute, f64 output.
//
// Absorption target: barracuda::ops::outer_product_mean_f64
// Requires: df64_core.wgsl + df64_transcendentals.wgsl (prepended via compile_shader_f64)

struct Params {
    n_seq: u32,
    n_res: u32,
    c_a:   u32,
    c_b:   u32,
}

@group(0) @binding(0) var<storage, read>       a:      array<f64>;
@group(0) @binding(1) var<storage, read>       b:      array<f64>;
@group(0) @binding(2) var<storage, read_write> output: array<f64>;
@group(0) @binding(3) var<uniform>             params: Params;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let c_out = params.c_a * params.c_b;
    let total = params.n_res * params.n_res * c_out;
    let idx = gid.x;
    if idx >= total { return; }

    let ij_flat = idx / c_out;
    let c_idx   = idx % c_out;
    let i  = ij_flat / params.n_res;
    let j  = ij_flat % params.n_res;
    let ca = c_idx / params.c_b;
    let cb = c_idx % params.c_b;

    let N  = params.n_res;
    let Ca = params.c_a;
    let Cb = params.c_b;

    // Zone 1+2: f64 → df64 outer product + mean
    var acc = df64_zero();
    for (var s = 0u; s < params.n_seq; s++) {
        let a_val = df64_from_f64(a[(s * N + i) * Ca + ca]);
        let b_val = df64_from_f64(b[(s * N + j) * Cb + cb]);
        acc = df64_add(acc, df64_mul(a_val, b_val));
    }

    let inv_n = df64_div(df64_from_f32(1.0), df64_from_f32(f32(params.n_seq)));
    let mean = df64_mul(acc, inv_n);

    // Zone 3: df64 → f64
    output[idx] = df64_to_f64(mean);
}
