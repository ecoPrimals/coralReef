# 02 — Full Compute Triangle

The complete compute flow: coralReef compiles, toadStool orchestrates,
barraCuda executes. This demo runs all three stages, with graceful
degradation when ecosystem services are absent.

## What it shows

- **coralReef** (this primal): WGSL → native GPU binary
- **toadStool** (orchestrator): discovers GPU capabilities, routes compute jobs
- **barraCuda** (executor): dispatches compiled shaders on GPU hardware
- The full triangle working end-to-end via JSON-RPC over Unix sockets
- Graceful degradation: each layer works independently when others are absent

## The Compute Triangle

```text
    WGSL source
         │
         ▼
  ┌──────────────┐
  │  coralReef   │  shader.compile.wgsl
  │  (compile)   │  WGSL → SM86 SASS / RDNA2 ISA
  └──────┬───────┘
         │ compiled binary
         ▼
  ┌──────────────┐
  │  toadStool   │  science.gpu.dispatch
  │ (orchestrate)│  route to best available GPU
  └──────┬───────┘
         │ dispatch request
         ▼
  ┌──────────────┐
  │  barraCuda   │  compute.submit
  │  (execute)   │  run on hardware, return results
  └──────────────┘
```

## Run it

```bash
# Standalone (compile-only, no services needed):
./demo.sh

# With toadStool and barraCuda running:
XDG_RUNTIME_DIR=/run/user/1000 ./demo.sh
```
