<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# coralReef — Compiler & Driver Evolution

**Last updated**: April 4, 2026 (Phase 10 — Iteration 73)
**Phase**: 10 — Multi-GPU Sovereignty, Cross-Vendor Parity & hotSpring Wiring

---

## Current Position

coralReef compiles WGSL, SPIR-V, and GLSL to native GPU binaries for NVIDIA
(SM35–SM120, including Blackwell) and AMD (RDNA2 GFX1030). Zero C dependencies, zero FFI.
4318 tests (153 ignored), ~64% line coverage (8 crates above 90%),
84/93 cross-spring WGSL shaders compile to SM70 SASS, plus 5/5 GLSL
compute shaders and 10/10 SPIR-V roundtrip tests passing. Multi-GPU
sovereignty: driver preference (vfio-first), nvidia-drm probing with
UVM delegation, ecosystem discovery, cross-vendor parity testing, zero DEBT
markers, zero libc dependency. Multi-device compile API
(`shader.compile.wgsl.multi`), FMA contraction enforcement
(`FmaPolicy::Separate` splits FFma→FMul+FAdd), `PCIe` topology awareness,
FMA hardware capability reporting per architecture.
`FirmwareInventory` + `compute_viable()` for PMU/GSP-aware dispatch viability.
`NvDrmDevice` delegates to `NvUvmComputeDevice` for full compute dispatch.
`KernelCacheEntry` + `dispatch_precompiled()` wire barraCuda kernel cache.
All DRM ioctls use `drm_ioctl_named` with operation-specific error messages.
`unsafe` confined to kernel ABI boundary in `coral-driver`.
VFIO PFIFO channel creation via BAR0 — full Volta hardware channel init with
V2 MMU 5-level page tables, RAMFC population, TSG+channel runlist, PCCSR
bind/enable. RAMUSERD offsets corrected (GP_GET@0x88, GP_PUT@0x8C).
USERMODE doorbell at BAR0+0x810090.

**Iteration 63–66**: ACR boot solver (multi-strategy SEC2→ACR→FECS chain),
falcon diagnostics, comprehensive audit closure, coralctl handler refactor
(1519→4 modules), `identity.get` + `capability.register` + `ipc.heartbeat`,
Songbird ecosystem registration. **hotSpring firmware wiring**: `MailboxSet` +
`MultiRing` on `DeviceSlot` for GPU engine interaction (FECS/GPCCS/SEC2/PMU
posted commands, ordered ring-buffer dispatch with fence values). Ember
`RingMeta` persistence across glowplug restarts. `coralctl` firmware
subcommands (`mailbox.*`, `ring.*`). Coverage push: 91 new tests.

**Iteration 47 milestone**: Deep debt evolution — unsafe code elimination (`from_raw_parts_mut` →
safe `as_mut_slice()`), zero-copy evolution (`KernelCacheEntry.binary: Vec<u8>` → `Bytes`),
driver string centralization (`DRIVER_VFIO`/`DRIVER_NOUVEAU`/`DRIVER_AMDGPU`/`DRIVER_NVIDIA_DRM`
constants in `preference.rs`), production panic elimination (6 `panic!()` → `warn!` +
`DEFAULT_LATENCY` / `debug_assert!`), `runner.rs` delegate to `experiments::run_experiment()`
(2509→778 LOC), `rm_client.rs` UUID/ioctl extraction to `rm_helpers.rs` (1000→944 LOC),
`FenceTimeout` constant, `unwrap()` → `Option::zip`. +15 new tests. 1819 tests passing.

**Iteration 46 milestone**: Structural refactoring — `diagnostic/runner.rs` (2485 LOC)
split into `experiments/` submodule with 8 handler files + context struct (769 LOC).
Clippy pedantic workspace-wide (identity ops, constant assertions, redundant closures,
range contains). 53+ new tests: AMD ISA table lookup (25), Unix JSON-RPC (8), SM70
latency/encoder (20). Coverage: 66.43% lines, 75.15% functions, 68.21% regions.
Zero files over 1000 lines.

**Iteration 19 milestone**: Back-edge live-in pre-allocation in RA (loop
headers pre-allocate for ALL live-in SSA values via `live_in_values()`),
back-edge-aware `calc_max_live_back_edge_aware()`, scheduler seeds
`live_set` from `live_in_values()` for loop headers, `calc_max_live`
multi-predecessor fix. 3 tests unblocked: su3_gauge_force_f64,
wilson_plaquette_f64, swarm_nn_forward. sigmoid_f64 fixed (Iteration 20 — SSA dominance repair via
fix_entry_live_in).

**Iteration 25 milestone**: Math function evolution — 9 new trig/inverse functions (Acos, Asin, Atan, Atan2, Sinh, Cosh, Asinh, Acosh, Atanh) via polynomial atan + identity chains. f64 precision: log2 2nd NR iteration (~52-bit), exp2 subnormal ldexp. Complex64 preamble auto-prepend for scientific shaders. All 37 DEBT markers resolved (ISA → documented constants, opt/feature → EVOLUTION markers). libc completely eliminated — ioctl via inline asm syscall. NVIDIA UVM module with ioctl definitions and device infrastructure. 1285 tests (1285 passing, 60 ignored).

**Iteration 18 milestone**: Pred→GPR legalization bug fix (`src_is_reg()`
incorrectly treated `SrcRef::True`/`SrcRef::False` as valid GPR sources),
`copy_alu_src_if_pred()` helper in all 12 SetP legalize methods, small
array promotion in `type_reg_comps()` (up to 32 registers) unblocking
xoshiro128ss, SM75 gpr.rs refactored to 929 LOC, 4 tests un-ignored.

**Iteration 17 milestone**: Cross-spring absorption (20 new shaders from
hotSpring + neuralSpring), full codebase audit, idiomatic refactoring.

