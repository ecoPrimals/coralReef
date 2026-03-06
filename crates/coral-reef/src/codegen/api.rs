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
use super::ir::*;

macro_rules! pass {
    ($s: expr, $pass: ident) => {
        $s.$pass();
        if DEBUG.print() {
            eprintln!("IR after {}:\n{}", stringify!($pass), $s);
        }
    };
}

pub(super) use pass;

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
