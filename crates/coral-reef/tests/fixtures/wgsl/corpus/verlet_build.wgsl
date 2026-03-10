// SPDX-License-Identifier: AGPL-3.0-only
//
// Verlet Neighbor List Build — populates compact per-particle neighbor arrays
//
// Iterates 27 neighboring cells via CellListGpu, storing all neighbors within
// (rc + skin)² into a flat [N × max_neighbors] array. Symmetric pairs are
// NOT deduplicated — each particle stores its own complete neighbor set.
//
// Bindings:
//   0: positions       [N*3]                     f64, read  — current positions
//   1: neighbor_list   [N * max_neighbors]       u32, write — flat neighbor indices
//   2: neighbor_count  [N]                       u32, write — count per particle
//   3: params          [16]                      f64, read  — simulation parameters
//   4: cell_start      [n_cells_total]           u32, read  — cell offsets
//   5: cell_count      [n_cells_total]           u32, read  — particles per cell
//   6: sorted_indices  [N]                       u32, read  — sorted slot → original index

@group(0) @binding(0) var<storage, read> positions: array<f64>;
@group(0) @binding(1) var<storage, read_write> neighbor_list: array<u32>;
@group(0) @binding(2) var<storage, read_write> neighbor_count: array<u32>;
@group(0) @binding(3) var<storage, read> params: array<f64>;
@group(0) @binding(4) var<storage, read> cell_start: array<u32>;
@group(0) @binding(5) var<storage, read> cell_count: array<u32>;
@group(0) @binding(6) var<storage, read> sorted_indices: array<u32>;

// params layout (same as cell-list force shader):
//   [0]  = n_particles
//   [1]  = kappa (unused here)
//   [2]  = prefactor (unused here)
//   [3]  = cutoff_sq  — (rc + skin)² for neighbor inclusion
//   [4]  = box_x
//   [5]  = box_y
//   [6]  = box_z
//   [7]  = max_neighbors (as f64, cast to u32)
//   [8]  = n_cells_x
//   [9]  = n_cells_y
//   [10] = n_cells_z
//   [11] = cell_size_x
//   [12] = cell_size_y
//   [13] = cell_size_z
//   [14] = n_cells_total

fn pbc_delta_vb(delta: f64, box_size: f64) -> f64 {
    return delta - box_size * round(delta / box_size);
}

fn cell_idx_vb(cx: i32, cy: i32, cz: i32, nx: i32, ny: i32, nz: i32) -> u32 {
    var wx = cx;
    if (wx < 0)  { wx = wx + nx; }
    if (wx >= nx) { wx = wx - nx; }
    var wy = cy;
    if (wy < 0)  { wy = wy + ny; }
    if (wy >= ny) { wy = wy - ny; }
    var wz = cz;
    if (wz < 0)  { wz = wz + nz; }
    if (wz >= nz) { wz = wz - nz; }
    return u32(wx + wy * nx + wz * nx * ny);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let n = u32(params[0]);
    if (i >= n) { return; }

    let cutoff_sq = params[3];
    let box_x     = params[4];
    let box_y     = params[5];
    let box_z     = params[6];
    let max_nb    = u32(params[7]);
    let nx        = i32(params[8]);
    let ny        = i32(params[9]);
    let nz        = i32(params[10]);
    let cell_sx   = params[11];
    let cell_sy   = params[12];
    let cell_sz   = params[13];

    let xi = positions[i * 3u];
    let yi = positions[i * 3u + 1u];
    let zi = positions[i * 3u + 2u];

    let ci_x = i32(xi / cell_sx);
    let ci_y = i32(yi / cell_sy);
    let ci_z = i32(zi / cell_sz);

    var count = 0u;
    let base = i * max_nb;

    for (var dz = -1; dz <= 1; dz = dz + 1) {
        for (var dy = -1; dy <= 1; dy = dy + 1) {
            for (var dx = -1; dx <= 1; dx = dx + 1) {
                let c_idx = cell_idx_vb(ci_x + dx, ci_y + dy, ci_z + dz, nx, ny, nz);
                let start = cell_start[c_idx];
                let cc = cell_count[c_idx];

                for (var jj = 0u; jj < cc; jj = jj + 1u) {
                    let j = sorted_indices[start + jj];
                    if (i == j) { continue; }

                    let xj = positions[j * 3u];
                    let yj = positions[j * 3u + 1u];
                    let zj = positions[j * 3u + 2u];

                    let ddx = pbc_delta_vb(xj - xi, box_x);
                    let ddy = pbc_delta_vb(yj - yi, box_y);
                    let ddz = pbc_delta_vb(zj - zi, box_z);

                    let r_sq = ddx * ddx + ddy * ddy + ddz * ddz;
                    if (r_sq > cutoff_sq) { continue; }

                    if (count < max_nb) {
                        neighbor_list[base + count] = j;
                        count = count + 1u;
                    }
                }
            }
        }
    }

    neighbor_count[i] = count;
}
