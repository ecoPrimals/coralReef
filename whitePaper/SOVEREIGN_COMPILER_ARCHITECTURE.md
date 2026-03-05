# Sovereign Compiler Architecture

**Status**: Implemented  
**Date**: March 5, 2026

---

## Vision

A pure-Rust NVIDIA GPU compilation pipeline that operates without
any Mesa C code, enabling the ecoPrimals ecosystem to compile shaders
to native GPU binaries independently.

## Architecture Layers

```
                 WGSL / SPIR-V
                      │
                      ▼
              ┌───────────────┐
              │     naga      │  Parse WGSL/SPIR-V → naga IR
              └───────┬───────┘
                      │
                      ▼
              ┌───────────────┐
              │  coral-reef    │  Translate → lower → optimize → encode
              │               │
              │  from_spirv   │  naga IR → NAK SSA IR
              │  lower_f64    │  DFMA software transcendentals
              │  optimize     │  copy prop, DCE, bar prop, scheduling
              │  legalize     │  arch-specific lowering
              │  alloc_regs   │  register allocation + spilling
              │  encode       │  SM70+ binary emission
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

All Mesa C dependencies have been replaced with pure-Rust implementations:

| Mesa dependency | Replacement | Status |
|----------------|-------------|--------|
| `compiler::cfg` | Pure Rust CFG + dominator tree | Evolved |
| `compiler::bitset` | Pure Rust dense BitSet | Evolved |
| `compiler::smallvec` | Stack-optimized SmallVec (None/One/Many) | Evolved |
| `compiler::as_slice` | Rust trait | Evolved |
| `compiler::dataflow` | Pure Rust worklist solver | Evolved |
| `nvidia_headers` | Pure Rust QMD definitions (Kepler–Blackwell) | Evolved |
| `nak_latencies` | Pure Rust SM100 latency model | Evolved |
| `compiler::nir` | Deleted — replaced by naga frontend | Removed |
| `nak_bindings` | Deleted — legacy FFI stubs removed | Removed |
| `nak_ir_proc` | `coral-reef-proc` (3 derive macros) | Evolved |
| `bitview` | `coral-reef-bitview` | Evolved |

## Integration with barraCuda

```
barraCuda (current):  WGSL → naga → SPIR-V → wgpu → driver → GPU
barraCuda (future):   WGSL → naga → coral-reef → native binary → coralDriver → GPU
```

The coral path eliminates the SPIR-V → driver compiler round-trip,
giving barraCuda direct control over instruction generation and
f64 precision guarantees.

---

*This architecture is implemented. Future work focuses on coralDriver and ecosystem integration.*
