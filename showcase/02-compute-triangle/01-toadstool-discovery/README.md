# 01 — toadStool Discovery

Demonstrates how coralReef discovers ecosystem services and GPU hardware
via toadStool's capability-based discovery — without knowing toadStool's
name, address, or protocol ahead of time.

## What it shows

- Capability-based self-description: what coralReef provides and requires
- Ecosystem discovery via shared capability files
- GPU device descriptor creation from ecosystem metadata
- `GpuContext::from_descriptor()` to create contexts from discovered devices
- Graceful fallback to direct DRM scan when toadStool is absent

## Run it

```bash
./demo.sh

# With toadStool running, set the discovery directory:
XDG_RUNTIME_DIR=/run/user/1000 ./demo.sh
```
