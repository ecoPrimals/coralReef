// SPDX-License-Identifier: AGPL-3.0-only
//! CPU compilation and validation service handlers.
//!
//! Delegates to `coral_reef_cpu` for interpretation and `coral_reef_jit` for
//! Cranelift JIT execution. Supports the progressive trust model:
//! - **Interpret**: reference executor only
//! - **Jit**: Cranelift JIT only (default)
//! - **`ValidatedJit`**: interpret-validate first, JIT-cache on success,
//!   periodic re-validation for drift detection

use std::sync::LazyLock;

use coral_reef::CompileError;
use coral_reef_cpu::types::{
    CompileCpuRequest, CpuError, DualPathResult, ExecuteCpuRequest, ExecuteCpuResponse,
    ExecutionStrategy, Mismatch, Tolerance, ValidateRequest, ValidateResponse,
};
use coral_reef_jit::cache::JitCache;

use super::types::CompileResponse;

/// Global JIT compilation cache for the progressive trust model.
static JIT_CACHE: LazyLock<JitCache> = LazyLock::new(JitCache::new);

/// `shader.compile.cpu` — compile WGSL for CPU execution.
///
/// In Phase 1 (interpreter), this parses and validates the WGSL module without
/// generating native code. Returns a sentinel binary indicating the module is
/// ready for `shader.execute.cpu`.
///
/// # Errors
///
/// Returns [`CompileError`] on WGSL parse or validation failures.
pub fn handle_compile_cpu(request: &CompileCpuRequest) -> Result<CompileResponse, CompileError> {
    let module = naga::front::wgsl::parse_str(&request.wgsl_source)
        .map_err(|e| CompileError::InvalidInput(format!("WGSL parse: {e}").into()))?;

    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator
        .validate(&module)
        .map_err(|e| CompileError::InvalidInput(format!("naga validation: {e}").into()))?;

    let entry_name = request.entry_point.as_deref();
    let has_entry = module.entry_points.iter().any(|ep| {
        ep.stage == naga::ShaderStage::Compute && entry_name.is_none_or(|name| ep.name == name)
    });
    if !has_entry {
        return Err(CompileError::InvalidInput(
            "no matching @compute entry point".into(),
        ));
    }

    let sentinel = b"coral-reef-cpu:interpreted";
    Ok(CompileResponse {
        binary: bytes::Bytes::from_static(sentinel),
        size: sentinel.len(),
        arch: Some(request.arch.clone()),
        status: Some("validated".into()),
    })
}

/// `shader.execute.cpu` — execute a WGSL compute shader on the CPU.
///
/// Dispatches based on the request's [`ExecutionStrategy`]:
/// - `Interpret`: Naga IR reference interpreter
/// - `Jit`: Cranelift JIT (default)
/// - `ValidatedJit`: interpret-validate first, then JIT-cache on success
///
/// # Errors
///
/// Returns [`CompileError`] wrapping the underlying execution error.
pub fn handle_execute_cpu(request: &ExecuteCpuRequest) -> Result<ExecuteCpuResponse, CompileError> {
    match request.strategy {
        ExecutionStrategy::Interpret => execute_interpret(request),
        ExecutionStrategy::Jit => execute_jit_direct(request),
        ExecutionStrategy::ValidatedJit => execute_validated_jit(request),
    }
}

/// Pure interpreter path — no JIT, no caching.
fn execute_interpret(request: &ExecuteCpuRequest) -> Result<ExecuteCpuResponse, CompileError> {
    let mut resp = coral_reef_cpu::execute_cpu(request).map_err(cpu_error_to_compile_error)?;
    resp.strategy_used = Some(ExecutionStrategy::Interpret);
    Ok(resp)
}

/// Direct JIT path — compile and execute without validation.
fn execute_jit_direct(request: &ExecuteCpuRequest) -> Result<ExecuteCpuResponse, CompileError> {
    let mut resp = coral_reef_jit::execute_jit(request).map_err(jit_error_to_compile_error)?;
    resp.strategy_used = Some(ExecutionStrategy::Jit);
    Ok(resp)
}

