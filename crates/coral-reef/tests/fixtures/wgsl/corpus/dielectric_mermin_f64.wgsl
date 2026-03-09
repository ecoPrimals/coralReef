// SPDX-License-Identifier: AGPL-3.0-only
// Batched Mermin dielectric function ε(k,ω) on GPU.
// Each thread evaluates one ω point through the full chain:
//   plasma_W → χ₀ → ε_Vlasov → ε_Mermin → Im[1/ε] → S(k,ω)
//
// NUMERICAL STABILITY: For |z| ≥ 4, W(z) is computed directly via its
// asymptotic expansion, avoiding catastrophic cancellation in 1 + z·Z(z)
// where z·Z(z) ≈ -1 for large z. See gpu_dielectric.rs for details.
//
// PRECISION: All f64 constants use the (zero + literal) pattern to avoid
// f32 truncation (see math_f64.wgsl gotcha #2).
// Prepend: complex_f64.wgsl (with exp_f64/sin_f64/cos_f64 polyfills)

struct DielectricParams {
    n_points: u32,
    pad0: u32,
    k: f64,
    nu: f64,
    k_debye: f64,
    v_th: f64,
    omega_p: f64,
    temperature: f64,
    density: f64,
}

@group(0) @binding(0) var<uniform> params: DielectricParams;
@group(0) @binding(1) var<storage, read> omegas: array<f64>;
@group(0) @binding(2) var<storage, read_write> loss_out: array<f64>;
@group(0) @binding(3) var<storage, read_write> dsf_out: array<f64>;

// W(z) = 1 + z·Z(z) — the Vlasov susceptibility kernel.
//
// Two branches to maintain f64 parity across CPU/GPU:
//
// |z| < 4: Power series for Z(z), then W = 1 + z·Z.
//          Cancellation is at most ~15× (|2z²Σ| < 0.97), safe for f64.
//
// |z| ≥ 4: Direct asymptotic expansion of W(z):
//          W(z) = i·z·√π·exp(-z²) - 1/(2z²) · Σ (2n+1)!! / (2z²)^n
//          No subtraction of near-equal values — naturally produces small W.
//          Converges rapidly: ratio test gives |a_{n+1}/a_n| = (2n+3)/(2z²).
//          For z=4, ratio = 0.19 at n=0 (excellent convergence).
fn plasma_w(z: Complex64) -> Complex64 {
    let zero = z.re - z.re;
    let z_abs = c64_abs(z);

    if z_abs < zero + 4.0 {
        // ── Small-z: power series for Z(z) ──
        // Z(z) = i√π exp(-z²) - 2z Σ_{n=0}^∞ (-2z²)^n / (2n+1)!!
        let z2 = c64_mul(z, z);
        let neg2z2 = c64_scale(z2, zero - 2.0);
        var term = c64_one();
        var total = term;
        for (var n = 1u; n < 80u; n++) {
            let denom = zero + f64(2u * n + 1u);
            term = c64_scale(c64_mul(term, neg2z2), (zero + 1.0) / denom);
            total = c64_add(total, term);
            if c64_abs(term) < (zero + 1e-16) * (c64_abs(total) + (zero + 1e-30)) {
                break;
            }
        }
        let exp_neg_z2 = c64_exp(c64_scale(z2, zero - 1.0));
        let sqrt_pi = zero + 1.7724538509055159;
        let imag_part = c64_scale(exp_neg_z2, sqrt_pi);
        // Z = i·√π·exp(-z²) - 2z·total
        let z_val = c64_sub(c64_new(-imag_part.im, imag_part.re),
                            c64_scale(c64_mul(z, total), zero + 2.0));
        // W = 1 + z·Z
        return c64_add(c64_one(), c64_mul(z, z_val));
    } else {
        // ── Large-z: direct asymptotic of W(z) ──
        // W(z) = i·z·√π·exp(-z²) + W_asymp
        // W_asymp = -1/(2z²) · (1 + 3/(2z²) + 15/(4z⁴) + 105/(8z⁶) + ...)
        //         = -inv2z2 · Σ_{n=0}^∞ (2n+1)!! · inv2z2^n
        //
        // Recurrence: a_0 = 1, a_{n+1} = a_n · (2n+3) · inv2z2
        let z2 = c64_mul(z, z);
        let inv_2z2 = c64_inv(c64_scale(z2, zero + 2.0));

        var coeff = c64_one();
        var total = coeff;
        for (var n = 0u; n < 30u; n++) {
            let factor = zero + f64(2u * n + 3u);
            coeff = c64_scale(c64_mul(coeff, inv_2z2), factor);
            total = c64_add(total, coeff);
            if c64_abs(coeff) < (zero + 1e-15) * (c64_abs(total) + (zero + 1e-30)) {
                break;
            }
        }
        // W_asymp = -inv2z2 · total
        let w_asymp = c64_scale(c64_mul(inv_2z2, total), zero - 1.0);

        // Exponential contribution: i·z·√π·exp(-z²)
        let exp_neg_z2 = c64_exp(c64_scale(z2, zero - 1.0));
        let sqrt_pi = zero + 1.7724538509055159;
        let zexp = c64_scale(c64_mul(z, exp_neg_z2), sqrt_pi);
        // i · zexp = (-zexp.im, zexp.re)
        let i_zexp = c64_new(-zexp.im, zexp.re);

        // sigma correction for Im(z) < 0 (Landau prescription)
        var sigma = zero + 1.0;
        if z.im < zero { sigma = zero + 2.0; }
        let w_exp = c64_scale(i_zexp, sigma);

        return c64_add(w_asymp, w_exp);
    }
}

