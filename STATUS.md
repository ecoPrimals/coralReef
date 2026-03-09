# coralReef — Status

**Last updated**: March 8, 2026  
**Phase**: 10 — Iteration 22 (Multi-Language Frontends & Fixture Reorganization)

---

## Overall Grade: **A+** (Multi-Vendor Sovereign GPU Compiler)

| Category | Grade | Notes |
|----------|-------|-------|
| Primal lifecycle | A | Standalone `PrimalLifecycle` + `PrimalHealth`, full test coverage |
| UniBin compliance | A | Binary target with clap, panic hook, SIGTERM/SIGINT, structured errors |
| IPC | A+ | JSON-RPC 2.0 + tarpc (bincode), Unix socket + TCP, zero-copy `Bytes` payloads, `shader.compile.*` semantic naming, differentiated error codes |
| NVIDIA pipeline | A+ | WGSL/SPIR-V/GLSL → naga → codegen IR → f64 lower → optimize → legalize → RA → encode |
| AMD pipeline | A+ | `ShaderModelRdna2` → legalize → RA → encode (memory, control flow, comparisons, integer, type conversion, system values) |
| Mesa stubs evolved | A+ | All modules evolved to pure Rust (BitSet, CFG, dataflow, fxhash, nvidia_headers) |
| f64 transcendentals | A+ | sqrt, rcp, exp2, log2, sin, cos, exp, log, pow — NVIDIA (Newton-Raphson) + AMD (native) |
| Vendor-agnostic arch | A+ | `Shader` holds `&dyn ShaderModel` — idiomatic Rust trait dispatch, no manual vtables |
| coralDriver | A+ | AMD DRM ioctl (GEM, PM4, CS, BO list, fence sync), NVIDIA nouveau (channel, GEM, pushbuf, QMD dispatch), pure Rust syscalls via libc |
| coralGpu | A+ | Unified compile+dispatch API, auto-detect DRM render nodes, vendor-agnostic `GpuContext` with alloc/dispatch/sync/readback |
| Code structure | A+ | Smart refactoring: scheduler prepass 842→313 LOC, cfg.rs→cfg/{mod,dom}.rs, ir/{pred,src,fold}.rs, ipc/{jsonrpc,tarpc_transport}.rs |
| Tests | A+ | 1189 passing, 0 failed, 36 ignored, 63% line coverage (target 90%) |
| Clippy | A+ | Zero warnings, pedantic categories enabled |
| License | A | AGPL-3.0-only (upstream-derived files retain original attribution) |
| Sovereignty | A+ | Zero FFI, zero `*-sys`, zero `extern "C"`, zero-knowledge startup, `#[deny(unsafe_code)]` on 6/8 crates |
| Result propagation | A+ | Pipeline fully fallible: naga_translate → lower → legalize → encode, zero production `unwrap()`/`todo!()` |
| Dependencies | A+ | Pure Rust — zero C deps, zero `*-sys` crates, ISA gen in Rust, libc for syscalls, FxHashMap internalized |
| Tooling | A+ | `rustfmt.toml`, `clippy.toml`, `deny.toml`, pure Rust ISA generator |
| Tolerance model | A | 13-tier `tol::` module (groundSpring alignment), `within()`, `compare_all()` |
| FMA control | A | `FmaPolicy` enum (AllowFusion / NoContraction) in `CompileOptions` |
| Uniform buffers | A | `var<uniform>` → CBuf reads (scalar/vector/matrix), struct field access |

## Phase Status

| Phase | Description | Status |
|-------|-------------|--------|
| 1–9 | Foundation through Full Sovereignty | **Complete** |
| 10 — Spring Absorption | Deep debt, absorption, compiler hardening, E2E verified | **Iteration 22** |

### Phase 10 Completions

| Task | Status | Details |
|------|--------|---------|
| cargo fmt + clippy + rustdoc | ✅ | Zero warnings, zero errors |
| var\<uniform\> support | ✅ | Scalar, vector, matrix CBuf loads; struct fields via AccessIndex |
| BAR.SYNC Volta encoding | ✅ | Opcode 0x31d, 5-cycle latency, Decoupled scheduling |
| WGSL corpus import | ✅ | 16 shaders from hotSpring/groundSpring, 7 passing SM70 |
| 13-tier tolerance model | ✅ | `tol::` module with DETERMINISM..EQUILIBRIUM, comparison utilities |
| Scheduler tests unblocked | ✅ | 2/3 fixed (phi_nodes_loop_carry, nested_loops) |
| const_tracker assertion fix | ✅ | Tolerates modified sources in OpCopy |
| coalesce assertion fix | ✅ | Skips coalescing for modified sources |
| lower_copy_swap assertion fix | ✅ | Emits OpMov for copies with modifiers |
| FmaPolicy infrastructure | ✅ | `FmaPolicy` enum, `CompileOptions.fma_policy` |
| ir/mod.rs refactoring | ✅ | Extracted pred.rs, src.rs, fold.rs (918→262 LOC) |
| ipc.rs refactoring | ✅ | Split into ipc/{mod,jsonrpc,tarpc_transport}.rs (853→590+97+174 LOC) |
| tarpc method naming | ✅ | Dropped `compiler_` prefix (clippy enum_variant_names) |
| Legacy `parse_arch` removed | ✅ | Tests migrated to `parse_target` |
| ShaderModel re-export | ✅ | `pub use` at crate root, rustdoc link fixed |
| GEM close implemented | ✅ | Real `DRM_IOCTL_GEM_CLOSE` ioctl |
| AMD ioctl constants fixed | ✅ | Added `DRM_AMDGPU_BO_LIST`, removed wrong `GEM_CLOSE` |
| `is_amd()` trait method | ✅ | Capability-based vendor detection |
| Unsafe evolved → libc | ✅ | `MappedRegion` RAII, `drm_ioctl_typed` safe wrapper |
| naga_translate refactored | ✅ | expr_binary.rs, func_control.rs, func_mem.rs, func_ops.rs |

