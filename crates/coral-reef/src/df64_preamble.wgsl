// coralReef df64 preamble — double-float (~48-bit) arithmetic via f32 pairs
//
// Algorithms: Knuth two-sum, Dekker multiplication (with FMA),
// error-free transformation. All operations use only f32 hardware.
//
// This preamble is prepended automatically when Fp64Strategy::DoubleFloat
// is selected, or when compiling shaders that use the df64 API.

struct Df64 {
    hi: f32,
    lo: f32,
}

// ── Constructors / conversions ──────────────────────────────────────

fn df64_zero() -> Df64 {
    return Df64(0.0, 0.0);
}

fn df64_from_f32(a: f32) -> Df64 {
    return Df64(a, 0.0);
}

fn df64_from_f64(a: f64) -> Df64 {
    let hi = f32(a);
    let lo = f32(a - f64(hi));
    return Df64(hi, lo);
}

fn df64_to_f64(a: Df64) -> f64 {
    return f64(a.hi) + f64(a.lo);
}

// ── Core primitives (error-free transformations) ────────────────────

// Knuth two-sum: exact a + b = s + err
fn two_sum(a: f32, b: f32) -> Df64 {
    let s = a + b;
    let v = s - a;
    let err = (a - (s - v)) + (b - v);
    return Df64(s, err);
}

// Fast two-sum: requires |a| >= |b|
fn fast_two_sum(a: f32, b: f32) -> Df64 {
    let s = a + b;
    let err = b - (s - a);
    return Df64(s, err);
}

// Dekker two-product via FMA: exact a * b = p + err
fn two_prod(a: f32, b: f32) -> Df64 {
    let p = a * b;
    let err = fma(a, b, -p);
    return Df64(p, err);
}

// ── Arithmetic ──────────────────────────────────────────────────────

fn df64_neg(a: Df64) -> Df64 {
    return Df64(-a.hi, -a.lo);
}

fn df64_add(a: Df64, b: Df64) -> Df64 {
    var s = two_sum(a.hi, b.hi);
    var t = two_sum(a.lo, b.lo);
    s.lo += t.hi;
    s = fast_two_sum(s.hi, s.lo);
    s.lo += t.lo;
    return fast_two_sum(s.hi, s.lo);
}

fn df64_sub(a: Df64, b: Df64) -> Df64 {
    return df64_add(a, df64_neg(b));
}

fn df64_mul(a: Df64, b: Df64) -> Df64 {
    var p = two_prod(a.hi, b.hi);
    p.lo += a.hi * b.lo + a.lo * b.hi;
    return fast_two_sum(p.hi, p.lo);
}

fn df64_div(a: Df64, b: Df64) -> Df64 {
    let q1 = a.hi / b.hi;
    let r = df64_sub(a, df64_mul(df64_from_f32(q1), b));
    let q2 = r.hi / b.hi;
    let r2 = df64_sub(r, df64_mul(df64_from_f32(q2), b));
    let q3 = r2.hi / b.hi;
    var result = fast_two_sum(q1, q2);
    return df64_add(result, df64_from_f32(q3));
}

// ── Transcendentals ─────────────────────────────────────────────────

fn sqrt_df64(a: Df64) -> Df64 {
    if a.hi <= 0.0 { return df64_zero(); }
    let x = 1.0 / sqrt(a.hi);
    let y = a.hi * x;
    let diff = df64_sub(a, df64_mul(df64_from_f32(y), df64_from_f32(y)));
    return df64_add(df64_from_f32(y), df64_from_f32(diff.hi * (x * 0.5)));
}

fn exp_df64(a: Df64) -> Df64 {
    let ln2_hi: f32 = 0.6931471805599453;
    let ln2_lo: f32 = 2.3190468138462996e-8;

    let k_f = round(a.hi / ln2_hi);
    let r = df64_sub(a, df64_add(
        df64_from_f32(k_f * ln2_hi),
        df64_from_f32(k_f * ln2_lo),
    ));

    // Horner minimax polynomial for exp(r) on [-ln2/2, ln2/2]
    let c6: f32 = 1.0 / 720.0;
    let c5: f32 = 1.0 / 120.0;
    let c4: f32 = 1.0 / 24.0;
    let c3: f32 = 1.0 / 6.0;
    let c2: f32 = 0.5;

    var p = df64_from_f32(c6);
    p = df64_add(df64_mul(p, r), df64_from_f32(c5));
    p = df64_add(df64_mul(p, r), df64_from_f32(c4));
    p = df64_add(df64_mul(p, r), df64_from_f32(c3));
    p = df64_add(df64_mul(p, r), df64_from_f32(c2));
    p = df64_add(df64_mul(p, r), df64_from_f32(1.0));
    p = df64_add(df64_mul(p, r), df64_from_f32(1.0));

    // Scale by 2^k using exp2 (avoids ldexp which isn't wired yet)
    let scale = exp2(k_f);
    return df64_mul(p, df64_from_f32(scale));
}

fn df64_gt(a: Df64, b: Df64) -> bool {
    return a.hi > b.hi || (a.hi == b.hi && a.lo > b.lo);
}

fn df64_lt(a: Df64, b: Df64) -> bool {
    return a.hi < b.hi || (a.hi == b.hi && a.lo < b.lo);
}

fn df64_ge(a: Df64, b: Df64) -> bool {
    return a.hi > b.hi || (a.hi == b.hi && a.lo >= b.lo);
}

fn tanh_df64(a: Df64) -> Df64 {
    // tanh(x) = (exp(2x) - 1) / (exp(2x) + 1)
    // For large |x|, clamp to ±1 for stability
    if a.hi > 20.0 { return df64_from_f32(1.0); }
    if a.hi < -20.0 { return df64_from_f32(-1.0); }

    let two_x = df64_add(a, a);
    let e2x = exp_df64(two_x);
    let one = df64_from_f32(1.0);
    return df64_div(df64_sub(e2x, one), df64_add(e2x, one));
}
