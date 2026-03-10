// SPDX-License-Identifier: AGPL-3.0-only
// 1D Euler fluid solver with HLL approximate Riemann solver.
//
// GPU-accelerated spatial mesh update for the kinetic-fluid coupling pipeline.
// Each thread computes the flux at one cell interface and applies the
// conservative update for the adjacent cell.
//
// Two-pass approach:
// Pass 1 (flux): Compute HLL flux at each interface i+1/2.
// Pass 2 (update): Apply conservative update U^{n+1} = U^n - (dt/dx)(F_{i+1/2} - F_{i-1/2}).

struct EulerParams {
    nx: u32,
    _pad: u32,
    dt: f64,
    dx: f64,
    gamma: f64,
}

@group(0) @binding(0) var<uniform> params: EulerParams;
// Conserved variables: [rho_0, rho_1, ..., rhou_0, rhou_1, ..., E_0, E_1, ...]
@group(0) @binding(1) var<storage, read_write> cons: array<f64>;
// HLL fluxes at interfaces: [F_rho_0, F_rho_1, ..., F_mom_0, ..., F_ene_0, ...]
@group(0) @binding(2) var<storage, read_write> flux: array<f64>;

fn cons_to_prim(rho: f64, rhou: f64, e: f64) -> vec3<f64> {
    let zero = params.dx - params.dx;
    var u = zero;
    if abs(rho) > 1e-30 { u = rhou / rho; }
    let p = max((params.gamma - 1.0) * (e - 0.5 * rho * u * u), zero + 1e-30);
    return vec3<f64>(rho, u, p);
}

fn euler_flux_vec(rho: f64, u: f64, p: f64) -> vec3<f64> {
    let e = p / (params.gamma - 1.0) + 0.5 * rho * u * u;
    return vec3<f64>(rho * u, rho * u * u + p, (e + p) * u);
}

fn sound_speed(rho: f64, p: f64) -> f64 {
    let zero = params.dx - params.dx;
    if rho > 1e-30 {
        return sqrt(params.gamma * p / rho);
    }
    return zero;
}

// Pass 1: compute HLL flux at interface i+1/2 (between cells i and i+1).
// Thread i computes flux at interface i (between cell i-1 and cell i).
// Thread 0 copies left boundary flux; thread nx copies right boundary flux.
@compute @workgroup_size(64)
fn compute_hll_flux(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let nx = params.nx;

    if i > nx { return; }

    let n = nx;

    if i == 0u {
        // Left boundary: outflow (copy cell 0 flux)
        let prim = cons_to_prim(cons[0], cons[n], cons[2u * n]);
        let f = euler_flux_vec(prim.x, prim.y, prim.z);
        flux[0] = f.x;
        flux[n + 1u] = f.y;
        flux[2u * (n + 1u)] = f.z;
        return;
    }

    if i == nx {
        // Right boundary: outflow (copy last cell flux)
        let last = nx - 1u;
        let prim = cons_to_prim(cons[last], cons[n + last], cons[2u * n + last]);
        let f = euler_flux_vec(prim.x, prim.y, prim.z);
        flux[nx] = f.x;
        flux[n + 1u + nx] = f.y;
        flux[2u * (n + 1u) + nx] = f.z;
        return;
    }

    // Interior interface between cell (i-1) and cell i
    let l = i - 1u;
    let r_idx = i;

    let prim_l = cons_to_prim(cons[l], cons[n + l], cons[2u * n + l]);
    let prim_r = cons_to_prim(cons[r_idx], cons[n + r_idx], cons[2u * n + r_idx]);

    let c_l = sound_speed(prim_l.x, prim_l.z);
    let c_r = sound_speed(prim_r.x, prim_r.z);

    let s_l = min(prim_l.y - c_l, prim_r.y - c_r);
    let s_r = max(prim_l.y + c_l, prim_r.y + c_r);

    let f_l = euler_flux_vec(prim_l.x, prim_l.y, prim_l.z);
    let f_r = euler_flux_vec(prim_r.x, prim_r.y, prim_r.z);

    var f_hll = vec3<f64>(0.0, 0.0, 0.0);

    if s_l >= 0.0 {
        f_hll = f_l;
    } else if s_r <= 0.0 {
        f_hll = f_r;
    } else {
        let denom = s_r - s_l;
        let u_l = vec3<f64>(cons[l], cons[n + l], cons[2u * n + l]);
        let u_r = vec3<f64>(cons[r_idx], cons[n + r_idx], cons[2u * n + r_idx]);
        f_hll = (f_l * s_r - f_r * s_l + u_r * (s_l * s_r) - u_l * (s_l * s_r)) * (1.0 / denom);
    }

    let fi = i;
    flux[fi] = f_hll.x;
    flux[n + 1u + fi] = f_hll.y;
    flux[2u * (n + 1u) + fi] = f_hll.z;
}

// Pass 2: conservative update U^{n+1} = U^n - (dt/dx)(F_{i+1/2} - F_{i-1/2}).
@compute @workgroup_size(64)
fn euler_update(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let nx = params.nx;
    if i >= nx { return; }

    let n = nx;
    let ratio = params.dt / params.dx;

    // Flux at i+1/2 is flux[i+1], flux at i-1/2 is flux[i]
    let f_rho_r = flux[i + 1u];
    let f_rho_l = flux[i];
    let f_mom_r = flux[n + 1u + i + 1u];
    let f_mom_l = flux[n + 1u + i];
    let f_ene_r = flux[2u * (n + 1u) + i + 1u];
    let f_ene_l = flux[2u * (n + 1u) + i];

    cons[i] = max(cons[i] - ratio * (f_rho_r - f_rho_l), 1e-10);
    cons[n + i] = cons[n + i] - ratio * (f_mom_r - f_mom_l);
    cons[2u * n + i] = cons[2u * n + i] - ratio * (f_ene_r - f_ene_l);
}
