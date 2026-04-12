# coralReef Experiments

Validated experiments for the sovereign GPU compiler and VFIO device management.

## Structure

Hardware experiments live in `crates/coral-driver/tests/hw_nv_vfio/` as Rust test
files. Diagnostic/probe scripts live in `scripts/`. End-to-end DRM dispatch examples
live in `crates/coral-driver/examples/`.

## Active Experiments

| ID | Name | Location | Status |
|----|------|----------|--------|
| 164 | **Sovereign Compute Dispatch** | `crates/coral-driver/examples/nvidia_nouveau_e2e.rs` | **5/5 PASS** — f32 write, f32 arith, multi-workgroup, f64 write, f64 LJ. WGSL→SM70→DRM. |
| 163 | NOP Dispatch (DRM) | `crates/coral-driver/examples/nvidia_nop_dispatch.rs` | ✅ NOP via pure Rust DRM ioctls on Titan V |
| 145 | ACR Boot v1 | `crates/coral-driver/tests/hw_nv_vfio/exp145_v1_acr_boot.rs` | PRAMIN writes, falcon upload, STARTCPU — runs safely via ember fork isolation. Cold VRAM detected gracefully. |
| 145d | Decomposed Phases | `scripts/exp145_decomposed.py` | Phase A/B/C isolated. PRAMIN probe detects cold VRAM. |
| — | Crash Probe | `scripts/crash_probe_exp145_sequence.py` | Replicates exp145 pre-crash sequence — 8 consecutive runs, zero lockups. |
| — | Crash Probe (general) | `scripts/crash_probe.py` | General GPU crash probe tool. |

## Sovereign Compute Dispatch (Exp 164, 2026-04-08)

Full WGSL → coral-reef (SM70) → nouveau DRM dispatch pipeline validated:
- Phase A: f32 write (64 threads write 42.0)
- Phase B: f32 arithmetic (6×7=42)
- Phase C: Multi-workgroup (4×64=256 threads)
- Phase D: f64 write (double precision)
- Phase E: f64 Lennard-Jones (Newton's 3rd law verified, tol=1e-8)

See `hotSpring/experiments/164_SOVEREIGN_COMPUTE_DISPATCH_PROVEN.md` for details.

## Ember Survivability Hardening (2026-04-07)

Tracked via plan file, not a numbered experiment. See `CHANGELOG.md` Iter 77
for full details. Validated: 8 consecutive exp145 fault runs — zero system lockups.

## Archived

Experiment journals and analysis for sovereign GPU work (Exp 058-163) are tracked
in hotSpring's `experiments/` directory and `experiments/archive/`.
