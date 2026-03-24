// SPDX-License-Identifier: AGPL-3.0-only
//! Request and response types for the compiler service.
//!
//! Separated from handler logic for clarity. All types are `Serialize` +
//! `Deserialize` so they work over both JSON-RPC and tarpc transports.
//!
//! Shader source strings use `Arc<str>` for zero-copy sharing across pipeline
//! stages per wateringHole standards.

use crate::capability::{Capability, Transport};
use bytes::Bytes;
use serde::{Deserialize, Deserializer, Serialize};
use std::borrow::Cow;
use std::sync::Arc;

/// Deserialize a string from JSON into `Arc<str>` for zero-copy sharing.
fn deserialize_arc_str<'de, D>(deserializer: D) -> Result<Arc<str>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = String::deserialize(deserializer)?;
    Ok(Arc::from(s.into_boxed_str()))
}

/// tarpc-only SPIR-V compile request (zero-copy via `Bytes`).
///
/// Uses `bytes::Bytes` so SPIR-V can be shared without copying over the wire.
/// Serializes as base64 when using JSON transport.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileSpirvRequestTarpc {
    /// Raw SPIR-V bytes (zero-copy).
    pub spirv: Bytes,
    /// Target GPU architecture name (e.g. `sm_70`, `rdna2`).
    pub arch: String,
    /// Optimization level (0-3).
    pub opt_level: u32,
    /// Enable f64 software transcendentals.
    pub fp64_software: bool,
}

/// Request to compile a shader (JSON-RPC wire format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileRequest {
    /// SPIR-V words (JSON array of u32; base64 in tarpc uses [`CompileSpirvRequestTarpc`]).
    pub spirv_words: Vec<u32>,
    /// Target GPU architecture name (e.g. `sm70`, `sm86`, `rdna2`). Optional; defaults to sm70.
    #[serde(default = "default_arch")]
    pub arch: String,
    /// Optimization level (0-3).
    #[serde(default = "default_opt_level")]
    pub opt_level: u32,
    /// Enable f64 software transcendentals.
    #[serde(default)]
    pub fp64_software: bool,
}

/// Request to compile WGSL source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileWgslRequest {
    /// WGSL source code (shared via `Arc<str>` across pipeline stages).
    #[serde(deserialize_with = "deserialize_arc_str")]
    pub wgsl_source: Arc<str>,
    /// Target GPU architecture name (e.g. `sm70`, `sm86`, `rdna2`). Optional; defaults to sm70.
    #[serde(default = "default_arch")]
    pub arch: String,
    /// Optimization level (0-3).
    #[serde(default = "default_opt_level")]
    pub opt_level: u32,
    /// Enable f64 software transcendentals.
    #[serde(default)]
    pub fp64_software: bool,
    /// f64 strategy hint from the caller (e.g. `"software"`, `"native"`).
    /// Optional — defaults to using `fp64_software` if absent.
    #[serde(default)]
    pub fp64_strategy: Option<String>,
    /// FMA fusion policy hint (e.g. `"fused"`, `"separate"`, `"auto"`).
    /// Optional — defaults to `"auto"` (compiler decides).
    #[serde(default)]
    pub fma_policy: Option<String>,
}

/// Response from shader compilation.
///
/// Uses `bytes::Bytes` for zero-copy IPC payloads — `Bytes::from(Vec<u8>)`
/// takes ownership of the allocation without copying.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileResponse {
    /// Compiled GPU binary (zero-copy via `bytes::Bytes`).
    pub binary: Bytes,
    /// Size in bytes.
    pub size: usize,
    /// Target architecture the binary was compiled for.
    #[serde(default)]
    pub arch: Option<String>,
    /// Compilation status (e.g. `"success"`, `"partial"`).
    #[serde(default)]
    pub status: Option<String>,
}

/// `identity.get` response — primal self-description for capability-based discovery.
///
/// Per wateringHole `CAPABILITY_BASED_DISCOVERY_STANDARD`: name, version, capability
/// lists, and bound transports after servers listen.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityGetResponse {
    /// Primal name (from crate / config).
    pub name: Cow<'static, str>,
    /// Semantic version.
    pub version: Cow<'static, str>,
    /// Capabilities this primal provides.
    pub provides: Vec<Capability>,
    /// Capabilities required from peers.
    pub requires: Vec<Capability>,
    /// IPC transports (populated after bind).
    pub transports: Vec<Transport>,
}

