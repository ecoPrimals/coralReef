// SPDX-License-Identifier: AGPL-3.0-only
//! `#[repr(C)]` struct definitions for NVIDIA RM and UVM ioctls.
//!
//! All structs derived from `nvidia-open-gpu-kernel-modules` (MIT license).
//! Each struct matches the kernel ABI layout and has compile-time size
//! assertions in the test module.

use bytemuck::Zeroable;

use super::NV_MAX_SUBDEVICES;

/// Arguments for `UVM_INITIALIZE`.
#[repr(C)]
#[derive(Debug, Default)]
pub struct UvmInitializeParams {
    /// Initialization flags.
    pub flags: u64,
    /// RM status code returned by kernel.
    pub rm_status: u32,
    /// Padding for alignment.
    pub padding: u32,
}

/// Arguments for `UVM_REGISTER_GPU` (driver 535+).
///
/// Layout (40 bytes):
/// ```text
/// 0x00  gpu_uuid       [u8; 16]
/// 0x10  numaEnabled    NvBool (u8)
/// 0x11  (pad)          3 bytes
/// 0x14  numaNodeId     i32
/// 0x18  rmCtrlFd       i32
/// 0x1C  hClient        u32
/// 0x20  hSmcPartRef    u32
/// 0x24  rmStatus       u32
/// ```
#[repr(C)]
#[derive(Debug, Default)]
pub struct UvmRegisterGpuParams {
    /// GPU UUID (16 bytes).
    pub gpu_uuid: [u8; 16],
    /// Output: NUMA enabled flag (`NvBool` = u8).
    pub numa_enabled: u8,
    _pad0: [u8; 3],
    /// Output: NUMA node ID (-1 if NUMA not enabled).
    pub numa_node_id: i32,
    /// File descriptor for RM control device.
    pub rm_ctrl_fd: i32,
    /// RM client handle.
    pub h_client: u32,
    /// SMC partition reference handle.
    pub h_smc_part_ref: u32,
    /// RM status code returned by kernel.
    pub rm_status: u32,
}

/// Arguments for `NV_ESC_RM_ALLOC` (simplified).
#[repr(C)]
#[derive(Debug, Default)]
pub struct NvRmAllocParams {
    /// Root object handle.
    pub h_root: u32,
    /// Parent object handle.
    pub h_object_parent: u32,
    /// New object handle (requested or kernel-assigned).
    pub h_object_new: u32,
    /// Object class (e.g. `NV01_ROOT`, `NV01_DEVICE_0`).
    pub h_class: u32,
    /// Pointer to allocation parameters.
    pub p_alloc_parms: u64,
    /// Size of allocation parameters.
    pub params_size: u32,
    /// Status code returned by kernel.
    pub status: u32,
}

/// Allocation parameters for `NV01_DEVICE_0` (`NV0080_ALLOC_PARAMETERS`).
///
/// The RM requires these to identify which GPU the device object targets.
/// Passing `p_alloc_parms = 0` causes `NV_ERR_OPERATING_SYSTEM` (0x59).
/// See: nvidia-open-gpu-kernel-modules `NV0080_ALLOC_PARAMETERS`.
///
/// Layout (56 bytes):
/// ```text
/// 0x00  deviceId          u32
/// 0x04  hClientShare      u32
/// 0x08  hTargetClient     u32
/// 0x0C  hTargetDevice     u32
/// 0x10  flags             u32
/// 0x14  (pad)             u32   — implicit from NV_DECLARE_ALIGNED(u64, 8)
/// 0x18  vaSpaceSize       u64
/// 0x20  vaStartInternal   u64
/// 0x28  vaLimitInternal   u64
/// 0x30  vaMode            u32
/// 0x34  (pad)             u32   — struct alignment to 8
/// ```
#[repr(C)]
#[derive(Debug, Default)]
pub struct Nv0080AllocParams {
    /// GPU device index (0 = /dev/nvidia0, 1 = /dev/nvidia1, ...).
    pub device_id: u32,
    /// Client handle for shared VA space (0 = create new).
    pub h_client_share: u32,
    /// Target client handle (0 = self).
    pub h_target_client: u32,
    /// Target device handle (0 = self).
    pub h_target_device: u32,
    /// Device allocation flags (0 = default).
    pub flags: u32,
    _pad0: u32,
    /// VA space size (0 = driver default).
    pub va_space_size: u64,
    /// VA start offset (0 = driver default).
    pub va_start_internal: u64,
    /// VA limit (0 = driver default).
    pub va_limit_internal: u64,
    /// VA mode (0 = default).
    pub va_mode: u32,
    _pad1: u32,
}

