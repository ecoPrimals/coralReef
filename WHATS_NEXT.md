# coralReef — What's Next

**Last updated**: March 16, 2026 (Phase 10 — Iteration 52)

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

## Phase 10 — Spring Absorption + Compiler Hardening (Iteration 52)

### Iteration 51 — Deep Audit Compliance + IPC Health + Doc Hygiene
- [x] wateringHole IPC health methods: `health.check`, `health.liveness`, `health.readiness` across JSON-RPC, tarpc, Unix socket
- [x] Socket path standard: `ECOSYSTEM_NAMESPACE` → `"biomeos"`, `primal_socket_name()` → `<primal>-<family_id>.sock`
- [x] Config self-knowledge: `PRIMAL_NAME`, `PRIMAL_VERSION` via `env!()`, `family_id()` from `$BIOMEOS_FAMILY_ID`
- [x] Zero-copy transport: `Bytes::copy_from_slice` → `buf.drain()` + `Bytes::from(buf)` in RPC client
- [x] `coral-gpu` smart refactor: `lib.rs` 977 LOC → 6 submodules (65 LOC lib.rs)
- [x] `// SAFETY:` comments on all `unsafe impl Send/Sync` in coral-driver
- [x] genomeBin manifest: `pie_verified = true`, `ecobin_grade = "A++"`
- [x] E2E IPC test: JSON-RPC + tarpc full semantic method validation
- [x] Clippy pedantic: 8+ additional lint categories resolved
- [x] 2157 passing, 0 failed; 57.28% region / 57.71% line / 67.98% function coverage

### Iteration 50 — Full Audit Execution + Coverage Expansion
- [x] Doc warnings eliminated (4 rustdoc warnings: escaped bit-field notation, fixed intra-doc links)
- [x] Clippy clean with VFIO features (`cargo clippy --workspace --features vfio -- -D warnings` — zero warnings)
- [x] Hardcoded `/home/biomegate` paths → `$HOTSPRING_DATA_DIR` env var
- [x] Production `unwrap()`/`expect()` evolved to `match`/`let-else` + `tracing::error!` in coral-glowplug
- [x] All production `eprintln!` → structured `tracing::*` macros
- [x] Smart refactoring: `devinit.rs` (2197→5), `probe.rs` (1572→6), `glowplug.rs` (1405→6), `hbm2_training.rs` (1355→10), `hw_nv_vfio.rs` (2469→5), `tests_unix.rs` (1094→2)
- [x] All files under 1000 LOC — zero violations
- [x] 16 experimental VFIO diagnostic modules annotated with `#![allow(missing_docs)]`
- [x] +214 coverage tests: texture, memory, f64, shader I/O, control flow, spiller, latency, fold across SM20/32/50/70/75/80/86/89 + RDNA2
- [x] GPU hardware tests: 9/14 nouveau passing on Titan V
- [x] Root docs updated: README.md, STATUS.md, CONTRIBUTING.md, hardware inventory
- [x] 1992 passing, 0 failed; 57.10% region / 57.54% line / 67.80% function coverage

