// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Anderson Lyapunov exponent — f64 compute shader for GPU validation.
//
// Each invocation processes one disorder realization: generates a random
// potential via xoshiro128**, computes the transfer matrix chain, and
// stores the per-realization Lyapunov exponent.
//
// Binding layout:
//   @group(0) @binding(0) params:  Params  {n_sites, n_realizations, disorder, energy}
//   @group(0) @binding(1) seeds:   array<u32>   xoshiro state (4 × u32 per realization)
//   @group(0) @binding(2) output:  array<f64>   one γ per realization
//
// Dispatch: (ceil(n_realizations / 64), 1, 1)
//
// CPU reference: groundspring::anderson::lyapunov_exponent()

struct Params {
    n_sites:         u32,
    n_realizations:  u32,
    disorder_x1000:  i32,
    energy_x1000:    i32,
}

@group(0) @binding(0) var<uniform>             params: Params;
@group(0) @binding(1) var<storage, read_write> seeds:  array<u32>;
@group(0) @binding(2) var<storage, read_write> output: array<f64>;

fn rotl(x: u32, k: u32) -> u32 {
    return (x << k) | (x >> (32u - k));
}

fn xoshiro_next(s: ptr<function, vec4<u32>>) -> u32 {
    let result = rotl((*s).y * 5u, 7u) * 9u;
    let t = (*s).y << 9u;
    (*s).z ^= (*s).x;
    (*s).w ^= (*s).y;
    (*s).y ^= (*s).z;
    (*s).x ^= (*s).w;
    (*s).z ^= t;
    (*s).w = rotl((*s).w, 11u);
    return result;
}

fn xoshiro_uniform(s: ptr<function, vec4<u32>>) -> f64 {
    let hi = xoshiro_next(s);
    let lo = xoshiro_next(s);
    return (f64(hi) * 4294967296.0 + f64(lo)) / 18446744073709551616.0;
}

@compute @workgroup_size(64, 1, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if idx >= params.n_realizations { return; }

    let disorder = f64(params.disorder_x1000) / 1000.0;
    let energy   = f64(params.energy_x1000) / 1000.0;

    let seed_base = idx * 4u;
    var state = vec4<u32>(
        seeds[seed_base],
        seeds[seed_base + 1u],
        seeds[seed_base + 2u],
        seeds[seed_base + 3u],
    );

    var log_growth: f64 = 0.0;
    var v0: f64 = 1.0;
    var v1: f64 = 0.0;

    for (var i = 0u; i < params.n_sites; i++) {
        let u = xoshiro_uniform(&state);
        let potential = disorder * (u - 0.5);
        let factor = energy - potential;

        let new_v0 = factor * v0 - v1;
        v1 = v0;
        v0 = new_v0;

        let norm = sqrt(v0 * v0 + v1 * v1);
        if norm > 0.0 {
            log_growth += log(norm);
            v0 /= norm;
            v1 /= norm;
        }
    }

    output[idx] = log_growth / f64(params.n_sites);

    seeds[seed_base]      = state.x;
    seeds[seed_base + 1u] = state.y;
    seeds[seed_base + 2u] = state.z;
    seeds[seed_base + 3u] = state.w;
}
