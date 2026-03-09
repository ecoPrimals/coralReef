// SPDX-License-Identifier: AGPL-3.0-or-later
//
// wright_fisher_step.wgsl — One generation of Wright-Fisher drift + selection
//
// Each thread handles one locus across all populations. For each population,
// computes the new allele frequency after one generation of:
//   1. Selection: p' = p * w_A / (p * w_A + (1-p) * w_a)
//      where w_A = 1+s (advantageous) and w_a = 1 (neutral)
//   2. Drift: sample from Binomial(2N, p') approximated by:
//      p_new = round(2N * p') / (2N) with stochastic rounding via PRNG
//
// Uses inline xoshiro128** PRNG (seeded per-thread from prng_state buffer).
// Each generation advances the PRNG state independently per thread.
//
// Papers: 024 (pangenome selection), 025 (meta-population dynamics)
// Absorption target: barracuda::ops::popgen or stochastic pipeline

struct Params {
    n_pops: u32,
    n_loci: u32,
    two_n: u32,
    _pad: u32,
}

@group(0) @binding(0) var<storage, read> freq_in: array<f32>;
@group(0) @binding(1) var<storage, read> selection: array<f32>;
@group(0) @binding(2) var<storage, read_write> freq_out: array<f32>;
@group(0) @binding(3) var<storage, read_write> prng_state: array<u32>;
@group(0) @binding(4) var<uniform> params: Params;

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

fn rand_uniform(s: ptr<function, array<u32, 4>>) -> f32 {
    return f32(xoshiro128ss(s)) / 4294967296.0;
}

@compute @workgroup_size(256)
fn wright_fisher(@builtin(global_invocation_id) gid: vec3<u32>) {
    let tid = gid.x;
    let total = params.n_pops * params.n_loci;
    if tid >= total { return; }

    let pop = tid / params.n_loci;
    let locus = tid % params.n_loci;

    // Load PRNG state
    let prng_base = tid * 4u;
    var s: array<u32, 4>;
    s[0] = prng_state[prng_base];
    s[1] = prng_state[prng_base + 1u];
    s[2] = prng_state[prng_base + 2u];
    s[3] = prng_state[prng_base + 3u];

    let p = freq_in[tid];
    let sel = selection[locus];

    // Selection: p' = p * (1+s) / (p * (1+s) + (1-p))
    let w_a = 1.0 + sel;
    let p_sel = (p * w_a) / (p * w_a + (1.0 - p));

    // Binomial drift approximation: count successes in 2N trials
    let two_n = params.two_n;
    var successes: u32 = 0u;
    for (var trial: u32 = 0u; trial < two_n; trial = trial + 1u) {
        if rand_uniform(&s) < p_sel {
            successes = successes + 1u;
        }
    }

    freq_out[tid] = f32(successes) / f32(two_n);

    // Write back PRNG state
    prng_state[prng_base] = s[0];
    prng_state[prng_base + 1u] = s[1];
    prng_state[prng_base + 2u] = s[2];
    prng_state[prng_base + 3u] = s[3];
}
