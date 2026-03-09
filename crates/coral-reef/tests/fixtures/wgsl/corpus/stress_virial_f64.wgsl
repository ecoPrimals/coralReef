// SPDX-License-Identifier: AGPL-3.0-only
//
// Off-diagonal stress tensor σ_xy (f64) — GPU virial computation
//
// Computes per-particle contributions to σ_xy for Green-Kubo viscosity.
// σ_xy = Σ_i [m v_ix v_iy + Σ_{j>i} F_ij_x r_ij_y]
//
// Output is per-particle σ_xy(i), reduced via ReduceScalarPipeline.
// Reuses the Yukawa pair kernel pattern from yukawa_force_f64.wgsl.
//
// Bindings:
//   0: positions  [N*3] f64, read  — (x,y,z) per particle
//   1: velocities [N*3] f64, read  — (vx,vy,vz) per particle
//   2: out        [N]   f64, write — per-particle σ_xy contribution
//   3: params     [8]   f64, read  — simulation parameters
//
// params layout (same as yukawa_force):
//   [0] = n_particles (as f64)
//   [1] = kappa
//   [2] = mass
//   [3] = cutoff_sq
//   [4] = box_x
//   [5] = box_y
//   [6] = box_z
//   [7] = epsilon (softening)

@group(0) @binding(0) var<storage, read> positions: array<f64>;
@group(0) @binding(1) var<storage, read> velocities: array<f64>;
@group(0) @binding(2) var<storage, read_write> out: array<f64>;
@group(0) @binding(3) var<storage, read> params: array<f64>;

fn pbc_delta(delta: f64, box_size: f64) -> f64 {
    return delta - box_size * round(delta / box_size);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let n = u32(params[0]);
    if (i >= n) { return; }

    let xi = positions[i * 3u];
    let yi = positions[i * 3u + 1u];
    let zi = positions[i * 3u + 2u];

    let vxi = velocities[i * 3u];
    let vyi = velocities[i * 3u + 1u];

    let kappa     = params[1];
    let mass      = params[2];
    let cutoff_sq = params[3];
    let box_x     = params[4];
    let box_y     = params[5];
    let box_z     = params[6];
    let eps       = params[7];

    // Kinetic contribution: m * vx * vy (full per particle)
    var sigma = mass * vxi * vyi;

    // Virial contribution: half of sum_{j!=i} F_ij_x * r_ij_y
    // Using Newton's third law: particle i accumulates half for j>i, half for j<i,
    // which equals the full i-sum when every particle runs this kernel.
    // F_ij_x * r_ij_y = f_mag * dx * dy / r (same as in force kernel)
    for (var j = 0u; j < n; j = j + 1u) {
        if (i == j) { continue; }

        let dx = pbc_delta(positions[j * 3u] - xi, box_x);
        let dy = pbc_delta(positions[j * 3u + 1u] - yi, box_y);
        let dz = pbc_delta(positions[j * 3u + 2u] - zi, box_z);

        let r2 = dx * dx + dy * dy + dz * dz + eps;
        if (r2 > cutoff_sq) { continue; }

        let r = sqrt(r2);
        let exp_kr = exp(-kappa * r);
        let f_mag = exp_kr * (1.0 + kappa * r) / r2;

        // Half-count: each pair counted once per particle, divide by 2
        sigma += 0.5 * f_mag * dx * dy / r;
    }

    out[i] = sigma;
}
