# coralNak

**Status**: Phase 5 Complete — Standalone Sovereign Compiler  
**Purpose**: Sovereign Rust NVIDIA shader compiler — forked from Mesa NAK

---

## Overview

coralNak is a pure-Rust NVIDIA GPU shader compiler. It compiles WGSL and
SPIR-V compute shaders to native SM70+ GPU binaries, with full f64
transcendental support via DFMA software lowering.

Part of the ecoPrimals Sovereign Compute Evolution.

## Quick Start

```bash
# Rust 1.85+ required (edition 2024)
cargo check --workspace
cargo test --workspace     # 390 tests
cargo clippy --workspace --all-targets -- -D warnings
```

## Compilation Pipeline

```
WGSL / SPIR-V input
       │
       ▼
┌──────────────────┐
│  naga             │  Parse WGSL/SPIR-V → naga IR
└────────┬─────────┘
         ▼
┌──────────────────┐
│  coral-nak        │  Compiler pipeline
│  ├ from_spirv     │  naga IR → NAK SSA IR
│  ├ lower_f64      │  f64 transcendental expansion (DFMA)
│  ├ optimize       │  copy prop, DCE, scheduling, bar prop
│  ├ legalize       │  Target-specific lowering
│  ├ assign_regs    │  Register allocation + spilling
│  └ encode         │  SM70+ instruction encoding
└────────┬─────────┘
         ▼
  Native GPU binary (SM70+)
```

## Structure

```
coralNak/
├── Cargo.toml                    # Workspace root
├── crates/
│   ├── coralnak-core/            # Primal lifecycle + IPC (JSON-RPC, tarpc)
│   ├── coral-nak/                # Shader compiler
│   │   ├── src/nak/
│   │   │   ├── ir/               # SSA IR types (12 submodules)
│   │   │   ├── from_spirv/       # naga → NAK IR translation (3 files)
│   │   │   ├── lower_f64/        # f64 transcendental lowering (3 files)
│   │   │   ├── sm70_encode/      # SM70+ encoder (6 submodules)
│   │   │   ├── sm50/             # Maxwell encoder (6 submodules)
│   │   │   ├── sm32/             # Kepler encoder (6 submodules)
│   │   │   └── pipeline.rs       # Full compilation pipeline
│   │   └── tests/                # Integration tests
│   ├── coral-nak-bitview/        # Bit-level field access for GPU encoding
│   ├── coral-nak-isa/            # ISA tables, latency model, SPH
│   ├── coral-nak-stubs/          # Pure-Rust Mesa replacements (all evolved)
│   └── nak-ir-proc/              # Proc-macro derives for IR types
├── specs/                        # Architecture specification
├── whitePaper/                   # Theory docs (f64 lowering, MUFU)
└── genomebin/                    # Deployment scaffolding
```

## Crates

| Crate | Purpose |
|-------|---------|
| `coralnak-core` | Primal lifecycle, health, CLI (`server`/`compile`/`doctor`), JSON-RPC + tarpc IPC, zero-copy `Bytes` |
| `coral-nak` | Shader compiler — naga frontend, f64 lowering, optimizers, register allocation, SM70+ encoding |
| `coral-nak-bitview` | `BitViewable`/`BitMutViewable` traits for GPU instruction encoding |
| `coral-nak-isa` | ISA encoding tables, instruction latencies (SM30–SM120), Shader Program Header |
| `coral-nak-stubs` | Pure-Rust Mesa replacements: CFG, BitSet, dataflow, SmallVec, nvidia_headers (all evolved) |
| `nak-ir-proc` | Proc-macro derives: `SrcsAsSlice`, `DstsAsSlice`, `DisplayOp`, `FromVariants` |

## f64 Transcendental Support

All six f64 transcendentals implemented with production precision:

| Function | Strategy | Precision |
|----------|----------|-----------|
| sqrt | MUFU.RSQ64H seed + 2 Newton-Raphson iterations | Full f64 |
| rcp | MUFU.RCP64H seed + 2 Newton-Raphson iterations | Full f64 |
| exp2 | Range reduction + degree-6 Horner polynomial + ldexp | Full f64 |
| log2 | MUFU.LOG2 seed + Newton refinement (EX2/RCP correction) | ~46-bit |
| sin | Cody-Waite range reduction + minimax polynomial + quadrant correction | Full domain |
| cos | Cody-Waite range reduction + minimax polynomial + quadrant correction | Full domain |

## Checks

| Check | Status |
|-------|--------|
| `cargo check --workspace` | PASS |
| `cargo test --workspace` | PASS (390 tests) |
| `cargo clippy -D warnings` | PASS |
| `cargo fmt --check` | PASS |
| `cargo doc --no-deps` | PASS |
| `cargo llvm-cov` | 37.1% line, 44.9% function |

---

**License**: AGPL-3.0-only (NAK-derived files retain MIT per upstream)  
**Standalone primal** — zero-knowledge startup, capability-based discovery, no hardcoded primals