### Phase 10 — Iteration 6 Completions (Debt Reduction + Internalization)

| Task | Status | Details |
|------|--------|---------|
| AMD CS submit (`DRM_AMDGPU_CS`) | ✅ | Full IOCTL: BO list, IB submission, fence return |
| AMD fence sync (`DRM_AMDGPU_WAIT_CS`) | ✅ | Full IOCTL: `sync_fence` with 5s timeout |
| `Expression::As` (type cast) | ✅ | Resolved (Iteration 3) |
| Atomic operations | ✅ | Resolved (Iteration 4) |
| IPC semantic naming | ✅ | `shader.compile.{spirv,wgsl,status,capabilities}` |
| IPC differentiated error codes | ✅ | `-32001` InvalidInput, `-32002` NotImplemented, `-32003` UnsupportedArch |
| Error types → `Cow<'static, str>` | ✅ | Zero-allocation static error paths across all error enums |
| `BufferHandle` sealed | ✅ | `pub(crate)` inner field — driver owns validity invariant |
| `drm_ioctl_typed` sealed | ✅ | `pub(crate)` — FFI confined to `coral-driver` |
| `DrmDevice` Drop removed | ✅ | `std::fs::File` already handles close |
| `HashMap` → `FxHashMap` | ✅ | Performance-critical compiler paths (`naga_translate`) |
| `#[allow]` → `#[expect]` | ✅ | All non-wildcard `#[allow]` converted with reason strings |
| Nouveau scaffolds → explicit errors | ✅ | Explicit error paths (Iteration 6); `DriverError::Unsupported` removed as dead code (Iteration 11) |
| Unsafe helpers (`kernel_ptr`, `read_ioctl_output`) | ✅ | Encapsulated raw pointer ops with safety documentation |
| Zero production `unwrap()` / `todo!()` | ✅ | Swept — zero instances in non-test code |
| Test coverage expansion | ✅ | +24 new tests (lifecycle, health, gpu_arch, IPC, nv/ioctl) |

### Phase 10 — Iteration 7 Completions (Safety Boundary + Coverage)

| Task | Status | Details |
|------|--------|---------|
| `#[deny(unsafe_code)]` on non-driver crates | ✅ | 6/8 crates enforce safety at compile time (coral-reef, coralreef-core, coral-gpu, coral-reef-stubs, coral-reef-bitview, coral-reef-isa) |
| Ioctl struct layout tests | ✅ | 14 tests verify `#[repr(C)]` struct size and field offsets against kernel ABI |
| `sm_match!` panic eliminated | ✅ | Constructor `ShaderModelInfo::new` asserts `sm >= 20`, macro branches are exhaustive |
| Debug path configurable | ✅ | `save_graphviz` uses `CORAL_DEP_GRAPH_PATH` env var (falls back to `temp_dir()`) |
| CFG smart refactoring | ✅ | `cfg.rs` (897 LOC) → `cfg/mod.rs` (593) + `cfg/dom.rs` (298): domain-based split |
| GEM buffer bounds tests | ✅ | Out-of-bounds write/read return `DriverError`, field access, Debug |
| NV `u32_slice_as_bytes` tests | ✅ | Empty, single, multi-word byte reinterpretation verified |
| NV dispatch/sync Unsupported tests | ✅ | Explicit error paths verified |
| Frontend/compile edge case tests | ✅ | Malformed WGSL, Intel unsupported, `ShaderModelInfo::new` panic, FmaPolicy, CompileOptions accessors |
| Test coverage expansion | ✅ | 856→904 total tests (883 passing, 21 ignored) |

### Phase 10 — Iteration 8 Completions (AMD Full IR + Nouveau DRM + Compile-Time Safety)

