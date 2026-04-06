// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals

//! Helpers for hardware-backed unit tests — discover any local GPU or skip.

use crate::GpuContext;

/// Discover whatever GPU is available locally and create a [`GpuContext`] for it.
///
/// Returns [`None`] if no GPU is found (the test should return early / skip).
#[must_use]
pub fn discover_local_gpu() -> Option<GpuContext> {
    #[cfg(not(target_os = "linux"))]
    {
        eprintln!("No local GPU: hardware discovery requires Linux");
        return None;
    }
    #[cfg(target_os = "linux")]
    match GpuContext::auto() {
        Ok(ctx) if ctx.has_device() => {
            eprintln!(
                "Discovered GPU: {} {}",
                ctx.target().vendor(),
                ctx.target().arch_name()
            );
            Some(ctx)
        }
        Ok(_) => {
            eprintln!("No local GPU available: compile-only context (no device attached)");
            None
        }
        Err(e) => {
            eprintln!("No local GPU available: {e}");
            None
        }
    }
}

/// Skip a test if no GPU is present. Use as `let ctx = require_gpu!();` at the start of a hardware test.
#[macro_export]
macro_rules! require_gpu {
    () => {
        match $crate::tests::local_gpu::discover_local_gpu() {
            Some(ctx) => ctx,
            None => {
                eprintln!("SKIPPED: no GPU available");
                return;
            }
        }
    };
}

#[cfg(test)]
mod smoke {
    #[test]
    fn require_gpu_macro_binds_ctx() {
        let ctx = crate::require_gpu!();
        assert!(!ctx.target().arch_name().is_empty());
    }
}
