// SPDX-License-Identifier: AGPL-3.0-only
//! Compiler service — shared logic for both JSON-RPC and tarpc transports.
//!
//! Follows wateringHole semantic method naming: `shader.compile.{operation}`.

use bytes::Bytes;
use coral_reef::{AmdArch, CompileError, CompileOptions, GpuTarget, NvArch};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

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
    /// Target GPU architecture name (e.g. `sm_70`, `rdna2`).
    pub arch: String,
    /// Optimization level (0-3).
    pub opt_level: u32,
    /// Enable f64 software transcendentals.
    pub fp64_software: bool,
}

/// Request to compile WGSL source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileWgslRequest {
    /// WGSL source code.
    pub wgsl_source: String,
    /// Target GPU architecture name (e.g. `sm_70`, `rdna2`).
    pub arch: String,
    /// Optimization level (0-3).
    pub opt_level: u32,
    /// Enable f64 software transcendentals.
    pub fp64_software: bool,
    /// f64 strategy hint from the caller (e.g. `"software"`, `"native"`).
    /// Optional — defaults to using `fp64_software` if absent.
    #[serde(default)]
    pub fp64_strategy: Option<String>,
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

/// Parse an architecture string into a [`GpuTarget`].
///
/// Tries NVIDIA first, then AMD. No hardcoded arch list.
///
/// # Errors
///
/// Returns an error if the architecture string is not recognized by any vendor.
pub fn parse_target(s: &str) -> Result<GpuTarget, CompileError> {
    if let Some(nv) = NvArch::parse(s) {
        return Ok(GpuTarget::Nvidia(nv));
    }
    if let Some(amd) = AmdArch::parse(s) {
        return Ok(GpuTarget::Amd(amd));
    }
    Err(CompileError::UnsupportedArch(s.to_owned().into()))
}

fn build_options(
    arch: &str,
    opt_level: u32,
    fp64_software: bool,
) -> Result<CompileOptions, CompileError> {
    let target = parse_target(arch)?;
    Ok(CompileOptions {
        target,
        opt_level,
        debug_info: false,
        fp64_software,
        ..CompileOptions::default()
    })
}

/// Convert SPIR-V bytes to words for the compiler.
fn bytes_to_spirv_words(bytes: &[u8]) -> Result<Vec<u32>, CompileError> {
    if bytes.len() % 4 != 0 {
        return Err(CompileError::InvalidInput(
            "SPIR-V must be multiple of 4 bytes".into(),
        ));
    }
    let mut words = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        debug_assert_eq!(chunk.len(), 4, "chunks_exact(4) yields 4 bytes");
        words.push(u32::from_le_bytes(
            chunk.try_into().expect("chunks_exact(4) yields 4 bytes"),
        ));
    }
    Ok(words)
}

/// Execute a SPIR-V compile from raw bytes (zero-copy friendly).
///
/// Accepts `Bytes` or `&[u8]` so IPC transports can pass SPIR-V without
/// copying. The compiler expects `&[u32]`, so we convert once at this boundary.
///
/// # Errors
///
/// Returns [`CompileError`] on invalid input or compilation failure.
pub fn handle_compile_spirv(
    spirv: impl AsRef<[u8]>,
    arch: &str,
    opt_level: u32,
    fp64_software: bool,
) -> Result<CompileResponse, CompileError> {
    let options = build_options(arch, opt_level, fp64_software)?;
    let words = bytes_to_spirv_words(spirv.as_ref())?;
    if words.is_empty() {
        return Err(CompileError::InvalidInput("empty SPIR-V module".into()));
    }
    let binary = coral_reef::compile(&words, &options)?;
    let size = binary.len();
    Ok(CompileResponse {
        binary: Bytes::from(binary),
        size,
        arch: Some(arch.to_owned()),
        status: Some("success".to_owned()),
    })
}

/// Execute a compile request (SPIR-V input).
///
/// Kept for backward compatibility with [`CompileRequest`] (JSON-RPC wire format).
///
/// # Errors
///
/// Returns [`CompileError`] on invalid input or compilation failure.
pub fn handle_compile(req: &CompileRequest) -> Result<CompileResponse, CompileError> {
    let bytes: Vec<u8> = req
        .spirv_words
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    handle_compile_spirv(bytes, &req.arch, req.opt_level, req.fp64_software)
}

