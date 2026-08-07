<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->

# Changelog

All notable changes to coralReef (sovereign Rust GPU compiler — WGSL/SPIR-V/GLSL → native GPU binary) are documented here. The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

**Current status**: Phase 10 — Sprint 14 / Wave 157a

---

## [Unreleased]

### Wave 157a: G68 Platform Substrate Deep Evolution (2026-08-07)

#### Evolved (G68 L1: Platform Links)
- `create_local_symlink()` in `transport.rs`: non-Unix stub now uses
  `std::os::windows::fs::symlink_file` on Windows instead of returning
  `Unsupported`. Falls back gracefully on targets without link support.
  Silicon deism eliminated from capability-domain discovery links.
- `primal-rpc-client/transport.rs`: unified `connect_local()` clippy
  compliance for Windows (`unused_async` allowed with documented reason).

#### Evolved (G68 IPC: Transport-Agnostic Discovery + Ecosystem)
- **`ecosystem/mod.rs`**: entire module evolved from UDS-only to
  `TransportEndpoint`-based. `spawn_registration()`, `send_jsonrpc_line()`,
  `heartbeat_loop()`, `send_capability_register()`, `send_primal_announce()`,
  and `send_ipc_heartbeat()` all use `connect_transport()` directly.
  Registration works on non-Unix platforms via TCP.
- **`TransportEndpoint::from_bind_string()`**: new canonical parser for
  bind strings (`unix://`, `/absolute/path`, `tcp://host:port`, `host:port`).
  Lives in `transport.rs` for lib+bin accessibility.
- **`btsp.rs` discovery**: `discover_by_capability()` and
  `discover_security_socket()` return `TransportEndpoint` instead of `PathBuf`.
  `check_discovery_file_for_method()` now checks TCP and `jsonrpc.bind`
  fallbacks when UDS socket doesn't exist.
- **`btsp_client.rs`**: `handshake_on_stream_sync()` and `provider_rpc()`
  accept `&TransportEndpoint` instead of `&Path`.
- **`service/provenance.rs`**: `CRYPTO_SIGN_ENDPOINT` (was `CRYPTO_SIGN_SOCKET`)
  caches `TransportEndpoint`. `try_sign()` uses `connect_transport_sync()`
  directly.
- **Pre-existing clippy fixes**: `protocol_negotiation.rs` items-after-statements
  (3 tests), `tolerances.rs` assertions-on-constants (12 tests).

#### G68 Audit Results (coralReef)
- **45 legitimate platform abstractions** — same thing done differently per platform.
- **18 silicon deism sites** identified and evolved.
- **L1 (Links)**: `create_local_symlink()` — evolved to G68.
- **L2 (Permissions)**: zero `PermissionsExt`/`set_mode` usage.
- **L3 (Device backends)**: zero `rustix`/`libc` direct usage.
- **IPC deism**: eliminated from ecosystem registration, BTSP discovery,
  provenance signing, and security provider handshake.
- Remaining `#[cfg(unix)]` is concentrated in `transport.rs` — the G66
  substrate layer where platform gates belong.

### Wave 156s: G66 Transport Abstraction (2026-08-06)

#### Added
- **G66 transport abstraction** (silicon-agnostic IPC): `TransportStream` enum
  (`Unix` + `Tcp`) with `AsyncRead + AsyncWrite` delegation, `TransportListener`
  with `accept() → TransportStream`, `connect_transport()` bridge function, and
  `TransportEndpoint::platform_default()` / `from_env_or_default()` for
  environment-injected transport selection.
- 13 new tests: `TransportStream` UDS/TCP roundtrips, `TransportListener` accept,
  `connect_transport` error cases, `platform_default` behavior, debug formatting.
- Test count: 3,689 → 3,702 (+13)

#### Changed
- Refactored G65 accept loop: `dispatch_connection()`, `handle_g65_connection()`,
  `handle_brace_connection()` all operate on `TransportStream` — transport-agnostic
  protocol negotiation and JSON-RPC dispatch.
- Refactored BTSP client: `security_rpc()` and `create_btsp_session()` use
  `connect_transport()` instead of direct `UnixStream::connect()`. Removed
  `#[cfg(unix)]` gates from these functions.
- Evolved `local_transport::connect_local()` to return `TransportStream` via
  `connect_transport()`. Evolved `bind_local()` to return `TransportListener`.
- Evolved tarpc local server to accept `TransportStream` from `TransportListener`.
- **Silicon deism eliminated**: `tokio::net::UnixStream`/`UnixListener` confined
  to `transport.rs` and the `#[cfg(unix)]` server bind. Zero unconditional Unix
  imports in IPC business logic.

### Wave 156q: C3 Health Shim Verification (2026-08-06)

#### Added
- **C3 verification**: 3 G65 E2E integration tests proving `health.liveness`
  works through the negotiated socket:
  - `test_g65_negotiate_jsonrpc_then_health_liveness` — full G65 handshake
    (`PROTOCOLS: jsonrpc\n` → `PROTOCOL: jsonrpc\n`) then health.liveness.
  - `test_g65_backward_compat_health_liveness` — plain JSON-RPC (no
    negotiation) health.liveness (C3 backward-compat path).
  - `test_g65_negotiate_tarpc_preferred_falls_back` — tarpc+jsonrpc
    negotiation with health verification on the selected protocol.
- Test count: 3,686 → 3,689 (+3)

### Wave 156p: G65 Protocol Negotiation (2026-08-06)

#### Added
- **G65 protocol negotiation** (Phase 3 cephalization): single-socket protocol
  selection between tarpc and JSON-RPC via `PROTOCOLS:` / `PROTOCOL:` wire
  handshake on the UDS listener. Backward-compatible — legacy JSON-RPC clients
  work with zero changes (no `PROTOCOLS:` line = JSON-RPC fallback).
- New module `ipc_protocol.rs`: `IpcProtocol` enum (`JsonRpc`, `Tarpc`) with
  wire-name parsing, `Display`, serde support, and 7 unit tests.
- New module `protocol_negotiation.rs`: `ProtocolRequest`/`ProtocolResponse`
  wire types, `select_protocol()` (client preference wins), byte-by-byte
  `read_negotiation_line_after_p()`, `negotiate_server_after_p()` server-side
  handler, and 14 unit tests including duplex tarpc/jsonrpc/malformed scenarios.
- `handle_tarpc_negotiated()` in `tarpc_transport.rs`: serves tarpc on an
  already-negotiated stream via `LengthDelimited` + bincode framing.
- Test count: 3,644 → 3,686 (+42)

#### Changed
- Restructured UDS accept loop in `unix_jsonrpc.rs`: first byte dispatches to
  G65 (`P`), BTSP (`{`), or guard (other). Two-stage timeout (100ms G65 +
  remaining for BTSP) preserves backward compatibility.

### Wave 156m: Dispatch Refactor & Adapter Inference Tests (2026-08-06)

#### Added
- 24 unit tests for `compile.rs` internal functions: adapter-aware architecture
  inference (`infer_arch_from_adapter` — 9 tests covering NVIDIA SM70-SM120,
  AMD RDNA2-3, unknown vendors/models), `resolve_arch` (3 tests — explicit,
  inferred, fallback), `wave_size_for` (2 tests), `dispatch_hint_from_precision_advice`
  (3 tests), `binary_format_for` (2 tests), `bytes_to_spirv_words` (2 tests),
  `parse_fma_policy` (3 tests).
- Test count: 3,596 → 3,644 (+48)

#### Changed
- Extracted `to_json` and `handler_result` helpers from `newline_jsonrpc.rs`,
  eliminating 14 identical `serde_json::to_value(...).map_err(...)` patterns
  (423 → 382 LOC, -41 lines).
- Simplified `bytes_to_spirv_words` in `compile.rs`: replaced manual loop +
  dead-code error path with iterator chain (validated `% 4 == 0` guarantees
  infallible `try_into`).

### Wave 156l: Cast/Conversion Coverage & Visibility Narrowing (2026-08-06)

#### Added
- `tests_cast_ops.rs` — 16 E2E tests covering type conversion translation
  (`translate_cast` in `func_ops.rs`). Tests exercise i32→f32, u32→f32 (I2F),
  f32→i32, f32→u32 (F2I), bitcast (f32↔u32, i32→f32, vec2), identity casts
  (i32↔u32), bool→u32 via select, vector conversions (vec3 u32→f32, vec2
  f32→i32), mixed int/float arithmetic, and relational all/any on vec2<bool>.
- Test count: 3,580 → 3,596 (+16)

#### Changed
- Narrowed `func_math_interp::translate` visibility from `pub` to `pub(super)`,
  consistent with all other math sub-module translate functions.
- Narrowed `TexQueueSimulationState` visibility from `pub` to `pub(super)` in
  `calc_instr_deps/types.rs` — only used within the module.

### Wave 156j (cont.): Memory + Binary Ops Coverage & Code Debt Cleanup (2026-08-06)

#### Added
- `tests_memory_ops.rs` — 15 E2E tests covering `func_mem.rs` (381 LOC, previously
  zero dedicated tests). Tests exercise global load/store, vec4 memory, shared memory
  with barriers, atomics (add/max/exchange/CAS), dynamic array indexing, struct field
  access, uniform buffers, local variables, local arrays, and arrayLength.
- `tests_binary_ops.rs` — 23 E2E tests covering `expr_binary.rs` (692 LOC, previously
  zero dedicated tests). Tests exercise f32/i32/u32 arithmetic, bitwise ops (AND/OR/XOR),
  shifts, float/int comparisons, logical AND/OR, vector ops, and f32 modulo.
- Test count: 3,542 → 3,580 (+38)

#### Changed
- Removed redundant `copy_src_ref()` function from `opt_copy_prop` — was manually
  reimplementing `SrcRef::clone()`. Two callsites replaced with `.clone()`.

#### Audited (no code changes needed)
- **Hardcoded primal names**: Zero other-primal names in production code. `BEARDOG_SOCKET`
  is a documented legacy env var alias with `btsp_provider_socket()` as preferred replacement.
- **External dependencies**: All deps pure Rust. `libc` only transitive via tokio/getrandom.
- **Mocks/stubs**: No mocks in production. `coral-reef-stubs` are complete pure-Rust impls.
  `Cpu`/`Npu` compile targets are intentional future extension points.
- **`.unwrap()` in library code**: Zero production unwraps confirmed.
- **TODO/FIXME/HACK**: Zero instances in committed `.rs` code.
- **Files >800 LOC**: All under 800 LOC.

### Wave 156j: C2 Dual-Socket Convention & Import Cleanup (2026-08-06)

#### Changed
- Adopted C2 dual-socket naming convention: tarpc sockets now use `.tarpc.sock`
  extension instead of `-tarpc.sock` dash-prefix (matching songBird/petalTongue pattern).
  Updated `default_tarpc_bind()`, `resolve_uds_binds()`, `primal_tarpc_socket_name()`,
  and discovery test fixtures.
- Cleaned 6 stale unused imports (`ComputeShaderInfo`, `ShaderIoInfo`, `ShaderStageInfo`)
  from test modules in `legalize`, `lower_f64`, `lower_fma`, `sm70_encode`, `opt_bar_prop`.
  Zero test compilation warnings.

#### Added
- `config::primal_tarpc_socket_name()` — canonical tarpc socket filename per C2 convention.
- 2 config tests: C2 `.tarpc.sock` extension assertion, socket name pair coherence.
- Test count: 3,540 → 3,542 (+2)

### Wave 156i: SPIR-V Module Extraction & Control Flow Test Coverage (2026-08-06)

#### Changed
- Extracted SPIR-V functions (`wgsl_to_spirv`, `module_to_spirv`, `parse_wgsl_to_naga`,
  `build_spirv_backend_options`) from `lib.rs` (820 LOC → 717 LOC) to new `spirv.rs`
  module (117 LOC). Public API unchanged via re-exports.

#### Added
- `tests_control_flow.rs`: 15 E2E control flow translation tests covering if-only,
  if/else with phis, nested if/else, loop with break, loop with continue, for loop,
  while loop, switch with valued cases, switch default-only, combined if-in-loop,
  loop-in-if, multi-variable phis, switch-in-loop, and early return dead code.
  Exercises `func_control.rs` (670 LOC, previously zero dedicated tests).
- Test count: 3,525 → 3,540 (+15)

### Wave 156g: Deep Debt — Alloc Elimination, Test Hardening & Error Reclassify (2026-08-05)

#### Changed
- `CompileResponse.status`: `Option<String>` → `Option<Cow<'static, str>>` — eliminates
  heap allocation per compile response; 5× `STATUS_SUCCESS.to_owned()` → `Cow::Borrowed`
- `GpuDeviceDescriptor.source`: `String` → `Cow<'static, str>` — eliminates per-device
  heap alloc for known `"ecosystem"` / `"drm-scan"` values
- Reclassified 9 CFG invariant errors (`break outside loop`, `continue outside loop`,
  `loop stack empty`) from `CompileError::NotImplemented` → `CompileError::Internal`
  (ICE — not user-facing feature gaps)

#### Added
- SM20 f64 legalize tests: 7 smoke-only tests now verify source reference preservation,
  dst integrity, fabs stripping, imm retention, and pred src invariants

### Wave 156e: Deep Debt — Registry Drift, Fossil Cleanup & SM30 Coverage (2026-08-05)

#### Changed
- `capability_registry.toml`: added missing `shader.compile.multi` method and operation,
  added consumed capabilities `primal.announce` and `crypto.sign`, removed stale NVIDIA
  architectures (sm50, sm60, sm90) not present in manifest or `NvArch` enum
- `WHATS_NEXT.md`: marked 6 unchecked diesel-stack items as EXCISED (coral-driver, UVM
  dispatch, PMU firmware — all delegated to toadStool since Sprint 9)
- Cleaned stale imports (`ShaderIoInfo`, `ComputeShaderInfo`, `ShaderStageInfo`) from
  `opt_out.rs` and `opt_prmt.rs` test modules

#### Added
- `sm30_instr_latencies.rs`: 13 direct unit tests — latency tables, exec latency,
  Kepler-A vs Kepler-B branching, scheduling byte patterns (TexDepBar 0xc2, Sync 0x00,
  base 0x20/0x40, delay clamping 1–32)

### Wave 156b: Deep Debt — Deduplication, Allocation & Test Hygiene (2026-08-03)

#### Changed
- `ShaderInfo::compute()` constructor added to `shader_info.rs` — replaced 14 verbose
  construction sites (~220 LOC) across naga_translate, test_shader_helpers, opt_bar_prop,
  legalize, opt_prmt, opt_out, lower_fma, lower_f64, sm70_encode, spill_values
- `infer_arch_from_adapter` returns `&'static str` instead of `String` — eliminates
  8 heap allocations per adapter inference in the IPC compile path
- Cleaned stale imports (`ComputeShaderInfo`, `ShaderIoInfo`, `ShaderStageInfo`) from
  test_shader_helpers.rs, spill_values/fixtures.rs, spill_values/cases_a.rs

#### Removed
- `codegen_coverage_saturation.rs` (551 LOC, 30 tests) — 100% duplicated by
  `codegen_coverage_sat_part01.rs` (20 tests) + `codegen_coverage_sat_part02.rs` (10 tests);
  parts 02/03 also contain 19 unique tests not in the monolith

### Wave 156a: Deep Debt — Test Extraction & Coverage (2026-08-03)

#### Changed
- `ipc/btsp.rs`: extracted 99-line inline test module to `btsp/btsp_guard_tests.rs` (747 → 648 LOC)
- `service/types.rs`: extracted 41-line `identity_tests` module to `types_identity_tests.rs` (753 → 712 LOC)

#### Added
- `env_keys.rs`: 2 unit tests — CORAL prefix invariant + SCREAMING_SNAKE_CASE validation
- `tolerances.rs`: 7 unit tests — constant range invariants, cross-constant ordering, hardware-spec anchoring

### Wave 155j: NUCLEUS Achieved + Lifecycle Readiness (2026-07-30)

#### Added
- `--bind` alias for `--port` in CLI (`coralreef server --bind <PORT>`) — biomeOS CLI flag standardization (Chain 1 item 5), matching rhizoCrypt and loamSpine

#### Validated (NUCLEUS composition)
- coralReef operational in first NUCLEUS composition on strandGate (8/9 healthy, 1,742 caps, 674 IPC methods)
- All 11 health/meta JSON-RPC methods responding on NUCLEUS instance (port 41511)
- WGSL compile (sm_86, 64 bytes, 44.1ms) and GEMM compile (26,907 bytes, 0.1ms) confirmed
- glibc-linked binary validated (PID 3376482, 11+ hours uptime, dynamic-linked to libm/libc)
- Windows P1 fix already shipped (`339eeb73`) — sporeGate can rebuild `coralreef.exe`

### Wave 155i: strandGate Live Validation + Deep Debt Execution + Windows Readiness (2026-07-29)

#### Fixed (Windows cross-compilation)
- `ipc/mod.rs` re-exports of `default_unix_socket_path`, `start_unix_jsonrpc_server`, `unix_socket_path_for_base` now platform-gated with non-Unix stubs returning `Unsupported`
- `ipc/btsp.rs` `use crate::env_keys` cfg-gated to `#[cfg(unix)]` (only used in Unix-gated functions)
- `ipc/btsp.rs` `b64_encode()` cfg-gated to `#[cfg(unix)]`
- `primal-rpc-client/transport.rs` non-Unix `connect_local` — `unused_async` allowed with reason
- `local_transport.rs` non-Unix `connect_local` — `unused_async` allowed with reason
- `ipc/btsp_client.rs` — `duplicated_attributes` allowed (parent module also gates `dead_code`)
- `ecosystem/mod.rs` `spawn_registration` — `needless_pass_by_value` allowed (ownership needed on Unix)
- `service/provenance.rs` `try_sign` — `missing_const_for_fn` allowed (Unix variant does I/O)
- `cargo check --target x86_64-pc-windows-gnu --all-features` now passes with zero errors/warnings
- `cargo clippy --target x86_64-pc-windows-gnu --all-features -- -D warnings` now passes

