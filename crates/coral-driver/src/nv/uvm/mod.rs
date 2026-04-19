// SPDX-License-Identifier: AGPL-3.0-or-later
//! NVIDIA UVM (Unified Virtual Memory) interface for proprietary driver compute.
//!
//! The nvidia-drm render node (`/dev/dri/renderD*`) provides device identification
//! but not buffer management or compute dispatch. For GPU compute on the proprietary
//! driver, NVIDIA's UVM subsystem is required:
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────┐    ┌──────────────┐    ┌──────────────────┐
//! │ /dev/nvidia0 │    │ /dev/nvidiactl│    │ /dev/nvidia-uvm  │
//! │  (device)    │    │  (control)    │    │ (virtual memory)  │
//! └──────┬──────┘    └──────┬───────┘    └──────────┬───────┘
//!        │                  │                       │
//!        ▼                  ▼                       ▼
//!   RM client         RM control            UVM allocation
//!   Channel alloc     Object mgmt           GPU VA mapping
//!   Work submit       Driver caps           Page migration
//! ```
//!
//! ## Ioctl sources
//!
//! Definitions derived from NVIDIA open-gpu-kernel-modules (MIT license):
//! - `kernel-open/nvidia-uvm/uvm_linux_ioctl.h`
//! - `kernel-open/common/inc/nv-ioctl-numbers.h`
//! - `kernel-open/common/inc/nv-ioctl.h`

pub(crate) mod rm_client;
mod rm_helpers;
pub mod structs;

pub use rm_client::RmClient;
pub use structs::*;

use crate::error::{DriverError, DriverResult};
use std::fs::{File, OpenOptions};
use std::os::unix::io::{AsRawFd, RawFd};
use std::path::Path;

// ── Linux kernel ABI device paths ───────────────────────────────────

/// NVIDIA control device — RM client allocation and GPU management.
const NV_CTL_PATH: &str = "/dev/nvidiactl";
/// NVIDIA UVM device — unified virtual memory allocation and dispatch.
const NV_UVM_PATH: &str = "/dev/nvidia-uvm";
/// Format prefix for GPU device nodes (`/dev/nvidia0`, `/dev/nvidia1`, ...).
const NV_GPU_PATH_PREFIX: &str = "/dev/nvidia";

// ── NVIDIA control device ioctls (/dev/nvidiactl) ───────────────────

/// Base ioctl type for NVIDIA control device.
const NV_IOCTL_MAGIC: u8 = b'F';

/// Register a GPU file descriptor with the RM client.
///
/// From `nv-ioctl-numbers.h`: `NV_ESC_REGISTER_FD = NV_IOCTL_BASE + 1 = 201`.
pub const NV_ESC_REGISTER_FD: u32 = 201;

/// Allocate an RM resource (client, device, channel, etc.).
pub const NV_ESC_RM_ALLOC: u32 = 0x2B;

/// Perform a control operation on an RM resource.
pub const NV_ESC_RM_CONTROL: u32 = 0x2A;

/// Free an RM resource.
pub const NV_ESC_RM_FREE: u32 = 0x29;

/// Map RM memory into user-space CPU address space (uses `nv_ioctl_nvos33_parameters_with_fd`).
pub const NV_ESC_RM_MAP_MEMORY: u32 = 0x4E;

/// Unmap previously CPU-mapped RM memory.
pub const NV_ESC_RM_UNMAP_MEMORY: u32 = 0x4F;

/// Map RM memory into GPU virtual address space (DMA mapping).
pub const NV_ESC_RM_MAP_MEMORY_DMA: u32 = 0x57;

/// `NVOS46_FLAGS_SHADER_ACCESS` (bits 7:6) = READ_WRITE (3).
///
/// Enables GPU shader instructions (LDC, LDG, STG, etc.) to access the
/// DMA mapping. Without this, shader reads silently return zero.
pub const NVOS46_FLAGS_SHADER_ACCESS_READ_WRITE: u32 = 3 << 6;

/// Unmap GPU VA mapping.
pub const NV_ESC_RM_UNMAP_MEMORY_DMA: u32 = 0x58;

/// Construct an NV ioctl number (read-write direction).
pub(crate) const fn nv_ioctl_rw(nr: u32, size: usize) -> u64 {
    let dir = (crate::drm::IOC_READ | crate::drm::IOC_WRITE) as u64;
    (dir << crate::drm::IOC_DIRSHIFT as u64)
        | ((NV_IOCTL_MAGIC as u64) << crate::drm::IOC_TYPESHIFT as u64)
        | ((nr as u64) << crate::drm::IOC_NRSHIFT as u64)
        | ((size as u64) << crate::drm::IOC_SIZESHIFT as u64)
}