/// Execute a WGSL compile request.
///
/// # Errors
///
/// Returns [`CompileError`] on invalid input or compilation failure.
pub fn handle_compile_wgsl(req: &CompileWgslRequest) -> Result<CompileResponse, CompileError> {
    let fp64_sw = req
        .fp64_strategy
        .as_deref()
        .map_or(req.fp64_software, |s| s == "software");
    let options = build_options(&req.arch, req.opt_level, fp64_sw)?;
    let binary = coral_reef::compile_wgsl(&req.wgsl_source, &options)?;
    let size = binary.len();
    Ok(CompileResponse {
        binary: Bytes::from(binary),
        size,
        arch: Some(req.arch.clone()),
        status: Some("success".to_owned()),
    })
}

/// Generate a health response listing all supported architectures.
#[must_use]
pub fn handle_health() -> HealthResponse {
    let mut archs: Vec<String> = NvArch::ALL.iter().map(ToString::to_string).collect();
    archs.extend(AmdArch::ALL.iter().map(ToString::to_string));
    HealthResponse {
        name: env!("CARGO_PKG_NAME").into(),
        version: env!("CARGO_PKG_VERSION").into(),
        status: "operational".into(),
        supported_archs: archs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use coral_reef::GpuArch;

    #[test]
    fn test_parse_target_nvidia_variants() {
        assert_eq!(
            parse_target("sm_70").unwrap(),
            GpuTarget::Nvidia(NvArch::Sm70)
        );
        assert_eq!(
            parse_target("sm70").unwrap(),
            GpuTarget::Nvidia(NvArch::Sm70)
        );
        assert_eq!(
            parse_target("sm_89").unwrap(),
            GpuTarget::Nvidia(NvArch::Sm89)
        );
    }

    #[test]
    fn test_parse_target_invalid() {
        assert!(parse_target("sm_99").is_err());
        assert!(parse_target("").is_err());
        assert!(parse_target("unknown_gpu").is_err());
    }

    #[test]
    fn test_parse_target_amd() {
        let t = parse_target("rdna2").unwrap();
        assert_eq!(t, GpuTarget::Amd(AmdArch::Rdna2));
        let t2 = parse_target("gfx1100").unwrap();
        assert_eq!(t2, GpuTarget::Amd(AmdArch::Rdna3));
    }

    #[test]
    fn test_health_response() {
        let health = handle_health();
        assert_eq!(health.name, env!("CARGO_PKG_NAME"));
        assert!(!health.supported_archs.is_empty());
        assert!(health.supported_archs.iter().any(|a| a.contains("sm_")));
        assert!(health.supported_archs.iter().any(|a| a.contains("rdna")));
    }

    #[test]
    fn test_compile_request_empty_spirv() {
        let req = CompileRequest {
            spirv_words: vec![],
            arch: GpuArch::default().to_string(),
            opt_level: 2,
            fp64_software: true,
        };
        assert!(handle_compile(&req).is_err());
    }

    #[test]
    fn test_compile_wgsl_empty() {
        let req = CompileWgslRequest {
            wgsl_source: String::new(),
            arch: "sm_70".to_owned(),
            opt_level: 2,
            fp64_software: true,
            fp64_strategy: None,
        };
        assert!(handle_compile_wgsl(&req).is_err());
    }

    #[test]
    fn test_handle_compile_spirv_invalid_length() {
        let bytes = vec![0u8; 5];
        let result = handle_compile_spirv(&bytes, "sm_70", 2, true);
        assert!(result.is_err());
        let e = result.unwrap_err();
        assert!(e.to_string().to_lowercase().contains("multiple of 4"));
    }

    #[test]
    fn test_handle_compile_spirv_empty() {
        let bytes: Vec<u8> = vec![];
        let result = handle_compile_spirv(&bytes, "sm_70", 2, true);
        assert!(result.is_err());
        let e = result.unwrap_err();
        assert!(e.to_string().to_lowercase().contains("empty"));
    }

    #[test]
    fn test_handle_compile_spirv_unsupported_arch() {
        let bytes = vec![0u8; 8];
        let result = handle_compile_spirv(&bytes, "sm_99", 2, true);
        assert!(result.is_err());
        let e = result.unwrap_err();
        assert!(e.to_string().to_lowercase().contains("unsupported"));
    }

    #[test]
    fn test_handle_compile_wgsl_unsupported_arch() {
        let req = CompileWgslRequest {
            wgsl_source: "@compute @workgroup_size(1) fn main() {}".to_owned(),
            arch: "unknown_gpu".to_owned(),
            opt_level: 2,
            fp64_software: true,
            fp64_strategy: None,
        };
        let result = handle_compile_wgsl(&req);
        assert!(result.is_err());
        let e = result.unwrap_err();
        assert!(e.to_string().to_lowercase().contains("unsupported"));
    }

    #[test]
    fn test_parse_target_intel_not_supported() {
        assert!(parse_target("xe_hpg").is_err());
    }
}
