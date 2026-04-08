// SPDX-License-Identifier: AGPL-3.0-only
//! Firmware management handlers — ember as sovereign firmware intermediary.
//!
//! These RPCs make ember the central authority for GPU firmware lifecycle:
//! inventory, loading, caching, and sovereign init orchestration. Firmware
//! blobs are treated as ingredients — loaded from `/lib/firmware/nvidia/`
//! and injected into hardware by the Rust driver.

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, RwLock};

use crate::error::EmberIpcError;
use crate::hold::HeldDevice;
use crate::isolation::{self, ForkResult};

use super::super::jsonrpc::{write_jsonrpc_error, write_jsonrpc_ok};
use super::{map_bar0_if_needed, preflight_gate, require_bdf, update_fault_counter};

/// `ember.firmware.inventory` — probe firmware availability for a held device.
///
/// Reads BOOT0 to determine chip identity, then checks `/lib/firmware/nvidia/{chip}/`
/// for each subsystem (ACR, GR, SEC2, PMU, GSP, NVDEC). Also probes VBIOS PROM
/// availability via fork-isolated BAR0 read.
///
/// Params: `{bdf}`
/// Result: `{bdf, chip, sm, acr, gr, sec2, pmu, gsp, nvdec, vbios_prom, compute_viable, blockers}`
pub(crate) fn firmware_inventory(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = require_bdf(params)?;

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    let Some(dev) = map.get_mut(bdf) else {
        write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: not held by ember"))
            .map_err(EmberIpcError::from)?;
        return Ok(());
    };

    if let Err(e) = map_bar0_if_needed(dev) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: BAR0 map failed: {e}"))
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

    let fork_result = isolation::fork_isolated_mmio(
        &bdf_owned,
        std::time::Duration::from_secs(10),
        |pipe_fd| {
            let bar0 = unsafe {
                coral_driver::vfio::device::MappedBar::from_raw(bar0_ptr as *mut u8, bar0_size)
            };

            let boot0 = bar0.read_u32(0).unwrap_or(0xFFFF_FFFF);
            let sm = coral_driver::nv::identity::boot0_to_sm(boot0).unwrap_or(0);
            let chip = coral_driver::nv::identity::chip_name(sm);

            let inv = coral_driver::nv::identity::firmware_inventory(chip);

            let vbios_available =
                coral_driver::vfio::channel::devinit::read_vbios_prom(&bar0).is_ok();

            let payload = serde_json::json!({
                "boot0": format!("{boot0:#010x}"),
                "chip": chip,
                "sm": sm,
                "acr": format!("{:?}", inv.acr),
                "gr": format!("{:?}", inv.gr),
                "sec2": format!("{:?}", inv.sec2),
                "pmu": format!("{:?}", inv.pmu),
                "gsp": format!("{:?}", inv.gsp),
                "nvdec": format!("{:?}", inv.nvdec),
                "vbios_prom": vbios_available,
                "compute_viable": inv.compute_viable(),
                "blockers": inv.compute_blockers(),
            });
            let bytes = serde_json::to_vec(&payload).unwrap_or_default();
            unsafe { libc::write(pipe_fd, bytes.as_ptr().cast(), bytes.len()); }
        },
    );

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    if let Some(dev) = map.get_mut(&bdf_owned) {
        match &fork_result {
            ForkResult::Ok(data) => {
                let boot0 = parse_boot0_from_json(data);
                let _ = update_fault_counter(dev, boot0);
            }
            ForkResult::Timeout | ForkResult::ChildFailed { .. } => {
                dev.mmio_fault_count += 1;
            }
            ForkResult::ForkFailed(_) | ForkResult::PipeFailed(_) => {}
        }
    }
    drop(map);

    handle_fork_result(stream, id, fork_result, &bdf_owned, "firmware inventory")
}

