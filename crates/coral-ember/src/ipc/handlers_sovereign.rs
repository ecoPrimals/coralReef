// SPDX-License-Identifier: AGPL-3.0-or-later
//! JSON-RPC handler for the `ember.sovereign.init` pipeline.
//!
//! Delegates to [`coral_driver::vfio::sovereign_init::sovereign_init`],
//! converting the [`SovereignInitResult`] into the JSON-RPC response that
//! glowplug expects (`all_ok`, `compute_ready`, `halted_at`, `stages`).

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, RwLock};

use coral_driver::vfio::sovereign_init::{SovereignInitOptions, SovereignInitResult};

use crate::error::EmberIpcError;
use crate::hold::HeldDevice;

use super::jsonrpc::{write_jsonrpc_error, write_jsonrpc_ok};

pub(crate) fn sovereign_init(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf'"))?;

    let opts: SovereignInitOptions = if let Some(opts_val) = params.get("options") {
        serde_json::from_value(opts_val.clone()).unwrap_or_default()
    } else {
        SovereignInitOptions::default()
    };

    let bar0 = {
        let map = held.read().map_err(|_| {
            EmberIpcError::LockPoisoned
        })?;
        let dev = match map.get(bdf) {
            Some(d) => d,
            None => {
                write_jsonrpc_error(
                    stream,
                    id,
                    -32000,
                    &format!("device {bdf} not held by ember"),
                )
                .map_err(EmberIpcError::from)?;
                return Ok(());
            }
        };
        match dev.device.map_bar(0) {
            Ok(b) => b,
            Err(e) => {
                write_jsonrpc_error(
                    stream,
                    id,
                    -32000,
                    &format!("BAR0 map failed: {e}"),
                )
                .map_err(EmberIpcError::from)?;
                return Ok(());
            }
        }
    };

    let result: SovereignInitResult =
        coral_driver::vfio::sovereign_init::sovereign_init(&bar0, bdf, &opts);

    let json = serde_json::to_value(&result).unwrap_or_else(|e| {
        serde_json::json!({
            "all_ok": false,
            "compute_ready": false,
            "error": format!("serialization failed: {e}"),
        })
    });

    write_jsonrpc_ok(stream, id, json).map_err(EmberIpcError::from)?;
    Ok(())
}