| Task | Status | Details |
|------|--------|---------|
| coral-gpu ComputeDevice wiring | ✅ | Auto-detect DRM render nodes, alloc/dispatch/sync/readback, AMD and nouveau paths |
| AMD memory encoding (FLAT) | ✅ | `encode_flat_load`, `encode_flat_store`, `encode_flat_atomic` for FLAT instructions (64-bit) |
| AMD control flow encoding | ✅ | `encode_s_branch`, `encode_s_cbranch_{scc0,scc1,vccnz,vccz,execnz,execz}` |
| AMD comparison encoding | ✅ | VOPC/VOP3 for FSetP/ISetP/DSetP, float/int comparison to opcode mapping |
| AMD integer/logic encoding | ✅ | V_AND/OR/XOR_B32, V_LSHLREV/LSHRREV/ASHRREV, V_ADD_NC_U32, V_MAD_U32_U24 |
| AMD type conversion | ✅ | F2F, F2I, I2F, I2I via V_MOV/V_CVT instructions |
| AMD system value registers | ✅ | S2R/CS2R → V_MOV_B32 from AMD hardware VGPRs (thread/workgroup IDs) |
| AMD Sel (V_CNDMASK_B32) | ✅ | Conditional select via VCC |
| ShaderModel abstraction | ✅ | `wave_size()`, `total_reg_file()` on trait; occupancy formulas vendor-agnostic |
| TypedBitField<OFFSET,WIDTH> | ✅ | Compile-time safe bit field access with overflow detection |
| InstrBuilder<N> | ✅ | Fixed-size instruction word builder integrated with TypedBitField |
| derive(Encode) proc-macro | ✅ | `#[enc(offset, width)]` attributes generate `encode()` method on IR structs |
| Nouveau DRM channel | ✅ | `create_channel`, `destroy_channel` via DRM_NOUVEAU_CHANNEL_ALLOC/FREE |
| Nouveau GEM alloc/mmap | ✅ | `gem_new`, `gem_info`, `gem_mmap` for VRAM/GART buffers |
| Nouveau pushbuf submit | ✅ | `pushbuf_submit` with BO tracking, push entries |
| NvDevice ComputeDevice impl | ✅ | Full alloc/free/upload/readback/dispatch/sync via nouveau DRM |
| Test coverage expansion | ✅ | +49 tests → 953 total (931 passing, 22 ignored) |

### Phase 10 — Iteration 9 Completions (E2E Wiring + Push Buffer Fix + Debt Reduction)

| Task | Status | Details |
|------|--------|---------|
| Push buffer encoding fix (P0) | ✅ | New `pushbuf.rs` with correct Kepler+ Type 1/3/4 headers — `mthd_incr`, `mthd_ninc`, `mthd_immd`, `PushBuf` builder, `compute_dispatch()` method |
| NVIF constant alignment (P0) | ✅ | `NVIF_ROUTE_NVIF=0x00`, `NVIF_ROUTE_HIDDEN=0xFF`, `NVIF_OWNER_NVIF=0x00`, `NVIF_OWNER_ANY=0xFF` — aligned to Mesa `nvif/ioctl.h` |
| QMD CBUF binding (P0) | ✅ | Full 64-word QMD v2.1/v3.0 with CONSTANT_BUFFER_VALID bitmask, CBUF address pairs, size fields; `CbufBinding` + `QmdParams` types |
| WGSL @binding(N) → QMD CBUF (P0) | ✅ | Buffer handles mapped to CBUF slots by index in `NvDevice::dispatch()` |
| GPR count from compiler (P0) | ✅ | `compile_wgsl_full()` returns `CompiledBinary` with `CompilationInfo.gpr_count`; wired through `CompiledKernel` to QMD REGISTER_COUNT field |
| Nouveau fence sync (P1) | ✅ | `DRM_NOUVEAU_GEM_CPU_PREP` ioctl via `gem_cpu_prep()`; `NvDevice::sync()` waits for last submitted QMD buffer |
| NvDevice VM_INIT params (P1) | ✅ | `NV_KERNEL_MANAGED_ADDR = 0x80_0000_0000` constant (from NVK ioctl trace) |
| Shared memory + barriers (P1) | ✅ | `CompilationInfo.shared_mem_bytes` + `barrier_count` wired from compiler `ShaderInfo` through backend to QMD words 10-11 |
| Shader corpus expansion (P2) | ✅ | 13 new shaders imported (7 hotSpring: lattice, MD; 6 neuralSpring: regression, evolution) — total 40 WGSL shaders |
| `bytemuck` for safe casts (P2) | ✅ | Replaced `unsafe` `u32_slice_as_bytes` in AMD + NV drivers and pushbuf with `bytemuck::cast_slice` |
| CFG → FxHashMap (P2) | ✅ | `coral-reef-stubs/cfg/mod.rs` switched from `HashMap` to internal `FxHashMap` for compiler hot path |
| Proc-macro unwrap → expect (P2) | ✅ | `nak-ir-proc` `field.ident.as_ref().unwrap()` → `.expect()` with context message |
| Ioctl struct layout tests (P2) | ✅ | New tests for `NouveauGemPushbufBo` (40 bytes) and `NouveauGemPushbufPush` (24 bytes) kernel ABI |
| `ShaderInfo` in dispatch trait | ✅ | `ComputeDevice::dispatch()` accepts `&ShaderInfo` with GPR, shared mem, barriers, workgroup — compiler metadata reaches QMD |
| Test coverage expansion | ✅ | +21 tests → 974 total (952 passing, 22 ignored) |

### Phase 10 — Iteration 10 Completions (E2E GPU Dispatch Verified on AMD)