// ── UVM ioctls (/dev/nvidia-uvm) ────────────────────────────────────
//
// `UVM_INITIALIZE` lives in `uvm_linux_ioctl.h` with the legacy 0x30000000
// prefix. ALL other UVM ioctls live in `uvm_ioctl.h` where the Linux
// definition of `UVM_IOCTL_BASE(i)` is simply `i` (plain integer).

/// Initialize the UVM context — from `uvm_linux_ioctl.h` (legacy prefix).
pub const UVM_INITIALIZE: u32 = 0x3000_0001;

/// Register a GPU with the UVM driver.
pub const UVM_REGISTER_GPU: u32 = 37;

/// Unregister a GPU from UVM.
pub const UVM_UNREGISTER_GPU: u32 = 38;

/// Pageable memory access (enable unified address space).
pub const UVM_PAGEABLE_MEM_ACCESS: u32 = 39;

/// Create a VA range group.
pub const UVM_CREATE_RANGE_GROUP: u32 = 23;

/// Register a GPU VA space with UVM.
pub const UVM_REGISTER_GPU_VASPACE: u32 = 25;

/// Create an external VA range for mapping.
pub const UVM_CREATE_EXTERNAL_RANGE: u32 = 73;

/// Map an external (RM-allocated) buffer into the UVM VA space.
pub const UVM_MAP_EXTERNAL_ALLOCATION: u32 = 33;

/// Free a UVM allocation.
pub const UVM_FREE: u32 = 34;

/// Unmap an external allocation.
pub const UVM_UNMAP_EXTERNAL: u32 = 66;

/// `NV_STATUS` codes from `nvstatuscodes.h` (580.119.02).
///
/// Canonical values from the NVIDIA open kernel modules.
pub mod nv_status {
    /// Operation succeeded.
    pub const NV_OK: u32 = 0x0000_0000;
    /// Caller lacks required permissions.
    pub const NV_ERR_INSUFFICIENT_PERMISSIONS: u32 = 0x0000_001B;
    /// Invalid access type for the operation.
    pub const NV_ERR_INVALID_ACCESS_TYPE: u32 = 0x0000_001D;
    /// GPU or CPU virtual address is invalid.
    pub const NV_ERR_INVALID_ADDRESS: u32 = 0x0000_001E;
    /// Invalid argument passed to RM.
    pub const NV_ERR_INVALID_ARGUMENT: u32 = 0x0000_001F;
    /// Object class not recognized or unsupported.
    pub const NV_ERR_INVALID_CLASS: u32 = 0x0000_0022;
    /// RM client handle is invalid.
    pub const NV_ERR_INVALID_CLIENT: u32 = 0x0000_0023;
    /// Device object handle is invalid.
    pub const NV_ERR_INVALID_DEVICE: u32 = 0x0000_0026;
    /// Flags parameter contains invalid bits.
    pub const NV_ERR_INVALID_FLAGS: u32 = 0x0000_0029;
    /// Size or limit parameter is out of range.
    pub const NV_ERR_INVALID_LIMIT: u32 = 0x0000_002E;
    /// Object is in an invalid state for this operation.
    pub const NV_ERR_INVALID_OBJECT: u32 = 0x0000_0031;
    /// Object handle not found in the RM hierarchy.
    pub const NV_ERR_INVALID_OBJECT_HANDLE: u32 = 0x0000_0033;
    /// Object parent is incorrect for this allocation.
    pub const NV_ERR_INVALID_OBJECT_PARENT: u32 = 0x0000_0036;
    /// Generic parameter validation failure.
    pub const NV_ERR_INVALID_PARAMETER: u32 = 0x0000_003B;
    /// Object or subsystem is in an unexpected state.
    pub const NV_ERR_INVALID_STATE: u32 = 0x0000_0040;
    /// Insufficient GPU or system memory.
    pub const NV_ERR_NO_MEMORY: u32 = 0x0000_0051;
    /// Requested feature or class is not supported.
    pub const NV_ERR_NOT_SUPPORTED: u32 = 0x0000_0056;
    /// Object not found during lookup.
    pub const NV_ERR_OBJECT_NOT_FOUND: u32 = 0x0000_0057;
    /// Kernel-level error (OS interaction failed).
    pub const NV_ERR_OPERATING_SYSTEM: u32 = 0x0000_0059;

