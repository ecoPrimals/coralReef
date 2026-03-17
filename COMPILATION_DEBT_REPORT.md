<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# Compilation Gaps and Debt Report

**Generated:** March 10, 2026 (metrics updated March 17, Iter 54)  
**Workspace:** coralReef

---

## 1. Test Summary

```
test result: ok. 127 passed; 0 failed; 5 ignored (coral-driver)
test result: ok. 0 passed; 0 failed; 6 ignored (hw_amd_buffers)
test result: ok. 0 passed; 0 failed; 2 ignored (hw_amd_dispatch)
test result: ok. 0 passed; 0 failed; 5 ignored (hw_amd_e2e)
test result: ok. 0 passed; 0 failed; 2 ignored (hw_amd_probe)
test result: ok. 0 passed; 0 failed; 8 ignored (hw_amd_stress)
test result: ok. 0 passed; 0 failed; 5 ignored (hw_nv_buffers)
test result: ok. 0 passed; 0 failed; 11 ignored (hw_nv_nouveau)
test result: ok. 0 passed; 0 failed; 3 ignored (hw_nv_probe)
test result: ok. 11 passed; 0 failed; 1 ignored (parity_harness)
test result: ok. 10 passed; 0 failed; 0 ignored (spirv_roundtrip)
test result: ok. 17 passed; 0 failed; 7 ignored (spring_absorption)
test result: ok. 84 passed; 0 failed; 0 ignored (wgsl_corpus)
test result: ok. ? passed; 0 failed; 5 ignored (spring_absorption_wave3)
```

**Total ignored:** 61 tests across workspace (hardware-gated + diagnostic).

---

## 2. Ignored Tests by Category

### 2.1 Hardware-gated (amdgpu)

| File | Test | Reason |
|------|------|--------|
| `hw_amd_buffers.rs` | alloc_gtt_succeeds, alloc_vram_succeeds, upload_readback_roundtrip, upload_readback_with_offset, alloc_multiple_free_reverse, double_free_returns_error | requires amdgpu hardware |
| `hw_amd_dispatch.rs` | dispatch_trivial_shader, dispatch_multiple_workgroups | requires amdgpu hardware |
| `hw_amd_e2e.rs` | storage_write_shader_compiles_for_rdna2, nop_shader_dispatches_and_syncs, handcrafted_store_42_shader, hardcoded_va_store_42_shader, dispatch_writes_42_and_readback_verifies, compute_double_readback_verifies, compute_add_dual_buffer_verifies | requires amdgpu hardware |
| `hw_amd_probe.rs` | amd_device_opens_successfully, amd_device_opens_twice | requires amdgpu hardware |
| `hw_amd_stress.rs` | large_buffer_4mb_roundtrip, large_buffer_64mb_vram_alloc, sequential_dispatches_10x, rapid_alloc_free_100x, many_concurrent_buffers, dispatch_no_buffers_20x, sync_without_dispatch_is_noop, mixed_domain_alloc_with_dispatch | requires amdgpu hardware |
| `parity_harness.rs` | parity_hw_amd_store42_dispatch | requires amdgpu hardware |

### 2.2 Hardware-gated (NVIDIA proprietary / nvidia-drm)

| File | Test | Reason |
|------|------|--------|
| `nv/uvm.rs` | uvm_device_opens, uvm_initialize, rm_client_alloc, rm_client_alloc_device, rm_client_alloc_subdevice | requires proprietary nvidia driver loaded |
| `hw_nv_buffers.rs` | device_opens_successfully, alloc_returns_pending_uvm_error, dispatch_returns_pending_uvm_error, sync_succeeds, sm86_compilation_independent_of_driver | requires nvidia-drm hardware |
| `hw_nv_probe.rs` | nvidia_drm_render_node_discovered, nvidia_drm_device_opens_and_queries_driver | requires nvidia GPU with nvidia-drm module |

### 2.3 Hardware-gated (nouveau)

