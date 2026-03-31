# Dispatch Boundary — coral-glowplug / coral-ember / toadStool

## Status: Transitional (deferred until GPU development stabilizes)

Per `wateringHole/PRIMAL_RESPONSIBILITY_MATRIX.md` V2, the compile-vs-dispatch
boundary between coralReef and toadStool is **acknowledged and deferred**.

## Current Architecture

```
WGSL Source
    │
    ▼
coralreef-core ─── shader.compile.* / shader.execute.cpu / shader.validate
    │
    ├──▶ coral-glowplug ─── device.dispatch (CUDA PTX, transitional)
    │        │                device.swap / device.health / device.lend
    │        │                mailbox.* / ring.* (firmware IPC)
    │        ▼
    │    coral-driver ─── CudaComputeDevice / NvVfioComputeDevice / GpuChannel
    │        ▲
    │        │
    └──▶ coral-ember ─── ember.vfio_fds / ember.swap / ember.diagnostics
                          Immortal VFIO fd holder, driver swap orchestration
```

## Eventual Architecture

```
coralReef compiles  ──▶  toadStool dispatches compiled shaders to hardware
```

When `toadStool` absorbs GPU dispatch responsibility:

- `device.dispatch` moves from `coral-glowplug` to `toadStool`
- `coral-glowplug` retains device lifecycle:
  - swap, health, lend/reclaim, diagnostics
  - register access (BAR0, PRAMIN)
  - firmware mailbox/ring IPC
- `coral-ember` continues as the immortal VFIO fd holder

## Why Transitional

`device.dispatch` exists in glowplug because sovereign GPU development
requires a direct dispatch path while hotSpring stabilizes the VFIO compute
pipeline. The dispatch implementation uses `coral-driver::cuda::CudaComputeDevice`
(CUDA path) and will eventually use `coral-driver::vfio::NvVfioComputeDevice`
(sovereign VFIO path).

The boundary is deferred because:

1. The sovereign VFIO compute path (`NvVfioComputeDevice` + GPFIFO/USERD)
   is still under active development
2. Warm handoff semantics (`PfifoInitConfig::warm_handoff()`) need validation
   across GPU generations
3. toadStool's S169 cleanup removed its shader compilation proxies;
   dispatch absorption is the next phase

## Capability Advertisement

`device.dispatch` is included in `capabilities.list` with a `"transitional"`
annotation. Consumers should:

1. Discover via `device.sock` domain symlink
2. Check `capabilities.list` for `device.dispatch`
3. Be prepared for this method to move to `toadStool` in the future

## Post-S169 Spring Routing

After toadStool S169 removed `shader.compile.*` proxy methods, springs must
call coralReef directly for shader compilation:

- Discover via `shader.sock` domain symlink → `coralreef-core`
- Discover via `device.sock` domain symlink → `coral-glowplug`

Both symlinks live in `$XDG_RUNTIME_DIR/biomeos/`.
