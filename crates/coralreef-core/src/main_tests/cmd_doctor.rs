// SPDX-License-Identifier: AGPL-3.0-only
use super::*;

use coralreef_core::commands;

#[tokio::test]
async fn cmd_doctor_output_formatting() {
    let result = cmd_doctor().await;
    assert!(matches!(result, UniBinExit::Success));
    let report = commands::run_doctor().await.unwrap();
    assert!(report.contains("doctor"));
    assert!(report.contains("[OK]"));
    assert!(report.contains("Capabilities"));
    assert!(report.contains("Supported architectures"));
    assert!(report.contains("Diagnostic complete"));
}
