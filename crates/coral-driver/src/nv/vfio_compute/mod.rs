// SPDX-License-Identifier: AGPL-3.0-or-later
//! NVIDIA VFIO compute device — direct BAR0/DMA dispatch without kernel driver.
//!
//! Implements [`ComputeDevice`] using the VFIO subsystem:
//! - BAR0 MMIO for register access (GR init, GPFIFO doorbell)
//! - DMA buffers for shader code, QMD, push buffers, and user data
//! - Direct GPFIFO submission via BAR0 USERD write
//!
//! # Prerequisites (provided by ecosystem hardware setup)
//!
//! - GPU bound to `vfio-pci`
//! - IOMMU enabled and configured
//! - User has `/dev/vfio/*` permissions
//!
//! # Architecture
//!
//! ```text
//! NvVfioComputeDevice
//!   ├─ VfioDevice       (container + group + device fd)
//!   ├─ MappedBar (BAR0) (MMIO register access)
//!   ├─ DmaBuffer pool   (IOMMU-mapped host memory for GPU)
//!   │   ├─ GPFIFO ring  (command entries)
//!   │   ├─ USERD page   (doorbell for GPFIFO put pointer)
//!   │   └─ user buffers (shader, QMD, data)
//!   └─ pushbuf + QMD    (reuses coral-driver's existing builders)
//! ```

pub mod acr_boot;
mod device_open;
pub mod diagnostics;
mod dispatch;
pub mod falcon_capability;
pub mod fecs_boot;
pub mod gr_context;
mod gr_engine_status;
mod init;
mod layout;
mod raw_device;
mod submission;

pub use gr_engine_status::GrEngineStatus;
pub use raw_device::RawVfioDevice;

pub(super) use layout::{LOCAL_MEM_WINDOW_LEGACY, LOCAL_MEM_WINDOW_VOLTA, gpfifo, sm_to_chip};

use crate::error::{DriverError, DriverResult};
use crate::vfio::channel::VfioChannel;
use crate::vfio::device::{DmaBackend, MappedBar, VfioDevice};
use crate::vfio::dma::DmaBuffer;
use crate::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

use std::collections::HashMap;

/// DMA-backed GPU buffer tracked by the VFIO device.
struct VfioBuffer {
    dma: DmaBuffer,
    size: u64,
}

/// NVIDIA compute device via VFIO — direct BAR0 + DMA dispatch.
///
/// Field order matters: DMA buffers and channel must drop (and unmap)
/// BEFORE `device` drops (which closes the container fd). Rust drops
/// fields in declaration order, so `device` is last.
pub struct NvVfioComputeDevice {
    bar0: MappedBar,
    sm_version: u32,
    compute_class: u32,
    gpfifo_ring: DmaBuffer,
    gpfifo_put: u32,
    userd: DmaBuffer,
    channel: VfioChannel,
    next_handle: u32,
    next_iova: u64,
    container: DmaBackend,
    buffers: HashMap<u32, VfioBuffer>,
    inflight: Vec<BufferHandle>,
    device: VfioDevice,
}

impl NvVfioComputeDevice {
    /// The SM architecture version of this device (auto-detected or validated).
    #[must_use]
    pub fn sm_version(&self) -> u32 {
        self.sm_version
    }

    /// Reads GR engine diagnostic status from BAR0 registers.
    pub fn gr_engine_status(&self) -> GrEngineStatus {
        use crate::vfio::channel::registers::{falcon, misc};
        let r = |off: usize| self.bar0.read_u32(off).unwrap_or(0xDEAD_DEAD);

        GrEngineStatus {
            pgraph_status: r(misc::PGRAPH_STATUS),
            fecs_cpuctl: r(falcon::FECS_BASE + falcon::CPUCTL),
            fecs_mailbox0: r(falcon::FECS_BASE + falcon::MAILBOX0),
            fecs_mailbox1: r(falcon::FECS_BASE + falcon::MAILBOX1),
            fecs_hwcfg: r(falcon::FECS_BASE + falcon::HWCFG),
            gpccs_cpuctl: r(falcon::GPCCS_BASE + falcon::CPUCTL),
            pmc_enable: r(misc::PMC_ENABLE),
            pfifo_enable: r(misc::PFIFO_SCHED_EN),
        }
    }