    /// Human-readable suffix for common RM status codes.
    #[must_use]
    pub const fn status_name(status: u32) -> &'static str {
        match status {
            NV_ERR_INSUFFICIENT_PERMISSIONS => " (INSUFFICIENT_PERMISSIONS)",
            NV_ERR_INVALID_ACCESS_TYPE => " (INVALID_ACCESS_TYPE)",
            NV_ERR_INVALID_ADDRESS => " (INVALID_ADDRESS)",
            NV_ERR_INVALID_ARGUMENT => " (INVALID_ARGUMENT)",
            NV_ERR_INVALID_CLASS => " (INVALID_CLASS)",
            NV_ERR_INVALID_CLIENT => " (INVALID_CLIENT)",
            NV_ERR_INVALID_DEVICE => " (INVALID_DEVICE)",
            NV_ERR_INVALID_FLAGS => " (INVALID_FLAGS)",
            NV_ERR_INVALID_LIMIT => " (INVALID_LIMIT)",
            NV_ERR_INVALID_OBJECT => " (INVALID_OBJECT)",
            NV_ERR_INVALID_OBJECT_HANDLE => " (INVALID_OBJECT_HANDLE)",
            NV_ERR_INVALID_OBJECT_PARENT => " (INVALID_OBJECT_PARENT)",
            NV_ERR_INVALID_PARAMETER => " (INVALID_PARAMETER)",
            NV_ERR_INVALID_STATE => " (INVALID_STATE)",
            NV_ERR_NO_MEMORY => " (NO_MEMORY)",
            NV_ERR_NOT_SUPPORTED => " (NOT_SUPPORTED)",
            NV_ERR_OBJECT_NOT_FOUND => " (OBJECT_NOT_FOUND)",
            NV_ERR_OPERATING_SYSTEM => " (OPERATING_SYSTEM)",
            _ => "",
        }
    }
}
pub use nv_status::*;

// ── RM allocation classes ───────────────────────────────────────────

/// `NV01_ROOT` — RM root client object (privileged).
pub const NV01_ROOT: u32 = 0x0000_0000;

/// `NV01_ROOT_CLIENT` — RM root client object (user-space).
pub const NV01_ROOT_CLIENT: u32 = 0x0000_0041;

/// `NV01_DEVICE_0` — GPU device object.
pub const NV01_DEVICE_0: u32 = 0x0000_0080;

/// `NV20_SUBDEVICE_0` — subdevice for GPU control.
pub const NV20_SUBDEVICE_0: u32 = 0x0000_2080;

/// `FERMI_VASPACE_A` — GPU virtual address space object.
pub const FERMI_VASPACE_A: u32 = 0x0000_90F1;

/// `FERMI_CONTEXT_SHARE_A` — GPU context share for TSG channels.
///
/// Must be allocated under the channel group (TSG) before any channels are
/// created. Required on 580.x GSP-RM for channels to be properly initialized
/// and placed on a runlist.
pub const FERMI_CONTEXT_SHARE_A: u32 = 0x0000_9067;

/// VA space flag: enable replayable faults at the RM/hardware level.
///
/// When set, MMU faults in this VA space are replayable rather than fatal.
/// Required on Blackwell GSP-RM where GR context buffers are demand-paged:
/// the SM's first access triggers a replayable fault that GSP services,
/// rather than a fatal "Invalid Address Space" (ESR 0x10).
pub const NV_VASPACE_FLAGS_ENABLE_FAULTING: u32 = 0x0000_0004;

/// VA space flag: page faulting enabled (required for UVM managed pages).
pub const NV_VASPACE_FLAGS_ENABLE_PAGE_FAULTING: u32 = 0x0000_0040;

/// VA space flag: externally owned (UVM manages page tables, not RM).
pub const NV_VASPACE_FLAGS_IS_EXTERNALLY_OWNED: u32 = 0x0000_0020;

/// `KEPLER_CHANNEL_GROUP_A` — Channel group (TSG) object.
pub const KEPLER_CHANNEL_GROUP_A: u32 = 0x0000_A06C;

/// `VOLTA_CHANNEL_GPFIFO_A` — GPFIFO channel for Volta.
pub const VOLTA_CHANNEL_GPFIFO_A: u32 = 0x0000_C36F;

/// `AMPERE_CHANNEL_GPFIFO_A` — GPFIFO channel for Ampere.
pub const AMPERE_CHANNEL_GPFIFO_A: u32 = 0x0000_C56F;

