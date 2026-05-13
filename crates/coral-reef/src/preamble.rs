// SPDX-License-Identifier: AGPL-3.0-or-later
//! WGSL preamble injection — auto-prepend domain-specific function libraries.
//!
//! Detects usage of domain types (Complex64, df64, SU3, PRNG, f32 transcendentals)
//! and prepends the corresponding WGSL preamble. Dependencies are auto-chained
//! (e.g. SU3 → Complex64 + PRNG).

use std::borrow::Cow;

use crate::{
    COMPLEX64_PREAMBLE, CompileOptions, DF64_PREAMBLE, F32_TRANSCENDENTAL_PREAMBLE, Fp64Strategy,
    PRNG_PREAMBLE, SU3_PREAMBLE,
};

pub fn strip_enable_directives(source: &str) -> String {
    source
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.starts_with("enable f64") && !trimmed.starts_with("enable f16")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Prepare WGSL source: auto-prepend preambles when needed,
/// strip `enable f64;` (naga handles f64 natively).
///
/// Preamble injection order (dependencies chain forward):
///   1. Complex64 (no deps)
///   2. PRNG (no deps)
///   3. SU3 (depends on Complex64 + PRNG — auto-chained)
///   4. df64 (no deps)
///   5. f32 transcendental (no deps)
///
/// Returns a `Cow::Borrowed` if no transformations are needed, avoiding allocation.
pub fn prepare_wgsl<'a>(wgsl: &'a str, options: &CompileOptions) -> Cow<'a, str> {
    let needs_df64 = options.fp64_strategy == Fp64Strategy::DoubleFloat
        || wgsl.contains("Df64")
        || wgsl.contains("df64_");
    let needs_complex64 = wgsl.contains("Complex64") || wgsl.contains("c64_");
    let needs_f32_transcendental = wgsl.contains("power_f32")
        || wgsl.contains("log_f32_safe")
        || wgsl.contains("exp_f32_safe");
    let needs_prng = wgsl.contains("xorshift32") || wgsl.contains("wang_hash");
    let needs_su3 = wgsl.contains("su3_");
    let has_enable_f64 = wgsl.contains("enable f64");
    let has_enable_f16 = wgsl.contains("enable f16");

    let needs_complex64 = needs_complex64 || needs_su3;
    let needs_prng = needs_prng || needs_su3;

    if !needs_df64
        && !needs_complex64
        && !needs_f32_transcendental
        && !needs_prng
        && !needs_su3
        && !has_enable_f64
        && !has_enable_f16
    {
        return Cow::Borrowed(wgsl);
    }

    let source = wgsl;
    let mut combined = String::new();

    let modified = if needs_complex64 && !source.contains("struct Complex64") {
        tracing::debug!("auto-prepending complex64 preamble");
        combined.reserve(
            COMPLEX64_PREAMBLE.len()
                + PRNG_PREAMBLE.len()
                + SU3_PREAMBLE.len()
                + DF64_PREAMBLE.len()
                + F32_TRANSCENDENTAL_PREAMBLE.len()
                + 8
                + source.len(),
        );
        combined.push_str(COMPLEX64_PREAMBLE);
        combined.push('\n');
        true
    } else {
        false
    };

    let modified = if needs_prng && !source.contains("fn xorshift32") {
        tracing::debug!("auto-prepending PRNG preamble");
        if !modified {
            combined.reserve(PRNG_PREAMBLE.len() + 1 + source.len());
        }
        combined.push_str(PRNG_PREAMBLE);
        combined.push('\n');
        true
    } else {
        modified
    };

    let modified = if needs_su3 && !source.contains("fn su3_identity") {
        tracing::debug!("auto-prepending SU3 lattice preamble");
        if !modified {
            combined.reserve(SU3_PREAMBLE.len() + 1 + source.len());
        }
        combined.push_str(SU3_PREAMBLE);
        combined.push('\n');
        true
    } else {
        modified
    };

    let modified = if needs_df64 && !source.contains("struct Df64") {
        tracing::debug!("auto-prepending df64 preamble");
        if !modified {
            combined.reserve(
                DF64_PREAMBLE.len() + F32_TRANSCENDENTAL_PREAMBLE.len() + 2 + source.len(),
            );
        }
        combined.push_str(DF64_PREAMBLE);
        combined.push('\n');
        true
    } else {
        modified
    };

    let modified = if needs_f32_transcendental && !source.contains("fn power_f32") {
        tracing::debug!("auto-prepending f32 transcendental preamble");
        if !modified {
            combined.reserve(F32_TRANSCENDENTAL_PREAMBLE.len() + 1 + source.len());
        }
        combined.push_str(F32_TRANSCENDENTAL_PREAMBLE);
        combined.push('\n');
        true
    } else {
        modified
    };

    let result = if modified {
        combined.push_str(source);
        combined
    } else {
        source.to_owned()
    };

    let result = if has_enable_f64 || has_enable_f16 {
        strip_enable_directives(&result)
    } else {
        result
    };

    Cow::Owned(result)
}
