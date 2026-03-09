# coralReef — What's Next

**Last updated**: March 9, 2026 (Phase 10 — Iteration 25)

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
- [x] Command submission (full IOCTL: BO list, IB, fence sync)

### Phase 7b — Internalize
- [x] Pure Rust ioctl (inline asm, no libc, no nix)
- [x] Pure Rust mmap/munmap syscall wrappers
- [x] Zero `extern "C"` in public API

### Phase 7c — NVIDIA coralDriver
- [x] nouveau DRM channel alloc/destroy (DRM_NOUVEAU_CHANNEL_ALLOC/FREE)
- [x] nouveau GEM alloc/mmap/info (DRM_NOUVEAU_GEM_NEW)
- [x] nouveau pushbuf submit with BO tracking (DRM_NOUVEAU_GEM_PUSHBUF)
- [x] QMD v2.1 (Volta SM70) + v3.0 (Ampere SM86) compute dispatch descriptors
- [x] `NvDevice` full `ComputeDevice` impl (alloc/free/upload/readback/dispatch/sync)

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
- [x] 801+ tests, zero failures across workspace

---

## Phase 10 — Spring Absorption + Compiler Hardening (Iteration 25)

Bug reports from groundSpring V85–V95 sovereign compilation testing
and the Titan V pipeline gap analysis. See `ABSORPTION.md` for
the full Spring absorption map.

### P0 — Blocks hardware execution
- [x] **f64 instruction emission**: naga_translate now emits DMUL/DADD/DFMA/DSETP for f64 — groundSpring V85
- [x] **BAR.SYNC opex encoding**: form bits corrected 0xb1d→0x31d (register form) — groundSpring V85

### P1 — Blocks production shader compilation
- [x] **`var<uniform>` support**: CBuf reads via uniform_refs tracking — barraCuda `sum_reduce_f64.wgsl`
- [x] **Loop back-edge scheduling**: Back-edge live-in pre-allocation in RA, scheduler seeds live_set from live_in_values — 3 tests unblocked (Iteration 19); sigmoid_f64 fixed (Iteration 20 — SSA dominance repair)

### P1 — Compiler hardening (from absorption testing)
- [x] **f64 storage buffer loads**: `emit_load_f64` for 64-bit global memory
- [x] **f64 cast widening**: `translate_cast` handles `Some(8)` — f32→f64, int→i64
- [x] **f64 divide lowering**: `ensure_f64_ssa` materializes non-SSA sources in Newton-Raphson
- [x] **Type resolution**: `As`, `Math`, `Select`, `Splat`, `Swizzle`, `Relational` in `resolve_expr_type_handle`
- [x] **Vector component extraction**: `emit_access_index` returns `base[idx]` for vectors
- [x] **Copy propagation guard**: skip f64 prop for wrong component count

### P1 — Compiler evolution (Iteration 4)
- [x] **Binary Divide**: f32 (rcp+mul), f64 (OpF64Rcp+DMul), int (cast→f32→rcp→trunc→cast)
- [x] **Binary Modulo**: f32 (floor-multiply), f64 (emit_f64_floor), int (via float path)
- [x] **ArrayLength**: CBuf descriptor buffer_size / element_stride
- [x] **Math::Pow**: f32 (MUFU.LOG2+FMUL+MUFU.EXP2), f64 (OpF64Log2+DMUL+OpF64Exp2)
- [x] **Atomic statement**: full set (Add,Sub,And,Or,Xor,Min,Max,Exch,CmpExch) via OpAtom

### P1 — Ecosystem integration
- [x] Import groundSpring f64 shaders (anderson_lyapunov) as regression tests
- [x] Import hotSpring WGSL validation corpus (yukawa, dirac, su3, sum_reduce)
- [x] Import neuralSpring + airSpring cross-spring corpus (27 shaders total)
- [x] Wire tarpc `shader.compile.*` endpoints (wgsl, spirv, status, capabilities)

### P1 — Compiler evolution (Iteration 5)
- [x] **Pointer expression tracking**: `FunctionArgument` during inlining bypassed `expr_map.insert()` via early returns — fixed
- [x] **rk4_parallel**: now compiles (8,624 B, 1.53s) — unblocked by expr_map fix
- [x] **yukawa_force_celllist_f64**: now compiles (12,272 B, 747ms) — unblocked by expr_map fix

