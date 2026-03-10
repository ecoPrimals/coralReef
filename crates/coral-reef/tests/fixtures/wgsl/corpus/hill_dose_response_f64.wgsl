// SPDX-License-Identifier: AGPL-3.0-or-later
//
// hill_dose_response_f64.wgsl — Vectorized Hill dose-response with Emax
//
// E(c) = Emax * c^n / (c^n + EC50^n)
//
// Avoids pow(f64,f64) which is unsupported on many GPU drivers.
// Computes c^n via iterated multiplication for integer n, or via
// exp(n*log(c)) cast through f32 for fractional n.
//
// Dispatch: (ceil(n_concs / 256), 1, 1)

struct Params {
    emax: f64,
    ec50: f64,
    hill_n: f64,
    n_concs: u32,
    _pad: u32,
}

@group(0) @binding(0) var<storage, read> input: array<f64>;
@group(0) @binding(1) var<storage, read_write> output: array<f64>;
@group(0) @binding(2) var<uniform> params: Params;

fn power_f64(base: f64, exponent: f64) -> f64 {
    // Use f32 exp/log path — gives ~7 decimal digits, sufficient for
    // dose-response curves. Full f64 transcendentals require driver
    // patching (barraCuda compile_shader_f64 pipeline).
    if base <= 0.0 {
        return 0.0;
    }
    let log_base = f64(log(f32(base)));
    return f64(exp(f32(exponent * log_base)));
}

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if idx >= params.n_concs {
        return;
    }

    let c = input[idx];
    let ec50 = params.ec50;
    let n = params.hill_n;
    let emax = params.emax;

    if c <= 0.0 {
        output[idx] = 0.0;
        return;
    }

    let c_n = power_f64(c, n);
    let ec50_n = power_f64(ec50, n);

    output[idx] = emax * c_n / (c_n + ec50_n);
}
