+++
title = "coralReef Validation Summary"
description = "Sovereign Rust GPU shader compiler — 3689 tests, WGSL/SPIR-V/GLSL to native GPU binary (NVIDIA SM35-SM120, AMD RDNA2-4), G65 protocol negotiation, C3 health shim verified, capability abstraction, PTX macro modernization, BTSP client handshake, zero unsafe, zero C deps"
date = 2026-08-06

[taxonomies]
primals = ["coralreef"]
springs = []
+++

## Status

- **3686 tests** (3680 passing, 6 ignored — hardware-gated)
- **Version**: 0.2.0 — Sprint 14 / Wave 156p, 3,686 tests
- **Grade**: A++ (Multi-Vendor Sovereign GPU Compiler — Stadial Ready)
- **License**: AGPL-3.0-or-later
- **Binary**: `coralreef` (single UniBin, clap subcommands)
- **Zero unsafe**, zero C dependencies, zero `*-sys` crates, `#![forbid(unsafe_code)]` on all crates
- **BTSP Phase 3** — authenticated IPC (ChaCha20-Poly1305 + HKDF)
- **Stale socket detection** — connect-probe discovery, PID file liveness

## Key Capabilities (18 served, 5 consumed)

### Compilation

| Method | Description |
|--------|-------------|
| `shader.compile.wgsl` | WGSL → native GPU binary (NVIDIA/AMD) |
| `shader.compile.spirv` | SPIR-V → native GPU binary |
| `shader.compile.wgsl.multi` | Same WGSL → multiple GPU targets in one call |
| `shader.compile.multi` | Mixed-input (WGSL/SPIR-V/GLSL) batch compilation |
| `shader.compile.gemm` | Tensor-core GEMM kernel (SM80+ mma.sync HMMA) |

### Health & Identity

| Method | Description |
|--------|-------------|
| `shader.compile.status` | Compiler health + supported architectures |
| `shader.compile.capabilities` | Architecture list + f64 transcendental availability |
| `health.check` / `health.liveness` / `health.readiness` / `health.version` | Standard health triad |
| `identity.get` | Primal self-description for discovery |
| `capability.list` | Wire Standard L3 capability advertisement |

### Security

| Method | Description |
|--------|-------------|
| `btsp.negotiate` | BTSP Phase 3 handshake |
| `auth.check` / `auth.mode` / `auth.peer_info` | Authenticated session queries |

## GPU Target Coverage

| Vendor | Architectures |
|--------|--------------|
| NVIDIA | SM35, SM70, SM75, SM80, SM86, SM89, SM120 |
| AMD | GCN5, RDNA2, RDNA3, RDNA4 |

## Compilation Pipeline

```
WGSL/SPIR-V/GLSL → naga → IR → f64 lower → optimize → legalize → RA → encode → native binary
```

- **f64 transcendentals**: sqrt, rcp, exp2, log2, sin, cos, exp, log, pow (Newton-Raphson / native)
- **Tensor-core GEMM**: HMMA via mma.sync (F16, F16F32, TF32) — SM80+ only
- **RayQuery PTX**: Returns `CompileError::NotImplemented` (vendor ISA undocumented)
- **FMA control**: `FmaPolicy` enum (AllowFusion / NoContraction)
- **Precision routing**: `dispatch_hints` with `hardware_hint` (compute/tensor_core/rt_core)

## IPC Transports

| Transport | Protocol | Socket |
|-----------|----------|--------|
| JSON-RPC 2.0 | Newline-delimited | Unix socket (primary), TCP (fallback) |
| tarpc | Bincode | Unix socket, TCP |

## Downstream Consumers

Consumers discover coralReef at runtime via `compute.dispatch` capability registration.
Compiler is consumer-agnostic — any primal requesting `shader.compile.*` is served.

## Test Categories

- PTX emitter: SM120 atomics, barriers, subgroups, scans, texture sampling, texture gather, image atomics, workgroup uniform load, RayQuery, function inlining, loop control flow, multi-dim subgroup builtins
- HMMA GEMM: tile computation, precision modes, boundary checks
- IPC: tarpc Unix roundtrip, JSON-RPC chaos/fault, BTSP Phase 3 AEAD crypto
- Compute Trio: wire contract serde, multi-device, dispatch hints
- f64 transcendentals: software lowering correctness across all operations
- AMD pipeline: legalization, register allocation, encoding

## Sovereignty

| Property | Status |
|----------|--------|
| `#![forbid(unsafe_code)]` | All crates |
| C/FFI dependencies | Zero |
| `*-sys` crates | Zero |
| `ring` / OpenSSL | Eliminated |
| `libc` direct | Eliminated |
| Build from source | Yes (pure Rust, no vendor SDKs) |
| deny.toml | ecoBin v3 C/FFI bans |

## See Also

- [Shader Compile Wire Contract](../docs/SHADER_COMPILE_WIRE_CONTRACT.md)
- [coralReef Specification](../specs/CORALREEF_SPECIFICATION.md)
- [Stadial Readiness (archived)](../docs/archive/STADIAL_READINESS_WAVE22_MAY17_2026.md)