/// Allocation parameters for `NV20_SUBDEVICE_0` (`NV2080_ALLOC_PARAMETERS`).
#[repr(C)]
#[derive(Debug, Default)]
pub struct Nv2080AllocParams {
    /// Subdevice ordinal (0 for single-GPU).
    pub sub_device_id: u32,
}

/// Arguments for `NV_ESC_RM_FREE`.
#[repr(C)]
#[derive(Debug, Default)]
pub struct NvRmFreeParams {
    /// Root object handle.
    pub h_root: u32,
    /// Parent of the object to free.
    pub h_object_parent: u32,
    /// Handle of the object to free.
    pub h_object_old: u32,
    /// Status code returned by kernel.
    pub status: u32,
}

/// Arguments for `NV_ESC_RM_CONTROL`.
#[repr(C)]
#[derive(Debug, Default)]
pub struct NvRmControlParams {
    /// Client handle (`h_root` from alloc).
    pub h_client: u32,
    /// Object handle to control.
    pub h_object: u32,
    /// Control command ID.
    pub cmd: u32,
    /// Flags (0 for default).
    pub flags: u32,
    /// Pointer to command-specific parameters.
    pub params: u64,
    /// Size of parameters in bytes.
    pub params_size: u32,
    /// Status code returned by kernel.
    pub status: u32,
}

/// Parameters for `NV2080_CTRL_CMD_GPU_GET_GID_INFO`.
#[repr(C)]
#[derive(Debug)]
pub struct Nv2080GpuGetGidInfoParams {
    /// Index (0 for default).
    pub index: u32,
    /// Flags (0 = binary format).
    pub flags: u32,
    /// Output: GID length in bytes.
    pub length: u32,
    /// Output: GID data (up to 256 bytes).
    pub data: [u8; 256],
}

impl Default for Nv2080GpuGetGidInfoParams {
    fn default() -> Self {
        Self {
            index: 0,
            flags: 0,
            length: 0,
            data: [0u8; 256],
        }
    }
}

/// VA space allocation parameters for `FERMI_VASPACE_A`.
#[repr(C)]
#[derive(Debug, Default)]
pub struct NvVaspaceAllocParams {
    /// VA space index (0 for default).
    pub index: u32,
    /// Allocation flags.
    pub flags: u32,
    /// Requested VA space size (0 = driver default).
    pub va_size: u64,
    /// VA start (0 = driver default).
    pub va_start_internal: u64,
    /// VA limit (0 = driver default).
    pub va_limit_internal: u64,
    /// Big page size (0 = driver default, typically 64 KiB or 128 KiB).
    pub big_page_size: u32,
    /// Padding.
    pub pad: u32,
    /// VA base (output, filled by kernel).
    pub va_base: u64,
}

/// Channel group allocation parameters for `KEPLER_CHANNEL_GROUP_A`.
///
/// Layout (32 bytes):
/// ```text
/// 0x00  hObjectError                   u32
/// 0x04  hObjectEccError                u32
/// 0x08  hVASpace                       u32
/// 0x0C  engineType                     u32
/// 0x10  bIsCallingContextVgpuPlugin    u8 (`NvBool`)
/// 0x11  (pad)                          7 bytes
/// 0x18  pGpuGrpInfo                    u64 (NvP64, aligned to 8)
/// ```
#[repr(C)]
#[derive(Debug, Default)]
pub struct NvChannelGroupAllocParams {
    /// Error notifier context DMA handle.
    pub h_object_error: u32,
    /// ECC error notifier context DMA handle.
    pub h_object_ecc_error: u32,
    /// VA space object handle.
    pub h_vaspace: u32,
    /// Engine type (`NV2080_ENGINE_TYPE_GR0` for compute).
    pub engine_type: u32,
    /// vGPU plugin context flag (0 for normal user-space).
    pub b_is_calling_context_vgpu_plugin: u8,
    _pad0: [u8; 7],
    /// Pointer to GPU group info (0 for user-space).
    pub p_gpu_grp_info: u64,
}

/// Memory descriptor for RM structures (push buffer, USERD, etc.).
#[repr(C)]
#[derive(Debug, Default, Clone, Copy, bytemuck::Zeroable, bytemuck::Pod)]
pub struct NvMemoryDescParams {
    /// Base address.
    pub base: u64,
    /// Size in bytes.
    pub size: u64,
    /// Address space (1 = sysmem, 2 = vidmem).
    pub address_space: u32,
    /// Cache attribute.
    pub cache_attrib: u32,
}

