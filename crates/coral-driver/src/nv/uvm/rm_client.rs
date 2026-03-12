// SPDX-License-Identifier: AGPL-3.0-only
//! RM (Resource Manager) client — allocates and manages NVIDIA GPU objects.

use crate::error::{DriverError, DriverResult};

use super::structs::*;
use super::{
    AMPERE_CHANNEL_GPFIFO_A, AMPERE_COMPUTE_A, AMPERE_COMPUTE_B, FERMI_VASPACE_A,
    KEPLER_CHANNEL_GROUP_A, NV_ESC_RM_ALLOC, NV_ESC_RM_CONTROL, NV_ESC_RM_FREE,
    NV_ESC_RM_MAP_MEMORY, NV_ESC_RM_MAP_MEMORY_DMA, NV_ESC_RM_UNMAP_MEMORY, NV_OK, NV01_DEVICE_0,
    NV01_MEMORY_LOCAL_USER, NV01_MEMORY_SYSTEM, NV01_MEMORY_VIRTUAL, NV01_ROOT_CLIENT,
    NV20_SUBDEVICE_0, NV2080_CTRL_CMD_GPU_GET_GID_INFO, NV2080_ENGINE_TYPE_GR0,
    NVOS32_ALLOC_FLAGS_ALIGNMENT_FORCE, NVOS32_ALLOC_FLAGS_IGNORE_BANK_PLACEMENT,
    NVOS32_ALLOC_FLAGS_MAP_NOT_REQUIRED, NVOS32_ATTR_PHYSICALITY_CONTIGUOUS,
    NVOS32_ATTR_PHYSICALITY_NONCONTIGUOUS, NvCtlDevice, NvUvmDevice, UVM_REGISTER_GPU, nv_ioctl_rw,
};
use crate::gsp::rm_observer::RmAllocEvent;

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
            h_root: 0,
            h_object_parent: 0,
            h_object_new: 0,
            h_class: NV01_ROOT_CLIENT,
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
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the `RM_ALLOC` ioctl fails.
    pub fn alloc_device(&mut self, gpu_index: u32) -> DriverResult<u32> {
        let h_device = self.h_client + 1 + gpu_index;

        let mut device_params = Nv0080AllocParams::default();
        device_params.device_id = gpu_index;

        let h = self.rm_alloc_typed(
            self.h_client,
            h_device,
            NV01_DEVICE_0,
            &mut device_params,
            "RM_ALLOC(NV01_DEVICE_0)",
        )?;

        tracing::info!(
            h_device = format_args!("0x{h:08X}"),
            gpu_index,
            "RM device object allocated"
        );
        Ok(h)
    }

    /// Allocate a subdevice object under a device.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the `RM_ALLOC` ioctl fails.
    pub fn alloc_subdevice(&mut self, h_device: u32) -> DriverResult<u32> {
        let h_subdevice = h_device + 0x1000;

        let mut subdev_params = Nv2080AllocParams { sub_device_id: 0 };

        let h = self.rm_alloc_typed(
            h_device,
            h_subdevice,
            NV20_SUBDEVICE_0,
            &mut subdev_params,
            "RM_ALLOC(NV20_SUBDEVICE_0)",
        )?;

        tracing::info!(
            h_subdevice = format_args!("0x{h:08X}"),
            "RM subdevice object allocated"
        );
        Ok(h)
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
            params_size: 0,
            status: 0,
        };

        let ioctl_nr = nv_ioctl_rw(NV_ESC_RM_ALLOC, std::mem::size_of::<NvRmAllocParams>());
        let t0 = std::time::Instant::now();
        // SAFETY:
        // 1. Validity:   NvRmAllocParams + T are #[repr(C)]
        // 2. Alignment:  stack-allocated, naturally aligned
        // 3. Lifetime:   synchronous ioctl; params + alloc_params outlive the call
        // 4. Exclusivity: sole mutable references
        unsafe {
            crate::drm::drm_ioctl_named(self.ctl.fd(), ioctl_nr, &mut params, label)?;
        }
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

    fn rm_alloc_simple(
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
            p_alloc_parms: 0,
            params_size: 0,
            status: 0,
        };

        let ioctl_nr = nv_ioctl_rw(NV_ESC_RM_ALLOC, std::mem::size_of::<NvRmAllocParams>());
        let t0 = std::time::Instant::now();
        // SAFETY: same contract as rm_alloc_typed, with no alloc params pointer
        unsafe {
            crate::drm::drm_ioctl_named(self.ctl.fd(), ioctl_nr, &mut params, label)?;
        }
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
        // SAFETY:
        // 1. Validity:   NvRmControlParams + T are #[repr(C)]
        // 2. Alignment:  stack-allocated, naturally aligned
        // 3. Lifetime:   synchronous ioctl; params + data outlive the call
        // 4. Exclusivity: sole mutable references
        unsafe {
            crate::drm::drm_ioctl_named(self.ctl.fd(), ioctl_nr, &mut params, label)?;
        }
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

        let len = gid.length as usize;
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

    /// Allocate a GPU virtual address space (`FERMI_VASPACE_A`).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the RM alloc fails.
    pub fn alloc_vaspace(&mut self, h_device: u32) -> DriverResult<u32> {
        self.alloc_vaspace_with_flags(h_device, 0)
    }

    /// Allocate a VA space with UVM-compatible flags.
    ///
    /// Sets `IS_EXTERNALLY_OWNED` so UVM can manage page tables,
    /// and `ENABLE_PAGE_FAULTING` for UVM page-fault handling.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the RM alloc fails.
    pub fn alloc_vaspace_for_uvm(&mut self, h_device: u32) -> DriverResult<u32> {
        use super::NV_VASPACE_FLAGS_ENABLE_PAGE_FAULTING;
        self.alloc_vaspace_with_flags(h_device, NV_VASPACE_FLAGS_ENABLE_PAGE_FAULTING)
    }

    fn alloc_vaspace_with_flags(&mut self, h_device: u32, flags: u32) -> DriverResult<u32> {
        let h_vaspace = h_device + 0x2000;

        let mut va_params = NvVaspaceAllocParams {
            flags,
            ..Default::default()
        };

        self.rm_alloc_typed(
            h_device,
            h_vaspace,
            FERMI_VASPACE_A,
            &mut va_params,
            "RM_ALLOC(FERMI_VASPACE_A)",
        )?;

        tracing::info!(
            h_vaspace = format_args!("0x{h_vaspace:08X}"),
            flags = format_args!("0x{flags:08X}"),
            "VA space allocated"
        );
        Ok(h_vaspace)
    }

    /// Allocate a channel group / TSG (`KEPLER_CHANNEL_GROUP_A`).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the RM alloc fails.
    pub fn alloc_channel_group(&mut self, h_device: u32, h_vaspace: u32) -> DriverResult<u32> {
        let h_changrp = h_device + 0x3000;

        let mut cg_params = NvChannelGroupAllocParams::default();
        cg_params.h_vaspace = h_vaspace;
        cg_params.engine_type = NV2080_ENGINE_TYPE_GR0;

        self.rm_alloc_typed(
            h_device,
            h_changrp,
            KEPLER_CHANNEL_GROUP_A,
            &mut cg_params,
            "RM_ALLOC(KEPLER_CHANNEL_GROUP_A)",
        )?;

        tracing::info!(
            h_changrp = format_args!("0x{h_changrp:08X}"),
            "Channel group allocated"
        );
        Ok(h_changrp)
    }

    /// Allocate system memory via RM (`NV01_MEMORY_SYSTEM`).
    ///
    /// The `attr` field must have `PHYSICALITY_NONCONTIGUOUS` set for
    /// system memory on 580.x GSP-RM — without it the kernel returns
    /// `NV_ERR_OPERATING_SYSTEM` (0x1F).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the RM alloc fails.
    pub fn alloc_system_memory(
        &mut self,
        h_parent: u32,
        handle: u32,
        size: u64,
    ) -> DriverResult<u32> {
        let mut mem_params = NvMemoryAllocParams {
            owner: self.h_client,
            flags: NVOS32_ALLOC_FLAGS_MAP_NOT_REQUIRED
                | NVOS32_ALLOC_FLAGS_IGNORE_BANK_PLACEMENT
                | NVOS32_ALLOC_FLAGS_ALIGNMENT_FORCE,
            attr: NVOS32_ATTR_PHYSICALITY_NONCONTIGUOUS,
            size,
            ..Default::default()
        };

        self.rm_alloc_typed(
            h_parent,
            handle,
            NV01_MEMORY_SYSTEM,
            &mut mem_params,
            "RM_ALLOC(NV01_MEMORY_SYSTEM)",
        )?;

        tracing::info!(
            handle = format_args!("0x{handle:08X}"),
            size,
            "System memory allocated via RM"
        );
        Ok(handle)
    }

    /// Allocate local (VRAM) memory via RM (`NV01_MEMORY_LOCAL_USER`).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the RM alloc fails.
    pub fn alloc_local_memory(
        &mut self,
        h_parent: u32,
        handle: u32,
        size: u64,
    ) -> DriverResult<u32> {
        let mut mem_params = NvMemoryAllocParams {
            owner: self.h_client,
            flags: NVOS32_ALLOC_FLAGS_MAP_NOT_REQUIRED
                | NVOS32_ALLOC_FLAGS_IGNORE_BANK_PLACEMENT
                | NVOS32_ALLOC_FLAGS_ALIGNMENT_FORCE,
            attr: NVOS32_ATTR_PHYSICALITY_CONTIGUOUS,
            size,
            ..Default::default()
        };

        self.rm_alloc_typed(
            h_parent,
            handle,
            NV01_MEMORY_LOCAL_USER,
            &mut mem_params,
            "RM_ALLOC(NV01_MEMORY_LOCAL_USER)",
        )?;

        tracing::info!(
            handle = format_args!("0x{handle:08X}"),
            size,
            "Local (VRAM) memory allocated via RM"
        );
        Ok(handle)
    }

    /// Allocate a virtual memory range in a GPU VA space (`NV01_MEMORY_VIRTUAL`).
    ///
    /// This creates a virtual address range within the specified VA space.
    /// The returned handle can be passed as `h_virt_mem` (= `hDma`) to
    /// [`rm_map_memory_dma`](Self::rm_map_memory_dma) to map physical memory
    /// into the GPU VA space.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the RM alloc fails.
    pub fn alloc_virtual_memory(
        &mut self,
        h_device: u32,
        handle: u32,
        h_vaspace: u32,
    ) -> DriverResult<u32> {
        let mut params = NvMemoryVirtualAllocParams {
            offset: 0,
            limit: 0,
            h_vaspace,
        };

        self.rm_alloc_typed(
            h_device,
            handle,
            NV01_MEMORY_VIRTUAL,
            &mut params,
            "RM_ALLOC(NV01_MEMORY_VIRTUAL)",
        )?;

        tracing::info!(
            handle = format_args!("0x{handle:08X}"),
            h_vaspace = format_args!("0x{h_vaspace:08X}"),
            limit = format_args!("0x{:016X}", params.limit),
            "Virtual memory range allocated in VA space"
        );
        Ok(handle)
    }

    /// Allocate a GPFIFO channel under a TSG (channel group).
    ///
    /// The channel inherits its VA space from the TSG — `hVASpace` in the
    /// alloc params must be 0 (the kernel rejects explicit VA space for
    /// TSG channels).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the RM alloc fails.
    pub fn alloc_gpfifo_channel(
        &mut self,
        h_changrp: u32,
        h_userd_mem: u32,
        gpfifo_gpu_va: u64,
        gpfifo_entries: u32,
        channel_class: u32,
    ) -> DriverResult<u32> {
        let h_channel = h_changrp + 0x100;

        // hVASpace = 0: TSG channels inherit VA space from the channel group.
        let mut chan_params = NvChannelAllocParams {
            gpfifo_offset: gpfifo_gpu_va,
            gpfifo_entries,
            engine_type: NV2080_ENGINE_TYPE_GR0,
            ..Default::default()
        };
        chan_params.h_userd_memory[0] = h_userd_mem;

        self.rm_alloc_typed(
            h_changrp,
            h_channel,
            channel_class,
            &mut chan_params,
            if channel_class == AMPERE_CHANNEL_GPFIFO_A {
                "RM_ALLOC(AMPERE_CHANNEL_GPFIFO_A)"
            } else {
                "RM_ALLOC(VOLTA_CHANNEL_GPFIFO_A)"
            },
        )?;

        tracing::info!(
            h_channel = format_args!("0x{h_channel:08X}"),
            channel_class = format_args!("0x{channel_class:04X}"),
            "GPFIFO channel allocated"
        );
        Ok(h_channel)
    }

    /// Bind a compute engine to a GPFIFO channel.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the RM alloc fails.
    pub fn alloc_compute_engine(
        &mut self,
        h_channel: u32,
        compute_class: u32,
    ) -> DriverResult<u32> {
        let h_compute = h_channel + 0x10;

        self.rm_alloc_simple(
            h_channel,
            h_compute,
            compute_class,
            match compute_class {
                AMPERE_COMPUTE_A => "RM_ALLOC(AMPERE_COMPUTE_A)",
                AMPERE_COMPUTE_B => "RM_ALLOC(AMPERE_COMPUTE_B)",
                _ => "RM_ALLOC(VOLTA_COMPUTE_A)",
            },
        )?;

        tracing::info!(
            h_compute = format_args!("0x{h_compute:08X}"),
            compute_class = format_args!("0x{compute_class:04X}"),
            "Compute engine bound to channel"
        );
        Ok(h_compute)
    }

    /// Free an RM object.
    pub fn free_object(&mut self, h_parent: u32, h_object: u32) -> DriverResult<()> {
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

        if let Some(obs) = self.observer.as_mut() {
            obs.on_free(self.h_client, h_object, params.status);
        }
        Ok(())
    }

    /// Map RM-allocated memory into user-space for CPU read/write.
    ///
    /// Returns the user-space virtual address of the mapping. The mapping
    /// is performed by the kernel via `vm_mmap` and persists until explicitly
    /// unmapped with [`rm_unmap_memory`](Self::rm_unmap_memory).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the ioctl fails or returns non-OK status.
    pub fn rm_map_memory(
        &mut self,
        h_device: u32,
        h_memory: u32,
        offset: u64,
        length: u64,
    ) -> DriverResult<u64> {
        let ctl_fd = self.ctl.fd();
        self.rm_map_memory_on_fd(ctl_fd, h_device, h_memory, offset, length)
    }

    /// Map RM memory into user-space, creating the mmap context on `mmap_target_fd`.
    ///
    /// The ioctl is always issued on the **control fd** (`/dev/nvidiactl`);
    /// `mmap_target_fd` is the fd on which the kernel creates the mmap context
    /// (used for the `nvidia_mmap` handler). On 580.x the kernel's `escape.c`
    /// expects `nv_ioctl_nvos33_parameters_with_fd` (56 bytes).
    ///
    /// After the ioctl, we `mmap()` on `mmap_target_fd` at the address the RM
    /// chose to populate the physical pages via `nvidia_mmap_helper`.
    pub fn rm_map_memory_on_fd(
        &mut self,
        mmap_target_fd: i32,
        h_device: u32,
        h_memory: u32,
        offset: u64,
        length: u64,
    ) -> DriverResult<u64> {
        let mut params = NvRmMapMemoryParams {
            h_client: self.h_client,
            h_device,
            h_memory,
            _pad: 0,
            offset,
            length,
            p_linear_address: 0,
            status: 0,
            flags: 0,
            fd: mmap_target_fd,
            _pad2: 0,
        };

        let ioctl_nr = nv_ioctl_rw(
            NV_ESC_RM_MAP_MEMORY,
            std::mem::size_of::<NvRmMapMemoryParams>(),
        );
        let ctl_fd = self.ctl.fd();

        // SAFETY: NvRmMapMemoryParams is #[repr(C)], stack-allocated, sole ref.
        let ret = unsafe { raw_nv_ioctl(ctl_fd, ioctl_nr, &mut params) };

        if ret < 0 {
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            if params.status != NV_OK {
                return Err(DriverError::SubmitFailed(
                    format!(
                        "RM_MAP_MEMORY failed: status=0x{:08X}{} h_mem=0x{h_memory:08X}",
                        params.status,
                        super::nv_status::status_name(params.status),
                    )
                    .into(),
                ));
            }
            return Err(DriverError::SubmitFailed(
                format!("RM_MAP_MEMORY ioctl errno={errno} h_mem=0x{h_memory:08X}").into(),
            ));
        }
        if params.status != NV_OK {
            return Err(DriverError::SubmitFailed(
                format!(
                    "RM_MAP_MEMORY failed: status=0x{:08X}{} h_mem=0x{h_memory:08X}",
                    params.status,
                    super::nv_status::status_name(params.status),
                )
                .into(),
            ));
        }

        // The RM reserved a VA range and created an mmap context on mmap_target_fd.
        // Now call mmap(MAP_FIXED) at that address to trigger nvidia_mmap_helper
        // which populates the physical pages.
        let rm_addr = params.p_linear_address;
        // SAFETY: mmap_target_fd is a valid open nvidia device fd. The address
        // and length were validated by the RM. MAP_FIXED replaces the
        // RM-reserved VMA with the actual page-backed mapping.
        let mapped = unsafe {
            rustix::mm::mmap(
                rm_addr as *mut std::ffi::c_void,
                length as usize,
                rustix::mm::ProtFlags::READ | rustix::mm::ProtFlags::WRITE,
                rustix::mm::MapFlags::SHARED | rustix::mm::MapFlags::FIXED,
                std::os::unix::io::BorrowedFd::borrow_raw(mmap_target_fd),
                0,
            )
        }
        .map_err(|e| {
            DriverError::SubmitFailed(
                format!(
                    "mmap after RM_MAP_MEMORY failed: {e} addr=0x{rm_addr:016X} \
                     h_mem=0x{h_memory:08X}"
                )
                .into(),
            )
        })?;

        tracing::debug!(
            h_memory = format_args!("0x{h_memory:08X}"),
            addr = format_args!("0x{mapped:?}"),
            length,
            "RM memory mapped to user-space"
        );
        Ok(mapped as u64)
    }

    /// Unmap previously CPU-mapped RM memory.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the ioctl fails.
    pub fn rm_unmap_memory(
        &mut self,
        h_device: u32,
        h_memory: u32,
        linear_address: u64,
    ) -> DriverResult<()> {
        let mut params = NvRmUnmapMemoryParams {
            h_client: self.h_client,
            h_device,
            h_memory,
            p_linear_address: linear_address,
            status: 0,
            flags: 0,
            _pad: 0,
        };

        let ioctl_nr = nv_ioctl_rw(
            NV_ESC_RM_UNMAP_MEMORY,
            std::mem::size_of::<NvRmUnmapMemoryParams>(),
        );
        // SAFETY: NvRmUnmapMemoryParams is #[repr(C)], stack-allocated, sole ref.
        let ret = unsafe { raw_nv_ioctl(self.ctl.fd(), ioctl_nr, &mut params) };
        if ret < 0 || params.status != NV_OK {
            return Err(DriverError::SubmitFailed(
                format!(
                    "RM_UNMAP_MEMORY failed: status=0x{:08X}{} h_mem=0x{h_memory:08X}",
                    params.status,
                    super::nv_status::status_name(params.status),
                )
                .into(),
            ));
        }
        Ok(())
    }

    /// Map an RM memory object into a GPU virtual address space.
    ///
    /// Uses `NV_ESC_RM_MAP_MEMORY_DMA` (NVOS46) to map `h_memory` into the
    /// virtual memory range identified by `h_virt_mem` (an `NV01_MEMORY_VIRTUAL`
    /// handle), returning the GPU virtual address.
    ///
    /// `h_virt_mem` must be an `NV01_MEMORY_VIRTUAL` handle allocated via
    /// [`alloc_virtual_memory`](Self::alloc_virtual_memory), NOT a raw
    /// `FERMI_VASPACE_A` handle.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the RM mapping fails.
    pub fn rm_map_memory_dma(
        &mut self,
        h_device: u32,
        h_virt_mem: u32,
        h_memory: u32,
        offset: u64,
        length: u64,
    ) -> DriverResult<u64> {
        let mut params = NvRmMapMemoryDmaParams {
            h_client: self.h_client,
            h_device,
            h_dma: h_virt_mem,
            h_memory,
            offset,
            length,
            ..Default::default()
        };

        let ioctl_nr = nv_ioctl_rw(
            NV_ESC_RM_MAP_MEMORY_DMA,
            std::mem::size_of::<NvRmMapMemoryDmaParams>(),
        );
        // SAFETY: NvRmMapMemoryDmaParams is #[repr(C)], stack-allocated, sole ref.
        let ret = unsafe { raw_nv_ioctl(self.ctl.fd(), ioctl_nr, &mut params) };

        if ret < 0 || params.status != NV_OK {
            return Err(DriverError::SubmitFailed(
                format!(
                    "RM_MAP_MEMORY_DMA failed: status=0x{:08X}{} h_dma=0x{h_virt_mem:08X} \
                     h_mem=0x{h_memory:08X}",
                    params.status,
                    super::nv_status::status_name(params.status),
                )
                .into(),
            ));
        }

        tracing::debug!(
            h_memory = format_args!("0x{h_memory:08X}"),
            gpu_va = format_args!("0x{:016X}", params.dma_offset),
            length,
            "RM memory mapped to GPU VA space"
        );
        Ok(params.dma_offset)
    }

    /// File descriptor for the control device (needed for UVM registration).
    #[must_use]
    pub fn ctl_fd(&self) -> i32 {
        self.ctl.fd()
    }
}