**Iteration 16 milestone**: Coverage expansion from 52.75% to 63% via
legacy SM20/32/50 encoder tests, SM75/SM80 GPR latency combinatorial
unit tests, 10 new WGSL shader fixtures, multi-architecture NVIDIA
(SM70–SM89) + AMD (RDNA2/3/4) integration tests. TODOs fully replaced
with 28 categorized DEBT comments. SM30 delay clamping fix.

---

## Compiler Evolution — Expression & Statement Coverage

These are the naga IR expression and statement types. Checked items compile
through the full pipeline (naga → SSA IR → optimize → legalize → RA → encode).

### Expressions

- [x] Access (array/struct indexing)
- [x] AccessIndex (constant struct field access)
- [x] As (type cast) — partial: f32↔i32, int→f64, f32→f64; **missing: f64→f32, f64→i32**
- [x] Binary — Add, Sub, Mul, Divide (f32/f64/int via rcp), Modulo (f32/f64/int), And, Or, Xor, ShiftLeft, ShiftRight, Less, LessEqual, Greater, GreaterEqual, Equal, NotEqual
- [x] Compose (vector/struct construction)
- [x] Constant
- [x] FunctionArgument
- [x] GlobalVariable
- [x] Literal (F32, F64, I32, U32, Bool)
- [x] Load (storage, uniform, function-scope)
- [x] LocalVariable
- [x] Math — partial (see Math Functions below)
- [x] Relational (All, Any, IsNan, IsInf)
- [x] Select (ternary)
- [x] Splat (scalar → vector)
- [x] Swizzle (vector component reorder)
- [x] Unary (Negate, Not, BitwiseNot)
- [x] ArrayLength (buffer_size / element_stride via CBuf descriptor)
- [ ] CallResult
- [ ] ImageLoad / ImageSample / ImageQuery
- [ ] Override
- [ ] RayQueryGetIntersection
- [ ] SubgroupBallotResult / SubgroupOperationResult
- [ ] WorkGroupUniformLoadResult
- [ ] ZeroValue

### Statements

- [x] Block
- [x] Call (function inlining)
- [x] Emit
- [x] If / Else
- [x] Loop (with continuing + break if)
- [x] Return
- [x] Store
- [x] Switch (chain-of-comparisons: ISetP + OpBra per case, default fallthrough)
- [x] WorkGroupBarrier (BAR.SYNC)
- [x] Atomic (Add, Sub, And, Or, Xor, Min, Max, Exchange, CompareExchange) via OpAtom
- [ ] Barrier (other barrier types)
- [ ] ImageStore
- [ ] RayQuery
- [ ] SubgroupBallot / SubgroupCollectiveOperation / SubgroupGather

### Math Functions

- [x] Sqrt (f32 via MUFU, f64 via Newton-Raphson / AMD native)
- [x] Inversesqrt
- [x] Exp2 (f32 MUFU, f64 polynomial)
- [x] Log2 (f32 MUFU, f64 polynomial)
- [x] Sin (f32 MUFU, f64 Cody-Waite + minimax)
- [x] Cos (f32 MUFU, f64 Cody-Waite + minimax)
- [x] Floor, Ceil, Trunc, Round (f32)
- [x] Abs (f32, i32)
- [x] Min, Max (f32, i32, u32)
- [x] Clamp
- [x] Fma
- [x] Mix (lerp)
- [x] Step
- [x] Dot (f32)
- [x] Cross
- [x] Length
- [x] Distance
- [x] Normalize
- [x] Sign
- [x] Smoothstep
- [x] Pow (f32: MUFU.LOG2 + FMUL + MUFU.EXP2; f64: OpF64Log2 + DMUL + OpF64Exp2)
- [x] Exp (x * log2(e) → exp2)
- [x] Log (log2(x) * ln(2))
- [x] Tan (f32 via MUFU, f64 via sin/cos)
- [x] Asin, Acos, Atan, Atan2
- [x] Sinh, Cosh, Tanh, Asinh, Acosh, Atanh
- [ ] Ldexp, Frexp, Modf
- [ ] Transpose, Determinant, Inverse (matrix)
- [ ] Pack/Unpack (2x16float, 4x8snorm, etc.)
- [x] CountOneBits, ReverseBits, FirstLeadingBit, CountLeadingZeros
- [x] FirstTrailingBit
- [ ] ExtractBits, InsertBits

---

## Compiler Evolution — Blocker Priorities

### Tier 1 — DONE (iterations 4 + 5)

| Feature | Result | Shaders Unblocked |
|---------|--------|-------------------|
| ~~Binary `Divide` (f32/f64/int)~~ | **Done** | su3 (now hits reg alloc) |
| ~~Binary `Modulo` (f32/f64/int)~~ | **Done** | su3 (now hits reg alloc) |
| ~~`ArrayLength`~~ | **Done** | chi2_batch now compiles |
| ~~`Math::Pow`~~ | **Done** | rk4 (now unblocked) |
| ~~`Math::Exp` / `Math::Log`~~ | **Done** | infrastructure for more shaders |
| ~~Atomic statement (full set)~~ | **Done** | rdf_histogram now compiles |
| ~~Pointer expression tracking~~ | **Done** | **rk4_parallel + yukawa_force now compile** |

Root cause: `FunctionArgument` during inline expansion used early `return`
that bypassed `expr_map.insert()`. All subsequent stores to global buffers
from inlined functions failed with "store value not resolved". Fix: replaced
early returns with standard control flow to ensure expr_map insertion.

### Tier 2 — Top blockers now

| Feature | Shaders Blocked | Complexity |
|---------|-----------------|------------|
| ~~RA straight-line block chain~~ | ~~sigmoid_f64~~ | **Fixed Iteration 20** — SSA dominance violation from builder; fix_entry_live_in inserts OpUndef + repair_ssa |
| ~~Register allocator SSA tracking~~ | ~~su3_gauge_force~~ | **Fixed Iteration 19** — back-edge live-in pre-allocation |
| ~~Scheduler loop-carried phi~~ | ~~wilson_plaquette, swarm_nn_forward~~ | **Fixed Iteration 19** — live_in_values seeding |
| ~~Pred→GPR encoder coercion chain~~ | ~~bcs_bisection, batched_hfb~~ | **Fixed Iteration 18** |
| ~~Acos/Asin/Atan2 math functions~~ | ~~local_elementwise~~ | **Fixed Iteration 25** — polynomial atan + identity chains |
| ~~Complex64 preamble~~ | ~~dielectric_mermin~~ | **Fixed Iteration 25** — auto-prepended preamble |
| ~~Encoder GPR→comparison reg file~~ | ~~semf_batch~~ | **Fixed Iteration 12** — semf_batch now passes |
| ~~const_tracker negated immediate~~ | ~~batched_hfb_hamiltonian~~ | **Fixed Iteration 12** |

