// SPDX-License-Identifier: AGPL-3.0-or-later
//
// stencil_cooperation.wgsl — Fermi imitation dynamics on 2D grid (Paper 019)
//
// Each thread updates one cell's strategy by comparing its fitness with a
// randomly selected neighbor's fitness. Probability of adopting the neighbor's
// strategy follows the Fermi function:
//   P(adopt) = 1 / (1 + exp((f_self - f_neighbor) / κ))
//
// where κ is the selection intensity (temperature). This is the standard
// imitation dynamics update rule for spatial evolutionary game theory.
//
// Requires fitness values pre-computed by spatial_payoff.wgsl.
// Uses a deterministic neighbor selection based on thread index for
// reproducibility (Moore neighborhood, 8 neighbors).
//
// Absorption target: barracuda::ops::stencil or game_theory module

struct Params {
    grid_size: u32,
    kappa_x1000: u32,
    step: u32,
    _pad: u32,
}

@group(0) @binding(0) var<storage, read> strategies: array<u32>;
@group(0) @binding(1) var<storage, read> fitness: array<f32>;
@group(0) @binding(2) var<storage, read_write> new_strategies: array<u32>;
@group(0) @binding(3) var<uniform> params: Params;

fn wrap(v: i32, n: i32) -> u32 {
    return u32((v + n) % n);
}

@compute @workgroup_size(256)
fn stencil_update(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let n = params.grid_size;
    if idx >= n * n { return; }

    let kappa = f32(params.kappa_x1000) / 1000.0;

    let i = i32(idx / n);
    let j = i32(idx % n);
    let ni = i32(n);

    // Deterministic neighbor selection: rotate through 8 Moore neighbors
    // based on step count and cell index for reproducible dynamics
    let neighbor_idx = (idx + params.step) % 8u;

    var di: i32 = 0;
    var dj: i32 = 0;
    switch neighbor_idx {
        case 0u: { di = -1; dj = -1; }
        case 1u: { di = -1; dj =  0; }
        case 2u: { di = -1; dj =  1; }
        case 3u: { di =  0; dj = -1; }
        case 4u: { di =  0; dj =  1; }
        case 5u: { di =  1; dj = -1; }
        case 6u: { di =  1; dj =  0; }
        default: { di =  1; dj =  1; }
    }

    let nb_i = wrap(i + di, ni);
    let nb_j = wrap(j + dj, ni);
    let nb_idx = nb_i * n + nb_j;

    let f_self = fitness[idx];
    let f_nb = fitness[nb_idx];

    // Fermi imitation probability: P = 1 / (1 + exp((f_self - f_nb) / κ))
    let p_adopt = 1.0 / (1.0 + exp((f_self - f_nb) / kappa));

    // Deterministic threshold: adopt if probability exceeds 0.5
    // (equivalent to: adopt if neighbor is fitter, with κ controlling sharpness)
    if p_adopt > 0.5 {
        new_strategies[idx] = strategies[nb_idx];
    } else {
        new_strategies[idx] = strategies[idx];
    }
}
