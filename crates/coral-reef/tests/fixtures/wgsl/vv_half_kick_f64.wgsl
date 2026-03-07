// SPDX-License-Identifier: AGPL-3.0-only
// VV second half-kick (f64)
//
// Bindings:
//   0: velocities   [N*3] f64, read-write  — updated in-place
//   1: forces       [N*3] f64, read        — NEW forces after drift
//   2: params       [4]   f64, read        — [n, dt, mass, _]

@group(0) @binding(0) var<storage, read_write> velocities: array<f64>;
@group(0) @binding(1) var<storage, read> forces: array<f64>;
@group(0) @binding(2) var<storage, read> params: array<f64>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let n = u32(params[0]);
    if (i >= n) { return; }

    let dt   = params[1];
    let mass = params[2];

    let inv_m   = 1.0 / mass;
    let half_dt = 0.5 * dt;

    velocities[i * 3u]      = velocities[i * 3u]      + half_dt * forces[i * 3u]      * inv_m;
    velocities[i * 3u + 1u] = velocities[i * 3u + 1u] + half_dt * forces[i * 3u + 1u] * inv_m;
    velocities[i * 3u + 2u] = velocities[i * 3u + 2u] + half_dt * forces[i * 3u + 2u] * inv_m;
}
