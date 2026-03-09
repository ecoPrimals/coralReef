// SPDX-License-Identifier: AGPL-3.0-only
//
// ESN readout kernel (f32)
//
// output[i] = W_out[i,:] · state  (matrix-vector product)
//
// Separated from reservoir update so readout can run on CPU
// while reservoir runs on GPU/NPU. In practice, readout is
// cheap (output_size << reservoir_size) and often stays on host.
//
// Bindings:
//   0: w_out   [O*R] f32, read  — readout weights (row-major, O × R)
//   1: state   [R]   f32, read  — reservoir state
//   2: output  [O]   f32, rw    — prediction (written)
//   3: params  [4]   f32, read  — [reservoir_size, output_size, _, _]

@group(0) @binding(0) var<storage, read> w_out: array<f32>;
@group(0) @binding(1) var<storage, read> state: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;
@group(0) @binding(3) var<storage, read> params: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let R = u32(params[0]);
    let O = u32(params[1]);

    if (i >= O) { return; }

    var sum: f32 = 0.0;
    let row = i * R;
    for (var j: u32 = 0u; j < R; j = j + 1u) {
        sum += w_out[row + j] * state[j];
    }
    output[i] = sum;
}