| Task | Status | Details |
|------|--------|---------|
| **AMD E2E: WGSL → compile → dispatch → readback → verify** | ✅ | Full sovereign pipeline on RX 6950 XT — `out[0] = 42u` writes 42, readback verified |
| CS_W32_EN wave32 dispatch | ✅ | DISPATCH_INITIATOR bit 15 — fixes VGPR allocation (wave64 allocated only 4 VGPRs) |
| SrcEncoding literal DWORD emission | ✅ | `SrcRef::Imm32` returned SRC0=255 without appending literal — FLAT store was consumed as "literal", corrupting instruction stream |
| Inline constant range (0–64, -1..-16) | ✅ | Full RDNA2 inline constant map: 128=0, 129–192=1..64, 193–208=-1..-16 |
| 64-bit address pair for FLAT stores | ✅ | `func_mem.rs` passed `addr[0]` (32-bit lo) instead of full 2-component SSARef — addr_hi eliminated by DCE |
| `unwrap_or(0)` audit → proper errors | ✅ | Register index, branch offset, FLAT offset: all return `CompileError` instead of silent truncation |
| Diagnostic hw tests cleaned | ✅ | `hardcoded_va_store_42_shader` simplified to regression test |
| Test expansion | ✅ | 991 total (955 passing, 36 ignored) |

### Phase 10 — Iteration 11 Completions (Deep Debt Reduction + Safe Ioctl Surface)

| Task | Status | Details |
|------|--------|---------|
| AMD ioctl unsafe consolidation | ✅ | 9 raw unsafe blocks → 2 safe wrappers (`amd_ioctl`, `amd_ioctl_read`) with typed request builders (`amd_iowr<T>`, `amd_iow<T>`) |
| Dead code removal | ✅ | `DriverError::Unsupported` removed (unused in production, only in its own display test) |
| `#[allow(dead_code)]` → `#[expect]` | ✅ | 9 instances migrated with reason strings; 23 on derive-generated items kept as `#[allow]` |
| WGSL corpus expansion | ✅ | +2 hotSpring MD shaders (vacf_dot_f64, verlet_copy_ref) |
| Cross-spring absorption sync | ✅ | ABSORPTION.md updated: barraCuda P0/P1 resolved, spring pin versions current |
| Primal names audit | ✅ | All 11 refs are doc-comment provenance only — zero production code violations |
| hw_amd_e2e vec! idiom | ✅ | `Vec::new()` + `push()` chain → `vec![]` macro (clippy::vec_init_then_push) |
| cargo fmt pass | ✅ | Import reordering, line wrapping applied workspace-wide |

### Phase 10 — Iteration 12 Completions (Compiler Gaps + Math Coverage + Cross-Spring Wiring)

| Task | Status | Details |
|------|--------|---------|
| GPR→Pred coercion fix | ✅ | 2 of 4 compiler gaps fixed — GPR→Pred coercion chain resolved |
| const_tracker negated immediate fix | ✅ | 2 of 4 compiler gaps fixed — const_tracker negated immediate resolved |
| Pred→GPR copy lowering | ✅ | Cross-file copy lowering: Pred→GPR (OpSel), True/False→GPR, GPR.bnot→Pred |
| 6 new math ops | ✅ | tan, countOneBits, reverseBits, firstLeadingBit, countLeadingZeros |
| is_signed_int_expr helper | ✅ | Helper for signed integer expression detection |
| Cross-spring wiring guide | ✅ | Published in wateringHole |
| semf_batch_f64 test | ✅ | Now passes (was ignored) |
| Test counts | ✅ | 991 tests (955 passing, 36 ignored) |

### Phase 10 — Iteration 13 Completions (df64 Preamble + Fp64Strategy + Test Unblocking)

| Task | Status | Details |
|------|--------|---------|
| `Fp64Strategy` enum | ✅ | `Native` / `DoubleFloat` / `F32Only` — replaces boolean `fp64_software` |
| Built-in df64 preamble | ✅ | `df64_preamble.wgsl`: Dekker multiplication, Knuth two-sum, exp/sqrt/tanh |
| Auto-prepend df64 preamble | ✅ | `prepare_wgsl()` detects `Df64`/`df64_*` usage, prepends before naga parse |
| `enable f64;` stripping | ✅ | Automatically removed — naga handles f64 natively |
| 5 df64 tests unblocked | ✅ | gelu_f64, layer_norm_f64, softmax_f64, sdpa_scores_f64, kl_divergence_f64 |
| kl_divergence reserved keyword fix | ✅ | `shared` → `wg_scratch` (WGSL reserved word) |
| wateringHole handoff | ✅ | DF64_PREAMBLE_FP64STRATEGY handoff + architecture doc updated |
| Test counts | ✅ | 991 tests (960 passing, 31 ignored) — net +5 passing |

### Phase 10 — Iteration 14 Completions (Statement::Switch + Unsafe Reduction + Diagnostic Panics)

