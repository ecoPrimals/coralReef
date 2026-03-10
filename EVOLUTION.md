# coralReef — Compiler & Driver Evolution

**Last updated**: March 9, 2026 (Phase 10 — Iteration 27)
**Phase**: 10 — Multi-GPU Sovereignty & Cross-Vendor Parity

---

## Current Position

coralReef compiles WGSL, SPIR-V, and GLSL to native GPU binaries for NVIDIA
(SM70–SM89) and AMD (RDNA2 GFX1030). Zero C dependencies, zero FFI.
1401 tests (1401 passing, 62 ignored), 63% line coverage (target 90%),
79/86 cross-spring WGSL shaders compile to SM70 SASS, plus 5/5 GLSL
compute shaders and 4/10 SPIR-V roundtrip tests passing. Multi-GPU
sovereignty: driver preference (nouveau-first), nvidia-drm probing,
toadStool ecosystem discovery, cross-vendor parity testing, zero DEBT
markers, zero libc dependency.

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
- [ ] FirstTrailingBit
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
| `shader.compile.wgsl` | ✅ | ✅ | WGSL → native binary |
| `shader.compile.status` | ✅ | ✅ | name, version, supported_archs |
| `shader.compile.capabilities` | ✅ | ✅ | dynamic arch enumeration |

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

## Cross-Spring Shader Corpus (47 shaders)

| Result | Count | Examples |
|--------|-------|---------|
| **Compiling** | 79 | axpy, cg_kernels, sum_reduce, berendsen, vv_half_kick, kinetic_energy, mean_reduce, anderson_lyapunov (f32+f64), stress_virial, chi2_batch, rdf_histogram, rk4_parallel, yukawa_force_celllist, semf_batch, bcs_bisection, batched_hfb_hamiltonian, xoshiro128ss, **su3_gauge_force_f64**, **wilson_plaquette_f64**, **swarm_nn_forward**, … |
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

### Unsafe Code Audit (Iteration 15)

| Location | Blocks | Assessment |
|----------|--------|------------|
| `coral-driver/src/drm.rs` | 2 | `drm_ioctl_typed` + `drm_ioctl_named` — documented `#[repr(C)]` safety; typed wrappers (`gem_close`, `drm_version`) eliminate call-site unsafe |
| `coral-driver/src/amd/gem.rs` | 1 | RAII `MappedRegion` (mmap/munmap + `as_slice()`/`as_mut_slice()`); zero raw ptr ops at call sites |
| `coral-driver/src/amd/ioctl.rs` | 3 | `amd_ioctl`/`amd_ioctl_read` safe wrappers + `clock_monotonic_ns` consolidated helper |
| `coral-driver/src/nv/ioctl.rs` | 1 | RAII `NvMappedRegion` (mmap/munmap + `as_slice()`/`as_mut_slice()`); `gem_mmap_region` returns safe type |
| `nak-ir-proc/src/lib.rs` | 2 | Proc-macro `from_raw_parts` — lifetime-bounded, `repr(C)` contiguity checked |

**libc eliminated** — DRM ioctls now use inline asm syscalls, mmap via rustix.
No C library links. Transitive FFI from tokio (libc) and jsonrpsee (ring) in
coralreef-core for async I/O and TLS. Evolution path: biomeOS BearDog/Songbird
provides pure Rust TLS — eliminates ring/openssl transitive C.

### Dependency Landscape

| Crate | Direct FFI | Pure Rust | Notes |
|-------|-----------|-----------|-------|
| coral-driver | libc | — | Required for Linux DRM syscalls |
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
| 10 iter 27 (current) | Deep debt + cross-spring absorption: RDNA2 literal materialization, f64 transcendental AMD encodings, 24/24 spring absorption tests | **1401** (1401 pass, 62 ignore) |

---

*The Rust compiler is our DNA synthase. Every evolution pass produces
strictly better code. No vendor lock-in. No C heritage. Pure Rust.
Iteration 27: 1401 tests passing, 62 ignored. RDNA2 literal
materialization, f64 transcendental AMD encodings, 24/24 spring
absorption tests on SM70+RDNA2. All AMD f64 ops encoded.
AMD E2E verified — sovereign pipeline proven on hardware.*
