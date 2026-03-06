# Sovereign Compiler Architecture

**Status**: Implemented  
**Date**: March 5, 2026

---

## Vision

A pure-Rust GPU compilation pipeline with vendor-agnostic architecture,
enabling the ecoPrimals ecosystem to compile shaders to native GPU
binaries independently — zero C dependencies, zero vendor lock-in.

## Architecture Layers

```
                 WGSL / SPIR-V
                      │
                      ▼
              ┌───────────────┐
              │  Frontend      │  Parse WGSL/SPIR-V → naga IR
              │  (pluggable)  │  NagaFrontend is the default
              └───────┬───────┘
                      │
                      ▼
              ┌───────────────┐
              │  codegen       │  Translate → lower → optimize → encode
              │               │
              │  naga_translate│  naga IR → codegen SSA IR
              │  lower_f64    │  DFMA software transcendentals
              │  optimize     │  copy prop, DCE, bar prop, scheduling
              │  legalize     │  target-specific lowering
              │  assign_regs  │  register allocation + spilling
              │  nv/encode    │  NVIDIA binary emission
              └───────┬───────┘
                      │
                      ▼
              ┌───────────────┐
              │  Backend       │  Vendor-specific encoding
              │  (pluggable)  │  NvidiaBackend, AmdBackend, IntelBackend
              └───────┬───────┘
                      │
                      ▼
              ┌───────────────┐
              │  coral-reef-isa│  Instruction encoding tables
              │               │  SPH / QMD generation
              └───────┬───────┘
                      │
                      ▼
              Native GPU binary
                      │
                      ▼
              ┌───────────────┐
              │  coralDriver  │  (future) Userspace GPU driver
              │  coralMem     │  (future) GPU memory management
              │  coralQueue   │  (future) Command submission
              └───────────────┘
```

## Dependency Elimination — Complete

All upstream C dependencies have been replaced with pure-Rust implementations:

| Upstream dependency | Replacement | Status |
|---------------------|-------------|--------|
| `compiler::cfg` | Pure Rust CFG + dominator tree | Evolved |
| `compiler::bitset` | Pure Rust dense BitSet | Evolved |
| `compiler::smallvec` | Stack-optimized SmallVec (None/One/Many) | Evolved |
| `compiler::as_slice` | Rust trait | Evolved |
| `compiler::dataflow` | Pure Rust worklist solver | Evolved |
| `nvidia_headers` | Pure Rust QMD definitions (Kepler–Blackwell) | Evolved |
| `nak_latencies` | Pure Rust SM100 latency model | Evolved |
| `compiler::nir` | Deleted — replaced by naga frontend | Removed |
| `nak_bindings` | Deleted — legacy FFI stubs removed | Removed |
| `nak_ir_proc` | `nak-ir-proc` crate (4 derive macros) | Evolved |
| `bitview` | `coral-reef-bitview` | Evolved |
| `rustc-hash` | Internalized as `fxhash` module | Evolved |

## Integration with barraCuda

```
barraCuda (current):  WGSL → naga → SPIR-V → wgpu → driver → GPU
barraCuda (future):   WGSL → naga → coral-reef → native binary → coralDriver → GPU
```

The coral path eliminates the SPIR-V → driver compiler round-trip,
giving barraCuda direct control over instruction generation and
f64 precision guarantees.

---

*This architecture is implemented. Future work focuses on multi-vendor
backends, coralDriver, and ecosystem integration.*
