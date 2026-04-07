# coralReef Experiments

Validated experiments for the sovereign GPU compiler and VFIO device management.

## Structure

Hardware experiments live in `crates/coral-driver/tests/hw_nv_vfio/` as Rust test
files. Diagnostic/probe scripts live in `scripts/`.

## Active Experiments

| ID | Name | Location | Status |
|----|------|----------|--------|
| 145 | ACR Boot v1 | `crates/coral-driver/tests/hw_nv_vfio/exp145_v1_acr_boot.rs` | PRAMIN writes, falcon upload, STARTCPU — runs safely via ember fork isolation. Cold VRAM detected gracefully. |
| 145d | Decomposed Phases | `scripts/exp145_decomposed.py` | Phase A/B/C isolated. PRAMIN probe detects cold VRAM. |
| — | Crash Probe | `scripts/crash_probe_exp145_sequence.py` | Replicates exp145 pre-crash sequence — 8 consecutive runs, zero lockups. |
| — | Crash Probe (general) | `scripts/crash_probe.py` | General GPU crash probe tool. |

## Ember Survivability Hardening (2026-04-07)

Tracked via plan file, not a numbered experiment. See `CHANGELOG.md` Iter 77
for full details. Validated: 8 consecutive exp145 fault runs — zero system lockups.

## Archived

Experiment journals and analysis for sovereign GPU work (Exp 058-151) are tracked
in hotSpring's `experiments/` directory and `experiments/archive/`.
