// SPDX-License-Identifier: AGPL-3.0-only
//
// Anderson Lyapunov exponent — f32 compute shader.
//
// f32 variant for GPUs where f64 WGSL shaders don't compile (NAK f64 gap,
// NVVM consumer limitation). Uses the same algorithm as the f64 version
// but with reduced precision (~7 significant digits vs ~15).
//
// For production precision on f32-only hardware, use ToadStool's DF64
// (double-float) emulation: pairs of f32 values giving ~50 bits of
// significand (vs f64's 52 bits).
//
// Binding layout:
//   @group(0) @binding(0) params:  Params  {n_sites, n_realizations, disorder_x1000, energy_x1000}
//   @group(0) @binding(1) seeds:   array<u32>   xoshiro state (4 × u32 per realization)
//   @group(0) @binding(2) output:  array<f32>   one γ per realization
//
// Dispatch: (ceil(n_realizations / 64), 1, 1)

struct Params {
    n_sites:         u32,
    n_realizations:  u32,
    disorder_x1000:  i32,
    energy_x1000:    i32,
}

@group(0) @binding(0) var<uniform>             params: Params;
@group(0) @binding(1) var<storage, read_write> seeds:  array<u32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;

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

fn xoshiro_uniform(s: ptr<function, vec4<u32>>) -> f32 {
    let bits = xoshiro_next(s);
    return f32(bits) / 4294967296.0;
}

@compute @workgroup_size(64, 1, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if idx >= params.n_realizations { return; }

    let disorder = f32(params.disorder_x1000) / 1000.0;
    let energy   = f32(params.energy_x1000) / 1000.0;

    let seed_base = idx * 4u;
    var state = vec4<u32>(
        seeds[seed_base],
        seeds[seed_base + 1u],
        seeds[seed_base + 2u],
        seeds[seed_base + 3u],
    );

    var log_growth: f32 = 0.0;
    var v0: f32 = 1.0;
    var v1: f32 = 0.0;

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

    output[idx] = log_growth / f32(params.n_sites);

    seeds[seed_base]      = state.x;
    seeds[seed_base + 1u] = state.y;
    seeds[seed_base + 2u] = state.z;
    seeds[seed_base + 3u] = state.w;
}