/// Channel allocation parameters for `VOLTA_CHANNEL_GPFIFO_A`.
///
/// Matches `NV_CHANNEL_ALLOC_PARAMS` from nvidia-open-gpu-kernel-modules
/// (MIT license). `NV_MAX_SUBDEVICES = 8`.
#[repr(C)]
#[derive(Debug, bytemuck::Zeroable)]
pub struct NvChannelAllocParams {
    /// Error context DMA handle.
    pub h_object_error: u32,
    /// Buffer handle (unused, legacy).
    pub h_object_buffer: u32,
    /// Offset to beginning of GPFIFO ring.
    pub gpfifo_offset: u64,
    /// Number of GPFIFO entries (each 8 bytes).
    pub gpfifo_entries: u32,
    /// Channel flags.
    pub flags: u32,
    /// Context share handle.
    pub h_context_share: u32,
    /// VA space handle.
    pub h_vaspace: u32,
    /// USERD memory handles (one per subdevice).
    pub h_userd_memory: [u32; NV_MAX_SUBDEVICES],
    /// USERD offsets (one per subdevice).
    pub userd_offset: [u64; NV_MAX_SUBDEVICES],
    /// Engine type for this channel.
    pub engine_type: u32,
    /// Channel identifier.
    pub cid: u32,
    /// Subdevice mask.
    pub sub_device_id: u32,
    /// ECC error context DMA handle.
    pub h_object_ecc_error: u32,
    /// Instance memory descriptor.
    pub instance_mem: NvMemoryDescParams,
    /// USERD memory descriptor.
    pub userd_mem: NvMemoryDescParams,
    /// RAMFC memory descriptor.
    pub ramfc_mem: NvMemoryDescParams,
    /// Method buffer memory descriptor.
    pub mthdbuf_mem: NvMemoryDescParams,
    /// Physical channel group handle (reserved).
    pub h_phys_channel_group: u32,
    /// Internal flags (reserved).
    pub internal_flags: u32,
    /// Error notifier memory descriptor (reserved).
    pub error_notifier_mem: NvMemoryDescParams,
    /// ECC error notifier memory descriptor (reserved).
    pub ecc_error_notifier_mem: NvMemoryDescParams,
    /// Process ID (reserved).
    pub process_id: u32,
    /// Sub-process ID (reserved).
    pub sub_process_id: u32,
    /// Encrypt IV (reserved, confidential compute).
    pub encrypt_iv: [u32; 3],
    /// Decrypt IV (reserved, confidential compute).
    pub decrypt_iv: [u32; 3],
    /// HMAC nonce (reserved, confidential compute).
    pub hmac_nonce: [u32; 8],
    /// TPC configuration ID.
    pub tpc_config_id: u32,
}

impl Default for NvChannelAllocParams {
    fn default() -> Self {
        Self::zeroed()
    }
}

/// Memory allocation parameters for `NV01_MEMORY_SYSTEM` / `NV01_MEMORY_LOCAL_USER`.
///
/// Matches `NV_MEMORY_ALLOCATION_PARAMS` (FINN-generated, not publicly documented).
/// Layout reverse-engineered from CUDA driver traces on 580.119.02.
///
/// ```text
/// 0x00  owner        u32      [IN]  client or device handle
/// 0x04  mem_type     u32      [IN]  NVOS32_TYPE_* (0 = IMAGE)
/// 0x08  flags        u32      [IN]  NVOS32_ALLOC_FLAGS_*
/// 0x0C  (reserved)   u32
/// 0x10  (reserved)   u64            must be 0 unless USE_BEGIN_END
/// 0x18  attr         u32      [IN]  NVOS32_ATTR_* — bit 25 MUST be set
/// 0x1C  attr2        u32      [IN]  NVOS32_ATTR2_*
/// 0x20  format       u32      [IN]  PTE kind
/// 0x24  (reserved)   [u32; 7]       comprCovg, zcullCovg, stride, w, h, pitch, pad
/// 0x40  size         u64      [IN]  allocation size in bytes (must be > 0)
/// 0x48  alignment    u64      [IN]  requested alignment
/// 0x50  offset       u64      [OUT] allocated offset
/// 0x58  limit        u64      [OUT] size − 1
/// 0x60  (tail)       [u64; 4]       address, ranges, etc.
/// ```
#[repr(C)]
#[derive(Debug, bytemuck::Zeroable)]
pub struct NvMemoryAllocParams {
    /// Owner handle (root client or device).
    pub owner: u32,
    /// Memory type (`NVOS32_TYPE_IMAGE` = 0 for normal allocations).
    pub mem_type: u32,
    /// Allocation modifier flags (`NVOS32_ALLOC_FLAGS_*`).
    pub flags: u32,
    pub(crate) _reserved0: u32,
    pub(crate) _reserved1: u64,
    /// Surface attributes (`NVOS32_ATTR_*`). Bit 25 must be set for system memory.
    pub attr: u32,
    /// Extended attributes (`NVOS32_ATTR2_*`).
    pub attr2: u32,
    /// PTE format / kind.
    pub format: u32,
    pub(crate) _reserved2: [u32; 7],
    /// Allocation size in bytes (must be > 0).
    pub size: u64,
    /// Requested alignment in bytes.
    pub alignment: u64,
    /// Allocated offset (output, filled by RM).
    pub offset: u64,
    /// Allocation limit = size − 1 (output, filled by RM).
    pub limit: u64,
    pub(crate) _tail: [u64; 4],
}