### Tier 3 — Full WGSL coverage

| Feature | Notes |
|---------|-------|
| Remaining math functions | Trig inverses, hyperbolic, bit ops, matrix ops |
| Image/texture ops | Not needed for compute shaders |
| Subgroup ops | Future: wave-level parallelism |

---

## Driver Evolution — coralDriver

### AMD (amdgpu DRM)

| Component | Status | Next Step |
|-----------|--------|-----------|
| DRM open/close | **Done** | — |
| GEM create/mmap | **Done** | — |
| GEM close | **Done** | — |
| PM4 command buffer | **Done** | SET_SH_REG + DISPATCH_DIRECT |
| BO list (buffer tracking) | **Done** | `DRM_AMDGPU_BO_LIST` create/destroy ioctl |
| CS submit | **Done** | `DRM_AMDGPU_CS` with IB + BO list |
| Fence wait | **Done** | `DRM_AMDGPU_WAIT_CS` with 5s timeout |
| **Hardware validation** | **✅ E2E verified** | RX 6950 XT — WGSL → compile → PM4 → execute → readback |

### NVIDIA (nouveau DRM)

| Component | Status | Next Step |
|-----------|--------|-----------|
| DRM open/close | **Done** | — |
| Channel create/destroy | **Done** | `DRM_NOUVEAU_CHANNEL_ALLOC` / `DRM_NOUVEAU_CHANNEL_FREE` |
| GEM alloc/mmap/info | **Done** | `DRM_NOUVEAU_GEM_NEW` + `gem_mmap` + `gem_info` |
| GEM close | **Done** | Real `DRM_IOCTL_GEM_CLOSE` ioctl |
| QMD v2.1 (Volta) + v3.0 (Ampere) | **Done** | Compute dispatch descriptors for SM70 + SM86 |
| Pushbuf submit | **Done** | `DRM_NOUVEAU_GEM_PUSHBUF` with BO tracking |
| NvDevice ComputeDevice | **Done** | Full alloc/free/upload/readback/dispatch/sync |
| Push buffer encoding | **Done** | Fixed Iteration 9 — `mthd_incr` count/method fields |
| NVIF constants | **Done** | Fixed Iteration 9 — aligned to Mesa `nvif/ioctl.h` |
| QMD CBUF binding | **Done** | Fixed Iteration 9 — `buffer_vas` → QMD constant buffer slots |
| Fence wait | **Done** | `gem_cpu_prep` (DRM_NOUVEAU_GEM_CPU_PREP) waits for last submitted QMD buffer |
| VM_INIT params | **Done** | `NV_KERNEL_MANAGED_ADDR = 0x80_0000_0000` from NVK trace |
| **Hardware validation** | **Not started** | Titan V + RTX 3090 on-site — driver path complete, awaiting HW test |

### Evolution Path

```
Current state:
  WGSL/SPIR-V/GLSL → naga → coralReef → native binary (SASS/GFX)
  ↓
  coralDriver: AMD amdgpu fully wired (GEM+PM4+CS+fence)
  coralDriver: NVIDIA nouveau fully wired (channel+GEM+pushbuf+QMD)
  coral-gpu: auto-detect DRM → alloc/dispatch/sync/readback

P0 blockers resolved (Iteration 9):
  Push buffer mthd_incr field swap, QMD CBUF binding, GPR count, NVIF constants, binding layout mapping

Next milestone (AMD):
  ... → binary → GEM BO → PM4 IB → CS submit → fence wait → readback
  Hardware: RX 6950 XT (RDNA2, on-site) — driver path ready

Next milestone (NVIDIA):
  Fence wait → E2E test (pushbuf + CBUF binding fixed Iteration 9)
  Hardware: Titan V (SM70) + RTX 3090 (SM86, on-site)

Iteration 22 — Multi-Language Frontends:
  GLSL 450 compute → naga glsl-in → coralReef pipeline (5/5 fixtures passing)
  SPIR-V roundtrip: WGSL → naga spv-out → compile() (4/10 passing, 6 blocked on Discriminant/const init)
  Fixtures reorganized: corpus/ (86 spring snapshots) vs compiler-owned (21 shaders)

Endgame:
  WGSL/SPIR-V/GLSL → coralReef → coralDriver → GPU execution → result
  No vendor SDK. No CUDA. No ROCm. Pure Rust sovereign compute.
```

---

## Ecosystem Integration

### IPC Contract (live)

| Method | JSON-RPC | tarpc | Status |
|--------|----------|-------|--------|
| `shader.compile.spirv` | ✅ | ✅ | SPIR-V → native binary |
| `shader.compile.wgsl` | ✅ | ✅ | WGSL → native binary (with `fma_policy` option) |
| `shader.compile.wgsl.multi` | ✅ | ✅ | WGSL → multiple native binaries (multi-device, cross-vendor) |
| `shader.compile.status` | ✅ | ✅ | name, version, supported_archs |
| `shader.compile.capabilities` | ✅ | ✅ | dynamic arch enumeration, FMA policies, vendor architectures |

### Spring Integration Status

