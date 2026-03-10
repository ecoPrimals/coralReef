// coralReef f32 transcendental workaround preamble — healthSpring-inspired
//
// Polyfill implementations for f32 pow/log/exp when hardware behavior is
// unreliable (e.g. domain/overflow edge cases, driver bugs). Auto-prepended
// when WGSL source uses power_f32, log_f32_safe, or exp_f32_safe.
//
// Provenance: healthSpring f32 transcendental workaround pattern.

/// Accurate f32 power: base^exp with proper handling of edge cases.
/// Uses log/exp identity: base^exp = exp2(log2(base) * exp).
fn power_f32(base: f32, exp: f32) -> f32 {
    if base <= 0.0 {
        return select(0.0, 1.0, exp == 0.0);
    }
    return exp_f32_safe(log_f32_safe(base) * exp);
}

/// f32 natural log with domain safety (x > 0).
/// Returns -inf for x <= 0 to avoid NaN propagation.
fn log_f32_safe(x: f32) -> f32 {
    if x <= 0.0 {
        return -1.0 / 0.0; // -inf
    }
    return log(x);
}

/// f32 exp with overflow protection.
/// Clamps input to avoid inf; returns 0 for large negative, inf for large positive.
fn exp_f32_safe(x: f32) -> f32 {
    let max_in: f32 = 88.0;   // exp(88) ~ 1.7e38, below f32 max
    let min_in: f32 = -88.0;  // exp(-88) ~ 6.7e-39, above f32 min
    let clamped = clamp(x, min_in, max_in);
    return exp(clamped);
}
