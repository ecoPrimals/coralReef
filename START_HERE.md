# coralNak — Start Here

Welcome to coralNak, the sovereign Rust NVIDIA shader compiler.

---

## What is this?

coralNak compiles WGSL and SPIR-V compute shaders to native NVIDIA SM70+
GPU binaries. It includes full f64 transcendental support via DFMA software
lowering — something the original Mesa NAK compiler cannot do.

Built as a standalone Rust workspace with zero C dependencies.

## Prerequisites

- Rust 1.85+ (edition 2024)
- `cargo` with workspace support

## Quick Start

```bash
cd coralNak
cargo check --workspace
cargo test --workspace     # 390 tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

## Repository Layout

```
coralNak/
├── crates/
│   ├── coralnak-core/         Primal lifecycle + IPC (JSON-RPC, tarpc)
│   ├── coral-nak/             Shader compiler
│   │   └── src/nak/           NAK compiler core
│   │       ├── ir/            SSA IR types — 12 submodules
│   │       ├── from_spirv/    naga → NAK IR translation — 3 files
│   │       ├── lower_f64/     f64 transcendental expansion — 3 files
│   │       ├── sm70_encode/   Turing+ encoder — 6 submodules
│   │       ├── sm50/          Maxwell encoder — 6 submodules
│   │       ├── sm32/          Kepler encoder — 6 submodules
│   │       ├── sm20/          Fermi encoder — 6 submodules
│   │       ├── assign_regs/   Register allocation — 5 files
│   │       ├── calc_instr_deps/ Instruction dependency analysis — 3 files
│   │       ├── spill_values/  Register spilling — 3 files
│   │       ├── builder/       IR construction helpers — 2 files
│   │       └── pipeline.rs    Full compilation pipeline
│   ├── coral-nak-bitview/     Bit-level field access for GPU encoding
│   ├── coral-nak-isa/         ISA tables, latency model, SPH
│   ├── coral-nak-stubs/       Pure-Rust Mesa replacements (all evolved)
│   └── nak-ir-proc/           Proc-macro derives for IR types
├── specs/                     Architecture specification
├── whitePaper/                Theory docs (f64 lowering, MUFU)
├── genomebin/                 Deployment scaffolding
└── STATUS.md                  Current status and grades
```

## Key Documents

| Document | Purpose |
|----------|---------|
| `STATUS.md` | Current grades and phase status |
| `WHATS_NEXT.md` | Completed phases and future work |
| `specs/CORALNAK_SPECIFICATION.md` | Architecture and crate layout |
| `CONVENTIONS.md` | Coding standards |
| `CONTRIBUTING.md` | How to contribute |
| `whitePaper/F64_LOWERING_THEORY.md` | DFMA polynomial lowering design |
| `whitePaper/MUFU_ANALYSIS.md` | NVIDIA MUFU instruction reference |
| `whitePaper/SOVEREIGN_COMPILER_ARCHITECTURE.md` | Architecture rationale |

## How the Compiler Works

```
WGSL / SPIR-V  →  naga parse  →  from_spirv (NAK IR)  →  lower_f64
    →  optimize (copy prop, DCE, bar prop, scheduling)
    →  legalize  →  assign_regs  →  encode  →  native binary
```

Key modules in `crates/coral-nak/src/nak/`:

- **`from_spirv/`** — Translates naga's IR into NAK's SSA-based IR. Handles
  expressions, statements, control flow, memory operations, and builtins.
- **`lower_f64/`** — Expands f64 transcendental placeholder ops into DFMA-based
  instruction sequences (Newton-Raphson for sqrt/rcp, Horner polynomial for
  exp2, MUFU seed + refinement for log2, Cody-Waite + minimax for sin/cos).
- **`pipeline.rs`** — Orchestrates the full compilation: optimize → lower_f64 →
  legalize → register allocation → encode.
- **`ir/`** — SSA intermediate representation with typed registers, predication,
  memory access descriptors, and shader metadata.
- **`sm70_encode/`** — Encodes NAK IR instructions to SM70+ native binary format.

The `coral-nak-stubs` crate provides pure-Rust replacements for Mesa's C
dependencies (CFG, BitSet, dataflow, nvidia_headers). The `nak-ir-proc`
crate generates trait implementations for IR instruction types via derives.

## Architecture

This primal starts with zero knowledge. It advertises its capabilities
(`shader.compile`, `shader.health`) via the universal adapter and discovers
peers by capability, not by name.

---

*Read `WHATS_NEXT.md` for completed phases and future work.*
