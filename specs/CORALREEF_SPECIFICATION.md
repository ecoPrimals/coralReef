<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# coralReef — Specification

**Version**: 0.2.0
**Date**: Aug 3, 2026
**Status**: Phase 10 — Sprint 14 / Wave 156i (Pure compiler primal, 3,540 tests, zero unsafe)

---

## Purpose

coralReef is a sovereign Rust GPU compiler. It compiles WGSL, SPIR-V,
and GLSL 450 compute shaders to native GPU binaries with full f64
transcendental support, as a standalone pure-Rust workspace.

Multi-vendor architecture: NVIDIA (SM35–SM120) and AMD (GCN5–RDNA4)
backends operational. Both share the same IR, optimizer passes, and
`ShaderModel` trait — Rust trait dispatch, no manual vtables.

**Note (Sprint 9)**: coralReef is now a pure compiler primal. GPU dispatch
(DRM ioctl, VFIO) and device lifecycle (PCIe binding, health monitoring,
personality hot-swap) were excised and delegated to toadStool. coralReef
compiles shaders; toadStool dispatches them. Zero FFI, zero `*-sys`,
zero `extern "C"`, zero unsafe.

## Target Hardware

| GPU | Architecture | ISA | Kernel Driver | f64 | Role |
|-----|-------------|-----|---------------|-----|------|
| NVIDIA Titan V #1 | Volta SM70 (GV100) | SASS | vfio-pci (sovereign) | 1/2, native | Oracle card — sovereign VFIO dispatch |
| NVIDIA Titan V #2 | Volta SM70 (GV100) | SASS | vfio-pci (sovereign) | 1/2, native | Compute target — sovereign VFIO dispatch |
| NVIDIA RTX 5060 | Ada SM89 | SASS | nvidia-drm | 1/64, DF64 | Desktop display + UVM dispatch |
| AMD MI50 | Vega GFX906 | GCN | amdgpu (open) | Full rate | GFX9 cross-architecture validation |

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
  IPC (JSON-RPC / tarpc)
       │
       ▼
  toadStool (hardware dispatch)
