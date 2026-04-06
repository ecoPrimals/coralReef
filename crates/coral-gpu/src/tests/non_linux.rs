// SPDX-License-Identifier: AGPL-3.0-or-later
//! Compile-only fallback tests for non-Linux platforms.

use super::common::*;

#[test]
fn auto_context_compiles_without_device() {
    let ctx = crate::GpuContext::auto();
    assert!(
        ctx.is_ok(),
        "auto() on non-Linux should create a compile-only context"
    );
}