    /// Capture comprehensive Layer 7 diagnostic state from BAR0.
    ///
    /// Reads all falcon states (FECS, GPCCS, PMU, SEC2), engine topology,
    /// engine status, PCCSR channel state, PFIFO scheduler registers, and
    /// PBDMA operational registers for the GR runlist's PBDMAs.
    pub fn layer7_diagnostics(&self, label: &str) -> diagnostics::Layer7Diagnostics {
        diagnostics::Layer7Diagnostics::capture(&self.bar0, label, self.channel.id())
    }

    /// Snapshot PBDMA registers for specific PBDMA IDs.
    pub fn pbdma_snapshot(&self, pbdma_ids: &[usize]) -> Vec<diagnostics::PbdmaSnapshot> {
        let start = std::time::Instant::now();
        pbdma_ids
            .iter()
            .map(|&id| diagnostics::PbdmaSnapshot::capture(&self.bar0, id, start))
            .collect()
    }

    /// Find the PBDMA IDs assigned to the GR engine runlist (runlist 1).
    pub fn gr_runlist_pbdma_ids(&self) -> Vec<usize> {
        use crate::vfio::channel::registers::pfifo;
        let pbdma_map = self.bar0.read_u32(pfifo::PBDMA_MAP).unwrap_or(0);
        diagnostics::find_pbdmas_for_runlist(pbdma_map, &self.bar0, 1)
    }

    /// Capture PCCSR channel status for this device's channel.
    pub fn pccsr_status(&self) -> diagnostics::PccsrSnapshot {
        diagnostics::PccsrSnapshot::capture(&self.bar0, self.channel.id())
    }

    /// Borrow the BAR0 mapped region for direct diagnostic reads.
    pub fn bar0_ref(&self) -> &crate::vfio::device::MappedBar {
        &self.bar0
    }

    /// Clone the DMA backend for allocating IOMMU-mapped host memory buffers.
    pub fn dma_backend(&self) -> DmaBackend {
        self.container.clone()
    }

    /// Trigger a VFIO device reset (PCI FLR).
    ///
    /// Fully resets the GPU hardware, clearing all falcon state including
    /// secure mode. BAR0 MMIO mapping remains valid after FLR.
    pub fn vfio_device_reset(&self) -> DriverResult<()> {
        self.device.reset()
    }

    /// PCI Secondary Bus Reset via VFIO hot reset ioctl.
    ///
    /// Asserts SBR through the upstream PCIe bridge, fully resetting all
    /// GPU engines including falcons stuck in LS mode. Works on GV100
    /// Titan V which lacks FLR. Bus master is re-enabled after reset.
    pub fn vfio_pci_hot_reset(&self) -> DriverResult<()> {
        self.device.pci_hot_reset()
    }

    /// Attempt sovereign FECS falcon boot from firmware files.
    ///
    /// Loads `fecs_bl.bin`, `fecs_inst.bin`, `fecs_data.bin` from
    /// `/lib/firmware/nvidia/{chip}/gr/` and uploads them directly
    /// to the FECS falcon IMEM/DMEM ports.
    pub fn sovereign_fecs_boot(&self) -> DriverResult<fecs_boot::FalconBootResult> {
        let chip = sm_to_chip(self.sm_version);
        fecs_boot::boot_fecs(&self.bar0, chip)
    }

    /// Attempt sovereign FECS + GPCCS falcon boot.
    pub fn sovereign_gr_boot(&self) -> DriverResult<fecs_boot::FalconBootResult> {
        let chip = sm_to_chip(self.sm_version);
        fecs_boot::boot_gr_falcons(&self.bar0, chip)
    }

    /// Probe all falcon states for boot strategy selection.
    pub fn falcon_probe(&self) -> acr_boot::FalconProbe {
        acr_boot::FalconProbe::capture(&self.bar0)
    }

    /// Run the Falcon Boot Solver — tries all strategies to boot FECS.
    pub fn falcon_boot_solver(
        &self,
        journal: Option<&dyn acr_boot::BootJournal>,
    ) -> DriverResult<Vec<acr_boot::AcrBootResult>> {
        let chip = sm_to_chip(self.sm_version);
        acr_boot::FalconBootSolver::boot(&self.bar0, chip, Some(self.container.clone()), journal)
    }

    /// Run only the system-memory ACR boot strategy (Exp 083).
    pub fn sysmem_acr_boot(&self) -> acr_boot::AcrBootResult {
        let chip = sm_to_chip(self.sm_version);
        let fw = acr_boot::AcrFirmwareSet::load(chip).expect("firmware load");
        acr_boot::attempt_sysmem_acr_boot(&self.bar0, &fw, self.container.clone())
    }