/// Validated-JIT path: interpret-validate first, then JIT-cache on success.
///
/// 1. Check the JIT cache for a previously validated kernel.
/// 2. If cached + validated: execute via JIT, periodically re-validate.
/// 3. If not cached: interpret first, JIT second, compare within tolerance.
///    On success, cache the JIT kernel as validated.
fn execute_validated_jit(request: &ExecuteCpuRequest) -> Result<ExecuteCpuResponse, CompileError> {
    let cache = &*JIT_CACHE;

    let (kernel, cache_hit, needs_revalidation) =
        coral_reef_jit::cache::compile_cached(cache, request)
            .map_err(jit_error_to_compile_error)?;

    if cache_hit && !needs_revalidation {
        let mut resp =
            coral_reef_jit::execute_kernel(&kernel, request).map_err(jit_error_to_compile_error)?;
        resp.strategy_used = Some(ExecutionStrategy::ValidatedJit);
        resp.cache_hit = true;
        return Ok(resp);
    }

    let interp_result = coral_reef_cpu::execute_cpu(request).map_err(cpu_error_to_compile_error)?;

    let jit_result =
        coral_reef_jit::execute_kernel(&kernel, request).map_err(jit_error_to_compile_error)?;

    let tolerance = Tolerance {
        abs: 1e-5,
        rel: 1e-5,
    };
    let mismatches =
        compare_path_outputs(&interp_result.bindings, &jit_result.bindings, &tolerance);

    if mismatches.is_empty() {
        cache.mark_validated(request);
        tracing::info!(
            cache_hit,
            revalidated = needs_revalidation,
            "validated-jit: paths agree, caching kernel"
        );
        let mut resp = jit_result;
        resp.strategy_used = Some(ExecutionStrategy::ValidatedJit);
        resp.cache_hit = cache_hit;
        resp.revalidated = needs_revalidation;
        Ok(resp)
    } else {
        cache.invalidate(request);
        tracing::warn!(
            mismatches = mismatches.len(),
            "validated-jit: paths diverge, falling back to interpreter"
        );
        let mut resp = interp_result;
        resp.strategy_used = Some(ExecutionStrategy::Interpret);
        resp.revalidated = needs_revalidation;
        Ok(resp)
    }
}

/// `shader.validate` — execute on CPU and compare against expected values.
///
/// Runs dual-path validation when the Cranelift JIT backend is available:
/// - **Path A** (Naga interpreter): reference oracle via `coral_reef_cpu`
/// - **Path B** (Cranelift JIT): optimized `CoralIR` → native code via `coral_reef_jit`
///
/// Both paths execute independently; if Path B fails (unsupported ops, etc.),
/// validation still succeeds based on Path A alone with a note in `dual_path`.
///
/// # Errors
///
/// Returns [`CompileError`] wrapping the underlying [`CpuError`].
pub fn handle_validate(request: &ValidateRequest) -> Result<ValidateResponse, CompileError> {
    let mut response = coral_reef_cpu::validate(request).map_err(cpu_error_to_compile_error)?;

    let dual_path = run_dual_path_validation(request, &response);
    response.dual_path = Some(dual_path);

    Ok(response)
}

/// Run both execution paths and compare their outputs.
fn run_dual_path_validation(
    request: &ValidateRequest,
    path_a_response: &ValidateResponse,
) -> DualPathResult {
    let exec_request = ExecuteCpuRequest {
        wgsl_source: request.wgsl_source.clone(),
        entry_point: request.entry_point.clone(),
        workgroups: request.workgroups,
        bindings: request.bindings.clone(),
        uniforms: request.uniforms.clone(),
        strategy: ExecutionStrategy::Jit,
    };

    let path_a_result = coral_reef_cpu::execute_cpu(&exec_request);
    let path_b_result = coral_reef_jit::execute_jit(&exec_request);

    let path_a_ns = path_a_result.as_ref().map_or(0, |r| r.execution_time_ns);

    match (path_a_result, path_b_result) {
        (Ok(a_resp), Ok(b_resp)) => {
            let tolerance = Tolerance {
                abs: 1e-5,
                rel: 1e-5,
            };
            let mismatches = compare_path_outputs(&a_resp.bindings, &b_resp.bindings, &tolerance);
            DualPathResult {
                paths_agree: mismatches.is_empty(),
                path_mismatches: mismatches,
                path_a_ns,
                path_b_ns: b_resp.execution_time_ns,
                note: None,
            }
        }
        (Ok(_), Err(jit_err)) => DualPathResult {
            paths_agree: path_a_response.passed,
            path_mismatches: vec![],
            path_a_ns,
            path_b_ns: 0,
            note: Some(format!("Path B (Cranelift) failed: {jit_err}")),
        },
        (Err(cpu_err), _) => DualPathResult {
            paths_agree: false,
            path_mismatches: vec![],
            path_a_ns: 0,
            path_b_ns: 0,
            note: Some(format!(
                "Path A (interpreter) re-execution failed: {cpu_err}"
            )),
        },
    }
}

/// Compare binding outputs from Path A and Path B element-wise as f32.
fn compare_path_outputs(
    path_a: &[coral_reef_cpu::BindingData],
    path_b: &[coral_reef_cpu::BindingData],
    tolerance: &Tolerance,
) -> Vec<Mismatch> {
    let mut mismatches = Vec::new();

    for a_binding in path_a {
        let b_binding = path_b
            .iter()
            .find(|b| b.group == a_binding.group && b.binding == a_binding.binding);

        let Some(b_binding) = b_binding else {
            mismatches.push(Mismatch {
                group: a_binding.group,
                binding: a_binding.binding,
                index: 0,
                got: f64::NAN,
                expected: f64::NAN,
                abs_error: f64::INFINITY,
                rel_error: f64::INFINITY,
            });
            continue;
        };

        let a_data = &a_binding.data[..];
        let b_data = &b_binding.data[..];
        let element_count = a_data.len().min(b_data.len()) / 4;

        for i in 0..element_count {
            let offset = i * 4;
            let a_val = f64::from(f32::from_le_bytes([
                a_data[offset],
                a_data[offset + 1],
                a_data[offset + 2],
                a_data[offset + 3],
            ]));
            let b_val = f64::from(f32::from_le_bytes([
                b_data[offset],
                b_data[offset + 1],
                b_data[offset + 2],
                b_data[offset + 3],
            ]));

            let abs_error = (a_val - b_val).abs();
            let rel_error = if a_val.abs() > f64::EPSILON {
                abs_error / a_val.abs()
            } else {
                abs_error
            };

            if abs_error > tolerance.abs && rel_error > tolerance.rel {
                mismatches.push(Mismatch {
                    group: a_binding.group,
                    binding: a_binding.binding,
                    index: i,
                    got: b_val,
                    expected: a_val,
                    abs_error,
                    rel_error,
                });
            }
        }
    }

    mismatches
}

