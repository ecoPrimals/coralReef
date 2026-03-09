// SPDX-License-Identifier: AGPL-3.0-only
// Per-particle kinetic energy (f64)
//
// Bindings:
//   0: velocities [N*3] f64, read
//   1: ke_buf     [N]   f64, write
//   2: params     [4]   f64, read  — [n, mass, _, _]

@group(0) @binding(0) var<storage, read> velocities: array<f64>;
@group(0) @binding(1) var<storage, read_write> ke_buf: array<f64>;
@group(0) @binding(2) var<storage, read> params: array<f64>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let n = u32(params[0]);
    if (i >= n) { return; }

    let mass = params[1];
    let vx = velocities[i * 3u];
    let vy = velocities[i * 3u + 1u];
    let vz = velocities[i * 3u + 2u];

    ke_buf[i] = 0.5 * mass * (vx * vx + vy * vy + vz * vz);
}
