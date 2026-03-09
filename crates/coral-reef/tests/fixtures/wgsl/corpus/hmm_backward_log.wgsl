// SPDX-License-Identifier: AGPL-3.0-or-later
//
// HMM backward pass in log-domain. Dispatched per-timestep from host.
// Each thread computes log_beta[i] for one hidden state i at timestep t.
//
// log_beta[t][i] = logsumexp_j(log_A[i,j] + log_B[j, obs_{t+1}] + log_beta[t+1][j])
//
// Absorption target: barracuda::ops::bio::hmm or StatefulPipeline.
//
// Binding layout:
//   0: storage, read — log_a: N*N log-transition matrix (flat row-major)
//   1: storage, read — log_b_col: N log-emission values for obs[t+1]
//   2: storage, read — log_beta_next: N values from timestep t+1
//   3: storage, read_write — log_beta_cur: N values for timestep t
//   4: uniform — params { n_states }

struct Params {
    n_states: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var<storage, read> log_a: array<f32>;
@group(0) @binding(1) var<storage, read> log_b_col: array<f32>;
@group(0) @binding(2) var<storage, read> log_beta_next: array<f32>;
@group(0) @binding(3) var<storage, read_write> log_beta_cur: array<f32>;
@group(0) @binding(4) var<uniform> params: Params;

@compute @workgroup_size(256)
fn hmm_backward_log(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let n = params.n_states;
    if (i >= n) { return; }

    // logsumexp over j: log_a[i,j] + log_b[j, obs_{t+1}] + log_beta_{t+1}[j]
    var max_val = -1e30;
    for (var j = 0u; j < n; j++) {
        let val = log_a[i * n + j] + log_b_col[j] + log_beta_next[j];
        max_val = max(max_val, val);
    }

    var sum_exp = 0.0;
    for (var j = 0u; j < n; j++) {
        let val = log_a[i * n + j] + log_b_col[j] + log_beta_next[j];
        sum_exp += exp(val - max_val);
    }

    log_beta_cur[i] = max_val + log(sum_exp);
}
