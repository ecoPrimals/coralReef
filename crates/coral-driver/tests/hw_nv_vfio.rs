// SPDX-License-Identifier: AGPL-3.0-only
//! NVIDIA VFIO hardware validation — core device opening, BAR0, basic ops.
//!
//! These tests exercise the VFIO compute pipeline:
//! open → alloc → upload → dispatch → sync → readback.
//!
//! # Prerequisites
//!
//! - GPU bound to `vfio-pci` (not nouveau/nvidia)
//! - IOMMU enabled in BIOS and kernel
//! - User has `/dev/vfio/*` permissions
//! - Set `CORALREEF_VFIO_BDF` env var to the GPU's PCIe address
//!
//! # GlowPlug integration
//!
//! If `coral-glowplug` is running and holds the VFIO fd, the test harness
//! automatically borrows the device via `device.lend` and returns it via
//! `device.reclaim` on drop. No manual VFIO management needed.
//!
//! Run: `CORALREEF_VFIO_BDF=0000:01:00.0 cargo test --test hw_nv_vfio --features vfio -- --ignored`

#[cfg(feature = "vfio")]
#[path = "glowplug_client.rs"]
mod glowplug_client;

#[cfg(feature = "vfio")]
#[path = "ember_client.rs"]
mod ember_client;

#[cfg(feature = "vfio")]
mod tests {
    use super::ember_client;
    use coral_driver::nv::NvVfioComputeDevice;
    use coral_driver::{ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

    fn vfio_bdf() -> String {
        std::env::var("CORALREEF_VFIO_BDF")
            .expect("set CORALREEF_VFIO_BDF=0000:XX:XX.X to run VFIO tests")
    }

    fn vfio_sm() -> u32 {
        std::env::var("CORALREEF_VFIO_SM")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(86)
    }

    fn sm_to_compute_class(sm: u32) -> u32 {
        match sm {
            70..=74 => coral_driver::nv::pushbuf::class::VOLTA_COMPUTE_A,
            75..=79 => coral_driver::nv::pushbuf::class::TURING_COMPUTE_A,
            _ => coral_driver::nv::pushbuf::class::AMPERE_COMPUTE_A,
        }
    }

    /// Open VFIO device — primary path: get fds from ember via SCM_RIGHTS.
    /// Fallback: open /dev/vfio/* directly (only works without ember).
    fn open_vfio() -> NvVfioComputeDevice {
        let bdf = vfio_bdf();
        let sm = vfio_sm();
        let cc = sm_to_compute_class(sm);

        match ember_client::request_fds(&bdf) {
            Ok(fds) => {
                eprintln!("ember: received VFIO fds for {bdf}");
                NvVfioComputeDevice::open_from_fds(
                    &bdf,
                    fds.container,
                    fds.group,
                    fds.device,
                    sm,
                    cc,
                )
                .expect("NvVfioComputeDevice::open_from_fds()")
            }
            Err(e) => {
                eprintln!("ember unavailable ({e}), opening VFIO directly");
                NvVfioComputeDevice::open(&bdf, sm, cc)
                    .expect("NvVfioComputeDevice::open() — is GPU bound to vfio-pci?")
            }
        }
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_open_and_bar0_read() {
        let _dev = open_vfio();
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_alloc_and_free() {
        let mut dev = open_vfio();
        let handle = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc");
        dev.free(handle).expect("free");
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_upload_and_readback() {
        let mut dev = open_vfio();
        let handle = dev.alloc(256, MemoryDomain::Gtt).expect("alloc");
        let data: Vec<u8> = (0..256).map(|i| (i & 0xFF) as u8).collect();
        dev.upload(handle, 0, &data).expect("upload");
        let result = dev.readback(handle, 0, 256).expect("readback");
        assert_eq!(result, data);
        dev.free(handle).expect("free");
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_multiple_buffers() {
        let mut dev = open_vfio();
        let handles: Vec<_> = (0..4)
            .map(|_| dev.alloc(4096, MemoryDomain::Gtt).expect("alloc"))
            .collect();
        for h in handles {
            dev.free(h).expect("free");
        }
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware + compute shader binary"]
    fn vfio_dispatch_nop_shader() {
        let mut dev = open_vfio();
        let sm = vfio_sm();

        let gr = dev.gr_engine_status();
        eprintln!("Pre-dispatch {gr}");

        if gr.fecs_halted() {
            eprintln!("FECS falcon halted — dispatch will fence-timeout on cold VFIO");
            eprintln!("  (FECS requires signed firmware loaded by nouveau/ACR)");
            eprintln!("  Use GlowPlug oracle warm-up to initialize GR before VFIO dispatch.");
        }

        let wgsl = "@compute @workgroup_size(64) fn main() {}";
        let opts = coral_reef::CompileOptions {
            target: match sm {
                70 => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm70),
                75 => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm75),
                80 => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm80),
                _ => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm86),
            },
            ..coral_reef::CompileOptions::default()
        };
        let compiled = coral_reef::compile_wgsl_full(wgsl, &opts).expect("compile");
        let info = ShaderInfo {
            gpr_count: compiled.info.gpr_count,
            shared_mem_bytes: compiled.info.shared_mem_bytes,
            barrier_count: compiled.info.barrier_count,
            workgroup: compiled.info.local_size,
        };

        dev.dispatch(&compiled.binary, &[], DispatchDims::linear(1), &info)
            .expect("dispatch");

        let sync_result = dev.sync();
        let gr_post = dev.gr_engine_status();
        eprintln!("Post-dispatch {gr_post}");

        if let Err(e) = sync_result {
            if gr.fecs_halted() {
                eprintln!(
                    "Dispatch fence-timeout expected: FECS not running — need GlowPlug oracle warm-up"
                );
            }
            panic!("sync: {e}");
        }
    }

    #[cfg(feature = "test-utils")]
    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_free_invalid_handle() {
        let mut dev = open_vfio();
        let result = dev.free(coral_driver::BufferHandle::from_id(9999));
        assert!(result.is_err());
    }

    #[cfg(feature = "test-utils")]
    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_readback_invalid_handle() {
        let dev = open_vfio();
        let result = dev.readback(coral_driver::BufferHandle::from_id(9999), 0, 16);
        assert!(result.is_err());
    }
}