    /// Sysmem physical DMA boot: simple flow, no instance block binding.
    ///
    /// Uses IOMMU-mapped host memory for ACR/WPR, with the falcon's physical
    /// DMA routed to system memory via `FBIF_TRANSCFG=0x93`. Avoids the
    /// binding step that breaks the boot on fresh-reset SEC2.
    pub fn sysmem_physical_boot(&self) -> acr_boot::AcrBootResult {
        let chip = sm_to_chip(self.sm_version);
        let fw = acr_boot::AcrFirmwareSet::load(chip).expect("firmware load");
        acr_boot::attempt_sysmem_acr_boot_with_config(
            &self.bar0,
            &fw,
            self.container.clone(),
            &acr_boot::BootConfig::exp095_baseline(),
        )
    }

    /// Run the hybrid ACR boot: VRAM page tables + system memory data (Exp 083b).
    pub fn hybrid_acr_boot(&self) -> acr_boot::AcrBootResult {
        let chip = sm_to_chip(self.sm_version);
        let fw = acr_boot::AcrFirmwareSet::load(chip).expect("firmware load");
        acr_boot::attempt_hybrid_acr_boot(&self.bar0, &fw, self.container.clone())
    }

    /// Perform a VFIO device reset (PCI Function Level Reset).
    /// After FLR, ALL GPU state is cleared. The channel, page tables, and
    /// DMA buffers must be re-initialized.
    pub fn device_reset(&self) -> DriverResult<()> {
        self.device.reset()
    }

    /// PCI D3→D0 power cycle. Puts GPU into D3hot sleep and brings it back
    /// to D0, resetting all GPU engines to power-on state.
    pub fn pci_power_cycle(&self) -> DriverResult<(u32, u32)> {
        self.device.pci_power_cycle()
    }

    /// Probe SEC2 falcon state specifically.
    pub fn sec2_probe(&self) -> acr_boot::Sec2Probe {
        acr_boot::Sec2Probe::capture(&self.bar0)
    }

    /// Probe FECS method interface — discover context sizes after falcon boot.
    pub fn fecs_method_probe(&self) -> acr_boot::fecs_method::FecsMethodProbe {
        acr_boot::fecs_method::fecs_probe_methods(&self.bar0)
    }

    /// Apply FECS exception configuration (GP100+).
    pub fn fecs_init_exceptions(&self) {
        acr_boot::fecs_method::fecs_init_exceptions(&self.bar0);
    }

    /// Check whether FECS is alive and responding.
    pub fn fecs_is_alive(&self) -> bool {
        gr_context::fecs_is_alive(&self.bar0)
    }

    /// Discover GR context image sizes from FECS.
    ///
    /// Returns `(image_size, zcull_size, pm_size)` in bytes.
    /// Requires FECS to be running (warm from nouveau or ACR boot).
    pub fn discover_gr_context_sizes(&self) -> DriverResult<(u32, u32, u32)> {
        gr_context::discover_context_sizes(&self.bar0)
    }

    /// Full GR context lifecycle: allocate DMA buffer, bind to FECS, golden save.
    ///
    /// This is the primary entry point for setting up GR context from scratch.
    /// Queries FECS for the required image size, allocates a DMA buffer,
    /// binds it to FECS, and performs a golden save.
    pub fn setup_gr_context(&mut self) -> DriverResult<gr_context::GrContext> {
        let (image_size, _, _) = gr_context::discover_context_sizes(&self.bar0)?;
        let alloc_size = (image_size as usize).max(4096);
        let (_handle, iova) = self.alloc_dma(alloc_size)?;
        gr_context::bind_and_golden_save(&self.bar0, iova)
    }

    /// Probe GR context lifecycle without panicking — returns structured status.
    pub fn probe_gr_context(&mut self) -> gr_context::GrContextStatus {
        if !gr_context::fecs_is_alive(&self.bar0) {
            return gr_context::GrContextStatus {
                description: "FECS not running".into(),
                fecs_alive: false,
                image_size: 0,
                golden_saved: false,
            };
        }
        match self.setup_gr_context() {
            Ok(ctx) => gr_context::GrContextStatus {
                description: format!(
                    "GR context ready: {}B image at IOVA {:#x}",
                    ctx.image_size, ctx.iova
                ),
                fecs_alive: true,
                image_size: ctx.image_size,
                golden_saved: ctx.golden_saved,
            },
            Err(e) => gr_context::GrContextStatus {
                description: format!("GR context setup failed: {e}"),
                fecs_alive: true,
                image_size: 0,
                golden_saved: false,
            },
        }
    }