| Spring | Uses coralReef | Status |
|--------|---------------|--------|
| barraCuda | `CoralCompiler` IPC client | Wired — compile + cache; `CoralReefDevice` backend pending |
| toadStool | `shader.compile.*` proxy | Wired — S130 proxies to coralReef `shader.compile.*` |
| hotSpring | Validation corpus (83 WGSL, 16 imported) | Active — 56 shaders available for import |
| groundSpring | Validation partner (V95 sovereign compilation) | Active — identified P0 push buffer fix |
| neuralSpring | coralForge shaders (43 WGSL, 8 imported) | Active — 35 shaders available for import |
| airSpring | Domain shaders (2 WGSL, 1 imported) | Active |
| wetSpring | No local WGSL (fully lean) | Indirect — Fp64Strategy dispatch model to study |

---

## Cross-Spring Shader Corpus (93 shaders)

| Result | Count | Examples |
|--------|-------|---------|
| **Compiling** | 84 | axpy, cg_kernels, sum_reduce, berendsen, vv_half_kick, kinetic_energy, mean_reduce, anderson_lyapunov (f32+f64), stress_virial, chi2_batch, rdf_histogram, rk4_parallel, yukawa_force_celllist, semf_batch, bcs_bisection, batched_hfb_hamiltonian, xoshiro128ss, **su3_gauge_force_f64**, **wilson_plaquette_f64**, **swarm_nn_forward**, … |
| df64 preamble (compiling) | 5 | gelu, layer_norm, softmax, sdpa_scores, kl_divergence |
| ~~RA straight-line block chain~~ | ~~1~~ | **Fixed Iteration 20** — sigmoid_f64 now compiles |
| ~~Register allocator SSA tracking~~ | ~~1~~ | **Fixed Iteration 19** — su3_gauge_force_f64 now compiles |
| ~~Scheduler loop-carried phi~~ | ~~2~~ | **Fixed Iteration 19** — wilson_plaquette_f64, swarm_nn_forward now compile |
| ~~Pred→GPR encoder coercion~~ | ~~2~~ | **Fixed Iteration 18** — bcs_bisection, batched_hfb_hamiltonian now pass |
| Math function (Acos) | 1 | local_elementwise |
| Complex64 preamble needed | 1 | dielectric_mermin |

### Compilation Benchmarks (SM70, debug build)

| Shader | Binary | Time | Spring |
|--------|--------|------|--------|
| `axpy_f64` | 672 B | 49 ms | hotSpring/lattice |
| `chi2_batch_f64` | 992 B | 51 ms | hotSpring/lattice |
| `cg_kernels_f64` | 768 B | 53 ms | hotSpring/lattice |
| `kinetic_energy_f64` | 944 B | 56 ms | hotSpring/md |
| `berendsen_f64` | 1,152 B | 58 ms | hotSpring/md |
| `vv_half_kick_f64` | 1,984 B | 70 ms | hotSpring/md |
| `mean_reduce` | 528 B | 80 ms | neuralSpring |
| `sum_reduce_f64` | 1,376 B | 161 ms | hotSpring/lattice |
| `rdf_histogram_f64` | 3,984 B | 196 ms | hotSpring/md |
| `anderson_lyapunov_f64` | 4,896 B | 271 ms | groundSpring |
| `anderson_lyapunov_f32` | 2,272 B | 279 ms | groundSpring |
| `stress_virial_f64` | 5,952 B | 437 ms | hotSpring/md |
| `yukawa_force_celllist_f64` | 12,272 B | 747 ms | hotSpring/md |
| `rk4_parallel` | 8,624 B | 1,527 ms | neuralSpring |

---

## Debt Reduction — Iteration 5

### Module Refactoring

| Module | Before | After | Strategy |
|--------|--------|-------|----------|
| `opt_instr_sched_prepass/mod.rs` | 842 LOC monolith | 313 LOC orchestration | Extracted `generate_order.rs` (408), `net_live.rs` (117) by logical boundary |
| `cfg.rs` | 897 LOC | **Split** → `cfg/mod.rs` (593) + `cfg/dom.rs` (298) — domain-based split (Iteration 7) |

### unwrap() Audit

| File | Count | Verdict |
|------|-------|---------|
| `ipc/mod.rs` | 46 | **All in test code** — no production debt |
| `naga_translate/mod.rs` | 29 | **All in test code** — no production debt |
| Production code total | ~210 | Concentrated in register allocator and encoder; these are internal invariant assertions |

### Unsafe Code Audit (Iteration 37 — updated)

| Location | Blocks | Assessment |
|----------|--------|------------|
| `coral-driver/src/drm.rs` | 8 | `drm_ioctl_named` (sole wrapper), `MappedRegion` mmap/munmap/as_slice/as_mut_slice, `DrmIoctlCmd`, `gem_close`, `drm_version` |
| `coral-driver/src/amd/ioctl.rs` | 1 | `amd_ioctl` safe wrapper via `drm_ioctl_named` |
| `coral-driver/src/nv/ioctl/mod.rs` | 5 | `channel_alloc/free`, `gem_new/info`, `pushbuf_submit`, `gem_cpu_prep` — all via `drm_ioctl_named` |
| `coral-driver/src/nv/ioctl/new_uapi.rs` | 4 | `vm_init`, `vm_bind_map/unmap`, `exec_submit` — all via `drm_ioctl_named` |
| `coral-driver/src/nv/uvm/mod.rs` | 3 | UVM_INITIALIZE, UVM raw_ioctl, NvUvmDevice helper |
| `coral-driver/src/nv/uvm/rm_client.rs` | 8 | RM_ALLOC, RM_CONTROL, RM_FREE, raw_nv_ioctl, rm_map_memory, rm_unmap_memory, rm_map_memory_dma — kernel ABI boundary |
| `coral-driver/src/nv/uvm_compute/` | 6 | `Send` + `Sync` impls, GPFIFO ring writes, USERD doorbell writes, GP_GET reads |

All unsafe confined to `coral-driver` (kernel ABI boundary).
8 of 9 crates enforce `#[deny(unsafe_code)]`.
5 `unsafe { zeroed() }` blocks eliminated via `bytemuck::Zeroable` (Iteration 37).

