<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# coralReef — Specification

**Version**: 0.6.0
**Date**: March 21, 2026
**Status**: Phase 10 — Iteration 62 (Deep Audit + Coverage + Hardcoding Evolution)

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

coralGlowPlug manages GPU lifecycle at the PCIe level — boot-persistent VFIO binding, health monitoring with circuit breaker, personality hot-swap, and boot sovereignty that prevents vendor drivers from touching managed devices.

## Target Hardware

| GPU | Architecture | ISA | Kernel Driver | f64 | Role |
|-----|-------------|-----|---------------|-----|------|
| NVIDIA Titan V #1 | Volta SM70 (GV100) | SASS | vfio-pci (sovereign) | 1/2, native | Oracle card — sovereign VFIO dispatch |
| NVIDIA Titan V #2 | Volta SM70 (GV100) | SASS | vfio-pci (sovereign) | 1/2, native | Compute target — sovereign VFIO dispatch |
| NVIDIA RTX 5060 | Ada SM89 | SASS | nvidia-drm | 1/64, DF64 | Desktop display + UVM dispatch |
| AMD MI50 (planned) | Vega GFX906 | GCN | amdgpu (open) | Full rate | GFX9 cross-architecture validation |

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
| `coral-glowplug` | PCIe GPU lifecycle daemon: VFIO binding, health monitoring, circuit breaker, personality hot-swap, boot sovereignty |
| `primal-rpc-client` | Pure Rust JSON-RPC 2.0 client for inter-primal IPC |

## Sovereign Pipeline — All Silicon

The aim is to target every piece of silicon available. Each GPU has a sovereign
dispatch path that bypasses vendor kernel modules entirely where possible.

### Dispatch Paths

| Path | Silicon | Status | Remaining Gap |
|------|---------|--------|---------------|
| VFIO BAR0 + PFIFO | GV100 (Titan V ×2) | 6/7 — GP_PUT last mile | Cache flush experiment (H1) |
| UVM (nvidia-drm) | SM89 (RTX 5060) | Code-complete | Hardware validation needed |
| DRM nouveau | SM70 (Volta) | Struct-complete | PMU firmware blocker |
| DRM amdgpu | GFX1030 (RDNA2) | E2E proven | — (COMPLETE) |
| DRM amdgpu | GFX906 (Vega) | Planned | MI50 hardware swap |

### Sovereign Boot (Iteration 56)

nvidia's open kernel module probes ALL nvidia PCI devices at boot. On GV100
(no GSP), the failed probe corrupts hardware state. coralReef defends at three layers:

1. **Kernel preemption**: `softdep nvidia pre: vfio-pci` + `vfio-pci.ids=10de:1d81`
2. **Circuit breaker**: halts BAR0 reads after 6 consecutive faults
3. **nvidia module guard**: blocks swap/resurrect when nvidia.ko loaded

### FECS Sovereign Compute (In Progress)

hotSpring Exp 068 proved FECS firmware executes from host-loaded IMEM on clean
falcon after D3hot→D0 cycle. Remaining: GPCCS address discovery on GV100,
DMA instance block, FECS halt resolution at PC=0x2835.

### Sovereignty Roadmap

| Phase | Target | Status |
|-------|--------|--------|
| Boot preemption (vfio-pci.ids) | GV100 protected from nvidia | COMPLETE (Iter 56) |
| GP_PUT DMA dispatch | Sovereign GPFIFO execution | 6/7 (cache flush H1 next) |
| UVM dispatch | RTX 5060 compute | Code-complete (needs HW validation) |
| Custom PMU Falcon firmware | Replace vendor firmware dependency | PLANNED |
| Sovereign HBM2 training | Direct FBPA/LTC/PFB register programming | PLANNED |
| Vendor-agnostic abstraction | Unified AMD/NVIDIA init + power + memory | VISION |

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
| 10 | Security hardening, boot sovereignty, all-silicon pipeline, deep debt evolution, hotSpring firmware wiring | **Complete** |
| 11 | Sovereign compiler frontend (coral-parse), naga elimination, deep debt resolution | **Iteration 71 — 4200+ tests, 1264 sovereign-only, ~66% line coverage** |

## Evolution Policy

FFI is acceptable as scaffolding in early passes. Every FFI
introduction is tracked for Rust replacement. No FFI survives to
production release. Each pass produces strictly better Rust.

See `specs/SOVEREIGN_MULTI_GPU_EVOLUTION.md` for the full evolution
plan, pass definitions, and dependency tracking.

---

**Date**: March 21, 2026
**Version**: 0.6.0
