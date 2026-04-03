// SPDX-License-Identifier: AGPL-3.0-only
//! Async compute dispatch and MMU oracle capture handlers.
//!
//! These handlers are spawned via `tokio::task::spawn_blocking` to avoid
//! blocking the async event loop during GPU operations.

use std::sync::Arc;
use tokio::sync::Mutex;

use super::validate_bdf;

/// Run oracle capture off the async event loop so it doesn't block the
/// watchdog or other RPC handlers.
pub(crate) async fn oracle_capture_async(
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

/// Run compute dispatch off the async event loop via `spawn_blocking`.
///
/// Params:
///  - `bdf`:        target device BDF
///  - `shader`:     base64-encoded PTX (or native binary)
///  - `inputs`:     array of base64-encoded input buffers
///  - `output_sizes`: array of output buffer sizes (bytes)
///  - `dims`:       `[x, y, z]` workgroup grid dimensions
///  - `workgroup`:  `[x, y, z]` threads per workgroup (default `[64,1,1]`)
///  - `shared_mem`: shared memory bytes (default 0)
pub(crate) async fn compute_dispatch_async(
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

        for data in &input_data {
            let h = dev
                .alloc(data.len() as u64, MemoryDomain::VramOrGtt)
                .map_err(|e| format!("alloc input: {e}"))?;
            dev.upload(h, 0, data).map_err(|e| format!("upload: {e}"))?;
            handles.push(h);
        }

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

/// Sovereign VFIO dispatch: compile WGSL via coral-parse, dispatch native
/// SASS binary via NvVfioComputeDevice, readback results. No CUDA in path.
///
/// Params:
///  - `bdf`:          target device BDF (must be VFIO-bound)
///  - `wgsl`:         WGSL compute shader source text
///  - `inputs`:       array of base64-encoded input buffers
///  - `output_sizes`: array of output buffer sizes (bytes)
///  - `dims`:         `[x, y, z]` workgroup grid dimensions
///  - `sm`:           SM version override (0 = auto-detect from BOOT0)
pub(crate) async fn sovereign_dispatch_async(
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

    let wgsl_source = params
        .get("wgsl")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'wgsl' (WGSL shader source)"))?
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

    let caller_sm = params
        .get("sm")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;

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
    let result = tokio::task::spawn_blocking(move || -> Result<SovereignDispatchResult, String> {
        use base64::Engine;
        use coral_driver::nv::NvVfioComputeDevice;
        use coral_parse::CoralFrontend;
        use coral_reef::{CompileOptions, GpuTarget, NvArch};

        let b64 = base64::engine::general_purpose::STANDARD;

        // Step 1: Open VFIO device via ember fds.
        //
        // Try warm first (preserves FECS from warm-fecs round-trip).
        // If warm fails (cold GPU, PRI faults), get fresh fds and cold-boot.
        // Falls back to direct open() if ember is unavailable.
        // Exp 140: read-only probe before any device open attempts.
        // Prevents open_warm from writing Volta registers to a cold
        // Kepler GPU and corrupting its BAR0 state.
        let mut dev = match coral_driver::vfio::ember_client::request_vfio_fds(&bdf_for_task) {
            Ok(fds) => {
                tracing::info!(bdf = %bdf_for_task, "sovereign dispatch: trying warm channel via ember fds");
                match NvVfioComputeDevice::open_warm(&bdf_for_task, fds, caller_sm, 0) {
                    Ok(dev) => dev,
                    Err(warm_err) => {
                        tracing::warn!(
                            bdf = %bdf_for_task,
                            error = %warm_err,
                            "warm open failed — falling back to cold boot"
                        );
                        // Get fresh fds — the warm attempt may have consumed
                        // the original set. The cold-open health gate will
                        // check BOOT0 before any writes.
                        let cold_fds = coral_driver::vfio::ember_client::request_vfio_fds(&bdf_for_task)
                            .map_err(|e| format!("ember fds for cold boot {bdf_for_task}: {e}"))?;
                        NvVfioComputeDevice::open_from_fds(&bdf_for_task, cold_fds, caller_sm, 0)
                            .map_err(|e| format!("VFIO cold open {bdf_for_task}: {e}"))?
                    }
                }
            }
            Err(_) => NvVfioComputeDevice::open(&bdf_for_task, caller_sm, 0)
                .map_err(|e| format!("VFIO open {bdf_for_task}: {e}"))?,
        };

        let sm = dev.sm_version();
        let arch_tag = format!("sm_{sm}");
        let nv_arch = NvArch::parse(&arch_tag)
            .ok_or_else(|| format!("unsupported SM version {sm} for compilation"))?;

        // Step 2: Compile WGSL → SASS via coral-reef + coral-parse (sovereign frontend)
        let opts = CompileOptions {
            target: GpuTarget::Nvidia(nv_arch),
            ..CompileOptions::default()
        };
        let compiled = coral_reef::compile_wgsl_full_with(&CoralFrontend, &wgsl_source, &opts)
            .map_err(|e| format!("compile: {e}"))?;

        let info = ShaderInfo {
            gpr_count: compiled.info.gpr_count,
            shared_mem_bytes: compiled.info.shared_mem_bytes,
            barrier_count: compiled.info.barrier_count,
            workgroup: compiled.info.local_size,
            wave_size: 32,
        };

        // Step 3: Upload inputs, allocate outputs
        let input_data: Vec<Vec<u8>> = inputs
            .iter()
            .map(|s| b64.decode(s).map_err(|e| format!("base64 decode input: {e}")))
            .collect::<Result<Vec<_>, _>>()?;

        let mut handles: Vec<BufferHandle> = Vec::new();

        for data in &input_data {
            let h = dev
                .alloc(data.len() as u64, MemoryDomain::VramOrGtt)
                .map_err(|e| format!("alloc input: {e}"))?;
            dev.upload(h, 0, data)
                .map_err(|e| format!("upload: {e}"))?;
            handles.push(h);
        }

        let output_start = handles.len();
        for &size in &output_sizes {
            let h = dev
                .alloc(size, MemoryDomain::VramOrGtt)
                .map_err(|e| format!("alloc output: {e}"))?;
            handles.push(h);
        }

        // Step 4: Dispatch SASS binary on GPU
        dev.dispatch(
            &compiled.binary,
            &handles,
            DispatchDims::new(dims[0], dims[1], dims[2]),
            &info,
        )
        .map_err(|e| format!("dispatch: {e}"))?;

        dev.sync().map_err(|e| format!("sync: {e}"))?;

        // Step 5: Readback outputs
        let mut outputs = Vec::new();
        for (i, &size) in output_sizes.iter().enumerate() {
            let h = handles[output_start + i];
            let data = dev
                .readback(h, 0, size as usize)
                .map_err(|e| format!("readback: {e}"))?;
            outputs.push(data);
        }

        Ok(SovereignDispatchResult {
            outputs,
            sm,
            gpr_count: compiled.info.gpr_count,
            instr_count: compiled.info.instr_count,
            binary_size: compiled.binary.len(),
        })
    })
    .await
    .map_err(|e| RpcError::internal(format!("sovereign dispatch task panicked: {e}")))?
    .map_err(RpcError::device_error)?;

    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    let output_b64: Vec<String> = result.outputs.iter().map(|d| b64.encode(d)).collect();

    Ok(serde_json::json!({
        "bdf": bdf,
        "outputs": output_b64,
        "output_count": output_b64.len(),
        "sm": result.sm,
        "compilation": {
            "gpr_count": result.gpr_count,
            "instr_count": result.instr_count,
            "binary_size": result.binary_size,
        },
        "pipeline": "sovereign",
    }))
}

struct SovereignDispatchResult {
    outputs: Vec<Vec<u8>>,
    sm: u32,
    gpr_count: u32,
    instr_count: u32,
    binary_size: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::socket::handlers::test_device_config;
    use coral_glowplug::device::DeviceSlot;

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
}