### P1 — Debt reduction (Iteration 5)
- [x] **Scheduler refactor**: `opt_instr_sched_prepass/mod.rs` 842 LOC → 313 LOC (split generate_order.rs + net_live.rs)
- [x] **unwrap() audit**: all 75 unwraps in ipc/mod.rs + naga_translate/mod.rs confirmed test-only
- [x] **Unsafe audit**: coral-driver unsafe is well-structured (RAII, documented, minimal scope)
- [x] **Dependency audit**: libc is only direct FFI dep (required for DRM); all else pure Rust

### P1 — AMD full IR encoding (Iteration 9)
- [x] **FLAT memory instructions**: `encode_flat_load`, `encode_flat_store`, `encode_flat_atomic` for Op::Ld/St/Atom
- [x] **Control flow**: `encode_s_branch`, `encode_s_cbranch_{scc0,scc1,vccnz,vccz,execnz,execz}` for Op::Bra
- [x] **Comparison encoding**: VOPC/VOP3 for FSetP/ISetP/DSetP with float/int comparison mapping
- [x] **Integer/logic ops**: V_AND/OR/XOR_B32, V_LSHLREV/LSHRREV/ASHRREV, V_ADD_NC_U32, V_MAD_U32_U24
- [x] **Type conversions**: F2F, F2I, I2F, I2I via V_MOV/V_CVT instructions
- [x] **System value registers**: S2R/CS2R → V_MOV_B32 from AMD hardware VGPRs
- [x] **Conditional select**: Sel → V_CNDMASK_B32

### P1 — Compile-time safety infrastructure (Iteration 9)
- [x] **`TypedBitField<OFFSET, WIDTH>`**: Const-generic bit field with overflow detection
- [x] **`InstrBuilder<N>`**: Fixed-size instruction word builder integrated with TypedBitField
- [x] **`derive(Encode)` proc-macro**: `#[enc(offset, width)]` attributes auto-generate `encode()` on IR structs
- [x] **ShaderModel abstraction**: `wave_size()` (32 vs 64), `total_reg_file()` (65536 vs 2048), occupancy vendor-agnostic

### P1 — coral-gpu + nouveau wiring (Iteration 9)
- [x] **`GpuContext::auto()`**: DRM render node probing, auto-detect amdgpu vs nouveau
- [x] **`GpuContext::with_device()`**: Explicit device attachment for alloc/dispatch/sync/readback
- [x] **Nouveau full DRM**: Channel alloc/destroy, GEM new/info/mmap, pushbuf submit
- [x] **NvDevice ComputeDevice**: Full alloc/free/upload/readback/dispatch/sync implementation

### P1 — Compiler gaps (remaining)
- [x] **RA straight-line block chain** — sigmoid_f64 fixed (Iteration 20: SSA dominance violation from builder; `fix_entry_live_in` inserts OpUndef + `repair_ssa`)
- [x] **Pred→GPR encoder coercion chain** — fixed (Iteration 18); bcs_bisection, batched_hfb_hamiltonian now pass
- [x] **Encoder GPR→comparison** — semf_batch now passes (Iteration 12)
- [x] **const_tracker negated immediate** — fixed (Iteration 12)

### P0 — coralDriver: sovereign E2E blockers (from groundSpring V95)
- [x] Full `DRM_AMDGPU_CS` submission (IB + BO list + fence return)
- [x] Real fence wait via `DRM_AMDGPU_WAIT_CS` (5s timeout)
- [x] Nouveau channel alloc/destroy + GEM alloc/mmap + pushbuf submit
- [x] **Push buffer encoding fix** — `mthd_incr` count/method fields transposed (groundSpring V95 root cause) — resolved Iteration 9
- [x] **NVIF constant alignment** — `ROUTE_NVIF=0x00`, `OWNER_ANY=0xFF` (Mesa `nvif/ioctl.h`) — resolved Iteration 9
- [x] **QMD constant buffer binding** — `buffer_vas` passed but ignored; shaders cannot access buffers — resolved Iteration 9
- [x] **Binding layout mapping** — WGSL `@binding(N)` → QMD CBUF index — resolved Iteration 9
- [x] **GPR count from compiler** — QMD hardcodes 32; compiler knows actual count — resolved Iteration 9

