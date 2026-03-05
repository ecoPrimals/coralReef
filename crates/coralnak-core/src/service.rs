// SPDX-License-Identifier: AGPL-3.0-only
//! Compiler service — shared logic for both JSON-RPC and tarpc transports.
//!
//! Follows wateringHole semantic method naming: `compiler.{operation}`.

use bytes::Bytes;
use coral_nak::{CompileError, CompileOptions, GpuArch};
use serde::{Deserialize, Serialize};

/// Request to compile a shader.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileRequest {
    /// SPIR-V words (base64-encoded in JSON-RPC, raw in tarpc).
    pub spirv_words: Vec<u32>,
    /// Target GPU architecture name (e.g. `sm_70`).
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

/// Parse an architecture string into a [`GpuArch`].
///
/// Delegates to `GpuArch::parse` — no hardcoded arch list here.
///
/// # Errors
///
/// Returns an error if the architecture string is not recognized.
pub fn parse_arch(s: &str) -> Result<GpuArch, CompileError> {
    GpuArch::parse(s).ok_or_else(|| CompileError::UnsupportedArch(s.to_owned()))
}

/// Execute a compile request.
///
/// # Errors
///
/// Returns [`CompileError`] on invalid input or compilation failure.
pub fn handle_compile(req: &CompileRequest) -> Result<CompileResponse, CompileError> {
    let arch = parse_arch(&req.arch)?;
    let options = CompileOptions {
        arch,
        opt_level: req.opt_level,
        debug_info: false,
        fp64_software: req.fp64_software,
    };
    let binary = coral_nak::compile(&req.spirv_words, &options)?;
    let size = binary.len();
    Ok(CompileResponse {
        binary: Bytes::from(binary),
        size,
    })
}

/// Generate a health response.
#[must_use]
pub fn handle_health() -> HealthResponse {
    HealthResponse {
        name: env!("CARGO_PKG_NAME").to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        status: "operational".to_owned(),
        supported_archs: GpuArch::ALL.iter().map(ToString::to_string).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_arch_valid() {
        assert_eq!(parse_arch("sm_70").unwrap(), GpuArch::Sm70);
        assert_eq!(parse_arch("sm70").unwrap(), GpuArch::Sm70);
        assert_eq!(parse_arch("sm_89").unwrap(), GpuArch::Sm89);
    }

    #[test]
    fn test_parse_arch_invalid() {
        assert!(parse_arch("sm_99").is_err());
        assert!(parse_arch("").is_err());
    }

    #[test]
    fn test_health_response() {
        let health = handle_health();
        assert_eq!(health.name, env!("CARGO_PKG_NAME"));
        assert!(!health.supported_archs.is_empty());
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
}