**libc eliminated** — DRM ioctls via rustix (pure Rust syscalls).
No C library links. Transitive FFI from tokio (libc) and jsonrpsee (ring) in
coralreef-core for async I/O and TLS. Evolution path: ecoPrimals BearDog/Songbird
provides pure Rust TLS — eliminates ring/openssl transitive C.

### Dependency Landscape

| Crate | Direct FFI | Pure Rust | Notes |
|-------|-----------|-----------|-------|
| coral-driver | — | rustix | Pure Rust syscalls via rustix; libc eliminated (Iter 30) |
| coral-reef | — | naga, thiserror, tracing | Zero FFI |
| coral-reef-stubs | — | (none) | Zero dependencies |
| coral-reef-bitview | — | (none) | Zero dependencies |
| coralreef-core | — | tokio, tarpc, jsonrpsee, serde | Transitive libc via tokio |

### Hardcoding Audit

- `DEFAULT_TCP_BIND = "127.0.0.1:0"` — OS-assigned port, correct for loopback discovery
- SM numbers (SM50, SM70, SM86, etc.) are legitimate ISA identifiers, not hardcoding
- No peer primal names in production code (all in tests/docs)
- coralReef has self-knowledge only; discovers other primals at runtime via IPC

---

## Phase History

| Phase | Milestone | Tests |
|-------|-----------|-------|
| 1–5.7 | NVIDIA compiler, pure Rust | 710 |
| 6a | AMD ISA tables + encoder | 744 |
| 6b–6d | AMD legalization, RA, f64, end-to-end | 787 |
| 7 | coralDriver (AMD + NVIDIA DRM) | 792 |
| 8 | coralGpu (unified API) | 797 |
| 9 | Full sovereignty (zero FFI) | 801 |
| 10 iter 4 | Spring absorption + compiler hardening | **832** |
| 10 iter 5 | Ptr tracking fix, scheduler refactor, debt audit | **832** (811 pass, 21 ignore) |
| 10 iter 6 | Deep debt internalization, IPC evolution | **856** (836 pass, 20 ignore) |
| 10 iter 7 | Safety boundary, ioctl layout tests, cfg split | **904** (883 pass, 21 ignore) |
| 10 iter 9 | E2E wiring, push buffer fix, QMD CBUF binding, GPR count, NVIF constants | **974** (952 pass, 22 ignore) |
| 10 iter 10 | AMD E2E verified — wave32, SrcEncoding, 64-bit addr, unwrap_or audit | **990** (953 pass, 37 ignore) |
| 10 iter 11 | Safe ioctl surface, dead code removed, corpus +2, absorption synced | **991** (954 pass, 37 ignore) |
| 10 iter 12 | Compiler gaps (GPR→Pred, const_tracker), 6 math ops, cross-spring wiring | **991** (955 pass, 36 ignore) |
| 10 iter 13 | Fp64Strategy enum, df64 preamble, prepare_wgsl() auto-prepend, 5 df64 tests unblocked | **991** (960 pass, 31 ignore) |
| 10 iter 14 | Statement::Switch lowering, NV MappedRegion RAII, clock_monotonic_ns, 14 diagnostic panics | **991** (960 pass, 31 ignore) |
| 10 iter 15 | AMD safe slices, inline var pre-alloc, typed DRM wrappers, TODO cleanup | **991** (960 pass, 31 ignore) |
| 10 iter 16 | Coverage expansion, legacy SM tests, latency unit tests, DEBT migration | **1116** (1116 pass, 31 ignore), 63% coverage |
| 10 iter 17 | Cross-spring absorption (20 shaders), codebase audit, idiomatic refactoring | **1134** (1134 pass, 33 ignore), 63% coverage |
| 10 iter 18 | Pred→GPR legalization fix, small array promotion, 4 tests un-ignored | **1138** (1138 pass, 29 ignore), 36/47 shaders SM70 |
| 10 iter 19 | Back-edge live-in RA, calc_max_live multi-pred, scheduler live_in seeding | **1141** (1141 pass, 26 ignore), 39/47 shaders SM70, WGSL 46/49 |
| 10 iter 20 | SSA dominance repair (fix_entry_live_in), sigmoid_f64 unblocked, gpr_tests.rs extraction | **1142** (1142 pass, 25 ignore), 40/47 shaders SM70, WGSL 47/49 |
| 10 iter 21 | Cross-spring absorption wave 2: +38 shaders (hotSpring+neuralSpring), df64_gt/lt/ge preamble, local_elementwise retired | **1174** (1174 pass, 30 ignore), 79/86 shaders SM70 |
| 10 iter 22 | Multi-language frontends: GLSL 450 compute, SPIR-V roundtrip, fixture reorg (corpus/), 5 GLSL + 10 SPIR-V RT tests | **1190** (1190 pass, 35 ignore), 79/86 WGSL + 5/5 GLSL + 4/10 SPIR-V RT |
| 10 iter 23 | Deep debt: 11 math functions (Tanh, Fract, Sign, Dot, Mix, Step, SmoothStep, Length, Normalize, Cross, Trunc), lib.rs 791→483, SM80 gpr 867→766, libc→rustix path documented, DEBT 37, audits | **1191** (1191 pass, 35 ignore), ESN reservoir unblocked, logical ops wired, GLSL fixtures expanded |
| 10 iter 24 | Multi-GPU sovereignty: DriverPreference, enumerate_render_nodes, nvidia-drm probing (UVM pending), toadStool discovery, cross-vendor parity, showcase suite | **1280** (1280 pass, 52 ignore), multi-GPU, showcase complete |
| 10 iter 25 | Math evolution: 9 trig/inverse, log2 2nd NR (~52-bit), exp2 subnormal, Complex64 preamble, 37 DEBT→0, libc eliminated, NVIDIA UVM infra | **1285** (1285 pass, 60 ignore), zero DEBT, zero libc |
| 10 iter 26 | hotSpring sovereign pipeline unblock: f64 min/max, Send+Sync, nouveau subchannel | **1286** (1286 pass, 59 ignore) |
| 10 iter 27 | Deep debt: RDNA2 literal materialization, f64 transcendental AMD encodings, f32 transcendental VOP1, OpShl/Shr/Sel non-VGPR fix, AMD SR mapping, FMA policy, PRNG preamble, 24/24 spring absorption | **1401** (1401 pass, 62 ignore) |
| 10 iter 28 | Unsafe elimination: nak-ir-proc from_raw_parts→compile_error!, 50 Op struct array migration, catch_ice, primal-rpc-client, NVVM poisoning bypass (12 tests), spring absorption wave 3 (7 shaders) | **1437** (1437 pass, 68 ignore) |
| 10 iter 29 | NVIDIA last mile: multi-GPU path-based open, SM auto-detect, Nouveau EINVAL diagnostics, UVM RM client PoC, buffer lifecycle safety | **1447** (1447 pass, 76 ignore) |
| 10 iter 30 | Spring absorption + FMA evolution: `shader.compile.wgsl.multi` API, FMA contraction enforcement (`lower_fma` pass), FMA hardware capability reporting, `PCIe` topology awareness, capability self-description evolution, NVVM bypass test hardening | **1487** (1487 pass, 76 ignore) |
| 10 iter 31 | Deep debt: doc link fixes, `#[allow]`/`#[expect]` tightening, SAFETY comments on unsafe blocks, service.rs refactor (→ service/), expanded codegen coverage, file size compliance | **1509** (1509 pass, 54 ignore) |
| 10 iter 32 | Deep debt evolution: `firstTrailingBit` + `distance` implemented, AMD `OpBRev`/`OpFlo` encoding (fixes discriminant 31), `CallResult` OpUndef→error, `BindingArray` stride fix, `shader_info.rs` split (→ shader_io/shader_model/shader_info), 19 new integration tests (interp, trig, exp/log, atomics, builtins, float modulo, uniform matrix), production mock audit, dependency analysis | **1556** (1556 pass, 54 ignore), 64% coverage |
| 10 iter 33 | NVVM poisoning validation: sovereign compilation of hotSpring DF64 Yukawa force shader (`exp_df64` + `sqrt_df64`) verified for SM70/SM86/RDNA2 — bypasses NVVM device-kill path. 6 new tests (`nvvm_poisoning_validation.rs`). Verlet integrator DF64 validated. | **1562** (1562 pass, 54 ignore) |
| 10 iter 34 | Legalize refactor, bytemuck unsafe elimination, 34 naga_translate tests, SM89 DF64, 5 HFB shaders, DRM ABI fixes (Exp 057), `quick-xml` 0.39 | **1608** (1608 pass, 55 ignore) |
| 10 iter 35 | `FirmwareInventory` + `compute_viable()` (hwLearn absorption), `drm_ioctl_typed` eliminated → all `drm_ioctl_named`, dead code removed. 24 unsafe blocks (down from 29). | **1616** (1616 pass, 55 ignore) |
| 10 iter 37 | Gap closure: `bytemuck::Zeroable` (5 structs), PCI vendor constants, AMD arch detection, `raw_nv_ioctl` helper, pushbuf constant unification, `NV_STATUS` documented, `uvm.rs` smart-refactored (→3 files), GPFIFO submission + USERD doorbell + completion polling, `NvDrmDevice` delegation to UVM, `KernelCacheEntry`, `dispatch_precompiled()`, `GpuTarget::arch_name()` | **1635** (1635 pass, 63 ignore) |
| 10 iter 38 | Deep debt solutions + idiomatic evolution: `cargo fmt` drift resolved, 6 clippy fixes (`ExternalMapping`/`RmAllocEvent`/`KernelCacheEntry` param structs, method refs, let-chain), 4 doc link fixes, smart refactors (naga_translate_tests 1486→3 files, rm_client 1031→997, op_conv 1047→796), zero-copy `Bytes`, 22 new tests (unix_jsonrpc + op_conv) | **1657** (1657 pass, 63 ignore) |
| 10 iter 39 | FECS GR context init (Gap 3), UVM CBUF descriptor alignment (Gap 2), Unsafe evolution (SAFETY comments, safe copy_from_slice), hotSpring dispatch fixes absorbed (a691023), Test coverage +10 | **1667** (1667 pass, 64 ignore) |
| 10 iter 40 | BAR0 breakthrough absorbed (sovereign MMIO GR init), 2 bugs fixed (`sm_version()` derivation, `pushbuf::class` portability), hardcoding evolved (sync timeout, page mask, local mem window, cache invalidation, SM defaults, FNV constants), Gap 6 error recovery (dispatch cleanup-on-error), chip mapping dedup, error logging, doc warning fixed | **1669** (1669 pass, 64 ignore) |
| 10 iter 41 | VFIO sovereign GPU dispatch: full VFIO core module (types, ioctls, DMA, VfioDevice), NvVfioComputeDevice with BAR0/DMA/GPFIFO dispatch, feature gate (`--features vfio`), DriverPreference updated (`vfio` first), sysfs VFIO discovery, `from_descriptor` VFIO path, 35 new unit tests + 5 HW integration tests, wateringHole toadStool hardware contract | **1669+35** (1669 default + 35 vfio, 64+5 ignore) |
| 10 iter 42 | VFIO sync + barraCuda API: `poll_gpfifo_completion()` reads GP_GET from USERD DMA page (volatile read, spin-loop, 5s timeout — mirrors UVM pattern), USERD GP_PUT write in `submit_pushbuf()`, `GpuContext::from_vfio(bdf)` + `from_vfio_with_sm()` convenience API for barraCuda integration, named constants (`userd::GP_PUT_OFFSET/GP_GET_OFFSET`, `SYNC_TIMEOUT`, `POLL_INTERVAL`) | **1669+35** (1669 default + 35 vfio, 64+5 ignore) |
| 10 iter 43 | PFIFO channel init + V2 MMU page tables + cross-primal rewire: `vfio/channel.rs` with full Volta PFIFO channel creation (RAMFC, instance block, 5-level V2 MMU, TSG+channel runlist, PCCSR bind/enable), RAMUSERD offset correction (GP_GET@0x88, GP_PUT@0x8C), USERMODE doorbell at BAR0+0x810090, subcontext PDB setup, toadStool S150-S152 acknowledged, barraCuda VFIO-primary wiring acknowledged, 12 new channel tests | **1693+47** (1693 default + 47 vfio, 71 ignore) |
| 10 iter 44 | USERD_TARGET + INST_TARGET runlist fix: `USERD_TARGET` bits (3:2) set to SYS_MEM_COHERENT (2) in DW0, `INST_TARGET` bits (5:4) set to SYS_MEM_NCOH (3) in DW2, resolves PBDMA unable to read USERD page from system memory, pfifo register constants replace literals, clippy clean, cargo fmt clean, 1 new VFIO test | **1669+48** (1669 default + 48 vfio, 74 ignore) |
| 10 iter 45 | Deep audit + refactor: vfio/channel.rs 2894→5 modules, eprintln!→tracing, IPC chaos/fault tests, 30+ unit tests (coralreef-core, coral-driver), 5 doctests fixed, unsafe evolution (SAFETY comments), clippy pedantic | **1721** (1721 passing, 61 ignored), 65.74% coverage |
| 10 iter 46 | Structural refactor + coverage: `diagnostic/runner.rs` 2485→769 LOC + `experiments/` submodule (8 handlers + context), clippy pedantic workspace-wide, 53+ new tests (AMD ISA 25, Unix JSON-RPC 8, SM70 latency/encoder 20), coverage 66.43% lines / 75.15% functions / 68.21% regions, zero files over 1000 LOC | **1804** (1804 passing, 61 ignored) |
| 10 iter 47 | Deep debt evolution: unsafe elimination (`from_raw_parts_mut` → safe `as_mut_slice()`), zero-copy `KernelCacheEntry.binary` → `Bytes`, driver string constants (`preference.rs`), 6 `panic!()` → `warn!`+`debug_assert!`, `runner.rs` experiment delegation (2509→778 LOC), `rm_helpers.rs` extraction (1000→944 LOC), `FenceTimeout` constant, `unwrap()` → `Option::zip`, +15 tests | **1819** (1819 passing, 61 ignored) |
| 10 iter 48 | Deep debt + sovereignty: `extern "C" { fn ioctl }` eliminated → `nv_rm_ioctl` via `rustix::ioctl`, clippy idiomatic patterns (`items_after_test_module`, `needless_range_loop`), formatting drift resolved, last 2 production `unwrap()` → `expect()`, capability test evolved (structural self-knowledge), +23 new tests (Unix JSON-RPC 15, main.rs 8) | **1842** (1842 passing, 61 ignored) |
| 10 iter 49 | hotSpring absorption: GV100 per-runlist registers (stride 0x10), MMU fault buffer DMA, PFIFO INTR bit 8 decode, PBDMA reset sequence, GlowPlug consolidation, `submit_runlist()` helper, GV100 register tests | **1842** (1842 passing, 61 ignored) |
| 10 iter 50 | Full audit execution: doc warnings eliminated, clippy clean with VFIO, hardcoded paths → env vars, production unwrap evolved, eprintln → tracing, smart refactoring (6 files), all files under 1000 LOC, +214 coverage tests | **1992** (1992 passing, 89 ignored), 57.54% coverage |
| 10 iter 51 | Deep audit compliance: wateringHole IPC health methods, socket path standard, config self-knowledge, zero-copy transport, coral-gpu smart refactor (977→65 LOC), SAFETY documentation, genomeBin manifest, E2E IPC test, clippy pedantic | **2157** (2157 passing, 89 ignored), 57.71% coverage |
| 10 iter 57 | Deep Debt Evolution + All-Silicon Pipeline: specs v0.6.0, socket.rs 1488→556 LOC, GP_PUT cache flush H1 (**proven insufficient** — cold silicon, not cache coherency), GlowPlug `device.lend`/`device.reclaim` VFIO broker (10x stress validated), `VfioLease` RAII harness, 35 VFIO HW tests passing, 9 hot-swap tests, or_exit()/Result evolution, VolatilePtr consolidation, AMD GFX906, Intel Dg2/XeLpg, pci_ids/chip_name(), coverage expansion, Clippy clean. **Handoff to hotSpring**: GPU init via `device.resurrect` → dispatch | **2560** (2560 passing, 90 ignored), 59.92% coverage |
| 10 iter 58 | Audit Hardening + Coverage: full codebase audit, `#[forbid(unsafe_code)]` on ember+glowplug, libc eliminated from direct deps, hardcoded paths → env vars, 14 `#[allow]`→`#[expect]`, tarpc Unix roundtrip tests (80→95% coverage), vendor_lifecycle tests, IPC error path tests, debris cleanup | **2680+** (90 ignored), 60.16% coverage |
| 10 iter 59 | Deep Coverage + Clone Reduction: +358 encoder tests (tex/mem/control/int/f64/f16 across SM20–SM70), glowplug socket+personality, unix_jsonrpc advanced, lower_f64/naga_translate clone reduction, panic→ice evolution, file splits | **3038+** (102 ignored), 65.8% line / 79.6% non-hw |
| 10 iter 60 | Deep Audit Execution + Code Quality: unwrap→expect, 14+ #[allow]→#[expect] across 11 files, tex.rs smart refactor (986→505+484), +24 tests (lib preambles/emit/compile, main shutdown_timeout), 8 SAFETY comments on unsafe, 9 unreachable→ice in encoder, hardcoding evolution (ember socket + socket group → env vars), amd-isa-gen template evolution | **3062+** (102 ignored), 65.8% line / 79.6% non-hw |
| 10 iter 62 | Deep Audit + Coverage + Hardcoding Evolution: 3460+ workspace tests, 68.7% line coverage, 108 ignored hardware-gated, quality gates green (fmt, clippy pedantic+nursery, doc, all files <1000 LOC) | **3460+** (108 ignored), 68.7% line |
| 10 iter 65 | Deep Debt Solutions + Ecosystem Integration: comprehensive audit closure (20 items), coralctl handlers refactor (1519→4 modules), `identity.get` + `capability.register` + `ipc.heartbeat`, Songbird ecosystem registration, `CORALREEF_DATA_DIR` env evolution | **3956** (119 ignored), ~66% line |
| 10 iter 66 (current) | hotSpring Firmware Wiring + Coverage Push: `MailboxSet` + `MultiRing` on `DeviceSlot`, ember `RingMeta` persistence, coralctl firmware subcommands, 31 new coverage tests (debug, FP16, ember hold, mailbox_ring handlers) | **4047** (121 ignored), ~66% line |
| 10 iter 52 | Ecosystem absorption: deny.toml `yanked = "deny"`, OrExit\<T\> pattern, IpcServiceError structured errors, coral-glowplug JSON-RPC 2.0, GpuPersonality trait system, CAP_SYS_ADMIN evolution, DRM consumer fence check, AMD Vega MI50/GFX906 metal registers, dual-format capability parsing | **2185** (2185 passing, 90 ignored), 57.71% coverage |