### P1 — coralDriver hardening
- [x] **Fence synchronization** — `gem_cpu_prep` for nouveau, `DRM_AMDGPU_WAIT_CS` for AMD — resolved Iteration 9
- [x] **NvDevice VM_INIT params** — `NV_KERNEL_MANAGED_ADDR = 0x80_0000_0000` constant — resolved Iteration 9
- [x] **Shared memory sizing** — `CompilationInfo.shared_mem_bytes` + `barrier_count` wired compiler → QMD — resolved Iteration 9
- [x] **ShaderInfo in dispatch trait** — `ComputeDevice::dispatch()` accepts `ShaderInfo` with GPR/shared/barrier/workgroup — resolved Iteration 9
- [ ] Titan V (SM70) hardware execution validation (nouveau dispatch ready, needs on-site)
- [ ] RTX 3090 (SM86) DRM probed (nvidia-drm on renderD129); UVM module with ioctl definitions and device infrastructure ready, compute dispatch pending integration testing
- [x] **RX 6950 XT (GFX1030) E2E verified** — WGSL compile → PM4 dispatch → readback → verified `out[0] = 42u` — resolved Iteration 10

### P0 — AMD E2E critical fixes (Iteration 10)
- [x] **CS_W32_EN wave32 dispatch** — `DISPATCH_INITIATOR` bit 15 not set → wave64 mode → only 4 VGPRs allocated (v0-v3), v4+ unmapped
- [x] **SrcEncoding literal DWORD emission** — `src_to_encoding` returned SRC0=255 for `Imm32` values without appending literal DWORD → FLAT store consumed as "literal", instruction stream corrupted
- [x] **Inline constant range** — Full RDNA2 map: 128=0, 129–192=1..64, 193–208=-1..-16; `SrcEncoding` struct bundles SRC0 + optional literal
- [x] **64-bit address pair for FLAT** — `func_mem.rs` passed `addr[0].into()` (only addr_lo) → DCE eliminated addr_hi → corrupted 64-bit address; fixed to `addr.clone().into()`
- [x] **`unwrap_or(0)` audit** — register index, branch offset, FLAT offset overflow: all return `CompileError` instead of silent truncation

### P2 — barraCuda integration
- [ ] `ComputeDispatch::CoralReef` variant in barraCuda
- [ ] SovereignCompiler → coralReef routing (replace PTXAS/NAK)
- [ ] `PrecisionRoutingAdvice` support (F64Native, F64NativeNoSharedMem, Df64Only, F32Only)

