// SPDX-License-Identifier: AGPL-3.0-only
//! CUDA backend for the `ComputeDevice` trait via `cudarc`.
//!
//! Wraps `cudarc::driver` to provide buffer allocation, data transfers,
//! PTX kernel dispatch, and synchronization through the same trait
//! interface used by DRM and VFIO backends.
//!
//! Gated behind the `cuda` feature flag in `coral-driver`.

use std::collections::HashMap;
use std::sync::Arc;

use cudarc::driver::{CudaContext, CudaSlice, CudaStream, DevicePtr, PushKernelArg};
use cudarc::nvrtc::Ptx;

use crate::error::DriverError;
use crate::{BufferHandle, ComputeDevice, DispatchDims, DriverResult, MemoryDomain, ShaderInfo};

struct CudaBuffer {
    slice: CudaSlice<u8>,
    size: u64,
}

/// CUDA compute device backed by `cudarc`.
///
/// Buffers are `CudaSlice<u8>` managed by the CUDA driver. Dispatch
/// accepts pre-compiled PTX as the shader binary, loads it as a module,
/// and launches the kernel named `"main_kernel"`.
///
/// `Debug` omits buffer contents for brevity.
pub struct CudaComputeDevice {
    ctx: Arc<CudaContext>,
    stream: Arc<CudaStream>,
    buffers: HashMap<u32, CudaBuffer>,
    next_handle: u32,
    device_name: String,
    ordinal: usize,
}

impl std::fmt::Debug for CudaComputeDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CudaComputeDevice")
            .field("device_name", &self.device_name)
            .field("ordinal", &self.ordinal)
            .field("buffer_count", &self.buffers.len())
            .finish()
    }
}

impl CudaComputeDevice {
    /// Open a CUDA device by ordinal.
    pub fn new(ordinal: usize) -> DriverResult<Self> {
        let ctx = CudaContext::new(ordinal).map_err(|e| {
            DriverError::OpenFailed(format!("CUDA context (ordinal {ordinal}): {e}").into())
        })?;
        Self::from_context(ctx, ordinal)
    }

    /// Open a CUDA device matching the PCI bus from a BDF address.
    ///
    /// Returns `OpenFailed` if no CUDA device matches the bus ID — never
    /// silently falls back to a different GPU.
    pub fn from_bdf_hint(bdf: &str) -> DriverResult<Self> {
        let count = CudaContext::device_count().map_err(|e| {
            DriverError::OpenFailed(format!("CUDA device enumeration failed: {e}").into())
        })? as usize;

        if count == 0 {
            return Err(DriverError::OpenFailed("no CUDA devices found".into()));
        }

        let expected_bus: i32 =
            i32::from_str_radix(bdf.split(':').nth(1).unwrap_or("ff"), 16).unwrap_or(-1);

        for i in 0..count {
            let Ok(ctx) = CudaContext::new(i) else {
                continue;
            };
            let Ok(pci_bus) = ctx
                .attribute(cudarc::driver::sys::CUdevice_attribute::CU_DEVICE_ATTRIBUTE_PCI_BUS_ID)
            else {
                continue;
            };
            if pci_bus == expected_bus {
                return Self::from_context(ctx, i);
            }
        }
        Err(DriverError::OpenFailed(
            format!("no CUDA device matches BDF {bdf} (scanned {count} devices)").into(),
        ))
    }

    /// Number of available CUDA devices.
    pub fn device_count() -> usize {
        CudaContext::device_count().unwrap_or(0) as usize
    }

    fn from_context(ctx: Arc<CudaContext>, ordinal: usize) -> DriverResult<Self> {
        let device_name = ctx.name().unwrap_or_else(|_| "unknown".into());
        let stream = ctx
            .new_stream()
            .map_err(|e| DriverError::OpenFailed(format!("CUDA stream: {e}").into()))?;
        Ok(Self {
            ctx,
            stream,
            buffers: HashMap::new(),
            next_handle: 1,
            device_name,
            ordinal,
        })
    }

    fn alloc_handle(&mut self) -> u32 {
        let h = self.next_handle;
        self.next_handle += 1;
        h
    }

    /// Device name reported by CUDA.
    #[must_use]
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// CUDA device ordinal.
    #[must_use]
    pub const fn ordinal(&self) -> usize {
        self.ordinal
    }
}

