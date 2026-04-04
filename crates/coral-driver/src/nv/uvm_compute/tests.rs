// SPDX-License-Identifier: AGPL-3.0-only

use crate::nv::uvm::{
    ADA_COMPUTE_A, AMPERE_CHANNEL_GPFIFO_A, AMPERE_COMPUTE_A, AMPERE_COMPUTE_B,
    BLACKWELL_CHANNEL_GPFIFO_B, BLACKWELL_COMPUTE_A, BLACKWELL_COMPUTE_B, HOPPER_COMPUTE_A,
    NvGpuDevice, NvUvmDevice, RmClient, VOLTA_CHANNEL_GPFIFO_A, VOLTA_COMPUTE_A,
};
use crate::{ComputeDevice, MemoryDomain};

use super::device::NvUvmComputeDevice;
use super::types::{GPFIFO_SIZE, GpuGen, USERD_SIZE, gpfifo_entry, page_align};

#[test]
fn gpu_gen_class_selection() {
    assert_eq!(GpuGen::from_sm(70).channel_class(), VOLTA_CHANNEL_GPFIFO_A);
    assert_eq!(GpuGen::from_sm(70).compute_class(), VOLTA_COMPUTE_A);
    assert_eq!(GpuGen::from_sm(75).channel_class(), VOLTA_CHANNEL_GPFIFO_A);
    assert_eq!(GpuGen::from_sm(75).compute_class(), VOLTA_COMPUTE_A);
    assert_eq!(GpuGen::from_sm(80).channel_class(), AMPERE_CHANNEL_GPFIFO_A);
    assert_eq!(GpuGen::from_sm(80).compute_class(), AMPERE_COMPUTE_A);
    assert_eq!(GpuGen::from_sm(86).channel_class(), AMPERE_CHANNEL_GPFIFO_A);
    assert_eq!(GpuGen::from_sm(86).compute_class(), AMPERE_COMPUTE_B);
    assert_eq!(GpuGen::from_sm(89).compute_class(), ADA_COMPUTE_A);
    assert_eq!(GpuGen::from_sm(90).compute_class(), HOPPER_COMPUTE_A);
    assert_eq!(GpuGen::from_sm(100).compute_class(), BLACKWELL_COMPUTE_A);
    assert_eq!(GpuGen::from_sm(120).compute_class(), BLACKWELL_COMPUTE_B);
    assert_eq!(
        GpuGen::from_sm(120).channel_class(),
        BLACKWELL_CHANNEL_GPFIFO_B
    );
}

#[test]
fn page_alignment() {
    assert_eq!(page_align(1), 4096);
    assert_eq!(page_align(4096), 4096);
    assert_eq!(page_align(4097), 8192);
    assert_eq!(page_align(0), 0);
}

#[test]
fn gpfifo_entry_encoding() {
    let va = 0x0000_0001_0000_1000_u64;
    let dwords = 64_u32;
    let entry = gpfifo_entry(va, dwords);
    // DWORD 0 = address[31:0] (bits[1:0]=0 for alignment)
    let dw0 = entry as u32;
    assert_eq!(dw0, va as u32);
    // DWORD 1 bits[8:0] = address[40:32], bits[30:10] = length
    let dw1 = (entry >> 32) as u32;
    let decoded_addr_hi = (dw1 & 0x1FF) as u64;
    let decoded_va = (dw0 as u64) | (decoded_addr_hi << 32);
    assert_eq!(decoded_va, va);
    let decoded_len = (dw1 >> 10) & 0x1F_FFFF;
    assert_eq!(decoded_len, dwords);
}

#[test]
fn gpfifo_entry_zero_length() {
    let entry = gpfifo_entry(0x1000, 0);
    let dw1 = (entry >> 32) as u32;
    assert_eq!((dw1 >> 10) & 0x1F_FFFF, 0);
    assert_eq!(entry as u32, 0x1000);
}

