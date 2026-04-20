// SPDX-License-Identifier: AGPL-3.0-or-later
//! RM channel allocation, GR context-switch setup, and GPU context promotion.

use crate::error::DriverResult;

use super::super::structs::{
    GetContextBuffersInfoParams, GpuPromoteCtxParams, NvChannelAllocParams, PromoteCtxBufferEntry,
};
use super::super::{
    ADA_COMPUTE_A, AMPERE_CHANNEL_GPFIFO_A, AMPERE_COMPUTE_A, AMPERE_COMPUTE_B,
    BLACKWELL_COMPUTE_A, BLACKWELL_COMPUTE_B, ENGINE_CONTEXT_PROPERTIES_ENGINE_ID_COUNT,
    ENGINE_CTX_ID_GRAPHICS, ENGINE_CTX_ID_GRAPHICS_ATTRIBUTE_CB, ENGINE_CTX_ID_GRAPHICS_BUNDLE_CB,
    ENGINE_CTX_ID_GRAPHICS_FECS_EVENT, ENGINE_CTX_ID_GRAPHICS_PAGEPOOL,
    ENGINE_CTX_ID_GRAPHICS_PATCH, ENGINE_CTX_ID_GRAPHICS_PRIV_ACCESS_MAP,
    ENGINE_CTX_ID_GRAPHICS_RTV_CB_GLOBAL, HOPPER_COMPUTE_A, NV2080_CTRL_CMD_GPU_PROMOTE_CTX,
    NV2080_CTRL_CMD_INTERNAL_STATIC_KGR_GET_CONTEXT_BUFFERS_INFO, NV2080_ENGINE_TYPE_GR0,
    PROMOTE_CTX_BUFFER_ID_ATTRIBUTE_CB, PROMOTE_CTX_BUFFER_ID_BUFFER_BUNDLE_CB,
    PROMOTE_CTX_BUFFER_ID_FECS_EVENT, PROMOTE_CTX_BUFFER_ID_MAIN, PROMOTE_CTX_BUFFER_ID_PAGEPOOL,
    PROMOTE_CTX_BUFFER_ID_PATCH, PROMOTE_CTX_BUFFER_ID_PRIV_ACCESS_MAP,
    PROMOTE_CTX_BUFFER_ID_RTV_CB_GLOBAL, PROMOTE_CTX_BUFFER_ID_UNRESTRICTED_PRIV_ACCESS_MAP,
};
use super::RmClient;
use super::alloc::CtxBufDesc;

impl RmClient {
    /// Allocate a GPFIFO channel under a TSG (channel group).
    ///
    /// The channel inherits its VA space from the TSG — `hVASpace` in the
    /// alloc params must be 0 (the kernel rejects explicit VA space for
    /// TSG channels).
    ///
    /// Returns `(h_channel, hw_channel_id)` — the RM handle and the hardware
    /// channel ID (used as the doorbell work submit token).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`](crate::error::DriverError) if the RM alloc fails.
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

    /// Bind GR context-switch state for a channel (`NV2080_CTRL_CMD_GR_CTXSW_SETUP_BIND`).
    ///
    /// On GSP-RM (580.x+), this tells the GPU System Processor to allocate
    /// all GR context buffers for the channel. Without this, the first compute
    /// dispatch hits `CTXNOTVALID` (error 0x20) because there is no GR context.
    ///
    /// `v_mem_ptr` is the GPU VA of a pre-allocated context buffer. When 0,
    /// RM allocates context buffers internally (demand-paged). When non-zero,
    /// RM uses the provided eagerly-mapped buffer, avoiding demand-paged faults
    /// that can't be serviced without UVM registration.
    pub fn gr_ctxsw_setup_bind(&mut self, h_subdevice: u32, h_channel: u32) -> DriverResult<()> {
        self.gr_ctxsw_setup_bind_with_mem(h_subdevice, h_channel, 0)
    }

