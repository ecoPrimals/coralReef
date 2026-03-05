# MUFU Instruction Analysis

**Status**: Reference  
**Date**: March 5, 2026

---

## Overview

MUFU (Multi-Function Unit) is the NVIDIA hardware unit for fast
transcendental approximation.  Understanding its precision and
throughput is critical for designing f64 software lowering.

## MUFU Operations

| Op code | Function | Precision | Input range | Max error |
|---------|----------|-----------|-------------|-----------|
| 0x0     | COS      | f32       | [-π, π]     | ~1 ULP    |
| 0x1     | SIN      | f32       | [-π, π]     | ~1 ULP    |
| 0x2     | EX2      | f32       | [-126, 128] | ~1 ULP    |
| 0x3     | LG2      | f32       | (0, +∞)     | ~1 ULP    |
| 0x4     | RCP      | f32       | (0, +∞)     | ~1 ULP    |
| 0x5     | RSQ      | f32       | (0, +∞)     | ~1 ULP    |
| 0x8     | RCP64H   | f64→f32   | (0, +∞)     | ~24 bits  |
| 0x9     | RSQ64H   | f64→f32   | (0, +∞)     | ~24 bits  |
| 0xA     | SQRT     | f32       | [0, +∞)     | ~1 ULP    |
| 0xB     | TANH     | f32       | (-∞, +∞)    | ~1 ULP    |

## Architecture Availability

| Op | SM50 | SM70 | SM75 | SM80 | SM89 |
|----|------|------|------|------|------|
| COS, SIN, EX2, LG2 | Yes | Yes | Yes | Yes | Yes |
| RCP, RSQ, SQRT | Yes | Yes | Yes | Yes | Yes |
| RCP64H, RSQ64H | Yes | Yes | Yes | Yes | Yes |
| TANH | No | No | No | Yes | Yes |

## Throughput

| Op | SM70 cycles | SM80 cycles |
|----|-------------|-------------|
| MUFU (any f32) | 4 | 4 |
| MUFU.RCP64H | 4 | 4 |
| MUFU.RSQ64H | 4 | 4 |
| DFMA | 8 | 8 |

## Implications for f64 Lowering

A single f64 sqrt via Newton refinement:
- 1x MUFU.RSQ64H (4 cycles)
- 2x Newton iteration (~6 DFMA each = 12 DFMA = 96 cycles)
- 1x DMUL (8 cycles)
- **Total: ~108 cycles** vs 4 cycles for f32 MUFU.SQRT

This 27x slowdown is inherent to the hardware.  The goal is to
match libm precision, not libm speed — the alternative is no f64
transcendentals at all.

---

*Reference: NVIDIA PTX ISA 8.5, CUDA Binary Utilities*
