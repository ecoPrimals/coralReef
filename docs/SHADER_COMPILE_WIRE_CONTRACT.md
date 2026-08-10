<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# Shader Compile Wire Contract

**Last updated**: May 20, 2026 (Sprint 12 — `CompileTarget` generalization: `execution_model` in `dispatch_hints`, `CompileTarget::Cpu`/`Npu` stubs)
**Audience**: Spring teams, barraCuda, neuralSpring, toadStool, primalSpring
**Transport**: JSON-RPC 2.0 (newline-delimited over UDS/TCP) or tarpc (bincode)

This document is the authoritative wire contract for coralReef's shader
compilation IPC endpoints. It specifies exact request/response/error shapes so
that composition layers (barraCuda compute trio, neuralSpring ML pipelines,
spring-level orchestration) can reliably wire compile → dispatch chains.

---

## Transport Framing

Per wateringHole `PRIMAL_IPC_PROTOCOL` v3.0:

- **UDS / TCP**: One JSON-RPC 2.0 object per line (`\n`-delimited).
- **HTTP** (jsonrpc-ws-server): Standard JSON-RPC POST bodies.
- **tarpc**: Binary (bincode) over TCP. Same request/response types, different
  serialization.

Socket discovery: `$XDG_RUNTIME_DIR/biomeos/coralreef-core.json` or
capability-based discovery via `capability.list`.

---

## Wire Compatibility Aliases

Canonical field names are authoritative. Legacy aliases are accepted on
deserialization for backward compatibility:

| Canonical (serialize) | Legacy alias (deserialize) | Struct |
|-----------------------|---------------------------|--------|
| `wgsl_source` | `source` | `CompileWgslRequest`, `MultiDeviceCompileRequest` |
| `binary_b64` | `binary` | `CompileResponse` |
| `shader_info` | `info` | `CompileResponse` |

---

## Methods

| Method | Input | Output | Description |
|--------|-------|--------|-------------|
| `shader.compile.wgsl` | `CompileWgslRequest` | `CompileResponse` | Compile WGSL → native GPU binary |
| `shader.compile.spirv` | `CompileRequest` | `CompileResponse` | Compile SPIR-V → native GPU binary |
| `shader.compile.wgsl.multi` | `MultiDeviceCompileRequest` | `MultiDeviceCompileResponse` | Compile one WGSL source for multiple GPU targets |
| `shader.compile.gemm` | `GemmRequest` | `CompileResponse` | Compile tensor-core GEMM kernel (SM80+ mma.sync) |
| `shader.compile.status` | *(none)* | `HealthResponse` | Compiler health/status |
| `shader.compile.capabilities` | *(none)* | `CompileCapabilitiesResponse` | Supported architectures + f64 capabilities |
| `health.check` | *(none)* | `HealthCheckResponse` | Full health probe (wateringHole standard) |
| `health.liveness` | *(none)* | `LivenessResponse` | Lightweight alive check |
| `health.readiness` | *(none)* | `ReadinessResponse` | Ready to accept work |
| `health.version` | *(none)* | `VersionResponse` | Build identity for post-upgrade verification |
| `identity.get` | *(none)* | `IdentityGetResponse` | Primal self-description for discovery |
| `capability.list` | *(none)* | `CapabilityListResponse` | Wire Standard L2 capability advertisement |

---

## `shader.compile.wgsl`

