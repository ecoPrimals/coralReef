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

Pending — genomeBin deployment will be configured once both AMD and
NVIDIA hardware paths are validated end-to-end. AMD E2E dispatch is
verified (RX 6950 XT). NVIDIA nouveau path is wired with full diagnostic
instrumentation, SM auto-detection, and inflight buffer safety (Iteration
29); nvidia-drm UVM RM client proof-of-concept in place. The compiler
pipeline is fully functional via `cargo run -- compile`.
