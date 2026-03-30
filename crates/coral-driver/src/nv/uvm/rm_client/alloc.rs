// SPDX-License-Identifier: AGPL-3.0-only
//! RM allocation operations — device, subdevice, vaspace, channel, memory.

use crate::error::DriverResult;

use super::super::structs::{
    Nv0080AllocParams, Nv2080AllocParams, NvChannelAllocParams, NvChannelGroupAllocParams,
    NvCtxShareAllocParams, NvMemoryAllocParams, NvMemoryVirtualAllocParams, NvVaspaceAllocParams,
};
use super::super::{
    ADA_COMPUTE_A, AMPERE_CHANNEL_GPFIFO_A, AMPERE_COMPUTE_A, AMPERE_COMPUTE_B,
    BLACKWELL_COMPUTE_A, BLACKWELL_COMPUTE_B, FERMI_CONTEXT_SHARE_A, FERMI_VASPACE_A,
    HOPPER_COMPUTE_A, KEPLER_CHANNEL_GROUP_A, NV_VASPACE_FLAGS_ENABLE_PAGE_FAULTING, NV01_DEVICE_0,
    NV01_MEMORY_LOCAL_USER, NV01_MEMORY_SYSTEM, NV01_MEMORY_VIRTUAL, NV20_SUBDEVICE_0,
    NV2080_ENGINE_TYPE_GR0, NVOS32_ALLOC_FLAGS_ALIGNMENT_FORCE,
    NVOS32_ALLOC_FLAGS_IGNORE_BANK_PLACEMENT, NVOS32_ALLOC_FLAGS_MAP_NOT_REQUIRED,
    NVOS32_ATTR_PHYSICALITY_CONTIGUOUS, NVOS32_ATTR_PHYSICALITY_NONCONTIGUOUS,
    NVOS32_ATTR2_32BIT_ADDRESSABLE,
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

    /// Allocate a context share under a TSG (`FERMI_CONTEXT_SHARE_A`).
    ///
    /// Required on 580.x GSP-RM for channels to be properly initialized.
    /// Must be allocated before any channels in the group.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`](crate::error::DriverError) if the RM alloc fails.
    pub fn alloc_context_share(
        &mut self,
        h_changrp: u32,
        h_vaspace: u32,
        h_subdevice: u32,
    ) -> DriverResult<u32> {
        let h_ctxshare = h_changrp + 0x50;

        let mut params = NvCtxShareAllocParams {
            h_vaspace,
            flags: 0,
            h_subdevice,
        };

        self.rm_alloc_typed(
            h_changrp,
            h_ctxshare,
            FERMI_CONTEXT_SHARE_A,
            &mut params,
            "RM_ALLOC(FERMI_CONTEXT_SHARE_A)",
        )?;

        tracing::info!(
            h_ctxshare = format_args!("0x{h_ctxshare:08X}"),
            "Context share allocated under TSG"
        );
        Ok(h_ctxshare)
    }

    /// Allocate an error notifier buffer for a channel.
    ///
    /// CUDA allocates error notifiers with `NVOS32_TYPE_NOTIFIER` (13) and
    /// the device handle as owner. This is required for the channel to be
    /// properly initialized and placed on a runlist on 580.x GSP-RM.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`](crate::error::DriverError) if the RM alloc fails.
    pub fn alloc_error_notifier(&mut self, h_device: u32, handle: u32) -> DriverResult<u32> {
        let mut mem_params = NvMemoryAllocParams {
            owner: h_device,
            mem_type: 13,       // NVOS32_TYPE_NOTIFIER
            flags: 0x0000_C001, // MAP_NOT_REQUIRED | ALIGNMENT_FORCE | PERSISTENT
            attr: 0x2A80_0000,
            attr2: 0x0000_0009,
            format: 6,
            size: 4096,
            alignment: 4096,
            limit: 0xFFF,
            ..Default::default()
        };

        self.rm_alloc_typed(
            h_device,
            handle,
            NV01_MEMORY_SYSTEM,
            &mut mem_params,
            "RM_ALLOC(NV01_MEMORY_SYSTEM_NOTIFIER)",
        )?;

        tracing::info!(
            handle = format_args!("0x{handle:08X}"),
            "Error notifier allocated"
        );
        Ok(handle)
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

    /// Allocate contiguous system memory via RM.
    ///
    /// Uses `PHYSICALITY_CONTIGUOUS` to guarantee a single contiguous physical
    /// allocation. Needed for USERD pages on GV100+ where the GPU's USERD page
    /// table requires physical addresses within a limited range.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`](crate::error::DriverError) if the RM alloc fails.
    pub fn alloc_contig_system_memory(
        &mut self,
        h_parent: u32,
        handle: u32,
        size: u64,
    ) -> DriverResult<u32> {
        let mut mem_params = NvMemoryAllocParams {
            owner: self.h_client,
            flags: NVOS32_ALLOC_FLAGS_MAP_NOT_REQUIRED | NVOS32_ALLOC_FLAGS_IGNORE_BANK_PLACEMENT,
            attr: NVOS32_ATTR_PHYSICALITY_CONTIGUOUS,
            attr2: NVOS32_ATTR2_32BIT_ADDRESSABLE,
            size,
            alignment: size,
            ..Default::default()
        };

        self.rm_alloc_typed(
            h_parent,
            handle,
            NV01_MEMORY_SYSTEM,
            &mut mem_params,
            "RM_ALLOC(NV01_MEMORY_SYSTEM_CONTIG)",
        )?;

        tracing::info!(
            handle = format_args!("0x{handle:08X}"),
            size,
            "Contiguous system memory allocated via RM"
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
            flags: 0x0001_C101, // MAP_NOT_REQUIRED | IGNORE_BANK | ALIGNMENT_FORCE | PERSISTENT | KERNEL_PRIV
            attr: 0x1180_0000,  // GPU_CACHEABLE | PAGE_SIZE_HUGE
            attr2: 0x0010_0005, // PAGE_SIZE_HUGE_2MB | GPU_CACHEABLE | 32BIT_ADDRESSABLE
            format: 6,
            size,
            alignment: size,
            limit: size - 1,
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
    /// Allocate a GPFIFO channel under a TSG (channel group).
    ///
    /// Returns `(h_channel, hw_channel_id)` — the RM handle and the hardware
    /// channel ID (used as the doorbell work submit token).
    #[expect(
        clippy::too_many_arguments,
        reason = "RM API requires all channel alloc params"
    )]
    pub fn alloc_gpfifo_channel(
        &mut self,
        h_changrp: u32,
        h_userd_mem: u32,
        h_err_notif: u32,
        h_context_share: u32,
        gpfifo_gpu_va: u64,
        gpfifo_entries: u32,
        channel_class: u32,
    ) -> DriverResult<(u32, u32)> {
        let h_channel = h_changrp + 0x100;

        let mut chan_params = NvChannelAllocParams {
            gpfifo_offset: gpfifo_gpu_va,
            gpfifo_entries,
            ..Default::default()
        };
        if h_err_notif != 0 {
            chan_params.h_object_error = h_err_notif;
        }
        if h_context_share != 0 {
            chan_params.h_context_share = h_context_share;
        }
        if h_userd_mem != 0 {
            chan_params.h_userd_memory[0] = h_userd_mem;
        }

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

        let hw_cid = chan_params.cid;
        tracing::info!(
            h_channel = format_args!("0x{h_channel:08X}"),
            hw_cid,
            channel_class = format_args!("0x{channel_class:04X}"),
            "GPFIFO channel allocated"
        );
        Ok((h_channel, hw_cid))
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

    /// Bind an engine object to a channel subchannel via
    /// `NV906F_CTRL_CMD_BIND` (0x906F0101).
    ///
    /// CUDA calls this after allocating each engine under the channel.
    /// Without this, the GPU doesn't know which engine should process
    /// push buffer methods on a given subchannel.
    pub fn channel_bind_engine(
        &mut self,
        h_channel: u32,
        h_engine: u32,
        engine_class: u32,
        engine_type: u32,
    ) -> DriverResult<()> {
        #[repr(C)]
        #[derive(Debug, Default)]
        struct Nv906fBindParams {
            h_engine_object: u32,
            engine_class_1: u32,
            engine_class_2: u32,
            engine_type: u32,
        }

        let mut params = Nv906fBindParams {
            h_engine_object: h_engine,
            engine_class_1: engine_class,
            engine_class_2: engine_class,
            engine_type,
        };

        self.rm_control(
            h_channel,
            0x906f_0101, // NV906F_CTRL_CMD_BIND
            &mut params,
            "RM_CONTROL(NV906F_BIND)",
        )
    }

    /// Enable scheduling on a TSG (channel group) via RM_CONTROL.
    ///
    /// CUDA calls `NVA06C_CTRL_CMD_GPFIFO_SCHEDULE` (0xA06C0101) on the
    /// TSG to enable scheduling for all channels in the group.
    pub fn tsg_gpfifo_schedule(&mut self, h_changrp: u32) -> DriverResult<()> {
        let mut params: [u8; 3] = [1, 0, 0]; // bEnable=1
        self.rm_control(
            h_changrp,
            0xa06c_0101, // NVA06C_CTRL_CMD_GPFIFO_SCHEDULE
            &mut params,
            "RM_CONTROL(TSG_GPFIFO_SCHEDULE)",
        )
    }

    /// Query the GPFIFO work submit token for the given channel.
    ///
    /// The token is written to the doorbell register
    /// (`NV_USERMODE_NOTIFY_CHANNEL_PENDING`) to notify the GPU that
    /// new GPFIFO entries are available.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`](crate::error::DriverError) if the RM control fails.
    pub fn get_work_submit_token(&mut self, h_channel: u32) -> DriverResult<u32> {
        // CUDA uses the Volta channel base class (0xC36F) for this command,
        // not Kepler (0xA06F) or the channel's own class.
        let cmd: u32 = 0xc36f_0108;

        let mut params = super::super::structs::NvA06fGetWorkSubmitTokenParams::default();
        self.rm_control(
            h_channel,
            cmd,
            &mut params,
            "RM_CONTROL(GPFIFO_GET_WORK_SUBMIT_TOKEN)",
        )?;
        tracing::info!(
            token = format_args!("0x{:08X}", params.work_submit_token),
            cmd = format_args!("0x{cmd:08X}"),
            "Work submit token acquired"
        );
        Ok(params.work_submit_token)
    }
}