### Request

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "shader.compile.wgsl",
  "params": [{
    "wgsl_source": "@compute @workgroup_size(256) fn main(@builtin(global_invocation_id) gid: vec3<u32>) { ... }",
    "arch": "sm86",
    "opt_level": 2,
    "fp64_software": false,
    "fp64_strategy": "native",
    "fma_policy": "fused"
  }]
}
```

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `wgsl_source` | `string` | **yes** | — | Complete WGSL compute shader source |
| `arch` | `string` | no | `"sm70"` | Target GPU arch: `sm70`, `sm75`, `sm80`, `sm86`, `sm89`, `rdna2` (`gfx1030`) |
| `opt_level` | `u32` | no | `2` | Optimization level: 0 (none) to 3 (aggressive) |
| `fp64_software` | `bool` | no | `false` | Enable f64 software transcendental lowering |
| `fp64_strategy` | `string?` | no | `null` | `"software"` or `"native"` — overrides `fp64_software` if set |
| `fma_policy` | `string?` | no | `null` (= `"auto"`) | `"fused"`, `"separate"`, or `"auto"` (compiler decides) |
| `precision_advice` | `PrecisionAdvice?` | no | `null` | Precision routing hint from barraCuda (see below) |
| `adapter` | `AdapterDescriptor?` | no | `null` | GPU adapter info for arch-agnostic compilation (future) |

#### `precision_advice` Object

| Field | Type | Description |
|-------|------|-------------|
| `tier` | `string` | Precision tier: `"F16"`, `"BF16"`, `"TF32"`, `"F32"`, `"F64"`, `"DF64"`, etc. |
| `needs_transcendental_lowering` | `bool` | Whether hardware f64 transcendentals are broken |
| `df64_naga_poisoned` | `bool` | Whether DF64 path is poisoned by naga |
| `domain` | `string?` | Physics domain that motivated this compilation |

#### `adapter` Object

| Field | Type | Description |
|-------|------|-------------|
| `vendor_id` | `u32` | PCI vendor ID (e.g. `0x10DE` for NVIDIA) |
| `device_name` | `string` | GPU adapter name from driver |
| `device_type` | `string` | `"DiscreteGpu"`, `"IntegratedGpu"`, `"Cpu"` |

### Success Response

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "binary_b64": "<base64-encoded native GPU binary>",
    "size": 1024,
    "target": "sm86",
    "status": "success",
    "shader_info": {
      "gprs": 24,
      "instr_count": 142,
      "shared_memory": 0,
      "barriers": 0,
      "workgroup": [256, 1, 1],
      "wave_size": 32,
      "local_memory": 0
    },
    "dispatch_hints": {
      "hardware_hint": "compute",
      "binary_format": "ptx",
      "execution_model": "simt"
    }
  }
}
```

| Field | Type | Always present | Description |
|-------|------|----------------|-------------|
| `binary` | `bytes` (base64 in JSON, raw in tarpc) | yes | Native GPU binary (SASS for NVIDIA, ISA for AMD) |
| `size` | `usize` | yes | Binary size in bytes |
| `arch` | `string?` | yes (on success) | Architecture compiled for |
| `status` | `string?` | yes (on success) | `"success"` |
| `info` | `CompilationInfo?` | yes (WGSL path) | Compilation metadata for dispatch |
| `dispatch_hints` | `DispatchHints?` | yes | Hardware unit + binary format routing hints |

#### `dispatch_hints` Object

| Field | Type | Description |
|-------|------|-------------|
| `hardware_hint` | `string` | Hardware unit target: `"compute"`, `"tensor_core"`, `"rt_core"`, `"npu"`, `"cpu"` |
| `binary_format` | `string?` | Binary format: `"ptx"`, `"sass"`, `"isa"`, `"cranelift"`, `"dataflow_graph"` |
| `execution_model` | `string?` | Execution model: `"simt"` (GPU), `"sequential"` (CPU), `"dataflow"` (NPU) |

#### `info` Object (CompilationInfo)

| Field | Type | Description |
|-------|------|-------------|
| `gpr_count` | `u32` | General-purpose registers used (for QMD/PM4 construction) |
| `instr_count` | `u32` | Instructions emitted |
| `shared_mem_bytes` | `u32` | Shared memory from `var<workgroup>` (bytes) |
| `barrier_count` | `u32` | Barriers used |
| `workgroup_size` | `[u32; 3]` | `[x, y, z]` from `@workgroup_size` |

The `info` field enables dispatch layers (toadStool, coralDriver, barraCuda) to
construct GPU dispatch descriptors (NVIDIA QMD, AMD PM4) without re-parsing the
compiled binary. This is the field primalSpring's composition layer needs to
wire "compile → dispatch" chains.

---

## `shader.compile.spirv`

