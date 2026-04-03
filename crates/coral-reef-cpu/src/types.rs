// SPDX-License-Identifier: AGPL-3.0-only
//! Wire types shared between `coral-reef-cpu` and `coralreef-core` IPC layers.

use bytes::Bytes;
use serde::{Deserialize, Serialize};

/// Request to compile WGSL for CPU execution (Phase 2: Cranelift native binary).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileCpuRequest {
    /// WGSL source code.
    pub wgsl_source: String,
    /// Target CPU architecture (e.g. `"x86_64"`, `"aarch64"`).
    pub arch: String,
    /// Optimisation level (`0`–`3`).
    #[serde(default)]
    pub opt_level: u32,
    /// Compute entry point name (defaults to first `@compute` entry).
    #[serde(default)]
    pub entry_point: Option<String>,
}

/// CPU execution strategy for the progressive trust model.
///
/// Controls how `shader.execute.cpu` dispatches work:
/// - `Interpret`: always use the reference interpreter (slow, inspectable)
/// - `Jit`: always use the Cranelift JIT (fast, opaque)
/// - `ValidatedJit`: interpret once to prove correctness, then cache and use
///   JIT for subsequent calls. Periodic re-validation detects drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionStrategy {
    /// Reference interpreter only (Naga IR walker). Slow but transparent.
    Interpret,
    /// Cranelift JIT only. Fast but opaque — no correctness proof.
    #[default]
    Jit,
    /// Interpret-validate first, then JIT-cache on success. Re-validates
    /// at the configured interval to detect Cranelift update drift.
    ValidatedJit,
}

/// Request to execute a WGSL compute shader on the CPU interpreter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteCpuRequest {
    /// WGSL source code.
    pub wgsl_source: String,
    /// Compute entry point name (defaults to first `@compute` entry).
    #[serde(default)]
    pub entry_point: Option<String>,
    /// Workgroup dispatch dimensions `[x, y, z]`.
    pub workgroups: [u32; 3],
    /// Storage / read-write buffer bindings.
    #[serde(default)]
    pub bindings: Vec<BindingData>,
    /// Uniform buffer bindings.
    #[serde(default)]
    pub uniforms: Vec<UniformData>,
    /// Execution strategy (defaults to `jit` for backward compatibility).
    #[serde(default)]
    pub strategy: ExecutionStrategy,
}

/// Request to validate GPU output against CPU reference execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidateRequest {
    /// WGSL source code.
    pub wgsl_source: String,
    /// Compute entry point name (defaults to first `@compute` entry).
    #[serde(default)]
    pub entry_point: Option<String>,
    /// Workgroup dispatch dimensions `[x, y, z]`.
    pub workgroups: [u32; 3],
    /// Storage / read-write buffer bindings (inputs to the shader).
    #[serde(default)]
    pub bindings: Vec<BindingData>,
    /// Uniform buffer bindings.
    #[serde(default)]
    pub uniforms: Vec<UniformData>,
    /// Expected output bindings with tolerance.
    pub expected: Vec<ExpectedBinding>,
}

/// A storage buffer binding with group/binding indices and raw data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BindingData {
    /// Bind group index.
    pub group: u32,
    /// Binding index within the group.
    pub binding: u32,
    /// Raw buffer data.
    pub data: Bytes,
    /// Whether the shader reads, writes, or both.
    #[serde(default = "default_usage")]
    pub usage: BindingUsage,
}

/// Buffer usage for a binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BindingUsage {
    /// Read-only storage buffer.
    ReadOnly,
    /// Write-only storage buffer.
    WriteOnly,
    /// Read-write storage buffer.
    #[default]
    ReadWrite,
}

const fn default_usage() -> BindingUsage {
    BindingUsage::ReadWrite
}

/// A uniform buffer binding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UniformData {
    /// Bind group index.
    pub group: u32,
    /// Binding index within the group.
    pub binding: u32,
    /// Raw uniform data.
    pub data: Bytes,
}

/// Result of CPU execution: modified bindings plus timing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteCpuResponse {
    /// Output bindings after shader execution (same group/binding keys as input).
    pub bindings: Vec<BindingData>,
    /// Wall-clock execution time in nanoseconds.
    pub execution_time_ns: u64,
    /// Which strategy was actually used for this execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strategy_used: Option<ExecutionStrategy>,
    /// Whether this result came from a cached JIT compilation.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cache_hit: bool,
    /// Whether re-validation was performed on this call.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub revalidated: bool,
}