impl Default for NvMemoryAllocParams {
    fn default() -> Self {
        Self::zeroed()
    }
}

/// Parameters for `NV_ESC_RM_MAP_MEMORY` (0x4E).
///
/// The kernel expects `nv_ioctl_nvos33_parameters_with_fd` which wraps
/// `NVOS33_PARAMETERS` with an extra `fd` field identifying the file
/// descriptor on which the mmap context is created. Total: 56 bytes.
///
/// ```text
/// 0x00  hClient          u32
/// 0x04  hDevice          u32
/// 0x08  hMemory          u32
/// 0x0C  (pad)            u32     implicit alignment for offset
/// 0x10  offset           u64
/// 0x18  length           u64
/// 0x20  pLinearAddress   u64     [OUT] user-space virtual address
/// 0x28  status           u32
/// 0x2C  flags            u32
/// 0x30  fd               i32     target fd for mmap context
/// 0x34  (pad)            u32     struct alignment to 8
/// ```
#[repr(C)]
#[derive(Debug, Default)]
pub struct NvRmMapMemoryParams {
    /// Client handle.
    pub h_client: u32,
    /// Device handle.
    pub h_device: u32,
    /// Memory object handle.
    pub h_memory: u32,
    /// Alignment padding (between hMemory and offset).
    pub pad: u32,
    /// Offset within the memory object.
    pub offset: u64,
    /// Length to map (0 = entire allocation).
    pub length: u64,
    /// Output: user-space virtual address of the mapping.
    pub p_linear_address: u64,
    /// RM status code.
    pub status: u32,
    /// Mapping flags.
    pub flags: u32,
    /// Target file descriptor for `rm_create_mmap_context`.
    pub fd: i32,
    /// Struct alignment padding to 56 bytes.
    pub pad2: u32,
}

/// Parameters for `NV_ESC_RM_UNMAP_MEMORY` (`NVOS34_PARAMETERS`).
#[repr(C)]
#[derive(Debug, Default)]
pub struct NvRmUnmapMemoryParams {
    /// Client handle.
    pub h_client: u32,
    /// Device handle.
    pub h_device: u32,
    /// Memory object handle.
    pub h_memory: u32,
    /// Alignment padding.
    pub pad: u32,
    /// User-space virtual address to unmap.
    pub p_linear_address: u64,
    /// RM status code.
    pub status: u32,
    /// Flags.
    pub flags: u32,
}

/// Parameters for `NV_ESC_RM_MAP_MEMORY_DMA` (`NVOS46_PARAMETERS`).
///
/// Maps an RM memory object into a GPU virtual address space (context DMA / VA space).
/// Returns the GPU VA in `dma_offset`.
#[repr(C)]
#[derive(Debug, Default)]
pub struct NvRmMapMemoryDmaParams {
    /// Client handle.
    pub h_client: u32,
    /// Device handle.
    pub h_device: u32,
    /// VA space (or context DMA) handle.
    pub h_dma: u32,
    /// Memory object handle.
    pub h_memory: u32,
    /// Offset within the memory object.
    pub offset: u64,
    /// Length to map.
    pub length: u64,
    /// Flags (`NVOS46_FLAGS_*`).
    pub flags: u32,
    /// Additional flags.
    pub flags2: u32,
    /// Page kind override.
    pub kind_override: u32,
    /// Padding for 8-byte alignment of `dma_offset`.
    pub pad: u32,
    /// Output: GPU virtual address of the mapping.
    /// Input if `NVOS46_FLAGS_DMA_OFFSET_FIXED` or `hDma` is not a CTXDMA.
    pub dma_offset: u64,
    /// RM status code.
    pub status: u32,
    /// Trailing padding.
    pub pad2: u32,
}

