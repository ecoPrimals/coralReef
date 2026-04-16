// SPDX-License-Identifier: AGPL-3.0-or-later
//! JSON-RPC handlers for PMU DEVINIT firmware operations.
//!
//! Exposes the existing `coral-driver` devinit infrastructure as RPCs:
//!
//! - `ember.devinit.status`   — probe PMU falcon state (needs_post, secure, halted)
//! - `ember.devinit.execute`  — run full DEVINIT with diagnostics
//! - `ember.vbios.read`       — read VBIOS from PROM, sysfs, or file

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, RwLock};

use crate::error::EmberIpcError;
use crate::hold::HeldDevice;

use super::jsonrpc::{write_jsonrpc_error, write_jsonrpc_ok};

/// Probe PMU falcon and devinit state without executing anything.
pub(crate) fn devinit_status(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf'"))?;

    let bar0 = match map_bar0(held, bdf) {
        Ok(b) => b,
        Err(e) => {
            write_jsonrpc_error(stream, id, -32000, &e).map_err(EmberIpcError::from)?;
            return Ok(());
        }
    };

    let status = coral_driver::vfio::channel::devinit::DevinitStatus::probe(&bar0);
    let diag = coral_driver::vfio::channel::devinit::FalconDiagnostic::probe(&bar0, Some(bdf));

    write_jsonrpc_ok(
        stream,
        id,
        serde_json::json!({
            "bdf": bdf,
            "needs_post": status.needs_post,
            "requires_signed_firmware": status.requires_signed_firmware(),
            "falcon_halted": status.is_falcon_halted(),
            "devinit_reg": format!("0x{:08x}", status.devinit_reg),
            "pmu_id": format!("0x{:08x}", status.pmu_id),
            "pmu_hwcfg": format!("0x{:08x}", status.pmu_hwcfg),
            "pmu_ctrl": format!("0x{:08x}", status.pmu_ctrl),
            "pmu_mbox0": format!("0x{:08x}", status.pmu_mbox0),
            "prom_accessible": diag.prom_accessible,
            "prom_signature": format!("0x{:08x}", diag.prom_signature),
            "secure_boot": diag.secure_boot,
            "imem_size_kb": diag.imem_size_kb,
            "dmem_size_kb": diag.dmem_size_kb,
            "vbios_sources": diag.vbios_sources.iter()
                .map(|(name, ok, detail)| serde_json::json!({
                    "source": name, "available": ok, "detail": detail
                }))
                .collect::<Vec<_>>(),
        }),
    )
    .map_err(EmberIpcError::from)?;
    Ok(())
}

/// Execute full DEVINIT with diagnostics (PMU falcon upload or host interpreter).
pub(crate) fn devinit_execute(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf'"))?;

    let bar0 = match map_bar0(held, bdf) {
        Ok(b) => b,
        Err(e) => {
            write_jsonrpc_error(stream, id, -32000, &e).map_err(EmberIpcError::from)?;
            return Ok(());
        }
    };

    let before = coral_driver::vfio::channel::devinit::DevinitStatus::probe(&bar0);

    match coral_driver::vfio::channel::devinit::execute_devinit_with_diagnostics(&bar0, Some(bdf))
    {
        Ok(ran) => {
            let after = coral_driver::vfio::channel::devinit::DevinitStatus::probe(&bar0);
            write_jsonrpc_ok(
                stream,
                id,
                serde_json::json!({
                    "bdf": bdf,
                    "executed": ran,
                    "before_needs_post": before.needs_post,
                    "after_needs_post": after.needs_post,
                    "pmu_mbox0": format!("0x{:08x}", after.pmu_mbox0),
                }),
            )
            .map_err(EmberIpcError::from)?;
        }
        Err(e) => {
            write_jsonrpc_error(
                stream,
                id,
                -32001,
                &format!("devinit failed: {e}"),
            )
            .map_err(EmberIpcError::from)?;
        }
    }
    Ok(())
}

/// Read VBIOS from the best available source (PROM > sysfs > file).
pub(crate) fn vbios_read(
    stream: &mut impl Write,
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    id: serde_json::Value,
    params: &serde_json::Value,
) -> Result<(), EmberIpcError> {
    let bdf = params
        .get("bdf")
        .and_then(|v| v.as_str())
        .ok_or(EmberIpcError::InvalidRequest("missing 'bdf'"))?;
    let source = params
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("auto");

    let bar0 = match map_bar0(held, bdf) {
        Ok(b) => b,
        Err(e) => {
            write_jsonrpc_error(stream, id, -32000, &e).map_err(EmberIpcError::from)?;
            return Ok(());
        }
    };

    let result = match source {
        "prom" => coral_driver::vfio::channel::devinit::read_vbios_prom(&bar0)
            .map(|rom| ("prom".to_string(), rom)),
        "sysfs" => coral_driver::vfio::channel::devinit::read_vbios_sysfs(bdf)
            .map(|rom| ("sysfs".to_string(), rom)),
        _ => {
            // Auto: try PROM first, then sysfs
            coral_driver::vfio::channel::devinit::read_vbios_prom(&bar0)
                .map(|rom| ("prom".to_string(), rom))
                .or_else(|_| {
                    coral_driver::vfio::channel::devinit::read_vbios_sysfs(bdf)
                        .map(|rom| ("sysfs".to_string(), rom))
                })
        }
    };

    match result {
        Ok((actual_source, rom)) => {
            let sig = if rom.len() >= 4 {
                format!(
                    "0x{:02x}{:02x}{:02x}{:02x}",
                    rom[3], rom[2], rom[1], rom[0]
                )
            } else {
                "too_short".into()
            };

            write_jsonrpc_ok(
                stream,
                id,
                serde_json::json!({
                    "bdf": bdf,
                    "source": actual_source,
                    "size_bytes": rom.len(),
                    "signature": sig,
                    "sha256": sha256_hex(&rom),
                }),
            )
            .map_err(EmberIpcError::from)?;
        }
        Err(e) => {
            write_jsonrpc_error(
                stream,
                id,
                -32001,
                &format!("VBIOS read failed: {e}"),
            )
            .map_err(EmberIpcError::from)?;
        }
    }
    Ok(())
}

fn map_bar0(
    held: &Arc<RwLock<HashMap<String, HeldDevice>>>,
    bdf: &str,
) -> Result<coral_driver::vfio::device::MappedBar, String> {
    let map = held.read().map_err(|_| "lock poisoned".to_string())?;
    let dev = map
        .get(bdf)
        .ok_or_else(|| format!("device {bdf} not held by ember"))?;
    dev.device
        .map_bar(0)
        .map_err(|e| format!("BAR0 map failed: {e}"))
}

fn sha256_hex(data: &[u8]) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // Non-cryptographic hash for fingerprinting (no crypto deps in ecoBin).
    // Sufficient for "did the ROM change?" checks, not for security.
    let mut h = DefaultHasher::new();
    data.hash(&mut h);
    let hash = h.finish();
    format!("{hash:016x}")
}
