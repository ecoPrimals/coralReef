// SPDX-License-Identifier: AGPL-3.0-only
// Gradient flow K-buffer accumulation: K[i] = alpha * K[i] + Z[i]
// Used in LSCFRK 2N-storage Lie group integrators (Bazavov & Chuna 2021).
// Each K/Z element is an SU(3) algebra element: 18 f64 (3x3 complex).

struct FlowAccumParams {
    n_links: u32,
    pad0: u32,
    alpha: f64,
}

@group(0) @binding(0) var<uniform> params: FlowAccumParams;
@group(0) @binding(1) var<storage, read> force: array<f64>;
@group(0) @binding(2) var<storage, read_write> k_buf: array<f64>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
    let idx = gid.x + gid.y * nwg.x * 64u;
    let link_idx = idx;
    if link_idx >= params.n_links { return; }

    let base = link_idx * 18u;
    let a = params.alpha;

    for (var i = 0u; i < 18u; i++) {
        k_buf[base + i] = a * k_buf[base + i] + force[base + i];
    }
}
