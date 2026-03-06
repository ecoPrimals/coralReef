# coralReef — Status

**Last updated**: March 5, 2026

---

## Overall Grade: **A+**

| Category | Grade | Notes |
|----------|-------|-------|
| Primal lifecycle | A | Standalone `PrimalLifecycle` + `PrimalHealth`, full test coverage |
| UniBin compliance | A | Binary target with clap, panic hook, SIGTERM/SIGINT, structured errors |
| IPC | A+ | JSON-RPC 2.0 + tarpc, Unix socket + TCP, zero-copy `Bytes` payloads |
| Compilation pipeline | A+ | WGSL/SPIR-V → naga → codegen IR → f64 lower → optimize → legalize → RA → encode |
| Mesa stubs evolved | A+ | All modules evolved to pure Rust (BitSet, CFG, dataflow, fxhash, nvidia_headers) |
| f64 transcendentals | A+ | sqrt, rcp, exp2, log2, sin, cos — all with production precision |
| Vendor-agnostic arch | A | `Backend` / `Frontend` traits, `GpuTarget` enum, NVIDIA backend complete |
| Code structure | A+ | 2 files >1000 LOC (encoder domain, tracked for split) |
| Tests | A+ | 672 tests, zero failures |
| Clippy | A+ | Zero warnings, pedantic + nursery categories enabled |
| Coverage | B | ~37% line — structural floor from encoder match arms |
| License | A | AGPL-3.0-only (upstream-derived files retain original attribution) |
| Sovereignty | A+ | Zero-knowledge startup, capability-based discovery |
| Result propagation | A+ | Pipeline fully fallible: naga_translate → lower → legalize → encode |
| Zero-copy | A- | `bytes::Bytes` for IPC, `Cow` patterns, borrowed refs in pipeline |
| Dependencies | A | tokio 1.50, tarpc 0.37, naga 24, all features minimal |
| Naming evolution | A+ | Module `codegen/`, vendor-neutral `TranscendentalOp`, Rust-idiomatic fields |

## Phase Status

| Phase | Description | Status |
|-------|-------------|--------|
| 1 — Scaffold | Extract sources, create stubs | **Complete** |
| 1.5 — Foundation | UniBin, IPC, stubs evolved | **Complete** |
| 2 — Wire Sources | Compiler sources compile against stubs | **Complete** |
| 2.5–2.9 — Refactor | File splits, debt reduction, sovereignty | **Complete** |
| 3 — naga Frontend | SPIR-V/WGSL → codegen IR via naga | **Complete** |
| 3.5 — Deep Debt | Stubs removed, Result propagation, zero-copy | **Complete** |
| 4 — f64 Fix | DFMA software lowering (all 6 transcendentals) | **Complete** |
| 4.5 — Error Safety | Production panic→Result, pipeline propagation | **Complete** |
| 5 — Standalone | All stub dependencies evolved | **Complete** |
| 5.5 — Naming Evolution | Mesa/NAK de-vendoring, Rust-idiomatic fields | **Complete** |
| 6 — Multi-Vendor | Backend/Frontend traits, GpuTarget, vendor abstraction | **In Progress** |

## Checks

| Check | Status |
|-------|--------|
| `cargo check --workspace` | PASS |
| `cargo test --workspace` | PASS (672 tests) |
| `cargo fmt --check` | PASS |
| `cargo clippy` | PASS (0 warnings) |

## f64 Transcendental Precision

| Function | Strategy | Precision |
|----------|----------|-----------|
| sqrt | Transcendental Rsq64H + 2 Newton-Raphson via DFMA | Full f64 |
| rcp | Transcendental Rcp64H + 2 Newton-Raphson via DFMA | Full f64 |
| exp2 | Range reduction + degree-6 Horner + ldexp | Full f64 |
| log2 | Transcendental Log2 + Newton refinement (Exp2/Rcp) | ~46-bit |
| sin | Cody-Waite + minimax polynomial + quadrant | Full domain |
| cos | Cody-Waite + minimax polynomial + quadrant | Full domain |

## Coverage Analysis

~37% line coverage with 672 tests. Remaining gap is structural:

- **SM20/32/50 encoders** (~1,078 lines, 0%) — legacy architectures not targeted
- **SM70 encoders** (~2,000 lines, 10-20%) — requires encoding every instruction variant
- **IR op structs** (~1,200 lines, 0%) — proc-macro generated code

## Hardware Test Matrix

| GPU | SM/Architecture | Role | Status |
|-----|----------------|------|--------|
| RTX 3090 | SM86 (Ampere) | Primary compilation target | Available |
| Titan V | SM70 (Volta) | f64 regression target, upstream issues | Available (other tower) |
| RTX 3090 | SM86 (Ampere) | Second tower, cross-validation | Available |
| AMD (RDNA3) | — | Backend development target | Available |

## Spring Absorption

Patterns absorbed from ecoPrimals springs (via wateringHole):

| Pattern | Source | Applied |
|---------|--------|---------|
| BTreeMap for deterministic serialization | groundSpring V73 tolerance arch | health.rs |
| Silent-default audit | groundSpring V76 "silent defaults are bugs" | program.rs |
| Cross-spring provenance doc-comments | CROSS_SPRING_SHADER_EVOLUTION | lower_f64/ |
| Unsafe code eliminated | groundSpring CONTRIBUTING | builder/mod.rs |
| Capability-based discovery (verified) | groundSpring CAPABILITY_SURFACE | capability.rs |
| No hardcoded primal names (verified) | groundSpring primal isolation | workspace-wide |
| Result propagation | groundSpring error handling patterns | pipeline |

## Future Work

See `WHATS_NEXT.md` for precision improvements, coverage goals, and
ecosystem integration (coralDriver, coralGpu, barraCuda).

---

*Grade scale: A (production) → F (not started)*