#### Validated
- All 18 JSON-RPC dispatch methods live-validated against running coralReef instance on strandGate (TCP :45071)
- BTSP Phase 2→3 chain verified end-to-end with live security-domain provider (`btsp.session.create` → `btsp.negotiate` — null cipher fallback correct for unauthenticated test session)
- RTX 3090 (sm_86) WGSL shader compilation confirmed (64 bytes, 13.3ms compile time, provenance hash generated)
- Multi-target compilation (sm_86 + sm_89 + sm_120): 3/3 success from single WGSL source
- GEMM compilation for sm_86 (1024×1024×1024 f32): 26,907 bytes
- Capability-domain symlink (`shader.sock → coralreef-core-default.sock`) active
- Discovery file (`coralreef-core.json`) correct with provides/requires/transports
- WireGuard mesh connectivity to golgiBody hub verified (37ms RTT)

#### Evolved (capability-based abstraction)
- `beardog_socket()` → `security_provider_legacy_socket()` — primal-name-free API
- `"BEARDOG_FAMILY_SEED"` → `env_keys::BTSP_FAMILY_SEED` — capability-based constant
- `"FAMILY_SEED"` → `env_keys::FAMILY_SEED` — centralized env key constant
- Added `BTSP_FAMILY_SEED` env key in `env_keys.rs`

#### Added
- `write_ptx!` / `writeln_ptx!` macros in `ptx_emit/macros.rs` — infallible String write wrappers
- 463 `.expect("write to String")` calls replaced across 16 PTX emitter files (-363 lines net)

#### Improved
- f64 lowering clone density reduced: `func_math_exp_log.rs` dispatch by `&SSARef` reference (-7 clones), `newton.rs` last-use move (-1 clone), `opt_copy_prop` copy-type source ref helper (-2 clones)
- Zero `.expect("write to String")` remaining in PTX emitter

#### Identified
- Glibc depot rebuild needed from sporeGate — no `x86_64-unknown-linux-gnu` depot binary exists yet (cellMembrane P0 code shipped, rebuild pending)
- Unix socket stale (Connection refused) — TCP transport operational, no impact

### Wave 155f: strandGate Deep Debt Execution (2026-07-28)

#### Fixed
- 10 compile errors in `coralreef-core` (missing `config::beardog_socket()`, missing `config::compile_timeout()`, private visibility on `btsp::discover_security_socket` and `btsp::discover_by_capability`, missing `unix_jsonrpc::handle_connection`)
- `default_unix_socket_path()` now delegates to canonical 4-tier `socket_base_dir()` resolution (`BIOMEOS_SOCKET_DIR` > `XDG_RUNTIME_DIR` > `/run/{ns}` > `$TMPDIR`)
- 4 previously failing integration tests (`unix_jsonrpc_default_socket_path_env`) now pass
- Clippy `assertions_on_constants` in `coral-reef-isa/latency.rs`
- Formatting drift across workspace
- ~50 clippy pedantic/nursery violations across both crates (infallible casts, doc backticks, redundant closures, `div_ceil`, needless collect, dead code annotations)

#### Added
- `config::security_provider_legacy_socket()` — composition launcher alias for security-domain provider (originally `beardog_socket()`, evolved in 155i)
- `config::compile_timeout()` — env-configurable compile deadline (`CORALREEF_COMPILE_TIMEOUT_SECS`)
- `unix_jsonrpc::handle_connection()` — BTSP Phase 3 encrypted transport handler with ChaCha20-Poly1305 AEAD frame loop
- 7 new JSON-RPC dispatch routes: `shader.compile.multi`, `shader.compile.gemm`, `health.version`, `btsp.negotiate`, `auth.check`, `auth.mode`, `auth.peer_info`
- `capabilities.list` alias for `capability.list`
- `BEARDOG_SOCKET` env key in `env_keys.rs`

#### Changed
- Repository URL updated from GitHub to Forgejo (`git.primals.eco`)
- `btsp::discover_security_socket` and `btsp::discover_by_capability` visibility: `fn` → `pub(crate) fn`
- `newline_jsonrpc` re-exports `config::compile_timeout` for tarpc transport
- Test count: 3527 passed, 0 failed, 6 ignored (was 3665+4 failing)
- Clippy pedantic+nursery: zero warnings across all targets

### Wave 155b: Deep Debt — Test Extraction & File Size (2026-07-27)

#### Changed
- `amd/encoding.rs`: extracted 362-line test module to `encoding_tests.rs` (795 → 436 LOC)
- `ir/op_misc/mod.rs`: extracted 237-line test module to `op_misc_tests.rs` (747 → 513 LOC)

### Wave 152: Deep Debt — Deduplication & File Size + Doc Refresh (2026-07-26)

#### Added
- `require_math_arg()` helper in PTX emit module: centralizes 18 `arg.ok_or_else(|| NotImplemented("func without argN"))` patterns across `math.rs`, `math_ext.rs`, `math_ext_trig.rs`
- `assert_ok_or_not_implemented()` shared helper in `compiler_integration/main.rs`: eliminates 26 repeated assertion patterns across `pipeline.rs`, `sm70.rs`, `stress.rs`
- `dataflow_tests.rs`: extracted 478-line test module from `dataflow.rs` (768 → 293 LOC)

#### Changed
- 5 codegen coverage test files now import from canonical `codegen_sat/helpers.rs` via `#[path = ...]` instead of duplicating `opts_for`, `compile_for`, `compile_fixture_all_nv`
- Test count reconciled to 3669 (3665 passing, 4 ignored)
- All 13 root docs aligned to Wave 152: README, STATUS, WHATS_NEXT, CONTEXT, EVOLUTION, ABSORPTION, CONTRIBUTING, START_HERE, sporeprint/validation-summary, genomebin/README, genomebin/manifest.toml, specs/CORALREEF_SPECIFICATION, CHANGELOG

#### Fixed
- Broken cross-reference in `specs/CORALREEF_SPECIFICATION.md`: `specs/SOVEREIGN_MULTI_GPU_EVOLUTION.md` → `docs/archive/SOVEREIGN_MULTI_GPU_EVOLUTION.md`

### Wave 151b: BTSP Client Handshake — Standard Evolution (2026-07-26)

#### Added
- `btsp_client` module: synchronous BTSP `ClientHello → ServerHello → ChallengeResponse → HandshakeComplete` wire protocol per `BTSP_PROTOCOL_STANDARD` v1.0
- `BtspSession` struct: authenticated session result with `session_id` + `cipher`
- `BtspClientError` typed errors via `thiserror` (Io, Json, Protocol)
- `provider_rpc()`: security provider JSON-RPC helper for `btsp.session.create` / `btsp.session.verify`
- Byte-by-byte wire I/O (`read_json_line`, `write_json_line`) avoiding `BufReader` buffering interference
- 14 new tests: session structs, wire error parsing, JSON-RPC line I/O, provider connectivity

#### Changed
- `provenance.rs`: `try_sign()` now performs BTSP handshake before `crypto.sign` RPC when `FAMILY_ID` is set (production mode); development mode unchanged
- `btsp.rs`: `discover_security_socket()` elevated to `pub` for cross-module client handshake use
- Uses songBird-standard params: `family_seed_ref: "env:FAMILY_SEED"`, `role: "client"` (aligned with `BTSP_PROTOCOL_STANDARD`)

### Wave 146: Silicon Atheism Phase 2 — Server-Side Transport Abstraction (2026-07-16)

#### Added
- `local_transport` server-side: `bind_local()`, `prepare_local_bind()`, `install_capability_symlink()` centralize all socket bind/cleanup/discovery logic — matching client-side `connect_local()` pattern
- New tests: `bind_local_to_tempdir_succeeds`, `prepare_local_bind_creates_parent_and_removes_stale`, symlink lifecycle tests

#### Changed
- `BoundAddr::Unix` → `BoundAddr::Local` — variant always compiled on all platforms, no more `#[cfg(unix)]` on the enum
- `unix_jsonrpc.rs`: module de-cfg-gated — `handle_connection` and path functions compile on all platforms; `start_unix_jsonrpc_server` returns `Unsupported` on non-Unix via `local_transport::bind_local`
- `tarpc_transport.rs`: `start_tarpc_unix_server` de-cfg-gated — uses `local_transport::prepare_local_bind` + `bind_local`; non-Unix returns `Unsupported`
- `ipc/mod.rs`: `mod unix_jsonrpc` and all re-exports de-cfg-gated; `default_tarpc_bind()` always returns `unix://` path (callers handle non-Unix gracefully)
- `main.rs`: **all `#[cfg(unix)]` / `#[cfg(not(unix))]` blocks removed** from `cmd_server` orchestration — unified `Option<PathBuf>` flow handles UDS/Both/TCP on all platforms via runtime error handling
- Test modules: `make_response` import path changed from `unix_jsonrpc` to `newline_jsonrpc` (module restructuring)

#### Removed
- 12 `#[cfg(unix)]` / `#[cfg(not(unix))]` annotations from server orchestration code

### Wave 145: Deep Debt — Error Handling, De-hardcoding, Deduplication (2026-07-16)

#### Removed
- **`BEARDOG_SOCKET`** env var and `security_provider_socket_legacy()` — hardcoded primal name excised; pure capability-based `$BTSP_PROVIDER_SOCKET` is the sole path

#### Changed
- `provenance.rs`: all `.ok()` calls in `try_sign()` now emit `tracing::warn` with context before discarding errors; timeouts extracted to `CRYPTO_SIGN_READ_TIMEOUT` / `CRYPTO_SIGN_WRITE_TIMEOUT` named constants
- `btsp_negotiate.rs`: mutex poison on session/key registries now logged with `tracing::warn` instead of silently downgrading
- `main.rs`: collapsed duplicate `cmd_server` (tarpc / no-tarpc) into single unified function (788→627 LOC, net -169 lines)
- `service/mod.rs`: `IDENTITY_ADVERTISED` stores `Arc<IdentityGetResponse>` — JSON-RPC `identity.get` avoids per-call clone
- `tarpc_transport.rs`: `identity_get` handler explicitly clones from Arc (tarpc requires owned return)

### Wave 144: Silicon Atheism Phase 2 — Transport Abstraction (2026-07-16)

#### Added
- `local_transport` module: centralized `connect_local()` (async) and `connect_local_sync()` platform-dispatched transport — Unix UDS, non-Unix returns `Unsupported`
- `primal-rpc-client` internal `connect_local()` helper replacing duplicated `#[cfg(unix)]`/`#[cfg(not(unix))]` stubs

#### Changed
- `ecosystem/mod.rs`: `socket_is_alive()` and `send_jsonrpc_line()` now use `local_transport` dispatch instead of raw `UnixStream`
- `ipc/btsp.rs`: `create_btsp_session()` uses `connect_local()` instead of raw `UnixStream::connect`
- `service/provenance.rs`: `try_sign()` uses `connect_local_sync()` instead of raw `std::os::unix::net::UnixStream`
- `primal-rpc-client/transport.rs`: `unix_roundtrip()` and `unix_line_roundtrip()` unified — no more duplicated cfg blocks

### Wave 143: Deep Debt — File Splits, Namespace-Agnostic Paths (2026-07-15)

#### Changed
- Split 3 test files >800 LOC into 6 focused modules: `codegen_coverage_extended` (926→540) + `codegen_coverage_crossarch` (422), `spring_absorption` (925→601) + `spring_absorption_advanced` (349), `codegen_coverage_targeted` (858→431) + `codegen_coverage_ops` (431)
- Extract `btsp.rs` inline tests to `btsp/tests_btsp.rs` (797→416 LOC production code)
- `socket_base_dir()` now uses `ecosystem_namespace()` for `/run/{ns}` and `{ns}-runtime` paths — full `$BIOMEOS_ECOSYSTEM_NAMESPACE` override support
- Removed 4 unused imports: `service`, `Bytes` (tests_tarpc), `DeviceTarget` (service/tests), `super::*` (main_tests/discovery)

### Wave 141a: Cross-Architecture Adoption (2026-07-09)

#### Changed
- `#[cfg(unix)]` / `#[cfg(not(unix))]` guards on all Unix-specific code: `transport.rs`, `ecosystem/mod.rs`, `btsp.rs`, `btsp_negotiate.rs`, `provenance.rs`, `main.rs`, `service/mod.rs`
- `cargo check --target x86_64-pc-windows-gnu` now succeeds with zero errors and zero warnings
- `unix_roundtrip()` returns `ErrorKind::Unsupported` on non-Unix platforms

### Wave 133b: Convergence Pattern Hardening (2026-07-07)

#### Added
- `rust-toolchain.toml`: explicit stable channel with `rustfmt`, `clippy`, `llvm-tools-preview`
- `.cargo/config.toml`: ecosystem cross-compilation targets (musl, Android) and convenience aliases

#### Changed
- Verified `PRIMAL_BIND_MODE=tcp_only` correctly skips all UDS attempts

### Wave 133a: Android UDS Adaptation (2026-07-07)

#### Fixed
- **CORALREEF-ANDROID-01**: UDS fatal on Android (grapheneGate) — `socket_base_dir()` now uses 4-tier resolution with temp-dir fallback when `/run/biomeos` does not exist (Android, Termux, constrained environments)
- **tarpc TCP fallback**: tarpc server bind failure on Unix sockets now falls back to TCP (`127.0.0.1:0`) instead of exiting fatally — allows startup on platforms where UDS paths are unwritable
- Updated socket resolution tests to be platform-aware (handle both `/run/biomeos`-present and temp-dir-fallback scenarios)
- Doc comments updated from "3-tier" to "4-tier" resolution across `config.rs`, `unix_jsonrpc.rs`, `ipc/mod.rs`

### Wave 126: SM120 Blackwell Edge Cases (2026-06-28)

#### Fixed
- **Loop break/continue control flow**: `break` now emits `bra` to loop exit label instead of `ret;`; `continue` branches to loop header. Fallback (break outside loop) adds `membar.sys` before `ret` for Blackwell readback safety (GAP-HS-115 closure)
- **Subgroup reduce multiply**: `SubgroupOperation::Mul` in `CollectiveOperation::Reduce` now uses shuffle-based warp reduction instead of emitting invalid `redux.sync.add` (PTX `redux.sync` has no `.mul` opcode)
- **NumSubgroups/SubgroupId multi-dim**: `NumSubgroups` now computes `ceil(ntid.x * ntid.y * ntid.z / 32)` instead of `ntid.x / WARP_SZ`. `SubgroupId` linearizes thread index across all dimensions before dividing by warp size
- **SubgroupSize literal**: uses `mov.u32 r, 32` instead of referencing `WARP_SZ` symbol, ensuring JIT compatibility

#### Added
- 6 new SM120 PTX emitter tests: break-uses-label, break-outside-loop-membar, subgroup-size-literal-32, num-subgroups-all-dims, subgroup-id-linearized, loop-break-no-bare-ret
- 5 new batch compile tests: single SPIR-V job, mixed WGSL/SPIR-V/GLSL batch, invalid base64 SPIR-V, SM120 batch compile, SPIR-V batch helper
- Stale documentation footers updated (3284/Wave 68 → 3649/Wave 126, sporeprint "0 ignored" corrected to "4 ignored (hardware-gated)")

### Wave 125: shader.compile.multi — Mixed-Input Batch Compilation (2026-06-23)

#### Added
- `shader.compile.multi` JSON-RPC method: accepts array of compilation jobs with mixed input types (WGSL, SPIR-V base64, GLSL) and independent target architectures in a single RPC call
- `BatchCompileRequest`, `BatchCompileJob`, `BatchCompileJobResult`, `BatchCompileResponse` wire types with caller-provided `label` for job correlation
- tarpc `compile_multi` method mirrors JSON-RPC for high-perf binary transport
- 17 new tests: handler tests (single WGSL, single GLSL, mixed, cross-vendor, partial failure, unsupported input type, empty jobs, labels, index order, serde roundtrip, FMA policy, case-insensitive input_type) + dispatch tests (WGSL job, GLSL job, mixed, empty rejected, invalid params)
- SM120 Blackwell codegen verified: 17 dedicated tests confirm PTX emission with `.target sm_120` — no fallback to SM70

### Wave 124: Code Size Compliance — File Splits + Named Constants (2026-06-22)

#### Changed
- `op_conv.rs` (801 LOC) split into `op_conv.rs` (365 LOC, conversion/move ops) + `op_shuffle.rs` (448 LOC, permute/select/shuffle/predicate/reduction ops) — semantic cohesion preserved, both re-exported via `mod.rs`
- `sm75_instr_latencies/gpr.rs` (814 LOC) split into `gpr.rs` (376 LOC, enum + op_category + RAW) + `gpr_hazards.rs` (467 LOC, WAW/WAR/pred hazard tables) — restricted visibility `pub(in ...)` maintains encapsulation
- Capability advertisement metadata extracted to 8 named constants: `LATENCY_WGSL_NV_P50_MS`, `LATENCY_WGSL_NV_P99_MS`, `LATENCY_WGSL_AMD_P50_MS`, `LATENCY_WGSL_AMD_P99_MS`, `LATENCY_SPIRV_NV_P50_MS`, `LATENCY_SPIRV_NV_P99_MS`, `MAX_CONCURRENT_COMPILES`, `MAX_MULTI_TARGETS` — zero magic numbers in `self_description()`

#### Added
- `PrmtSelByte::is_valid()` public API for hardware nibble validity checking — replaces private field access pattern

### Wave 123: Artifact Provenance Evolution — Signing + sporePrint Hash (2026-06-22)

#### Added
- `provenance` module: extracted artifact provenance logic from `CompileResponse::with_provenance()` into dedicated `service::provenance` module
- `crypto.sign` discovery: runtime discovery of crypto-domain provider via ecosystem discovery files; best-effort signing of artifact content hashes (Ed25519 over NDJSON Unix socket)
- `sporeprint_hash` field on `ArtifactProvenance`: BLAKE3 content hash for Nest provenance integration (content-addressed storage indexing without re-hash)
- `blake3` dependency (v1.8, `pure` feature — zero C, zero ASM)
- `crypto.sign` added to `genomebin/manifest.toml` consumed capabilities
- BTSP discovery helpers promoted to `pub(crate)` for cross-module capability scanning

#### Changed
- `CompileResponse::with_provenance()` now delegates to `provenance::build_provenance()` — same SHA-256 hash + gate + compiler version, now also populates BLAKE3 sporePrint hash and attempts crypto.sign signing
- BTSP discovery flake fix: `discover_security_socket_returns_none_in_clean_env` test now guards against live discovery files on host

#### Fixed
- Environmental test flake in `discover_security_socket_returns_none_in_clean_env` (was hitting a real `btsp.session.create` provider on the build host)

### Wave 120: Deep Primal Self-Knowledge — Full Ecosystem Name Scrub (2026-06-21)

