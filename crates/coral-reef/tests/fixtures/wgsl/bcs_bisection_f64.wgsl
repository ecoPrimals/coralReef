// SPDX-License-Identifier: AGPL-3.0-only
// BCS Bisection Root-Finding (f64) — GPU-Parallel
//
// Solves batched BCS chemical-potential problems in parallel:
//   Find μ such that Σ_k deg_k · v²_k(μ) = N
//   where v²_k = ½(1 - (ε_k - μ)/√((ε_k - μ)² + Δ²))
//
// hotSpring domain-specific BCS shader with nuclear HFB degeneracy support.
// ToadStool's `target` keyword bug was fixed (commit 0c477306, Feb 16 2026).
// This shader retained for the `use_degeneracy` feature (2j+1 shell model).
//
// Layout:
//   use_degeneracy=0: params per problem = [ε_0..ε_{n-1}, Δ, N]
//   use_degeneracy=1: params per problem = [ε_0..ε_{n-1}, deg_0..deg_{n-1}, Δ, N]

struct BisectionParams {
    batch_size: u32,
    max_iterations: u32,
    n_levels: u32,
    use_degeneracy: u32,
    tolerance: f64,
}

@group(0) @binding(0) var<storage, read> lower: array<f64>;
@group(0) @binding(1) var<storage, read> upper: array<f64>;
@group(0) @binding(2) var<storage, read> params: array<f64>;
@group(0) @binding(3) var<storage, read_write> roots: array<f64>;
@group(0) @binding(4) var<storage, read_write> iterations: array<u32>;
@group(0) @binding(5) var<uniform> config: BisectionParams;

// abs_f64 injected via ShaderTemplate::with_math_f64_auto() preamble

// BCS particle number: Σ_k deg_k · v²_k(μ) - N
fn bcs_particle_number(mu: f64, problem_idx: u32) -> f64 {
    let n_levels = config.n_levels;
    let use_deg = config.use_degeneracy;

    var params_per_problem: u32;
    if (use_deg == 1u) {
        params_per_problem = n_levels * 2u + 2u;
    } else {
        params_per_problem = n_levels + 2u;
    }

    let base = problem_idx * params_per_problem;
    let delta = params[base + params_per_problem - 2u];
    let target_n = params[base + params_per_problem - 1u];

    var sum = f64(0.0);
    for (var k = 0u; k < n_levels; k = k + 1u) {
        let eps_k = params[base + k];
        let diff = eps_k - mu;
        let e_k = sqrt(diff * diff + delta * delta);
        var v2_k: f64;
        if (abs(diff) > abs(delta) && e_k > f64(1e-30)) {
            let d2 = delta * delta;
            let v2_s = d2 / (f64(2.0) * e_k * (e_k + abs(diff)));
            v2_k = select(f64(1.0) - v2_s, v2_s, diff > f64(0.0));
        } else {
            v2_k = f64(0.5) * (f64(1.0) - diff / e_k);
        }

        var deg_k = f64(1.0);
        if (use_deg == 1u) {
            deg_k = params[base + n_levels + k];
        }

        sum = sum + deg_k * v2_k;
    }

    return sum - target_n;
}

@compute @workgroup_size(64)
fn bcs_bisection(@builtin(global_invocation_id) gid: vec3<u32>) {
    let problem_idx = gid.x;
    if (problem_idx >= config.batch_size) { return; }

    var lo = lower[problem_idx];
    var hi = upper[problem_idx];
    var f_lo = bcs_particle_number(lo, problem_idx);
    var iter_count = 0u;

    for (var iter = 0u; iter < config.max_iterations; iter = iter + 1u) {
        let mid = f64(0.5) * (lo + hi);
        let f_mid = bcs_particle_number(mid, problem_idx);
        iter_count = iter + 1u;

        if (abs_f64(f_mid) < config.tolerance || (hi - lo) < config.tolerance) {
            roots[problem_idx] = mid;
            iterations[problem_idx] = iter_count;
            return;
        }

        if (f_lo * f_mid < f64(0.0)) {
            hi = mid;
        } else {
            lo = mid;
            f_lo = f_mid;
        }
    }

    roots[problem_idx] = f64(0.5) * (lo + hi);
    iterations[problem_idx] = config.max_iterations;
}

// Polynomial test: f(x) = x² - target_val
// (renamed from 'target' which is a WGSL reserved keyword)
fn polynomial_test(x: f64, problem_idx: u32) -> f64 {
    let target_val = params[problem_idx];
    return x * x - target_val;
}

@compute @workgroup_size(64)
fn poly_bisection(@builtin(global_invocation_id) gid: vec3<u32>) {
    let problem_idx = gid.x;
    if (problem_idx >= config.batch_size) { return; }

    var lo = lower[problem_idx];
    var hi = upper[problem_idx];
    var f_lo = polynomial_test(lo, problem_idx);
    var iter_count = 0u;

    for (var iter = 0u; iter < config.max_iterations; iter = iter + 1u) {
        let mid = f64(0.5) * (lo + hi);
        let f_mid = polynomial_test(mid, problem_idx);
        iter_count = iter + 1u;

        if (abs_f64(f_mid) < config.tolerance || (hi - lo) < config.tolerance) {
            roots[problem_idx] = mid;
            iterations[problem_idx] = iter_count;
            return;
        }

        if (f_lo * f_mid < f64(0.0)) {
            hi = mid;
        } else {
            lo = mid;
            f_lo = f_mid;
        }
    }

    roots[problem_idx] = f64(0.5) * (lo + hi);
    iterations[problem_idx] = config.max_iterations;
}
