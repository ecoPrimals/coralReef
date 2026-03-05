# Coding Conventions

This primal follows the ecoPrimals coding conventions (modeled on wateringHole standards).

## Quick Reference

- **Edition**: 2024
- **MSRV**: 1.85
- **Linting**: `#![warn(clippy::all, clippy::pedantic)]`
- **Docs**: `#![warn(missing_docs)]`
- **Max file size**: 1000 LOC (all files currently comply)
- **Test coverage**: 90%+ target (37.1% line — structural floor from encoder match arms)
- **License**: AGPL-3.0-only (NAK-derived files in `crates/coral-reef/src/nak/` retain MIT)
- **Error handling**: `thiserror` for libraries, `Result` propagation throughout pipeline

## NAK Module Conventions

NAK-derived code follows additional conventions:

- Large files are split into directory modules with logical submodules (`ir/`, `sm70_encode/`, `from_spirv/`, `lower_f64/`)
- Submodules use `use super::*;` to access parent scope
- Proc macros in `nak-ir-proc` generate trait impls — prefer derives over manual impls
- `#[repr(C)]` is required on op structs for contiguous memory layout (used by `AsSlice`)
- `from_spirv/` translates naga IR to NAK IR (replaced the deleted `from_nir.rs`)
- `lower_f64/` expands f64 transcendental ops before legalization

## Error Handling

- All pipeline stages return `Result<_, CompileError>`
- Optimizer passes skip unrecognized patterns instead of panicking
- `debug_assert!` for internal invariants (panics only in debug builds)
- `CompileError` variants: `InvalidInput`, `NotImplemented`, `UnsupportedArch`

## f64 Lowering Conventions

- Virtual ops (`OpF64Sqrt`, `OpF64Sin`, etc.) are emitted by `from_spirv`
- The `lower_f64` pass expands them pre-legalize in `pipeline.rs`
- Polynomial coefficients are stored as `const f64` with full-precision hex comments
- Newton-Raphson iterations use `FRndMode::NearestEven` throughout

---

*Consistency is the foundation of collaboration.*
