// SPDX-License-Identifier: AGPL-3.0-only
//! PRAMIN bulk VRAM read/write handlers.
//!
//! Bulk PRAMIN writes are one of the most dangerous MMIO operations:
//! hundreds of posted writes can exhaust PCIe flow-control credits,
//! stalling the CPU core at the hardware level. A thread watchdog
//! cannot interrupt this — the core is physically waiting for credits
//! that never arrive.
//!
//! To protect ember, all bulk PRAMIN writes run in a **forked child
//! process** via [`fork_isolated_mmio`]. If the child stalls, the
//! parent detects the timeout, triggers a PCIe secondary bus reset
//! from the bridge side, kills the child, and marks the device
//! faulted. Ember stays alive and can recover.

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, RwLock};

use coral_driver::vfio::memory::{MemoryRegion, PraminRegion};

use crate::error::EmberIpcError;
use crate::hold::HeldDevice;

use super::super::jsonrpc::{write_jsonrpc_error, write_jsonrpc_ok};
use super::{
    base64_decode, base64_encode, le_bytes_to_u32, map_bar0_if_needed, preflight_gate,
    require_bdf, require_held_mut,
};

const PRAMIN_TRACE_PATH: &str = "/var/lib/coralreef/traces/ember_pramin_trace.log";

/// Crash-resilient trace for PRAMIN operations — survives system lockups.
/// Uses raw fd writes safe for use after fork().
fn pt(msg: &str) {
    use std::io::Write as W;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(PRAMIN_TRACE_PATH)
    {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let _ = writeln!(f, "[{ts}] {msg}");
    }
}