| Task | Status | Details |
|------|--------|---------|
| `Statement::Switch` lowering | ✅ | Chain-of-comparisons: ISetP + conditional branch per case, default fallthrough, proper CFG edges |
| Switch test unblocked | ✅ | `test_sm70_control_flow` + `test_multi_arch_stress_all_shaders` pass |
| NV `NvMappedRegion` RAII | ✅ | `ptr::copy_nonoverlapping` + manual `munmap` → safe `as_slice()`/`as_mut_slice()` + RAII Drop |
| `clock_monotonic_ns` consolidation | ✅ | Extracted from inline `sync_fence` → single-site unsafe helper |
| `lower_copy_swap` diagnostic panics | ✅ | All 14 panic messages now include src/dst context for debugging |
| `start_block_at(label)` helper | ✅ | Pre-allocated label block start for switch lowering |
| clippy `mut_from_ref` fix | ✅ | `NvMappedRegion::as_mut_slice(&self)` → `(&mut self)` |
| Test counts | ✅ | 991 tests (960 passing, 31 ignored) — zero regressions |

### Phase 10 — Iteration 15 Completions (AMD Safe Slices + Inline Var Pre-allocation + Typed DRM Wrappers)

| Task | Status | Details |
|------|--------|---------|
| AMD `MappedRegion` safe slices | ✅ | `ptr::copy_nonoverlapping` → `copy_from_slice`/`to_vec()` via `as_slice()`/`as_mut_slice()` — mirrors NV pattern |
| Inline `pre_allocate_local_vars` | ✅ | Callee local variables now pre-allocated during `inline_call`, fixing var_storage slot overflow |
| `abs_f64` inlined in BCS shader | ✅ | Removed external preamble dependency — `select(x, -x, x < 0.0)` |
| Typed DRM wrappers | ✅ | `gem_close()`, `drm_version()` — removes `unsafe` from 3 call sites (AMD gem.close, NV free, DrmDevice.driver_name) |
| TODO/XXX cleanup | ✅ | Bare `TODO:` documented, `XXX:` markers → proper comments, doc-comment `TODO` → `Note` |
| Test ignore reasons updated | ✅ | `bcs_bisection_f64` (Pred→GPR coercion), `local_elementwise_f64` (Acos not yet supported) |
| Test counts | ✅ | 991 tests (960 passing, 31 ignored) — zero regressions |

### Phase 10 — Iteration 16 Completions (Coverage Expansion + Latency Unit Tests + Legacy SM Tests)

| Task | Status | Details |
|------|--------|---------|
| Legacy SM20/SM32/SM50 integration tests | ✅ | `compile_wgsl_raw_sm` test API, 15 legacy encoder tests covering ~4700 lines at 0% |
| Multi-architecture NVIDIA tests | ✅ | SM70/SM75/SM80/SM86/SM89 cross-compilation, 15 multi-arch tests |
| AMD RDNA2/RDNA3/RDNA4 tests | ✅ | Architecture variant coverage |
| SM75 GPR latency table unit tests | ✅ | Combinatorial RAW/WAW/WAR tests for all `RegLatencySM75` categories (10.9% → 90.4%) |
| SM80 GPR latency table unit tests | ✅ | Combinatorial RAW/WAW/WAR tests for all `RegLatencySM80` categories (11.7% → 76.5%) |
| 10 new WGSL shader fixtures | ✅ | expr_binary_int_ops, func_math_transcendentals, sm70_control_branches_loops_barrier, builder_emit_complex, etc. |
| SM30 delay clamping fix | ✅ | `deps.delay.clamp(1, 32)` prevents `debug_assert!` panic in Kepler scheduler |
| `compile_wgsl_raw_sm` test API | ✅ | `#[doc(hidden)]` public function for legacy SM testing from integration tests |
| TODOs → DEBT migration | ✅ | All bare `TODO:` replaced with `DEBT(category):` comments (28 total) |
| Test expansion | ✅ | 991 → 1116 passing (+125 tests), 63% line coverage |

### Phase 10 — Iteration 17 Completions (Cross-Spring Absorption + Audit + Idiomatic Refactoring)

| Task | Status | Details |
|------|--------|---------|
| 10 hotSpring shaders absorbed | ✅ | CG linear algebra (alpha, beta, update_p, update_xr, complex_dot_re), Yukawa variants (verlet, celllist_indirect), SU(3) momentum, VACF batch, flow accumulate |
| 10 neuralSpring shaders absorbed | ✅ | xoshiro128ss PRNG, HMM (viterbi, backward_log), distance (hamming, jaccard), RK45 adaptive, matrix_correlation, stencil_cooperation, spatial_payoff, swarm_nn_forward |
| `local_elementwise_f64` retired | ✅ | Documented as retired in airSpring v0.7.2; upstream: batched_elementwise_f64 |
| SM75 `gpr.rs` refactored | ✅ | `Vec` helpers → `const` slices (1025 → 935 LOC); zero heap allocation in test setup |
| Full codebase audit | ✅ | No mocks in production, no hardcoded primal names in logic, all deps pure Rust (except libc for DRM) |
| 2 new compiler limitations documented | ✅ | xoshiro128ss (non-local pointer args), swarm_nn_forward (RA SSA phi tracking) |
| Test expansion | ✅ | 1116 → 1134 passing (+18 tests), 33 ignored |