### Request

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "shader.compile.spirv",
  "params": [{
    "spirv_words": [119734787, 65536, 524295, ...],
    "arch": "sm70",
    "opt_level": 2,
    "fp64_software": false
  }]
}
```

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `spirv_words` | `[u32]` | **yes** | — | SPIR-V module as array of u32 words |
| `arch` | `string` | no | `"sm70"` | Target GPU architecture |
| `opt_level` | `u32` | no | `2` | Optimization level (0-3) |
| `fp64_software` | `bool` | no | `false` | Enable f64 software transcendentals |

### Success Response

Same as `shader.compile.wgsl`. The `info` field is `null` for the SPIR-V path
(the SPIR-V pipeline does not yet return full `CompilationInfo`; use the WGSL
path for dispatch metadata).

---

## `shader.compile.wgsl.multi`

Compile the **same** WGSL source for **multiple GPU architectures** in a single
request. This is for multi-GPU systems — not for compiling different shaders.

### Request

```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "shader.compile.wgsl.multi",
  "params": [{
    "wgsl_source": "@compute @workgroup_size(64) fn main() { ... }",
    "targets": [
      { "card_index": 0, "arch": "sm70" },
      { "card_index": 1, "arch": "sm86" },
      { "card_index": 2, "arch": "rdna2", "pcie_group": 1 }
    ],
    "opt_level": 2,
    "fp64_software": false,
    "fp64_strategy": "native",
    "fma_policy": "auto"
  }]
}
```

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `wgsl_source` | `string` | **yes** | — | WGSL source (shared across all targets) |
| `targets` | `[DeviceTarget]` | **yes** | — | At least one target device |
| `targets[].card_index` | `u32` | no | `0` | Card slot index (0-based) |
| `targets[].arch` | `string` | **yes** | — | GPU architecture |
| `targets[].pcie_group` | `u32?` | no | `null` | PCIe switch affinity hint |
| `opt_level` | `u32` | no | `2` | Optimization level (0-3) |
| `fp64_software` | `bool` | no | `false` | Enable f64 software transcendentals |
| `fp64_strategy` | `string?` | no | `null` | `"software"` or `"native"` |
| `fma_policy` | `string?` | no | `null` | `"fused"`, `"separate"`, or `"auto"` |

### Success Response

```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "result": {
    "results": [
      {
        "card_index": 0,
        "arch": "sm70",
        "binary_b64": "<base64>",
        "size": 1024,
        "error": null,
        "shader_info": { "gprs": 28, "instr_count": 160, "shared_memory": 0, "barriers": 0, "workgroup": [64, 1, 1], "wave_size": 32, "local_memory": 0 }
      },
      {
        "card_index": 1,
        "arch": "sm86",
        "binary_b64": "<base64>",
        "size": 960,
        "error": null,
        "shader_info": { "gprs": 24, "instr_count": 142, "shared_memory": 0, "barriers": 0, "workgroup": [64, 1, 1], "wave_size": 32, "local_memory": 0 }
      },
      {
        "card_index": 2,
        "arch": "rdna2",
        "binary_b64": "<base64>",
        "size": 512,
        "error": null,
        "shader_info": { "gprs": 32, "instr_count": 80, "shared_memory": 0, "barriers": 0, "workgroup": [64, 1, 1], "wave_size": 64, "local_memory": 0 }
      }
    ],
    "success_count": 3,
    "total_count": 3
  }
}
```

Per-target failures are reported inline (`binary_b64: null`, `error: "message"`),
not as top-level JSON-RPC errors. A request-level error (empty source, no
targets) returns a JSON-RPC error.

---

## Error Shapes

### JSON-RPC Error Codes

| Code | Constant | Triggered by |
|------|----------|--------------|
| `-32001` | `INVALID_INPUT` | Empty source, malformed SPIR-V, bad params |
| `-32002` | `NOT_IMPLEMENTED` | Feature not yet supported (e.g. Intel target) |
| `-32003` | `UNSUPPORTED_ARCH` | Unrecognized architecture string |
| `-32000` | `INTERNAL_COMPILE` | Validation, register allocation, encoding, or ICE |

### Error Response Example

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32003,
    "message": "unsupported architecture: sm_10"
  }
}
```