#### Changed
- Complete primal-name scrub: evolved all remaining ecosystem component references across test files and comments to capability-domain language (tensor-dispatch, neural-compute, materials-compute, health-domain, bio-statistical pipeline)
- Deprecated env alias doc: `PRIMALSPRING_AUTH_MODE` comment now uses "legacy ecosystem naming" instead of referencing source primal
- Transport wire-compat doc: `sourdough_core::TransportEndpoint` → "ecosystem canonical `TransportEndpoint`"
- Test fixtures evolved: discovery test data uses capability-domain identifiers (`compute-dispatch`, `security-provider`, `storage-provider`) instead of primal names
- Test function names evolved: `mesh_registration_payload_is_songbird_compatible` → `mesh_registration_payload_is_discovery_compatible`, `discover_from_ecosystem_toadstool_compute_dispatch` → `discover_from_ecosystem_compute_dispatch_provider`

#### Added
- `primal.announce` added to `genomebin/manifest.toml` consumed capabilities (matches actual startup behavior)

### Wave 119: Deep Debt Evolution — Health Readiness & Derived Capabilities (2026-06-19)

#### Changed
- `health.readiness` evolved from stub to track actual startup state via `STARTUP_INSTANT` `OnceLock`
- `sm_target` in compile capabilities dynamically derived from `NvArch::ALL.last()` instead of hardcoded `"sm_120"`
- `ecosystem.rs` refactored to directory module: production code in `mod.rs` (343 LOC), tests extracted to `tests/tests_ecosystem.rs`

### Wave 118: Deep Debt Evolution — Primal Self-Knowledge (2026-06-19)

#### Changed
- Primal self-knowledge compliance: scrubbed all peer-primal names from production docs and comments (capability-domain language throughout)
- Ray-query honesty: all `RayQueryFunction` variants now return `CompileError::NotImplemented` instead of emitting incorrect PTX stubs
- `btsp.rs` test extraction: inline tests (~310 lines) moved to `btsp/tests/tests_btsp_session.rs` (file 1178 → 779 LOC)
- `save_graphviz` diagnostic function gated under `#[cfg(test)]` (was `#[expect(dead_code)]`)
- Named constants: `NONCE_BYTES`, `MIN_CLIENT_NONCE_BYTES`, `POLY1305_TAG_BYTES`, `MIN_FRAME_BYTES` (btsp_negotiate), `NVIDIA_DEFAULT_WARP_SIZE` (compile)

#### Removed
- `RayIntersectionRegs` struct (unused after ray-query NotImplemented evolution)
- Hardcoded primal names from doc comments: `barraCuda`, `toadStool`, `bearDog`, `Songbird`, `sourDough`, `neuralSpring`, `wetSpring/rhizoCrypt`

#### Fixed
- Ray-query test assertions updated to expect `CompileError::NotImplemented`
- Clippy: `&mut self` → `&self` on ray-query functions (no longer mutating)
- Clippy: removed `feature = "diagnostics"` cfg condition (undeclared feature)

### Wave 113: riboCipher Signal Compliance (2026-06-15)

#### Added
- Bare `health` JSON-RPC method (guideStone HEALTH-01 schema: `{status, primal, version, uptime_s}`)
- `service::mark_startup()` uptime tracking via `OnceLock<Instant>`
- `RIBOCIPHER_PREFIX` constant (`[0xEC, 0x01]`) for ecosystem signal protocol

#### Fixed
- riboCipher signal acceptance: consume full 2-byte prefix on UDS and TCP sockets (was consuming only 1 byte, leaving `0x01` in stream → parse error)
- `health` added to `PUBLIC_METHODS` gate (probe passes without auth)

### Wave 109: Standard Primal Startup Envelope (2026-06-11)

### Wave 107: PRIMAL-SOCKET-CLEANUP (2026-06-10)

#### Fixed
- `default_tarpc_bind()` fallback: `std::env::temp_dir()` → `config::socket_base_dir()` (3-tier resolution)
- `unix_socket_path_for_base()` fallback: `std::env::temp_dir()` → `config::socket_base_dir()`
- Zero `/tmp` usage in any production socket path — enables `ProtectSystem=strict` systemd hardening

### Wave 101: Deep Debt — Server Lifecycle Extraction (2026-06-08)

#### Added
- `server_lifecycle.rs`: extracted `write_discovery_file`, `remove_discovery_file`, `write_pid_file`, `remove_pid_file`, `wait_for_shutdown_signal` from `main.rs`
- Named constants: `TCP_PEEK_TIMEOUT`, `DEFAULT_HEARTBEAT_INTERVAL_SECS`, `PCI_VENDOR_NVIDIA`, `PCI_VENDOR_AMD`, `INTEL_DEFAULT_WAVE_SIZE`, cost/latency hint constants

#### Changed
- `main.rs` reduced from 827 to 704 lines
- `naga` dependency hoisted to `[workspace.dependencies]`
- Eliminated duplication between per-crate naga version declarations

### Wave 100: Transport Evolution — TRANSPORT_ENDPOINT Injection (2026-06-08)

#### Added
- `ipc::transport` module: `TransportEndpoint` enum (Uds/Tcp/MeshRelay), `ResolvedBind` enum, `resolve_bind()` function
- Ecosystem wire-compatible `#[serde(tag = "transport")]` format — zero new deps
- `TRANSPORT_ENDPOINT` env var accepted at startup (launcher/Tower Atomic injection)
- 19 new transport tests (wire format, resolution, error paths)

#### Changed
- `cmd_server` uses `resolve_bind()` for dynamic transport setup based on env injection
- `log_composition_env()` now logs `TRANSPORT_ENDPOINT`

### Wave 99: capabilities.list IPC Compliance (2026-06-08)

#### Added
- `capabilities.list` alias in newline JSON-RPC dispatch (plural form probed by ecosystem)
- `capabilities.list` added to `SERVED_METHODS` and `capability_registry.toml`
- 2 new dispatch tests: `dispatch_capability_list_singular`, `dispatch_capabilities_list_plural_alias`

#### Fixed
- `default_unix_socket_path()` now delegates to `config::default_socket_path()` for canonical 3-tier resolution

### Wave 79: Headless Fix — Default Members (2026-06-05)

#### Fixed
- VPS deployment regression: `cargo build --release` no longer builds `tools/amd-isa-gen`
- Added `default-members` to workspace `Cargo.toml` excluding `tools/amd-isa-gen`
- Flaky `discover_returns_none_when_no_socket` test isolated via temp dir

#### Metrics
- 3304 tests, 0 failures, 0 clippy warnings

### Wave 78: Mesh Propagation & SPIR-V End-to-End Verification (2026-06-04)

#### Added
- `mesh_registration_payload_is_songbird_compatible` test: validates `capability.register` payload serialization, transport advertisement, SPIR-V metadata presence
- `discovery_peers_response_matches_shader_compile_schema` test: verifies peers see correct output_formats (spirv, native_binary), SPIR-V version list, provenance flag
- `test_spirv_end_to_end_compile_provenance_output` test: full pipeline (WGSL → native + SPIR-V → provenance → naga re-validation → entry point assertion)
- `spv-in` feature added to naga dev-dependency for SPIR-V roundtrip validation in tests

#### Changed
- Removed unused imports from `service/tests.rs` and `service/tests_serde.rs` (hygiene)

#### Metrics
- 3307 tests, 0 failures, 0 clippy warnings

### Wave 77: Deep Debt Sweep — Smart Refactoring & Full Audit (2026-06-03)

#### Changed
- `expr_eval.rs` (834L→340L): Extracted image/texture/surface evaluation into new `expr_image.rs` (492L) — cohesive PTX image load, sample, query, gather operations
- `math.rs` (809L→646L): Moved `Tanh`/`Sinh`/`Cosh` implementations into `math_ext_trig.rs` (428L→592L) where trig/hyperbolic ops belong
- Formatting normalized across workspace via `cargo fmt`

#### Audit Results (all clean)
- **Unsafe code**: All isolated to test files (env var mutation, Rust 1.85+), all with SAFETY docs + mutex serialization. All prod crates `#![forbid(unsafe_code)]`
- **External dependencies**: 100% pure Rust — zero C/C++, zero `*-sys`, zero openssl/ring/vendor SDKs
- **Hardcoded values**: All paths use 3-tier resolution (env override → XDG → fallback). Zero primal name coupling
- **Production mocks**: None — `coral-reef-stubs` is legitimate compiler IR, not a mock
- **NotImplemented stubs**: All are legitimate architecture boundaries (mesh/task shaders, hardware-specific PTX features) or defensive error paths

#### Metrics
- 3303 tests, 0 failures, 0 clippy warnings, 0 unsafe in production
- Zero production files over 800 lines (excluding tests and ISA-generated tables)

### Wave 76: SPIR-V Portable Output & Mesh Capability (2026-06-03)

#### Added
- `SpirVOptions` struct: configurable SPIR-V version targeting (`(1,3)`, `(1,5)`, `(1,6)`), `zero_init_workgroup_memory`, `force_loop_bounding`
- `CompileOptions.spirv` field for SPIR-V backend control
- `CompileWgslRequest.emit_spirv` — opt-in SPIR-V output alongside native binary
- `CompileWgslRequest.spirv_version` — target specific SPIR-V version `[major, minor]`
- Capability metadata now advertises `output_formats: ["native_binary", "spirv"]` and `spirv_output` details
- 3 provenance unit tests: `test_provenance_attached_on_with_provenance`, `test_provenance_serde_roundtrip`, `test_provenance_hash_deterministic`
- SPIR-V output validation tests: naga validates emitted SPIR-V, entry point preservation, version targeting, atomics, shared memory, complex control flow
- Service tests: `test_compile_wgsl_no_spirv_when_emit_false`, `test_compile_wgsl_spirv_version_targeting`

#### Changed
- `wgsl_to_spirv()` now respects `CompileOptions.spirv` for version/flags (was hardcoded defaults)
- `handle_compile_wgsl` SPIR-V emission is now conditional on `emit_spirv` request field (was always-on)
- Mesh capability readiness confirmed: Songbird propagation fixed upstream, `shader.compile` registers correctly

#### Metrics
- 3301 tests, 0 failures, 0 clippy warnings, 0 unsafe blocks

### Wave 74: Evolution Sweep — Composition, Coverage, Self-Knowledge (2026-06-03)

#### Added
- **3x3 matrix inverse** (`emit_inverse3x3`): Cofactor-based adjugate/det inverse for 3x3 matrices via PTX
- **`config::socket_base_dir()`**: Canonical 3-tier socket resolution — single source of truth
- **`config::default_socket_path()`**: Canonical primal socket path for bind + announce
- **`config::shutdown_timeout()`** / **`config::registry_timeout()`**: Env-configurable via `$CORALREEF_SHUTDOWN_TIMEOUT_SECS` / `$CORALREEF_REGISTRY_TIMEOUT_SECS`
- **`service::SERVED_METHODS`**: Single `pub const` eliminating duplication between capability.list and primal.announce

#### Fixed
- **Socket path divergence** (composition bug): ecosystem announced wrong path when `BIOMEOS_SOCKET_DIR` set — now both bind and announce use `config::default_socket_path()`
- **`ANNOUNCED_METHODS` duplication**: Removed from ecosystem.rs — now references `service::SERVED_METHODS`
- **AMD `tanh` error clarity**: Precise "should lower to Exp2+Rcp+FMul" (defensive guard)

#### Changed
- math_pack.rs refactored (904→662), service/tests.rs refactored (974→589)
- Ray query stub evolved (fail-dangerous → fail-safe)
- All ipc/btsp socket resolution delegates to `config::socket_base_dir()`
- `clippy::pedantic` + `clippy::nursery` pass clean

#### Metrics
- 3284 tests, 0 failures, 0 clippy warnings
- Zero unsafe, zero C deps, zero production unwrap()
- Socket path: single canonical resolution across all modules

### Wave 68: SM120 Barrier Fix, Sovereign SPIR-V, Math Pack/Unpack, Module Hardening (2026-06-02)

#### Added
- **Sovereign SPIR-V emission** (GAP-HS-124): `wgsl_to_spirv()` public API emits valid SPIR-V binary via `naga::back::spv::write_vec()`. `CompileResponse.spirv_binary: Option<Bytes>` populated for WGSL compile paths — toadStool can pass directly to `vkCreateShaderModule`
- **13 math pack/unpack builtins** (PTX SM100+): `Pack4x8unorm`, `Pack4x8snorm`, `Unpack4x8unorm`, `Unpack4x8snorm`, `Pack2x16float`, `Unpack2x16float`, `Pack2x16unorm`, `Pack2x16snorm`, `Unpack2x16unorm`, `Unpack2x16snorm`, `Transpose` (2x2/3x3/4x4), `Determinant` (2x2/3x3/4x4), `Inverse` (2x2)
- New module `crates/coral-reef/src/codegen/nv/ptx_emit/math_pack.rs` — dedicated PTX emission for data packing, matrix, and struct-returning math
- `resolve_entry_point()` hardened: explicit compute-stage enforcement, improved error messages with available entry point list
- 39 new tests: 11 math pack/unpack PTX, 5 module hardening, 3 SPIR-V emission, 2 service SPIR-V, 1 membar verification, + coverage improvements

#### Fixed
- **SM120 membar.sys barrier** (GAP-HS-115): PTX emitter now inserts `membar.sys` before `ret;`/`exit;` for SM120+ targets. SASS does this via `insert_exit_system_membar()` automatically but PTX does not — root cause of Blackwell zero readback in ReduceScalarPipeline
- **tarpc bincode serialization**: Removed `#[serde(skip_serializing_if)]` from `spirv_binary` field — bincode is positional and `skip_serializing_if` caused `UnexpectedEof` on deserialization

#### Changed
- `CompileResponse` gains `spirv_binary: Option<Bytes>` field (set to `None` for SPIR-V input and GEMM compiles)
- `eval_math()` catch-all now routes through `eval_math_pack()` before falling back to `NotImplemented`

#### Metrics
- 3284 tests, 0 failures, 0 clippy warnings, 0 unsafe
- coral-reef lib: 79.82% line coverage, 85.36% function coverage

#### FRAGOs
- `FRAGO_CORALREEF_SM120_BARRIER_FIX_WAVE68_JUN02_2026.md` — hotSpring: rebuild + re-test ReduceScalarPipeline on RTX 5060
- `FRAGO_CORALREEF_SPIRV_EMISSION_WAVE68_JUN02_2026.md` — hotSpring/toadStool: use `spirv_binary` for `vkCreateShaderModule`

---

### Wave 67b: hotSpring Gap Resolution — Arch Routing & Copy Prop Fix (2026-06-01)

#### Fixed
- **GAP-CR-001**: `resolve_arch()` — adapter-aware architecture inference. When callers pass an `AdapterDescriptor` without explicit arch, coralReef now infers the correct SM target from hardware identity (e.g. RTX 5060 → sm_120, RTX 4090 → sm_89). Response `arch` field now reflects effective target, not just the default
- **GAP-CR-002**: `opt_copy_prop` panic on multi-component SSA — assertion `entry_ssa.comps() == 1` changed to a guard (`continue` on multi-component sources). Fixes crash on `subgroupBallot` which returns `uvec4` (4 components)
- **GAP-CR-003 (documented)**: `pow(f64, f64)` is rejected by naga's WGSL parser per spec. IR-level f64 pow (via `OpF64Log2 + OpDMul + OpF64Exp2`) works through SPIR-V. Polyfill pattern: `exp2(y * log2(x))`. Test coverage added for both paths

#### Added
- `infer_arch_from_adapter()` — maps NVIDIA/AMD device names to SM/ISA targets (Blackwell, Ada, Ampere, Volta, RDNA2/3)
- `test_subgroup_ballot_copy_prop_f64_sm70` — regression test for uvec4 copy propagation
- `func_ops_f64_pow_wgsl_rejected_by_spec` — documents naga spec limitation
- `func_ops_f64_pow_ir_translation_works` — verifies f64 multiply/IR path

#### Metrics
- 3245 tests, 0 failures, 0 clippy warnings, 0 unsafe
- `plasmidbin install coralreef` deployed (BLAKE3: 74e7a98c)

---

### Wave 67: Pipeline Completeness — Hyperbolic & Float Decomposition (2026-06-01)

#### Added
- **Hyperbolic trig**: `sinh` (exp difference), `cosh` (exp sum), `asinh` (log + sqrt), `acosh` (log + sqrt), `atanh` (log ratio)
- **Float decomposition**: `modf` (trunc + subtract), `frexp` (lg2 + normalize), `ldexp` (x * 2^exp)
- **Bit scan**: `firstTrailingBit` (brev + clz), `firstLeadingBit` (clz + invert)
- 8 new PTX emitter unit tests (sinh, cosh, asinh, acosh, atanh, ldexp, firstTrailingBit, firstLeadingBit)

#### Fixed
- **`/tmp` elimination**: Removed last `std::env::temp_dir()` fallback in `opt_instr_sched_common.rs` debug path — now uses `XDG_RUNTIME_DIR` or `/run/biomeos`
- **`shader.compile.capabilities` math_ops count**: Updated from 25 → 34 to reflect full coverage

#### Metrics
- 3242 tests, 0 failures, 0 clippy warnings, 0 unsafe
- `math.rs` 803 LOC, `math_ext.rs` 896 LOC — both under limit

---

### Wave 61: Deep Debt Resolution — Math Completeness & Module Refactor (2026-05-29)

#### Added
- **Inverse trigonometry**: `tan` (sin/cos ratio), `atan` (polynomial approx), `atan2` (ratio + approx), `asin` (rsqrt-scaled atan), `acos` (π/2 − asin)
- **Geometry math**: `reflect` (I − 2·dot(N,I)·N), `faceForward` (setp + selp conditional negate)
- **Bit manipulation**: `extractBits` → PTX `bfe.u32`, `insertBits` → PTX `bfi.b32`
- **Texture query routing**: `eval_image_query` now tries texture bindings first (`txq.*`) before surfaces (`suq.*`)
- **`textureNumLevels`**: Emits `txq.num_mip_levels.b32` via texture binding path
- **`textureDimensions` (texture path)**: Emits `txq.width/height/depth.b32`
- **`textureNumLayers` (texture path)**: Emits `txq.array_size.b32` via texture binding
- 12 new PTX emitter unit tests (tan, atan, atan2, asin, acos, reflect, faceForward, extractBits, insertBits, textureNumLevels, textureDimensions, textureNumLayers)

