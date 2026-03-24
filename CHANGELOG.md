<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# Changelog

All notable changes to coralReef (sovereign Rust GPU compiler — WGSL/SPIR-V/GLSL → native GPU binary) are documented here. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

**Current status**: Phase 10 — Iteration 65

---

## [Unreleased]

### Iteration 65 — Deep Debt Solutions + Ecosystem Integration (Mar 24 2026)

- **Audit closure**: All 20 priority items from the comprehensive audit addressed
- **coralctl handlers refactor**: `handlers.rs` 1519 lines → 4 domain modules (`device_ops`, `compute`, `quota`, `mod`)
- **opt_copy_prop tests**: `tests.rs` 1018 → 973 lines via shared test helper extraction
- **Warnings / docs**: schedule.rs unused vars; dma.rs broken doc links; coral-driver unfulfilled lint expectations resolved
- **`#[forbid(unsafe_code)]`**: Added to `coral-ember/src/main.rs`
- **coral-driver**: SAFETY comments on all `unsafe` blocks
- **JSON-RPC `identity.get`**: Implemented per CAPABILITY_BASED_DISCOVERY_STANDARD
- **`capability.register`**: Ecosystem integration (fire-and-forget, graceful degradation)
- **`ipc.heartbeat`**: Periodic registration (45s interval)
- **Env**: `HOTSPRING_DATA_DIR` evolved to `CORALREEF_DATA_DIR` with backward-compatible fallback
- **Hardcoding**: Removed hardcoded `"hotSpring"` string from `swap.rs`
- **coralreef-core `ecosystem.rs`**: Songbird registration module
- **Tests / coverage**: Expanded across coral-driver, coral-glowplug, coral-ember, coral-gpu; shared `test_shader_helpers` for codegen tests
- **Metrics**: 3956 tests passing, 0 failed, ~119 ignored hardware-gated; ~66% workspace line coverage; fmt, clippy (pedantic+nursery), doc, release build — PASS

### Iteration 63 — Layer 7 Sovereign Pipeline: ACR Boot Solver + Falcon Diagnostics (Mar 23 2026)

- **Falcon Boot Solver (`acr_boot.rs`)**: Multi-strategy SEC2→ACR→FECS boot chain with `FalconProbe`, `Sec2Probe`, `AcrFirmwareSet`, `NvFwBinHeader`/`HsBlDescriptor` firmware parsing. Strategies: direct HRESET clear, EMEM-based SEC2 boot, IMEM-based SEC2 boot, system-memory WPR, hybrid WPR. SEC2 correctly probed, EMEM PIO verified, HS ROM PC advancing
- **Falcon Diagnostics (`diagnostics.rs`)**: Comprehensive falcon state capture — FECS/GPCCS/PMU/SEC2, HWCFG decode, security mode, IMEM/DMEM sizes, exception info
- **FECS Boot Module (`fecs_boot.rs`)**: Direct firmware upload (IMEM/DMEM PIO), warm-handoff-aware boot, ACR-bypass based on HWCFG security_mode
- **SEC2 base address fix**: `0x0084_0000` → `0x0008_7000` (GV100 PTOP topology) — unlocked all SEC2 diagnostics
- **CPUCTL v4+ bit layout**: Bit 0 = IINVAL, Bit 1 = STARTCPU (previously swapped). Aligns with Nouveau `gm200_flcn_fw_boot`
- **ACR firmware format decoded**: `nvfw_bin_hdr` (magic `0x10DE`), sub-headers, payload offsets. BL descriptor with DMA targeting
- **DMA context index fix**: `ctx_dma` from `PHYS_SYS(6)` → `VIRT(4)` matching `FALCON_DMAIDX_VIRT`. PC advanced `0x14b9` → `0x1505`
- **Full PMC disable+enable cycle**: Nouveau-style `nvkm_falcon_disable`/`enable` — ITFEN clear, interrupt clear, PMC disable/enable, falcon-local reset, memory scrub, BOOT0
- **Instance block + V2 MMU**: System-memory and hybrid page table construction for ACR WPR DMA. Bind polling implemented
- **Complexity debt flagged for team**: 5 files >1000 LOC: `acr_boot.rs` (4462), `coralctl.rs` (1649), `socket.rs` (1434), `mmu_oracle.rs` (1131), `device.rs` (1030)

### Iteration 62 — Deep Audit + Coverage Expansion + Hardcoding Evolution (Mar 21 2026)

