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

mod constants;
pub use constants::*;

use constants::{NV_CTL_PATH, NV_GPU_PATH_PREFIX, NV_UVM_PATH};

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
}
