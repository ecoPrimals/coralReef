# f64 Transcendental Lowering — Theory

**Status**: Draft  
**Date**: March 4, 2026

---

## Problem

NVIDIA's MUFU (Multi-Function Unit) provides hardware transcendentals
(sin, cos, exp2, log2, rcp, rsq, sqrt) but **only for f32**.

When NAK encounters an f64 transcendental, it has no lowering path.
NVK silently drops these operations or emits incorrect code.

## Available f64 Hardware

All SM70+ GPUs have:

| Instruction | Precision | Throughput (SM70) |
|-------------|-----------|-------------------|
| DFMA        | f64       | 1/2 per cycle     |
| DADD        | f64       | 1/2 per cycle     |
| DMUL        | f64       | 1/2 per cycle     |
| MUFU.RCP64H | f64 seed  | 1 per cycle       |
| MUFU.RSQ64H | f64 seed  | 1 per cycle       |

MUFU.RCP64H and MUFU.RSQ64H provide ~24-bit initial approximations
of the high 32 bits.  Newton-Raphson iteration via DFMA converges
to full f64 precision.

## Lowering Strategies

### sqrt(x: f64)

```
y₀ = MUFU.RSQ64H(x)     // ~24-bit 1/√x seed
y₁ = y₀ · (3 - x·y₀²)/2  // Newton iteration 1 (via DFMA)
y₂ = y₁ · (3 - x·y₁²)/2  // Newton iteration 2
result = x · y₂            // √x = x · (1/√x)
```

### rcp(x: f64)

```
y₀ = MUFU.RCP64H(x)     // ~24-bit 1/x seed
y₁ = y₀ · (2 - x·y₀)    // Newton iteration 1 (via DFMA)
y₂ = y₁ · (2 - x·y₁)    // Newton iteration 2
```

### exp2(x: f64)

```
n = round(x)              // integer part
f = x - n                 // fractional part, |f| ≤ 0.5
// Minimax polynomial for 2^f on [-0.5, 0.5]:
p = c₀ + f·(c₁ + f·(c₂ + f·(c₃ + f·(c₄ + f·(c₅ + f·c₆)))))
result = ldexp(p, n)      // scale by 2^n
```

### log2(x: f64)

```
(m, e) = frexp(x)         // mantissa [0.5, 1.0) and exponent
y₀ = MUFU.LOG2(m as f32)  // f32 seed for log2(m)
// Newton refinement:  log2(m) ≈ y₀ + (m - 2^y₀) / (m · ln2)
result = e + refined_log2_m
```

### sin(x: f64) / cos(x: f64)

```
// Cody-Waite range reduction to [-π/4, π/4]:
n = round(x / (π/2))
r = x - n · (π/2)_hi - n · (π/2)_lo    // DFMA for precision
// Minimax polynomial (degree 7 for sin, degree 6 for cos):
result = polynomial(r)
// Apply quadrant correction based on n mod 4
```

## Polynomial Coefficients

Coefficients derived via Remez algorithm on the indicated intervals.
Full-precision constants stored as pairs of f64 for extra-precision
intermediate computation (double-double where needed).

## Validation

- Compare against libm (glibc) for all f64 transcendentals
- ULP error budget: ≤ 1 ULP for sqrt/rcp, ≤ 2 ULP for exp2/log2, ≤ 4 ULP for sin/cos
- Test with barraCuda DF64 precision benchmarks

---

*This document evolves as lowering implementations mature.*
