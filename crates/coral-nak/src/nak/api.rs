// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! Internal compiler pipeline — pure Rust, no C FFI.
//!
//! The Mesa C API (`extern "C"` entry points, bindgen types, NIR integration)
//! has been removed. The public Rust API lives in `coral-nak/src/lib.rs`.
//! This module provides the internal compilation pipeline that will be
//! connected once `from_spirv` replaces `from_nir` (Phase 3).

#![allow(clippy::wildcard_imports)]

pub use super::debug::{DEBUG, GetDebugFlags};
use super::ir::*;

macro_rules! pass {
    ($s: expr, $pass: ident) => {
        $s.$pass();
        if DEBUG.print() {
            eprintln!("NAK IR after {}:\n{}", stringify!($pass), $s);
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