/// `ember.firmware.load` — load and validate firmware blobs for a held device.
///
/// Reads BOOT0 to determine chip, then loads ACR firmware set and GR firmware
/// blobs from `/lib/firmware/nvidia/{chip}/`. Reports parse statistics and
/// file counts. Loading is fork-isolated (filesystem I/O only, no MMIO beyond
/// BOOT0).
///
/// Params: `{bdf}`
/// Result: `{bdf, chip, acr_loaded, gr_loaded, acr_files, gr_bundles, gr_methods, error?}`
pub(crate) fn firmware_load(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = require_bdf(params)?;

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    let Some(dev) = map.get_mut(bdf) else {
        write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: not held by ember"))
            .map_err(EmberIpcError::from)?;
        return Ok(());
    };

    if let Err(e) = map_bar0_if_needed(dev) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: BAR0 map failed: {e}"))
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

    let fork_result = isolation::fork_isolated_mmio(
        &bdf_owned,
        std::time::Duration::from_secs(15),
        |pipe_fd| {
            let bar0 = unsafe {
                coral_driver::vfio::device::MappedBar::from_raw(bar0_ptr as *mut u8, bar0_size)
            };

            let boot0 = bar0.read_u32(0).unwrap_or(0xFFFF_FFFF);
            let sm = coral_driver::nv::identity::boot0_to_sm(boot0).unwrap_or(0);
            let chip = coral_driver::nv::identity::chip_name(sm);

            let acr_result =
                coral_driver::nv::vfio_compute::acr_boot::AcrFirmwareSet::load(chip);
            let (acr_loaded, acr_error) = match &acr_result {
                Ok(_) => (true, None),
                Err(e) => (false, Some(format!("{e}"))),
            };

            let gr_result = coral_driver::gsp::GrFirmwareBlobs::parse(chip);
            let (gr_loaded, gr_bundles, gr_methods, gr_error) = match &gr_result {
                Ok(blobs) => (
                    true,
                    blobs.bundle_init.len(),
                    blobs.method_init.len(),
                    None,
                ),
                Err(e) => (false, 0, 0, Some(format!("{e}"))),
            };

            let payload = serde_json::json!({
                "boot0": format!("{boot0:#010x}"),
                "chip": chip,
                "sm": sm,
                "acr_loaded": acr_loaded,
                "acr_error": acr_error,
                "gr_loaded": gr_loaded,
                "gr_bundles": gr_bundles,
                "gr_methods": gr_methods,
                "gr_error": gr_error,
            });
            let bytes = serde_json::to_vec(&payload).unwrap_or_default();
            unsafe { libc::write(pipe_fd, bytes.as_ptr().cast(), bytes.len()); }
        },
    );

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    if let Some(dev) = map.get_mut(&bdf_owned) {
        match &fork_result {
            ForkResult::Ok(data) => {
                let boot0 = parse_boot0_from_json(data);
                let _ = update_fault_counter(dev, boot0);
            }
            ForkResult::Timeout | ForkResult::ChildFailed { .. } => {
                dev.mmio_fault_count += 1;
            }
            ForkResult::ForkFailed(_) | ForkResult::PipeFailed(_) => {}
        }
    }
    drop(map);

    handle_fork_result(stream, id, fork_result, &bdf_owned, "firmware load")
}

