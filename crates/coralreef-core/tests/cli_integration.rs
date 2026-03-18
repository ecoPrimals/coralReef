// SPDX-License-Identifier: AGPL-3.0-only
//! Integration tests for coralreef CLI — subcommands, exit codes, help output.

use std::process::Command;

fn coralreef_bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_coralreef"))
}

#[test]
fn compile_help_prints_usage() {
    let output = coralreef_bin()
        .args(["compile", "--help"])
        .output()
        .expect("failed to run coralreef");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("Input file") || combined.contains("input"),
        "help should describe input: stdout={stdout} stderr={stderr}"
    );
    assert!(combined.contains("compile"));
}

#[test]
fn doctor_help_prints_usage() {
    let output = coralreef_bin()
        .args(["doctor", "--help"])
        .output()
        .expect("failed to run coralreef");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("doctor") || combined.contains("Health"),
        "doctor help should describe command: {combined}"
    );
}

#[test]
fn doctor_exit_code_success() {
    let output = coralreef_bin()
        .args(["doctor"])
        .output()
        .expect("failed to run coralreef");

    assert_eq!(
        output.status.code(),
        Some(0),
        "doctor should exit 0 on success: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn compile_nonexistent_file_exit_code_config_error() {
    let output = coralreef_bin()
        .args(["compile", "/nonexistent/path/shader.wgsl"])
        .output()
        .expect("failed to run coralreef");

    assert_eq!(
        output.status.code(),
        Some(2),
        "compile with nonexistent file should exit 2 (ConfigError)"
    );
}

#[test]
fn compile_missing_input_exit_code_error() {
    let output = coralreef_bin()
        .args(["compile"])
        .output()
        .expect("failed to run coralreef");

    assert!(
        !output.status.success(),
        "compile without input should fail"
    );
}

#[test]
fn invalid_args_exit_code_config_error() {
    let output = coralreef_bin()
        .args(["invalid-subcommand"])
        .output()
        .expect("failed to run coralreef");

    assert_eq!(
        output.status.code(),
        Some(2),
        "invalid subcommand should exit 2 (ConfigError): stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn missing_subcommand_exit_code_config_error() {
    let output = coralreef_bin().output().expect("failed to run coralreef");

    assert_eq!(
        output.status.code(),
        Some(2),
        "missing subcommand should exit 2 (ConfigError)"
    );
}

#[test]
fn compile_valid_wgsl_produces_output() {
    let tmp = std::env::temp_dir().join("coralreef_cli_test_compile.wgsl");
    std::fs::write(&tmp, "@compute @workgroup_size(1)\nfn main() {}").unwrap();
    let out_path = tmp.with_extension("bin");

    let output = coralreef_bin()
        .args([
            "compile",
            tmp.to_str().unwrap(),
            "--output",
            out_path.to_str().unwrap(),
            "--arch",
            "sm70",
        ])
        .output()
        .expect("failed to run coralreef");

    let success = output.status.success();
    let out_exists = out_path.exists();

    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_file(&out_path);

    assert!(
        success,
        "compile valid WGSL should succeed: stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(out_exists, "output file should exist on success");
}