### Iteration 49 — hotSpring Absorption: GV100 Dispatch Fixes
- [x] **GV100 per-runlist registers**: all RUNLIST_BASE/SUBMIT migrated from GK104 global constants to GV100 per-runlist at stride 0x10 (`runlist_base(id)`, `runlist_submit(id)`). Value encoding: base = `lower_32(iova >> 12)`, submit = `upper_32(iova >> 12) | (count << 16)`
- [x] **MMU fault buffer DMA**: interpreter probe L3 fault buffers migrated from VRAM (`PraminRegion`) to DMA system memory (`DmaBuffer` at `FAULT_BUF_IOVA`). Both BUF0+BUF1 configured with PUT enable bit
- [x] **PFIFO INTR bit 8**: new `INTR_BIT8` constant + `clear_pfifo_intr_bit8()` on `ExperimentContext`. Z experiment (full_pfifo_reinit) clears bit 8 before checking bit 30 completion
- [x] **PBDMA reset sequence**: `reset_pbdma()` method: clear PBDMA INTR+HCE, clear PCCSR faults, toggle PMC PBDMA enable bit. Applied pre-dispatch in experiments N (full_dispatch_with_inst_bind) and P (scheduled_plus_direct_pbdma)
- [x] **GlowPlug consolidation**: ~100 LOC inline glow plug warming in `diagnostic_matrix()` replaced with `GlowPlug::check_state()` + `GlowPlug::full_init()`. Runner.rs reduced by ~90 LOC
- [x] **`submit_runlist()` helper**: `ExperimentContext::submit_runlist()` encapsulates per-runlist register writes — 20+ call sites migrated from inline `pfifo::RUNLIST_BASE`/`SUBMIT` pairs
- [x] **GV100 register tests**: `runlist_gv100_register_addresses` + `runlist_gv100_value_encoding` unit tests validate stride and value format
- [x] All CI gates pass: `fmt`, `clippy`, `test` — 1842+ passing, 0 failures

### Iteration 48 — Deep Debt Solutions + Sovereignty Evolution
- [x] `extern "C" { fn ioctl }` eliminated — `raw_nv_ioctl` → `nv_rm_ioctl` via `rustix::ioctl` through `drm_ioctl_named`; zero C FFI remaining in entire workspace
- [x] Clippy `items_after_test_module` + `needless_range_loop` in `rm_helpers.rs` — idiomatic iterator patterns
- [x] Formatting drift resolved workspace-wide (`context.rs`, `layers.rs`, `probe.rs`, `rm_helpers.rs`, `rm_client.rs`)
- [x] Last 2 production `unwrap()` → `expect()` with context (runner.rs DMA buffer verification)
- [x] Capability test evolved — hardcoded primal name list → structural self-knowledge assertions
- [x] +23 new tests: Unix JSON-RPC dispatch (15), main.rs coverage (8)
- [x] 1842 passing, 0 failed, 61 ignored; 66.67% region / 68.45% line coverage

### Iteration 47 — Deep Debt Evolution + Modern Idiomatic Rust
- [x] `runner.rs` delegate to `experiments::run_experiment()` — 2509 LOC → 778 LOC (eliminated duplicated inline experiment dispatch)
- [x] Unsafe code elimination — `unsafe { from_raw_parts_mut }` → safe `as_mut_slice()` in NOP pushbuf init
- [x] `rm_client.rs` extract helpers — UUID parsing + raw ioctl → `rm_helpers.rs` (1000 → 944 LOC)
- [x] Zero-copy evolution — `KernelCacheEntry.binary: Vec<u8>` → `Bytes` (eliminates copy in `to_cache_entry`/`from_cache_entry`)
- [x] Driver string centralization — `DRIVER_VFIO`/`DRIVER_NOUVEAU`/`DRIVER_AMDGPU`/`DRIVER_NVIDIA_DRM` constants in `preference.rs`; all match arms use constants
- [x] Production panic elimination — 6 `panic!()` in `sm70_instr_latencies.rs` → `warn!` + `DEFAULT_LATENCY` / `debug_assert!`
- [x] Production unwrap elimination — `runner.rs` unwrap → `Option::zip` pattern
- [x] FenceTimeout constant — hardcoded `5000` → `SYNC_TIMEOUT.as_millis()`
- [x] +15 new tests: rm_helpers UUID parsing (9), cache entry roundtrip (3), driver constants (2), zero-copy clone (1)
- [x] 1819 passing, 0 failed, 61 ignored

### Iteration 46 — Structural Refactor + Coverage Expansion
- [x] `diagnostic/runner.rs` smart refactor: 2485 LOC → 769 LOC — split into `experiments/` submodule with 8 handler files + context struct
- [x] Clippy pedantic workspace-wide: all warnings resolved (identity ops, constant assertions, redundant closures, range contains, etc.)
- [x] 53+ new tests: AMD ISA generated table lookup (25), Unix JSON-RPC coverage (8), SM70 latency/encoder tests (20)
- [x] Coverage: 65.90% → 66.43% lines, 73.75% → 75.15% functions, 68.21% regions
- [x] Zero files over 1000 lines
- [x] 1804 passing, 0 failed, 61 ignored

