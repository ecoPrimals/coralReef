// SPDX-License-Identifier: AGPL-3.0-or-later
//
// sigmoid_f64.wgsl — df64-precision Sigmoid activation with core streaming
//
// sigma(x) = 1 / (1 + exp(-x))
//
// Three-zone core streaming: f64 buffer I/O, df64 compute, f64 output.
// Uses exp_df64 for ~14-digit precision throughout.
// Numerically stable formulation: uses sign-branch to avoid overflow.
//
// Cross-spring: gating operations in folding (Evoformer pair bias),
// WDM transport (output normalization), baseCamp (agent coordination).
//
// Absorption target: barracuda::ops::sigmoid_f64
// Requires: df64_core.wgsl + df64_transcendentals.wgsl (prepended via compile_shader_f64)

struct Params {
    n: u32,
    _pad1: u32,
    _pad2: u32,
    _pad3: u32,
}

@group(0) @binding(0) var<storage, read> input: array<f64>;
@group(0) @binding(1) var<storage, read_write> output: array<f64>;
@group(0) @binding(2) var<uniform> params: Params;

@compute @workgroup_size(256)
fn sigmoid_f64(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if idx >= params.n { return; }

    // Zone 1: f64 → df64
    let x = df64_from_f64(input[idx]);
    let one = df64_from_f32(1.0);

    // Zone 2: df64 sigmoid with sign-branch stability
    var result: Df64;
    if x.hi >= 0.0 {
        let e = exp_df64(df64_neg(x));
        result = df64_div(one, df64_add(one, e));
    } else {
        let e = exp_df64(x);
        result = df64_div(e, df64_add(one, e));
    }

    // Zone 3: df64 → f64
    output[idx] = df64_to_f64(result);
}
