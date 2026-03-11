// SPDX-License-Identifier: AGPL-3.0-only
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
//! ## Pipeline for compute dispatch
//!
//! 1. Open `/dev/nvidiactl` and create an RM client (`NV_ESC_RM_ALLOC`)
//! 2. Open `/dev/nvidia0` and attach to the GPU device
//! 3. Open `/dev/nvidia-uvm` and initialize (`UVM_INITIALIZE`)
//! 4. Register the GPU with UVM (`UVM_REGISTER_GPU`)
//! 5. Allocate GPU memory via RM and map through UVM
//! 6. Create a compute channel via RM
//! 7. Build and submit push buffer with QMD
//! 8. Fence sync via UVM semaphore
//!
//! ## Ioctl sources
//!
//! Definitions derived from NVIDIA open-gpu-kernel-modules (MIT license):
//! - `kernel-open/nvidia-uvm/uvm_linux_ioctl.h`
//! - `kernel-open/common/inc/nv-ioctl-numbers.h`
//! - `kernel-open/common/inc/nv-ioctl.h`
//!
//! ## Status
//!
//! Research phase: ioctl definitions and device infrastructure documented.
//! Full implementation requires hardware testing on a system with the
//! proprietary nvidia driver loaded (nvidia-drm on renderD*).

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

/// Register a file descriptor with the RM client.
pub const NV_ESC_REGISTER_FD: u32 = 0x21;

/// Allocate an RM resource (client, device, channel, etc.).
pub const NV_ESC_RM_ALLOC: u32 = 0x2B;

/// Perform a control operation on an RM resource.
pub const NV_ESC_RM_CONTROL: u32 = 0x2A;

/// Free an RM resource.
pub const NV_ESC_RM_FREE: u32 = 0x29;

/// Construct an NV ioctl number (read-write direction).
const fn nv_ioctl_rw(nr: u32, size: usize) -> u64 {
    let dir = (crate::drm::IOC_READ | crate::drm::IOC_WRITE) as u64;
    (dir << crate::drm::IOC_DIRSHIFT as u64)
        | ((NV_IOCTL_MAGIC as u64) << crate::drm::IOC_TYPESHIFT as u64)
        | ((nr as u64) << crate::drm::IOC_NRSHIFT as u64)
        | ((size as u64) << crate::drm::IOC_SIZESHIFT as u64)
}

// ── UVM ioctls (/dev/nvidia-uvm) ────────────────────────────────────

/// UVM ioctl base (from `uvm_linux_ioctl.h`).
const UVM_IOCTL_BASE: u32 = 0x3000_0000;

/// Initialize the UVM context for a file descriptor.
pub const UVM_INITIALIZE: u32 = UVM_IOCTL_BASE | 0x0001;

/// Pageable memory access (enable unified address space).
pub const UVM_PAGEABLE_MEM_ACCESS: u32 = UVM_IOCTL_BASE | 0x0004;

/// Register a GPU with the UVM driver.
pub const UVM_REGISTER_GPU: u32 = UVM_IOCTL_BASE | 0x0007;

/// Unregister a GPU from UVM.
pub const UVM_UNREGISTER_GPU: u32 = UVM_IOCTL_BASE | 0x0008;

/// Create a VA range for external allocation mapping.
pub const UVM_CREATE_RANGE_GROUP: u32 = UVM_IOCTL_BASE | 0x0012;

/// Map an external (RM-allocated) buffer into the UVM VA space.
pub const UVM_MAP_EXTERNAL_ALLOCATION: u32 = UVM_IOCTL_BASE | 0x0013;

/// Free a UVM allocation.
pub const UVM_FREE: u32 = UVM_IOCTL_BASE | 0x0003;

