// SPDX-License-Identifier: AGPL-3.0-only
//! RM (Resource Manager) client — allocates and manages NVIDIA GPU objects.

mod alloc;
mod memory;

use crate::error::{DriverError, DriverResult};

use super::structs::{
    Nv2080GpuGetGidInfoParams, NvRmAllocParams, NvRmControlParams, NvRmFreeParams,
    UvmRegisterGpuParams,
};
use super::{
    NV_ESC_RM_ALLOC, NV_ESC_RM_CONTROL, NV_ESC_RM_FREE, NV_OK, NV01_ROOT_CLIENT,
    NV2080_CTRL_CMD_GPU_GET_GID_INFO, NvCtlDevice, NvUvmDevice, UVM_REGISTER_GPU, nv_ioctl_rw,
};
use crate::gsp::rm_observer::RmAllocEvent;

use super::rm_helpers::parse_gid_to_uuid;

/// An RM client handle allocated via `/dev/nvidiactl`.
///
/// The RM client is the root object in the NVIDIA resource manager hierarchy.
/// All subsequent GPU resource allocations (devices, channels, memory) are
/// children of this client.
///
/// When an [`RmObserver`](crate::gsp::rm_observer::RmObserver) is attached, every RM operation is recorded for
/// the virtual GSP knowledge base.
pub struct RmClient {
    ctl: NvCtlDevice,
    h_client: u32,
    observer: Option<Box<dyn crate::gsp::rm_observer::RmObserver>>,
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
        Ok(Self {
            ctl,
            h_client,
            observer: None,
        })
    }

    /// Attach an RM protocol observer for virtual GSP learning.
    ///
    /// Every subsequent RM operation will be recorded by the observer.
    pub fn attach_observer(&mut self, obs: Box<dyn crate::gsp::rm_observer::RmObserver>) {
        self.observer = Some(obs);
    }

    /// Detach and return the observer (if any).
    pub fn detach_observer(&mut self) -> Option<Box<dyn crate::gsp::rm_observer::RmObserver>> {
        self.observer.take()
    }

    /// Allocate a new RM root client and register a GPU's FD.
    ///
    /// The `NV_ESC_REGISTER_FD` ioctl must be issued on the GPU device
    /// before allocating `NV01_DEVICE_0` objects — the RM uses this
    /// association to authorize GPU access for the client.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if any step fails.
    pub fn new_with_gpu(gpu: &super::NvGpuDevice) -> DriverResult<Self> {
        let client = Self::new()?;
        gpu.register_fd(client.ctl_fd())?;
        Ok(client)
    }

    fn alloc_root_client(ctl: &NvCtlDevice) -> DriverResult<u32> {
        let mut params = NvRmAllocParams {
            h_class: NV01_ROOT_CLIENT,
            ..Default::default()
        };

        let ioctl_nr = nv_ioctl_rw(NV_ESC_RM_ALLOC, std::mem::size_of::<NvRmAllocParams>());
        crate::drm::drm_ioctl_named(
            ctl.fd(),
            ioctl_nr,
            &mut params,
            "NV_ESC_RM_ALLOC(NV01_ROOT)",
        )?;

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

    fn rm_alloc_typed<T>(
        &mut self,
        h_parent: u32,
        h_new: u32,
        h_class: u32,
        alloc_params: &mut T,
        label: &'static str,
    ) -> DriverResult<u32> {
        let mut params = NvRmAllocParams {
            h_root: self.h_client,
            h_object_parent: h_parent,
            h_object_new: h_new,
            h_class,
            p_alloc_parms: std::ptr::from_mut(alloc_params) as u64,
            params_size: u32::try_from(std::mem::size_of::<T>()).unwrap_or(0),
            ..Default::default()
        };

        let ioctl_nr = nv_ioctl_rw(NV_ESC_RM_ALLOC, std::mem::size_of::<NvRmAllocParams>());
        let t0 = std::time::Instant::now();
        // ioctl contract: `NvRmAllocParams` + `T` match this RM escape; opcode matches.
        crate::drm::drm_ioctl_named(self.ctl.fd(), ioctl_nr, &mut params, label)?;
        let elapsed = t0.elapsed();

        if let Some(obs) = self.observer.as_mut() {
            obs.on_alloc(&RmAllocEvent {
                h_root: self.h_client,
                h_parent,
                h_new,
                h_class,
                params_size: u32::try_from(std::mem::size_of::<T>()).unwrap_or(0),
                status: params.status,
                elapsed,
            });
        }

        if params.status != NV_OK {
            return Err(DriverError::SubmitFailed(
                format!(
                    "{label} failed: status=0x{:08X}{}",
                    params.status,
                    super::nv_status::status_name(params.status),
                )
                .into(),
            ));
        }

        Ok(params.h_object_new)
    }

    /// Allocate an RM object with no params (zero-length param buffer).
    pub fn rm_alloc_simple(
        &mut self,
        h_parent: u32,
        h_new: u32,
        h_class: u32,
        label: &'static str,
    ) -> DriverResult<u32> {
        let mut params = NvRmAllocParams {
            h_root: self.h_client,
            h_object_parent: h_parent,
            h_object_new: h_new,
            h_class,
            ..Default::default()
        };

        let ioctl_nr = nv_ioctl_rw(NV_ESC_RM_ALLOC, std::mem::size_of::<NvRmAllocParams>());
        let t0 = std::time::Instant::now();
        // ioctl contract: same as `rm_alloc_typed`, with `p_alloc_parms == 0`.
        crate::drm::drm_ioctl_named(self.ctl.fd(), ioctl_nr, &mut params, label)?;
        let elapsed = t0.elapsed();

        if let Some(obs) = self.observer.as_mut() {
            obs.on_alloc(&RmAllocEvent {
                h_root: self.h_client,
                h_parent,
                h_new,
                h_class,
                params_size: 0,
                status: params.status,
                elapsed,
            });
        }

        if params.status != NV_OK {
            return Err(DriverError::SubmitFailed(
                format!(
                    "{label} failed: status=0x{:08X}{}",
                    params.status,
                    super::nv_status::status_name(params.status),
                )
                .into(),
            ));
        }

        Ok(params.h_object_new)
    }

    fn rm_control<T>(
        &mut self,
        h_object: u32,
        cmd: u32,
        data: &mut T,
        label: &'static str,
    ) -> DriverResult<()> {
        let params_size = u32::try_from(std::mem::size_of::<T>())
            .map_err(|_| DriverError::platform_overflow("control params size fits u32"))?;

        let mut params = NvRmControlParams {
            h_client: self.h_client,
            h_object,
            cmd,
            flags: 0,
            params: std::ptr::from_mut(data) as u64,
            params_size,
            status: 0,
        };

        let ioctl_nr = nv_ioctl_rw(NV_ESC_RM_CONTROL, std::mem::size_of::<NvRmControlParams>());
        let t0 = std::time::Instant::now();
        // ioctl contract: `NvRmControlParams` + `T` match this RM escape; opcode matches.
        crate::drm::drm_ioctl_named(self.ctl.fd(), ioctl_nr, &mut params, label)?;
        let elapsed = t0.elapsed();

        if let Some(obs) = self.observer.as_mut() {
            obs.on_control(self.h_client, h_object, cmd, params.status, elapsed);
        }

        if params.status != NV_OK {
            return Err(DriverError::SubmitFailed(
                format!(
                    "{label} failed: status=0x{:08X}{}",
                    params.status,
                    super::nv_status::status_name(params.status),
                )
                .into(),
            ));
        }

        Ok(())
    }

    /// Query the GPU UUID via `NV2080_CTRL_CMD_GPU_GET_GID_INFO`.
    ///
    /// Returns the raw UUID bytes (16 bytes).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the RM control call fails.
    pub fn query_gpu_uuid(&mut self, h_subdevice: u32) -> DriverResult<[u8; 16]> {
        let mut gid = Nv2080GpuGetGidInfoParams::default();

        self.rm_control(
            h_subdevice,
            NV2080_CTRL_CMD_GPU_GET_GID_INFO,
            &mut gid,
            "RM_CONTROL(GPU_GET_GID_INFO)",
        )?;

        let len = usize::try_from(gid.length)
            .map_err(|_| DriverError::platform_overflow("GID length fits in usize"))?;
        let uuid = parse_gid_to_uuid(&gid.data[..len])?;

        tracing::info!(
            uuid = format_args!(
                "GPU-{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
                uuid[0],
                uuid[1],
                uuid[2],
                uuid[3],
                uuid[4],
                uuid[5],
                uuid[6],
                uuid[7],
                uuid[8],
                uuid[9],
                uuid[10],
                uuid[11],
                uuid[12],
                uuid[13],
                uuid[14],
                uuid[15],
            ),
            "GPU UUID queried"
        );

        Ok(uuid)
    }

    /// Register a GPU with UVM using its UUID queried from RM.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the UUID query or UVM registration fails.
    pub fn register_gpu_with_uvm(
        &mut self,
        h_subdevice: u32,
        uvm: &NvUvmDevice,
    ) -> DriverResult<[u8; 16]> {
        let uuid = self.query_gpu_uuid(h_subdevice)?;

        let mut reg_params = UvmRegisterGpuParams::default();
        reg_params.gpu_uuid = uuid;
        reg_params.rm_ctrl_fd = self.ctl.fd();
        reg_params.h_client = self.h_client;

        let ret = uvm.raw_ioctl(UVM_REGISTER_GPU, &mut reg_params, "UVM_REGISTER_GPU");
        if ret.is_err() || reg_params.rm_status != NV_OK {
            return Err(DriverError::SubmitFailed(
                format!(
                    "UVM_REGISTER_GPU failed: ioctl={ret:?}, status=0x{:08X}",
                    reg_params.rm_status
                )
                .into(),
            ));
        }

        tracing::info!("GPU registered with UVM");
        Ok(uuid)
    }

    /// Free an RM object.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the `NV_ESC_RM_FREE` ioctl fails.
    pub fn free_object(&mut self, h_parent: u32, h_object: u32) -> DriverResult<()> {
        let mut params = NvRmFreeParams {
            h_root: self.h_client,
            h_object_parent: h_parent,
            h_object_old: h_object,
            status: 0,
        };

        let ioctl_nr = nv_ioctl_rw(NV_ESC_RM_FREE, std::mem::size_of::<NvRmFreeParams>());
        // ioctl contract: `NvRmFreeParams` is `#[repr(C)]` for this escape; opcode matches.
        crate::drm::drm_ioctl_named(self.ctl.fd(), ioctl_nr, &mut params, "NV_ESC_RM_FREE")?;

        if let Some(obs) = self.observer.as_mut() {
            obs.on_free(self.h_client, h_object, params.status);
        }
        Ok(())
    }

    /// File descriptor for the control device (needed for UVM registration).
    #[must_use]
    pub fn ctl_fd(&self) -> i32 {
        self.ctl.fd()
    }
}

impl Drop for RmClient {
    fn drop(&mut self) {
        let _ = self.free_object(0, self.h_client);
    }
}

#[cfg(test)]
#[path = "../rm_client_tests.rs"]
mod tests;
