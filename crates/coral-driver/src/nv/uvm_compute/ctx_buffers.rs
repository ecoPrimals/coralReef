// SPDX-License-Identifier: AGPL-3.0-or-later
//! Context buffer promotion (`GPU_PROMOTE_CTX`) and GR context-switch binding.

use crate::error::DriverResult;
use crate::nv::uvm::RmClient;

use super::types::{CtxBuffer, GpuGen};

/// Query / allocate context buffers, promote them via RM, and optionally bind GR
/// context-switch state.
///
/// Returns promoted [`CtxBuffer`] entries and whether kmod `BindChannelResources`
/// already bound the context (`kmod_bind_ok`).
#[expect(
    clippy::too_many_arguments,
    reason = "GPU_PROMOTE_CTX path threads many RM object handles together"
)]
pub(super) fn promote_ctx_buffers_userspace(
    client: &mut RmClient,
    sm: u32,
    gpu_uuid: &[u8; 16],
    h_device: u32,
    h_subdevice: u32,
    h_vaspace: u32,
    h_channel: u32,
    h_virt_mem: u32,
) -> DriverResult<(Vec<CtxBuffer>, bool)> {
    // Blackwell+ requires kernel privilege for context buffer promotion
    // (GPU_PROMOTE_CTX returns INSUFFICIENT_PERMISSIONS from userspace).
    //
    // Hybrid approach: if coral-kmod is loaded, use CORAL_IOCTL_BIND_CHANNEL
    // which calls nvUvmInterface{RetainChannel,BindChannelResources} from
    // kernel context. Falls back to userspace GPU_PROMOTE_CTX for older GPUs.
    let (ctx_buffers, kmod_bind_ok) = 'promote: {
        // Try kernel-privileged binding via coral-kmod (Blackwell+).
        if sm >= 100
            && let Some(kmod) = crate::nv::coral_kmod::CoralKmod::try_open()
        {
            match kmod.bind_channel(gpu_uuid, client.handle(), h_vaspace, h_channel, sm) {
                Ok(result) => {
                    tracing::info!(
                        resource_count = result.resource_count,
                        hw_channel_id = result.hw_channel_id,
                        channel_engine_type = result.channel_engine_type,
                        tsg_id = result.tsg_id,
                        "BIND_CHANNEL via kmod succeeded"
                    );
                    let ctx = result
                        .resources
                        .iter()
                        .map(|r| {
                            tracing::debug!(
                                resource_id = r.resource_id,
                                gpu_va = format_args!("0x{:016X}", r.gpu_va),
                                size = format_args!("0x{:X}", r.size),
                                alignment = format_args!("0x{:X}", r.alignment),
                                "kmod bind channel resource"
                            );
                            CtxBuffer {
                                buffer_id: r.resource_id as u16,
                                h_memory: 0,
                                size: r.size,
                                gpu_va: r.gpu_va,
                            }
                        })
                        .collect::<Vec<_>>();
                    break 'promote (ctx, true);
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "BIND_CHANNEL via kmod failed, falling back to GPU_PROMOTE_CTX"
                    );
                }
            }
        }

        // Userspace GPU_PROMOTE_CTX path (works on pre-Blackwell).
        let descs = match client.query_gr_context_buffers_info(h_subdevice) {
            Ok(d) if !d.is_empty() => {
                tracing::info!(
                    buffer_count = d.len(),
                    "GPU_PROMOTE_CTX: buffers from RM query"
                );
                d
            }
            other => {
                if let Err(e) = &other {
                    tracing::warn!(
                        error = %e,
                        "KGR_GET_CONTEXT_BUFFERS_INFO failed"
                    );
                }
                tracing::warn!("using hardcoded Blackwell context buffer sizes");
                crate::nv::uvm::rm_client::alloc::hardcoded_blackwell_ctx_buffers()
            }
        };

        use crate::nv::uvm::structs::PromoteCtxBufferEntry;

        let mut promote_entries: Vec<PromoteCtxBufferEntry> = Vec::new();
        let mut allocated: Vec<CtxBuffer> = Vec::new();
        let mut ctx_handle_counter = h_device + 0x7000_u32;

        for desc in &descs {
            let h_mem = ctx_handle_counter;
            ctx_handle_counter += 1;

            if let Err(e) = client.alloc_system_memory(h_device, h_mem, desc.size) {
                tracing::warn!(
                    buffer_id = desc.buffer_id,
                    error = %e,
                    "alloc ctx_buf failed"
                );
                continue;
            }

            let gpu_va = if desc.is_nonmapped {
                0_u64
            } else {
                match client.rm_map_memory_dma(h_device, h_virt_mem, h_mem, 0, desc.size) {
                    Ok(va) => va,
                    Err(e) => {
                        tracing::warn!(
                            buffer_id = desc.buffer_id,
                            error = %e,
                            "map ctx_buf failed"
                        );
                        client.free_object(h_device, h_mem).ok();
                        continue;
                    }
                }
            };

            tracing::debug!(
                buffer_id = desc.buffer_id,
                gpu_va = format_args!("0x{gpu_va:016X}"),
                size = format_args!("0x{:X}", desc.size),
                "ctx_buf allocated"
            );

            let mut entry = PromoteCtxBufferEntry {
                gpu_virt_addr: gpu_va,
                buffer_id: desc.buffer_id,
                b_initialize: u8::from(desc.needs_init),
                b_nonmapped: u8::from(desc.is_nonmapped),
                ..Default::default()
            };
            if desc.needs_init {
                entry.size = desc.size;
                entry.phys_attr = 4;
            }
            promote_entries.push(entry);

            allocated.push(CtxBuffer {
                buffer_id: desc.buffer_id,
                h_memory: h_mem,
                size: desc.size,
                gpu_va,
            });
        }

        if !promote_entries.is_empty() {
            match client.gpu_promote_ctx(h_subdevice, h_channel, &promote_entries) {
                Ok(()) => {
                    tracing::info!(
                        entry_count = promote_entries.len(),
                        "GPU_PROMOTE_CTX buffers promoted OK"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "GPU_PROMOTE_CTX failed (kernel-only — will fall back to gr_ctxsw_setup_bind)"
                    );
                    for cb in &allocated {
                        if cb.gpu_va != 0 {
                            client
                                .rm_unmap_memory_dma(h_device, h_virt_mem, cb.h_memory, cb.gpu_va)
                                .ok();
                        }
                        client.free_object(h_device, cb.h_memory).ok();
                    }
                    break 'promote (Vec::new(), false);
                }
            }
        }

        (allocated, false)
    };

    Ok((ctx_buffers, kmod_bind_ok))
}

