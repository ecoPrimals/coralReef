// SPDX-License-Identifier: AGPL-3.0-only
//! Command implementations for the coralReef `UniBin`.
//!
//! Extracted from the binary crate for testability.

use coral_reef::{CompileError, CompileOptions, GpuArch};
use std::io;
use std::path::Path;

/// `UniBin` exit codes per ecoPrimals standard.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitStatus {
    /// Success.
    Success = 0,
    /// General error.
    GeneralError = 1,
    /// Configuration / input error.
    ConfigError = 2,
    /// Internal error (panic, OOM).
    InternalError = 3,
}

/// Map a compile error to the appropriate exit status.
#[must_use]
pub const fn exit_status_for(e: &CompileError) -> ExitStatus {
    match e {
        CompileError::InvalidInput(_) | CompileError::UnsupportedArch(_) => ExitStatus::ConfigError,
        _ => ExitStatus::GeneralError,
    }
}

/// Compile a shader file, returning the binary bytes or an error with exit status.
///
/// # Errors
///
/// Returns `(ExitStatus, String)` on:
/// - File read failure (`ConfigError` if not found, `GeneralError` otherwise)
/// - Invalid UTF-8 in WGSL source (`ConfigError`)
/// - Compilation failure (`ConfigError` for `InvalidInput`/`UnsupportedArch`, `GeneralError` otherwise)
pub fn compile_file(
    input: &Path,
    arch: GpuArch,
    opt_level: u32,
    fp64_software: bool,
) -> Result<Vec<u8>, (ExitStatus, String)> {
    let input_bytes = std::fs::read(input).map_err(|e| {
        let status = if e.kind() == io::ErrorKind::NotFound {
            ExitStatus::ConfigError
        } else {
            ExitStatus::GeneralError
        };
        (status, format!("failed to read {}: {e}", input.display()))
    })?;

    let options = CompileOptions {
        target: arch.into(),
        opt_level,
        debug_info: false,
        fp64_software,
        ..CompileOptions::default()
    };

    if input.extension().is_some_and(|e| e == "wgsl") {
        let source = String::from_utf8(input_bytes).map_err(|e| {
            (
                ExitStatus::ConfigError,
                format!("invalid UTF-8 in WGSL source: {e}"),
            )
        })?;
        coral_reef::compile_wgsl(&source, &options)
            .map_err(|e| (exit_status_for(&e), e.to_string()))
    } else {
        let words: Vec<u32> = input_bytes
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        coral_reef::compile(&words, &options).map_err(|e| (exit_status_for(&e), e.to_string()))
    }
}

