<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# Sovereign Multi-GPU Evolution — Pure Rust Pipeline

**Version**: 0.2.0
**Date**: March 18, 2026
**Status**: Phase 10 — Iteration 78 (Deep Debt Evolution: Typed Errors + Smart Refactoring)
**Hardware**: NVIDIA Titan V ×2 (GV100/SM70) + NVIDIA RTX 5060 (AD107/SM89)

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
PCI 03:00.0 — NVIDIA Titan V #1 (GV100)
  Kernel:   vfio-pci (sovereign — nvidia preempted at boot)
  ISA:      SM70 (Volta)
  f64:      1/2 rate (native DFMA)
  VRAM:     12 GB HBM2
  Status:   VFIO sovereign dispatch 6/7, boot sovereignty complete

PCI 4a:00.0 — NVIDIA Titan V #2 (GV100)
  Kernel:   vfio-pci (sovereign — nvidia preempted at boot)
  ISA:      SM70 (Volta)
  f64:      1/2 rate (native DFMA)
  VRAM:     12 GB HBM2
  Status:   VFIO sovereign dispatch target, boot sovereignty complete

PCI 21:00.0 — NVIDIA RTX 5060 (AD107)
  Kernel:   nvidia-drm (proprietary)
  ISA:      SM89 (Ada Lovelace)
  f64:      1/64 rate (DF64 preferred)
  VRAM:     8 GB GDDR7
  Status:   Desktop display + UVM dispatch (code-complete)
