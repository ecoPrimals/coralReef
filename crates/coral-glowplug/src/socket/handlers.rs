// SPDX-License-Identifier: AGPL-3.0-only
//! JSON-RPC method handlers (sync and async).

use std::sync::Arc;
use tokio::sync::Mutex;

use super::protocol::{DeviceInfo, HealthInfo};

/// Validate that a BDF string matches the expected PCI address format.
///
/// Rejects path traversal attempts, null bytes, and malformed addresses
/// that could be interpolated into sysfs paths by device operations.
pub(crate) fn validate_bdf(bdf: &str) -> Result<&str, coral_glowplug::error::RpcError> {
    let is_valid = !bdf.is_empty()
        && bdf.len() <= 16
        && !bdf.contains('/')
        && !bdf.contains('\0')
        && !bdf.contains("..")
        && bdf
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.');
    if is_valid {
        Ok(bdf)
    } else {
        Err(coral_glowplug::error::RpcError::invalid_params(format!(
            "invalid BDF address: {bdf:?}"
        )))
    }
}

/// Run oracle capture off the async event loop so it doesn't block the
/// watchdog or other RPC handlers.
pub(super) async fn oracle_capture_async(
    params: &serde_json::Value,
    devices: &Mutex<Vec<coral_glowplug::device::DeviceSlot>>,
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let raw_bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(raw_bdf)?.to_owned();
    let max_channels = params
        .get("max_channels")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;

    let (bar0_handle, _busy_guard) = {
        let devs = devices.lock().await;
        let slot = devs
            .iter()
            .find(|d| d.bdf.as_ref() == bdf)
            .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                bdf: Arc::from(bdf.as_str()),
            })
            .map_err(RpcError::from)?;
        let guard = slot.try_acquire_busy().ok_or_else(|| {
            RpcError::device_error(format!(
                "device {bdf} is busy with another long-running operation"
            ))
        })?;
        (slot.vfio_bar0_handle(), guard)
    };

    let bdf_clone = bdf.clone();
    let result = tokio::task::spawn_blocking(move || {
        if let Some(handle) = bar0_handle {
            handle.capture_page_tables(&bdf_clone, max_channels)
        } else {
            coral_driver::vfio::channel::mmu_oracle::capture_page_tables(&bdf_clone, max_channels)
        }
    })
    .await
    .map_err(|e| RpcError::internal(format!("oracle task panicked: {e}")))?
    .map_err(RpcError::device_error)?;

    serde_json::to_value(&result).map_err(|e| RpcError::internal(e.to_string()))
}