/// `VOLTA_COMPUTE_A` — Volta+ compute channel class.
pub const VOLTA_COMPUTE_A: u32 = 0x0000_C3C0;

/// `TURING_COMPUTE_A` — Turing compute channel class.
pub const TURING_COMPUTE_A: u32 = 0x0000_C5C0;

/// `AMPERE_COMPUTE_A` — Ampere compute class (GA100 / SM 8.0).
pub const AMPERE_COMPUTE_A: u32 = 0x0000_C6C0;

/// `AMPERE_COMPUTE_B` — Ampere compute class (`GA10x` / SM 8.6+).
pub const AMPERE_COMPUTE_B: u32 = 0x0000_C7C0;

/// `ADA_COMPUTE_A` — Ada Lovelace compute class (AD10x / SM 8.9).
pub const ADA_COMPUTE_A: u32 = 0x0000_C9C0;

/// `HOPPER_COMPUTE_A` — Hopper compute class (GH100 / SM 9.0).
pub const HOPPER_COMPUTE_A: u32 = 0x0000_CBC0;

/// `BLACKWELL_COMPUTE_A` — Blackwell compute class (GB100/200 data center, SM 10.0).
pub const BLACKWELL_COMPUTE_A: u32 = 0x0000_CDC0;

/// `BLACKWELL_COMPUTE_B` — Blackwell compute class (GB20x consumer, SM 12.0).
pub const BLACKWELL_COMPUTE_B: u32 = 0x0000_CEC0;

/// `BLACKWELL_CHANNEL_GPFIFO_A` — GPFIFO channel for Blackwell (data center).
pub const BLACKWELL_CHANNEL_GPFIFO_A: u32 = 0x0000_C96F;

/// `BLACKWELL_CHANNEL_GPFIFO_B` — GPFIFO channel for Blackwell (consumer).
pub const BLACKWELL_CHANNEL_GPFIFO_B: u32 = 0x0000_CA6F;

/// `NV01_MEMORY_SYSTEM` — System memory allocation via RM.
pub const NV01_MEMORY_SYSTEM: u32 = 0x0000_003E;

/// `NV01_MEMORY_LOCAL_USER` — Local (VRAM) memory allocation.
pub const NV01_MEMORY_LOCAL_USER: u32 = 0x0000_0040;

/// `NV01_MEMORY_VIRTUAL` — Virtual memory range in a GPU VA space.
///
/// Used as an intermediary for `MAP_MEMORY_DMA`: allocate this under a device
/// with `hVASpace` pointing to a `FERMI_VASPACE_A`, then pass this handle as
/// `hDma` in `NV_ESC_RM_MAP_MEMORY_DMA`.
pub const NV01_MEMORY_VIRTUAL: u32 = 0x0000_0070;

// ── NVOS32_ATTR_* — surface attribute bits for NV_MEMORY_ALLOCATION_PARAMS ──

/// `NVOS32_ATTR_PHYSICALITY_NONCONTIGUOUS` (bits 26:25 = 01).
/// Required for system memory allocations on 580.x GSP-RM.
pub const NVOS32_ATTR_PHYSICALITY_NONCONTIGUOUS: u32 = 0x0200_0000;

/// `NVOS32_ATTR_PHYSICALITY_CONTIGUOUS` (bits 26:25 = 10).
pub const NVOS32_ATTR_PHYSICALITY_CONTIGUOUS: u32 = 0x0400_0000;

/// `NVOS32_ATTR2_32BIT_ADDRESSABLE` — force DMA address below 4 GiB.
///
/// Required for allocations that the GPU accesses via limited-width DMA
/// address fields (e.g., USERD page table on GV100+).
pub const NVOS32_ATTR2_32BIT_ADDRESSABLE: u32 = 0x0000_0001;

// ── NVOS32_ALLOC_FLAGS_* — allocation modifier flags ────────────────

/// Don't require a CPU mapping at alloc time.
pub const NVOS32_ALLOC_FLAGS_MAP_NOT_REQUIRED: u32 = 0x0000_0001;

/// Ignore bank placement hints.
pub const NVOS32_ALLOC_FLAGS_IGNORE_BANK_PLACEMENT: u32 = 0x0000_4000;

/// Force the requested alignment.
pub const NVOS32_ALLOC_FLAGS_ALIGNMENT_FORCE: u32 = 0x0000_8000;

/// Maximum subdevices per device (from nvlimits.h).
pub const NV_MAX_SUBDEVICES: usize = 8;

/// Engine type: NULL / unspecified.
pub const NV2080_ENGINE_TYPE_NULL: u32 = 0x0000_0000;