| File | Test | Reason |
|------|------|--------|
| `hw_nv_nouveau.rs` | nouveau_device_opens, nouveau_alloc_free, nouveau_upload_readback_roundtrip, nouveau_full_dispatch_cycle, nouveau_multiple_dispatches, nouveau_sync_without_dispatch | requires nouveau hardware (Titan V / SM70) |
| `hw_nv_nouveau.rs` | nouveau_diagnose_channel_alloc, nouveau_channel_alloc_hex_dump, nouveau_firmware_probe, nouveau_gpu_identity_probe, nouveau_gem_alloc_without_channel | requires nouveau hardware — diagnostic |

### 2.4 Hardware-gated (multi-GPU)

| File | Test | Reason |
|------|------|--------|
| `hw_nv_probe.rs` | multi_gpu_enumerates_both | requires amdgpu + nvidia GPUs |

### 2.5 SPIR-V path — **RESOLVED (Iteration 31)**

All 10 SPIR-V roundtrip tests now pass:
- `Expression::Relational` (All, Any, IsNan, IsInf) implemented
- Non-literal constant initializer (`Expression::Compose` in global expressions) supported
- Critical-edge phi handling in `repair_ssa` for multi-successor blocks

### 2.6 AMD encoding (RDNA2 VOP3 / Discriminant) — **RESOLVED (Iterations 27+31)**

All AMD RDNA2 encoding issues resolved:
- Literal constant materialization (V_MOV_B32 prefix) — Iteration 27
- AMD `OpFRnd` encoding (V_TRUNC/FLOOR/CEIL/RNDNE for f32/f64) — Iteration 31
- AMD Discriminant expression tests all pass — Iteration 31

### 2.7 Compilation gaps (WGSL corpus) — **9/9 RESOLVED (Iteration 31)**

| Shader | File | Status |
|--------|------|--------|
| ~~dielectric_mermin_f64~~ | wgsl_corpus.rs | **RESOLVED** — Complex64 preamble auto-prepended (Iteration 25) |
| ~~wilson_action_f64~~ | wgsl_corpus.rs | **RESOLVED** — SU3 preamble auto-prepended (Iteration 31) |
| ~~polyakov_loop_f64~~ | wgsl_corpus.rs | **RESOLVED** — Complex64 + SU3 preamble (Iteration 31) |
| ~~lattice_init_f64~~ | wgsl_corpus.rs | **RESOLVED** — SU3 + PRNG preamble (Iteration 31) |
| ~~torsion_angles_f64~~ | wgsl_corpus.rs | **RESOLVED** — unreachable block elimination in repair_ssa (Iteration 31) |
| ~~euler_hll_f64 (SM70)~~ | spring_absorption_wave3.rs | **RESOLVED** — vec3\<f64\> componentwise scalarization (Iteration 31) |
| ~~hill_dose_response_f64 (SM70)~~ | spring_absorption_wave3.rs | **RESOLVED** — f64 log2/exp2 widening for 1-component sources (Iteration 31) |
| ~~deformed_potentials_f64 (RDNA2)~~ | spring_absorption_wave3.rs | **RESOLVED** — Relational expression support (Iteration 31) |
| ~~population_pk_f64 (RDNA2), hill_dose_response_f64 (RDNA2)~~ | spring_absorption_wave3.rs | **RESOLVED** — Relational expression support (Iteration 31) |

---

## 3. EVOLUTION Markers (Documented Future Work)

| File | Line | Context |
|------|------|---------|
| `codegen/ir/op_conv.rs` | 462 | `PrmtSel` for non-Index modes (Index is the only one used) |
| `codegen/ir/op_cf.rs` | 142 | OpBra .u form with additional UPred input |
| `codegen/nv/sm70.rs` | 186 | co-issue |
| `codegen/nv/sm30_instr_latencies.rs` | 54 | Dual-issue support; check both previous ops |
| `codegen/nv/sm30_instr_latencies.rs` | 73 | Dual issue (0x04), Functional Unit tracking |
| `codegen/nv/sm32/control.rs` | 141 | Add .s modifier to next instruction instead of nop.s |
| `codegen/opt_instr_sched_prepass/mod.rs` | 23 | Model more cases where we actually need 2 reserved GPRs |
| `codegen/nv/sm70_encode/encoder.rs` | 558 | set_src_cx for CBuf ALU encoding |
| `codegen/opt_jump_thread.rs` | 60 | Jump threading for OpBra with non-uniform predicate |
| `coral-driver/.../nv_metal.rs` | 736 | AMD Metal MI50 support (blocked by hardware) |

