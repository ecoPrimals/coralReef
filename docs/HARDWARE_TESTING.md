# Hardware Testing Guide — coralReef GPU Parity

## Hardware Inventory

| Node | GPU | Architecture | Driver | Status |
|------|-----|-------------|--------|--------|
| `renderD128` | AMD RX 6950 XT | RDNA2 (GFX1030) | `amdgpu` | Full pipeline: compile + dispatch + readback |
| `renderD129` | NVIDIA RTX 3090 | SM86 (Ampere) | `nvidia-drm` | Probe + compile-only (dispatch pending UVM) |

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
| vecadd (a + b) | PASS | BLOCKED (SR 0x29) | N/A | Pending UVM |
| saxpy (α*x + y) | PASS | BLOCKED (SR 0x29) | N/A | Pending UVM |
| reduce (sum) | PASS | BLOCKED (SR 0x29) | N/A | Pending UVM |
| matmul (tiled) | PASS | BLOCKED (SR 0x29) | N/A | Pending UVM |

### Known Compiler Limitations

1. **RDNA2 `global_invocation_id`**: The AMD backend does not yet map NVIDIA
   system register index 0x29 to the AMD equivalent. Shaders using
   `@builtin(global_invocation_id)` fail to compile for RDNA2.

2. **RDNA2 VOP2 VSRC1**: Certain register allocation patterns produce
   invalid `VOP2 VSRC1 must be a VGPR register` errors on RDNA2 when
   multiple storage operations interact.

3. **RDNA2 buffer reads**: Compiled shaders that read from storage buffers
   produce incorrect results (GPU reads 0). Write-constant shaders work.

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

## Titan Team Instructions

1. **GPU setup**: The Titan should use the `nouveau` driver (open-source).
   Enable with `--features nouveau`.

2. **Run all parity tests**:
   ```bash
   cargo test --test parity_harness -p coral-gpu --features nouveau -- --ignored
   ```

3. **Run NV hardware tests** (create `hw_nv_dispatch.rs` mirroring AMD):
   ```bash
   cargo test --test 'hw_nv*' -p coral-driver --features nouveau -- --ignored
   ```

4. **Verify multi-GPU**:
   ```bash
   cargo test --test hw_nv_probe -p coral-driver -- --ignored multi_gpu
   ```

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
