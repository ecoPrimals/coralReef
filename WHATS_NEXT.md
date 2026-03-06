# coralReef — What's Next

**Last updated**: March 5, 2026

---

## All Core Phases Complete

### Phase 1 — Scaffold
- [x] Compiler sources extracted from upstream (46 files, 51K LOC)
- [x] Stub crate (`coral-reef-stubs`) with 7 evolved modules
- [x] ISA crate (`coral-reef-isa`) with latency tables

### Phase 1.5 — Foundation
- [x] License: AGPL-3.0-only (upstream-derived files retain original attribution)
- [x] UniBin binary target with `server`, `compile`, `doctor` subcommands
- [x] JSON-RPC 2.0 + tarpc IPC with semantic method names
- [x] Stubs evolved: BitSet, CFG, dataflow, SmallVec, fxhash, nvidia_headers, as_slice
- [x] `bitview` crate with `BitViewable`/`BitMutViewable`
- [x] `#[non_exhaustive]` on public enums
- [x] Capability-based discovery, zero-knowledge startup

### Phase 2 — Wire Compiler Sources
- [x] `mod codegen;` wired into `coral-reef/src/lib.rs`
- [x] `nak-ir-proc` proc-macro crate with 4 derives
- [x] Full compilation pipeline wired via `pipeline.rs`
- [x] `compile()` and `compile_wgsl()` public API
- [x] SM100 latency stubs evolved to real implementations

### Phase 2.5–2.9 — Refactoring & Debt Reduction
- [x] All oversized files split into directory modules (ir/, nv/sm70_encode/, nv/sm50/, nv/sm32/, nv/sm20/)
- [x] Dead code removed (8.7K LOC)
- [x] Clippy pedantic with zero warnings workspace-wide
- [x] `assign_regs/`, `calc_instr_deps/`, `spill_values/`, `builder/` all split
- [x] `llvm-cov` coverage configured
- [x] Standalone `PrimalLifecycle` (zero external primal dependency)

### Phase 3 — naga Frontend
- [x] `naga_translate/` module: naga IR → codegen SSA IR (3 files)
- [x] WGSL and SPIR-V parsing via naga (`wgsl-in`, `spv-in`)
- [x] Expression translation: literals, binary/unary, math, select, cast, compose, splat, swizzle
- [x] Statement translation: emit, store, if/else, loop, barrier, kill
- [x] Builtin resolution: GlobalInvocationId, LocalInvocationId, WorkGroupId, SubgroupInvocationId
- [x] Memory: global load/store, array/struct access with stride calculation
- [x] CFG construction with branch/merge/back-edge patterns
- [x] f64 expression detection routing to f64 lowering ops

### Phase 3.5 — Deep Debt
- [x] 4 legacy FFI stubs deleted (bindings, nir, nir_instr_printer, nak_bindings)
- [x] `paste` dependency removed, `tokio` features narrowed
- [x] `naga_translate` Result propagation (`.expect()` → `Result<_, CompileError>`)
- [x] Zero-copy IPC: `CompileResponse::binary` → `bytes::Bytes`
- [x] Latency files split (sm75: 3 files, sm80: 4 files)

### Phase 4 — f64 Software Lowering
- [x] `lower_f64/` module (3 files, 1557 LOC total)
- [x] sqrt: Transcendental Rsq64H + 2 Newton-Raphson iterations via DFMA (full f64)
- [x] rcp: Transcendental Rcp64H + 2 Newton-Raphson iterations via DFMA (full f64)
- [x] exp2: Range reduction (F2I/I2F + DAdd) + degree-6 Horner polynomial + ldexp
- [x] log2: Transcendental Log2 seed + Newton refinement (Exp2/Rcp/FMul correction, ~46-bit)
- [x] sin: Cody-Waite range reduction + minimax polynomial + quadrant correction
- [x] cos: Cody-Waite range reduction + minimax polynomial + quadrant correction
- [x] 6 virtual IR ops expanded pre-legalize in pipeline