---

## 4. Production `unwrap()` Status — **Audit Complete (Iteration 31)**

All production `unwrap()` calls in codegen and driver code have been replaced with
`expect()` with descriptive messages (Iterations 30–31). Remaining `unwrap()` calls
are in non-critical paths (config parsing, health checks, command dispatch) where
the values are guaranteed by prior validation.

The sm70_encode `unwrap()` calls (8 in control.rs + encoder.rs) were the last
codegen instances — all replaced with `expect()` in Iteration 31.

---

## 5. The 9 Cross-Spring Shaders — **ALL RESOLVED (Iteration 31)**

All 93 WGSL shaders now compile to SM70 and RDNA2. See Section 2.7 for per-shader resolution.

| # | Shader | Resolution |
|---|--------|------------|
| 1 | dielectric_mermin_f64 | Complex64 preamble auto-prepended (Iter 25) |
| 2 | wilson_action_f64 | SU3 preamble auto-prepended (Iter 31) |
| 3 | polyakov_loop_f64 | Complex64 + SU3 preamble (Iter 31) |
| 4 | lattice_init_f64 | SU3 + PRNG preamble (Iter 31) |
| 5 | torsion_angles_f64 | repair_ssa unreachable block elimination (Iter 31) |
| 6 | euler_hll_f64 | vec3\<f64\> componentwise scalarization (Iter 31) |
| 7 | deformed_potentials_f64 | Relational expression support (Iter 31) |
| 8 | population_pk_f64 | Relational expression support (Iter 31) |
| 9 | hill_dose_response_f64 | f64 log2/exp2 widening + Relational (Iter 31) |

---

## 6. `todo!()`, `unimplemented!()`, `panic!()` in Production Code

- **todo!():** 0
- **unimplemented!():** 0
- **panic!():** Many in codegen (ICE, illegal instruction, etc.) — see below.

### panic! in production (codegen / driver)

| Area | Count | Notes |
|------|-------|------|
| sm80_instr_latencies/gpr.rs | ~35 | Illegal field/instruction/writer/reader |
| sm75_instr_latencies/gpr.rs | ~30 | Illegal HMMA, R2UR, IMAD, RAW, WAR |
| sm70_instr_latencies.rs | ~25 | Illegal field, category, register file |
| sm120_instr_latencies.rs | ~15 | Illegal instruction, R2UR, Vote |
| codegen/ir/src.rs | 6 | ICE: float/int modifier |
| codegen/ir/src_dst.rs | 12 | ICE: SSA, modifier |
| codegen/ir/op_mem.rs | 1 | ICE: Not a cbuf |
| codegen/ir/types/cmp.rs | 1 | ICE: Cannot flip unop |
| codegen/nv/sm32/tex.rs | ~12 | Unknown LOD, CBuf, format |
| codegen/nv/sm50/alu/int.rs | ~20 | Invalid bfe, flo, iadd, imad, etc. |
| codegen/nv/sm20/alu/int.rs | 2 | imadsp src |
| codegen/nv/sm50/alu/misc.rs | 5 | Invalid mov, prmt, sel, shfl |
| codegen/nv/sm32/alu/misc.rs | 1 | Invalid mov |
| codegen/lower_f64/poly/trig.rs | 6 | Expected Many instructions |
| codegen/lower_f64/poly/exp2.rs | 1 | Expected Many instructions |
| codegen/builder/emit.rs | 5 | Unsupported iadd64, permute |
| codegen/nv/sm20/mem.rs | ~10 | CBuf, atomic, cctl |
| codegen/opt_bar_prop.rs | 4 | expected BSync, SSA bar |
| codegen/opt_out.rs | 1 | expected Out |
| codegen/assign_regs/block.rs | 1 | (panic) |
| codegen/mod.rs | 1 | ICE macro |

Most are ICE / illegal-path guards in codegen; some are assertion-style panics.

---

## 7. `#[allow(clippy::*)]` That Could Be Tightened