/// UVM status codes (`NV_STATUS`).
/// See: nvidia-open-gpu-kernel-modules/src/common/sdk/nvidia/inc/nvstatuscodes.h
pub const NV_OK: u32 = 0x0000_0000;
/// `NV_ERR_INVALID_ARGUMENT` — parameter rejected by RM.
pub const NV_ERR_INVALID_ARGUMENT: u32 = 0x0000_000E;
/// `NV_ERR_OPERATING_SYSTEM` — kernel/driver OS-level failure.
/// hotSpring Exp 051: RM_ALLOC(NV01_DEVICE_0) returns 0x1F on RTX 3090
/// with nvidia-drm 580.119.02. Root cause: likely missing GPU index
/// in device params, or RM access control (needs `NV_ESC_CARD_INFO` first).
pub const NV_ERR_OPERATING_SYSTEM: u32 = 0x0000_001F;
/// `NV_ERR_INVALID_OBJECT_HANDLE` — handle not found in RM.
pub const NV_ERR_INVALID_OBJECT_HANDLE: u32 = 0x0000_0036;

// ── RM allocation classes ───────────────────────────────────────────

/// `NV01_ROOT` — RM root client object.
pub const NV01_ROOT: u32 = 0x0000_0000;

/// `NV01_DEVICE_0` — GPU device object.
pub const NV01_DEVICE_0: u32 = 0x0000_0080;

/// `NV20_SUBDEVICE_0` — subdevice for GPU control.
pub const NV20_SUBDEVICE_0: u32 = 0x0000_2080;

/// `VOLTA_COMPUTE_A` — Volta+ compute channel class.
pub const VOLTA_COMPUTE_A: u32 = 0x0000_C3C0;

/// `AMPERE_COMPUTE_A` — Ampere+ compute channel class.
pub const AMPERE_COMPUTE_A: u32 = 0x0000_C6C0;

// ── Ioctl argument structures ───────────────────────────────────────

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

/// Arguments for `UVM_REGISTER_GPU`.
#[repr(C)]
#[derive(Debug, Default)]
pub struct UvmRegisterGpuParams {
    /// GPU UUID (16 bytes).
    pub gpu_uuid: [u8; 16],
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
/// Passing `p_alloc_parms = 0` causes `NV_ERR_OPERATING_SYSTEM` (0x1F).
/// See: nvidia-open-gpu-kernel-modules `NV0080_ALLOC_PARAMETERS`.
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
    /// Padding.
    pub pad: [u32; 3],
    /// VA space size (0 = driver default).
    pub va_space_size: u64,
    /// VA start offset (0 = driver default).
    pub va_start_internal: u64,
    /// VA limit (0 = driver default).
    pub va_limit_internal: u64,
    /// VA mode (0 = default).
    pub va_mode: u32,
    /// Padding.
    pub pad2: u32,
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

    /// Raw file descriptor for ioctl.
    #[must_use]
    pub fn fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
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

    /// Initialize the UVM context on this file descriptor.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the `UVM_INITIALIZE` ioctl fails or
    /// returns a non-OK status.
    pub fn initialize(&self) -> DriverResult<()> {
        let mut params = UvmInitializeParams::default();
        // SAFETY:
        // 1. Validity:   UvmInitializeParams is #[repr(C)] matching kernel UVM_INITIALIZE
        // 2. Alignment:  stack-allocated, naturally aligned
        // 3. Lifetime:   synchronous ioctl; params outlives the call
        // 4. Exclusivity: &mut params — sole reference
        unsafe {
            crate::drm::drm_ioctl_named(
                self.fd(),
                u64::from(UVM_INITIALIZE),
                &mut params,
                "UVM_INITIALIZE",
            )?;
        }
        if params.rm_status != NV_OK {
            return Err(DriverError::SubmitFailed(
                format!("UVM_INITIALIZE failed: status=0x{:08X}", params.rm_status).into(),
            ));
        }
        Ok(())
    }
}

/// Handle to a specific NVIDIA GPU device (`/dev/nvidia0`, etc.).
pub struct NvGpuDevice {
    file: File,
    index: u32,
}

