<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# coralReef — Stadial Readiness

**Date**: May 17, 2026
**Version**: 0.2.0
**Gate**: Interstadial exit CLEARED — stadial transition ready
**Reference**: primalSpring Wave 22 audit

---

## Universal Standards Checklist

### Runtime

- [x] Health triad: `health.liveness`, `health.readiness`, `health.check`
- [x] UDS socket at `$XDG_RUNTIME_DIR/biomeos/coralreef.sock`
- [x] TCP fallback respects `ports.env` assignment
- [x] `server` subcommand with `--port` for JSON-RPC
- [x] Standalone startup without `FAMILY_ID`/`NODE_ID`

### Discovery

- [x] `capability.list` returns `{ "capabilities": [...], "count": N, "primal": "coralreef" }`
- [x] `identity.get` returns canonical identity response
- [x] `capability.register` client-side self-registration (calls ecosystem registry)
- [x] All methods follow `{domain}.{operation}[.{variant}]` naming

### Security

- [x] BTSP handshake mandatory when `FAMILY_ID` is set (non-"default")
- [x] ChaCha20-Poly1305 + HKDF with `btsp-v1`
- [x] `FAMILY_ID` + `BIOMEOS_INSECURE=1` = refuse to start
- [x] `btsp.capabilities` registered in capability response
- [x] Zero metadata leakage (stripped binary, no path/hostname/username)
- [x] UDS-first default (TCP off unless explicitly enabled)
- [x] `deny.toml` bans `ring`, `openssl`, `aws-lc-sys` (and 13 more C FFI crates)

### Build / plasmidBin

- [x] `genomebin/manifest.toml` version `0.2.0` matches workspace Cargo.toml
- [x] `seed_fingerprint` BLAKE3 hash present and correct
- [x] `edition = "2024"` in workspace Cargo.toml
- [x] `build_from_source = true` — intentional: WGSL compilation requires naga
      source compilation; no pre-built naga binaries exist for distribution

### Documentation

- [x] README.md version matches manifest (Sprint 12 / 0.2.0)
- [x] CHANGELOG.md documents recent evolution (Sprint 12 RayQuery)
- [x] CONTEXT.md current status, known gaps

### Composition Readiness

- [x] Stability tiers annotated (see below)
- [x] Degradation behavior documented (see below)
- [x] Downstream pairing documented (see below)

---

## Method Registry — 16 Provided Methods

### Stability Tiers

| Method | Tier | Notes |
|--------|------|-------|
| `shader.compile.wgsl` | **Stable** | Primary compilation entry point. Wire contract frozen. |
| `shader.compile.spirv` | **Stable** | SPIR-V binary input. Same wire contract. |
| `shader.compile.wgsl.multi` | **Stable** | Multi-device cross-vendor compilation. |
| `shader.compile.gemm` | **Stable** | Tensor-core GEMM codegen (SM80+ HMMA). |
| `shader.compile.status` | **Stable** | Name, version, supported architectures. |
| `shader.compile.capabilities` | **Stable** | Dynamic arch enumeration, FMA policies. |
| `health.check` | **Stable** | Full health report (name, version, archs, status). |
| `health.liveness` | **Stable** | Alive probe. |
| `health.readiness` | **Stable** | Ready-to-serve probe. |
| `health.version` | **Stable** | Build identity (session, hash, version, name). |
| `identity.get` | **Stable** | Canonical identity response per Capability Wire Standard. |
| `capability.list` | **Stable** | Self-advertisement envelope. Alias: `capabilities.list`. |
| `btsp.negotiate` | **Stable** | BTSP Phase 3 cipher negotiation. |
| `auth.check` | **Stable** | Authentication status probe. |
| `auth.mode` | **Stable** | Current auth enforcement mode. |
| `auth.peer_info` | **Stable** | UDS peer credential info (uid, pid). |

### Consumed Capabilities

