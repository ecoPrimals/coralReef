// SPDX-License-Identifier: AGPL-3.0-or-later
//! RM allocation operations — device, subdevice, vaspace, channel, memory.

use crate::error::DriverResult;

use super::super::structs::{
    Nv0080AllocParams, Nv2080AllocParams, NvChannelGroupAllocParams, NvCtxShareAllocParams,
    NvMemoryAllocParams, NvMemoryVirtualAllocParams, NvVaspaceAllocParams,
};
use super::super::{
    FERMI_CONTEXT_SHARE_A, FERMI_VASPACE_A, KEPLER_CHANNEL_GROUP_A,
    NV_VASPACE_FLAGS_ENABLE_FAULTING, NV_VASPACE_FLAGS_ENABLE_PAGE_FAULTING, NV01_DEVICE_0,
    NV01_MEMORY_LOCAL_USER, NV01_MEMORY_SYSTEM, NV01_MEMORY_VIRTUAL, NV20_SUBDEVICE_0,
    NV2080_ENGINE_TYPE_GR0, NVOS32_ALLOC_FLAGS_ALIGNMENT_FORCE,
    NVOS32_ALLOC_FLAGS_IGNORE_BANK_PLACEMENT, NVOS32_ALLOC_FLAGS_MAP_NOT_REQUIRED,
    NVOS32_ATTR_PHYSICALITY_CONTIGUOUS, NVOS32_ATTR_PHYSICALITY_NONCONTIGUOUS,
    NVOS32_ATTR2_32BIT_ADDRESSABLE,
};
use super::RmClient;

/// Descriptor for one GR context buffer requirement.
///
/// Returned by [`RmClient::query_gr_context_buffers_info`] and used to
/// allocate + promote context buffers via `GPU_PROMOTE_CTX`.
#[derive(Debug, Clone)]
pub struct CtxBufDesc {
    /// Promote context buffer ID (`PROMOTE_CTX_BUFFER_ID_*`).
    pub buffer_id: u16,
    /// Aligned allocation size in bytes.
    pub size: u64,
    /// Required allocation alignment.
    pub alignment: u64,
    /// Whether the buffer needs zero-initialization before promotion.
    pub needs_init: bool,
    /// Whether the buffer should NOT be VA-mapped (physical address only).
    pub is_nonmapped: bool,
}

