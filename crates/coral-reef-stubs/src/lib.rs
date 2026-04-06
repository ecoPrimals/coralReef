// SPDX-License-Identifier: AGPL-3.0-or-later
#![forbid(unsafe_code)]
#![warn(missing_docs)]
//! # coral-reef-stubs — Pure-Rust Dependency Replacements
//!
//! This crate provides standalone Rust replacements for upstream C dependencies
//! that the original compiler relied upon. Each sub-module replaces a specific
//! upstream crate or module.
//!
//! ## Replacement Map
//!
//! | Original dependency         | Replacement module          | Status |
//! |----------------------------|-----------------------------|--------|
//! | `compiler::cfg`            | [`mod@cfg`]                 | Evolved (CFG + dominator tree) |
//! | `compiler::dataflow`       | [`dataflow`]                | Evolved (worklist solver) |
//! | `compiler::bitset`         | [`bitset`]                  | Evolved (dense bitmap) |
//! | `compiler::smallvec`       | [`smallvec`]                | Evolved (zero/one/many) |
//! | `compiler::as_slice`       | [`as_slice`]                | Evolved (type-safe views) |
//! | `nvidia_headers`           | [`nvidia_headers`]          | Evolved (full QMD) |
//! | `nak_latencies`            | [`nak_latencies`]           | Evolved (SM100 latency model) |
//! | `rustc-hash`               | [`fxhash`]                  | Evolved (`FxHash` internalized) |
//!
//! All modules fully evolved to pure Rust with zero C dependencies.
//!

/// Replacement for `compiler::cfg` (control-flow graph).
pub mod cfg;

/// Replacement for `compiler::dataflow` (forward/backward analysis).
pub mod dataflow;

/// Replacement for `compiler::bitset`.
pub mod bitset;

/// Replacement for `compiler::smallvec`.
pub mod smallvec;

/// Replacement for `compiler::as_slice`.
pub mod as_slice;

/// Replacement for `nvidia_headers` (NVIDIA class definitions).
pub mod nvidia_headers;

/// Replacement for `nak_latencies` (instruction latency tables).
pub mod nak_latencies;

/// Fast non-cryptographic hash — replaces `rustc-hash` external crate.
pub mod fxhash;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_types_exist() {
        let _: bitset::BitSet<u32> = bitset::BitSet::new(8);
        let _ = cfg::CFGBuilder::<()>::new();
    }

    #[test]
    fn all_modules_compile() {
        let _: bitset::BitSet<u32> = bitset::BitSet::new(8);
        let _ = cfg::CFGBuilder::<()>::new();
        let _ = nvidia_headers::classes::clc3c0::VOLTA_COMPUTE_A;
    }
}
