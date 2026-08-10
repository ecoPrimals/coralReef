<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# coralReef — Context

## What You Are

coralReef is the ecosystem's shader compiler. You compile compute kernels
targeting GPU architectures from SM35 (Kepler) through SM120 (Blackwell).
You are **Layer 2** of the sovereign compute stack — DONE for all target archs.

Input languages: WGSL (primary), SPIR-V (binary), GLSL 450 (compute),
PTX (planned). Output: native GPU binaries — NVIDIA SASS and AMD
GCN5/RDNA2–RDNA4. Full f64 transcendental support. Pure Rust. Zero vendor SDK.

## Where You Sit

```
Layer 0: toadStool sysmon       (COMPLETE)
Layer 1: barraCuda math engine  (COMPLETE — your peer)
Layer 2: coralReef compiler     (YOU — DONE, SM35 through SM120)
Layer 3: toadStool dispatch     (PARTIAL — wgpu working, VFIO blocked)
Layer 4: toadStool GPU driver   (3/3 GPUs sovereign, FECS remaining)
```

You coordinate with toadStool (dispatch) and barraCuda (math) — the
compute trio. External deps: naga (WGSL parser — your evolution target
to replace).

## Ecosystem Position

coralReef is one primal in the **ecoPrimals** sovereign compute
ecosystem. Primals are standalone Rust binaries that communicate via
JSON-RPC 2.0 and tarpc. They discover each other by capability at
runtime — no hardcoded primal names, no shared code imports.

- Pull wateringHole: `membrane temporal.cascade`
- Your gate: ironGate (RTX 5070, GPU compute node)
- Ecosystem standards live in `ecoPrimals/infra/wateringHole/`

## Project status (Sprint 14)

- **Tests**: 3,814 workspace tests (3,810 passed, 4 ignored). Zero clippy warnings. Zero unsafe.
- **Sprint 14 (current)**: Wave 157d — Deep debt evolution: PLop3 module split, SM80 hazard table split, BEARDOG_SOCKET deprecation, 31 new AMD ops encoder tests, GEMM Phase 2, coverage push (sm70 float/float64), gemm.rs directory module split. Previous waves: 157a — G68 16/16, self-audit 18/18 RPC, Node Atomic AAR. 156s — G66 transport abstraction. 156j — C2 dual-socket. 156g — alloc elimination. 155j — NUCLEUS ACHIEVED on strandGate.
- **Sprint 9**: Diesel engine excision. coral-ember/coral-glowplug/coral-driver/coral-gpu removed (153K lines). Pure compiler primal. Hardware dispatch delegated to toadStool.
- **Sprint 8**: Feature freeze + toadStool handoff (E1/E2/E3 documented).
- **Sprint 7**: FECS/GPCCS cold-silicon stability proof — `boot_gr_falcons_with_recovery()` retries up to 3× with PMC GR reset, structured `GrBootOutcome` enum.
- **Sprint 6**: toadStool Phase C COMPLETE. Phase D markers. FECS error hardening (`falcon_boot()` returns Err on timeout/halt).
- **Sprint 5**: Pass 12 sentinel gaps — `naga::Module` direct ingest API, compile deadline, FECS cold init.
- **Debt**: Zero across all categories — no `Result<_, String>`, no `.unwrap()` in library code, no `eprintln!` in production library, no `async_trait`/`lazy_static`. `deny.toml` enforced (ecoBin v3 C/FFI bans). All production files under 1000 LOC. All bare `unreachable!()` → `ice!()`.
- **Compute Trio**: coralReef = HOW (compiler). Wire contract frozen. toadStool owns dispatch.
- **BTSP Phase 3**: Complete (ChaCha20-Poly1305 AEAD encrypted transport).
- **JH-0 MethodGate**: Pre-dispatch capability authorization live.
- **Diesel stack excised** (Sprint 9): `coral-ember`, `coral-glowplug`, `coral-driver`, `coral-gpu` removed. Hardware lifecycle fully delegated to toadStool. coralReef is now a pure compiler primal — zero unsafe, zero hardware ioctl.

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
| `coral-reef-isa` | ISA encoding tables (SM35–SM120, GCN5, RDNA2) |
| `coral-reef-bitview` | Bit-level field access for GPU instruction encoding |
| `coral-reef-stubs` | Pure Rust replacements for Mesa dependencies |
| `nak-ir-proc` | Proc-macro derives for IR types |
| `primal-rpc-client` | JSON-RPC 2.0 HTTP client for inter-primal IPC |

## Key constraints

- **License**: AGPL-3.0-or-later. NAK-derived files retain MIT. scyBorg Provenance Trio.
- **Rust 2024 edition**, MSRV 1.85. No C/C++/Python in production.
- **`clippy::pedantic` + `clippy::nursery`** — zero warnings.
- **`#![forbid(unsafe_code)]`** on all crates. Zero unsafe in the entire workspace.
- **`unsafe_code = "deny"`** at workspace lint level. No opt-outs (diesel stack excised Sprint 9).
- **No `.unwrap()` in library code**. `Result<T, E>` + `thiserror`. `.expect()` with reason is acceptable.
- **Max 1000 LOC per file**. Split into cohesive submodules.
- **IPC**: JSON-RPC 2.0 primary, tarpc optional. Semantic method names: `shader.compile.wgsl`, `health.check`, etc.
- **Zero-copy**: `bytes::Bytes` for IPC payloads. Minimize `.clone()`.
- **No hardcoded paths or addresses**: env var overrides with sane defaults.

## IPC Methods (18 served, 5 consumed)

### Served (this primal handles)

```
shader.compile.spirv         shader.compile.wgsl
shader.compile.status        shader.compile.capabilities
shader.compile.wgsl.multi    shader.compile.multi
shader.compile.gemm
health.check                 health.liveness
health.readiness             health.version
identity.get                 capability.list
capabilities.list            btsp.negotiate
auth.check                   auth.mode
auth.peer_info
```

### Consumed (called on peer primals)

```
compute.dispatch             capability.register
ipc.heartbeat                primal.announce
crypto.sign
```

## Resolved Gaps (historical)

**GAP-HS-124 — SPIR-V output**: RESOLVED (Wave 68). `wgsl_to_spirv()` emits valid
SPIR-V binary via `naga::back::spv::write_vec()`.

**GAP-HS-115 — SM120 membar.sys**: RESOLVED (Wave 68/126). Missing `membar.sys`
before `ret;`/`exit;` in PTX emitter for SM120+ targets — fix deployed.

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
- **Evolution plan**: `docs/archive/SOVEREIGN_MULTI_GPU_EVOLUTION.md` (historical — pre-excision driver narrative)
- **Changelog**: `CHANGELOG.md`
