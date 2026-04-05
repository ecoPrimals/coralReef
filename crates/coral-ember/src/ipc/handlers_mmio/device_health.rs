// SPDX-License-Identifier: AGPL-3.0-only
//! Device health query and recovery handlers.

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, RwLock};

use crate::error::EmberIpcError;
use crate::hold::{DeviceHealth, HeldDevice};

use super::super::jsonrpc::{write_jsonrpc_error, write_jsonrpc_ok};
use super::{map_bar0_if_needed, require_bdf, require_held_mut};

/// `ember.device.health` — query device health state.
///
/// Params: `{bdf}`
/// Result: `{bdf, health, mmio_fault_count, circuit_breaker_threshold}`
pub(crate) fn device_health(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = require_bdf(params)?;

    let map = held.read().map_err(|_| EmberIpcError::LockPoisoned)?;
    let dev = map
        .get(bdf)
        .ok_or(EmberIpcError::InvalidRequest("device not held"))?;

    let result = serde_json::json!({
        "bdf": bdf,
        "health": dev.health,
        "mmio_fault_count": dev.mmio_fault_count,
        "circuit_breaker_threshold": crate::hold::MMIO_CIRCUIT_BREAKER_THRESHOLD,
    });

    drop(map);
    write_jsonrpc_ok(stream, id, result).map_err(EmberIpcError::from)
}

/// `ember.device.recover` — attempt recovery of a faulted device.
///
/// When a device is in `Faulted` state (e.g., after an MMIO watchdog timeout
/// triggered a bus reset), this handler attempts to bring it back:
///
/// 1. Sets health to `Recovering`
/// 2. Re-maps BAR0
/// 3. Reads BOOT0 to verify device responsiveness
/// 4. On success: sets health to `Alive`, resets fault counter
/// 5. On failure: leaves health as `Faulted`
///
/// Params: `{bdf}`
/// Result: `{bdf, health, boot0}`
pub(crate) fn device_recover(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = require_bdf(params)?;

    let mut map = held.write().map_err(|_| EmberIpcError::LockPoisoned)?;
    let dev = require_held_mut(&mut map, bdf, stream, &id)?;

    if dev.health.allows_mmio() {
        let result = serde_json::json!({
            "bdf": bdf,
            "health": dev.health,
            "message": "device already healthy — no recovery needed",
        });
        drop(map);
        return write_jsonrpc_ok(stream, id, result).map_err(EmberIpcError::from);
    }

    tracing::info!(bdf, "device recovery: starting");
    dev.health = DeviceHealth::Recovering;

    // Force re-map BAR0 (old mapping may be invalid after bus reset)
    dev.bar0 = None;
    if let Err(e) = map_bar0_if_needed(dev) {
        tracing::error!(bdf, error = %e, "device recovery: BAR0 re-map failed");
        dev.health = DeviceHealth::Faulted;
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(
            stream,
            id,
            -32000,
            &format!("{bdf}: recovery failed — BAR0 map error: {e}"),
        )
        .map_err(EmberIpcError::from);
    }

    let bar0 = dev.bar0.as_ref().unwrap();
    let boot0 = bar0.read_u32(0).unwrap_or(0xFFFF_FFFF);

    if boot0 == 0xFFFF_FFFF || boot0 == 0 || boot0 == 0xDEAD_DEAD {
        tracing::error!(bdf, boot0 = format_args!("{boot0:#010x}"), "device recovery: BOOT0 unresponsive — staying Faulted");
        dev.health = DeviceHealth::Faulted;
        let bdf = bdf.to_string();
        drop(map);
        return write_jsonrpc_error(
            stream,
            id,
            -32000,
            &format!("{bdf}: recovery failed — BOOT0={boot0:#010x} (device unresponsive)"),
        )
        .map_err(EmberIpcError::from);
    }

    dev.health = DeviceHealth::Pristine;
    dev.mmio_fault_count = 0;
    if let Some(ref mut armor) = dev.pcie_armor {
        armor.sequencer_mut().reset_to_pristine();
    }
    tracing::info!(bdf, boot0 = format_args!("{boot0:#010x}"), "device recovery: SUCCESS — device is Pristine");

    let result = serde_json::json!({
        "bdf": bdf,
        "health": dev.health,
        "boot0": boot0,
        "message": "recovery successful — device is responsive",
    });

    drop(map);
    write_jsonrpc_ok(stream, id, result).map_err(EmberIpcError::from)
}
