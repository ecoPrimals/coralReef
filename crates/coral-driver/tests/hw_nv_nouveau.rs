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

    // ── Diagnostic tests for EINVAL investigation ──────────────────────

    #[test]
    #[ignore = "requires nouveau hardware — diagnostic: isolate EINVAL source"]
    fn nouveau_diagnose_channel_alloc() {
        use coral_driver::nv::ioctl::{NVIF_CLASS_VOLTA_COMPUTE_A, diagnose_channel_alloc};

        let drm = coral_driver::drm::DrmDevice::open_by_driver("nouveau")
            .expect("open nouveau render node");

        let diags = diagnose_channel_alloc(drm.fd(), NVIF_CLASS_VOLTA_COMPUTE_A);
        for diag in &diags {
            match &diag.result {
                Ok(ch) => eprintln!("[PASS] {} → channel {ch}", diag.description),
                Err(e) => eprintln!("[FAIL] {} → {e}", diag.description),
            }
        }
        assert!(
            !diags.is_empty(),
            "diagnostic should produce at least one result"
        );
    }

    #[test]
    #[ignore = "requires nouveau hardware — diagnostic: hex dump channel alloc struct"]
    fn nouveau_channel_alloc_hex_dump() {
        use coral_driver::nv::ioctl::{NVIF_CLASS_VOLTA_COMPUTE_A, dump_channel_alloc_hex};

        let hex = dump_channel_alloc_hex(NVIF_CLASS_VOLTA_COMPUTE_A);
        eprintln!("{hex}");
        assert!(hex.contains("NouveauChannelAlloc"));
    }

    #[test]
    #[ignore = "requires nouveau hardware — diagnostic: check firmware files"]
    fn nouveau_firmware_probe() {
        use coral_driver::nv::ioctl::check_nouveau_firmware;

        for chip in &["gv100", "tu102", "ga102"] {
            let entries = check_nouveau_firmware(chip);
            eprintln!("Firmware for {chip}:");
            let mut missing = 0;
            for (path, exists) in &entries {
                let status = if *exists { "OK" } else { "MISSING" };
                eprintln!("  [{status}] {path}");
                if !*exists {
                    missing += 1;
                }
            }
            eprintln!(
                "  → {}/{} present\n",
                entries.len() - missing,
                entries.len()
            );
        }
    }

    #[test]
    #[ignore = "requires nouveau hardware — diagnostic: probe GPU identity via sysfs"]
    fn nouveau_gpu_identity_probe() {
        use coral_driver::drm::enumerate_render_nodes;
        use coral_driver::nv::ioctl::probe_gpu_identity;

        let nodes = enumerate_render_nodes();
        for info in &nodes {
            if info.driver == "nouveau" {
                if let Some(id) = probe_gpu_identity(&info.path) {
                    eprintln!(
                        "{}: vendor=0x{:04X} device=0x{:04X} sm={:?}",
                        info.path,
                        id.vendor_id,
                        id.device_id,
                        id.nvidia_sm()
                    );
                } else {
                    eprintln!("{}: could not probe sysfs identity", info.path);
                }
            }
        }
    }

    #[test]
    #[ignore = "requires nouveau hardware — diagnostic: try GEM alloc without channel"]
    fn nouveau_gem_alloc_without_channel() {
        let drm = coral_driver::drm::DrmDevice::open_by_driver("nouveau")
            .expect("open nouveau render node");
        let result =
            coral_driver::nv::ioctl::gem_new(drm.fd(), 4096, coral_driver::MemoryDomain::Gtt);
        match result {
            Ok(handle) => {
                eprintln!("GEM alloc succeeded: handle={handle}");
                let _ = coral_driver::drm::gem_close(drm.fd(), handle);
            }
            Err(e) => {
                eprintln!("GEM alloc failed: {e}");
            }
        }
    }
}
