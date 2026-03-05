# coralNak — What's Next

**Last updated**: March 5, 2026

---

## Completed

### Phase 1 — Scaffold
- [x] NAK sources extracted from Mesa (46 files, 51K LOC)
- [x] Mesa stub crate (`coral-nak-stubs`) with 12 modules
- [x] ISA crate (`coral-nak-isa`) with SPH, latency tables

### Phase 1.5 — Foundation
- [x] License: AGPL-3.0-only (NAK files retain MIT)
- [x] UniBin binary target with `server`, `compile`, `doctor` subcommands
- [x] JSON-RPC 2.0 + tarpc IPC with semantic method names
- [x] Stubs evolved: BitSet → dense bitmap, CFG → dominator tree, dataflow → worklist solver
- [x] SmallVec evolved to stack-optimized enum (None/One/Many)
- [x] 183 tests passing
- [x] `bitview` crate: `BitViewable`/`BitMutViewable` with `BitCastU64`
- [x] `#[non_exhaustive]` on public enums
- [x] CONTRIBUTING.md, capability-based discovery

### Phase 2 — Wire NAK Sources
- [x] `mod nak;` wired into `coral-nak/src/lib.rs`
- [x] `nak-ir-proc` proc-macro crate: `SrcsAsSlice`, `DstsAsSlice`, `DisplayOp`, `FromVariants`
- [x] Enum support added to `SrcsAsSlice`/`DstsAsSlice` (boxed + unboxed variants)
- [x] All `extern crate` removed, imports rewritten
- [x] `from_nir` module removed (replaced by Phase 3)
- [x] Full compilation pipeline wired via `pipeline.rs` (16 passes + encoding)
- [x] `compile_ir()` public API for pre-built IR
- [x] All compilation errors resolved (0 errors)
- [x] SM100 latency stubs evolved to real implementations
- [x] QMD stubs deprecated (hardware dispatch is another primal's capability)

### Phase 2.5 — Smart Refactoring & Cleanup
- [x] `ir.rs` (9,816 lines) → `ir/` directory with 12 submodules
- [x] `sm70_encode.rs` (4,227 lines) → `sm70_encode/` with 6 submodules
- [x] `sm50.rs` (3,470 lines) → `sm50/` with 6 submodules
- [x] `sm32.rs` (3,415 lines) → `sm32/` with 6 submodules
- [x] `sm20.rs` (3,129 lines) → `sm20/` with 6 submodules
- [x] Dead code removed: `from_nir.rs`, `qmd.rs`, `hw_runner.rs`, `hw_tests.rs`, `nvdisasm_tests.rs`, `ir_proc.rs` (8.7K LOC)
- [x] Backup files removed (`lib.rs.bak`)
- [x] Empty directories cleaned (`archive/`, `tests/integration/`, genomebin scaffolding)
- [x] Blanket `#[allow]` narrowed to specific justified attributes
- [x] Clippy warnings reduced from 1450 to ~968

### Sovereignty & Discovery
- [x] Zero-knowledge startup — no hardcoded primal names in production code
- [x] Capability-based self-description (`capability.rs`)
- [x] `GpuArch::ALL`, `GpuArch::parse()`, `GpuArch::default()`, `FromStr` — no hardcoded arch lists
- [x] Named constants for bind addresses (`DEFAULT_BIND`)
- [x] `env!("CARGO_PKG_NAME")` for all self-identification
- [x] Universal adapter integration points in server startup

## Immediate — Phase 3: SPIR-V Frontend

| Priority | Task |
|----------|------|
| 1 | Create `from_spirv.rs` using naga's SPIR-V → IR translation |
| 2 | Wire `compile()` to use `from_spirv` instead of returning `NotImplemented` |
| 3 | End-to-end test: SPIR-V compute shader → native binary |

## Phase 4 — f64 Software Lowering

1. Implement DFMA-based f64 transcendentals per `whitePaper/F64_LOWERING_THEORY.md`
2. Functions: sin, cos, exp2, log2, sqrt, rcp
3. Validate against libm reference (ULP accuracy)

## Phase 5 — Standalone

1. Remove remaining Mesa-derived stubs
2. Replace `panic!()`/`unwrap()` with `Result` propagation
3. All files under 1000 lines
4. Publish to crates.io

## Safe Rust Evolution

| Pattern | Count | Strategy |
|---------|-------|----------|
| `panic!()` | ~596 | Convert to `Result` returns, use `thiserror` |
| `.unwrap()` | ~287 | Replace with `?`, `.ok_or()`, `.expect("reason")` |
| `unsafe` | ~68 | Remove C FFI (Phase 5), use safe abstractions |
| `#[allow(...)]` | ~58 | Audit each, fix underlying issue or document reason |