```

## Crate Layout

| Crate | Purpose |
|-------|---------|
| `coralreef-core` | Primal lifecycle, health, IPC (JSON-RPC 2.0, tarpc), zero-copy `Bytes` |
| `coral-reef` | Shader compiler: pluggable frontend, f64 lowering, optimizers, RA, vendor encoding |
| `coral-reef-isa` | ISA tables, instruction latencies (SM35–SM120, AMD RDNA2) |
| `coral-reef-stubs` | Pure-Rust dependency replacements: CFG, BitSet, dataflow, SmallVec, fxhash |
| `coral-reef-bitview` | Bit-level field manipulation for instruction encoding |
| `nak-ir-proc` | Proc-macro derives: `SrcsAsSlice`, `DstsAsSlice`, `DisplayOp`, `FromVariants`, `Encode` |
| `amd-isa-gen` | Pure Rust ISA table generator from AMD XML specs |
| `primal-rpc-client` | Pure Rust JSON-RPC 2.0 client for inter-primal IPC |

> **Note (Sprint 9)**: `coral-driver`, `coral-gpu`, `coral-glowplug`, and `coral-ember` were excised in Sprint 9. Hardware dispatch is now toadStool's domain. This spec documents the compiler architecture only.

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
| 10 | Compiler hardening, Compute Trio, diesel excision, pure compiler evolution | **Sprint 14 / Wave 156i — 3,540 tests, zero unsafe, zero debt** |

## Full-GPU Silicon Exploitation — Future Horizons

coralReef currently targets compute shaders, engaging ~55% of GPU silicon
(shader cores + tensor cores). The long-term vision is **full silicon
exploitation** — every hardware unit on the GPU doing useful work, whether
for physics simulation, game rendering, scientific visualization, or any
domain where the math runs on parallel hardware.

This aligns with ludoSpring's Symphony Architecture: the GPU is an orchestra
where shader cores, tensor cores, RT cores, TMUs, ROPs, and the rasterizer
all play simultaneously. coralReef is the composer that writes the score
for every section.

### GPU Hardware Units — Current and Future Coverage

| GPU Unit | Silicon % | Current | Future | coralReef Work |
|----------|-----------|---------|--------|---------------|
| Shader cores (CUDA/CU) | ~40% | Compute shaders | + Vertex/Fragment/Mesh shaders | Graphics-stage compilation |
| Tensor cores | ~15% | HMMA GEMM (`compile_gemm`) | Cooperative matrix in WGSL | WGSL spec evolution |
| RT cores | ~10% | Not targeted | RayQuery in compute | `Statement::RayQuery` PTX emission |
| TMUs | ~10% | `ImageSample`/`textureGather` | Compute lookup tables, biome maps | Already started (Sprint 11) |
| ROPs | ~8% | Not targeted | Blend/output in graphics pipeline | Graphics pipeline compilation |
| Rasterizer | ~5% | Not targeted | Triangle scan conversion | Vertex shader + SPH emission |
| L2 cache | ~8% | Implicit | Persistent frame state, double buffers | toadStool dispatch strategy |
| Memory controllers | ~4% | Implicit | Bandwidth-limited streaming | Compile-time memory layout hints |

### Evolution Phases

| Phase | Silicon Engaged | What Ships | Ecosystem Impact |
|-------|----------------|-----------|-----------------|
| **Phase A (current)** | ~55% (shader + tensor) | Compute shaders SM37-SM120, HMMA GEMM, subgroups, f64, ImageSample/Store/Query/Gather, function inlining | hotSpring QCD, barraCuda math, ludoSpring GPU physics |
| **Phase B (near)** | ~65% (+ RT cores) | `RayQuery` in compute shaders: `Initialize`, `Proceed`, `GetIntersection` → PTX `optix.*` / inline RT | Spatial queries, line-of-sight, nearest-neighbor, `game.gpu.batch_raycast` |
| **Phase C (medium)** | ~85% (+ rasterizer + ROPs + TMUs) | Vertex + Fragment shader compilation: graphics builtins, SPH headers, interpolation, derivatives, `discard` | Full render pipeline, petalTongue 3D sovereign rendering, ludoSpring game visuals |
| **Phase D (far)** | ~95% (+ mesh/task) | Mesh shaders, task shaders, full modern graphics pipeline | AAA-equivalent rendering without vendor SDK |

### Phase B: RayQuery — RT Core Activation

naga's `Statement::RayQuery` carries:
- `Initialize { acceleration_structure, descriptor }` — cast a ray into a BVH
- `Proceed { result }` — advance to next intersection candidate
- `GenerateIntersection { hit_t }` — add candidate hit
- `ConfirmIntersection` — confirm triangle intersection
- `Terminate` — stop query

PTX emission targets `optix.*` intrinsics for SM75+ or inline ray tracing
instructions. This keeps shaders in the compute domain while activating RT
cores for hardware-accelerated spatial queries.

Use cases that unlock immediately:
- ludoSpring `game.gpu.batch_raycast` — GPU raycasting for visibility, pathfinding
- hotSpring spatial queries — particle neighbor detection via BVH
- petalTongue 3D visualization — ambient occlusion, shadow probes

### Phase C: Graphics Pipeline — Full Render

Vertex and fragment shader compilation requires:

1. **Graphics-stage entry points**: `@vertex` and `@fragment` in WGSL (naga already parses these)
2. **Shader Program Header (SPH)**: NVIDIA graphics pipelines require SPH metadata (GPR count, output topology, attribute mapping) — infrastructure partially exists
3. **Graphics builtins**: `@builtin(position)`, `@location(N)`, interpolation qualifiers
4. **Fragment operations**: `dpdx`/`dpdy` derivatives, `discard`, depth writes
5. **Render state**: Pipeline state objects (blend, depth, stencil) — toadStool domain

Once vertex + fragment compilation ships, the Symphony model from ludoSpring
becomes fully sovereign: CPU game logic + GPU compute physics + GPU render
visuals, all compiled by coralReef, dispatched by toadStool, with zero vendor
SDK in the loop.

### The Universal Principle

Real physics simulations and videogame physics are both math dispatched to
parallel hardware. The rasterizer is a special-purpose computer that answers
"which pixels does this triangle cover?" — a pure function. Fragment shading
is "what color is this pixel?" — another pure function. There is no
fundamental difference between a lattice QCD HMC trajectory and a game
physics frame: both are parallel math with a time budget.

coralReef's role is to compile that math — all of it — to every piece of
silicon available. The ecosystem (toadStool dispatch, barraCuda routing,
ludoSpring/hotSpring/petalTongue domain logic) orchestrates the symphony.
coralReef writes the score.

## Evolution Policy

FFI is acceptable as scaffolding in early passes. Every FFI
introduction is tracked for Rust replacement. No FFI survives to
production release. Each pass produces strictly better Rust.

See `docs/archive/SOVEREIGN_MULTI_GPU_EVOLUTION.md` for the historical evolution
plan, pass definitions, and dependency tracking.

---

**Date**: Aug 3, 2026
**Version**: 0.2.0 — Sprint 14 / Wave 156g