impl NvGpuDevice {
    /// Open a specific NVIDIA GPU device node.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::DeviceNotFound`] if the device node cannot be opened.
    pub fn open(index: u32) -> DriverResult<Self> {
        let path = format!("{NV_GPU_PATH_PREFIX}{index}");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| {
                DriverError::DeviceNotFound(
                    format!("cannot open {NV_GPU_PATH_PREFIX}{index}: {e}").into(),
                )
            })?;
        Ok(Self { file, index })
    }

    /// Raw file descriptor for ioctl.
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

/// Probe whether the proprietary NVIDIA driver is loaded by checking
/// for the existence of UVM and control device nodes.
#[must_use]
pub fn nvidia_uvm_available() -> bool {
    Path::new(NV_UVM_PATH).exists() && Path::new(NV_CTL_PATH).exists()
}

// ── RM client allocation ────────────────────────────────────────────

/// An RM client handle allocated via `/dev/nvidiactl`.
///
/// The RM client is the root object in the NVIDIA resource manager hierarchy.
/// All subsequent GPU resource allocations (devices, channels, memory) are
/// children of this client.
pub struct RmClient {
    ctl: NvCtlDevice,
    h_client: u32,
}

impl RmClient {
    /// Allocate a new RM root client via `NV_ESC_RM_ALLOC`.
    ///
    /// This is the first step in the NVIDIA proprietary dispatch pipeline.
    /// The returned client handle is used as the root for all subsequent
    /// RM resource allocations.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if `/dev/nvidiactl` cannot be opened or
    /// the `RM_ALLOC` ioctl fails.
    pub fn new() -> DriverResult<Self> {
        let ctl = NvCtlDevice::open()?;
        let h_client = Self::alloc_root_client(&ctl)?;
        tracing::info!(
            h_client = format_args!("0x{h_client:08X}"),
            "RM root client allocated"
        );
        Ok(Self { ctl, h_client })
    }

    fn alloc_root_client(ctl: &NvCtlDevice) -> DriverResult<u32> {
        // NV01_ROOT allocation: h_root=0, parent=0, new=requested handle, class=NV01_ROOT
        // When h_object_new is 0, the kernel assigns a handle.
        let mut params = NvRmAllocParams {
            h_root: 0,
            h_object_parent: 0,
            h_object_new: 0,
            h_class: NV01_ROOT,
            p_alloc_parms: 0,
            params_size: 0,
            status: 0,
        };

        let ioctl_nr = nv_ioctl_rw(NV_ESC_RM_ALLOC, std::mem::size_of::<NvRmAllocParams>());
        // SAFETY:
        // 1. Validity:   NvRmAllocParams is #[repr(C)] matching kernel NVOS21_PARAMETERS
        // 2. Alignment:  stack-allocated, naturally aligned
        // 3. Lifetime:   synchronous ioctl; params outlives the call
        // 4. Exclusivity: &mut params — sole reference
        unsafe {
            crate::drm::drm_ioctl_named(
                ctl.fd(),
                ioctl_nr,
                &mut params,
                "NV_ESC_RM_ALLOC(NV01_ROOT)",
            )?;
        }

        if params.status != NV_OK {
            return Err(DriverError::SubmitFailed(
                format!("RM_ALLOC(NV01_ROOT) failed: status=0x{:08X}", params.status).into(),
            ));
        }

        Ok(params.h_object_new)
    }

    /// The RM client handle.
    #[must_use]
    pub const fn handle(&self) -> u32 {
        self.h_client
    }

