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
Pure compiler primal — hardware dispatch delegated to toadStool (Sprint 9 excision).
Sovereign SPIR-V emission (Wave 68), SM120 barrier fix, full math builtin coverage.
3631 tests, zero clippy warnings, zero FFI, zero unsafe.
