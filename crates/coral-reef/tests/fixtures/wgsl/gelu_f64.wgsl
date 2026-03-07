// SPDX-License-Identifier: AGPL-3.0-or-later
//
// gelu_f64.wgsl — df64-precision GELU activation with core streaming
//
// GELU(x) = 0.5 * x * (1 + tanh(sqrt(2/pi) * (x + 0.044715 * x^3)))
//
// Three-zone core streaming: f64 buffer I/O, df64 compute, f64 output.
// Uses tanh_df64 for ~14-digit precision throughout.
//
// Cross-spring: transformer building block for neuralSpring, folding
// (Evoformer FFN), baseCamp (Sub-02 attention FFN).
//
// Absorption target: barracuda::ops::gelu_f64
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
fn gelu_f64(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if idx >= params.n { return; }

    // Zone 1: f64 → df64
    let x = df64_from_f64(input[idx]);

    // Zone 2: df64 compute
    let x2 = df64_mul(x, x);
    let x3 = df64_mul(x2, x);
    let coeff = df64_from_f32(0.044715);
    let inner_sum = df64_add(x, df64_mul(coeff, x3));
    let sqrt_2_over_pi = df64_from_f32(0.7978845608);
    let inner = df64_mul(sqrt_2_over_pi, inner_sum);
    let t = tanh_df64(inner);
    let one = df64_from_f32(1.0);
    let half = df64_from_f32(0.5);
    let result = df64_mul(half, df64_mul(x, df64_add(one, t)));

    // Zone 3: df64 → f64
    output[idx] = df64_to_f64(result);
}