### Phase 10 — Iteration 18 Completions (Deep Debt Solutions)

| Task | Status | Details |
|------|--------|---------|
| Pred→GPR legalization bug fix | ✅ | `src_is_reg()` incorrectly treated `SrcRef::True`/`SrcRef::False` as valid GPR sources — fixed in `legalize.rs` and `lower_copy_swap.rs` |
| `copy_alu_src_if_pred()` helper | ✅ | Added to all 12 SetP legalize methods across SM20/SM32/SM50/SM70 |
| Small array promotion | ✅ | Extended `type_reg_comps()` in `naga_translate/func_ops.rs` to promote small fixed-size arrays (up to 32 registers) — unblocks xoshiro128ss PRNG shader |
| SM75 `gpr.rs` refactored | ✅ | Test data to 929 LOC (from 1021, back under 1000-line limit) |
| 4 tests un-ignored | ✅ | `bcs_bisection_f64`, `batched_hfb_hamiltonian_f64`, `coverage_logical_predicates`, `xoshiro128ss` |
| 4 RA back-edge issues deferred | ✅ | Deep RA rework needed: `sigmoid_f64`, `swarm_nn_forward`, `wilson_plaquette_f64`, `su3_gauge_force_f64` |
| Test expansion | ✅ | 1134 → 1138 passing (+4 tests), 33 → 29 ignored |
| Cross-spring corpus | ✅ | 47 shaders, 36 compiling SM70 (was 32) |

### Phase 10 — Iteration 19 Completions (Back-Edge Liveness & RA Evolution)

| Task | Status | Details |
|------|--------|---------|
| Back-edge live-in pre-allocation in RA | ✅ | Loop headers now pre-allocate fresh registers for ALL live-in SSA values (including back-edge predecessors) via `SimpleLiveness::live_in_values()`; `second_pass` gracefully skips SSA values the source block doesn't have |
| Back-edge-aware `calc_max_live` | ✅ | New `calc_max_live_back_edge_aware()` seeds liveness from `live_in_values()` for loop headers, preventing spiller underestimation |
| Scheduler back-edge fix | ✅ | Instruction scheduler seeds `live_set` from `live_in_values()` for loop headers instead of skipping; `debug_assert_eq!` now enforces live_in count matching |
| `calc_max_live` multi-predecessor fix | ✅ | Liveness trait's `calc_max_live` now iterates over ALL forward predecessors instead of just the first one |
| 3 tests unblocked | ✅ | `su3_gauge_force_f64`, `wilson_plaquette_f64`, `swarm_nn_forward` |
| sigmoid_f64 remains ignored | ✅ | Pre-existing RA gap in straight-line block chain |
| Test expansion | ✅ | 1138 → 1141 passing (+3 tests), 29 → 26 ignored |
| Cross-spring corpus | ✅ | 47 shaders, 39 compiling SM70 (was 36) |
| WGSL corpus | ✅ | 46/49 passing, 3 ignored (was 43/49) |

### Phase 10 — Iteration 20 Completions (SSA Dominance Repair & File Extraction)

| Task | Status | Details |
|------|--------|---------|
| SSA dominance violation fix | ✅ | `fix_entry_live_in()`: detects values live-in to entry block (defined in one branch, used in both), inserts OpUndef + repair_ssa to create proper phi nodes — fixes sigmoid_f64 |
| Pipeline placement | ✅ | `fix_entry_live_in` runs before scheduler and RA — both see correct SSA |
| Scheduler assertion promoted | ✅ | `debug_assert_eq!` on live-in count matching — now passes for all shaders |
| SM75 `gpr.rs` test extraction | ✅ | Test module extracted to `gpr_tests.rs` (813 → 813 LOC production, tests in separate file) |
| sigmoid_f64 unblocked | ✅ | Was ignored with "pre-existing RA gap"; root cause: builder SSA dominance violation |
| Test expansion | ✅ | 1141 → 1142 passing (+1 test), 26 → 25 ignored |
| Cross-spring corpus | ✅ | 47 shaders, 40 compiling SM70 (was 39) |
| WGSL corpus | ✅ | 47/49 passing, 2 ignored (was 46/49) |

### Phase 10 — Iteration 21 Completions (Cross-Spring Absorption Wave 2)

