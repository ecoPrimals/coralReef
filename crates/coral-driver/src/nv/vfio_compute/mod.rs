// SPDX-License-Identifier: AGPL-3.0-only
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
pub mod diagnostics;
mod dispatch;
pub mod falcon_capability;
pub mod fecs_boot;
pub mod gr_context;
mod gr_status;
mod init;
mod raw_device;
mod submission;

pub use gr_status::GrEngineStatus;
pub use raw_device::RawVfioDevice;

use crate::error::{DriverError, DriverResult};
use crate::vfio::channel::{GpuChannel, KeplerChannel, VfioChannel};
use crate::vfio::device::{DmaBackend, MappedBar, VfioDevice};
use crate::vfio::dma::DmaBuffer;
use crate::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

use std::collections::HashMap;

/// BAR0 register offsets for NVIDIA GPU.
mod bar0_reg {
    /// Boot0 register — chip identification.
    pub const BOOT0: usize = 0x0000_0000;
}

/// GPFIFO configuration constants.
pub(super) mod gpfifo {
    /// Number of GPFIFO entries (must be power of 2).
    pub const ENTRIES: usize = 128;
    /// Size of each GPFIFO entry in bytes.
    pub const ENTRY_SIZE: usize = 8;
    /// Total GPFIFO ring size in bytes.
    pub const RING_SIZE: usize = ENTRIES * ENTRY_SIZE;

    /// Encode a GPFIFO indirect-buffer entry (NVB06F GP_ENTRY format).
    pub fn encode_entry(gpu_addr: u64, len_bytes: u32) -> u64 {
        let lo = gpu_addr & 0xFFFF_FFFC;
        let hi_addr = (gpu_addr >> 32) & 0xFF;
        let len_dwords = u64::from(len_bytes / 4);
        let hi = hi_addr | (len_dwords << 10);
        lo | (hi << 32)
    }
}

/// IOVA base for user DMA allocations — above GPFIFO/USERD.
const USER_IOVA_BASE: u64 = 0x10_0000;

/// GPFIFO ring IOVA.
const GPFIFO_IOVA: u64 = 0x1000;

/// USERD page IOVA.
const USERD_IOVA: u64 = 0x2000;

/// Local memory window address for Volta+ (SM >= 70).
pub(super) const LOCAL_MEM_WINDOW_VOLTA: u64 = 0xFF00_0000_0000_0000;

/// Local memory window address for pre-Volta (SM < 70).
pub(super) const LOCAL_MEM_WINDOW_LEGACY: u64 = 0xFF00_0000;