### Phase 4.5 — Error Safety
- [x] `shader_info.rs`: 8 production panics → `Result<_, CompileError>`
- [x] `legalize()`, `gather_info()`, `encode_shader()` all return `Result`
- [x] Pipeline fully fallible with `?` chain
- [x] `opt_copy_prop` (17), `opt_bar_prop` (7), `builder` (4), `ir/program` (8) — panics evolved to skip/fallback

### Phase 5 — Standalone
- [x] All stub modules evolved (nvidia_headers with QMD for Kepler through Blackwell)
- [x] Dependencies aligned: tokio 1.50, tarpc 0.37, tokio-util workspace-aligned
- [x] 672 tests, zero clippy warnings
- [x] Zero-knowledge startup, capability-based discovery

### Phase 5.5 — Naming Evolution
- [x] `nak/` → `codegen/` module rename
- [x] `from_spirv/` → `naga_translate/` module rename
- [x] `MuFuOp` → `TranscendentalOp` (vendor-neutral)
- [x] C-style fields → Rust idiomatic (`num_gprs` → `gpr_count`, `src_ref` → `reference`, etc.)
- [x] `src.src_ref/src_mod/src_swizzle` → `src.reference/modifier/swizzle`
- [x] `pred.pred_ref/pred_inv` → `pred.predicate/inverted`
- [x] Copyright headers standardized (AGPL-3.0-only, 129 files)
- [x] All Mesa/NIR/NAK references evolved in compiler core docs
- [x] Blanket `#[allow]` list categorized and justified

---

## Future Work

### Phase 6 — Multi-Vendor Backends
- [ ] AMD backend: RDNA3/RDNA4 instruction encoding via `Backend` trait
- [ ] Intel backend: Xe/Xe2 instruction encoding via `Backend` trait
- [ ] Vendor-agnostic mid-level IR verified against all three backends
- [ ] `Fp64Strategy` (Native/Hybrid/Concurrent) added to `CompileOptions`

### Precision Improvements
- [ ] log2 Newton refinement: second iteration for full f64 (~52-bit)
- [ ] exp2 edge cases: subnormal handling in ldexp
- [ ] sin/cos: extended precision constants for large argument reduction

### Coverage
- [ ] SM70 instruction-level encoder tests (each instruction type)
- [ ] SM20/32/50 legacy encoder tests (if these architectures are retained)
- [ ] IR op struct exercising via proc-macro-generated code

### Hardware Testing
- [ ] RTX 3090 (SM86): end-to-end compilation + GPU execution validation
- [ ] Titan V (SM70): reproduce and fix upstream f64 issues
- [ ] AMD RDNA3: backend smoke tests once AMD encoding lands
- [ ] Cross-tower CI: validate on both 3090+AMD and 3090+Titan rigs

### Ecosystem
- [ ] Phase 7: coralDriver — userspace GPU driver
- [ ] Phase 8: coralGpu — unified Rust GPU abstraction
- [ ] barraCuda integration: WGSL → coral-reef → native binary → coralDriver → GPU
- [ ] crates.io publication

### Remaining Debt
- [ ] ~100 production `unwrap()` calls → `expect()` or `Result` propagation
- [ ] 28 TODO/FIXME comments in encoder/scheduler (optional instruction features)
- [ ] 1 `unimplemented!` in IPC (WGSL compile endpoint)
- [ ] 2 files over 1000 LOC (`op_float.rs` 1320, `op_misc.rs` 1009) — split candidates
- [ ] Rename `CoralNakPrimal` / `CoralNakRpc` / `CoralNakTarpc` → `CoralReefPrimal` etc.
- [ ] Update remaining NAK references in stub/core crate docs
- [ ] Duplicate MIT header in `cmp.rs` line 4

---

*All core compiler functionality is complete. Future work focuses on
multi-vendor backends, hardware validation, precision refinement, and
ecosystem integration.*
