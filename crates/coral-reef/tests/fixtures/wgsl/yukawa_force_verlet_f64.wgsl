// SPDX-License-Identifier: AGPL-3.0-only
//
// Yukawa Force — Verlet Neighbor List (f64)
//
// Iterates only the compact per-particle neighbor list instead of all
// particles (AllPairs) or the full 27-cell stencil (CellList). This is
// the fastest force kernel when the neighbor list is already built.
//
// Bindings:
//   0: positions       [N*3]                f64, read  — particle positions
//   1: forces          [N*3]                f64, write — per-particle forces
//   2: pe_buf          [N]                  f64, write — per-particle PE (half-counted)
//   3: params          [8]                  f64, read  — force parameters
//   4: neighbor_list   [N * max_neighbors]  u32, read  — flat neighbor indices
//   5: neighbor_count  [N]                  u32, read  — count per particle

@group(0) @binding(0) var<storage, read> positions: array<f64>;
@group(0) @binding(1) var<storage, read_write> forces: array<f64>;
@group(0) @binding(2) var<storage, read_write> pe_buf: array<f64>;
@group(0) @binding(3) var<storage, read> params: array<f64>;
@group(0) @binding(4) var<storage, read> neighbor_list: array<u32>;
@group(0) @binding(5) var<storage, read> neighbor_count: array<u32>;

// params layout:
//   [0] = n_particles
//   [1] = kappa
//   [2] = prefactor
//   [3] = cutoff_sq (rc², NOT (rc+skin)²)
//   [4] = box_x
//   [5] = box_y
//   [6] = box_z
//   [7] = max_neighbors (as f64, cast to u32)

fn pbc_delta_vf(delta: f64, box_size: f64) -> f64 {
    return delta - box_size * round(delta / box_size);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let n = u32(params[0]);
    if (i >= n) { return; }

    let kappa     = params[1];
    let prefactor = params[2];
    let cutoff_sq = params[3];
    let box_x     = params[4];
    let box_y     = params[5];
    let box_z     = params[6];
    let max_nb    = u32(params[7]);

    let xi = positions[i * 3u];
    let yi = positions[i * 3u + 1u];
    let zi = positions[i * 3u + 2u];

    var fx = xi - xi;
    var fy = xi - xi;
    var fz = xi - xi;
    var pe = xi - xi;

    let nb_count = neighbor_count[i];
    let base = i * max_nb;

    for (var jj = 0u; jj < nb_count; jj = jj + 1u) {
        let j = neighbor_list[base + jj];

        let xj = positions[j * 3u];
        let yj = positions[j * 3u + 1u];
        let zj = positions[j * 3u + 2u];

        let dx = pbc_delta_vf(xj - xi, box_x);
        let dy = pbc_delta_vf(yj - yi, box_y);
        let dz = pbc_delta_vf(zj - zi, box_z);

        let r_sq = dx * dx + dy * dy + dz * dz;
        if (r_sq > cutoff_sq) { continue; }

        let r = sqrt(r_sq);
        let screening = exp(-kappa * r);
        let force_mag = prefactor * screening * (1.0 + kappa * r) / r_sq;
        let inv_r = 1.0 / r;

        fx = fx - force_mag * dx * inv_r;
        fy = fy - force_mag * dy * inv_r;
        fz = fz - force_mag * dz * inv_r;
        pe = pe + 0.5 * prefactor * screening * inv_r;
    }

    forces[i * 3u]      = fx;
    forces[i * 3u + 1u] = fy;
    forces[i * 3u + 2u] = fz;
    pe_buf[i] = pe;
}