/// Engine type: GR0 — primary graphics/compute engine.
pub const NV2080_ENGINE_TYPE_GR0: u32 = 0x0000_0001;

/// RM control command: query GPU GID (UUID).
pub const NV2080_CTRL_CMD_GPU_GET_GID_INFO: u32 = 0x2080_014A;

/// RM control command: query GR engine info (V2, inline array).
pub const NV2080_CTRL_CMD_GR_GET_INFO_V2: u32 = 0x2080_1228;

/// RM control command: set per-thread local memory size on a device.
///
/// Triggers the RM to allocate the SLM pool and configure the GR context.
/// Equivalent to `cuCtxSetLimit(CU_LIMIT_STACK_SIZE, ...)`.
/// NOTE: Returns NOT_SUPPORTED on GSP-RM (driver 580+).
pub const NV0080_CTRL_CMD_GR_SET_LOCAL_MEMORY_SIZE: u32 = 0x0080_1105;

/// RM control command: bind GR context switch state for a channel.
///
/// Called on the subdevice (NV2080) handle. This is how CUDA triggers full
/// GR context creation (including SLM pool allocation) WITHOUT allocating
/// a GR class object on the channel. The GSP firmware creates all context
/// buffers when this is called.
pub const NV2080_CTRL_CMD_GR_CTXSW_SETUP_BIND: u32 = 0x2080_123A;

/// RM control command: promote virtual context buffers to RM.
///
/// Called on the subdevice handle. Tells RM about explicitly-allocated
/// context buffers (MAIN, PATCH, BUNDLE_CB, etc.) so the GR engine can
/// use them. This is how nouveau sets up per-channel GR contexts on
/// GSP-RM — required on Blackwell where demand-paged internal buffers
/// cause `SM Warp Exception: Invalid Address Space`.
pub const NV2080_CTRL_CMD_GPU_PROMOTE_CTX: u32 = 0x2080_012B;

/// RM internal control: query GR context buffer sizes from GSP-RM.
///
/// Returns per-engine-ID size and alignment for all context buffer types.
/// Used with `engineContextBuffersInfo[0].engine[i]` for the first GR engine.
pub const NV2080_CTRL_CMD_INTERNAL_STATIC_KGR_GET_CONTEXT_BUFFERS_INFO: u32 = 0x2080_0A32;

/// Number of engine context buffer entries returned by the info query.
pub const ENGINE_CONTEXT_PROPERTIES_ENGINE_ID_COUNT: usize = 0x1a;

/// Maximum GR engines in the info query response.
pub const INTERNAL_GR_MAX_ENGINES: usize = 8;

/// Maximum entries in a single `GPU_PROMOTE_CTX` call.
pub const GPU_PROMOTE_CONTEXT_MAX_ENTRIES: usize = 16;

// ── Promote context buffer IDs ──────────────────────────────────────
//
// These correspond to `NV2080_CTRL_GPU_PROMOTE_CTX_BUFFER_ID_*` from
// the NVIDIA open headers (`ctrl2080gpu.h`).

/// Main GR context image (per-channel, needs init).
pub const PROMOTE_CTX_BUFFER_ID_MAIN: u16 = 0;
/// Performance monitoring context.
pub const PROMOTE_CTX_BUFFER_ID_PM: u16 = 1;
/// Patch context buffer (per-channel, needs init).
pub const PROMOTE_CTX_BUFFER_ID_PATCH: u16 = 2;
/// Bundle constant buffer (global).
pub const PROMOTE_CTX_BUFFER_ID_BUFFER_BUNDLE_CB: u16 = 3;
/// Page pool (global).
pub const PROMOTE_CTX_BUFFER_ID_PAGEPOOL: u16 = 4;
/// Attribute constant buffer (global, needs power-of-2 alignment).
pub const PROMOTE_CTX_BUFFER_ID_ATTRIBUTE_CB: u16 = 5;
/// RTV constant buffer (global).
pub const PROMOTE_CTX_BUFFER_ID_RTV_CB_GLOBAL: u16 = 6;
/// FECS event buffer (global, needs init).
pub const PROMOTE_CTX_BUFFER_ID_FECS_EVENT: u16 = 9;
/// Privilege access map (global, needs init, read-only, non-mapped).
pub const PROMOTE_CTX_BUFFER_ID_PRIV_ACCESS_MAP: u16 = 10;
/// Unrestricted privilege access map (global, needs init, read-only, VA-mapped).
pub const PROMOTE_CTX_BUFFER_ID_UNRESTRICTED_PRIV_ACCESS_MAP: u16 = 11;

