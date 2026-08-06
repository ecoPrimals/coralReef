<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# coralReef

**Version**: 0.2.0 — Phase 10, Sprint 14 / Wave 156q (G65 protocol negotiation, C3 health shim verified, C2 dual-socket. 3,689 tests, zero unsafe, zero clippy warnings)  
**Purpose**: Sovereign Rust GPU compiler — WGSL/SPIR-V/GLSL → native GPU binary

---

## Overview

coralReef is a pure-Rust GPU shader compiler. It compiles WGSL,
SPIR-V, and GLSL 450 compute shaders to native GPU binaries, with
full f64 transcendental support. Zero C dependencies, zero direct libc (transitive only via tokio/mio), FxHashMap internalized, zero vendor lock-in.

NVIDIA backend complete (SM35–SM120: Kepler through Blackwell). AMD backend operational
(RDNA2/GFX1030 — RX 6950 XT on-site, GCN5/GFX906 — MI50 E2E verified). Both share the same IR,
optimization passes, and `ShaderModel` trait — Rust's trait dispatch
drives vendor-specific legalization, register allocation, and encoding.
No manual vtables, no C-era dispatch macros.

Hardware dispatch is delegated to the `compute.dispatch` provider via IPC — coralReef is a pure
compiler primal. Zero FFI, zero `*-sys`, zero `extern "C"`, zero `unsafe`.

Part of the ecoPrimals Sovereign Compute Evolution.

## Quick Start

```bash
# Rust 1.85+ required (edition 2024)
cargo check --workspace
cargo test --workspace     # 3,536 passed, 0 failed, 6 ignored
cargo clippy --all-features -- -W clippy::pedantic -W clippy::nursery -D warnings
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
│ SM35–SM120  │  │ GFX906+    │
│ SASS binary │  │ GFX binary │
└────────────┘  └────────────┘
         │
         ▼
   IPC (JSON-RPC)
         │
         ▼
   compute.dispatch provider
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
│   │   │       ├── nv/           # NVIDIA vendor backend (SM35–SM120)
│   │   │       ├── amd/          # AMD vendor backend (RDNA2+)
│   │   │       └── pipeline.rs   # Full compilation pipeline
│   │   └── tests/                # Integration tests + WGSL corpus
│   ├── coral-reef-bitview/        # Bit-level field access for GPU encoding
│   ├── coral-reef-isa/            # ISA tables, latency model
│   ├── coral-reef-stubs/          # Pure-Rust dependency replacements
│   ├── nak-ir-proc/              # Proc-macro derives for IR types
│   └── primal-rpc-client/        # JSON-RPC 2.0 client library
├── tools/
│   └── amd-isa-gen/              # Pure Rust ISA table generator
├── specs/                        # Architecture specification
├── whitePaper/                   # Theory docs (f64 lowering, transcendentals)
└── genomebin/                    # Deployment scaffolding
```

## Crates

| Crate | Purpose |
|-------|---------|
| `coralreef-core` | Primal lifecycle, health, CLI (`server`/`compile`/`doctor`), JSON-RPC + tarpc IPC |
| `coral-reef` | Shader compiler — f64 lowering, optimizers, RA, NVIDIA + AMD vendor encoding |
| `coral-reef-bitview` | `BitViewable`/`BitMutViewable` traits + `TypedBitField<OFFSET, WIDTH>` |
| `coral-reef-isa` | ISA encoding tables, instruction latencies (SM35–SM120, AMD GCN5+RDNA2) |
| `coral-reef-stubs` | Pure-Rust dependency replacements: CFG, BitSet, dataflow, SmallVec, fxhash |
| `nak-ir-proc` | Proc-macro derives: `SrcsAsSlice`, `DstsAsSlice`, `DisplayOp`, `FromVariants`, `Encode` |
| `primal-rpc-client` | Pure Rust JSON-RPC 2.0 client for inter-primal communication |
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
| `cargo test --all-features` | PASS (3,506 passed, 0 failed, 6 ignored) |
| `cargo llvm-cov` | Target 90% line coverage |
| `cargo clippy --all-targets --all-features -- -W clippy::pedantic -W clippy::nursery -D warnings` | PASS (0 warnings) |
| `cargo fmt --check` | PASS |
| `cargo doc --all-features --no-deps` | PASS (0 warnings) |
| `cargo build --workspace --release` | PASS |

## Target Sovereignty

coralReef compiles for every supported GPU architecture. The compiled shader
binary is target-specific (SM70/SM89/RDNA2/etc.) but driver-agnostic —
toadStool handles dispatch.

## Hardware Dispatch

Hardware dispatch is owned by the `compute.dispatch` provider (discovered at runtime).
coralReef compiles shaders and returns native binaries via IPC — it never
touches hardware directly. See `infra/wateringHole/handoffs/` for the
diesel engine migration handoff.

## vs CUDA / Kokkos

| | CUDA | Kokkos | coralReef |
|---|---|---|---|
| Vendor lock-in | NVIDIA only | Abstracts (needs SDK underneath) | None — generates native ISA directly |
| C/C++ dependency | CUDA toolkit | Host compiler + vendor SDK | Zero — pure Rust |
| GPU ISAs | PTX → SASS (NVIDIA only) | Delegates to vendor | SASS (SM35–SM120) + GCN5/RDNA (AMD) |
| Runtime library | libcuda.so | kokkos runtime | None — toadStool dispatches via IPC |
| Cross-vendor | No | Yes (via SDKs) | Yes (native, no SDK) |
| Open source | No (ptxas proprietary) | Yes | Yes (AGPL-3.0-or-later) |

## Sovereign Evolution

Each evolution pass produces strictly better Rust. FFI is scaffolding —
tracked and replaced. The Rust language and compilation model is the
advantage. See `docs/archive/SOVEREIGN_MULTI_GPU_EVOLUTION.md` (historical).

| Phase | Milestone | Status |
|-------|-----------|--------|
| 1–5.7 | NVIDIA compiler, pure Rust, 710 tests | **Complete** |
| 6a | AMD ISA tables + encoder (LLVM-validated) | **Complete** |
| 6b–6d | AMD legalization, RA, f64, end-to-end | **Complete** |
| 7 | GPU driver (migrated to toadStool) | **Excised** (Sprint 9) |
| 8 | Unified GPU abstraction (migrated to toadStool) | **Excised** (Sprint 9) |
| 9 | Full sovereignty (zero FFI, zero C, zero unsafe) | **Complete** |
| 10 | Spring absorption, compiler hardening, Compute Trio, deep debt | **Active** — Sprint 14 / Wave 156g: compile alloc elimination, error reclassification, SM20 test hardening. 3,525 tests, zero debt |

---

**License**: AGPL-3.0-or-later (upstream-derived files retain original attribution)
**Standalone primal** — zero-knowledge startup, capability-based discovery, no hardcoded primals  
**IPC**: 18 served methods (JSON-RPC 2.0 + tarpc): `shader.compile.wgsl`, `shader.compile.spirv`, `shader.compile.multi`, `shader.compile.wgsl.multi`, `shader.compile.gemm`, `shader.compile.status`, `shader.compile.capabilities`, `health.check`, `health.liveness`, `health.readiness`, `health.version`, `identity.get`, `capability.list`, `capabilities.list`, `btsp.negotiate`, `auth.check`, `auth.mode`, `auth.peer_info` — 5 consumed: `compute.dispatch`, `capability.register`, `ipc.heartbeat`, `primal.announce`, `crypto.sign`
