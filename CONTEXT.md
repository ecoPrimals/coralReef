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

## Project status (Iteration 101+, Sprint 8)

- **Tests**: 4790 workspace tests, 0 failed, 181 ignored hardware-gated (see `STATUS.md` / `CHANGELOG.md`).
- **Sprint 8**: Diesel engine migration feature freeze. coral-ember/coral-glowplug/coral-driver hardware runtime feature-frozen. E1 (cylinder subprocess pattern) and E2 (warm handoff API) documented as toadStool reference. E3 (FECS cold silicon init) shipped (Sprint 7). Handoff created for toadStool C1-C7 cutover.
- **Sprint 7**: FECS/GPCCS cold-silicon stability proof — `boot_gr_falcons_with_recovery()` retries up to 3× with PMC GR reset, structured `GrBootOutcome` enum.
- **Sprint 6**: toadStool Phase C COMPLETE. Phase D markers. FECS error hardening (`falcon_boot()` returns Err on timeout/halt).
- **Sprint 5**: Pass 12 sentinel gaps — `naga::Module` direct ingest API, compile deadline, FECS cold init.
- **Debt**: Zero across all categories — no `Result<_, String>`, no `.unwrap()` in library code, no `eprintln!` in production library, no `async_trait`/`lazy_static`, all `#[expect(dead_code)]` verified. `deny.toml` enforced (ecoBin v3 C/FFI bans). `mem::zeroed()` eliminated for ioctl structs. Smart refactoring: all production files under 1000 LOC limit. All bare `unreachable!()` evolved to `ice!()`.
- **Compute Trio**: coralReef = HOW (compiler). Wire contract frozen. Phase D in progress (toadStool cutover). QMD split boundary documented.
- **BTSP Phase 3**: Complete (ChaCha20-Poly1305 AEAD encrypted transport).
- **JH-0 MethodGate**: Pre-dispatch capability authorization live.
- **Diesel stack (feature-frozen)**: `coral-ember`, `coral-glowplug`, `coral-driver` hardware runtime — no new features. toadStool Phase C implements equivalents (C1-C7). Removal gated on hardware validation (Titan V / K80 / RTX 5060).

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
capability.register          ipc.heartbeat
btsp.negotiate               auth.check
auth.mode                    auth.peer_info
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
