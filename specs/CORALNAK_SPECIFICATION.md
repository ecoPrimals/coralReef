# coralNak — Specification

**Version**: 0.1.0  
**Date**: March 4, 2026  
**Status**: Draft

---

## Purpose

coralNak is a sovereign Rust NVIDIA shader compiler, forked from Mesa's NAK.
It fixes f64 transcendental emission gaps and evolves into a standalone Rust
crate independent of the Mesa C build system.

## Architecture

```
SPIR-V / WGSL input
       │
       ▼
┌──────────────┐
│  coral-nak    │  Compiler pipeline
│  ├ from_spirv │  naga SPIR-V → coral-nak IR (replaces from_nir)
│  ├ optimize   │  opt_copy_prop, opt_dce, opt_lop, ...
│  ├ legalize   │  Target-specific legalization
│  ├ alloc_regs │  Register allocation
│  └ encode     │  SM70+ instruction encoding
└──────────────┘
       │
       ▼
Native GPU binary (SM70+)
```

## Crate Layout

| Crate | Purpose |
|-------|---------|
| `coralnak-core` | Primal lifecycle, health, IPC |
| `coral-nak` | Shader compiler (NAK extraction) |
| `coral-nak-isa` | NVIDIA ISA tables and encoding |
| `coral-nak-stubs` | Mesa dependency replacements (6 evolved, legacy FFI stubs remain for Phase 3) |

## f64 Transcendental Gap

NAK's MUFU instructions only support f32.  For f64 transcendentals, coralNak
adds software lowering using DFMA (Double-precision Fused Multiply-Add):

| Function | Strategy |
|----------|----------|
| sin/cos | Range reduction + DFMA polynomial |
| exp2 | Integer/fraction split + DFMA reconstruction |
| log2 | Exponent extraction + MUFU.LOG2 + Newton refinement |
| sqrt | MUFU.RSQ64H + two Newton iterations |
| rcp | MUFU.RCP64H + two Newton iterations |

## Sovereign Compute Roadmap

| Level | Milestone | Status |
|-------|-----------|--------|
| 1 | barraCuda DF64 pipeline (workaround) | Complete |
| 2 | Fork NAK, fix f64 emission | Complete (pipeline wired, frontend pending) |
| 3 | Standalone Rust crate | In progress (standalone lifecycle, zero external primal deps) |
| 4 | coralDriver (userspace GPU driver) | Planned |
| 5 | coralGpu (unified Rust GPU abstraction) | Planned |

---

**Date**: March 4, 2026  
**Version**: 0.1.0