// ── Engine context properties IDs ───────────────────────────────────
//
// Index into the `engineContextBuffersInfo[0].engine[]` array returned by
// `KGR_GET_CONTEXT_BUFFERS_INFO`. Mapped to `PROMOTE_CTX_BUFFER_ID_*` via
// the same table nouveau uses in `r535_gr_get_ctxbuf_info()`.

/// Engine ID for the main GRAPHICS context.
pub const ENGINE_CTX_ID_GRAPHICS: usize = 0x00;
/// Engine ID for GRAPHICS_PATCH.
pub const ENGINE_CTX_ID_GRAPHICS_PATCH: usize = 0x09;
/// Engine ID for GRAPHICS_BUNDLE_CB.
pub const ENGINE_CTX_ID_GRAPHICS_BUNDLE_CB: usize = 0x01;
/// Engine ID for GRAPHICS_PAGEPOOL.
pub const ENGINE_CTX_ID_GRAPHICS_PAGEPOOL: usize = 0x04;
/// Engine ID for GRAPHICS_ATTRIBUTE_CB.
pub const ENGINE_CTX_ID_GRAPHICS_ATTRIBUTE_CB: usize = 0x02;
/// Engine ID for GRAPHICS_RTV_CB_GLOBAL.
pub const ENGINE_CTX_ID_GRAPHICS_RTV_CB_GLOBAL: usize = 0x0B;
/// Engine ID for GRAPHICS_FECS_EVENT.
pub const ENGINE_CTX_ID_GRAPHICS_FECS_EVENT: usize = 0x0D;
/// Engine ID for GRAPHICS_PRIV_ACCESS_MAP.
pub const ENGINE_CTX_ID_GRAPHICS_PRIV_ACCESS_MAP: usize = 0x11;

/// GR info index: total number of SMs (streaming multiprocessors).
pub const NV2080_CTRL_GR_INFO_INDEX_SM_COUNT: u32 = 0x002B;

/// GR info index: maximum resident warps per SM.
pub const NV2080_CTRL_GR_INFO_INDEX_MAX_WARPS_PER_SM: u32 = 0x002A;

/// RM control command: get GPFIFO work submit token (channel doorbell).
pub const NVA06F_CTRL_CMD_GPFIFO_GET_WORK_SUBMIT_TOKEN: u32 = 0xA06F_0108;

/// Volta+ USERMODE class — provides user-space doorbell register access.
pub const VOLTA_USERMODE_A: u32 = 0x0000_C361;

// ── Device handles ──────────────────────────────────────────────────

/// Handle to the NVIDIA control device (`/dev/nvidiactl`).
pub struct NvCtlDevice {
    file: File,
}

impl NvCtlDevice {
    /// Open the NVIDIA control device.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::DeviceNotFound`] if `/dev/nvidiactl` cannot be opened.
    pub fn open() -> DriverResult<Self> {
        let path = Path::new(NV_CTL_PATH);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| {
                DriverError::DeviceNotFound(format!("cannot open {NV_CTL_PATH}: {e}").into())
            })?;
        Ok(Self { file })
    }

    /// Wrap an existing `File` as a control device handle.
    pub(crate) fn from_file(file: File) -> Self {
        Self { file }
    }

    /// Raw file descriptor for ioctl.
    #[must_use]
    pub fn fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
}

/// Parameters for mapping an RM-allocated memory object into a UVM external VA range.
///
/// Groups the arguments for [`NvUvmDevice::map_external_allocation`] into a
/// single named struct, improving readability and satisfying `clippy::too_many_arguments`.
#[derive(Debug, Clone)]
pub struct ExternalMapping<'a> {
    /// Start of the VA range (must be page-aligned).
    pub base: u64,
    /// Length of the mapping in bytes (must be page-aligned).
    pub length: u64,
    /// Byte offset into the RM memory object.
    pub offset: u64,
    /// File descriptor for the RM control device (`/dev/nvidiactl`).
    pub rm_ctrl_fd: i32,
    /// RM client handle that owns the memory object.
    pub h_client: u32,
    /// RM handle of the memory object to map.
    pub h_memory: u32,
    /// 16-byte GPU UUID for the target device.
    pub gpu_uuid: &'a [u8; 16],
}

/// Handle to the NVIDIA UVM device (`/dev/nvidia-uvm`).
pub struct NvUvmDevice {
    file: File,
}

