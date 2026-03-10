// SPDX-License-Identifier: AGPL-3.0-only
//! Nouveau hardware validation — full dispatch cycle on NVIDIA via the
//! sovereign open-source driver.
//!
//! These tests exercise the complete nouveau compute pipeline:
//! alloc → upload → dispatch → sync → readback.
//!
//! Requires an NVIDIA GPU with the `nouveau` kernel module loaded
//! (not `nvidia`/`nvidia-drm` — these use the proprietary driver).
//!
//! Run: `cargo test --test hw_nv_nouveau --features nouveau -- --ignored`

#[cfg(feature = "nouveau")]
mod tests {
    use coral_driver::nv::NvDevice;
    use coral_driver::{ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};
    use coral_reef::{CompileOptions, FmaPolicy, GpuTarget, NvArch};

    fn open_nv() -> NvDevice {
        NvDevice::open().expect("NvDevice::open() — is nouveau loaded?")
    }

    fn compile_for_sm70(wgsl: &str) -> coral_reef::backend::CompiledBinary {
        let opts = CompileOptions {
            target: GpuTarget::Nvidia(NvArch::Sm70),
            opt_level: 2,
            debug_info: false,
            fp64_software: false,
            fma_policy: FmaPolicy::Fused,
            ..CompileOptions::default()
        };
        coral_reef::compile_wgsl_full(wgsl, &opts).expect("SM70 compilation")
    }

    const WRITE_42_SHADER: &str = r"
@group(0) @binding(0)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main() {
    out[0] = 42u;
}
";

    #[test]
    #[ignore = "requires nouveau hardware (Titan V / SM70)"]
    fn nouveau_device_opens() {
        let _dev = open_nv();
    }

    #[test]
    #[ignore = "requires nouveau hardware (Titan V / SM70)"]
    fn nouveau_alloc_free() {
        let mut dev = open_nv();
        let buf = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc GTT");
        dev.free(buf).expect("free");
    }

    #[test]
    #[ignore = "requires nouveau hardware (Titan V / SM70)"]
    fn nouveau_upload_readback_roundtrip() {
        let mut dev = open_nv();
        let buf = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc");

        let payload: Vec<u8> = (0..256)
            .map(|i: i32| u8::try_from(i & 0xFF).unwrap())
            .collect();
        dev.upload(buf, 0, &payload).expect("upload");

        let readback = dev.readback(buf, 0, 256).expect("readback");
        assert_eq!(readback, payload);

        dev.free(buf).expect("free");
    }

    #[test]
    #[ignore = "requires nouveau hardware (Titan V / SM70)"]
    fn nouveau_full_dispatch_cycle() {
        let compiled = compile_for_sm70(WRITE_42_SHADER);
        let mut dev = open_nv();

        let buf = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc");
        dev.upload(buf, 0, &[0u8; 4096]).expect("zero buffer");

        let info = ShaderInfo {
            gpr_count: compiled.info.gpr_count,
            shared_mem_bytes: compiled.info.shared_mem_bytes,
            barrier_count: compiled.info.barrier_count,
            workgroup: compiled.info.local_size,
        };

        dev.dispatch(&compiled.binary, &[buf], DispatchDims::linear(1), &info)
            .expect("dispatch");
        dev.sync().expect("sync");

        let readback = dev.readback(buf, 0, 4).expect("readback");
        let value = u32::from_le_bytes(readback[..4].try_into().unwrap());
        assert_eq!(value, 42, "nouveau dispatch: expected 42, got {value}");

        dev.free(buf).expect("free");
    }

    #[test]
    #[ignore = "requires nouveau hardware (Titan V / SM70)"]
    fn nouveau_multiple_dispatches() {
        let compiled = compile_for_sm70(WRITE_42_SHADER);
        let mut dev = open_nv();

        let buf = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc");

        let info = ShaderInfo {
            gpr_count: compiled.info.gpr_count,
            shared_mem_bytes: compiled.info.shared_mem_bytes,
            barrier_count: compiled.info.barrier_count,
            workgroup: compiled.info.local_size,
        };

        for i in 0..5 {
            dev.upload(buf, 0, &[0u8; 4096]).expect("zero");
            dev.dispatch(&compiled.binary, &[buf], DispatchDims::linear(1), &info)
                .unwrap_or_else(|e| panic!("dispatch {i} failed: {e}"));
            dev.sync().expect("sync");

            let readback = dev.readback(buf, 0, 4).expect("readback");
            let value = u32::from_le_bytes(readback[..4].try_into().unwrap());
            assert_eq!(value, 42, "dispatch {i}: expected 42, got {value}");
        }

        dev.free(buf).expect("free");
    }

    #[test]
    #[ignore = "requires nouveau hardware (Titan V / SM70)"]
    fn nouveau_sync_without_dispatch() {
        let mut dev = open_nv();
        dev.sync().expect("sync without dispatch should succeed");
    }
}
