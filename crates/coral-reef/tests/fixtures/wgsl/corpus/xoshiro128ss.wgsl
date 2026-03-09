// SPDX-License-Identifier: AGPL-3.0-or-later
//
// xoshiro128ss.wgsl — GPU-parallel PRNG (Xoshiro128**)
//
// Each thread has independent state (4 × u32). Seeded via SplitMix32.
// Generates uniform f32 in [0, 1) by dividing result by 2^32.
//
// Absorption target: barracuda::ops::prng or StatefulPipeline extension

struct Params {
    n_threads: u32,
    n_samples: u32,
}

@group(0) @binding(0) var<storage, read_write> state: array<u32>;  // 4 * n_threads
@group(0) @binding(1) var<storage, read_write> output: array<f32>; // n_threads * n_samples
@group(0) @binding(2) var<uniform> params: Params;

fn rotl(x: u32, k: u32) -> u32 {
    return (x << k) | (x >> (32u - k));
}

fn xoshiro128ss(s: ptr<function, array<u32, 4>>) -> u32 {
    let result = rotl((*s)[1] * 5u, 7u) * 9u;
    let t = (*s)[1] << 9u;
    (*s)[2] ^= (*s)[0];
    (*s)[3] ^= (*s)[1];
    (*s)[1] ^= (*s)[2];
    (*s)[0] ^= (*s)[3];
    (*s)[2] ^= t;
    (*s)[3] = rotl((*s)[3], 11u);
    return result;
}

@compute @workgroup_size(256)
fn generate(@builtin(global_invocation_id) gid: vec3<u32>) {
    let tid = gid.x;
    if tid >= params.n_threads {
        return;
    }

    var s: array<u32, 4>;
    let base = tid * 4u;
    s[0] = state[base];
    s[1] = state[base + 1u];
    s[2] = state[base + 2u];
    s[3] = state[base + 3u];

    let out_base = tid * params.n_samples;
    for (var i: u32 = 0u; i < params.n_samples; i = i + 1u) {
        let raw = xoshiro128ss(&s);
        output[out_base + i] = f32(raw) / 4294967296.0;  // 2^32
    }

    // Write back updated state
    state[base] = s[0];
    state[base + 1u] = s[1];
    state[base + 2u] = s[2];
    state[base + 3u] = s[3];
}