impl NvUvmDevice {
    /// Open the NVIDIA UVM device.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::DeviceNotFound`] if `/dev/nvidia-uvm` cannot be opened.
    pub fn open() -> DriverResult<Self> {
        let path = Path::new(NV_UVM_PATH);
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| {
                DriverError::DeviceNotFound(format!("cannot open {NV_UVM_PATH}: {e}").into())
            })?;
        Ok(Self { file })
    }

    /// Raw file descriptor for ioctl.
    #[must_use]
    pub fn fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }

    /// Issue a raw UVM ioctl with typed parameters.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the ioctl syscall fails.
    pub fn raw_ioctl<T>(&self, cmd: u32, data: &mut T, label: &'static str) -> DriverResult<()> {
        crate::drm::drm_ioctl_named(self.fd(), u64::from(cmd), data, label)?;
        Ok(())
    }

    /// Initialize the UVM context on this file descriptor.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the `UVM_INITIALIZE` ioctl fails or
    /// returns a non-OK status.
    pub fn initialize(&self) -> DriverResult<()> {
        let mut params = UvmInitializeParams::default();
        self.raw_ioctl(UVM_INITIALIZE, &mut params, "UVM_INITIALIZE")?;
        if params.rm_status != NV_OK {
            return Err(DriverError::SubmitFailed(
                format!("UVM_INITIALIZE failed: status=0x{:08X}", params.rm_status).into(),
            ));
        }
        Ok(())
    }

    /// Query whether pageable memory access is supported and return the result.
    ///
    /// CUDA calls this during context creation on Blackwell (R580+).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the ioctl fails or returns non-OK status.
    pub fn pageable_mem_access(&self) -> DriverResult<bool> {
        let mut params = UvmPageableMemAccessParams::default();
        self.raw_ioctl(
            UVM_PAGEABLE_MEM_ACCESS,
            &mut params,
            "UVM_PAGEABLE_MEM_ACCESS",
        )?;
        if params.rm_status != NV_OK {
            return Err(DriverError::SubmitFailed(
                format!(
                    "UVM_PAGEABLE_MEM_ACCESS failed: status=0x{:08X}",
                    params.rm_status
                )
                .into(),
            ));
        }
        Ok(params.pageable_mem_access != 0)
    }

    /// Register an RM VA space with UVM.
    ///
    /// This must be called after [`RmClient::register_gpu_with_uvm`](crate::nv::uvm::RmClient::register_gpu_with_uvm) and before any
    /// `UVM_MAP_EXTERNAL_ALLOCATION` calls. It connects the RM VA space
    /// to the UVM VA space so that external memory can be GPU-mapped.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the ioctl fails or returns non-OK status.
    pub fn register_gpu_vaspace(
        &self,
        gpu_uuid: &[u8; 16],
        rm_ctrl_fd: i32,
        h_client: u32,
        h_vaspace: u32,
    ) -> DriverResult<()> {
        let mut params = UvmRegisterGpuVaspaceParams {
            gpu_uuid: *gpu_uuid,
            rm_ctrl_fd,
            h_client,
            h_vaspace,
            rm_status: 0,
        };
        self.raw_ioctl(
            UVM_REGISTER_GPU_VASPACE,
            &mut params,
            "UVM_REGISTER_GPU_VASPACE",
        )?;
        if params.rm_status != NV_OK {
            return Err(DriverError::SubmitFailed(
                format!(
                    "UVM_REGISTER_GPU_VASPACE failed: status=0x{:08X}",
                    params.rm_status
                )
                .into(),
            ));
        }
        tracing::debug!(
            h_vaspace = format_args!("0x{h_vaspace:08X}"),
            "GPU VA space registered with UVM"
        );
        Ok(())
    }

    /// Reserve a GPU VA range for subsequent external memory mappings.
    ///
    /// Both `base` and `length` must be page-aligned.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the ioctl fails or returns non-OK status.
    pub fn create_external_range(&self, base: u64, length: u64) -> DriverResult<()> {
        let mut params = UvmCreateExternalRangeParams {
            base,
            length,
            rm_status: 0,
            pad: 0,
        };
        self.raw_ioctl(
            UVM_CREATE_EXTERNAL_RANGE,
            &mut params,
            "UVM_CREATE_EXTERNAL_RANGE",
        )?;
        if params.rm_status != NV_OK {
            return Err(DriverError::SubmitFailed(
                format!(
                    "UVM_CREATE_EXTERNAL_RANGE failed: status=0x{:08X} base=0x{base:X} len=0x{length:X}",
                    params.rm_status
                )
                .into(),
            ));
        }
        tracing::debug!(
            base = format_args!("0x{base:X}"),
            length = format_args!("0x{length:X}"),
            "UVM external range created"
        );
        Ok(())
    }

    /// Map an RM-allocated memory object into a UVM external VA range.
    ///
    /// The VA range must have been previously created with
    /// [`create_external_range`](Self::create_external_range).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the ioctl fails or returns non-OK status.
    pub fn map_external_allocation(&self, mapping: &ExternalMapping<'_>) -> DriverResult<()> {
        let mut params = UvmMapExternalAllocParams {
            base: mapping.base,
            length: mapping.length,
            offset: mapping.offset,
            rm_ctrl_fd: mapping.rm_ctrl_fd,
            h_client: mapping.h_client,
            h_memory: mapping.h_memory,
            gpu_attributes_count: 1,
            ..UvmMapExternalAllocParams::default()
        };
        params.per_gpu_attributes[0].gpu_uuid = *mapping.gpu_uuid;

        self.raw_ioctl(
            UVM_MAP_EXTERNAL_ALLOCATION,
            &mut params,
            "UVM_MAP_EXTERNAL_ALLOCATION",
        )?;
        if params.rm_status != NV_OK {
            return Err(DriverError::SubmitFailed(
                format!(
                    "UVM_MAP_EXTERNAL_ALLOCATION failed: status=0x{:08X} base=0x{:X} h_mem=0x{:08X}",
                    params.rm_status, mapping.base, mapping.h_memory,
                )
                .into(),
            ));
        }
        tracing::debug!(
            base = format_args!("0x{:X}", mapping.base),
            length = format_args!("0x{:X}", mapping.length),
            h_memory = format_args!("0x{:08X}", mapping.h_memory),
            "UVM external allocation mapped"
        );
        Ok(())
    }
}

