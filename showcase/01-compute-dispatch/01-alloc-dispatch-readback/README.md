# 01 — Alloc → Dispatch → Readback

Full GPU compute cycle: allocate a buffer, compile a shader, dispatch
it on hardware, sync, and read back the results. Requires GPU hardware.

## What it shows

- Complete sovereign compute pipeline (no Vulkan, no wgpu)
- `GpuContext::auto()` with driver preference
- Buffer lifecycle: alloc → dispatch → sync → readback → verify
- Real GPU execution with result validation

## Run it

Requires AMD (amdgpu) or NVIDIA (nouveau/nvidia-drm) hardware:

```bash
./demo.sh
```