    pub(super) fn alloc_dma(&mut self, size: usize) -> DriverResult<(BufferHandle, u64)> {
        let aligned = size.div_ceil(4096) * 4096;
        let iova = self.next_iova;
        self.next_iova += aligned as u64;

        let dma = DmaBuffer::new(self.container.clone(), size, iova)?;
        let handle_id = self.next_handle;
        self.next_handle += 1;

        let handle = BufferHandle(handle_id);
        self.buffers.insert(
            handle_id,
            VfioBuffer {
                dma,
                size: size as u64,
            },
        );

        Ok((handle, iova))
    }
}

impl ComputeDevice for NvVfioComputeDevice {
    fn alloc(&mut self, size: u64, _domain: MemoryDomain) -> DriverResult<BufferHandle> {
        let size_usize = usize::try_from(size).map_err(|_| DriverError::AllocFailed {
            size,
            domain: _domain,
            detail: "size exceeds usize".into(),
        })?;
        let (handle, _iova) = self.alloc_dma(size_usize)?;
        Ok(handle)
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
        let slice = buf.dma.as_mut_slice();
        if off + data.len() > slice.len() {
            return Err(DriverError::SubmitFailed(
                "upload exceeds buffer bounds".into(),
            ));
        }
        slice[off..off + data.len()].copy_from_slice(data);
        Ok(())
    }

    fn readback(&self, handle: BufferHandle, offset: u64, len: usize) -> DriverResult<Vec<u8>> {
        let buf = self
            .buffers
            .get(&handle.0)
            .ok_or(DriverError::BufferNotFound(handle))?;
        let off = offset as usize;
        let slice = buf.dma.as_slice();
        if off + len > slice.len() {
            return Err(DriverError::SubmitFailed(
                "readback exceeds buffer bounds".into(),
            ));
        }
        Ok(slice[off..off + len].to_vec())
    }

    fn dispatch(
        &mut self,
        shader: &[u8],
        buffers: &[BufferHandle],
        dims: DispatchDims,
        info: &ShaderInfo,
    ) -> DriverResult<()> {
        let mut temps: Vec<BufferHandle> = Vec::with_capacity(4);
        let result = self.dispatch_inner(shader, buffers, dims, info, &mut temps);
        if result.is_ok() {
            self.inflight.extend(temps);
        } else {
            for h in temps {
                let _ = self.free(h);
            }
        }
        result
    }

    fn sync(&mut self) -> DriverResult<()> {
        self.poll_gpfifo_completion()?;
        let inflight = std::mem::take(&mut self.inflight);
        for handle in inflight {
            let _ = self.free(handle);
        }
        Ok(())
    }
}

impl NvVfioComputeDevice {
    /// Dispatch a compute shader with timed post-doorbell diagnostic captures.
    ///
    /// Identical to `dispatch()` but uses `submit_pushbuf_traced()` internally,
    /// capturing PBDMA + PCCSR state at fixed intervals after the doorbell.
    /// Returns the timed captures on success, or `Err` if submission fails.
    pub fn dispatch_traced(
        &mut self,
        shader: &[u8],
        buffers: &[BufferHandle],
        dims: DispatchDims,
        info: &ShaderInfo,
    ) -> DriverResult<Vec<diagnostics::TimedCapture>> {
        let mut temps: Vec<BufferHandle> = Vec::with_capacity(4);
        let result = self.dispatch_inner_traced(shader, buffers, dims, info, &mut temps);
        match &result {
            Ok(_) => self.inflight.extend(temps),
            Err(_) => {
                for h in temps {
                    let _ = self.free(h);
                }
            }
        }
        result
    }
}

impl Drop for NvVfioComputeDevice {
    fn drop(&mut self) {
        let inflight = std::mem::take(&mut self.inflight);
        for h in inflight {
            let _ = self.free(h);
        }
        let handles: Vec<BufferHandle> = self.buffers.keys().map(|k| BufferHandle(*k)).collect();
        for h in handles {
            let _ = self.free(h);
        }
    }
}

impl std::fmt::Debug for NvVfioComputeDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NvVfioComputeDevice")
            .field("sm_version", &self.sm_version)
            .field("compute_class", &self.compute_class)
            .field("buffers", &self.buffers.len())
            .field("gpfifo_put", &self.gpfifo_put)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_nonexistent_device() {
        let result = NvVfioComputeDevice::open("9999:99:99.9", 86, 0xC6C0);
        assert!(result.is_err());
    }
}
