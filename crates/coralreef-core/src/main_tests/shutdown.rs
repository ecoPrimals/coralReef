// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

use crate::TEST_SHUTDOWN_JOIN_TIMEOUT_MS_OVERRIDE;

static SHUTDOWN_TIMEOUT_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn shutdown_join_timeout_mutex_override_roundtrip() {
    const EXPECTED_MS: u64 = 42;
    let _guard = SHUTDOWN_TIMEOUT_ENV_LOCK.lock().unwrap();
    *crate::TEST_SHUTDOWN_JOIN_TIMEOUT_MS_OVERRIDE
        .lock()
        .unwrap() = Some(EXPECTED_MS);
    assert_eq!(
        super::super::shutdown_join_timeout().as_millis(),
        u128::from(EXPECTED_MS),
        "named tolerance: exact ms from mutex override"
    );
    *crate::TEST_SHUTDOWN_JOIN_TIMEOUT_MS_OVERRIDE
        .lock()
        .unwrap() = None;
    assert_eq!(
        super::super::shutdown_join_timeout(),
        crate::config::DEFAULT_SHUTDOWN_TIMEOUT
    );
}

#[test]
fn shutdown_join_timeout_elapsed_message_includes_duration() {
    const NANOS: u32 = 7;
    let msg = super::super::shutdown_join_timeout_elapsed_message(std::time::Duration::from_nanos(
        u64::from(NANOS),
    ));
    assert!(
        msg.starts_with("shutdown timed out after "),
        "expected diagnostic prefix: {msg}"
    );
    assert!(
        msg.contains(&format!("{NANOS}ns")),
        "expected nanosecond duration fragment: {msg}"
    );
}

#[test]
fn shutdown_join_timeout_elapsed_message_contains_duration() {
    let dur = std::time::Duration::from_millis(500);
    let msg = shutdown_join_timeout_elapsed_message(dur);
    assert!(
        msg.contains("500ms"),
        "message should contain duration: {msg}"
    );
    assert!(
        msg.contains("shutdown"),
        "message should mention shutdown: {msg}"
    );
}

#[test]
fn shutdown_join_timeout_with_test_override() {
    *TEST_SHUTDOWN_JOIN_TIMEOUT_MS_OVERRIDE.lock().unwrap() = Some(42);
    let dur = shutdown_join_timeout();
    assert_eq!(dur, std::time::Duration::from_millis(42));
    *TEST_SHUTDOWN_JOIN_TIMEOUT_MS_OVERRIDE.lock().unwrap() = None;
}

#[test]
fn shutdown_join_timeout_default_without_override() {
    *TEST_SHUTDOWN_JOIN_TIMEOUT_MS_OVERRIDE.lock().unwrap() = None;
    let dur = shutdown_join_timeout();
    assert_eq!(dur, config::DEFAULT_SHUTDOWN_TIMEOUT);
}
