// SPDX-License-Identifier: AGPL-3.0-only
//! # coral-reef-stubs — Mesa Dependency Replacements
//!
//! This crate provides standalone Rust replacements for Mesa C dependencies
//! that the original NAK compiler relied upon.  Each sub-module replaces a
//! specific Mesa crate or module.
//!
//! ## Replacement Map
//!
//! | Original Mesa dependency    | Replacement module          | Status |
//! |----------------------------|-----------------------------|--------|
//! | `compiler::cfg`            | [`mod@cfg`]                 | Evolved (CFG + dominator tree) |
//! | `compiler::dataflow`       | [`dataflow`]                | Evolved (worklist solver) |
//! | `compiler::bitset`         | [`bitset`]                  | Evolved (dense bitmap) |
//! | `compiler::smallvec`       | [`smallvec`]                | Evolved (zero/one/many) |
//! | `compiler::as_slice`       | [`as_slice`]                | Evolved (type-safe views) |
//! | `nvidia_headers`           | [`nvidia_headers`]          | Stub   |
//! | `nak_latencies`            | [`nak_latencies`]           | Evolved (SM100 latency model) |
//!
//! Legacy FFI stubs (`bindings`, `nir`, `nir_instr_printer`, `nak_bindings`) were removed in Phase 4.
//!
//! ## Evolution Strategy
//!
//! 1. **Phase 1**: Empty stubs to make `cargo check` parse the workspace *(complete)*
//! 2. **Phase 2**: Port core Mesa utility types to pure Rust *(complete — 6 modules evolved)*
//! 3. **Phase 3**: Replace NIR frontend with naga SPIR-V → coral-reef IR
//! 4. **Phase 4**: Remove remaining legacy FFI stubs *(complete)*
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
