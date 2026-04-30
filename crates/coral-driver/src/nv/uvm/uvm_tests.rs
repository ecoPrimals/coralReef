// SPDX-License-Identifier: AGPL-3.0-or-later
//! Unit tests for UVM ioctl constants, RM class IDs, and struct ABI layout.

use super::*;

#[test]
fn uvm_ioctl_constants_are_valid() {
    assert_eq!(UVM_INITIALIZE, 0x3000_0001);
    assert_eq!(UVM_REGISTER_GPU, 37);
    assert_eq!(UVM_UNREGISTER_GPU, 38);
    assert_eq!(UVM_PAGEABLE_MEM_ACCESS, 39);
    assert_eq!(UVM_FREE, 34);
    assert_eq!(UVM_MAP_EXTERNAL_ALLOCATION, 33);
    assert_eq!(UVM_CREATE_EXTERNAL_RANGE, 73);
}

#[test]
fn rm_class_constants() {
    assert_eq!(NV01_ROOT, 0);
    assert_eq!(NV01_DEVICE_0, 0x80);
    assert_eq!(NV20_SUBDEVICE_0, 0x2080);
    assert_eq!(FERMI_VASPACE_A, 0x90F1);
    assert_eq!(KEPLER_CHANNEL_GROUP_A, 0xA06C);
    assert_eq!(VOLTA_CHANNEL_GPFIFO_A, 0xC36F);
    assert_eq!(AMPERE_CHANNEL_GPFIFO_A, 0xC56F);
    assert_eq!(VOLTA_COMPUTE_A, 0xC3C0);
    assert_eq!(TURING_COMPUTE_A, 0xC5C0);
    assert_eq!(AMPERE_COMPUTE_A, 0xC6C0);
    assert_eq!(AMPERE_COMPUTE_B, 0xC7C0);
    assert_eq!(ADA_COMPUTE_A, 0xC9C0);
    assert_eq!(HOPPER_COMPUTE_A, 0xCBC0);
    assert_eq!(BLACKWELL_COMPUTE_A, 0xCDC0);
    assert_eq!(BLACKWELL_COMPUTE_B, 0xCEC0);
    assert_eq!(BLACKWELL_CHANNEL_GPFIFO_A, 0xC96F);
    assert_eq!(BLACKWELL_CHANNEL_GPFIFO_B, 0xCA6F);
    assert_eq!(NV01_MEMORY_SYSTEM, 0x3E);
    assert_eq!(NV01_MEMORY_LOCAL_USER, 0x40);
}

#[test]
fn uvm_param_struct_sizes() {
    assert_eq!(std::mem::size_of::<UvmInitializeParams>(), 16);
    assert_eq!(std::mem::size_of::<NvRmAllocParams>(), 48);
    assert_eq!(std::mem::size_of::<NvRmFreeParams>(), 16);
    assert_eq!(std::mem::size_of::<NvRmControlParams>(), 32);
    assert_eq!(std::mem::size_of::<NvMemoryDescParams>(), 24);
    assert_eq!(std::mem::size_of::<Nv2080GpuGetGidInfoParams>(), 268);
    assert_eq!(std::mem::size_of::<Nv0080AllocParams>(), 56);
    assert_eq!(std::mem::size_of::<UvmRegisterGpuParams>(), 40);
    assert_eq!(std::mem::size_of::<UvmPageableMemAccessParams>(), 8);
    assert_eq!(std::mem::size_of::<UvmGpuMappingAttributes>(), 36);
    assert_eq!(std::mem::size_of::<NvChannelGroupAllocParams>(), 32);
    assert_eq!(std::mem::size_of::<NvMemoryAllocParams>(), 128);
    assert_eq!(std::mem::size_of::<NvRmMapMemoryParams>(), 56);
    assert_eq!(std::mem::size_of::<NvRmUnmapMemoryParams>(), 32);
    assert_eq!(std::mem::size_of::<NvRmMapMemoryDmaParams>(), 64);
    assert_eq!(std::mem::size_of::<NvRmUnmapMemoryDmaParams>(), 40);
    assert_eq!(std::mem::size_of::<NvMemoryVirtualAllocParams>(), 24);
    assert_eq!(std::mem::size_of::<UvmCreateExternalRangeParams>(), 24);
    assert_eq!(std::mem::size_of::<UvmMapExternalAllocParams>(), 9264);

    // GPU_PROMOTE_CTX structs — must match the NVIDIA RM ABI exactly.
    assert_eq!(std::mem::size_of::<EngineContextBufferInfo>(), 8);
    assert_eq!(
        std::mem::size_of::<GrContextBuffersInfo>(),
        8 * ENGINE_CONTEXT_PROPERTIES_ENGINE_ID_COUNT
    );
    assert_eq!(
        std::mem::size_of::<GetContextBuffersInfoParams>(),
        8 * ENGINE_CONTEXT_PROPERTIES_ENGINE_ID_COUNT * INTERNAL_GR_MAX_ENGINES
    );
    assert_eq!(std::mem::size_of::<PromoteCtxBufferEntry>(), 32);
    // GpuPromoteCtxParams: 6×u32(24) + 2×u64(16) + u32(4) + pad(4) + 16×32(512) = 560
    assert_eq!(std::mem::size_of::<GpuPromoteCtxParams>(), 560);
}

#[test]
fn nvidia_uvm_probe() {
    let _ = nvidia_uvm_available();
}

#[test]
#[ignore = "requires proprietary nvidia driver loaded"]
fn uvm_device_opens() {
    let ctl = NvCtlDevice::open().expect("should open /dev/nvidiactl");
    assert!(ctl.fd() >= 0);
    let uvm = NvUvmDevice::open().expect("should open /dev/nvidia-uvm");
    assert!(uvm.fd() >= 0);
    let gpu = NvGpuDevice::open(0).expect("should open /dev/nvidia0");
    assert!(gpu.fd() >= 0);
}

#[test]
#[ignore = "requires proprietary nvidia driver loaded"]
fn uvm_initialize() {
    let uvm = NvUvmDevice::open().expect("open uvm");
    uvm.initialize().expect("UVM_INITIALIZE should succeed");
}
