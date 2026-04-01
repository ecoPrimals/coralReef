// SPDX-License-Identifier: AGPL-3.0-only
//! HBM2 resurrect path for a managed device slot.

use std::sync::Arc;

use super::validate_bdf;

pub(crate) fn handle_resurrect(
    params: &serde_json::Value,
    devices: &mut [coral_glowplug::device::DeviceSlot],
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let raw_bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(raw_bdf)?.to_owned();
    let slot = devices
        .iter_mut()
        .find(|d| d.bdf.as_ref() == bdf)
        .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
            bdf: Arc::from(bdf.as_str()),
        })
        .map_err(RpcError::from)?;
    if slot.is_busy() {
        return Err(RpcError::device_error(format!(
            "device {bdf} is busy — cannot resurrect while a long-running operation is in progress"
        )));
    }
    let alive = slot
        .resurrect_hbm2()
        .map_err(|e| RpcError::device_error(e.to_string()))?;
    Ok(serde_json::json!({
        "bdf": bdf,
        "vram_alive": alive,
        "domains_alive": slot.health.domains_alive,
    }))
}
