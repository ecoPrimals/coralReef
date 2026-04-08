// SPDX-License-Identifier: AGPL-3.0-only
//! PFIFO hardware channel creation for Volta+ via BAR0 MMIO.
//!
//! Creates a GPU command channel from scratch using direct register writes,
//! bypassing the kernel GPU driver. This is the bridge between VFIO BAR0/DMA
//! setup and actual GPU command dispatch — without a channel, the GPU's PFIFO
//! engine does not know our GPFIFO ring exists.
//!
//! # Channel creation sequence
//!
//! 1. Allocate DMA buffers for instance block, runlist, and V2 page tables
//! 2. Populate RAMFC (GPFIFO base, USERD pointer, channel ID, signature)
//! 3. Set up V2 MMU page tables (identity map for first 2 MiB of IOVA space)
//! 4. Build runlist with TSG header + channel entry (Volta RAMRL format)
//! 5. Bind instance block to channel via PCCSR registers
//! 6. Enable channel and submit runlist to PFIFO

pub mod devinit;
pub mod glowplug;
pub mod hbm2_training;
#[expect(
    missing_docs,
    reason = "diagnostic oracle — struct fields are self-documenting"
)]
pub mod mmu_oracle;
pub mod nouveau_oracle;
pub mod oracle;
pub mod pri_monitor;
pub mod registers;

pub mod diagnostic;
pub mod mmu_fault;
mod page_tables;
mod pfifo;

pub use diagnostic::{
    ExperimentConfig, ExperimentOrdering, ExperimentResult, GpuCapabilities,
    build_experiment_matrix, build_metal_discovery_matrix, diagnostic_matrix,
    interpreter::{ProbeInterpreter, ProbeReport, memory_probe},
};
pub use pfifo::PfifoInitConfig;
pub use registers::ramuserd;

use std::borrow::Cow;

use crate::error::{DriverError, DriverResult};
use crate::vfio::device::{DmaBackend, MappedBar};
use crate::vfio::dma::DmaBuffer;

use registers::*;

/// PFIFO hardware channel — owns all DMA resources for a single GPU channel.
///
/// Created during `NvVfioComputeDevice::open()` and held alive for
/// the device lifetime. Dropped automatically when the parent device drops,
/// releasing all DMA allocations.
pub struct VfioChannel {
    instance: DmaBuffer,
    runlist: DmaBuffer,
    pd3: DmaBuffer,
    pd2: DmaBuffer,
    pd1: DmaBuffer,
    pd0: DmaBuffer,
    pt0: DmaBuffer,
    #[expect(dead_code, reason = "kept alive for DMA buffer lifecycle")]
    fault_buf: DmaBuffer,
    channel_id: u32,
    runlist_id: u32,
}

impl VfioChannel {
    /// Create and activate a GPU PFIFO channel via BAR0 register programming.
    ///
    /// This performs the full channel lifecycle:
    /// 1. Allocate DMA buffers for instance block, runlist, and page tables
    /// 2. Populate RAMFC (GPFIFO base, USERD, channel ID)
    /// 3. Set up V2 MMU page tables (identity map for first 2 MiB)
    /// 4. Build runlist with TSG header + channel entry
    /// 5. Bind instance block and enable channel via PCCSR
    /// 6. Submit runlist to PFIFO
    ///
    /// # Errors
    ///
    /// Returns error if any DMA allocation or BAR0 write fails.
    pub fn create(
        container: DmaBackend,
        bar0: &MappedBar,
        gpfifo_iova: u64,
        gpfifo_entries: u32,
        userd_iova: u64,
        channel_id: u32,
    ) -> DriverResult<Self> {
        Self::create_with_config(
            container,
            bar0,
            gpfifo_iova,
            gpfifo_entries,
            userd_iova,
            channel_id,
            &pfifo::PfifoInitConfig::default(),
        )
    }

    /// Create a VFIO channel in warm handoff mode — preserves PFIFO/PMC
    /// state from nouveau so falcon engines (FECS/GPCCS) remain alive.
    pub fn create_warm(
        container: DmaBackend,
        bar0: &MappedBar,
        gpfifo_iova: u64,
        gpfifo_entries: u32,
        userd_iova: u64,
        channel_id: u32,
    ) -> DriverResult<Self> {
        Self::create_with_config(
            container,
            bar0,
            gpfifo_iova,
            gpfifo_entries,
            userd_iova,
            channel_id,
            &pfifo::PfifoInitConfig::warm_handoff(),
        )
    }

    /// Create a VFIO channel on a GPU initialized by nvidia-535 with
    /// `NVreg_PreserveHwState=1`. The GPU retains HBM2 training, PMU RM
    /// firmware, and full PRI privilege from the nvidia driver session.
    /// We create our channel alongside the preserved security context.
    pub fn create_sovereign(
        container: DmaBackend,
        bar0: &MappedBar,
        gpfifo_iova: u64,
        gpfifo_entries: u32,
        userd_iova: u64,
        channel_id: u32,
    ) -> DriverResult<Self> {
        Self::create_with_config(
            container,
            bar0,
            gpfifo_iova,
            gpfifo_entries,
            userd_iova,
            channel_id,
            &pfifo::PfifoInitConfig::preserved_nvidia(),
        )
    }