/// Execute the bulk PRAMIN write in a forked child.
///
/// This is the closure that runs inside `fork_isolated_mmio`. It only
/// uses async-signal-safe operations and raw BAR0 access.
///
/// # Safety rationale
///
/// Unsafe is required here for two reasons:
/// 1. `MappedBar::from_raw` — reconstructs a BAR0 handle from the mmap
///    inherited across `fork()` (same virtual address, same physical mapping)
/// 2. `libc::write` — writes result to the pipe fd (Rust stdio locks are
///    poisoned after fork in a multi-threaded process)
#[allow(unsafe_code)]
fn pramin_write_child(
    bar0_ptr: usize,
    bar0_size: usize,
    vram_addr: u32,
    data: &[u8],
    pipe_fd: i32,
) {
    let bar0 = unsafe {
        coral_driver::vfio::device::MappedBar::from_raw(bar0_ptr as *mut u8, bar0_size)
    };

    let _ = std::fs::remove_file(PRAMIN_TRACE_PATH);
    pt(&format!(
        "pramin_write ENTER (fork child) vram_addr={vram_addr:#x} data_len={}",
        data.len()
    ));

    let mut bytes_written: usize = 0;
    let mut err: Option<String> = None;

    // Liveness probe: single PRAMIN read before committing to bulk writes
    pt("LIVENESS: reading BAR0_WINDOW (0x1700)");
    let saved_window = bar0.read_u32(0x1700).unwrap_or(0xDEAD_DEAD);
    pt(&format!("LIVENESS: BAR0_WINDOW={saved_window:#010x}"));

    let window_val = (vram_addr >> 16) as u32;
    pt(&format!("LIVENESS: writing BAR0_WINDOW={window_val:#010x}"));
    let _ = bar0.write_u32(0x1700, window_val);

    pt("LIVENESS: reading PRAMIN probe @ 0x700000");
    let probe = bar0.read_u32(0x0070_0000).unwrap_or(0xDEAD_DEAD);
    pt(&format!("LIVENESS: probe={probe:#010x}"));

    let _ = bar0.write_u32(0x1700, saved_window);
    pt("LIVENESS: restored OK");

    if probe == 0xDEAD_DEAD || probe == 0xFFFF_FFFF {
        err = Some(format!(
            "PRAMIN liveness FAILED: {probe:#010x} — BAR0 unresponsive"
        ));
    } else if probe & 0xFFF0_0000 == 0xBAD0_0000 {
        err = Some(format!(
            "PRAMIN liveness FAILED: {probe:#010x} — VRAM dead (warm GPU first)"
        ));
    }

    if let Some(ref e) = err {
        pt(&format!("LIVENESS: FAILED — {e}"));
    } else {
        pt("LIVENESS: PASSED — VRAM alive");
    }

    // PRAMIN write-then-readback canary: verify the PRAMIN write path
    // is functional BEFORE committing to bulk writes. A single read can
    // succeed (cached or lucky timing) while bulk writes stall because the
    // GPU's internal memory controller or PRAMIN engine buffer is stuck.
    //
    // The canary writes a known pattern to the target VRAM address via
    // PRAMIN, reads it back, and verifies the round-trip. This exercises
    // the full BAR0 → PRAMIN engine → VRAM → readback path.
    if err.is_none() {
        pt("CANARY: PRI drain before write test");
        let _ = bar0.write_u32(0x12004C, 0x02);
        std::thread::sleep(std::time::Duration::from_millis(5));

        pt("CANARY: setting BAR0_WINDOW for target VRAM");
        let canary_window = (vram_addr >> 16) as u32;
        let _ = bar0.write_u32(0x1700, canary_window);
        let pramin_base = 0x0070_0000_usize + (vram_addr as usize & 0xFFFF);

        let original = bar0.read_u32(pramin_base).unwrap_or(0xDEAD_DEAD);
        pt(&format!("CANARY: original value at PRAMIN={original:#010x}"));

        let canary_val = 0xA5A5_BEEF_u32;
        pt(&format!("CANARY: writing {canary_val:#010x} to PRAMIN"));
        let _ = bar0.write_u32(pramin_base, canary_val);

        let readback = bar0.read_u32(pramin_base).unwrap_or(0xDEAD_DEAD);
        pt(&format!("CANARY: readback={readback:#010x} expected={canary_val:#010x}"));

        // Restore original value
        let _ = bar0.write_u32(pramin_base, original);
        let _ = bar0.write_u32(0x1700, saved_window);

        if readback != canary_val {
            err = Some(format!(
                "PRAMIN canary FAILED: wrote {canary_val:#010x}, read back {readback:#010x} — \
                 PRAMIN write path is stalled or broken. GPU needs SBR + warm cycle."
            ));
            pt(&format!("CANARY: FAILED — {}", err.as_deref().unwrap_or("")));
        } else {
            pt("CANARY: PASSED — PRAMIN write path verified");
        }
    }

    for (chunk_idx, chunk) in data.chunks(4096).enumerate() {
        if err.is_some() {
            break;
        }
        let offset = chunk_idx * 4096;

        if chunk_idx > 0 && chunk_idx % 4 == 0 {
            pt(&format!("CHUNK {chunk_idx}: PRI drain"));
            let _ = bar0.write_u32(0x0012_004C, 0x2);
            std::thread::sleep(std::time::Duration::from_micros(100));
        }

        pt(&format!(
            "CHUNK {chunk_idx}: PraminRegion::new(vram={:#x}, len={})",
            vram_addr + offset as u32,
            chunk.len()
        ));

        match PraminRegion::new(&bar0, vram_addr + offset as u32, chunk.len()) {
            Ok(mut rgn) => {
                pt(&format!(
                    "CHUNK {chunk_idx}: region OK — writing {} words",
                    chunk.len() / 4
                ));

                for (i, word) in chunk.chunks(4).enumerate() {
                    let val = le_bytes_to_u32(word);
                    if rgn.write_u32(i * 4, val).is_err() {
                        err = Some(format!(
                            "PRAMIN write failed at vram={vram_addr:#x} chunk={chunk_idx} word={i}"
                        ));
                        pt(&format!("CHUNK {chunk_idx}: WRITE FAILED word {i}"));
                        break;
                    }
                    if i > 0 && i % 128 == 0 {
                        pt(&format!("CHUNK {chunk_idx}: wrote {i}/{}", chunk.len() / 4));
                    }
                }

                if err.is_none() {
                    let rb = rgn.read_u32(0).unwrap_or(0xDEAD_DEAD);
                    pt(&format!("CHUNK {chunk_idx}: done, readback[0]={rb:#010x}"));
                    bytes_written += chunk.len();
                }
            }
            Err(e) => {
                pt(&format!("CHUNK {chunk_idx}: PraminRegion FAILED: {e}"));
                err = Some(format!("PRAMIN region open failed at offset {offset:#x}: {e}"));
                break;
            }
        }
        pt(&format!(
            "CHUNK {chunk_idx}: PraminRegion dropped (window restored)"
        ));
    }

    pt(&format!(
        "pramin_write DONE bytes={bytes_written} err={}",
        err.as_deref().unwrap_or("none")
    ));

    // Write result as JSON to the pipe (raw libc::write — safe after fork)
    let json = serde_json::json!({
        "bytes_written": bytes_written,
        "error": err,
    });
    if let Ok(bytes) = serde_json::to_vec(&json) {
        let mut written = 0usize;
        while written < bytes.len() {
            let n = unsafe {
                libc::write(pipe_fd, bytes[written..].as_ptr().cast(), bytes.len() - written)
            };
            if n <= 0 {
                break;
            }
            written += n as usize;
        }
    }

    // Do NOT let MappedBar::drop unmap — the parent still owns it.
    std::mem::forget(bar0);
}

