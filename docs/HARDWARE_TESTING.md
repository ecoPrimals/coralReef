<!-- SPDX-License-Identifier: CC-BY-SA-4.0 -->

# Hardware Testing Guide — coralReef GPU Parity

**Last updated**: March 12, 2026 (Phase 10 — Iteration 37)

## Hardware Inventory

| Node | GPU | Architecture | Driver | Status |
|------|-----|-------------|--------|--------|
| `renderD128` | AMD RX 6950 XT | RDNA2 (GFX1030) | `amdgpu` | Full pipeline: compile + dispatch + readback |
| `renderD129` | NVIDIA RTX 3090 | SM86 (Ampere) | `nvidia-drm` | UVM dispatch pipeline code-complete (pending hardware validation) |

### hotSpring Test Rig (remote)

| GPU | Architecture | Driver Available | Needed Tests |
|-----|-------------|-----------------|-------------|
| NVIDIA Titan V | GV100 SM70 (Volta) | nouveau (open) | **Blocked**: PMU firmware missing (Exp 057). VM_INIT succeeds, CHANNEL_ALLOC fails. |
| NVIDIA RTX 3090 | GA102 SM86 (Ampere) | nvidia-drm (proprietary) | UVM RM client, buffer mapping, compute dispatch |

## Feature Flags

| Feature | Crate | Purpose |
|---------|-------|---------|
| `nouveau` | `coral-driver`, `coral-gpu` | Enables nouveau DRM backend for open-source NVIDIA driver |
| `nvidia-drm` | `coral-driver`, `coral-gpu` | Enables nvidia-drm backend for proprietary NVIDIA driver |
| `rdna2-buffer-read` | `coral-driver` | Enables blocked E2E tests for RDNA2 buffer-read shader patterns |
| `test-utils` | `coral-driver` | Exposes `BufferHandle::from_id()` for mock device construction |

## Running Tests

### AMD hardware (full pipeline)

```bash
cargo test --test 'hw_amd*' -p coral-driver -- --ignored
```

### AMD stress tests

```bash
cargo test --test hw_amd_stress -p coral-driver -- --ignored
```

### NVIDIA probe tests (device detection)

```bash
cargo test --test hw_nv_probe -p coral-driver -- --ignored
```

### NVIDIA buffer tests (nvidia-drm feature required)

```bash
cargo test --test hw_nv_buffers -p coral-driver --features nvidia-drm -- --ignored
```

### Parity compilation (no hardware required)

```bash
cargo test --test parity_compilation -p coral-reef
cargo test --test parity_harness -p coral-gpu
```

### Parity hardware dispatch (AMD only, currently)

```bash
cargo test --test parity_harness -p coral-gpu -- --ignored
```

## Parity Test Matrix

| Shader | SM86 Compile | RDNA2 Compile | AMD Dispatch | NV Dispatch |
|--------|-------------|---------------|-------------|-------------|
| store_42 (constant write) | PASS | PASS | PASS | Pending UVM |
| vecadd (a + b) | PASS | PASS | PASS | Pending UVM |
| saxpy (α*x + y) | PASS | PASS | PASS | Pending UVM |
| reduce (sum) | PASS | PASS | PASS | Pending UVM |
| matmul (tiled) | PASS | PASS | PASS | Pending UVM |

### Known Compiler Limitations (resolved)

1. ~~**RDNA2 `global_invocation_id`**: SR 0x29 mapping~~ — **Fixed Iter 25**
2. ~~**RDNA2 VOP2 VSRC1**: operand legalization~~ — **Fixed Iter 25**
3. ~~**RDNA2 buffer reads**: incorrect results~~ — **Fixed Iter 27** (literal materialization)

### Remaining Limitations

1. **NVIDIA UVM dispatch**: Full dispatch pipeline code-complete (Iter 37: GPFIFO submission + USERD doorbell + completion polling). `NvDrmDevice` delegates to `NvUvmComputeDevice`. Pending: on-site RTX 3090 hardware validation.
2. **Nouveau dispatch — Titan V PMU firmware blocker**: hotSpring Exp 057 validated that all 4 DRM ioctl struct ABI mismatches are now fixed (VM_INIT succeeds), but CHANNEL_ALLOC fails due to missing PMU firmware. NVIDIA does not distribute signed PMU firmware blobs for desktop Volta (GV100). Firmware inventory: ACR ✓, GR ✓, SEC2 ✓, NVDEC ✓, PMU ✗. Channel creation cannot proceed without PMU. Paths forward: (a) GSP firmware on Ampere+ (RTX 3090 has GSP); (b) nvidia-drm UVM integration bypasses nouveau entirely; (c) eastgate 4070 (Ada, GSP-based) as teacher for Volta via hw-learn.
3. **Nouveau UAPI struct ABI**: 7 compile-time size assertions now guard against future struct drift (5 new UAPI + 2 fixed legacy).

## CI Configuration

### Feature matrix for CI