/// Result of validation: pass/fail with detailed mismatches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidateResponse {
    /// Whether all expected bindings match within tolerance.
    pub passed: bool,
    /// Per-element mismatches (empty when `passed == true`).
    pub mismatches: Vec<Mismatch>,
    /// Dual-path validation result (Path A: Naga interpreter vs Path B: Cranelift JIT).
    /// Present only when both paths successfully execute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dual_path: Option<DualPathResult>,
}

/// Result of dual-path validation comparing Naga interpreter (Path A) against
/// Cranelift JIT (Path B) to prove optimization pipeline correctness.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DualPathResult {
    /// Whether Path A and Path B agree within tolerance.
    pub paths_agree: bool,
    /// Per-element mismatches between the two paths.
    pub path_mismatches: Vec<Mismatch>,
    /// Execution time of Path A (Naga interpreter) in nanoseconds.
    pub path_a_ns: u64,
    /// Execution time of Path B (Cranelift JIT) in nanoseconds.
    pub path_b_ns: u64,
    /// Human-readable note if one path failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Expected output binding with per-element tolerance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedBinding {
    /// Bind group index.
    pub group: u32,
    /// Binding index within the group.
    pub binding: u32,
    /// Expected raw buffer data.
    pub data: Bytes,
    /// Tolerance for comparison.
    pub tolerance: Tolerance,
}

/// Absolute and relative tolerance for floating-point comparison.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Tolerance {
    /// Maximum absolute difference.
    pub abs: f64,
    /// Maximum relative difference (fraction of expected value).
    pub rel: f64,
}

impl Default for Tolerance {
    fn default() -> Self {
        Self {
            abs: 1e-6,
            rel: 1e-6,
        }
    }
}

/// A single element mismatch in validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mismatch {
    /// Bind group index.
    pub group: u32,
    /// Binding index within the group.
    pub binding: u32,
    /// Element index within the buffer (byte offset / element size).
    pub index: usize,
    /// Actual value from CPU execution.
    pub got: f64,
    /// Expected value.
    pub expected: f64,
    /// Absolute error `|got - expected|`.
    pub abs_error: f64,
    /// Relative error `|got - expected| / |expected|`.
    pub rel_error: f64,
}

/// Errors from CPU compilation or execution.
#[derive(Debug, thiserror::Error)]
pub enum CpuError {
    /// WGSL parse failure.
    #[error("WGSL parse error: {0}")]
    Parse(String),
    /// Naga validation failure.
    #[error("naga validation error: {0}")]
    Validation(String),
    /// No matching compute entry point found.
    #[error("no compute entry point '{0}' in module")]
    EntryPointNotFound(String),
    /// Unsupported naga IR construct encountered during interpretation.
    #[error("unsupported IR: {0}")]
    Unsupported(String),
    /// Binding referenced by shader not provided in request.
    #[error("missing binding (group={group}, binding={binding})")]
    MissingBinding {
        /// Bind group index.
        group: u32,
        /// Binding index.
        binding: u32,
    },
    /// Internal interpreter error.
    #[error("interpreter error: {0}")]
    Internal(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tolerance_default_values() {
        let t = Tolerance::default();
        assert!(t.abs > 0.0);
        assert!(t.rel > 0.0);
    }

    #[test]
    fn binding_usage_default_is_read_write() {
        assert_eq!(BindingUsage::default(), BindingUsage::ReadWrite);
    }

    #[test]
    fn execute_cpu_request_round_trip() {
        let req = ExecuteCpuRequest {
            wgsl_source: "@compute @workgroup_size(1) fn main() {}".into(),
            entry_point: None,
            workgroups: [1, 1, 1],
            bindings: vec![],
            uniforms: vec![],
            strategy: ExecutionStrategy::Jit,
        };
        let json = serde_json::to_string(&req).expect("serialize");
        let _: ExecuteCpuRequest = serde_json::from_str(&json).expect("deserialize");
    }

    #[test]
    fn validate_response_round_trip() {
        let resp = ValidateResponse {
            passed: true,
            mismatches: vec![],
            dual_path: None,
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        let deser: ValidateResponse = serde_json::from_str(&json).expect("deserialize");
        assert!(deser.passed);
    }

    #[test]
    fn mismatch_round_trip() {
        let m = Mismatch {
            group: 0,
            binding: 0,
            index: 42,
            got: std::f64::consts::PI,
            expected: 3.15,
            abs_error: 0.01,
            rel_error: 0.003_174_603,
        };
        let json = serde_json::to_string(&m).expect("serialize");
        let deser: Mismatch = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deser.index, 42);
    }
}
