# coralNak

**Status**: Phase 2.8 — Standalone Sovereign Primal  
**Purpose**: Sovereign Rust NVIDIA shader compiler — forked from Mesa NAK

---

## Overview

coralNak extracts Mesa's NAK shader compiler into a standalone Rust crate,
fixing the f64 transcendental emission gap and removing all C dependencies.

Part of the ecoPrimals Sovereign Compute Evolution.

## Quick Start

```bash
# Rust 1.85+ required (edition 2024)
cargo check --workspace
cargo test --workspace
```

## Structure

```
coralNak/
├── Cargo.toml                    # Workspace root
├── crates/
│   ├── coralnak-core/            # Primal lifecycle + IPC (JSON-RPC, tarpc)
│   ├── coral-nak/                # Shader compiler
│   │   └── src/nak/              # NAK sources (72 files, 51K LOC)
│   │       ├── ir/               # IR types (12 submodules)
│   │       ├── sm70_encode/      # SM70+ encoder (6 submodules)
│   │       ├── sm50/             # Maxwell encoder (6 submodules)
│   │       └── sm32/             # Kepler encoder (6 submodules)
│   ├── coral-nak-bitview/        # Bit-level field access for GPU encoding
│   ├── coral-nak-isa/            # ISA tables, latency model, SPH
│   ├── coral-nak-stubs/          # Mesa dependency replacements
│   └── nak-ir-proc/              # Proc-macro derives for IR types
├── specs/                        # Architecture specification
├── whitePaper/                   # Theory docs (f64 lowering, DFMA)
└── genomebin/                    # Deployment scaffolding
```

## Crates

| Crate | Purpose |
|-------|---------|
| `coralnak-core` | Primal lifecycle, health, CLI (`server`/`compile`/`doctor`), JSON-RPC + tarpc IPC |
| `coral-nak` | Shader compiler — NAK sources wired against stubs |
| `coral-nak-bitview` | `BitViewable`/`BitMutViewable` traits for GPU instruction encoding |
| `coral-nak-isa` | ISA encoding tables, instruction latencies, Shader Program Header |
| `coral-nak-stubs` | Pure-Rust replacements for Mesa C bindings (BitSet, CFG, dataflow) |
| `nak-ir-proc` | Proc-macro derives: `SrcsAsSlice`, `DstsAsSlice`, `DisplayOp`, `FromVariants` |

## Evolution Phases

1. **Scaffold** — Extract NAK sources, create Mesa stubs *(complete)*
2. **Foundation** — UniBin, IPC, stubs evolved, test coverage *(complete)*
3. **Wire NAK** — NAK sources compile against stubs *(complete — 193 tests passing)*
4. **Replace NIR** — naga SPIR-V frontend instead of Mesa NIR
5. **f64 Fix** — DFMA-based software lowering for transcendentals
6. **Standalone** — Remove all Mesa dependencies, publish

---

**License**: AGPL-3.0-only (NAK-derived files retain MIT per upstream)  
**Standalone primal** — lifecycle and health patterns modeled on sourDough, zero compile-time dependency
