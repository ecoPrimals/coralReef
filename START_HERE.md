<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# coralReef — Start Here

Welcome to coralReef, the sovereign Rust GPU compiler.

---

## What is this?

coralReef compiles WGSL, SPIR-V, and GLSL 450 compute shaders to native
GPU binaries. It includes full f64 transcendental support — NVIDIA via
DFMA software lowering, AMD via native hardware instructions.

Vendor-agnostic architecture with pluggable frontends and backends.
NVIDIA SM70+ and AMD RDNA2 (GFX1030) backends are operational. Both
share the same `ShaderModel` trait via Rust trait dispatch.

Three input languages feed the same pipeline via the naga frontend:
WGSL (primary), SPIR-V (binary intermediate), and GLSL 450 compute
(for absorbing existing GPU compute libraries).

coralDriver provides userspace GPU dispatch via DRM ioctl (AMD amdgpu,
NVIDIA nouveau). coralGpu wraps both into a unified compile + dispatch
API. Every layer is pure Rust — zero FFI, zero `*-sys`, zero `extern "C"`.

## Prerequisites

- Rust 1.85+ (edition 2024)
- `cargo` with workspace support

## Quick Start

```bash
cd coralReef
cargo check --workspace
cargo test --workspace     # 2680+ passing, 0 failed (+48 VFIO)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

## Repository Layout

```
coralReef/
├── crates/
│   ├── coralreef-core/         Primal lifecycle + IPC (JSON-RPC, tarpc)
│   ├── coral-reef/             Shader compiler (WGSL + SPIR-V + GLSL)
│   │   └── src/
│   │       ├── backend.rs      Backend trait (vendor-agnostic)
│   │       ├── frontend.rs     Frontend trait (WGSL, SPIR-V, GLSL)
│   │       ├── gpu_arch.rs     GpuTarget: Nvidia/Amd/Intel
│   │       └── codegen/        Compiler core
│   │           ├── ir/            SSA IR types
│   │           ├── naga_translate/ naga → codegen IR translation
│   │           ├── lower_f64/     f64 transcendental expansion
│   │           ├── nv/            NVIDIA vendor backend (SM20–SM89)
│   │           ├── amd/           AMD vendor backend (RDNA2 GFX1030)
│   │           │   ├── shader_model.rs  ShaderModelRdna2 (trait impl)
│   │           │   ├── encoding.rs      instruction encoding
│   │           │   ├── isa_generated/   1,446 opcodes (Rust-generated)
│   │           │   └── reg.rs           VGPR/SGPR register model
│   │           ├── assign_regs/   Register allocation — 5 files
│   │           ├── calc_instr_deps/ Instruction dependency analysis
│   │           ├── spill_values/  Register spilling
│   │           ├── builder/       IR construction helpers
│   │           └── pipeline.rs    Full compilation pipeline
│   ├── coral-driver/            Userspace GPU dispatch (DRM ioctl)
│   │   └── src/
│   │       ├── drm.rs           Pure Rust DRM interface (inline asm syscalls)
│   │       ├── amd/             amdgpu: GEM, PM4, command submission, fence
│   │       └── nv/              nouveau: channel, GEM, QMD, pushbuf submit
│   ├── coral-gpu/               Unified GPU compute abstraction
│   ├── coral-reef-bitview/     Bit-level field access for GPU encoding
│   ├── coral-reef-isa/         ISA tables, latency model
│   ├── coral-reef-stubs/       Pure-Rust dependency replacements
│   └── nak-ir-proc/           Proc-macro derives for IR types
├── tools/
│   └── amd-isa-gen/           Pure Rust ISA table generator
├── specs/                     Architecture specification
├── whitePaper/                Theory docs (f64 lowering, transcendentals)
└── genomebin/                 Deployment scaffolding
```

## Key Documents

| Document | Purpose |
|----------|---------|
| `STATUS.md` | Current grades and phase status |
| `WHATS_NEXT.md` | Completed phases and future work |
| `specs/CORALREEF_SPECIFICATION.md` | Architecture and crate layout |
| `CONVENTIONS.md` | Coding standards |
| `CONTRIBUTING.md` | How to contribute |
| `whitePaper/F64_LOWERING_THEORY.md` | DFMA polynomial lowering design |
| `whitePaper/SOVEREIGN_COMPILER_ARCHITECTURE.md` | Architecture rationale |

## How the Compiler Works

```
WGSL / SPIR-V / GLSL  →  Frontend (naga)  →  naga_translate (codegen IR)
    →  lower_f64  →  optimize (copy prop, DCE, bar prop, scheduling)
    →  legalize  →  assign_regs  →  Backend (encode)  →  native binary
```

Key architecture: `Shader<'a>` holds `&'a dyn ShaderModel`. Each GPU
architecture implements `ShaderModel` directly — the Rust compiler
drives vendor dispatch via trait objects. No manual vtables.

Key modules in `crates/coral-reef/src/codegen/`:

- **`naga_translate/`** — Translates naga's IR into the codegen SSA IR.
  Handles expressions, statements, control flow, memory operations, builtins.
- **`lower_f64/`** — Expands f64 transcendental placeholder ops into
  hardware instruction sequences. NVIDIA: Newton-Raphson for sqrt/rcp,
  Horner polynomial for exp2. AMD: native v_sqrt_f64/v_rcp_f64 passthrough.
- **`pipeline.rs`** — Orchestrates the full compilation: optimize →
  lower_f64 → legalize → register allocation → encode.
- **`ir/`** — SSA intermediate representation with typed registers,
  predication, memory access descriptors, and shader metadata.
- **`nv/`** — NVIDIA vendor backend: SM20–SM89 instruction encoders.
- **`amd/`** — AMD vendor backend: RDNA2 GFX1030 encoder.

## Architecture

This primal starts with zero knowledge. It advertises its capabilities
(`shader.compile`, `shader.health`) via the universal adapter and discovers
peers by capability, not by name.

---

*Read `WHATS_NEXT.md` for completed phases and future work.*
