# Dispatch Boundary — coral-glowplug / coral-ember / toadStool

## Status: Sovereign path implemented; CUDA path transitional

Per `wateringHole/PRIMAL_RESPONSIBILITY_MATRIX.md` V2, the compile-vs-dispatch
boundary between coralReef and toadStool is **acknowledged and deferred**.

## Current Architecture

```
WGSL Source
    │
    ├──▶ device.dispatch_sovereign (SOVEREIGN — no CUDA)
    │        │  coral-parse → CoralIR → SASS binary
    │        │  NvVfioComputeDevice::dispatch (VFIO BAR0 + DMA)
    │        │  Readback via DMA → base64 outputs
    │        │
    ├──▶ device.dispatch (CUDA PTX, transitional)
    │        │  CudaComputeDevice (CUDA driver JIT)
    │        │
    │        ├── device.swap / device.health / device.lend
    │        ├── device.cold_boot (K80 sovereign boot, auto-detect recipe format)
    │        ├── device.warm_handoff (nouveau→FECS freeze→PFIFO snap→vfio)
    │        └── mailbox.* / ring.* (firmware IPC)
    │        ▼
    │    coral-driver ─── NvVfioComputeDevice / CudaComputeDevice / GpuChannel
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

## Sovereign vs Transitional

**`device.dispatch_sovereign`** is the full sovereign pipeline:
WGSL → coral-parse → CoralIR → SASS → NvVfioComputeDevice (VFIO BAR0/DMA).
No CUDA driver, no proprietary code. Accepts WGSL source directly.

**`device.dispatch`** is the transitional CUDA path: accepts pre-compiled PTX,
dispatches via `CudaComputeDevice` (requires CUDA driver). Will be deprecated
once sovereign path is validated across all hardware.

**`device.cold_boot`** orchestrates K80 sovereign boot from the daemon,
auto-detecting recipe format from agentReagents captures.

The boundary with toadStool is deferred because:

1. toadStool's S169 cleanup removed shader compilation proxies;
   dispatch absorption is the next phase
2. Warm handoff semantics need validation across GPU generations

## Capability Advertisement

`device.dispatch_sovereign` and `device.cold_boot` are in `capabilities.list`
under the `"sovereign"` key. `device.dispatch` remains under `"transitional"`.
Consumers should:

1. Discover via `device.sock` domain symlink
2. Check `capabilities.list` for `device.dispatch`
3. Be prepared for this method to move to `toadStool` in the future

## Post-S169 Spring Routing

After toadStool S169 removed `shader.compile.*` proxy methods, springs must
call coralReef directly for shader compilation:

- Discover via `shader.sock` domain symlink → `coralreef-core`
- Discover via `device.sock` domain symlink → `coral-glowplug`

Both symlinks live in `$XDG_RUNTIME_DIR/biomeos/`.