| Method | Provider | Purpose |
|--------|----------|---------|
| `compute.dispatch` | toadStool | GPU hardware dispatch (compiled shader → execution) |
| `capability.register` | Ecosystem registry (Songbird) | Self-registration on startup |
| `ipc.heartbeat` | Ecosystem registry (Songbird) | 45-second keepalive |

---

## Degradation Behavior

### When coralReef is down

**Impact**: Shader compilation unavailable. No new GPU compute kernels can be
compiled from WGSL/SPIR-V/GLSL source.

**Affected primals**:
- **barraCuda**: Cannot compile new WGSL shaders. Falls back to pre-compiled
  kernel cache (`KernelCacheEntry`). Cached GEMM kernels continue working.
- **toadStool**: Cannot request new shader compilations for dispatch. Existing
  compiled binaries in the dispatch pipeline continue executing.
- **hotSpring**: Cannot compile validation corpus shaders. Does not affect
  running experiments — only new compilation requests.

**Degradation mode**: Graceful. All downstream primals that consume compiled
shader binaries continue operating with their cache. Only the production of
**new** compiled binaries is blocked. No data loss, no state corruption.

**Recovery**: Restart coralReef. No state to recover — coralReef is stateless
(pure compiler). First IPC request after restart triggers `capability.register`
and `ipc.heartbeat` to re-join the ecosystem graph.

### When upstream is down

**toadStool down**: coralReef continues compiling shaders. Compiled binaries
accumulate in client-side caches. No functional impact on compilation.
Dispatch of compiled binaries is toadStool's domain — its outage does not
affect coralReef.

**Songbird down**: `capability.register` fails silently (debug log). coralReef
continues serving compilation requests on its existing socket. Ecosystem
discovery of coralReef is degraded until Songbird recovers and coralReef's
next heartbeat re-registers.

---

## Downstream Pairing (Stadial)

| Partner | Relationship | Validation |
|---------|-------------|-----------|
| **barraCuda** | Compilation target — coralReef compiles WGSL/GEMM, barraCuda executes | `shader.compile.wgsl` + `shader.compile.gemm` wire contract validated |
| **hotSpring** | VFIO dispatch — sovereign GPU pipeline (coralReef compiles, toadStool dispatches via hotSpring validation) | 93/93 cross-spring WGSL corpus compiles |
| **toadStool** | Hardware dispatch — coralReef compiles, toadStool dispatches compiled binaries to GPU silicon | Compute Trio wire contract frozen |

---

## `build_from_source` Justification

`build_from_source = true` in the ecosystem manifest is **intentional**:

1. coralReef depends on `naga` 28.0.0 (shader IR library) which must be
   compiled from source — no pre-built binaries exist in any distribution
2. The `amd-isa-gen` tool (AMD ISA table generator) is a build-time Rust
   binary that generates encoding tables from XML
3. `nak-ir-proc` is a proc-macro crate that generates code at compile time

All three require source compilation. The `musl-static` build works correctly:
`cargo build --target x86_64-unknown-linux-musl` produces a fully static binary.

---

## Method Namespace Review

The audit asked whether the `shader.*` namespace (11 methods → 16 total) needs
expansion for stadial. Assessment:

**Current coverage is appropriate.** coralReef is a compiler, not an IDE — the
compilation surface (`compile`, `status`, `capabilities`, `gemm`) plus the
standard infra methods (`health.*`, `identity.*`, `capability.*`, `btsp.*`,
`auth.*`) fully covers the stadial validation surface.

**Considered and deferred:**
- `shader.validate` — validate WGSL/SPIR-V without compiling. Useful but not
  stadial-blocking. naga validation is already available via `compile` with
  `validate: true` in `CompileOptions`. A dedicated method could be added
  post-stadial if downstream demand materializes.
- `shader.optimize` — not meaningful as a standalone method. Optimization is
  an integral pipeline stage, not a separable service.
