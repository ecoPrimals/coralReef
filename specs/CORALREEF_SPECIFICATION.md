# coralReef — Specification

**Version**: 0.3.0  
**Date**: March 5, 2026  
**Status**: Active

---

## Purpose

coralReef is a sovereign Rust GPU compiler. It compiles WGSL and
SPIR-V compute shaders to native GPU binaries with full f64
transcendental support, as a standalone pure-Rust workspace.

Vendor-agnostic architecture: NVIDIA backend active (SM70–SM120),
AMD and Intel backends planned via the `Backend` trait.

## Architecture

```
WGSL / SPIR-V input
       │
       ▼
┌───────────────────┐
│  Frontend (naga)   │  Parse WGSL/SPIR-V → naga IR
└────────┬──────────┘
         ▼
┌───────────────────┐
│  codegen            │  Compiler pipeline
│  ├ naga_translate  │  naga IR → codegen SSA IR
│  ├ lower_f64       │  f64 transcendental expansion (DFMA)
│  ├ optimize        │  copy prop, DCE, scheduling, bar prop, lop, prmt
│  ├ legalize        │  Target-specific lowering
│  ├ assign_regs     │  Register allocation + spilling
│  └ nv/encode       │  Vendor-specific instruction encoding
└────────┬──────────┘
         ▼
  Backend (NvidiaBackend / AmdBackend / IntelBackend)
         │
         ▼
  Native GPU binary
```

## Crate Layout

| Crate | Purpose |
|-------|---------|
| `coralreef-core` | Primal lifecycle, health, IPC (JSON-RPC 2.0, tarpc), zero-copy `Bytes` |
| `coral-reef` | Shader compiler: pluggable frontend, f64 lowering, optimizers, RA, vendor encoding |
| `coral-reef-isa` | ISA tables, instruction latencies (SM30–SM120) |
| `coral-reef-stubs` | Pure-Rust dependency replacements: CFG, BitSet, dataflow, SmallVec, fxhash |
| `coral-reef-bitview` | Bit-level field manipulation for instruction encoding |
| `nak-ir-proc` | Proc-macro derives: `SrcsAsSlice`, `DstsAsSlice`, `DisplayOp`, `FromVariants` |

## f64 Transcendental Lowering

GPU transcendental hardware units only support f32. coralReef adds software
lowering using DFMA (Double-precision Fused Multiply-Add) for all six
transcendentals:

| Function | Strategy | Precision |
|----------|----------|-----------|
| sqrt | Transcendental Rsq64H seed + 2 Newton-Raphson via DFMA | Full f64 |
| rcp | Transcendental Rcp64H seed + 2 Newton-Raphson via DFMA | Full f64 |
| exp2 | Range reduction + degree-6 Horner polynomial + ldexp | Full f64 |
| log2 | Transcendental Log2 seed + Newton refinement (Exp2/Rcp correction) | ~46-bit |
| sin | Cody-Waite reduction + minimax polynomial + quadrant correction | Full domain |
| cos | Cody-Waite reduction + minimax polynomial + quadrant correction | Full domain |

## Sovereign Compute Roadmap

| Level | Milestone | Status |
|-------|-----------|--------|
| 1 | barraCuda DF64 pipeline (workaround) | **Complete** |
| 2 | Standalone Rust compiler (f64 fix) | **Complete** |
| 3 | Multi-vendor backend architecture | **In Progress** |
| 4 | coralDriver (userspace GPU driver) | Planned |
| 5 | coralGpu (unified Rust GPU abstraction) | Planned |

---

**Date**: March 5, 2026  
**Version**: 0.3.0
