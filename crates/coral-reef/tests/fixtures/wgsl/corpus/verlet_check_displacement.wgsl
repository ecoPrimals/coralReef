// SPDX-License-Identifier: AGPL-3.0-only
//
// Verlet Displacement Check — atomic max of |pos - ref_pos|
//
// Computes the maximum displacement of any particle from its reference
// position (stored at last Verlet list build). Uses fixed-point u32
// representation (value × 1e6) with atomicMax for GPU-wide reduction.
//
// If the result > skin/2 × 1e6, the Verlet list needs rebuilding.
//
// Bindings:
//   0: positions       [N*3] f64, read  — current positions
//   1: ref_positions   [N*3] f64, read  — reference positions from last build
//   2: max_disp        [1]   atomic<u32>, read_write — global max (fixed-point)
//   3: params          [4]   f64, read  — [n, box_x, box_y, box_z]

@group(0) @binding(0) var<storage, read> positions: array<f64>;
@group(0) @binding(1) var<storage, read> ref_positions: array<f64>;
@group(0) @binding(2) var<storage, read_write> max_disp: array<atomic<u32>>;
@group(0) @binding(3) var<storage, read> params: array<f64>;

fn pbc_delta_vc(delta: f64, box_size: f64) -> f64 {
    return delta - box_size * round(delta / box_size);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let n = u32(params[0]);
    if (i >= n) { return; }

    let box_x = params[1];
    let box_y = params[2];
    let box_z = params[3];

    let dx = pbc_delta_vc(positions[i * 3u]      - ref_positions[i * 3u],      box_x);
    let dy = pbc_delta_vc(positions[i * 3u + 1u]  - ref_positions[i * 3u + 1u],  box_y);
    let dz = pbc_delta_vc(positions[i * 3u + 2u]  - ref_positions[i * 3u + 2u],  box_z);

    let disp_sq = dx * dx + dy * dy + dz * dz;
    let disp = sqrt(disp_sq);

    let fixed = u32(disp * 1000000.0);
    atomicMax(&max_disp[0], fixed);
}
