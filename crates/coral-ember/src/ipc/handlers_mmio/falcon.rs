// SPDX-License-Identifier: AGPL-3.0-only
//! High-level falcon operation handlers (SEC2 prepare, IMEM/DMEM upload, start, poll).

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, RwLock};

use crate::error::EmberIpcError;
use crate::hold::HeldDevice;

use super::super::jsonrpc::{write_jsonrpc_error, write_jsonrpc_ok};
use super::{
    decode_b64_param, map_bar0_if_needed, preflight_check, require_bdf, require_held_mut,
    require_u64,
};

/// `ember.sec2.prepare_physical` — runs sec2_prepare_v1() server-side.
///
/// Uses falcon v1 register interface: PMC reset with correct SEC2 bit,
/// VRAM page tables, instance block bind via 0x480, FBIF → PHYS_VID.
///
/// The entire operation runs in a **forked child process** to provide
/// true process-level isolation. If the child's CPU core gets stuck on
/// a PRAMIN write (PCIe flow-control stall), the parent detects the
/// timeout, triggers a bus reset from the bridge side (always accessible),
/// kills the child, and marks the device faulted. Ember stays alive.
///
/// Params: `{bdf}`
/// Result: `{ok, notes[]}`
pub(crate) fn sec2_prepare_physical(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = require_bdf(params)?;

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    let dev = require_held_mut(&mut map, bdf, stream, &id)?;

    if let Err(e) = map_bar0_if_needed(dev) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: BAR0 map failed: {e}"))
            .map_err(EmberIpcError::from);
    }

    if let Err(e) = preflight_check(dev) {
        drop(map);
        return write_jsonrpc_error(stream, id, -32011, &e).map_err(EmberIpcError::from);
    }

    let bdf_owned = bdf.to_string();

    // Get the raw BAR0 pointer + size so the forked child can use it.
    // After fork(), the mmap'd region is inherited — same virtual address,
    // same physical mapping. The child can do volatile writes directly.
    let bar0 = dev.bar0.as_ref().unwrap();
    let bar0_ptr = bar0.base_ptr() as usize;
    let bar0_size = bar0.size();

    tracing::info!(
        bdf,
        bar0_ptr = format_args!("{bar0_ptr:#x}"),
        bar0_size = format_args!("{bar0_size:#x}"),
        "sec2_prepare_physical: launching fork-isolated child"
    );

    let fork_result = crate::isolation::fork_isolated_mmio(
        &bdf_owned,
        crate::isolation::MMIO_WATCHDOG_TIMEOUT,
        |pipe_fd| {
            // ═══ CHILD PROCESS ═══
            // Reconstruct MappedBar from the inherited mmap pointer.
            // SAFETY: the mmap is inherited across fork — same virtual
            // address, same physical device mapping. No Rust locks are
            // acquired here (MappedBar::from_raw is a trivial constructor).
            let child_bar0 = unsafe {
                coral_driver::vfio::device::MappedBar::from_raw(
                    bar0_ptr as *mut u8,
                    bar0_size,
                )
            };
            let (ok, notes) =
                coral_driver::nv::vfio_compute::acr_boot::sec2_prepare_v1(&child_bar0);

            // Serialize result as JSON to the pipe.
            // Use raw libc::write to avoid Rust stdio locks (poisoned after fork).
            let json = serde_json::json!({"ok": ok, "notes": notes});
            if let Ok(bytes) = serde_json::to_vec(&json) {
                let mut written = 0usize;
                while written < bytes.len() {
                    let n = unsafe {
                        libc::write(
                            pipe_fd,
                            bytes[written..].as_ptr().cast(),
                            bytes.len() - written,
                        )
                    };
                    if n <= 0 {
                        break;
                    }
                    written += n as usize;
                }
            }

            // Do NOT let MappedBar::drop run — the parent still owns the mmap.
            std::mem::forget(child_bar0);
        },
    );

    match fork_result {
        crate::isolation::ForkResult::Ok(_) => {
            // Child succeeded. Read the result from the trace file since the
            // pipe was closed. Parse the JSON from the sec2 trace if needed.
            // For now, re-read the trace to get the result.
            let trace_path = "/var/lib/coralreef/traces/ember_sec2_trace.log";
            let trace = std::fs::read_to_string(trace_path).unwrap_or_default();
            let ok = trace.contains("sec2_prepare_v1 EXIT");
            let bind_ok = trace.contains("BIND: complete ok=true");

            let mut notes = vec![
                "fork-isolated: child completed successfully".to_string(),
            ];

            // Try to parse the result from the pipe data written by child
            // (the pipe read fd was already closed in fork_isolated_mmio,
            // so we rely on the trace file)
            for line in trace.lines() {
                if let Some(suffix) = line.split(']').nth(1) {
                    let trimmed = suffix.trim();
                    if trimmed.starts_with("PMC0:")
                        || trimmed.starts_with("RESET_")
                        || trimmed.starts_with("BIND:")
                        || trimmed.starts_with("PT_BUILD:")
                        || trimmed.starts_with("FINAL:")
                        || trimmed.starts_with("POST_RESET:")
                        || trimmed.starts_with("VRAM_CHECK:")
                        || trimmed.starts_with("FBIF:")
                    {
                        notes.push(trimmed.to_string());
                    }
                }
            }

            dev.experiment_dirty = true;
            drop(map);

            tracing::info!(bdf, ok = bind_ok, "sec2_prepare_physical (fork): complete");
            write_jsonrpc_ok(
                stream,
                id,
                serde_json::json!({"ok": ok && bind_ok, "notes": notes}),
            )
            .map_err(EmberIpcError::from)
        }

        crate::isolation::ForkResult::Timeout => {
            tracing::error!(
                bdf,
                "sec2_prepare_physical: fork child TIMED OUT — device bus-reset, marking FAULTED"
            );
            dev.health = crate::hold::DeviceHealth::Faulted;
            dev.bar0 = None;
            drop(map);

            write_jsonrpc_error(
                stream,
                id,
                -32099,
                &format!(
                    "{bdf_owned}: sec2_prepare_physical timed out in fork-isolated child — \
                     GPU was bus-reset. Device marked faulted. \
                     Use ember.device.recover to attempt recovery."
                ),
            )
            .map_err(EmberIpcError::from)
        }

        crate::isolation::ForkResult::ChildFailed { status } => {
            tracing::error!(
                bdf,
                status,
                "sec2_prepare_physical: fork child failed"
            );
            dev.health = crate::hold::DeviceHealth::Faulted;
            drop(map);

            write_jsonrpc_error(
                stream,
                id,
                -32098,
                &format!(
                    "{bdf_owned}: sec2_prepare_physical child process failed (status={status}). \
                     Device marked faulted."
                ),
            )
            .map_err(EmberIpcError::from)
        }

        crate::isolation::ForkResult::ForkFailed(e) => {
            tracing::error!(bdf, error = %e, "sec2_prepare_physical: fork() failed");
            drop(map);

            write_jsonrpc_error(
                stream,
                id,
                -32097,
                &format!("{bdf_owned}: fork() failed: {e}. Cannot isolate dangerous operation."),
            )
            .map_err(EmberIpcError::from)
        }

        crate::isolation::ForkResult::PipeFailed(e) => {
            tracing::error!(bdf, error = %e, "sec2_prepare_physical: pipe() failed");
            drop(map);

            write_jsonrpc_error(
                stream,
                id,
                -32096,
                &format!("{bdf_owned}: pipe() failed: {e}"),
            )
            .map_err(EmberIpcError::from)
        }
    }
}

