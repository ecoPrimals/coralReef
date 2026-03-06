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

Pending — genomeBin deployment will be configured once coralDriver
is hardened for production GPU dispatch. The compiler pipeline is
fully functional via `cargo run -- compile`. coralDriver (DRM ioctl)
and coralGpu (unified API) are implemented but need hardware
validation before production deployment wrappers are built.
