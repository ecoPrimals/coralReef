# coralNak — Status

**Last updated**: March 5, 2026

---

## Overall Grade: **B (Pipeline Wired, Frontend Pending)**

| Category | Grade | Notes |
|----------|-------|-------|
| Primal lifecycle | A | `PrimalLifecycle` + `PrimalHealth` via sourDough, full test coverage |
| UniBin compliance | A | Binary target with clap, panic hook, SIGTERM/SIGINT, structured errors, exit codes |
| IPC | A | JSON-RPC 2.0 + tarpc servers, semantic method names, integration-tested |
| Compilation pipeline | B- | Full NAK pass pipeline wired (`pipeline.rs`), SPIR-V frontend pending |
| Mesa stubs | A- | 12 modules evolved: BitSet, CFG, dataflow, SmallVec, latencies — real implementations |
| Proc macros | B | `nak-ir-proc`: `SrcsAsSlice`, `DstsAsSlice`, `DisplayOp`, `FromVariants` |
| f64 transcendentals | F | Not started — blocked on SPIR-V frontend |
| ISA encoding | B | SPH, latency tables, bitview crate, SM20-SM120 encoders compiled |
| Code structure | A- | Large files refactored, dead code removed (8.7K LOC of debris cleaned) |
| Tests | A | 183 tests passing, integration + capability + lifecycle coverage |
| CI | B- | Workflow file present, needs validation on GitHub |
| Documentation | B | Spec, whitePapers, all root docs current |
| License | A | AGPL-3.0-only (NAK files retain MIT per upstream) |
| Sovereignty | A | Zero-knowledge startup, capability-based discovery, no hardcoded primals |

## Phase Status

| Phase | Description | Status |
|-------|-------------|--------|
| 1 — Scaffold | Extract NAK, create stubs | **Complete** |
| 1.5 — Foundation | UniBin, IPC, stubs evolved, tests | **Complete** |
| 2 — Wire NAK | NAK sources compile against stubs | **Complete** — 0 errors, 183 tests |
| 2.5 — Refactor | Smart split of large files, dead code removal | **Complete** |
| 3 — Replace NIR | naga SPIR-V frontend | Not started |
| 4 — f64 Fix | DFMA software lowering | Not started |
| 5 — Standalone | Remove all Mesa deps | In progress (stubs being evolved) |

## Checks

| Check | Status |
|-------|--------|
| `cargo check --workspace` | PASS (0 errors) |
| `cargo test --workspace` | PASS (183 tests) |
| `cargo fmt --check` | PASS |
| `cargo clippy` (non-NAK crates) | PASS |

---

*Grade scale: A (production) → F (not started)*