    /// Allocate a device object under this client.
    ///
    /// `gpu_index` is the GPU device index (e.g. 0 for `/dev/nvidia0`).
    /// Passes `NV0080_ALLOC_PARAMETERS` with `device_id = gpu_index` so the
    /// RM can identify the target GPU. Without these params, RM returns
    /// `NV_ERR_OPERATING_SYSTEM` (0x1F).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the `RM_ALLOC` ioctl fails.
    pub fn alloc_device(&self, gpu_index: u32) -> DriverResult<u32> {
        let h_device = self.h_client + 1 + gpu_index;

        let mut device_params = Nv0080AllocParams {
            device_id: gpu_index,
            ..Default::default()
        };

        let params_size = u32::try_from(std::mem::size_of::<Nv0080AllocParams>())
            .map_err(|_| DriverError::platform_overflow("Nv0080AllocParams size fits u32"))?;

        let mut params = NvRmAllocParams {
            h_root: self.h_client,
            h_object_parent: self.h_client,
            h_object_new: h_device,
            h_class: NV01_DEVICE_0,
            p_alloc_parms: std::ptr::from_mut(&mut device_params) as u64,
            params_size,
            status: 0,
        };

        let ioctl_nr = nv_ioctl_rw(NV_ESC_RM_ALLOC, std::mem::size_of::<NvRmAllocParams>());
        // SAFETY:
        // 1. Validity:   NvRmAllocParams + Nv0080AllocParams are #[repr(C)]
        // 2. Alignment:  stack-allocated, naturally aligned
        // 3. Lifetime:   synchronous ioctl; params + device_params outlive the call
        // 4. Exclusivity: sole references to both
        unsafe {
            crate::drm::drm_ioctl_named(
                self.ctl.fd(),
                ioctl_nr,
                &mut params,
                "NV_ESC_RM_ALLOC(NV01_DEVICE_0)",
            )?;
        }

        if params.status != NV_OK {
            let status_name = match params.status {
                NV_ERR_INVALID_ARGUMENT => " (INVALID_ARGUMENT)",
                NV_ERR_OPERATING_SYSTEM => " (OPERATING_SYSTEM)",
                NV_ERR_INVALID_OBJECT_HANDLE => " (INVALID_OBJECT_HANDLE)",
                _ => "",
            };
            return Err(DriverError::SubmitFailed(
                format!(
                    "RM_ALLOC(NV01_DEVICE_0) failed: status=0x{:08X}{status_name}",
                    params.status
                )
                .into(),
            ));
        }

        tracing::info!(
            h_device = format_args!("0x{h_device:08X}"),
            gpu_index,
            "RM device object allocated"
        );
        Ok(h_device)
    }

    /// Allocate a subdevice object under a device.
    ///
    /// Passes `NV2080_ALLOC_PARAMETERS` with `sub_device_id = 0` (default).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the `RM_ALLOC` ioctl fails.
    pub fn alloc_subdevice(&self, h_device: u32) -> DriverResult<u32> {
        let h_subdevice = h_device + 0x1000;

        let mut subdev_params = Nv2080AllocParams { sub_device_id: 0 };

        let params_size = u32::try_from(std::mem::size_of::<Nv2080AllocParams>())
            .map_err(|_| DriverError::platform_overflow("Nv2080AllocParams size fits u32"))?;

        let mut params = NvRmAllocParams {
            h_root: self.h_client,
            h_object_parent: h_device,
            h_object_new: h_subdevice,
            h_class: NV20_SUBDEVICE_0,
            p_alloc_parms: std::ptr::from_mut(&mut subdev_params) as u64,
            params_size,
            status: 0,
        };

        let ioctl_nr = nv_ioctl_rw(NV_ESC_RM_ALLOC, std::mem::size_of::<NvRmAllocParams>());
        // SAFETY:
        // 1. Validity:   NvRmAllocParams + Nv2080AllocParams are #[repr(C)]
        // 2. Alignment:  stack-allocated, naturally aligned
        // 3. Lifetime:   synchronous ioctl; all params outlive the call
        // 4. Exclusivity: sole references
        unsafe {
            crate::drm::drm_ioctl_named(
                self.ctl.fd(),
                ioctl_nr,
                &mut params,
                "NV_ESC_RM_ALLOC(NV20_SUBDEVICE_0)",
            )?;
        }

        if params.status != NV_OK {
            let status_name = match params.status {
                NV_ERR_INVALID_ARGUMENT => " (INVALID_ARGUMENT)",
                NV_ERR_OPERATING_SYSTEM => " (OPERATING_SYSTEM)",
                NV_ERR_INVALID_OBJECT_HANDLE => " (INVALID_OBJECT_HANDLE)",
                _ => "",
            };
            return Err(DriverError::SubmitFailed(
                format!(
                    "RM_ALLOC(NV20_SUBDEVICE_0) failed: status=0x{:08X}{status_name}",
                    params.status
                )
                .into(),
            ));
        }

        tracing::info!(
            h_subdevice = format_args!("0x{h_subdevice:08X}"),
            "RM subdevice object allocated"
        );
        Ok(h_subdevice)
    }