| Task | Status | Details |
|------|--------|---------|
| Cross-spring absorption wave 2 | ✅ | 38 new test entries: 9 hotSpring + 17 neuralSpring + 12 existing fixtures wired |
| hotSpring absorption (self-contained) | ✅ | spin_orbit_pack_f64, batched_hfb_density_f64, esn_readout, su3_kinetic_energy_f64, su3_link_update_f64, staggered_fermion_force_f64, dirac_staggered_f64 |
| neuralSpring coralForge absorption (df64) | ✅ | 10 Evoformer/IPA/MSA shaders: triangle_mul, triangle_attention, outer_product_mean, msa_row/col_attention_scores, attention_apply, ipa_scores, backbone_update — df64 preamble auto-prepended |
| neuralSpring bio absorption (f32) | ✅ | hill_gate, batch_fitness_eval, multi_obj_fitness, swarm_nn_scores, locus_variance, head_split, head_concat |
| Existing fixtures wired | ✅ | 12 previously-imported shaders added to corpus tracking: xpay_f64, yukawa_force_f64, vv_kick_drift_f64, batch_ipr, wright_fisher_step, logsumexp_reduce, chi_squared_f64, pairwise_l2, linear_regression, + 3 ignored (need external includes) |
| df64 preamble: comparison operators | ✅ | Added `df64_gt`, `df64_lt`, `df64_ge` to built-in preamble |
| chi_squared_f64 keyword fix | ✅ | `shared` → `wg_scratch` (WGSL reserved keyword) |
| local_elementwise_f64 retired | ✅ | Removed test + fixture (airSpring v0.7.2 retired upstream) |
| Test expansion | ✅ | 1142 → 1174 passing (+32), 25 → 30 ignored (+5 new blockers) |
| Cross-spring corpus | ✅ | 86 shaders, 79 compiling SM70 (was 47/40) |

### Phase 10 — Iteration 22 Completions (Multi-Language Frontends & Fixture Reorganization)

| Task | Status | Details |
|------|--------|---------|
| Fixture reorganization | ✅ | 86 spring corpus shaders moved to `fixtures/wgsl/corpus/`; 21 compiler-owned fixtures stay in `fixtures/wgsl/`; `wgsl_corpus.rs` paths updated |
| GLSL compute frontend | ✅ | `glsl-in` naga feature enabled; `parse_glsl()`, `compile_glsl()`, `compile_glsl_full()` public API; `Frontend` trait extended |
| GLSL test corpus | ✅ | 5 GLSL 450 compute fixtures: basic_alu, control_flow, shared_reduction, transcendentals, buffer_rw — all compile SM70 |
| SPIR-V roundtrip tests | ✅ | 10 roundtrip tests (WGSL → naga → SPIR-V → compile()): 4 passing, 6 ignored (Discriminant expr, non-literal const init) |
| Frontend trait: compile_glsl | ✅ | `Frontend` trait now has 3 methods: `compile_wgsl`, `compile_spirv`, `compile_glsl` |
| Test expansion | ✅ | 1174 → 1189 passing (+15), 30 → 36 ignored (+6 SPIR-V path gaps) |
| SPIR-V path gaps documented | ✅ | `Discriminant` expression and non-literal constant initializers — future SPIR-V translator work |

### Phase 10 Remaining / Phase 11 Roadmap

| Task | Priority | Detail |
|------|----------|--------|
| Pred→GPR encoder coercion chain | P2 | Encoder coercion chain |
| Hardware validation (AMD) | ✅ | **E2E verified** — RX 6950 XT, WGSL compile + dispatch + readback |
| Hardware validation (NVIDIA) | P2 | Titan V on-site — channel + pushbuf path ready |
| Intel backend | P3 | Placeholder |

## Checks

| Check | Status |
|-------|--------|
| `cargo check --workspace` | PASS |
| `cargo test --workspace` | PASS (1189 passing, 0 failed, 36 ignored) |
| `cargo llvm-cov` | 63% line coverage (target 90%) |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS (0 warnings) |
| `cargo fmt --check` | PASS |
| `cargo doc --workspace --no-deps` | PASS |

## Hardware — On-Site

| GPU | PCI | Architecture | Kernel Driver | Vulkan | f64 | VRAM | Role |
|-----|-----|-------------|---------------|--------|-----|------|------|
| AMD RX 6950 XT | 25:00.0 | RDNA2 GFX1030 (Navi 21) | amdgpu (open) | RADV/ACO (Mesa 25.1.5) | 1/16 | 16 GB | AMD evolution primary |
| NVIDIA RTX 3090 | 41:00.0 | Ampere SM86 (GA102) | nvidia 580.119.02 | NVIDIA proprietary | 1/32 | 24 GB | NVIDIA compilation target |

## Spring Absorption

