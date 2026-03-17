// SPDX-License-Identifier: AGPL-3.0-only
//! RM memory mapping operations — CPU mmap and GPU DMA mapping.

use crate::error::{DriverError, DriverResult};

use super::super::rm_helpers::nv_rm_ioctl;
use super::super::structs::*;
use super::super::{
    NV_ESC_RM_MAP_MEMORY, NV_ESC_RM_MAP_MEMORY_DMA, NV_ESC_RM_UNMAP_MEMORY, nv_ioctl_rw,
};
use super::RmClient;

impl RmClient {
    /// Map RM-allocated memory into user-space for CPU read/write.
    ///
    /// Returns the user-space virtual address of the mapping. The mapping
    /// is performed by the kernel via `vm_mmap` and persists until explicitly
    /// unmapped with [`rm_unmap_memory`](Self::rm_unmap_memory).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the ioctl fails or returns non-OK status.
    pub fn rm_map_memory(
        &mut self,
        h_device: u32,
        h_memory: u32,
        offset: u64,
        length: u64,
    ) -> DriverResult<u64> {
        let ctl_fd = self.ctl.fd();
        self.rm_map_memory_on_fd(ctl_fd, h_device, h_memory, offset, length)
    }

    /// Map RM memory into user-space, creating the mmap context on `mmap_target_fd`.
    ///
    /// The ioctl is always issued on the **control fd** (`/dev/nvidiactl`);
    /// `mmap_target_fd` is the fd on which the kernel creates the mmap context
    /// (used for the `nvidia_mmap` handler). On 580.x the kernel's `escape.c`
    /// expects `nv_ioctl_nvos33_parameters_with_fd` (56 bytes).
    ///
    /// After the ioctl, we `mmap()` on `mmap_target_fd` at the address the RM
    /// chose to populate the physical pages via `nvidia_mmap_helper`.
    pub fn rm_map_memory_on_fd(
        &mut self,
        mmap_target_fd: i32,
        h_device: u32,
        h_memory: u32,
        offset: u64,
        length: u64,
    ) -> DriverResult<u64> {
        let mut params = NvRmMapMemoryParams {
            h_client: self.h_client,
            h_device,
            h_memory,
            _pad: 0,
            offset,
            length,
            p_linear_address: 0,
            status: 0,
            flags: 0,
            fd: mmap_target_fd,
            _pad2: 0,
        };

        let ioctl_nr = nv_ioctl_rw(
            NV_ESC_RM_MAP_MEMORY,
            std::mem::size_of::<NvRmMapMemoryParams>(),
        );
        let ctl_fd = self.ctl.fd();

        // SAFETY: NvRmMapMemoryParams is #[repr(C)], ctl_fd is a valid nvidiactl fd.
        unsafe { nv_rm_ioctl(ctl_fd, ioctl_nr, &mut params, "RM_MAP_MEMORY", |p| p.status) }?;

        // The RM reserved a VA range and created an mmap context on mmap_target_fd.
        // Now call mmap(MAP_FIXED) at that address to trigger nvidia_mmap_helper
        // which populates the physical pages.
        let rm_addr = params.p_linear_address;
        // SAFETY:
        // 1. mmap_target_fd: valid open nvidia device fd (BorrowedFd::borrow_raw).
        // 2. rm_addr, length: validated by RM_MAP_MEMORY ioctl above.
        // 3. MAP_FIXED: replaces RM-reserved VMA with page-backed mapping.
        let mapped = unsafe {
            rustix::mm::mmap(
                rm_addr as *mut std::ffi::c_void,
                length as usize,
                rustix::mm::ProtFlags::READ | rustix::mm::ProtFlags::WRITE,
                rustix::mm::MapFlags::SHARED | rustix::mm::MapFlags::FIXED,
                std::os::unix::io::BorrowedFd::borrow_raw(mmap_target_fd),
                0,
            )
        }
        .map_err(|e| {
            DriverError::SubmitFailed(
                format!(
                    "mmap after RM_MAP_MEMORY failed: {e} addr=0x{rm_addr:016X} \
                     h_mem=0x{h_memory:08X}"
                )
                .into(),
            )
        })?;

        tracing::debug!(
            h_memory = format_args!("0x{h_memory:08X}"),
            addr = format_args!("0x{mapped:?}"),
            length,
            "RM memory mapped to user-space"
        );
        Ok(mapped as u64)
    }

    /// Unmap previously CPU-mapped RM memory.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the ioctl fails.
    pub fn rm_unmap_memory(
        &mut self,
        h_device: u32,
        h_memory: u32,
        linear_address: u64,
    ) -> DriverResult<()> {
        let mut params = NvRmUnmapMemoryParams {
            h_client: self.h_client,
            h_device,
            h_memory,
            p_linear_address: linear_address,
            status: 0,
            flags: 0,
            _pad: 0,
        };

        let ioctl_nr = nv_ioctl_rw(
            NV_ESC_RM_UNMAP_MEMORY,
            std::mem::size_of::<NvRmUnmapMemoryParams>(),
        );
        // SAFETY: NvRmUnmapMemoryParams is #[repr(C)], ctl fd is a valid nvidiactl fd.
        unsafe {
            nv_rm_ioctl(
                self.ctl.fd(),
                ioctl_nr,
                &mut params,
                "RM_UNMAP_MEMORY",
                |p| p.status,
            )
        }
    }

    /// Map an RM memory object into a GPU virtual address space.
    ///
    /// Uses `NV_ESC_RM_MAP_MEMORY_DMA` (NVOS46) to map `h_memory` into the
    /// virtual memory range identified by `h_virt_mem` (an `NV01_MEMORY_VIRTUAL`
    /// handle), returning the GPU virtual address.
    ///
    /// `h_virt_mem` must be an `NV01_MEMORY_VIRTUAL` handle allocated via
    /// [`alloc_virtual_memory`](Self::alloc_virtual_memory), NOT a raw
    /// `FERMI_VASPACE_A` handle.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError`] if the RM mapping fails.
    pub fn rm_map_memory_dma(
        &mut self,
        h_device: u32,
        h_virt_mem: u32,
        h_memory: u32,
        offset: u64,
        length: u64,
    ) -> DriverResult<u64> {
        let mut params = NvRmMapMemoryDmaParams {
            h_client: self.h_client,
            h_device,
            h_dma: h_virt_mem,
            h_memory,
            offset,
            length,
            ..Default::default()
        };

        let ioctl_nr = nv_ioctl_rw(
            NV_ESC_RM_MAP_MEMORY_DMA,
            std::mem::size_of::<NvRmMapMemoryDmaParams>(),
        );
        // SAFETY: NvRmMapMemoryDmaParams is #[repr(C)], ctl fd is a valid nvidiactl fd.
        unsafe {
            nv_rm_ioctl(
                self.ctl.fd(),
                ioctl_nr,
                &mut params,
                "RM_MAP_MEMORY_DMA",
                |p| p.status,
            )
        }?;

        tracing::debug!(
            h_memory = format_args!("0x{h_memory:08X}"),
            gpu_va = format_args!("0x{:016X}", params.dma_offset),
            length,
            "RM memory mapped to GPU VA space"
        );
        Ok(params.dma_offset)
    }
}
