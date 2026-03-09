// SPDX-License-Identifier: AGPL-3.0-or-later
//
// hill_gate.wgsl — Two-input Hill AND gate (Paper 021)
//
// Evaluates f(cdg, ai) = vmax * (cdg^n1 / (k1^n1 + cdg^n1)) * (ai^n2 / (k2^n2 + ai^n2))
// over a 2D grid. Each thread handles one (i, j) point. idx = gid.x; ix = idx / ny; iy = idx % ny.
//
// Absorption target: barracuda::ops::elementwise two-input Hill

struct HillParams {
    nx: u32,
    ny: u32,
    vmax: f32,
    k1: f32,
    k2: f32,
    n1: f32,
    n2: f32,
    _pad: u32,
}

@group(0) @binding(0) var<storage, read> cdg_grid: array<f32>;
@group(0) @binding(1) var<storage, read> ai_grid: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;
@group(0) @binding(3) var<uniform> params: HillParams;

@compute @workgroup_size(256)
fn hill_gate(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let n_total = params.nx * params.ny;
    if idx >= n_total {
        return;
    }

    let iy = idx % params.ny;
    let ix = idx / params.ny;

    let cdg = cdg_grid[ix];
    let ai = ai_grid[iy];

    let h1 = pow(cdg, params.n1) / (pow(params.k1, params.n1) + pow(cdg, params.n1));
    let h2 = pow(ai, params.n2) / (pow(params.k2, params.n2) + pow(ai, params.n2));

    output[idx] = params.vmax * h1 * h2;
}
