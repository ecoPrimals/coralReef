# 04 — Hardware Discovery

Scan the local system for GPU hardware using DRM render nodes and
ecosystem capability discovery. No vendor SDK or proprietary tooling.

## What it shows

- DRM render node enumeration (`/dev/dri/renderD*`)
- Driver identification (amdgpu, nouveau, nvidia-drm)
- Ecosystem discovery via toadStool capability files (when available)
- Fallback chain: toadStool discovery → direct DRM scan

## Run it

```bash
./demo.sh
```