### CompileError Variants (Rust → wire mapping)

| Rust variant | JSON-RPC code | When |
|--------------|---------------|------|
| `InvalidInput` | `-32001` | Empty WGSL/SPIR-V, bad alignment, malformed source |
| `NotImplemented` | `-32002` | Unsupported WGSL feature, missing lowering pass |
| `UnsupportedArch` | `-32003` | Architecture string not recognized by any vendor backend |
| `Validation` | `-32000` | IR validation failure (type mismatch, etc.) |
| `RegisterAllocation` | `-32000` | Register pressure exceeded, spill failed |
| `Encoding` | `-32000` | Target-specific instruction encoding error |
| `Internal` | `-32000` | Internal compiler error (ICE) — bug in coralReef |

---

## Multi-Stage ML Pipeline Composition

coralReef compiles **one shader per request**. Multi-stage pipelines
(tokenizer → attention → FFN) are composed by the caller as sequential or
parallel compile calls, then dispatched through toadStool / coralDriver.

See [IPC Composition & Latency Guide](IPC_COMPOSITION_AND_LATENCY.md) for the
full pattern, latency budget, and sequence diagram.

For compiling the **same shader** for **multiple GPU architectures**:
use `shader.compile.wgsl.multi`.

For compiling **different shaders** for the **same architecture**:
issue parallel `shader.compile.wgsl` calls.

---

## Capability Discovery

### `capability.list` Response

```json
{
  "jsonrpc": "2.0",
  "id": 10,
  "result": {
    "primal": "coralreef-core",
    "version": "0.1.0",
    "methods": [
      "shader.compile.wgsl",
      "shader.compile.spirv",
      "shader.compile.wgsl.multi",
      "shader.compile.status",
      "shader.compile.capabilities",
      "health.check",
      "health.liveness",
      "health.readiness",
      "identity.get",
      "capability.list"
    ],
    "capabilities": [
      "shader.compile",
      "health",
      "identity"
    ]
  }
}
```

### `shader.compile.capabilities` Response

```json
{
  "jsonrpc": "2.0",
  "id": 11,
  "result": {
    "supported_archs": ["sm_70", "sm_75", "sm_80", "sm_86", "sm_89", "rdna2"],
    "f64_transcendentals": {
      "sin": true,
      "cos": true,
      "sqrt": true,
      "exp2": true,
      "log2": true,
      "rcp": true,
      "exp": true,
      "log": true,
      "composite_lowering": true
    }
  }
}
```

---

## `shader.compile.gemm`

Compiles a tensor-core GEMM kernel using `mma.sync.aligned` HMMA instructions.

### Request (`GemmCompileRequest`)

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `m` | `u32` | *(required)* | Matrix rows (M dimension) |
| `n` | `u32` | *(required)* | Matrix columns (N dimension) |
| `k` | `u32` | *(required)* | Inner/reduction dimension (K dimension) |
| `precision` | `string` | `"f16f32"` | `"f16"`, `"f16f32"`, or `"tf32"` |
| `arch` | `string` | `"sm_80"` | Target GPU architecture (`sm_80`+) |
| `tiling` | `string` | `"auto"` | `"auto"`, `"global"`, or `"smem"` |

### Tiling Strategies

| Value | Phase | Threads/CTA | Shared Memory | Requirements |
|-------|-------|-------------|---------------|--------------|
| `"global"` | Phase 1 | 32 (1 warp) | None | M%16==0, N%8==0 |
| `"smem"` | Phase 2 | 128 (4 warps) | ~2.5 KB | M%64==0, N%16==0 |
| `"auto"` | Auto-select | Depends | Depends | Selects smem when M%64==0 and N%16==0, else global |

Phase 2 (`smem`) uses `ldmatrix.sync.aligned` for warp-cooperative fragment loads
and `bar.sync` for shared-memory pipeline synchronization. Block tile: BM=64, BN=16.

### Tensor Layout Constraints