/// Run compute dispatch off the async event loop via spawn_blocking.
///
/// Params:
///  - `bdf`:        target device BDF
///  - `shader`:     base64-encoded PTX (or native binary)
///  - `inputs`:     array of base64-encoded input buffers
///  - `output_sizes`: array of output buffer sizes (bytes)
///  - `dims`:       [x, y, z] workgroup grid dimensions
///  - `workgroup`:  [x, y, z] threads per workgroup (default [64,1,1])
///  - `shared_mem`: shared memory bytes (default 0)
pub(super) async fn compute_dispatch_async(
    params: &serde_json::Value,
    devices: &Mutex<Vec<coral_glowplug::device::DeviceSlot>>,
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_driver::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};
    use coral_glowplug::error::RpcError;

    let raw_bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf'"))?;
    let bdf = validate_bdf(raw_bdf)?.to_owned();

    let shader_b64 = params
        .get("shader")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'shader' (base64 PTX)"))?
        .to_owned();

    let inputs: Vec<String> = params
        .get("inputs")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let output_sizes: Vec<u64> = params
        .get("output_sizes")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_u64()).collect())
        .unwrap_or_default();

    let dims_arr = params
        .get("dims")
        .and_then(|v| v.as_array())
        .ok_or_else(|| RpcError::invalid_params("missing 'dims' [x,y,z]"))?;
    let dims = [
        dims_arr.first().and_then(|v| v.as_u64()).unwrap_or(1) as u32,
        dims_arr.get(1).and_then(|v| v.as_u64()).unwrap_or(1) as u32,
        dims_arr.get(2).and_then(|v| v.as_u64()).unwrap_or(1) as u32,
    ];

    let workgroup = params
        .get("workgroup")
        .and_then(|v| v.as_array())
        .map(|arr| {
            [
                arr.first().and_then(|v| v.as_u64()).unwrap_or(64) as u32,
                arr.get(1).and_then(|v| v.as_u64()).unwrap_or(1) as u32,
                arr.get(2).and_then(|v| v.as_u64()).unwrap_or(1) as u32,
            ]
        })
        .unwrap_or([64, 1, 1]);

    let shared_mem = params
        .get("shared_mem")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

    let kernel_name = params
        .get("kernel_name")
        .and_then(|v| v.as_str())
        .unwrap_or("main_kernel")
        .to_owned();

    // Validate device is managed and acquire busy guard
    let _busy_guard = {
        let devs = devices.lock().await;
        let slot = devs
            .iter()
            .find(|d| d.bdf.as_ref() == bdf)
            .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                bdf: Arc::from(bdf.as_str()),
            })
            .map_err(RpcError::from)?;
        slot.try_acquire_busy().ok_or_else(|| {
            RpcError::device_error(format!(
                "device {bdf} is busy with another long-running operation"
            ))
        })?
    };

    let bdf_for_task = bdf.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Vec<Vec<u8>>, String> {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD;

        let shader_bytes = b64
            .decode(&shader_b64)
            .map_err(|e| format!("base64 decode shader: {e}"))?;

        let input_data: Vec<Vec<u8>> = inputs
            .iter()
            .map(|s| {
                b64.decode(s)
                    .map_err(|e| format!("base64 decode input: {e}"))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut dev = coral_driver::cuda::CudaComputeDevice::from_bdf_hint(&bdf_for_task)
            .map_err(|e| format!("CUDA open for {bdf_for_task}: {e}"))?;

        let mut handles: Vec<BufferHandle> = Vec::new();

        // Allocate and upload input buffers
        for data in &input_data {
            let h = dev
                .alloc(data.len() as u64, MemoryDomain::VramOrGtt)
                .map_err(|e| format!("alloc input: {e}"))?;
            dev.upload(h, 0, data).map_err(|e| format!("upload: {e}"))?;
            handles.push(h);
        }

        // Allocate output buffers
        let output_start = handles.len();
        for &size in &output_sizes {
            let h = dev
                .alloc(size, MemoryDomain::VramOrGtt)
                .map_err(|e| format!("alloc output: {e}"))?;
            handles.push(h);
        }

        let info = ShaderInfo {
            gpr_count: 0,
            shared_mem_bytes: shared_mem,
            barrier_count: 0,
            workgroup,
            wave_size: 32,
        };

        dev.dispatch_named(
            &shader_bytes,
            &handles,
            DispatchDims::new(dims[0], dims[1], dims[2]),
            &info,
            &kernel_name,
        )
        .map_err(|e| format!("dispatch: {e}"))?;

        dev.sync().map_err(|e| format!("sync: {e}"))?;

        // Readback output buffers
        let mut outputs = Vec::new();
        for (i, &size) in output_sizes.iter().enumerate() {
            let h = handles[output_start + i];
            let data = dev
                .readback(h, 0, size as usize)
                .map_err(|e| format!("readback: {e}"))?;
            outputs.push(data);
        }

        Ok(outputs)
    })
    .await
    .map_err(|e| RpcError::internal(format!("dispatch task panicked: {e}")))?
    .map_err(RpcError::device_error)?;

    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    let output_b64: Vec<String> = result.iter().map(|d| b64.encode(d)).collect();

    Ok(serde_json::json!({
        "bdf": bdf,
        "outputs": output_b64,
        "output_count": output_b64.len(),
    }))
}

