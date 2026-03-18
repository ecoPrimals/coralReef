// SPDX-License-Identifier: AGPL-3.0-only
//! Integration tests for `OrExit` error paths.
//!
//! The error path calls `process::exit()`, so we must run in a subprocess.

#[test]
fn or_exit_err_exits_with_code_one() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_or_exit_err"))
        .output()
        .expect("failed to run or_exit_err binary");

    assert_eq!(
        output.status.code(),
        Some(1),
        "OrExit Result::Err path should exit with code 1: stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
}
