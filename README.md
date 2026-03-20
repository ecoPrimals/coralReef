<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# coralReef

**Status**: Phase 10 — Iteration 59 (Deep Coverage Expansion + Clone Reduction)  
**Purpose**: Sovereign Rust GPU compiler — WGSL/SPIR-V/GLSL → native GPU binary

---

## Overview

coralReef is a pure-Rust GPU shader compiler. It compiles WGSL,
SPIR-V, and GLSL 450 compute shaders to native GPU binaries, with
full f64 transcendental support. Zero C dependencies, zero libc, FxHashMap internalized, zero vendor lock-in.

NVIDIA backend complete (SM70–SM89). AMD backend operational
(RDNA2/GFX1030 — RX 6950 XT on-site). Both share the same IR,
optimization passes, and `ShaderModel` trait — Rust's trait dispatch
drives vendor-specific legalization, register allocation, and encoding.
No manual vtables, no C-era dispatch macros.

coralDriver provides userspace GPU dispatch via DRM ioctl — AMD amdgpu
(fully wired: GEM, PM4, CS submit, fence sync), NVIDIA nouveau
(legacy + new UAPI: VM_INIT/VM_BIND/EXEC for kernel 6.6+, auto-detected),
nvidia-drm/UVM (proprietary driver with RM alloc), and NVIDIA VFIO
(direct BAR0/DMA dispatch without kernel GPU driver — maximum sovereignty).
coralGpu unifies compilation and dispatch into a single API with automatic
multi-GPU detection and sovereign driver preference (`vfio` > `nouveau` >
`amdgpu` > `nvidia-drm`). Every layer pure Rust — zero FFI, zero `*-sys`,
zero `extern "C"`, syscalls via rustix.

Part of the ecoPrimals Sovereign Compute Evolution.

## Quick Start

```bash
# Rust 1.85+ required (edition 2024)
cargo check --workspace
cargo test --workspace     # 3038+ passing, 0 failed (+102 ignored VFIO/hardware)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

## Compilation Pipeline

```
WGSL / SPIR-V / GLSL input
       │
       ▼
┌──────────────────┐
│  Frontend (naga)  │  Parse WGSL/SPIR-V/GLSL → naga IR (pluggable)
└────────┬─────────┘
         ▼
┌──────────────────────────────────────────┐
│  codegen (shared)                         │
│  ├ naga_translate  naga IR → SSA IR      │
│  ├ lower_f64       f64 transcendentals   │
│  ├ lower_fma       FMA contraction ctrl  │
│  ├ optimize        copy prop, DCE, ...   │
│  └ pipeline.rs     orchestration         │
└────────┬─────────────────────────────────┘
         │
    ┌────┴─────────────┐
    ▼                  ▼
┌────────────┐  ┌────────────┐
│ nv/ backend │  │ amd/       │
│ SM20–SM89   │  │ GFX1030+   │
│ SASS binary │  │ GFX binary │
└────────────┘  └────────────┘
         │             │
         ▼             ▼
┌───────────────────────────────┐
│ coral-driver                  │
│ ├ amd/  DRM amdgpu ioctl    │
│ ├ nv/   DRM nouveau ioctl   │
│ ├ nv/   nvidia-drm (compat) │
│ ├ nv/   UVM infra (research) │
│ └ vfio/ VFIO direct dispatch │
└───────────────────────────────┘
         │
         ▼
