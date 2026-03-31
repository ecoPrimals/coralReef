// SPDX-License-Identifier: AGPL-3.0-only
//! Register access handlers — BAR0, PRAMIN, snapshots.
//!
//! Extracted from `device_ops` to keep each module under 1000 lines.

use std::sync::Arc;

use super::validate_bdf;

pub(super) fn handle_register_dump(
    params: &serde_json::Value,
    devices: &[coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(bdf)?;
    let slot = devices
        .iter()
        .find(|d| d.bdf.as_ref() == bdf)
        .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
            bdf: Arc::from(bdf),
        })
        .map_err(RpcError::from)?;
    if !slot.has_vfio() {
        return Err(RpcError::device_error(format!(
            "device {bdf} has no VFIO fd — register reads require VFIO personality"
        )));
    }
    let custom_offsets: Vec<usize> = params
        .get("offsets")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_u64().map(|n| n as usize))
                .collect()
        })
        .unwrap_or_default();
    let regs = slot.dump_registers(&custom_offsets);
    let entries: Vec<serde_json::Value> = regs
        .iter()
        .map(|(off, val)| serde_json::json!({"offset": format!("{off:#010x}"), "value": format!("{val:#010x}"), "raw_offset": off, "raw_value": val}))
        .collect();
    Ok(serde_json::json!({"bdf": bdf, "register_count": entries.len(), "registers": entries}))
}

pub(super) fn handle_register_snapshot(
    params: &serde_json::Value,
    devices: &[coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(bdf)?;
    let slot = devices
        .iter()
        .find(|d| d.bdf.as_ref() == bdf)
        .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
            bdf: Arc::from(bdf),
        })
        .map_err(RpcError::from)?;
    let snap = slot.last_snapshot();
    let entries: Vec<serde_json::Value> = snap
        .iter()
        .map(|(off, val)| serde_json::json!({"offset": format!("{off:#010x}"), "value": format!("{val:#010x}"), "raw_offset": off, "raw_value": val}))
        .collect();
    Ok(serde_json::json!({"bdf": bdf, "register_count": entries.len(), "registers": entries}))
}

pub(super) fn handle_write_register(
    params: &serde_json::Value,
    devices: &[coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(bdf)?;
    let offset = params
        .get("offset")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| RpcError::invalid_params("missing 'offset' parameter"))?
        as usize;
    let value = params
        .get("value")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| RpcError::invalid_params("missing 'value' parameter"))?
        as u32;
    let allow_dangerous = params
        .get("allow_dangerous")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let slot = devices
        .iter()
        .find(|d| d.bdf.as_ref() == bdf)
        .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
            bdf: Arc::from(bdf),
        })
        .map_err(RpcError::from)?;
    slot.write_register(offset, value, allow_dangerous)
        .map_err(|e| RpcError::device_error(e.to_string()))?;
    Ok(serde_json::json!({
        "bdf": bdf,
        "offset": format!("{offset:#010x}"),
        "value": format!("{value:#010x}"),
    }))
}

pub(super) fn handle_read_bar0_range(
    params: &serde_json::Value,
    devices: &[coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(bdf)?;
    let offset = params
        .get("offset")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| RpcError::invalid_params("missing 'offset' parameter"))?
        as usize;
    let count = params
        .get("count")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| RpcError::invalid_params("missing 'count' parameter"))?
        as usize;
    if count > 4096 {
        return Err(RpcError::invalid_params("count exceeds 4096 maximum"));
    }
    let slot = devices
        .iter()
        .find(|d| d.bdf.as_ref() == bdf)
        .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
            bdf: Arc::from(bdf),
        })
        .map_err(RpcError::from)?;
    let values = slot.read_bar0_range(offset, count);
    Ok(serde_json::json!({
        "bdf": bdf,
        "offset": format!("{offset:#010x}"),
        "count": values.len(),
        "values": values,
    }))
}

pub(super) fn handle_pramin_read(
    params: &serde_json::Value,
    devices: &[coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(bdf)?;
    let vram_offset = params
        .get("vram_offset")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| RpcError::invalid_params("missing 'vram_offset' parameter"))?;
    let count = params
        .get("count")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| RpcError::invalid_params("missing 'count' parameter"))?
        as usize;
    if count > 4096 {
        return Err(RpcError::invalid_params("count exceeds 4096 maximum"));
    }
    let slot = devices
        .iter()
        .find(|d| d.bdf.as_ref() == bdf)
        .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
            bdf: Arc::from(bdf),
        })
        .map_err(RpcError::from)?;
    let values = slot
        .pramin_read(vram_offset, count)
        .map_err(|e| RpcError::device_error(e.to_string()))?;
    Ok(serde_json::json!({
        "bdf": bdf,
        "vram_offset": format!("{vram_offset:#010x}"),
        "count": values.len(),
        "values": values,
    }))
}

pub(super) fn handle_pramin_write(
    params: &serde_json::Value,
    devices: &[coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(bdf)?;
    let vram_offset = params
        .get("vram_offset")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| RpcError::invalid_params("missing 'vram_offset' parameter"))?;
    let values: Vec<u32> = params
        .get("values")
        .and_then(|v| v.as_array())
        .ok_or_else(|| RpcError::invalid_params("missing 'values' array parameter"))?
        .iter()
        .filter_map(|v| v.as_u64().map(|n| n as u32))
        .collect();
    if values.len() > 4096 {
        return Err(RpcError::invalid_params(
            "values array exceeds 4096 maximum",
        ));
    }
    let slot = devices
        .iter()
        .find(|d| d.bdf.as_ref() == bdf)
        .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
            bdf: Arc::from(bdf),
        })
        .map_err(RpcError::from)?;
    slot.pramin_write(vram_offset, &values)
        .map_err(|e| RpcError::device_error(e.to_string()))?;
    Ok(serde_json::json!({
        "bdf": bdf,
        "vram_offset": format!("{vram_offset:#010x}"),
        "count": values.len(),
    }))
}
