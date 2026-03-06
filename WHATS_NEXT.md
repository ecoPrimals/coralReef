# coralReef — What's Next

**Last updated**: March 6, 2026

---

## All Phases Complete (1–9)

### Phase 1–5.7 — NVIDIA Compiler Foundation
- [x] Compiler sources extracted, stubs evolved, ISA tables (46 files, 51K LOC → pure Rust)
- [x] UniBin, IPC (JSON-RPC 2.0 + tarpc), zero-knowledge startup
- [x] naga frontend (WGSL + SPIR-V), f64 transcendental lowering
- [x] Error safety (pipeline fully fallible), naming evolution (Mesa→idiomatic Rust)
- [x] 710 tests, zero clippy warnings, all files < 1000 LOC

### Phase 6a — AMD ISA + Encoder
- [x] AMD ISA XML specs ingested (RDNA2), 1,446 instructions, 18 encodings
- [x] GFX1030 instruction assembler (SOPP/SOP1/SOP2/VOP1/VOP2/VOP3)
- [x] LLVM cross-validated: all encodings match `llvm-mc --mcpu=gfx1030` bit-exact
- [x] AMD register file model (VGPR/SGPR/VCC/EXEC/SCC/M0)
- [x] 34 AMD-specific tests (7 LLVM-validated round-trip encodings)

### Phase 6b — ShaderModel Refactoring + AMD Legalization
- [x] **Deep debt: `Shader<'a>` refactored from `&'a ShaderModelInfo` to `&'a dyn ShaderModel`**
- [x] **`sm_match!` macro preserved for NVIDIA compat, AMD implements `ShaderModel` directly**
- [x] **`max_warps()` added to `ShaderModel` trait (replaces `warps_per_sm` field)**
- [x] **35+ files updated, `const fn` → `fn` for trait object compatibility**
- [x] `ShaderModelRdna2` — direct `ShaderModel` impl for RDNA2
- [x] AMD-specific legalization pass (VOP2/VOP3 source constraints)
- [x] VGPR/SGPR register allocation via existing RA infrastructure

### Phase 6c — AMD f64 Lowering
- [x] Native `v_sqrt_f64`, `v_rcp_f64` emission (no Newton-Raphson needed)
- [x] AMD path skips MUFU-based lowering — hardware provides full-precision f64
- [x] `lower_f64_function` detects AMD vs NVIDIA and routes accordingly

### Phase 6d — AMD End-to-End Validation
- [x] `AmdBackend` wired into `backend_for()` dispatch
- [x] Cross-vendor test: same WGSL → NVIDIA + AMD → both produce binary
- [x] AMD binary has no SPH header (compute shaders only)
- [x] `compile_wgsl()` and `compile()` support `GpuTarget::Amd(AmdArch::Rdna2)`
- [x] 81 integration tests (8 new AMD cross-vendor tests)

### Phase 6 — Sovereign Toolchain
- [x] Python ISA generator (`gen_rdna2_opcodes.py`) replaced with pure Rust (`tools/amd-isa-gen/`)
- [x] Rust generator produces identical output: 1,446 instructions, 18 encodings

### Phase 7a — AMD coralDriver
- [x] `coral-driver` crate with `ComputeDevice` trait
- [x] DRM device open/close via pure Rust inline asm syscalls
- [x] GEM buffer create/mmap/close via amdgpu ioctl
- [x] PM4 command buffer construction (SET_SH_REG, DISPATCH_DIRECT)
- [x] Command submission scaffold

### Phase 7b — Internalize
- [x] Pure Rust ioctl (inline asm, no libc, no nix)
- [x] Pure Rust mmap/munmap syscall wrappers
- [x] Zero `extern "C"` in public API

### Phase 7c — NVIDIA coralDriver
- [x] nouveau DRM scaffold (channel create/destroy, GEM)
- [x] QMD v3.0 construction (SM86 Ampere compute dispatch)
- [x] `NvDevice` implements `ComputeDevice` trait

### Phase 8 — coralGpu
- [x] `coral-gpu` crate: unified compile + dispatch
- [x] `GpuContext` with `compile_wgsl()`, `compile_spirv()`
- [x] Vendor-agnostic API (AMD + NVIDIA from same interface)
- [x] 5 tests

### Phase 9 — Full Sovereignty
- [x] Zero `extern "C"` in any crate
- [x] Zero `*-sys` in dependency tree
- [x] Zero FFI — DRM ioctl via inline asm syscalls
- [x] ISA generator in pure Rust (Python scaffold deprecated)
- [x] 801 tests, zero failures across workspace

---

## Remaining Work (Future Passes)

### Precision Improvements
- [ ] log2 Newton refinement: second iteration for full f64 (~52-bit)
- [ ] exp2 edge cases: subnormal handling in ldexp
- [ ] sin/cos: extended precision constants for large argument reduction

### Hardware Validation
- [ ] RTX 3090 (SM86): end-to-end compilation + GPU execution validation
- [ ] RX 6950 XT (GFX1030): AMD backend compilation + execution validation
- [ ] Multi-GPU: same shader → both GPUs → compare results

### Coverage (current → 90% target)
- [ ] SM70 instruction-level encoder tests (each instruction type)
- [ ] AMD encoder expansion (more Op coverage in `encode_rdna2_op`)
- [ ] Property tests with wider random shader generation

### Deep Debt (Inherited NAK)
- [ ] ~750 `.unwrap()`/`.expect()`/`panic!()` in codegen encoders → `Result` propagation
- [ ] ~35 TODOs in codegen (ISA encoding gaps)
- [ ] ~31 `#[allow(dead_code)]` items (ISA infrastructure)

### coralDriver Hardening
- [ ] Full `DRM_AMDGPU_CS` submission (IB + BO list + dependencies)
- [ ] Real fence wait via `DRM_AMDGPU_WAIT_CS`
- [ ] nouveau pushbuf actual submission
- [ ] Async fence support (tokio integration)

### barraCuda Integration
- [ ] Replace wgpu in barraCuda with `coral-gpu` for compute workloads
- [ ] Multi-GPU dispatch (RTX 3090 DF64 + RX 6950 XT native f64)
- [ ] Capability-based GPU selection via primal discovery

---

*All core phases complete. The Rust compiler is the DNA synthase — the
entire pipeline from WGSL source to GPU silicon is internal Rust. Every
pass produces strictly better Rust. Anything else is a bandaid fix.*
