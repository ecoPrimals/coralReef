# coralReef Showcase

Sovereign GPU compiler — from WGSL source to native GPU binary, no Vulkan, no wgpu,
no vendor SDK. Pure Rust all the way down.

## Learning Path

### Level 00 — Local Primal

What coralReef can do on its own, no services running, no hardware required.

| Demo | What it shows |
|------|---------------|
| [01-hello-compiler](00-local-primal/01-hello-compiler/) | Compile a WGSL shader to native GPU binary. Identity and capabilities. |
| [02-multi-target-compile](00-local-primal/02-multi-target-compile/) | Same WGSL → NVIDIA SM86 + AMD RDNA2. Cross-vendor compilation. |
| [03-driver-sovereignty](00-local-primal/03-driver-sovereignty/) | Sovereign vs pragmatic driver selection. Prefer nouveau, fall back to what exists. |
| [04-hardware-discovery](00-local-primal/04-hardware-discovery/) | DRM render node scan. Enumerate all GPUs on the system. |

### Level 01 — Compute Dispatch

Real GPU work. Requires hardware (AMD or NVIDIA).

| Demo | What it shows |
|------|---------------|
| [01-alloc-dispatch-readback](01-compute-dispatch/01-alloc-dispatch-readback/) | Full GPU compute cycle: alloc → upload → compile → dispatch → sync → readback. |
| [02-cross-vendor-parity](01-compute-dispatch/02-cross-vendor-parity/) | Same shader compiled and dispatched on AMD and NVIDIA. Verify identical results. |

### Level 02 — Compute Triangle

Inter-primal compute patterns. coralReef + toadStool + barraCuda working together.

| Demo | What it shows |
|------|---------------|
| [01-toadstool-discovery](02-compute-triangle/01-toadstool-discovery/) | Ecosystem capability discovery via toadStool sockets. |
| [02-full-compute-triangle](02-compute-triangle/02-full-compute-triangle/) | coralReef (compile) → toadStool (orchestrate) → barraCuda (execute). |

## Running Demos

Each demo is self-contained. Enter any demo directory and run:

```bash
./demo.sh
```

Demos at Level 00 work anywhere (compile-only, no GPU needed).
Level 01 requires GPU hardware. Level 02 requires ecosystem services.

## Architecture

```text
         coralReef                    toadStool               barraCuda
    ┌───────────────────┐        ┌──────────────┐        ┌──────────────┐
    │  WGSL → native    │──────▶│  orchestrate  │──────▶│   execute    │
    │  GPU binary       │ compile│  WHERE to run │ route  │  WHAT to     │
    │                   │        │               │        │  compute     │
    │  SM70/75/80/86/89 │        │  discover GPUs│        │  wgpu or     │
    │  RDNA2/3/4        │        │  route jobs   │        │  sovereign   │
    └───────────────────┘        └──────────────┘        └──────────────┘
```

## Driver Sovereignty

coralReef compiles for everything but prefers sovereign (open-source) drivers
at runtime. This forces deep understanding and gives full control.

```text
Default preference:  nouveau → amdgpu → nvidia-drm
Override:            CORALREEF_DRIVER_PREFERENCE=nvidia-drm,amdgpu
```

The compiled shader binary is identical regardless of which driver dispatches it.
