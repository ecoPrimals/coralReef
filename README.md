# coralReef

**Status**: Phase 10 — Iteration 12 (Compiler Gaps + Math Coverage + Cross-Spring Wiring)
**Purpose**: Sovereign Rust GPU compiler — WGSL/SPIR-V → native GPU binary

---

## Overview

coralReef is a pure-Rust GPU shader compiler. It compiles WGSL and
SPIR-V compute shaders to native GPU binaries, with full f64
transcendental support. Zero C dependencies, zero vendor lock-in.

NVIDIA backend complete (SM70–SM89). AMD backend operational
(RDNA2/GFX1030 — RX 6950 XT on-site). Both share the same IR,
optimization passes, and `ShaderModel` trait — Rust's trait dispatch
drives vendor-specific legalization, register allocation, and encoding.
No manual vtables, no C-era dispatch macros.

coralDriver provides userspace GPU dispatch via DRM ioctl — AMD amdgpu
(fully wired: GEM, PM4, CS submit, fence sync) and NVIDIA nouveau
(channel alloc, GEM, pushbuf submit, QMD dispatch). coralGpu unifies
compilation and dispatch into a single API with automatic hardware
detection. Every layer pure Rust — zero FFI, zero `*-sys`, zero
`extern "C"`.

Part of the ecoPrimals Sovereign Compute Evolution.

## Quick Start

```bash
# Rust 1.85+ required (edition 2024)
cargo check --workspace
cargo test --workspace     # 991 tests (955 passing, 36 ignored)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

## Compilation Pipeline

```
WGSL / SPIR-V input
       │
       ▼
┌──────────────────┐
│  Frontend (naga)  │  Parse WGSL/SPIR-V → naga IR (pluggable)
└────────┬─────────┘
         ▼
┌──────────────────────────────────────────┐
│  codegen (shared)                         │
│  ├ naga_translate  naga IR → SSA IR      │
│  ├ lower_f64       f64 transcendentals   │
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
│ └ nv/   DRM nouveau ioctl   │
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
│   ├── coral-reef/                # Shader compiler
│   │   ├── src/
│   │   │   ├── backend.rs        # Backend trait (vendor-agnostic)
│   │   │   ├── frontend.rs       # Frontend trait (pluggable parsers)
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
│   │       ├── drm.rs            # Pure Rust DRM interface (via libc)
│   │       ├── amd/              # amdgpu: GEM, PM4, command submission, fence
│   │       └── nv/               # nouveau: channel, GEM, QMD, pushbuf submit
│   ├── coral-gpu/                 # Unified GPU compute abstraction
│   ├── coral-reef-bitview/        # Bit-level field access for GPU encoding
│   ├── coral-reef-isa/            # ISA tables, latency model
│   ├── coral-reef-stubs/          # Pure-Rust dependency replacements
│   └── nak-ir-proc/              # Proc-macro derives for IR types
├── tools/
│   └── amd-isa-gen/              # Pure Rust ISA table generator (replaces Python)
├── specs/                        # Architecture specification + evolution plan
├── whitePaper/                   # Theory docs (f64 lowering, transcendental analysis)
└── genomebin/                    # Deployment scaffolding
```

## Crates

| Crate | Purpose |
|-------|---------|
| `coralreef-core` | Primal lifecycle, health, CLI (`server`/`compile`/`doctor`), JSON-RPC + tarpc IPC, FMA control |
| `coral-reef` | Shader compiler — 14/27 cross-spring shaders compiling, f64 lowering, optimizers, RA, vendor encoding |
| `coral-driver` | Userspace GPU dispatch — AMD amdgpu (full: GEM+PM4+CS+fence) + NVIDIA nouveau (channel+GEM+pushbuf+QMD+CBUF+fence) via DRM ioctl (pure Rust, bytemuck, zero FFI) |
| `coral-gpu` | Unified GPU compute — compile WGSL + dispatch on hardware in one API, auto-detect DRM render nodes |
| `coral-reef-bitview` | `BitViewable`/`BitMutViewable` traits + `TypedBitField<OFFSET, WIDTH>` compile-time safe bit access |
| `coral-reef-isa` | ISA encoding tables, instruction latencies (SM30–SM120, AMD RDNA2) |
| `coral-reef-stubs` | Pure-Rust dependency replacements: CFG, BitSet, dataflow, SmallVec, fxhash |
| `nak-ir-proc` | Proc-macro derives: `SrcsAsSlice`, `DstsAsSlice`, `DisplayOp`, `FromVariants`, `Encode` |
| `amd-isa-gen` | Pure Rust ISA table generator from AMD XML specs (replaces Python scaffold) |

## f64 Transcendental Support

NVIDIA: DFMA software lowering (hardware SFU is f32-only).
AMD: Native `v_fma_f64` / `v_sqrt_f64` / `v_rcp_f64` emission.

| Function | NVIDIA | AMD | Precision |
|----------|--------|-----|-----------|
| sqrt | Rsq64H + 2 Newton-Raphson | `v_sqrt_f64` (native) | Full f64 |
| rcp | Rcp64H + 2 Newton-Raphson | `v_rcp_f64` (native) | Full f64 |
| exp2 | Range reduction + Horner | Polynomial via `v_fma_f64` | Full f64 |
| log2 | Log2 seed + Newton | Polynomial via `v_fma_f64` | ~46-bit+ |
| sin | Cody-Waite + minimax | Cody-Waite via `v_fma_f64` | Full domain |
| cos | Cody-Waite + minimax | Cody-Waite via `v_fma_f64` | Full domain |

## Checks

| Check | Status |
|-------|--------|
| `cargo check --workspace` | PASS |
| `cargo test --workspace` | PASS (991 tests — 955 passing, 36 ignored) |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS (0 warnings) |
| `cargo fmt --check` | PASS |
| `cargo doc --workspace --no-deps` | PASS |

## Hardware — On-Site

| GPU | Architecture | Kernel Driver | f64 | Role |
|-----|-------------|---------------|-----|------|
| AMD RX 6950 XT | RDNA2 GFX1030 | amdgpu (open) | 1/16 | AMD evolution primary |
| NVIDIA RTX 3090 | Ampere SM86 | nvidia 580.119.02 | 1/32 | NVIDIA compilation target |

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
| 10 | Spring absorption, compiler hardening, E2E verified | **Iteration 10** |

---

**License**: AGPL-3.0-only (upstream-derived files retain original attribution)
**Standalone primal** — zero-knowledge startup, capability-based discovery, no hardcoded primals
