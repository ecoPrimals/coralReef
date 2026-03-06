# Sovereign Compiler Architecture

**Status**: Implemented (NVIDIA + AMD), Sovereign Pipeline Complete
**Date**: March 6, 2026

---

## Vision

A pure-Rust GPU compilation pipeline with vendor-agnostic architecture,
enabling the ecoPrimals ecosystem to compile shaders to native GPU
binaries independently — zero C dependencies, zero vendor lock-in.

The Rust language and compilation model is the competitive advantage.
Ownership eliminates GPU memory bugs at compile time. Exhaustive
`match` catches missing ISA encodings as compiler errors. Traits make
vendor backends pluggable without runtime overhead. Every layer from
shader source to GPU silicon becomes internal Rust.

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
              ┌───────────────────────────────────┐
              │  codegen (shared)                  │
              │                                   │
              │  naga_translate  naga → SSA IR    │
              │  lower_f64      DFMA / native f64 │
              │  optimize       copy prop, DCE... │
              └───────┬───────────────────────────┘
                      │
             ┌────────┴────────┐
             ▼                 ▼
      ┌────────────┐   ┌────────────┐
      │  nv/        │   │  amd/       │
      │  legalize   │   │  legalize   │
      │  assign_regs│   │  assign_regs│
      │  sm70_encode│   │  gfx10_encode│
      │  SPH header │   │  ELF emit   │
      └──────┬─────┘   └──────┬─────┘
             │                │
             ▼                ▼
       NVIDIA SASS       AMD GFX binary
             │                │
             ▼                ▼
      ┌───────────────────────────────┐
      │  coralDriver                   │
      │  ├ NvDevice   nouveau DRM     │
      │  ├ AmdDevice  amdgpu DRM     │
      │  └ ComputeDevice trait        │
      ├───────────────────────────────┤
      │  coralMem    GPU memory       │
      │  coralQueue  Command submit   │
      └───────────────────────────────┘
```

## Dependency Elimination — Complete (Compiler Layer)

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

## AMD Backend (Complete)

AMD backend targets the on-site RX 6950 XT (RDNA2, GFX1030, Navi 21).
The fully open amdgpu/RADV stack makes this the cleanest path to
sovereign compute.

AMD ISA sources:
- Machine-readable ISA XML specs from GPUOpen (RDNA2/3/4, CDNA1-4)
- RDNA2 Instruction Set Architecture Reference Guide (public PDF)
- Mesa ACO (MIT, C++) as behavioral reference — never imported

Key architectural differences from NVIDIA:
- **Wave32/64** dual-mode (NVIDIA is always warp=32)
- **VGPR/SGPR split** (vector vs scalar register files)
- **Exec mask** for predication (NVIDIA uses per-thread predicates)
- **No SPH** — AMD uses ELF-like binary format
- **Native f64** — `v_fma_f64`, `v_sqrt_f64`, `v_rcp_f64` (simpler than MUFU+Newton)
- **Simpler encoding** — mostly fixed-width (vs NVIDIA variable-width SASS)

## Integration with barraCuda

```
barraCuda (current):
  WGSL → naga → SPIR-V → wgpu → Vulkan → GPU

barraCuda (compiler sovereignty — implemented):
  WGSL → naga → coralReef → native binary

barraCuda (dispatch sovereignty — implemented):
  WGSL → naga → coralReef → native binary → coralDriver → GPU
  (no wgpu, no Vulkan, no Mesa)

barraCuda (full sovereignty — implemented):
  WGSL → coralReef → native binary → coralDriver → GPU
  (single Rust binary, zero external C at any layer)
```

## Evolution Policy

FFI is scaffolding. Each pass produces strictly better Rust:

| Pass | Character | Acceptable | Replaced by |
|------|-----------|------------|-------------|
| 1 | Scaffold | FFI wrappers, raw ioctl | Safe Rust wrappers |
| 2 | Structure | Safe wrappers around FFI | Internal Rust types |
| 3 | Internalize | Pure Rust replacements | — |
| 4 | Optimize | Zero-copy, const verification | — |
| 5 | Sovereign | 100% internal Rust | — |

No FFI survives to production release.

---

*NVIDIA + AMD compilers are production. coralDriver + coralGpu implemented.
Zero FFI. Each pass produced better Rust. The language is the advantage.*