#### Changed
- **`math.rs` split**: Core scalar math in `math.rs` (655 LOC), extended geometry/trig/bits in `math_ext.rs` (651 LOC) — both under 1000-line limit
- **Test module extraction**: `ptx_emit/mod.rs` (1814→143 LOC) split into `tests_core.rs`, `tests_image.rs`, `tests_math_ext.rs`
- **Dependencies**: All transitive deps bumped to latest compatible patch versions

#### Fixed
- `eval_math` dispatch correctly routes 13 extended functions through `eval_math_extended` helper

### Wave 53: Primal Mountain — Implementation Depth (2026-05-26)

#### Added
- **Vector math functions**: `normalize` (rsqrt + component mul), `length` (dot + sqrt), `cross` (fma-based 3-component), `distance` (sub + dot + sqrt)
- **Texture load `tld.*`**: `textureLoad` on sampled textures now emits `tld.b.{dim}.v4.s32.f32` (previously only storage surfaces were supported)
- **`ImageQuery::NumLayers`**: Emits `suq.array_size.b32` for storage array surfaces
- **Depth texture comparison PTX**: `tex.level.compare.{dim}.f32.f32` with reference value in coordinate tuple
- **Array/cube texture sampling**: Extended `ImageDim` with `Cube`, `A1d`, `A2d`, `Acube` variants. Layer indices wired through `format_tex_coord`.
- **Live toadStool discovery integration tests**: Full node-atomic pipeline (discovery → target resolution → compile)
- 11 new tests: vector math, tld, NumLayers, branching, loops, multi-arch, shared memory

#### Changed
- **RT core intersection**: Evolved from zero-init stub to `_rt_query_get_intersection_*` driver-resolved call builtins (kind, t, instance_custom_index, instance_id, sbt_offset, geometry_index, primitive_index, barycentrics, front_face)

#### Fixed
- **Surface collection bug**: Sampled and depth textures were incorrectly collected as surfaces (defaulting to `Rgba32` format). Now only `Storage` class images become surfaces; sampled/depth images are handled exclusively by the texture path.

### Wave 47: Deployment Behavioral Convergence (2026-05-24)

#### Added
- **`--socket PATH` CLI flag**: Server subcommand now accepts `--socket` to override the default UDS path. Enables uniform NUCLEUS composition launcher usage across all primals.
- CLI test `parse_cli_server_socket_override` validates the new flag.

#### Changed
- **`health.liveness` response**: Normalized from `{"alive":true}` to `{"status":"alive"}` per `DEPLOYMENT_BEHAVIOR_STANDARD`. Health sweeps now work with `jq -r .status` uniformly.

### Wave 44: Neural API Wire Fix (2026-05-23)

#### Fixed
- **`primal.announce` wire identity**: Renamed `"name"` → `"primal"` in the JSON-RPC params (biomeOS `PrimalAnnouncement` requires this exact key — prior payload was silently rejected).
- **Added `methods` array**: 16 served method names now included in the announce payload (`shader.compile.*`, `health.*`, `identity.get`, `capability.list`, `btsp.negotiate`, `auth.*`), enabling `methods_registered` tracking in biomeOS.
- **Added `pid` field**: Process ID for biomeOS utilization tracking.

#### Changed
- Test `primal_announce_payload_has_required_fields` now asserts `"primal"` field (not `"name"`), methods array is non-empty, and pid is present.

### Wave 43: Neural API `primal.announce` Adoption (2026-05-23)

#### Added
- **`primal.announce` handler**: On startup, coralReef sends a `primal.announce` JSON-RPC call to biomeOS (Neural API), registering routing metadata:
  - `capabilities`: `["compile", "shader_compile", "gpu"]`
  - `signal_tiers`: `["node"]`
  - `cost_hints`: `{ "compile": 60.0, "shader_compile": 80.0, "gpu": 100.0 }`
  - `latency_estimates`: `{ "compile": 500, "shader_compile": 800, "gpu": 50 }`
  - `socket`: full UDS path for routing back
- Unit test `primal_announce_payload_has_required_fields` validates schema correctness.

### Sprint 12: RayQuery PTX Emission — RT Core Activation (2026-05-14)

#### Added
- **`Statement::RayQuery` PTX emission**: All five `RayQueryFunction` operations now compile for SM75+ targets:
  - `Initialize` — allocates opaque query state (64-bit handle), emits RT initialization sequence
  - `Proceed` — queries traversal state machine, produces bool predicate for candidate availability
  - `GenerateIntersection` — reports procedural hit at given `t` value
  - `ConfirmIntersection` — confirms current triangle candidate as committed hit
  - `Terminate` — early termination of ray traversal