/// `ember.falcon.upload_imem` — upload code to falcon IMEM server-side.
///
/// Params: `{bdf, base, imem_addr, code_b64, start_tag}`
/// Result: `{ok}`
pub(crate) fn falcon_upload_imem(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = require_bdf(params)?;
    let base = require_u64(params, "base")? as usize;
    let imem_addr = require_u64(params, "imem_addr")? as u32;
    let start_tag = require_u64(params, "start_tag")? as u32;
    let code = decode_b64_param(params, "code_b64")?;

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    let dev = require_held_mut(&mut map, bdf, stream, &id)?;

    if let Err(e) = map_bar0_if_needed(dev) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: BAR0 map failed: {e}"))
            .map_err(EmberIpcError::from);
    }

    if let Err(e) = preflight_check(dev) {
        drop(map);
        return write_jsonrpc_error(stream, id, -32011, &e).map_err(EmberIpcError::from);
    }

    {
        let bar0 = dev.bar0.as_ref().unwrap();
        coral_driver::nv::vfio_compute::acr_boot::falcon_imem_upload_nouveau(
            bar0, base, imem_addr, &code, start_tag,
        );
    }

    dev.experiment_dirty = true;
    drop(map);

    tracing::info!(bdf, base = format_args!("{base:#x}"), imem_addr = format_args!("{imem_addr:#x}"), bytes = code.len(), "ember.falcon.upload_imem: complete");
    write_jsonrpc_ok(stream, id, serde_json::json!({"ok": true, "bytes": code.len()}))
        .map_err(EmberIpcError::from)
}

