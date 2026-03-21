// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022) — upstream NAK.
//! Internal compiler pipeline — pure Rust, no C FFI.
//!
//! The public Rust API lives in `coral-reef/src/lib.rs`.
//! This module provides the internal compilation pipeline, connected
//! via `naga_translate` (naga frontend).

pub use super::debug::{DEBUG, GetDebugFlags};

pub(super) fn debug_hex(label: &str, data: &[u32]) {
    use std::fmt::Write;
    let mut buf = String::with_capacity(data.len() * 10 + label.len() + 8);
    let _ = write!(buf, "{label}:");
    for (i, word) in data.iter().enumerate() {
        if (i % 8) == 0 {
            buf.push('\n');
            buf.push(' ');
        }
        let _ = write!(buf, " {word:08x}");
    }
    tracing::debug!("{buf}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_hex_no_panic() {
        debug_hex("test", &[0x1234_5678, 0xabcd_ef00]);
        debug_hex("empty", &[]);
        debug_hex("single", &[0xdead_beef]);
    }

    #[test]
    fn test_debug_re_export() {
        // Verify DEBUG and GetDebugFlags are re-exported and usable
        let _ = &DEBUG;
        assert!(!DEBUG.print());
        assert!(!DEBUG.annotate());
    }
}
