<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# coralReef — genomeBin

Deployment scaffolding for the coralReef primal.

## Structure

```
genomebin/
└── config/
    └── config-template.toml   deployment config template
```

`wrapper/`, `services/`, and `scripts/` are not yet present.

## Status

**Active** — compiler pipeline fully operational (`cargo run -- compile`).
Pure compiler primal — hardware dispatch delegated to toadStool (Sprint 9 excision).
Sovereign SPIR-V emission, SM120 barrier fix, full math builtin coverage.
3669 tests, zero clippy warnings, zero FFI, zero unsafe. Sprint 14 / Wave 152.
