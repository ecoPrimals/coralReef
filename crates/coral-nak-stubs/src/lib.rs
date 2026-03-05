// SPDX-License-Identifier: AGPL-3.0-only
//! # coral-nak-stubs — Mesa Dependency Replacements
//!
//! This crate provides standalone Rust replacements for Mesa C dependencies
//! that the original NAK compiler relied upon.  Each sub-module replaces a
//! specific Mesa crate or module.
//!
//! ## Replacement Map
//!
//! | Original Mesa dependency    | Replacement module          | Status |
//! |----------------------------|-----------------------------|--------|
//! | `compiler::bindings`       | [`bindings`]                | Stub   |
//! | `compiler::cfg`            | [`mod@cfg`]                 | Stub   |
//! | `compiler::dataflow`       | [`dataflow`]                | Stub   |
//! | `compiler::bitset`         | [`bitset`]                  | Stub   |
//! | `compiler::smallvec`       | [`smallvec`]                | Stub   |
//! | `compiler::as_slice`       | [`as_slice`]                | Stub   |
//! | `compiler::nir`            | [`nir`]                     | Stub   |
//! | `compiler::nir_instr_printer` | [`nir_instr_printer`]    | Stub   |
//! | `compiler_proc::as_slice`  | [`compiler_proc`]           | Stub   |
//! | `nak_bindings`             | [`nak_bindings`]            | Stub   |
//! | `nvidia_headers`           | [`nvidia_headers`]          | Stub   |
//! | `nak_latencies`            | [`nak_latencies`]           | Stub   |
//!
//! ## Evolution Strategy
//!
//! 1. **Phase 1 (current)**: Empty stubs to make `cargo check` parse the workspace
//! 2. **Phase 2**: Port core Mesa utility types to pure Rust
//! 3. **Phase 3**: Replace NIR frontend with naga SPIR-V → coral-nak IR
//! 4. **Phase 4**: Remove stubs entirely once all Mesa deps are replaced

/// Replacements for `compiler::bindings::*` (Mesa C FFI structs).
#[deprecated(note = "Legacy Mesa FFI — will be removed when from_nir is replaced by from_spirv")]
pub mod bindings;

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

/// Replacement for `compiler::nir` (NIR IR types).
#[deprecated(note = "Legacy Mesa FFI — will be removed when from_nir is replaced by from_spirv")]
pub mod nir;

/// Replacement for `compiler::nir_instr_printer`.
#[deprecated(note = "Legacy Mesa FFI — will be removed when from_nir is replaced by from_spirv")]
pub mod nir_instr_printer;

/// Replacement for `compiler_proc::as_slice` (proc macro helpers).
#[deprecated(
    note = "Legacy — nak_ir_proc uses coral_nak_stubs::as_slice directly; this stub is unused"
)]
pub mod compiler_proc;

/// Replacement for `nak_bindings::*` (NAK-specific C bindings).
#[deprecated(note = "Legacy Mesa FFI — will be removed when from_nir is replaced by from_spirv")]
pub mod nak_bindings;

/// Replacement for `nvidia_headers` (NVIDIA class definitions).
pub mod nvidia_headers;

/// Replacement for `nak_latencies` (instruction latency tables).
pub mod nak_latencies;

#[cfg(test)]
mod tests {
    #![allow(deprecated)]

    use super::*;

    #[test]
    fn stubs_exist() {
        let _ = bindings::STUB_MARKER;
        let _ = cfg::STUB_MARKER;
    }

    #[test]
    fn all_modules_compile() {
        let _: bitset::BitSet<u32> = bitset::BitSet::new(8);
        let _ = cfg::CFGBuilder::<()>::new();
        let _ = nir_instr_printer::NirInstrPrinter::new();
        let _ = nir::nir_shader;
        let _ = nak_bindings::nak_compiler { sm: 70 };
        let _ = nvidia_headers::classes::clc3c0::VOLTA_COMPUTE_A;
        let _ = compiler_proc::as_slice::AsSliceDerive;
    }
}
