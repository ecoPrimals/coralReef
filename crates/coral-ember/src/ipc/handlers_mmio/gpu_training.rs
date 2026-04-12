// SPDX-License-Identifier: AGPL-3.0-only
//! HBM2 training handler — sovereign GPU memory initialization via ember.

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, RwLock};

use coral_driver::vfio::channel::hbm2_training;

use crate::error::EmberIpcError;
use crate::hold::HeldDevice;
use crate::isolation::{self, ForkResult};

use super::super::jsonrpc::{write_jsonrpc_error, write_jsonrpc_ok};
use super::{map_bar0_if_needed, preflight_gate, require_bdf, update_fault_counter};

/// `ember.gpu.train_hbm2` — run HBM2 training pipeline on a held GPU.
///
/// Probes DEVINIT status, skips if HBM2 is already trained, otherwise runs
/// the full Untrained → Verified typestate pipeline via BAR0. Training is
/// fork-isolated so a stuck MMIO cannot freeze the main ember thread.
///
/// Params: `{bdf}`
/// Result: `{bdf, needs_post, trained, writes, vram_alive, error?}`
pub(crate) fn gpu_train_hbm2(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = require_bdf(params)?;

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    let Some(dev) = map.get_mut(bdf) else {
        write_jsonrpc_error(
            stream,
            id,
            -32000,
            &format!("{bdf}: not held by ember"),
        )
        .map_err(EmberIpcError::from)?;
        return Ok(());
    };

    if let Err(e) = map_bar0_if_needed(dev) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(
            stream,
            id,
            -32000,
            &format!("{bdf}: BAR0 map failed: {e}"),
        )
        .map_err(EmberIpcError::from);
    }

    if let Err(e) = preflight_gate(dev) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: {e}"))
            .map_err(EmberIpcError::from);
    }

    let bar0 = dev.bar0.as_ref().unwrap();
    let bar0_ptr = bar0.base_ptr() as usize;
    let bar0_size = bar0.size();
    let bdf_owned = bdf.to_string();
    drop(map);

    // Fork-isolate the training: if a BAR0 write hangs during PHY enable or
    // link training, the main ember thread survives and can report the failure.
    let fork_result = isolation::fork_isolated_mmio(
        &bdf_owned,
        std::time::Duration::from_secs(30),
        |pipe_fd| {
            let bar0 = unsafe {
                coral_driver::vfio::device::MappedBar::from_raw(bar0_ptr as *mut u8, bar0_size)
            };

            // PRI ring drain + BOOT0 preflight
            let _ = bar0.write_u32(0x0012_004C, 0x2);
            let boot0 = bar0.read_u32(0).unwrap_or(0xFFFF_FFFF);

            let devinit = bar0.read_u32(0x0002_240C).unwrap_or(0);
            let needs_post = (devinit & 2) == 0;

            if !needs_post {
                let payload = serde_json::json!({
                    "boot0": boot0,
                    "needs_post": false,
                    "trained": true,
                    "writes": 0,
                    "vram_alive": true,
                    "skipped": true,
                });
                let bytes = serde_json::to_vec(&payload).unwrap_or_default();
                unsafe { libc::write(pipe_fd, bytes.as_ptr().cast(), bytes.len()); }
                return;
            }

            let result = hbm2_training::train_hbm2(&bar0, Some(&bdf_owned), None);

            let (trained, writes, error) = match &result {
                Ok(ctrl) => (true, ctrl.training_log().write_count(), None),
                Err((err, _log)) => (false, 0, Some(format!("{err}"))),
            };

            let vram_alive = if let Ok(mut region) =
                coral_driver::vfio::memory::PraminRegion::new(&bar0, 0x0002_6000, 8)
            {
                use coral_driver::vfio::memory::MemoryRegion;
                region.probe_sentinel(0, 0xCAFE_DEAD).is_working()
            } else {
                false
            };

            let payload = serde_json::json!({
                "boot0": boot0,
                "needs_post": true,
                "trained": trained,
                "writes": writes,
                "vram_alive": vram_alive,
                "error": error,
            });
            let bytes = serde_json::to_vec(&payload).unwrap_or_default();
            unsafe { libc::write(pipe_fd, bytes.as_ptr().cast(), bytes.len()); }
        },
    );

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    if let Some(dev) = map.get_mut(&bdf_owned) {
        match &fork_result {
            ForkResult::Ok(data) => {
                let boot0 = serde_json::from_slice::<serde_json::Value>(data)
                    .ok()
                    .and_then(|v| v.get("boot0").and_then(|b| b.as_u64()))
                    .map(|b| b as u32)
                    .unwrap_or(0xFFFF_FFFF);
                let _ = update_fault_counter(dev, boot0);
            }
            ForkResult::Timeout | ForkResult::ChildFailed { .. } => {
                dev.mmio_fault_count += 1;
                tracing::error!(
                    bdf = %bdf_owned,
                    fault_count = dev.mmio_fault_count,
                    "HBM2 training fork failed"
                );
            }
            ForkResult::ForkFailed(_) | ForkResult::PipeFailed(_) => {}
        }
    }
    drop(map);

    match fork_result {
        ForkResult::Ok(data) => {
            match serde_json::from_slice::<serde_json::Value>(&data) {
                Ok(mut result) => {
                    result.as_object_mut().map(|o| {
                        o.insert("bdf".into(), serde_json::Value::String(bdf_owned));
                    });
                    write_jsonrpc_ok(stream, id, result).map_err(EmberIpcError::from)
                }
                Err(e) => {
                    write_jsonrpc_error(
                        stream,
                        id,
                        -32000,
                        &format!("{bdf_owned}: training result parse error: {e}"),
                    )
                    .map_err(EmberIpcError::from)
                }
            }
        }
        ForkResult::Timeout => {
            write_jsonrpc_error(
                stream,
                id,
                -32000,
                &format!(
                    "{bdf_owned}: HBM2 training timed out — \
                     GPU may be stuck during PHY/link training"
                ),
            )
            .map_err(EmberIpcError::from)
        }
        ForkResult::ChildFailed { status } => {
            write_jsonrpc_error(
                stream,
                id,
                -32000,
                &format!("{bdf_owned}: HBM2 training fork crashed (exit {status})"),
            )
            .map_err(EmberIpcError::from)
        }
        ForkResult::ForkFailed(e) | ForkResult::PipeFailed(e) => {
            write_jsonrpc_error(
                stream,
                id,
                -32000,
                &format!("{bdf_owned}: fork failed: {e}"),
            )
            .map_err(EmberIpcError::from)
        }
    }
}
