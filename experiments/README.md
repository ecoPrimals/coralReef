# coralReef Experiments

Validated experiments for the sovereign GPU compiler.

## Location

Active experiments live within their respective crate source trees:

- **VFIO channel diagnostic experiments**: `crates/coral-driver/src/vfio/channel/diagnostic/experiments/`
  - `direct_pbdma` — Direct PBDMA command submission
  - `dispatch` — GPU compute dispatch
  - `sched_doorbell` — Scheduler doorbell interaction
  - `scheduler` — PFIFO scheduler probing
  - `runlist_ack` — Runlist bind/enable acknowledgment
  - `vram` — VRAM allocation and mapping
  - `context` — Experiment context (shared DMA state)
  - `reinit` — Engine re-initialization

## Structure

Each experiment module contains:
- Hypothesis and methodology in doc comments
- Self-contained functions callable from the diagnostic matrix
- Register snapshot capture for post-mortem analysis
