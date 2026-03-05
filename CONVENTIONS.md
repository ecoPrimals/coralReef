# Coding Conventions

This primal follows the ecoPrimals coding conventions (modeled on wateringHole standards).

## Quick Reference

- **Edition**: 2024
- **MSRV**: 1.85
- **Linting**: `#![warn(clippy::all, clippy::pedantic)]`
- **Docs**: `#![warn(missing_docs)]`
- **Max file size**: 1000 LOC for new code; NAK-derived ALU encoder blocks (cohesive op impls) may exceed this
- **Test coverage**: 90%+
- **License**: AGPL-3.0-only (NAK-derived files in `crates/coral-nak/src/nak/` retain MIT)
- **Error handling**: `thiserror` for libraries, `anyhow` for CLI

## NAK Module Conventions

NAK-derived code follows additional conventions during the porting process:

- Large files are split into directory modules with logical submodules (`ir/`, `sm70_encode/`, `sm50/`, `sm32/`)
- Submodules use `use super::encoder::*;` or `use super::*;` to access parent scope
- Proc macros in `nak-ir-proc` generate trait impls — prefer derives over manual impls
- `#[repr(C)]` is required on op structs for contiguous memory layout (used by `AsSlice`)
- `from_nir.rs` is disabled — will be replaced by `from_spirv.rs` using naga

---

*Consistency is the foundation of collaboration.*
