// SPDX-License-Identifier: AGPL-3.0-or-later
#![cfg(target_os = "linux")]
#![allow(unsafe_code)]
//! `notify_watchdog` integration smoke test (Rust 2024 `remove_var` is `unsafe`).

use std::sync::Mutex;

static NOTIFY_ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn notify_watchdog_smoke_without_notify_socket() {
    let _g = NOTIFY_ENV_LOCK.lock().expect("lock");
    // SAFETY: test mutex serializes env access; no concurrent readers.
    unsafe {
        std::env::remove_var("NOTIFY_SOCKET");
    }
    coral_glowplug::test_support_notify_watchdog();
    coral_glowplug::test_support_notify_watchdog();
}