---

*The Rust compiler is our DNA synthase. Every evolution pass produces
strictly better code. No vendor lock-in. No C heritage. Pure Rust.
Iteration 73: 4318 tests passing, 153 ignored. ~64% line coverage (8 crates above 90%).

Zero clippy warnings. Zero doc warnings. Zero files over 1000 LOC (production).
Zero-copy transport via bytes::Bytes (including KernelCacheEntry.binary).
OrExit\<T\> for zero-panic binary validation. IpcServiceError for structured IPC errors.
coral-glowplug JSON-RPC 2.0 compliant with `device.lend`/`device.reclaim` VFIO broker,
`mailbox.*` posted-command firmware interaction, `ring.*` multi-ring GPU dispatch.
Ember ring-keeper: `RingMeta` persistence across glowplug restarts.
GpuPersonality trait-based system. `VfioLease` RAII test harness.
VFIO sovereign dispatch: BAR0 + DMA + GPFIFO + PFIFO channel + V2 MMU + sync.
GP_PUT H1 cache flush experiment: proven insufficient — root cause is cold silicon (PFIFO/GPCCS not initialized).
NVIDIA UVM dispatch: GPFIFO submission, USERD doorbell, completion polling.
IPC: `shader.compile.*` + `health.*` + `trace.*` + `identity.get` + `capability.register` + `ipc.heartbeat` + `mailbox.*` + `ring.*` + `ember.ring_meta.*` — JSON-RPC 2.0 + tarpc + Unix socket (wateringHole compliant).
Hardware: 2× Titan V (VFIO sovereign) + RTX 5060 (nvidia-drm/UVM, dedicated display GPU).
8 of 9 crates enforce #[deny(unsafe_code)].
All pure Rust. Sovereignty is a runtime choice.*

