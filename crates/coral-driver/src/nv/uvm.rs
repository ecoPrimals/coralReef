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

// ── NVIDIA control device ioctls (/dev/nvidiactl) ───────────────────

/// Base ioctl type for NVIDIA control device.
#[allow(dead_code, reason = "infrastructure for RM ioctl construction")]
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
#[allow(dead_code, reason = "infrastructure for RM ioctl construction")]
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
pub const NV_OK: u32 = 0x0000_0000;

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
    pub flags: u64,
    pub rm_status: u32,
    pub padding: u32,
}

/// Arguments for `UVM_REGISTER_GPU`.
#[repr(C)]
#[derive(Debug, Default)]
pub struct UvmRegisterGpuParams {
    pub gpu_uuid: [u8; 16],
    pub rm_ctrl_fd: i32,
    pub h_client: u32,
    pub h_smc_part_ref: u32,
    pub rm_status: u32,
}

/// Arguments for `NV_ESC_RM_ALLOC` (simplified).
#[repr(C)]
#[derive(Debug, Default)]
pub struct NvRmAllocParams {
    pub h_root: u32,
    pub h_object_parent: u32,
    pub h_object_new: u32,
    pub h_class: u32,
    pub p_alloc_parms: u64,
    pub params_size: u32,
    pub status: u32,
}

/// Arguments for `NV_ESC_RM_FREE`.
#[repr(C)]
#[derive(Debug, Default)]
pub struct NvRmFreeParams {
    pub h_root: u32,
    pub h_object_parent: u32,
    pub h_object_old: u32,
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
        let path = Path::new("/dev/nvidiactl");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| {
                DriverError::DeviceNotFound(format!("cannot open /dev/nvidiactl: {e}").into())
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
        let path = Path::new("/dev/nvidia-uvm");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| {
                DriverError::DeviceNotFound(format!("cannot open /dev/nvidia-uvm: {e}").into())
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
        // SAFETY: UvmInitializeParams is #[repr(C)] matching kernel's UVM_INITIALIZE
        // ioctl struct. Stack-allocated, synchronous ioctl.
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
        let path = format!("/dev/nvidia{index}");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| {
                DriverError::DeviceNotFound(format!("cannot open /dev/nvidia{index}: {e}").into())
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
    Path::new("/dev/nvidia-uvm").exists() && Path::new("/dev/nvidiactl").exists()
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
}
