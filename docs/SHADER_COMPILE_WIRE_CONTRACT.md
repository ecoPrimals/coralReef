<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# Shader Compile Wire Contract

**Last updated**: April 12, 2026 (Iteration 80)
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

## Methods

| Method | Input | Output | Description |
|--------|-------|--------|-------------|
| `shader.compile.wgsl` | `CompileWgslRequest` | `CompileResponse` | Compile WGSL → native GPU binary |
| `shader.compile.spirv` | `CompileRequest` | `CompileResponse` | Compile SPIR-V → native GPU binary |
| `shader.compile.wgsl.multi` | `MultiDeviceCompileRequest` | `MultiDeviceCompileResponse` | Compile one WGSL source for multiple GPU targets |
| `shader.compile.status` | *(none)* | `HealthResponse` | Compiler health/status |
| `shader.compile.capabilities` | *(none)* | `CompileCapabilitiesResponse` | Supported architectures + f64 capabilities |
| `health.check` | *(none)* | `HealthCheckResponse` | Full health probe (wateringHole standard) |
| `health.liveness` | *(none)* | `LivenessResponse` | Lightweight alive check |
| `health.readiness` | *(none)* | `ReadinessResponse` | Ready to accept work |
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

### Success Response

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "binary": "<base64-encoded native GPU binary>",
    "size": 1024,
    "arch": "sm86",
    "status": "success",
    "info": {
      "gpr_count": 24,
      "instr_count": 142,
      "shared_mem_bytes": 0,
      "barrier_count": 0,
      "workgroup_size": [256, 1, 1]
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
        "binary": "<base64>",
        "size": 1024,
        "error": null,
        "info": { "gpr_count": 28, "instr_count": 160, "shared_mem_bytes": 0, "barrier_count": 0, "workgroup_size": [64, 1, 1] }
      },
      {
        "card_index": 1,
        "arch": "sm86",
        "binary": "<base64>",
        "size": 960,
        "error": null,
        "info": { "gpr_count": 24, "instr_count": 142, "shared_mem_bytes": 0, "barrier_count": 0, "workgroup_size": [64, 1, 1] }
      },
      {
        "card_index": 2,
        "arch": "rdna2",
        "binary": "<base64>",
        "size": 512,
        "error": null,
        "info": { "gpr_count": 32, "instr_count": 80, "shared_mem_bytes": 0, "barrier_count": 0, "workgroup_size": [64, 1, 1] }
      }
    ],
    "success_count": 3,
    "total_count": 3
  }
}
```

Per-target failures are reported inline (`binary: null`, `error: "message"`),
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