/// `ember.falcon.upload_dmem` — upload data to falcon DMEM server-side.
///
/// Params: `{bdf, base, dmem_addr, data_b64}`
/// Result: `{ok}`
pub(crate) fn falcon_upload_dmem(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = require_bdf(params)?;
    let base = require_u64(params, "base")? as usize;
    let dmem_addr = require_u64(params, "dmem_addr")? as u32;
    let data = decode_b64_param(params, "data_b64")?;

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    let dev = require_held_mut(&mut map, bdf, stream, &id)?;

    if let Err(e) = map_bar0_if_needed(dev) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: BAR0 map failed: {e}"))
            .map_err(EmberIpcError::from);
    }

    if let Err(e) = preflight_check(dev) {
        drop(map);
        return write_jsonrpc_error(stream, id, -32011, &e).map_err(EmberIpcError::from);
    }

    {
        let bar0 = dev.bar0.as_ref().unwrap();
        coral_driver::nv::vfio_compute::acr_boot::falcon_dmem_upload(
            bar0, base, dmem_addr, &data,
        );
    }

    dev.experiment_dirty = true;
    drop(map);

    tracing::info!(bdf, base = format_args!("{base:#x}"), dmem_addr = format_args!("{dmem_addr:#x}"), bytes = data.len(), "ember.falcon.upload_dmem: complete");
    write_jsonrpc_ok(stream, id, serde_json::json!({"ok": true, "bytes": data.len()}))
        .map_err(EmberIpcError::from)
}

/// `ember.falcon.start_cpu` — issue STARTCPU to a falcon, server-side.
///
/// Params: `{bdf, base}`
/// Result: `{ok, pc, exci, cpuctl}`
pub(crate) fn falcon_start_cpu(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = require_bdf(params)?;
    let base = require_u64(params, "base")? as usize;

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    let dev = require_held_mut(&mut map, bdf, stream, &id)?;

    if let Err(e) = map_bar0_if_needed(dev) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: BAR0 map failed: {e}"))
            .map_err(EmberIpcError::from);
    }

    if let Err(e) = preflight_check(dev) {
        drop(map);
        return write_jsonrpc_error(stream, id, -32011, &e).map_err(EmberIpcError::from);
    }

    let (pc, exci, cpuctl) = {
        let bar0 = dev.bar0.as_ref().unwrap();
        coral_driver::nv::vfio_compute::acr_boot::falcon_start_cpu(bar0, base);
        (
            bar0.read_u32(base + 0x030).unwrap_or(0xDEAD),
            bar0.read_u32(base + 0x148).unwrap_or(0xDEAD),
            bar0.read_u32(base + 0x100).unwrap_or(0xDEAD),
        )
    };

    dev.experiment_dirty = true;
    drop(map);

    tracing::info!(bdf, base = format_args!("{base:#x}"), pc = format_args!("{pc:#x}"), "ember.falcon.start_cpu: complete");
    write_jsonrpc_ok(
        stream,
        id,
        serde_json::json!({"ok": true, "pc": pc, "exci": exci, "cpuctl": cpuctl}),
    )
    .map_err(EmberIpcError::from)
}

