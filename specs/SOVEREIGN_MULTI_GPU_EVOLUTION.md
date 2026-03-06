# Sovereign Multi-GPU Evolution — Pure Rust Pipeline

**Version**: 0.1.0
**Date**: March 6, 2026
**Status**: Complete — Phases 6–9 implemented (March 6, 2026)
**Hardware**: NVIDIA RTX 3090 (GA102/SM86) + AMD RX 6950 XT (Navi 21/RDNA2/GFX1030)

---

## Thesis

The Rust language and its compilation model is our greatest advantage.
Every FFI binding is a bandaid — acceptable temporarily, replaced
eventually. Each evolution pass produces better Rust. By the end of
primal evolution, every layer from shader source to GPU hardware
interaction is internal Rust: owned, audited, sovereign.

External C/C++ references (Mesa ACO, NVK, nouveau, amdgpu userspace)
are scaffolding. We study them, learn the hardware interface, and
build our own. The scaffold comes down when the structure stands.

---

## Hardware Present

```
PCI 25:00.0 — AMD Radeon RX 6950 XT (Navi 21)
  Kernel:   amdgpu (open-source)
  Vulkan:   RADV (Mesa 25.1.5, ACO compiler)
  DRM:      /dev/dri/card0, /dev/dri/renderD128
  ISA:      GFX1030 (RDNA2)
  f64:      1/16 rate (v_fma_f64 native)
  VRAM:     16 GB GDDR6
  Status:   Fully open stack — best sovereign candidate

PCI 41:00.0 — NVIDIA GeForce RTX 3090 (GA102)
  Kernel:   nvidia (proprietary, 580.119.02)
  Vulkan:   NVIDIA proprietary (Vulkan 1.4.312)
  DRM:      /dev/dri/card1, /dev/dri/renderD129
  ISA:      SM86 (Ampere)
  f64:      1/32 rate (DFMA native, DF64 preferred)
  VRAM:     24 GB GDDR6X
  Status:   Proprietary driver — sovereign path requires nouveau
```

Both GPUs verified accessible via Vulkan (`vulkaninfo`) and wgpu
(barraCuda `doctor`). The AMD card is the primary sovereign evolution
target because its entire stack is already open-source.

---

## Evolution Philosophy

### FFI Is a Bandaid

FFI (C bindings via `*-sys` crates, `bindgen`, raw ioctl structs) is
acceptable in early passes as scaffolding to learn hardware interfaces.
But FFI is technical debt:

- It breaks Rust's safety guarantees at the boundary
- It couples us to C ABI stability
- It prevents cross-compilation without C toolchains
- It violates the ecoBin "100% Pure Rust" standard

Every FFI introduction comes with a tracking ticket for its Rust
replacement. No FFI survives to production release.

### Evolution Passes

Each pass produces strictly better Rust. The codebase never regresses.

| Pass | Character | Acceptable | Not acceptable |
|------|-----------|------------|----------------|
| Pass 1 | Scaffold | FFI wrappers, `unsafe` ioctl calls, raw pointers | Untracked FFI, unlabeled unsafe |
| Pass 2 | Structure | Safe Rust wrappers around FFI, typed interfaces | Raw pointers in public API |
| Pass 3 | Internalize | Pure Rust replacements for FFI modules | Any remaining `extern "C"` |
| Pass 4 | Optimize | Zero-copy, const generics, compile-time verification | Performance regressions |
| Pass 5 | Sovereign | 100% internal Rust, no external C/C++ at any layer | Any FFI, any `*-sys` crate |

### Lean Into the Language

Rust's type system, ownership model, and compilation guarantees are
not constraints — they are the competitive advantage:

- **Ownership** eliminates GPU memory leaks and double-frees at compile time
- **Enums** model ISA instruction formats exhaustively — missing an encoding is a compiler error
- **Traits** make vendor backends pluggable without dynamic dispatch overhead
- **Const generics** enable compile-time ISA table verification
- **Proc macros** generate encoding/decoding boilerplate from ISA specifications
- **No data races** in concurrent multi-GPU dispatch — the borrow checker enforces it
- **`#[repr(C)]` + `bytemuck`** gives zero-copy GPU buffer access without `unsafe`
- **Algebraic types** model register files, instruction operands, and memory layouts exactly

C/C++ GPU drivers fight these problems at runtime with assertions,
sanitizers, and prayer. We solve them at compile time.

---

## Layer Architecture

```
Layer 7   barraCuda           Rust    WE OWN     Math, shaders, precision strategy
Layer 6   coralReef           Rust    WE OWN     Shader compiler (NVIDIA + AMD backends)
Layer 5   naga                Rust    Mozilla    WGSL/SPIR-V frontend
Layer 4   coralDriver         Rust    WE BUILD   Userspace GPU driver (DRM ioctl)
Layer 3   coralMem            Rust    WE BUILD   GPU memory management
Layer 2   coralQueue          Rust    WE BUILD   Command buffer + submission
Layer 1   DRM kernel          C       Linux      Kernel memory/DMA (amdgpu, nouveau)
Layer 0   GPU silicon          —      AMD/NVIDIA  Hardware
```

