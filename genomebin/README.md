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

Pending — genomeBin deployment will be configured once NVIDIA VFIO and
UVM hardware paths are validated end-to-end. AMD E2E dispatch is
verified (RX 6950 XT). NVIDIA VFIO dispatch pipeline is functionally
complete (BAR0 + DMA + GPFIFO + sync with GP_GET polling). UVM dispatch
pipeline is code-complete (GPFIFO + USERD doorbell + completion polling).
Both await on-site hardware validation. The compiler pipeline is fully
functional via `cargo run -- compile`.
