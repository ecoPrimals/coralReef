// SPDX-License-Identifier: AGPL-3.0-or-later
//
// HMM Viterbi decoding in log-domain. Dispatched per-timestep from host.
// Each thread computes the best predecessor for one hidden state j.
//
// delta[t][j] = max_i(delta[t-1][i] + log_A[i,j]) + log_B[j, obs_t]
// psi[t][j]   = argmax_i(delta[t-1][i] + log_A[i,j])
//
// Absorption target: barracuda::ops::bio::hmm or StatefulPipeline.
//
// Binding layout:
//   0: storage, read — log_a: N*N log-transition matrix (flat row-major)
//   1: storage, read — log_b_col: N log-emission values for obs[t]
//   2: storage, read — delta_prev: N values from timestep t-1
//   3: storage, read_write — delta_cur: N values for timestep t
//   4: storage, read_write — psi_cur: N backpointer indices for timestep t
//   5: uniform — params { n_states }

struct Params {
    n_states: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var<storage, read> log_a: array<f32>;
@group(0) @binding(1) var<storage, read> log_b_col: array<f32>;
@group(0) @binding(2) var<storage, read> delta_prev: array<f32>;
@group(0) @binding(3) var<storage, read_write> delta_cur: array<f32>;
@group(0) @binding(4) var<storage, read_write> psi_cur: array<u32>;
@group(0) @binding(5) var<uniform> params: Params;

@compute @workgroup_size(256)
fn hmm_viterbi(@builtin(global_invocation_id) gid: vec3<u32>) {
    let j = gid.x;
    let n = params.n_states;
    if (j >= n) { return; }

    var best_val = -1e30;
    var best_i = 0u;

    for (var i = 0u; i < n; i++) {
        let val = delta_prev[i] + log_a[i * n + j];
        if (val > best_val) {
            best_val = val;
            best_i = i;
        }
    }

    delta_cur[j] = best_val + log_b_col[j];
    psi_cur[j] = best_i;
}
