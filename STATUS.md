# coralNak — Status

**Last updated**: March 5, 2026

---

## Overall Grade: **B+ (Pipeline Wired, Clippy Clean, Frontend Pending)**

| Category | Grade | Notes |
|----------|-------|-------|
| Primal lifecycle | A | Standalone `PrimalLifecycle` + `PrimalHealth` (modeled on sourDough, zero dependency), full test coverage |
| UniBin compliance | A | Binary target with clap, panic hook, SIGTERM/SIGINT, structured errors, exit codes |
| IPC | A+ | JSON-RPC 2.0 + tarpc servers, semantic method names, concrete `IpcError` type, integration-tested |
| Compilation pipeline | B- | Full NAK pass pipeline wired (`pipeline.rs`), SPIR-V frontend pending |
| Mesa stubs | A | 6 modules evolved (BitSet, CFG, dataflow, SmallVec, AsSlice, latencies), 5 legacy FFI stubs remain, dead `compiler_proc` removed |
| Proc macros | B+ | `nak-ir-proc`: `SrcsAsSlice`, `DstsAsSlice`, `DisplayOp`, `FromVariants` — unsafe has compile-time layout checks |
| f64 transcendentals | F | Not started — blocked on SPIR-V frontend |
| ISA encoding | B | SPH, latency tables, bitview crate, SM20-SM120 encoders compiled |
| Code structure | A | 4 oversized files refactored into directory modules, 9 NAK-derived files >1000 LOC (ALU encoders, latency tables, IR types — convention exception) |
| Tests | A | 193 tests passing, integration + capability + lifecycle + health coverage |
| Clippy | A | Full workspace passes `clippy --all-targets -D warnings` with pedantic |
| Coverage | C | 11.83% line coverage (llvm-cov configured, `scripts/coverage.sh` available) |
| Documentation | B+ | Spec, whitePapers, all root docs current, SPDX on 100% of files |
| License | A | AGPL-3.0-only (NAK files retain MIT per upstream), SPDX on all 104 .rs files |
| Sovereignty | A+ | Zero-knowledge startup, capability-based discovery, no hardcoded primals, standalone lifecycle (zero primal deps) |

## Phase Status

| Phase | Description | Status |
|-------|-------------|--------|
| 1 — Scaffold | Extract NAK, create stubs | **Complete** |
| 1.5 — Foundation | UniBin, IPC, stubs evolved, tests | **Complete** |
| 2 — Wire NAK | NAK sources compile against stubs | **Complete** — 0 errors, 193 tests |
| 2.5 — Refactor | Smart split of large files, dead code removal | **Complete** |
| 2.75 — Debt Reduction | Clippy pedantic, unsafe evolution, stub markers, file splits | **Complete** |
| 2.8 — Sovereignty | Standalone lifecycle (zero sourDough dependency), dead stub removal | **Complete** |
| 3 — Replace NIR | naga SPIR-V frontend | Not started |
| 4 — f64 Fix | DFMA software lowering | Not started |
| 5 — Standalone | Remove all Mesa deps | In progress (5 legacy FFI stubs remain) |

## Checks

| Check | Status |
|-------|--------|
| `cargo check --workspace` | PASS (0 errors) |
| `cargo test --workspace` | PASS (193 tests) |
| `cargo fmt --check` | PASS |
| `cargo clippy -D warnings` (full workspace) | PASS |
| `cargo doc --no-deps` | PASS |
| `cargo llvm-cov --summary-only` | 11.83% line coverage |

---

*Grade scale: A (production) → F (not started)*