    /// Like [`gr_ctxsw_setup_bind`](Self::gr_ctxsw_setup_bind) but with an
    /// explicit context buffer GPU VA.
    pub fn gr_ctxsw_setup_bind_with_mem(
        &mut self,
        h_subdevice: u32,
        h_channel: u32,
        v_mem_ptr: u64,
    ) -> DriverResult<()> {
        #[repr(C)]
        #[derive(Debug, Default)]
        struct GrCtxswSetupBindParams {
            h_client: u32,
            h_channel: u32,
            v_mem_ptr: u64,
        }

        let mut params = GrCtxswSetupBindParams {
            h_client: self.h_client,
            h_channel,
            v_mem_ptr,
        };

        tracing::debug!(
            h_channel = format_args!("0x{h_channel:08X}"),
            v_mem_ptr = format_args!("0x{v_mem_ptr:016X}"),
            "GR_CTXSW_SETUP_BIND request"
        );

        let result = self.rm_control(
            h_subdevice,
            super::super::NV2080_CTRL_CMD_GR_CTXSW_SETUP_BIND,
            &mut params,
            "RM_CONTROL(GR_CTXSW_SETUP_BIND)",
        );

        match &result {
            Ok(()) => {
                tracing::info!(
                    h_channel = format_args!("0x{h_channel:08X}"),
                    v_mem_ptr = format_args!("0x{v_mem_ptr:016X}"),
                    "GR context switch setup bound — context ready for compute"
                );
            }
            Err(e) => {
                tracing::warn!(
                    h_channel = format_args!("0x{h_channel:08X}"),
                    v_mem_ptr = format_args!("0x{v_mem_ptr:016X}"),
                    error = %e,
                    "GR_CTXSW_SETUP_BIND failed"
                );
            }
        }

        result
    }

    /// Query GR context buffer requirements from GSP-RM.
    ///
    /// Calls `NV2080_CTRL_CMD_INTERNAL_STATIC_KGR_GET_CONTEXT_BUFFERS_INFO`
    /// on the subdevice and returns a list of `(buffer_id, size, alignment)`
    /// descriptors for the first GR engine instance (index 0).
    ///
    /// The mapping from engine-context-property IDs to promote-context buffer
    /// IDs follows the same table nouveau uses in `r535_gr_get_ctxbuf_info()`.
    pub fn query_gr_context_buffers_info(
        &mut self,
        h_subdevice: u32,
    ) -> DriverResult<Vec<CtxBufDesc>> {
        let mut params = GetContextBuffersInfoParams::default();

        let result = self.rm_control(
            h_subdevice,
            NV2080_CTRL_CMD_INTERNAL_STATIC_KGR_GET_CONTEXT_BUFFERS_INFO,
            &mut params,
            "RM_CONTROL(KGR_GET_CONTEXT_BUFFERS_INFO)",
        );

        if let Err(e) = result {
            tracing::warn!(
                error = %e,
                "KGR_GET_CONTEXT_BUFFERS_INFO failed (internal RM command — may be kernel-only)"
            );
            return Err(e);
        }

        let gr0 = &params.engine_context_buffers_info[0];

        // Mapping table: (engine_ctx_id, promote_buffer_id, needs_init, is_nonmapped)
        // Mirrors nouveau's r535_gr_get_ctxbuf_info() table.
        let mapping: &[(usize, u16, bool, bool)] = &[
            (
                ENGINE_CTX_ID_GRAPHICS,
                PROMOTE_CTX_BUFFER_ID_MAIN,
                true,
                false,
            ),
            (
                ENGINE_CTX_ID_GRAPHICS_PATCH,
                PROMOTE_CTX_BUFFER_ID_PATCH,
                true,
                false,
            ),
            (
                ENGINE_CTX_ID_GRAPHICS_BUNDLE_CB,
                PROMOTE_CTX_BUFFER_ID_BUFFER_BUNDLE_CB,
                false,
                false,
            ),
            (
                ENGINE_CTX_ID_GRAPHICS_PAGEPOOL,
                PROMOTE_CTX_BUFFER_ID_PAGEPOOL,
                false,
                false,
            ),
            (
                ENGINE_CTX_ID_GRAPHICS_ATTRIBUTE_CB,
                PROMOTE_CTX_BUFFER_ID_ATTRIBUTE_CB,
                false,
                false,
            ),
            (
                ENGINE_CTX_ID_GRAPHICS_RTV_CB_GLOBAL,
                PROMOTE_CTX_BUFFER_ID_RTV_CB_GLOBAL,
                false,
                false,
            ),
            (
                ENGINE_CTX_ID_GRAPHICS_FECS_EVENT,
                PROMOTE_CTX_BUFFER_ID_FECS_EVENT,
                true,
                false,
            ),
            (
                ENGINE_CTX_ID_GRAPHICS_PRIV_ACCESS_MAP,
                PROMOTE_CTX_BUFFER_ID_PRIV_ACCESS_MAP,
                true,
                true,
            ),
            // Unrestricted priv access map uses the same engine ID as priv access map
            // but with a different promote buffer ID — nouveau allocates it separately.
            (
                ENGINE_CTX_ID_GRAPHICS_PRIV_ACCESS_MAP,
                PROMOTE_CTX_BUFFER_ID_UNRESTRICTED_PRIV_ACCESS_MAP,
                true,
                true,
            ),
        ];

        let mut descs = Vec::new();
        for &(engine_id, buffer_id, needs_init, is_nonmapped) in mapping {
            if engine_id >= ENGINE_CONTEXT_PROPERTIES_ENGINE_ID_COUNT {
                continue;
            }
            let info = &gr0.engine[engine_id];
            if info.size == 0 {
                continue;
            }

            let mut size = info.size as u64;
            let mut alignment = info.alignment as u64;

            // MAIN buffer: nouveau adds 64 * 0x1000 (256 KiB) for per-subctx headers.
            if buffer_id == PROMOTE_CTX_BUFFER_ID_MAIN {
                size += 64 * 0x1000;
            }

            // ATTRIBUTE_CB: nouveau uses power-of-2 alignment.
            if buffer_id == PROMOTE_CTX_BUFFER_ID_ATTRIBUTE_CB && alignment > 0 {
                alignment = alignment.next_power_of_two();
            }

            // Minimum page alignment.
            if alignment < 0x1000 {
                alignment = 0x1000;
            }

            // Round size up to alignment.
            size = (size + alignment - 1) & !(alignment - 1);

            tracing::debug!(
                buffer_id,
                engine_id = format_args!("0x{engine_id:02X}"),
                size = format_args!("0x{size:X}"),
                alignment = format_args!("0x{alignment:X}"),
                needs_init,
                is_nonmapped,
                "ctx_buf descriptor from RM query"
            );

            descs.push(CtxBufDesc {
                buffer_id,
                size,
                alignment,
                needs_init,
                is_nonmapped,
            });
        }

        Ok(descs)
    }