---

## Titan V Sovereignty Evolution

### Phase 1: Boot Preemption (COMPLETE — Iteration 56)

nvidia's open kernel module (580.126.18) probes ALL nvidia PCI devices at boot,
including Titan V (GV100) which has no GSP. The failed probe corrupts hardware
state, causing kernel panics when vfio-pci subsequently reads registers.

**Fix**: `softdep nvidia pre: vfio-pci` + `options vfio-pci ids=10de:1d81` forces
vfio-pci to claim both Titan V's before nvidia loads. nvidia sees "already bound
to vfio-pci" and skips them. RTX 5060 (10de:2d05) is unaffected.

Runtime guards: circuit breaker (halts BAR0 reads after 6 faults), nvidia module
guard (blocks swap/resurrect), DRM consumer guard (blocks unbind of active displays).

### Phase 1b: VFIO Dispatch — Cold Silicon Blocker (ACTIVE — Iteration 57)

VFIO dispatch pipeline is software-complete: GPFIFO ring, USERD doorbell, PFIFO
channel, V2 MMU page tables, QMD construction. H1 experiment (`clflush_range` on
GPFIFO/USERD before doorbell) confirmed: **not a cache coherency issue.**

GP_GET never advances because the GPU compute engines are cold:
- `pfifo_status = 0xbad00200` (PFIFO uninitialized)
- `gpccs_status = 0xbadf3000` (GPCCS/GR engine not loaded)
- HBM2 is untrained (cold VFIO bind with no prior driver init)

