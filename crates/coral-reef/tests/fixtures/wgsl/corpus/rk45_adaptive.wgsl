// SPDX-License-Identifier: AGPL-3.0-or-later
//
// rk45_adaptive.wgsl — Adaptive Dormand-Prince RK45 for regulatory networks
//
// Single adaptive step of the Dormand-Prince 5(4) embedded pair.
// Each thread handles one independent ODE system with Hill function kinetics
// (Paper 020: regulatory network, Paper 021: signal integration).
//
// Math:
//   y_new = y + h * Σ b_i * k_i     (5th order solution)
//   err   = h * Σ (b_i - b*_i) * k_i (error estimate from 4th order)
//
// State layout per system: [y0, y1, ..., y_{dim-1}]
// Coefficients per system: [prod_0, deg_0, act_idx_0, ...] (3 per variable)
//
// Output: new_state (5th order) and error (per-variable absolute error)
// Host uses error to adapt step size: h_new = h * min(5, max(0.2, 0.9*(tol/err)^0.2))
//
// Absorption target: barracuda::ops::ode (extends rk45_f64 with Hill RHS)
// Reference: Dormand & Prince (1980), Mhatre et al. (2020) PNAS

struct Params {
    n_systems: u32,
    dim: u32,
    n_coeffs: u32,
    _pad: u32,
    dt: f32,
    _pad2: f32,
    _pad3: f32,
    _pad4: f32,
}

@group(0) @binding(0) var<storage, read> state: array<f32>;
@group(0) @binding(1) var<storage, read> coeffs: array<f32>;
@group(0) @binding(2) var<storage, read_write> new_state: array<f32>;
@group(0) @binding(3) var<storage, read_write> error: array<f32>;
@group(0) @binding(4) var<uniform> params: Params;
@group(0) @binding(5) var<storage, read_write> scratch: array<f32>;

fn hill(x: f32, k: f32, n: f32) -> f32 {
    let xn = pow(x, n);
    return xn / (pow(k, n) + xn);
}

fn rhs(sys: u32, d: u32, y_base: u32) -> f32 {
    let c_base = sys * params.n_coeffs + d * 3u;
    let prod = coeffs[c_base];
    let deg = coeffs[c_base + 1u];
    let act_idx = u32(coeffs[c_base + 2u]);
    let activator = scratch[y_base + act_idx];
    return prod * hill(activator, 0.5, 2.0) - deg * scratch[y_base + d];
}

fn write_k(sys: u32, stage: u32, d: u32, val: f32) {
    // k layout: scratch[sys * dim * 8 + stage * dim + d]  (stages 0-6 for k1-k7)
    scratch[sys * params.dim * 8u + stage * params.dim + d] = val;
}

fn read_k(sys: u32, stage: u32, d: u32) -> f32 {
    return scratch[sys * params.dim * 8u + stage * params.dim + d];
}

fn write_tmp(sys: u32, d: u32, val: f32) {
    // tmp state in stage slot 7
    scratch[sys * params.dim * 8u + 7u * params.dim + d] = val;
}