/// `ember.pramin.write` — bulk VRAM write via PRAMIN window, server-side.
///
/// Runs in a **forked child process** for crash isolation. If the child's
/// CPU core stalls on a PCIe posted-write, the parent detects the timeout,
/// triggers a raw PCIe bus reset, kills the child, and marks the device
/// faulted. Ember stays alive.
///
/// Params: `{bdf, vram_addr, data_b64}` — data is base64-encoded bytes
/// Result: `{ok, bytes_written}`
pub(crate) fn pramin_write(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = require_bdf(params)?;
    let vram_addr = params
        .get("vram_addr")
        .and_then(|v| v.as_u64())
        .ok_or(EmberIpcError::InvalidRequest("missing 'vram_addr'"))? as u32;
    let data_b64 = params
        .get("data_b64")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'data_b64'"))?;

    let data = base64_decode(data_b64).map_err(|e| {
        EmberIpcError::InvalidRequest(Box::leak(format!("base64 decode: {e}").into_boxed_str()))
    })?;

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    let dev = require_held_mut(&mut map, bdf, stream, &id)?;

    if let Err(e) = map_bar0_if_needed(dev) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: BAR0 map failed: {e}"))
            .map_err(EmberIpcError::from);
    }

    if let Err(e) = preflight_gate(dev) {
        drop(map);
        return write_jsonrpc_error(stream, id, -32011, &e).map_err(EmberIpcError::from);
    }

    let bdf_owned = bdf.to_string();
    let bar0 = dev.bar0.as_ref().unwrap();
    let bar0_ptr = bar0.base_ptr() as usize;
    let bar0_size = bar0.size();

    // ── Warm cycle gate ──
    // After any experiment that runs DMA (SEC2 ACR boot, etc.), the GPU's
    // internal PRAMIN bulk write path becomes degraded. Sustained writes
    // (256+ words) stall, exhausting PCIe flow-control credits. On AMD Zen,
    // this cascades through the NBIO → data fabric and freezes ALL CPU cores
    // (not just the writing core). No software recovery is possible.
    //
    // The ONLY proven restoration is a full GPU warm cycle (nouveau bind →
    // unbind). We block PRAMIN writes after DMA experiments rather than
    // attempting a canary write (which triggers the same unrecoverable freeze).
    if dev.needs_warm_cycle {
        tracing::warn!(
            bdf,
            vram_addr = format_args!("{vram_addr:#x}"),
            data_len = data.len(),
            "PRAMIN write BLOCKED — GPU needs warm cycle after previous DMA experiment"
        );
        drop(map);
        return write_jsonrpc_error(
            stream, id, -32017,
            &format!("{bdf_owned}: PRAMIN write blocked. GPU bulk write path is degraded \
                after previous DMA experiment. Run a GPU warm cycle (nouveau bind/unbind) \
                to restore VRAM write capability, then retry."),
        ).map_err(EmberIpcError::from);
    }

    tracing::info!(
        bdf,
        vram_addr = format_args!("{vram_addr:#x}"),
        data_len = data.len(),
        "pramin_write: launching fork-isolated child"
    );

    let fork_result = crate::isolation::fork_isolated_mmio_bus_master_on(
        &bdf_owned,
        crate::isolation::OperationTier::BulkVram.timeout(),
        |pipe_fd| {
            pramin_write_child(bar0_ptr, bar0_size, vram_addr, &data, pipe_fd);
        },
    );

    match fork_result {
        crate::isolation::ForkResult::Ok(pipe_data) => {
            dev.experiment_dirty = true;

            // Parse result from child's pipe output
            let result: serde_json::Value = serde_json::from_slice(&pipe_data)
                .unwrap_or_else(|_| {
                    // Fallback: read the trace file
                    serde_json::json!({"bytes_written": 0, "error": "pipe parse failed"})
                });

            let bytes_written = result
                .get("bytes_written")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
            let child_error = result
                .get("error")
                .and_then(|v| v.as_str())
                .map(String::from);

            drop(map);

            if let Some(err_msg) = child_error {
                tracing::error!(bdf = bdf_owned, error = %err_msg, "pramin_write (fork): child reported error");
                return write_jsonrpc_error(stream, id, -32000, &err_msg)
                    .map_err(EmberIpcError::from);
            }

            tracing::info!(
                bdf = bdf_owned,
                vram_addr = format_args!("{vram_addr:#x}"),
                bytes_written,
                "pramin_write (fork): complete"
            );
            write_jsonrpc_ok(
                stream,
                id,
                serde_json::json!({"ok": true, "bytes_written": bytes_written}),
            )
            .map_err(EmberIpcError::from)
        }

        crate::isolation::ForkResult::Timeout => {
            tracing::error!(
                bdf = bdf_owned,
                vram_addr = format_args!("{vram_addr:#x}"),
                "pramin_write: fork child TIMED OUT — bus reset triggered, device FAULTED"
            );
            dev.emergency_quiesce();
            drop(map);
            crate::hold::check_voluntary_death(held);

            write_jsonrpc_error(
                stream,
                id,
                -32099,
                &format!(
                    "{bdf_owned}: pramin_write timed out in fork child — \
                     GPU was bus-reset. Device faulted. Use ember.device.recover."
                ),
            )
            .map_err(EmberIpcError::from)
        }

        crate::isolation::ForkResult::ChildFailed { status } => {
            tracing::error!(bdf = bdf_owned, status, "pramin_write: fork child failed");
            dev.emergency_quiesce();
            drop(map);
            crate::hold::check_voluntary_death(held);

            write_jsonrpc_error(
                stream,
                id,
                -32098,
                &format!(
                    "{bdf_owned}: pramin_write child failed (status={status}). Device faulted."
                ),
            )
            .map_err(EmberIpcError::from)
        }

        crate::isolation::ForkResult::ForkFailed(e) => {
            tracing::error!(bdf = bdf_owned, error = %e, "pramin_write: fork() failed");
            drop(map);
            write_jsonrpc_error(
                stream,
                id,
                -32097,
                &format!("{bdf_owned}: fork() failed: {e}"),
            )
            .map_err(EmberIpcError::from)
        }

        crate::isolation::ForkResult::PipeFailed(e) => {
            tracing::error!(bdf = bdf_owned, error = %e, "pramin_write: pipe() failed");
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

/// Execute bulk PRAMIN read in a forked child.
///
/// # Safety rationale
///
/// Same as [`pramin_write_child`]: `MappedBar::from_raw` for inherited mmap,
/// `libc::write` for pipe communication in post-fork context.
#[allow(unsafe_code)]
fn pramin_read_child(
    bar0_ptr: usize,
    bar0_size: usize,
    vram_addr: u32,
    length: usize,
    pipe_fd: i32,
) {
    let bar0 = unsafe {
        coral_driver::vfio::device::MappedBar::from_raw(bar0_ptr as *mut u8, bar0_size)
    };

    let mut result_data = Vec::with_capacity(length);
    let mut err: Option<String> = None;

    // Liveness probe
    let saved_window = bar0.read_u32(0x1700).unwrap_or(0xDEAD_DEAD);
    let _ = bar0.write_u32(0x1700, (vram_addr >> 16) as u32);
    let probe = bar0.read_u32(0x0070_0000).unwrap_or(0xDEAD_DEAD);
    let _ = bar0.write_u32(0x1700, saved_window);

    if probe == 0xDEAD_DEAD || probe == 0xFFFF_FFFF {
        err = Some(format!(
            "PRAMIN liveness FAILED: {probe:#010x} — BAR0 unresponsive"
        ));
    } else if probe & 0xFFF0_0000 == 0xBAD0_0000 {
        err = Some(format!(
            "PRAMIN liveness FAILED: {probe:#010x} — VRAM dead"
        ));
    }

    if err.is_none() {
        let _ = bar0.write_u32(0x12004C, 0x02);
        std::thread::sleep(std::time::Duration::from_millis(5));
        let boot0_check = bar0.read_u32(0x000).unwrap_or(0xDEAD_DEAD);
        if boot0_check == 0xDEAD_DEAD || boot0_check == 0xFFFF_FFFF
            || boot0_check & 0xFFF0_0000 == 0xBAD0_0000
        {
            err = Some(format!(
                "PRI ring dead after drain: BOOT0={boot0_check:#010x}"
            ));
        }
    }

    for (chunk_idx, _) in (0..length).step_by(4096).enumerate() {
        if err.is_some() {
            break;
        }
        let chunk_byte_offset = chunk_idx * 4096;
        let chunk_len = 4096.min(length - chunk_byte_offset);
        match PraminRegion::new(&bar0, vram_addr + chunk_byte_offset as u32, chunk_len) {
            Ok(rgn) => {
                let word_count = chunk_len.div_ceil(4);
                for i in 0..word_count {
                    let val = rgn.read_u32(i * 4).unwrap_or(0xDEAD_DEAD);
                    result_data.extend_from_slice(&val.to_le_bytes());
                }
            }
            Err(e) => {
                err = Some(format!("PRAMIN region open failed: {e}"));
                break;
            }
        }
    }
    result_data.truncate(length);

    // Write result: base64-encoded data or error
    let json = if let Some(e) = err {
        serde_json::json!({"error": e})
    } else {
        serde_json::json!({"data_b64": base64_encode(&result_data), "length": result_data.len()})
    };
    if let Ok(bytes) = serde_json::to_vec(&json) {
        let mut written = 0usize;
        while written < bytes.len() {
            let n = unsafe {
                libc::write(pipe_fd, bytes[written..].as_ptr().cast(), bytes.len() - written)
            };
            if n <= 0 {
                break;
            }
            written += n as usize;
        }
    }

    std::mem::forget(bar0);
}

/// `ember.pramin.read` — bulk VRAM read via PRAMIN window, server-side.
///
/// Runs in a **forked child process** for crash isolation (same as write).
///
/// Params: `{bdf, vram_addr, length}`
/// Result: `{data_b64}`
pub(crate) fn pramin_read(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = require_bdf(params)?;
    let vram_addr = params
        .get("vram_addr")
        .and_then(|v| v.as_u64())
        .ok_or(EmberIpcError::InvalidRequest("missing 'vram_addr'"))? as u32;
    let length = params
        .get("length")
        .and_then(|v| v.as_u64())
        .ok_or(EmberIpcError::InvalidRequest("missing 'length'"))? as usize;

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    let dev = require_held_mut(&mut map, bdf, stream, &id)?;

    if let Err(e) = map_bar0_if_needed(dev) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: BAR0 map failed: {e}"))
            .map_err(EmberIpcError::from);
    }

    if let Err(e) = preflight_gate(dev) {
        drop(map);
        return write_jsonrpc_error(stream, id, -32011, &e).map_err(EmberIpcError::from);
    }

    let bdf_owned = bdf.to_string();
    let bar0 = dev.bar0.as_ref().unwrap();
    let bar0_ptr = bar0.base_ptr() as usize;
    let bar0_size = bar0.size();

    tracing::info!(
        bdf,
        vram_addr = format_args!("{vram_addr:#x}"),
        length,
        "pramin_read: launching fork-isolated child"
    );

    let fork_result = crate::isolation::fork_isolated_mmio_bus_master_on(
        &bdf_owned,
        crate::isolation::OperationTier::BulkVram.timeout(),
        |pipe_fd| {
            pramin_read_child(bar0_ptr, bar0_size, vram_addr, length, pipe_fd);
        },
    );

    match fork_result {
        crate::isolation::ForkResult::Ok(pipe_data) => {
            drop(map);

            let result: serde_json::Value =
                serde_json::from_slice(&pipe_data).unwrap_or_else(|_| {
                    serde_json::json!({"error": "pipe parse failed"})
                });

            if let Some(err_msg) = result.get("error").and_then(|v| v.as_str()) {
                return write_jsonrpc_error(stream, id, -32000, err_msg)
                    .map_err(EmberIpcError::from);
            }

            tracing::info!(bdf = bdf_owned, vram_addr = format_args!("{vram_addr:#x}"), length, "pramin_read (fork): complete");
            write_jsonrpc_ok(stream, id, result).map_err(EmberIpcError::from)
        }

        crate::isolation::ForkResult::Timeout => {
            dev.emergency_quiesce();
            drop(map);
            crate::hold::check_voluntary_death(held);

            write_jsonrpc_error(
                stream,
                id,
                -32099,
                &format!(
                    "{bdf_owned}: pramin_read timed out — bus reset triggered. Device faulted."
                ),
            )
            .map_err(EmberIpcError::from)
        }

        crate::isolation::ForkResult::ChildFailed { status } => {
            dev.emergency_quiesce();
            drop(map);
            crate::hold::check_voluntary_death(held);
            write_jsonrpc_error(
                stream,
                id,
                -32098,
                &format!("{bdf_owned}: pramin_read child failed (status={status})."),
            )
            .map_err(EmberIpcError::from)
        }

        crate::isolation::ForkResult::ForkFailed(e) => {
            drop(map);
            write_jsonrpc_error(stream, id, -32097, &format!("{bdf_owned}: fork() failed: {e}"))
                .map_err(EmberIpcError::from)
        }

        crate::isolation::ForkResult::PipeFailed(e) => {
            drop(map);
            write_jsonrpc_error(stream, id, -32096, &format!("{bdf_owned}: pipe() failed: {e}"))
                .map_err(EmberIpcError::from)
        }
    }
}