### Iteration 45 — Deep Audit + Refactor + Coverage Expansion
- [x] Full codebase audit: specs, wateringHole standards, UniBin/ecoBin/genomeBin compliance
- [x] `vfio/channel.rs` smart refactor (2894 LOC → 5 modules: mod.rs, registers.rs, page_tables.rs, pfifo.rs, diagnostic.rs — all production files under 1000 LOC)
- [x] `eprintln!` → `tracing` migration in production code (pfifo.rs, mod.rs, vfio_compute.rs, device.rs)
- [x] IPC chaos/fault tests: concurrent JSON-RPC, malformed requests, rapid connect/disconnect, oversized payloads, concurrent tarpc, invalid methods
- [x] 30+ new unit tests across coralreef-core (config, health, lifecycle, capability) and coral-driver (error, qmd, pushbuf, pm4, identity, knowledge)
- [x] 5 ignored doctests fixed (coral-gpu, coral-reef, coral-reef-isa, nak-ir-proc)
- [x] Unsafe evolution: `// SAFETY:` comments on all unsafe blocks, null checks for mmap, assert over debug_assert for DMA slices
- [x] Clippy pedantic: `map_unwrap_or` → `map_or`, `identity_op` resolved, `cast_possible_truncation` with `#[expect]`
- [x] STATUS.md and COMPILATION_DEBT_REPORT.md updated with iteration 45 metrics

### Iteration 44 — USERD_TARGET + INST_TARGET Runlist Fix
- [x] Runlist DW0: `USERD_TARGET` bits (3:2) set to SYS_MEM_COHERENT (2) — PBDMA reads USERD from system memory
- [x] Runlist DW2: `INST_TARGET` bits (5:4) set to SYS_MEM_NCOH (3) — PBDMA locates instance block in system memory
- [x] Replaced literal PFIFO register addresses with named constants (`pfifo::RUNLIST_BASE`, `pfifo::RUNLIST`)
- [x] Removed unused constants (`pmc::PBDMA_ENABLE`, `pmc::PBDMA_INTR_EN`)
- [x] Clippy clean: `identity_op`, `assertions_on_constants`, `unused_variables` all resolved
- [x] Tests: `runlist_chan_dw0_userd_target` + `runlist_chan_dw2_inst_target` validate target bit encoding
- [x] Root cause: hotSpring HW debug traced FenceTimeout to PBDMA unable to read USERD page (target=VRAM instead of SYS_MEM)

### Iteration 43 — PFIFO Channel Init + Cross-Primal Rewire
- [x] PFIFO hardware channel creation via BAR0 MMIO (`vfio/channel.rs`)
- [x] V2 MMU 5-level page tables (PD3→PD2→PD1→PD0→PT) with identity-mapped 2 MiB IOVA
- [x] RAMFC population: GPFIFO base, USERD ptr, channel signature, engine config
- [x] TSG+channel runlist construction and submission via PFIFO registers
- [x] PCCSR channel bind/enable for Volta+ GPUs
- [x] RAMUSERD offset correction: GP_GET at 0x88, GP_PUT at 0x8C (per `dev_ram.ref.txt`)
- [x] USERMODE doorbell: NV_USERMODE_NOTIFY_CHANNEL_PENDING at BAR0+0x810090
- [x] Subcontext PDB setup (SC_PDB_VALID(0) + SC_PAGE_DIR_BASE(0))
- [x] toadStool S150-S152 evolution acknowledged (12 software gaps resolved)
- [x] barraCuda VFIO-primary wiring acknowledged (dispatch_binary/dispatch_kernel)
- [x] 12 new channel unit tests (PDE/PTE encoding, register offsets, IOVA layout)

