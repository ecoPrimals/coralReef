# coralReef — Start Here

Welcome to coralReef, the sovereign Rust GPU compiler.

---

## What is this?

coralReef compiles WGSL and SPIR-V compute shaders to native GPU
binaries. It includes full f64 transcendental support via DFMA software
lowering — something the upstream compiler cannot do.

Vendor-agnostic architecture with pluggable frontends and backends.
Currently targets NVIDIA SM70+ with AMD and Intel backends planned.

Built as a standalone Rust workspace with zero C dependencies.

## Prerequisites

- Rust 1.85+ (edition 2024)
- `cargo` with workspace support

## Quick Start

```bash
cd coralReef
cargo check --workspace
cargo test --workspace     # 672 tests
cargo clippy --workspace --all-targets
cargo fmt --check
```

## Repository Layout

```
coralReef/
├── crates/
│   ├── coralreef-core/         Primal lifecycle + IPC (JSON-RPC, tarpc)
│   ├── coral-reef/             Shader compiler
│   │   └── src/
│   │       ├── backend.rs      Backend trait (vendor-agnostic)
│   │       ├── frontend.rs     Frontend trait (pluggable parsers)
│   │       ├── gpu_arch.rs     GpuTarget: Nvidia/Amd/Intel
│   │       └── codegen/        Compiler core
│   │           ├── ir/            SSA IR types — 12 submodules
│   │           ├── naga_translate/ naga → codegen IR translation
│   │           ├── lower_f64/     f64 transcendental expansion
│   │           ├── nv/            NVIDIA vendor backend
│   │           │   ├── shader_header.rs  Shader Program Header
│   │           │   ├── sm70_encode/      Volta+ encoder
│   │           │   ├── sm50/             Maxwell encoder
│   │           │   ├── sm32/             Kepler encoder
│   │           │   └── sm20/             Fermi encoder
│   │           ├── assign_regs/   Register allocation — 5 files
│   │           ├── calc_instr_deps/ Instruction dependency analysis
│   │           ├── spill_values/  Register spilling
│   │           ├── builder/       IR construction helpers
│   │           └── pipeline.rs    Full compilation pipeline
│   ├── coral-reef-bitview/     Bit-level field access for GPU encoding
│   ├── coral-reef-isa/         ISA tables, latency model
│   ├── coral-reef-stubs/       Pure-Rust dependency replacements
│   └── nak-ir-proc/           Proc-macro derives for IR types
├── specs/                     Architecture specification
├── whitePaper/                Theory docs (f64 lowering, transcendentals)
├── genomebin/                 Deployment scaffolding
└── STATUS.md                  Current status and grades
```

## Key Documents

| Document | Purpose |
|----------|---------|
| `STATUS.md` | Current grades and phase status |
| `WHATS_NEXT.md` | Completed phases and future work |
| `specs/CORALREEF_SPECIFICATION.md` | Architecture and crate layout |
| `CONVENTIONS.md` | Coding standards |
| `CONTRIBUTING.md` | How to contribute |
| `whitePaper/F64_LOWERING_THEORY.md` | DFMA polynomial lowering design |
| `whitePaper/SOVEREIGN_COMPILER_ARCHITECTURE.md` | Architecture rationale |

## How the Compiler Works

```
WGSL / SPIR-V  →  Frontend (naga)  →  naga_translate (codegen IR)
    →  lower_f64  →  optimize (copy prop, DCE, bar prop, scheduling)
    →  legalize  →  assign_regs  →  Backend (encode)  →  native binary
```

Key modules in `crates/coral-reef/src/codegen/`:

- **`naga_translate/`** — Translates naga's IR into the codegen SSA IR.
  Handles expressions, statements, control flow, memory operations, builtins.
- **`lower_f64/`** — Expands f64 transcendental placeholder ops into
  hardware instruction sequences (Newton-Raphson for sqrt/rcp, Horner
  polynomial for exp2, transcendental seed + refinement for log2,
  Cody-Waite + minimax for sin/cos).
- **`pipeline.rs`** — Orchestrates the full compilation: optimize →
  lower_f64 → legalize → register allocation → encode.
- **`ir/`** — SSA intermediate representation with typed registers,
  predication, memory access descriptors, and shader metadata.
- **`nv/`** — NVIDIA vendor backend: SM20–SM120 instruction encoders
  and shader program header.

The `coral-reef-stubs` crate provides pure-Rust replacements for
upstream C dependencies (CFG, BitSet, dataflow, SmallVec). The
`nak-ir-proc` crate generates trait implementations for IR instruction
types via derives.

## Architecture

This primal starts with zero knowledge. It advertises its capabilities
(`shader.compile`, `shader.health`) via the universal adapter and discovers
peers by capability, not by name.

---

*Read `WHATS_NEXT.md` for completed phases and future work.*