- **`Expression::RayQueryGetIntersection`**: Returns `RayIntersection` struct with 10 fields (kind, t, instance_custom_data, instance_index, sbt_record_offset, geometry_index, primitive_index, barycentrics, front_face). Supports both committed and candidate intersection queries.
- **`Expression::RayQueryProceedResult`**: Boolean result expression for ray query proceed operations.
- **SM75+ RT validation gate**: RayQuery operations reject SM70 and earlier with a clear error message ("requires SM75+ for RT core access").
- New types: `RayQueryState`, `RayIntersectionRegs` in `ptx_emit/types.rs`.
- Type resolution: `resolve_expr_type_handle` handles `RayQueryGetIntersection` (returns module's `ray_intersection` special type) and `RayQueryProceedResult` (returns `Scalar::BOOL`).
- 4 new tests: Initialize+Proceed, GetIntersection, Terminate, SM70 rejection.

#### Changed
- `EVOLUTION.md` checklist updated: `RayQuery` statement and `RayQueryGetIntersection` expression now marked as Phase B horizon (wired).
- Total: **3202 tests** (was 3177). Zero clippy warnings. Zero unsafe.

#### Architecture Note
The emitted PTX uses RT comment stubs (`// rt.trace.*`) marking where hardware RT core instructions will be inserted once toadStool provides acceleration structure dispatch. The wiring is complete — WGSL ray query shaders compile through the full pipeline and produce valid PTX structure. Hardware activation requires SM75+ RT core access via toadStool's dispatch surface.

---

### Post-101 — Sprint 11: PTX Evolution — Textures, Calls, tarpc (2026-05-14)

#### Added
- **`ImageSample` PTX emission**: Sampled textures (`texture_1d<f32>`, `texture_2d<f32>`, `texture_3d<f32>`, `texture_2d<u32>`, depth textures) now emit PTX `tex.*` instructions via `.texref` declarations. Supports `textureSampleLevel` (explicit LOD), `textureSampleGrad` (gradient-based), and depth comparison sampling. Coordinate formatting handles 1D scalar, 2D vec2, and 3D vec3.
- **`textureGather` PTX emission**: `tld4.{r,g,b,a}.2d.v4.{type}.{type}` instructions for 2D texture gather operations. Returns 4 texels from a 2x2 footprint at the specified component.
- **Function call inlining**: `Statement::Call` now inlines callee functions directly into PTX body. Supports arguments, return values, local variables, nested calls, and void functions. Callee expressions are evaluated in an isolated context with argument mapping.
- New types: `TextureBinding`, `TexChannelType` (F32/S32/U32) in `ptx_emit/types.rs`.
- **`ImageAtomic` PTX emission**: `sured.b.{1d,2d,3d}.{op}.{type}.zero` instructions for atomic operations on storage texture surfaces. Supports add, min, max, and, or, xor, exchange, and compare-and-swap.
- **`WorkGroupUniformLoad` statement**: Emits `bar.sync 0` + `ld.shared.u32` + `bar.sync 0` (barrier-load-barrier pattern per WGSL spec semantics).
- 12 new tests: 5 ImageSample, 1 textureGather, 4 function call inlining, 1 WorkGroupUniformLoad, 1 ImageAtomic.
- **`shader.compile.gemm` on tarpc transport**: `ShaderCompileTarpc` trait now exposes `gemm(GemmCompileRequest)` method. tarpc consumers (bincode over TCP/Unix) can use tensor-core GEMM compilation without JSON-RPC.

#### Changed
- **Subgroup multiply reduction documented as unsupported**: Error message explicitly states SM70-SM120 scope (no hardware `redux.sync mul` on any SM generation).
- Total: **3177 tests** (was 3165). Zero clippy warnings. Zero unsafe.

---

### Post-101 — Sprint 10: PTX Coverage Evolution + Code Quality (2026-05-15)

#### Added
- **`ImageQuery` PTX emission**: `textureDimensions()` on storage textures now emits `suq.width.b32`/`suq.height.b32`/`suq.depth.b32` via the PTX emitter. Supports 1D, 2D, and 3D storage texture queries.
- **8 new math functions** in PTX emitter: `saturate`, `radians`, `degrees`, `countOneBits` (`popc`), `countLeadingZeros` (`clz`), `countTrailingZeros` (`brev` + `clz`), `reverseBits` (`brev`), `smoothStep` (polynomial).
- **3 new builtin variables** (PTX emitter): `WorkGroupSize` (`%ntid`), `NumSubgroups` (`ntid.x / WARP_SZ`), `SubgroupId` (`tid.x / WARP_SZ`).
- **4 new builtins in `naga_translate`** (IR path): `SubgroupSize` (constant 32), `NumSubgroups` (compile-time `ceil(flat_wg/32)`), `WorkGroupSize` (compile-time constants), `SubgroupId` (runtime `local_invocation_index >> 5`). Fixes `func_builtins.rs` `NotImplemented` fallthrough that blocked `sum_reduce_subgroup_f64.wgsl`.
- 2 new ImageQuery tests (1D size, 2D size).
- 3 new SubgroupSize/NumSubgroups tests (SM70 builtin, SM70 reduction, f64 reduction with SubgroupSize indexing).

#### Changed
- **`lib_tests.rs` split** (1052→4 files): Split monolithic test file into `lib_tests/{mod,compile,options,module,gemm_subgroup}.rs` submodules (largest: 441L). Resolves workspace <1000 line rule violation.
- Total: **3165 tests** (was 3160). Zero files >1000 lines. Zero clippy warnings. Zero unsafe.

---

### Post-101 — Sprint 9+: Post-Excision Evolution (2026-05-14)

#### Changed
- **`lib.rs` refactored**: 868→630 lines. GEMM types + `compile_gemm` extracted to `src/gemm.rs` (112L). Preamble injection extracted to `src/preamble.rs` (149L).
- **`newton.rs` refactored**: 849→568 lines. Tests extracted to `newton_tests.rs` (278L).
- **Hardcoded primal names eliminated**: `service/types.rs`, `shader_model.rs`, `func_builtins.rs`, `discovery.rs` doc comments genericized (replaced "toadStool", "coralDriver" with capability-based references).
- **`ECOSYSTEM_AUTH_MODE`** added as primary env var for method gate enforcement; `PRIMALSPRING_AUTH_MODE` kept as legacy fallback.
- **Discovery filter evolved**: Now matches toadStool capability names (`compute.dispatch.*`, `gpu.*`, `compute.hardware.*`) in addition to legacy `gpu.dispatch`
- **Requires declaration**: `gpu.dispatch` → `compute.dispatch` (with `legacy_id` metadata for backward compat)
- **Cross-primal leaks eliminated**: `beardog_socket()` → `security_provider_socket_legacy()`; doc comments no longer reference peer primal internal architecture
- **All workspace deps updated** to latest patch versions (42 updates)
- **Texture format coverage expanded**: `TexelFormat` enum expanded from 4 to 10 variants (`R8`, `R16`, `R32`, `Rg8`, `Rg16`, `Rg32`, `Rgba8`, `Bgra8`, `Rgba16`, `Rgba32`). `StorageFormat` → `TexelFormat` classifier in `emitter.rs` now explicitly handles all naga format variants instead of silently falling through to Rgba32.
- **IPC wire compatibility aliases**: `CompileWgslRequest::wgsl_source` now accepts `"source"` as serde alias. `CompileResponse` fields accept legacy names (`"binary"` → `binary_b64`, `"info"` → `shader_info`). Both `CompileWgslRequest` and `MultiDeviceCompileRequest` carry the alias.

#### Added
- **HMMA codegen for tensor-core GEMM**: `compile_gemm()` API generates PTX `mma.sync.aligned` kernels for NVIDIA SM80+ (Ampere, Ada, Blackwell). Supports `F16`, `F16F32` (mixed-precision), and `TF32` operand precisions. Tile shapes: `m16n8k16` (f16) and `m16n8k8` (TF32). New types: `GemmShape`, `GemmPrecision`.
- `codegen/nv/ptx_emit/gemm.rs`: Standalone PTX GEMM emitter with K-loop unrolling, accumulator zeroing, and fragment load/store
- 5 unit tests (f16f32 basic, TF32 basic, f16 accumulate, multi-K iteration, SM120 Blackwell)
- 7 integration tests (SM80 f16f32, pre-SM80 rejection, AMD rejection, misaligned K, zero dimensions, SM120 Blackwell multi-tile, TF32 K-alignment)
- **`naga::Module` direct ingest H2**: Entry point selection via `CompileOptions::entry_point`, module validation via `naga::valid::Validator` (opt-out with `validate: false`)
- PTX emit path (`emit_compute_ptx_module`) now accepts entry point name for SM100+ targets
- 2 new ecosystem discovery tests (toadStool-style `compute.dispatch.*` JSON, `compute.hardware.*`)
- 4 new `compile_module` tests (f64 software lowering, FMA fused, SM120 PTX path, shared memory reporting)
- 5 new entry point + validation tests (EP selection by name, missing EP error, compute-stage preference, validation rejection, validation bypass)
- 5 new texture format tests (rg32float store, r32uint store, rgba16float store, r32float load, bgra8unorm store)
- 3 new serde wire-compat tests (source alias, multi-device source alias, legacy field alias roundtrip)
- **Subgroup operations**: Full WGSL subgroup support in `naga_translate` — `SubgroupBallot` → `OpVote`, `SubgroupCollectiveOperation/Reduce` → `OpRedux` (SM73+) or butterfly `OpShfl` chain (SM70), `SubgroupGather` → `OpShfl`. Inclusive/exclusive scan via iterated `shfl.up`. `enable subgroups;` directive stripped during preprocessing (naga 28 parser limitation).
- **f64 CallResult type resolution fix**: `resolve_expr_type_handle` now properly resolves `CallResult` expressions to the function's return type. Fixes "wrong type" errors when math functions (`sqrt`, `pow`, `exp`) operate on f64 values returned from user-defined functions.
- 4 new subgroup tests (subgroupAdd SM70/SM120, subgroupBroadcast SM70, subgroupBallot SM70)
- 1 new f64 CallResult type resolution test
- **f64 nested struct member type fix**: `is_f64_expr` now uses `resolve_expr_type_handle` for `AccessIndex`/`Access` expressions instead of the limited `element_scalar` helper (which lacked struct support). Fixes incorrect f32 lowering path for `sqrt`/`exp` on f64 struct member access chains (e.g., `params.inner.value`). Dead `element_scalar` function removed.
- **`health.version` RPC**: New `health.version` method returns `{ session, build_hash, version, name }` for post-upgrade verification. Build hash injected at compile time via `CORALREEF_BUILD_HASH` env var.
- **`shader.compile.gemm` IPC wiring**: The `compile_gemm` library API is now exposed as a JSON-RPC endpoint. Request type `GemmCompileRequest` accepts `{ m, n, k, precision, arch }`. Dispatched through the blocking pool with standard compile timeout.
- 1 new f64 nested struct member math test
- Total: **3160 tests** (was 3159, was 3154, was 3143, was 3130, was 3115 at excision). Zero files >800 lines in production code.

---

### Post-101 — Sprint 9: Diesel Engine Excision — Pure Compiler Primal (2026-05-13)

#### Removed (diesel stack)
- **coral-ember** crate deleted (52 .rs files) — VFIO fd holder, PCIe keepalive, `ember.*` JSON-RPC → toadStool
- **coral-glowplug** crate deleted (70 .rs files) — Root daemon, cylinder subprocesses, ECU routing → toadStool
- **coral-driver** crate deleted (367 .rs files) — DRM/VFIO/nouveau GPU driver layer → toadStool
- **coral-gpu** crate deleted — Unified compile+dispatch API → dispatch delegated to toadStool
- **showcase/** directory deleted — hardware dispatch demos → toadStool domain

#### Changed
- `coralreef-core/discovery.rs`: DRM render node scan replaced with empty fallback — hardware enumeration delegated to toadStool via `compute.dispatch.capabilities` IPC
- Workspace `unsafe_code = "deny"`: no more `unsafe` exception — `#![forbid(unsafe_code)]` on all remaining crates
- `genomebin/manifest.toml`: updated to pure compiler (zero unsafe, 3115 tests at time of excision)
- `.gitignore`: cleaned orphaned entries

#### Tests
- 3115 passing, 0 failed, 3 ignored. Zero clippy warnings. Zero unsafe.

### Post-101 — Sprint 8: Diesel Engine Migration — Feature Freeze + Upstream Handoff (2026-05-13)

#### Feature Freeze (diesel stack)
- `coral-ember/lib.rs`: Added feature freeze notice — no new features, toadStool implements C3/C4/C5
- `coral-glowplug/lib.rs`: Added feature freeze notice with key pattern references for toadStool (cylinder subprocess model, warm handoff API, diesel mode routing)
- `coral-driver/lib.rs`: Added diesel engine migration context to Phase D status doc

#### Upstream Reference (E1/E2)
- E1 (cylinder translation): Documented subprocess isolation pattern — `sovereign.rs`, `socket/mod.rs`, `observer/vfio.rs`, `device/health.rs`
- E2 (warm API): Documented warm handoff/capture API — `capture.rs`, `vendor_lifecycle/nvidia.rs`, `hbm2_training/`, `boot_sequence.rs`
- E3 (FECS cold silicon init): Already shipped (Sprint 7)

#### wateringHole Handoff
- Created `CORALREEF_DIESEL_MIGRATION_HANDOFF_MAY13_2026.md` with full reference map, socket path documentation, and removal criteria

#### Tests
- 4790 passing, 0 failed, 181 ignored. Zero clippy warnings. No regressions.

---

### Post-101 — Sprint 7: FECS/GPCCS Cold-Silicon Stability Proof (2026-05-12)

#### Cold-Silicon Recovery (sentinel blocker — "fully recoverable" proof)
- `fecs_boot.rs`: New `GrBootOutcome` enum — structured result (`Running`/`Failed`/`NoFirmware`) so callers can pattern-match recovery decisions
- `fecs_boot.rs`: New `boot_gr_falcons_with_recovery()` — retries boot up to 3× with PMC GR engine reset cycle between attempts (toggle PGRAPH enable bit to clear stale falcon state)
- `fecs_boot.rs`: New `pmc_gr_reset()` — dedicated PMC GR engine reset (disable→fence→settle→enable→fence→settle pattern)
- `boot_sequence.rs`: Volta+ and Blackwell `cold_init` now use `boot_gr_falcons_with_recovery` instead of single-attempt `boot_gr_falcons`
- `sovereign_stages.rs`: PIO re-bootstrap and ACR fallback paths upgraded to recovery-aware boot
- `mod.rs`: New public `sovereign_gr_boot_with_recovery()` API on `NvVfioComputeDevice`

#### Tests
- 4790 passing, 0 failed, 181 ignored. Zero clippy warnings. No regressions.

---

### Post-101 — Sprint 6: Ecosystem Wave Sync — Phase D Markers, FECS Stability (2026-05-12)

#### FECS Cold Silicon Stability (sentinel blocker evolution)
- `fecs_boot.rs`: `falcon_boot()` now returns `Err(DriverError::SubmitFailed)` on timeout and halted-without-response — previously returned `Ok(result)` masking cold silicon failures
- Callers (`boot_fecs`, `boot_gpccs`, `boot_gr_falcons`) propagate the error to `cold_init` and `sovereign_stages`
- `cold_init` (Volta/Blackwell) continues to handle FECS failures gracefully (warn + continue) but now receives structured errors instead of silent success

#### Phase D Transition Markers (toadStool Phase C is COMPLETE)
- `coral-gpu/context.rs`: Updated dispatch comments from "Blocked on toadStool Phase C" to "Phase C is COMPLETE (S245-S250); Phase D local dispatch wiring is stadial"
- `coralreef-core/discovery.rs`: Updated to "Phase D transition" language
- `coral-driver/lib.rs`: Added Phase D status module doc — hardware modules remain for backward compatibility, compiler-adjacent modules stay with coralReef
- `coral-driver/nv/qmd/mod.rs`: "Phase C contested" → "Phase D status" — encoding absorbed into toadstool-cylinder
- IPC method name aligned to `compute.dispatch.execute` (upstream contract)

#### Soft-Deprecation Updates
- `coral-ember/lib.rs`: Updated from "until Phase C confirms coverage" to "Phase C is COMPLETE — removal gated on Phase D dispatch validation"
- `coral-glowplug/lib.rs`: Same update

#### Tests
- 4790 passing, 0 failed, 181 ignored. Zero clippy warnings. No regressions.

---

### Post-101 — Sprint 5 Cont'd: Deep Debt — Firmware Paths, ICE Consistency, Allow Reasons (2026-05-12)

#### Firmware Path Centralization
- `linux_paths.rs`: New `nvidia_firmware_root()` and `nvidia_firmware_path(chip, tail)` — single source of truth for `CORALREEF_NVIDIA_FIRMWARE_ROOT` (default `/lib/firmware/nvidia`)
- 8 firmware loading sites migrated from hardcoded `/lib/firmware/nvidia/` to `linux_paths::nvidia_firmware_path()`: `fecs_boot.rs`, `pri.rs`, `acr_boot/firmware.rs`, `sovereign_stages.rs`, `kepler_fecs_boot/firmware.rs`, `identity/firmware.rs` (2×), `gsp/firmware_source.rs`
- `gsp/firmware_parser.rs`: Delegated to `linux_paths::nvidia_firmware_root()`, removed redundant `OnceLock`

#### Compiler ICE Consistency
- 11 `unreachable!()` calls in production codegen evolved to `ice!()`: `assign_regs/block.rs`, `nv/sm32/mem.rs` (2×), `nv/sm32/tex.rs` (3×), `nv/sm20/alu/int.rs` (4×), `ir/op_tex/surface_addr.rs`

#### `#[allow]` Reason Annotations
- `coral-glowplug/src/lib.rs`: `#![allow(deprecated)]` and 4× `#[allow(deprecated)]` re-exports annotated with `reason = "..."`
- `coral-ember/src/lib.rs`: `#![allow(deprecated)]` annotated with reason

#### Tests
- 4790 passing, 0 failed, 181 ignored. Zero clippy warnings. No regressions.

---

### Post-101 — Sprint 5: Pass 12 Sentinel Gaps (2026-05-12)

#### `naga::Module` Direct Ingest (Pass 12 — coralReef stability)
- `lib.rs`: New public `compile_module()` and `compile_module_full()` — accept pre-parsed `naga::Module` directly, skipping text→parse round-trip
- `ptx_emit/mod.rs`: New `emit_compute_ptx_module()` — PTX path accepts `&naga::Module` directly for SM100+ targets
- `pub use naga` re-export — downstream crates can construct `naga::Module` without a separate dependency
- 6 new tests: empty module rejection, minimal compute, full metadata, output parity with WGSL path, AMD target, Intel unsupported

#### Compile Deadline (Pass 12 — `bind_stat` timeout)
- `newline_jsonrpc.rs`: All `shader.compile.*` IPC handlers wrapped in `tokio::time::timeout` (default 120s, configurable via `CORALREEF_COMPILE_TIMEOUT_SECS`)
- `tarpc_transport.rs`: Same deadline applied to all tarpc compile methods (spirv, wgsl, wgsl_multi)
- Prevents unbounded blocking from stalling the IPC server on pathological inputs

#### FECS/GPCCS Cold Silicon Init (Pass 12 — firmware command sequencing)
- `boot_sequence.rs`: `VoltaBoot::cold_init` and `BlackwellBoot::cold_init` now attempt PIO falcon boot (`boot_gr_falcons`) when firmware is available on disk
- Graceful fallback: logs clear diagnostic when firmware is missing or ACR/SEC2 chain is required
- `fecs_boot.rs`: `firmware_available()` respects `CORALREEF_NVIDIA_FIRMWARE_ROOT` env var (consistent with GSP firmware parser)
- Unblocks hotSpring Titan V / K80 sovereign GPU validation path

#### Tests
- 4790 passing (+6), 0 failed, 181 ignored. Zero clippy warnings. No regressions.

---

### Post-101 — Sprint 4: PTX SM120 Evolution + Coverage Push (2026-05-12)

#### PTX Emitter — Subgroup Scan Implementation
- `ptx_emit/statements.rs`: Implemented inclusive and exclusive warp-level prefix scans via `shfl.sync.up` butterfly accumulation (5 iterations for warp-32)
- Supports Add, Mul, Min, Max, And, Or, Xor operations with correct identity elements for exclusive scans
- Exclusive scan uses `selp` with type-appropriate identity (0 for add, 1 for mul, +inf/-inf for min/max)

#### PTX Emitter — Silent Catch-All Eliminated
- `ptx_emit/statements.rs`: `_ => Ok(())` catch-all replaced with explicit `NotImplemented` errors for `ImageStore`, `ImageAtomic`, `Call`, `RayQuery`, `WorkGroupUniformLoad`
- Unhandled statement types now fail loudly instead of silently producing incorrect code

#### PTX Expression Evaluator — Subgroup Result Handling
- `ptx_emit/expr_eval.rs`: Added `SubgroupOperationResult` and `SubgroupBallotResult` expression handling — pre-allocates typed registers for statement-driven writes

#### coral-reef-isa — API Evolution + Coverage
- `IsaTarget`: Added `Hash` derive, `ALL` constant, `sm_version()`, `has_independent_thread_scheduling()`, `has_uniform_datapath()` methods
- `InstrLatency`: Extended test coverage for throughput values and edge cases
- `SphBuilder`: Added tests for max barriers, large shared memory, zero GPRs, LE alignment

#### Deep Debt — Hardcoded Path Evolution
- 6 hardcoded `/sys/` and `/proc/` paths in production code evolved to use `sysfs_root()`/`proc_root()`/`dri_render_prefix()` helpers with env var overrides (`CORALREEF_SYSFS_ROOT`, `CORALREEF_PROC_ROOT`, `CORALREEF_DRI_RENDER_PREFIX`)
- Files: `coral-gpu/pcie.rs`, `nvidia_drm.rs`, `vfio_compute/pri.rs`, `mmu_oracle/capture.rs`, `intel/ioctl.rs`, `bin/coral_probe.rs`
- `dri_render_prefix()` promoted to `pub(crate)` for cross-module reuse
- All `#[allow]` attributes given reason strings

#### Tests
- 4784 passing (+19), 0 failed, 181 ignored. Zero clippy warnings. No regressions.

---

### Post-101 — Sprint 3 Cleanup + ICE Consistency (2026-05-12)

#### Vestigial Pattern Cleanup (Sprint 3 — Compute Trio Phase C)
- `coral-ember/src/lib.rs`: Crate-level deprecation doc comment (absorbed into toadStool Phase A)
- `coral-glowplug/src/lib.rs`: Crate-level deprecation doc comment (absorbed into toadStool Phase B)
- `coral-gpu/src/context.rs`: Phase D TODO — future routing through toadStool IPC (`compute.dispatch.submit`)
- `coralreef-core/src/discovery.rs`: Phase C/D transition comment — replace DRM call with toadStool IPC when ready
- `coral-driver/src/nv/qmd/mod.rs`: Documented as Phase C contested module (toadStool absorbs encoding, coralReef provides values)

#### RDNA2 Atomics Correctness Fix
- `codegen/ops/memory.rs`: `atom_op_to_flat` now takes `AtomType` — unsigned min/max correctly maps to `FLAT_ATOMIC_UMIN`/`UMAX` instead of signed opcodes

#### ICE Consistency (PTX Emitter)
- `ptx_emit/math.rs`: bare `unreachable!()` → `ice!("rounding mode matched Floor|Ceil|Round|Trunc above")`
- `ptx_emit/expr_arith.rs`: 2× bare `unreachable!()` → `ice!()` with descriptive invariant messages

#### Tests
- 4765 passing, 0 failed, 181 ignored. Zero clippy warnings. No regressions.

---

### Iteration 101 — Deep Debt: Smart Refactoring + Unsafe Evolution (2026-05-12)

#### Smart Refactoring (3 files, semantic domain extraction)
- `error.rs` (928L) → `error/mod.rs` (412L) + `error/vfio.rs` (523L): VFIO-path error enums (`PciDiscoveryError`, `DevinitError`, `ChannelError`, `SovereignStagesError`) extracted to dedicated module; public API preserved via re-exports
- `nv/mod.rs` (857L) → 747L + `nv/fecs_init.rs` (124L): FECS channel initialization (Phase 3 of nouveau device init) extracted as self-contained method module
- `pfifo.rs` (882L) → 695L + `vfio/channel/bar2_init.rs` (199L): BAR2 page table setup (virtual memory concern) extracted from PFIFO scheduler module; callers in `glowplug/warm.rs` and `diagnostic/interpreter/probe/domain.rs` updated

#### Unsafe Evolution
- `mem::zeroed()` eliminated for 3 `#[repr(C)]` ioctl param structs in `coral_kmod.rs`: `CoralInitComputeParams`, `CoralBindChannelParams`, `CoralAllocGpuBufferParams` → `#[derive(Default)]` + safe struct literal initialization with `..Default::default()`

#### ICE Consistency
- 3 bare `panic!()` calls in PTX emitter (`types.rs`, `emitter.rs`) evolved to `ice!()` macro — consistent with codegen ICE policy; provides source location and bug-report guidance

#### Comprehensive Audits
- `.unwrap()` sweep: all instances confined to `#[cfg(test)]` modules — zero in production library code
- `panic!()` sweep: all remaining production `panic!()` are via `ice!()` macro (compiler invariants) — zero bare panics in library code
- External deps: all pure Rust except optional `cudarc` (cuda feature); transitive `libc` via rustix only
- Production mocks: zero — all test mocks in `#[cfg(test)]` modules
- Hardcoded primal names: zero in runtime code (doc comments only)
- `#[allow]` audit: all annotations have explicit `reason` strings — zero unexplained suppressions

#### Tests
- 4765 passing, 0 failed, 181 ignored. Zero clippy warnings. Identical counts to pre-refactoring.

### Iteration 100 — PTX Atomics + Warp Primitives + Soft-Deprecation (2026-05-12)

#### PTX Atomics (7 ops + CAS)
- `atom.{global,shared}.{add,and,or,xor,min,max,exch,cas}` + subtract via negate-then-add

#### PTX Memory Barriers
- `membar.{cta,gl}` for STORAGE and WORK_GROUP scopes

#### PTX Warp/Subgroup Primitives
- `shfl.sync.{idx,up,down,bfly}`, `vote.sync.ballot`, `redux.sync.*`
- `SubgroupInvocationId` / `SubgroupSize` builtins

#### Soft-Deprecation
- `coral-glowplug`: `#[deprecated(since = "0.2.0")]` on all public modules — toadStool Phase B absorption confirmed
- `coral-ember`: `#[deprecated(since = "0.2.0")]` on all public modules — toadStool Phase A absorption confirmed

#### Wire Contract Enhancement
- `math_ops`, `sm_target`, `atomics`, `subgroup_ops` fields added to `shader.compile.capabilities` response

#### RDNA2 Parity
- Full parity confirmed (25+ ops): all PTX math operations have RDNA2 equivalents via IR decomposition

#### Tests
- 4765 passing (+4), 0 failed, 181 ignored. Zero clippy warnings.

### Iteration 99 — PTX Emitter SM120/Blackwell Evolution (2026-05-12)

#### Switch Statement Implementation
- PTX emitter now handles `naga::Statement::Switch` — generates `setp.eq.s32` comparison chain with labeled branches, default fallthrough, and proper break semantics
- Unblocks real-world shaders using switch/case on SM120

#### Math Functions (10 new operations)
- `Pow`: `lg2 → mul → ex2` chain (base-2 logarithm trick)
- `Exp`: `x * log2(e) → ex2` (natural exponential via base-2)
- `Log`: `lg2 → * ln(2)` (natural logarithm via base-2)
- `Sign`: predicate-based sign extraction (`setp.gt/lt → selp`)
- `Fract`: `cvt.rmi (floor) → sub` (fractional part)
- `Mix`: `sub → fma` (linear interpolation via FMA)
- `Step`: `setp.ge → selp` (step function)
- `Dot`: vector component-wise `mul + fma` accumulation
- `Tanh`: `2x * log2e → ex2 → rcp → selp` approximation
- Existing: `Abs`, `Min`, `Max`, `Clamp`, `Floor/Ceil/Round/Trunc`, `Sqrt`, `InverseSqrt`, `Sin`, `Cos`, `Exp2`, `Log2`, `Fma`

#### Tests
- 7 new PTX unit tests: switch, pow/exp/log, fma/clamp/abs, fract, sqrt/exp2/log2, if/else, loop
- 4761 passing (+7), 0 failures, 181 ignored. Zero clippy warnings.

### Iteration 98 — Firmware Panic Elimination + Deep Audit (2026-05-11)

#### Firmware `.expect()` → Result Propagation
- **`NvVfioComputeDevice::sysmem_acr_boot`**: `AcrFirmwareSet::load().expect("firmware load")` → `DriverResult<AcrBootResult>` with `?` propagation
- **`NvVfioComputeDevice::sysmem_physical_boot`**: Same evolution — firmware load errors now propagate instead of panicking
- **`NvVfioComputeDevice::hybrid_acr_boot`**: Same evolution — all three ACR boot entry points are now panic-free on firmware load failure
- Hardware test callers updated to `.expect("firmware load")` (acceptable in `#[ignore]` hardware-gated tests)

#### Deep Audit Results (Comprehensive 800L+ File Review)
- 7 production files between 800–928L assessed: `error.rs` (928), `uvm/structs.rs` (907), `pfifo.rs` (882), `nv/mod.rs` (857), `newton.rs` (849), `vbios_devinit.rs` (836), `gpr.rs` (814) — all under 1000L cap, all cohesive (hardware data definitions, algorithmic units, error type hierarchies). No forced splits.
- `ember.rs` (802L): 649L production + 153L tests. Monitor item only.
- `btsp_negotiate.rs` `.expect()`: confirmed test-only, not attacker-reachable
- `Arc<Mutex<>>` / `Arc<RwLock<>>` patterns in ember/glowplug: shared mutable registries, correct use; OnceLock/LazyLock not applicable
- Zero new debt found across all categories

#### Tests
- 4754 passing, 0 failures, 181 ignored. Zero clippy warnings.

### Iteration 97 — Smart Refactoring + Stub Evolution (2026-05-11)

#### Smart File Refactoring (>800L)
- **`nv/ioctl/mod.rs`** (929→655 lines): Extracted GEM buffer management (alloc, mapping, pushbuf submission) to `nv/ioctl/gem.rs` (299 lines). Natural domain boundary — GEM operations vs channel management.
- **`vfio/channel/mod.rs`** (896→594 lines): Extracted Kepler (GK110/GK210) channel creation to `vfio/channel/kepler_channel.rs` (305 lines). Architecture-specific split — Kepler 2-level page tables vs Volta+ 5-level.

#### Stub Elimination
- `IntelDevice::stub()` → `IntelDevice::host_emulated()`: Removed "stub" naming from production API. Method now documented as host-memory emulation (buffer ops work in host memory, dispatch builds real batch but returns error until DRM exec is wired).

#### Audit Results (No Action Needed)
- Zero `Result<_, String>` in production code
- Zero `.unwrap()` in library code
- Zero `eprintln!` in production library code (CLI binaries retain idiomatic `eprintln!`)
- Zero `async_trait` or `lazy_static` direct usage
- `MockWritesMutexPoisoned` already `#[cfg(test)]` gated
- All 45+ `#[expect(dead_code)]` annotations verified valid (DMA lifetime, HW register maps, WIP, generated tables)
- `deny.toml` enforced: no `openssl`, `ring`, `cmake`, `bindgen`, `*-sys` (except `linux-raw-sys` via rustix)

#### Tests
- 4754 passing, 0 failures, 181 ignored. Zero clippy warnings.

### Iteration 96 — Compute Trio Wire Contract + Extraction Boundary (2026-05-11)

#### Wire Contract Alignment (Compute Trio Gate 1)
- `CompileResponse` field renames via `#[serde(rename)]`: `binary`→`binary_b64`, `arch`→`target`, `info`→`shader_info`
- `CompilationInfoResponse` field renames: `gpr_count`→`gprs`, `shared_mem_bytes`→`shared_memory`, `barrier_count`→`barriers`, `workgroup_size`→`workgroup`
- New fields: `wave_size` (32 NVIDIA, 32/64 AMD), `local_memory` (per-thread scratch bytes), `compile_time_ms` (wall-clock compilation timing)
- `CompileCapabilitiesResponse.supported_archs` → wire name `targets` (Gate 1 contract)
- `DeviceCompileResult` field renames: `binary`→`binary_b64`, `info`→`shader_info`

#### Upstream Compiler Changes
- `coral-reef::CompilationInfo` gained `local_mem_bytes: u32` — populated from `ShaderInfo::shared_local_mem_size` in NVIDIA and AMD backends
- PTX emit path defaults to 0 (PTX driver manages local memory)
- `wave_size_for(GpuTarget)` helper derives warp/wave size from target architecture

#### Extraction Boundary Documentation
- Handoff: `CORALREEF_ITER96_COMPUTE_TRIO_CONTRACT_EXTRACTION_MAY11_2026.md`
- coral-ember (~11k LOC, 216 tests) + coral-glowplug (~21k LOC, 484 tests) → toadStool absorption candidates
- coral-driver hardware modules (BAR0/MMIO, VFIO, DRM, UVM, GSP, Falcon/SEC2) → toadStool
- coralReef retains: coralreef-core, coral-reef compiler, coral-gpu, coral-driver QMD/cubin/generation

#### Tests
- Wire contract shape assertions: `binary_b64`, `target`, `shader_info`, `gprs`, `shared_memory`, `barriers`, `workgroup`, `wave_size`, `local_memory`, `compile_time_ms` verified on wire
- Gate 1 assertion: `targets` array present on `shader.compile.capabilities` wire output
- 4754 passing, 0 failures, 181 ignored. Zero clippy warnings.

### Iteration 95 — eprintln! → tracing Migration (2026-05-08)

#### Structured Logging
- 57 `eprintln!` calls migrated to `tracing::{debug,warn,error}` with structured fields across 5 production files
- Files: `open_userspace.rs` (30), `open_kmod.rs` (16), `compute_trait.rs` (9), `device/mod.rs` (1), `nvidia_drm.rs` (1 duplicate removed)
- CLI binary (`coral_probe.rs`) and test files retain `eprintln!` per convention

### Iteration 94 — JH-0 MethodGate Adoption (2026-05-07)

#### Security: Pre-Dispatch Capability Gate (JH-0)
- New `ipc/method_gate.rs`: ecosystem-standard pre-dispatch authorization per `METHOD_GATE_STANDARD.md` v1.0
- Method classification: Public (`health.*`, `identity.get`, `capability.list`, `auth.*`, `lifecycle.status`) vs Protected (`shader.compile.*`, `btsp.negotiate`, all other)
- `EnforcementMode::Permissive` (default): logs unauthenticated calls to protected methods but allows
- `EnforcementMode::Enforced`: rejects with `-32001 PERMISSION_DENIED` when `CORALREEF_AUTH_MODE=enforced`
- `CallerContext`: bearer token + peer credentials + connection origin (Unix/Loopback/Remote)
- Global gate via `OnceLock` — initialized from env on first access

#### New JSON-RPC Methods
- `auth.check` — returns `{ authenticated: false, origin: "loopback" }` (bearer token presence check)
- `auth.mode` — returns `{ mode: "permissive" }` (current enforcement mode)
- `auth.peer_info` — returns `{ peer: null, origin: "loopback" }` (peer credential introspection)

#### Capability Advertisement
- `capability.list` now includes `auth.check`, `auth.mode`, `auth.peer_info` in methods array
- `auth` added to capability domains

#### Tests
- 15 unit tests: method classification (health, identity, capability, auth public; shader, btsp protected; unknown protected), gate behavior (permissive allows, enforced rejects, token passes), enforcement mode, caller context
- 3 TCP integration tests: `auth.check`, `auth.mode`, `auth.peer_info` roundtrip verification
- 4742 passing, 0 failures, 181 ignored. Zero clippy warnings.

### Iteration 93 — hotSpring Merge Hardening + Coverage Expansion (2026-05-07)

#### Deep Debt — Env-Overridable Paths (hotSpring regression)
- `open_userspace.rs`: 5 hardcoded `/dev/nvidiactl` and `/dev/nvidia{N}` → `nv_ctl_path()` / `nv_gpu_path_prefix()`
- `open_kmod.rs`: 3 hardcoded paths → same env-overridable functions
- `compute_trait.rs`: 2 hardcoded paths → same
- `channel_setup.rs`: 5 hardcoded paths → `nv_ctl_path()` / `nv_gpu_path_prefix()`

#### Unsafe Code Safety
- `open_kmod.rs`: 4 missing `// SAFETY:` comments on VolatilePtr writes and ptr::copy_nonoverlapping
- `open_userspace.rs`: 2 missing `// SAFETY:` comments on fence init and push buffer copy

#### Coverage Expansion
- `pri.rs`: 4 new tests — hub station params writes, privring timing mask application, VBIOS ring init logic (dead/alive)
- `pgob.rs`: 6 new tests — power step table validation (count, alignment, range), PgobOutcome clone/debug/data

#### Full Audit (All Clear)
- `Result<_, String>` in production: zero (all remaining in test helpers)
- Large files: all under 1000L, cohesive (no splits needed)
- Unsafe code: all blocks documented with `// SAFETY:`, inherent to GPU driver (ioctl/mmap/MMIO)
- External deps: all pure Rust; `cudarc` feature-gated opt-in; transitive `libc` is upstream evolution
- Mocks in production: zero (all `#[cfg(test)]` gated)
- Hardcoded paths: zero remaining in production (sysfs/VFIO kernel ABI paths are inherent)

#### Tests
- 4704 passing, 0 failures, 181 ignored (hardware-gated). Zero clippy warnings.

### Iteration 92 — Wire Standard L3 + Deep Debt Pass (2026-05-06)

#### Deep Debt — Typed Errors
- `coral_probe.rs`: All `Result<_, String>` evolved to `ProbeError` enum with `thiserror` (ResourceOpen, Mmap, Timeout, ChildFailed, Fork, HexParse)
- Zero `Result<_, String>` remaining in any production code (binary or library)

#### Deep Debt — Env-Overridable Paths
- `coral_kmod.rs`: `CORAL_RM_PATH` → `coral_rm_path()` with `CORALREEF_CORAL_RM_PATH` env override
- `uvm/constants.rs`: `NV_CTL_PATH`, `NV_UVM_PATH`, `NV_GPU_PATH_PREFIX` → functions with `CORALREEF_NV_*` env overrides
- `drm.rs`: `DRI_RENDER_PREFIX` → `dri_render_prefix()` with `CORALREEF_DRI_RENDER_PREFIX` env override
- `handlers_kmod.rs`: `KMOD_SYSFS`, `KMOD_DEV` → `kmod_sysfs_path()`, `kmod_dev_path()` with env overrides

#### Large File Assessment (all under 1000L, cohesive)
- `error.rs` (893L): 5 domain error enums — cohesive error hierarchy, no split needed
- `nv/mod.rs` (811L): `NvDevice` + `ComputeDevice` trait impl — single-responsibility
- `vfio/channel/mod.rs` (896L): `VfioChannel` — already has 12 submodules extracted

#### Wire Standard L3
- `CapabilityListResponse` now includes `protocol: "jsonrpc-2.0"` and `transport: ["uds", "tcp", "tarpc"]` fields
- Test `capability_list_wire_standard_l2` upgraded to `capability_list_wire_standard_l3` with L3 field assertions
- Serde roundtrip test updated to include new fields

#### Cross-Cutting Audit (primalSpring Phase 59)
- **BufReader post-negotiate**: Verified correct — BufReader is passed through to `process_encrypted_frames`, no `into_inner()` needed. Buffered bytes are correctly consumed by the encrypted frame reader.
- **Whitespace-tolerant TCP detection**: Assessed — not needed for coralReef's ecosystem. Our BTSP marker classification (`{` = JSON-RPC, other = BTSP) is correct for all ecosystem clients.
- **Port 9730**: Confirmed operational on ironGate. primalSpring now has `TCP_FALLBACK_CORALREEF_PORT = 9730`.

### Iteration 91 — Coverage Expansion + Deep Debt Audit (2026-05-04)

#### Coverage
- `coral-glowplug/capture.rs`: 7 new tests for `TrainingRecipe` (JSON roundtrip, load nonexistent, load invalid JSON, `flat_writes` aggregation + empty, `training_dir` env override, `recipe_path_for_chip` formatting)

#### Zero-Alloc Performance Evolution
- `EntryFlags::aperture_name`: `String` → `Cow<'static, str>` (eliminates heap alloc on every PDE/PTE decode)
- `ember.rs`: 7× `format!("{req}\n")` → `write_rpc_line()` helper (eliminates redundant String allocation per JSON-RPC write)
- `diff_snapshots`: `Vec::with_capacity(domains.len())` pre-sizes output
- `device_open` probe: `Vec::with_capacity(8)` for fixed-size register scan

#### Deep Debt Audit (All Clear)
- Zero `async_trait` usage (native async traits throughout)
- Zero `lazy_static` usage (`OnceLock`/`LazyLock` only)
- Zero `Box<dyn Error>` in production code
- All `.clone()` hotspots are necessary SSA IR manipulation (compiler passes)
- All `Arc<Mutex<>>` are correct patterns (short critical sections, no `.await` under lock)
- All `#[expect(dead_code)]` carry reason strings
- `Result<_, String>` only in `coral_probe.rs` (binary CLI tool)
- No modernization gaps found

#### Tests
- 4686 passing, 0 failures, 160 ignored (hardware-gated)
- Zero clippy warnings (pedantic + nursery)

### Iteration 90 — BTSP Phase 3 Transport Verification + Deep Debt (2026-05-03)

#### Transport Reachability Fix
- Marker byte consumption: non-`{` first byte (BTSP handshake marker) now consumed from `BufReader` before `handle_connection` — previously left in buffer, corrupting first JSON-RPC line read on production BTSP-authenticated connections
- TCP accept loop: same fix applied for consistency

#### GAP-04 Resolution (tarpc health endpoint)
- Documented as **intentional design**, not debt: tarpc transport has full health triad (`health_check`, `health_liveness`, `health_readiness`) + `identity_get` + `capability_list`
- Architecture: tarpc on `-tarpc.sock` suffix, JSON-RPC on main socket — primalSpring reaches health via JSON-RPC, hotSpring compilers use tarpc for high-perf binary calls

#### Deep Debt Pass
- `compile_file()` evolved from `Result<Vec<u8>, (ExitStatus, String)>` to typed `CompileFileError` with `thiserror` — carries `ReadInput`/`InvalidUtf8`/`Compile` variants, source chaining, `exit_status()` accessor
- `ember.rs` rustdoc socket path corrected to XDG-based resolution
- `shader_binary.rs` gfx909 comment: "placeholder" → "Raven Ridge APU"
- Full audit confirmed: zero `.unwrap()` in production, zero `Result<_, String>` in library, zero mocks leaking to production, all hardcoded paths env-overridable, all unsafe confined to `coral-driver` with `// SAFETY:`

#### Verification
- Integration test `test_btsp_phase3_encrypted_frame_loop_reachable`: full roundtrip through `handle_connection` → `btsp.negotiate` → `take_negotiated_keys` → `process_encrypted_frames` with client-side AEAD encrypt/decrypt

### Iteration 89 — BTSP Phase 3 + Kepler Hardening + Deep Audit (2026-05-02)

#### BTSP Phase 3 — Full AEAD + Wire Transport
- `handle_negotiate` extracts `handshake_key` from BearDog's `btsp.session.create` response (per BTSP_PROTOCOL_STANDARD v1.0)
- Derives ChaCha20-Poly1305 session keys via HKDF-SHA256 (`btsp-session-v1-c2s`/`btsp-session-v1-s2c`)
- Returns `cipher:"chacha20-poly1305"` when key available; graceful `"null"` fallback when absent
- `SessionKeys` struct: Zeroize-on-drop, encrypt/decrypt with random 12-byte nonces
- **Encrypted frame loop** wired in `unix_jsonrpc.rs`: after `btsp.negotiate` → `take_negotiated_keys()` → `[4B BE u32 len][nonce||ciphertext+tag]` framing
- `BtspOutcome::session_id()` accessor for transport layer
- `#[allow(dead_code)]` removed from `encrypt`/`decrypt`/`take_negotiated_keys` — all live production paths
- Session registry upgraded: `HashMap<String, SessionEntry>` with `handshake_key`
- `btsp.rs` split: Phase 2 guard (461L) + `btsp_negotiate.rs` (619L)
- deps: `hkdf` 0.12, `sha2` 0.10, `chacha20poly1305` 0.10, `getrandom` 0.3, `zeroize` 1, `rand` 0.9, `base64` 0.22

#### Kepler SCHED_ERROR Resolution (hotSpring downstream)
- RAMFC fields 0x3C (`DMA_LIMIT_REF`) and 0x44 (`PB_DMA_SUBROUTINE`) added — fixes CONTEXT_RELOAD_TIMEOUT
- Kepler runlist polling: replaced GV100 `RUNLIST_PENDING` (0x2284) with PFIFO_INTR bit 30
- Human-readable SCHED_ERROR reason decoding
- `expect()` → `Result` propagation in `device_open.rs` Kepler guard

#### Deep Audit Confirmation
- Zero bare `#[allow]` without reason — all 7 crate-level blocks now carry `reason` strings
- `SovereignStagesError::vfio_compute` cfg-gated behind `feature = "vfio"`
- Unfulfilled `#[expect(cast_possible_truncation)]` removed from page_tables.rs
- `ok_or_else` → `ok_or` for constant error value (clippy)
- Intel `ioctl.rs` kernel UAPI mirror allow block annotated with reason

#### Tests
- 21 BTSP Phase 3 + crypto tests (negotiate, HKDF derivation, encrypt/decrypt round-trip, tamper detection, wrong-key rejection)
- 4 Kepler PFIFO unit tests
- Zero clippy warnings (pedantic + nursery), zero fmt drift

### Iteration 88 — Deep Debt, Typed Errors, Hotspring Merge (2026-04-30)

#### Branch Consolidation
- Merged `hotspring-sec2-hal`: GPU generation profiles (`GenerationProfile`, `NvArch`, `AmdArch`), WIP PTX emitter for SM120/Blackwell, Intel/AMD dispatch
- Merged `iter70d-deep-audit-evolution` (ours strategy — superseded by main)
- Deleted both remote branches post-merge

#### Smart File Refactoring (>1000L → Cohesive Submodules)
- `ptx_emit.rs` (2190L → 11 files, max 396L): emitter, builtins, expr_arith, expr_cast, expr_eval, expr_misc, math, statements, pointers, types
- `uvm_compute/device.rs` (1625L → 5 files, max 868L): gpfifo, memory, open_kmod, open_userspace
- `qmd.rs` (1307L → 10 files, max 609L): types, field, sm_config, v21_v22, v23, v30, v50, build, tests
- `uvm/mod.rs` (1037L → 4 files, max 514L): constants, devices, uvm_tests
- `kepler_fecs_boot.rs` (1774L → 8 files, max 526L): reg_access, firmware, gr_precursor, firmware_upload, boot_protocol, load_boot, post_done
- `kepler_warm.rs` (1375L → 6 files, max 532L): preflight, post_done_firmware, early_pmu_pmc, gr_hub_load, fecs_engctl_warm

#### CR-04: Typed Errors Wave 4
- `SovereignStagesError` (coral-driver): BAR0/PMC, HBM2 training, devinit, Kepler firmware, falcon/GR, verify-stage
- `TrainingRecipeError` (coral-glowplug): read/parse/create-parent/serialize/write
- `GoldenStateLoadError` + `HeldBar0Error` (coral-ember): golden state I/O, BAR0 mapping
- Zero `Result<_, String>` remaining in production library code

#### Safety & Code Quality
- Added `// SAFETY:` comments to all undocumented unsafe blocks (coral_probe, pri, cuda, device, compute_trait)
- Hardcoded BDF in `pri.rs` parameterized; env var override for firmware dumps (`CORALREEF_FECS_DUMP_DIR`)
- All `.unwrap()` in ptx_emit → `.expect("reason")` or error propagation
- SM120 test tolerance via `catch_unwind` (8 test files)

#### IPC Timing
- `docs/IPC_COMPOSITION_AND_LATENCY.md` updated with transport overhead table (Unix JSON-RPC ~0.1–0.3ms, TCP ~0.3–0.8ms, tarpc ~0.05–0.15ms)

#### Tests
- 4639 passing, 0 failures, 177 ignored (hardware-gated). Zero clippy warnings.

### Iteration 87 — P1: UDS JSON-RPC Protocol Fix (2026-04-30)

#### Protocol Fix
- **`resolve_uds_binds`**: When composition passes `--tarpc-bind unix://...sock`, the main socket path is now used for JSON-RPC 2.0 (health, capability, shader methods) and tarpc is redirected to a `-tarpc.sock` suffix
- JSON-RPC UDS server now starts BEFORE tarpc to claim the ecosystem-expected socket
- `jsonrpc+unix` transport advertised in discovery file alongside `jsonrpc` (TCP) and `tarpc`
- Fixes primalSpring v0.9.24 P1: 4 composition experiment failures (exp004 health/caps, exp094 shader_supported_archs, exp004 composition_all_healthy)

#### Safety Audit
- Added missing `// SAFETY:` comments: `channel_setup.rs` fence_cpu volatile write, `isolation.rs` write/batch inner blocks, `mapped_bar.rs` isolation call sites
- Standardized `// Safety:` → `// SAFETY:` on all bytemuck Pod/Zeroable impls in `uvm/structs.rs`

#### Audit Results (no action needed)
- **Large files**: 14 files >800L — all justified (8 test, 2 generated ISA, 1 example, 1 hardware harness, 2 dense data tables). All under 1000L
- **Dependencies**: No C deps in default builds. Transitive `libc` via mio tracked as EVOLUTION
- **Hardcoding**: All paths have env var overrides. Zero hardcoded primal names in production
- **Mocks**: All test-isolated. `coral-reef-stubs` is legitimate production stub crate
- **Code quality**: Zero `.unwrap()` in production library, zero TODO/FIXME/HACK, zero commented-out code
- **`#[allow(dead_code)]`**: All justified (platform-conditional, BTSP detection patterns)

#### Tests
- 4 new `resolve_uds_binds` unit tests: TCP passthrough, composition socket redirect, tarpc suffix skip, no-extension handling

### Iteration 86 — Deep Debt: Smart File Refactoring + Safety Audit (2026-04-28)

#### Smart File Refactoring (>800L → Cohesive Modules)
- tex.rs (854L → 341 + 517 tex_tests.rs): extracted SM20 texture encoder tests
- amd-isa-gen main.rs (826L → 112 + 715 main_tests.rs): extracted generator tests
- tests_unix_edge.rs (935L → 517 + 443 tests_unix_dispatch.rs): split integration vs dispatch unit tests

#### Safety Audit
- Added missing `// SAFETY:` comment on `coral_kmod.rs` `alloc_gpu_buffer` zeroed struct
- Removed unused `AsyncReadExt` import from `tests_chaos.rs`

#### Audit Results (no action needed)
- **Unsafe code**: All confined to `coral-driver` with SAFETY comments; all other crates `#![forbid(unsafe_code)]`
- **Dependencies**: No C/C++ in production (transitive `libc` via mio only; `cudarc` feature-gated)
- **Hardcoded paths**: All have env var overrides (`CORALREEF_TRAINING_DIR`, `CORALREEF_JOURNAL_PATH`, `CORALREEF_TRACE_DIR`, etc.)
- **Mocks**: All test-isolated; `coral-reef-stubs` is legitimate shim crate
- **`#[allow(dead_code)]`**: Justified platform-conditional or API-evolution annotations only
- **TODO/FIXME/HACK**: Zero in committed `.rs` code
- **Commented-out code**: None found
- **`.unwrap()` in library code**: None (all test-only)

### Iteration 85 — Wire NUCLEUS Composition Env Vars (2026-04-28)

#### NUCLEUS Composition Integration
- All 3 binaries (`coralreef`, `coral-ember`, `coral-glowplug`) now read and act on composition launcher env vars:
  - `BEARDOG_SOCKET` / `BTSP_PROVIDER_SOCKET` — preferred in BTSP security-provider discovery (before filesystem scan)
  - `DISCOVERY_SOCKET` — preferred in Songbird ecosystem registry discovery
  - `BIOMEOS_SOCKET_DIR` — explicit socket directory override (before `$XDG_RUNTIME_DIR`)
  - `CORALREEF_FAMILY_ID` — alias for `BIOMEOS_FAMILY_ID` (composition launcher sets this per-primal)
  - `FAMILY_SEED` — read and logged at startup; forwarded for future crypto purpose-key derivation
- Startup diagnostic log reports all composition env vars present/absent for operator visibility
- Per `NUCLEUS_TWO_TIER_CRYPTO_MODEL.md` v1.0 — `shader` purpose key derivation path documented; shader artifact signing deferred (low priority per primalSpring v0.9.20)

### Iteration 84 — ecoBin Cross-Arch Evolution + Deep Debt Solutions (2026-04-19)

#### Cross-Architecture Compliance (ecoBin v3)
- coral-glowplug: Linux-specific modules (capture, sec2_bridge, device, ember, health) gated behind `#[cfg(target_os = "linux")]`. PowerState extracted to portable `power_state` module. Stub main for non-Linux.
- coral-gpu: `probe_pcie_topology()` cross-platform stub (returns empty Vec on non-Linux)
- All 3 daemon crates pass `cargo check` on x86_64-apple-darwin, aarch64-apple-darwin, aarch64-unknown-linux-musl — 0 errors
- coral-driver dependency made target-specific in coral-glowplug Cargo.toml (vfio feature Linux-only)

#### Smart File Refactoring (>800L → Cohesive Modules)
- alloc.rs (996L → 536 + 473 alloc_channel.rs): extracted GPFIFO/compute/channel methods
- sovereign_init.rs (984L → 457 + 411 + 132): extracted pipeline stages and types
- uvm/mod.rs (869L → 481 + 397 constants.rs): extracted ioctl/RM/UVM constant surface
- runner.rs (815L → 589 + 269 matrix_support.rs): extracted diagnostic matrix support

#### Code Hygiene
- 62 eprintln!/println! → tracing structured logging across coralctl handlers + union_find.rs
- All production `.unwrap()` eliminated (nvdec_scrubber.rs → `.expect("4-byte slice")`)
- coral-ember socket group hardcoding → `CORALREEF_SOCKET_GROUP` env var override
- All mocks verified `#[cfg(test)]`-gated (MockSysfs, MockFirmwareSource, MockBar0, MockRegs)

#### Metrics
- 4541 passing, 0 failed, 155 ignored (hardware-gated)
- Zero clippy warnings (pedantic + nursery)
- Zero fmt drift
- Cross-compile: 0 errors on 3 non-native target triples

### Blackwell Sovereign Dispatch — ABI fixes + kmod evolution (2026-04-19)

#### coral-driver: UVM struct ABI fix
- Fixed `UvmPageableMemAccessParams`: was 4 bytes, kernel expects 8 — added `pageable_mem_access: u8` + padding before `rm_status`
- `pageable_mem_access()` now returns `DriverResult<bool>` (was `DriverResult<()>`)
- Added size assertion: `assert_eq!(size_of::<UvmPageableMemAccessParams>(), 8)`

#### coral-kmod: VRAM allocation fix
- Fixed `alloc_gpu_buffer` page size: `PAGE_SIZE_BOTH` + `PAGE_SIZE_HUGE_2MB` → `PAGE_SIZE_4KB` for data buffers
- Eliminates `FAULT_PDE` on Blackwell (page directory entries now correct for 4KB allocations)
- Removed `FIXED_ADDRESS_ALLOCATE` flag from alloc flags

#### coral-kmod: new ioctl surface
- `CORAL_IOCTL_ALLOC_GPU_BUFFER` — kernel-context VRAM allocation + GPU VA mapping
- `CORAL_IOCTL_FREE_GPU_BUFFER` — kernel-context VRAM deallocation
- Rust bindings in `coral_kmod.rs` for both ioctls

#### coral-driver: Blackwell compute path
- `NvUvmComputeDevice` gains `coral_kmod` field for kmod-based buffer allocation
- `alloc()` conditionally routes Blackwell through kmod VRAM path (BAR1 CPU mapping)
- Channel class fixed to `BLACKWELL_CHANNEL_GPFIFO_A` (0xC96F, matches CUDA R580 trace)

### Iteration 83 — Drop jsonrpsee, Pure serde_json JSON-RPC (2026-04-16)

#### jsonrpsee Removal (Ecosystem Standard Migration)
- Deleted `jsonrpc.rs` — the jsonrpsee HTTP JSON-RPC server (195 lines, `#[rpc(server)]` proc macro)
- Promoted newline-delimited TCP as the sole JSON-RPC transport (was secondary, now primary)
- Removed `jsonrpsee`, `jsonrpsee-http-client`, `jsonrpsee-core` from all Cargo.toml
- Drops transitive `async-trait`, `hyper`, `http`, `tower`, `pin-project-lite` from dep tree
- JSON-RPC dispatch is now pure `serde_json` manual match — matches songBird (`TowerAtomic`) and bearDog (`HandlerRegistry`) patterns

#### primal-rpc-client: NDJSON Transport
- Added `Transport::TcpLine` and `Transport::UnixLine` for newline-delimited JSON-RPC
- New constructors: `RpcClient::tcp_line(addr)`, `RpcClient::unix_line(path)`
- Ecosystem-standard wire framing (wateringHole v3.1)

#### Server Simplification
- `cmd_server` takes 2 args (removed `--port`, `--bind` flags — newline TCP is the primary)
- Discovery file format simplified (single `jsonrpc` transport key)
- Shutdown uses `watch::Receiver` only (no more `ServerHandle`)

#### Test Migration
- Migrated ~30 tests from jsonrpsee HTTP to newline TCP + `RpcClient::tcp_line`
- Migrated e2e tests from `jsonrpsee-http-client` to `primal-rpc-client`
- Replaced `raw_http_post` helpers with `raw_newline_rpc`
- Empty/whitespace payload tests adapted for NDJSON semantics (empty lines are no-ops)
- 4509 passing, 0 failed, 153 ignored

### Iteration 82 — Large File Refactoring, Hardcoding Dedup, Audit Cleanup (2026-04-16)

#### Smart File Refactoring (>800L → Cohesive Modules)
- Extracted `nvidia_headers.rs` tests (839→460L) into `nvidia_headers_tests.rs`
- Extracted `firmware_parser.rs` tests (806→318L) into `firmware_parser_tests.rs`
- Extracted `registers.rs` tests (822→725L) into `registers_tests.rs`
- Assessed remaining >800L files: `runner.rs` (802L, monolithic diagnostic), `sm20/tex.rs` (854L), `sm75_instr_latencies/gpr.rs` (814L) — dense hardware tables, splitting would reduce cohesion

#### Hardcoding Deduplication
- Added `ECOSYSTEM_NAMESPACE` constants to `coral-glowplug` and `coral-ember` config modules, matching `coralreef-core` pattern (no cross-crate imports per primal self-knowledge rule)

#### Deep Audit — All Clear
- `.unwrap()` in library code: zero instances (all in `#[cfg(test)]`)
- `.ok()` calls: all production uses in diagnostic/teardown/best-effort paths — justified
- `#[allow(dead_code)]` on BTSP types: justified (used in tests, Debug formatting, future evolution)
- Hardcoded primal names: only `biomeos` ecosystem namespace (self-knowledge, env-overridable)
- Mocks in production: none (all `#[cfg(test)]` gated)
- `unsafe` code: all in `coral-driver` with `// SAFETY:` comments on every block
- Transitive `libc`: documented permanent coexistence — tokio/mio (libc) alongside coral-driver/ember/glowplug (rustix/linux_raw)

### Iteration 81 — Deep Debt Resolution, Codegen Modernization, Capability-Based Discovery (2026-04-15)

#### Codegen Modernization (60 Fixes Across 30 Files)
- Removed 14 suppressed `#![allow(clippy::...)]` categories from `codegen/mod.rs` — largest lint debt in the workspace
- Fixed ~60 clippy style issues: elidable lifetimes, redundant closures/returns, `let...else`, `if let`, `.is_empty()`, method references, tail expressions, `?` operator, direct iteration
- Only 3 pedantic defers remain (`missing_const_for_fn`, `option_if_let_else`, `derive_partial_eq_without_eq`)

#### File Split (1000-Line Policy)
- `codegen_coverage_saturation.rs` (982L) → `codegen_coverage_saturation.rs` (572L, data ops sections 1–30) + `codegen_coverage_saturation_compute.rs` (441L, workgroup/kernel/edge/legacy sections 31+)

#### Production Observability
- `coralreef-core/main.rs` shutdown: replaced `.ok()` on task join handles with `tracing::warn!` on `Err` (newline JSON-RPC, tarpc, Unix JSON-RPC)
- `remove_discovery_file()`: replaced `discovery_dir().ok()` with `tracing::debug!` when directory unavailable

#### SAFETY Annotation Completeness
- Added `// SAFETY:` comments to all `unsafe {}` blocks in `config_env.rs`, `config_and_paths.rs`, `unix_jsonrpc_default_socket_path_env.rs`

#### Capability-Based Discovery (Showcase Evolution)
- `02-full-compute-triangle`: replaced `ecosystem_socket("toadstool.jsonrpc")` (primal-name-based) with `discover_provider("gpu.orchestrate")` / `"gpu.dispatch"` (capability-based directory scan)
- All showcase display text updated: primal names replaced with capability identifiers
- `01-toadstool-discovery`, `04-hardware-discovery`: display text evolved to capability language

#### Identifier Quality
- `dummy` → `placeholder` in `naga_translate/expr.rs` (SSA placeholder for uniform buffer access)

#### Docs Sync
- `CORALREEF_SPECIFICATION.md` v0.7.0, `SOVEREIGN_MULTI_GPU_EVOLUTION.md` v0.3.0 — iteration 81, April 2026
- `STATUS.md`, `WHATS_NEXT.md` synced to Iteration 81

### Iteration 80 — Wire Contract, CompilationInfo IPC, Socket Alignment, Deep Debt (2026-04-12)

#### Wire Contract Documentation (Composition Blocker)
- New `docs/SHADER_COMPILE_WIRE_CONTRACT.md`: authoritative JSON-RPC/tarpc wire contract for all `shader.compile.*` methods
- Request/response/error schemas for `shader.compile.wgsl`, `shader.compile.spirv`, `shader.compile.wgsl.multi`
- Multi-stage ML pipeline composition guidance for neuralSpring
- Capability discovery response schemas (`capability.list`, `shader.compile.capabilities`)
- tarpc transport notes, composition checklist for springs

#### CompilationInfo in IPC Responses
- `CompilationInfoResponse` struct: `gpr_count`, `instr_count`, `shared_mem_bytes`, `barrier_count`, `workgroup_size`
- `CompileResponse.info` and `DeviceCompileResult.info` carry compilation metadata over IPC
- `handle_compile_wgsl` and `handle_compile_wgsl_multi` use `compile_wgsl_full` to populate info
- Updated `IPC_COMPOSITION_AND_LATENCY.md` to reference wire contract and show info fields in sequence diagrams

#### Crypto Socket Discovery Alignment
- `coral-glowplug/src/config.rs`: centralized `resolve_socket_dir()`, `family_id()`, `ecosystem_namespace()` as `pub`
- `coral-glowplug/src/socket/btsp.rs`: delegates to centralized config (removed duplicate helpers)
- `coral-ember/src/config.rs`: new `resolve_socket_dir()` helper
- `coral-ember/src/btsp.rs`: delegates to centralized config

#### Idiomatic Rust Evolution
- `NvArch::parse()`: eliminated format!() per-comparison allocation — zero-allocation match table
- `IntelArch::Display`: consolidated to delegate to `short_name()` (single source of truth)
- `primal-rpc-client` UDS transport: `Host: localhost` → socket-name-derived host header

#### Hot-Path Allocation Elimination
- IPC newline handler: eliminated `format!("{resp}\n")` full-copy — writes bytes + newline separately
- IPC newline handler: eliminated per-line `trim().to_owned()` — borrows `&str` from owned `String`
- Compile handler: `STATUS_SUCCESS` constant avoids `"success".to_owned()` heap allocation per compile

#### Feature Gate Fix
- `coral-driver/uvm_compute`: fixed `crate::vfio::cache_ops` import without `#[cfg(feature = "vfio")]` — broke non-workspace builds. Inlined `CLFLUSH`/no-op directly in `uvm_compute`

#### Smart Refactoring
- `capture.rs` (825 LOC) → 654 + `engine_regs.rs` (165): extracted engine register capture tables into static data arrays with shared `read_regs()` helper (eliminated 7 duplicated for-loop patterns)
- `BootConfig::Display`: inlined `write!` directly instead of allocating via `label()` → `format!()` → `write!`

#### Wire Contract Test Coverage
- 5 new focused serde roundtrip tests: `HealthCheckResponse`, `LivenessResponse`, `ReadinessResponse`, `CompileCapabilitiesResponse`, `TarpcCompileError`
- 6 multi-stage ML pipeline composition tests: sequential 3-stage compile, workgroup validation, cross-vendor, occupancy planning, stage independence, serde roundtrip with `CompilationInfo`
- f64 IR docstrings: "placeholder" → "pseudo-op" (correct compiler terminology)
- `#[must_use]` on `make_response`, `parse_fma_policy`, `parse_target`, `handle_compile_spirv`, `handle_compile`, `handle_compile_wgsl`, `handle_compile_wgsl_multi`, `dispatch_jsonrpc`, `BootConfig::label`; removed redundant type annotation in `deserialize_arc_str`

#### CLI Bind Host (primalSpring benchScale gap)
- New `--bind` flag on `coralreef server`: sets host/IP for newline TCP server (e.g. `--bind 0.0.0.0` for Docker/benchScale)
- `CORALREEF_IPC_HOST` env var (primary), `CORALREEF_NEWLINE_TCP_HOST` (legacy fallback), default `127.0.0.1`
- Resolves: coralReef unreachable from outside container in benchScale deployments

#### Feature Gate Cleanup
- `coral-driver/error.rs`: `PciDiscoveryError::sysfs_io`, `DevinitError::vbios_resource_io`, `ChannelError::resource_io` constructors gated behind `#[cfg(feature = "vfio")]` — eliminates dead_code warnings on default-feature builds
- Associated tests gated behind `#[cfg(feature = "vfio")]` to match

#### MMU Oracle Test Coverage
- 11 new pure-Rust unit tests for `decode_entry_addr` (zero, flag-stripping, shift, roundtrip)
- 5 new `EntryFlags` decode tests (invalid, VRAM, SYS_COH, SYS_NCOH, volatile)
- Serde roundtrip tests for `EntryFlags`, `PageEntry`, `EngineRegisters`
- Register table invariant tests (unique names, unique offsets, non-empty)

#### Metrics
- 4506 tests passing, 0 failed, 153 ignored (hardware-gated)
- 0 clippy warnings (pedantic + nursery) — both default and all-features
- 0 doc warnings, 0 fmt issues
- 0 files >1000 LOC
- `cargo deny check`: advisories ok, bans ok, licenses ok, sources ok

### Iteration 79 — Deep Debt Cleanup: ecoBin Deny, IPC Latency, Configurable Hardcoding (2026-04-11)

#### ecoBin v3 Compliance (CR-01)
- `deny.toml` C/FFI ban list: openssl-sys, ring, aws-lc-sys, native-tls, cmake, pkg-config, bindgen, vcpkg, bzip2-sys, curl-sys, libz-sys, zstd-sys, lz4-sys, libsqlite3-sys
- `cargo deny check` passes with all bans active

#### IPC Composition & Latency
- `capability.list` metadata: `compile_latency` (p50/p99 per compile path — WGSL→SASS, WGSL→RDNA2, SPIR-V→SASS)
- `capability.list` metadata: `multi_stage_ml` (supported, sequential_compile_and_dispatch pattern, max 64 concurrent compiles)
- New doc: `docs/IPC_COMPOSITION_AND_LATENCY.md` — composition patterns for ML pipelines, latency budget tables

#### Hardcoding → Configurable
- `CORALREEF_HEARTBEAT_SECS` env var (default 45s) for ecosystem heartbeat interval
- `CORALREEF_INTEL_SETTLE_SECS` env var (default 5s) for Intel FLR settle time
- `BIOMEOS_ECOSYSTEM_NAMESPACE` consolidated in BTSP module (was hardcoded constant)
- glowplug health: hardcoded primal name → `env!("CARGO_PKG_NAME")` + `env!("CARGO_PKG_VERSION")`

#### Intel Lifecycle Evolution
- `IntelXeLifecycle` evolved from stub to configurable constructor with env-based settle time
- `device_id` stored for future Arc vs Battlemage differentiation

#### Typed Errors Wave 4 (CR-04 complete)
- `BootTrace::from_mmiotrace`: `Result<Self, String>` → `Result<Self, ChannelError>` (VFIO diagnostic domain)
- `ChannelAllocDiag.result`: `Result<u32, String>` → `Result<u32, DriverError>` (preserves ioctl context)
- Zero `Result<_, String>` remaining in coral-driver production code

#### Dead Code Removal (CR-05)
- `cpu_exec.rs` deleted — orphaned Phase 3 stub (not in module tree, missing types, missing deps)

#### libc Canary
- `libc` documented as transitive-only dependency (tokio→mio, signal-hook-registry); zero direct imports
- Ban deferred until upstream `mio`→`rustix` migration (mio#1735); STATUS.md corrected

#### Lint & Coverage
- Conditional `#[expect]` → `#[allow]` for wildcard_imports/enum_glob_use in codegen/mod.rs (fires conditionally across lib vs test targets)
- 3 new TCP IPC tests for coral-ember `handle_client_tcp` path

#### Metrics
- 4462 tests passing, 0 failed, 153 ignored (hardware-gated)
- 0 clippy warnings (pedantic + nursery)
- 0 doc warnings
- 0 files >1000 LOC

### Iteration 79c — Dead Code Cleanup, Test Recovery, #[allow] Audit (2026-04-11)

#### Dead Code Recovery
- Orphaned `uvm_compute_tests.rs` (275 lines, never compiled): 5 unique tests merged into active `uvm_compute/tests.rs` (USERD offsets, SM boundary values, GPFIFO alignment/overflow/round-trip), orphan deleted

#### Lint Audit
- All `#[allow]` in production code audited: added missing `reason=` on `gsp/knowledge` re-export
- `probe/channel.rs` reason documented (missing_docs partial coverage makes `#[expect]` fail)
- Confirmed: all remaining `#[allow]` attrs are intentionally conditional (dead_code on enum variants, unused_imports across lib/bin targets)

#### Metrics
- 4477 tests passing (+5 recovered from orphan), 0 failed, 153 ignored
- 0 clippy warnings (pedantic + nursery)
- 0 doc warnings, 0 fmt issues
- 0 files >1000 LOC

### Iteration 78 — Deep Debt Evolution: Typed Errors + Smart Refactoring (2026-04-09)

#### Typed Error Migration
- tarpc transport: `Result<_, String>` → `TarpcCompileError` (Serialize/Deserialize typed wrapper)
- Wave 1: `PciDiscoveryError` — PCI config space, power management, device info, sysfs I/O
- Wave 2: `ChannelError` — BAR0 oracle (dump/text/live), nouveau page tables, glowplug, sysfs BAR0, HBM2 training
- Wave 3: `DevinitError` — VBIOS parsing, PMU devinit, BIT tables, script interpreter, PMU timeout

#### Smart Refactoring (7 files split)
- `nv_metal.rs` (882 LOC) → `nv_metal/` (6 submodules: reg_offsets, identity, metal, probe, detect, tests)
- `memory.rs` (874 LOC) → `memory/` (4 submodules: core, regions, topology, tests)
- `vfio_compute/mod.rs` (866 LOC) → 464 + 3 new siblings (layout, raw_device, device_open)
- `falcon_capability.rs` (856 LOC) → `falcon_capability/` (4 submodules: types, probe, pio)
- `knowledge.rs` (852 LOC) → `knowledge/` (5 submodules: types, chip, gpu_knowledge, tests)
- `device/mod.rs` (835 LOC) → ~32 + 4 siblings (mapped_bar, open, runtime, handles)
- `codegen/ops/mod.rs` (831 LOC) → ~34 + 3 siblings (gfx9, amd_dispatch, encoding_helpers)

#### Lint Hardening
- `#[allow(clippy::result_large_err)]` → `#[expect]` in sysmem_prepare.rs
- `#[allow(unused_imports)]` → `#[expect]` in shader_header/mod.rs

#### BTSP Phase 2 (BearDog Delegation)
- `gate_connection()` evolved to `guard_connection()` with full BearDog delegation
- Capability-based security provider discovery (crypto-{family_id}.sock → crypto.sock → .json scan)
- `BtspOutcome` enum: DevMode, Authenticated, Degraded, Rejected
- Async implementation (coralreef-core, coral-glowplug) + blocking (coral-ember)
- Degraded mode: accept with warning when BearDog unavailable (operational resilience)

#### Metrics
- 4459 tests passing, 0 failed, 153 ignored (hardware-gated)
- 0 clippy warnings (pedantic + nursery)
- 0 doc warnings
- 0 files >1000 LOC

### Iteration 77 — primalSpring Gap Resolution + Deep Debt Evolution (2026-04-09)

#### Security
- CR-01: BIOMEOS_INSECURE guard — all 3 binaries refuse startup when FAMILY_ID + INSECURE=1

#### Wire Standard
- CR-02: `capability.list` returns Wire Standard L2 envelope (primal, version, methods, capabilities)

#### BTSP
- CR-03: BTSP Phase 2 scaffolding — BtspMode detection, gate_connection() in all accept loops

#### Code Quality
- validate_insecure_guard evolved from Result<(), String> to typed ConfigError (thiserror)
- `#[allow]` → `#[expect]` conversion in codegen/mod.rs
- Commented-out code cleaned in 13+ codegen files (match arms → architectural doc comments)
- eprintln! → tracing::info! in coral-driver diagnostic experiments (5 files)
- matches!() clippy fix in sm75_instr_latencies

#### Refactoring
- shader_header.rs (905 LOC) → shader_header/ directory (5 submodules, max 385 lines)
- personality.rs (809 LOC) → personality/ directory (2 submodules, max 469 lines)

#### Documentation
- discovery.rs and ecosystem.rs: T6 overstep audit — both legitimate (client-only + GPU targeting)
- Module docs clarified for BTSP, Wire Standard, discovery roles

### Iteration 76 — Deep Debt Smart Refactoring (2026-04-06)

#### Smart Refactoring
- `sysmem_impl.rs` (973 LOC) → 66-line orchestrator + 5 submodules (sysmem_prepare, sysmem_state, sysmem_wpr_mmu, sysmem_boot_finish)
- `sec2_hal.rs` (935 LOC) → 9-file directory (probe, emem, diagnostics, pmc, falcon_reset, boot_prepare, falcon_cpu)
- `identity.rs` (926 LOC) → 7-file directory (constants, chip_map, gpu_identity, sysfs, firmware, tests)
- `coral-ember/lib.rs` (924 LOC) → 54 lines + config.rs, runtime.rs, background.rs, lib_tests.rs
- `cfg/mod.rs` (937 LOC) → 22 lines + types.rs, ops.rs, traverse.rs, builder.rs, tests.rs
- `service/mod.rs` (828 LOC) → 146 lines + tests.rs; `config.rs` (767) → 403 + tests/

#### Mock Isolation
- `SysfsError::MockWritesMutexPoisoned` gated behind `#[cfg(test)]`

#### Idiomatic Rust
- 19 `if let Some` → `let...else` conversions (handlers_device/mod.rs, nv/mod.rs, personality.rs)

#### Unsafe Documentation
- 5 missing `// SAFETY:` comments added to coral-driver test files

#### Audit Verified
- Zero library `.unwrap()` (all test-only), zero hardcoded IPs without env override, pure Rust dep stack, zero TODO/FIXME/HACK

### Iteration 75 — primalSpring Audit Resolution (2026-04-06)

#### License Evolution
- AGPL-3.0-only → AGPL-3.0-or-later across 857 files (Cargo.toml, SPDX headers, LICENSE, docs, scripts, WGSL fixtures)

#### Workspace Lints
- Added `unsafe_code = "deny"` to `[workspace.lints.rust]`; coral-driver opts out and manages unsafe locally

#### Documentation
- Created `CONTEXT.md` at repo root (architecture overview, crate map, constraints)
- IPC `#[allow]` cleanup: updated reason strings documenting cross-target lint behavior

### Iteration 74 — Deep Debt Execution (2026-04-04)

#### Build & Tooling
- Added `.cargo/config.toml`: LTO=thin, codegen-units=1, strip=symbols (release); split-debuginfo (dev)
- coral-gpu: `[lints] workspace = true` + all 33 pedantic/nursery findings resolved

#### Code Quality
- `#[allow]` → `#[expect]` evolution (coral-ember error.rs)
- SAFETY comment added to vfio `device_pci_hot_reset`
- `DmaBufferBytes` safe abstraction wrapping raw DMA pointer+length
- Send/Sync documentation on 6 hardware types (DmaBuffer, MappedBar, Bar0Access, SysfsBar0, NvUvmComputeDevice, Bar0Handle)
- SAFETY audit: all unsafe blocks verified, 3 gaps fixed

#### Refactoring
- `pci_discovery.rs` (966 LOC) → 7 cohesive submodules: types, parse, config_space, device_info, power_mgmt, mod, tests
- `uvm_compute.rs` (969 LOC) → 5 cohesive submodules: types, device, compute_trait, mod, tests
- Removed `tests.rs.bak` debris

#### Hardcoding Evolution
- `CORALREEF_EMBER_TCP_HOST` env override (was hardcoded 127.0.0.1)
- `CORALREEF_NEWLINE_TCP_HOST` env override (was hardcoded 127.0.0.1)

#### Licensing
- Added `LICENSE-ORC` for scyBorg Provenance Trio (AGPL + ORC + CC-BY-SA)
- Updated `LICENSE` with scyBorg trio section

#### Test Coverage
- +89 tests (4318 → 4407), 0 failures, 153 ignored
- coral-driver: gsp parser, linux_paths, nv/identity, nv/qmd + 3 integration tests
- coral-glowplug: error.rs Display/conversion, sec2_bridge
- coral-ember: handlers_journal (0%→77%), error.rs (0%→50%)
- Doctests added to coral-driver, coral-ember, coral-glowplug, primal-rpc-client

### Iteration 73 — Logic/IO Untangling + Test Consolidation (2026-04-04)

#### Added
- Architectural plan for separating logic from I/O (5 entanglement patterns, 3 strategies)
- Pure modules in coral-driver: `acr_buffer_layout` (`AcrBufferLayout`, `Sec2PollState`), `sysmem_decode`, `sysmem_vram`, `init_plan` (`DynamicGrInitPlan`, `WarmRestartDecision`, `fecs_init_methods`), `channel_layout` (`ChannelLayout::compute`), `pci_config`; sec2_hal tests extracted
- Split test directories: `opt_copy_prop/tests/`, `spill_values/tests/`; `codegen_coverage_saturation` tests in 3 parts + helpers

#### Changed
- coral-glowplug: boot safety evaluation, health decisions, config classification extracted
- coral-ember: startup decomposition, reset plan, lifecycle steps
- coralreef-core: `cmd_compile` tests isolated with `tempfile::tempdir` (no fixed `/tmp` paths)

#### Metrics
- 4318 tests passing, 0 failed, 153 ignored (hardware-gated); clippy 0 warnings (pedantic + nursery); 0 files >1000 LOC; ~72,000 Rust LOC

### Iteration 72 — GPU-Agnostic Detection + Ada PCI Fix (2026-04-04)

#### Changed
- GPU-agnostic auto-detection (NVIDIA SM35–SM120, AMD GCN5–RDNA4); Ada Lovelace PCI identity range fix (e.g. RTX 4070 → SM89)
- nvidia-drm fallback uses sysfs SM detection; VFIO SM detection extended for Ada range

### Iteration 71 — MmioRegion, MockBar0, Sovereign Frontend (2026-03)

#### Added
- `MmioRegion` RAII for bounds-checked BAR0 volatile access; `MockBar0` and `NvidiaFirmwareSource` for hardware-free tests
- Workspace dependency centralization; CUDA opt-in on coral-glowplug; coverage infrastructure expansion

### Iteration 70c — Deep Evolution (2026-03-30)

#### Added
- Typed error system: `SysfsError`, `SwapError`, `TraceError` via `thiserror` in coral-ember
- `ecosystem_namespace()` runtime function with `$BIOMEOS_ECOSYSTEM_NAMESPACE` override
- 7 swap_preflight.rs unit tests, 10 observer tests, 2 identity tests
- SAFETY comments on 3 unsafe blocks (uvm_compute, ioctl IRQ helpers)

#### Changed
- `observer.rs` (934 lines) → `observer/` directory (mod.rs + nouveau.rs + vfio.rs + nvidia.rs + nvidia_open.rs + tests.rs)
- Public API: `handle_swap_device` → `Result<SwapObservation, SwapError>`, sysfs ops → `Result<(), SysfsError>`
- ~100 `println!/eprintln!` → structured `tracing` in 10 diagnostic/oracle/library files
- `uvm_compute` inline `_mm_clflush` routed through `cache_ops` module
- `NvidiaObserver` evolved from stub to full mmiotrace parser (PRIV resets, PMC enables, falcon boots, slow-bind anomaly)
- HOTSPRING_DATA_DIR deprecated with `tracing::warn!`
- HTTP `Host:` header derived from `SocketAddr` (primal-rpc-client)
- 8 `#[allow]` given `reason` strings, 7 bare `#[ignore]` given reason strings

#### Removed
- `vis_test` binary (stale build artifact committed at repo root)

### Iteration 70 — ludoSpring V35 Gap Resolution + Deep Audit (2026-03-30)

#### Added
- `capability.list` JSON-RPC method on both newline-delimited (UDS/TCP) and HTTP servers
- Unit test for `capability.list` endpoint

#### Changed
- `swap.rs` 1102→708 lines: extracted preflight checks to `swap_preflight.rs` (362 lines)
- `vfio_compute/mod.rs` 1018→855 lines: extracted `GrEngineStatus` to `gr_engine_status.rs` (173 lines)
- 0 production `.rs` files over 1000 LOC

#### Fixed
- 8 clippy errors: `branches_sharing_code` (×2, codegen ops + naga_translate expr), `redundant_clone`, `collapsible_if`, `struct_excessive_bools`, `unused_variables`, `dead_code`+`missing_docs`+`too_many_arguments` (coral-driver), `unfulfilled_lint_expectations`

### Iteration 69 — Deep Debt Resolution + wateringHole v3.1 Compliance (2026-03-29)

#### Added
- `--port` flag on `coralreef server` for wateringHole UniBin v1.1 compliance
- Raw newline-delimited TCP JSON-RPC listener (wateringHole IPC v3.1 mandatory framing)
- `coral-ember server --port` UniBin CLI with clap subcommands
- Capability-domain symlink (`shader.sock → coralreef.sock`) per CAPABILITY_BASED_DISCOVERY v1.1
- `CORALREEF_CAPABILITY_DOMAIN` env var for symlink naming
- `CORALREEF_X11_CONF_DIR`, `CORALREEF_UDEV_RULES_DIR`, `CORALREEF_JOURNAL_PATH`, `CORALREEF_GROUP_FILE` env overrides
- 30+ new tests: ecosystem discovery, newline TCP JSON-RPC, server error paths, capability symlinks
- `rust-version = "1.85"` to all showcase and tools Cargo.toml
- `#![forbid(unsafe_code)]` on all showcase and test main.rs files

#### Changed
- Refactored 10 files over 1000 LOC into cohesive directory modules (vendor_lifecycle→8, ipc→6, handlers_device→2, ACR strategies→directories, sec2_hal/device/registers split)
- Replaced all production println!/eprintln! with tracing
- Eliminated all .unwrap() from library code; .expect() with documented invariants
- Collapsed nested if statements to Rust 2024 let-chains across coral-ember and coral-glowplug
- All `#[allow(dead_code)]` now documented with `reason = "..."`

#### Fixed
- 30+ clippy errors: manual_div_ceil, identity_op, collapsible_else_if, derivable_impls, unnecessary_cast, missing_docs, doc_lazy_continuation, deprecated calls
- 457 formatting regions (cargo fmt)
- 43 broken intra-doc links (zero rustdoc warnings)
- Unreachable pattern in Hopper device ID range
- Stale re-export of deprecated `attempt_sysmem_physical_boot`

#### Removed
- `attempt_sysmem_physical_boot` (243 lines, superseded by `attempt_sysmem_acr_boot_with_config`)

### Iteration 66 — hotSpring Firmware Wiring + Coverage Push (Mar 25 2026)

- **Mailbox system (`coral-glowplug`)**: `MailboxSet` + `PostedCommand` + `Sequence` — posted-command firmware interaction for FECS/GPCCS/SEC2/PMU engines. Per-device mailboxes wired into `DeviceSlot`. JSON-RPC: `mailbox.create`, `mailbox.post`, `mailbox.poll`, `mailbox.complete`, `mailbox.drain`, `mailbox.stats`
- **Ring buffer system (`coral-glowplug`)**: `MultiRing` + `Ring` + `RingPayload` — ordered, timed, fence-based GPU command submission. Per-device rings wired into `DeviceSlot`. JSON-RPC: `ring.create`, `ring.submit`, `ring.consume`, `ring.fence`, `ring.peek`, `ring.stats`
- **Ember ring-keeper**: `RingMeta` persistence in `HeldDevice` for cross-restart ring/mailbox reconstruction. JSON-RPC: `ember.ring_meta.get`, `ember.ring_meta.set`. Systemd watchdog heartbeat (`spawn_watchdog`)
- **coralctl firmware subcommands**: `coralctl mailbox {create,post,poll,drain,stats}` + `coralctl ring {create,submit,consume,fence,peek,stats}` — CLI interface for hotSpring firmware probing
- **Coverage**: `debug.rs` (7 new tests), `op_float/f16_ops.rs` display tests (12 new — `OpHSet2`, `OpHSetP2`, `OpHMul2` dnz, `OpHAdd2` ftz, `OpHFma2` ftz+sat, `OpHMnMx2` ftz, `ImmaSize`/`HmmaSize` exhaustive), `ember hold.rs` (2 new), `mailbox_ring.rs` handler tests (10 new)
- **Metrics**: 4047 tests passing, 0 failed, ~121 ignored hardware-gated; ~66% workspace line coverage; fmt, clippy (pedantic+nursery), doc, release build — PASS

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
