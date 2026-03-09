// SPDX-License-Identifier: AGPL-3.0-or-later
//
// triangle_mul_outgoing_f64.wgsl — Triangle multiplicative update (outgoing edges)
//
// From Jumper et al. 2021 (AlphaFold2), Algorithm 11.
// Updates pair representation z[i,j] using outgoing edges:
//   z[i,j] += sum_k (a[i,k] * b[j,k])
// where a = sigmoid(gate_a) * linear_a(z), b = sigmoid(gate_b) * linear_b(z).
//
// Three-zone core streaming: f64 buffer I/O, df64 compute, f64 output.
// df64 accumulation over k prevents drift at large N (>256 residues).
//
// Absorption target: barracuda::ops::triangle_mul_outgoing_f64
// Requires: df64_core.wgsl + df64_transcendentals.wgsl (prepended via compile_shader_f64)

struct Params {
    n_res:    u32,
    channels: u32,
    _pad0:    u32,
    _pad1:    u32,
}

@group(0) @binding(0) var<storage, read>       proj_a: array<f64>;
@group(0) @binding(1) var<storage, read>       proj_b: array<f64>;
@group(0) @binding(2) var<storage, read_write> output: array<f64>;
@group(0) @binding(3) var<uniform>             params: Params;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let N = params.n_res;
    let C = params.channels;
    let total = N * N * C;
    let idx = gid.x;
    if idx >= total { return; }

    let ij = idx / C;
    let c  = idx % C;
    let i  = ij / N;
    let j  = ij % N;

    // Zone 1+2: f64 → df64 contraction
    var acc = df64_zero();
    for (var k = 0u; k < N; k++) {
        let a_val = df64_from_f64(proj_a[(i * N + k) * C + c]);
        let b_val = df64_from_f64(proj_b[(j * N + k) * C + c]);
        acc = df64_add(acc, df64_mul(a_val, b_val));
    }

    // Zone 3: df64 → f64
    output[(i * N + j) * C + c] = df64_to_f64(acc);
}