```yaml
strategy:
  matrix:
    include:
      - features: ""
        name: "default (compile-only)"
      - features: "--features nouveau"
        name: "nouveau"
      - features: "--features nvidia-drm"
        name: "nvidia-drm"
```

### Target matrix for parity tests

```yaml
targets:
  - SM70 (Volta)
  - SM75 (Turing)
  - SM80 (Ampere A100)
  - SM86 (Ampere consumer)
  - SM89 (Ada Lovelace)
  - RDNA2 (RX 6000)
  - RDNA3 (RX 7000)
  - RDNA4 (RX 9000)
```

## Titan V + RTX 3090 Test Instructions (hotSpring Test Rig)

### Prerequisites

```bash
git clone git@github.com:ecoPrimals/coralReef.git
cd coralReef
cargo check --workspace
cargo test --workspace  # 2241 passing, should complete in ~60s
```

### Step 1: Nouveau EINVAL Diagnostics (Titan V)

The Titan V channel creation returns EINVAL. The diagnostic suite tries
multiple channel configurations and reports which succeed. This is the
**highest priority** data we need.

```bash
# Full diagnostic suite — captures all 5 diagnostic tests
cargo test --test hw_nv_nouveau -p coral-driver --features nouveau -- --ignored --nocapture 2>&1 | tee nouveau_diag.log
```

**What to capture**: The entire `nouveau_diag.log` output. Key data points:

| Test | What it tells us |
|------|-----------------|
| `nouveau_diagnose_channel_alloc` | Which channel configurations work (bare, compute, NVK-style, alt-class) |
| `nouveau_channel_alloc_hex_dump` | Raw 92-byte struct for kernel ABI verification |
| `nouveau_firmware_probe` | Which firmware files are present in `/lib/firmware/nvidia/gv100/` |
| `nouveau_gpu_identity_probe` | PCI device ID → SM version mapping verification |
| `nouveau_gem_alloc_without_channel` | Whether GEM buffer alloc works independent of channel creation |

### Step 2: Environment Data

```bash
# Kernel and driver info
uname -r
cat /proc/version
modinfo nouveau | head -20
ls -la /dev/dri/renderD*
ls -la /dev/nvidia*

# GPU identity from sysfs
for d in /sys/class/drm/renderD*/device; do
  echo "=== $d ==="
  cat "$d/vendor" "$d/device" 2>/dev/null
  cat "$d/driver_override" 2>/dev/null
done

# Firmware status
ls -la /lib/firmware/nvidia/gv100/ 2>/dev/null || echo "No gv100 firmware dir"
ls -la /lib/firmware/nvidia/ga102/ 2>/dev/null || echo "No ga102 firmware dir"

# Recent DRM messages
dmesg | grep -i 'nouveau\|nvidia\|drm' | tail -50
```

### Step 3: nvidia-drm UVM Probing (RTX 3090)

If the proprietary NVIDIA driver is loaded alongside nouveau:

```bash
# UVM device probing
cargo test --test hw_nv_probe -p coral-driver -- --ignored --nocapture 2>&1 | tee nv_probe.log

# UVM RM client tests (requires /dev/nvidiactl)
cargo test uvm -p coral-driver -- --ignored --nocapture 2>&1 | tee uvm_diag.log

# nvidia-drm buffer tests
cargo test --test hw_nv_buffers -p coral-driver --features nvidia-drm -- --ignored --nocapture 2>&1 | tee nv_buffers.log
```

### Step 4: Multi-GPU Enumeration

```bash
cargo test --test hw_nv_probe -p coral-driver -- --ignored multi_gpu --nocapture 2>&1 | tee multi_gpu.log
```

### Step 5: Full Parity Suite

```bash
# Compile-only parity (no hardware needed)
cargo test --test parity_compilation -p coral-reef 2>&1 | tee parity_compile.log

# Hardware dispatch parity (nouveau)
cargo test --test parity_harness -p coral-gpu --features nouveau -- --ignored --nocapture 2>&1 | tee parity_nouveau.log
```

### What to Send Back

Please capture and share these files:
1. `nouveau_diag.log` — **critical** for EINVAL debugging
2. Environment data output (kernel, driver, sysfs, firmware, dmesg)
3. `nv_probe.log` — device detection
4. `uvm_diag.log` — if proprietary driver present
5. `nv_buffers.log` — if proprietary driver present
6. `multi_gpu.log` — multi-GPU enumeration
7. `parity_nouveau.log` — E2E dispatch attempt

## Device Discovery

coralReef discovers GPUs through two paths:

1. **toadStool ecosystem discovery**: Reads
   `$XDG_RUNTIME_DIR/ecoPrimals/discovery/*.json` for capability files
   advertising `gpu.dispatch`. When toadStool is running, it provides
   vendor-agnostic device metadata.

2. **Direct DRM scan** (fallback): Enumerates `/dev/dri/renderD*` nodes
   via `coral_driver::drm::enumerate_render_nodes()`.

The `GpuContext::auto()` method uses DRM scan directly.
The `GpuContext::from_descriptor()` method accepts discovery results
from the toadStool integration in `coralreef-core::discovery`.