@compute @workgroup_size(64)
fn rk45_step(@builtin(global_invocation_id) gid: vec3<u32>) {
    let sys = gid.x;
    if sys >= params.n_systems { return; }

    let dim = params.dim;
    let h = params.dt;
    let y_base = sys * dim * 8u + 7u * dim;

    // Load initial state into tmp
    for (var d: u32 = 0u; d < dim; d = d + 1u) {
        write_tmp(sys, d, state[sys * dim + d]);
    }

    // Stage 1: k1 = f(y)
    for (var d: u32 = 0u; d < dim; d = d + 1u) {
        write_k(sys, 0u, d, rhs(sys, d, y_base));
    }

    // Stage 2: k2 = f(y + h * (1/5) * k1)
    for (var d: u32 = 0u; d < dim; d = d + 1u) {
        let y0 = state[sys * dim + d];
        write_tmp(sys, d, y0 + h * (1.0 / 5.0) * read_k(sys, 0u, d));
    }
    for (var d: u32 = 0u; d < dim; d = d + 1u) {
        write_k(sys, 1u, d, rhs(sys, d, y_base));
    }

    // Stage 3: k3 = f(y + h * (3/40*k1 + 9/40*k2))
    for (var d: u32 = 0u; d < dim; d = d + 1u) {
        let y0 = state[sys * dim + d];
        let inc = (3.0 / 40.0) * read_k(sys, 0u, d) + (9.0 / 40.0) * read_k(sys, 1u, d);
        write_tmp(sys, d, y0 + h * inc);
    }
    for (var d: u32 = 0u; d < dim; d = d + 1u) {
        write_k(sys, 2u, d, rhs(sys, d, y_base));
    }

    // Stage 4: k4 = f(y + h * (44/45*k1 - 56/15*k2 + 32/9*k3))
    for (var d: u32 = 0u; d < dim; d = d + 1u) {
        let y0 = state[sys * dim + d];
        let inc = (44.0 / 45.0) * read_k(sys, 0u, d)
                - (56.0 / 15.0) * read_k(sys, 1u, d)
                + (32.0 / 9.0)  * read_k(sys, 2u, d);
        write_tmp(sys, d, y0 + h * inc);
    }
    for (var d: u32 = 0u; d < dim; d = d + 1u) {
        write_k(sys, 3u, d, rhs(sys, d, y_base));
    }

    // Stage 5: k5
    for (var d: u32 = 0u; d < dim; d = d + 1u) {
        let y0 = state[sys * dim + d];
        let inc = (19372.0 / 6561.0)  * read_k(sys, 0u, d)
                - (25360.0 / 2187.0)  * read_k(sys, 1u, d)
                + (64448.0 / 6561.0)  * read_k(sys, 2u, d)
                - (212.0 / 729.0)     * read_k(sys, 3u, d);
        write_tmp(sys, d, y0 + h * inc);
    }
    for (var d: u32 = 0u; d < dim; d = d + 1u) {
        write_k(sys, 4u, d, rhs(sys, d, y_base));
    }

    // Stage 6: k6
    for (var d: u32 = 0u; d < dim; d = d + 1u) {
        let y0 = state[sys * dim + d];
        let inc = (9017.0 / 3168.0)   * read_k(sys, 0u, d)
                - (355.0 / 33.0)      * read_k(sys, 1u, d)
                + (46732.0 / 5247.0)  * read_k(sys, 2u, d)
                + (49.0 / 176.0)      * read_k(sys, 3u, d)
                - (5103.0 / 18656.0)  * read_k(sys, 4u, d);
        write_tmp(sys, d, y0 + h * inc);
    }
    for (var d: u32 = 0u; d < dim; d = d + 1u) {
        write_k(sys, 5u, d, rhs(sys, d, y_base));
    }

    // 5th order solution and error estimate
    for (var d: u32 = 0u; d < dim; d = d + 1u) {
        let y0 = state[sys * dim + d];
        let k1 = read_k(sys, 0u, d);
        let k3 = read_k(sys, 2u, d);
        let k4 = read_k(sys, 3u, d);
        let k5 = read_k(sys, 4u, d);
        let k6 = read_k(sys, 5u, d);

        // 5th order weights (Dormand-Prince)
        let y5 = y0 + h * (
            (35.0 / 384.0)    * k1
          + (500.0 / 1113.0)  * k3
          + (125.0 / 192.0)   * k4
          - (2187.0 / 6784.0) * k5
          + (11.0 / 84.0)     * k6
        );

        // Error estimate: difference between 5th and 4th order
        // e_i = h * Σ (b_i - b*_i) * k_i
        let e = h * (
            (71.0 / 57600.0)     * k1
          - (71.0 / 16695.0)     * k3
          + (71.0 / 1920.0)      * k4
          - (17253.0 / 339200.0) * k5
          + (22.0 / 525.0)       * k6
        );

        new_state[sys * dim + d] = y5;
        error[sys * dim + d] = abs(e);
    }
}
