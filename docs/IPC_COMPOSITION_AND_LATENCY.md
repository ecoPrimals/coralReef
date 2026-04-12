<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# IPC Composition & Compile Latency Guide

**Last updated**: April 12, 2026 (Iteration 80)
**Audience**: Spring teams composing with coralReef (`shader.compile.*`)
**Wire contract**: See [SHADER_COMPILE_WIRE_CONTRACT.md](SHADER_COMPILE_WIRE_CONTRACT.md)
for exact request/response/error JSON shapes.

---

## Compile Latency Budget

Measured on typical compute shaders (64-thread workgroup, f32 ALU + mixed ops)
using `cargo bench --bench compile_bench`. Hardware: AMD Ryzen / Intel Core class
CPU. These are *compile* times — GPU dispatch latency is separate and depends on
the `gpu.dispatch` provider (toadStool, coralDriver, etc.).

| Path | p50 | p99 | Notes |
|------|-----|-----|-------|
| WGSL → NVIDIA SASS (SM70) | ~10 ms | ~25 ms | Full pipeline: naga parse → IR → f64 lower → optimize → legalize → RA → encode |
| WGSL → AMD RDNA2 (GFX1030) | ~0.09 ms | ~0.5 ms | Shorter pipeline: no SASS scheduling pass |
| SPIR-V → NVIDIA SASS (SM70) | ~19 ms | ~35 ms | Skips WGSL parse but adds SPIR-V → naga front-end |

### Scaling guidance

- **Shader complexity**: f64 transcendental lowering (Newton-Raphson) adds ~2-5 ms
  per op on NVIDIA. AMD f64 uses native hardware and adds negligible overhead.
- **Multi-target** (`shader.compile.wgsl.multi`): Targets compile sequentially.
  Budget = `N × single_compile_latency`. Up to 64 targets per request.
- **Caching**: The `coral-gpu` in-process API caches compiled kernels by source
  hash + options. IPC callers should cache binaries on their side.

### Composition budget examples

| Spring | Use case | Budget |
|--------|----------|--------|
| barraCuda | Single WGSL dispatch (f32 BLAS) | ~10 ms compile + dispatch |
| neuralSpring | 3-stage ML pipeline (tokenize + attn + FFN) | ~30 ms compile (3×10 ms), cacheable |
| hotSpring | f64 Metropolis kernel | ~15 ms compile (f64 lowering) |
| ludoSpring | Game compute shader | ~10 ms compile, cache after first |

---

## Multi-Stage ML Pipeline Composition

**Question from neuralSpring**: Does `shader.compile.wgsl` support multi-stage ML
pipelines (tokenization → attention → FFN as sequential WGSL dispatches)?

### Answer: Yes, by composition

coralReef compiles **one shader per request**. Multi-stage pipelines are composed
by the caller (neuralSpring or orchestrator) as sequential compile + dispatch
operations. This is intentional — coralReef is a compiler, not a runtime scheduler.

### Pattern: Sequential Compile & Dispatch

```
neuralSpring                      coralReef                   gpu.dispatch provider
     │                                │                              │
     ├─ shader.compile.wgsl ─────────►│ (tokenizer.wgsl)             │
     │◄──── {binary_a, info_a} ───────┤                              │
     │                                │                              │
     ├─ shader.compile.wgsl ─────────►│ (attention.wgsl)             │
     │◄──── {binary_b, info_b} ───────┤                              │
     │                                │                              │
     ├─ shader.compile.wgsl ─────────►│ (ffn.wgsl)                   │
     │◄──── {binary_c, info_c} ───────┤                              │
     │                                │                              │
     ├─ gpu.dispatch(binary_a,        │                              │
     │    info_a, bufs) ──────────────┼─────────────────────────────►│
     │◄──── done ─────────────────────┼──────────────────────────────┤
     ├─ gpu.dispatch(binary_b,        │                              │
     │    info_b, bufs) ──────────────┼─────────────────────────────►│
     │◄──── done ─────────────────────┼──────────────────────────────┤
     ├─ gpu.dispatch(binary_c,        │                              │
     │    info_c, bufs) ──────────────┼─────────────────────────────►│
     │◄──── done ─────────────────────┼──────────────────────────────┤
```

### Key points

1. **Compile once, dispatch many**: Cache compiled binaries. The 3-stage compile
   cost (~30 ms for NVIDIA) is paid once; subsequent dispatches reuse binaries.

2. **Parallel compilation**: The three `shader.compile.wgsl` calls are independent
   and can be issued concurrently (separate JSON-RPC requests or tarpc calls).

3. **Memory layout is caller responsibility**: Buffer bindings between stages
   (tokenizer output → attention input) are managed by the dispatch provider,
   not by coralReef. Use consistent `@group(0) @binding(N)` conventions.

4. **`shader.compile.wgsl.multi`** compiles the **same** WGSL for **multiple GPU
   architectures** (e.g. SM70 + RDNA2). It does **not** compile multiple distinct
   WGSL sources. For multi-stage, call `shader.compile.wgsl` per stage.

5. **Inter-stage synchronization**: Dispatch ordering and memory barriers are
   handled by the `gpu.dispatch` provider. coralReef has no opinion on execution
   order — it only produces native binaries.

### Future evolution

If demand warrants it, a `shader.compile.wgsl.batch` method could accept an array
of distinct WGSL sources in a single request, reducing IPC round-trips for
multi-stage pipelines. This is not currently implemented but would be a
straightforward extension of the existing multi-device pattern.

Cross-stage optimizations (kernel fusion, shared register allocation) are a
research-level compiler feature and are not on the near-term roadmap.

---

## Discovery

These latency and composition capabilities are advertised programmatically via
`capability.list` → `shader.compile` metadata:

```json
{
  "compile_latency": {
    "unit": "ms",
    "wgsl_to_nvidia_sass": { "p50": 10, "p99": 25 },
    "wgsl_to_amd_rdna2": { "p50": 0.1, "p99": 0.5 },
    "spirv_to_nvidia_sass": { "p50": 19, "p99": 35 }
  },
  "multi_stage_ml": {
    "supported": true,
    "pattern": "sequential_compile_and_dispatch",
    "max_concurrent_compiles": 64
  }
}
```

Springs can query this at runtime to plan their composition budgets without
hardcoding assumptions about coralReef's performance characteristics.
