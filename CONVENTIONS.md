# Coding Conventions

This primal follows the ecoPrimals coding conventions (modeled on wateringHole standards).

## Quick Reference

- **Edition**: 2024
- **MSRV**: 1.85
- **Linting**: `#![warn(clippy::all, clippy::pedantic)]`
- **Docs**: `#![warn(missing_docs)]`
- **Max file size**: 1000 LOC (2 encoder files tracked for split)
- **Test coverage**: 90%+ target (~37% line — structural floor from encoder match arms)
- **License**: AGPL-3.0-only (upstream-derived files retain original attribution)
- **Error handling**: `thiserror` for libraries, `Result` propagation throughout pipeline

## Codegen Module Conventions

Compiler-derived code follows additional conventions:

- Large files are split into directory modules with logical submodules (`ir/`, `nv/sm70_encode/`, `naga_translate/`, `lower_f64/`)
- Submodules use `use super::*;` to access parent scope
- Proc macros in `nak-ir-proc` generate trait impls — prefer derives over manual impls
- `#[repr(C)]` is required on op structs for contiguous memory layout (used by `AsSlice`)
- `naga_translate/` translates naga IR to codegen IR
- `lower_f64/` expands f64 transcendental ops before legalization
- Vendor-specific code lives under `nv/` (NVIDIA), with `amd/` and `intel/` planned

## Naming Conventions

- Rust-idiomatic field names: `gpr_count` not `num_gprs`, `shared_mem_size` not `smem_size`
- No stuttering: `src.reference` not `src.src_ref`, `pred.predicate` not `pred.pred_ref`
- Vendor-neutral types: `TranscendentalOp` not `MuFuOp`, `GpuTarget` not `NvArch`
- Module paths: `codegen::ir` not `nak::ir`, `codegen::naga_translate` not `nak::from_spirv`

## Error Handling

- All pipeline stages return `Result<_, CompileError>`
- Optimizer passes skip unrecognized patterns instead of panicking
- `debug_assert!` for internal invariants (panics only in debug builds)
- `CompileError` variants: `InvalidInput`, `NotImplemented`, `UnsupportedArch`

## f64 Lowering Conventions

- Virtual ops (`OpF64Sqrt`, `OpF64Sin`, etc.) are emitted by `naga_translate`
- The `lower_f64` pass expands them pre-legalize in `pipeline.rs`
- Polynomial coefficients are stored as `const f64` with full-precision hex comments
- Newton-Raphson iterations use `FRndMode::NearestEven` throughout

---

*Consistency is the foundation of collaboration.*
