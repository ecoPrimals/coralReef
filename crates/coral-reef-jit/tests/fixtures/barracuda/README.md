# Vendored barraCuda WGSL Fixtures

Validated compute shader patterns extracted from coralReef's triple-path test
corpus (CoralIR interpreter + sovereign Cranelift JIT + reference expected values).

These fixtures originated from barraCuda WGSL shader patterns used for
math validation during the Interpreter/JIT gap closure work (March 2026).

## Tiers

| Tier | Pattern | Fixtures |
|------|---------|----------|
| 0 | Elementwise / Activation | relu, sigmoid, leaky_relu, elu, silu, hardsigmoid, hardtanh, abs, sqrt, sign, elementwise_add/sub/mul/fma |
| 1 | For-loop accumulator | scalar_dot_product, scalar_sum_reduce, scalar_mean, scalar_variance |
| 2 | Shared memory reduction | sum_reduce_workgroup, max_reduce_workgroup |
| 3 | Tiled shared memory | layer_norm, tiled_matmul_2x2 |

## Tolerance

All shaders validated within f32 tolerance of 1e-5 (absolute + relative)
against both the CoralIR interpreter and sovereign JIT paths.

## Attribution

These patterns are derived from barraCuda's WGSL compute shader library.
The coupling was a short-term dev-dependency for validation; barraCuda
continues to use coralReef via JSON-RPC IPC (`shader.validate`,
`shader.execute.cpu`) per ecoPrimals inter-primal architecture.
