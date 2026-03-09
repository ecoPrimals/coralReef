// SPDX-License-Identifier: AGPL-3.0-only
//
// ESN reservoir update kernel (f32)
//
// Fused W_in*input + W_res*state → leaky tanh → new state
// Single dispatch replaces two matmul + element-wise ops.
//
// This is the shader-originating math for reservoir computing.
// Same kernel runs on GPU (via ToadStool dispatch) and the
// equivalent math runs on Akida NPU (via load_reservoir + infer).
// CPU reference: barracuda/src/md/reservoir.rs::EchoStateNetwork::update()
//
// Bindings:
//   0: w_in    [R*I] f32, read  — input weights (row-major, R × I)
//   1: w_res   [R*R] f32, read  — reservoir weights (row-major, R × R)
//   2: input   [I]   f32, read  — current input vector
//   3: state   [R]   f32, rw    — reservoir state (updated in-place)
//   4: params  [4]   f32, read  — [reservoir_size, input_size, leak_rate, _]

@group(0) @binding(0) var<storage, read> w_in: array<f32>;
@group(0) @binding(1) var<storage, read> w_res: array<f32>;
@group(0) @binding(2) var<storage, read> input: array<f32>;
@group(0) @binding(3) var<storage, read_write> state: array<f32>;
@group(0) @binding(4) var<storage, read> params: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let R = u32(params[0]);
    let I = u32(params[1]);
    let alpha = params[2];

    if (i >= R) { return; }

    // pre = W_in[i,:] · input + W_res[i,:] · state
    var pre: f32 = 0.0;

    let w_in_row = i * I;
    for (var j: u32 = 0u; j < I; j = j + 1u) {
        pre += w_in[w_in_row + j] * input[j];
    }

    let w_res_row = i * R;
    for (var j: u32 = 0u; j < R; j = j + 1u) {
        pre += w_res[w_res_row + j] * state[j];
    }

    // Leaky integration: state[i] = (1 - alpha) * state[i] + alpha * tanh(pre)
    state[i] = (1.0 - alpha) * state[i] + alpha * tanh(pre);
}
