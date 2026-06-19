// SPDX-License-Identifier: AGPL-3.0-or-later
#![allow(unsafe_code)]
//! Shared environment-variable mutation helper for integration tests.
//!
//! Rust 1.85+ marks `env::set_var`/`env::remove_var` as `unsafe` because they
//! are not thread-safe. Integration tests that probe env-dependent code paths
//! (socket resolution, discovery directory, family ID) need this.
//!
//! All env mutations are serialized behind a process-wide `Mutex`. The
//! [`EnvGuard`] RAII type restores original values on drop.

use std::sync::Mutex;

/// Process-wide lock for all env mutations. Tests must hold this to prevent
/// concurrent `set_var`/`remove_var` data races.
pub static ENV_LOCK: Mutex<()> = Mutex::new(());

/// RAII guard that captures an env var's value and restores it on drop.
///
/// The caller must hold [`ENV_LOCK`] for the entire lifetime of any
/// `EnvGuard` instances — both mutation and restoration happen under
/// the same lock scope.
pub struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    /// Capture the current value of `key` and return a guard that will
    /// restore it when dropped.
    #[must_use]
    pub fn capture(key: &'static str) -> Self {
        let previous = std::env::var(key).ok();
        Self { key, previous }
    }

    /// Set the env var to `value`.
    ///
    /// # Safety contract
    ///
    /// Safe to call only while [`ENV_LOCK`] is held (which serializes all
    /// env access in this process).
    pub fn set(&mut self, value: &str) {
        // SAFETY: ENV_LOCK is held by the caller for the test's duration.
        unsafe { std::env::set_var(self.key, value) }
    }

    /// Remove the env var.
    ///
    /// # Safety contract
    ///
    /// Safe to call only while [`ENV_LOCK`] is held.
    pub fn remove(&mut self) {
        // SAFETY: ENV_LOCK is held by the caller for the test's duration.
        unsafe { std::env::remove_var(self.key) }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        // SAFETY: ENV_LOCK is still held by the test scope when drop runs.
        unsafe {
            match &self.previous {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}