pub(crate) fn dispatch(
    method: &str,
    params: &serde_json::Value,
    devices: &mut [coral_glowplug::device::DeviceSlot],
    started_at: std::time::Instant,
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    match method {
        "device.list" => {
            let infos: Vec<DeviceInfo> = devices.iter().map(device_to_info).collect();
            serde_json::to_value(infos).map_err(|e| RpcError::internal(e.to_string()))
        }
        "device.get" => {
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
            serde_json::to_value(device_to_info(slot))
                .map_err(|e| RpcError::internal(e.to_string()))
        }
        "device.swap" => {
            let raw_bdf = params
                .get("bdf")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
            let bdf = validate_bdf(raw_bdf)?.to_owned();
            let target = params
                .get("target")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| RpcError::invalid_params("missing 'target' parameter"))?
                .to_owned();
            let enable_trace = params
                .get("trace")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let slot = devices
                .iter_mut()
                .find(|d| d.bdf.as_ref() == bdf)
                .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                    bdf: Arc::from(bdf.as_str()),
                })
                .map_err(RpcError::from)?;
            if slot.is_busy() {
                return Err(RpcError::device_error(format!(
                    "device {bdf} is busy — cannot swap while a long-running operation is in progress"
                )));
            }
            slot.swap_traced(&target, enable_trace)
                .map_err(|e| RpcError::device_error(e.to_string()))?;
            Ok(serde_json::json!({
                "bdf": bdf,
                "personality": slot.personality.to_string(),
                "vram_alive": slot.health.vram_alive,
            }))
        }
        "device.health" => {
            let bdf = params
                .get("bdf")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
            let bdf = validate_bdf(bdf)?;
            let slot = devices
                .iter_mut()
                .find(|d| d.bdf.as_ref() == bdf)
                .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                    bdf: Arc::from(bdf),
                })
                .map_err(RpcError::from)?;
            slot.check_health();
            serde_json::to_value(HealthInfo {
                bdf: bdf.to_owned(),
                boot0: slot.health.boot0,
                pmc_enable: slot.health.pmc_enable,
                vram_alive: slot.health.vram_alive,
                power: slot.health.power.to_string(),
                domains_alive: slot.health.domains_alive,
                domains_faulted: slot.health.domains_faulted,
            })
            .map_err(|e| RpcError::internal(e.to_string()))
        }
        "device.register_dump" => {
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
            Ok(
                serde_json::json!({"bdf": bdf, "register_count": entries.len(), "registers": entries}),
            )
        }
        "device.register_snapshot" => {
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
            Ok(
                serde_json::json!({"bdf": bdf, "register_count": entries.len(), "registers": entries}),
            )
        }
        "device.write_register" => {
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
        "device.read_bar0_range" => {
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
        "device.pramin_read" => {
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
        "device.pramin_write" => {
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
        "device.lend" => {
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
            let group_id = slot
                .lend()
                .map_err(|e| RpcError::device_error(e.to_string()))?;
            Ok(serde_json::json!({
                "bdf": bdf,
                "group_id": group_id,
                "personality": slot.personality.to_string(),
            }))
        }
        "device.reclaim" => {
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
                    "device {bdf} is busy — cannot reclaim while a long-running operation is in progress"
                )));
            }
            slot.reclaim()
                .map_err(|e| RpcError::device_error(e.to_string()))?;
            Ok(serde_json::json!({
                "bdf": bdf,
                "personality": slot.personality.to_string(),
                "vram_alive": slot.health.vram_alive,
                "has_vfio_fd": slot.has_vfio(),
            }))
        }
        "device.resurrect" => {
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
        "device.reset" => {
            let raw_bdf = params
                .get("bdf")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
            let bdf = validate_bdf(raw_bdf)?.to_owned();
            let slot = devices
                .iter()
                .find(|d| d.bdf.as_ref() == bdf)
                .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                    bdf: Arc::from(bdf.as_str()),
                })
                .map_err(RpcError::from)?;
            if slot.is_busy() {
                return Err(RpcError::device_error(format!(
                    "device {bdf} is busy — cannot reset while a long-running operation is in progress"
                )));
            }
            slot.reset_device()
                .map_err(|e| RpcError::device_error(e.to_string()))?;
            tracing::info!(bdf = %bdf, "PCIe FLR completed via VFIO_DEVICE_RESET");
            Ok(serde_json::json!({
                "bdf": bdf,
                "reset": true,
            }))
        }
        "health.check" | "health.liveness" => Ok(serde_json::json!({
            "alive": true,
            "name": "coral-glowplug",
            "device_count": devices.len(),
            "healthy_count": devices.iter().filter(|d| d.health.vram_alive).count(),
        })),
        "daemon.status" => Ok(serde_json::json!({
            "uptime_secs": started_at.elapsed().as_secs(),
            "device_count": devices.len(),
            "healthy_count": devices.iter().filter(|d| d.health.vram_alive).count(),
        })),
        "device.compute_info" | "device.quota" => {
            Err(RpcError::internal("routed to async handler"))
        }
        "device.set_quota" => Err(RpcError::internal("routed to async handler")),
        "daemon.shutdown" => {
            tracing::info!("shutdown requested via JSON-RPC");
            Err(RpcError::device_error("shutdown"))
        }
        other => Err(RpcError::method_not_found(other)),
    }
}

/// Query GPU compute info via nvidia-smi, releasing the device lock first.
pub(super) async fn compute_info_async(
    params: &serde_json::Value,
    devices: &Mutex<Vec<coral_glowplug::device::DeviceSlot>>,
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let raw_bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(raw_bdf)?.to_owned();

    let (chip, personality, role, protected) = {
        let devs = devices.lock().await;
        let slot = devs
            .iter()
            .find(|d| d.bdf.as_ref() == bdf)
            .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                bdf: Arc::from(bdf.as_str()),
            })
            .map_err(RpcError::from)?;
        (
            slot.chip_name.clone(),
            slot.personality.to_string(),
            slot.config.role.clone(),
            slot.config.is_protected(),
        )
    };

    let bdf2 = bdf.clone();
    let info = tokio::task::spawn_blocking(move || query_nvidia_smi(&bdf2))
        .await
        .map_err(|e| RpcError::internal(format!("nvidia-smi task panicked: {e}")))?;

    let render_node = coral_glowplug::sysfs::find_render_node(&bdf);
    Ok(serde_json::json!({
        "bdf": bdf,
        "chip": chip,
        "personality": personality,
        "role": role,
        "protected": protected,
        "render_node": render_node,
        "compute": info,
    }))
}

/// Query GPU quota info via nvidia-smi, releasing the device lock first.
pub(super) async fn quota_info_async(
    params: &serde_json::Value,
    devices: &Mutex<Vec<coral_glowplug::device::DeviceSlot>>,
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let raw_bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(raw_bdf)?.to_owned();

    let (role, protected, quota) = {
        let devs = devices.lock().await;
        let slot = devs
            .iter()
            .find(|d| d.bdf.as_ref() == bdf)
            .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                bdf: Arc::from(bdf.as_str()),
            })
            .map_err(RpcError::from)?;
        (
            slot.config.role.clone(),
            slot.config.is_protected(),
            slot.config.shared.as_ref().cloned().unwrap_or_default(),
        )
    };

    let bdf2 = bdf.clone();
    let current = tokio::task::spawn_blocking(move || query_nvidia_smi(&bdf2))
        .await
        .map_err(|e| RpcError::internal(format!("nvidia-smi task panicked: {e}")))?;

    Ok(serde_json::json!({
        "bdf": bdf,
        "role": role,
        "protected": protected,
        "quota": {
            "power_limit_w": quota.power_limit_w,
            "vram_budget_mib": quota.vram_budget_mib,
            "compute_mode": quota.compute_mode,
            "compute_priority": quota.compute_priority,
        },
        "current": current,
    }))
}

