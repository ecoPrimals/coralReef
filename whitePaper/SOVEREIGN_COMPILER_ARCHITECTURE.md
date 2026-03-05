# Sovereign Compiler Architecture

**Status**: Draft  
**Date**: March 4, 2026

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
              │  coral-nak    │  Translate → optimize → lower → encode
              │               │
              │  from_spirv   │  naga IR → NAK IR
              │  optimize     │  copy prop, DCE, scheduling
              │  legalize     │  arch-specific lowering
              │  f64_lower    │  DFMA software transcendentals
              │  alloc_regs   │  register allocation
              │  encode       │  SM70+ binary emission
              └───────┬───────┘
                      │
                      ▼
              ┌───────────────┐
              │  coral-nak-isa│  Instruction encoding tables
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

## Dependency Elimination Roadmap

| Mesa dependency | Replacement | Complexity |
|----------------|-------------|------------|
| `compiler::cfg` | Pure Rust CFG | Low — ~200 LOC |
| `compiler::bitset` | Pure Rust BitSet | Low — ~100 LOC |
| `compiler::smallvec` | `smallvec` crate | Trivial |
| `compiler::as_slice` | Rust trait | Trivial |
| `compiler::dataflow` | Pure Rust dataflow | Medium — ~300 LOC |
| `nak_bindings` | Pure Rust types | High — ~2K LOC of C struct ports |
| `nvidia_headers` | `coral-hw-headers` | High — build-time C header parsing |
| `compiler::nir` | Delete (replace with naga) | N/A |
| `nak_ir_proc` | `coral-nak-proc` | Medium — 3 derive macros |
| `bitview` | Vendor or rewrite | Low — ~500 LOC |

## Integration with barraCuda

```
barraCuda (current):  WGSL → naga → SPIR-V → wgpu → driver → GPU
barraCuda (future):   WGSL → naga → coral-nak → native binary → coralDriver → GPU
```

The coral path eliminates the SPIR-V → driver compiler round-trip,
giving barraCuda direct control over instruction generation and
f64 precision guarantees.

---

*This architecture evolves as implementation progresses.*
