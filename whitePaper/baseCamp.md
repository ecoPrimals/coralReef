# coralReef — baseCamp

Entry point for coralReef's technical white papers and research documents.

**Updated:** 2026-04-07 — Ember Survivability Hardening complete, FdVault + warm cycle resurrection wired

## Papers

| Paper | Topic |
|-------|-------|
| [Sovereign Compiler Architecture](SOVEREIGN_COMPILER_ARCHITECTURE.md) | Vendor-agnostic Rust GPU compilation pipeline (NVIDIA + AMD) |
| [f64 Transcendental Lowering — Theory](F64_LOWERING_THEORY.md) | Software lowering of f64 transcendentals via Newton-Raphson and MUFU seeds |
| [MUFU Instruction Analysis](MUFU_ANALYSIS.md) | NVIDIA MUFU hardware unit: precision, throughput, and architecture availability |

## Ember / GlowPlug Fault Containment Architecture

The sovereign GPU stack includes a sacrificial daemon architecture:

- **coral-ember** — Disposable VFIO fd holder. All MMIO operations fork-isolated. On total fault, `abort()`s for instant death (no cleanup stalls). Zero I/O in any recovery path.
- **coral-glowplug** — Immortal PCIe lifecycle broker. Monitors ember heartbeat, resurrects on death with GPU warm cycle (nouveau bind/unbind), maintains FdVault backup of VFIO fds.
- **Key RPCs**: `ember.warm_cycle` (live GPU warm cycle without restart), `ember.mmio.read/write/batch` (fork-isolated register access), `ember.pramin.read/write` (fork-isolated VRAM access)

Validated: 8 consecutive GPU fault runs with zero system lockups.

## Reading Order

1. Start with the architecture overview
2. Then the f64 lowering theory
3. Then MUFU analysis for hardware transcendental units