/// Bind GR context-switch state after promotion. Skips when kmod already bound.
pub(super) fn gr_ctxsw_setup_after_promotion(
    client: &mut RmClient,
    kmod_bind_ok: bool,
    ctx_buffers: &[CtxBuffer],
    h_subdevice: u32,
    h_channel: u32,
) -> DriverResult<()> {
    // Bind GR context-switch state. If kmod BindChannelResources succeeded,
    // the context is already bound — skip. Otherwise, if GPU_PROMOTE_CTX
    // succeeded pass the MAIN buffer VA; else use vMemPtr=0 (RM demand-pages).
    if kmod_bind_ok {
        tracing::info!(
            "skipping gr_ctxsw_setup_bind (kmod BindChannelResources already bound context)"
        );
    } else {
        let main_ctx_va = ctx_buffers
            .iter()
            .find(|cb| cb.buffer_id == crate::nv::uvm::PROMOTE_CTX_BUFFER_ID_MAIN)
            .map_or(0_u64, |cb| cb.gpu_va);
        client.gr_ctxsw_setup_bind_with_mem(h_subdevice, h_channel, main_ctx_va)?;
    }
    Ok(())
}

/// Whether this GPU generation uses semaphore-based GPFIFO completion (Blackwell+).
pub(super) const fn uses_semaphore_fence_for_gen(gpu_gen: GpuGen) -> bool {
    matches!(gpu_gen, GpuGen::BlackwellA | GpuGen::BlackwellB)
}
