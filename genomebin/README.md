<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# coralReef — genomeBin

Deployment scaffolding for the coralReef primal.

## Structure

```
genomebin/
└── config/     config templates, environment configs
```

Planned:

- `wrapper/` — genome-wrapper.sh, system detection
- `services/` — systemd, launchd, rc.d templates
- `scripts/` — create, test, sign scripts

## Status

**Active** — compiler pipeline fully operational (`cargo run -- compile`).
AMD E2E dispatch verified (RX 6950 XT). NVIDIA VFIO dispatch
pipeline functionally complete (BAR0 + DMA + GPFIFO + PFIFO + V2
MMU). UVM dispatch pipeline code-complete (GPFIFO + USERD doorbell).
Both NVIDIA paths await on-site hardware validation (RTX 5060).
4790 tests, zero clippy warnings, zero FFI.
