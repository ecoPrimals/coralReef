# coralReef — What's Next

**Last updated**: March 5, 2026

---

## All Core Phases Complete

### Phase 1 — Scaffold
- [x] NAK sources extracted from Mesa (46 files, 51K LOC)
- [x] Mesa stub crate (`coral-reef-stubs`) with 7 evolved modules
- [x] ISA crate (`coral-reef-isa`) with SPH, latency tables

### Phase 1.5 — Foundation
- [x] License: AGPL-3.0-only (NAK files retain MIT)
- [x] UniBin binary target with `server`, `compile`, `doctor` subcommands
- [x] JSON-RPC 2.0 + tarpc IPC with semantic method names
- [x] Stubs evolved: BitSet, CFG, dataflow, SmallVec, nvidia_headers, nak_latencies, as_slice
- [x] `bitview` crate with `BitViewable`/`BitMutViewable`
- [x] `#[non_exhaustive]` on public enums
- [x] Capability-based discovery, zero-knowledge startup

### Phase 2 — Wire NAK Sources
- [x] `mod nak;` wired into `coral-reef/src/lib.rs`
- [x] `nak-ir-proc` proc-macro crate with 4 derives
- [x] Full compilation pipeline wired via `pipeline.rs`
- [x] `compile()` and `compile_wgsl()` public API
- [x] SM100 latency stubs evolved to real implementations

### Phase 2.5–2.9 — Refactoring & Debt Reduction
- [x] All oversized files split into directory modules (ir/, sm70_encode/, sm50/, sm32/, sm20/)
- [x] Dead code removed (8.7K LOC)
- [x] Clippy pedantic with zero warnings workspace-wide
- [x] `assign_regs/`, `calc_instr_deps/`, `spill_values/`, `builder/` all split
- [x] `llvm-cov` coverage configured
- [x] Standalone `PrimalLifecycle` (zero external primal dependency)

### Phase 3 — naga Frontend
- [x] `from_spirv/` module: naga IR → NAK SSA IR (3 files, 2023 LOC)
- [x] WGSL and SPIR-V parsing via naga (`wgsl-in`, `spv-in`)
- [x] Expression translation: literals, binary/unary, math, select, cast, compose, splat, swizzle
- [x] Statement translation: emit, store, if/else, loop, barrier, kill
- [x] Builtin resolution: GlobalInvocationId, LocalInvocationId, WorkGroupId, SubgroupInvocationId
- [x] Memory: global load/store, array/struct access with stride calculation
- [x] CFG construction with branch/merge/back-edge patterns
- [x] f64 expression detection (`is_f64_expr()`) routing to f64 lowering ops

### Phase 3.5 — Deep Debt
- [x] 4 legacy FFI stubs deleted (bindings, nir, nir_instr_printer, nak_bindings)
- [x] `paste` dependency removed, `tokio` features narrowed
- [x] `from_spirv` Result propagation (`.expect()` → `Result<_, CompileError>`)
- [x] Zero-copy IPC: `CompileResponse::binary` → `bytes::Bytes`
- [x] Latency files split (sm75: 3 files, sm80: 4 files)

### Phase 4 — f64 Software Lowering
- [x] `lower_f64/` module (3 files, 1557 LOC total)
- [x] sqrt: MUFU.RSQ64H + 2 Newton-Raphson iterations via DFMA (full f64)
- [x] rcp: MUFU.RCP64H + 2 Newton-Raphson iterations via DFMA (full f64)
- [x] exp2: Range reduction (F2I/I2F + DAdd) + degree-6 Horner polynomial + ldexp
- [x] log2: MUFU.LOG2 seed + Newton refinement (EX2/RCP/FMul correction, ~46-bit)
- [x] sin: Cody-Waite range reduction + minimax polynomial + quadrant correction
- [x] cos: Cody-Waite range reduction + minimax polynomial + quadrant correction
- [x] 6 virtual IR ops expanded pre-legalize in pipeline

### Phase 4.5 — Error Safety
- [x] `shader_info.rs`: 8 production panics → `Result<_, CompileError>`
- [x] `legalize()`, `gather_info()`, `encode_shader()` all return `Result`
- [x] Pipeline fully fallible with `?` chain
- [x] `opt_copy_prop` (17), `opt_bar_prop` (7), `builder` (4), `ir/program` (8) — panics evolved to skip/fallback

### Phase 5 — Standalone
- [x] All 7 stub modules evolved (nvidia_headers with QMD for Kepler through Blackwell)
- [x] Dependencies aligned: tokio 1.50, tarpc 0.37, tokio-util workspace-aligned
- [x] 0 files >1000 LOC
- [x] 390 tests, 37.1% line coverage

---

## Future Work

### Precision Improvements
- [ ] log2 Newton refinement: second iteration for full f64 (~52-bit)
- [ ] exp2 edge cases: subnormal handling in ldexp
- [ ] sin/cos: extended precision constants for large argument reduction

### Coverage
- [ ] SM70 instruction-level encoder tests (each instruction type)
- [ ] SM20/32/50 legacy encoder tests (if these architectures are retained)
- [ ] IR op struct exercising via proc-macro-generated code

### Ecosystem
- [ ] Phase 6: coralDriver — userspace GPU driver
- [ ] Phase 7: coralGpu — unified Rust GPU abstraction
- [ ] barraCuda integration: WGSL → coral-reef → native binary → coralDriver → GPU
- [ ] crates.io publication

### Debt
- [ ] ~130 `panic!` in latency tables (internal invariants, low priority)
- [ ] 34 TODO comments in encoder/scheduler code (optional instruction features)
- [ ] naga upgrade from 24 to 28 (evaluate changelog for breaking changes)

---

*All core compiler functionality is complete. Future work focuses on precision refinement, ecosystem integration, and coverage hardening.*
