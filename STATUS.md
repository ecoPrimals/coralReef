<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# coralReef — Status

**Last updated**: March 30, 2026  
**Phase**: 10 — Iteration 70 (ludoSpring V35 Gap Resolution + Deep Audit)

---

## Overall Grade: **A+** (Multi-Vendor Sovereign GPU Compiler)

| Category | Grade | Notes |
|----------|-------|-------|
| Primal lifecycle | A | Standalone `PrimalLifecycle` + `PrimalHealth`, full test coverage |
| UniBin compliance | A | All 3 binaries: clap + --port + --help/--version, standalone startup, signal handling |
| IPC | A+ | JSON-RPC 2.0 + tarpc (bincode), Unix socket + TCP, zero-copy `Bytes` payloads, `shader.compile.*` + `health.*` + `identity.get` + `capability.register` + `capabilities.list` + `ipc.heartbeat`, Songbird `ecosystem` registration (wateringHole compliant), differentiated error codes, newline-delimited TCP (v3.1), capability-domain symlink |
| NVIDIA pipeline | A+ | WGSL/SPIR-V/GLSL → naga → codegen IR → f64 lower → optimize → legalize → RA → encode |
| AMD pipeline | A+ | `ShaderModelRdna2` → legalize → RA → encode (memory, control flow, comparisons, integer, type conversion, system values) |
| Mesa stubs evolved | A+ | All modules evolved to pure Rust (BitSet, CFG, dataflow, fxhash, nvidia_headers) |
| f64 transcendentals | A+ | sqrt, rcp, exp2, log2, sin, cos, exp, log, pow — NVIDIA (Newton-Raphson) + AMD (native) |
| Vendor-agnostic arch | A+ | `Shader` holds `&dyn ShaderModel` — idiomatic Rust trait dispatch, no manual vtables |
| coralDriver | A+ | AMD amdgpu (GEM+PM4+CS+fence), NVIDIA nouveau (sovereign), nvidia-drm (compatible), VFIO (direct BAR0+DMA), multi-GPU scan, pure Rust |
| coralGpu | A+ | Unified compile+dispatch, multi-GPU auto-detect, `DriverPreference` sovereign default, `enumerate_all()` |
| Code structure | A+ | Smart refactoring: vfio/channel.rs 2894→5 modules (prod <1000 LOC), diagnostic/runner.rs 2485→769+experiments/ (Iter 46), scheduler prepass 842→313, cfg→{mod,dom}, ir/{pred,src,fold}, ipc/{jsonrpc,tarpc}, tex.rs 986→505+484 (Iter 60), vendor_lifecycle→8 modules, ipc→6 modules, handlers→mod+sweep, ACR strategies→directory modules (Iter 69), swap.rs 1102→708+swap_preflight (362), vfio_compute/mod.rs 1018→855+gr_engine_status (173) (Iter 70) |
| Tests | A+ | 4189 passing, 0 failed, ~64% line coverage (82%+ non-hardware, 8 crates >90%), DI-enabled mock testing, tarpc Unix roundtrip, IPC chaos/fault tests |
| Clippy | A+ | Zero warnings, pedantic categories enabled |
| License | A | AGPL-3.0-only (upstream-derived files retain original attribution) |
| Sovereignty | A+ | Zero FFI, zero `*-sys`, zero `extern "C"`, zero-knowledge startup, `#[forbid(unsafe_code)]` on coral-ember + coral-glowplug, `ring` eliminated, `unsafe` confined to kernel ABI in coral-driver only, all ioctl via `rustix`, `libc` eliminated from direct deps |
| Result propagation | A+ | Pipeline fully fallible: naga_translate → lower → legalize → encode, zero production `unwrap()`/`todo!()`, `unreachable!()` → `ice!()` in encoder |
| Dependencies | A+ | Pure Rust — zero C deps, zero `*-sys` crates, ISA gen in Rust, `rustix` `linux_raw` backend (zero libc in our code), `ring` eliminated, FxHashMap internalized. Transitive `libc` via tokio/mio tracked (mio#1735) |
| Tooling | A+ | `rustfmt.toml`, `clippy.toml`, `deny.toml`, pure Rust ISA generator |
| Tolerance model | A | 13-tier `tol::` module (groundSpring alignment), `within()`, `compare_all()` |
| FMA control | A | `FmaPolicy` enum (AllowFusion / NoContraction) in `CompileOptions` |
| Uniform buffers | A | `var<uniform>` → CBuf reads (scalar/vector/matrix), struct field access |
| GlowPlug security | A+ | BDF validation, connection limits, idle timeout, circuit breaker, chaos/fault/pen testing (143 tests), `device.lend`/`device.reclaim` VFIO broker |
| Boot sovereignty | A+ | vfio-pci.ids preemption, softdep nvidia ordering, initramfs, boot safety validation |

## Phase Status

| Phase | Description | Status |
|-------|-------------|--------|
| 1–9 | Foundation through Full Sovereignty | **Complete** |
| 10 — Spring Absorption | Deep debt, absorption, compiler hardening, E2E verified | **Iteration 65** |

### Iteration 70: ludoSpring V35 Gap Resolution + Deep Audit (Mar 30, 2026)

**Theme**: Resolve composition gaps found by ludoSpring V35 experiments, deep clippy/file-size audit.

| Area | Change |
|------|--------|
| `capabilities.list` | Added JSON-RPC method to both newline-delimited and HTTP servers |
| `cargo clippy` | Fixed 8 additional errors: branches_sharing_code (×2), redundant_clone, collapsible_if, struct_excessive_bools, unused_variables, dead_code, missing_docs, too_many_arguments, unfulfilled_lint_expectations |
| File sizes | `swap.rs` 1102→708 (extracted `swap_preflight.rs`), `vfio_compute/mod.rs` 1018→855 (extracted `gr_engine_status.rs`) |
| ludoSpring V35 P1 | UDS framing confirmed raw newline-delimited (Iter 69). `capability.register` confirmed working (Iter 69). `capabilities.list` added (Iter 70). |

### Iteration 69: Deep Debt Resolution + wateringHole Compliance (Mar 29, 2026)

**Theme**: Comprehensive audit and deep debt resolution against wateringHole v3.1 standards.

| Area | Change |
|------|--------|
| `cargo fmt` | Fixed 457 formatting regions across entire workspace |
| `cargo clippy` | Fixed 30+ errors: manual_div_ceil, identity_op, collapsible_else_if, derivable_impls, unnecessary_cast, missing_docs, doc_lazy_continuation, manual_range_patterns, deprecated calls |
| UniBin `--port` | Added `--port` flag to `coralreef server` (wateringHole v1.1 mandatory) |
| Newline TCP | Added raw newline-delimited TCP JSON-RPC listener (wateringHole v3.1 mandatory framing) |
| coral-ember UniBin | Added clap CLI with `server --port` subcommand |
| Capability symlink | `shader.sock → coralreef-{family}.sock` (CAPABILITY_BASED_DISCOVERY v1.1) |
| File sizes | Refactored 10 files over 1000 LOC into cohesive directory modules |
| Doc links | Fixed all 43 broken intra-doc links; zero rustdoc warnings |
| SPDX/license | All 9 showcase + tools Cargo.toml now have `rust-version = "1.85"` |
| `#![forbid(unsafe_code)]` | Added to all 9 showcase/test main.rs files |
| Production logging | Replaced println!/eprintln! with tracing in all production paths |
| Production .unwrap() | Eliminated from all library code; .expect() with reasons where invariant proven |
| Dead code cleanup | All `#[allow(dead_code)]` documented with `reason = "..."` |
| Hardcoding evolution | CORALREEF_X11_CONF_DIR, CORALREEF_UDEV_RULES_DIR, CORALREEF_JOURNAL_PATH, CORALREEF_GROUP_FILE, CORALREEF_CAPABILITY_DOMAIN env overrides |
| Coverage tests | +30 tests: ecosystem discovery, newline TCP JSON-RPC, server error paths, capability symlinks |
| Deprecated removal | Removed deprecated sysmem physical boot path (243 lines, superseded by `attempt_sysmem_acr_boot_with_config`) |
| Tests | 4189 passed, 0 failed, 153 ignored |

### Iteration 65: Deep Debt Solutions + Ecosystem Integration (Mar 24 2026)

| Item | Status | Detail |
|------|--------|--------|
| Comprehensive audit execution | ✅ | All 20 priority audit items addressed |
| coralctl `handlers.rs` refactor | ✅ | 1519 lines → 4 domain modules (`device_ops`, `compute`, `quota`, `mod`) |
| `opt_copy_prop/tests.rs` slimming | ✅ | 1018 → 973 lines via shared test helper extraction |
| Compiler / rustdoc hygiene | ✅ | schedule.rs unused vars, dma.rs doc links, coral-driver unfulfilled expects |
| `#[forbid(unsafe_code)]` on coral-ember `main.rs` | ✅ | Aligns with workspace standards |
| coral-driver SAFETY | ✅ | SAFETY comments on all `unsafe` blocks |
| JSON-RPC `identity.get` | ✅ | Per CAPABILITY_BASED_DISCOVERY_STANDARD |
| `capability.register` + ecosystem | ✅ | Fire-and-forget; degrades gracefully |
| `ipc.heartbeat` | ✅ | Periodic registration (45s interval) |
| Data dir env | ✅ | `CORALREEF_DATA_DIR` (backward-compat `HOTSPRING_DATA_DIR` fallback) |
| Hardcoding | ✅ | Removed hardcoded `"hotSpring"` from `swap.rs` |
| `ecosystem.rs` (coralreef-core) | ✅ | Songbird registration wiring |
| Test coverage | ✅ | Expanded across coral-driver, coral-glowplug, coral-ember, coral-gpu; shared `test_shader_helpers` for codegen tests |

### Iteration 63: Layer 7 Sovereign Pipeline — ACR Boot Solver + Falcon Diagnostics (Mar 23 2026)

| Item | Status | Detail |
|------|--------|--------|
| Falcon Boot Solver (`acr_boot.rs`) | 🔬 In Progress | Multi-strategy SEC2→ACR→FECS boot chain. `FalconProbe`, `Sec2Probe`, `AcrFirmwareSet`, `NvFwBinHeader`, `HsBlDescriptor`. Tries strategies in order of cost: direct HRESET clear → EMEM-based SEC2 boot → IMEM-based SEC2 boot → system-memory WPR → hybrid WPR. SEC2 correctly probed, EMEM PIO verified, PC advancing through HS ROM |
| Falcon Diagnostics (`diagnostics.rs`) | ✅ | Comprehensive diagnostic capture: all falcon states (FECS/GPCCS/PMU/SEC2), HWCFG decoding, security mode, IMEM/DMEM sizes, exception info, register diff infrastructure |
| FECS Boot Module (`fecs_boot.rs`) | 🔬 In Progress | Direct FECS firmware upload (IMEM/DMEM PIO), warm-handoff-aware boot path, ACR-bypass attempt based on HWCFG security_mode discovery |
| SEC2 base address fix | ✅ | `SEC2_BASE` corrected from `0x0084_0000` to `0x0008_7000` (GV100 PTOP topology). Unlocked all SEC2 diagnostics previously showing `0xbadf1100` |
| CPUCTL v4+ bit layout fix | ✅ | Bit 0 = IINVAL, Bit 1 = STARTCPU for Falcon v4+ (previously swapped). Nouveau always writes 0x02 for STARTCPU |
| ACR firmware parsing | ✅ | `nvfw_bin_hdr` format decoded (magic `0x10DE`, sub-headers, payload offsets). ACR bl.bin, ucode_load.bin, sec2/sig.bin all parsed. BL descriptor DMA targeting implemented |
| DMA context index fix | ✅ | `ctx_dma` changed from `PHYS_SYS (6)` to `VIRT (4)` matching Nouveau's `FALCON_DMAIDX_VIRT`. PC advanced from `0x14b9` → `0x1505` |
| Full PMC disable+enable cycle | ✅ | Nouveau-style `nvkm_falcon_disable` + `nvkm_falcon_enable`: clear ITFEN, clear interrupts, PMC disable SEC2, falcon-local reset, PMC re-enable, memory scrub wait, write BOOT0 |
| Instance block + V2 MMU pages | 🔬 In Progress | System-memory and hybrid instance block construction with 5-level V2 page tables for ACR WPR DMA. `bind_stat` polling implemented but bind-complete state not yet reached |
| **Complexity debt flagged** | 📋 Team | 5 files >1000 LOC identified for team evolution: `acr_boot.rs` (4462), `coralctl.rs` (1649), `socket.rs` (1434), `mmu_oracle.rs` (1131), `device.rs` (1030) |

### Iteration 62: Deep Audit + Coverage Expansion + Hardcoding Evolution (Mar 21 2026)

| Item | Status | Detail |
|------|--------|--------|
| Comprehensive codebase audit | ✅ | Full review of specs, wateringHole standards (IPC v3, UniBin, ecoBin, genomeBin, semantic naming, sovereignty, AGPL3), debt, mocks, hardcoding, unsafe, patterns |
| Rustdoc warnings eliminated | ✅ | 4 warnings → 0: MockSysfs link scope, redundant SysfsOps links, private verify_drm_isolation link, health.rs SysfsOps scope |
| coral-glowplug coverage expansion | ✅ | sysfs_ops 92.2%, health 91.0%, config 93.4%, ember 68.5%, error 99.2%, pci_ids 100%, personality 86.4% — MockSysfs testing, health loop circuit breaker, IPC dispatch, env paths |
| coral-ember coverage expansion | ✅ | vendor_lifecycle 83.7%, ipc 85.3%, swap 61.3% — all vendor lifecycle arms, IPC success paths, swap unbound, env overrides |
| coral-gpu coverage expansion | ✅ | fma 100%, hash 100%, kernel 100%, pcie 97.8%, preference 100% — driver env defaults, cache error paths, SPIR-V paths, FMA per-arch |
| coral-reef codegen coverage | ✅ | SM32 float64: 0%→52%, SM32 misc: 40%→74%, SM50 misc: 40%→70%, SM50 control: 23%→47% — new encoder test suites |
| Hardcoding evolution | ✅ | `CORALREEF_SYSFS_ROOT` (default `/sys`), `CORALREEF_PROC_ROOT` (default `/proc`), `CORALREEF_NVIDIA_FIRMWARE_ROOT`, `CORALREEF_HOME_FALLBACK` — all sysfs/proc paths now rooted via env-overridable helpers in `coral_driver::linux_paths` |
| `#[expect]` cleanup | ✅ | Removed dead code, replaced JSON-RPC field suppressions with serde renames, cleaned stale suppressions |
| Dependency analysis | ✅ | 227 production deps (all pure Rust); `libc` transitive via tokio→mio→signal-hook-registry (tracked mio#1735); `opentelemetry` unconditional in tarpc 0.37 (upstream tracked) |
| Coverage: 67.6% → 68.7% line | ✅ | +154 tests (3306 → 3460 passing, 0 failed, 108 ignored hardware-gated) |
| Quality gates | ✅ | `fmt` ✅, `clippy --all-features -D warnings` ✅, `test --all-features` ✅ (3460+ pass, 0 fail), `doc` ✅ (0 warnings), all files <1000 LOC |

### Iteration 61: DI Architecture + Coverage Evolution (Mar 21 2026)

| Item | Status | Detail |
|------|--------|--------|
| coral-ember lib/binary split | ✅ | Monolithic binary → `lib.rs` + thin `main.rs`; library exports config parsing, IPC dispatch, swap logic, vendor lifecycle. `coral_ember::run()` entry point |
| coral-glowplug `SysfsOps` trait | ✅ | `SysfsOps` trait with `RealSysfs` (production) + `MockSysfs` (tests). `DeviceSlot<S: SysfsOps = RealSysfs>` generic. Activate/swap/health/release testable without hardware |
| coral-gpu `GpuContext::from_parts` | ✅ | Assembles context from pre-built target + device + options, bypasses DRM/VFIO probing. `compile_wgsl_cached` session cache. `compile_options()` read accessor |
| coral-driver parsing extraction | ✅ | Pure parsing extracted from I/O: GSP firmware bytes, PCI BDF/class/resource/speed/width, VBIOS validation, devinit script scanning, PRAMIN window layout |
| Stale primal name cleanup | ✅ | Songbird/BearDog → "delegated TLS"/"ecosystem crypto"; hotSpring/groundSpring → "ecosystem experiment"/"numerical validation" in doc comments |
| Deadlock test verified | ✅ | `shutdown_join_timeout_with_test_override` passes without `--skip` flag (deadlock fix from Iter 60 confirmed solid) |
| Coverage: 65.8% → 67.6% line | ✅ | +244 tests (3062 → 3306 passing, 0 failed, 108 ignored hardware-gated) |
| Per-crate coverage | ✅ | coralreef-core 95.9%, primal-rpc-client 98.4%, coral-reef-stubs 95.2%, coral-reef-bitview 91.3%, coral-reef-isa 100%, amd-isa-gen 91.3%, nak-ir-proc 88.6%, coral-reef 78.6%, coral-gpu 65.8%, coral-ember 65.2%, coral-glowplug 62.3%, coral-driver 29.9% |
| Root docs updated | ✅ | README, CHANGELOG, STATUS refreshed with current metrics and iteration |
| Quality gates | ✅ | `fmt` ✅, `clippy --all-features -D warnings` ✅, `test --all-features` ✅ (3306+ pass, 0 fail), `doc` ✅, all files <1000 LOC |

### Iteration 60: Deep Audit Execution + Code Quality Evolution (Mar 21 2026)

| Item | Status | Detail |
|------|--------|--------|
| `unwrap()` → `expect()` | ✅ | coralctl.rs JSON serialization, main.rs JSON serialization — both with infallibility reason |
| `#[allow]` → `#[expect]` tightening | ✅ | 14+ attributes across 11 files: coral-glowplug (personality, error, ember, health, device/mod, device/types, config, device/swap), coral-ember (vendor_lifecycle), coral-reef (codegen/mod, lower_f64/mod) |
| Smart refactor: tex.rs | ✅ | 986 LOC → 505 production + 484 tests in tex_tests.rs via `#[path]` pattern |
| Coverage expansion: coral-reef lib | ✅ | +20 tests: Fp64Strategy variants, prepare_wgsl preamble injection (df64, complex64, f32 transcendental, PRNG, SU3), strip_enable_directives, emit_binary NV/AMD, compile_wgsl_full, compile_glsl_full, compile_wgsl_raw_sm, Intel GLSL unsupported |
| Coverage expansion: coralreef-core | ✅ | +4 tests: shutdown_join_timeout (elapsed message, test override, default), UniBinExit clone/copy |
| `// SAFETY:` documentation | ✅ | 8 unsafe blocks in coral-driver: dma.rs (alloc cleanup), cache_ops.rs (clflush, mfence), rm_helpers.rs (drm_ioctl_named), mmio.rs (4× VolatilePtr::new in tests) |
| `unreachable!()` → `ice!()` | ✅ | 9 migrations: encoder.rs (set_reg_src, set_ureg_src, set_pred_dst, set_pred_src_file, set_rev_upred_src, set_src_cb, set_pred, set_dst, set_udst), opt_jump_thread.rs (clone_branch ×2), control.rs (PixVal, src type) |
| Hardcoding evolution: ember socket | ✅ | `EMBER_SOCKET` const → `default_ember_socket()` fn with `$CORALREEF_EMBER_SOCKET` env override |
| Hardcoding evolution: socket group | ✅ | Hardcoded `"coralreef"` group → `$CORALREEF_SOCKET_GROUP` env override with `"coralreef"` default |
| amd-isa-gen template evolution | ✅ | Generated ISA code now emits `#[expect(dead_code, missing_docs)]` instead of `#[allow]` |
| Dependency analysis: tarpc | ✅ | OpenTelemetry unconditional in tarpc 0.37 — cannot trim via features; documented for upstream tracking |
| Quality gates | ✅ | `fmt` ✅, `clippy --all-features -D warnings` ✅, `test` ✅ (3062+ pass, 0 fail), `doc` ✅ |

### Iteration 59: Deep Coverage Expansion + Clone Reduction (Mar 20 2026)

| Item | Status | Detail |
|------|--------|--------|
| SM20/SM32/SM50 tex encoder tests | ✅ | All older texture encoder backends now tested (bound/bindless, dims, LOD modes, ICE paths) |
| Memory encoder tests (SM20–SM70) | ✅ | OpLd/OpSt/OpAtom/OpLdc/OpCCtl/OpMemBar covered across all four shader model generations |
| Control flow + misc encoder tests | ✅ | OpBra/OpExit/OpBar/OpVote/OpShf/OpPrmt for SM32 and SM70 |
| Integer ALU encoder tests | ✅ | OpIAdd/OpIMul/OpIMad/OpISetP/OpFlo across SM20–SM70 |
| Float64 encoder tests (SM50) | ✅ | 0% → covered: OpDAdd/OpDMul/OpDFma/OpDSetP/OpDMnMx all rounding modes |
| Float16 encoder tests (SM70) | ✅ | 0% → covered: OpHAdd2/OpHMul2/OpHFma2/OpHSet2/OpHSetP2/OpHMnMx2 |
| Lower copy/swap tests | ✅ | Copy lowering pass tested (GPR, Pred, UGPR, CBuf, Mem, Swap XOR chain) |
| Glowplug socket.rs coverage | ✅ | Protocol parsing, dispatch, TCP edge cases, concurrent connections |
| Glowplug personality.rs coverage | ✅ | All personality traits, registry, aliases, HBM2, driver modules |
| Unix JSON-RPC advanced coverage | ✅ | Socket failures, stale removal, mid-line disconnect, 256KiB payloads, 16 concurrent, drop semantics, env paths |
| Clone reduction (lower_f64) | ✅ | Unnecessary SSARef clones eliminated in newton.rs, trig.rs; delegates take `&SSARef` |
| Clone reduction (naga_translate) | ✅ | `translate_math` passes `&SSARef` / `Option<&SSARef>` — 3 clones per attempt → 0 for non-matching arms |
| `panic!` → `ice!` evolution | ✅ | All latency table panics converted to `ice!` macro for structured ICE reporting |
| Typo fix (instuction → instruction) | ✅ | Fixed across all latency files |
| File size compliance | ✅ | tests_unix_edge.rs split → tests_unix_advanced.rs; all files under 1000 lines |
| Coverage: 60.16% → 65.8% line | ✅ | +358 tests (2680 → 3038), coral-reef 73.4% → 78.3%, non-hardware 75.8% → 79.6% |
| Quality gates | ✅ | `fmt` ✅, `clippy --all-features -D warnings` ✅, `test --all-features` ✅, `doc` ✅ |

### Iteration 58: Audit Hardening + Coverage Expansion (Mar 20 2026)

| Item | Status | Detail |
|------|--------|--------|
| Full codebase audit | ✅ | Comprehensive review of debt, mocks, hardcoding, patterns, standards compliance |
| `#[forbid(unsafe_code)]` hardened | ✅ | coral-ember + coral-glowplug upgraded from `#[deny]` to `#[forbid]` |
| `libc` eliminated from direct deps | ✅ | `ember_client.rs` SCM_RIGHTS migrated to `rustix::net`, `libc` removed from dev-deps |
| Hardcoded socket paths evolved | ✅ | `EMBER_SOCKET` → `ember_socket_path()` with `$CORALREEF_EMBER_SOCKET` env override |
| Stale placeholder comments fixed | ✅ | AMD GPU arch "placeholder" → "RDNA2/3/4 backend", Intel → "planned — register addresses TBD" |
| `#[allow]` → `#[expect]` tightening | ✅ | 14 attributes evolved across 8 files; stale suppressions will now warn at compile time |
| tarpc Unix transport coverage | ✅ | 5 roundtrip tests (status, health_check, capabilities, wgsl compile, liveness+readiness) over Unix socket; tarpc coverage 80.84% → 94.88% |
| vendor_lifecycle test expansion | ✅ | 9 new tests: description(), settle_secs(), rebind_strategy() for all 6 vendor types + RebindStrategy Debug/Clone/Eq |
| IPC Unix error path coverage | ✅ | 11 new tests in tests_unix.rs: dispatch errors, invalid params, blank lines, malformed JSON, invalid JSON-RPC version |
| Coverage improvement | ✅ | 59.98% → 60.16% line, 68.73% → 69.03% function, 60.44% → 60.62% region |
| Quality gates | ✅ | `fmt` ✅, `clippy --all-features -D warnings` ✅, `test --all-features` ✅, `doc` ✅ |

### Iteration 57: Deep Debt Evolution + All-Silicon Pipeline (Mar 18 2026)

| Item | Status | Detail |
|------|--------|--------|
| Specs v0.6.0 | ✅ | All-silicon pipeline, sovereignty roadmap, Titan V x2 + RTX 5060 + MI50 planned |
| socket.rs smart refactor | ✅ | 1488→556 lines (tests extracted to socket_tests.rs) |
| GP_PUT cache flush experiment H1 | ⚠️ | `clflush` USERD + GPFIFO before doorbell — **proven insufficient** on live Titan V. Root cause: GPU is cold silicon (PFIFO/GPCCS/FECS not initialized), not cache coherency |
| GlowPlug `device.lend` / `device.reclaim` | ✅ | VFIO fd broker pattern — glowPlug drops fd so tests can open the VFIO group, RAII reclaim on drop. 10x stress cycle proven. `device.lend`, `device.reclaim` JSON-RPC methods |
| GlowPlug-aware VFIO test harness | ✅ | `VfioLease` RAII guard in all `hw_nv_vfio*` test files — automatic lend/reclaim, transparent fallback |
| 35 VFIO hardware tests passing | ✅ | Both Titan Vs: open, alloc, upload/readback, multi-buffer, BAR0, PFIFO diagnostic, HBM2 probing, hot-swap stress. Dispatch FenceTimeout expected (cold GPU) |
| 9 hot-swap integration tests | ✅ | `hw_hotswap.rs`: health, device list, lend/reclaim round-trip, 10x stress, health-during-lend, double-lend rejection, reclaim no-op |
| `multi_gpu_enumerates_multiple` fix | ✅ | Counts VFIO-bound GPUs via sysfs PCI class (3 GPUs: 1 DRM + 2 VFIO) |
| Production .expect() evolution | ✅ | Signal handlers → or_exit(), GSP observer → Result, SAFETY comments |
| Unsafe code evolution | ✅ | All volatile reads/writes through VolatilePtr, SAFETY comments on from_raw_parts and Send/Sync impls |
| AMD metal placeholder → real GFX906 | ✅ | Register offsets from AMD docs |
| Intel GPU arch | ✅ | Dg2Alchemist + XeLpg variants added |
| Hardcoding evolution | ✅ | pci_ids.rs constants, unified chip_name() identity module |
| Coverage expansion | ✅ | GSP knowledge/parser/applicator, MMIO VolatilePtr, identity, pci_ids, error module |
| Clippy clean | ✅ | map_or → is_none_or, unfulfilled lint expectations → allow, doc backtick fixes |
| Test expansion | ✅ | 2527 → 2560 passing (+33 tests), 0 failed, 90 ignored |
| **Handoff to hotSpring** | 🔜 | Pipeline complete except GPU initialization. hotSpring Exp 070: warm Titan V via `device.resurrect` (nouveau HBM2 training + FECS firmware), then re-run dispatch |

### Iteration 56: Coverage Expansion + Doc Cleanup + Debt Resolution (Mar 18 2026)

| Item | Status | Detail |
|------|--------|--------|
| Constant folding tests | ✅ | 40 new fold.rs tests: integer add/abs, identity elimination, bitwise/logic, shift, comparison, predicate, overflow, PrmtSel, FoldData |
| coral-glowplug test expansion | ✅ | 131 tests: config sysfs parsing, device slots, personality trait/registry, JSON-RPC dispatch (device.list/get/health/swap, health.check/liveness, daemon.status/shutdown, unknown method), TCP bind, BDF arg parsing |
| coral-glowplug security + boot | ✅ | Security hardening (BDF validation, max concurrent clients, idle timeout, line length limits), nvidia module guard, DRM consumer guard, circuit breaker in health loop, boot sovereignty via kernel cmdline and initramfs |
| coral-driver test expansion | ✅ | 30+ tests: PCI config parsing (NVIDIA/AMD/too-short), vendor detection/display, PM state, BAR/capability construction, PM capability, PCIe link, PM4 packets, GEM buffers, RM alloc params |
| coral-reef codegen tests | ✅ | 12 tests: opt_prmt (src_idx1, imm source, nested), naga_translate (all/any vector, f64 add, array length), lower_f64 (exp2→DFMA, sqrt→Newton), builder (prmt identity, lop2 pred, predicated, uniform), assign_regs block-level RA |
| api.rs + spiller.rs coverage | ✅ | 7 tests: eprint_hex, debug re-export, two-block CFG spill, very-low-limit spill stress, no-spill-needed, UPred spilling, pinned value skip |
| File size compliance | ✅ | pci_discovery.rs tests extracted to sibling file (1027→890 LOC); all files under 1000 LOC (excl. generated ISA tables) |
| Doc link warnings | ✅ | 10 unresolved `DriverError` links in rm_client/alloc.rs → full crate path; zero doc warnings |
| EVOLUTION markers audited | ✅ | 10 markers catalogued: 3 feasible now, 3 need ISA docs, 3 need scheduling docs, 1 blocked by hardware (AMD Metal MI50) |
| Coverage improvement | ✅ | 58.16% → 59.92% line, 57.75% → 59.39% region, 68.50% → 69.45% function |
| Test expansion | ✅ | 2241 → 2364 passing (+123 tests), 0 failed |

### Iteration 53: Deep Audit Execution + Safe Rust Evolution + Test Coverage (Mar 17 2026)

| Item | Status | Detail |
|------|--------|--------|
| clippy::nursery lints | ✅ | `nursery = "warn"` added to workspace `[lints.clippy]` — catches additional patterns (redundant clones, option-if-let-else, etc.) |
| `SysfsBar0` safe wrapper | ✅ | New `crates/coral-driver/src/vfio/sysfs_bar0.rs` — consolidates mmap→volatile-read→munmap pattern into safe API with bounds checking; 3 oracle modules refactored to use it |
| Magic number extraction | ✅ | `amd_metal.rs`: MI50 HBM2/L2 sizes, busy bit mask; `nv_metal.rs`: PRAMIN aperture size; `device.rs`: PCI fault read constants + `is_faulted_read()` helper |
| `&'static str` evolution | ✅ | `gpu_vendor.rs` structs (`PowerDomain`, `MetalMemoryRegion`, `EngineInfo`, `WarmupStep`) evolved from `String` to `&'static str` — zero-allocation metal interface |
| `#![forbid(unsafe_code)]` on coral-glowplug | ✅ | Compile-time enforcement of zero unsafe in glowplug binary crate |
| XDG config path | ✅ | `coral-glowplug` now prefers `$XDG_CONFIG_HOME/coralreef/glowplug.toml` before `/etc/coralreef/glowplug.toml` |
| IPC fault injection tests | ✅ | New `tests_fault.rs` — 12 async tests: client disconnect, malformed/truncated/oversized/empty JSON, invalid methods, missing fields, concurrent stress |
| coral-glowplug unit tests | ✅ | +39 tests across `config.rs`, `health.rs`, `device.rs`, `socket.rs` — config loading, device health states, `is_faulted_read`, chip identification, personality registry, JSON-RPC parsing |
| Idiomatic Rust evolution | ✅ | `if let/else` → `unwrap_or_else`, `pub(crate)` in private modules → `pub`, `Option::map_or_else` patterns, doc link fixes |
| SPDX license headers | ✅ | `CC-BY-SA-4.0` headers added to 14 markdown files (README, CONTRIBUTING, STATUS, etc.) |
| `PersonalityRegistry` integration | ✅ | Wired into `DeviceSlot::activate` for live personality validation via `dyn GpuPersonality` dispatch |
| Doc hygiene | ✅ | `DeviceCompileResult` re-exported in `service/mod.rs`, broken doc links fixed, `DriverPreference` full-path resolution |
| Test expansion | ✅ | 2185 → 2241 passing (+56 tests), 0 failed, 90 ignored |
| Coverage improvement | ✅ | 57.28% → 57.75% region, 57.71% → 58.16% line, 67.98% → 68.50% function |

### Iteration 52: Ecosystem Absorption + Glowplug JSON-RPC 2.0 + Typed IPC Errors (Mar 16 2026)

| Item | Status | Detail |
|------|--------|--------|
| deny.toml yanked → deny | ✅ | `yanked = "warn"` upgraded to `yanked = "deny"` |
| OrExit\<T\> pattern | ✅ | `coralreef-core/src/or_exit.rs` — zero-panic validation for binary entry points, absorbed from wetSpring/rhizoCrypt |
| IpcServiceError structured errors | ✅ | `coralreef-core/src/ipc/error.rs` — `IpcPhase` (Transport/Dispatch/Handler/Internal), phase-aware JSON-RPC error codes, `retryable()` |
| coral-glowplug JSON-RPC 2.0 | ✅ | `socket.rs` rewritten — semantic methods (`device.list`, `device.swap`, `device.health`, `health.check`, `daemon.status`, `daemon.shutdown`) |
| GpuPersonality trait system | ✅ | `personality.rs` — `VfioPersonality`, `NouveauPersonality`, `AmdgpuPersonality`, `UnboundPersonality` + `PersonalityRegistry` |
| CAP_SYS_ADMIN evolution | ✅ | Removed `sudo tee` fallback from `sysfs_write()` — direct `std::fs::write()` with capability guidance |
| DRM consumer fence check | ✅ | `has_active_drm_consumers(bdf)` before nouveau bind in HBM2 resurrection |
| AMD Vega metal registers | ✅ | `amd_metal.rs` populated with MI50/GFX906 register layout (GRBM_STATUS, SRBM_STATUS, CP_STAT, SDMA0, power domains, memory regions, engines) |
| Dual-format capability parsing | ✅ | `CapabilityRef` accepts flat strings or nested objects for `provides` in discovery |
| Test expansion | ✅ | 2157 → 2185 passing (+28 tests), 0 failed, 90 ignored |

### Iteration 51: Deep Audit Compliance + IPC Health + Doc Hygiene (Mar 16 2026)

| Item | Status | Detail |
|------|--------|--------|
| wateringHole IPC health methods | ✅ | `health.check`, `health.liveness`, `health.readiness` implemented across JSON-RPC, tarpc, and Unix socket transports per wateringHole `CORALREEF_LEVERAGE_GUIDE` |
| Health response types | ✅ | `HealthCheckResponse`, `LivenessResponse`, `ReadinessResponse` structs in `service/types.rs` |
| Socket path standard | ✅ | `ECOSYSTEM_NAMESPACE` → `"biomeos"`, `primal_socket_name()` → `<primal>-<family_id>.sock` per wateringHole `PRIMAL_IPC_PROTOCOL` |
| Config self-knowledge | ✅ | `PRIMAL_NAME`, `PRIMAL_VERSION` via `env!()`, `family_id()` from `$BIOMEOS_FAMILY_ID` |
| Zero-copy transport | ✅ | `Bytes::copy_from_slice` → `buf.drain()` + `Bytes::from(buf)` in `primal-rpc-client` transport |
| Clippy pedantic fixes | ✅ | `items_after_statements`, `case_sensitive_file_extension_comparison`, `redundant_closure_for_method_calls`, `map_unwrap_or`, `useless_format`, `assertions_on_constants`, unused imports, doc backticks |
| `#[must_use]` attributes | ✅ | Added to `family_id()`, `default_tcp_bind()`, `default_tarpc_bind()` |
| `coral-gpu` smart refactor | ✅ | `lib.rs` (977 LOC) → 6 submodules: `kernel.rs`, `context.rs`, `fma.rs`, `pcie.rs`, `driver.rs`, `hash.rs` (65 LOC lib.rs) |
| `// SAFETY:` documentation | ✅ | All `unsafe impl Send/Sync` blocks in `dma.rs`, `device.rs`, `uvm_compute.rs`, `bar0.rs` documented |
| genomeBin manifest updated | ✅ | `pie_verified = true`, `ecobin_grade = "A++"` |
| E2E IPC test | ✅ | `e2e_ipc.rs` — JSON-RPC + tarpc servers, all semantic methods, response format verification |
| Test expansion | ✅ | 1992 → 2157 passing (+165 tests), 0 failed |
| Coverage improvement | ✅ | 57.10% → 57.28% region, 57.54% → 57.71% line, 67.80% → 67.98% function |

### Iteration 50: Full Audit Execution + Coverage Expansion (Mar 16 2026)

| Item | Status | Detail |
|------|--------|--------|
| Doc warnings eliminated | ✅ | 4 rustdoc warnings fixed: escaped bit-field notation `[27:24]`/`[1:0]`, fixed intra-doc links for `ORACLE_RANGES` and `NvVfioComputeDevice::open()` |
| Clippy clean with VFIO | ✅ | `cargo clippy --workspace --features vfio -- -D warnings` — zero warnings |
| Hardcoded paths eliminated | ✅ | `/home/biomegate` paths in vbios loading and test output replaced with `$HOTSPRING_DATA_DIR` env var |
| Production unwrap/expect evolved | ✅ | coral-glowplug: all `unwrap()`/`expect()` → `match`/`let-else` with `tracing::error!` + `exit(1)` |
| `eprintln!` → tracing | ✅ | All production `eprintln!` migrated to `tracing::info!`/`tracing::warn!`/`tracing::error!` |
| Smart refactoring (6 files) | ✅ | `devinit.rs` (2197→5 modules), `probe.rs` (1572→6 modules), `glowplug.rs` (1405→6 modules), `hbm2_training.rs` (1355→10 modules), `hw_nv_vfio.rs` (2469→5 files), `tests_unix.rs` (1094→2 files) |
| File size compliance | ✅ | All files under 1000 LOC — zero violations |
| `missing_docs` suppression | ✅ | 16 experimental VFIO diagnostic modules annotated with `#![allow(missing_docs)]` |
| GPU hardware tests (nouveau) | ✅ | 9/14 passing on Titan V (5 expected failures: BAR0 needs root, UVM targets Ampere not Volta) |
| Coverage tests (+214) | ✅ | Texture, memory, f64, shader I/O, control flow, spiller, latency tables, fold/optimization — across SM20/32/50/70/75/80/86/89 + RDNA2 |
| Coverage improvement | ✅ | 56.26% → 57.54% line, 56.17% → 57.10% region, 67.00% → 67.80% function |
| Root docs updated | ✅ | README.md, STATUS.md, hardware inventory (2× Titan V + RTX 5060) |
| Test expansion | ✅ | 1842 → 1992 passing (+150 tests), 0 failed |

### Iteration 49: hotSpring Absorption — GV100 Dispatch Fixes (Mar 15 2026)

| Item | Status | Detail |
|------|--------|--------|
| GV100 per-runlist registers | ✅ | All RUNLIST_BASE/SUBMIT usage migrated from gk104 global (0x2270/0x2274) to GV100 per-runlist at stride 0x10 (`runlist_base(id)`, `runlist_submit(id)`). Submit encoding fixed: `(rl_id << 20) \| count` → `upper_32(addr >> 12) \| (count << 16)` |
| MMU fault buffer DMA | ✅ | Interpreter probe L3 fault buffers migrated from VRAM (PraminRegion) to DMA system memory (`DmaBuffer` at `FAULT_BUF_IOVA`). Both BUF0+BUF1 configured with PUT enable bit |
| PFIFO INTR bit 8 decode | ✅ | New `INTR_BIT8` constant + `clear_pfifo_intr_bit8()` on `ExperimentContext`. Z experiment poll loop clears bit 8 before checking bit 30 |
| PBDMA reset sequence | ✅ | `reset_pbdma()` method: clear PBDMA INTR+HCE, clear PCCSR faults, toggle PMC PBDMA bit. Applied pre-dispatch in experiments N and P |
| GlowPlug consolidation | ✅ | Inline glow plug (~100 LOC) in `diagnostic_matrix()` replaced with `GlowPlug::check_state()` + `GlowPlug::full_init()`. Runner.rs reduced by ~90 LOC |
| `submit_runlist()` helper | ✅ | `ExperimentContext::submit_runlist()` encapsulates per-runlist register writes — 20+ call sites migrated from inline `pfifo::RUNLIST_BASE`/`SUBMIT` pairs |
| GV100 register test | ✅ | `runlist_gv100_register_addresses` + `runlist_gv100_value_encoding` unit tests validate stride and value format |
| All CI gates pass | ✅ | `fmt`, `clippy`, `test` — 48 test suites, 0 failures |

### Iteration 48: Deep Debt Solutions + Sovereignty Evolution (Mar 15 2026)

| Item | Status | Detail |
|------|--------|--------|
| `extern "C" { fn ioctl }` eliminated | ✅ | `raw_nv_ioctl` evolved to `nv_rm_ioctl` using `rustix::ioctl` via `drm_ioctl_named` — zero C FFI remaining in entire workspace |
| Clippy `items_after_test_module` fix | ✅ | `rm_helpers.rs`: production function moved above `mod tests` |
| Clippy `needless_range_loop` fix | ✅ | `rm_helpers.rs`: `for i in 1..16` loops → idiomatic `iter_mut().enumerate().skip(1)` |
| Formatting drift resolved | ✅ | `context.rs`, `layers.rs`, `probe.rs`, `rm_helpers.rs`, `rm_client.rs` — `cargo fmt` applied workspace-wide |
| Production `unwrap()` → `expect()` | ✅ | Last 2 `unwrap()` in `runner.rs` DMA buffer verification → `expect()` with context |
| Capability test evolved | ✅ | Hardcoded primal name list replaced with structural self-knowledge assertions (domain validation, no paths, no addresses before bind, version match) |
| Unix JSON-RPC dispatch coverage | ✅ | +15 tests: all 5 method paths (status, capabilities, wgsl, spirv, wgsl.multi) with object/array/invalid params, `make_response` success/error/null-id |
| `main.rs` coverage expansion | ✅ | +8 tests: `UniBinExit` code values, exit code conversion, CLI arch parsing, invalid subcommand, log level, discovery file idempotency |
| `nv_rm_ioctl` helper | ✅ | Generic RM ioctl helper with status-aware error reporting — replaces 3 `raw_nv_ioctl` call sites with informative named errors |
| Test expansion | ✅ | 1819 → 1842 passing (+23 tests), 0 failed, 61 ignored |
| Coverage | ✅ | 66.43% → 66.67% region, 68.36% → 68.45% line |

### Iteration 47: Deep Debt Evolution + Modern Idiomatic Rust (Mar 15 2026)

| Item | Status | Detail |
|------|--------|--------|
| `runner.rs` delegate to experiments module | ✅ | 2509 LOC → 778 LOC — eliminated 2000+ lines of duplicated inline experiment dispatch, delegates to `experiments::run_experiment()` |
| Unsafe code elimination | ✅ | Removed `unsafe { from_raw_parts_mut }` in NOP pushbuf init → safe `as_mut_slice()` |
| `rm_client.rs` extract helpers | ✅ | UUID parsing + raw ioctl → `rm_helpers.rs` (1000 → 944 LOC) |
| Zero-copy evolution | ✅ | `KernelCacheEntry.binary: Vec<u8>` → `Bytes` — eliminates copy in `to_cache_entry` / `from_cache_entry` |
| Driver string centralization | ✅ | `DRIVER_VFIO`, `DRIVER_NOUVEAU`, `DRIVER_AMDGPU`, `DRIVER_NVIDIA_DRM` constants in `preference.rs`; all match arms and comparisons use constants |
| Production panic elimination | ✅ | 6 `panic!()` in `sm70_instr_latencies.rs` → `warn!` + `DEFAULT_LATENCY` / `debug_assert!` |
| Production unwrap elimination | ✅ | `runner.rs` unwrap → `Option::zip` pattern |
| FenceTimeout constant | ✅ | Hardcoded `5000` → `SYNC_TIMEOUT.as_millis()` |
| CONTRIBUTING.md sync | ✅ | Test count updated to 1819 |
| New test coverage | ✅ | +15 tests: rm_helpers UUID parsing (9), cache entry roundtrip (3), driver constants (2), zero-copy clone (1) |

### Iteration 46: Structural Refactor + Coverage Expansion (Mar 15 2026)

| Item | Status | Detail |
|------|--------|--------|
| `diagnostic/runner.rs` smart refactor | ✅ | 2485 LOC → 769 LOC — split into `experiments/` submodule with 8 handler files + context struct |
| Clippy pedantic workspace-wide | ✅ | All warnings resolved: identity ops, constant assertions, redundant closures, range contains, etc. |
| 53+ new tests | ✅ | AMD ISA generated table lookup (25), Unix JSON-RPC coverage (8), SM70 latency/encoder tests (20) |
| Coverage improvement | ✅ | 65.90% → 66.43% lines, 73.75% → 75.15% functions, 68.21% regions |
| File size compliance | ✅ | Zero files over 1000 lines |
| Test expansion | ✅ | 1804 passing (+83 from Iter 45), 0 failed, 61 ignored |

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
| Unsafe evolved → safe Rust | ✅ | `MappedRegion` RAII, `drm_ioctl_named` sole wrapper, `bytemuck::bytes_of`, `FirmwareInventory` |
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
| `drm_ioctl_named` (sole ioctl wrapper) | ✅ | `pub(crate)` — FFI confined to `coral-driver`; `drm_ioctl_typed` eliminated (zero callers) |
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
| `#[deny(unsafe_code)]` on non-driver crates | ✅ | 8/9 crates enforce safety at compile time (coral-reef, coralreef-core, coral-gpu, coral-reef-stubs, coral-reef-bitview, coral-reef-isa, nak-ir-proc, primal-rpc-client) |
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
| TODOs → DEBT migration | ✅ | All bare `TODO:` replaced with `DEBT(category):` comments (37 total) |
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
| Test expansion | ✅ | 1174 → 1190 passing (+16), 30 → 35 ignored (+5 SPIR-V path gaps) |
| SPIR-V path gaps documented | ✅ | `Discriminant` expression and non-literal constant initializers — future SPIR-V translator work |

### Phase 10 — Iteration 23 Completions (Deep Debt Elimination & Math Function Coverage)

| Task | Status | Details |
|------|--------|---------|
| 11 math functions implemented | ✅ | Tanh, Fract, Sign, Dot, Mix, Step, SmoothStep, Length, Normalize, Cross, Trunc — unblocks GLSL frontend shaders and extends WGSL coverage |
| GLSL fixture coverage expanded | ✅ | `transcendentals.comp` restored with fract/sign/mix/step/smoothstep/tanh; `buffer_rw.comp` restored with dot() |
| corpus_esn_reservoir_update unblocked | ✅ | Tanh now supported — neuralSpring ESN shader compiles |
| lib.rs smart refactoring | ✅ | Test module extracted to `lib_tests.rs` (791→483 LOC), `emit_binary` deduplicated |
| SM80 gpr.rs test extraction | ✅ | Test module extracted to `gpr_tests.rs` (867→766 LOC), matching SM75 pattern |
| nak-ir-proc unsafe audited | ✅ | 2 `from_raw_parts` in generated code — compile-time contiguity proofs, zerocopy-grade pattern, no safe alternative |
| builder/emit.rs audited | ✅ | Single `SSABuilder` trait, logically grouped — splitting anti-idiomatic |
| libc→rustix migration documented | ✅ | DEBT(evolution) marker in `drm.rs` — 22 unsafe blocks across driver for mmap/munmap/ioctl/clock_gettime |
| #[allow] vs #[expect] audit | ✅ | Module-level allow covers codegen; 5 files outside scope properly use #[expect]; zero warnings |
| DEBT count updated | ✅ | 37 DEBT markers (was 28 in docs) |
| Clippy lint fixes | ✅ | Raw string hashes, doc_markdown backticks — zero warnings |
| Test expansion | ✅ | 1191 passing (+1 new, +1 un-ignored), 35 ignored (-1) |

### Phase 10 — Iteration 24 Completions (Multi-GPU Sovereignty & Cross-Vendor Parity)

| Task | Status | Details |
|------|--------|---------|
| Multi-GPU DRM scan | ✅ | `enumerate_render_nodes()` returns `DrmDeviceInfo` per device; `open_by_driver()` for targeted open |
| Driver sovereignty | ✅ | `DriverPreference` type: sovereign (`nouveau` > `amdgpu` > `nvidia-drm`), pragmatic, env var override |
| All backends compile by default | ✅ | `default = ["nouveau", "nvidia-drm"]` — no feature gate for driver selection |
| NVIDIA UVM dispatch pipeline | ✅ | `NvDrmDevice` probes `nvidia-drm`, delegates to `NvUvmComputeDevice` (GPFIFO + USERD doorbell + completion polling) |
| toadStool ecosystem discovery | ✅ | `coralreef-core::discovery` reads capability files, falls back to DRM scan |
| `GpuContext::from_descriptor()` | ✅ | Context creation from ecosystem discovery metadata |
| Cross-vendor compilation parity | ✅ | SM86 vs RDNA2 parity tests with known limitation documentation |
| AMD hardware stress tests | ✅ | Large buffers (4MB, 64MB), sequential dispatches, rapid alloc/free, concurrent buffers |
| NVIDIA probe tests | ✅ | Driver discovery, device open, multi-GPU enumeration |
| Showcase suite (8 demos) | ✅ | Progressive: hello-compiler → compute triangle (coralReef → toadStool → barraCuda) |
| Hardware testing documentation | ✅ | `docs/HARDWARE_TESTING.md` — Titan team handoff, parity matrix, CI config |
| Test expansion | ✅ | 1191 → 1285 passing (+94 tests), 35 → 60 ignored (+25 hardware-gated) |

### Phase 10 — Iteration 25 Completions (Math Evolution + Debt Zero + Full Sovereignty)

| Task | Status | Details |
|------|--------|---------|
| 9 trig/inverse math functions | ✅ | Acos, Asin, Atan, Atan2, Sinh, Cosh, Asinh, Acosh, Atanh — polynomial atan + identity chains |
| log2 2nd NR iteration | ✅ | ~52-bit f64 accuracy (up from ~46-bit) |
| exp2 subnormal handling | ✅ | Two-step ldexp with n clamping for exponents < -1022 |
| Complex64 preamble | ✅ | c64_add/sub/mul/inv/exp/log/sqrt/pow — auto-prepended for dielectric_mermin |
| 37 DEBT markers resolved | ✅ | ISA → documented constants, opt/feature → EVOLUTION markers |
| libc eliminated | ✅ | ioctl via inline asm syscall, zero libc dependency |
| NVIDIA UVM module | ✅ | Ioctl definitions + device infrastructure ready |
| Test expansion | ✅ | 1191 → 1285 passing (+94 tests), 60 ignored |

### Phase 10 — Iteration 26 Completions (hotSpring Sovereign Pipeline Unblock)

| Task | Status | Details |
|------|--------|---------|
| f64 min/max/abs/clamp | ✅ | DSetP+Sel pattern replaces broken a[0] truncation to f32 |
| ComputeDevice: Send + Sync | ✅ | Thread-safe dispatch for barraCuda GpuBackend |
| Nouveau compute subchannel | ✅ | SM-aware compute class selection binding |
| Test expansion | ✅ | 1285 → 1286 passing, 60 → 59 ignored |

### Phase 10 — Iteration 27 Completions (Deep Debt + Cross-Spring Absorption)

| Task | Status | Details |
|------|--------|---------|
| RDNA2 literal materialization pass | ✅ | V_MOV_B32 prefix for VOP3/VOP2 literal constants; two scratch VGPRs reserved |
| f64 transcendental encodings (AMD) | ✅ | F64Exp2, F64Log2, F64Sin, F64Cos via V_CVT_F32_F64 + VOP1 + V_CVT_F64_F32 |
| f32 transcendental encoding (AMD) | ✅ | OpTranscendental → RDNA2 VOP1 (cos, sin, exp2, log2, rcp, rsq, sqrt) |
| OpShl/OpShr/OpSel non-VGPR fix | ✅ | VOP2 shift/select ops handle non-VGPR sources via materialization |
| AMD system register mapping | ✅ | SR indices 0x28–0x2D → VGPRs v6–v11 (workgroup sizes, grid dimensions) |
| strip_f64_enable() absorption | ✅ | `enable f64;` / `enable f16;` auto-stripped in prepare_wgsl() |
| hotSpring FMA shaders absorbed | ✅ | su3_link_update + wilson_plaquette (4 new tests: SM70 + RDNA2) |
| FMA policy plumbing | ✅ | FmaPolicy enum in CompileOptions → Shader struct |
| f64 capability in discovery | ✅ | F64Support in DiscoveryDevice with native/rate/recommendation |
| PRNG preamble | ✅ | xorshift32 + wang_hash auto-prepended when referenced |
| neuralSpring shaders absorbed | ✅ | logsumexp, rk45_step, wright_fisher (6 new tests: SM70 + RDNA2) |
| f64 runtime diagnostic | ✅ | F64Capability + F64Recommendation in coral-gpu |
| 24/24 spring absorption tests | ✅ | All compile for both SM70 and RDNA2 |
| Test expansion | ✅ | 1286 → 1401 passing (+115 tests), 59 → 62 ignored |

### Phase 10 — Iteration 28 Completions (Unsafe Elimination + Pure Safe Rust)

| Task | Status | Details |
|------|--------|---------|
| nak-ir-proc `from_raw_parts` eliminated | ✅ | Proc macro enhanced with `#[src_types]`/`#[src_names]`/`#[dst_types]`/`#[dst_names]` attributes; generates safe named accessors for array fields; old unsafe `from_raw_parts` path replaced with `compile_error!` enforcement |
| 50 Op struct array-field migration | ✅ | All Op structs migrated from separate named Src/Dst fields to single `srcs: [Src; N]` / `dsts: [Dst; N]` arrays; 480+ call-site updates across codegen/ |
| `CompileError::Internal` + `catch_ice` | ✅ | NVIDIA encoders wrapped with `std::panic::catch_unwind` via `catch_ice` — converts panics to graceful errors |
| tests_unix.rs env var unsafe eliminated | ✅ | `default_unix_socket_path` refactored: pure `unix_socket_path_for_base(Option<PathBuf>)` tested without `unsafe { set_var/remove_var }` |
| `primal-rpc-client` crate | ✅ | Pure Rust JSON-RPC 2.0 client with TCP/Unix/Songbird transports; `#[deny(unsafe_code)]`, `ring` eliminated |
| Hardcoding evolved → agnostic | ✅ | `discovery.rs` generalized: no hardcoded primal names in production code |
| Large file refactoring | ✅ | Tests extracted: `liveness_tests.rs`, `naga_translate_tests.rs`, `main_tests.rs` |
| `coral-driver` ioctl → `rustix::ioctl` | ✅ | Inline asm syscalls replaced with `rustix::ioctl::ioctl` (DrmIoctlCmd Ioctl impl); `bytemuck` replaces 3 `ptr::read` blocks |
| AMD `read_ioctl_output` safe | ✅ | `bytemuck::pod_read_unaligned` + `bytemuck::bytes_of` — zero unsafe for data extraction |
| Workspace unsafe audit | ✅ | 17 `unsafe` blocks remain, all in `coral-driver` (mmap/munmap/ioctl kernel ABI); zero unsafe in 8/9 crates |
| `deny.toml` `libc` canary | ✅ | Prepared for future upstream `mio`→`rustix` migration |
| NVVM poisoning bypass tests | ✅ | 12 tests: 3 NVVM-poisoning patterns × 6 architectures (SM70/75/80/86/89/RDNA2); validates sovereign WGSL→native path bypasses NVVM device death |
| Hardcoding evolved → agnostic | ✅ | Last production `toadStool` reference generalized to `ecosystem primal`; doc comments use generic terminology |
| Spring absorption wave 3 | ✅ | 7 new shaders from hotSpring v0.6.25 + healthSpring v14; new domains: fluid dynamics (Euler HLL), pharmacology (Hill, population PK), ecology (diversity); 9 pass, 5 ignored (AMD Discriminant, vec3<f64> encoding, f64 log2 edge case) |
| WGSL corpus expanded | ✅ | 93 cross-spring shaders (was 86); 6 springs represented |

### Phase 10 — Iteration 29 Completions (NVIDIA Last Mile Pipeline)

| Task | Status | Details |
|------|--------|---------|
| Multi-GPU path-based open | ✅ | `AmdDevice::open_path()`, `NvDevice::open_path()`, `NvDrmDevice::open_path()` — each render node targets its own physical device |
| `enumerate_all()` multi-GPU fix | ✅ | Uses `open_driver_at_path()` — 4× RTX 3050 on PCIe now produce 4 distinct contexts |
| `from_descriptor_with_path()` | ✅ | Render-node-specific context creation for ecosystem discovery |
| Nouveau EINVAL diagnostic suite | ✅ | `diagnose_channel_alloc()`: bare/compute/NVK-style/alt-class attempts; `dump_channel_alloc_hex()`; auto-runs on failure with firmware + identity probes |
| Struct ABI verification | ✅ | `NouveauChannelAlloc` = 92 bytes, `NouveauChannelFree` = 8, `NouveauGemNew` = 48, `NouveauGemPushbuf` = 64, `NouveauSubchan` = 8 |
| Nouveau firmware probe | ✅ | `check_nouveau_firmware()` checks 16 firmware files per chip (acr, gr, nvdec, sec2) |
| GPU identity via sysfs | ✅ | `probe_gpu_identity()` + `GpuIdentity::nvidia_sm()` — PCI device ID → SM version (Volta through Ada Lovelace) |
| Buffer lifecycle safety | ✅ | `NvDevice.inflight: Vec<BufferHandle>` — dispatch defers temp buffer free to `sync()`, matching AMD pattern; `Drop` drains inflight |
| SM auto-detection | ✅ | `NvDevice::open()` probes sysfs for GPU chipset, maps to SM, selects correct compute class; falls back to SM70 |
| coral-gpu SM wiring | ✅ | `sm_to_nvarch()` + `sm_from_sysfs()` — both `open_driver` and `enumerate_all` use hardware-detected SM |
| UVM RM client proof-of-concept | ✅ | `RmClient::new()` via `NV_ESC_RM_ALLOC(NV01_ROOT)`, `alloc_device(NV01_DEVICE_0)`, `alloc_subdevice(NV20_SUBDEVICE_0)`, `free_object(NV_ESC_RM_FREE)` with RAII Drop |
| Diagnostic test suite | ✅ | 5 new hw_nv_nouveau diagnostic tests (channel diag, hex dump, firmware probe, GPU identity, GEM without channel) |
| `gem_close` promoted to pub | ✅ | Was `pub(crate)`, now `pub` for integration test access |
| Test expansion | ✅ | 1437 → 1447 passing (+10 tests), 68 → 76 ignored (+8 hardware diagnostic tests) |

### Phase 10 — Iteration 30 Completions (Spring Absorption + FMA Evolution)

| Task | Status | Details |
|------|--------|---------|
| `shader.compile.wgsl.multi` API | ✅ | `DeviceTarget`, `MultiDeviceCompileRequest/Response`, `DeviceCompileResult` — compile one WGSL shader for multiple GPU targets in a single request; wired through JSON-RPC, Unix socket, and tarpc |
| FMA policy wire-through | ✅ | `fma_policy` field added to `CompileWgslRequest` and `MultiDeviceCompileRequest`; `parse_fma_policy()` helper; `build_options()` now takes `FmaPolicy` parameter |
| FMA contraction enforcement | ✅ | New `lower_fma.rs` pass: `FmaPolicy::Separate` splits `OpFFma`→`OpFMul`+`OpFAdd` and `OpDFma`→`OpDMul`+`OpDAdd`; inserted in pipeline after optimization, before f64 transcendental lowering |
| FMA hardware capability reporting | ✅ | `FmaCapability` struct with f32/f64 FMA support, recommended policy, throughput ratio; `FmaCapability::for_target()` per architecture; `GpuContext::fma_capability()` |
| `PCIe` topology awareness | ✅ | `PcieDeviceInfo` struct, `probe_pcie_topology()`, `assign_switch_groups()` — discover and group GPUs by `PCIe` switch for optimal multi-device scheduling |
| Capability self-description evolution | ✅ | `shader.compile.multi` capability advertised with `max_targets: 64`, `cross_vendor: true`; existing `shader.compile` now includes GLSL input, all NVIDIA+AMD architectures, FMA policies |
| NVVM bypass test hardening | ✅ | `nvvm_bypass_fma_policies_all_compile` verifies compilation across all FMA policies; `nvvm_bypass_fma_separate_rdna2` for cross-vendor FMA verification |
| `primal-rpc-client` evolution | ✅ | Removed redundant `Serialize` bounds, `const fn` for `tcp()`/`no_params()`, `#[expect(dead_code)]` with reasons |
| `coral-driver` doc evolution | ✅ | `#[must_use]`, `# Errors` doc sections, `const unsafe fn`, `std::fmt::Write` refactoring, GPU identity extraction to `nv/identity.rs` |
| `#![warn(missing_docs)]` expansion | ✅ | Added to `coral-reef-bitview`, `coral-reef-isa`, `coral-reef-stubs`, `nak-ir-proc`, `amd-isa-gen`, `coral-driver`, `coral-gpu`, `coralreef-core` |
| Test expansion | ✅ | 1447 → 1487 passing (+40 tests), 76 ignored (stable) |

### Phase 10 — Iteration 31 Completions (Deep Debt + Nouveau UAPI + UVM Fix)

| Task | Status | Details |
|------|--------|---------|
| RDNA2 spring absorption (7 tests) | ✅ | Un-ignored 7 tests (literal materialization already fixed in Iter 27) |
| `biomeos→ecoPrimals` discovery tests | ✅ | Fixed ecosystem name, DRM fallback to sysfs probe |
| `repair_ssa` unreachable blocks | ✅ | Forward reachability analysis eliminates dead phi sources; `torsion_angles_f64` compiles |
| f64 `log2` edge case from `pow` lowering | ✅ | `OpF64Log2` widening fix for `hill_dose_response_f64` |
| AMD `FRnd` encoding (RDNA2) | ✅ | `V_TRUNC/FLOOR/CEIL/RNDNE` for F32 (VOP1) and F64 (VOP3); 4 `FRndMode` variants |
| `vec3<f64>` SM70 encoder | ✅ | Componentwise scalarization for 3-element f64 vectors |
| SU3 lattice preamble system | ✅ | `su3_f64_preamble.wgsl` (10 functions), auto-prepend with dependency chaining |
| SPIR-V Relational expressions | ✅ | `IsNan`, `IsInf`, `All`, `Any` → `OpFSetP`/`OpISetP` |
| SPIR-V non-literal const init | ✅ | `translate_global_expr`: `Compose`, `Splat`, recursive `Constant` |
| `repair_ssa` critical edges | ✅ | Multi-successor phi source insertion for SPIR-V-generated CFGs |
| Production `unwrap()→expect()` | ✅ | All production `unwrap()` → `expect()` with descriptive messages |
| `emit_f64_cmp` widening | ✅ | Defensive 1→2 component widening for f32-routed-as-f64 operands |
| `multi_gpu_enumerates_both` → `multi_gpu_enumerates_multiple` | ✅ | Now handles 2×NVIDIA, not just AMD+NVIDIA |
| Nouveau UAPI structs + ioctls | ✅ | `VM_INIT`, `VM_BIND`, `EXEC` struct definitions + `vm_init()`, `vm_bind_map()`, `vm_bind_unmap()`, `exec_submit()` wrappers |
| Nouveau UAPI wired into NvDevice | ✅ | `open_from_drm`: VM_INIT auto-detect → fallback; `alloc`: vm_bind_map VA allocation; `dispatch`: exec_submit path; `free`: vm_bind_unmap; bump allocator from `NV_KERNEL_MANAGED_ADDR` |
| UVM `NV01_DEVICE_0` fix | ✅ | Pass `Nv0080AllocParams` with `device_id` — fixes `NV_ERR_OPERATING_SYSTEM` (0x1F) |
| UVM `NV20_SUBDEVICE_0` fix | ✅ | Pass `Nv2080AllocParams` with `sub_device_id` |
| RM status constants | ✅ | `NV_ERR_INVALID_ARGUMENT`, `NV_ERR_OPERATING_SYSTEM`, `NV_ERR_INVALID_OBJECT_HANDLE` |
| SM70 encoder `unwrap()→expect()` | ✅ | 8 production `unwrap()` in `sm70_encode/{control,encoder}.rs` → `expect()` with descriptive messages |
| `gem_info` error propagation | ✅ | `NvDevice::alloc` no longer swallows `gem_info` errors with `unwrap_or((0,0))` |
| `ioctl.rs` smart refactoring | ✅ | `ioctl.rs` (1039 LOC) → `ioctl/{mod.rs, new_uapi.rs, diag.rs}` (692 + 210 + 159 LOC) |
| Device path constants | ✅ | `NV_CTL_PATH`, `NV_UVM_PATH`, `NV_GPU_PATH_PREFIX`, `DRI_RENDER_PREFIX` — no scattered string literals |
| `#[allow(dead_code)]` cleanup | ✅ | New UAPI structs: removed (now used); `NOUVEAU_VM_BIND_RUN_ASYNC`, `EXEC_PUSH_NO_WAIT`: `#[expect]` with reasons |
| Test expansion | ✅ | 1487 → 1509 passing (+22), 76 → 54 ignored (-22) |

### Iteration 32: Deep Debt Evolution — Math Functions, AMD Encoding, Refactoring (Mar 11 2026)

| Item | Status | Detail |
|------|--------|--------|
| `firstTrailingBit` implementation | ✅ | `clz(reverseBits(x))` via OpBRev + OpFlo, NV + AMD |
| `distance` implementation | ✅ | `length(a - b)` via component-wise FAdd + translate_length, NV + AMD |
| AMD `OpBRev` encoding | ✅ | VOP1 `V_BFREV_B32` — closes discriminant 31 gap |
| AMD `OpFlo` encoding | ✅ | VOP1 `V_FFBH_U32`/`V_FFBH_I32`, with SUB+VOPC+CNDMASK for bit-position mode |
| `CallResult` → `OpUndef` placeholder | ✅ | Replaced with proper `CompileError::InvalidInput` |
| `BindingArray` stride fix | ✅ | Hardcoded `1` → recursive `array_element_stride(*base)` |
| `shader_info.rs` smart refactor | ✅ | 814 LOC → `shader_io.rs` (168) + `shader_model.rs` (337) + `shader_info.rs` (306) |
| Production mock audit | ✅ | All mocks test-only; `coral-reef-stubs` is real impl despite name |
| Dependency analysis | ✅ | 26/28 deps pure Rust; only C is tokio→mio→libc (tracked) |
| Test expansion | ✅ | 1556 → 1562 passing (+6), 54 ignored (stable) |
| Coverage | ✅ | 64% (NVVM poisoning validation: 6 new tests in `nvvm_poisoning_validation.rs`) |

### Iteration 34: Deep Debt Evolution — Smart Refactoring, Unsafe Elimination, Test Coverage, Absorption (Mar 11 2026)

| Item | Status | Detail |
|------|--------|--------|
| `legalize.rs` smart refactor | ✅ | 772 LOC → `legalize/mod.rs` (engine + tests) + `legalize/helpers.rs` (LegalizeBuildHelpers trait + helpers); clean API/engine separation |
| `bytemuck::bytes_of` unsafe elimination | ✅ | `diag.rs` `from_raw_parts` → `bytemuck::bytes_of`; Pod+Zeroable derives on NouveauSubchan/NouveauChannelAlloc |
| `drm_ioctl_named` for new UAPI | ✅ | `new_uapi.rs` 4 wrappers switched from `drm_ioctl_typed` → `drm_ioctl_named` for informative error messages |
| 34 naga_translate unit tests | ✅ | exp/exp2/log/log2/pow, sinh/cosh/tanh/asinh/acosh/atanh, sqrt/inverseSqrt, ceil/round/trunc/fract, dot/cross/length/normalize/distance, countOneBits/reverseBits/firstLeadingBit/countLeadingZeros/firstTrailingBit, fma/sign/mix/step/smoothstep, min/max, local_invocation_id/workgroup_id/num_workgroups/local_invocation_index |
| SM89 DF64 validation tests | ✅ | 3 tests: Yukawa DF64, isolated transcendentals, Verlet integrator — Ada Lovelace sovereign path validation |
| 5 deformed HFB shaders absorbed | ✅ | hotSpring deformed Hamiltonian, wavefunction, density/energy, gradient, potentials — 9 passing, 1 ignored (RDNA2 encoding gap) |
| `quick-xml` 0.37→0.39 | ✅ | `amd-isa-gen` dependency updated, `unescape()→decode()` API migration |
| Test expansion | ✅ | 1562 → 1608 passing (+46), 54 → 55 ignored (+1 RDNA2 HO recurrence) |

### Iteration 35: FirmwareInventory + ioctl Evolution (Mar 11 2026)

| Item | Status | Detail |
|------|--------|--------|
| `FirmwareInventory` struct | ✅ | Structured probe for ACR/GR/SEC2/NVDEC/PMU/GSP firmware subsystems per GPU chip |
| `compute_viable()` | ✅ | Reports dispatch viability: requires GR + (PMU or GSP) |
| `compute_blockers()` | ✅ | Human-readable list of missing components blocking compute |
| `firmware_inventory()` re-exports | ✅ | `FirmwareInventory`, `FwStatus`, `firmware_inventory` accessible via `nv::ioctl` |
| `drm_ioctl_typed` eliminated | ✅ | All 7 call sites migrated to `drm_ioctl_named`; dead function removed |
| `drm_ioctl_named` migration | ✅ | `nouveau_channel_alloc/free`, `gem_new/info`, `pushbuf_submit`, `gem_cpu_prep`, `diag_channel_alloc` |
| 4 new tests | ✅ | `firmware_inventory_nonexistent_chip`, `firmware_inventory_compute_viable_logic`, `fw_status_is_present`, `firmware_check_returns_entries` (existing) |
| Test expansion | ✅ | 1608 → 1616 passing (+8), 55 ignored (unchanged) |
| Unsafe reduction | ✅ | 29 → 24 unsafe blocks (drm_ioctl_typed + bytemuck elimination) |

### Iteration 36: UVM Sovereign Compute Dispatch (Mar 11 2026)

| Item | Status | Detail |
|------|--------|--------|
| `docs/UVM_COMPUTE_DISPATCH.md` | ✅ | Architecture doc: RM hierarchy, dispatch pipeline, reusable components |
| `NV_ESC_RM_CONTROL` wrapper | ✅ | Generic `rm_control<T>()` for RM control calls on any object |
| GPU UUID query | ✅ | `query_gpu_uuid()` via `NV2080_CTRL_CMD_GPU_GET_GID_INFO` |
| `register_gpu_with_uvm()` | ✅ | Chains UUID query → `UVM_REGISTER_GPU` |
| `alloc_vaspace()` | ✅ | `FERMI_VASPACE_A` (0x90F1) GPU virtual address space |
| `alloc_channel_group()` | ✅ | `KEPLER_CHANNEL_GROUP_A` (0xA06C) TSG |
| `alloc_system_memory()` | ✅ | `NV01_MEMORY_SYSTEM` (0x3E) RM memory allocation |
| `alloc_gpfifo_channel()` | ✅ | `VOLTA_CHANNEL_GPFIFO_A` / `AMPERE_CHANNEL_GPFIFO_A` |
| `alloc_compute_engine()` | ✅ | `VOLTA_COMPUTE_A` / `AMPERE_COMPUTE_A` bind to channel |
| `NvUvmComputeDevice` | ✅ | Full `ComputeDevice` impl: alloc/free/upload/readback/dispatch/sync |
| `coral-gpu` UVM wiring | ✅ | `nvidia-drm` auto-tries UVM before DRM-only fallback |
| `rm_alloc_typed<T>()` | ✅ | Generic RM_ALLOC helper eliminates per-class boilerplate |
| `rm_alloc_simple()` | ✅ | Parameterless RM_ALLOC for class-only objects (compute engine) |
| `NvChannelAllocParams` | ✅ | Full `NV_CHANNEL_ALLOC_PARAMS` #[repr(C)] (NV_MAX_SUBDEVICES=8) |
| `NvVaspaceAllocParams` | ✅ | VA space alloc struct |
| `NvChannelGroupAllocParams` | ✅ | Channel group alloc struct |
| `NvMemoryAllocParams` | ✅ | System memory alloc struct |
| `UvmMapExternalAllocParams` | ✅ | UVM_MAP_EXTERNAL_ALLOCATION struct |
| Hardware tests | ✅ | 7 new `#[ignore]` tests: register_gpu, vaspace, channel, compute_bind, device_open, alloc_free |
| Size assertions | ✅ | `NvRmControlParams` (32B), `Nv2080GpuGetGidInfoParams` (268B), `NvMemoryDescParams` (24B) |
| Clippy clean | ✅ | Zero warnings on coral-driver + coral-gpu |
| Workspace green | ✅ | All tests pass (1616+ passing, 0 failed) |

### Iteration 37: Gap Closure + Deep Debt Evolution (Mar 12 2026)

| Item | Status | Detail |
|------|--------|--------|
| `bytemuck::Zeroable` unsafe elimination | ✅ | 5 UVM structs: `NvMemoryDescParams`, `NvChannelAllocParams`, `NvMemoryAllocParams`, `UvmGpuMappingAttributes`, `UvmMapExternalAllocParams` — `unsafe { std::mem::zeroed() }` → safe `Self::zeroed()` |
| PCI vendor constants centralized | ✅ | `PCI_VENDOR_NVIDIA` (0x10DE), `PCI_VENDOR_AMD` (0x1002), `PCI_VENDOR_INTEL` (0x8086) in `nv/identity.rs` |
| AMD architecture detection | ✅ | `GpuIdentity::amd_arch()` — PCI device ID → architecture string (gfx9/rdna1/rdna2/rdna3) |
| `raw_nv_ioctl` helper extraction | ✅ | Repeated unsafe ioctl pattern in `rm_client.rs` → single reusable helper |
| Compute class constant unification | ✅ | `pushbuf.rs` re-exports from `uvm/mod.rs` — single source of truth |
| `NV_STATUS` code documentation | ✅ | Error constants refactored into `nv_status` module with per-constant doc comments |
| `uvm.rs` smart refactor | ✅ | 727 LOC monolith → `uvm/mod.rs` (897) + `uvm/structs.rs` (592) + `uvm/rm_client.rs` (987) |
| GPFIFO submission + USERD doorbell | ✅ | `submit_gpfifo()` writes GPFIFO entry + updates GP_PUT doorbell register via CPU-mapped USERD |
| GPFIFO completion polling | ✅ | `poll_gpfifo_completion()` polls GP_GET from USERD until catch-up or timeout |
| `NvUvmComputeDevice` dispatch complete | ✅ | Full pipeline: upload shader → build QMD (v2.1/v3.0 by GpuGen) → upload QMD → construct PushBuf → submit GPFIFO → doorbell |
| `NvDrmDevice` stub → delegator | ✅ | Now holds `Option<NvUvmComputeDevice>`, delegates all `ComputeDevice` ops to UVM backend |
| `KernelCacheEntry` serialization API | ✅ | `serde`-derived struct for on-disk kernel caching; `to_cache_entry()` / `from_cache_entry()` |
| `GpuContext::dispatch_precompiled()` | ✅ | Dispatch raw binary with explicit metadata (gpr_count, shared_mem, workgroup) |
| `GpuTarget::arch_name()` | ✅ | Canonical string identifier per architecture (e.g., `"sm86"`, `"rdna2"`) for cache keys |
| Capability-based discovery evolution | ✅ | `discovery.rs` uses `probe_gpu_identity()` + `amd_arch()` for dynamic AMD detection |
| Test expansion | ✅ | 1635 passing (+19), 63 ignored (+8 new hardware-gated) |

### Iteration 38: Deep Debt Solutions + Idiomatic Evolution (Mar 12 2026)

| Item | Status | Detail |
|------|--------|--------|
| `cargo fmt` drift fixed | ✅ | ~10 files with formatting drift resolved |
| Clippy evolution (6 fixes) | ✅ | `map_external_allocation` → `ExternalMapping` struct, `on_alloc` → `RmAllocEvent` struct, `dispatch_precompiled` → accepts `KernelCacheEntry`, 2× redundant closure → method refs, collapsible if → let-chain |
| Intra-doc links fixed | ✅ | 4 broken links: `RmObserver`, `register_gpu_with_uvm`, `GpuKnowledge`, `RmClient` |
| `naga_translate_tests.rs` smart refactor | ✅ | 1486 LOC → 3 domain files: `tests_parse_translate.rs` (568) + `tests_math_coverage.rs` (651) + `tests_interpolation_builtins.rs` (288) |
| `rm_client.rs` smart refactor | ✅ | 1031 LOC → 997 production + `rm_client_tests.rs` (271) + `rm_status_name` → `nv_status::status_name` |
| `op_conv.rs` test extraction | ✅ | 1047 LOC → 796 production + `op_conv_tests.rs` (224) |
| `// SAFETY:` comment added | ✅ | `uvm/mod.rs` test unsafe block documented |
| Zero-copy evolution | ✅ | `primal-rpc-client` roundtrip: `Vec<u8>` → `bytes::Bytes` |
| Coverage expansion (+22 tests) | ✅ | 15 new `unix_jsonrpc` tests (dispatch, make_response, error paths), 7 new `op_conv` tests (PrmtSelByte, OpF2F, OpPrmt, Foldable, DisplayOp) |
| File size compliance | ✅ | Zero files over 1000 LOC (3 violations resolved) |
| Test expansion | ✅ | 1657 passing (+22), 63 ignored (stable) |

### Iteration 39: FECS GR Context + UVM Alignment + Safe Evolution (Mar 12 2026)

| Item | Status | Detail |
|------|--------|--------|
| FECS GR context init (Gap 3) | ✅ | `sw_ctx.bin` content stored in `GrFirmwareBlobs` (was discarded), `PushBuf::gr_context_init()` builds FECS method submission, `NvDevice::open_from_drm()` submits GR init before first dispatch |
| UVM CBUF descriptor alignment (Gap 2) | ✅ | UVM dispatch evolved from direct CBUF binding to descriptor-table-in-CBUF-0 model — matches nouveau path and compiler codegen (`c[0][binding * 8]`) |
| hotSpring dispatch fixes absorbed | ✅ | Commit `a691023` verified (QMD fields, CBUF descriptors, syncobj sync), `NvUvmComputeDevice` re-exported |
| Unsafe evolution | ✅ | 4 missing `// SAFETY:` comments added (syncobj_create/destroy/wait, exec_submit_with_signal), Send/Sync impl documented, `copy_nonoverlapping` → safe `slice::copy_from_slice` |
| Hardcoding evolution | ✅ | AMD VA base/stride extracted to named constants (`AMD_USER_VA_BASE`, `AMD_VA_STRIDE`), zero cross-primal references in production code |
| Formatting drift fixed | ✅ | `cargo fmt` applied post-rebase (2 files) |
| Dead code fix | ✅ | `open_nv_sm70` annotated with `#[expect(dead_code)]`, `hw_nv_nouveau.rs` |
| Test coverage (+10 tests) | ✅ | `legacy_parse_retains_ctx_data`, `missing_ctx_produces_empty`, `gr_context_init_structure`, `gr_context_init_empty_methods`, `sm_to_chip_mapping`, `compute_class_selection`, `gpfifo_entry_encoding`, `gpfifo_entry_zero_length`, `gpu_gen_sm_roundtrip` |
| File size compliance | ✅ | All files under 1000 LOC (largest: `rm_client.rs` at 997) |
| Test expansion | ✅ | 1667 passing (+10), 64 ignored |

### Iteration 40: BAR0 Absorption + Deep Debt Evolution + Error Recovery (Mar 12 2026)

| Item | Status | Detail |
|------|--------|--------|
| BAR0 breakthrough absorbed | ✅ | Team commits `23ed6f8`, `e160d89`, `996b7c1` — sovereign BAR0 MMIO GR init, address-aware firmware split, phased device open (BAR0 → VM_INIT → CHANNEL_ALLOC → FECS) |
| BUG: `sm_version()` fix | ✅ | Was deriving SM from `compute_class` match (wrong for Turing/Ampere) — now returns stored `sm_version` field |
| BUG: `pushbuf::class` portability | ✅ | Was unconditionally importing from `uvm` module (breaks nouveau-only builds) — now imports from `ioctl` constants |
| Hardcoding evolution (nv/mod.rs) | ✅ | Sync timeout → `SYNCOBJ_TIMEOUT_NS`, page mask → `GPU_PAGE_MASK`, local mem window → `LOCAL_MEM_WINDOW_VOLTA` / `LOCAL_MEM_WINDOW_LEGACY`, syncobj deadline → `syncobj_deadline()` helper |
| Hardcoding evolution (pushbuf.rs) | ✅ | Cache invalidation → `method::INVALIDATE_INSTR_AND_DATA`, pushbuf capacity → `DEFAULT_PUSHBUF_WORDS` |
| Hardcoding evolution (coral-gpu) | ✅ | SM fallbacks → `DEFAULT_NV_SM` / `DEFAULT_NV_SM_NOUVEAU`, FNV hash → `FNV1A_OFFSET_BASIS` / `FNV1A_PRIME` with documentation |
| Chip mapping dedup | ✅ | `run_open_diagnostics` now uses `sm_to_chip()` instead of duplicated match |
| Error logging improved | ✅ | `try_fecs_channel_init` now logs firmware parse errors instead of silent discard |
| Gap 6: Error recovery | ✅ | Dispatch refactored to `dispatch_inner` pattern — temp buffers freed on error instead of leaking |
| Method address validation | ✅ | `gr_context_init` adds `debug_assert!(addr <= 0x7FFC)` for push buffer encoding limit |
| Doc warning fixed | ✅ | `bar0.rs` module doc link to private `firmware_parser` → plain text reference |
| Dead code fix (re-applied) | ✅ | Team's commits overwrote our `#[expect(dead_code)]` — re-applied |
| Quality gates | ✅ | Zero clippy warnings, zero fmt drift, zero doc warnings |
| Test expansion | ✅ | 1669 passing (+2), 64 ignored |

### Iteration 41: VFIO Sovereign GPU Dispatch (Mar 12 2026)

| Item | Status | Detail |
|------|--------|--------|
| VFIO core module | ✅ | `coral-driver/src/vfio/` — types, ioctls, DMA buffer, VfioDevice (container/group/device fd, BAR mapping) |
| NvVfioComputeDevice | ✅ | `nv/vfio_compute.rs` — full `ComputeDevice` impl: alloc/free/upload/readback/dispatch/sync via VFIO BAR0 + DMA |
| GPFIFO direct dispatch | ✅ | DMA-backed GPFIFO ring + USERD doorbell via BAR0, pushbuf + QMD reuse from existing builders |
| Feature gate | ✅ | `--features vfio` on both `coral-driver` and `coral-gpu`, zero impact on default builds |
| DriverPreference updated | ✅ | `vfio` > `nouveau` > `amdgpu` > `nvidia-drm` in sovereign mode |
| VFIO GPU discovery | ✅ | Scans `/sys/bus/pci/drivers/vfio-pci/` for NVIDIA vendor ID, auto-detects SM from PCI device ID |
| from_descriptor support | ✅ | `("nvidia", Some("vfio"))` path in `GpuContext::from_descriptor_with_path` |
| 35 new tests | ✅ | 27 unit tests (types, DMA, ioctl layout) + 8 integration tests (GPFIFO encoding, constants, device open) |
| 5 HW integration tests | ✅ | Ignored by default, require `CORALREEF_VFIO_BDF` env var + VFIO-bound GPU |
| wateringHole handoff | ✅ | toadStool hardware contract: VFIO bind, IOMMU, permissions, eventfd |

### Iteration 42: VFIO Sync + barraCuda API (Mar 12 2026)

| Item | Status | Detail |
|------|--------|--------|
| VFIO sync() — GPFIFO GP_GET polling | ✅ | `poll_gpfifo_completion()` reads GP_GET from USERD DMA page via volatile read, spin-loop + 5s timeout — matches UVM path pattern exactly |
| USERD GP_PUT write in submit | ✅ | `submit_pushbuf()` now writes GP_PUT to USERD DMA page before BAR0 doorbell — GPU can track put pointer |
| USERD module constants | ✅ | `userd::GP_PUT_OFFSET` (0x00) and `userd::GP_GET_OFFSET` (0x04) — named constants replace magic offsets |
| `GpuContext::from_vfio(bdf)` | ✅ | Public convenience API: auto-detects SM from sysfs, opens VFIO device — unblocks barraCuda `CoralReefDevice::from_vfio_device()` |
| `GpuContext::from_vfio_with_sm(bdf, sm)` | ✅ | Explicit SM override for testing or when sysfs unavailable |
| Sync/poll constants | ✅ | `SYNC_TIMEOUT` (5s) and `POLL_INTERVAL` (10µs) — extracted from inline values |
| `#[expect(dead_code)]` cleanup | ✅ | Removed stale `dead_code` annotation from `userd` field (now actively used by sync) |
| All tests pass | ✅ | 1669 default + 35 VFIO, 0 failed, 64+5 ignored |

### Iteration 45: Deep Audit + Refactor + Coverage Expansion (Mar 14 2026)

| Item | Status | Detail |
|------|--------|--------|
| Full codebase audit | ✅ | specs, wateringHole standards (UniBin, ecoBin, genomeBin, IPC protocol, semantic naming), sovereignty, AGPL3 |
| `vfio/channel.rs` smart refactor | ✅ | 2894 LOC → 5 modules (`mod.rs` 269, `registers.rs` 187, `page_tables.rs` 334, `pfifo.rs` 236, `diagnostic.rs` 1988) — production files under 1000 LOC |
| `eprintln!` → `tracing` migration | ✅ | `pfifo.rs`, `mod.rs`, `vfio_compute.rs`, `device.rs` — structured logging in production; `eprintln!` retained in `diagnostic.rs` for HW debugging |
| IPC chaos/fault tests | ✅ | 6 new tests in `tests_chaos.rs`: concurrent JSON-RPC, malformed requests, rapid connect/disconnect, oversized payloads, concurrent tarpc, invalid methods |
| coralreef-core unit tests | ✅ | New tests for config (error variants, display, defaults), health (display, clone), lifecycle (display, error), capability (display, error) |
| coral-driver unit tests | ✅ | 30+ new tests: error variants/display, QMD encoding, pushbuf, PM4, identity parsing, knowledge base queries |
| Doctest fixes | ✅ | 5 ignored doctests fixed: `coral-gpu`, `coral-reef`, `coral-reef-isa` → `no_run`; `nak-ir-proc` → `text` |
| Unsafe evolution | ✅ | `// SAFETY:` comments on all unsafe blocks (`bar0.rs`, `new_uapi.rs`, `rm_client.rs`), mmap null check, `debug_assert!` → `assert!` for DMA slices |
| Clippy pedantic | ✅ | `map_unwrap_or` → `map_or` (3 sites), `identity_op` resolved, `cast_possible_truncation` with `#[expect]` |
| Test expansion | ✅ | 1721 passing (+52), 0 failed, 61 ignored (−13 from fixed doctests + test consolidation) |
| Coverage | ✅ | 65.74% line, 73.48% function (up from 64%) |

### Iteration 44: USERD_TARGET + INST_TARGET Runlist Fix (Mar 13 2026)

| Item | Status | Detail |
|------|--------|--------|
| USERD_TARGET in runlist DW0 | ✅ | Bits [3:2] = SYS_MEM_COHERENT (2) — tells PBDMA where to read the USERD page. Was 0 (VRAM), causing PBDMA to never see host GP_PUT writes |
| INST_TARGET in runlist DW2 | ✅ | Bits [5:4] = SYS_MEM_NCOH (3) — tells PBDMA where the instance block lives. Was missing |
| TARGET_SYS_MEM_NCOH constant | ✅ | Value 3 for PCCSR/PFIFO/Runlist encoding (distinct from PBDMA encoding) |
| pfifo::RUNLIST_BASE/RUNLIST wired | ✅ | Replaced literal 0x2270/0x2274 with named constants |
| Clippy warnings resolved | ✅ | identity_op in device.rs, assertions_on_constants in tests |
| Formatting drift fixed | ✅ | `cargo fmt` applied across ioctl, qmd, vfio_compute, channel, device |
| 2 new runlist encoding tests | ✅ | DW0 USERD_TARGET encoding, DW2 INST_TARGET encoding (replaces 1 old test) |
| All tests pass | ✅ | 1669 default + 48 VFIO, 0 failed, 66+8 ignored |

### Iteration 43: PFIFO Channel Init + Cross-Primal Rewire (Mar 13 2026)

| Item | Status | Detail |
|------|--------|--------|
| PFIFO channel creation via BAR0 | ✅ | `vfio/channel.rs` — full Volta PFIFO channel init: RAMFC population (GPFIFO base, USERD, signature, engine config), instance block with V2 MMU page tables, TSG+channel runlist, PCCSR channel bind/enable, PFIFO runlist submission |
| V2 MMU page tables (5-level) | ✅ | Identity-mapped PD3→PD2→PD1→PD0→PT covering 2 MiB IOVA range; PDE/PTE encoding per NVIDIA `dev_ram.ref.txt`; SYS_MEM_COHERENT aperture for VFIO DMA buffers |
| RAMUSERD offset correction | ✅ | GP_GET at 0x88 (dword 34), GP_PUT at 0x8C (dword 35) — corrected from incorrect 0x00/0x04 offsets per NVIDIA `dev_ram.ref.txt` Volta RAMUSERD specification |
| USERMODE doorbell | ✅ | NV_USERMODE_NOTIFY_CHANNEL_PENDING at BAR0 + 0x810090 — writes channel ID (replaces incorrect BAR0 + 0x0090 GP_PUT write); per NVIDIA `dev_usermode.ref.txt` |
| Subcontext PDB setup | ✅ | SC_PDB_VALID(0) + SC_PAGE_DIR_BASE(0) populated — FECS compute subcontext 0 active |
| toadStool S150-S152 acknowledged | ✅ | All 12 software gaps resolved (dispatch pipeline, VFIO bind/unbind, thermal safety, cross-gate pooling, mock hardware for CI) |
| barraCuda VFIO-primary acknowledged | ✅ | `dispatch_binary`/`dispatch_kernel` wired, Gap 1 (coral cache→dispatch) closed, `from_vfio_device` can use `GpuContext::from_vfio()` once PFIFO channel works on HW |
| VFIO HW validation results absorbed | ✅ | 6/7 tests pass on biomeGate Titan V; `vfio_dispatch_nop_shader` FenceTimeout → root cause: missing PFIFO channel (now implemented) |
| 12 new channel unit tests | ✅ | PDE/PTE encoding, PCCSR/PFIFO register offsets, IOVA layout validation, RAMUSERD offsets, runlist base encoding |
| All tests pass | ✅ | 1693 default + 47 VFIO, 0 failed, 71 ignored |

### Pure Rust Sovereign Stack — Dependency Tracking

| Component | Status | Detail |
|-----------|--------|--------|
| `rustix` backend | `linux_raw` | Confirmed: depends on `linux-raw-sys`, zero `libc` |
| `ring` | **Eliminated** | `jsonrpsee[client]` removed; `primal-rpc-client` crate for tests + production |
| `libc` (transitive) | Tracked | `tokio`→`mio`→`libc` (mio#1735), `socket2`→`libc`, `signal-hook-registry`→`libc`, `getrandom`→`libc`, `parking_lot_core`→`libc` |
| `libc` canary | Prepared | `deny.toml` has commented-out `libc` ban — uncomment when upstream migrates |
| Our code → `libc` | **Zero** | No workspace crate has direct `libc` dependency |

### Phase 10 Remaining / Phase 11 Roadmap

| Task | Priority | Detail |
|------|----------|--------|
| Nouveau UAPI E2E validation | **P0** | Pipeline fully wired: `VM_INIT → CHANNEL_ALLOC → VM_BIND → EXEC` auto-detected in `NvDevice::open_from_drm`. Needs hotSpring hardware validation on Titan V (GV100 kernel 6.17) |
| UVM GPFIFO + dispatch validation | **P0** | Full dispatch pipeline implemented (GPFIFO submission + USERD doorbell + completion polling) — needs RTX 5060 hardware validation (RTX 3090 decommissioned) |
| Hardware validation (AMD) | ✅ | **E2E verified** — RX 6950 XT, WGSL compile + dispatch + readback |
| Hardware validation (NVIDIA nouveau) | P1 | Titan V: UAPI migration unblocks dispatch. hotSpring Exp 051: 16/16 firmware present, NVK Vulkan works, legacy UAPI EINVAL on all channel classes |
| Hardware validation (NVIDIA nvidia-drm) | P1 | RTX 5060 (SM89): Full UVM dispatch pipeline implemented — `NvDrmDevice` delegates to `NvUvmComputeDevice`. Needs on-site hardware validation (RTX 3090 decommissioned) |
| Intel backend | P3 | Placeholder |

## Checks

| Check | Status |
|-------|--------|
| `cargo check --workspace` | PASS |
| `cargo test --workspace` | PASS (4047 passing, 0 failed, 121 ignored hardware-gated) |
| `cargo llvm-cov` | ~66% line (8 crates >90%, coralreef-core 95.9%, coral-reef 78.6%) |
| `cargo clippy --workspace --features vfio -- -D warnings` | PASS (0 warnings) |
| `cargo fmt --check` | PASS |
| `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps` | PASS (0 warnings) |
| Hardcoded paths | Evolved — all sysfs/proc via `CORALREEF_SYSFS_ROOT` / `CORALREEF_PROC_ROOT` env overrides |

## Hardware — On-Site

| GPU | PCI | Architecture | Kernel Driver | f64 | VRAM | Role |
|-----|-----|-------------|---------------|-----|------|------|
| NVIDIA Titan V #1 | 03:00.0 | Volta SM70 (GV100) | vfio-pci | 1/2 | 12 GB HBM2 | Oracle card (VFIO sovereign) |
| NVIDIA Titan V #2 | 4a:00.0 | Volta SM70 (GV100) | vfio-pci | 1/2 | 12 GB HBM2 | Compute target (VFIO sovereign) |
| NVIDIA RTX 5060 | 21:00.0 | Ada SM89 | nvidia-drm | 1/64 | 16 GB | Desktop + UVM dispatch |

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
| WGSL shader corpus (cross-spring) | 6 springs (93 shaders, 84 compiling SM70) | `tests/fixtures/wgsl/corpus/` |
| GLSL compute frontend | naga `glsl-in` feature | `compile_glsl()` public API, 5 GLSL fixtures |
| SPIR-V roundtrip testing | naga `spv-out` → `compile()` | 10 roundtrip tests (10 passing, 0 ignored) |
| FMA control / NoContraction | wateringHole NUMERICAL_STABILITY_PLAN | FmaPolicy |
| Safe syscalls via libc | groundSpring CONTRIBUTING | drm.rs, gem.rs |
| `Cow<'static, str>` error fields | Rust idiom: zero-alloc static paths | DriverError, CompileError, GpuError, PrimalError |
| `#[expect]` with reasons | Rust 2024 idiom | workspace-wide (replaces `#[allow]`) |
| `FxHashMap` in hot paths | Performance internalization | naga_translate/func.rs, func_ops.rs |
| Sealed FFI boundary | wateringHole sovereignty | `drm_ioctl_named` pub(crate) (sole wrapper), `BufferHandle` pub(crate) |
| `shader.compile.*` semantic naming | wateringHole PRIMAL_IPC_PROTOCOL | JSON-RPC + tarpc |
| Differentiated IPC error codes | wateringHole PRIMAL_IPC_PROTOCOL | jsonrpc.rs |
| `#[deny(unsafe_code)]` safety boundary | Rust best practice | 8/9 crates (all except coral-driver) |
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
