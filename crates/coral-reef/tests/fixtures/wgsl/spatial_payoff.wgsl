// SPDX-License-Identifier: AGPL-3.0-or-later
//
// spatial_payoff.wgsl — Spatial prisoner's dilemma payoff stencil (Paper 019)
//
// Each thread computes the cumulative PD payoff for one cell in a
// 2D grid using a Moore neighborhood (8 neighbors) with periodic
// boundary conditions.
//
// Absorption target: barracuda::ops::stencil or conv1d-based cooperation

struct Params {
    grid_size: u32,
    b_x1000: u32,
    c_x1000: u32,
    _pad: u32,
}

@group(0) @binding(0) var<storage, read> grid: array<u32>;
@group(0) @binding(1) var<storage, read_write> fitness: array<f32>;
@group(0) @binding(2) var<uniform> params: Params;

@compute @workgroup_size(256)
fn spatial_payoff(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let n = params.grid_size;
    if idx >= n * n { return; }

    let b = f32(params.b_x1000) / 1000.0;
    let c = f32(params.c_x1000) / 1000.0;

    let i = idx / n;
    let j = idx % n;
    let me = grid[idx];

    var total: f32 = 0.0;

    // Moore neighborhood: 8 neighbors with periodic boundary
    for (var di: i32 = -1; di <= 1; di = di + 1) {
        for (var dj: i32 = -1; dj <= 1; dj = dj + 1) {
            if di == 0 && dj == 0 { continue; }
            let ni = u32((i32(i) + di + i32(n)) % i32(n));
            let nj = u32((i32(j) + dj + i32(n)) % i32(n));
            let other = grid[ni * n + nj];

            if me == 1u && other == 1u {
                total = total + b - c;
            } else if me == 1u && other == 0u {
                total = total - c;
            } else if me == 0u && other == 1u {
                total = total + b;
            }
        }
    }

    fitness[idx] = total;
}
