// SPDX-License-Identifier: AGPL-3.0-or-later
//! NVIDIA UVM and RM user-space device nodes (`/dev/nvidiactl`, `/dev/nvidia-uvm`, `/dev/nvidia*`).

use crate::error::{DriverError, DriverResult};
use std::fs::{File, OpenOptions};
use std::os::unix::io::{AsRawFd, RawFd};
use std::path::Path;

use super::constants::{
    NV_ESC_REGISTER_FD, NV_OK, UVM_CREATE_EXTERNAL_RANGE, UVM_FREE, UVM_INITIALIZE,
    UVM_MAP_EXTERNAL_ALLOCATION, UVM_MM_INITIALIZE, UVM_PAGEABLE_MEM_ACCESS,
    UVM_REGISTER_CHANNEL, UVM_REGISTER_GPU_VASPACE, UVM_UNREGISTER_GPU_VASPACE,
    nv_ctl_path, nv_gpu_path_prefix, nv_ioctl_rw, nv_uvm_path,
};
use super::structs::{
    UvmCreateExternalRangeParams, UvmFreeParams, UvmInitializeParams, UvmMapExternalAllocParams,
    UvmMmInitializeParams, UvmPageableMemAccessParams, UvmRegisterChannelParams,
    UvmRegisterGpuVaspaceParams, UvmUnregisterGpuVaspaceParams,
};

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
        let path = nv_ctl_path();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| {
                DriverError::DeviceNotFound(format!("cannot open {path}: {e}").into())
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
    /// Secondary UVM FD used for `UVM_MM_INITIALIZE` to pin the process mm.
    /// Kept open for the lifetime of the UVM context.
    mm_fd: Option<File>,
}

impl NvUvmDevice {
    /// Open the NVIDIA UVM device.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::DeviceNotFound`] if `/dev/nvidia-uvm` cannot be opened.
    pub fn open() -> DriverResult<Self> {
        let path = nv_uvm_path();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| {
                DriverError::DeviceNotFound(format!("cannot open {path}: {e}").into())
            })?;
        Ok(Self { file, mm_fd: None })
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

    /// Initialize the UVM mm association by opening a secondary UVM FD and
    /// calling `UVM_MM_INITIALIZE`. This pins the process's `mm_struct` so that
    /// `UVM_REGISTER_GPU_VASPACE` can retain page tables on systems with MMU
    /// notifiers.
    ///
    /// Returns `Ok(true)` if mm was initialized, `Ok(false)` if the platform
    /// doesn't need it (`NV_WARN_NOTHING_TO_DO`).
    pub fn mm_initialize(&mut self) -> DriverResult<bool> {
        let primary_fd = self.fd();
        let path = nv_uvm_path();
        let mm_file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| {
                DriverError::DeviceNotFound(
                    format!("cannot open secondary {path} for UVM_MM_INITIALIZE: {e}").into(),
                )
            })?;
        let mm_fd = mm_file.as_raw_fd();

        let mut params = UvmMmInitializeParams {
            uvm_fd: primary_fd,
            rm_status: 0,
        };
        crate::drm::drm_ioctl_named(
            mm_fd,
            u64::from(UVM_MM_INITIALIZE),
            &mut params,
            "UVM_MM_INITIALIZE",
        )
        .map_err(|e| {
            DriverError::SubmitFailed(format!("UVM_MM_INITIALIZE ioctl failed: {e}").into())
        })?;

        const NV_WARN_NOTHING_TO_DO: u32 = 0x0000_010B;
        if params.rm_status == NV_OK {
            self.mm_fd = Some(mm_file);
            Ok(true)
        } else if params.rm_status == NV_WARN_NOTHING_TO_DO {
            Ok(false)
        } else {
            Err(DriverError::SubmitFailed(
                format!(
                    "UVM_MM_INITIALIZE failed: status=0x{:08X}",
                    params.rm_status
                )
                .into(),
            ))
        }
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

    /// Unregister a GPU VA space from UVM.
    ///
    /// Used to clear an auto-registered default VA space (from `UVM_REGISTER_GPU`)
    /// before registering a faulting VA space explicitly.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the ioctl fails or returns non-OK status.
    pub fn unregister_gpu_vaspace(&self, gpu_uuid: &[u8; 16]) -> DriverResult<()> {
        let mut params = UvmUnregisterGpuVaspaceParams {
            gpu_uuid: *gpu_uuid,
            rm_status: 0,
        };
        self.raw_ioctl(
            UVM_UNREGISTER_GPU_VASPACE,
            &mut params,
            "UVM_UNREGISTER_GPU_VASPACE",
        )?;
        if params.rm_status != NV_OK {
            return Err(DriverError::SubmitFailed(
                format!(
                    "UVM_UNREGISTER_GPU_VASPACE failed: status=0x{:08X}",
                    params.rm_status
                )
                .into(),
            ));
        }
        tracing::debug!("GPU VA space unregistered from UVM");
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
        // ReadWriteAtomic (1) grants full GPU access. Default (0) may
        // produce a read-only mapping on Blackwell, silently dropping STG.
        params.per_gpu_attributes[0].gpu_mapping_type = 1;

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

    /// Free a UVM VA range (unmaps any external allocations and releases the range).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the ioctl fails or returns non-OK status.
    pub fn uvm_free(&self, base: u64, length: u64, gpu_uuid: &[u8; 16]) -> DriverResult<()> {
        let mut params = UvmFreeParams {
            base,
            length,
            gpu_uuid: *gpu_uuid,
            rm_status: 0,
            pad: 0,
        };
        self.raw_ioctl(UVM_FREE, &mut params, "UVM_FREE")?;
        if params.rm_status != NV_OK {
            return Err(DriverError::SubmitFailed(
                format!(
                    "UVM_FREE failed: status=0x{:08X} base=0x{base:X} len=0x{length:X}",
                    params.rm_status
                )
                .into(),
            ));
        }
        Ok(())
    }

    /// Register a channel with UVM for context buffer resource binding.
    ///
    /// On Blackwell+ with externally-owned VA spaces, RM refuses to schedule
    /// a channel that has unbound internal allocations. The UVM module resolves
    /// this by calling `nvUvmInterfaceRetainChannel` (which discovers all internal
    /// resources) and `nvUvmInterfaceBindChannelResources` (which maps and binds
    /// them into the GPU VA space) using its own kernel-internal session.
    ///
    /// `base` and `length` define the GPU VA range where UVM will map the
    /// channel's internal resources (GR context buffers, etc.).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the ioctl fails or returns non-OK status.
    pub fn register_channel(
        &self,
        gpu_uuid: &[u8; 16],
        rm_ctrl_fd: i32,
        h_client: u32,
        h_channel: u32,
        base: u64,
        length: u64,
    ) -> DriverResult<()> {
        let mut params = UvmRegisterChannelParams::default();
        params.gpu_uuid = *gpu_uuid;
        params.rm_ctrl_fd = rm_ctrl_fd;
        params.h_client = h_client;
        params.h_channel = h_channel;
        params.base = base;
        params.length = length;
        self.raw_ioctl(UVM_REGISTER_CHANNEL, &mut params, "UVM_REGISTER_CHANNEL")?;
        if params.rm_status != NV_OK {
            return Err(DriverError::SubmitFailed(
                format!(
                    "UVM_REGISTER_CHANNEL failed: status=0x{:08X} base=0x{base:X} len=0x{length:X}",
                    params.rm_status
                )
                .into(),
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
        let prefix = nv_gpu_path_prefix();
        let path = format!("{prefix}{index}");
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
    Path::new(nv_uvm_path()).exists() && Path::new(nv_ctl_path()).exists()
}
