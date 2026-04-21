// SPDX-License-Identifier: AGPL-3.0-or-later
//! End-to-end pipeline integration test.
//!
//! Exercises the full sovereign GPU dispatch pipeline shape:
//! WGSL-equivalent metadata → QMD build → cubin assembly → ComputeDevice
//! trait flow (alloc, upload, dispatch attempt, readback).
//!
//! Runs without a physical GPU by using the Intel stub device for trait
//! flow and testing the cubin assembler + QMD builder as pure functions.

#[cfg(feature = "intel")]
mod intel_trait_flow {
    use coral_driver::intel::IntelDevice;
    use coral_driver::{ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

    #[test]
    fn alloc_upload_readback_round_trip() {
        let mut dev = IntelDevice::stub(12);

        let data = vec![42u8; 256];
        let buf = dev.alloc(256, MemoryDomain::Gtt).unwrap();
        dev.upload(buf, 0, &data).unwrap();

        let readback = dev.readback(buf, 0, 256).unwrap();
        assert_eq!(readback, data);

        dev.free(buf).unwrap();
    }

    #[test]
    fn multi_buffer_alloc_and_free() {
        let mut dev = IntelDevice::stub(13);

        let bufs: Vec<_> = (0..8)
            .map(|i| dev.alloc(1024 * (i + 1), MemoryDomain::Vram).unwrap())
            .collect();

        for (i, &buf) in bufs.iter().enumerate() {
            let pattern = vec![(i as u8).wrapping_mul(37); 128];
            dev.upload(buf, 0, &pattern).unwrap();
            let rb = dev.readback(buf, 0, 128).unwrap();
            assert_eq!(rb, pattern);
        }

        for buf in bufs {
            dev.free(buf).unwrap();
        }
    }

    #[test]
    fn dispatch_returns_skeleton_error() {
        let mut dev = IntelDevice::stub(12);
        let info = ShaderInfo {
            gpr_count: 32,
            shared_mem_bytes: 0,
            barrier_count: 0,
            workgroup: [64, 1, 1],
            wave_size: 32,
            local_mem_bytes: None,
        };
        let result = dev.dispatch(&[0xDE, 0xAD], &[], DispatchDims::linear(1), &info);
        assert!(result.is_err());

        dev.sync().unwrap();
    }

    #[test]
    fn capabilities_are_correct() {
        let dev = IntelDevice::stub(12);
        let caps = dev.capabilities();
        assert_eq!(caps.vendor, coral_driver::hardware::Vendor::Intel);
        assert!(!caps.has_hardware_f64);
    }
}

#[cfg(feature = "cuda")]
mod cubin_assembly {
    use coral_driver::cuda::cubin::{CubinKernelInfo, assemble_cubin, is_cubin};
    use coral_driver::ShaderInfo;

    #[test]
    fn from_shader_info_round_trip() {
        let info = ShaderInfo {
            gpr_count: 48,
            shared_mem_bytes: 4096,
            barrier_count: 2,
            workgroup: [128, 1, 1],
            wave_size: 32,
            local_mem_bytes: Some(256),
        };

        let cubin_info = CubinKernelInfo::from_shader_info(&info, 86);
        assert_eq!(cubin_info.sm, 86);
        assert_eq!(cubin_info.gpr_count, 48);
        assert_eq!(cubin_info.shared_mem_bytes, 4096);
        assert_eq!(cubin_info.barrier_count, 2);
    }

    #[test]
    fn assembled_cubin_is_valid_elf() {
        let sass = vec![0xABu8; 128];
        let info = CubinKernelInfo {
            sm: 70,
            gpr_count: 32,
            shared_mem_bytes: 0,
            barrier_count: 0,
        };
        let elf = assemble_cubin(&sass, &info);
        assert!(is_cubin(&elf));
        assert!(elf.len() > 128);
        assert_eq!(&elf[..4], b"\x7fELF");
    }

    #[test]
    fn cubin_preserves_sass_content() {
        let sass: Vec<u8> = (0..64).collect();
        let info = CubinKernelInfo {
            sm: 86,
            gpr_count: 16,
            shared_mem_bytes: 0,
            barrier_count: 0,
        };
        let elf = assemble_cubin(&sass, &info);
        assert_eq!(&elf[64..128], &sass[..]);
    }
}

mod qmd_cbuf_layout {
    use coral_driver::nv::qmd;
    use coral_driver::DispatchDims;

    #[test]
    fn standard_cbufs_has_eight_entries() {
        let cbufs = qmd::build_standard_cbufs(0x1000, 256, 0x2000, 64);
        assert_eq!(cbufs.len(), 8);
    }

    #[test]
    fn standard_cbufs_slot_seven_is_driver_const() {
        let cbufs = qmd::build_standard_cbufs(0x1000, 256, 0xDC00, 64);
        let slot7 = &cbufs[7];
        assert_eq!(slot7.index, qmd::DRIVER_CBUF_INDEX);
        assert_eq!(slot7.addr, 0xDC00);
        assert_eq!(slot7.size, 64);
    }

    #[test]
    fn slots_zero_through_six_are_descriptor_table() {
        let cbufs = qmd::build_standard_cbufs(0xABCD, 512, 0x0, 64);
        for i in 0..7 {
            assert_eq!(cbufs[i].index, i as u32);
            assert_eq!(cbufs[i].addr, 0xABCD);
            assert_eq!(cbufs[i].size, 512);
        }
    }

    #[test]
    fn encode_driver_constants_layout() {
        let dims = DispatchDims::new(8, 4, 2);
        let buf = qmd::encode_driver_constants(&dims);
        assert_eq!(buf.len(), qmd::DRIVER_CONST_SIZE as usize);

        let x = u32::from_le_bytes(buf[0..4].try_into().unwrap());
        let y = u32::from_le_bytes(buf[4..8].try_into().unwrap());
        let z = u32::from_le_bytes(buf[8..12].try_into().unwrap());
        assert_eq!((x, y, z), (8, 4, 2));

        assert!(buf[12..].iter().all(|&b| b == 0), "padding must be zero");
    }

    #[test]
    fn qmd_builds_for_all_known_sm_versions() {
        let params = qmd::QmdParams::simple(0x1000, DispatchDims::linear(1), 32);
        for sm in [35, 52, 61, 70, 75, 80, 86, 89, 90, 100, 120] {
            let words = qmd::build_qmd_for_sm(sm, &params);
            assert!(!words.is_empty(), "QMD for SM {sm} must be non-empty");
        }
    }
}
