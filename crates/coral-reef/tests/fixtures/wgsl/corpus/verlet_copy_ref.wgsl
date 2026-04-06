// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Copy current positions to reference buffer for Verlet displacement tracking.
//
// Bindings:
//   0: positions       [N*3] f64, read   — current positions
//   1: ref_positions   [N*3] f64, write  — reference positions (output)
//   2: params          [4]   f64, read   — [n, ...]

@group(0) @binding(0) var<storage, read> positions: array<f64>;
@group(0) @binding(1) var<storage, read_write> ref_positions: array<f64>;
@group(0) @binding(2) var<storage, read> params: array<f64>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let n = u32(params[0]);
    if (i >= n) { return; }

    ref_positions[i * 3u]      = positions[i * 3u];
    ref_positions[i * 3u + 1u] = positions[i * 3u + 1u];
    ref_positions[i * 3u + 2u] = positions[i * 3u + 2u];
}