/// Set GPU quota and apply via nvidia-smi, releasing the device lock for the
/// blocking nvidia-smi call.
pub(super) async fn set_quota_async(
    params: &serde_json::Value,
    devices: &Mutex<Vec<coral_glowplug::device::DeviceSlot>>,
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let raw_bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf' parameter"))?;
    let bdf = validate_bdf(raw_bdf)?.to_owned();

    let quota = {
        let mut devs = devices.lock().await;
        let slot = devs
            .iter_mut()
            .find(|d| d.bdf.as_ref() == bdf)
            .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                bdf: Arc::from(bdf.as_str()),
            })
            .map_err(RpcError::from)?;

        if !slot.config.is_shared() && !slot.config.is_display() {
            return Err(RpcError::device_error(
                "set_quota only applies to role=shared or role=display devices",
            ));
        }

        let mut quota = slot.config.shared.clone().unwrap_or_default();
        if let Some(pl) = params.get("power_limit_w").and_then(|v| v.as_u64()) {
            quota.power_limit_w = Some(pl as u32);
        }
        if let Some(vb) = params.get("vram_budget_mib").and_then(|v| v.as_u64()) {
            quota.vram_budget_mib = Some(vb as u32);
        }
        if let Some(cm) = params.get("compute_mode").and_then(|v| v.as_str()) {
            quota.compute_mode = cm.to_string();
        }
        if let Some(cp) = params.get("compute_priority").and_then(|v| v.as_u64()) {
            quota.compute_priority = cp as u32;
        }

        slot.config.shared = Some(quota.clone());
        quota
    };

    let bdf2 = bdf.clone();
    let quota2 = quota.clone();
    let results = tokio::task::spawn_blocking(move || apply_quota(&bdf2, &quota2))
        .await
        .map_err(|e| RpcError::internal(format!("nvidia-smi task panicked: {e}")))?;

    Ok(serde_json::json!({
        "bdf": bdf,
        "quota": {
            "power_limit_w": quota.power_limit_w,
            "vram_budget_mib": quota.vram_budget_mib,
            "compute_mode": quota.compute_mode,
            "compute_priority": quota.compute_priority,
        },
        "applied": results,
    }))
}

/// Apply quota settings to a GPU via nvidia-smi.
fn apply_quota(bdf: &str, quota: &coral_glowplug::config::SharedQuota) -> serde_json::Value {
    let pci_bus_id = bdf.trim_start_matches("0000:");
    let mut results = serde_json::Map::new();

    if let Some(pl) = quota.power_limit_w {
        let out = std::process::Command::new("nvidia-smi")
            .args(["-i", pci_bus_id, &format!("--power-limit={pl}")])
            .output();
        let ok = out.as_ref().is_ok_and(|o| o.status.success());
        let msg = out
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|e| e.to_string());
        results.insert(
            "power_limit".into(),
            serde_json::json!({"ok": ok, "message": msg}),
        );
    }

    match quota.compute_mode.as_str() {
        "default" | "exclusive_process" | "prohibited" => {
            let mode_id = match quota.compute_mode.as_str() {
                "default" => "0",
                "exclusive_process" => "3",
                "prohibited" => "2",
                _ => "0",
            };
            let out = std::process::Command::new("nvidia-smi")
                .args(["-i", pci_bus_id, &format!("--compute-mode={mode_id}")])
                .output();
            let ok = out.as_ref().is_ok_and(|o| o.status.success());
            let msg = out
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_else(|e| e.to_string());
            results.insert(
                "compute_mode".into(),
                serde_json::json!({"ok": ok, "message": msg}),
            );
        }
        _ => {
            results.insert(
                "compute_mode".into(),
                serde_json::json!({"ok": false, "message": "unknown mode"}),
            );
        }
    }

    serde_json::Value::Object(results)
}

/// Query nvidia-smi for GPU compute info. Returns a JSON object with memory, clocks, power, temp.
/// Returns null fields if nvidia-smi is unavailable or the BDF doesn't match a managed GPU.
fn query_nvidia_smi(bdf: &str) -> serde_json::Value {
    let pci_bus_id = bdf.trim_start_matches("0000:");
    let output = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=gpu_name,memory.total,memory.free,memory.used,temperature.gpu,power.draw,power.limit,clocks.current.sm,clocks.current.memory,compute_cap,pcie.link.width.current",
            "--format=csv,noheader,nounits",
            &format!("--id={pci_bus_id}"),
        ])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            let fields: Vec<&str> = text.trim().splitn(11, ", ").collect();
            if fields.len() >= 11 {
                serde_json::json!({
                    "gpu_name": fields[0],
                    "memory_total_mib": fields[1].trim().parse::<f64>().unwrap_or(0.0),
                    "memory_free_mib": fields[2].trim().parse::<f64>().unwrap_or(0.0),
                    "memory_used_mib": fields[3].trim().parse::<f64>().unwrap_or(0.0),
                    "temperature_c": fields[4].trim().parse::<u32>().unwrap_or(0),
                    "power_draw_w": fields[5].trim().parse::<f64>().unwrap_or(0.0),
                    "power_limit_w": fields[6].trim().parse::<f64>().unwrap_or(0.0),
                    "clock_sm_mhz": fields[7].trim().parse::<u32>().unwrap_or(0),
                    "clock_mem_mhz": fields[8].trim().parse::<u32>().unwrap_or(0),
                    "compute_cap": fields[9].trim(),
                    "pcie_width": fields[10].trim().parse::<u32>().unwrap_or(0),
                })
            } else {
                serde_json::json!({"error": "unexpected nvidia-smi output format"})
            }
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            serde_json::json!({"error": format!("nvidia-smi failed: {}", stderr.trim())})
        }
        Err(e) => serde_json::json!({"error": format!("nvidia-smi not available: {e}")}),
    }
}