- **Comprehensive audit**: Full review against wateringHole standards (IPC v3, UniBin, ecoBin, genomeBin, semantic naming, sovereignty, AGPL3). All quality gates verified: fmt, clippy (pedantic+nursery), test, doc (0 warnings)
- **Rustdoc: 4 warnings → 0**: Fixed MockSysfs link scope, redundant SysfsOps explicit targets, private verify_drm_isolation link, health.rs SysfsOps scope
- **coral-glowplug coverage**: sysfs_ops 92.2%, health 91.0%, config 93.4%, error 99.2%, pci_ids 100%, personality 86.4%. MockSysfs testing, health loop circuit breaker, env path overrides
- **coral-ember coverage**: vendor_lifecycle 83.7%, ipc 85.3%. All vendor lifecycle match arms tested, IPC success paths, swap "unbound" success path
- **coral-gpu coverage**: fma 100%, hash 100%, kernel 100%, pcie 97.8%, preference 100%. Driver env defaults, cache error paths, SM arch mapping
- **coral-reef codegen zero-coverage eliminated**: SM32 float64 0%→52%, SM32 misc 40%→74%, SM50 misc 40%→70%, SM50 control 23%→47%. New encoder test suites for all four backends
- **Hardcoding evolution**: New `coral_driver::linux_paths` module with `CORALREEF_SYSFS_ROOT` (default `/sys`), `CORALREEF_PROC_ROOT` (default `/proc`), `CORALREEF_NVIDIA_FIRMWARE_ROOT`, `CORALREEF_HOME_FALLBACK` env overrides. All sysfs/proc paths rooted via env-overridable helpers
- **`#[expect]` cleanup**: Removed dead code suppressions, replaced JSON-RPC field dead_code with serde renames, cleaned stale suppressions
- **Dependency analysis**: 227 production deps, all pure Rust. Transitive `libc` via tokio→mio tracked (mio#1735). OpenTelemetry unconditional in tarpc 0.37 (upstream tracked). Zero `*-sys`, zero `ring`, zero `openssl`
- **SM50/SM32 encoder test suites**: int ALU (IMad, ISetP, Flo), float ALU (FAdd imm/CBuf/neg/abs, FMul, FFma all combos, all FloatCmpOp variants), conv (F2F/F2I/I2F/I2I), mem (Atom, Ldc, MemBar, CCtl)
- **SM70 encoder expansion**: control (PixLd all PixVal, Out all OutType, MemBar scopes), conv (F2F rounding/ftz, F2I, I2F, FRnd)
- **Optimization pass coverage**: opt_bar_prop barrier propagation, opt_copy_prop sel/b2i patterns
- **linux_paths.rs**: 58% → **100%** — all env-overridable sysfs/proc path helpers fully tested
- **Coverage: 67.6% → 68.7% line** (+154 tests: 3306 → 3460 passing, 0 failed, 108 ignored hardware-gated; 8 crates above 90% target)
- All quality gates green: fmt, clippy (pedantic + nursery), test (3460+), doc (0 warnings), all files <1000 LOC

### Iteration 61 — DI Architecture + Coverage Evolution (Mar 21 2026)

- **coral-ember lib/binary split**: Monolithic binary → `lib.rs` + thin `main.rs`. Library exports config parsing, IPC dispatch, swap logic, vendor lifecycle for integration testing. `coral_ember::run()` entry point
- **coral-glowplug `SysfsOps` trait**: Dependency injection for sysfs operations — `RealSysfs` (production), `MockSysfs` (tests). `DeviceSlot<S: SysfsOps = RealSysfs>` generic. Activate/swap/health/release paths now testable without hardware
- **coral-gpu `GpuContext::from_parts`**: Assembles context from pre-built target + device + options, bypassing DRM/VFIO probing. `compile_wgsl_cached` for session-local caching. `compile_options()` accessor
- **coral-driver parsing extraction**: Pure parsing functions extracted from I/O: GSP firmware `from_legacy_bytes`/`parse_net_img_bytes`, PCI BDF/class/resource/speed/width parsing, VBIOS `validate_vbios`, devinit script scanning, `pramin_window_layout`
- **Stale primal name cleanup**: Remaining Songbird/BearDog/hotSpring/groundSpring references evolved to capability-based descriptions in doc comments and provenance citations
- **Coverage: 65.8% → 67.6% line** (+244 tests: 3062 → 3306 passing, 0 failed, 108 ignored hardware-gated)
- **Per-crate coverage**: coralreef-core 95.9%, primal-rpc-client 98.4%, coral-reef-stubs 95.2%, coral-reef-bitview 91.3%, coral-reef-isa 100%, amd-isa-gen 91.3% (6 crates above 90% target)
- **Root docs updated**: README, CHANGELOG, STATUS refreshed with current metrics
- **wateringHole handoff**: Iter 61 handoff with DI architecture decisions and coverage data
- All quality gates green: fmt, clippy (pedantic + nursery), test (3306+), doc, all files <1000 LOC

### Iteration 60 — Deep Audit Execution + Code Quality Evolution (Mar 21 2026)

- `unwrap()` → `expect()` with infallibility reasons: coralctl.rs JSON serialization, main.rs JSON serialization
- 14+ `#[allow]` → `#[expect]` tightened across 11 files (coral-glowplug, coral-ember, coral-reef codegen, amd-isa-gen generated templates)
- Smart refactor: `tex.rs` 986 LOC → 505 production + 484 tests in `tex_tests.rs` via `#[path]` pattern
- +20 coral-reef lib tests: Fp64Strategy variants, `prepare_wgsl` preamble injection (df64, complex64, f32 transcendental, PRNG, SU3 auto-chaining), `strip_enable_directives`, `emit_binary` NV/AMD, `compile_wgsl_full`, `compile_glsl_full`, `compile_wgsl_raw_sm`, Intel GLSL unsupported
- +4 coralreef-core tests: `shutdown_join_timeout` (elapsed message, test override, default), `UniBinExit` clone/copy
- 8 `// SAFETY:` comments added to unsafe blocks in coral-driver (dma.rs, cache_ops.rs, rm_helpers.rs, mmio.rs)
- 9 `unreachable!()` → `ice!()` migrations in SM70 encoder (set_reg_src, set_ureg_src, set_pred_dst, set_pred_src_file, set_rev_upred_src, set_src_cb, set_pred, set_dst, set_udst), opt_jump_thread (clone_branch ×2), SM70 control (PixVal, src type)
- Hardcoding evolution: EmberClient socket path → `default_ember_socket()` with `$CORALREEF_EMBER_SOCKET` env override
- Hardcoding evolution: socket group → `$CORALREEF_SOCKET_GROUP` env override with `"coralreef"` default
- amd-isa-gen template evolution: generated ISA code emits `#[expect(dead_code, missing_docs)]` instead of `#[allow]`
- Dependency analysis: tarpc 0.37 OpenTelemetry unconditional — documented for upstream tracking
- All quality gates green: fmt, clippy (pedantic + nursery), test (3062+), doc, all files <1000 LOC

### Iteration 59 — Deep Coverage Expansion + Clone Reduction (Mar 20 2026)

- **+358 tests** (2680 → 3038 passing, 0 failed, 102 ignored hardware-gated)
- **Line coverage 60.16% → 65.8%** (region 60.62% → 66.1%, function 69.03% → 72.9%)
- **Non-hardware coverage: 79.6%** (coral-reef 78.3%, coralreef-core 95.8%, bitview 91.3%)
- SM20/SM32/SM50 texture encoder tests: all older backends tested (bound, bindless, dims, LOD, ICE paths)
- SM20–SM70 memory encoder tests: OpLd/OpSt/OpAtom/OpLdc/OpCCtl/OpMemBar across all generations
- SM32+SM70 control flow + misc encoder tests: OpBra/OpExit/OpBar/OpVote/OpShf/OpPrmt
- SM20–SM70 integer ALU encoder tests: OpIAdd/OpIMul/OpIMad/OpISetP/OpFlo
- SM50 float64 encoder tests: OpDAdd/OpDMul/OpDFma/OpDSetP/OpDMnMx (0% → covered)
- SM70 float16 encoder tests: OpHAdd2/OpHMul2/OpHFma2/OpHSet2/OpHSetP2/OpHMnMx2 (0% → covered)
- Lower copy/swap pass tests (GPR, Pred, UGPR, CBuf, Mem, Swap XOR chain)
- Glowplug socket.rs + personality.rs coverage expanded (dispatch, parsing, traits, registry)
- Unix JSON-RPC advanced coverage: socket failures, stale removal, 256KiB payloads, 16 concurrent, env paths
- Clone reduction: lower_f64 SSARef clones eliminated, naga_translate delegates take `&SSARef`
- `panic!` → `ice!` evolution: all latency table panics converted to structured ICE reporting
- Typo fix: "instuction" → "instruction" across latency files
- `tests_unix_edge.rs` split → `tests_unix_advanced.rs` (1000-line compliance)
- All quality gates green: fmt, clippy, test, doc, all files <1000 LOC

### Iteration 58 — Audit Hardening + Coverage Expansion (Mar 20 2026)

- Full codebase audit: debt, mocks, hardcoding, patterns, standards compliance
- `#[forbid(unsafe_code)]` hardened on coral-ember + coral-glowplug (upgraded from `#[deny]`)
- `libc` eliminated from direct deps: `ember_client.rs` SCM_RIGHTS migrated to `rustix::net`
- Hardcoded socket paths evolved: `EMBER_SOCKET` → `ember_socket_path()` with `$CORALREEF_EMBER_SOCKET` env override
- Stale placeholder comments fixed: AMD GPU arch "placeholder" → "RDNA2/3/4 backend"
- 14 `#[allow]` → `#[expect]` tightening across 8 files (stale suppressions now warn at compile time)
- 5 tarpc Unix socket roundtrip tests (status, health_check, capabilities, wgsl compile, liveness+readiness); tarpc coverage 80.84% → 94.88%
- 9 vendor_lifecycle tests for all 6 vendor types
- 11 IPC Unix error path tests: dispatch errors, blank lines, malformed JSON, invalid JSON-RPC version
- Coverage: 59.98% → 60.16% line, 68.73% → 69.03% function, 60.44% → 60.62% region
- Debris cleanup: stale `.analysis-*` files removed
- All quality gates green: fmt, clippy, test, doc

### Iteration 57 — Deep Debt Evolution + All-Silicon Pipeline (Mar 18 2026)

- Specs updated to v0.6.0 — all-silicon pipeline, sovereignty roadmap, Titan V x2 + RTX 5060 + MI50 planned
- Smart refactor: socket.rs 1488→556 lines (tests extracted to socket_tests.rs)
- GP_PUT cache flush experiment H1: `clflush` USERD + GPFIFO before doorbell — **proven insufficient** on live Titan V. Root cause identified: cold silicon (PFIFO/GPCCS not initialized), not cache coherency
- **GlowPlug `device.lend` / `device.reclaim`**: VFIO fd broker pattern for test access. GlowPlug drops VFIO fd so tests can open the group, RAII reclaim on drop. 10x stress cycle validated on both Titan Vs
- **GlowPlug-aware VFIO test harness**: `VfioLease` RAII guard in all `hw_nv_vfio*` tests — automatic lend/reclaim with transparent fallback when glowPlug not running
- **35 VFIO hardware tests passing** on live Titan V x2: open, alloc, upload/readback, multi-buffer, BAR0 probing, PFIFO diagnostics, HBM2 PHY/timing/FALCON, hot-swap stress, PRI backpressure
- **9 hot-swap integration tests**: health, device list, lend/reclaim round-trip, lend+open+reclaim, 10x stress cycle, health-during-lend, double-lend rejection, reclaim no-op
- `multi_gpu_enumerates_multiple` fix: counts VFIO-bound GPUs via sysfs PCI class (3 GPUs: 1 DRM + 2 VFIO)
- Production .expect() evolution: signal handlers → or_exit(), GSP observer → Result, SAFETY comments
- Unsafe code evolution: all volatile reads/writes consolidated through VolatilePtr, SAFETY comments on all from_raw_parts and Send/Sync impls
- AMD metal placeholder → real GFX906 register offsets from AMD docs
- Intel GPU arch: added Dg2Alchemist + XeLpg variants
- Hardcoding evolution: pci_ids.rs constants, unified chip_name() identity module
- Coverage expansion: GSP knowledge/parser/applicator, MMIO VolatilePtr, identity, pci_ids, error module
- Clippy clean: fixed map_or → is_none_or, unfulfilled lint expectations → allow, doc backtick fixes
- Test expansion: 2527 → 2560 passing (+33 tests), 0 failed, 90 ignored
- **Handoff to hotSpring**: Pipeline 9/11 stages complete. Remaining blocker: GPU initialization (warm via `device.resurrect`). hotSpring Exp 070: twin experiment with both Titan Vs

### Added
- GlowPlug security hardening: BDF validation (path traversal, null bytes, shell injection), max 64 concurrent clients via semaphore, 30s idle timeout, 64KiB max request line (iter56)
- 27 chaos/fault/penetration tests: JSON fuzzing, connection chaos, BDF injection, method probing, repeated shutdown (iter56)
- Circuit breaker in health loop: stops BAR0 reads after 6 consecutive faults, prevents kernel instability (iter56)
- nvidia module guard: blocks swap/resurrect/auto-resurrection when nvidia.ko loaded (iter56)
- DRM consumer guard: refuses driver unbind when active display clients detected — prevents kernel panic (iter56)
- Boot sovereignty: `softdep nvidia pre: vfio-pci`, `vfio-pci.ids=10de:1d81` in kernel cmdline, initramfs rebuild (iter56)
- Boot safety validation in coral-glowplug startup: checks /proc/cmdline, warns if nvidia probed managed devices (iter56)
- `scripts/boot/` deployment scripts: `deploy-boot.sh`, canonical modprobe and udev configs (iter56)
- `ActiveDrmConsumers` error variant in DeviceError (iter56)
- thiserror error hierarchy: DeviceError, ConfigError, RpcError with JSON-RPC 2.0 codes (iter55)
- clap CLI evolution: replaced manual std::env::args with derive Parser (iter55)
- sysfs module extraction: device.rs refactored 886→703 lines, sysfs.rs 268 lines (iter55)
- 131 coral-glowplug tests (was 72 at iter54)

### Fixed
- Deadlock in socket.rs: spawn_blocking + block_on on async mutex replaced with direct .lock().await (iter55)
- Graceful shutdown: watch channel coordination, accept loop abort, 5s mutex timeout (iter55)
- Kernel panic on driver unbind: DRM consumer check prevents unbinding GPUs with active display (iter56)
- Kernel crash loop: circuit breaker + nvidia guard prevent repeated BAR0 reads on faulted hardware (iter56)

---

## Phase 10 — Iterations 50–54

### Added
- GlowPlug JSON-RPC 2.0, typed IPC errors, trait personality (iter52)
- wateringHole IPC health compliance, coral-gpu refactor, 2157 tests (iter51)
- Coverage expansion: +123 tests (2364 total), 59.92% line coverage (iter54)
- 40 constant folding unit tests for IR fold pass (iter54)
- 30+ coral-glowplug tests: JSON-RPC dispatch, personality, config, TCP bind (iter54)
- 30+ coral-driver tests: PCI config parsing, vendor detection, PM4, GEM, RM params (iter54)
- 12 codegen tests: opt_prmt, naga_translate, lower_f64, builder, assign_regs (iter54)
- 7 api.rs + spiller.rs tests: spill pressure, pinned values, UPred (iter54)
- Deep audit execution, safe Rust evolution, +56 tests, nursery lints (iter53)
- GlowPlug graceful shutdown — SIGTERM handler, state snapshot, clean fd release
- GlowPlug boot persistence — systemd service, IOMMU group handling, auto-discovery
- GrEngineStatus diagnostics, MappedBar alignment guards, VFIO FECS probe
- HBM2 resurrection — GlowPlug can detect death and resurrect VRAM live
- coral-glowplug daemon — sovereign PCIe device lifecycle broker
- Clock gating sweep and PCLOCK deep probe to GlowPlug
- PRI bus backpressure sensor, progressive domain enable, GlowPlug health listener
- Host-side USERD GP_GET/GP_PUT readback to experiment results
- coral-gpu preference API, UVM rm_helpers refactor

### Changed
- pci_discovery.rs tests extracted to sibling file (1027→890 LOC) (iter54)
- 10 DriverError doc links → full crate path, zero doc warnings (iter54)
- 10 EVOLUTION markers audited and catalogued for feasibility (iter54)
- Full audit execution, 1992 tests, zero warnings (iter50)

### Fixed
- V2 MMU PDE/PTE aperture encoding, PBDMA USERD target, PD0 layout
- USERD_TARGET + INST_TARGET in runlist channel entry
- GP_BASE_HI aperture + PFIFO channel diagnostics

---

## Phase 10 — Iterations 42–49

### Added
- PFIFO channel init, V2 MMU page tables, cross-primal rewire (iter43)
- VFIO sync, barraCuda from_vfio API (iter42)
- Experiment Q — VRAM instance block + preempt/ACK protocol
- Structural refactor, clippy zero, coverage expansion (iter46)
- Deep audit, vfio/channel refactor, coverage expansion (iter45)
- Deep debt evolution, docs sync, VFIO cache flush (iter47)

### Changed
- Deep debt — 2 bugs fixed, 11 magic numbers eliminated, dispatch error recovery (iter40)

### Fixed
- USERD_TARGET + INST_TARGET in runlist channel entry (iter44)

---

## Phase 10 — Iterations 30–41

### Added
- Sovereign BAR0 GR init — bypass nouveau CTXNOTVALID
- FECS GR context init, UVM CBUF alignment, safe Rust evolution (iter39)
- Deep debt solutions, idiomatic evolution, doc updates (iter38)
- Gap closure, UVM dispatch pipeline, deep debt evolution (iter37)
- FirmwareInventory, ioctl evolution, unsafe reduction (iter35)
- NVVM poisoning validation, doc cleanup (iter33)
- Deep debt evolution, math functions, AMD encoding (iter32)
- Deep debt, Nouveau UAPI migration, UVM fix, doc cleanup (iter31)
- Spring absorption, FMA evolution, multi-device compile (iter30)
- NVIDIA last mile pipeline foundation (iter29)
- Unsafe elimination, NVVM poisoning bypass, spring absorption wave 3 (iter28)
- Deep debt, cross-spring absorption, root docs refresh (iter27)
- Sovereign pipeline unblock (hotSpring blockers) (iter26)
- Math evolution, DEBT zero, full sovereignty (iter25)

### Changed
- Deep debt evolution, test coverage expansion (iter34)

### Fixed
- QMD field layout, CBUF descriptors, syncobj sync, dispatch diagnostics
- Sovereign DRM dispatch — 3 bugs unlocking CHANNEL_ALLOC on all NVIDIA GPUs
- DRM struct size assertions + UAPI ABI guards + PMU firmware docs
- DRM ioctl struct ABI — 4 mismatches against kernel UAPI
- Filter BAR0 register addresses from FECS channel init
- Wire FECS GR context init into NvDevice open path

---

## Phase 10 — Iterations 20–29

### Added
- Multi-GPU sovereignty, cross-vendor parity, showcase (iter24)
- Multi-language frontends & fixture reorganization (iter22)
- Cross-spring absorption wave 2 (iter21)
- SSA dominance repair, sigmoid_f64 unblocked (iter20)
- Back-edge liveness & RA evolution (iter19)
- Deep debt: Pred→GPR legalization, small array promotion (iter18)
- Absorb 20 cross-spring shaders, audit, idiomatic refactoring (iter17)
- Coverage expansion, legacy SM tests, latency unit tests (iter16)
- AMD safe slices, inline var pre-allocation, typed DRM wrappers (iter15)
- Statement::Switch, unsafe reduction, diagnostic panics (iter14)
- df64 preamble, Fp64Strategy enum, 5 tests unblocked (iter13)
- Compiler gaps, math coverage, cross-spring wiring (iter12)

### Changed
- Root docs, debris sweep, orphaned fixture wired (iter23)

---

## Phase 10 — Iterations 7–11

### Added
- Deep debt reduction, safe ioctl surface, corpus expansion (iter11)
- AMD E2E GPU dispatch verified (iter10)
- E2E wiring, push buffer fix, debt reduction (iter9)
- Safety boundary, ioctl layout tests, cfg domain-split (iter7)
- Deep debt internalization, idiomatic Rust evolution (iter6)
- Pointer tracking fix, scheduler refactor, debt audit (iter5)

### Changed
- nak/ → codegen/, vendor-neutral naming, doc evolution
- Smart-refactor 990+ LOC files, panic evolution in IR types
- Spring absorption — deterministic serialization, unsafe removal, provenance

### Fixed
- Conditional branches in translate_if + multi-pred RA merge

---

## Phases 6–9 — Sovereign Pipeline

### Added
- Sovereign pipeline complete (phases 6–9)
- F64 transcendentals, error safety, 1000 LOC compliance, 390 tests
- Standalone sovereignty, debt reduction, cleanup

### Changed
- coralNak → coralReef rename

---

## Initial

### Added
- Sovereign Rust shader compiler — initial commit
- WGSL/SPIR-V/GLSL frontend, naga IR, SSA codegen
- NVIDIA (SM20–SM89) and AMD (GFX1030) backends
- coral-driver: DRM amdgpu, nouveau, nvidia-drm, UVM, VFIO dispatch