### Iteration 42 — VFIO Sync + barraCuda API
- [x] VFIO `sync()` — proper GPFIFO GP_GET polling from USERD DMA page (volatile read, spin-loop, 5s timeout)
- [x] USERD GP_PUT write in `submit_pushbuf()` — GPU tracks put pointer for completion detection
- [x] `GpuContext::from_vfio(bdf)` — public convenience API, auto SM detection, unblocks barraCuda
- [x] `GpuContext::from_vfio_with_sm(bdf, sm)` — explicit SM override for testing
- [x] Named constants: `userd::GP_PUT_OFFSET`, `userd::GP_GET_OFFSET`, `SYNC_TIMEOUT`, `POLL_INTERVAL`

### Iteration 41 — VFIO Sovereign GPU Dispatch
- [x] VFIO core module: types, ioctls, DMA buffer, VfioDevice
- [x] NvVfioComputeDevice: full ComputeDevice impl via BAR0/DMA
- [x] Feature gate: `--features vfio` on coral-driver and coral-gpu
- [x] DriverPreference: `vfio` > `nouveau` > `amdgpu` > `nvidia-drm`
- [x] VFIO GPU discovery: sysfs scan for vfio-pci NVIDIA devices
- [x] 35 new unit tests, 5 ignored HW integration tests
- [x] wateringHole handoff: toadStool hardware contract for VFIO setup

### Iteration 40 (previous)

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
- [ ] RTX 3090 (SM86) UVM dispatch pipeline code-complete (GPFIFO + USERD doorbell + completion polling); `NvDrmDevice` delegates to `NvUvmComputeDevice` — needs on-site hardware validation
- [x] **RX 6950 XT (GFX1030) E2E verified** — WGSL compile → PM4 dispatch → readback → verified `out[0] = 42u` — resolved Iteration 10

### P0 — AMD E2E critical fixes (Iteration 10)
- [x] **CS_W32_EN wave32 dispatch** — `DISPATCH_INITIATOR` bit 15 not set → wave64 mode → only 4 VGPRs allocated (v0-v3), v4+ unmapped
- [x] **SrcEncoding literal DWORD emission** — `src_to_encoding` returned SRC0=255 for `Imm32` values without appending literal DWORD → FLAT store consumed as "literal", instruction stream corrupted
- [x] **Inline constant range** — Full RDNA2 map: 128=0, 129–192=1..64, 193–208=-1..-16; `SrcEncoding` struct bundles SRC0 + optional literal
- [x] **64-bit address pair for FLAT** — `func_mem.rs` passed `addr[0].into()` (only addr_lo) → DCE eliminated addr_hi → corrupted 64-bit address; fixed to `addr.clone().into()`
- [x] **`unwrap_or(0)` audit** — register index, branch offset, FLAT offset overflow: all return `CompileError` instead of silent truncation