| File | Attribute | Suggestion |
|------|-----------|------------|
| coralreef-core/src/ipc/tarpc_transport.rs | `#[allow(clippy::unused_async, ...)]` | Consider `#[expect]` if the lint is expected to be fixed |
| coral-reef/src/codegen/amd/shader_model.rs | `#[allow(clippy::wildcard_imports)]` | Could narrow to specific imports |
| coral-reef/src/codegen/ops/*.rs | `#[allow(clippy::wildcard_imports)]` | Same — narrow scope if possible |
| coral-reef/src/codegen/nv/sm80_instr_latencies/gpr.rs | `#[allow(dead_code, reason = "...")]` | Good candidate for `#[expect(dead_code)]` — documents future SM support |
| coral-reef/src/codegen/nv/sm75_instr_latencies/gpr.rs | `#[allow(dead_code, reason = "...")]` | Same |
| coralreef-core/src/main.rs | `#[allow(dead_code)]` on InternalError | Consider `#[expect(dead_code)]` if enum variant is intentionally unused |
| coral-reef/src/lib.rs | `#[allow(non_camel_case_types, non_snake_case, dead_code, missing_docs)]` | Broad; consider per-module or per-type overrides |
| coral-reef/src/codegen/amd/isa_generated/mod.rs | Multiple `#[allow(dead_code)]` | Generated code; acceptable |

### Status (Iter 32)

All reviewed. `#[allow]` is preferred over `#[expect]` for configuration-dependent lints
(dead_code, unused_async, wildcard_imports) that may not fire in all build configurations.
`#[expect]` causes "unfulfilled lint expectation" warnings across test vs lib builds.
Current attributes have documented `reason` strings where appropriate.

---

## Summary

| Metric | Value (as of Iter 54) |
|--------|-------|
| Tests passing | 2364 default + 48 VFIO |
| Ignored tests | 90 (hardware-gated + diagnostic + VFIO HW) |
| EVOLUTION markers | 10 (documented future optimizations — intentional) |
| TODO markers | 0 (amd_metal.rs stubs filled with MI50/GFX906 registers, Iter 52) |
| Production unwraps | 0 (all evolved to expect/error) |
| Non-compiling shaders | 0 (93/93 resolved Iter 31) |
| todo!/unimplemented! | 0 |
| panic! in production | ~150+ (codegen ICE guards — intentional; ~80 standardized to ice!() macro, Iter 53) |
| #[allow] to tighten | Reviewed Iter 32: `#[allow]` preferred for config-dependent lints |
| unsafe { zeroed() } | 0 (eliminated via bytemuck::Zeroable, Iter 37) |
| unsafe { from_raw_parts_mut } | 0 (eliminated → safe as_mut_slice(), Iter 47) |
| extern "C" | 0 (eliminated Iter 48: raw_nv_ioctl → nv_rm_ioctl via rustix) |
| Files over 1000 LOC | 0 (Iter 54: pci_discovery.rs test extraction; excl. generated ISA tables) |
| Clippy warnings | 0 (pedantic + nursery + all) |
| Doc warnings | 0 (Iter 54: 10 DriverError links fixed) |
| Region coverage (llvm-cov) | 59.39% (target 90%) |
| Line coverage (llvm-cov) | 59.92% (target 90%; testable code: 72.7%) |
| Function coverage | 69.45% (target 90%) |
| IPC health methods | 3 (`health.check`, `health.liveness`, `health.readiness` — wateringHole compliant) |
| IPC chaos/fault tests | 6 (Iter 45) + 12 fault injection (Iter 53) |
| eprintln! in production | 0 (migrated to tracing, Iter 45; diagnostic eprintln retained for HW debug) |
| Zero-copy | KernelCacheEntry.binary: Bytes, RPC transport: buf.drain() → Bytes |
| Socket path standard | `$XDG_RUNTIME_DIR/biomeos/<primal>-<family_id>.sock` (wateringHole IPC protocol) |
| Driver constants | DRIVER_VFIO/NOUVEAU/AMDGPU/NVIDIA_DRM in preference.rs (Iter 47) |
| RM ioctl sovereignty | nv_rm_ioctl via rustix (Iter 48: zero extern "C") |
| SAFETY documentation | All `unsafe impl Send/Sync` blocks documented (Iter 51) |
