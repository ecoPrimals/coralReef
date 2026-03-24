// SPDX-License-Identifier: AGPL-3.0-only
//! RM allocation operations — device, subdevice, vaspace, channel, memory.

use crate::error::DriverResult;

use super::super::structs::{
    Nv0080AllocParams, Nv2080AllocParams, NvChannelAllocParams, NvChannelGroupAllocParams,
    NvMemoryAllocParams, NvMemoryVirtualAllocParams, NvVaspaceAllocParams,
};
use super::super::{
    ADA_COMPUTE_A, AMPERE_CHANNEL_GPFIFO_A, AMPERE_COMPUTE_A, AMPERE_COMPUTE_B,
    BLACKWELL_COMPUTE_A, BLACKWELL_COMPUTE_B, FERMI_VASPACE_A, HOPPER_COMPUTE_A,
    KEPLER_CHANNEL_GROUP_A, NV_VASPACE_FLAGS_ENABLE_PAGE_FAULTING, NV01_DEVICE_0,
    NV01_MEMORY_LOCAL_USER, NV01_MEMORY_SYSTEM, NV01_MEMORY_VIRTUAL, NV20_SUBDEVICE_0,
    NV2080_ENGINE_TYPE_GR0, NVOS32_ALLOC_FLAGS_ALIGNMENT_FORCE,
    NVOS32_ALLOC_FLAGS_IGNORE_BANK_PLACEMENT, NVOS32_ALLOC_FLAGS_MAP_NOT_REQUIRED,
    NVOS32_ATTR_PHYSICALITY_CONTIGUOUS, NVOS32_ATTR_PHYSICALITY_NONCONTIGUOUS,
};
use super::RmClient;

impl RmClient {
    /// Allocate a device object under this client.
    ///
    /// `gpu_index` is the GPU device index (e.g. 0 for `/dev/nvidia0`).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`](crate::error::DriverError) if the `RM_ALLOC` ioctl fails.
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
    /// Returns [`DriverError`](crate::error::DriverError) if the `RM_ALLOC` ioctl fails.
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

    /// Allocate a GPU virtual address space (`FERMI_VASPACE_A`).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`](crate::error::DriverError) if the RM alloc fails.
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
    /// Returns [`DriverError`](crate::error::DriverError) if the RM alloc fails.
    pub fn alloc_vaspace_for_uvm(&mut self, h_device: u32) -> DriverResult<u32> {
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
    /// Returns [`DriverError`](crate::error::DriverError) if the RM alloc fails.
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
    /// Returns [`DriverError`](crate::error::DriverError) if the RM alloc fails.
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
    /// Returns [`DriverError`](crate::error::DriverError) if the RM alloc fails.
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
    /// Returns [`DriverError`](crate::error::DriverError) if the RM alloc fails.
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
    /// Returns [`DriverError`](crate::error::DriverError) if the RM alloc fails.
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
    /// Returns [`DriverError`](crate::error::DriverError) if the RM alloc fails.
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
                BLACKWELL_COMPUTE_B => "RM_ALLOC(BLACKWELL_COMPUTE_B)",
                BLACKWELL_COMPUTE_A => "RM_ALLOC(BLACKWELL_COMPUTE_A)",
                HOPPER_COMPUTE_A => "RM_ALLOC(HOPPER_COMPUTE_A)",
                ADA_COMPUTE_A => "RM_ALLOC(ADA_COMPUTE_A)",
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
}
