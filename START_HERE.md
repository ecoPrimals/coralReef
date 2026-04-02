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

Three input languages feed the same pipeline via the sovereign `coral-parse`
frontend (pure Rust): WGSL (primary), SPIR-V (binary intermediate), and
GLSL 450 compute (for absorbing existing GPU compute libraries). The legacy
naga frontend is available as an optional Cargo feature for diff-testing.

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
cargo test --workspace     # 4200+ passing, 0 failed (~155 ignored hardware-gated)
cargo test -p coral-parse -p coral-reef --no-default-features  # 1264 sovereign-only tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --check
```

## Repository Layout

```
coralReef/
├── crates/
│   ├── coralreef-core/         Primal lifecycle + IPC (JSON-RPC, tarpc)
│   ├── coral-parse/            Sovereign compiler frontend (pure Rust)
│   │   └── src/
│   │       ├── ast/            Sovereign AST (Module, Type, Expression, Statement)
│   │       ├── wgsl/           WGSL lexer + recursive-descent parser
│   │       ├── spirv/          SPIR-V binary reader (two-pass)
│   │       ├── glsl/           GLSL 450/460 lexer + parser
│   │       └── lower/          AST → CoralIR lowering (6 submodules)
│   ├── coral-reef/             Shader compiler core (IR + backends)
│   │   └── src/
│   │       ├── backend.rs      Backend trait (vendor-agnostic)
│   │       ├── frontend.rs     Frontend trait (pluggable)
│   │       ├── gpu_arch.rs     GpuTarget: Nvidia/Amd/Intel
│   │       └── codegen/        Compiler core
│   │           ├── ir/            SSA IR types
│   │           ├── naga_translate/ naga → codegen IR (optional feature)
│   │           ├── lower_f64/     f64 transcendental expansion
│   │           ├── nv/            NVIDIA vendor backend (SM35–SM120)
│   │           ├── amd/           AMD vendor backend (GFX906+RDNA2)
│   │           ├── assign_regs/   Register allocation
│   │           ├── calc_instr_deps/ Instruction dependency analysis
│   │           ├── spill_values/  Register spilling
│   │           ├── builder/       IR construction helpers
│   │           └── pipeline.rs    Full compilation pipeline
│   ├── coral-driver/            Userspace GPU dispatch (DRM ioctl)
│   │   └── src/
│   │       ├── drm.rs           Pure Rust DRM interface (rustix syscalls)
│   │       ├── amd/             amdgpu: GEM, PM4, command submission, fence
│   │       └── nv/              nouveau: channel, GEM, QMD, pushbuf submit
│   ├── coral-glowplug/         GPU device broker (VFIO, mailbox, multi-ring)
│   ├── coral-ember/            VFIO fd holder + ring-keeper (restart persistence)
│   ├── coral-gpu/              Unified GPU compute abstraction
│   ├── coral-reef-bitview/     Bit-level field access for GPU encoding
│   ├── coral-reef-isa/         ISA tables, latency model
│   ├── coral-reef-jit/         Cranelift JIT backend for CPU shader execution
│   ├── coral-reef-stubs/       Pure-Rust dependency replacements
│   └── nak-ir-proc/            Proc-macro derives for IR types
├── tools/
│   └── amd-isa-gen/            Pure Rust ISA table generator
├── specs/                      Architecture specification
├── whitePaper/                 Theory docs (f64 lowering, transcendentals)
└── genomebin/                  Deployment scaffolding
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
WGSL / SPIR-V / GLSL  →  coral-parse (sovereign)  →  AST → CoralIR lowering
    →  lower_f64  →  optimize (copy prop, DCE, bar prop, scheduling)
    →  legalize  →  assign_regs  →  Backend (encode)  →  native binary

Alternative path (feature-gated):
    →  NagaFrontend  →  naga_translate (codegen IR)  →  same pipeline
```

Key architecture: `Shader<'a>` holds `&'a dyn ShaderModel`. Each GPU
architecture implements `ShaderModel` directly — the Rust compiler
drives vendor dispatch via trait objects. No manual vtables.

Key crates:

- **`coral-parse`** — Sovereign compiler frontend. Pure-Rust WGSL lexer +
  parser, SPIR-V binary reader, GLSL 450 lexer + parser. AST → CoralIR
  lowering via 6 focused submodules (math, binary, convert, stmt, builtin).
  Replaces naga for all parsing — zero external parser dependencies.

Key modules in `crates/coral-reef/src/codegen/`:

- **`naga_translate/`** — (Optional, `naga` feature) Translates naga IR into
  the codegen SSA IR. Available for diff-testing against coral-parse.
- **`lower_f64/`** — Expands f64 transcendental placeholder ops into
  hardware instruction sequences. NVIDIA: Newton-Raphson for sqrt/rcp,
  Horner polynomial for exp2. AMD: native v_sqrt_f64/v_rcp_f64 passthrough.
- **`pipeline.rs`** — Orchestrates the full compilation: optimize →
  lower_f64 → legalize → register allocation → encode.
- **`ir/`** — SSA intermediate representation with typed registers,
  predication, memory access descriptors, and shader metadata.
- **`nv/`** — NVIDIA vendor backend: SM35–SM120 instruction encoders.
- **`amd/`** — AMD vendor backend: GFX906 + RDNA2 GFX1030 encoder.

## Architecture

This primal starts with zero knowledge. It advertises its capabilities
(`shader.compile`, `shader.health`) via the universal adapter and discovers
peers by capability, not by name. Iteration 65 added `identity.get` (per
CAPABILITY_BASED_DISCOVERY_STANDARD), fire-and-forget `capability.register` with
ecosystem integration, and periodic `ipc.heartbeat` registration (45s) toward
Songbird. Iteration 70d–70e added CPU shader execution (`shader.compile.cpu`,
`shader.execute.cpu`, `shader.validate`) via Naga interpreter (Path A) and
Cranelift JIT (`coral-reef-jit`, Path B) for dual-path validation.

---

*Read `WHATS_NEXT.md` for completed phases and future work.*
