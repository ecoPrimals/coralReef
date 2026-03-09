// SPDX-License-Identifier: AGPL-3.0-only
//
// DEPRECATED — Use barracuda::ops::md::forces::YukawaForceF64
// Retained for fossil record. barracuda canonical: ops/md/forces/yukawa_f64.wgsl
//
// Yukawa All-Pairs Force (f64) with PBC + potential energy
//
// Bindings:
//   0: positions  [N*3] f64, read     — (x,y,z) per particle
//   1: forces     [N*3] f64, write    — (fx,fy,fz) per particle
//   2: pe_buf     [N]   f64, write    — per-particle PE (half-counted)
//   3: params     [12]  f64, read     — simulation parameters

@group(0) @binding(0) var<storage, read> positions: array<f64>;
@group(0) @binding(1) var<storage, read_write> forces: array<f64>;
@group(0) @binding(2) var<storage, read_write> pe_buf: array<f64>;
@group(0) @binding(3) var<storage, read> params: array<f64>;

// params layout:
//   [0] = n_particles (as f64, cast to u32)
//   [1] = kappa (screening parameter, reduced units)
//   [2] = prefactor (coupling: Gamma * a_ws in reduced = Gamma for OCP convention)
//   [3] = cutoff_sq (rc² in reduced units)
//   [4] = box_x (box side in reduced units)
//   [5] = box_y
//   [6] = box_z
//   [7] = epsilon (softening, typically 0 or 1e-30)

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

    let kappa    = params[1];
    let prefactor = params[2];
    let cutoff_sq = params[3];
    let box_x    = params[4];
    let box_y    = params[5];
    let box_z    = params[6];
    let eps      = params[7];

    // Accumulate force and PE
    var fx = xi - xi;  // 0.0 as f64
    var fy = xi - xi;
    var fz = xi - xi;
    var pe = xi - xi;

    for (var j = 0u; j < n; j = j + 1u) {
        if (i == j) { continue; }

        let xj = positions[j * 3u];
        let yj = positions[j * 3u + 1u];
        let zj = positions[j * 3u + 2u];

        // PBC minimum image
        var dx = pbc_delta(xj - xi, box_x);
        var dy = pbc_delta(yj - yi, box_y);
        var dz = pbc_delta(zj - zi, box_z);

        let r_sq = dx * dx + dy * dy + dz * dz;

        if (r_sq > cutoff_sq) { continue; }

        let r = sqrt(r_sq + eps);

        // Yukawa force: F = prefactor * exp(-kappa*r) * (1 + kappa*r) / r^2
        let screening = exp(-kappa * r);
        let force_mag = prefactor * screening * (1.0 + kappa * r) / r_sq;

        // Force on particle i due to j: repulsive → push AWAY from j
        // F_i = -prefactor * exp(-κr) * (1+κr)/r² * r̂_ij
        // r̂_ij = (dx,dy,dz)/r points from i to j, so negate for repulsion
        let inv_r = 1.0 / r;
        fx = fx - force_mag * dx * inv_r;
        fy = fy - force_mag * dy * inv_r;
        fz = fz - force_mag * dz * inv_r;

        // PE: U = prefactor * exp(-kappa*r) / r  (half-count: each pair once)
        pe = pe + 0.5 * prefactor * screening * inv_r;
    }

    forces[i * 3u]      = fx;
    forces[i * 3u + 1u] = fy;
    forces[i * 3u + 2u] = fz;
    pe_buf[i] = pe;
}
