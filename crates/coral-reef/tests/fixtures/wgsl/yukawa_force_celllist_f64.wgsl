// SPDX-License-Identifier: AGPL-3.0-only
//
// DEPRECATED — Use barracuda::ops::md::forces::YukawaCelllistF64
// Retained for fossil record. barracuda canonical: ops/md/forces/yukawa_celllist_f64.wgsl
// NOTE: hotSpring cell_idx branch fix should be upstreamed to barracuda.
//
// Yukawa Cell-List Force (f64) with PBC + potential energy
//
// Bindings:
//   0: positions    [N*3]       f64, read  — sorted by cell index
//   1: forces       [N*3]       f64, write
//   2: pe_buf       [N]         f64, write — per-particle PE (half-counted)
//   3: params       [16]        f64, read  — simulation parameters
//   4: cell_start   [n_cells_total] u32, read — first particle index in each cell
//   5: cell_count   [n_cells_total] u32, read — number of particles in each cell

@group(0) @binding(0) var<storage, read> positions: array<f64>;
@group(0) @binding(1) var<storage, read_write> forces: array<f64>;
@group(0) @binding(2) var<storage, read_write> pe_buf: array<f64>;
@group(0) @binding(3) var<storage, read> params: array<f64>;
@group(0) @binding(4) var<storage, read> cell_start: array<u32>;
@group(0) @binding(5) var<storage, read> cell_count: array<u32>;

// params layout:
//   [0]  = n_particles
//   [1]  = kappa
//   [2]  = prefactor (1.0 in reduced units)
//   [3]  = cutoff_sq
//   [4]  = box_x
//   [5]  = box_y
//   [6]  = box_z
//   [7]  = epsilon (softening)
//   [8]  = n_cells_x
//   [9]  = n_cells_y
//   [10] = n_cells_z
//   [11] = cell_size_x
//   [12] = cell_size_y
//   [13] = cell_size_z
//   [14] = n_cells_total

fn pbc_delta_cl(delta: f64, box_size: f64) -> f64 {
    return delta - box_size * round(delta / box_size);
}

// Map 3D cell coordinates to linear index
// Uses branch-based wrapping instead of modular arithmetic because
// WGSL i32 remainder (%) has inconsistent behavior for negative operands
// across GPU drivers (Naga/NVIDIA), causing cell 0 to be revisited.
fn cell_idx(cx: i32, cy: i32, cz: i32, nx: i32, ny: i32, nz: i32) -> u32 {
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

    let kappa     = params[1];
    let prefactor = params[2];
    let cutoff_sq = params[3];
    let box_x     = params[4];
    let box_y     = params[5];
    let box_z     = params[6];
    let eps       = params[7];
    let nx        = i32(params[8]);
    let ny        = i32(params[9]);
    let nz        = i32(params[10]);
    let cell_sx   = params[11];
    let cell_sy   = params[12];
    let cell_sz   = params[13];

    let xi = positions[i * 3u];
    let yi = positions[i * 3u + 1u];
    let zi = positions[i * 3u + 2u];

    // Which cell is particle i in?
    let ci_x = i32(xi / cell_sx);
    let ci_y = i32(yi / cell_sy);
    let ci_z = i32(zi / cell_sz);

    var fx = xi - xi;
    var fy = xi - xi;
    var fz = xi - xi;
    var pe = xi - xi;

    // Loop over 27 neighbor cells (including self)
    for (var dz = -1; dz <= 1; dz = dz + 1) {
        for (var dy = -1; dy <= 1; dy = dy + 1) {
            for (var dx = -1; dx <= 1; dx = dx + 1) {
                let c_idx = cell_idx(ci_x + dx, ci_y + dy, ci_z + dz, nx, ny, nz);
                let start = cell_start[c_idx];
                let count = cell_count[c_idx];

                for (var jj = 0u; jj < count; jj = jj + 1u) {
                    let j = start + jj;
                    if (i == j) { continue; }

                    let xj = positions[j * 3u];
                    let yj = positions[j * 3u + 1u];
                    let zj = positions[j * 3u + 2u];

                    var ddx = pbc_delta_cl(xj - xi, box_x);
                    var ddy = pbc_delta_cl(yj - yi, box_y);
                    var ddz = pbc_delta_cl(zj - zi, box_z);

                    let r_sq = ddx * ddx + ddy * ddy + ddz * ddz;
                    if (r_sq > cutoff_sq) { continue; }

                    let r = sqrt(r_sq + eps);
                    let screening = exp(-kappa * r);
                    let force_mag = prefactor * screening * (1.0 + kappa * r) / r_sq;
                    let inv_r = 1.0 / r;

                    fx = fx - force_mag * ddx * inv_r;
                    fy = fy - force_mag * ddy * inv_r;
                    fz = fz - force_mag * ddz * inv_r;
                    pe = pe + 0.5 * prefactor * screening * inv_r;
                }
            }
        }
    }

    forces[i * 3u]      = fx;
    forces[i * 3u + 1u] = fy;
    forces[i * 3u + 2u] = fz;
    pe_buf[i] = pe;
}
