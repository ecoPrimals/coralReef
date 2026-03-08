// SPDX-License-Identifier: AGPL-3.0-or-later
//
// swarm_nn_forward.wgsl — Batch neural-net controller forward pass (Paper 015)
//
// Evaluates many controllers × sense inputs: 1 input → 4 hidden sigmoid → 5
// output sigmoid → argmax. Each thread handles one (controller, sense_input) pair.
//
// Absorption target: barracuda::ops::batch_gemm + elementwise sigmoid + argmax

fn sigmoid(x: f32) -> f32 {
    return 1.0 / (1.0 + exp(-x));
}

struct Config {
    n_controllers: u32,
    n_evals: u32,
}

@group(0) @binding(0) var<storage, read> params: array<f32>;
@group(0) @binding(1) var<storage, read> inputs: array<f32>;
@group(0) @binding(2) var<storage, read_write> actions: array<u32>;
@group(0) @binding(3) var<uniform> config: Config;

@compute @workgroup_size(256)
fn swarm_nn_forward(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let n_controllers = config.n_controllers;
    let n_evals = config.n_evals;
    if idx >= n_controllers * n_evals {
        return;
    }

    let ctrl = idx / n_evals;
    let eval_idx = idx % n_evals;

    let sense = inputs[eval_idx];
    let base = ctrl * 33u;

    // Hidden layer: h[i] = sigmoid(sense * W1[i] + b1[i]) for i in 0..4
    var h: array<f32, 4>;
    for (var i: u32 = 0u; i < 4u; i = i + 1u) {
        let w = params[base + i];
        let b = params[base + 4u + i];
        h[i] = sigmoid(sense * w + b);
    }

    // Output layer: o[j] = sigmoid(sum(h[i] * W2[i][j]) + b2[j]) for j in 0..5
    var o: array<f32, 5>;
    for (var j: u32 = 0u; j < 5u; j = j + 1u) {
        var sum: f32 = params[base + 28u + j];
        for (var i: u32 = 0u; i < 4u; i = i + 1u) {
            sum = sum + h[i] * params[base + 8u + i * 5u + j];
        }
        o[j] = sigmoid(sum);
    }

    // argmax(o[0..5])
    var best: u32 = 0u;
    var best_val: f32 = o[0];
    for (var j: u32 = 1u; j < 5u; j = j + 1u) {
        if o[j] > best_val {
            best_val = o[j];
            best = j;
        }
    }

    actions[ctrl * n_evals + eval_idx] = best;
}
