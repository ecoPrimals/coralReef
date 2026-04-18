// SPDX-License-Identifier: AGPL-3.0-or-later
//! JSON-RPC handlers for fork-isolated BAR0 MMIO operations.
//!
//! All register access goes through [`coral_driver::vfio::isolation`] so that
//! a D-state in the child is killed after the timeout rather than blocking the
//! ember daemon.
//!
//! Layer 1 — raw BAR0 proxy:
//!   `mmio.read32`, `mmio.write32`, `mmio.batch`, `mmio.pramin.read32`
//!
//! Layer 2 — experiment helpers:
//!   `mmio.falcon.status`, `mmio.bar0.probe`

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use coral_driver::vfio::device::MappedBar;
use coral_driver::vfio::isolation::IsolationResult;

use crate::error::EmberIpcError;
use crate::hold::HeldDevice;

use super::jsonrpc::{write_jsonrpc_error, write_jsonrpc_ok};

const MMIO_TIMEOUT: Duration = Duration::from_secs(3);

fn held_bar0(
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    bdf: &str,
) -> Result<MappedBar, String> {
    let map = held.read().map_err(|_| "lock poisoned".to_string())?;
    let dev = map
        .get(bdf)
        .ok_or_else(|| format!("device {bdf} not held by ember"))?;
    dev.device
        .map_bar(0)
        .map_err(|e| format!("BAR0 map failed: {e}"))
}

// ─── Layer 1: raw BAR0 proxy ────────────────────────────────────────

pub(crate) fn read32(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf'"))?;
    let offset = params
        .get("offset")
        .and_then(|v| v.as_u64())
        .ok_or(EmberIpcError::InvalidRequest("missing 'offset'"))? as u32;

    let bar0 = match held_bar0(held, bdf) {
        Ok(b) => b,
        Err(e) => {
            write_jsonrpc_error(stream, id, -32000, &e).map_err(EmberIpcError::from)?;
            return Ok(());
        }
    };

    let result = bar0.isolated_read_u32(offset, MMIO_TIMEOUT);

    match result {
        IsolationResult::Ok(val) => {
            write_jsonrpc_ok(
                stream,
                id,
                serde_json::json!({ "value": val, "offset": offset }),
            )
            .map_err(EmberIpcError::from)?;
        }
        IsolationResult::Timeout => {
            write_jsonrpc_error(
                stream,
                id,
                -32001,
                &format!("MMIO read at 0x{offset:x} timed out (BAR0 D-state)"),
            )
            .map_err(EmberIpcError::from)?;
        }
        IsolationResult::ChildFailed { status } => {
            write_jsonrpc_error(
                stream,
                id,
                -32001,
                &format!("MMIO read child failed: status={status}"),
            )
            .map_err(EmberIpcError::from)?;
        }
        IsolationResult::ForkError(e) => {
            write_jsonrpc_error(stream, id, -32000, &format!("fork error: {e}"))
                .map_err(EmberIpcError::from)?;
        }
    }
    Ok(())
}

pub(crate) fn write32(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf'"))?;
    let offset = params
        .get("offset")
        .and_then(|v| v.as_u64())
        .ok_or(EmberIpcError::InvalidRequest("missing 'offset'"))? as u32;
    let value = params
        .get("value")
        .and_then(|v| v.as_u64())
        .ok_or(EmberIpcError::InvalidRequest("missing 'value'"))? as u32;

    let bar0 = match held_bar0(held, bdf) {
        Ok(b) => b,
        Err(e) => {
            write_jsonrpc_error(stream, id, -32000, &e).map_err(EmberIpcError::from)?;
            return Ok(());
        }
    };

    let result = bar0.isolated_write_u32(offset, value, MMIO_TIMEOUT);

    match result {
        IsolationResult::Ok(()) => {
            write_jsonrpc_ok(
                stream,
                id,
                serde_json::json!({ "ok": true, "offset": offset }),
            )
            .map_err(EmberIpcError::from)?;
        }
        IsolationResult::Timeout => {
            write_jsonrpc_error(
                stream,
                id,
                -32001,
                &format!("MMIO write at 0x{offset:x} timed out (BAR0 D-state)"),
            )
            .map_err(EmberIpcError::from)?;
        }
        IsolationResult::ChildFailed { status } => {
            write_jsonrpc_error(
                stream,
                id,
                -32001,
                &format!("MMIO write child failed: status={status}"),
            )
            .map_err(EmberIpcError::from)?;
        }
        IsolationResult::ForkError(e) => {
            write_jsonrpc_error(stream, id, -32000, &format!("fork error: {e}"))
                .map_err(EmberIpcError::from)?;
        }
    }
    Ok(())
}