    /// Promote explicitly-allocated context buffers to RM.
    ///
    /// Calls `NV2080_CTRL_CMD_GPU_PROMOTE_CTX` to inform GSP-RM about
    /// the context buffers we allocated in our VA space. This replaces the
    /// demand-paged internal allocation that causes `SM Warp Exception:
    /// Invalid Address Space` on Blackwell.
    pub fn gpu_promote_ctx(
        &mut self,
        h_subdevice: u32,
        h_channel: u32,
        entries: &[PromoteCtxBufferEntry],
    ) -> DriverResult<()> {
        use super::super::GPU_PROMOTE_CONTEXT_MAX_ENTRIES;

        if entries.len() > GPU_PROMOTE_CONTEXT_MAX_ENTRIES {
            return Err(crate::error::DriverError::SubmitFailed(
                format!(
                    "GPU_PROMOTE_CTX: {} entries exceeds max {}",
                    entries.len(),
                    GPU_PROMOTE_CONTEXT_MAX_ENTRIES
                )
                .into(),
            ));
        }

        let mut params = GpuPromoteCtxParams {
            engine_type: NV2080_ENGINE_TYPE_GR0,
            h_client: self.h_client,
            ch_id: 0, // RM looks up by h_object (channel handle)
            h_chan_client: self.h_client,
            h_object: h_channel,
            entry_count: entries.len() as u32,
            ..Default::default()
        };

        for (i, entry) in entries.iter().enumerate() {
            params.promote_entry[i] = *entry;
        }

        tracing::info!(
            h_channel = format_args!("0x{h_channel:08X}"),
            entry_count = entries.len(),
            "GPU_PROMOTE_CTX request"
        );

        self.rm_control(
            h_subdevice,
            NV2080_CTRL_CMD_GPU_PROMOTE_CTX,
            &mut params,
            "RM_CONTROL(GPU_PROMOTE_CTX)",
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
