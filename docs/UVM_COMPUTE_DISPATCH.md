<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# UVM Sovereign Compute Dispatch — Bypass Nouveau

**Created**: March 11, 2026 (Phase 10 — Iteration 36)

---

## Problem

Desktop Volta GPUs (Titan V / GV100) are missing PMU firmware, which
blocks nouveau from creating compute channels. The proprietary NVIDIA
driver manages its own firmware internally and does not need separate
PMU blobs. By using the RM (Resource Manager) API through `/dev/nvidiactl`
and UVM through `/dev/nvidia-uvm`, we bypass nouveau entirely.

## Architecture

```
┌─────────────┐    ┌──────────────┐    ┌──────────────────┐
│ /dev/nvidia0 │    │ /dev/nvidiactl│    │ /dev/nvidia-uvm  │
│  (GPU device)│    │  (RM control) │    │ (virtual memory)  │
└──────┬──────┘    └──────┬───────┘    └──────────┬───────┘
       │                  │                       │
       ▼                  ▼                       ▼
  GPU mmap           RM object tree          UVM alloc/map
  (CPU access)       (client→device→         (GPU VA space)
                      channel→compute)
```

## RM Object Hierarchy

The NVIDIA Resource Manager uses a tree of objects allocated via
`NV_ESC_RM_ALLOC`. Each object has a class ID, a parent handle,
and type-specific allocation parameters.

```
NV01_ROOT (0x0000) ─── root client
  ├── NV01_DEVICE_0 (0x0080) ─── GPU device
  │     ├── NV20_SUBDEVICE_0 (0x2080) ─── subdevice (GPU control queries)
  │     ├── FERMI_VASPACE_A (0x90F1) ─── GPU virtual address space
  │     └── KEPLER_CHANNEL_GROUP_A (0xA06C) ─── channel group (TSG)
  │           └── VOLTA_CHANNEL_GPFIFO_A (0xC36F) ─── GPFIFO channel
  │                 └── VOLTA_COMPUTE_A (0xC3C0) ─── compute engine
  └── NV01_MEMORY_SYSTEM (0x003E) ─── system memory allocation
```

## Dispatch Pipeline

1. **Open devices**: `/dev/nvidiactl` + `/dev/nvidia-uvm` + `/dev/nvidia0`
2. **RM client**: `NV_ESC_RM_ALLOC(NV01_ROOT)` — root object
3. **Device + subdevice**: `RM_ALLOC(DEVICE)` + `RM_ALLOC(SUBDEVICE)`
4. **GPU UUID query**: `NV_ESC_RM_CONTROL(GET_GID_INFO)` on subdevice
5. **UVM registration**: `UVM_REGISTER_GPU` with UUID from step 4
6. **VA space**: `RM_ALLOC(FERMI_VASPACE_A)` — GPU virtual address space
7. **Channel group**: `RM_ALLOC(KEPLER_CHANNEL_GROUP_A)` — TSG
8. **GPFIFO ring + USERD**: `RM_ALLOC(NV01_MEMORY_SYSTEM)` × 2
9. **GPFIFO channel**: `RM_ALLOC(VOLTA_CHANNEL_GPFIFO_A)`
10. **Compute engine**: `RM_ALLOC(VOLTA_COMPUTE_A)` on the channel
11. **Allocate buffers**: `RM_ALLOC(MEMORY)` + `UVM_MAP_EXTERNAL_ALLOCATION`
12. **Build QMD + push buffer**: reuse `qmd.rs` + `pushbuf.rs` (identical format)
13. **Submit**: write to GPFIFO ring, ring doorbell
14. **Sync**: UVM semaphore or spin

## Reusable Components

The push buffer format and QMD construction are identical between
nouveau and the proprietary driver path:

- `nv/qmd.rs` — QMD v2.1 (Volta) and v3.0 (Ampere) construction
- `nv/pushbuf.rs` — Kepler+ Type 1/3/4 push buffer headers
- `ShaderInfo` — compiler-derived GPR count, shared memory, barriers

## Struct Sources

All `#[repr(C)]` struct definitions derived from:
- `nvidia-open-gpu-kernel-modules` (MIT license)
- `src/common/sdk/nvidia/inc/` headers
- Every struct has a compile-time `assert_eq!(size_of::<T>(), N)` test

## GPU Generation Support

| GPU | Generation | Channel Class | Compute Class |
|-----|-----------|---------------|---------------|
| Titan V | Volta | 0xC36F | 0xC3C0 |
| RTX 2080 | Turing | 0xC46F | 0xC5C0 |
| RTX 3090 | Ampere | 0xC56F | 0xC6C0 |
| RTX 4070 | Ada | 0xC76F | 0xC9C0 |
