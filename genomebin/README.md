# coralNak — genomeBin

Deployment scaffolding for the coralNak primal.

## Structure

```
genomebin/
└── config/     config templates, environment configs
```

Planned (Phase 6 — coralDriver):

- `wrapper/` — genome-wrapper.sh, system detection
- `services/` — systemd, launchd, rc.d templates
- `scripts/` — create, test, sign scripts

## Status

Pending — genomeBin deployment will be configured once coralDriver
(Phase 6) provides the runtime execution target for compiled binaries.
The compiler pipeline is fully functional via `cargo run -- compile`.