/// Map SM version to chip codename for firmware lookup.
///
/// Delegates to [`crate::nv::identity::chip_name`] — single source of truth.
pub(super) const fn sm_to_chip(sm: u32) -> &'static str {
    crate::nv::identity::chip_name(sm)
}

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
    channel: GpuChannel,
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

    /// Resolve SM version and compute class from BOOT0, validating against
    /// caller-supplied hints. Pass `sm_version=0` to auto-detect; pass a
    /// nonzero value to assert it matches hardware.
    fn resolve_sm(
        bar0: &MappedBar,
        bdf: &str,
        caller_sm: u32,
        caller_class: u32,
    ) -> DriverResult<(u32, u32)> {
        let boot0 = bar0.read_u32(bar0_reg::BOOT0)?;
        let hw_sm = crate::nv::identity::boot0_to_sm(boot0);

        let sm =
            if caller_sm == 0 {
                match hw_sm {
                    Some(sm) => {
                        tracing::info!(
                            bdf,
                            boot0 = format_args!("{boot0:#010x}"),
                            sm,
                            "SM auto-detected from BOOT0"
                        );
                        sm
                    }
                    None => {
                        return Err(DriverError::OpenFailed(format!(
                        "BOOT0 {boot0:#010x} maps to unknown chipset — cannot auto-detect SM. \
                         Pass an explicit sm_version or add the chipset to boot0_to_sm()."
                    ).into()));
                    }
                }
            } else {
                if let Some(hw) = hw_sm {
                    if hw != caller_sm {
                        return Err(DriverError::OpenFailed(
                            format!(
                                "SM mismatch: caller passed sm={caller_sm} but BOOT0 {boot0:#010x} \
                         decodes to sm={hw}. Wrong SM corrupts GPU state — aborting."
                            )
                            .into(),
                        ));
                    }
                } else {
                    tracing::warn!(
                        bdf,
                        boot0 = format_args!("{boot0:#010x}"),
                        caller_sm,
                        "BOOT0 chipset unknown — trusting caller-supplied SM"
                    );
                }
                caller_sm
            };

        let compute_class = if caller_class == 0 {
            crate::nv::identity::sm_to_compute_class(sm)
        } else {
            caller_class
        };

        tracing::info!(
            bdf,
            boot0 = format_args!("{boot0:#010x}"),
            sm,
            compute_class = format_args!("{compute_class:#06x}"),
            "VFIO GPU identity resolved"
        );

        Ok((sm, compute_class))
    }

    /// Opens an NVIDIA VFIO compute device by PCI BDF.
    ///
    /// Pass `sm_version=0` and `compute_class=0` to auto-detect from BOOT0.
    /// Nonzero values are validated against the hardware register.
    pub fn open(bdf: &str, sm_version: u32, compute_class: u32) -> DriverResult<Self> {
        let device = VfioDevice::open(bdf)?;
        let container = device.dma_backend();
        let bar0 = device.map_bar(0)?;

        let (sm_version, compute_class) = Self::resolve_sm(&bar0, bdf, sm_version, compute_class)?;

        NvVfioComputeDevice::apply_gr_bar0_init(&bar0, sm_version);

        let gpfifo_ring = DmaBuffer::new(container.clone(), gpfifo::RING_SIZE, GPFIFO_IOVA)?;
        let userd = DmaBuffer::new(container.clone(), 4096, USERD_IOVA)?;

        #[expect(
            clippy::cast_possible_truncation,
            reason = "GPFIFO entries constant always fits u32"
        )]
        let channel = Self::create_channel(
            sm_version,
            container.clone(),
            &bar0,
            GPFIFO_IOVA,
            gpfifo::ENTRIES as u32,
            USERD_IOVA,
        )?;

        let mut dev = Self {
            device,
            bar0,
            sm_version,
            compute_class,
            gpfifo_ring,
            gpfifo_put: 0,
            userd,
            channel,
            next_handle: 1,
            next_iova: USER_IOVA_BASE,
            container,
            buffers: HashMap::new(),
            inflight: Vec::new(),
        };

        dev.apply_fecs_channel_init();

        Ok(dev)
    }

    /// Opens from pre-existing VFIO fds with a cold-boot recipe applied first.
    ///
    /// For non-POST-ed Kepler GPUs (K80), the nvidia-470 cold→warm recipe
    /// must be applied before engines are accessible. This method:
    /// 1. Reconstructs the VFIO device from ember fds
    /// 2. Applies the recipe (engine enable + register writes)
    /// 3. Proceeds with normal GR init + channel creation
    ///
    /// The recipe is a slice of `(BAR0_offset, value)` pairs. Pass an empty
    /// slice to skip recipe application (equivalent to `open_from_fds`).
    pub fn open_from_fds_with_recipe(
        bdf: &str,
        fds: crate::vfio::ReceivedVfioFds,
        sm_version: u32,
        compute_class: u32,
        recipe: &[(u32, u32)],
    ) -> DriverResult<Self> {
        let device = VfioDevice::from_received(bdf, fds)?;
        let container = device.dma_backend();
        let bar0 = device.map_bar(0)?;

        let (sm_version, compute_class) = Self::resolve_sm(&bar0, bdf, sm_version, compute_class)?;

        if !recipe.is_empty() {
            let (applied, failed) = bar0.apply_gr_bar0_writes(recipe);
            tracing::info!(
                applied,
                failed,
                total = recipe.len(),
                "cold-boot recipe applied"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        NvVfioComputeDevice::apply_gr_bar0_init(&bar0, sm_version);

        let gpfifo_ring = DmaBuffer::new(container.clone(), gpfifo::RING_SIZE, GPFIFO_IOVA)?;
        let userd = DmaBuffer::new(container.clone(), 4096, USERD_IOVA)?;

        #[expect(
            clippy::cast_possible_truncation,
            reason = "GPFIFO entries constant always fits u32"
        )]
        let channel = Self::create_channel(
            sm_version,
            container.clone(),
            &bar0,
            GPFIFO_IOVA,
            gpfifo::ENTRIES as u32,
            USERD_IOVA,
        )?;

        let mut dev = Self {
            device,
            bar0,
            sm_version,
            compute_class,
            gpfifo_ring,
            gpfifo_put: 0,
            userd,
            channel,
            next_handle: 1,
            next_iova: USER_IOVA_BASE,
            container,
            buffers: HashMap::new(),
            inflight: Vec::new(),
        };

        dev.apply_fecs_channel_init();
        Ok(dev)
    }

    /// Opens from pre-existing VFIO fds (received from coral-ember via `SCM_RIGHTS`).
    ///
    /// Pass `sm_version=0` and `compute_class=0` to auto-detect from BOOT0.
    /// Nonzero values are validated against the hardware register.
    pub fn open_from_fds(
        bdf: &str,
        fds: crate::vfio::ReceivedVfioFds,
        sm_version: u32,
        compute_class: u32,
    ) -> DriverResult<Self> {
        let device = VfioDevice::from_received(bdf, fds)?;
        let container = device.dma_backend();
        let bar0 = device.map_bar(0)?;

        let (sm_version, compute_class) = Self::resolve_sm(&bar0, bdf, sm_version, compute_class)?;

        NvVfioComputeDevice::apply_gr_bar0_init(&bar0, sm_version);

        let gpfifo_ring = DmaBuffer::new(container.clone(), gpfifo::RING_SIZE, GPFIFO_IOVA)?;
        let userd = DmaBuffer::new(container.clone(), 4096, USERD_IOVA)?;

        #[expect(
            clippy::cast_possible_truncation,
            reason = "GPFIFO entries constant always fits u32"
        )]
        let channel = Self::create_channel(
            sm_version,
            container.clone(),
            &bar0,
            GPFIFO_IOVA,
            gpfifo::ENTRIES as u32,
            USERD_IOVA,
        )?;

        let mut dev = Self {
            device,
            bar0,
            sm_version,
            compute_class,
            gpfifo_ring,
            gpfifo_put: 0,
            userd,
            channel,
            next_handle: 1,
            next_iova: USER_IOVA_BASE,
            container,
            buffers: HashMap::new(),
            inflight: Vec::new(),
        };

        dev.apply_fecs_channel_init();
        Ok(dev)
    }

    /// Create the appropriate GPU channel based on SM version.
    ///
    /// SM >= 70 (Volta+): 5-level V2 page tables, doorbell, GV100 runlists.
    /// SM < 70 (Kepler): 2-level V1 page tables, USERD polling, GK104 runlists.
    fn create_channel(
        sm_version: u32,
        container: DmaBackend,
        bar0: &MappedBar,
        gpfifo_iova: u64,
        gpfifo_entries: u32,
        userd_iova: u64,
    ) -> DriverResult<GpuChannel> {
        if sm_version >= 70 {
            let ch =
                VfioChannel::create(container, bar0, gpfifo_iova, gpfifo_entries, userd_iova, 0)?;
            Ok(GpuChannel::Volta(ch))
        } else {
            tracing::info!(
                sm_version,
                "using Kepler channel path (GF100 V1 page tables)"
            );
            let ch =
                KeplerChannel::create(container, bar0, gpfifo_iova, gpfifo_entries, userd_iova, 0)?;
            Ok(GpuChannel::Kepler(ch))
        }
    }

    /// Open from ember FDs in warm handoff mode.
    ///
    /// After `coralctl warm-fecs` + livepatch, FECS/GPCCS firmware is
    /// preserved in IMEM. This path skips GR BAR0 init (already done by
    /// nouveau) and uses a lighter PFIFO init that preserves PMC/engine state.
    ///
    /// Warm-specific steps (Exp 126):
    /// 1. Clear ALL stale PCCSR entries left by nouveau before channel creation
    /// 2. Verify BAR2 PHYSICAL mode readback after channel creation
    /// 3. Verify MMU fault buffers are fresh (GET == 0, PUT enabled)
    /// 4. Clear stale PFIFO interrupts
    /// 5. Restart falcons with improved wake sequence
    pub fn open_warm(
        bdf: &str,
        fds: crate::vfio::ReceivedVfioFds,
        sm_version: u32,
        compute_class: u32,
    ) -> DriverResult<Self> {
        use crate::vfio::channel::registers::{misc, mmu, pccsr};

        let device = VfioDevice::from_received(bdf, fds)?;
        let container = device.dma_backend();
        let bar0 = device.map_bar(0)?;

        let (sm_version, compute_class) = Self::resolve_sm(&bar0, bdf, sm_version, compute_class)?;

        tracing::info!("warm handoff mode: skipping GR BAR0 init (nouveau already configured)");

        // Exp 126 fix: clear ALL stale PCCSR entries before creating our channel.
        // Nouveau leaves channels in various states; stale entries can confuse the
        // scheduler and block our new channel from being scheduled.
        Self::clear_stale_pccsr_all(&bar0);

        // Clear any accumulated PFIFO interrupts from nouveau's teardown.
        let _ = bar0.write_u32(0x2100, 0xFFFF_FFFF);

        let gpfifo_ring = DmaBuffer::new(container.clone(), gpfifo::RING_SIZE, GPFIFO_IOVA)?;
        let userd = DmaBuffer::new(container.clone(), 4096, USERD_IOVA)?;

        #[expect(
            clippy::cast_possible_truncation,
            reason = "GPFIFO entries constant always fits u32"
        )]
        let channel = GpuChannel::Volta(VfioChannel::create_warm(
            container.clone(),
            &bar0,
            GPFIFO_IOVA,
            gpfifo::ENTRIES as u32,
            USERD_IOVA,
            0,
        )?);

        // Verify BAR2 and fault buffer state after channel creation.
        let bar2_readback = bar0.read_u32(misc::PBUS_BAR2_BLOCK).unwrap_or(0xDEAD);
        let bar2_target = (bar2_readback >> 28) & 0x3;
        tracing::info!(
            bar2_block = format_args!("{bar2_readback:#010x}"),
            target = bar2_target,
            "warm: BAR2_BLOCK verification (expect target=2 COH, mode=PHYS)"
        );

        let fb0_get = bar0.read_u32(mmu::FAULT_BUF0_GET).unwrap_or(0xDEAD);
        let fb0_put = bar0.read_u32(mmu::FAULT_BUF0_PUT).unwrap_or(0xDEAD);
        if fb0_get != 0 {
            tracing::warn!(
                get = format_args!("{fb0_get:#010x}"),
                put = format_args!("{fb0_put:#010x}"),
                "warm: fault buffer 0 has non-zero GET — resetting"
            );
            let _ = bar0.write_u32(mmu::FAULT_BUF0_GET, 0);
            let _ = bar0.write_u32(mmu::FAULT_BUF1_GET, 0);
        }

        // Verify our channel is properly bound in PCCSR.
        let our_pccsr = bar0.read_u32(pccsr::channel(0)).unwrap_or(0);
        let our_inst = bar0.read_u32(pccsr::inst(0)).unwrap_or(0);
        tracing::info!(
            pccsr_inst = format_args!("{our_inst:#010x}"),
            pccsr_chan = format_args!("{our_pccsr:#010x}"),
            "warm: our channel (ch0) PCCSR state"
        );

        let mut dev = Self {
            device,
            bar0,
            sm_version,
            compute_class,
            gpfifo_ring,
            gpfifo_put: 0,
            userd,
            channel,
            next_handle: 1,
            next_iova: USER_IOVA_BASE,
            container,
            buffers: HashMap::new(),
            inflight: Vec::new(),
        };

        dev.restart_warm_falcons()?;

        Ok(dev)
    }

    /// Clear stale PCCSR entries for all 512 channels.
    ///
    /// Nouveau may leave channels enabled with bound instance blocks. After
    /// warm handoff, these stale entries can confuse the PFIFO scheduler
    /// and block our new channel from being scheduled on the GR runlist.
    fn clear_stale_pccsr_all(bar0: &MappedBar) {
        use crate::vfio::channel::registers::pccsr;

        let mut cleared = 0u32;
        for ch in 0..512u32 {
            let chan_val = bar0.read_u32(pccsr::channel(ch)).unwrap_or(0);
            let inst_val = bar0.read_u32(pccsr::inst(ch)).unwrap_or(0);
            if chan_val == 0 && inst_val == 0 {
                continue;
            }
            // Disable channel if enabled.
            if chan_val & 1 != 0 {
                let _ = bar0.write_u32(pccsr::channel(ch), pccsr::CHANNEL_ENABLE_CLR);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            // Clear fault flags.
            let _ = bar0.write_u32(
                pccsr::channel(ch),
                pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET,
            );
            // Clear instance block binding.
            let _ = bar0.write_u32(pccsr::inst(ch), 0);
            cleared += 1;
        }
        tracing::info!(
            cleared,
            "warm: cleared stale PCCSR entries (nouveau residue)"
        );
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
    ///
    /// **Not available on K80 (Kepler) or Titan V (Volta)** — these GPUs
    /// lack FLR hardware. Use [`pmc_soft_reset`] or ember's `device.reset`
    /// (bridge SBR / remove-rescan) instead.
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

    /// PMC soft-reset: toggle engine enable bits via BAR0 to reset GPU
    /// sub-engines without any PCI-level reset.
    ///
    /// This is the only reliable recovery path for GPUs without FLR
    /// (K80/Kepler, Titan V/Volta). The sequence:
    ///
    /// 1. Read current PMC_ENABLE
    /// 2. Write 0 (disable all engines)
    /// 3. Wait for engines to drain
    /// 4. Restore PMC_ENABLE (re-enable all engines)
    ///
    /// After PMC soft-reset, falcon firmware is lost (FECS/GPCCS return
    /// to PRI fault or HALTED state) and must be re-booted. PFIFO and
    /// PBDMA also reset to initial state.
    ///
    /// Returns the PMC_ENABLE value after reset.
    pub fn pmc_soft_reset(&self) -> DriverResult<u32> {
        pmc_soft_reset(&self.bar0)
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

/// PMC soft-reset via BAR0 — the universal GPU recovery path.
///
/// Works on ALL NVIDIA GPUs regardless of FLR support. Toggles
/// PMC_ENABLE to reset all engine clock domains. After this call,
/// engines return to their power-on state (FECS halted/PRI-fault,
/// PFIFO disabled). Firmware must be re-loaded.
///
/// This is the correct recovery for K80 (no FLR) and Titan V (no FLR).
/// For GPUs with FLR, prefer `VfioDevice::reset()` through ember.
pub fn pmc_soft_reset(bar0: &MappedBar) -> DriverResult<u32> {
    use crate::vfio::channel::registers::misc;
    use std::borrow::Cow;

    let pmc_before = bar0.read_u32(misc::PMC_ENABLE).map_err(|e| {
        DriverError::SubmitFailed(Cow::Owned(format!("PMC_ENABLE read: {e}")))
    })?;

    bar0.write_u32(misc::PMC_ENABLE, 0).map_err(|e| {
        DriverError::SubmitFailed(Cow::Owned(format!("PMC_ENABLE disable: {e}")))
    })?;

    std::thread::sleep(std::time::Duration::from_millis(20));

    bar0.write_u32(misc::PMC_ENABLE, pmc_before).map_err(|e| {
        DriverError::SubmitFailed(Cow::Owned(format!("PMC_ENABLE restore: {e}")))
    })?;

    std::thread::sleep(std::time::Duration::from_millis(50));

    let pmc_after = bar0.read_u32(misc::PMC_ENABLE).unwrap_or(0xDEAD_DEAD);

    tracing::info!(
        pmc_before = format_args!("{pmc_before:#010x}"),
        pmc_after = format_args!("{pmc_after:#010x}"),
        "PMC soft-reset complete (all engines toggled)"
    );

    Ok(pmc_after)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpfifo_entry_encoding() {
        let addr = 0x1000_u64;
        let size = 64_u32;
        let entry = gpfifo::encode_entry(addr, size);
        let dw0 = entry as u32;
        assert_eq!(dw0, 0x1000, "DW0 = addr with type=0");
        let dw1 = (entry >> 32) as u32;
        let len_field = (dw1 >> 10) & 0x1F_FFFF;
        assert_eq!(len_field, 16, "length = 16 dwords");
        let recovered = (dw0 as u64 & 0xFFFF_FFFC) | ((dw1 as u64 & 0xFF) << 32);
        assert_eq!(recovered, addr);
    }

    #[test]
    fn gpfifo_entry_zero() {
        assert_eq!(gpfifo::encode_entry(0, 0), 0);
    }

    #[test]
    fn gpfifo_ring_size() {
        assert_eq!(gpfifo::RING_SIZE, 128 * 8);
    }

    #[test]
    fn gpfifo_entry_large_addr() {
        let addr = 0x10_0000_0000_u64;
        let size = 256_u32;
        let entry = gpfifo::encode_entry(addr, size);
        let dw0 = entry as u32;
        let dw1 = (entry >> 32) as u32;
        let recovered = (dw0 as u64 & 0xFFFF_FFFC) | ((dw1 as u64 & 0xFF) << 32);
        assert_eq!(recovered, addr);
        let len_field = (dw1 >> 10) & 0x1F_FFFF;
        assert_eq!(len_field, 64, "length = 64 dwords");
    }

    #[test]
    fn iova_constants_non_overlapping() {
        const { assert!(GPFIFO_IOVA < USERD_IOVA) };
        const { assert!(USERD_IOVA + 4096 <= USER_IOVA_BASE) };
    }

    #[test]
    fn open_nonexistent_device() {
        let result = NvVfioComputeDevice::open("9999:99:99.9", 86, 0xC6C0);
        assert!(result.is_err());
    }

    #[test]
    fn local_mem_window_volta() {
        assert_eq!(LOCAL_MEM_WINDOW_VOLTA, 0xFF00_0000_0000_0000);
    }

    #[test]
    fn local_mem_window_legacy() {
        assert_eq!(LOCAL_MEM_WINDOW_LEGACY, 0xFF00_0000);
    }
}
