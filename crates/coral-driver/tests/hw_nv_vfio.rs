// SPDX-License-Identifier: AGPL-3.0-only
//! NVIDIA VFIO hardware validation — direct BAR0/DMA dispatch.
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
//! Run: `CORALREEF_VFIO_BDF=0000:01:00.0 cargo test --test hw_nv_vfio --features vfio -- --ignored`

#[cfg(feature = "vfio")]
mod tests {
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

    fn open_vfio() -> NvVfioComputeDevice {
        let bdf = vfio_bdf();
        let sm = vfio_sm();
        let cc = sm_to_compute_class(sm);
        NvVfioComputeDevice::open(&bdf, sm, cc)
            .expect("NvVfioComputeDevice::open() — is GPU bound to vfio-pci?")
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
        dev.sync().expect("sync");
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_pfifo_diagnostic_matrix() {
        use coral_driver::nv::RawVfioDevice;
        use coral_driver::vfio::channel::{build_experiment_matrix, diagnostic_matrix};

        let bdf = vfio_bdf();
        let mut raw =
            RawVfioDevice::open(&bdf).expect("RawVfioDevice::open() — is GPU bound to vfio-pci?");

        // Verify PCIe bus mastering via sysfs (critical for DMA)
        let config_path = format!("/sys/bus/pci/devices/{bdf}/config");
        if let Ok(cfg) = std::fs::read(&config_path)
            && cfg.len() >= 6
        {
            let cmd = u16::from_le_bytes([cfg[4], cfg[5]]);
            let bm = cmd & 0x0004 != 0;
            eprintln!("PCI_COMMAND={cmd:#06x} BusMaster={bm}");
            assert!(bm, "PCIe bus mastering MUST be enabled for DMA");
        }

        let configs = build_experiment_matrix();
        eprintln!(
            "\n=== PFIFO DIAGNOSTIC MATRIX: {} configurations ===\n",
            configs.len()
        );

        let results = diagnostic_matrix(
            raw.container_fd,
            &raw.bar0,
            RawVfioDevice::gpfifo_iova(),
            RawVfioDevice::gpfifo_entries(),
            RawVfioDevice::userd_iova(),
            0, // channel ID
            &configs,
            raw.gpfifo_ring.as_mut_slice(),
            raw.userd.as_mut_slice(),
        )
        .expect("diagnostic_matrix failed");

        let total = results.len();
        let faulted: Vec<_> = results.iter().filter(|r| r.faulted).collect();
        let scheduled: Vec<_> = results.iter().filter(|r| r.scheduled).collect();
        let clean: Vec<_> = results
            .iter()
            .filter(|r| !r.faulted && r.scheduled)
            .collect();
        let pbdma_ours: Vec<_> = results.iter().filter(|r| r.pbdma_ours).collect();

        eprintln!("\n=== SUMMARY ===");
        eprintln!("Total:        {total}");
        eprintln!("Faulted:      {}", faulted.len());
        eprintln!("Scheduled:    {}", scheduled.len());
        eprintln!("Clean:        {} (no fault + scheduled)", clean.len());
        eprintln!(
            "PBDMA ours:   {} (registers changed from residual)",
            pbdma_ours.len()
        );

        if !clean.is_empty() {
            eprintln!("\n=== WINNING CONFIGURATIONS ===");
            for r in &clean {
                eprintln!("  {}", r.name);
            }
        }

        if !pbdma_ours.is_empty() {
            eprintln!("\n=== PBDMA REGISTERS CHANGED (direct programming worked) ===");
            for r in &pbdma_ours {
                eprintln!(
                    "  {} | USERD@D0={:08x} @08={:08x} GP_BASE={:08x}_{:08x} SIG={:08x} GP_PUT={} GP_FETCH={}",
                    r.name,
                    r.pbdma_userd_lo,
                    r.pbdma_ramfc_userd_lo,
                    r.pbdma_gp_base_hi,
                    r.pbdma_gp_base_lo,
                    r.pbdma_signature,
                    r.pbdma_gp_put,
                    r.pbdma_gp_fetch
                );
            }
        }

        if !scheduled.is_empty() {
            eprintln!("\n=== SCHEDULED (may have faults) ===");
            for r in &scheduled {
                eprintln!("  {} (faulted={})", r.name, r.faulted);
            }
        }

        eprintln!("\nDiagnostic matrix complete. Analyze the table above.");
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