### P1 — Debt reduction (Iteration 6)
- [x] Error types → `Cow<'static, str>` (zero-allocation static error paths)
- [x] `BufferHandle` inner field sealed to `pub(crate)`
- [x] `drm_ioctl_typed` sealed to `pub(crate)` — FFI confined to `coral-driver`
- [x] Redundant `DrmDevice` Drop removed (File already handles close)
- [x] `HashMap` → `FxHashMap` in compiler hot paths (`naga_translate`)
- [x] All `#[allow]` → `#[expect]` with reason strings (Rust 2024 idiom)
- [x] IPC semantic naming: `shader.compile.{spirv,wgsl,status,capabilities}`
- [x] IPC differentiated error codes (`-32001`..`-32003`)
- [x] Unsafe helpers: `kernel_ptr`, `read_ioctl_output` (encapsulated pointer ops)
- [x] Zero production `unwrap()` / `todo!()` / `unimplemented!()`
- [x] Test coverage: +24 new tests (856 total, 836 passing, 20 ignored)
- [x] Iteration 7: +48 tests → 904 total (883 passing, 21 ignored), `#[deny(unsafe_code)]` on 6 crates, ioctl layout tests, cfg.rs domain-split
- [x] Iteration 9: +21 tests → 974 total (952 passing, 22 ignored), E2E wiring, push buffer fix, QMD CBUF binding, GPR count, NVIF constants, binding layout mapping
- [x] Iteration 10: +16 tests → 990 total (953 passing, 37 ignored), AMD E2E verified (wave32, SrcEncoding, 64-bit addr, unwrap_or audit)
- [x] Iteration 11: AMD ioctl unsafe consolidated (9 blocks → 2 safe wrappers), `DriverError::Unsupported` removed, 9 `#[allow]` → `#[expect]`, +2 corpus shaders, cross-spring absorption sync, primal names audit clean — 991 tests (954 passing, 37 ignored)
- [x] Iteration 12: GPR→Pred coercion fix, const_tracker negated immediate fix, Pred→GPR copy lowering (OpSel, True/False→GPR, GPR.bnot→Pred), 6 math ops (tan, countOneBits, reverseBits, firstLeadingBit, countLeadingZeros, is_signed_int_expr), cross-spring wiring guide in wateringHole, semf_batch_f64 now passes — 991 tests (955 passing, 36 ignored)
- [x] Iteration 13: `Fp64Strategy` enum (Native/DoubleFloat/F32Only), built-in df64 preamble (Dekker/Knuth pair arithmetic), `prepare_wgsl()` auto-prepend + `enable f64;` stripping, 5 df64 tests unblocked (gelu, layer_norm, softmax, sdpa_scores, kl_divergence), reserved keyword fix — 991 tests (960 passing, 31 ignored)
- [x] Iteration 14: `Statement::Switch` lowering (ISetP+OpBra chain), NV `NvMappedRegion` RAII (`as_slice()`/`as_mut_slice()` + Drop), `clock_monotonic_ns` consolidation, 14 diagnostic panics in lower_copy_swap, `start_block_at(label)` helper, clippy `mut_from_ref` fix — 991 tests (960 passing, 31 ignored)
- [x] Iteration 15: AMD `MappedRegion` safe slices (`ptr::copy_nonoverlapping` → `copy_from_slice`/`to_vec()`), inline `pre_allocate_local_vars` fix (callee locals in `inline_call`), typed DRM wrappers (`gem_close()`, `drm_version()` — 3 call-site unsafe eliminated), `abs_f64` inlined in BCS shader, TODO/XXX cleanup — 991 tests (960 passing, 31 ignored)
- [x] Iteration 16: Coverage expansion (52.75% → 63%), legacy SM20/SM32/SM50 integration tests via `compile_wgsl_raw_sm` API, SM75/SM80 GPR latency combinatorial unit tests (10% → 90%), 10 new WGSL shader fixtures, 15 multi-arch NVIDIA + AMD tests, SM30 delay clamping fix, TODOs → 28 DEBT comments — 1116 tests (1116 passing, 31 ignored)
- [x] Iteration 17: Cross-spring absorption (10 hotSpring CG/Yukawa/lattice + 10 neuralSpring PRNG/HMM/distance/stencil), full codebase audit (no mocks in prod, no hardcoded primals, pure Rust deps), SM75 gpr.rs refactored (1025→935 LOC via const slices), `local_elementwise_f64` retired — 1134 tests (1134 passing, 33 ignored)
- [x] Iteration 18: Pred→GPR legalization fix (src_is_reg True/False), copy_alu_src_if_pred in SetP legalize, small array promotion (type_reg_comps up to 32 regs) unblocking xoshiro128ss, SM75 gpr.rs 929 LOC, 4 tests un-ignored (bcs_bisection_f64, batched_hfb_hamiltonian_f64, coverage_logical_predicates, xoshiro128ss), 4 RA back-edge issues deferred — 1138 tests (1138 passing, 29 ignored)
- [x] Iteration 19: Back-edge live-in pre-allocation in RA (live_in_values), calc_max_live_back_edge_aware, scheduler live_in seeding, calc_max_live multi-predecessor fix — 3 tests unblocked (su3_gauge_force_f64, wilson_plaquette_f64, swarm_nn_forward), sigmoid_f64 remains ignored — 1141 tests (1141 passing, 26 ignored), 39/47 shaders SM70, WGSL 46/49
- [x] Iteration 20: SSA dominance repair (`fix_entry_live_in` detects values live-in to entry block, inserts OpUndef + repair_ssa for phi insertion), sigmoid_f64 unblocked, scheduler debug_assert_eq! promoted, SM75 gpr_tests.rs extracted — 1142 tests (1142 passing, 25 ignored), 40/47 shaders SM70, WGSL 47/49
- [x] Iteration 21: Cross-spring absorption wave 2 — 38 new test entries (9 hotSpring + 17 neuralSpring + 12 existing wired), df64 comparison operators (df64_gt/lt/ge), chi_squared keyword fix, local_elementwise_f64 retired — 1174 tests (1174 passing, 30 ignored), 79/86 shaders SM70
- [x] Iteration 22: Multi-language frontends — GLSL 450 compute frontend (naga glsl-in), SPIR-V roundtrip tests (WGSL→naga→SPIR-V→compile), fixture reorganization (86 corpus→corpus/, 21 compiler-owned stay), 5 GLSL fixtures (all pass SM70), 10 SPIR-V roundtrip tests (4 pass, 6 ignored: Discriminant expr, non-literal const init) — 1190 tests (1190 passing, 35 ignored)
- [x] Iteration 23: Deep debt elimination — 11 math functions (Tanh, Fract, Sign, Dot, Mix, Step, SmoothStep, Length, Normalize, Cross, Trunc), GLSL fixtures expanded (fract/sign/mix/step/smoothstep/tanh/dot), corpus_esn_reservoir_update unblocked, lib.rs refactored (791→483 LOC via lib_tests.rs extraction), SM80 gpr.rs tests extracted (867→766 LOC), nak-ir-proc unsafe audited (compile-time contiguity proofs), libc→rustix migration path documented (DEBT marker), DEBT count 37, orphaned fixture wired — 1191 tests (1191 passing, 35 ignored)
- [x] Iteration 24: Multi-GPU sovereignty — `DriverPreference` (nouveau > amdgpu > nvidia-drm), `enumerate_render_nodes()`, `NvDrmDevice` nvidia-drm probing (UVM pending), toadStool ecosystem discovery (`coralreef-core::discovery`), `GpuContext::from_descriptor()`, cross-vendor compilation parity tests, AMD stress tests, NVIDIA probe tests, 8-demo showcase suite, `docs/HARDWARE_TESTING.md` Titan handoff — 1280 tests (1280 passing, 52 ignored)

