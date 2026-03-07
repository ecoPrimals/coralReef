// SPDX-License-Identifier: AGPL-3.0-only
// Berendsen velocity rescaling (f64)
//
// Bindings:
//   0: velocities [N*3] f64, read-write
//   1: params     [4]   f64, read  — [n, scale_factor, _, _]

@group(0) @binding(0) var<storage, read_write> velocities: array<f64>;
@group(0) @binding(1) var<storage, read> params: array<f64>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let n = u32(params[0]);
    if (i >= n) { return; }

    let scale = params[1];

    velocities[i * 3u]      = velocities[i * 3u]      * scale;
    velocities[i * 3u + 1u] = velocities[i * 3u + 1u] * scale;
    velocities[i * 3u + 2u] = velocities[i * 3u + 2u] * scale;
}
