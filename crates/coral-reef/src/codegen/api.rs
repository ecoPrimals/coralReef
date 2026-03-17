// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022) — upstream NAK.
//! Internal compiler pipeline — pure Rust, no C FFI.
//!
//! The public Rust API lives in `coral-reef/src/lib.rs`.
//! This module provides the internal compilation pipeline, connected
//! via `naga_translate` (naga frontend).

#![allow(clippy::wildcard_imports)]

pub use super::debug::{DEBUG, GetDebugFlags};

pub(super) fn eprint_hex(label: &str, data: &[u32]) {
    eprint!("{label}:");
    for (i, word) in data.iter().enumerate() {
        if (i % 8) == 0 {
            eprintln!();
            eprint!(" ");
        }
        eprint!(" {word:08x}");
    }
    eprintln!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eprint_hex_no_panic() {
        // eprint_hex prints to stderr; verify it doesn't panic
        eprint_hex("test", &[0x1234_5678, 0xabcd_ef00]);
        eprint_hex("empty", &[]);
        eprint_hex("single", &[0xdead_beef]);
    }

    #[test]
    fn test_debug_re_export() {
        // Verify DEBUG and GetDebugFlags are re-exported and usable
        let _ = &DEBUG;
        assert!(!DEBUG.print());
        assert!(!DEBUG.annotate());
    }
}