/// Hardcoded Blackwell (GB20x, SM 12.0) context buffer sizes.
///
/// Used as a fallback when `KGR_GET_CONTEXT_BUFFERS_INFO` is rejected
/// (internal RM command, kernel-only). Sizes are conservatively large —
/// RM will use at most what it needs from each buffer.
///
/// Derived from nouveau's `r535_gr_promote_ctx` patterns and typical
/// Blackwell context requirements (GB202: 5 GPCs, 20 TPCs, 40 SMs).
pub fn hardcoded_blackwell_ctx_buffers() -> Vec<CtxBufDesc> {
    use super::super::{
        PROMOTE_CTX_BUFFER_ID_ATTRIBUTE_CB, PROMOTE_CTX_BUFFER_ID_BUFFER_BUNDLE_CB,
        PROMOTE_CTX_BUFFER_ID_FECS_EVENT, PROMOTE_CTX_BUFFER_ID_MAIN,
        PROMOTE_CTX_BUFFER_ID_PAGEPOOL, PROMOTE_CTX_BUFFER_ID_PATCH,
        PROMOTE_CTX_BUFFER_ID_PRIV_ACCESS_MAP, PROMOTE_CTX_BUFFER_ID_RTV_CB_GLOBAL,
        PROMOTE_CTX_BUFFER_ID_UNRESTRICTED_PRIV_ACCESS_MAP,
    };

    vec![
        // MAIN: largest buffer, holds the GR context image.
        // Nouveau adds 64 * 0x1000 (256 KiB) for per-subctx headers.
        // 8 MiB is generous for Blackwell.
        CtxBufDesc {
            buffer_id: PROMOTE_CTX_BUFFER_ID_MAIN,
            size: 8 * 1024 * 1024, // 8 MiB
            alignment: 0x1000,
            needs_init: true,
            is_nonmapped: false,
        },
        // PATCH: per-channel patch context.
        CtxBufDesc {
            buffer_id: PROMOTE_CTX_BUFFER_ID_PATCH,
            size: 512 * 1024, // 512 KiB
            alignment: 0x1000,
            needs_init: true,
            is_nonmapped: false,
        },
        // BUNDLE_CB: global constant bundle buffer.
        CtxBufDesc {
            buffer_id: PROMOTE_CTX_BUFFER_ID_BUFFER_BUNDLE_CB,
            size: 512 * 1024, // 512 KiB
            alignment: 0x1000,
            needs_init: false,
            is_nonmapped: false,
        },
        // PAGEPOOL: global page pool for GR.
        CtxBufDesc {
            buffer_id: PROMOTE_CTX_BUFFER_ID_PAGEPOOL,
            size: 1024 * 1024, // 1 MiB
            alignment: 0x1000,
            needs_init: false,
            is_nonmapped: false,
        },
        // ATTRIBUTE_CB: global attribute constant buffer (power-of-2 aligned).
        CtxBufDesc {
            buffer_id: PROMOTE_CTX_BUFFER_ID_ATTRIBUTE_CB,
            size: 2 * 1024 * 1024,      // 2 MiB
            alignment: 2 * 1024 * 1024, // power-of-2
            needs_init: false,
            is_nonmapped: false,
        },
        // RTV_CB_GLOBAL: render target view constant buffer.
        CtxBufDesc {
            buffer_id: PROMOTE_CTX_BUFFER_ID_RTV_CB_GLOBAL,
            size: 256 * 1024, // 256 KiB
            alignment: 0x1000,
            needs_init: false,
            is_nonmapped: false,
        },
        // FECS_EVENT: FECS event buffer.
        CtxBufDesc {
            buffer_id: PROMOTE_CTX_BUFFER_ID_FECS_EVENT,
            size: 256 * 1024, // 256 KiB
            alignment: 0x1000,
            needs_init: true,
            is_nonmapped: false,
        },
        // PRIV_ACCESS_MAP: privilege access map (non-mapped, physical only).
        CtxBufDesc {
            buffer_id: PROMOTE_CTX_BUFFER_ID_PRIV_ACCESS_MAP,
            size: 64 * 1024, // 64 KiB
            alignment: 0x1000,
            needs_init: true,
            is_nonmapped: true,
        },
        // UNRESTRICTED_PRIV_ACCESS_MAP: unrestricted privilege access map.
        // Unlike PRIV_ACCESS_MAP, this one IS VA-mapped (nouveau sets bNonmapped=0).
        CtxBufDesc {
            buffer_id: PROMOTE_CTX_BUFFER_ID_UNRESTRICTED_PRIV_ACCESS_MAP,
            size: 64 * 1024, // 64 KiB
            alignment: 0x1000,
            needs_init: true,
            is_nonmapped: false,
        },
    ]
}

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

    /// Allocate a VA space with replayable fault support for Blackwell.
    ///
    /// Tries flag combinations in order of preference:
    /// 1. `ENABLE_FAULTING | ENABLE_PAGE_FAULTING` (0x44) — full fault support
    /// 2. `ENABLE_FAULTING` (0x04) — RM-level replayable faults
    /// 3. `ENABLE_PAGE_FAULTING` (0x40) — UVM-level page faulting
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`](crate::error::DriverError) if all flag combinations fail.
    pub fn alloc_vaspace_for_uvm(&mut self, h_device: u32) -> DriverResult<u32> {
        let flag_sets: &[(u32, &str)] = &[
            (
                NV_VASPACE_FLAGS_ENABLE_FAULTING | NV_VASPACE_FLAGS_ENABLE_PAGE_FAULTING,
                "ENABLE_FAULTING|ENABLE_PAGE_FAULTING (0x44)",
            ),
            (NV_VASPACE_FLAGS_ENABLE_FAULTING, "ENABLE_FAULTING (0x04)"),
            (
                NV_VASPACE_FLAGS_ENABLE_PAGE_FAULTING,
                "ENABLE_PAGE_FAULTING (0x40)",
            ),
        ];

        for &(flags, label) in flag_sets {
            match self.alloc_vaspace_with_flags(h_device, flags) {
                Ok(h) => {
                    tracing::info!(
                        flag_set = label,
                        h_vaspace = format_args!("0x{h:08X}"),
                        "alloc_vaspace_for_uvm succeeded"
                    );
                    return Ok(h);
                }
                Err(e) => {
                    tracing::debug!(
                        flag_set = label,
                        error = %e,
                        "alloc_vaspace flag set rejected"
                    );
                }
            }
        }

        Err(crate::error::DriverError::SubmitFailed(
            "all VA space flag combinations rejected by RM".into(),
        ))
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
            va_size = format_args!("0x{:016X}", va_params.va_size),
            va_base = format_args!("0x{:016X}", va_params.va_base),
            va_start = format_args!("0x{:016X}", va_params.va_start_internal),
            va_limit = format_args!("0x{:016X}", va_params.va_limit_internal),
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
}
