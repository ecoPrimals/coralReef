# coralReef

**Status**: Phase 5+ — Sovereign Multi-Vendor GPU Compiler  
**Purpose**: Sovereign Rust GPU compiler — WGSL/SPIR-V → native GPU binary

---

## Overview

coralReef is a pure-Rust GPU shader compiler. It compiles WGSL and
SPIR-V compute shaders to native SM70+ GPU binaries, with full f64
transcendental support via DFMA software lowering.

Vendor-agnostic architecture: NVIDIA backend active (SM70–SM120),
AMD and Intel backends planned via the same `Backend` trait.

Part of the ecoPrimals Sovereign Compute Evolution.

## Quick Start

```bash
# Rust 1.85+ required (edition 2024)
cargo check --workspace
cargo test --workspace     # 672 tests
cargo clippy --workspace --all-targets
cargo fmt --check
```

## Compilation Pipeline

```
WGSL / SPIR-V input
       │
       ▼
┌──────────────────┐
│  Frontend         │  Parse WGSL/SPIR-V → naga IR (pluggable)
└────────┬─────────┘
         ▼
┌──────────────────┐
│  codegen           │  Compiler pipeline
│  ├ naga_translate │  naga IR → SSA IR
│  ├ lower_f64      │  f64 transcendental expansion (DFMA)
│  ├ optimize       │  copy prop, DCE, scheduling, bar prop
│  ├ legalize       │  Target-specific lowering
│  ├ assign_regs    │  Register allocation + spilling
│  └ encode         │  Vendor-specific instruction encoding
└────────┬─────────┘
         ▼
   Backend (NvidiaBackend / AmdBackend / IntelBackend)
         │
         ▼
  Native GPU binary
```

## Structure

```
coralReef/
├── Cargo.toml                    # Workspace root
├── crates/
│   ├── coralreef-core/            # Primal lifecycle + IPC (JSON-RPC, tarpc)
│   ├── coral-reef/                # Shader compiler
│   │   ├── src/
│   │   │   ├── backend.rs        # Backend trait (vendor-agnostic)
│   │   │   ├── frontend.rs       # Frontend trait (pluggable parsers)
│   │   │   ├── gpu_arch.rs       # GpuTarget: Nvidia/Amd/Intel
│   │   │   └── codegen/          # Compiler core
│   │   │       ├── ir/           # SSA IR types (12 submodules)
│   │   │       ├── naga_translate/ # naga → codegen IR translation
│   │   │       ├── lower_f64/    # f64 transcendental lowering
│   │   │       ├── nv/           # NVIDIA vendor backend
│   │   │       │   ├── shader_header.rs  # Shader Program Header
│   │   │       │   ├── sm70_encode/      # Volta+ encoder
│   │   │       │   ├── sm50/             # Maxwell encoder
│   │   │       │   ├── sm32/             # Kepler encoder
│   │   │       │   └── sm20/             # Fermi encoder
│   │   │       └── pipeline.rs   # Full compilation pipeline
│   │   └── tests/                # Integration tests
│   ├── coral-reef-bitview/        # Bit-level field access for GPU encoding
│   ├── coral-reef-isa/            # ISA tables, latency model
│   ├── coral-reef-stubs/          # Pure-Rust dependency replacements
│   └── nak-ir-proc/              # Proc-macro derives for IR types
├── specs/                        # Architecture specification
├── whitePaper/                   # Theory docs (f64 lowering, transcendental analysis)
└── genomebin/                    # Deployment scaffolding
```

## Crates

| Crate | Purpose |
|-------|---------|
| `coralreef-core` | Primal lifecycle, health, CLI (`server`/`compile`/`doctor`), JSON-RPC + tarpc IPC, zero-copy `Bytes` |
| `coral-reef` | Shader compiler — pluggable frontend, f64 lowering, optimizers, register allocation, vendor encoding |
| `coral-reef-bitview` | `BitViewable`/`BitMutViewable` traits for GPU instruction encoding |
| `coral-reef-isa` | ISA encoding tables, instruction latencies (SM30–SM120) |
| `coral-reef-stubs` | Pure-Rust dependency replacements: CFG, BitSet, dataflow, SmallVec, fxhash |
| `nak-ir-proc` | Proc-macro derives: `SrcsAsSlice`, `DstsAsSlice`, `DisplayOp`, `FromVariants` |

## f64 Transcendental Support

All six f64 transcendentals implemented with production precision:

| Function | Strategy | Precision |
|----------|----------|-----------|
| sqrt | Transcendental Rsq64H seed + 2 Newton-Raphson iterations | Full f64 |
| rcp | Transcendental Rcp64H seed + 2 Newton-Raphson iterations | Full f64 |
| exp2 | Range reduction + degree-6 Horner polynomial + ldexp | Full f64 |
| log2 | Transcendental Log2 seed + Newton refinement (Exp2/Rcp correction) | ~46-bit |
| sin | Cody-Waite range reduction + minimax polynomial + quadrant correction | Full domain |
| cos | Cody-Waite range reduction + minimax polynomial + quadrant correction | Full domain |

## Checks

| Check | Status |
|-------|--------|
| `cargo check --workspace` | PASS |
| `cargo test --workspace` | PASS (672 tests) |
| `cargo clippy` | PASS (0 warnings) |
| `cargo fmt --check` | PASS |

## Hardware Testing

| GPU | Architecture | Status |
|-----|-------------|--------|
| RTX 3090 | SM86 (Ampere) | Primary test target |
| Titan V | SM70 (Volta) | f64 regression target — NAK has known issues here |
| AMD (RDNA3) | — | Backend planned |

---

**License**: AGPL-3.0-only (upstream-derived files retain original attribution)  
**Standalone primal** — zero-knowledge startup, capability-based discovery, no hardcoded primals
