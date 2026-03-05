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
- [x] 193 tests passing
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

### Phase 2.75 — Debt Reduction
- [x] Full workspace `clippy --all-targets -D warnings` passes (was 735+ errors)
- [x] NAK module-level allows with documented justifications
- [x] `Box<dyn Error>` → concrete `IpcError` type with `thiserror`
- [x] SPDX header added to `sm30_instr_latencies.rs` (was missing)
- [x] STUB_MARKER debt flags removed from cfg.rs and bindings.rs
- [x] Stubs documentation evolved: 6 modules marked "Evolved", 6 remain "Stub (legacy FFI)"
- [x] `unsafe` in nak-ir-proc: compile-time layout assertions + runtime debug checks added
- [x] `assign_regs.rs` (1511 LOC) → `assign_regs/` directory (5 files, all <500 LOC)
- [x] `calc_instr_deps.rs` (1176 LOC) → `calc_instr_deps/` directory (3 files, all <720 LOC)
- [x] `spill_values.rs` (1100 LOC) → `spill_values/` directory (3 files, all <600 LOC)
- [x] `builder.rs` (1063 LOC) → `builder/` directory (2 files, all <810 LOC)
- [x] `llvm-cov` configured with `scripts/coverage.sh` (11.83% baseline)
- [x] `bitview` crate: `BitCastU64: Copy` bound, signed cast safety documented
- [x] `cfg.rs` `compute_dom_analysis` refactored from 155-line monolith to 5 focused methods
- [x] `dataflow.rs` `solve()` methods: `# Panics` docs added
- [x] `nak_latencies.rs` match arms consolidated with nested or-patterns
- [x] `nir_instr_printer` `#[deprecated]` removed (module docs state legacy status)

### Phase 2.8 — Sovereignty
- [x] `sourdough-core` dependency removed — standalone `PrimalLifecycle`, `PrimalHealth`, `PrimalError`, `HealthReport`, `HealthStatus` (modeled on sourDough, zero compile-time coupling)
- [x] CI no longer clones sourDough repo
- [x] Dead `compiler_proc` stub module removed
- [x] All docs updated to reflect standalone status

## Safe Rust Evolution

| Pattern | Count | Strategy |
|---------|-------|----------|
| `panic!()` | ~596 | Convert to `Result` returns, use `thiserror` (Phase 5) |
| `.unwrap()` | ~287 | Replace with `?`, `.ok_or()`, `.expect("reason")` (Phase 5) |
| `unsafe` | 2 | nak-ir-proc only; compile-time + runtime safety checks added |
| `#[allow(...)]` | ~40 | NAK module: documented justifications; non-NAK: all resolved |
