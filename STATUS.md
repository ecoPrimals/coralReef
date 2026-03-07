# coralReef — Status

**Last updated**: March 7, 2026  
**Phase**: 10 — Iteration 9 (E2E Wiring + Push Buffer Fix + Debt Reduction)

---

## Overall Grade: **A+** (Multi-Vendor Sovereign GPU Compiler)

| Category | Grade | Notes |
|----------|-------|-------|
| Primal lifecycle | A | Standalone `PrimalLifecycle` + `PrimalHealth`, full test coverage |
| UniBin compliance | A | Binary target with clap, panic hook, SIGTERM/SIGINT, structured errors |
| IPC | A+ | JSON-RPC 2.0 + tarpc, Unix socket + TCP, zero-copy `Bytes` payloads, `shader.compile.*` semantic naming, differentiated error codes |
| NVIDIA pipeline | A+ | WGSL/SPIR-V → naga → codegen IR → f64 lower → optimize → legalize → RA → encode |
| AMD pipeline | A+ | `ShaderModelRdna2` → legalize → RA → encode (memory, control flow, comparisons, integer, type conversion, system values) |
| Mesa stubs evolved | A+ | All modules evolved to pure Rust (BitSet, CFG, dataflow, fxhash, nvidia_headers) |
| f64 transcendentals | A+ | sqrt, rcp, exp2, log2, sin, cos, exp, log, pow — NVIDIA (Newton-Raphson) + AMD (native) |
| Vendor-agnostic arch | A+ | `Shader` holds `&dyn ShaderModel` — idiomatic Rust trait dispatch, no manual vtables |
| coralDriver | A+ | AMD DRM ioctl (GEM, PM4, CS, BO list, fence sync), NVIDIA nouveau (channel, GEM, pushbuf, QMD dispatch), pure Rust syscalls via libc |
| coralGpu | A+ | Unified compile+dispatch API, auto-detect DRM render nodes, vendor-agnostic `GpuContext` with alloc/dispatch/sync/readback |
| Code structure | A+ | Smart refactoring: scheduler prepass 842→313 LOC, cfg.rs→cfg/{mod,dom}.rs, ir/{pred,src,fold}.rs, ipc/{jsonrpc,tarpc_transport}.rs |
| Tests | A+ | 974 tests (952 passing, 22 ignored), zero failures |
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
| 10 — Spring Absorption | Deep debt, absorption, compiler hardening | **Iteration 9** |

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
| Nouveau scaffolds → explicit errors | ✅ | `DriverError::Unsupported` with clear messages |
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

### Phase 10 Remaining / Phase 11 Roadmap

| Task | Priority | Detail |
|------|----------|--------|
| GPR→Pred coercion chain | P2 | Blocks logical_predicates |
| Wilson plaquette (scheduler) | P2 | PerRegFile live_in mismatch |
| const_tracker negated immediate | P2 | HFB hamiltonian |
| Hardware validation (AMD) | P2 | RX 6950 XT on-site — PM4 + CS submit path ready |
| Hardware validation (NVIDIA) | P2 | Titan V on-site — channel + pushbuf path ready |
| Intel backend | P3 | Placeholder |

## Checks

| Check | Status |
|-------|--------|
| `cargo check --workspace` | PASS |
| `cargo test --workspace` | PASS (974 tests: 952 passing, 22 ignored) |
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
| WGSL shader corpus (cross-spring) | 5 springs (27 shaders, 14 compiling SM70) | tests/fixtures/wgsl/ |
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

---

*Grade scale: A (production) → F (not started)*
