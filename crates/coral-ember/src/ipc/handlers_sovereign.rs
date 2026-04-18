// SPDX-License-Identifier: AGPL-3.0-or-later
//! JSON-RPC handler for the `ember.sovereign.init` pipeline.
//!
//! Delegates to [`coral_driver::vfio::sovereign_init::sovereign_init`],
//! converting the [`SovereignInitResult`] into the JSON-RPC response that
//! glowplug expects (`all_ok`, `compute_ready`, `halted_at`, `stages`).
//!
//! Supports `golden_state_path` and `vbios_rom_path` parameters that let
//! RPC clients pass large data by file reference (the in-memory fields are
//! `#[serde(skip)]`).

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, RwLock};

use coral_driver::vfio::sovereign_init::{SovereignInitOptions, SovereignInitResult};

use crate::error::EmberIpcError;
use crate::hold::HeldDevice;

use super::jsonrpc::{write_jsonrpc_error, write_jsonrpc_ok};

/// Load golden state from a file path. Accepts two formats:
/// 1. Raw array of `[offset, value]` pairs: `[[4096, 255], ...]`
/// 2. A `TrainingRecipe` JSON (has `training_writes` with domain captures) —
///    flattened to `(offset, value)` pairs automatically.
fn load_golden_state(path: &str) -> Result<Vec<(usize, u32)>, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read golden state {path}: {e}"))?;

    if let Ok(pairs) = serde_json::from_str::<Vec<(usize, u32)>>(&content) {
        return Ok(pairs);
    }

    #[derive(serde::Deserialize)]
    struct DomainCapture {
        registers: Vec<(usize, u32)>,
    }
    #[derive(serde::Deserialize)]
    struct Recipe {
        training_writes: Vec<DomainCapture>,
    }

    if let Ok(recipe) = serde_json::from_str::<Recipe>(&content) {
        let pairs: Vec<(usize, u32)> = recipe
            .training_writes
            .into_iter()
            .flat_map(|d| d.registers)
            .collect();
        if pairs.is_empty() {
            return Err(format!("recipe {path} has no training writes"));
        }
        return Ok(pairs);
    }

    Err(format!(
        "{path}: not a valid golden state (expected [[offset,value],...] or TrainingRecipe JSON)"
    ))
}

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

    let mut opts: SovereignInitOptions = if let Some(opts_val) = params.get("options") {
        serde_json::from_value(opts_val.clone()).unwrap_or_default()
    } else {
        let mut o = SovereignInitOptions::default();
        if let Some(hb) = params.get("halt_before").and_then(|v| v.as_str()) {
            o.halt_before = serde_json::from_value(serde_json::json!(hb)).ok();
        }
        o
    };

    if opts.golden_state.is_none() {
        if let Some(path) = opts.golden_state_path.as_deref().or_else(|| {
            params.get("golden_state_path").and_then(|v| v.as_str())
        }) {
            match load_golden_state(path) {
                Ok(gs) => {
                    tracing::info!(path, writes = gs.len(), "loaded golden state from file");
                    opts.golden_state = Some(gs);
                }
                Err(e) => {
                    tracing::warn!(path, error = %e, "failed to load golden state");
                    write_jsonrpc_error(stream, id, -32000, &e)
                        .map_err(EmberIpcError::from)?;
                    return Ok(());
                }
            }
        }
    }

    if opts.vbios_rom.is_none() {
        if let Some(path) = opts.vbios_rom_path.as_deref().or_else(|| {
            params.get("vbios_rom_path").and_then(|v| v.as_str())
        }) {
            match std::fs::read(path) {
                Ok(rom) => {
                    tracing::info!(path, bytes = rom.len(), "loaded VBIOS ROM from file");
                    opts.vbios_rom = Some(rom);
                }
                Err(e) => {
                    tracing::warn!(path, error = %e, "failed to load VBIOS ROM");
                    write_jsonrpc_error(
                        stream,
                        id,
                        -32000,
                        &format!("cannot read VBIOS ROM {path}: {e}"),
                    )
                    .map_err(EmberIpcError::from)?;
                    return Ok(());
                }
            }
        }
    }

    let (bar0, dma_backend) = {
        let map = held.read().map_err(|_| EmberIpcError::LockPoisoned)?;
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
        let b = match dev.device.map_bar(0) {
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
        };
        (b, dev.device.dma_backend())
    };

    opts.dma_backend = Some(dma_backend);

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