    fn create_with_config(
        container: DmaBackend,
        bar0: &MappedBar,
        gpfifo_iova: u64,
        gpfifo_entries: u32,
        userd_iova: u64,
        channel_id: u32,
        pfifo_cfg: &pfifo::PfifoInitConfig,
    ) -> DriverResult<Self> {
        let instance = DmaBuffer::new(container.clone(), 4096, INSTANCE_IOVA)?;
        let runlist = DmaBuffer::new(container.clone(), 4096, RUNLIST_IOVA)?;
        let pd3 = DmaBuffer::new(container.clone(), 4096, PD3_IOVA)?;
        let pd2 = DmaBuffer::new(container.clone(), 4096, PD2_IOVA)?;
        let pd1 = DmaBuffer::new(container.clone(), 4096, PD1_IOVA)?;
        let pd0 = DmaBuffer::new(container.clone(), 4096, PD0_IOVA)?;
        let pt0 = DmaBuffer::new(container.clone(), 4096, PT0_IOVA)?;
        let fault_buf = DmaBuffer::new(container.clone(), 4096, FAULT_BUF_IOVA)?;

        let pfifo_trace = |bar0: &MappedBar, label: &str| {
            let en = bar0.read_u32(registers::pfifo::ENABLE).unwrap_or(0xDEAD);
            let intr = bar0.read_u32(registers::pfifo::INTR).unwrap_or(0xDEAD);
            tracing::debug!(
                en = format_args!("{en:#010x}"),
                intr = format_args!("{intr:#010x}"),
                "{label}"
            );
        };

        let (runq, target_runlist) = pfifo::init_pfifo_engine_with(bar0, pfifo_cfg)?;

        let mut chan = Self {
            instance,
            runlist,
            pd3,
            pd2,
            pd1,
            pd0,
            pt0,
            fault_buf,
            channel_id,
            runlist_id: target_runlist,
        };
        pfifo_trace(bar0, "after-pfifo-init");

        // Configure BAR2 in PHYSICAL mode targeting system memory.
        // The VRAM-based BAR2 setup (VIRTUAL mode) fails on cold VFIO cards
        // because VRAM is not initialized. PHYSICAL mode bypasses page tables
        // and gives PFIFO a direct path to system memory via PCIe+IOMMU.
        {
            let bar2_val: u32 = 2 << 28; // target=COH, mode=PHYSICAL, ptr=0
            bar0.write_u32(registers::misc::PBUS_BAR2_BLOCK, bar2_val)
                .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("BAR2_BLOCK: {e}"))))?;
            std::thread::sleep(std::time::Duration::from_millis(5));
            tracing::info!(
                bar2_block = format_args!("{bar2_val:#010x}"),
                "BAR2 set to PHYSICAL mode (SYS_MEM_COH)"
            );
        }
        pfifo_trace(bar0, "after-bar2-setup");

        // Volta requires non-replayable fault buffers configured before any
        // MMU translation can succeed. Without them, FBHUB stalls on the
        // first fault entry (nowhere to write it) and subsequent PBUS reads
        // return 0xbad00200. This was the Layer 6 MMU blocker.
        {
            use registers::mmu;
            let fb_lo = (FAULT_BUF_IOVA >> 12) as u32;
            let fb_entries: u32 = 64;
            bar0.write_u32(mmu::FAULT_BUF0_LO, fb_lo).map_err(|e| {
                DriverError::SubmitFailed(Cow::Owned(format!("FAULT_BUF0_LO: {e}")))
            })?;
            bar0.write_u32(mmu::FAULT_BUF0_HI, 0).map_err(|e| {
                DriverError::SubmitFailed(Cow::Owned(format!("FAULT_BUF0_HI: {e}")))
            })?;
            bar0.write_u32(mmu::FAULT_BUF0_SIZE, fb_entries)
                .map_err(|e| {
                    DriverError::SubmitFailed(Cow::Owned(format!("FAULT_BUF0_SIZE: {e}")))
                })?;
            bar0.write_u32(mmu::FAULT_BUF0_GET, 0).map_err(|e| {
                DriverError::SubmitFailed(Cow::Owned(format!("FAULT_BUF0_GET: {e}")))
            })?;
            bar0.write_u32(mmu::FAULT_BUF0_PUT, 0x8000_0000)
                .map_err(|e| {
                    DriverError::SubmitFailed(Cow::Owned(format!("FAULT_BUF0_PUT: {e}")))
                })?;
            bar0.write_u32(mmu::FAULT_BUF1_LO, fb_lo).map_err(|e| {
                DriverError::SubmitFailed(Cow::Owned(format!("FAULT_BUF1_LO: {e}")))
            })?;
            bar0.write_u32(mmu::FAULT_BUF1_HI, 0).map_err(|e| {
                DriverError::SubmitFailed(Cow::Owned(format!("FAULT_BUF1_HI: {e}")))
            })?;
            bar0.write_u32(mmu::FAULT_BUF1_SIZE, fb_entries)
                .map_err(|e| {
                    DriverError::SubmitFailed(Cow::Owned(format!("FAULT_BUF1_SIZE: {e}")))
                })?;
            bar0.write_u32(mmu::FAULT_BUF1_GET, 0).map_err(|e| {
                DriverError::SubmitFailed(Cow::Owned(format!("FAULT_BUF1_GET: {e}")))
            })?;
            bar0.write_u32(mmu::FAULT_BUF1_PUT, 0x8000_0000)
                .map_err(|e| {
                    DriverError::SubmitFailed(Cow::Owned(format!("FAULT_BUF1_PUT: {e}")))
                })?;
            tracing::info!(
                fault_buf_iova = format_args!("{FAULT_BUF_IOVA:#x}"),
                entries = fb_entries,
                "MMU fault buffers configured (non-replayable + replayable)"
            );
        }
        pfifo_trace(bar0, "after-fault-buf-setup");

        page_tables::populate_page_tables(
            chan.pd3.as_mut_slice(),
            chan.pd2.as_mut_slice(),
            chan.pd1.as_mut_slice(),
            chan.pd0.as_mut_slice(),
            chan.pt0.as_mut_slice(),
        );
        page_tables::populate_instance_block(
            chan.instance.as_mut_slice(),
            gpfifo_iova,
            gpfifo_entries,
            userd_iova,
            channel_id,
        );
        page_tables::populate_runlist(
            chan.runlist.as_mut_slice(),
            userd_iova,
            channel_id,
            INSTANCE_IOVA,
            runq,
        );

        Self::invalidate_tlb(bar0, PD3_IOVA)?;
        pfifo_trace(bar0, "after-tlb-invalidate");

        // Clear stale PCCSR state from prior driver (nouveau residue).
        let stale = bar0.read_u32(pccsr::channel(channel_id)).unwrap_or(0);
        if stale != 0 {
            Self::clear_stale_pccsr(bar0, channel_id, stale)?;
        }
        pfifo_trace(bar0, "after-clear-pccsr");

        chan.bind_channel(bar0)?;
        pfifo_trace(bar0, "after-bind-channel");

        std::thread::sleep(std::time::Duration::from_millis(5));
        chan.clear_channel_faults(bar0)?;
        pfifo_trace(bar0, "after-clear-faults");

        chan.enable_channel(bar0)?;
        pfifo_trace(bar0, "after-enable-channel");

        chan.submit_runlist(bar0)?;
        pfifo_trace(bar0, "after-submit-runlist");

        std::thread::sleep(std::time::Duration::from_millis(50));
        pfifo_trace(bar0, "after-50ms-settle");

        // Post-init liveness probe: issue a runlist preempt and check for ACK.
        // On GV100, PFIFO_ENABLE (0x2200) reads 0 even when the engine is
        // functional. The preempt ACK is the authoritative liveness signal.
        let pfifo_live = {
            let w = |reg, val| bar0.write_u32(reg, val).ok();
            w(registers::pfifo::INTR, 0xFFFF_FFFF);
            w(registers::pfifo::GV100_PREEMPT, 1u32 << chan.runlist_id);
            let mut ack = false;
            for _ in 0..25 {
                std::thread::sleep(std::time::Duration::from_millis(2));
                let intr = bar0.read_u32(registers::pfifo::INTR).unwrap_or(0);
                if intr & registers::pfifo::INTR_RL_COMPLETE != 0 {
                    w(registers::pfifo::INTR, registers::pfifo::INTR_RL_COMPLETE);
                    ack = true;
                    break;
                }
            }
            ack
        };
        if pfifo_live {
            tracing::info!("PFIFO liveness probe: preempt ACK received — engine functional");
        } else {
            tracing::warn!("PFIFO liveness probe: NO preempt ACK — engine may be non-responsive");
        }

        pfifo::log_pfifo_diagnostics(bar0);

        let faults = mmu_fault::read_mmu_faults(bar0);
        mmu_fault::log_mmu_faults(&faults);

        tracing::info!(
            channel_id,
            gpfifo_iova = format_args!("{gpfifo_iova:#x}"),
            userd_iova = format_args!("{userd_iova:#x}"),
            instance_iova = format_args!("{INSTANCE_IOVA:#x}"),
            pfifo_live,
            "VFIO PFIFO channel created"
        );

        Ok(chan)
    }

    /// Create a channel with scheduler structures in VRAM.
    ///
    /// On GV100, the PFIFO scheduler cannot DMA-read from system memory.
    /// After a nouveau warm-cycle (HBM2 trained, VRAM alive), we write
    /// the instance block and runlist into VRAM via PRAMIN, while keeping
    /// GPFIFO/USERD in system memory DMA buffers accessed via page table
    /// translation.
    ///
    /// Prerequisites: VRAM alive (nouveau warm-cycle or nvidia init).
    #[expect(
        clippy::cast_possible_truncation,
        reason = "VRAM offsets fit in u32 for our allocation range"
    )]
    pub fn create_vram_sched(
        container: DmaBackend,
        bar0: &MappedBar,
        gpfifo_iova: u64,
        gpfifo_entries: u32,
        userd_iova: u64,
        channel_id: u32,
    ) -> DriverResult<Self> {
        Self::create_vram_sched_on(container, bar0, gpfifo_iova, gpfifo_entries, userd_iova, channel_id, None)
    }

    /// Like [`create_vram_sched`] but targets a specific runlist.
    /// Pass `Some(2)` for a CE runlist (bypasses FECS for GR-free dispatch).
    #[expect(
        clippy::cast_possible_truncation,
        reason = "VRAM offsets fit in u32 for our allocation range"
    )]
    pub fn create_vram_sched_on(
        container: DmaBackend,
        bar0: &MappedBar,
        gpfifo_iova: u64,
        gpfifo_entries: u32,
        userd_iova: u64,
        channel_id: u32,
        override_runlist: Option<u32>,
    ) -> DriverResult<Self> {
        let pfifo_trace = |bar0: &MappedBar, label: &str| {
            let en = bar0.read_u32(registers::pfifo::ENABLE).unwrap_or(0xDEAD);
            let intr = bar0.read_u32(registers::pfifo::INTR).unwrap_or(0xDEAD);
            tracing::debug!(
                en = format_args!("{en:#010x}"),
                intr = format_args!("{intr:#010x}"),
                "{label}"
            );
        };

        let instance = DmaBuffer::new(container.clone(), 4096, INSTANCE_IOVA)?;
        let runlist = DmaBuffer::new(container.clone(), 4096, RUNLIST_IOVA)?;
        let pd3 = DmaBuffer::new(container.clone(), 4096, PD3_IOVA)?;
        let pd2 = DmaBuffer::new(container.clone(), 4096, PD2_IOVA)?;
        let pd1 = DmaBuffer::new(container.clone(), 4096, PD1_IOVA)?;
        let pd0 = DmaBuffer::new(container.clone(), 4096, PD0_IOVA)?;
        let pt0 = DmaBuffer::new(container.clone(), 4096, PT0_IOVA)?;
        let fault_buf = DmaBuffer::new(container.clone(), 4096, FAULT_BUF_IOVA)?;

        // GV100 discovery-only mode: on GV100, PFIFO_ENABLE (0x2200),
        // SCHED_EN (0x2504), and SCHED_DISABLE (0x2630) are non-functional
        // (PRI-gated even with nouveau running). The scheduler runs
        // implicitly when PFIFO is PMC-enabled. We only need PRIV_RING
        // fault clearing and PBDMA/runlist topology discovery.
        let pfifo_cfg = pfifo::PfifoInitConfig::gv100_warm();
        let (_, discovered_runlist) = pfifo::init_pfifo_engine_with(bar0, &pfifo_cfg)?;
        let target_runlist = override_runlist.unwrap_or(discovered_runlist);
        if override_runlist.is_some() {
            tracing::info!(
                discovered = discovered_runlist,
                target = target_runlist,
                "runlist override active"
            );
        }
        pfifo_trace(bar0, "vram-sched: after-pfifo-init");

        // Set up BAR2 VRAM page tables (identity maps IOVAs → system memory).
        pfifo::setup_bar2_page_table(bar0)?;
        pfifo_trace(bar0, "vram-sched: after-bar2-vram-setup");

        // Configure MMU fault buffers.
        {
            use registers::mmu;
            let fb_lo = (FAULT_BUF_IOVA >> 12) as u32;
            let fb_entries: u32 = 64;
            bar0.write_u32(mmu::FAULT_BUF0_LO, fb_lo)
                .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("FAULT_BUF0_LO: {e}"))))?;
            bar0.write_u32(mmu::FAULT_BUF0_HI, 0)
                .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("FAULT_BUF0_HI: {e}"))))?;
            bar0.write_u32(mmu::FAULT_BUF0_SIZE, fb_entries)
                .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("FAULT_BUF0_SIZE: {e}"))))?;
            bar0.write_u32(mmu::FAULT_BUF0_GET, 0)
                .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("FAULT_BUF0_GET: {e}"))))?;
            bar0.write_u32(mmu::FAULT_BUF0_PUT, 0x8000_0000)
                .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("FAULT_BUF0_PUT: {e}"))))?;
            bar0.write_u32(mmu::FAULT_BUF1_LO, fb_lo)
                .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("FAULT_BUF1_LO: {e}"))))?;
            bar0.write_u32(mmu::FAULT_BUF1_HI, 0)
                .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("FAULT_BUF1_HI: {e}"))))?;
            bar0.write_u32(mmu::FAULT_BUF1_SIZE, fb_entries)
                .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("FAULT_BUF1_SIZE: {e}"))))?;
            bar0.write_u32(mmu::FAULT_BUF1_GET, 0)
                .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("FAULT_BUF1_GET: {e}"))))?;
            bar0.write_u32(mmu::FAULT_BUF1_PUT, 0x8000_0000)
                .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("FAULT_BUF1_PUT: {e}"))))?;
        }
        pfifo_trace(bar0, "vram-sched: after-fault-buf");

        let mut chan = Self {
            instance,
            runlist,
            pd3,
            pd2,
            pd1,
            pd0,
            pt0,
            fault_buf,
            channel_id,
            runlist_id: target_runlist,
        };

        // Write instance block (RAMFC + PDB) into VRAM via PRAMIN.
        // Reuse the BAR2 page tables at VRAM 0x21000 for the channel's PDB.
        let vram_pd3 = registers::BAR2_VRAM_BASE + registers::BAR2_PD3_OFF;
        {
            let w = |off: usize, val: u32| {
                bar0.write_u32(registers::misc::PRAMIN_BASE + off, val)
                    .map_err(|e| DriverError::SubmitFailed(Cow::Owned(
                        format!("PRAMIN write inst+{off:#x}: {e}")
                    )))
            };

            // Steer PRAMIN window to CHAN_VRAM_INST region.
            bar0.write_u32(
                registers::misc::BAR0_WINDOW,
                registers::CHAN_VRAM_INST >> 16,
            ).map_err(|e| DriverError::SubmitFailed(Cow::Owned(
                format!("BAR0_WINDOW: {e}")
            )))?;
            std::thread::sleep(std::time::Duration::from_millis(1));

            // Zero the instance block region.
            for off in (0..4096).step_by(4) {
                w(off, 0)?;
            }

            // RAMFC fields — match nouveau gv100_chan_ramfc_write exactly.
            let limit2 = gpfifo_entries.ilog2();
            w(ramfc::USERD_LO, (userd_iova as u32 & 0xFFFF_FE00) | PBDMA_TARGET_SYS_MEM_COHERENT)?;
            w(ramfc::USERD_HI, (userd_iova >> 32) as u32)?;
            w(ramfc::SIGNATURE, 0x0000_FACE)?;
            w(ramfc::ACQUIRE, 0x7FFF_F902)?;
            w(ramfc::GP_BASE_LO, gpfifo_iova as u32)?;
            w(ramfc::GP_BASE_HI, (gpfifo_iova >> 32) as u32 | (limit2 << 16))?;
            w(ramfc::PB_HEADER, 0x2040_0000)?;
            w(ramfc::SUBDEVICE, 0x3000_0000 | 0xFFF)?;
            w(ramfc::HCE_CTRL, 0x0000_0020)?;
            w(ramfc::CHID, channel_id)?;
            // 0x0F4: PBDMA target config — SYS_MEM(bit12) + PRIV(bit8).
            w(0x0F4, 0x0000_1100)?;
            // 0x0F8: PBDMA format/method config (nouveau: 0x10003080).
            w(0x0F8, 0x1000_3080)?;

            // RAMIN PDB: point to BAR2's PD3 in VRAM (target=VID_MEM=0).
            let pdb_lo = vram_pd3
                | (1 << 11)  // BIG_PAGE_SIZE = 64 KiB
                | (1 << 10); // USE_VER2_PT_FORMAT = TRUE
            // target bits [1:0] = 0 (VID_MEM), VOL=0 for VRAM
            w(ramin::PAGE_DIR_BASE_LO, pdb_lo)?;
            w(ramin::PAGE_DIR_BASE_HI, 0)?;
            w(ramin::ADDR_LIMIT_LO, 0xFFFF_FFFF)?;
            w(ramin::ADDR_LIMIT_HI, 0x0001_FFFF)?;
            w(ramin::ENGINE_WFI_VEID, 0)?;
            w(ramin::SC_PDB_VALID, 1)?;
            w(ramin::SC0_PAGE_DIR_BASE_LO, pdb_lo)?;
            w(ramin::SC0_PAGE_DIR_BASE_HI, 0)?;
            w(ramin::SC1_PAGE_DIR_BASE_LO, 1)?;
            w(ramin::SC1_PAGE_DIR_BASE_HI, 1)?;

            tracing::info!(
                vram_inst = format_args!("{:#x}", registers::CHAN_VRAM_INST),
                vram_pd3 = format_args!("{vram_pd3:#x}"),
                "channel instance block written to VRAM"
            );

            // Verify RAMFC readback from VRAM via PRAMIN (window still set).
            let pm = registers::misc::PRAMIN_BASE;
            let rb_userd = bar0.read_u32(pm + ramfc::USERD_LO).unwrap_or(0xDEAD);
            let rb_sig = bar0.read_u32(pm + ramfc::SIGNATURE).unwrap_or(0xDEAD);
            let rb_gpbase = bar0.read_u32(pm + ramfc::GP_BASE_LO).unwrap_or(0xDEAD);
            let rb_0f4 = bar0.read_u32(pm + 0x0F4).unwrap_or(0xDEAD);
            let rb_0f8 = bar0.read_u32(pm + 0x0F8).unwrap_or(0xDEAD);
            let rb_pdb = bar0.read_u32(pm + ramin::PAGE_DIR_BASE_LO).unwrap_or(0xDEAD);
            tracing::info!(
                rb_userd = format_args!("{rb_userd:#010x}"),
                rb_sig = format_args!("{rb_sig:#010x}"),
                rb_gpbase = format_args!("{rb_gpbase:#010x}"),
                rb_0f4 = format_args!("{rb_0f4:#010x}"),
                rb_0f8 = format_args!("{rb_0f8:#010x}"),
                rb_pdb = format_args!("{rb_pdb:#010x}"),
                "VRAM readback verification"
            );
        }
        pfifo_trace(bar0, "vram-sched: after-inst-write");

        // Write runlist (TSG + channel entry) into VRAM via PRAMIN.
        // PRAMIN window is still at CHAN_VRAM_INST >> 16 = 0x3 (VRAM 0x30000).
        // The runlist at 0x31000 is at PRAMIN offset 0x1000 within this window.
        {
            let rl_pramin_off = (registers::CHAN_VRAM_RUNLIST
                - (registers::CHAN_VRAM_INST & 0xFFFF_0000)) as usize;
            let w = |off: usize, val: u32| {
                bar0.write_u32(registers::misc::PRAMIN_BASE + rl_pramin_off + off, val)
                    .map_err(|e| DriverError::SubmitFailed(Cow::Owned(
                        format!("PRAMIN write rl+{off:#x}: {e}")
                    )))
            };

            // Zero the runlist region.
            for off in (0..4096).step_by(4) {
                w(off, 0)?;
            }

            // TSG header (16 bytes) — GV100 RAMRL format:
            //   [26]    = 1 (TSG header marker)
            //   [25:14] = channel count (1)
            //   [7:0]   = timeslice_lo (128 = 0x80)
            let tsg_dw0 = (1u32 << 26) | (1u32 << 14) | 128;
            w(0x00, tsg_dw0)?;
            w(0x04, 0)?; // TSG ID = 0
            w(0x08, 0)?;
            w(0x0C, 0)?;

            // Channel entry (16 bytes) — USERD in system memory, INST in VRAM.
            // DW0 format: [31:12]=USERD_ADDR, [3:2]=TARGET, [1]=RUNQ, [0]=TYPE(0=chan)
            let userd_dw0 = (userd_iova as u32 & 0xFFFF_F000)
                | (TARGET_SYS_MEM_COHERENT << 2);
            w(0x10, userd_dw0)?;
            w(0x14, (userd_iova >> 32) as u32)?;
            // DW2: INST in VRAM (target bits [21:20] = 0 = VID_MEM)
            let inst_dw2 = (registers::CHAN_VRAM_INST & 0xFFFF_F000) | channel_id;
            w(0x18, inst_dw2)?;
            w(0x1C, 0)?;

            tracing::info!(
                vram_runlist = format_args!("{:#x}", registers::CHAN_VRAM_RUNLIST),
                "runlist written to VRAM"
            );
        }
        pfifo_trace(bar0, "vram-sched: after-runlist-write");

        // Restore PRAMIN window.
        bar0.write_u32(registers::misc::BAR0_WINDOW, 0).ok();

        // TLB invalidate for the VRAM page tables.
        // Use VRAM target (0) instead of SYS_MEM_COH (2).
        {
            use registers::pfb;
            for _ in 0..200 {
                let ctrl = bar0.read_u32(pfb::MMU_CTRL).unwrap_or(0);
                if ctrl & 0x00FF_0000 != 0 { break; }
                std::thread::sleep(std::time::Duration::from_micros(100));
            }
            let pdb_inv = ((vram_pd3 as u64) >> 12) << 4; // target=0 (VID_MEM)
            bar0.write_u32(pfb::MMU_INVALIDATE_PDB, pdb_inv as u32)
                .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("TLB inv PDB: {e}"))))?;
            bar0.write_u32(pfb::MMU_INVALIDATE_PDB_HI, 0)
                .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("TLB inv PDB_HI: {e}"))))?;
            bar0.write_u32(pfb::MMU_INVALIDATE, 0x8000_0005)
                .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("TLB inv trigger: {e}"))))?;
            for _ in 0..200 {
                let ctrl = bar0.read_u32(pfb::MMU_CTRL).unwrap_or(0);
                if ctrl & 0x0000_8000 != 0 { break; }
                std::thread::sleep(std::time::Duration::from_micros(100));
            }
            tracing::info!(vram_pd3 = format_args!("{vram_pd3:#x}"), "VRAM TLB invalidated");
        }
        pfifo_trace(bar0, "vram-sched: after-tlb-inv");

        // Clear stale PCCSR.
        let stale = bar0.read_u32(pccsr::channel(channel_id)).unwrap_or(0);
        if stale != 0 {
            Self::clear_stale_pccsr(bar0, channel_id, stale)?;
        }

        // Bind PCCSR to VRAM instance block (target=VID_MEM=0).
        let pccsr_inst_val = (registers::CHAN_VRAM_INST >> 12)
            | pccsr::INST_BIND_TRUE; // target=0 (VID_MEM)
        bar0.write_u32(pccsr::inst(channel_id), pccsr_inst_val)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("PCCSR bind: {e}"))))?;
        tracing::debug!(
            pccsr_inst = format_args!("{pccsr_inst_val:#010x}"),
            "PCCSR bound to VRAM instance block"
        );
        pfifo_trace(bar0, "vram-sched: after-bind");

        std::thread::sleep(std::time::Duration::from_millis(5));
        chan.clear_channel_faults(bar0)?;
        chan.enable_channel(bar0)?;
        pfifo_trace(bar0, "vram-sched: after-enable");

        // UNK260 bracket + PFIFO_ENABLE + FB_TIMEOUT already done in init_pfifo_engine_with.
        let pfifo_rb = bar0.read_u32(registers::pfifo::ENABLE).unwrap_or(0xDEAD);
        tracing::info!(pfifo_en = format_args!("{pfifo_rb:#010x}"), "PFIFO enable (pre-runlist-submit)");

        // Submit runlist from VRAM (no SYS_MEM target).
        let rl_vram_addr = registers::CHAN_VRAM_RUNLIST as u64;
        let rl_base = (rl_vram_addr >> 12) as u32; // target=0 (VID_MEM)
        let rl_submit = 2u32 << 16; // 2 entries, upper_addr=0
        let rl_base_reg = registers::pfifo::runlist_base(chan.runlist_id);
        let rl_submit_reg = registers::pfifo::runlist_submit(chan.runlist_id);
        bar0.write_u32(rl_base_reg, rl_base)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("runlist base: {e}"))))?;
        bar0.write_u32(rl_submit_reg, rl_submit)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("runlist submit: {e}"))))?;
        let rb_base = bar0.read_u32(rl_base_reg).unwrap_or(0xDEAD);
        let rb_submit = bar0.read_u32(rl_submit_reg).unwrap_or(0xDEAD);
        tracing::info!(
            rl_base = format_args!("{rl_base:#010x}"),
            rl_submit = format_args!("{rl_submit:#010x}"),
            rb_base = format_args!("{rb_base:#010x}"),
            rb_submit = format_args!("{rb_submit:#010x}"),
            rl_base_reg = format_args!("{rl_base_reg:#06x}"),
            rl_submit_reg = format_args!("{rl_submit_reg:#06x}"),
            runlist_id = chan.runlist_id,
            "runlist submitted from VRAM (readback verification)"
        );
        pfifo_trace(bar0, "vram-sched: after-runlist-submit");

        // Wait for runlist completion.
        std::thread::sleep(std::time::Duration::from_millis(50));
        let intr_post = bar0.read_u32(registers::pfifo::INTR).unwrap_or(0xDEAD);
        tracing::info!(
            intr = format_args!("{intr_post:#010x}"),
            "vram-sched: post-submit interrupt status"
        );
        pfifo_trace(bar0, "vram-sched: after-50ms-settle");

        // Non-destructive liveness check: read PFIFO_EN and PCCSR state
        // without issuing a preempt (which would evict our channel from PBDMA).
        let pfifo_en = bar0.read_u32(registers::pfifo::ENABLE).unwrap_or(0);
        let pccsr_chan = bar0.read_u32(pccsr::channel(channel_id)).unwrap_or(0);
        let pfifo_live = pccsr_chan & pccsr::CHANNEL_ENABLE_SET != 0;
        tracing::info!(
            pfifo_en = format_args!("{pfifo_en:#010x}"),
            pccsr_chan = format_args!("{pccsr_chan:#010x}"),
            pfifo_live,
            "PFIFO liveness check (no preempt — preserving PBDMA context)"
        );

        let faults = mmu_fault::read_mmu_faults(bar0);
        mmu_fault::log_mmu_faults(&faults);

        tracing::info!(
            channel_id,
            gpfifo_iova = format_args!("{gpfifo_iova:#x}"),
            userd_iova = format_args!("{userd_iova:#x}"),
            pfifo_live,
            "VRAM-sched PFIFO channel created"
        );

        Ok(chan)
    }

    /// Create a channel on a specific runlist (for PBDMA isolation tests).
    ///
    /// Identical to `create` but overrides the auto-discovered GR runlist
    /// with `target_runlist`. Use this to test PBDMA command delivery on
    /// non-GR runlists (e.g. copy engine) independent of FECS state.
    pub fn create_on_runlist(
        container: DmaBackend,
        bar0: &MappedBar,
        gpfifo_iova: u64,
        gpfifo_entries: u32,
        userd_iova: u64,
        channel_id: u32,
        target_runlist: u32,
    ) -> DriverResult<Self> {
        let mut chan = Self::create(
            container,
            bar0,
            gpfifo_iova,
            gpfifo_entries,
            userd_iova,
            channel_id,
        )?;
        if chan.runlist_id != target_runlist {
            tracing::info!(
                from = chan.runlist_id,
                to = target_runlist,
                "overriding runlist for PBDMA isolation"
            );
            chan.runlist_id = target_runlist;
            chan.submit_runlist(bar0)?;
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        Ok(chan)
    }

    /// Channel ID used for doorbell notification.
    #[must_use]
    pub const fn id(&self) -> u32 {
        self.channel_id
    }

    /// BAR0 offset for the USERMODE doorbell register.
    #[must_use]
    pub const fn doorbell_offset() -> usize {
        usermode::NOTIFY_CHANNEL_PENDING
    }

    /// Invalidate the GPU MMU TLB for our page directory base.
    ///
    /// Matches nouveau's `gf100_vmm_invalidate`: write the PDB address to
    /// `MMU_INVALIDATE_PDB`, then trigger with `PAGE_ALL | HUB_ONLY`.
    /// For system memory targets, PDB addr uses the IOVA with target=SYS_COH.
    fn invalidate_tlb(bar0: &MappedBar, pd3_iova: u64) -> DriverResult<()> {
        use registers::pfb;

        // Wait for flush slot availability.
        for _ in 0..200 {
            let ctrl = bar0.read_u32(pfb::MMU_CTRL).unwrap_or(0);
            if ctrl & 0x00FF_0000 != 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_micros(100));
        }

        // PDB address for invalidation: (iova >> 12) << 4 | target.
        // target=2 (SYS_MEM_COH) to match our page table aperture.
        let pdb_inv = ((pd3_iova >> 12) << 4) | 2; // SYS_MEM_COH target
        bar0.write_u32(pfb::MMU_INVALIDATE_PDB, pdb_inv as u32)
            .map_err(|e| {
                DriverError::SubmitFailed(Cow::Owned(format!("MMU_INVALIDATE_PDB: {e}")))
            })?;
        bar0.write_u32(pfb::MMU_INVALIDATE_PDB_HI, (pd3_iova >> 32) as u32)
            .map_err(|e| {
                DriverError::SubmitFailed(Cow::Owned(format!("MMU_INVALIDATE_PDB_HI: {e}")))
            })?;

        // Trigger: PAGE_ALL (bit 0) | HUB_ONLY (bit 2) | trigger (bit 31).
        bar0.write_u32(pfb::MMU_INVALIDATE, 0x8000_0005)
            .map_err(|e| {
                DriverError::SubmitFailed(Cow::Owned(format!("MMU_INVALIDATE trigger: {e}")))
            })?;

        // Wait for flush acknowledgement.
        for _ in 0..200 {
            let ctrl = bar0.read_u32(pfb::MMU_CTRL).unwrap_or(0);
            if ctrl & 0x0000_8000 != 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_micros(100));
        }

        tracing::info!(
            pd3_iova = format_args!("{pd3_iova:#x}"),
            "GPU MMU TLB invalidated"
        );
        Ok(())
    }

    /// Clear stale PCCSR state inherited from a previous driver.
    fn clear_stale_pccsr(bar0: &MappedBar, channel_id: u32, stale: u32) -> DriverResult<()> {
        if stale & 1 != 0 {
            bar0.write_u32(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_CLR)
                .map_err(|e| {
                    DriverError::SubmitFailed(Cow::Owned(format!("PCCSR disable: {e}")))
                })?;
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        bar0.write_u32(
            pccsr::channel(channel_id),
            pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET,
        )
        .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("PCCSR fault clear: {e}"))))?;
        std::thread::sleep(std::time::Duration::from_millis(10));

        bar0.write_u32(pccsr::inst(channel_id), 0)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("PCCSR clear inst: {e}"))))?;
        std::thread::sleep(std::time::Duration::from_millis(10));
        Ok(())
    }

    /// Bind the channel's instance block to PCCSR.
    fn bind_channel(&self, bar0: &MappedBar) -> DriverResult<()> {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "INSTANCE_IOVA >> 12 fits u32 for our allocation range"
        )]
        let value =
            (INSTANCE_IOVA >> 12) as u32 | (TARGET_SYS_MEM_COHERENT << 28) | pccsr::INST_BIND_TRUE;
        tracing::debug!(
            value = format_args!("{value:#010x}"),
            "PCCSR inst (BIND | SYS_MEM_COH)"
        );
        bar0.write_u32(pccsr::inst(self.channel_id), value)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("PCCSR bind: {e}"))))
    }

    /// Clear stale `PBDMA_FAULTED` / `ENG_FAULTED` flags.
    fn clear_channel_faults(&self, bar0: &MappedBar) -> DriverResult<()> {
        let ch = pccsr::channel(self.channel_id);
        let pre = bar0.read_u32(ch).unwrap_or(0);
        if pre & (pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET) != 0 {
            bar0.write_u32(ch, pccsr::CHANNEL_ENABLE_CLR)
                .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("chan disable: {e}"))))?;
            std::thread::sleep(std::time::Duration::from_millis(2));

            bar0.write_u32(ch, pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET)
                .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("fault clear: {e}"))))?;
            std::thread::sleep(std::time::Duration::from_millis(2));

            tracing::debug!(
                pre = format_args!("{pre:#010x}"),
                post = format_args!("{:#010x}", bar0.read_u32(ch).unwrap_or(0xDEAD)),
                "cleared channel faults"
            );
        }
        Ok(())
    }

    /// Enable the channel via PCCSR `ENABLE_SET` trigger.
    fn enable_channel(&self, bar0: &MappedBar) -> DriverResult<()> {
        bar0.write_u32(pccsr::channel(self.channel_id), pccsr::CHANNEL_ENABLE_SET)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("channel enable: {e}"))))
    }

    /// Submit runlist to PFIFO using GV100 per-runlist registers.
    ///
    /// GV100 uses per-runlist registers at stride 0x10:
    ///   BASE(rl) = 0x2270 + rl*0x10   → lower_32(iova >> 12)
    ///   SUBMIT(rl) = 0x2274 + rl*0x10 → upper_32(iova >> 12) | (count << 16)
    /// Writing SUBMIT triggers the scheduler.
    /// Source: nouveau `gv100_runl_commit()`.
    fn submit_runlist(&self, bar0: &MappedBar) -> DriverResult<()> {
        let rl_base = registers::pfifo::gv100_runlist_base_value(RUNLIST_IOVA)
            | (TARGET_SYS_MEM_COHERENT << 28);
        let rl_submit = registers::pfifo::gv100_runlist_submit_value(RUNLIST_IOVA, 2);

        tracing::debug!(
            runlist_id = self.runlist_id,
            rl_base = format_args!("{rl_base:#010x}"),
            rl_submit = format_args!("{rl_submit:#010x}"),
            "submitting runlist (gv100 per-RL)"
        );

        bar0.write_u32(registers::pfifo::runlist_base(self.runlist_id), rl_base)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("runlist base: {e}"))))?;
        bar0.write_u32(registers::pfifo::runlist_submit(self.runlist_id), rl_submit)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("runlist submit: {e}"))))
    }
}

impl std::fmt::Debug for VfioChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VfioChannel")
            .field("channel_id", &self.channel_id)
            .field("instance_iova", &format_args!("{INSTANCE_IOVA:#x}"))
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_info_constants() {
        assert_eq!(VfioChannel::doorbell_offset(), 0x81_0090);
    }
}
