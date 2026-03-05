# coralNak — Specification

**Version**: 0.2.0  
**Date**: March 5, 2026  
**Status**: Active

---

## Purpose

coralNak is a sovereign Rust NVIDIA shader compiler. It compiles WGSL and
SPIR-V compute shaders to native SM70+ GPU binaries with full f64
transcendental support, independent of the Mesa C build system.

## Architecture

```
WGSL / SPIR-V input
       │
       ▼
┌──────────────┐
│  coral-nak    │  Compiler pipeline
│  ├ from_spirv │  naga IR → NAK SSA IR
│  ├ lower_f64  │  f64 transcendental expansion (DFMA)
│  ├ optimize   │  copy prop, DCE, scheduling, bar prop, lop, prmt
│  ├ legalize   │  Target-specific lowering
│  ├ alloc_regs │  Register allocation + spilling
│  └ encode     │  SM70+ instruction encoding
└──────────────┘
       │
       ▼
Native GPU binary (SM70+)
```

## Crate Layout

| Crate | Purpose |
|-------|---------|
| `coralnak-core` | Primal lifecycle, health, IPC (JSON-RPC 2.0, tarpc), zero-copy `Bytes` |
| `coral-nak` | Shader compiler: frontend, f64 lowering, optimizers, RA, encoding |
| `coral-nak-isa` | NVIDIA ISA tables, instruction latencies (SM30–SM120), SPH encoding |
| `coral-nak-stubs` | Pure-Rust Mesa replacements: CFG, BitSet, dataflow, SmallVec, nvidia_headers (all evolved) |
| `coral-nak-bitview` | Bit-level field manipulation for instruction encoding |
| `nak-ir-proc` | Proc-macro derives: `SrcsAsSlice`, `DstsAsSlice`, `DisplayOp`, `FromVariants` |

## f64 Transcendental Lowering

NVIDIA's MUFU instructions only support f32. coralNak adds software lowering
using DFMA (Double-precision Fused Multiply-Add) for all six transcendentals:

| Function | Strategy | Precision |
|----------|----------|-----------|
| sqrt | MUFU.RSQ64H seed + 2 Newton-Raphson via DFMA | Full f64 |
| rcp | MUFU.RCP64H seed + 2 Newton-Raphson via DFMA | Full f64 |
| exp2 | Range reduction + degree-6 Horner polynomial + ldexp | Full f64 |
| log2 | MUFU.LOG2 seed + Newton refinement (EX2/RCP correction) | ~46-bit |
| sin | Cody-Waite reduction + minimax polynomial + quadrant correction | Full domain |
| cos | Cody-Waite reduction + minimax polynomial + quadrant correction | Full domain |

## Sovereign Compute Roadmap

| Level | Milestone | Status |
|-------|-----------|--------|
| 1 | barraCuda DF64 pipeline (workaround) | **Complete** |
| 2 | Fork NAK, fix f64 emission | **Complete** |
| 3 | Standalone Rust compiler | **Complete** |
| 4 | coralDriver (userspace GPU driver) | Planned |
| 5 | coralGpu (unified Rust GPU abstraction) | Planned |

---

**Date**: March 5, 2026  
**Version**: 0.2.0
