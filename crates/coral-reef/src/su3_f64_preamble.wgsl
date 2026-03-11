// coralReef SU(3) lattice preamble — 3×3 unitary matrix operations for lattice QCD
//
// Provides: SU(3) matrix ops using Complex64 pairs (re, im) for f64 precision.
// Matrices are stored as array<Complex64, 9> in row-major order:
//   [m00, m01, m02, m10, m11, m12, m20, m21, m22]
//
// Prepended automatically when WGSL source uses su3_ functions.
// Requires: complex_f64_preamble.wgsl (auto-prepended by coralReef).

fn su3_identity() -> array<Complex64, 9> {
    var m: array<Complex64, 9>;
    m[0] = c64_one();  m[1] = c64_zero(); m[2] = c64_zero();
    m[3] = c64_zero(); m[4] = c64_one();  m[5] = c64_zero();
    m[6] = c64_zero(); m[7] = c64_zero(); m[8] = c64_one();
    return m;
}

fn su3_zero() -> array<Complex64, 9> {
    var m: array<Complex64, 9>;
    for (var i = 0u; i < 9u; i++) { m[i] = c64_zero(); }
    return m;
}

fn su3_mul(a: array<Complex64, 9>, b: array<Complex64, 9>) -> array<Complex64, 9> {
    var r: array<Complex64, 9>;
    for (var i = 0u; i < 3u; i++) {
        for (var j = 0u; j < 3u; j++) {
            var s = c64_zero();
            for (var k = 0u; k < 3u; k++) {
                s = c64_add(s, c64_mul(a[i * 3u + k], b[k * 3u + j]));
            }
            r[i * 3u + j] = s;
        }
    }
    return r;
}

fn su3_adj(a: array<Complex64, 9>) -> array<Complex64, 9> {
    var r: array<Complex64, 9>;
    for (var i = 0u; i < 3u; i++) {
        for (var j = 0u; j < 3u; j++) {
            r[i * 3u + j] = c64_conj(a[j * 3u + i]);
        }
    }
    return r;
}

fn su3_add(a: array<Complex64, 9>, b: array<Complex64, 9>) -> array<Complex64, 9> {
    var r: array<Complex64, 9>;
    for (var i = 0u; i < 9u; i++) { r[i] = c64_add(a[i], b[i]); }
    return r;
}

fn su3_sub(a: array<Complex64, 9>, b: array<Complex64, 9>) -> array<Complex64, 9> {
    var r: array<Complex64, 9>;
    for (var i = 0u; i < 9u; i++) { r[i] = c64_sub(a[i], b[i]); }
    return r;
}

fn su3_scale(a: array<Complex64, 9>, s: f64) -> array<Complex64, 9> {
    var r: array<Complex64, 9>;
    for (var i = 0u; i < 9u; i++) { r[i] = c64_scale(a[i], s); }
    return r;
}

fn su3_trace(a: array<Complex64, 9>) -> Complex64 {
    return c64_add(c64_add(a[0], a[4]), a[8]);
}

fn su3_re_trace(a: array<Complex64, 9>) -> f64 {
    return a[0].re + a[4].re + a[8].re;
}

// Plaquette: U_mu(x) * U_nu(x+mu) * U_mu†(x+nu) * U_nu†(x)
fn su3_plaquette(
    u_mu: array<Complex64, 9>,
    u_nu_fwd_mu: array<Complex64, 9>,
    u_mu_fwd_nu: array<Complex64, 9>,
    u_nu: array<Complex64, 9>,
) -> array<Complex64, 9> {
    return su3_mul(su3_mul(u_mu, u_nu_fwd_mu), su3_mul(su3_adj(u_mu_fwd_nu), su3_adj(u_nu)));
}

// Random SU(3) near identity using xorshift32 PRNG.
// Generates a small Hermitian perturbation H, then returns exp(i*eps*H) ≈ I + i*eps*H.
// Uses Gram-Schmidt to re-unitarize the first two rows, then cross product for row 3.
fn su3_random_near_identity(state: ptr<function, u32>, epsilon: f64) -> array<Complex64, 9> {
    var m = su3_identity();
    for (var i = 0u; i < 9u; i++) {
        let r1 = f64(xorshift32(state)) / f64(4294967295.0) - f64(0.5);
        let r2 = f64(xorshift32(state)) / f64(4294967295.0) - f64(0.5);
        m[i] = c64_add(m[i], c64_scale(c64_new(r1, r2), epsilon));
    }

    // Gram-Schmidt on rows to approximate unitarity
    // Normalize row 0
    var n0 = f64(0.0);
    for (var j = 0u; j < 3u; j++) { n0 += c64_abs_sq(m[j]); }
    n0 = f64(1.0) / sqrt(n0);
    for (var j = 0u; j < 3u; j++) { m[j] = c64_scale(m[j], n0); }

    // Row 1: subtract projection onto row 0, normalize
    var dot01 = c64_zero();
    for (var j = 0u; j < 3u; j++) { dot01 = c64_add(dot01, c64_mul(m[3u + j], c64_conj(m[j]))); }
    for (var j = 0u; j < 3u; j++) { m[3u + j] = c64_sub(m[3u + j], c64_mul(dot01, m[j])); }
    var n1 = f64(0.0);
    for (var j = 0u; j < 3u; j++) { n1 += c64_abs_sq(m[3u + j]); }
    n1 = f64(1.0) / sqrt(n1);
    for (var j = 0u; j < 3u; j++) { m[3u + j] = c64_scale(m[3u + j], n1); }

    // Row 2: cross product of conj(row0) × conj(row1) for SU(3) determinant = 1
    m[6] = c64_sub(c64_mul(c64_conj(m[1]), c64_conj(m[5])),
                   c64_mul(c64_conj(m[2]), c64_conj(m[4])));
    m[7] = c64_sub(c64_mul(c64_conj(m[2]), c64_conj(m[3])),
                   c64_mul(c64_conj(m[0]), c64_conj(m[5])));
    m[8] = c64_sub(c64_mul(c64_conj(m[0]), c64_conj(m[4])),
                   c64_mul(c64_conj(m[1]), c64_conj(m[3])));

    return m;
}