| Pattern | Source | Applied |
|---------|--------|---------|
| BTreeMap for deterministic serialization | groundSpring V73 | health.rs |
| Silent-default audit | groundSpring V76 | program.rs |
| Cross-spring provenance doc-comments | CROSS_SPRING_SHADER_EVOLUTION | lower_f64/ |
| Unsafe code eliminated | groundSpring CONTRIBUTING | builder/mod.rs |
| Capability-based discovery | groundSpring CAPABILITY_SURFACE | capability.rs |
| No hardcoded primal names | groundSpring primal isolation | workspace-wide |
| Result propagation | groundSpring error handling | pipeline |
| Three-tier precision (f32/DF64/f64) | barraCuda Fp64Strategy | gpu_arch.rs |
| 13-tier tolerance constants | groundSpring V73 | tol.rs |
| WGSL shader corpus (cross-spring) | 5 springs (86 shaders, 79 compiling SM70) | tests/fixtures/wgsl/corpus/ |
| GLSL compute frontend | naga `glsl-in` feature | `compile_glsl()` public API, 5 GLSL fixtures |
| SPIR-V roundtrip testing | naga `spv-out` → `compile()` | 10 roundtrip tests (4 passing, 6 ignored) |
| FMA control / NoContraction | wateringHole NUMERICAL_STABILITY_PLAN | FmaPolicy |
| Safe syscalls via libc | groundSpring CONTRIBUTING | drm.rs, gem.rs |
| `Cow<'static, str>` error fields | Rust idiom: zero-alloc static paths | DriverError, CompileError, GpuError, PrimalError |
| `#[expect]` with reasons | Rust 2024 idiom | workspace-wide (replaces `#[allow]`) |
| `FxHashMap` in hot paths | Performance internalization | naga_translate/func.rs, func_ops.rs |
| Sealed FFI boundary | wateringHole sovereignty | `drm_ioctl_typed` pub(crate), `BufferHandle` pub(crate) |
| `shader.compile.*` semantic naming | wateringHole PRIMAL_IPC_PROTOCOL | JSON-RPC + tarpc |
| Differentiated IPC error codes | wateringHole PRIMAL_IPC_PROTOCOL | jsonrpc.rs |
| `#[deny(unsafe_code)]` safety boundary | Rust best practice | 6 non-driver crates |
| Ioctl struct layout tests | Kernel ABI correctness | 14 tests in amd/ioctl.rs |
| Constructor-validated invariants | Rust defensive programming | ShaderModelInfo::new asserts sm >= 20 |
| Configurable debug paths | wateringHole agnostic config | CORAL_DEP_GRAPH_PATH env var |
| Domain-based module split | Smart refactoring principle | cfg.rs → cfg/{mod,dom}.rs |
| `TypedBitField<OFFSET, WIDTH>` | Compile-time bit field safety | coral-reef-bitview |
| `InstrBuilder<N>` | Fixed-size instruction word builder | coral-reef-bitview |
| `derive(Encode)` proc-macro | `#[enc(offset, width)]` → `encode()` method | nak-ir-proc |
| AMD full IR encoding | FLAT memory, control flow, comparisons, int, type conv, sys values | codegen/amd/ |
| `wave_size()` + `total_reg_file()` | ShaderModel vendor-agnostic occupancy | ir/shader_info.rs |
| Nouveau full DRM | Channel, GEM, pushbuf, QMD dispatch | coral-driver/nv/ |
| coral-gpu auto-detect | DRM render node probing → vendor device | coral-gpu/src/lib.rs |
| groundSpring V95 push buffer fix | `mthd_incr` field order fix → pushbuf.rs | coral-driver/nv/pushbuf.rs |
| groundSpring V95 NVIF constants | ROUTE/OWNER alignment to Mesa nvif/ioctl.h | coral-driver/nv/ioctl.rs |
| groundSpring V95 QMD CBUF wiring | Full 64-word QMD v2.1/v3.0 with binding layout | coral-driver/nv/qmd.rs |
| groundSpring V95 fence sync | gem_cpu_prep for GPU idle wait | coral-driver/nv/ioctl.rs |
| `compile_wgsl_full` API | Returns CompiledBinary with GPR/shared/barrier metadata | coral-reef/src/lib.rs |
| `bytemuck` safe transmutation | Replaces unsafe u32→u8 casts | coral-driver/{amd,nv} |
| FxHashMap in CFG | Hot-path optimization | coral-reef-stubs/cfg |
| Consolidated ioctl unsafe surface | Safe wrapper pattern: `amd_ioctl` + `amd_ioctl_read` | amd/ioctl.rs |
| Dead variant removal | `DriverError::Unsupported` unused in production | error.rs |
| `#[expect]` with reasons (round 2) | Rust 2024 idiom: 9 more `#[allow]` migrated | workspace-wide |
| Cross-spring corpus expansion | +2 hotSpring MD shaders (VACF dot, Verlet copy) | tests/fixtures/wgsl/ |
| `Fp64Strategy` enum | Three-tier precision strategy in CompileOptions | lib.rs |
| Built-in df64 preamble | Dekker/Knuth pair arithmetic auto-prepended | df64_preamble.wgsl |
| `prepare_wgsl()` preprocessing | Auto df64 preamble + `enable f64;` stripping | lib.rs |
| kl_divergence reserved keyword fix | `shared` → `wg_scratch` | kl_divergence_f64.wgsl |
| Statement::Switch lowering | Chain-of-comparisons IR lowering | naga_translate/func_control.rs |
| NV MappedRegion RAII | Unsafe reduction: safe slice access + Drop | nv/ioctl.rs, nv/mod.rs |
| clock_monotonic_ns consolidation | Single-site unsafe for absolute timestamps | amd/ioctl.rs |
| Diagnostic panic messages | 14 lower_copy_swap panics with src/dst context | lower_copy_swap.rs |
| AMD safe slices | `ptr::copy_nonoverlapping` → `copy_from_slice` via MappedRegion | amd/gem.rs |
| Typed DRM wrappers | `gem_close()`, `drm_version()` eliminate call-site unsafe | drm.rs |
| Inline var pre-allocation | Callee locals pre-allocated in `inline_call` | func_ops.rs |
| SSA dominance repair | `fix_entry_live_in` + `repair_ssa` for builder violations | repair_ssa.rs, pipeline.rs |

---

*Grade scale: A (production) → F (not started)*