**Root cause**: FECS/GPCCS firmware must be loaded and HBM2 must be trained
before PBDMA can process GPFIFO entries. This initialization is normally
performed by `nouveau` during driver load.

**Resolution**: hotSpring Exp 070 — "twin experiment" with both Titan Vs.
GlowPlug `device.resurrect` (nvidia unload → nouveau bind → HBM2 training +
FECS firmware load → nouveau unbind → vfio-pci rebind). RTX 5060 remains as
dedicated display GPU while Titan Vs are warmed. Once GP_GET advances,
run the full dispatch + readback battery.

### Phase 2: Custom PMU Falcon Firmware (PLANNED)

GV100 has a programmable PMU (Falcon microcontroller) that accepts unsigned
firmware. Writing custom Falcon firmware in Rust would replace vendor firmware
dependency entirely. The PMU handles power management, fan control, and clock
gating — all currently managed by nouveau or left to hardware defaults.

### Phase 3: Sovereign HBM2 Training (PLANNED)

coral-driver already has the HBM2 training typestate machine
(`Untrained` -> `PhyUp` -> `LinkTrained` -> `DramReady` -> `Verified`).
Completing this eliminates the dependency on nouveau for HBM2 resurrection.
Direct FBPA, LTC, PFB, and PCLOCK register programming via VFIO BAR0.

### Phase 4: Vendor-Agnostic GPU Abstraction (VISION)

Build a unified Rust abstraction over AMD and NVIDIA register interfaces.
coral-driver already abstracts over both via the `ShaderModel` trait and
vendor-specific BAR0 backends. Extending this to cover initialization,
memory training, and power management creates a truly sovereign stack
where vendor kernel modules are never needed.