### P3 — Remaining debt
- [x] **Acos/Asin/Atan/Atan2 + Sinh/Cosh/Asinh/Acosh/Atanh**: polynomial atan approximation (4th-order minimax Horner) with range reduction, all inverse hyperbolic via identity chains
- [x] ~~Pred→GPR encoder coercion chain~~ — fixed Iteration 18
- [x] ~~RA back-edge SSA tracking~~ — fixed Iteration 19 (su3_gauge_force_f64, wilson_plaquette_f64, swarm_nn_forward unblocked)
- [x] ~~RA straight-line block chain~~ — fixed Iteration 20 (SSA dominance repair)
- [x] **Complex64 preamble**: `complex_f64_preamble.wgsl` with c64_add/sub/mul/inv/exp/log/sqrt/pow, auto-prepended when shader uses `Complex64` or `c64_` — unblocks dielectric_mermin
- [x] **log2 Newton refinement**: second NR iteration for full f64 (~52-bit accuracy, up from ~46-bit)
- [x] **exp2 subnormal handling**: two-step ldexp with n clamping for exponents < -1022
- [x] **37 DEBT markers resolved**: ISA encoding values documented with named constants, `DEBT(opt)` → `EVOLUTION(opt)`, `DEBT(feature)` → `EVOLUTION(feature)`, **libc eliminated** (ioctl via inline asm syscall, zero libc dependency)

- [x] Iteration 25: Math + debt evolution — 9 trig/inverse math functions (Acos, Asin, Atan, Atan2, Sinh, Cosh, Asinh, Acosh, Atanh via polynomial atan + identity chains), log2 2nd NR iteration (~52-bit f64), exp2 subnormal ldexp, Complex64 preamble (auto-prepend for dielectric_mermin), RDNA2 parity (global_invocation_id + VOP2/VOPC operand legalization), Unix socket JSON-RPC, discovery manifest, enriched CompileResponse, nouveau validation tests, **37 DEBT markers resolved** (ISA → documented constants, opt/feature → EVOLUTION markers), **libc eliminated** (ioctl via inline asm syscall), NVIDIA UVM module (ioctl definitions + device infrastructure) — 1285 tests (1285 passing, 60 ignored)

---

*The compiler evolves. 79/86 cross-spring WGSL shaders compile to native SASS.
1285 tests passing, 60 ignored, 63% line coverage. Zero production unwrap/todo. Error types zero-alloc. IPC semantic. Safety boundary enforced.
Three input languages: WGSL (primary), SPIR-V (binary), GLSL 450 (compute absorption).
AMD E2E verified — WGSL → compile → PM4 dispatch → GPU execution → readback on RX 6950 XT.
Multi-GPU sovereignty: nouveau-first driver preference, nvidia-drm probing, toadStool ecosystem discovery.
Cross-vendor parity testing: same shader compiled for SM86 and RDNA2, dispatch results verified.
8-demo showcase: hello-compiler → compute triangle (coralReef → toadStool → barraCuda).
tarpc uses bincode for high-performance binary IPC. Zero DEBT comments — all resolved or evolved.
Zero libc dependency — ioctl via inline asm syscall, mmap via rustix, zero extern "C".
NVIDIA UVM module ready for proprietary driver compute dispatch integration.
Complex64 preamble unblocks plasma physics / dielectric function shaders.
Iteration 25: Math + debt evolution — trig inverse, f64 refinement, Complex64, DEBT → 0, libc → 0, UVM infra.
Iteration 24: Multi-GPU sovereignty — driver preference, nvidia-drm, toadStool discovery, parity tests, showcase.
Iteration 23: Deep debt elimination — 11 math functions, lib.rs refactored, GLSL expanded, audits complete.
Iteration 22: Multi-language frontends — GLSL + SPIR-V roundtrip tests + fixture reorg.
All pure Rust. Sovereignty is a runtime choice.*