```

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
| Download AMD ISA XML specs (RDNA2) | GPUOpen `isa_spec_manager` | `specs/amd/amdgpu_isa_rdna2.xml` |
| Build XML → Rust codegen tool | AMD spec schema docs | `tools/amd-isa-gen/` (build-time) |
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
| nouveau DRM ioctl interface | NVK `nvk_cmd_dispatch.c` | **Done** — full struct ABI, 7 size assertions |
| QMD construction (SM70 Volta + SM86 Ampere) | coralReef `nvidia_headers` | **Done** — QMD v2.1 + v3.0 |
| Pushbuf command buffer | nouveau `nouveau_pushbuf.c` | **Done** — `DRM_NOUVEAU_GEM_PUSHBUF` |
| Memory management (GEM) | NVK + nouveau | **Done** — alloc/mmap/info/close |
| Fence/sync | nouveau fence API | **Done** — `gem_cpu_prep` |
| VFIO BAR0 dispatch | bare metal reverse-engineering | **6/7** — GP_PUT DMA read remaining |
| UVM RM dispatch | nvidia proprietary driver | **Code-complete** — needs HW validation |
| New UAPI (VM_INIT/VM_BIND/EXEC) | kernel 6.6+ | **Struct-complete** — wired in `new_uapi.rs` |
| Boot sovereignty | modprobe + vfio-pci.ids | **COMPLETE** (Iter 56) |

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
| `drm` crate | Rust crate (FFI) | Replaced — pure Rust DRM types in `coral-driver/src/drm.rs` | Pure Rust DRM types | Pass 2→3 |
| `nix` | Rust crate (FFI) | Replaced — rustix used for all syscalls | Replace with direct syscall wrappers | Pass 3 |
| `tokio` | Rust crate | IPC runtime | Keep (ecosystem standard) | Stable |
| `tarpc` | Rust crate | RPC framework | Keep (ecosystem standard) | Stable |
| `thiserror` | Rust crate | Error derives | Keep (zero-cost) | Stable |
| `bytes` | Rust crate | Zero-copy buffers | Keep (ecosystem standard) | Stable |
| `clap` | Rust crate | CLI parsing | Keep (ecosystem standard) | Stable |
| `tracing` | Rust crate | Observability | Keep (ecosystem standard) | Stable |
| AMD ISA XML | Reference docs | Ingested — `tools/amd-isa-gen/` generates 1,446 opcodes | Rust encoding tables generated at build time | Pass 1 |
| Mesa ACO | Reference (C++) | Study only | Never ingested — we build our own from ISA docs | — |
| Mesa NVK | Reference (C) | Study only | Never ingested — coralDriver replaces it | — |
| Linux DRM headers | Kernel ABI | Replaced — pure Rust ioctl constants (Iter 30) | Pure Rust ioctl constants (stable ABI) | Pass 1→3 |

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
| Unit (encoder) | ✅ 4318 tests | ✅ 100+ tests | Instruction-level encoding verification |
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
- AGPL-3.0-or-later license
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
| 7a — coralDriver AMD (complete) | DRM ioctl dispatch to RX 6950 XT | ✅ Done | 6d |
| 7b — coralDriver AMD (internalize) | Pure Rust DRM layer | 2-3 weeks | 7a |
| 7c — coralDriver NVIDIA | nouveau dispatch (if stable) | 3-4 weeks | 7b |
| 8 — coralGpu | Unified Rust GPU abstraction | 4-6 weeks | 7b |
| 9 — Full sovereignty | Zero FFI, zero C, all internal Rust | Ongoing | 8 |
| 10a — VFIO sovereign dispatch | BAR0 + PFIFO channel + V2 MMU, 6/7 HW tests | ✅ Done | 7b |
| 10b — UVM code-complete | RM hierarchy + GPFIFO + USERD doorbell | ✅ Done | 9 |
| 10c — Boot sovereignty | vfio-pci.ids preemption, nvidia guard | ✅ Done (Iter 56) | 9 |
| 10d — Security hardening | BDF validation, circuit breaker, chaos tests | ✅ Done (Iter 56) | 10c |
| 10e — GP_PUT last mile | H1 cache flush **proven insufficient** — cold silicon. PFIFO/GPCCS/FECS not initialized. Needs GPU warm-up via `device.resurrect` | **Blocked: cold silicon** | 10a |
| 10f — UVM hardware validation | RTX 5060 on-site validation | Next | 10b |
| 10g — Twin Titan V experiment | hotSpring Exp 070: warm GPU via nouveau (HBM2 + FECS), rebind vfio-pci, run dispatch. RTX 5060 stays as display GPU | **hotSpring** | 10e |

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

## hotSpring Exp 070 — Twin Titan V Dispatch Experiment

**Status**: Ready for hotSpring execution
**Prerequisite**: GlowPlug `device.lend`/`device.reclaim` working (validated Iter 57)

### Problem
VFIO dispatch software stack is complete, but GP_GET never advances because the
Titan V GPUs boot cold under `vfio-pci` preemption (PFIFO=0xbad00200,
GPCCS=0xbadf3000). The H1 cache flush experiment proved this is not a
cache coherency issue — the compute engines were never initialized.

### Hardware Layout
- **RTX 5060** (21:00.0, nvidia-drm) — dedicated display GPU, stays running
- **Titan V #1** (03:00.0, vfio-pci) — oracle card
- **Titan V #2** (4a:00.0, vfio-pci) — compute target

### Experiment Steps
1. Modify `device.resurrect` nvidia guard to check per-device binding (not module presence)
   — or temporarily `rmmod nvidia` (kills display, use TTY)
2. `device.resurrect` on Titan V: nouveau binds → HBM2 trains → FECS loads → GPCCS initializes
3. nouveau unbinds → vfio-pci rebinds (GlowPlug handles transition)
4. Run dispatch: `CORALREEF_VFIO_BDF=<bdf> CORALREEF_VFIO_SM=70 cargo test --test hw_nv_vfio --features vfio -- --ignored vfio_dispatch_nop_shader --test-threads=1`

### Success Criteria
- GP_GET advances past GP_PUT → software stack confirmed working
- Readback of `store_42` returns expected value
- Full benchmark battery: `bench_sovereign_dispatch`, DF64 physics, multi-buffer dispatch

### If GR Context Lost on Rebind
coralReef has `gr_context_init` in `pushbuf.rs` that can submit FECS methods
after VFIO open. If GR context does not survive the nouveau→vfio rebind,
submit `gr_context_init` before first dispatch.

---

*The Rust compiler is the first GPU compiler. coralReef is the second.
We lean into the language — it is our greatest advantage.*
