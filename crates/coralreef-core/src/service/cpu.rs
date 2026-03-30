// SPDX-License-Identifier: AGPL-3.0-only
//! CPU compilation and validation service handlers.
//!
//! Delegates to `coral_reef_cpu` for interpretation and validation.

use coral_reef::CompileError;
use coral_reef_cpu::types::{
    CompileCpuRequest, CpuError, ExecuteCpuRequest, ExecuteCpuResponse, ValidateRequest,
    ValidateResponse,
};

use super::types::CompileResponse;

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

/// `shader.execute.cpu` — execute a WGSL compute shader on the CPU interpreter.
///
/// # Errors
///
/// Returns [`CompileError`] wrapping the underlying [`CpuError`].
pub fn handle_execute_cpu(request: &ExecuteCpuRequest) -> Result<ExecuteCpuResponse, CompileError> {
    coral_reef_cpu::execute_cpu(request).map_err(cpu_error_to_compile_error)
}

/// `shader.validate` — execute on CPU and compare against expected values.
///
/// # Errors
///
/// Returns [`CompileError`] wrapping the underlying [`CpuError`].
pub fn handle_validate(request: &ValidateRequest) -> Result<ValidateResponse, CompileError> {
    coral_reef_cpu::validate(request).map_err(cpu_error_to_compile_error)
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
        };
        let resp = handle_execute_cpu(&req).expect("should execute");
        assert!(resp.execution_time_ns > 0 || resp.bindings.is_empty());
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
