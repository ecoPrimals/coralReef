<!-- SPDX-License-Identifier: AGPL-3.0-only -->

# Changelog

All notable changes to coralReef (sovereign Rust GPU compiler — WGSL/SPIR-V/GLSL → native GPU binary) are documented here. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

**Current status**: Phase 10 — Iteration 54

---

## [Unreleased]

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