fn chi0_classical(k_val: f64, omega: Complex64) -> Complex64 {
    let zero = k_val - k_val;
    let sqrt_2 = zero + 1.4142135623730951;
    let z = c64_scale(omega, (zero + 1.0) / (sqrt_2 * k_val * params.v_th));
    let w = plasma_w(z);
    let factor = -(params.k_debye * params.k_debye) / (k_val * k_val);
    return c64_scale(w, factor);
}

fn eps_vlasov(k_val: f64, omega: Complex64) -> Complex64 {
    return c64_sub(c64_one(), chi0_classical(k_val, omega));
}

fn eps_mermin(k_val: f64, omega_val: f64, nu_val: f64) -> Complex64 {
    let zero = k_val - k_val;
    if abs(omega_val) < zero + 1e-15 {
        return eps_vlasov(k_val, c64_zero());
    }

    let omega_c = c64_new(omega_val, zero);
    let omega_shifted = c64_new(omega_val, nu_val);
    let eps_shifted = eps_vlasov(k_val, omega_shifted);
    let eps_static = eps_vlasov(k_val, c64_zero());

    let ratio = c64_mul(c64_mul(omega_shifted, c64_inv(omega_c)),
                        c64_sub(eps_shifted, c64_one()));
    let nu_over_omega = c64_scale(c64_inv(omega_c), nu_val);
    let denom = c64_add(c64_one(),
                        c64_mul(c64_new(zero, zero + 1.0),
                                c64_mul(nu_over_omega,
                                        c64_mul(c64_sub(eps_shifted, c64_one()),
                                                c64_inv(c64_sub(eps_static, c64_one()))))));

    return c64_add(c64_one(), c64_mul(ratio, c64_inv(denom)));
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
    let idx = gid.x + gid.y * nwg.x * 64u;
    if idx >= params.n_points { return; }

    let zero = params.k - params.k;
    let omega = omegas[idx];
    let eps = eps_mermin(params.k, omega, params.nu);
    let inv_eps = c64_inv(eps);

    loss_out[idx] = inv_eps.im;

    let pi = zero + 3.14159265358979323846;
    let prefactor = params.temperature * params.k * params.k / (pi * params.density);
    if abs(omega) < zero + 1e-15 {
        dsf_out[idx] = zero;
    } else {
        dsf_out[idx] = prefactor * (-inv_eps.im) / omega;
    }
}