impl IdentityGetResponse {
    /// Minimal identity when full advertisement is not yet available.
    #[must_use]
    pub fn fallback() -> Self {
        let desc = crate::capability::self_description();
        Self {
            name: crate::config::PRIMAL_NAME.into(),
            version: crate::config::PRIMAL_VERSION.into(),
            provides: desc.provides,
            requires: desc.requires,
            transports: Vec::new(),
        }
    }
}

/// Health check response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    /// Primal name.
    pub name: Cow<'static, str>,
    /// Version.
    pub version: Cow<'static, str>,
    /// Current status.
    pub status: Cow<'static, str>,
    /// Supported architectures.
    pub supported_archs: Vec<String>,
}

/// A single device target for multi-device compilation.
///
/// Carries an architecture hint and optional `PCIe` group ID so the caller
/// can request compilation for specific GPU slots in a multi-GPU system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceTarget {
    /// Card index (0-based, maps to `/dev/dri/renderD128+N`).
    #[serde(default)]
    pub card_index: u32,
    /// GPU architecture hint (e.g. `"sm_89"`, `"rdna2"`).
    pub arch: String,
    /// Optional `PCIe` group / switch affinity hint.
    #[serde(default)]
    pub pcie_group: Option<u32>,
}

/// Request to compile a single WGSL shader for multiple GPU targets at once.
///
/// Implements the `shader.compile.wgsl.multi` endpoint (ecosystem protocol S144)
/// handoff. Compiles the same shader source to native binaries for each
/// target device in a single request, enabling multi-GPU dispatch preparation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiDeviceCompileRequest {
    /// WGSL source code (shared via `Arc<str>` across all targets).
    #[serde(deserialize_with = "deserialize_arc_str")]
    pub wgsl_source: Arc<str>,
    /// Target devices to compile for.
    pub targets: Vec<DeviceTarget>,
    /// Optimization level (0-3).
    #[serde(default = "default_opt_level")]
    pub opt_level: u32,
    /// Enable f64 software transcendentals.
    #[serde(default)]
    pub fp64_software: bool,
    /// f64 strategy hint (e.g. `"software"`, `"native"`).
    #[serde(default)]
    pub fp64_strategy: Option<String>,
    /// FMA fusion policy hint (e.g. `"fused"`, `"separate"`, `"auto"`).
    #[serde(default)]
    pub fma_policy: Option<String>,
}

/// Result of compiling for a single device in a multi-device request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCompileResult {
    /// Card index this result corresponds to.
    pub card_index: u32,
    /// Architecture compiled for.
    pub arch: String,
    /// Compiled binary (zero-copy via `bytes::Bytes`), or `None` on failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binary: Option<Bytes>,
    /// Binary size in bytes (0 on failure).
    pub size: usize,
    /// Error message if compilation failed for this target.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Response from multi-device compilation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiDeviceCompileResponse {
    /// Per-device compilation results (same order as request `targets`).
    pub results: Vec<DeviceCompileResult>,
    /// Number of targets that compiled successfully.
    pub success_count: usize,
    /// Total number of targets requested.
    pub total_count: usize,
}

/// `health.check` response per wateringHole `PRIMAL_IPC_PROTOCOL` v3.0.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResponse {
    /// Primal name (self-knowledge only).
    pub name: Cow<'static, str>,
    /// Primal version.
    pub version: Cow<'static, str>,
    /// Whether the primal is healthy.
    pub healthy: bool,
    /// Human-readable status.
    pub status: Cow<'static, str>,
    /// Supported GPU architectures.
    pub supported_archs: Vec<String>,
    /// Family ID for multi-instance disambiguation.
    pub family_id: Cow<'static, str>,
}

/// `health.liveness` response — lightweight alive check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LivenessResponse {
    /// `true` if the process is alive and responsive.
    pub alive: bool,
}

/// `health.readiness` response — ready to accept work.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadinessResponse {
    /// `true` if the primal is ready to serve requests.
    pub ready: bool,
    /// Primal name (self-knowledge).
    pub name: Cow<'static, str>,
}

/// Default GPU architecture string for serde deserialization.
#[must_use]
pub fn default_arch() -> String {
    coral_reef::GpuArch::default().to_string()
}

/// Default optimization level for compilation requests.
#[must_use]
pub const fn default_opt_level() -> u32 {
    2
}

#[cfg(test)]
mod identity_tests {
    use super::*;

    #[test]
    fn identity_get_fallback_matches_package() {
        let r = IdentityGetResponse::fallback();
        assert_eq!(r.name.as_ref(), env!("CARGO_PKG_NAME"));
        assert_eq!(r.version.as_ref(), env!("CARGO_PKG_VERSION"));
        assert!(r.transports.is_empty());
        assert!(!r.provides.is_empty());
    }
}