┌───────────────────────────────┐
│ coral-gpu                     │
│ Unified compile + dispatch   │
└───────────────────────────────┘
```

## Structure

```
coralReef/
├── Cargo.toml                    # Workspace root
├── crates/
│   ├── coralreef-core/            # Primal lifecycle + IPC (JSON-RPC, tarpc)
│   ├── coral-reef/                # Shader compiler (WGSL + SPIR-V + GLSL)
│   │   ├── src/
│   │   │   ├── backend.rs        # Backend trait (vendor-agnostic)
│   │   │   ├── frontend.rs       # Frontend trait (WGSL, SPIR-V, GLSL)
│   │   │   ├── gpu_arch.rs       # GpuTarget: Nvidia/Amd/Intel
│   │   │   └── codegen/          # Compiler core
│   │   │       ├── ir/           # SSA IR types
│   │   │       ├── naga_translate/ # naga → codegen IR translation
│   │   │       ├── lower_f64/    # f64 transcendental lowering
│   │   │       ├── nv/           # NVIDIA vendor backend
│   │   │       ├── amd/          # AMD vendor backend
│   │   │       │   ├── shader_model.rs  # ShaderModelRdna2 (direct trait impl)
│   │   │       │   ├── encoding.rs      # RDNA2 instruction encoding
│   │   │       │   ├── isa_generated/   # 1,446 ISA opcodes (Rust-generated)
│   │   │       │   └── reg.rs           # VGPR/SGPR register model
│   │   │       └── pipeline.rs   # Full compilation pipeline
│   │   ├── src/tol.rs            # 13-tier numerical tolerance model
│   │   └── tests/                # Integration tests + WGSL corpus
│   ├── coral-driver/              # Userspace GPU dispatch (DRM ioctl)
│   │   └── src/
│   │       ├── drm.rs            # Pure Rust DRM interface (multi-GPU scan)
│   │       ├── amd/              # amdgpu: GEM, PM4, command submission, fence
│   │       └── nv/               # nouveau (sovereign) + nvidia-drm (compatible)
│   ├── coral-gpu/                 # Unified GPU compute + driver preference
│   ├── coral-reef-bitview/        # Bit-level field access for GPU encoding
│   ├── coral-reef-isa/            # ISA tables, latency model
│   ├── coral-glowplug/            # GPU device broker (VFIO, health, hot-swap)
│   ├── coral-ember/               # VFIO device swap daemon (SCM_RIGHTS)
│   ├── coral-reef-stubs/          # Pure-Rust dependency replacements
│   └── nak-ir-proc/              # Proc-macro derives for IR types
├── tools/
│   └── amd-isa-gen/              # Pure Rust ISA table generator (replaces Python)
├── specs/                        # Architecture specification + evolution plan
├── showcase/                     # Progressive demos (hello-compiler → compute triangle)
├── whitePaper/                   # Theory docs (f64 lowering, transcendental analysis)
└── genomebin/                    # Deployment scaffolding
```

## Crates

| Crate | Purpose |
|-------|---------|
| `coralreef-core` | Primal lifecycle, health, CLI (`server`/`compile`/`doctor`), JSON-RPC + tarpc (bincode) IPC, FMA control, multi-device compile API |
| `coral-reef` | Shader compiler — 24 spring absorption tests passing (14 original + 4 FMA + 6 neuralSpring), f64 lowering, optimizers, RA, vendor encoding |
| `coral-driver` | Userspace GPU dispatch — AMD amdgpu (full: GEM+PM4+CS+fence) + NVIDIA nouveau (sovereign) + nvidia-drm (compatible) via DRM ioctl. Multi-GPU scan, pure Rust, zero libc, UVM research infra |
| `coral-gpu` | Unified GPU compute — compile + dispatch in one API, multi-GPU auto-detect, `DriverPreference` (sovereign default: vfio > nouveau > amdgpu > nvidia-drm), `from_vfio()` convenience API, FMA capability reporting, `PCIe` topology discovery |
| `coral-reef-bitview` | `BitViewable`/`BitMutViewable` traits + `TypedBitField<OFFSET, WIDTH>` compile-time safe bit access |
| `coral-reef-isa` | ISA encoding tables, instruction latencies (SM30–SM120, AMD RDNA2) |
| `coral-reef-stubs` | Pure-Rust dependency replacements: CFG, BitSet, dataflow, SmallVec, fxhash |
| `nak-ir-proc` | Proc-macro derives: `SrcsAsSlice`, `DstsAsSlice`, `DisplayOp`, `FromVariants`, `Encode` |
| `primal-rpc-client` | Pure Rust JSON-RPC 2.0 client for inter-primal communication (tests + production) |
| `coral-glowplug` | GPU device broker — VFIO device management, JSON-RPC socket, health monitoring, hot-swap, circuit breaker, boot sovereignty. `bdf: Arc<str>` for zero-alloc device identity |
| `coral-ember` | VFIO device swap daemon — `SCM_RIGHTS` fd passing (fully safe via `rustix` `AsFd`), vendor lifecycle hooks, Xorg/udev isolation |
| `amd-isa-gen` | Pure Rust ISA table generator from AMD XML specs (replaces Python scaffold) |

## f64 Transcendental Support

NVIDIA: DFMA software lowering (hardware SFU is f32-only).
AMD: Native `v_fma_f64` / `v_sqrt_f64` / `v_rcp_f64` emission.

| Function | NVIDIA | AMD | Precision |
|----------|--------|-----|-----------|
| sqrt | Rsq64H + 2 Newton-Raphson | `v_sqrt_f64` (native) | Full f64 |
| rcp | Rcp64H + 2 Newton-Raphson | `v_rcp_f64` (native) | Full f64 |
| exp2 | Range reduction + Horner | V_CVT_F32_F64 + VOP1 + V_CVT_F64_F32 (~23-bit seed) | Full f64 |
| log2 | Log2 seed + Newton | V_CVT_F32_F64 + VOP1 + V_CVT_F64_F32 (~23-bit seed) | ~52-bit (2 NR iterations) |
| sin | Cody-Waite + minimax | V_CVT_F32_F64 + VOP1 + V_CVT_F64_F32 (~23-bit seed) | Full domain |
| cos | Cody-Waite + minimax | V_CVT_F32_F64 + VOP1 + V_CVT_F64_F32 (~23-bit seed) | Full domain |
| exp | x * log2(e) → exp2 | Via `v_fma_f64` | Full f64 |
| log | log2(x) * ln(2) | Via `v_fma_f64` | ~52-bit (2 NR iterations) |
| pow | log2 + mul + exp2 | Via `v_fma_f64` | ~46-bit+ |
| tan | sin/cos division | Via `v_fma_f64` | Full domain |
| atan | polynomial minimax | Via `v_fma_f64` | Full domain |
| asin | via atan2 | Via `v_fma_f64` | Full domain |
| acos | via atan2 | Via `v_fma_f64` | Full domain |
| sinh/cosh/tanh | exp-based | Via `v_fma_f64` | Full domain |
| Complex64 | preamble (auto-prepend) | Via `v_fma_f64` | Full domain |

## Checks

| Check | Status |
|-------|--------|
| `cargo check --workspace` | PASS |
| `cargo test --workspace` | PASS (3038+ passing, 0 failed, 102 ignored hardware-gated) |
| `cargo llvm-cov` | 66.1% region / 65.8% line / 72.9% function (non-hardware: 79.6%) |
| `cargo clippy --workspace --features vfio -- -D warnings` | PASS (0 warnings) |
| `cargo fmt --check` | PASS |
| `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` | PASS (0 warnings) |

## Driver Sovereignty

coralReef compiles for everything, prefers open-source drivers at runtime:

```
Default:   vfio → nouveau → amdgpu → nvidia-drm
Override:  CORALREEF_DRIVER_PREFERENCE=nvidia-drm,amdgpu
```

The compiled shader binary is identical regardless of which driver dispatches it.
Sovereignty is a runtime choice, not a compile-time lock.

## Showcase

8 progressive demos in `showcase/` — from hello-compiler to the full
compute triangle (coralReef → toadStool → barraCuda). Level 00 works
anywhere (compile-only). Level 01 requires GPU hardware. Level 02
demonstrates inter-primal ecosystem integration.

```bash
cd showcase/00-local-primal/01-hello-compiler && ./demo.sh
```

## Hardware — On-Site

| GPU | Architecture | Kernel Driver | f64 | Role |
|-----|-------------|---------------|-----|------|
| NVIDIA Titan V #1 | Volta SM70 (GV100) | vfio-pci | 1/2 | Oracle card (VFIO sovereign) |
| NVIDIA Titan V #2 | Volta SM70 (GV100) | vfio-pci | 1/2 | Compute target (VFIO sovereign) |
| NVIDIA RTX 5060 | Ada SM89 | nvidia-drm | 1/64 | Desktop + UVM dispatch |

## vs CUDA / Kokkos

| | CUDA | Kokkos | coralReef |
|---|---|---|---|
| Vendor lock-in | NVIDIA only | Abstracts (needs SDK underneath) | None — generates native ISA directly |
| C/C++ dependency | CUDA toolkit | Host compiler + vendor SDK | Zero — pure Rust |
| GPU ISAs | PTX → SASS (NVIDIA only) | Delegates to vendor | SASS (SM70–89) + GCN/RDNA (AMD) |
| Runtime library | libcuda.so | kokkos runtime | None — DRM ioctl dispatch |
| Cross-vendor | No | Yes (via SDKs) | Yes (native, no SDK) |
| Open source | No (ptxas proprietary) | Yes | Yes (AGPL-3.0-only) |

## Sovereign Evolution

Each evolution pass produces strictly better Rust. FFI is scaffolding —
tracked and replaced. The Rust language and compilation model is the
advantage. See `specs/SOVEREIGN_MULTI_GPU_EVOLUTION.md`.

| Phase | Milestone | Status |
|-------|-----------|--------|
| 1–5.7 | NVIDIA compiler, pure Rust, 710 tests | **Complete** |
| 6a | AMD ISA tables + encoder (LLVM-validated) | **Complete** |
| 6b–6d | AMD legalization, RA, f64, end-to-end | **Complete** |
| 7 | coralDriver (AMD amdgpu + NVIDIA nouveau) | **Complete** |
| 8 | coralGpu (unified Rust GPU abstraction) | **Complete** |
| 9 | Full sovereignty (zero FFI, zero C) | **Complete** |
| 10 | Spring absorption, compiler hardening, E2E verified | **Iteration 59 — Deep Coverage + Clone Reduction, 3038+ workspace tests, 65.8% line coverage (79.6% non-hardware)** |

---

**License**: AGPL-3.0-only (upstream-derived files retain original attribution)
**Standalone primal** — zero-knowledge startup, capability-based discovery, no hardcoded primals  
**IPC**: `shader.compile.wgsl`, `shader.compile.spirv`, `shader.compile.wgsl.multi`, `shader.compile.status`, `shader.compile.capabilities`, `health.check`, `health.liveness`, `health.readiness` — JSON-RPC 2.0 + tarpc