#[test]
fn gpu_gen_sm_roundtrip() {
    assert_eq!(GpuGen::Volta, GpuGen::from_sm(70));
    assert_eq!(GpuGen::Turing, GpuGen::from_sm(75));
    assert_eq!(GpuGen::AmpereA, GpuGen::from_sm(80));
    assert_eq!(GpuGen::AmpereB, GpuGen::from_sm(86));
    assert_eq!(GpuGen::Ada, GpuGen::from_sm(89));
    assert_eq!(GpuGen::Hopper, GpuGen::from_sm(90));
    assert_eq!(GpuGen::BlackwellA, GpuGen::from_sm(100));
    assert_eq!(GpuGen::BlackwellB, GpuGen::from_sm(120));
}

fn detect_sm_version() -> u32 {
    std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=compute_cap", "--format=csv,noheader"])
        .output()
        .ok()
        .and_then(|out| {
            let s = String::from_utf8_lossy(&out.stdout);
            let parts: Vec<&str> = s.trim().split('.').collect();
            if parts.len() == 2 {
                let major: u32 = parts[0].parse().ok()?;
                let minor: u32 = parts[1].parse().ok()?;
                Some(major * 10 + minor)
            } else {
                None
            }
        })
        .unwrap_or(86)
}

#[test]
#[ignore = "requires proprietary nvidia driver loaded"]
fn uvm_compute_device_open() {
    let sm = detect_sm_version();
    let device = NvUvmComputeDevice::open(0, sm).expect("UVM compute device");
    assert!(device.is_open());
}

#[test]
#[ignore = "requires proprietary nvidia driver loaded"]
fn uvm_compute_alloc_free() {
    let sm = detect_sm_version();
    let mut device = NvUvmComputeDevice::open(0, sm).expect("UVM compute device");
    let handle = device.alloc(4096, MemoryDomain::Gtt).expect("buffer alloc");
    device.free(handle).expect("buffer free");
}

#[test]
#[ignore = "requires proprietary nvidia driver loaded"]
fn uvm_map_memory_single_context() {
    let mut client = RmClient::new().expect("RM root client");
    let uvm = NvUvmDevice::open().expect("open UVM");
    let gpu = NvGpuDevice::open(0).expect("open GPU");
    gpu.register_fd(client.ctl_fd()).expect("register GPU fd");
    uvm.initialize().expect("UVM_INITIALIZE");

    let h_device = client.alloc_device(gpu.index()).expect("RM device");
    let h_subdevice = client.alloc_subdevice(h_device).expect("RM subdevice");
    let _uuid = client
        .register_gpu_with_uvm(h_subdevice, &uvm)
        .expect("register UVM");

    // On Blackwell (580.x), only one rm_map_memory context per nvidiactl fd.
    // Verify the combined-allocation strategy from open() works.
    let h_mem = h_device + 0x5000;
    let combined_size = USERD_SIZE + GPFIFO_SIZE;
    client
        .alloc_system_memory(h_device, h_mem, combined_size)
        .expect("alloc combined");
    let addr = client
        .rm_map_memory(h_device, h_mem, 0, combined_size)
        .expect("rm_map_memory combined");
    assert!(addr != 0);

    let userd_ptr = addr as *mut u32;
    let gpfifo_ptr = (addr + USERD_SIZE) as *mut u32;
    // SAFETY: addr is a valid kernel mmap'd address from rm_map_memory
    // (asserted non-null above). userd_ptr and gpfifo_ptr are within the
    // mapped range (USERD_SIZE + GPFIFO_SIZE). Volatile reads/writes
    // match the GPU-visible mapping semantics.
    unsafe {
        crate::mmio::VolatilePtr::new(userd_ptr).write(0xDEAD_BEEF);
        crate::mmio::VolatilePtr::new(gpfifo_ptr).write(0xCAFE_BABE);
        assert_eq!(crate::mmio::VolatilePtr::new(userd_ptr).read(), 0xDEAD_BEEF);
        assert_eq!(
            crate::mmio::VolatilePtr::new(gpfifo_ptr).read(),
            0xCAFE_BABE
        );
    }

    client
        .rm_unmap_memory(h_device, h_mem, addr)
        .expect("unmap");
}