impl CudaComputeDevice {
    /// Dispatch a PTX kernel by explicit entry point name.
    ///
    /// Same as the trait `dispatch` method but allows specifying the kernel
    /// entry point instead of defaulting to `"main_kernel"`.
    pub fn dispatch_named(
        &mut self,
        shader: &[u8],
        buffers: &[BufferHandle],
        dims: DispatchDims,
        info: &ShaderInfo,
        kernel_name: &str,
    ) -> DriverResult<()> {
        let ptx_src = std::str::from_utf8(shader).map_err(|e| {
            DriverError::DispatchFailed(format!("shader is not valid PTX UTF-8: {e}").into())
        })?;

        let ptx = Ptx::from_src(ptx_src);
        let module = self
            .ctx
            .load_module(ptx)
            .map_err(|e| DriverError::DispatchFailed(format!("CUDA module load: {e}").into()))?;
        let func = module.load_function(kernel_name).map_err(|e| {
            DriverError::DispatchFailed(format!("kernel '{kernel_name}' not found: {e}").into())
        })?;

        let config = cudarc::driver::LaunchConfig {
            grid_dim: (dims.x, dims.y, dims.z),
            block_dim: (
                info.workgroup[0].max(1),
                info.workgroup[1].max(1),
                info.workgroup[2].max(1),
            ),
            shared_mem_bytes: info.shared_mem_bytes,
        };

        let dev_ptrs: Vec<cudarc::driver::sys::CUdeviceptr> = buffers
            .iter()
            .map(|bh| {
                self.buffers
                    .get(&bh.0)
                    .map(|b| {
                        let (ptr, _guard) = b.slice.device_ptr(&self.stream);
                        ptr
                    })
                    .ok_or(DriverError::BufferNotFound(*bh))
            })
            .collect::<DriverResult<Vec<_>>>()?;

        let mut builder = self.stream.launch_builder(&func);
        for ptr in &dev_ptrs {
            builder.arg(ptr);
        }

        unsafe {
            builder.launch(config).map_err(|e| {
                DriverError::DispatchFailed(format!("CUDA kernel launch: {e}").into())
            })?;
        }

        Ok(())
    }
}

impl ComputeDevice for CudaComputeDevice {
    fn alloc(&mut self, size: u64, domain: MemoryDomain) -> DriverResult<BufferHandle> {
        let slice =
            self.stream
                .alloc_zeros::<u8>(size as usize)
                .map_err(|e| DriverError::AllocFailed {
                    size,
                    domain,
                    detail: format!("CUDA alloc: {e}"),
                })?;
        let handle_id = self.alloc_handle();
        self.buffers.insert(handle_id, CudaBuffer { slice, size });
        Ok(BufferHandle(handle_id))
    }

    fn free(&mut self, handle: BufferHandle) -> DriverResult<()> {
        self.buffers
            .remove(&handle.0)
            .ok_or(DriverError::BufferNotFound(handle))?;
        Ok(())
    }

    fn upload(&mut self, handle: BufferHandle, offset: u64, data: &[u8]) -> DriverResult<()> {
        let buf = self
            .buffers
            .get_mut(&handle.0)
            .ok_or(DriverError::BufferNotFound(handle))?;
        let off = offset as usize;
        if off + data.len() > buf.size as usize {
            return Err(DriverError::SubmitFailed(
                format!(
                    "upload out of bounds: offset={off} len={} size={}",
                    data.len(),
                    buf.size
                )
                .into(),
            ));
        }
        let mut slice_view = buf.slice.try_slice_mut(off..).ok_or_else(|| {
            DriverError::SubmitFailed(format!("CUDA slice view failed at offset {off}").into())
        })?;
        self.stream
            .memcpy_htod(&data[..data.len().min(slice_view.len())], &mut slice_view)
            .map_err(|e| DriverError::SubmitFailed(format!("CUDA htod: {e}").into()))?;
        Ok(())
    }

    fn readback(&self, handle: BufferHandle, offset: u64, len: usize) -> DriverResult<Vec<u8>> {
        let buf = self
            .buffers
            .get(&handle.0)
            .ok_or(DriverError::BufferNotFound(handle))?;
        let off = offset as usize;
        if off + len > buf.size as usize {
            return Err(DriverError::SubmitFailed(
                format!(
                    "readback out of bounds: offset={off} len={len} size={}",
                    buf.size
                )
                .into(),
            ));
        }
        let slice_view = buf.slice.try_slice(off..off + len).ok_or_else(|| {
            DriverError::SubmitFailed(format!("CUDA slice view failed at offset {off}").into())
        })?;
        let mut host = vec![0u8; len];
        self.stream
            .memcpy_dtoh(&slice_view, &mut host)
            .map_err(|e| DriverError::SubmitFailed(format!("CUDA dtoh: {e}").into()))?;
        Ok(host)
    }

    fn dispatch(
        &mut self,
        shader: &[u8],
        buffers: &[BufferHandle],
        dims: DispatchDims,
        info: &ShaderInfo,
    ) -> DriverResult<()> {
        self.dispatch_named(shader, buffers, dims, info, "main_kernel")
    }

    fn sync(&mut self) -> DriverResult<()> {
        self.ctx
            .synchronize()
            .map_err(|e| DriverError::SyncFailed(format!("CUDA synchronize: {e}").into()))?;
        Ok(())
    }
}