| Constraint | Requirement |
|------------|-------------|
| **Matrix A** | Row-major (M x K) |
| **Matrix B** | Column-major (K x N) |
| **Matrix C** | Row-major (M x N), output-only (accumulators zeroed) |
| **K alignment** | Multiple of 16 (F16/F16F32) or 8 (TF32) |
| **Pointer ABI** | `.param .u64` — three bare pointers (A, B, C) |
| **Minimum SM** | SM80 (Ampere). Rejects SM70 and below |
| **Tile shape** | 16x8x16 (F16/F16F32) or 16x8x8 (TF32) — warp-level MMA |
| **M/N validation** | Enforced: M%16, N%8 minimum; smem requires M%64, N%16 |

### Precision Modes

| Value | Inputs | Accumulator | PTX instruction |
|-------|--------|-------------|-----------------|
| `f16` | f16 | f16 | `mma.sync.aligned.m16n8k16.row.col.f16.f16.f16.f16` |
| `f16f32` | f16 | f32 | `mma.sync.aligned.m16n8k16.row.col.f32.f16.f16.f32` |
| `tf32` | f32 via TF32 | f32 | `mma.sync.aligned.m16n8k8.row.col.f32.tf32.tf32.f32` |

### Example

```json
{
  "jsonrpc": "2.0",
  "method": "shader.compile.gemm",
  "params": {
    "m": 256,
    "n": 128,
    "k": 64,
    "precision": "f16f32",
    "arch": "sm_80",
    "tiling": "smem"
  },
  "id": 20
}
```

Response is a standard `CompileResponse` with PTX binary in `binary_b64`.

---

## `health.version`

Returns build identity for post-upgrade verification without parsing CLI output.

### Response (`VersionResponse`)

| Field | Type | Description |
|-------|------|-------------|
| `session` | `string` | Build session label (from `CORALREEF_SESSION` env at compile time, or version) |
| `build_hash` | `string` | Git commit hash (from `CORALREEF_BUILD_HASH` env at compile time, or `"dev"`) |
| `version` | `string` | Semantic version from Cargo.toml |
| `name` | `string` | Primal name (self-knowledge) |

### Example

```json
{
  "jsonrpc": "2.0",
  "method": "health.version",
  "id": 21,
  "result": {
    "session": "0.1.0",
    "build_hash": "5ae0328",
    "version": "0.1.0",
    "name": "coralreef-core"
  }
}
```

---

## tarpc Transport Notes

The tarpc service exposes the same operations with identical type semantics:

| tarpc method | Request type | Response type |
|--------------|-------------|---------------|
| `spirv` | `CompileSpirvRequestTarpc` | `Result<CompileResponse, TarpcCompileError>` |
| `wgsl` | `CompileWgslRequest` | `Result<CompileResponse, TarpcCompileError>` |
| `multi` | `MultiDeviceCompileRequest` | `Result<MultiDeviceCompileResponse, TarpcCompileError>` |
| `status` | *(none)* | `HealthResponse` |
| `capabilities` | *(none)* | `CompileCapabilitiesResponse` |

`CompileSpirvRequestTarpc` uses `bytes::Bytes` for zero-copy SPIR-V transfer
over bincode (vs. `Vec<u32>` in the JSON-RPC path). `TarpcCompileError`
wraps the error message as a serializable string.

---

## Composition Checklist for Springs

1. **Discover** coralReef via `capability.list` or filesystem discovery.
   Do not hardcode socket paths or primal names.

2. **Query capabilities** (`shader.compile.capabilities`) to know which
   architectures and f64 ops are available before compiling.

3. **Compile** via `shader.compile.wgsl` — the response includes `info`
   with GPR count, shared memory, barriers, and workgroup size.

4. **Pass binary + info to dispatch**: The dispatch layer (toadStool,
   coralDriver) needs both the `binary` and the `info` fields to construct
   the GPU dispatch descriptor (QMD for NVIDIA, PM4 for AMD).

5. **Handle errors** by checking the JSON-RPC error code and message.
   `-32003` (unsupported arch) is recoverable by falling back to a
   different architecture.

6. **Cache compiled binaries**: Source hash + arch + options → binary.
   coralReef does not cache across IPC calls.