/// `ember.sovereign.init` — run the full SovereignInit pipeline on a held device.
///
/// Orchestrates: HBM2 training → PMC gating → topology discovery → PFB →
/// falcon boot (15 strategies) → GR init → PFIFO discovery. Fork-isolated
/// with a 60s timeout (full cold boot is the heaviest operation).
///
/// Params: `{bdf, cold?: bool}`
/// Result: `{bdf, chip, hbm2_trained, pmc_gated, topology_ok, pfb_ok, falcons_alive,
///          gr_ready, pfifo_ready, fecs_responsive, compute_ready, diagnostics}`
#[allow(unsafe_code)]
pub(crate) fn sovereign_init(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = require_bdf(params)?;

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    let Some(dev) = map.get_mut(bdf) else {
        write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: not held by ember"))
            .map_err(EmberIpcError::from)?;
        return Ok(());
    };

    if let Err(e) = map_bar0_if_needed(dev) {
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: BAR0 map failed: {e}"))
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

    let fork_result = isolation::fork_isolated_mmio(
        &bdf_owned,
        std::time::Duration::from_secs(60),
        |pipe_fd| {
            let bar0 = unsafe {
                coral_driver::vfio::device::MappedBar::from_raw(bar0_ptr as *mut u8, bar0_size)
            };

            let boot0 = bar0.read_u32(0).unwrap_or(0xFFFF_FFFF);
            let sm = coral_driver::nv::identity::boot0_to_sm(boot0).unwrap_or(70);
            let chip = coral_driver::nv::identity::chip_name(sm);

            let pipeline =
                coral_driver::nv::vfio_compute::sovereign_init::SovereignInit::new(&bar0, sm)
                    .with_bdf(&bdf_owned);

            let result = pipeline.init_all();

            let total_applied: u32 = result.stages.iter().map(|s| s.writes_applied).sum();
            let total_failed: u32 = result.stages.iter().map(|s| s.writes_failed).sum();

            let payload = serde_json::json!({
                "boot0": format!("{boot0:#010x}"),
                "chip": chip,
                "sm": sm,
                "hbm2_trained": result.hbm2_trained,
                "falcons_alive": result.falcons_alive,
                "gr_ready": result.gr_ready,
                "pfifo_ready": result.pfifo_ready,
                "fecs_responsive": result.fecs_responsive,
                "compute_ready": result.compute_ready(),
                "stages": result.stages.len(),
                "all_ok": result.all_ok(),
                "writes_applied": total_applied,
                "writes_failed": total_failed,
                "topology": result.topology.as_ref().map(|t| serde_json::json!({
                    "gpc": t.gpc_count,
                    "sm": t.sm_count,
                    "fbp": t.fbp_count,
                    "pbdma": t.pbdma_count,
                })),
                "diagnostics": result.diagnostic_summary(),
            });
            let bytes = serde_json::to_vec(&payload).unwrap_or_default();
            unsafe { libc::write(pipe_fd, bytes.as_ptr().cast(), bytes.len()); }
        },
    );

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    if let Some(dev) = map.get_mut(&bdf_owned) {
        match &fork_result {
            ForkResult::Ok(data) => {
                let compute_ready = serde_json::from_slice::<serde_json::Value>(data)
                    .ok()
                    .and_then(|v| v.get("compute_ready").and_then(|b| b.as_bool()))
                    .unwrap_or(false);
                if compute_ready {
                    dev.health = crate::hold::DeviceHealth::Active;
                }
                let boot0_str = serde_json::from_slice::<serde_json::Value>(data)
                    .ok()
                    .and_then(|v| v.get("boot0").and_then(|b| b.as_str()).map(String::from));
                let boot0 = boot0_str
                    .and_then(|s| {
                        u32::from_str_radix(s.trim_start_matches("0x"), 16).ok()
                    })
                    .unwrap_or(0xFFFF_FFFF);
                let _ = update_fault_counter(dev, boot0);
                dev.experiment_dirty = true;
            }
            ForkResult::Timeout | ForkResult::ChildFailed { .. } => {
                dev.mmio_fault_count += 1;
                dev.health = crate::hold::DeviceHealth::Faulted;
                tracing::error!(
                    bdf = %bdf_owned,
                    "sovereign init fork failed — device marked faulted"
                );
            }
            ForkResult::ForkFailed(_) | ForkResult::PipeFailed(_) => {}
        }
    }
    drop(map);

    handle_fork_result(stream, id, fork_result, &bdf_owned, "sovereign init")
}

// ── Shared helpers ──────────────────────────────────────────────────

fn parse_boot0_from_json(data: &[u8]) -> u32 {
    let val: serde_json::Value = match serde_json::from_slice(data) {
        Ok(v) => v,
        Err(_) => return 0xFFFF_FFFF,
    };
    val.get("boot0")
        .and_then(|b| b.as_str())
        .and_then(|s| u32::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .unwrap_or(0xFFFF_FFFF)
}

fn handle_fork_result(
    stream: &mut impl Write,
    id: serde_json::Value,
    fork_result: ForkResult,
    bdf: &str,
    op_name: &str,
) -> Result<(), EmberIpcError> {
    match fork_result {
        ForkResult::Ok(data) => match serde_json::from_slice::<serde_json::Value>(&data) {
            Ok(mut result) => {
                result
                    .as_object_mut()
                    .map(|o| o.insert("bdf".into(), serde_json::Value::String(bdf.to_string())));
                write_jsonrpc_ok(stream, id, result).map_err(EmberIpcError::from)
            }
            Err(e) => write_jsonrpc_error(
                stream,
                id,
                -32000,
                &format!("{bdf}: {op_name} parse error: {e}"),
            )
            .map_err(EmberIpcError::from),
        },
        ForkResult::Timeout => write_jsonrpc_error(
            stream,
            id,
            -32000,
            &format!("{bdf}: {op_name} timed out"),
        )
        .map_err(EmberIpcError::from),
        ForkResult::ChildFailed { status } => write_jsonrpc_error(
            stream,
            id,
            -32000,
            &format!("{bdf}: {op_name} fork crashed (exit {status})"),
        )
        .map_err(EmberIpcError::from),
        ForkResult::ForkFailed(e) | ForkResult::PipeFailed(e) => {
            write_jsonrpc_error(stream, id, -32000, &format!("{bdf}: fork failed: {e}"))
                .map_err(EmberIpcError::from)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firmware_inventory_missing_bdf_returns_error() {
        let held = Arc::new(RwLock::new(HashMap::<String, HeldDevice>::new()));
        let params = serde_json::json!({});
        let mut buf = Vec::new();

        let result = firmware_inventory(&mut buf, &held, serde_json::json!(1), &params);
        assert!(result.is_err() || {
            let s = String::from_utf8_lossy(&buf);
            s.contains("missing") || s.contains("bdf")
        });
    }

    #[test]
    fn firmware_load_missing_bdf_returns_error() {
        let held = Arc::new(RwLock::new(HashMap::<String, HeldDevice>::new()));
        let params = serde_json::json!({});
        let mut buf = Vec::new();

        let result = firmware_load(&mut buf, &held, serde_json::json!(1), &params);
        assert!(result.is_err() || {
            let s = String::from_utf8_lossy(&buf);
            s.contains("missing") || s.contains("bdf")
        });
    }

    #[test]
    fn sovereign_init_missing_bdf_returns_error() {
        let held = Arc::new(RwLock::new(HashMap::<String, HeldDevice>::new()));
        let params = serde_json::json!({});
        let mut buf = Vec::new();

        let result = sovereign_init(&mut buf, &held, serde_json::json!(1), &params);
        assert!(result.is_err() || {
            let s = String::from_utf8_lossy(&buf);
            s.contains("missing") || s.contains("bdf")
        });
    }

    #[test]
    fn firmware_inventory_device_not_held() {
        let held = Arc::new(RwLock::new(HashMap::<String, HeldDevice>::new()));
        let params = serde_json::json!({"bdf": "0000:65:00.0"});
        let mut buf = Vec::new();

        let _ = firmware_inventory(&mut buf, &held, serde_json::json!(1), &params);
        let s = String::from_utf8_lossy(&buf);
        assert!(s.contains("not held"), "expected 'not held' error, got: {s}");
    }
}