/// Parse a GID (either binary with 0x04 header or ASCII "GPU-XXXX-...")
/// into a 16-byte `NvProcessorUuid`.
fn parse_gid_to_uuid(gid: &[u8]) -> DriverResult<[u8; 16]> {
    if gid.len() >= 16 && gid[0] == 0x04 {
        let mut uuid = [0u8; 16];
        uuid.copy_from_slice(&gid[..16]);
        return Ok(uuid);
    }

    let s = std::str::from_utf8(gid)
        .map_err(|_| DriverError::SubmitFailed("GID is neither binary nor valid ASCII".into()))?;

    let hex: String = s
        .trim_start_matches("GPU-")
        .trim_end_matches('\0')
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect();

    if hex.len() < 32 {
        return Err(DriverError::SubmitFailed(
            format!("GID hex too short: {} chars from {s:?}", hex.len()).into(),
        ));
    }

    let mut uuid = [0u8; 16];
    for (i, chunk) in hex.as_bytes().chunks(2).take(16).enumerate() {
        let hi = hex_nibble(chunk[0]);
        let lo = hex_nibble(chunk[1]);
        uuid[i] = (hi << 4) | lo;
    }
    Ok(uuid)
}

fn hex_nibble(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => 10 + b - b'a',
        b'A'..=b'F' => 10 + b - b'A',
        _ => 0,
    }
}

/// Raw ioctl via C FFI, bypassing `rustix::ioctl::Ioctl` (mishandles NV_ESC_RM_*).
///
/// # Safety
///
/// `fd` must be valid; `params` must be `#[repr(C)]` and the sole mutable reference.
unsafe fn raw_nv_ioctl<T>(fd: i32, ioctl_nr: u64, params: &mut T) -> i32 {
    unsafe extern "C" {
        fn ioctl(fd: i32, request: u64, ...) -> i32;
    }
    unsafe { ioctl(fd, ioctl_nr, std::ptr::from_mut(params)) }
}

impl Drop for RmClient {
    fn drop(&mut self) {
        let _ = self.free_object(0, self.h_client);
    }
}

#[cfg(test)]
#[path = "rm_client_tests.rs"]
mod tests;
