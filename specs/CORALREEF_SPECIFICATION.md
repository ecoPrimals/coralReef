# coralReef — Specification

**Version**: 0.5.0
**Date**: March 6, 2026
**Status**: Phase 10 — Spring Absorption + Compiler Hardening

---

## Purpose

coralReef is a sovereign Rust GPU compiler. It compiles WGSL and
SPIR-V compute shaders to native GPU binaries with full f64
transcendental support, as a standalone pure-Rust workspace.

Multi-vendor architecture: NVIDIA (SM70–SM89) and AMD (RDNA2 GFX1030)
backends operational. Both share the same IR, optimizer passes, and
`ShaderModel` trait — Rust trait dispatch, no manual vtables.

coralDriver provides userspace GPU dispatch via DRM ioctl (AMD amdgpu,
NVIDIA nouveau). coralGpu wraps compilation and dispatch into a unified
API. Every layer pure Rust — zero FFI, zero `*-sys`, zero `extern "C"`.

## Target Hardware

| GPU | Architecture | ISA | Kernel Driver | f64 | Role |
|-----|-------------|-----|---------------|-----|------|
| NVIDIA RTX 3090 | Ampere SM86 | SASS | nvidia (proprietary) | 1/32, DF64 | NVIDIA compilation target |
| NVIDIA Titan V | Volta SM70 | SASS | nouveau (open) | 1/2, native | Sovereign f64 endgame |
| AMD RX 6950 XT | RDNA2 GFX1030 | GCN/RDNA | amdgpu (open) | 1/16, native `v_fma_f64` | AMD compilation target |

## Architecture

```
WGSL / SPIR-V input
       │
       ▼
┌───────────────────┐
│  Frontend (naga)   │  Parse WGSL/SPIR-V → naga IR
└────────┬──────────┘
         ▼
┌───────────────────────────────────────────────┐
│  codegen (shared)                              │
│  ├ naga_translate   naga IR → codegen SSA IR  │
│  ├ lower_f64        f64 transcendentals       │
│  ├ optimize         copy prop, DCE, lop, ...  │
│  └ pipeline.rs      orchestration             │
└────────┬──────────────────────────────────────┘
         │
    ┌────┴────────────────┐
    ▼                     ▼
┌──────────────┐   ┌──────────────┐
│  nv/ backend  │   │  amd/ backend │
│  legalize     │   │  legalize     │
│  assign_regs  │   │  assign_regs  │
│  sm70_encode  │   │  gfx10_encode │
│  SPH header   │   │  ELF emit     │
│  SM20–SM89    │   │  GFX1030+     │
└──────┬───────┘   └──────┬───────┘
       │                  │
       ▼                  ▼
  NVIDIA SASS         AMD GFX binary
       │                  │
       ▼                  ▼
┌───────────────────────────────────┐
│  coral-driver                      │
│  ├ AmdDevice   DRM + PM4 dispatch │
│  ├ NvDevice    DRM + pushbuf      │
│  └ ComputeDevice trait            │
└──────────────┬────────────────────┘
               ▼
┌───────────────────────────────────┐
│  coral-gpu                         │
│  GpuContext — unified API         │
│  compile_wgsl() + dispatch()      │
└───────────────────────────────────┘
```

## Crate Layout

| Crate | Purpose |
|-------|---------|
| `coralreef-core` | Primal lifecycle, health, IPC (JSON-RPC 2.0, tarpc), zero-copy `Bytes` |
| `coral-reef` | Shader compiler: pluggable frontend, f64 lowering, optimizers, RA, vendor encoding |
| `coral-driver` | Userspace GPU dispatch: AMD amdgpu DRM + NVIDIA nouveau DRM, pure Rust syscalls |
| `coral-gpu` | Unified GPU compute: compile WGSL + dispatch on hardware in one API |
| `coral-reef-isa` | ISA tables, instruction latencies (SM30–SM120, AMD RDNA2) |
| `coral-reef-stubs` | Pure-Rust dependency replacements: CFG, BitSet, dataflow, SmallVec, fxhash |
| `coral-reef-bitview` | Bit-level field manipulation for instruction encoding |
| `nak-ir-proc` | Proc-macro derives: `SrcsAsSlice`, `DstsAsSlice`, `DisplayOp`, `FromVariants`, `Encode` |
| `amd-isa-gen` | Pure Rust ISA table generator from AMD XML specs |

## f64 Transcendental Lowering

GPU transcendental hardware units only support f32. coralReef adds software
lowering using DFMA (Double-precision Fused Multiply-Add) for NVIDIA, and
native f64 instruction emission for AMD:

| Function | NVIDIA Strategy | AMD Strategy | Precision |
|----------|----------------|-------------|-----------|
| sqrt | Rsq64H seed + 2 Newton-Raphson via DFMA | `v_sqrt_f64` (native) | Full f64 |
| rcp | Rcp64H seed + 2 Newton-Raphson via DFMA | `v_rcp_f64` (native) | Full f64 |
| exp2 | Range reduction + degree-6 Horner + ldexp | Polynomial via `v_fma_f64` | Full f64 |
| log2 | Log2 seed + Newton refinement | Polynomial via `v_fma_f64` | ~46-bit+ |
| sin | Cody-Waite + minimax polynomial | Cody-Waite via `v_fma_f64` | Full domain |
| cos | Cody-Waite + minimax polynomial | Cody-Waite via `v_fma_f64` | Full domain |

## Three-Tier Precision Model

Adopted from barraCuda's `Fp64Strategy`:

| Tier | Precision | Source | Use Case |
|------|-----------|--------|----------|
| f32 | ~24-bit mantissa | Native f32 cores | Visualization, inference, throughput |
| DF64 | ~48-bit mantissa | f32 core pairs (idle capacity) | Most scientific compute |
| f64 | ~53-bit mantissa | Native f64 units (scarce) | Reference validation, accumulation |

| Hardware | Native f64 Rate | Recommended Strategy |
|----------|----------------|---------------------|
| NVIDIA Volta/A100 | 1:2 | Concurrent (f64 + DF64 simultaneously) |
| NVIDIA RTX 3090 | 1:32 | Hybrid (DF64 primary, f64 accumulation) |
| AMD RX 6950 XT | 1:16 | Hybrid (DF64 primary, f64 precision-critical) |

## Sovereign Compute Roadmap

| Phase | Milestone | Status |
|-------|-----------|--------|
| 1–5 | Standalone NVIDIA compiler (f64, pure Rust) | **Complete** |
| 5.5 | Naming evolution, vendor-neutral IR types | **Complete** |
| 5.7 | Deep debt audit, tooling, proc-macro safety | **Complete** |
| 6a | AMD ISA tables + GFX1030 encoder | **Complete** |
| 6b | AMD legalization + VGPR/SGPR register allocation | **Complete** |
| 6c | AMD f64 lowering (native `v_fma_f64`) | **Complete** |
| 6d | AMD compilation validation vs RADV/ACO | **Complete** |
| 7 | coralDriver — userspace GPU dispatch (AMD + NVIDIA) | **Complete** |
| 8 | coralGpu — unified Rust GPU abstraction | **Complete** |
| 9 | Full sovereignty — zero FFI, zero C, all Rust | **Complete** |

## Evolution Policy

FFI is acceptable as scaffolding in early passes. Every FFI
introduction is tracked for Rust replacement. No FFI survives to
production release. Each pass produces strictly better Rust.

See `specs/SOVEREIGN_MULTI_GPU_EVOLUTION.md` for the full evolution
plan, pass definitions, and dependency tracking.

---

**Date**: March 6, 2026
**Version**: 0.5.0
