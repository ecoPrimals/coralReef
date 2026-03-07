// SPDX-License-Identifier: AGPL-3.0-only
//! Named tolerance tiers for precision validation.
//!
//! Adopted from groundSpring's 13-tier tolerance architecture.
//! Each tier represents a distinct mathematical regime with an associated
//! epsilon value.  Using named constants instead of raw floats makes
//! precision requirements explicit and self-documenting across the
//! coralReef test suite and any downstream validation harnesses.
//!
//! # Tier ordering (tightest → loosest)
//!
//! | Tier | Epsilon | Regime |
//! |------|---------|--------|
//! | `DETERMINISM` | 1e-15 | Same seed, same path — only IEEE 754 rounding |
//! | `STRICT` | 1e-14 | Compensated / extended precision |
//! | `EXACT` | 1e-12 | Pure f64 summation paths |
//! | `ANALYTICAL` | 1e-10 | One transcendental (sqrt, ln) — ~1 ULP |
//! | `INTEGRATION` | 1e-8 | ODE RK4 O(dt⁴) accumulation |
//! | `CDF_APPROX` | 1e-6 | CDF/erf approximation |
//! | `ROUNDTRIP` | 1e-5 | CDF↔PPF round-trip |
//! | `RECONSTRUCTION` | 1e-4 | Spectral Tikhonov roundtrip RMSE |
//! | `LITERATURE` | 0.001 | Published 3–4 significant figures |
//! | `DECOMPOSITION` | 0.005 | Pythagorean identity decomposition |
//! | `STOCHASTIC` | 0.01 | O(1/√N) sampling noise |
//! | `NORM_2PCT` | 0.02 | ~2% normalization tolerance |
//! | `EQUILIBRIUM` | 0.1 | ODE steady-state / measurement precision |

/// Same seed, same path — only IEEE 754 rounding separates results.
pub const DETERMINISM: f64 = 1e-15;

/// Compensated arithmetic / extended precision (e.g. Kahan summation).
pub const STRICT: f64 = 1e-14;

/// Pure f64 summation paths, no transcendentals.
pub const EXACT: f64 = 1e-12;

/// Single transcendental (sqrt, ln, exp) — approximately 1 ULP.
/// This is the target tier for coralReef f64 software lowering
/// (Newton-Raphson, DFMA polynomial).
pub const ANALYTICAL: f64 = 1e-10;

/// ODE RK4 O(dt⁴) accumulation over many timesteps.
pub const INTEGRATION: f64 = 1e-8;

/// CDF/erf approximation (Abramowitz & Stegun 7.1.26 class).
pub const CDF_APPROX: f64 = 1e-6;

/// CDF↔PPF round-trip (both approximations stacked).
pub const ROUNDTRIP: f64 = 1e-5;

/// Spectral Tikhonov regularization roundtrip RMSE.
pub const RECONSTRUCTION: f64 = 1e-4;

/// Published results with 3–4 significant figures.
pub const LITERATURE: f64 = 0.001;

/// Pythagorean identity: RMSE² = MBE² + σ².
pub const DECOMPOSITION: f64 = 0.005;

/// O(1/√N) sampling noise floor.
pub const STOCHASTIC: f64 = 0.01;

/// ~2% normalization / integral tolerance.
pub const NORM_2PCT: f64 = 0.02;

/// ODE steady-state or measurement precision.
pub const EQUILIBRIUM: f64 = 0.1;

/// Production epsilon guards for division and underflow.
pub mod eps {
    /// Safe divisor floor — prevents NaN from `x / y.max(SAFE_DIV)`.
    pub const SAFE_DIV: f64 = 1e-10;

    /// Underflow guard for condition numbers and matrix elements.
    pub const UNDERFLOW: f64 = 1e-300;
}

/// Compare two f64 values within a tolerance tier.
///
/// Uses relative difference when the reference is large enough,
/// falls back to absolute difference near zero.
#[inline]
#[must_use]
pub fn within(reference: f64, actual: f64, tier: f64) -> bool {
    let diff = if reference.abs() > eps::SAFE_DIV {
        ((actual - reference) / reference).abs()
    } else {
        (actual - reference).abs()
    };
    diff < tier
}

/// Summary of a bulk comparison between two value slices.
#[derive(Debug, Clone)]
pub struct ComparisonSummary {
    /// Total number of elements compared.
    pub total: usize,
    /// Number of elements within the tolerance tier.
    pub passed: usize,
    /// Number of elements exceeding the tolerance tier.
    pub failed: usize,
    /// Maximum relative difference across all elements.
    pub max_diff: f64,
    /// Mean relative difference across all elements.
    pub mean_diff: f64,
}

/// Compare two slices element-wise against a tolerance tier.
///
/// # Panics
///
/// Panics if `reference` and `actual` have different lengths.
#[must_use]
pub fn compare_all(reference: &[f64], actual: &[f64], tier: f64) -> ComparisonSummary {
    assert_eq!(reference.len(), actual.len());
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut max_diff = 0.0f64;
    let mut sum_diff = 0.0f64;

    for (&r, &a) in reference.iter().zip(actual.iter()) {
        let diff = if r.abs() > eps::SAFE_DIV {
            ((a - r) / r).abs()
        } else {
            (a - r).abs()
        };
        if diff < tier {
            passed += 1;
        } else {
            failed += 1;
        }
        max_diff = max_diff.max(diff);
        sum_diff += diff;
    }

    let total = reference.len();
    ComparisonSummary {
        total,
        passed,
        failed,
        max_diff,
        #[expect(
            clippy::cast_precision_loss,
            reason = "tolerance comparison: precision loss is acceptable for ULP comparison"
        )]
        mean_diff: if total > 0 {
            sum_diff / total as f64
        } else {
            0.0
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_ordering() {
        let tiers = [
            DETERMINISM,
            STRICT,
            EXACT,
            ANALYTICAL,
            INTEGRATION,
            CDF_APPROX,
            ROUNDTRIP,
            RECONSTRUCTION,
            LITERATURE,
            DECOMPOSITION,
            STOCHASTIC,
            NORM_2PCT,
            EQUILIBRIUM,
        ];
        for pair in tiers.windows(2) {
            assert!(
                pair[0] < pair[1],
                "tier ordering violated: {} >= {}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn within_passes_for_close_values() {
        assert!(within(1.0, 1.0 + 1e-11, ANALYTICAL));
        assert!(!within(1.0, 1.0 + 1e-9, ANALYTICAL));
    }

    #[test]
    fn within_handles_zero_reference() {
        assert!(within(0.0, 1e-16, DETERMINISM));
        assert!(!within(0.0, 1e-14, DETERMINISM));
    }

    #[test]
    fn compare_all_summary() {
        let refs = vec![1.0, 2.0, 3.0];
        let actual = vec![1.0 + 1e-13, 2.0 + 1e-13, 3.0 + 1.0];
        let summary = compare_all(&refs, &actual, EXACT);
        assert_eq!(summary.passed, 2);
        assert_eq!(summary.failed, 1);
        assert!(summary.max_diff > 0.3);
    }
}