#[expect(clippy::needless_pass_by_value, reason = "used as map_err closure")]
fn jit_error_to_compile_error(e: coral_reef_jit::error::JitError) -> CompileError {
    CompileError::Internal(e.to_string().into())
}

fn cpu_error_to_compile_error(e: CpuError) -> CompileError {
    match e {
        CpuError::Parse(msg) | CpuError::Validation(msg) => CompileError::InvalidInput(msg.into()),
        CpuError::EntryPointNotFound(name) => {
            CompileError::InvalidInput(format!("entry point not found: {name}").into())
        }
        CpuError::Unsupported(msg) => CompileError::NotImplemented(msg.into()),
        CpuError::MissingBinding { group, binding } => CompileError::InvalidInput(
            format!("missing binding (group={group}, binding={binding})").into(),
        ),
        CpuError::Internal(msg) => CompileError::Internal(msg.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use coral_reef_cpu::types::{BindingData, BindingUsage, Tolerance};

    #[test]
    fn compile_cpu_valid_wgsl() {
        let req = CompileCpuRequest {
            wgsl_source: "@compute @workgroup_size(1) fn main() {}".into(),
            arch: std::env::consts::ARCH.into(),
            opt_level: 0,
            entry_point: None,
        };
        let resp = handle_compile_cpu(&req).expect("should compile");
        assert!(resp.size > 0);
        assert_eq!(resp.status.as_deref(), Some("validated"));
    }

    #[test]
    fn compile_cpu_invalid_wgsl() {
        let req = CompileCpuRequest {
            wgsl_source: "not valid {{{{".into(),
            arch: "x86_64".into(),
            opt_level: 0,
            entry_point: None,
        };
        assert!(handle_compile_cpu(&req).is_err());
    }

    #[test]
    fn execute_cpu_trivial() {
        let req = ExecuteCpuRequest {
            wgsl_source: "@compute @workgroup_size(1) fn main() {}".into(),
            entry_point: None,
            workgroups: [1, 1, 1],
            bindings: vec![],
            uniforms: vec![],
            strategy: ExecutionStrategy::Jit,
        };
        let resp = handle_execute_cpu(&req).expect("should execute");
        assert!(resp.execution_time_ns > 0 || resp.bindings.is_empty());
    }

    #[test]
    fn execute_validated_jit_trivial() {
        let req = ExecuteCpuRequest {
            wgsl_source: "@compute @workgroup_size(1) fn main() {}".into(),
            entry_point: None,
            workgroups: [1, 1, 1],
            bindings: vec![],
            uniforms: vec![],
            strategy: ExecutionStrategy::ValidatedJit,
        };
        let resp = handle_execute_cpu(&req).expect("should execute");
        assert_eq!(resp.strategy_used, Some(ExecutionStrategy::ValidatedJit));
    }

    #[test]
    fn execute_interpret_strategy() {
        let req = ExecuteCpuRequest {
            wgsl_source: "@compute @workgroup_size(1) fn main() {}".into(),
            entry_point: None,
            workgroups: [1, 1, 1],
            bindings: vec![],
            uniforms: vec![],
            strategy: ExecutionStrategy::Interpret,
        };
        let resp = handle_execute_cpu(&req).expect("should execute");
        assert_eq!(resp.strategy_used, Some(ExecutionStrategy::Interpret));
    }

    #[test]
    fn validate_trivial() {
        let req = ValidateRequest {
            wgsl_source: r"
@group(0) @binding(0) var<storage, read_write> output: array<f32>;
@compute @workgroup_size(1) fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    output[id.x] = 1.0;
}
"
            .into(),
            entry_point: None,
            workgroups: [1, 1, 1],
            bindings: vec![BindingData {
                group: 0,
                binding: 0,
                data: bytes::Bytes::from(vec![0u8; 4]),
                usage: BindingUsage::ReadWrite,
            }],
            uniforms: vec![],
            expected: vec![coral_reef_cpu::ExpectedBinding {
                group: 0,
                binding: 0,
                data: bytes::Bytes::from(1.0f32.to_le_bytes().to_vec()),
                tolerance: Tolerance {
                    abs: 1e-6,
                    rel: 1e-6,
                },
            }],
        };
        let resp = handle_validate(&req).expect("should validate");
        assert!(
            resp.passed,
            "trivial shader should match: {:?}",
            resp.mismatches
        );
    }
}
