// SPDX-License-Identifier: AGPL-3.0-only
//! Compiler service — shared logic for both JSON-RPC and tarpc transports.
//!
//! Follows wateringHole semantic method naming: `compiler.{operation}`.

use bytes::Bytes;
use coral_reef::{AmdArch, CompileError, CompileOptions, GpuTarget, NvArch};
use serde::{Deserialize, Serialize};

/// Request to compile a shader.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileRequest {
    /// SPIR-V words (base64-encoded in JSON-RPC, raw in tarpc).
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
}

/// Health check response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    /// Primal name.
    pub name: String,
    /// Version.
    pub version: String,
    /// Current status.
    pub status: String,
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
    Err(CompileError::UnsupportedArch(s.to_owned()))
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

/// Execute a compile request (SPIR-V input).
///
/// # Errors
///
/// Returns [`CompileError`] on invalid input or compilation failure.
pub fn handle_compile(req: &CompileRequest) -> Result<CompileResponse, CompileError> {
    let options = build_options(&req.arch, req.opt_level, req.fp64_software)?;
    let binary = coral_reef::compile(&req.spirv_words, &options)?;
    let size = binary.len();
    Ok(CompileResponse {
        binary: Bytes::from(binary),
        size,
    })
}

/// Execute a WGSL compile request.
///
/// # Errors
///
/// Returns [`CompileError`] on invalid input or compilation failure.
pub fn handle_compile_wgsl(req: &CompileWgslRequest) -> Result<CompileResponse, CompileError> {
    let options = build_options(&req.arch, req.opt_level, req.fp64_software)?;
    let binary = coral_reef::compile_wgsl(&req.wgsl_source, &options)?;
    let size = binary.len();
    Ok(CompileResponse {
        binary: Bytes::from(binary),
        size,
    })
}

/// Generate a health response listing all supported architectures.
#[must_use]
pub fn handle_health() -> HealthResponse {
    let mut archs: Vec<String> = NvArch::ALL.iter().map(ToString::to_string).collect();
    archs.extend(AmdArch::ALL.iter().map(ToString::to_string));
    HealthResponse {
        name: env!("CARGO_PKG_NAME").to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        status: "operational".to_owned(),
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
        };
        assert!(handle_compile_wgsl(&req).is_err());
    }
}
