// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

use coralreef_core::commands;

#[tokio::test]
async fn cmd_doctor_output_formatting() {
    let result = cmd_doctor().await;
    assert!(matches!(result, UniBinExit::Success));
    let report = commands::run_doctor().await.expect("run_doctor succeeds");
    assert!(report.contains("doctor"));
    assert!(report.contains("[OK]"));
    assert!(report.contains("Capabilities"));
    assert!(report.contains("Capabilities (provides)"));
    assert!(report.contains("Capabilities (requires)"));
    assert!(report.contains("Supported architectures"));
    assert!(report.contains("Primal created"));
    assert!(report.contains("Primal started"));
    assert!(report.contains("Health:"));
    assert!(report.contains("Diagnostic complete"));
}
