# coralReef — Status

**Last updated**: March 6, 2026

---

## Overall Grade: **A+** (Multi-Vendor Sovereign GPU Compiler)

| Category | Grade | Notes |
|----------|-------|-------|
| Primal lifecycle | A | Standalone `PrimalLifecycle` + `PrimalHealth`, full test coverage |
| UniBin compliance | A | Binary target with clap, panic hook, SIGTERM/SIGINT, structured errors |
| IPC | A+ | JSON-RPC 2.0 + tarpc, Unix socket + TCP, zero-copy `Bytes` payloads |
| NVIDIA pipeline | A+ | WGSL/SPIR-V → naga → codegen IR → f64 lower → optimize → legalize → RA → encode |
| AMD pipeline | A | `ShaderModelRdna2` → legalize → RA → encode, cross-vendor WGSL compilation |
| Mesa stubs evolved | A+ | All modules evolved to pure Rust (BitSet, CFG, dataflow, fxhash, nvidia_headers) |
| f64 transcendentals | A+ | sqrt, rcp, exp2, log2, sin, cos — NVIDIA (Newton-Raphson) + AMD (native v_sqrt/rcp_f64) |
| Vendor-agnostic arch | A+ | `Shader` holds `&dyn ShaderModel` — idiomatic Rust trait dispatch, no manual vtables |
| coralDriver | A | AMD DRM ioctl (GEM, PM4, CS), NVIDIA nouveau (QMD), pure Rust syscalls |
| coralGpu | A | Unified compile+dispatch API, vendor-agnostic `GpuContext` |
| Code structure | A+ | All files < 1000 LOC |
| Tests | A+ | 801 tests, zero failures, 5 ignored |
| Clippy | A+ | Zero warnings, pedantic categories enabled |
| License | A | AGPL-3.0-only (upstream-derived files retain original attribution) |
| Sovereignty | A+ | Zero FFI, zero `*-sys`, zero `extern "C"`, zero-knowledge startup |
| Result propagation | A+ | Pipeline fully fallible: naga_translate → lower → legalize → encode |
| Dependencies | A+ | Pure Rust — zero C deps, zero `*-sys` crates, ISA gen in Rust |
| Tooling | A+ | `rustfmt.toml`, `clippy.toml`, `deny.toml`, pure Rust ISA generator |

## Phase Status

| Phase | Description | Status |
|-------|-------------|--------|
| 1 — Scaffold | Extract sources, create stubs | **Complete** |
| 1.5 — Foundation | UniBin, IPC, stubs evolved | **Complete** |
| 2 — Wire Sources | Compiler sources compile against stubs | **Complete** |
| 2.5–2.9 — Refactor | File splits, debt reduction, sovereignty | **Complete** |
| 3 — naga Frontend | SPIR-V/WGSL → codegen IR via naga | **Complete** |
| 3.5 — Deep Debt | Stubs removed, Result propagation, zero-copy | **Complete** |
| 4 — f64 Fix | DFMA software lowering (all 6 transcendentals) | **Complete** |
| 4.5 — Error Safety | Production panic→Result, pipeline propagation | **Complete** |
| 5 — Standalone | All stub dependencies evolved | **Complete** |
| 5.5 — Naming Evolution | Mesa/NAK de-vendoring, Rust-idiomatic fields | **Complete** |
| 5.7 — Deep Debt Audit | 710 tests, tooling, proc-macro safety | **Complete** |
| 6a — AMD ISA + Encoder | RDNA2 GFX1030 instruction encoding | **Complete** |
| 6b — AMD Legalization + RA | ShaderModelRdna2, legalize, exec mask | **Complete** |
| 6c — AMD f64 Lowering | Native v_sqrt_f64, v_rcp_f64, v_fma_f64 | **Complete** |
| 6d — AMD Validation | End-to-end WGSL → GFX1030, cross-vendor | **Complete** |
| 7a — AMD coralDriver | DRM ioctl, GEM, PM4, command submission | **Complete** |
| 7b — Internalize | Pure Rust ioctl, zero unsafe public API | **Complete** |
| 7c — NVIDIA coralDriver | nouveau DRM, QMD v3.0 | **Complete** |
| 8 — coralGpu | Unified Rust GPU abstraction | **Complete** |
| 9 — Full Sovereignty | Zero FFI, zero C, zero *-sys | **Complete** |

## Checks

| Check | Status |
|-------|--------|
| `cargo check --workspace` | PASS |
| `cargo test --workspace` | PASS (801 tests, 5 ignored) |
| `cargo clippy --workspace --all-targets -- -D warnings` | PASS (0 warnings) |
| `cargo fmt --check` | PASS |
| `cargo doc --workspace --no-deps` | PASS |

## Hardware — On-Site

| GPU | PCI | Architecture | Kernel Driver | Vulkan | f64 | VRAM | Role |
|-----|-----|-------------|---------------|--------|-----|------|------|
| AMD RX 6950 XT | 25:00.0 | RDNA2 GFX1030 (Navi 21) | amdgpu (open) | RADV/ACO (Mesa 25.1.5) | 1/16 | 16 GB | AMD evolution primary |
| NVIDIA RTX 3090 | 41:00.0 | Ampere SM86 (GA102) | nvidia 580.119.02 | NVIDIA proprietary | 1/32 | 24 GB | NVIDIA compilation target |

## Architecture Evolution (March 6, 2026)

Key architectural change: replaced the Mesa NAK artifact (`ShaderModelInfo` +
`sm_match!` macro — a manual vtable in C-think) with idiomatic Rust trait
dispatch. `Shader<'a>` now holds `&'a dyn ShaderModel`. Each GPU architecture
implements `ShaderModel` directly. The Rust compiler is the DNA synthase.

| Before | After |
|--------|-------|
| `Shader<'a> { sm: &'a ShaderModelInfo }` | `Shader<'a> { sm: &'a dyn ShaderModel }` |
| `sm_match!` macro dispatch | Rust trait object dispatch |
| `ShaderModelInfo` manual vtable | Each vendor implements `ShaderModel` directly |
| NVIDIA-only | NVIDIA + AMD (Intel planned) |
| Python ISA generator | Pure Rust `amd-isa-gen` |
| No driver layer | `coral-driver` (DRM ioctl, pure Rust) |
| No unified API | `coral-gpu` (compile + dispatch) |

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

---

*Grade scale: A (production) → F (not started)*