### P2 — barraCuda integration
- [x] `GpuContext::from_vfio()` convenience API — unblocks `CoralReefDevice::from_vfio_device()` stub in barraCuda
- [ ] `CoralReefDevice` fully wired (barraCuda-side: currently stub, needs `from_vfio` call)
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
- [x] Iteration 26: hotSpring sovereign pipeline unblock — f64 min/max/abs/clamp via DSetP+Sel (batched_hfb_energy_f64 unblocked), `ComputeDevice: Send + Sync` for thread-safe GpuBackend, nouveau compute subchannel binding (SM-aware class selection), docs updated — 1286 tests (1286 passing, 59 ignored)
- [x] Iteration 27: Deep debt + cross-spring absorption — RDNA2 literal materialization pass (V_MOV_B32 prefix for VOP3/VOP2 literals), f64 transcendental AMD encodings (F64Exp2/Log2/Sin/Cos via V_CVT_F32_F64+VOP1+V_CVT_F64_F32), f32 transcendental encoding (OpTranscendental→VOP1), OpShl/OpShr/OpSel non-VGPR fix, AMD SR 0x28–0x2D mapping, strip_f64_enable absorption, hotSpring FMA shaders (su3_link_update, wilson_plaquette), FMA policy plumbing, f64 discovery manifest, PRNG preamble, neuralSpring shaders (logsumexp, rk45_step, wright_fisher), f64 runtime diagnostic, 24/24 spring absorption tests on SM70+RDNA2 — 1401 tests (1401 passing, 62 ignored)
- [x] Iteration 29: NVIDIA last mile — multi-GPU path-based open (AmdDevice/NvDevice/NvDrmDevice::open_path), enumerate_all fix (4× RTX 3050 → 4 contexts), from_descriptor_with_path, Nouveau EINVAL diagnostics (diagnose_channel_alloc, dump_channel_alloc_hex, check_nouveau_firmware), GPU identity via sysfs (probe_gpu_identity, GpuIdentity::nvidia_sm), buffer lifecycle safety (NvDevice.inflight), SM auto-detect, coral-gpu SM wiring, UVM RM client PoC, 5 hw_nv_nouveau diagnostic tests, gem_close promoted to pub — 1447 tests (1447 passing, 76 ignored)
- [x] Iteration 30: Spring absorption + FMA evolution — `shader.compile.wgsl.multi` API (multi-device cross-vendor compilation in single request), FMA contraction enforcement (`lower_fma.rs` pass: `FmaPolicy::Separate` splits FFma→FMul+FAdd), FMA hardware capability reporting (`FmaCapability::for_target()`), `PCIe` topology awareness (`probe_pcie_topology()`, switch grouping), capability self-description evolution (`shader.compile.multi` + FMA policies + expanded arch list), NVVM bypass test hardening, `primal-rpc-client` evolution, `#![warn(missing_docs)]` expansion to all crates, `coral-driver` doc + identity extraction — 1487 tests (1487 passing, 76 ignored)

- [x] Iteration 31: Deep debt + NVIDIA pipeline fixes — repair_ssa unreachable block elimination + critical edge phi handling, f64 log2 pow-lowering fix, AMD FRnd encoding (VOP1 F32 + VOP3 F64), vec3<f64> SM70 scalarization, SU3 lattice preamble (10 functions + auto-prepend), SPIR-V Relational expressions (IsNan/IsInf/All/Any), non-literal const init (Compose/Splat/recursive), emit_f64_cmp widening, multi_gpu test generalized, **Nouveau new UAPI** (`VM_INIT/VM_BIND/EXEC` struct defs + ioctl wrappers), **UVM device alloc fix** (`Nv0080AllocParams` with `device_id` — root-causes 0x1F from hotSpring Exp 051), RM status constants, production unwrap→expect — 1509 tests (1509 passing, 54 ignored)

- [x] Iteration 32: Deep debt evolution — `firstTrailingBit` implementation (clz(reverseBits(x)) via OpBRev+OpFlo, NV+AMD), `distance` implementation (length(a-b), NV+AMD), AMD `OpBRev`/`OpFlo` encoding (V_BFREV_B32, V_FFBH_U32/I32 — closes discriminant 31 gap), `CallResult` OpUndef→CompileError, `BindingArray` stride fix (hardcoded 1→recursive element stride), `shader_info.rs` smart refactor (814→3 files: shader_io/shader_model/shader_info), production mock audit (all test-only), dependency analysis (26/28 pure Rust), 19 new integration tests (mix/step/smoothstep/sign, tan/atan/atan2/asin/acos, exp/log/tanh/sinh/cosh, atomics, builtins, float modulo, uniform matrix), doc updates — 1556 tests (1556 passing, 54 ignored), 64% coverage

- [x] Iteration 33: NVVM poisoning validation — sovereign compilation of hotSpring DF64 Yukawa force shader (`exp_df64` + `sqrt_df64`) verified for SM70/SM86/RDNA2. The exact shader that permanently kills NVIDIA proprietary wgpu devices compiles cleanly through coralReef. 6 new tests in `nvvm_poisoning_validation.rs` (full Yukawa DF64, isolated transcendentals, Verlet integrator). This is the 4-8x throughput unlock for hotSpring's 12.4x Kokkos gap — eliminates native f64 fallback on Ampere. Handoff to hotSpring/barraCuda/toadStool — 1562 tests (1562 passing, 54 ignored)

