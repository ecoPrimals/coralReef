# 03 — Driver Sovereignty

Demonstrates coralReef's driver preference system: prefer sovereign
(open-source) drivers for deep control, fall back to whatever exists
on the deployment target.

## What it shows

- Default preference: nouveau (sovereign) → amdgpu → nvidia-drm (compatible)
- Environment variable override: `CORALREEF_DRIVER_PREFERENCE`
- The compiled binary is identical regardless of which driver dispatches it
- Selection logic: preference order matched against available DRM nodes

## Run it

```bash
./demo.sh

# Override preference:
CORALREEF_DRIVER_PREFERENCE=nvidia-drm,amdgpu ./demo.sh
```
