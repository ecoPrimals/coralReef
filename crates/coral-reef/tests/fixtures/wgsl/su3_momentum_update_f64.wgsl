// SPDX-License-Identifier: AGPL-3.0-only
// Momentum update: P[i] += dt * F[i] for SU(3) algebra elements.
// Each momentum/force is 18 f64 (3x3 complex, row-major).
// Prepend: (none required — plain f64 arithmetic)

struct MomParams {
    n_links: u32,
    pad0: u32,
    dt: f64,
}

@group(0) @binding(0) var<uniform> params: MomParams;
@group(0) @binding(1) var<storage, read> force: array<f64>;
@group(0) @binding(2) var<storage, read_write> momenta: array<f64>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
    let idx = gid.x + gid.y * nwg.x * 64u;
    let link_idx = idx;
    if link_idx >= params.n_links { return; }

    let base = link_idx * 18u;
    let dt = params.dt;

    for (var i = 0u; i < 18u; i++) {
        momenta[base + i] = momenta[base + i] + dt * force[base + i];
    }
}