pub(crate) fn batch(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf'"))?;
    let ops_json = params
        .get("ops")
        .and_then(|v| v.as_array())
        .ok_or(EmberIpcError::InvalidRequest(
            "missing 'ops' array of {offset, value?}",
        ))?;

    let mut ops: Vec<(u32, Option<u32>)> = Vec::with_capacity(ops_json.len());
    for op in ops_json {
        let offset = op
            .get("offset")
            .and_then(|v| v.as_u64())
            .ok_or(EmberIpcError::InvalidRequest("op missing 'offset'"))?
            as u32;
        let value = op.get("value").and_then(|v| v.as_u64()).map(|v| v as u32);
        ops.push((offset, value));
    }

    let bar0 = match held_bar0(held, bdf) {
        Ok(b) => b,
        Err(e) => {
            write_jsonrpc_error(stream, id, -32000, &e).map_err(EmberIpcError::from)?;
            return Ok(());
        }
    };

    let result = bar0.isolated_batch(&ops, MMIO_TIMEOUT);

    match result {
        IsolationResult::Ok(values) => {
            let results: Vec<serde_json::Value> = ops
                .iter()
                .zip(values.iter())
                .map(|((offset, _), val)| serde_json::json!({"offset": offset, "value": val}))
                .collect();
            write_jsonrpc_ok(stream, id, serde_json::json!({ "results": results }))
                .map_err(EmberIpcError::from)?;
        }
        IsolationResult::Timeout => {
            write_jsonrpc_error(stream, id, -32001, "MMIO batch timed out (BAR0 D-state)")
                .map_err(EmberIpcError::from)?;
        }
        IsolationResult::ChildFailed { status } => {
            write_jsonrpc_error(
                stream,
                id,
                -32001,
                &format!("MMIO batch child failed: status={status}"),
            )
            .map_err(EmberIpcError::from)?;
        }
        IsolationResult::ForkError(e) => {
            write_jsonrpc_error(stream, id, -32000, &format!("fork error: {e}"))
                .map_err(EmberIpcError::from)?;
        }
    }
    Ok(())
}

const BAR0_WINDOW: u32 = 0x0000_1700;
const PRAMIN_BASE: u32 = 0x0070_0000;

