// coralReef Complex64 preamble — complex arithmetic on native f64 pairs
//
// Provides: Complex64 struct + c64_* operations for scientific GPU shaders
// (plasma physics, dielectric functions, quantum lattice).
//
// Prepended automatically when WGSL source uses Complex64 or c64_ functions.
// Requires hardware f64 support (naga `enable f64` handled by coralReef).

struct Complex64 {
    re: f64,
    im: f64,
}

fn c64_zero() -> Complex64 {
    return Complex64(0.0, 0.0);
}

fn c64_one() -> Complex64 {
    return Complex64(1.0, 0.0);
}

fn c64_new(re: f64, im: f64) -> Complex64 {
    return Complex64(re, im);
}

fn c64_conj(z: Complex64) -> Complex64 {
    return Complex64(z.re, -z.im);
}

fn c64_add(a: Complex64, b: Complex64) -> Complex64 {
    return Complex64(a.re + b.re, a.im + b.im);
}

fn c64_sub(a: Complex64, b: Complex64) -> Complex64 {
    return Complex64(a.re - b.re, a.im - b.im);
}

fn c64_mul(a: Complex64, b: Complex64) -> Complex64 {
    return Complex64(
        a.re * b.re - a.im * b.im,
        a.re * b.im + a.im * b.re,
    );
}

fn c64_scale(z: Complex64, s: f64) -> Complex64 {
    return Complex64(z.re * s, z.im * s);
}

fn c64_abs(z: Complex64) -> f64 {
    return sqrt(z.re * z.re + z.im * z.im);
}

fn c64_abs_sq(z: Complex64) -> f64 {
    return z.re * z.re + z.im * z.im;
}

fn c64_inv(z: Complex64) -> Complex64 {
    let d = c64_abs_sq(z);
    return Complex64(z.re / d, -z.im / d);
}

fn c64_div(a: Complex64, b: Complex64) -> Complex64 {
    return c64_mul(a, c64_inv(b));
}

fn c64_exp(z: Complex64) -> Complex64 {
    // exp(a + bi) = exp(a) * (cos(b) + i*sin(b))
    let ea = exp(z.re);
    return Complex64(ea * cos(z.im), ea * sin(z.im));
}

fn c64_log(z: Complex64) -> Complex64 {
    // ln(z) = ln|z| + i*arg(z)
    return Complex64(log(c64_abs(z)), atan2(z.im, z.re));
}

fn c64_sqrt(z: Complex64) -> Complex64 {
    let r = c64_abs(z);
    if r < 1e-300 { return c64_zero(); }
    let half = z.re - z.re + 0.5;
    let t = sqrt(half * (r + abs(z.re)));
    if z.re >= z.re - z.re {
        return Complex64(t, z.im / (t + t));
    } else {
        let sign_im = select(-1.0, 1.0, z.im >= z.im - z.im);
        return Complex64(abs(z.im) / (t + t), sign_im * t);
    }
}

fn c64_pow(base: Complex64, exp: Complex64) -> Complex64 {
    // z^w = exp(w * ln(z))
    return c64_exp(c64_mul(exp, c64_log(base)));
}