    /// Free an RM object.
    fn free_object(&self, h_parent: u32, h_object: u32) -> DriverResult<()> {
        let mut params = NvRmFreeParams {
            h_root: self.h_client,
            h_object_parent: h_parent,
            h_object_old: h_object,
            status: 0,
        };

        let ioctl_nr = nv_ioctl_rw(NV_ESC_RM_FREE, std::mem::size_of::<NvRmFreeParams>());
        // SAFETY: same contract as alloc_root_client
        unsafe {
            crate::drm::drm_ioctl_named(self.ctl.fd(), ioctl_nr, &mut params, "NV_ESC_RM_FREE")?;
        }
        Ok(())
    }
}

impl Drop for RmClient {
    fn drop(&mut self) {
        let _ = self.free_object(0, self.h_client);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uvm_ioctl_constants_are_valid() {
        assert_eq!(UVM_INITIALIZE, 0x3000_0001);
        assert_eq!(UVM_REGISTER_GPU, 0x3000_0007);
        assert_eq!(UVM_FREE, 0x3000_0003);
    }

    #[test]
    fn rm_class_constants() {
        assert_eq!(NV01_ROOT, 0);
        assert_eq!(NV01_DEVICE_0, 0x80);
        assert_eq!(VOLTA_COMPUTE_A, 0xC3C0);
        assert_eq!(AMPERE_COMPUTE_A, 0xC6C0);
    }

    #[test]
    fn uvm_param_struct_sizes() {
        assert_eq!(
            std::mem::size_of::<UvmInitializeParams>(),
            16,
            "UvmInitializeParams should be 16 bytes"
        );
        assert_eq!(
            std::mem::size_of::<NvRmAllocParams>(),
            32,
            "NvRmAllocParams should be 32 bytes"
        );
    }

    #[test]
    fn nvidia_uvm_probe() {
        let avail = nvidia_uvm_available();
        // Just runs without panic; availability depends on host driver
        let _ = avail;
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

    #[test]
    #[ignore = "requires proprietary nvidia driver loaded"]
    fn rm_client_alloc() {
        let client = RmClient::new().expect("RM root client allocation");
        assert!(client.handle() != 0, "client handle should be non-zero");
        eprintln!("RM client handle: 0x{:08X}", client.handle());
    }

    #[test]
    #[ignore = "requires proprietary nvidia driver loaded"]
    fn rm_client_alloc_device() {
        let client = RmClient::new().expect("RM root client");
        let h_device = client.alloc_device(0).expect("RM device allocation");
        eprintln!(
            "RM client=0x{:08X}, device=0x{:08X}",
            client.handle(),
            h_device
        );
    }

    #[test]
    #[ignore = "requires proprietary nvidia driver loaded"]
    fn rm_client_alloc_subdevice() {
        let client = RmClient::new().expect("RM root client");
        let h_device = client.alloc_device(0).expect("RM device");
        let h_subdevice = client.alloc_subdevice(h_device).expect("RM subdevice");
        eprintln!(
            "RM client=0x{:08X}, device=0x{:08X}, subdevice=0x{:08X}",
            client.handle(),
            h_device,
            h_subdevice
        );
    }

    #[test]
    fn rm_alloc_params_struct_size() {
        assert_eq!(
            std::mem::size_of::<NvRmAllocParams>(),
            32,
            "NvRmAllocParams must be 32 bytes"
        );
    }

    #[test]
    fn rm_free_params_struct_size() {
        assert_eq!(
            std::mem::size_of::<NvRmFreeParams>(),
            16,
            "NvRmFreeParams must be 16 bytes"
        );
    }
}
