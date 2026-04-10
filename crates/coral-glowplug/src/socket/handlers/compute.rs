// SPDX-License-Identifier: AGPL-3.0-or-later
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
    let bdf: Arc<str> = Arc::from(validate_bdf(raw_bdf)?);
    let max_channels = params
        .get("max_channels")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;

    let (bar0_handle, _busy_guard) = {
        let devs = devices.lock().await;
        let slot = devs
            .iter()
            .find(|d| d.bdf == bdf)
            .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                bdf: Arc::clone(&bdf),
            })
            .map_err(RpcError::from)?;
        let guard = slot.try_acquire_busy().ok_or_else(|| {
            RpcError::device_error(format!(
                "device {bdf} is busy with another long-running operation"
            ))
        })?;
        (slot.vfio_bar0_handle(), guard)
    };

    let bdf_task = Arc::clone(&bdf);
    let result = tokio::task::spawn_blocking(move || {
        if let Some(handle) = bar0_handle {
            handle.capture_page_tables(&bdf_task, max_channels)
        } else {
            coral_driver::vfio::channel::mmu_oracle::capture_page_tables(&bdf_task, max_channels)
        }
    })
    .await
    .map_err(|e| RpcError::internal(format!("oracle task panicked: {e}")))?
    .map_err(|e| RpcError::device_error(e.to_string()))?;

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
#[cfg(feature = "cuda")]
struct CudaDispatchWork {
    bdf_for_task: Arc<str>,
    shader_bytes: Vec<u8>,
    input_data: Vec<Vec<u8>>,
    output_sizes: Vec<u64>,
    dims: [u32; 3],
    workgroup: [u32; 3],
    shared_mem: u32,
    kernel_name: String,
}

#[cfg(feature = "cuda")]
fn cuda_dispatch_blocking(
    work: CudaDispatchWork,
) -> Result<Vec<Vec<u8>>, coral_glowplug::error::ComputeDispatchError> {
    use coral_driver::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};
    use coral_glowplug::error::ComputeDispatchError;

    let CudaDispatchWork {
        bdf_for_task,
        shader_bytes,
        input_data,
        output_sizes,
        dims,
        workgroup,
        shared_mem,
        kernel_name,
    } = work;

    let bdf_str = bdf_for_task.as_ref().to_owned();

    let mut dev = coral_driver::cuda::CudaComputeDevice::from_bdf_hint(bdf_for_task.as_ref())
        .map_err(|e| ComputeDispatchError::CudaOpen {
            bdf: bdf_str.clone(),
            message: e.to_string(),
        })?;

    let mut handles: Vec<BufferHandle> = Vec::new();

    for data in &input_data {
        let h = dev
            .alloc(data.len() as u64, MemoryDomain::VramOrGtt)
            .map_err(|e| ComputeDispatchError::AllocInput {
                message: e.to_string(),
            })?;
        dev.upload(h, 0, data)
            .map_err(|e| ComputeDispatchError::Upload {
                message: e.to_string(),
            })?;
        handles.push(h);
    }

    let output_start = handles.len();
    for &size in &output_sizes {
        let h = dev.alloc(size, MemoryDomain::VramOrGtt).map_err(|e| {
            ComputeDispatchError::AllocOutput {
                message: e.to_string(),
            }
        })?;
        handles.push(h);
    }

    let info = ShaderInfo {
        gpr_count: 0,
        shared_mem_bytes: shared_mem,
        barrier_count: 0,
        workgroup,
        wave_size: 32,
        local_mem_bytes: None,
    };

    dev.dispatch_named(
        &shader_bytes,
        &handles,
        DispatchDims::new(dims[0], dims[1], dims[2]),
        &info,
        &kernel_name,
    )
    .map_err(|e| ComputeDispatchError::Dispatch {
        message: e.to_string(),
    })?;

    dev.sync().map_err(|e| ComputeDispatchError::Sync {
        message: e.to_string(),
    })?;

    let mut outputs = Vec::new();
    for (i, &size) in output_sizes.iter().enumerate() {
        let h = handles[output_start + i];
        let data =
            dev.readback(h, 0, size as usize)
                .map_err(|e| ComputeDispatchError::Readback {
                    message: e.to_string(),
                })?;
        outputs.push(data);
    }

    Ok(outputs)
}

pub(crate) async fn compute_dispatch_async(
    params: &serde_json::Value,
    devices: &Mutex<Vec<coral_glowplug::device::DeviceSlot>>,
) -> Result<serde_json::Value, coral_glowplug::error::RpcError> {
    use coral_glowplug::error::RpcError;

    let raw_bdf = params
        .get("bdf")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing 'bdf'"))?;
    let bdf: Arc<str> = Arc::from(validate_bdf(raw_bdf)?);

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
            .find(|d| d.bdf == bdf)
            .ok_or_else(|| coral_glowplug::error::DeviceError::NotManaged {
                bdf: Arc::clone(&bdf),
            })
            .map_err(RpcError::from)?;
        slot.try_acquire_busy().ok_or_else(|| {
            RpcError::device_error(format!(
                "device {bdf} is busy with another long-running operation"
            ))
        })?
    };

    let bdf_for_task = Arc::clone(&bdf);
    let result = tokio::task::spawn_blocking(
        move || -> Result<Vec<Vec<u8>>, coral_glowplug::error::ComputeDispatchError> {
            use base64::Engine;
            use coral_glowplug::error::ComputeDispatchError;
            let b64 = base64::engine::general_purpose::STANDARD;

            let shader_bytes = b64
                .decode(&shader_b64)
                .map_err(ComputeDispatchError::ShaderBase64)?;

            let input_data: Vec<Vec<u8>> = inputs
                .iter()
                .map(|s| b64.decode(s).map_err(ComputeDispatchError::InputBase64))
                .collect::<Result<Vec<_>, _>>()?;

            #[cfg(not(feature = "cuda"))]
            {
                let _ = (
                    &bdf_for_task,
                    &shader_bytes,
                    &input_data,
                    &output_sizes,
                    dims,
                    workgroup,
                    shared_mem,
                    &kernel_name,
                );
                Err(ComputeDispatchError::CudaFeatureDisabled)
            }

            #[cfg(feature = "cuda")]
            cuda_dispatch_blocking(CudaDispatchWork {
                bdf_for_task,
                shader_bytes,
                input_data,
                output_sizes,
                dims,
                workgroup,
                shared_mem,
                kernel_name,
            })
        },
    )
    .await
    .map_err(|e| RpcError::internal(format!("dispatch task panicked: {e}")))?
    .map_err(RpcError::from)?;

    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD;
    let output_b64: Vec<String> = result.iter().map(|d| b64.encode(d)).collect();

    Ok(serde_json::json!({
        "bdf": bdf,
        "outputs": output_b64,
        "output_count": output_b64.len(),
    }))
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