pub(crate) fn pramin_read32(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf'"))?;
    let vram_offset = params
        .get("vram_offset")
        .and_then(|v| v.as_u64())
        .ok_or(EmberIpcError::InvalidRequest("missing 'vram_offset'"))?
        as u32;

    let bar0 = match held_bar0(held, bdf) {
        Ok(b) => b,
        Err(e) => {
            write_jsonrpc_error(stream, id, -32000, &e).map_err(EmberIpcError::from)?;
            return Ok(());
        }
    };

    let window_page = vram_offset >> 16;
    let page_offset = vram_offset & 0xFFFF;

    let ops = vec![
        (BAR0_WINDOW, Some(window_page)),
        (PRAMIN_BASE + page_offset, None),
    ];

    let result = bar0.isolated_batch(&ops, MMIO_TIMEOUT);

    match result {
        IsolationResult::Ok(values) => {
            let readback = values.get(1).copied().unwrap_or(0);
            write_jsonrpc_ok(
                stream,
                id,
                serde_json::json!({
                    "value": readback,
                    "vram_offset": vram_offset,
                    "window_page": window_page,
                }),
            )
            .map_err(EmberIpcError::from)?;
        }
        IsolationResult::Timeout => {
            write_jsonrpc_error(
                stream,
                id,
                -32001,
                &format!(
                    "PRAMIN read at VRAM 0x{vram_offset:x} timed out (BAR0 window D-state)"
                ),
            )
            .map_err(EmberIpcError::from)?;
        }
        IsolationResult::ChildFailed { status } => {
            write_jsonrpc_error(
                stream,
                id,
                -32001,
                &format!("PRAMIN read child failed: status={status}"),
            )
            .map_err(EmberIpcError::from)?;
        }
        IsolationResult::ForkError(e) => {
            write_jsonrpc_error(stream, id, -32000, &format!("fork error: {e}"))
                .map_err(EmberIpcError::from)?;
        }
    }
    Ok(())
}

// ─── Layer 2: experiment helpers ────────────────────────────────────

const NV_PMC_BOOT_0: u32 = 0x0000_0000;
const NV_PMC_BOOT_42: u32 = 0x0000_0000;
const NV_PMC_ENABLE: u32 = 0x0000_0200;
const NV_PMC_INTR_EN_0: u32 = 0x0000_0140;
const NV_PBUS_PCI_NV_0: u32 = 0x0000_1800;
const NV_PTIMER_TIME_0: u32 = 0x0000_9400;
const NV_PTIMER_TIME_1: u32 = 0x0000_9410;

pub(crate) fn bar0_probe(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf'"))?;

    let bar0 = match held_bar0(held, bdf) {
        Ok(b) => b,
        Err(e) => {
            write_jsonrpc_error(stream, id, -32000, &e).map_err(EmberIpcError::from)?;
            return Ok(());
        }
    };

    let probe_regs: Vec<(u32, Option<u32>)> = vec![
        (NV_PMC_BOOT_0, None),
        (NV_PMC_BOOT_42, None),
        (NV_PMC_ENABLE, None),
        (NV_PMC_INTR_EN_0, None),
        (NV_PBUS_PCI_NV_0, None),
        (NV_PTIMER_TIME_0, None),
        (NV_PTIMER_TIME_1, None),
    ];

    let result = bar0.isolated_batch(&probe_regs, MMIO_TIMEOUT);

    match result {
        IsolationResult::Ok(values) => {
            let boot0 = values.first().copied().unwrap_or(0);
            let chip_id = (boot0 >> 20) & 0x1FF;

            write_jsonrpc_ok(
                stream,
                id,
                serde_json::json!({
                    "bdf": bdf,
                    "boot0": format!("0x{:08x}", boot0),
                    "chip_id": format!("0x{:03x}", chip_id),
                    "pmc_enable": format!("0x{:08x}", values.get(2).unwrap_or(&0)),
                    "intr_en_0": format!("0x{:08x}", values.get(3).unwrap_or(&0)),
                    "pci_nv_0": format!("0x{:08x}", values.get(4).unwrap_or(&0)),
                    "ptimer_lo": format!("0x{:08x}", values.get(5).unwrap_or(&0)),
                    "ptimer_hi": format!("0x{:08x}", values.get(6).unwrap_or(&0)),
                }),
            )
            .map_err(EmberIpcError::from)?;
        }
        IsolationResult::Timeout => {
            write_jsonrpc_error(stream, id, -32001, "BAR0 probe timed out (GPU unreachable)")
                .map_err(EmberIpcError::from)?;
        }
        IsolationResult::ChildFailed { status } => {
            write_jsonrpc_error(
                stream,
                id,
                -32001,
                &format!("BAR0 probe child failed: status={status}"),
            )
            .map_err(EmberIpcError::from)?;
        }
        IsolationResult::ForkError(e) => {
            write_jsonrpc_error(stream, id, -32000, &format!("fork error: {e}"))
                .map_err(EmberIpcError::from)?;
        }
    }
    Ok(())
}

