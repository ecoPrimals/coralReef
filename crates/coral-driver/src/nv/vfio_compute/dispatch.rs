// SPDX-License-Identifier: AGPL-3.0-or-later
//! Compute dispatch — QMD build and GPFIFO submission.

use crate::error::{DriverError, DriverResult};
use crate::{BufferHandle, ComputeDevice, DispatchDims, ShaderInfo};

use super::super::pushbuf::PushBuf;
use super::super::qmd;
use super::{LOCAL_MEM_WINDOW_LEGACY, LOCAL_MEM_WINDOW_VOLTA, NvVfioComputeDevice};

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

        // Build CBUF descriptor for group 0 (same layout as NvDevice).
        let desc_entry_size = 16_usize;
        let desc_buf_size = desc_entry_size * buffers.len().max(1);
        let (desc_handle, desc_iova) = self.alloc_dma(desc_buf_size)?;
        temps.push(desc_handle);

        let mut desc_data = vec![0u8; desc_buf_size];
        for (i, bh) in buffers.iter().enumerate() {
            if let Some(buf) = self.buffers.get(&bh.0) {
                let va = buf.dma.iova();
                let sz = u32::try_from(buf.size).unwrap_or(u32::MAX);
                let off = i * 8;
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "deliberate split into 32-bit halves"
                )]
                {
                    desc_data[off..off + 4].copy_from_slice(&(va as u32).to_le_bytes());
                    desc_data[off + 4..off + 8].copy_from_slice(&((va >> 32) as u32).to_le_bytes());
                }
                let sz_off = off + 8;
                if sz_off + 4 <= desc_data.len() {
                    desc_data[sz_off..sz_off + 4].copy_from_slice(&sz.to_le_bytes());
                }
            }
        }
        self.upload(desc_handle, 0, &desc_data)?;

        let cbufs = vec![qmd::CbufBinding {
            index: 0,
            addr: desc_iova,
            size: u32::try_from(desc_buf_size).unwrap_or(u32::MAX),
        }];

        let qmd_params = qmd::QmdParams {
            shader_va: shader_iova,
            grid: dims,
            workgroup: info.workgroup,
            gpr_count: info.gpr_count.max(4),
            shared_mem_bytes: info.shared_mem_bytes,
            barrier_count: info.barrier_count,
            cbufs,
        };
        let qmd_words = qmd::build_qmd_for_sm(self.sm_version, &qmd_params);
        let qmd_bytes: &[u8] = bytemuck::cast_slice(&qmd_words);

        let (qmd_handle, qmd_iova) = self.alloc_dma(qmd_bytes.len())?;
        temps.push(qmd_handle);
        self.upload(qmd_handle, 0, qmd_bytes)?;

        let local_mem_window = if self.sm_version >= 70 {
            LOCAL_MEM_WINDOW_VOLTA
        } else {
            LOCAL_MEM_WINDOW_LEGACY
        };
        let pb = PushBuf::compute_dispatch(self.compute_class, qmd_iova, local_mem_window);
        let pb_bytes = pb.as_bytes();

        let (pb_handle, pb_iova) = self.alloc_dma(pb_bytes.len())?;
        temps.push(pb_handle);
        self.upload(pb_handle, 0, pb_bytes)?;

        let pb_size = u32::try_from(pb_bytes.len())
            .map_err(|_| DriverError::platform_overflow("pushbuf size fits in u32"))?;
        self.submit_pushbuf(pb_iova, pb_size)?;

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
        let desc_buf_size = desc_entry_size * buffers.len().max(1);
        let (desc_handle, desc_iova) = self.alloc_dma(desc_buf_size)?;
        temps.push(desc_handle);

        let mut desc_data = vec![0u8; desc_buf_size];
        for (i, bh) in buffers.iter().enumerate() {
            if let Some(buf) = self.buffers.get(&bh.0) {
                let va = buf.dma.iova();
                let sz = u32::try_from(buf.size).unwrap_or(u32::MAX);
                let off = i * 8;
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "deliberate split into 32-bit halves"
                )]
                {
                    desc_data[off..off + 4].copy_from_slice(&(va as u32).to_le_bytes());
                    desc_data[off + 4..off + 8].copy_from_slice(&((va >> 32) as u32).to_le_bytes());
                }
                let sz_off = off + 8;
                if sz_off + 4 <= desc_data.len() {
                    desc_data[sz_off..sz_off + 4].copy_from_slice(&sz.to_le_bytes());
                }
            }
        }
        self.upload(desc_handle, 0, &desc_data)?;

        let cbufs = vec![qmd::CbufBinding {
            index: 0,
            addr: desc_iova,
            size: u32::try_from(desc_buf_size).unwrap_or(u32::MAX),
        }];

        let qmd_params = qmd::QmdParams {
            shader_va: shader_iova,
            grid: dims,
            workgroup: info.workgroup,
            gpr_count: info.gpr_count.max(4),
            shared_mem_bytes: info.shared_mem_bytes,
            barrier_count: info.barrier_count,
            cbufs,
        };
        let qmd_words = qmd::build_qmd_for_sm(self.sm_version, &qmd_params);
        let qmd_bytes: &[u8] = bytemuck::cast_slice(&qmd_words);

        let (qmd_handle, qmd_iova) = self.alloc_dma(qmd_bytes.len())?;
        temps.push(qmd_handle);
        self.upload(qmd_handle, 0, qmd_bytes)?;

        let local_mem_window = if self.sm_version >= 70 {
            LOCAL_MEM_WINDOW_VOLTA
        } else {
            LOCAL_MEM_WINDOW_LEGACY
        };
        let pb = PushBuf::compute_dispatch(self.compute_class, qmd_iova, local_mem_window);
        let pb_bytes = pb.as_bytes();

        let (pb_handle, pb_iova) = self.alloc_dma(pb_bytes.len())?;
        temps.push(pb_handle);
        self.upload(pb_handle, 0, pb_bytes)?;

        let pb_size = u32::try_from(pb_bytes.len())
            .map_err(|_| DriverError::platform_overflow("pushbuf size fits in u32"))?;
        self.submit_pushbuf_traced(pb_iova, pb_size)
    }
}