Layers 7, 6, 5 exist today. Layers 4, 3, 2 are the evolution target.
Layer 1 is the kernel boundary — we accept it (DRM is stable ABI).
Layer 0 is hardware — we accept it.

---

## AMD Evolution (Primary Path)

The RX 6950 XT on the fully open amdgpu/RADV stack is the cleanest
path to sovereign compute. Every layer above the kernel is replaceable.

### Pass 1 — ISA Foundation (weeks)

**Goal**: Parse AMD's machine-readable ISA specs, generate Rust
encoding tables, build the GFX1030 instruction assembler.

| Task | Scaffolds From | Output |
|------|---------------|--------|
| Download AMD ISA XML specs (RDNA2) | GPUOpen `isa_spec_manager` | `specs/amd/rdna2.xml` |
| Build XML → Rust codegen tool | AMD spec schema docs | `tools/isa_gen/` (build-time) |
| Generate RDNA2 instruction encoding tables | XML specs | `codegen/amd/isa_tables.rs` |
| Implement GFX1030 instruction assembler | Generated tables + ACO reference | `codegen/amd/encode.rs` |
| Wire `AmdBackend` into `backend_for()` | Existing `Backend` trait | `backend.rs` |

**FFI at this pass**: None. ISA encoding is pure data transformation.

**Rust advantage**: Exhaustive `match` on instruction enums catches
missing encodings at compile time. ACO discovers missing encodings
at runtime (GPU hang or silent corruption).

### Pass 2 — AMD Legalization + Register Allocation (weeks)

**Goal**: Lower vendor-agnostic IR to AMD-specific instructions,
allocate VGPR/SGPR registers.

| Task | Scaffolds From | Output |
|------|---------------|--------|
| AMD register file model (VGPR/SGPR/VCC) | ACO `aco_ir.h` reference | `codegen/amd/reg_file.rs` |
| AMD-specific legalization pass | ACO `aco_lower_to_hw_instr.cpp` | `codegen/amd/legalize.rs` |
| Wave32/64 scheduling model | ACO scheduler heuristics | `codegen/amd/scheduling.rs` |
| VGPR/SGPR register allocator | coralReef RA (adapted) | `codegen/amd/assign_regs.rs` |
| Exec mask handling | ACO exec mask lowering | `codegen/amd/exec_mask.rs` |

**FFI at this pass**: None. All compiler logic is pure Rust.

**Rust advantage**: The register allocator uses Rust's type system to
enforce VGPR/SGPR constraints at compile time. ACO uses runtime
assertions (`assert(reg.type() == RegType::vgpr)`).

### Pass 3 — AMD f64 Lowering (days)

**Goal**: f64 support for RDNA2.

| Task | Scaffolds From | Output |
|------|---------------|--------|
| RDNA2 f64 lowering | coralReef NVIDIA f64 (adapted) | `codegen/amd/lower_f64.rs` |
| `v_fma_f64` native emission | AMD ISA docs | Direct — no MUFU workaround needed |
| `v_sqrt_f64` native emission | AMD ISA docs | Simpler than NVIDIA (native instruction) |
| DF64 (f32-pair) for RDNA2 | barraCuda DF64 strategy | `Fp64Strategy::Hybrid` for 1/16 rate |

**RDNA2 advantage**: AMD has native `v_fma_f64`, `v_sqrt_f64`,
`v_rcp_f64` instructions. No MUFU seed + Newton-Raphson workaround
needed. f64 lowering is simpler than NVIDIA.

### Pass 4 — End-to-End Compilation Validation (days)

**Goal**: Compile WGSL → AMD GFX1030 binary, validate against RADV/ACO.

| Task | Output |
|------|--------|
| Compile test shaders to GFX1030 binary | Binary comparison framework |
| Disassemble with AMD tools (if available) | Correctness validation |
| Compare register allocation vs ACO | Performance analysis |
| Round-trip test: coralReef binary vs RADV binary | Equivalence checking |
| Wire into barraCuda's `CoralReef` compilation path | Integration test |

---

## NVIDIA Evolution (Secondary Path)

The RTX 3090 stays on the proprietary driver for now. The sovereign
NVIDIA path requires nouveau, which has known compute instability.
NVIDIA sovereign evolution follows AMD's trail once the architecture
is proven.

### coralDriver for NVIDIA (future)

