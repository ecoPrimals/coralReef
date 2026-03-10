// SPDX-License-Identifier: AGPL-3.0-or-later
//
// population_pk_f64.wgsl — Population PK Monte Carlo on GPU
//
// Each thread simulates one virtual patient:
//   1. Wang hash + xorshift32 PRNG for well-distributed clearance variation
//   2. AUC = F * Dose / CL where CL varies per patient
//
// Uses u32-only PRNG (no SHADER_INT64 needed).
//
// Dispatch: (ceil(n_patients / 256), 1, 1)

struct Params {
    n_patients: u32,
    base_seed: u32,
    dose_mg: f64,
    f_bioavail: f64,
}

@group(0) @binding(0) var<storage, read_write> output: array<f64>;
@group(0) @binding(1) var<uniform> params: Params;

// Wang hash for seed mixing — turns correlated inputs into uniform state
fn wang_hash(input: u32) -> u32 {
    var x = input;
    x = (x ^ 61u) ^ (x >> 16u);
    x = x * 9u;
    x = x ^ (x >> 4u);
    x = x * 0x27d4eb2du;
    x = x ^ (x >> 15u);
    return x;
}

fn xorshift32(state: ptr<function, u32>) -> u32 {
    var x = *state;
    x ^= x << 13u;
    x ^= x >> 17u;
    x ^= x << 5u;
    *state = x;
    return x;
}

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if idx >= params.n_patients {
        return;
    }

    // Per-thread seed via Wang hash for uniform distribution
    var rng_state: u32 = wang_hash(params.base_seed + idx);
    if rng_state == 0u {
        rng_state = 1u;
    }

    let bits = xorshift32(&rng_state);
    let u = f64(bits) / 4294967295.0;

    // CL varies 0.5x to 1.5x around base of 10.0 L/hr
    let cl_factor = 0.5 + u;
    let cl = 10.0 * cl_factor;

    // Single-compartment AUC = F * Dose / CL
    let auc = params.f_bioavail * params.dose_mg / cl;

    output[idx] = auc;
}
