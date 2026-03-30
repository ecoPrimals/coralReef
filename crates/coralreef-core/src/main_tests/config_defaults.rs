// SPDX-License-Identifier: AGPL-3.0-only
//! Tests for default configuration values used by the UniBin entry point.
//!
//! Environment-based bind overrides are covered in integration tests under `tests/` because
//! `std::env::set_var` / `remove_var` are `unsafe` in Rust 2024 and this crate forbids `unsafe_code`.

use super::*;

use coral_reef::GpuArch;

const DOCUMENTED_DEFAULT_OPT_LEVEL: u32 = 2;

#[test]
fn default_shutdown_timeout_matches_documented_seconds() {
    const EXPECTED_SECS: u64 = 30;
    assert_eq!(
        config::DEFAULT_SHUTDOWN_TIMEOUT.as_secs(),
        EXPECTED_SECS,
        "graceful shutdown join should match DEFAULT_SHUTDOWN_TIMEOUT"
    );
}

#[test]
fn default_opt_level_matches_compile_cli_default() {
    let cli = parse_cli_from(["coralreef", "compile", "x.wgsl"]).unwrap();
    match &cli.command {
        Commands::Compile { opt_level, .. } => {
            assert_eq!(*opt_level, DOCUMENTED_DEFAULT_OPT_LEVEL);
        }
        _ => panic!("expected Compile command"),
    }
}

#[test]
fn default_gpu_arch_string_matches_compile_cli_default() {
    let expected = GpuArch::default().to_string();
    let cli = parse_cli_from(["coralreef", "compile", "x.wgsl"]).unwrap();
    match &cli.command {
        Commands::Compile { arch, .. } => {
            assert_eq!(arch.to_string(), expected);
        }
        _ => panic!("expected Compile command"),
    }
}

#[test]
fn default_tcp_bind_resolves_to_nonempty_string() {
    let bind = ipc::default_tcp_bind();
    assert!(
        !bind.is_empty() && bind.chars().any(|c| !c.is_whitespace()),
        "CORALREEF_TCP_BIND or fallback should yield a non-empty bind string"
    );
}