| Task | Scaffolds From | Status |
|------|---------------|--------|
| nouveau DRM ioctl interface | NVK `nvk_cmd_dispatch.c` | Not started |
| QMD construction (SM86 Ampere) | coralReef `nvidia_headers` (QMD v3.0) | QMD structs exist |
| Pushbuf command buffer | nouveau `nouveau_pushbuf.c` | Not started |
| Memory management (GEM) | NVK + nouveau | Not started |
| Fence/sync | nouveau fence API | Not started |

**Blocker**: nouveau compute dispatch is unstable (known system freezes
on Volta). The AMD path validates the architecture first, then NVIDIA
follows.

---

## coralDriver Architecture

coralDriver is the userspace GPU driver — the bridge between compiled
shader binaries and GPU hardware execution.

### Design Principles

1. **Trait-based dispatch**: `ComputeDevice` trait with `AmdDevice` and `NvidiaDevice` impls
2. **Zero-copy buffers**: `bytemuck` + `mmap` for GPU buffer access
3. **Async-ready**: `tokio` integration for non-blocking GPU operations
4. **Capability discovery**: Runtime detection of GPU features (f64 rate, VRAM, wave size)

### Target API

```rust
pub trait ComputeDevice {
    fn alloc(&self, size: usize, placement: Placement) -> Result<Buffer>;
    fn upload(&self, buf: &Buffer, data: &[u8]) -> Result<()>;
    fn dispatch(&self, binary: &CompiledBinary, buffers: &[Buffer],
                workgroups: [u32; 3]) -> Result<Fence>;
    fn sync(&self, fence: &Fence) -> Result<()>;
    fn readback(&self, buf: &Buffer) -> Result<Vec<u8>>;
}
```

### AMD Implementation (Primary)

```
CompiledBinary (GFX1030)
       │
       ▼
┌─────────────────────┐
│  AmdDevice           │
│  ├ open /dev/dri/    │  DRM device file
│  ├ GEM_CREATE        │  Allocate GPU buffers
│  ├ GEM_MMAP          │  Map to userspace
│  ├ Build PM4 packet  │  Dispatch command
│  ├ CS_SUBMIT / USERQ │  Submit to GPU
│  └ Wait fence        │  Completion
└─────────────────────┘
```

### Evolution Passes for coralDriver

| Pass | AMD | NVIDIA | Character |
|------|-----|--------|-----------|
| Pass 1 | `nix` raw ioctl + DRM constants | — | FFI scaffold (raw syscalls) |
| Pass 2 | Safe Rust DRM wrapper types | — | Typed safe layer |
| Pass 3 | Pure Rust PM4 builder, MQD types | — | Internal Rust |
| Pass 4 | Zero-copy dispatch, async fences | — | Optimized Rust |
| Pass 5 | No `unsafe` in public API | nouveau ioctl | Sovereign |

---

## Vendor-Agnostic IR Evolution

### Current State

The codegen IR is NAK-inherited and NVIDIA-flavored but already
partially vendor-agnostic:

| Component | Vendor-Agnostic? | Evolution Needed |
|-----------|:----------------:|------------------|
| SSA IR (ops, types, control flow) | Mostly | Minor: generalize warp → wave |
| `TranscendentalOp` | Yes | Already renamed from MuFuOp |
| `RegFile` (GPR/UGPR/Pred/Carry/Bar) | No | Parameterize per vendor |
| Optimization passes | Yes | Shared across all backends |
| `legalize()` | No | Per-vendor legalization |
| `assign_regs()` | Partially | RA core is generic, register files differ |
| Encoders (`sm70_encode/`) | No | Per-vendor encoding modules |

### Evolution Plan

1. **Phase 6a**: AMD backend uses existing IR with an AMD register file
   mapping. Validate that optimization passes work for both vendors.

2. **Phase 6b**: Extract shared IR into `coral-ir` if the abstraction
   boundary becomes clear. Do not force premature abstraction — let the
   AMD backend reveal what is truly vendor-agnostic.

3. **Phase 6c**: Formalize `GpuTarget` trait (replacing enum) if a third
   backend (Intel) materializes. Two backends can share an enum; three
   backends need a trait.

---

## Dependency Evolution Tracking

Every external dependency has a planned evolution path:

