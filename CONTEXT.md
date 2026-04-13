<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# coralReef — Context

## What is this project?

coralReef is a sovereign Rust GPU shader compiler. It compiles WGSL,
SPIR-V, and GLSL 450 compute shaders to native GPU binaries — NVIDIA
SASS (SM35–SM120) and AMD GCN5/RDNA2–RDNA4 (GFX906–GFX1201). Full f64
transcendental support. Pure Rust; transitive libc only via tokio/mio
(deferred to mio#1735 rustix migration). Zero vendor SDK.

## Ecosystem position

coralReef is one primal in the **ecoPrimals** sovereign compute
ecosystem. Primals are standalone Rust binaries that communicate via
JSON-RPC 2.0 and tarpc. They discover each other by capability at
runtime — no hardcoded primal names, no shared code imports.

Ecosystem standards live in `ecoPrimals/infra/wateringHole/`.

## Project status (Iteration 80)

- **Tests**: 4477 workspace tests, 0 failed, ~153 ignored hardware-gated (see `STATUS.md` / `CHANGELOG.md`).
- **Compliance (Iter 80)**: Wire contract documented (`SHADER_COMPILE_WIRE_CONTRACT.md`); `CompilationInfo` in IPC responses; crypto socket discovery aligned; ecoBin v3 `deny.toml` C/FFI bans; zero hardcoded primal names; all mocks test-isolated. Details in `CHANGELOG.md` and `STATUS.md`.

## Architecture

```
WGSL / SPIR-V / GLSL  →  naga frontend  →  SSA IR
  →  lower_f64  →  optimize  →  legalize  →  RA  →  encode
  →  native GPU binary
```

| Crate | Role |
|-------|------|
| `coralreef-core` | Primal lifecycle, CLI, IPC (JSON-RPC + tarpc) |
| `coral-reef` | Shader compiler (frontends, IR, optimizers, backends) |
| `coral-driver` | Userspace GPU dispatch (DRM ioctl, VFIO BAR0/DMA) |
| `coral-gpu` | Unified compile + dispatch API, multi-GPU auto-detect |
| `coral-glowplug` | GPU device broker (VFIO mgmt, health, hot-swap) |
| `coral-ember` | VFIO fd holder + ring-keeper (SCM_RIGHTS, watchdog) |
| `coral-reef-isa` | ISA encoding tables (SM35–SM120, GCN5, RDNA2) |
| `coral-reef-bitview` | Bit-level field access for GPU instruction encoding |
| `coral-reef-stubs` | Pure Rust replacements for Mesa dependencies |
| `nak-ir-proc` | Proc-macro derives for IR types |
| `primal-rpc-client` | JSON-RPC 2.0 HTTP client for inter-primal IPC |

## Key constraints

- **License**: AGPL-3.0-or-later. NAK-derived files retain MIT. scyBorg Provenance Trio.
- **Rust 2024 edition**, MSRV 1.85. No C/C++/Python in production.
- **`clippy::pedantic` + `clippy::nursery`** — zero warnings.
- **`unsafe`** confined to `coral-driver` (kernel ioctl/mmap/MMIO), documented with `// SAFETY:`. All other crates use `#![forbid(unsafe_code)]`.
- **`unsafe_code = "deny"`** at workspace lint level; `coral-driver` opts out.
- **No `.unwrap()` in library code**. `Result<T, E>` + `thiserror`. `.expect()` with reason is acceptable.
- **Max 1000 LOC per file**. Split into cohesive submodules.
- **IPC**: JSON-RPC 2.0 primary, tarpc optional. Semantic method names: `shader.compile.wgsl`, `health.check`, etc.
- **Zero-copy**: `bytes::Bytes` for IPC payloads. Minimize `.clone()`.
- **No hardcoded paths or addresses**: env var overrides with sane defaults.

## IPC capabilities

```
shader.compile.wgsl          shader.compile.spirv
shader.compile.wgsl.multi    shader.compile.status
shader.compile.capabilities  health.check
health.liveness              health.readiness
identity.get                 capability.list
```

## Quick start

```bash
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-features -- -D warnings
cargo fmt --check
```

## Entry points

- **Start here**: `START_HERE.md`
- **Conventions**: `CONVENTIONS.md`
- **Status**: `STATUS.md`
- **Spec**: `specs/CORALREEF_SPECIFICATION.md`
- **Evolution plan**: `specs/SOVEREIGN_MULTI_GPU_EVOLUTION.md`
- **Changelog**: `CHANGELOG.md`