pub(crate) fn falcon_status(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf'"))?;
    let engine = params
        .get("engine")
        .and_then(|v| v.as_str())
        .unwrap_or("fecs");

    let base = match engine {
        "fecs" => 0x0040_9000_u32,
        "gpccs" => 0x0041_a000_u32,
        "sec2" => 0x0008_7000_u32,
        "pmu" => 0x0010_a000_u32,
        "nvdec" => 0x0084_8000_u32,
        other => {
            write_jsonrpc_error(
                stream,
                id,
                -32602,
                &format!("unknown falcon engine: {other}"),
            )
            .map_err(EmberIpcError::from)?;
            return Ok(());
        }
    };

    let bar0 = match held_bar0(held, bdf) {
        Ok(b) => b,
        Err(e) => {
            write_jsonrpc_error(stream, id, -32000, &e).map_err(EmberIpcError::from)?;
            return Ok(());
        }
    };

    let ops: Vec<(u32, Option<u32>)> = vec![
        (base + 0x100, None), // CPUCTL
        (base + 0x040, None), // MAILBOX0
        (base + 0x044, None), // MAILBOX1
        (base + 0x240, None), // SCTL
        (base + 0x104, None), // BOOTVEC
        (base + 0x080, None), // OS
        (base + 0x108, None), // HWCFG
        (base + 0x030, None), // PC
        (base + 0x148, None), // EXCI
    ];

    let result = bar0.isolated_batch(&ops, MMIO_TIMEOUT);

    match result {
        IsolationResult::Ok(values) => {
            let cpuctl = *values.first().unwrap_or(&0);
            let sctl = *values.get(3).unwrap_or(&0);
            let sec_mode = (sctl >> 12) & 3;
            let halted = cpuctl & 0x20 != 0;
            let hreset = cpuctl & 0x10 != 0;
            write_jsonrpc_ok(
                stream,
                id,
                serde_json::json!({
                    "engine": engine,
                    "base": format!("0x{base:08x}"),
                    "cpuctl": format!("0x{cpuctl:08x}"),
                    "mailbox0": format!("0x{:08x}", values.get(1).unwrap_or(&0)),
                    "mailbox1": format!("0x{:08x}", values.get(2).unwrap_or(&0)),
                    "sctl": format!("0x{sctl:08x}"),
                    "bootvec": format!("0x{:08x}", values.get(4).unwrap_or(&0)),
                    "os": format!("0x{:08x}", values.get(5).unwrap_or(&0)),
                    "hwcfg": format!("0x{:08x}", values.get(6).unwrap_or(&0)),
                    "pc": format!("0x{:08x}", values.get(7).unwrap_or(&0)),
                    "exci": format!("0x{:08x}", values.get(8).unwrap_or(&0)),
                    "sec_mode": sec_mode,
                    "halted": halted,
                    "hreset": hreset,
                }),
            )
            .map_err(EmberIpcError::from)?;
        }
        IsolationResult::Timeout => {
            write_jsonrpc_error(
                stream,
                id,
                -32001,
                &format!("{engine} falcon status timed out (engine unreachable)"),
            )
            .map_err(EmberIpcError::from)?;
        }
        IsolationResult::ChildFailed { status } => {
            write_jsonrpc_error(
                stream,
                id,
                -32001,
                &format!("{engine} falcon child failed: status={status}"),
            )
            .map_err(EmberIpcError::from)?;
        }
        IsolationResult::ForkError(e) => {
            write_jsonrpc_error(stream, id, -32000, &format!("fork error: {e}"))
                .map_err(EmberIpcError::from)?;
        }
    }
    Ok(())
}