/// `UVM_REGISTER_GPU_VASPACE` parameters.
///
/// Registers an RM VA space with UVM so that external memory can be mapped
/// into GPU-visible virtual addresses. Must be called after `UVM_REGISTER_GPU`.
#[repr(C)]
#[derive(Debug, Default)]
pub struct UvmRegisterGpuVaspaceParams {
    /// GPU UUID (same 16 bytes used in `UVM_REGISTER_GPU`).
    pub gpu_uuid: [u8; 16],
    /// RM control fd (nvidiactl).
    pub rm_ctrl_fd: i32,
    /// RM client handle.
    pub h_client: u32,
    /// RM VA space handle (`FERMI_VASPACE_A` object).
    pub h_vaspace: u32,
    /// RM status code returned by kernel.
    pub rm_status: u32,
}

/// `UVM_CREATE_EXTERNAL_RANGE` parameters.
///
/// Reserves a GPU VA range in the UVM VA space for subsequent
/// `UVM_MAP_EXTERNAL_ALLOCATION` calls.
#[repr(C)]
#[derive(Debug, Default)]
pub struct UvmCreateExternalRangeParams {
    /// Base VA for the range (must be page-aligned).
    pub base: u64,
    /// Length of the range in bytes (must be page-aligned).
    pub length: u64,
    /// RM status code returned by kernel.
    pub rm_status: u32,
    /// Padding.
    pub pad: u32,
}

/// Per-GPU mapping attributes for `UVM_MAP_EXTERNAL_ALLOCATION`.
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Zeroable, bytemuck::Pod)]
pub struct UvmGpuMappingAttributes {
    /// GPU UUID (16 bytes).
    pub gpu_uuid: [u8; 16],
    /// Mapping type (`UvmGpuMappingType`).
    pub gpu_mapping_type: u32,
    /// Caching type (`UvmGpuCachingType`).
    pub gpu_caching_type: u32,
    /// Format type (`UvmGpuFormatType`).
    pub gpu_format_type: u32,
    /// Element bits (`UvmGpuFormatElementBits`).
    pub gpu_element_bits: u32,
    /// Compression type (`UvmGpuCompressionType`).
    pub gpu_compression_type: u32,
}

impl Default for UvmGpuMappingAttributes {
    fn default() -> Self {
        Self::zeroed()
    }
}

/// Parameters for `NV01_MEMORY_VIRTUAL` (class 0x70) allocation.
///
/// Allocates a virtual memory range within a GPU VA space. The returned
/// handle can be used as `hDma` in `NV_ESC_RM_MAP_MEMORY_DMA` to map
/// physical memory into the GPU VA space.
#[repr(C)]
#[derive(Debug, Default)]
pub struct NvMemoryVirtualAllocParams {
    /// Offset into the VA space (GPU VA hint / lower bound).
    pub offset: u64,
    /// Limit of the VA range. 0 = use maximum.
    pub limit: u64,
    /// VA space handle (`FERMI_VASPACE_A`). 0 = default VA space.
    pub h_vaspace: u32,
}

/// `UVM_MAX_GPUS` = `NV_MAX_DEVICES` (32) * `UVM_PARENT_ID_MAX_SUB_PROCESSORS` (8).
pub const UVM_MAX_GPUS: usize = 256;

/// `UVM_MAP_EXTERNAL_ALLOCATION` parameters.
///
/// Matches the kernel's `UVM_MAP_EXTERNAL_ALLOCATION_PARAMS` from
/// `uvm_ioctl.h` (580.x).
#[repr(C)]
#[derive(Debug, bytemuck::Zeroable)]
pub struct UvmMapExternalAllocParams {
    /// Base VA for the mapping.
    pub base: u64,
    /// Mapping length.
    pub length: u64,
    /// Offset within the RM allocation.
    pub offset: u64,
    /// Per-GPU mapping attributes array.
    pub per_gpu_attributes: [UvmGpuMappingAttributes; UVM_MAX_GPUS],
    /// Number of valid entries in `per_gpu_attributes`.
    pub gpu_attributes_count: u64,
    /// RM control fd.
    pub rm_ctrl_fd: i32,
    /// RM client handle.
    pub h_client: u32,
    /// RM memory handle.
    pub h_memory: u32,
    /// RM status.
    pub rm_status: u32,
}

impl Default for UvmMapExternalAllocParams {
    fn default() -> Self {
        Self::zeroed()
    }
}