/// `ember.falcon.poll` — server-side register polling with stop conditions.
///
/// Params: `{bdf, base, timeout_ms, mailbox_sentinel}`
/// Result: `{snapshots[], pc_trace[], final: {...}}`
pub(crate) fn falcon_poll(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = require_bdf(params)?;
    let base = require_u64(params, "base")? as usize;
    let timeout_ms = params
        .get("timeout_ms")
        .and_then(|v| v.as_u64())
        .unwrap_or(5000);
    let mailbox_sentinel = params
        .get("mailbox_sentinel")
        .and_then(|v| v.as_u64())
        .unwrap_or(0xDEAD_A5A5) as u32;

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    let dev = require_held_mut(&mut map, bdf, stream, &id)?;

    if let Err(e) = map_bar0_if_needed(dev) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: BAR0 map failed: {e}"))
            .map_err(EmberIpcError::from);
    }

    if let Err(e) = preflight_check(dev) {
        drop(map);
        return write_jsonrpc_error(stream, id, -32011, &e).map_err(EmberIpcError::from);
    }

    let (snapshots, pc_trace, final_snapshot) = {
        let bar0 = dev.bar0.as_ref().unwrap();
        let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);

        let timeout = std::time::Duration::from_millis(timeout_ms);
        let start_time = std::time::Instant::now();
        let mut pc_trace: Vec<u32> = Vec::new();
        let mut snapshots: Vec<serde_json::Value> = Vec::new();

        for _ in 0..500 {
            let pc = r(0x030);
            if pc_trace.last() != Some(&pc) {
                pc_trace.push(pc);
            }
            std::thread::sleep(std::time::Duration::from_micros(100));
            if start_time.elapsed().as_millis() > 50 {
                break;
            }
        }

        let mut settled = 0u32;
        let mut last_pc = pc_trace.last().copied().unwrap_or(0);
        loop {
            std::thread::sleep(std::time::Duration::from_millis(5));
            let cpuctl = r(0x100);
            let sctl = r(0x240);
            let mb0 = r(0x040);
            let mb1 = r(0x044);
            let pc = r(0x030);
            let elapsed_ms = start_time.elapsed().as_millis() as u64;

            if pc != last_pc {
                pc_trace.push(pc);
                last_pc = pc;
                settled = 0;
            } else {
                settled += 1;
            }

            let halted = cpuctl & (1 << 4) != 0;

            if mb0 != mailbox_sentinel || halted {
                snapshots.push(serde_json::json!({
                    "cpuctl": cpuctl, "sctl": sctl, "pc": pc,
                    "mailbox0": mb0, "mailbox1": mb1,
                    "elapsed_ms": elapsed_ms, "reason": "stop_condition",
                }));
                break;
            }
            if settled > 200 || start_time.elapsed() > timeout {
                snapshots.push(serde_json::json!({
                    "cpuctl": cpuctl, "sctl": sctl, "pc": pc,
                    "mailbox0": mb0, "mailbox1": mb1,
                    "elapsed_ms": elapsed_ms,
                    "reason": if settled > 200 { "settled" } else { "timeout" },
                }));
                break;
            }
        }

        let final_snap = serde_json::json!({
            "cpuctl": r(0x100), "sctl": r(0x240), "pc": r(0x030), "exci": r(0x148),
            "mailbox0": r(0x040), "mailbox1": r(0x044),
            "dmactl": r(0x10C), "itfen": r(0x048), "fbif_transcfg": r(0x624),
            "hs_mode": r(0x240) & 0x02 != 0,
        });

        (snapshots, pc_trace, final_snap)
    };

    drop(map);

    tracing::info!(bdf, base = format_args!("{base:#x}"), pc_entries = pc_trace.len(), "ember.falcon.poll: complete");
    write_jsonrpc_ok(
        stream,
        id,
        serde_json::json!({
            "snapshots": snapshots,
            "pc_trace": pc_trace,
            "final": final_snapshot,
        }),
    )
    .map_err(EmberIpcError::from)
}