- [x] Iteration 34: Deep debt evolution — smart refactor `legalize.rs` (772 LOC → `legalize/mod.rs` + `legalize/helpers.rs`, clean engine/API separation), `bytemuck::bytes_of` unsafe elimination in `diag.rs` (Pod+Zeroable derives on NouveauSubchan/NouveauChannelAlloc), `drm_ioctl_named` for new UAPI wrappers (informative error messages), 34 targeted naga_translate unit tests (exp/log/pow, sinh/cosh/tanh/asinh/acosh/atanh, sqrt/inverseSqrt, ceil/round/trunc/fract, dot/cross/length/normalize/distance, countOneBits/reverseBits/firstLeadingBit/countLeadingZeros/firstTrailingBit, fma/sign/mix/step/smoothstep, min/max, builtins), SM89 DF64 validation (3 tests: Yukawa, transcendentals, Verlet for Ada Lovelace sovereign path), 5 deformed HFB shaders absorbed from hotSpring (9 passing, 1 ignored RDNA2 encoding gap), `quick-xml` 0.37→0.39 with API migration — 1608 tests (1608 passing, 55 ignored)

- [x] Iteration 35: `FirmwareInventory` + ioctl evolution — structured firmware probe for PMU/GSP (absorbs hwLearn pattern from toadStool handoff), `compute_viable()` reports dispatch viability, `compute_blockers()` lists missing firmware components. All `drm_ioctl_typed` calls migrated to `drm_ioctl_named` (7 calls, operation-specific error messages), dead `drm_ioctl_typed` function removed. 24 unsafe blocks (down from 29). `FirmwareInventory`/`FwStatus`/`firmware_inventory` publicly accessible via `nv::ioctl` — 1616 tests (1616 passing, 55 ignored)

- [x] Iteration 37: Gap closure + deep debt evolution — `bytemuck::Zeroable` on 5 UVM structs (eliminates `unsafe { zeroed() }`), PCI vendor constants centralized (`PCI_VENDOR_NVIDIA`/`AMD`/`INTEL`), AMD architecture auto-detection (`GpuIdentity::amd_arch()`), `raw_nv_ioctl` helper extraction, compute class constant unification (pushbuf re-exports from uvm), `NV_STATUS` documented (`nv_status` module), `uvm.rs` smart-refactored (727 LOC → 3 files), GPFIFO submission (`submit_gpfifo()` + USERD doorbell + `poll_gpfifo_completion()`), `NvDrmDevice` evolved to delegator (`Option<NvUvmComputeDevice>`), `KernelCacheEntry` (serde-derived cache struct), `GpuContext::dispatch_precompiled()`, `GpuTarget::arch_name()` — 1635 tests (1635 passing, 63 ignored)

- [x] Iteration 38: Deep debt solutions + idiomatic evolution — `cargo fmt` drift resolved (~10 files), 6 clippy fixes (`ExternalMapping` struct, `RmAllocEvent` struct, `KernelCacheEntry` param, redundant closures → method refs, collapsible if → let-chain), 4 intra-doc links fixed, `naga_translate_tests.rs` smart-refactored (1486 LOC → 3 domain files), `rm_client.rs` under 1000 LOC (`rm_status_name` → `nv_status::status_name`, `rm_client_tests.rs` extracted), `op_conv.rs` test extraction (1047 → 796 LOC), `// SAFETY:` comments on all unsafe blocks, zero-copy `primal-rpc-client` (`Vec<u8>` → `bytes::Bytes`), 22 new tests (15 `unix_jsonrpc` + 7 `op_conv`), zero files over 1000 LOC — 1657 tests (1657 passing, 63 ignored)
- [x] Iteration 40: FECS GR context init (Gap 3 — critical blocker addressed), UVM CBUF descriptor alignment (Gap 2 — consistency fix), Unsafe evolution (SAFETY comments, safe copy_from_slice), hotSpring dispatch fixes absorbed (commit a691023), Test coverage +10 (total 1669) — 1669 tests (1669 passing, 64 ignored)

