// SPDX-License-Identifier: AGPL-3.0-or-later
//! GPU virtual address mapping for RM buffers (RM DMA vs UVM external range).

use crate::error::DriverResult;

use super::NvUvmComputeDevice;

impl NvUvmComputeDevice {
    /// Map an RM buffer into the GPU VA space with shader read/write access.
    ///
    /// On Blackwell+ with UVM-managed VA spaces, uses `UVM_CREATE_EXTERNAL_RANGE`
    /// + `UVM_MAP_EXTERNAL_ALLOCATION` (bump-allocated VA). Otherwise uses
    ///
    ///   `RM_MAP_MEMORY_DMA` with `SHADER_ACCESS_READ_WRITE`.
    pub(in crate::nv::uvm_compute) fn gpu_map_buffer(
        &mut self,
        h_mem: u32,
        size: u64,
    ) -> DriverResult<u64> {
        if self.uses_uvm_mapping {
            self.uvm_map_rm_buffer(h_mem, size)
        } else {
            self.client
                .rm_map_memory_dma_shader(self.h_device, self.h_virt_mem, h_mem, 0, size)
        }
    }

    /// Map an RM memory object into the GPU VA space via UVM external mapping.
    ///
    /// Uses bump allocation from `uvm_va_next` to assign GPU VAs. The VA range
    /// is created and mapped through UVM, which manages the page tables for
    /// the externally-owned VA space.
    fn uvm_map_rm_buffer(&mut self, h_mem: u32, size: u64) -> DriverResult<u64> {
        use super::super::types::page_align;
        use crate::nv::uvm::ExternalMapping;

        let aligned = page_align(size);
        let va = self.uvm_va_next;

        self.uvm.create_external_range(va, aligned)?;
        self.uvm.map_external_allocation(&ExternalMapping {
            base: va,
            length: aligned,
            offset: 0,
            rm_ctrl_fd: self.client.ctl_fd(),
            h_client: self.client.handle(),
            h_memory: h_mem,
            gpu_uuid: &self.gpu_uuid,
        })?;

        self.uvm_va_next = va + aligned;
        Ok(va)
    }

    /// Map an RM buffer into the GPU VA space (internal infrastructure,
    /// no shader access flag). Uses UVM mapping on Blackwell, RM DMA otherwise.
    pub(in crate::nv::uvm_compute) fn gpu_map_buffer_infra(
        &mut self,
        h_mem: u32,
        size: u64,
    ) -> DriverResult<u64> {
        if self.uses_uvm_mapping {
            self.uvm_map_rm_buffer(h_mem, size)
        } else {
            self.client
                .rm_map_memory_dma(self.h_device, self.h_virt_mem, h_mem, 0, size)
        }
    }
}
