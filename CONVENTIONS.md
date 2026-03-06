# Coding Conventions

This primal follows the ecoPrimals coding conventions (modeled on wateringHole standards).

## Quick Reference

- **Edition**: 2024
- **MSRV**: 1.85
- **Linting**: `#![warn(clippy::all, clippy::pedantic)]`
- **Docs**: `#![warn(missing_docs)]`
- **Max file size**: 1000 LOC
- **Test coverage**: 90%+ target (structural floor from encoder match arms)
- **License**: AGPL-3.0-only (upstream-derived files retain original attribution)
- **Error handling**: `thiserror` for libraries, `Result` propagation throughout pipeline
- **Tooling**: `rustfmt.toml`, `clippy.toml`, `deny.toml` all configured

## Codegen Module Conventions

Compiler-derived code follows additional conventions:

- Large files are split into directory modules with logical submodules (`ir/`, `nv/sm70_encode/`, `naga_translate/`, `lower_f64/`)
- Submodules use `use super::*;` to access parent scope
- Proc macros in `nak-ir-proc` generate trait impls — prefer derives over manual impls
- `#[repr(C)]` is required on op structs for contiguous memory layout (used by `AsSlice`)
- `naga_translate/` translates naga IR to codegen IR
- `lower_f64/` expands f64 transcendental ops before legalization

## Vendor Backend Conventions

Vendor-specific code lives under namespaced directories within `codegen/`:

| Vendor | Module | ISA Reference | Register Model |
|--------|--------|---------------|----------------|
| NVIDIA | `codegen/nv/` | SASS (SM20–SM89) | GPR/UGPR/Pred/Carry/Bar |
| AMD | `codegen/amd/` | GFX10+ (RDNA2) | VGPR/SGPR/VCC |
| Intel | `codegen/intel/` (future) | Xe EU ISA | GRF |

Each vendor backend implements:
- `legalize.rs` — target-specific instruction lowering
- `assign_regs.rs` — register allocation for the vendor's register file
- `encode.rs` — instruction binary encoding
- `lower_f64.rs` — f64 strategy (native instructions or DFMA workaround)

Shared passes (copy propagation, DCE, scheduling, jump threading, etc.)
remain in `codegen/` and are vendor-agnostic. Only legalization,
register allocation, and encoding are vendor-specific.

The `Backend` trait in `backend.rs` and `GpuTarget` enum in
`gpu_arch.rs` are the extension points for new vendors.

## Naming Conventions

- Rust-idiomatic field names: `gpr_count` not `num_gprs`, `shared_mem_size` not `smem_size`
- No stuttering: `src.reference` not `src.src_ref`, `pred.predicate` not `pred.pred_ref`
- Vendor-neutral types: `TranscendentalOp` not `MuFuOp`, `GpuTarget` not `NvArch`
- Module paths: `codegen::ir` not `nak::ir`, `codegen::naga_translate` not `nak::from_spirv`
- AMD types follow AMD conventions: `Vgpr`/`Sgpr` not `Gpr`/`Ugpr`, `wave_size` not `warp_size`

## Error Handling

- All pipeline stages return `Result<_, CompileError>`
- Optimizer passes skip unrecognized patterns instead of panicking
- `debug_assert!` for internal invariants (panics only in debug builds)
- `CompileError` variants: `InvalidInput`, `NotImplemented`, `UnsupportedArch`
- Production `.unwrap()` → `.expect("invariant description")`

## f64 Lowering Conventions

- Virtual ops (`OpF64Sqrt`, `OpF64Sin`, etc.) are emitted by `naga_translate`
- The `lower_f64` pass expands them pre-legalize in `pipeline.rs`
- Polynomial coefficients are stored as `const f64` with full-precision hex comments
- Newton-Raphson iterations use `FRndMode::NearestEven` throughout
- AMD backend uses native `v_fma_f64` where available — no MUFU workaround needed

## Toolchain Sovereignty Policy

The Rust compiler is the DNA synthase of this project. Every tool in
the pipeline — from ISA spec parsing to binary encoding to GPU dispatch
— must be internal Rust by production release. Non-Rust tools (Python
scripts, C bindings, shell wrappers) are acceptable only as Pass 1
scaffolding and must be tracked for replacement.

### FFI (C/C++ interop)

FFI (`*-sys` crates, `bindgen`, raw ioctl structs) is acceptable as
scaffolding in early evolution passes. Every FFI introduction must:

1. Be documented with a tracking comment linking to its Rust replacement plan
2. Be isolated behind a safe Rust wrapper (no raw FFI in public API)
3. Have a test that validates behavior independent of the FFI layer
4. Be replaced with internal Rust by production release

### Non-Rust tooling (Python, shell, etc.)

Non-Rust tools used for code generation or build orchestration follow
the same evolution policy:

1. Generated output must be committed pure Rust (no runtime dependency on the tool)
2. The tool must be documented with its Rust replacement plan
3. The tool must be replaced with an internal Rust equivalent by Pass 3

Current scaffolding tools and their replacement plan:

| Tool | Purpose | Status | Rust replacement |
|------|---------|--------|------------------|
| ~~`tools/amd-isa-gen/gen_rdna2_opcodes.py`~~ | ~~Parse AMD ISA XML → Rust encoding tables~~ | **Replaced** | `tools/amd-isa-gen/` Rust binary (complete) |

No non-Rust tool survives to production. Each pass produces strictly
better Rust. The Rust language and compilation model is the competitive
advantage — anything else is a bandaid fix.

See `specs/SOVEREIGN_MULTI_GPU_EVOLUTION.md` for the full evolution
pass definitions and dependency tracking.

---

*Consistency is the foundation of collaboration. Rust is the foundation of sovereignty.*