/// Handle to a specific NVIDIA GPU device (`/dev/nvidia0`, etc.).
pub struct NvGpuDevice {
    file: File,
    index: u32,
}

/// Parameters for `NV_ESC_REGISTER_FD`.
#[repr(C)]
#[derive(Debug)]
struct NvRegisterFdParams {
    ctl_fd: i32,
}

impl NvGpuDevice {
    /// Open a specific NVIDIA GPU device node.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::DeviceNotFound`] if the device cannot be opened.
    pub fn open(index: u32) -> DriverResult<Self> {
        let path = format!("{NV_GPU_PATH_PREFIX}{index}");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| DriverError::DeviceNotFound(format!("cannot open {path}: {e}").into()))?;
        Ok(Self { file, index })
    }

    /// Register this GPU's file descriptor with an RM control device.
    ///
    /// This must be called before allocating `NV01_DEVICE_0` objects — the RM
    /// uses this association to verify the client has access to the GPU.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the `NV_ESC_REGISTER_FD` ioctl fails.
    pub fn register_fd(&self, ctl_fd: RawFd) -> DriverResult<()> {
        let mut params = NvRegisterFdParams { ctl_fd };
        let ioctl_nr = nv_ioctl_rw(
            NV_ESC_REGISTER_FD,
            std::mem::size_of::<NvRegisterFdParams>(),
        );
        // ioctl contract: `NvRegisterFdParams` is `#[repr(C)]` for `NV_ESC_REGISTER_FD`.
        crate::drm::drm_ioctl_named(self.fd(), ioctl_nr, &mut params, "NV_ESC_REGISTER_FD")?;
        tracing::debug!(
            gpu_index = self.index,
            ctl_fd,
            "GPU FD registered with RM control device"
        );
        Ok(())
    }

    /// Raw file descriptor.
    #[must_use]
    pub fn fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }

    /// GPU device index.
    #[must_use]
    pub const fn index(&self) -> u32 {
        self.index
    }
}

/// Probe whether the proprietary NVIDIA driver is loaded.
#[must_use]
pub fn nvidia_uvm_available() -> bool {
    Path::new(NV_UVM_PATH).exists() && Path::new(NV_CTL_PATH).exists()
}

#[cfg(test)]
mod tests {
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
        assert_eq!(std::mem::size_of::<GrContextBuffersInfo>(), 8 * ENGINE_CONTEXT_PROPERTIES_ENGINE_ID_COUNT);
        assert_eq!(std::mem::size_of::<GetContextBuffersInfoParams>(), 8 * ENGINE_CONTEXT_PROPERTIES_ENGINE_ID_COUNT * INTERNAL_GR_MAX_ENGINES);
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
}