/// Run the doctor diagnostic, returning a formatted report.
///
/// # Errors
///
/// Returns an error string if primal start, health check, or stop fails.
pub async fn run_doctor() -> Result<String, String> {
    use crate::CoralReefPrimal;
    use crate::health::PrimalHealth;
    use crate::lifecycle::PrimalLifecycle;
    use std::fmt::Write;

    let mut report = String::new();
    let _ = writeln!(
        report,
        "{} doctor — diagnostic check\n",
        env!("CARGO_PKG_NAME")
    );

    let desc = crate::capability::self_description();
    report.push_str("[OK] Capabilities (provides):\n");
    for cap in &desc.provides {
        let _ = writeln!(report, "     - {} v{}", cap.id, cap.version);
    }
    report.push_str("[OK] Capabilities (requires):\n");
    for cap in &desc.requires {
        let _ = writeln!(report, "     - {} v{}", cap.id, cap.version);
    }
    report.push_str("[OK] Supported architectures:\n");
    for arch in GpuArch::ALL {
        let _ = writeln!(report, "     - {arch}");
    }

    let mut primal = CoralReefPrimal::new();
    let _ = writeln!(report, "[OK] Primal created (state: {:?})", primal.state());

    primal
        .start()
        .await
        .map_err(|e| format!("primal start failed: {e}"))?;
    let _ = writeln!(report, "[OK] Primal started (state: {:?})", primal.state());

    let health = primal
        .health_check()
        .await
        .map_err(|e| format!("health check failed: {e}"))?;
    let _ = writeln!(report, "[OK] Health: {:?}", health.status);

    let test_opts = CompileOptions::default();
    let test_wgsl = "@compute @workgroup_size(1)\nfn main() {}";
    match coral_reef::compile_wgsl(test_wgsl, &test_opts) {
        Ok(_) => report.push_str("[OK] Compile pipeline operational\n"),
        Err(CompileError::NotImplemented(_)) => {
            report.push_str("[WARN] Compile pipeline: not yet implemented\n");
        }
        Err(e) => {
            let _ = primal.stop().await;
            return Err(format!("compile pipeline failed: {e}"));
        }
    }

    primal
        .stop()
        .await
        .map_err(|e| format!("primal stop failed: {e}"))?;
    report.push_str("[OK] Primal stopped cleanly\n\nDiagnostic complete.");
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exit_status_for_invalid_input() {
        let e = CompileError::InvalidInput("test".into());
        assert_eq!(exit_status_for(&e), ExitStatus::ConfigError);
    }

    #[test]
    fn test_exit_status_for_unsupported_arch() {
        let e = CompileError::UnsupportedArch("sm_99".into());
        assert_eq!(exit_status_for(&e), ExitStatus::ConfigError);
    }

    #[test]
    fn test_exit_status_for_not_implemented() {
        let e = CompileError::NotImplemented("feature".into());
        assert_eq!(exit_status_for(&e), ExitStatus::GeneralError);
    }

    #[test]
    fn test_compile_file_nonexistent_returns_config_error() {
        let result = compile_file(
            Path::new("/nonexistent/shader.spv"),
            GpuArch::default(),
            2,
            true,
        );
        assert!(result.is_err());
        let (status, _) = result.unwrap_err();
        assert_eq!(status, ExitStatus::ConfigError);
    }

    #[test]
    fn test_compile_file_empty_wgsl() {
        let tmp = std::env::temp_dir().join("coralreef_test_empty.wgsl");
        std::fs::write(&tmp, "").unwrap();
        let result = compile_file(&tmp, GpuArch::default(), 2, true);
        assert!(result.is_err());
        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn test_run_doctor_succeeds() {
        let result = run_doctor().await;
        assert!(result.is_ok());
        let report = result.unwrap();
        assert!(report.contains("doctor"));
        assert!(report.contains("[OK]"));
        assert!(report.contains("Diagnostic complete"));
    }

    #[tokio::test]
    async fn test_run_doctor_contains_capabilities() {
        let result = run_doctor().await.unwrap();
        assert!(result.contains("Capabilities (provides)"));
        assert!(result.contains("shader.compile"));
    }

    #[tokio::test]
    async fn test_run_doctor_contains_architectures() {
        let result = run_doctor().await.unwrap();
        assert!(result.contains("Supported architectures"));
        assert!(result.contains("sm_70"));
    }

    #[test]
    fn test_exit_status_for_validation() {
        let e = CompileError::Validation("type error".into());
        assert_eq!(exit_status_for(&e), ExitStatus::GeneralError);
    }

    #[test]
    fn test_exit_status_for_encoding() {
        let e = CompileError::Encoding("bad opcode".into());
        assert_eq!(exit_status_for(&e), ExitStatus::GeneralError);
    }

    #[test]
    fn test_exit_status_for_register_allocation() {
        let e = CompileError::RegisterAllocation("spill".into());
        assert_eq!(exit_status_for(&e), ExitStatus::GeneralError);
    }

    #[test]
    fn test_compile_file_invalid_utf8_wgsl() {
        let tmp = std::env::temp_dir().join("coralreef_test_invalid_utf8.wgsl");
        std::fs::write(&tmp, [0xff, 0xfe, 0xfd]).unwrap();
        let result = compile_file(&tmp, GpuArch::default(), 2, true);
        assert!(result.is_err());
        let (status, msg) = result.unwrap_err();
        assert_eq!(status, ExitStatus::ConfigError);
        assert!(msg.to_lowercase().contains("utf-8"));
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn test_compile_file_valid_spv_extension() {
        let tmp = std::env::temp_dir().join("coralreef_test_minimal.spv");
        // Minimal SPIR-V header (magic, version, generator, bound, schema)
        let words: Vec<u32> = vec![0x0723_0203, 0x0001_0000, 0, 0, 0];
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        std::fs::write(&tmp, &bytes).unwrap();
        let result = compile_file(&tmp, GpuArch::default(), 2, true);
        let _ = std::fs::remove_file(&tmp);
        // May succeed or fail depending on SPIR-V validity; we just verify it doesn't panic
        let _ = result;
    }
}
