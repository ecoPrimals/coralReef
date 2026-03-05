# coralNak — Start Here

Welcome to coralNak, the sovereign Rust shader compiler.

---

## What is this?

coralNak extracts Mesa's NAK shader compiler into a standalone Rust crate.
It fixes the f64 transcendental emission gap and evolves into a pure-Rust
GPU compiler with zero-knowledge, capability-based architecture.

## Prerequisites

- Rust 1.85+ (edition 2024)
- `cargo` with workspace support

## Quick Start

```bash
cd coralNak
cargo check --workspace
cargo test --workspace     # 183 tests
cargo clippy --workspace
cargo fmt --check
```

## Repository Layout

```
coralNak/
├── crates/
│   ├── coralnak-core/         Primal lifecycle + IPC (JSON-RPC, tarpc)
│   ├── coral-nak/             Shader compiler
│   │   └── src/nak/           NAK sources
│   │       ├── ir/            IR types — 12 submodules
│   │       ├── sm70_encode/   Turing+ encoder — 6 submodules
│   │       ├── sm50/          Maxwell encoder — 6 submodules
│   │       ├── sm32/          Kepler encoder — 6 submodules
│   │       ├── sm20/          Fermi encoder — 6 submodules
│   │       └── pipeline.rs    Full compilation pipeline (16 passes)
│   ├── coral-nak-bitview/     Bit-level field access for GPU encoding
│   ├── coral-nak-isa/         ISA tables, latency model, SPH
│   ├── coral-nak-stubs/       Mesa dependency replacements (evolving to real)
│   └── nak-ir-proc/           Proc-macro derives for IR types
├── specs/                     Architecture specification
├── whitePaper/                Theory docs (f64 lowering, DFMA, MuFu)
├── genomebin/                 Deployment scaffolding
└── .github/workflows/         CI
```

## Key Documents

| Document | Purpose |
|----------|---------|
| `specs/CORALNAK_SPECIFICATION.md` | Architecture and roadmap |
| `STATUS.md` | Current grades and phase status |
| `WHATS_NEXT.md` | Prioritized task list |
| `CONVENTIONS.md` | Coding standards |
| `CONTRIBUTING.md` | How to contribute |
| `whitePaper/F64_LOWERING_THEORY.md` | DFMA polynomial design |
| `whitePaper/MUFU_ANALYSIS.md` | MuFu instruction analysis |
| `whitePaper/SOVEREIGN_COMPILER_ARCHITECTURE.md` | Architecture rationale |

## How NAK Sources Work

The `crates/coral-nak/src/nak/` directory contains the NAK compiler sources
extracted from Mesa and wired against pure-Rust stub replacements. Large files
have been refactored into directory modules with logical submodules:

- **`ir/`** — Intermediate representation (registers, src/dst types, op structs,
  type enums, shader info) split across 12 files
- **`sm70_encode/`** — SM70+ instruction encoding (encoder core, ALU, texture,
  memory, control flow)
- **`sm50/`**, **`sm32/`**, **`sm20/`** — Maxwell, Kepler, Fermi encoders (same pattern)
- **`pipeline.rs`** — Full compilation pipeline: 16 optimization, scheduling,
  legalization, and allocation passes followed by architecture-specific encoding

The `coral-nak-stubs` crate provides pure-Rust replacements for Mesa's C FFI
bindings. The `nak-ir-proc` crate provides proc-macro derives that generate
trait implementations for IR instruction types.

## Architecture

This primal starts with zero knowledge. It advertises its capabilities
(`shader.compile`, `shader.health`) via the universal adapter and discovers
peers by capability, not by name.

---

*Read `WHATS_NEXT.md` for what to work on next.*
