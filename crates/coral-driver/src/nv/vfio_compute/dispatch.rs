// SPDX-License-Identifier: AGPL-3.0-or-later
//! Compute dispatch — QMD build and GPFIFO submission.

use crate::error::{DriverError, DriverResult};
use crate::nv::generation;
use crate::{BufferHandle, ComputeDevice, DispatchDims, ShaderInfo};

use super::super::pushbuf::PushBuf;
use super::super::qmd;
use super::NvVfioComputeDevice;

impl NvVfioComputeDevice {
    /// Inner dispatch — builds QMD + pushbuf, submits via GPFIFO.
    pub(super) fn dispatch_inner(
        &mut self,
        shader: &[u8],
        buffers: &[BufferHandle],
        dims: DispatchDims,
        info: &ShaderInfo,
        temps: &mut Vec<BufferHandle>,
    ) -> DriverResult<()> {
        let (shader_handle, shader_iova) = self.alloc_dma(shader.len())?;
        temps.push(shader_handle);
        self.upload(shader_handle, 0, shader)?;

        // Build CBUF descriptor table (same layout as UVM: slots 0-6 mirror
        // the descriptor table, slot 7 = driver constants with grid dims).
        let desc_entry_size = 16_usize;
        let desc_buf_size = (desc_entry_size * buffers.len().max(1)).max(64);
        let (desc_handle, desc_iova) = self.alloc_dma(desc_buf_size)?;
        temps.push(desc_handle);

        let mut desc_data = vec![0u8; desc_buf_size];
        for (i, bh) in buffers.iter().enumerate() {
            if let Some(buf) = self.buffers.get(&bh.0) {
                let va = buf.dma.iova();
                let sz = u32::try_from(buf.size).unwrap_or(u32::MAX);
                let off = i * 16;
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "deliberate split into 32-bit halves"
                )]
                {
                    desc_data[off..off + 4].copy_from_slice(&(va as u32).to_le_bytes());
                    desc_data[off + 4..off + 8].copy_from_slice(&((va >> 32) as u32).to_le_bytes());
                }
                desc_data[off + 8..off + 12].copy_from_slice(&sz.to_le_bytes());
            }
        }
        self.upload(desc_handle, 0, &desc_data)?;

        let (cbufs, dc_handle) = self.build_vfio_cbufs(desc_iova, desc_buf_size, &dims, temps)?;

        let qmd_params = qmd::QmdParams {
            shader_va: shader_iova,
            grid: dims,
            workgroup: info.workgroup,
            gpr_count: info.gpr_count.max(4),
            shared_mem_bytes: info.shared_mem_bytes,
            barrier_count: info.barrier_count,
            local_mem_low_bytes: info.local_mem_bytes.unwrap_or(0),
            cbufs,
        };
        let qmd_words = qmd::build_qmd_for_sm(self.sm_version, &qmd_params);
        let qmd_bytes: &[u8] = bytemuck::cast_slice(&qmd_words);

        let (qmd_handle, qmd_iova) = self.alloc_dma(qmd_bytes.len())?;
        temps.push(qmd_handle);
        self.upload(qmd_handle, 0, qmd_bytes)?;

        let profile = generation::profile_for_sm(self.sm_version);
        let mut pb = PushBuf::compute_init(self.compute_class, profile.local_mem_window, 0, 0);
        let dispatch = PushBuf::compute_dispatch_with_launch(profile.launch_method, qmd_iova);
        pb.append(&dispatch);
        let pb_bytes = pb.as_bytes();

        let (pb_handle, pb_iova) = self.alloc_dma(pb_bytes.len())?;
        temps.push(pb_handle);
        self.upload(pb_handle, 0, pb_bytes)?;

        let pb_size = u32::try_from(pb_bytes.len())
            .map_err(|_| DriverError::platform_overflow("pushbuf size fits in u32"))?;
        self.submit_pushbuf(pb_iova, pb_size)?;

        if self.uses_semaphore_fence {
            self.submit_fence_release()?;
        }

        let _ = dc_handle; // kept alive via temps
        Ok(())
    }

    /// Like `dispatch_inner` but uses `submit_pushbuf_traced` for diagnostic captures.
    pub(super) fn dispatch_inner_traced(
        &mut self,
        shader: &[u8],
        buffers: &[BufferHandle],
        dims: DispatchDims,
        info: &ShaderInfo,
        temps: &mut Vec<BufferHandle>,
    ) -> DriverResult<Vec<super::diagnostics::TimedCapture>> {
        let (shader_handle, shader_iova) = self.alloc_dma(shader.len())?;
        temps.push(shader_handle);
        self.upload(shader_handle, 0, shader)?;

        let desc_entry_size = 16_usize;
        let desc_buf_size = (desc_entry_size * buffers.len().max(1)).max(64);
        let (desc_handle, desc_iova) = self.alloc_dma(desc_buf_size)?;
        temps.push(desc_handle);

        let mut desc_data = vec![0u8; desc_buf_size];
        for (i, bh) in buffers.iter().enumerate() {
            if let Some(buf) = self.buffers.get(&bh.0) {
                let va = buf.dma.iova();
                let sz = u32::try_from(buf.size).unwrap_or(u32::MAX);
                let off = i * 16;
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "deliberate split into 32-bit halves"
                )]
                {
                    desc_data[off..off + 4].copy_from_slice(&(va as u32).to_le_bytes());
                    desc_data[off + 4..off + 8].copy_from_slice(&((va >> 32) as u32).to_le_bytes());
                }
                desc_data[off + 8..off + 12].copy_from_slice(&sz.to_le_bytes());
            }
        }
        self.upload(desc_handle, 0, &desc_data)?;

        let (cbufs, dc_handle) = self.build_vfio_cbufs(desc_iova, desc_buf_size, &dims, temps)?;

        let qmd_params = qmd::QmdParams {
            shader_va: shader_iova,
            grid: dims,
            workgroup: info.workgroup,
            gpr_count: info.gpr_count.max(4),
            shared_mem_bytes: info.shared_mem_bytes,
            barrier_count: info.barrier_count,
            local_mem_low_bytes: info.local_mem_bytes.unwrap_or(0),
            cbufs,
        };
        let qmd_words = qmd::build_qmd_for_sm(self.sm_version, &qmd_params);
        let qmd_bytes: &[u8] = bytemuck::cast_slice(&qmd_words);

        let (qmd_handle, qmd_iova) = self.alloc_dma(qmd_bytes.len())?;
        temps.push(qmd_handle);
        self.upload(qmd_handle, 0, qmd_bytes)?;

        let profile = generation::profile_for_sm(self.sm_version);
        let mut pb = PushBuf::compute_init(self.compute_class, profile.local_mem_window, 0, 0);
        let dispatch = PushBuf::compute_dispatch_with_launch(profile.launch_method, qmd_iova);
        pb.append(&dispatch);
        let pb_bytes = pb.as_bytes();

        let (pb_handle, pb_iova) = self.alloc_dma(pb_bytes.len())?;
        temps.push(pb_handle);
        self.upload(pb_handle, 0, pb_bytes)?;

        let pb_size = u32::try_from(pb_bytes.len())
            .map_err(|_| DriverError::platform_overflow("pushbuf size fits in u32"))?;
        let captures = self.submit_pushbuf_traced(pb_iova, pb_size)?;

        if self.uses_semaphore_fence {
            self.submit_fence_release()?;
        }

        let _ = dc_handle;
        Ok(captures)
    }

    /// Build the unified CBUF layout for VFIO dispatch (slots 0-6 + 7).
    fn build_vfio_cbufs(
        &mut self,
        desc_iova: u64,
        desc_buf_size: usize,
        dims: &DispatchDims,
        temps: &mut Vec<BufferHandle>,
    ) -> DriverResult<(Vec<qmd::CbufBinding>, BufferHandle)> {
        let desc_cbuf_size = u32::try_from(desc_buf_size).unwrap_or(u32::MAX);
        let (dc_handle, dc_iova) = self.alloc_dma(qmd::DRIVER_CONST_SIZE as usize)?;
        temps.push(dc_handle);
        let driver_consts = qmd::encode_driver_constants(dims);
        self.upload(dc_handle, 0, &driver_consts)?;

        let cbufs =
            qmd::build_standard_cbufs(desc_iova, desc_cbuf_size, dc_iova, qmd::DRIVER_CONST_SIZE);
        Ok((cbufs, dc_handle))
    }
}