| Dependency | Type | Current | Evolution Target | Pass |
|------------|------|---------|------------------|------|
| `naga` | Rust crate | Frontend parser | Fork or upstream contrib | Pass 3 |
| `drm` crate | Rust crate (FFI) | Not yet used | Pure Rust DRM types | Pass 2→3 |
| `nix` | Rust crate (FFI) | Not yet used | Replace with direct syscall wrappers | Pass 3 |
| `tokio` | Rust crate | IPC runtime | Keep (ecosystem standard) | Stable |
| `tarpc` | Rust crate | RPC framework | Keep (ecosystem standard) | Stable |
| `thiserror` | Rust crate | Error derives | Keep (zero-cost) | Stable |
| `bytes` | Rust crate | Zero-copy buffers | Keep (ecosystem standard) | Stable |
| `clap` | Rust crate | CLI parsing | Keep (ecosystem standard) | Stable |
| `tracing` | Rust crate | Observability | Keep (ecosystem standard) | Stable |
| AMD ISA XML | Reference docs | Not yet ingested | Rust encoding tables generated at build time | Pass 1 |
| Mesa ACO | Reference (C++) | Study only | Never ingested — we build our own from ISA docs | — |
| Mesa NVK | Reference (C) | Study only | Never ingested — coralDriver replaces it | — |
| Linux DRM headers | Kernel ABI | Not yet used | Pure Rust ioctl constants (stable ABI) | Pass 1→3 |

### Categories

- **Keep**: Pure Rust, no FFI, ecosystem standard, no sovereignty concern
- **Fork**: May need patches for our use case, upstream-first where possible
- **Replace**: FFI or C dependency, tracked for internal Rust replacement
- **Reference**: Study only, never imported — we build from specs

---

## Test Strategy

### Compilation Validation

| Test Type | NVIDIA | AMD | Method |
|-----------|:------:|:---:|--------|
| Unit (encoder) | ✅ 710+ tests | ✅ 34+ tests | Instruction-level encoding verification |
| Integration (pipeline) | ✅ Complete | ✅ Complete | WGSL → binary round-trip |
| Property (proptest) | ✅ 5 tests | Extend | Random shader fuzzing |
| Chaos | ✅ 6 tests | Extend | Concurrent, truncated, determinism |
| Cross-vendor | ✅ Complete | ✅ Complete | Same WGSL → both backends → compare semantics |

### Hardware Execution Validation

| Test Type | Method | Status |
|-----------|--------|--------|
| AMD correctness | coralReef binary vs RADV/ACO binary on same shader | Planned |
| NVIDIA correctness | coralReef binary vs PTXAS binary on same shader | Planned |
| f64 precision | ULP comparison vs CPU reference | Planned (extend barraCuda suite) |
| Multi-GPU | Same shader → both GPUs → compare results | Planned |

---

## Quality Gates

All evolution passes must maintain:

- `cargo check --workspace`: PASS
- `cargo test --workspace`: PASS
- `cargo clippy --workspace --all-targets -- -D warnings`: PASS
- `cargo fmt --check`: PASS
- `cargo doc --workspace --no-deps`: PASS (no warnings)
- Zero `TODO`/`FIXME` in production (tracked in `WHATS_NEXT.md`)
- All files < 1000 LOC
- AGPL-3.0-only license
- Zero hardcoded primal names
- Capability-based discovery

---

## Timeline

| Phase | Target | Duration | Depends On |
|-------|--------|----------|------------|
| 6a — AMD ISA tables + encoder | GFX1030 instruction encoding | 2-3 weeks | AMD ISA XML specs |
| 6b — AMD legalization + RA | VGPR/SGPR allocation, wave32/64 | 2-3 weeks | 6a |
| 6c — AMD f64 lowering | v_fma_f64 native + DF64 | 3-5 days | 6b |
| 6d — AMD compilation validation | WGSL → GFX1030 binary verified | 1 week | 6c |
| 7a — coralDriver AMD (scaffold) | DRM ioctl dispatch to RX 6950 XT | 2-3 weeks | 6d |
| 7b — coralDriver AMD (internalize) | Pure Rust DRM layer | 2-3 weeks | 7a |
| 7c — coralDriver NVIDIA | nouveau dispatch (if stable) | 3-4 weeks | 7b |
| 8 — coralGpu | Unified Rust GPU abstraction | 4-6 weeks | 7b |
| 9 — Full sovereignty | Zero FFI, zero C, all internal Rust | Ongoing | 8 |

---

## Cross-Primal Integration

```
barraCuda (current):
  WGSL → naga → SovereignCompiler → SPIR-V → wgpu → Vulkan → GPU

barraCuda (Phase 6 — compiler sovereignty):
  WGSL → naga → SovereignCompiler → SPIR-V → coralReef → native binary
  (still dispatched via wgpu, but shader compilation is sovereign)

barraCuda (Phase 7 — dispatch sovereignty):
  WGSL → naga → SovereignCompiler → coralReef → native binary
  → coralDriver → DRM → GPU
  (no wgpu, no Vulkan, no Mesa — pure Rust from source to silicon)

barraCuda (Phase 9 — full sovereignty):
  WGSL → coralReef (naga integrated) → native binary
  → coralDriver → GPU
  (single Rust binary, zero external C at any layer)
```

---

*The Rust compiler is the first GPU compiler. coralReef is the second.
We lean into the language — it is our greatest advantage.*