fn device_to_info(d: &coral_glowplug::device::DeviceSlot) -> DeviceInfo {
    DeviceInfo {
        bdf: d.bdf.to_string(),
        name: d.config.name.clone(),
        chip: d.chip_name.clone(),
        vendor_id: d.vendor_id,
        device_id: d.device_id,
        personality: d.personality.to_string(),
        role: d.config.role.clone(),
        power: d.health.power.to_string(),
        vram_alive: d.health.vram_alive,
        domains_alive: d.health.domains_alive,
        domains_faulted: d.health.domains_faulted,
        has_vfio_fd: d.has_vfio(),
        pci_link_width: d.health.pci_link_width,
        protected: d.config.is_protected(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use coral_glowplug::config::{DeviceConfig, SharedQuota};
    use coral_glowplug::device::DeviceSlot;
    use std::time::Instant;
    use tokio::sync::Mutex;

    fn test_device_config(bdf: &str) -> DeviceConfig {
        DeviceConfig {
            bdf: bdf.into(),
            name: None,
            boot_personality: "vfio".into(),
            power_policy: "always_on".into(),
            role: None,
            oracle_dump: None,
            shared: None,
        }
    }

    #[test]
    fn validate_bdf_accepts_max_length_hex_address() {
        let s = "0000:ab:cd.ef";
        assert_eq!(validate_bdf(s).expect("valid BDF"), s);
    }

    #[test]
    fn dispatch_swap_rejects_when_device_busy() {
        let mut devices = vec![DeviceSlot::new(test_device_config("0000:99:00.0"))];
        let _guard = devices[0]
            .try_acquire_busy()
            .expect("slot should not start busy");
        let started = Instant::now();
        let err = dispatch(
            "device.swap",
            &serde_json::json!({"bdf": "0000:99:00.0", "target": "nouveau"}),
            &mut devices,
            started,
        )
        .expect_err("swap while busy");
        assert_eq!(i32::from(err.code), -32000);
        assert!(err.message.contains("busy"), "{}", err.message);
    }

    #[test]
    fn dispatch_reclaim_rejects_when_device_busy() {
        let mut devices = vec![DeviceSlot::new(test_device_config("0000:99:00.0"))];
        let _guard = devices[0]
            .try_acquire_busy()
            .expect("slot should not start busy");
        let started = Instant::now();
        let err = dispatch(
            "device.reclaim",
            &serde_json::json!({"bdf": "0000:99:00.0"}),
            &mut devices,
            started,
        )
        .expect_err("reclaim while busy");
        assert_eq!(i32::from(err.code), -32000);
        assert!(err.message.contains("busy"), "{}", err.message);
    }

    #[test]
    fn dispatch_resurrect_rejects_when_device_busy() {
        let mut devices = vec![DeviceSlot::new(test_device_config("0000:99:00.0"))];
        let _guard = devices[0]
            .try_acquire_busy()
            .expect("slot should not start busy");
        let started = Instant::now();
        let err = dispatch(
            "device.resurrect",
            &serde_json::json!({"bdf": "0000:99:00.0"}),
            &mut devices,
            started,
        )
        .expect_err("resurrect while busy");
        assert_eq!(i32::from(err.code), -32000);
        assert!(err.message.contains("busy"), "{}", err.message);
    }

    #[test]
    fn dispatch_reset_rejects_when_device_busy() {
        let mut devices = vec![DeviceSlot::new(test_device_config("0000:99:00.0"))];
        let _guard = devices[0]
            .try_acquire_busy()
            .expect("slot should not start busy");
        let started = Instant::now();
        let err = dispatch(
            "device.reset",
            &serde_json::json!({"bdf": "0000:99:00.0"}),
            &mut devices,
            started,
        )
        .expect_err("reset while busy");
        assert_eq!(i32::from(err.code), -32000);
        assert!(err.message.contains("busy"), "{}", err.message);
    }

    #[test]
    fn dispatch_write_register_missing_offset() {
        let mut devices = vec![DeviceSlot::new(test_device_config("0000:99:00.0"))];
        let started = Instant::now();
        let err = dispatch(
            "device.write_register",
            &serde_json::json!({"bdf": "0000:99:00.0", "value": 0}),
            &mut devices,
            started,
        )
        .expect_err("missing offset");
        assert_eq!(i32::from(err.code), -32602);
        assert!(err.message.contains("offset"), "{}", err.message);
    }

    #[test]
    fn dispatch_write_register_missing_value() {
        let mut devices = vec![DeviceSlot::new(test_device_config("0000:99:00.0"))];
        let started = Instant::now();
        let err = dispatch(
            "device.write_register",
            &serde_json::json!({"bdf": "0000:99:00.0", "offset": 0}),
            &mut devices,
            started,
        )
        .expect_err("missing value");
        assert_eq!(i32::from(err.code), -32602);
        assert!(err.message.contains("value"), "{}", err.message);
    }

    #[test]
    fn dispatch_read_bar0_range_missing_count() {
        let mut devices = vec![DeviceSlot::new(test_device_config("0000:99:00.0"))];
        let started = Instant::now();
        let err = dispatch(
            "device.read_bar0_range",
            &serde_json::json!({"bdf": "0000:99:00.0", "offset": 0}),
            &mut devices,
            started,
        )
        .expect_err("missing count");
        assert_eq!(i32::from(err.code), -32602);
        assert!(err.message.contains("count"), "{}", err.message);
    }

    #[test]
    fn dispatch_pramin_read_missing_vram_offset() {
        let mut devices = vec![DeviceSlot::new(test_device_config("0000:99:00.0"))];
        let started = Instant::now();
        let err = dispatch(
            "device.pramin_read",
            &serde_json::json!({"bdf": "0000:99:00.0", "count": 1}),
            &mut devices,
            started,
        )
        .expect_err("missing vram_offset");
        assert_eq!(i32::from(err.code), -32602);
        assert!(err.message.contains("vram_offset"), "{}", err.message);
    }

    #[test]
    fn dispatch_pramin_read_missing_count() {
        let mut devices = vec![DeviceSlot::new(test_device_config("0000:99:00.0"))];
        let started = Instant::now();
        let err = dispatch(
            "device.pramin_read",
            &serde_json::json!({"bdf": "0000:99:00.0", "vram_offset": 0}),
            &mut devices,
            started,
        )
        .expect_err("missing count");
        assert_eq!(i32::from(err.code), -32602);
        assert!(err.message.contains("count"), "{}", err.message);
    }

    #[test]
    fn dispatch_device_swap_invalid_bdf() {
        let mut devices = vec![DeviceSlot::new(test_device_config("0000:99:00.0"))];
        let started = Instant::now();
        let err = dispatch(
            "device.swap",
            &serde_json::json!({"bdf": "not-a-bdf", "target": "nouveau"}),
            &mut devices,
            started,
        )
        .expect_err("invalid bdf");
        assert_eq!(i32::from(err.code), -32602);
    }

    #[test]
    fn apply_quota_unknown_compute_mode_records_failure() {
        let quota = SharedQuota {
            power_limit_w: None,
            vram_budget_mib: None,
            compute_mode: "bogus_mode".into(),
            compute_priority: 0,
        };
        let v = apply_quota("0000:01:00.0", &quota);
        let cm = v
            .as_object()
            .expect("apply_quota returns JSON object")
            .get("compute_mode")
            .expect("compute_mode key");
        assert_eq!(cm["ok"], false);
        assert_eq!(cm["message"], "unknown mode");
    }

    #[test]
    fn apply_quota_power_limit_and_default_compute_mode_produce_structured_results() {
        let quota = SharedQuota {
            power_limit_w: Some(250),
            vram_budget_mib: None,
            compute_mode: "default".into(),
            compute_priority: 0,
        };
        let v = apply_quota("0000:01:00.0", &quota);
        let obj = v.as_object().expect("apply_quota returns JSON object");
        let pl = obj
            .get("power_limit")
            .expect("power_limit branch")
            .as_object()
            .expect("power_limit object");
        assert!(pl.contains_key("ok"));
        assert!(pl.contains_key("message"));
        let cm = obj
            .get("compute_mode")
            .expect("compute_mode branch")
            .as_object()
            .expect("compute_mode object");
        assert!(cm.contains_key("ok"));
        assert!(cm.contains_key("message"));
    }

    #[test]
    fn apply_quota_exclusive_process_compute_mode_invokes_nvidia_smi_branch() {
        let quota = SharedQuota {
            power_limit_w: None,
            vram_budget_mib: None,
            compute_mode: "exclusive_process".into(),
            compute_priority: 0,
        };
        let v = apply_quota("0000:01:00.0", &quota);
        let obj = v.as_object().expect("apply_quota returns JSON object");
        let cm = obj
            .get("compute_mode")
            .expect("compute_mode")
            .as_object()
            .expect("compute_mode object");
        assert!(cm.contains_key("ok"));
        assert!(cm.contains_key("message"));
    }

    #[test]
    fn apply_quota_prohibited_compute_mode_invokes_nvidia_smi_branch() {
        let quota = SharedQuota {
            power_limit_w: None,
            vram_budget_mib: None,
            compute_mode: "prohibited".into(),
            compute_priority: 0,
        };
        let v = apply_quota("0000:01:00.0", &quota);
        let obj = v.as_object().expect("apply_quota returns JSON object");
        let cm = obj
            .get("compute_mode")
            .expect("compute_mode")
            .as_object()
            .expect("compute_mode object");
        assert!(cm.contains_key("ok"));
        assert!(cm.contains_key("message"));
    }

    #[test]
    fn query_nvidia_smi_returns_json_object() {
        let v = query_nvidia_smi("0000:ff:00.0");
        assert!(
            v.get("error").is_some() || v.get("gpu_name").is_some(),
            "expected error or metrics: {v}"
        );
    }

    #[tokio::test]
    async fn compute_dispatch_async_missing_bdf() {
        let devices = Mutex::new(vec![]);
        let err = compute_dispatch_async(
            &serde_json::json!({"dims": [1, 1, 1], "shader": "YQ=="}),
            &devices,
        )
        .await
        .expect_err("missing bdf");
        assert!(err.message.contains("bdf"));
    }

    #[tokio::test]
    async fn compute_dispatch_async_missing_shader() {
        let devices = Mutex::new(vec![]);
        let err = compute_dispatch_async(
            &serde_json::json!({"bdf": "0000:01:00.0", "dims": [1, 1, 1]}),
            &devices,
        )
        .await
        .expect_err("missing shader");
        assert!(err.message.contains("shader"));
    }

    #[tokio::test]
    async fn compute_dispatch_async_missing_dims() {
        let devices = Mutex::new(vec![]);
        let err = compute_dispatch_async(
            &serde_json::json!({"bdf": "0000:01:00.0", "shader": "YQ=="}),
            &devices,
        )
        .await
        .expect_err("missing dims");
        assert!(err.message.contains("dims"));
    }

    #[tokio::test]
    async fn oracle_capture_async_missing_bdf() {
        let devices = Mutex::new(vec![]);
        let err = oracle_capture_async(&serde_json::json!({}), &devices)
            .await
            .expect_err("missing bdf");
        assert!(err.message.contains("bdf"));
    }

    #[tokio::test]
    async fn oracle_capture_async_invalid_bdf() {
        let devices = Mutex::new(vec![]);
        let err = oracle_capture_async(&serde_json::json!({"bdf": "00/00/00.0"}), &devices)
            .await
            .expect_err("invalid bdf");
        assert_eq!(i32::from(err.code), -32602);
    }

    #[tokio::test]
    async fn oracle_capture_async_device_not_managed() {
        let devices = Mutex::new(vec![]);
        let err = oracle_capture_async(
            &serde_json::json!({"bdf": "0000:01:00.0", "max_channels": 0}),
            &devices,
        )
        .await
        .expect_err("not managed");
        assert_eq!(i32::from(err.code), -32000);
        assert!(err.message.contains("not managed"), "{}", err.message);
    }

    #[tokio::test]
    async fn oracle_capture_async_device_busy() {
        let slot = DeviceSlot::new(test_device_config("0000:99:00.0"));
        let _guard = slot
            .try_acquire_busy()
            .expect("acquire busy for oracle test");
        let devices = Mutex::new(vec![slot]);
        let err = oracle_capture_async(
            &serde_json::json!({"bdf": "0000:99:00.0", "max_channels": 0}),
            &devices,
        )
        .await
        .expect_err("busy");
        assert_eq!(i32::from(err.code), -32000);
        assert!(err.message.contains("busy"), "{}", err.message);
    }

    #[tokio::test]
    async fn compute_dispatch_async_invalid_bdf() {
        let devices = Mutex::new(vec![]);
        let err = compute_dispatch_async(
            &serde_json::json!({
                "bdf": "not!!hex",
                "shader": "YQ==",
                "dims": [1, 1, 1],
                "output_sizes": [],
            }),
            &devices,
        )
        .await
        .expect_err("invalid bdf");
        assert_eq!(i32::from(err.code), -32602);
        assert!(
            err.message.contains("BDF") || err.message.contains("bdf"),
            "{}",
            err.message
        );
    }

    #[tokio::test]
    async fn compute_dispatch_async_device_not_managed() {
        let devices = Mutex::new(vec![]);
        let err = compute_dispatch_async(
            &serde_json::json!({
                "bdf": "0000:01:00.0",
                "shader": "YQ==",
                "dims": [1, 1, 1],
                "output_sizes": [],
            }),
            &devices,
        )
        .await
        .expect_err("not managed");
        assert_eq!(i32::from(err.code), -32000);
    }

    #[tokio::test]
    async fn compute_dispatch_async_invalid_shader_base64() {
        let devices = Mutex::new(vec![DeviceSlot::new(test_device_config("0000:99:00.0"))]);
        let err = compute_dispatch_async(
            &serde_json::json!({
                "bdf": "0000:99:00.0",
                "shader": "@@@notbase64@@@",
                "dims": [1, 1, 1],
                "output_sizes": [],
            }),
            &devices,
        )
        .await
        .expect_err("bad base64");
        assert_eq!(i32::from(err.code), -32000);
        assert!(
            err.message.contains("base64") || err.message.contains("decode"),
            "{}",
            err.message
        );
    }

    #[tokio::test]
    async fn compute_dispatch_async_device_busy() {
        let slot = DeviceSlot::new(test_device_config("0000:99:00.0"));
        let _guard = slot.try_acquire_busy().expect("busy for dispatch");
        let devices = Mutex::new(vec![slot]);
        let err = compute_dispatch_async(
            &serde_json::json!({
                "bdf": "0000:99:00.0",
                "shader": "YQ==",
                "dims": [1, 1, 1],
                "output_sizes": [],
            }),
            &devices,
        )
        .await
        .expect_err("busy");
        assert_eq!(i32::from(err.code), -32000);
        assert!(err.message.contains("busy"), "{}", err.message);
    }

    #[tokio::test]
    async fn compute_info_async_missing_bdf() {
        let devices = Mutex::new(vec![]);
        let err = compute_info_async(&serde_json::json!({}), &devices)
            .await
            .expect_err("missing bdf");
        assert_eq!(i32::from(err.code), -32602);
    }

    #[tokio::test]
    async fn compute_info_async_invalid_bdf() {
        let devices = Mutex::new(vec![]);
        let err = compute_info_async(&serde_json::json!({"bdf": "x/y/z"}), &devices)
            .await
            .expect_err("invalid bdf");
        assert_eq!(i32::from(err.code), -32602);
    }

    #[tokio::test]
    async fn compute_info_async_device_not_managed() {
        let devices = Mutex::new(vec![]);
        let err = compute_info_async(&serde_json::json!({"bdf": "0000:01:00.0"}), &devices)
            .await
            .expect_err("not managed");
        assert_eq!(i32::from(err.code), -32000);
    }

    #[tokio::test]
    async fn compute_info_async_returns_structured_json_with_slot() {
        let devices = Mutex::new(vec![DeviceSlot::new(test_device_config("0000:99:00.0"))]);
        let val = compute_info_async(&serde_json::json!({"bdf": "0000:99:00.0"}), &devices)
            .await
            .expect("compute_info");
        assert_eq!(val["bdf"], "0000:99:00.0");
        assert!(val.get("chip").is_some());
        assert!(val.get("personality").is_some());
        assert!(val.get("role").is_some());
        assert!(val.get("protected").is_some());
        assert!(val.get("render_node").is_some());
        assert!(val.get("compute").is_some());
    }

    #[tokio::test]
    async fn quota_info_async_missing_bdf() {
        let devices = Mutex::new(vec![]);
        let err = quota_info_async(&serde_json::json!({}), &devices)
            .await
            .expect_err("missing bdf");
        assert_eq!(i32::from(err.code), -32602);
    }

    #[tokio::test]
    async fn quota_info_async_not_managed() {
        let devices = Mutex::new(vec![]);
        let err = quota_info_async(&serde_json::json!({"bdf": "0000:02:00.0"}), &devices)
            .await
            .expect_err("not managed");
        assert_eq!(i32::from(err.code), -32000);
    }

    #[tokio::test]
    async fn quota_info_async_returns_quota_and_current() {
        let devices = Mutex::new(vec![DeviceSlot::new(test_device_config("0000:99:00.0"))]);
        let val = quota_info_async(&serde_json::json!({"bdf": "0000:99:00.0"}), &devices)
            .await
            .expect("quota_info");
        assert_eq!(val["bdf"], "0000:99:00.0");
        assert!(val.get("quota").is_some());
        assert!(val.get("current").is_some());
        assert!(val.get("protected").is_some());
    }

    #[tokio::test]
    async fn set_quota_async_missing_bdf() {
        let devices = Mutex::new(vec![]);
        let err = set_quota_async(&serde_json::json!({}), &devices)
            .await
            .expect_err("missing bdf");
        assert_eq!(i32::from(err.code), -32602);
    }

    #[tokio::test]
    async fn set_quota_async_rejects_non_shared_non_display_role() {
        let mut cfg = test_device_config("0000:99:00.0");
        cfg.role = Some("compute".into());
        let devices = Mutex::new(vec![DeviceSlot::new(cfg)]);
        let err = set_quota_async(
            &serde_json::json!({"bdf": "0000:99:00.0", "power_limit_w": 200}),
            &devices,
        )
        .await
        .expect_err("wrong role");
        assert_eq!(i32::from(err.code), -32000);
        assert!(
            err.message.contains("shared") || err.message.contains("display"),
            "{}",
            err.message
        );
    }

    #[tokio::test]
    async fn set_quota_async_applies_for_shared_role() {
        let mut cfg = test_device_config("0000:99:00.0");
        cfg.role = Some("shared".into());
        cfg.shared = Some(SharedQuota {
            power_limit_w: None,
            vram_budget_mib: None,
            compute_mode: "default".into(),
            compute_priority: 0,
        });
        let devices = Mutex::new(vec![DeviceSlot::new(cfg)]);
        let val = set_quota_async(
            &serde_json::json!({
                "bdf": "0000:99:00.0",
                "power_limit_w": 100,
                "vram_budget_mib": 1024,
                "compute_mode": "default",
                "compute_priority": 0,
            }),
            &devices,
        )
        .await
        .expect("set_quota shared");
        assert_eq!(val["bdf"], "0000:99:00.0");
        assert!(val.get("applied").is_some());
        let q = val["quota"].as_object().expect("quota object");
        assert_eq!(q["power_limit_w"], 100);
        assert_eq!(q["vram_budget_mib"], 1024);
    }
}