### P3 — Remaining gaps (sovereign pipeline)
- [x] ~~f64 min/max/clamp broken for f64 (used a[0] truncating to f32)~~ — fixed Iteration 26
- [x] ~~ComputeDevice not Send + Sync~~ — fixed Iteration 26
- [x] **DRM ioctl struct ABI fixes** — 4 mismatches resolved (Exp 057): VM_INIT size, EXEC field order, VM_BIND field order, ChannelAlloc/Free padding. VM_INIT now succeeds on Titan V.
- [ ] **Wire new UAPI into NvDevice::open_from_drm** — replace legacy `create_channel` with `vm_init→gem_new→vm_bind→exec` (ioctls ready)
- [ ] **Titan V nouveau dispatch blocked: PMU firmware** — CHANNEL_ALLOC fails after VM_INIT succeeds. NVIDIA does not ship signed PMU firmware for desktop Volta (GV100). **UVM bypass path now implemented** (Iteration 36).
- [x] **UVM Sovereign Compute Dispatch (Iteration 36)** — Full RM object hierarchy: `RM_CONTROL` wrapper, GPU UUID query, `UVM_REGISTER_GPU`, `FERMI_VASPACE_A`, `KEPLER_CHANNEL_GROUP_A`, `VOLTA_CHANNEL_GPFIFO_A`, `VOLTA_COMPUTE_A` bind, `NV01_MEMORY_SYSTEM` alloc, `NvUvmComputeDevice` with full `ComputeDevice` trait impl, `coral-gpu` auto-UVM wiring.
- [x] **UVM GPFIFO submission (Iteration 37)** — `submit_gpfifo()` writes GPFIFO entry to CPU-mapped ring buffer + updates GP_PUT via USERD doorbell. `poll_gpfifo_completion()` polls GP_GET for sync.
- [x] **NvDrmDevice delegation (Iteration 37)** — Evolved from stub to functional delegator: holds `Option<NvUvmComputeDevice>`, all `ComputeDevice` ops pass through to UVM backend.
- [x] **dispatch_binary API (Iteration 37)** — `KernelCacheEntry` (serde-derived), `GpuContext::dispatch_precompiled()`, `GpuTarget::arch_name()` — wires barraCuda kernel cache integration.
- [x] **Deep debt evolution (Iteration 37)** — `bytemuck::Zeroable` eliminates 5 `unsafe { zeroed() }` blocks, PCI vendor constants centralized, `raw_nv_ioctl` helper, pushbuf constant unification, NV_STATUS documented, uvm.rs smart-refactored (727 LOC → 3 files).
- [ ] **UVM hardware validation** — Full dispatch pipeline ready, needs RTX 3090 on-site testing
- [ ] Coverage 57.71% → 90% (57.71% line reflects full workspace measurement including hardware-gated VFIO code)

---

*The compiler evolves. 24/24 cross-spring absorption tests pass on both SM70 and RDNA2.
2185+48 tests passing, 0 failed, 57.71% line coverage. Zero production unwrap/todo. Zero extern "C". Error types zero-alloc. IPC semantic.
Three input languages: WGSL (primary), SPIR-V (binary), GLSL 450 (compute absorption).
VFIO sovereign dispatch complete — BAR0 + DMA + GPFIFO + PFIFO channel + V2 MMU + sync.
NVIDIA UVM dispatch pipeline complete — GPFIFO submission, USERD doorbell, completion polling.
IPC: `shader.compile.*` + `health.*` methods — JSON-RPC 2.0 + tarpc + Unix socket (wateringHole compliant).
Hardware: 2× Titan V (VFIO sovereign) + RTX 5060 (nvidia-drm/UVM).
Zero files over 1000 LOC. Zero clippy warnings. Zero doc warnings. Zero fmt drift.
OrExit\<T\> for zero-panic binary entry. IpcServiceError for structured IPC errors.
coral-glowplug JSON-RPC 2.0 with trait-based GpuPersonality system.
All pure Rust. Sovereignty is a runtime choice.*
