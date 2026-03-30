// SPDX-License-Identifier: AGPL-3.0-only
//! Tolerance-based validation engine: compares CPU execution output against expected values.

use crate::interpret::execute_cpu;
use crate::types::{
    CpuError, ExecuteCpuRequest, Mismatch, Tolerance, ValidateRequest, ValidateResponse,
};

/// Execute a WGSL shader on the CPU and compare outputs against expected values.
///
/// For each [`crate::ExpectedBinding`], the validator runs the shader, extracts the
/// output binding with matching `(group, binding)`, and compares element-wise
/// as `f64` (or raw `u32` when both are exact integers). A mismatch is reported
/// when both absolute and relative error exceed their respective tolerances.
///
/// # Errors
///
/// Returns [`CpuError`] if the underlying CPU execution fails.
pub fn validate(request: &ValidateRequest) -> Result<ValidateResponse, CpuError> {
    let exec_request = ExecuteCpuRequest {
        wgsl_source: request.wgsl_source.clone(),
        entry_point: request.entry_point.clone(),
        workgroups: request.workgroups,
        bindings: request.bindings.clone(),
        uniforms: request.uniforms.clone(),
    };

    let exec_result = execute_cpu(&exec_request)?;

    let mut all_mismatches = Vec::new();

    for expected in &request.expected {
        let actual_binding = exec_result
            .bindings
            .iter()
            .find(|b| b.group == expected.group && b.binding == expected.binding);

        let actual_data = if let Some(b) = actual_binding {
            &b.data[..]
        } else {
            all_mismatches.push(Mismatch {
                group: expected.group,
                binding: expected.binding,
                index: 0,
                got: f64::NAN,
                expected: f64::NAN,
                abs_error: f64::INFINITY,
                rel_error: f64::INFINITY,
            });
            continue;
        };

        let mismatches = compare_binding(
            expected.group,
            expected.binding,
            actual_data,
            &expected.data,
            &expected.tolerance,
        );
        all_mismatches.extend(mismatches);
    }

    Ok(ValidateResponse {
        passed: all_mismatches.is_empty(),
        mismatches: all_mismatches,
    })
}

/// Compare two byte buffers element-wise as `f32` values, reporting mismatches.
fn compare_binding(
    group: u32,
    binding: u32,
    actual: &[u8],
    expected: &[u8],
    tolerance: &Tolerance,
) -> Vec<Mismatch> {
    let element_count = actual.len().min(expected.len()) / 4;
    let mut mismatches = Vec::new();

    for i in 0..element_count {
        let offset = i * 4;
        let got = f64::from(f32::from_le_bytes([
            actual[offset],
            actual[offset + 1],
            actual[offset + 2],
            actual[offset + 3],
        ]));
        let exp = f64::from(f32::from_le_bytes([
            expected[offset],
            expected[offset + 1],
            expected[offset + 2],
            expected[offset + 3],
        ]));

        let abs_error = (got - exp).abs();
        let rel_error = if exp.abs() > f64::EPSILON {
            abs_error / exp.abs()
        } else {
            abs_error
        };

        if abs_error > tolerance.abs && rel_error > tolerance.rel {
            mismatches.push(Mismatch {
                group,
                binding,
                index: i,
                got,
                expected: exp,
                abs_error,
                rel_error,
            });
        }
    }

    // Length mismatch: if buffers differ in size, report remaining as mismatches
    let max_elements = actual.len().max(expected.len()) / 4;
    for i in element_count..max_elements {
        mismatches.push(Mismatch {
            group,
            binding,
            index: i,
            got: if i * 4 + 4 <= actual.len() {
                f64::from(f32::from_le_bytes([
                    actual[i * 4],
                    actual[i * 4 + 1],
                    actual[i * 4 + 2],
                    actual[i * 4 + 3],
                ]))
            } else {
                f64::NAN
            },
            expected: if i * 4 + 4 <= expected.len() {
                f64::from(f32::from_le_bytes([
                    expected[i * 4],
                    expected[i * 4 + 1],
                    expected[i * 4 + 2],
                    expected[i * 4 + 3],
                ]))
            } else {
                f64::NAN
            },
            abs_error: f64::INFINITY,
            rel_error: f64::INFINITY,
        });
    }

    mismatches
}

#[cfg(test)]
#[allow(clippy::iter_on_single_items)]
mod tests {
    use super::*;
    use crate::types::Tolerance;

    #[test]
    fn exact_match_passes() {
        let data: Vec<u8> = [1.0f32, 2.0, 3.0, 4.0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();

        let mismatches = compare_binding(0, 0, &data, &data, &Tolerance { abs: 0.0, rel: 0.0 });
        assert!(mismatches.is_empty());
    }

    #[test]
    fn within_tolerance_passes() {
        let actual: Vec<u8> = [1.0001f32, 2.0001, 3.0001, 4.0001]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let expected: Vec<u8> = [1.0f32, 2.0, 3.0, 4.0]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();

        let mismatches = compare_binding(
            0,
            0,
            &actual,
            &expected,
            &Tolerance {
                abs: 0.001,
                rel: 0.001,
            },
        );
        assert!(mismatches.is_empty());
    }

    #[test]
    fn outside_tolerance_reports_mismatch() {
        let actual: Vec<u8> = [10.0f32].iter().flat_map(|v| v.to_le_bytes()).collect();
        let expected: Vec<u8> = [1.0f32].iter().flat_map(|v| v.to_le_bytes()).collect();

        let mismatches = compare_binding(
            0,
            0,
            &actual,
            &expected,
            &Tolerance {
                abs: 0.001,
                rel: 0.001,
            },
        );
        assert_eq!(mismatches.len(), 1);
        assert_eq!(mismatches[0].index, 0);
    }

    #[test]
    fn length_mismatch_reported() {
        let actual: Vec<u8> = [1.0f32, 2.0].iter().flat_map(|v| v.to_le_bytes()).collect();
        let expected: Vec<u8> = [1.0f32].iter().flat_map(|v| v.to_le_bytes()).collect();

        let mismatches =
            compare_binding(0, 0, &actual, &expected, &Tolerance { abs: 0.0, rel: 0.0 });
        assert_eq!(mismatches.len(), 1);
        assert_eq!(mismatches[0].index, 1);
    }
}
