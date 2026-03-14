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
//!
//! # Register sources
//!
//! - NVIDIA open-gpu-doc `dev_fifo.ref.txt` (PCCSR, PFIFO runlist)
//! - NVIDIA open-gpu-doc `dev_ram.ref.txt` (RAMFC, RAMUSERD, RAMRL, RAMIN)
//! - NVIDIA open-gpu-doc `dev_usermode.ref.txt` (doorbell)
//! - nouveau `nvkm/engine/fifo/gv100.c` (Volta RAMFC write, runlist format)

use crate::error::{DriverError, DriverResult};
use crate::vfio::device::MappedBar;
use crate::vfio::dma::DmaBuffer;

use std::borrow::Cow;
use std::os::fd::RawFd;

// ── BAR0 register offsets ──────────────────────────────────────────────

/// NV_PFIFO registers (BAR0 + 0x2000..0x3FFF).
mod pfifo {
    /// PFIFO engine enable — toggle 0→1 to reset scheduler + PBDMAs.
    pub const ENABLE: usize = 0x0000_2200;
    /// FIFO scheduler enable (1 = enabled).
    pub const SCHED_EN: usize = 0x0000_2504;
    /// PFIFO interrupt status — write 0xFFFFFFFF to clear pending.
    pub const INTR: usize = 0x0000_2100;
    /// PFIFO interrupt enable mask.
    pub const INTR_EN: usize = 0x0000_2140;
    /// PBDMA active map — bit N = 1 means PBDMA N exists.
    pub const PBDMA_MAP: usize = 0x0000_2004;
    /// Runlist base address and aperture target.
    pub const RUNLIST_BASE: usize = 0x0000_2270;
    /// Runlist submit: length + runlist ID.
    pub const RUNLIST: usize = 0x0000_2274;
}

/// NV_PMC — Power Management Controller / engine enables.
mod pmc {
    /// Master engine enable — write 0xFFFFFFFF to un-gate all clock domains.
    /// After readback, bits that remained 1 correspond to present engines.
    pub const ENABLE: usize = 0x0000_0200;
    /// PBDMA master enable — set bit N to enable PBDMA N.
    pub const PBDMA_ENABLE: usize = 0x0000_0204;
    /// PBDMA interrupt routing.
    pub const PBDMA_INTR_EN: usize = 0x0000_2A04;
}

/// Per-PBDMA registers (stride 0x2000, base 0x040000).
mod pbdma {
    const BASE: usize = 0x0004_0000;
    const STRIDE: usize = 0x2000;

    const fn reg(id: usize, off: usize) -> usize {
        BASE + id * STRIDE + off
    }

    /// PBDMA interrupt status — write 0xFFFFFFFF to clear.
    pub const fn intr(id: usize) -> usize {
        reg(id, 0x0108)
    }
    /// PBDMA interrupt enable mask.
    pub const fn intr_en(id: usize) -> usize {
        reg(id, 0x010C)
    }
    /// PBDMA HCE (Host Compute Engine) interrupt status.
    pub const fn hce_intr(id: usize) -> usize {
        reg(id, 0x0148)
    }
    /// PBDMA HCE interrupt enable mask.
    pub const fn hce_intr_en(id: usize) -> usize {
        reg(id, 0x014C)
    }
}

/// NV_PCCSR — per-channel control/status registers (BAR0 + 0x800000).
mod pccsr {
    /// Instance block pointer register for channel `id`.
    /// Contains INST_PTR[27:0], INST_TARGET[29:28], INST_BIND[31].
    pub const fn inst(id: u32) -> usize {
        0x0080_0000 + (id as usize) * 8
    }

    /// Channel control register for channel `id`.
    /// Contains ENABLE[0], ENABLE_SET[10], ENABLE_CLR[11], STATUS[27:24].
    pub const fn channel(id: u32) -> usize {
        0x0080_0004 + (id as usize) * 8
    }

    /// INST_TARGET = SYS_MEM_NONCOHERENT (bits [29:28] = 3).
    /// Nouveau uses NCOH for system memory (gk104_runl_commit: target=3).
    /// On x86, PCIe DMA is always coherent regardless of this bit, but
    /// the PFIFO may require target=3 for system memory paths.
    pub const INST_TARGET_SYS_MEM_NCOH: u32 = 3 << 28;
    /// INST_BIND = TRUE (bit 31).
    pub const INST_BIND_TRUE: u32 = 1 << 31;
    /// CHANNEL_ENABLE_SET trigger (bit 10).
    pub const CHANNEL_ENABLE_SET: u32 = 1 << 10;
    /// CHANNEL_ENABLE_CLR trigger (bit 11).
    pub const CHANNEL_ENABLE_CLR: u32 = 1 << 11;
    /// PBDMA_FAULTED — write 1 to clear (bit 24).
    pub const PBDMA_FAULTED_RESET: u32 = 1 << 24;
    /// ENG_FAULTED — write 1 to clear (bit 28).
    pub const ENG_FAULTED_RESET: u32 = 1 << 28;
}

/// NV_USERMODE doorbell (BAR0 + 0x810000..0x81FFFF).
mod usermode {
    /// Write channel ID here to notify Host that a channel has new work.
    /// Equivalent to setting PENDING in NV_PCCSR_CHANNEL_STATUS.
    pub const NOTIFY_CHANNEL_PENDING: usize = 0x0081_0090;
}

/// Volta RAMUSERD (User-Driver State Descriptor) offsets within a 512-byte
/// channel USERD page. These offsets are from NVIDIA `dev_ram.ref.txt`.
pub mod ramuserd {
    /// GP_GET: next GP entry the GPU will process (GPU writes, host reads).
    /// NV_RAMUSERD_GP_GET = dword 34 = byte offset 0x88.
    pub const GP_GET: usize = 34 * 4;
    /// GP_PUT: next GP entry available for GPU (host writes, GPU reads).
    /// NV_RAMUSERD_GP_PUT = dword 35 = byte offset 0x8C.
    pub const GP_PUT: usize = 35 * 4;
}

// ── RAMFC (FIFO Context) offsets within the 512-byte RAMFC region ──────

/// Offsets within the RAMFC region of the instance block.
/// Derived from `gv100_chan_ramfc_write()` and `dev_ram.ref.txt`.
mod ramfc {
    /// NV_RAMFC_USERD (dword 2) — USERD base address low + aperture target.
    pub const USERD_LO: usize = 0x008;
    /// NV_RAMFC_USERD_HI (dword 3) — USERD base address high.
    pub const USERD_HI: usize = 0x00C;
    /// NV_RAMFC_SIGNATURE (dword 4) — channel signature (0xFACE).
    pub const SIGNATURE: usize = 0x010;
    /// NV_RAMFC_ACQUIRE (dword 12) — semaphore acquire timeout.
    pub const ACQUIRE: usize = 0x030;
    /// NV_RAMFC_GP_BASE (dword 18) — GPFIFO ring GPU VA low.
    pub const GP_BASE_LO: usize = 0x048;
    /// NV_RAMFC_GP_BASE_HI (dword 19) — GPFIFO ring GPU VA high + limit.
    pub const GP_BASE_HI: usize = 0x04C;
    /// NV_RAMFC_PB_HEADER (dword 33).
    pub const PB_HEADER: usize = 0x084;
    /// NV_RAMFC_SUBDEVICE (dword 37) — subdevice mask.
    pub const SUBDEVICE: usize = 0x094;
    /// NV_RAMFC_HCE_CTRL (dword 57) — host compute engine control.
    pub const HCE_CTRL: usize = 0x0E4;
    /// Channel ID (Volta-specific, dword 58).
    pub const CHID: usize = 0x0E8;
    /// NV_RAMFC_CONFIG (dword 61) — PBDMA configuration.
    pub const CONFIG: usize = 0x0F4;
    /// Volta-specific channel info (dword 62).
    pub const CHANNEL_INFO: usize = 0x0F8;
}

/// NV_RAMIN offsets beyond RAMFC for MMU page directory configuration.
mod ramin {
    /// Page directory base — low word (DW128, offset 0x200).
    /// Contains TARGET[1:0], VOL[2], VER2_PT[10], BIG_PAGE[11], ADDR_LO[31:12].
    pub const PAGE_DIR_BASE_LO: usize = 128 * 4;
    /// Page directory base — high word (DW129, offset 0x204).
    pub const PAGE_DIR_BASE_HI: usize = 129 * 4;
    /// Engine WFI VEID (DW134, offset 0x218).
    pub const ENGINE_WFI_VEID: usize = 134 * 4;
    /// Subcontext PDB valid bitmap (DW166, offset 0x298).
    pub const SC_PDB_VALID: usize = 166 * 4;
    /// Subcontext 0 page directory base — low word (DW168, offset 0x2A0).
    pub const SC0_PAGE_DIR_BASE_LO: usize = 168 * 4;
    /// Subcontext 0 page directory base — high word (DW169, offset 0x2A4).
    pub const SC0_PAGE_DIR_BASE_HI: usize = 169 * 4;
}

// ── IOVA assignments for channel infrastructure DMA buffers ────────────

const INSTANCE_IOVA: u64 = 0x3000;
const RUNLIST_IOVA: u64 = 0x4000;
const PD3_IOVA: u64 = 0x5000;
const PD2_IOVA: u64 = 0x6000;
const PD1_IOVA: u64 = 0x7000;
const PD0_IOVA: u64 = 0x8000;
const PT0_IOVA: u64 = 0x9000;

/// SYS_MEM_COHERENT aperture target for PCCSR/PFIFO/RAMIN/runlist registers.
/// PCCSR_INST_TARGET[29:28]: 0=VRAM, 2=COHERENT_SYSMEM, 3=NONCOHERENT_SYSMEM
/// RUNLIST_BASE_TARGET[31:28]: same encoding as PCCSR.
/// RAMRL DW0 USERD_TARGET[3:2] and DW2 INST_TARGET[5:4]: same encoding.
const TARGET_SYS_MEM_COHERENT: u32 = 2;

/// SYS_MEM_NONCOHERENT aperture target (PCCSR/RAMIN/runlist encoding).
const TARGET_SYS_MEM_NONCOHERENT: u32 = 3;

/// SYS_MEM_COHERENT aperture target for PBDMA registers (RAMFC fields).
/// NV_PPBDMA_USERD_TARGET[1:0]: 0=VID_MEM, 1=SYS_MEM_COHERENT, 2=SYS_MEM_NONCOHERENT
/// These differ from the PCCSR/RAMIN encoding!
const PBDMA_TARGET_SYS_MEM_COHERENT: u32 = 1;

/// Number of 4 KiB pages identity-mapped in PT0.
const PT_ENTRIES: usize = 512;

// ── PFIFO hardware channel ────────────────────────────────────────────

/// PFIFO hardware channel — owns all DMA resources for a single GPU channel.
///
/// Created during [`super::NvVfioComputeDevice::open()`] and held alive for
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
        container_fd: RawFd,
        bar0: &MappedBar,
        gpfifo_iova: u64,
        gpfifo_entries: u32,
        userd_iova: u64,
        channel_id: u32,
    ) -> DriverResult<Self> {
        let instance = DmaBuffer::new(container_fd, 4096, INSTANCE_IOVA)?;
        let runlist = DmaBuffer::new(container_fd, 4096, RUNLIST_IOVA)?;
        let pd3 = DmaBuffer::new(container_fd, 4096, PD3_IOVA)?;
        let pd2 = DmaBuffer::new(container_fd, 4096, PD2_IOVA)?;
        let pd1 = DmaBuffer::new(container_fd, 4096, PD1_IOVA)?;
        let pd0 = DmaBuffer::new(container_fd, 4096, PD0_IOVA)?;
        let pt0 = DmaBuffer::new(container_fd, 4096, PT0_IOVA)?;

        let mut chan = Self {
            instance,
            runlist,
            pd3,
            pd2,
            pd1,
            pd0,
            pt0,
            channel_id,
            runlist_id: 0, // will be set by init_pfifo_engine discovery
        };

        // Read PCCSR state to detect if GPU is warm (nouveau-initialized) or cold (after reset).
        let boot0 = bar0.read_u32(0).unwrap_or(0xDEAD);
        let raw_pmc = bar0.read_u32(pmc::ENABLE).unwrap_or(0);
        let raw_chan = bar0.read_u32(pccsr::channel(channel_id)).unwrap_or(0xDEAD);
        eprintln!("║ BOOT0={boot0:#010x} PMC_EN={raw_pmc:#010x} PCCSR_CHAN={raw_chan:#010x}");

        let gpu_warm = boot0 != 0xFFFF_FFFF && boot0 != 0xBAD0_DA00 && raw_pmc != 0;

        // Always run full PFIFO init — on a warm GPU it just re-enables clocks
        // and resets the PFIFO scheduler + PBDMAs, which is safe.
        let (runq, runlist_id) = Self::init_pfifo_engine(bar0)?;
        chan.runlist_id = runlist_id;

        // Probe: read VRAM at offset 0x3000 via PRAMIN to check what the scheduler
        // would see if it reads the instance block from VRAM (target=0).
        const NV_PBUS_BAR0_WINDOW: usize = 0x0000_1700;
        const PRAMIN_BASE: usize = 0x0070_0000;
        let _ = bar0.write_u32(NV_PBUS_BAR0_WINDOW, 0);
        // PRAMIN maps a 64K window starting at VRAM offset = BAR0_WINDOW * 0x10000.
        // To read VRAM:0x3000, set BAR0_WINDOW = 0 → PRAMIN base = VRAM:0.
        // Then read PRAMIN_BASE + 0x3000.
        let vram_3000 = bar0.read_u32(PRAMIN_BASE + 0x3000).unwrap_or(0xDEAD);
        let vram_3008 = bar0.read_u32(PRAMIN_BASE + 0x3008).unwrap_or(0xDEAD);
        let vram_3048 = bar0.read_u32(PRAMIN_BASE + 0x3048).unwrap_or(0xDEAD);
        let vram_3200 = bar0.read_u32(PRAMIN_BASE + 0x3200).unwrap_or(0xDEAD);
        eprintln!("║ VRAM probe: [0x3000]={vram_3000:#010x} [0x3008]={vram_3008:#010x} [0x3048]={vram_3048:#010x} [0x3200]={vram_3200:#010x}");

        chan.populate_page_tables();
        chan.populate_instance_block(gpfifo_iova, gpfifo_entries, userd_iova);
        chan.populate_runlist(userd_iova, runq);

        // ── Clear stale PCCSR state from prior driver (nouveau residue) ──
        // After empty runlist flush, check if channel 0 still has fault flags.
        let stale = bar0.read_u32(pccsr::channel(channel_id)).unwrap_or(0);
        eprintln!("║ PCCSR_CHAN post-flush: {stale:#010x}  PBDMA_FAULT={} ENG_FAULT={}",
            (stale >> 24) & 1, (stale >> 28) & 1);

        if stale != 0 {
            // Disable channel if it was left enabled.
            if stale & 1 != 0 {
                bar0.write_u32(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_CLR)
                    .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("PCCSR disable: {e}"))))?;
                std::thread::sleep(std::time::Duration::from_millis(10));
            }

            // Clear fault bits (W1C) — must be done after channel is disabled.
            bar0.write_u32(
                pccsr::channel(channel_id),
                pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET,
            )
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("PCCSR fault clear: {e}"))))?;
            std::thread::sleep(std::time::Duration::from_millis(10));

            // Clear instance binding (nouveau gk104_chan_unbind).
            bar0.write_u32(pccsr::inst(channel_id), 0)
                .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("PCCSR clear inst: {e}"))))?;
            std::thread::sleep(std::time::Duration::from_millis(10));

            let after_clear = bar0.read_u32(pccsr::channel(channel_id)).unwrap_or(0);
            eprintln!("║ PCCSR_CHAN after clear: {after_clear:#010x}");
        }

        chan.bind_channel(bar0)?;
        std::thread::sleep(std::time::Duration::from_millis(5));

        // Bind may inherit stale faults — clear them before enabling.
        chan.clear_channel_faults(bar0)?;

        let pre_enable = bar0.read_u32(pccsr::channel(channel_id)).unwrap_or(0xDEAD);
        eprintln!("║ PCCSR_CHAN pre-enable: {pre_enable:#010x}  PBDMA_FAULT={}", (pre_enable >> 24) & 1);

        chan.enable_channel(bar0)?;

        // Check PCCSR state after enable, before runlist submit.
        let post_enable = bar0.read_u32(pccsr::channel(channel_id)).unwrap_or(0xDEAD);
        eprintln!("║ PCCSR_CHAN post-enable: {post_enable:#010x}  PBDMA_FAULT={}", (post_enable >> 24) & 1);

        chan.submit_runlist(bar0)?;

        // Brief delay for scheduler to process runlist.
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Check PCCSR state after runlist submit (scheduler may have assigned channel).
        let post_runlist = bar0.read_u32(pccsr::channel(channel_id)).unwrap_or(0xDEAD);
        eprintln!("║ PCCSR_CHAN post-runlist: {post_runlist:#010x}  PBDMA_FAULT={}", (post_runlist >> 24) & 1);

        Self::log_pfifo_diagnostics(bar0);

        // Hex dump of runlist + instance block for validation.
        {
            let rl_bytes = chan.runlist.as_slice();
            eprintln!("║ ── RUNLIST hex (32 bytes) ──");
            for i in (0..32).step_by(4) {
                let dw = u32::from_le_bytes([rl_bytes[i], rl_bytes[i+1], rl_bytes[i+2], rl_bytes[i+3]]);
                eprintln!("║  RL[{i:#04x}] = {dw:#010x}");
            }
            let inst_bytes = chan.instance.as_slice();
            eprintln!("║ ── INSTANCE (RAMFC + RAMIN) ──");
            for &(off, name) in &[
                (ramfc::USERD_LO, "USERD_LO"),
                (ramfc::USERD_HI, "USERD_HI"),
                (ramfc::SIGNATURE, "SIGNATURE"),
                (ramfc::ACQUIRE, "ACQUIRE"),
                (ramfc::GP_BASE_LO, "GP_BASE_LO"),
                (ramfc::GP_BASE_HI, "GP_BASE_HI"),
                (ramfc::PB_HEADER, "PB_HEADER"),
                (ramfc::SUBDEVICE, "SUBDEVICE"),
                (ramfc::HCE_CTRL, "HCE_CTRL"),
                (ramfc::CHID, "CHID"),
                (ramfc::CONFIG, "CONFIG"),
                (ramfc::CHANNEL_INFO, "CHANNEL_INFO"),
                (ramin::PAGE_DIR_BASE_LO, "PDB_LO"),
                (ramin::PAGE_DIR_BASE_HI, "PDB_HI"),
            ] {
                let off: usize = off;
                if off + 4 <= inst_bytes.len() {
                    let dw = u32::from_le_bytes([
                        inst_bytes[off], inst_bytes[off+1],
                        inst_bytes[off+2], inst_bytes[off+3],
                    ]);
                    eprintln!("║  [{off:#05x}] {name:15} = {dw:#010x}");
                }
            }
        }

        tracing::info!(
            channel_id,
            gpfifo_iova = format_args!("{gpfifo_iova:#x}"),
            userd_iova = format_args!("{userd_iova:#x}"),
            instance_iova = format_args!("{INSTANCE_IOVA:#x}"),
            "VFIO PFIFO channel created"
        );

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

    // ── PFIFO engine initialization ──────────────────────────────────

    /// Enable the PFIFO engine in PMC, discover PBDMAs, and initialize.
    ///
    /// Returns the RUNQ selector (0-based index into the PBDMAs serving
    /// runlist 0). The RUNQ field in the runlist channel entry is this
    /// index, NOT the hardware PBDMA ID.
    ///
    /// After VFIO FLR the GPU's engine clock domains are gated — PFIFO
    /// registers read `0xBAD0DA00`.  We must enable the engine in
    /// `NV_PMC_ENABLE` (0x200) first, matching nouveau's `gp100_mc_init()`
    /// which writes `0xFFFFFFFF` to un-gate all clock domains.
    ///
    /// Then we discover available PBDMAs via `NV_PFIFO_PBDMA_MAP` (0x2004)
    /// and run the init sequence from nouveau: `gk104_fifo_init()` +
    /// `gk104_fifo_init_pbdmas()` + `gf100_runq_init()` + `gk104_runq_init()`.
    fn init_pfifo_engine(bar0: &MappedBar) -> DriverResult<(u32, u32)> {
        let w = |reg: usize, val: u32| {
            bar0.write_u32(reg, val)
                .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("PFIFO init {reg:#x}: {e}"))))
        };

        // ── Step 0: Enable all engines in PMC (un-gate clock domains) ────
        let pmc_before = bar0.read_u32(pmc::ENABLE).unwrap_or(0);
        w(pmc::ENABLE, 0xFFFF_FFFF)?;
        let pmc_after = bar0.read_u32(pmc::ENABLE).unwrap_or(0);

        tracing::info!(
            pmc_before = format_args!("{pmc_before:#010x}"),
            pmc_after = format_args!("{pmc_after:#010x}"),
            "PMC engine enable"
        );

        // ── Step 1: Verify PFIFO is enabled (warm from nouveau) ──────────
        // On a warm GPU, the scheduler is already running. Toggling
        // PFIFO_ENABLE would reset the scheduler to a state we cannot
        // reinitialize from userspace (SCHED_EN at 0x2504 is inaccessible
        // on GV100). Instead, we verify PFIFO is enabled and proceed to
        // flush stale channels via empty runlist submissions.
        let pfifo_en = bar0.read_u32(pfifo::ENABLE).unwrap_or(0);
        if pfifo_en == 0 {
            // Cold GPU — must toggle PFIFO to initialize.
            w(pfifo::ENABLE, 0)?;
            std::thread::sleep(std::time::Duration::from_millis(2));
            w(pfifo::ENABLE, 1)?;
            std::thread::sleep(std::time::Duration::from_millis(2));
            eprintln!("║ PFIFO was disabled, toggled 0→1");
        } else {
            eprintln!("║ PFIFO already enabled ({pfifo_en:#010x}), preserving scheduler");
        }

        // ── Step 2: Discover PBDMAs and their runlist assignments ────────
        let pbdma_map = bar0.read_u32(pfifo::PBDMA_MAP).unwrap_or(0);
        if pbdma_map == 0 {
            return Err(DriverError::SubmitFailed(Cow::Borrowed(
                "no PBDMAs found in PBDMA_MAP (0x2004)",
            )));
        }

        // Dump raw TOP_INFO entries and parse the GR engine's runlist.
        // gk104_top chained format: each device is 1-3 entries, bit 31 = chain end.
        let mut gr_runlist: Option<u32> = None;
        let mut cur_type: u32 = 0xFFFF;
        let mut cur_runlist: u32 = 0xFFFF;
        for i in 0..64_u32 {
            let data = bar0.read_u32(0x0002_2700 + (i as usize) * 4).unwrap_or(0);
            if data == 0 {
                break;
            }
            let kind = data & 3;
            match kind {
                1 => cur_type = (data >> 2) & 0x3F,
                3 => cur_runlist = (data >> 11) & 0x1F,
                _ => {}
            }
            eprintln!("║ TOP[{i:2}] = {data:#010x}  kind={kind} {}{}",
                if kind == 1 { format!("type={} inst={}", (data >> 2) & 0x3F, (data >> 6) & 0xFF) }
                else if kind == 2 { format!("addr={:#x} fault={}", (data >> 12) & 0xFFF, (data >> 3) & 0x1F) }
                else if kind == 3 { format!("runlist={} engine={} reset={}", (data >> 11) & 0x1F, (data >> 7) & 0xF, data & 0x7F) }
                else { String::new() },
                if data & (1 << 31) != 0 { " [END]" } else { "" },
            );
            if data & (1 << 31) != 0 {
                if cur_type == 0 && gr_runlist.is_none() && cur_runlist != 0xFFFF {
                    gr_runlist = Some(cur_runlist);
                    eprintln!("║   → GR engine found on runlist {cur_runlist}");
                }
                cur_type = 0xFFFF;
                cur_runlist = 0xFFFF;
            }
        }

        // Scan PBDMA-to-runlist map. On GV100 the registers at 0x002390+i*4
        // use SEQUENTIAL indexing (i = 0..pbdma_count-1), NOT by PBDMA ID.
        let pbdma_count = pbdma_map.count_ones();
        let mut pbdma_ids: Vec<u32> = Vec::new();
        for pid in 0..32_u32 {
            if pbdma_map & (1 << pid) != 0 {
                pbdma_ids.push(pid);
            }
        }
        let mut pbdma_runlists: Vec<(u32, u32)> = Vec::new(); // (pbdma_id, runlist_id)
        for (seq, &pid) in pbdma_ids.iter().enumerate() {
            let rl = bar0.read_u32(0x0000_2390 + seq * 4).unwrap_or(0xFFFF);
            pbdma_runlists.push((pid, rl));
            eprintln!("║ PBDMA {pid:2} (seq={seq}) → runlist {rl}");
        }

        // Use GR runlist if found, otherwise first PBDMA's runlist.
        let target_runlist = gr_runlist.unwrap_or_else(|| pbdma_runlists.first().map_or(0, |e| e.1));
        eprintln!("║ GR runlist: {gr_runlist:?}, using runlist {target_runlist}");
        eprintln!("║ PBDMA_MAP={pbdma_map:#010x}");

        tracing::info!(
            pbdma_map = format_args!("{pbdma_map:#010x}"),
            target_runlist,
            "PBDMA/runlist discovery"
        );

        // ── Step 3: Per-PBDMA init (gk104_fifo_init_pbdmas + gk208_runq_init) ──
        for id in 0..32_usize {
            if pbdma_map & (1 << id) == 0 {
                continue;
            }
            let b = 0x0004_0000 + id * 0x2000;

            // gk104_fifo_init_pbdmas: clear + enable PBDMA interrupts.
            w(pbdma::intr(id), 0xFFFF_FFFF)?;
            w(pbdma::intr_en(id), 0xFFFF_FEFF)?;

            // gf100_runq_init: clear PBDMA METHOD0 register (stale methods).
            w(b + 0x13C, 0)?;

            // gk104_runq_init: clear + disable HCE interrupts.
            w(pbdma::hce_intr(id), 0)?;
            w(pbdma::hce_intr_en(id), 0)?;

            // gk208_runq_init: initialize PBDMA token register.
            w(b + 0x164, 0xFFFF_FFFF)?;
        }

        // ── Step 4: Clear + enable PFIFO interrupts ──────────────────────
        w(pfifo::INTR, 0xFFFF_FFFF)?;
        w(pfifo::INTR_EN, 0x7FFF_FFFF)?;

        // ── Step 4b: Enable PFIFO scheduler (nouveau gk104_fifo_init) ──
        // The PFIFO toggle resets the scheduler. It must be re-enabled
        // explicitly or runlists are accepted but never processed.
        w(pfifo::SCHED_EN, 1)?;

        // ── Step 5: Submit empty runlists to flush stale channels ────────
        // GK104 format: 0x2270 = (target<<28)|(addr>>12), 0x2274 = (rl_id<<20)|count.
        // Submitting 0-entry runlists tells the scheduler to unload all channels.
        let mut flushed_runlists = std::collections::HashSet::new();
        #[expect(clippy::cast_possible_truncation)]
        let rl_base = (3_u32 << 28) | (RUNLIST_IOVA >> 12) as u32;
        for &(_, rl) in &pbdma_runlists {
            if rl > 31 || !flushed_runlists.insert(rl) {
                continue;
            }
            w(0x2270, rl_base)?;
            w(0x2274, (rl << 20) | 0)?; // 0 entries
            std::thread::sleep(std::time::Duration::from_millis(5));
            eprintln!("║ Flushed runlist {rl} (empty)");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));

        // Also confirm GR runlist via ENGN0_STATUS register (bits [15:12] = runlist ID).
        let engn0 = bar0.read_u32(0x0000_2640).unwrap_or(0);
        let engn0_runlist = (engn0 >> 12) & 0xF;
        eprintln!("║ ENGN0_STATUS={engn0:#010x} → GR runlist={engn0_runlist}");
        if gr_runlist.is_none() && engn0_runlist <= 31 {
            gr_runlist = Some(engn0_runlist);
        }
        let target_runlist = gr_runlist.unwrap_or_else(|| pbdma_runlists.first().map_or(0, |e| e.1));
        eprintln!("║ Final target runlist: {target_runlist}");

        let runq: u32 = 0;

        tracing::info!(
            target_runlist,
            runq,
            "PFIFO engine initialized"
        );

        Ok((runq, target_runlist))
    }

    /// Read back PFIFO/PBDMA/PCCSR state for diagnostics.
    fn log_pfifo_diagnostics(bar0: &MappedBar) {
        let r = |reg: usize| bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD);

        let pfifo_intr = r(pfifo::INTR);
        let pfifo_en = r(pfifo::INTR_EN);
        let sched = r(pfifo::SCHED_EN);
        let pccsr_inst = r(pccsr::inst(0));
        let pccsr_chan = r(pccsr::channel(0));
        let pbdma0_intr = r(pbdma::intr(0));
        let pbdma0_hce = r(pbdma::hce_intr(0));
        let pbdma1_intr = r(pbdma::intr(1));

        // Engine status registers: GR=engn0, CE0..CE7
        let engn0_status = r(0x0000_2640);
        // PBDMA idle status
        let pbdma0_idle = r(0x0000_3080);
        let pbdma1_idle = r(0x0000_3084);
        // RUNLIST pending
        let rl0_info = r(0x0000_2284);
        // PMC enable (bit 8 = PFIFO, bit 1 = PBDMA)
        let pmc_enable = r(0x0000_0200);
        // BIND_ERROR status
        let bind_err = r(0x0000_252C);

        // Additional Volta-specific registers
        let sched_dis = r(0x0000_2630); // NV_PFIFO_SCHED_DISABLE
        let preempt = r(0x0000_2634);   // NV_PFIFO_PREEMPT
        let runl_submit_info = r(0x0000_2270); // RUNLIST_BASE readback
        let doorbell_test = r(0x0081_0090); // Doorbell register probe
        let pbdma_map = r(0x0000_2004); // PBDMA active map
        let top_info0 = r(0x0002_2700); // Top device info entry 0

        eprintln!("╔══ PFIFO DIAGNOSTICS ══════════════════════════════════════╗");
        eprintln!("║ PMC_ENABLE:     {pmc_enable:#010x}");
        eprintln!("║ SCHED_EN:       {sched:#010x}");
        eprintln!("║ SCHED_DISABLE:  {sched_dis:#010x}");
        eprintln!("║ PREEMPT:        {preempt:#010x}");
        eprintln!("║ PFIFO_INTR:     {pfifo_intr:#010x}");
        eprintln!("║ PFIFO_INTR_EN:  {pfifo_en:#010x}");
        eprintln!("║ PCCSR_INST[0]:  {pccsr_inst:#010x}");
        eprintln!("║ PCCSR_CHAN[0]:  {pccsr_chan:#010x}  (bit10=en_set, bit0=enabled)");
        eprintln!("║ PBDMA0_INTR:    {pbdma0_intr:#010x}");
        eprintln!("║ PBDMA0_HCE:     {pbdma0_hce:#010x}");
        eprintln!("║ PBDMA1_INTR:    {pbdma1_intr:#010x}");
        eprintln!("║ PBDMA0_IDLE:    {pbdma0_idle:#010x}  (bits[15:13]=busy)");
        eprintln!("║ PBDMA1_IDLE:    {pbdma1_idle:#010x}");
        eprintln!("║ ENGN0_STATUS:   {engn0_status:#010x}");
        eprintln!("║ RUNLIST0_INFO:  {rl0_info:#010x}  (bit20=pending)");
        eprintln!("║ RUNLIST_BASE:   {runl_submit_info:#010x}");
        eprintln!("║ BIND_ERROR:     {bind_err:#010x}");
        eprintln!("║ PBDMA_MAP:      {pbdma_map:#010x}");
        eprintln!("║ TOP_INFO[0]:    {top_info0:#010x}");
        eprintln!("║ DOORBELL_PROBE: {doorbell_test:#010x}");
        // Dump internal state for each PBDMA that actually exists.
        let mut seq = 0_usize;
        for pid in 0..32_usize {
            if pbdma_map & (1 << pid) == 0 {
                continue;
            }
            let b = 0x040000 + pid * 0x2000;
            let rl_assign = r(0x2390 + seq * 4);
            eprintln!("║ ── PBDMA{pid} (seq={seq}, →runlist {rl_assign}) ──");
            eprintln!("║ GP_BASE:  {:#010x}_{:#010x}", r(b + 0x44), r(b + 0x40));
            eprintln!("║ GP_PUT:   {:#010x}  GP_FETCH: {:#010x}  GP_STATE: {:#010x}",
                r(b + 0x54), r(b + 0x48), r(b + 0x4C));
            eprintln!("║ USERD:    {:#010x}_{:#010x}", r(b + 0xD4), r(b + 0xD0));
            eprintln!("║ CHN_INF:  {:#010x}  CHN_STATE: {:#010x}  SIG: {:#010x}",
                r(b + 0xAC), r(b + 0xB0), r(b + 0xC0));
            seq += 1;
        }
        eprintln!("╚═══════════════════════════════════════════════════════════╝");
    }

    // ── Page table setup ───────────────────────────────────────────────

    /// Populate V2 MMU page tables with identity mapping for the first 2 MiB.
    ///
    /// The 5-level hierarchy (PD3→PD2→PD1→PD0→PT) maps GPU virtual addresses
    /// directly to their IOVA equivalents, so GPU VA 0x1000 → physical 0x1000
    /// (which the IOMMU then translates to the actual host physical address).
    fn populate_page_tables(&mut self) {
        // PD3 entry 0 → PD2 (VA[48:47] = 0)
        write_pde(self.pd3.as_mut_slice(), 0, PD2_IOVA);

        // PD2 entry 0 → PD1 (VA[46:38] = 0)
        write_pde(self.pd2.as_mut_slice(), 0, PD1_IOVA);

        // PD1 entry 0 → PD0 (VA[37:29] = 0)
        write_pde(self.pd1.as_mut_slice(), 0, PD0_IOVA);

        // PD0 entry 0: dual PDE format — 16 bytes per entry.
        // Bytes [0:7]  = big page PDE (unused, leave as 0)
        // Bytes [8:15] = small page PDE → PT0
        let pd0_slice = self.pd0.as_mut_slice();
        let small_pde = encode_pde(PT0_IOVA);
        pd0_slice[8..16].copy_from_slice(&small_pde.to_le_bytes());

        // PT0: identity-map 512 small pages (4 KiB each, total 2 MiB).
        // Page 0 left unmapped as a null guard.
        let pt_slice = self.pt0.as_mut_slice();
        for i in 1..PT_ENTRIES {
            let phys = (i as u64) * 4096;
            let pte = encode_pte(phys);
            let off = i * 8;
            pt_slice[off..off + 8].copy_from_slice(&pte.to_le_bytes());
        }
    }

    // ── Instance block / RAMFC ─────────────────────────────────────────

    /// Populate RAMFC and page directory base within the instance block.
    ///
    /// Field values match `gv100_chan_ramfc_write()` from nouveau with
    /// `priv=true` and `devm=0xFFF`, adapted for system memory aperture.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "IOVA values and ilog2 results always fit u32"
    )]
    fn populate_instance_block(&mut self, gpfifo_iova: u64, gpfifo_entries: u32, userd_iova: u64) {
        let inst = self.instance.as_mut_slice();
        let limit2 = gpfifo_entries.ilog2();

        // ── RAMFC fields (offsets 0x000..0x1FF) ────────────────────────

        // USERD pointer with SYS_MEM_COHERENT target in low bits.
        // NV_PPBDMA_USERD: ADDR[31:9], TARGET[1:0].
        // PBDMA encoding: 0=VID_MEM, 1=SYS_MEM_COH, 2=SYS_MEM_NCOH
        // (differs from PCCSR where 2=COH!)
        write_u32_le(
            inst,
            ramfc::USERD_LO,
            (userd_iova as u32 & 0xFFFF_FE00) | PBDMA_TARGET_SYS_MEM_COHERENT,
        );
        write_u32_le(inst, ramfc::USERD_HI, (userd_iova >> 32) as u32);

        write_u32_le(inst, ramfc::SIGNATURE, 0x0000_FACE);
        write_u32_le(inst, ramfc::ACQUIRE, 0x7FFF_F902);

        // GPFIFO base (GPU virtual address — our identity map makes VA = IOVA).
        // No target/aperture bits: the PBDMA accesses GPFIFO through the GPU MMU,
        // which uses the page tables in this instance block to translate.
        write_u32_le(inst, ramfc::GP_BASE_LO, gpfifo_iova as u32);
        write_u32_le(
            inst,
            ramfc::GP_BASE_HI,
            (gpfifo_iova >> 32) as u32 | (limit2 << 16),
        );

        write_u32_le(inst, ramfc::PB_HEADER, 0x2040_0000);
        write_u32_le(inst, ramfc::SUBDEVICE, 0x3000_0000 | 0xFFF);
        write_u32_le(inst, ramfc::HCE_CTRL, 0x0000_0020);
        write_u32_le(inst, ramfc::CHID, self.channel_id);
        write_u32_le(inst, ramfc::CONFIG, 0x0000_1100);
        write_u32_le(inst, ramfc::CHANNEL_INFO, 0x1000_3080);

        // ── NV_RAMIN page directory base (offset 0x200) ────────────────

        let pdb_lo: u32 = ((PD3_IOVA >> 12) as u32) << 12
            | (1 << 11) // BIG_PAGE_SIZE = 64 KiB
            | (1 << 10) // USE_VER2_PT_FORMAT = TRUE
            | (1 << 2)  // VOL = TRUE
            | TARGET_SYS_MEM_COHERENT;
        write_u32_le(inst, ramin::PAGE_DIR_BASE_LO, pdb_lo);
        write_u32_le(inst, ramin::PAGE_DIR_BASE_HI, (PD3_IOVA >> 32) as u32);

        // Clear engine WFI VEID.
        write_u32_le(inst, ramin::ENGINE_WFI_VEID, 0);

        // ── Subcontext 0 page directory (mirrors main PDB) ────────────
        // FECS uses subcontexts for compute; at least SC0 must be valid.

        write_u32_le(inst, ramin::SC_PDB_VALID, 1); // SC0 valid
        write_u32_le(inst, ramin::SC0_PAGE_DIR_BASE_LO, pdb_lo);
        write_u32_le(inst, ramin::SC0_PAGE_DIR_BASE_HI, (PD3_IOVA >> 32) as u32);
    }

    // ── Runlist ────────────────────────────────────────────────────────

    /// Populate runlist with a TSG header + channel entry (Volta RAMRL format).
    ///
    /// The runlist contains one channel group (TSG) with one channel.
    /// Format from `gv100_runl_insert_cgrp()` and `gv100_runl_insert_chan()`.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "IOVA values always fit u32 for our allocation range"
    )]
    fn populate_runlist(&mut self, userd_iova: u64, runq: u32) {
        let rl = self.runlist.as_mut_slice();

        // ── TSG (channel group) header — 16 bytes ──────────────────────
        // DW0: (timeslice=128 << 24) | (tsg_length=3 << 16) | type=1
        write_u32_le(rl, 0x00, (128 << 24) | (3 << 16) | 1);
        write_u32_le(rl, 0x04, 1); // 1 channel in group
        write_u32_le(rl, 0x08, 0); // group ID = 0
        write_u32_le(rl, 0x0C, 0);

        // ── Channel entry — 16 bytes (gv100_runl_insert_chan) ────────────
        //
        // GV100 RAMRL channel entry format (from dev_ram.ref.txt):
        //   DW0: USERD_PTR_LO[31:8] | USERD_TARGET[7:6] | INST_TARGET[5:4] | RQ[1] | TYPE[0]=0
        //   DW1: USERD_PTR_HI[31:0]
        //   DW2: INST_PTR_LO[31:12] | CHID[11:0]
        //   DW3: INST_PTR_HI[31:0]
        //
        // Note: GV100 hardware ignores the RAMRL INST fields and uses PCCSR
        // INST_PTR instead. USERD fields may also be ignored (read from RAMFC).
        // TARGET encoding: 0=VID_MEM, 2=SYS_MEM_COH, 3=SYS_MEM_NCOH
        write_u32_le(
            rl,
            0x10,
            (userd_iova as u32 & 0xFFFF_FF00)
                | (TARGET_SYS_MEM_COHERENT << 6)
                | (TARGET_SYS_MEM_NONCOHERENT << 4)
                | (runq << 1),
        );
        write_u32_le(rl, 0x14, (userd_iova >> 32) as u32);
        write_u32_le(
            rl,
            0x18,
            (INSTANCE_IOVA as u32 & 0xFFFF_F000) | self.channel_id,
        );
        write_u32_le(rl, 0x1C, (INSTANCE_IOVA >> 32) as u32);
    }

    // ── BAR0 register programming ─────────────────────────────────────

    /// Bind the channel's instance block to PCCSR.
    ///
    /// Matches nouveau's `gk104_chan_bind_inst()`: write INST_PTR and
    /// INST_TARGET to PCCSR_INST. On GV100 with VFIO, the instance block
    /// is in system memory — we set INST_TARGET to SYS_MEM_NONCOHERENT
    /// (matching the runlist entry DW2 encoding). INST_BIND is NOT set;
    /// context load happens via the scheduler + runlist path.
    fn bind_channel(&self, bar0: &MappedBar) -> DriverResult<()> {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "INSTANCE_IOVA >> 12 fits u32 for our allocation range"
        )]
        let value = (INSTANCE_IOVA >> 12) as u32
            | pccsr::INST_TARGET_SYS_MEM_NCOH;

        bar0.write_u32(pccsr::inst(self.channel_id), value)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("PCCSR bind: {e}"))))
    }

    /// Clear stale PBDMA_FAULTED / ENG_FAULTED flags, then disable-before-re-enable
    /// to put the channel in a clean state for a new runlist submission.
    fn clear_channel_faults(&self, bar0: &MappedBar) -> DriverResult<()> {
        let ch = pccsr::channel(self.channel_id);
        let pre = bar0.read_u32(ch).unwrap_or(0);
        if pre & (pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET) != 0 {
            // Disable channel first.
            bar0.write_u32(ch, pccsr::CHANNEL_ENABLE_CLR)
                .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("chan disable: {e}"))))?;
            std::thread::sleep(std::time::Duration::from_millis(2));

            // Clear fault bits by writing 1 to them.
            bar0.write_u32(ch, pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET)
                .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("fault clear: {e}"))))?;
            std::thread::sleep(std::time::Duration::from_millis(2));

            let post = bar0.read_u32(ch).unwrap_or(0xDEAD);
            eprintln!("║ Cleared channel faults: {pre:#010x} → {post:#010x}");
        }
        Ok(())
    }

    /// Enable the channel via PCCSR ENABLE_SET trigger.
    fn enable_channel(&self, bar0: &MappedBar) -> DriverResult<()> {
        bar0.write_u32(pccsr::channel(self.channel_id), pccsr::CHANNEL_ENABLE_SET)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("channel enable: {e}"))))
    }

    /// Submit runlist to PFIFO using the GK104 register format.
    ///
    /// GK104 uses a SINGLE pair of registers (0x2270, 0x2274) for ALL runlists:
    /// - 0x2270: (target << 28) | (addr >> 12)
    /// - 0x2274: (runlist_id << 20) | count
    fn submit_runlist(&self, bar0: &MappedBar) -> DriverResult<()> {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "RUNLIST_IOVA >> 12 fits u32 for our allocation range"
        )]
        // target=3 (NCOH) for system memory, addr is IOVA >> 12.
        let rl_base = (3_u32 << 28) | (RUNLIST_IOVA >> 12) as u32;
        // runlist_id in bits [23:20], count=2 (TSG header + channel entry).
        let rl_submit = (self.runlist_id << 20) | 2;

        eprintln!("║ submit_runlist: id={} base={rl_base:#010x} submit={rl_submit:#010x}",
            self.runlist_id);

        bar0.write_u32(0x0000_2270, rl_base)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("runlist base: {e}"))))?;

        bar0.write_u32(0x0000_2274, rl_submit)
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

// ── Diagnostic experiment matrix ────────────────────────────────────────

/// Populate page tables in pre-allocated buffers (static version for matrix).
fn populate_page_tables_static(
    pd3: &mut [u8], pd2: &mut [u8], pd1: &mut [u8], pd0: &mut [u8], pt0: &mut [u8],
) {
    write_pde(pd3, 0, PD2_IOVA);
    write_pde(pd2, 0, PD1_IOVA);
    write_pde(pd1, 0, PD0_IOVA);
    let small_pde = encode_pde(PT0_IOVA);
    pd0[8..16].copy_from_slice(&small_pde.to_le_bytes());
    for i in 1..PT_ENTRIES {
        let phys = (i as u64) * 4096;
        let pte = encode_pte(phys);
        let off = i * 8;
        pt0[off..off + 8].copy_from_slice(&pte.to_le_bytes());
    }
}

/// Populate instance block in a pre-allocated buffer (static version for matrix).
#[expect(clippy::cast_possible_truncation)]
fn populate_instance_block_static(
    inst: &mut [u8],
    gpfifo_iova: u64, gpfifo_entries: u32, userd_iova: u64, channel_id: u32,
) {
    let limit2 = gpfifo_entries.ilog2();

    write_u32_le(inst, ramfc::USERD_LO,
        (userd_iova as u32 & 0xFFFF_FE00) | PBDMA_TARGET_SYS_MEM_COHERENT);
    write_u32_le(inst, ramfc::USERD_HI, (userd_iova >> 32) as u32);
    write_u32_le(inst, ramfc::SIGNATURE, 0x0000_FACE);
    write_u32_le(inst, ramfc::ACQUIRE, 0x7FFF_F902);
    write_u32_le(inst, ramfc::GP_BASE_LO, gpfifo_iova as u32);
    // GP_BASE is a GPU virtual address (goes through instance block's page tables).
    // No target/aperture bits — only upper address + limit2 (log2 entries).
    write_u32_le(inst, ramfc::GP_BASE_HI,
        (gpfifo_iova >> 32) as u32 | (limit2 << 16));
    write_u32_le(inst, ramfc::PB_HEADER, 0x2040_0000);
    write_u32_le(inst, ramfc::SUBDEVICE, 0x3000_0000 | 0xFFF);
    write_u32_le(inst, ramfc::HCE_CTRL, 0x0000_0020);
    write_u32_le(inst, ramfc::CHID, channel_id);
    write_u32_le(inst, ramfc::CONFIG, 0x0000_1100);
    write_u32_le(inst, ramfc::CHANNEL_INFO, 0x1000_3080);

    let pdb_lo: u32 = ((PD3_IOVA >> 12) as u32) << 12
        | (1 << 11)
        | (1 << 10)
        | (1 << 2)
        | TARGET_SYS_MEM_COHERENT;
    write_u32_le(inst, ramin::PAGE_DIR_BASE_LO, pdb_lo);
    write_u32_le(inst, ramin::PAGE_DIR_BASE_HI, (PD3_IOVA >> 32) as u32);
    write_u32_le(inst, ramin::ENGINE_WFI_VEID, 0);
    write_u32_le(inst, ramin::SC_PDB_VALID, 1);
    write_u32_le(inst, ramin::SC0_PAGE_DIR_BASE_LO, pdb_lo);
    write_u32_le(inst, ramin::SC0_PAGE_DIR_BASE_HI, (PD3_IOVA >> 32) as u32);
}

/// Populate runlist in a pre-allocated buffer (static version for matrix).
#[expect(clippy::cast_possible_truncation)]
fn populate_runlist_static(
    rl: &mut [u8],
    userd_iova: u64, channel_id: u32,
    userd_target: u32, inst_target: u32,
    runq: u32,
) {
    write_u32_le(rl, 0x00, (128 << 24) | (3 << 16) | 1);
    write_u32_le(rl, 0x04, 1);
    write_u32_le(rl, 0x08, 0);
    write_u32_le(rl, 0x0C, 0);
    // DW0: USERD_PTR_LO[31:8] | USERD_TARGET[7:6] | INST_TARGET[5:4] | RUNQUEUE[1] | TYPE=0
    write_u32_le(rl, 0x10,
        (userd_iova as u32 & 0xFFFF_FF00)
        | (userd_target << 6)
        | (inst_target << 4)
        | (runq << 1));
    write_u32_le(rl, 0x14, (userd_iova >> 32) as u32);
    // DW2: INST_PTR_LO[31:12] | CHID[11:0]
    write_u32_le(rl, 0x18,
        (INSTANCE_IOVA as u32 & 0xFFFF_F000) | channel_id);
    write_u32_le(rl, 0x1C, (INSTANCE_IOVA >> 32) as u32);
}

/// Operation ordering for the diagnostic experiment.
#[derive(Debug, Clone, Copy)]
pub enum ExperimentOrdering {
    /// A: bind → enable → runlist (current production path)
    BindEnableRunlist,
    /// B: bind → runlist → enable
    BindRunlistEnable,
    /// C: runlist → bind → enable
    RunlistBindEnable,
    /// D: bind_with_INST_BIND → enable → runlist (force immediate context load)
    BindWithInstBindEnableRunlist,
    /// E: Direct PBDMA register programming — bypass scheduler entirely.
    /// Writes GP_BASE, USERD, SIGNATURE, etc. directly to PBDMA MMIO registers
    /// instead of submitting a runlist and waiting for the scheduler.
    DirectPbdmaProgramming,
    /// F: Direct PBDMA + PCCSR bind (with INST_BIND) — combine both paths.
    DirectPbdmaWithInstBind,
    /// G: Direct PBDMA + activate: reset GP_FETCH, write GPFIFO entry,
    /// set USERD GP_PUT=1, set PBDMA GP_PUT=1. Tests if PBDMA processes work.
    DirectPbdmaActivate,
    /// H: G + doorbell notification via NV_USERMODE_NOTIFY_CHANNEL_PENDING.
    DirectPbdmaActivateDoorbell,
    /// I: G + write PCCSR scheduled bit directly (bit 1).
    DirectPbdmaActivateScheduled,
    /// J: Instance block written to VRAM via PRAMIN, normal runlist submit + doorbell.
    VramInstanceBind,
    /// K: ALL structures in VRAM via PRAMIN — instance block, runlist, GPFIFO,
    /// USERD, page tables, push buffer. Eliminates ALL system memory access.
    AllVram,
    /// L: Hybrid — VRAM structures + direct PBDMA programming (skip INST_BIND).
    /// Combines proven direct-PBDMA-write approach with all-VRAM data.
    AllVramDirectPbdma,
    /// M: PFIFO engine reset + re-init — replicate nouveau's gk104_fifo_init() via
    /// PMC toggle (disable/enable bit 8), then normal scheduler path. Tests whether
    /// a fresh PFIFO reset restores scheduler operation.
    PfifoResetInit,
    /// N: INST_BIND + scheduled + GPFIFO work + doorbell — the full dispatch path.
    /// Combines D's working INST_BIND (which achieves SCHEDULED on warm GPU) with
    /// actual GPFIFO entry + USERD GP_PUT + doorbell notification. Tests whether
    /// the PBDMA processes work once properly scheduled.
    FullDispatchWithInstBind,
    /// O: N + PREEMPT to force context switch into PBDMA.
    /// Nouveau reference shows PREEMPT=0x01000001. The scheduler may need an
    /// explicit preempt to load the newly-scheduled channel into the PBDMA.
    FullDispatchWithPreempt,
    /// P: INST_BIND(SCHEDULED) + direct PBDMA writes + doorbell.
    /// Combines scheduler path (D=SCHEDULED) with manual PBDMA register
    /// injection from the RAMFC, then rings doorbell. If the scheduler can't
    /// load context itself, we'll do it manually.
    ScheduledPlusDirectPbdma,
    /// Q: Instance block in VRAM (via PRAMIN) + full dispatch path.
    /// Nouveau uses VRAM for instance blocks exclusively. On Volta, INST_BIND
    /// for system memory targets faults (0x11000000), and the PBDMA never loads
    /// context from RAMFC. This experiment writes the instance block to VRAM
    /// via the PRAMIN window, uses VRAM target in PCCSR_INST and runlist entry,
    /// then follows the full D+N dispatch path with doorbell.
    VramFullDispatch,
}

/// Configuration for a single experiment in the diagnostic matrix.
#[derive(Debug, Clone)]
pub struct ExperimentConfig {
    /// Human-readable experiment name.
    pub name: &'static str,
    /// PCCSR INST_TARGET: 2=COH, 3=NCOH
    pub pccsr_target: u32,
    /// Runlist channel entry DW0 USERD_TARGET: 2=COH, 3=NCOH
    pub runlist_userd_target: u32,
    /// Runlist channel entry DW2 INST_TARGET: 2=COH, 3=NCOH
    pub runlist_inst_target: u32,
    /// Runlist base register (0x2270) target: 2=COH, 3=NCOH
    pub runlist_base_target: u32,
    /// Operation ordering.
    pub ordering: ExperimentOrdering,
    /// Whether to skip the PFIFO_ENABLE toggle (leave as-is from nouveau).
    pub skip_pfifo_toggle: bool,
}

/// Result snapshot from a single experiment.
#[derive(Debug)]
pub struct ExperimentResult {
    /// Experiment name.
    pub name: String,
    /// PCCSR channel register value.
    pub pccsr_chan: u32,
    /// PCCSR inst register readback.
    pub pccsr_inst_readback: u32,
    /// PBDMA USERD low word.
    pub pbdma_userd_lo: u32,
    /// PBDMA USERD high word.
    pub pbdma_userd_hi: u32,
    /// PBDMA GP_BASE low word.
    pub pbdma_gp_base_lo: u32,
    /// PBDMA GP_BASE high word.
    pub pbdma_gp_base_hi: u32,
    /// PBDMA GP_PUT register.
    pub pbdma_gp_put: u32,
    /// PBDMA GP_FETCH register — if this changes after we write, the PBDMA is alive.
    pub pbdma_gp_fetch: u32,
    /// PBDMA CHANNEL_STATE register.
    pub pbdma_channel_state: u32,
    /// PBDMA SIGNATURE register.
    pub pbdma_signature: u32,
    /// PFIFO interrupt status.
    pub pfifo_intr: u32,
    /// MMU fault status register.
    pub mmu_fault_status: u32,
    /// ENGN0 status register.
    pub engn0_status: u32,
    /// Whether PBDMA_FAULTED or ENG_FAULTED is set.
    pub faulted: bool,
    /// Whether PCCSR bit 1 (NEXT/scheduled) is set.
    pub scheduled: bool,
    /// Whether PBDMA registers changed from residual state (i.e. our writes stuck).
    pub pbdma_ours: bool,
}

impl ExperimentResult {
    /// Single-line summary for the experiment table.
    pub fn summary_line(&self) -> String {
        let pbdma_tag = if self.pbdma_ours { "OUR" } else { "old" };
        format!(
            "{:<42} | {:08x} | {:<5} | {:<5} | {:08x}_{:08x} | {:>3} | gp={:02x}/{:02x} | {:08x}",
            self.name,
            self.pccsr_chan,
            if self.faulted { "FAULT" } else { "ok" },
            if self.scheduled { "SCHED" } else { "no" },
            self.pbdma_userd_hi,
            self.pbdma_userd_lo,
            pbdma_tag,
            self.pbdma_gp_put,
            self.pbdma_gp_fetch,
            self.engn0_status,
        )
    }
}

/// Build the full experiment configuration matrix.
///
/// Generates scheduler-based experiments (A-D × encoding axes) plus
/// direct PBDMA programming experiments (E, F).
pub fn build_experiment_matrix() -> Vec<ExperimentConfig> {
    let mut configs = Vec::new();

    // ── Scheduler-based orderings (A-D) with encoding axes ──────────────

    let orderings = [
        (ExperimentOrdering::BindEnableRunlist, "A"),
        (ExperimentOrdering::BindRunlistEnable, "B"),
        (ExperimentOrdering::RunlistBindEnable, "C"),
        (ExperimentOrdering::BindWithInstBindEnableRunlist, "D"),
    ];

    let pccsr_targets = [(2_u32, "coh"), (3_u32, "ncoh")];

    let runlist_targets = [
        (2_u32, 3_u32, "Ucoh_Incoh"),
        (2_u32, 2_u32, "Ucoh_Icoh"),
        (3_u32, 3_u32, "Uncoh_Incoh"),
    ];

    let runlist_base_targets = [(3_u32, "rlNcoh"), (2_u32, "rlCoh")];

    for &(ordering, ord_name) in &orderings {
        for &(pccsr_t, pccsr_name) in &pccsr_targets {
            for &(userd_t, inst_t, rl_name) in &runlist_targets {
                for &(rl_base_t, rl_base_name) in &runlist_base_targets {
                    let name = Box::leak(
                        format!("{ord_name}_{pccsr_name}_{rl_name}_{rl_base_name}")
                            .into_boxed_str(),
                    );
                    configs.push(ExperimentConfig {
                        name,
                        pccsr_target: pccsr_t,
                        runlist_userd_target: userd_t,
                        runlist_inst_target: inst_t,
                        runlist_base_target: rl_base_t,
                        ordering,
                        skip_pfifo_toggle: true,
                    });
                }
            }
        }
    }

    // ── Q: VRAM instance block + full dispatch — run FIRST on warm GPU ──
    // Hypothesis: Volta PFIFO requires instance blocks in VRAM (like nouveau).
    // INST_BIND for system memory faults; PBDMA never loads RAMFC context.
    // PRAMIN writes to low VRAM offsets are non-destructive to warm state.
    for &(rl_utgt, rl_btgt, suffix) in &[
        (2_u32, 3_u32, "Ucoh"),
        (2,     2,     "Ucoh_rlCoh"),
        (3,     3,     "Uncoh"),
    ] {
        configs.push(ExperimentConfig {
            name: match suffix {
                "Ucoh" => "Q_vramInst_Ucoh",
                "Ucoh_rlCoh" => "Q_vramInst_Ucoh_rlCoh",
                _ => "Q_vramInst_Uncoh",
            },
            pccsr_target: 0, // VRAM
            runlist_userd_target: rl_utgt,
            runlist_inst_target: 0, // VRAM
            runlist_base_target: rl_btgt,
            ordering: ExperimentOrdering::VramFullDispatch,
            skip_pfifo_toggle: true,
        });
    }

    // ── N: Full dispatch (INST_BIND + GPFIFO + doorbell) — run early ────
    // Must run before VRAM/PRAMIN experiments (J/K/L) which can corrupt state.
    for &(pccsr_tgt, rl_utgt, rl_itgt, rl_btgt, suffix) in &[
        (2_u32, 2_u32, 3_u32, 3_u32, "coh"),
        (3,     2,     3,     2,     "ncoh"),
        (2,     2,     2,     2,     "allCoh"),
    ] {
        configs.push(ExperimentConfig {
            name: match suffix {
                "coh" => "N_fullDispatch_coh",
                "ncoh" => "N_fullDispatch_ncoh",
                _ => "N_fullDispatch_allCoh",
            },
            pccsr_target: pccsr_tgt,
            runlist_userd_target: rl_utgt,
            runlist_inst_target: rl_itgt,
            runlist_base_target: rl_btgt,
            ordering: ExperimentOrdering::FullDispatchWithInstBind,
            skip_pfifo_toggle: true,
        });
    }

    // ── O: Full dispatch + PREEMPT — force context switch ────────────────
    configs.push(ExperimentConfig {
        name: "O_dispatch_preempt_coh",
        pccsr_target: 2,
        runlist_userd_target: 2,
        runlist_inst_target: 3,
        runlist_base_target: 3,
        ordering: ExperimentOrdering::FullDispatchWithPreempt,
        skip_pfifo_toggle: true,
    });
    configs.push(ExperimentConfig {
        name: "O_dispatch_preempt_ncoh",
        pccsr_target: 3,
        runlist_userd_target: 2,
        runlist_inst_target: 3,
        runlist_base_target: 2,
        ordering: ExperimentOrdering::FullDispatchWithPreempt,
        skip_pfifo_toggle: true,
    });

    // ── P: Scheduled + direct PBDMA inject + doorbell ────────────────────
    configs.push(ExperimentConfig {
        name: "P_sched_directPbdma_coh",
        pccsr_target: 2,
        runlist_userd_target: 2,
        runlist_inst_target: 3,
        runlist_base_target: 3,
        ordering: ExperimentOrdering::ScheduledPlusDirectPbdma,
        skip_pfifo_toggle: true,
    });

    // ── Direct PBDMA experiments (E, F) — register write test ─────────

    for &(pccsr_t, pccsr_name) in &[(3_u32, "ncoh"), (2_u32, "coh")] {
        configs.push(ExperimentConfig {
            name: Box::leak(format!("E_direct_{pccsr_name}_noInstBind").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 3,
            ordering: ExperimentOrdering::DirectPbdmaProgramming,
            skip_pfifo_toggle: true,
        });
        configs.push(ExperimentConfig {
            name: Box::leak(format!("F_direct_{pccsr_name}_instBind").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 3,
            ordering: ExperimentOrdering::DirectPbdmaWithInstBind,
            skip_pfifo_toggle: true,
        });
    }

    // ── Direct PBDMA activation experiments (G, H, I) ───────────────────

    for &(pccsr_t, pccsr_name) in &[(3_u32, "ncoh"), (2_u32, "coh")] {
        configs.push(ExperimentConfig {
            name: Box::leak(format!("G_activate_{pccsr_name}").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 3,
            ordering: ExperimentOrdering::DirectPbdmaActivate,
            skip_pfifo_toggle: true,
        });
        configs.push(ExperimentConfig {
            name: Box::leak(format!("H_activate_doorbell_{pccsr_name}").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 3,
            ordering: ExperimentOrdering::DirectPbdmaActivateDoorbell,
            skip_pfifo_toggle: true,
        });
        configs.push(ExperimentConfig {
            name: Box::leak(format!("I_activate_sched_{pccsr_name}").into_boxed_str()),
            pccsr_target: pccsr_t,
            runlist_userd_target: 2,
            runlist_inst_target: 3,
            runlist_base_target: 3,
            ordering: ExperimentOrdering::DirectPbdmaActivateScheduled,
            skip_pfifo_toggle: true,
        });
    }

    // ── VRAM instance block experiments (J) ─────────────────────────────

    for &(rl_base_t, rl_name) in &[(3_u32, "rlNcoh"), (2_u32, "rlCoh")] {
        configs.push(ExperimentConfig {
            name: Box::leak(format!("J_vramInst_{rl_name}").into_boxed_str()),
            pccsr_target: 0,
            runlist_userd_target: 2,
            runlist_inst_target: 0,
            runlist_base_target: rl_base_t,
            ordering: ExperimentOrdering::VramInstanceBind,
            skip_pfifo_toggle: true,
        });
    }

    // ── ALL-VRAM experiment (K) — definitive scheduler test ─────────────

    configs.push(ExperimentConfig {
        name: "K_allVram",
        pccsr_target: 0,
        runlist_userd_target: 0,
        runlist_inst_target: 0,
        runlist_base_target: 0,
        ordering: ExperimentOrdering::AllVram,
        skip_pfifo_toggle: true,
    });

    // ── Hybrid VRAM + direct PBDMA (L) ──────────────────────────────────

    configs.push(ExperimentConfig {
        name: "L_vramDirectPbdma",
        pccsr_target: 0,
        runlist_userd_target: 0,
        runlist_inst_target: 0,
        runlist_base_target: 0,
        ordering: ExperimentOrdering::AllVramDirectPbdma,
        skip_pfifo_toggle: true,
    });

    // ── M: PFIFO engine reset + re-init ──────────────────────────────────
    for &(pccsr_tgt, rl_utgt, rl_itgt, rl_btgt, suffix) in &[
        (2_u32, 2_u32, 3_u32, 3_u32, "coh_Ucoh_Incoh_rlNcoh"),
        (3,     2,     3,     2,     "ncoh_Ucoh_Incoh_rlCoh"),
    ] {
        configs.push(ExperimentConfig {
            name: match suffix {
                "coh_Ucoh_Incoh_rlNcoh" => "M_pfifoReset_coh",
                _ => "M_pfifoReset_ncoh",
            },
            pccsr_target: pccsr_tgt,
            runlist_userd_target: rl_utgt,
            runlist_inst_target: rl_itgt,
            runlist_base_target: rl_btgt,
            ordering: ExperimentOrdering::PfifoResetInit,
            skip_pfifo_toggle: true,
        });
    }

    configs
}

/// Run the full diagnostic experiment matrix.
///
/// Allocates shared DMA buffers, runs PFIFO engine init ONCE, then iterates
/// over all configurations, capturing register snapshots for each.
///
/// The GPU should be warm from nouveau (bind nouveau → unbind → bind vfio-pci)
/// so the PFIFO scheduler is already running.
#[expect(clippy::cast_possible_truncation, clippy::too_many_lines)]
pub fn diagnostic_matrix(
    container_fd: RawFd,
    bar0: &MappedBar,
    gpfifo_iova: u64,
    gpfifo_entries: u32,
    userd_iova: u64,
    channel_id: u32,
    configs: &[ExperimentConfig],
    gpfifo_ring: &mut [u8],
    userd_page: &mut [u8],
) -> DriverResult<Vec<ExperimentResult>> {
    let mut instance = DmaBuffer::new(container_fd, 4096, INSTANCE_IOVA)?;
    let mut runlist = DmaBuffer::new(container_fd, 4096, RUNLIST_IOVA)?;
    let mut pd3 = DmaBuffer::new(container_fd, 4096, PD3_IOVA)?;
    let mut pd2 = DmaBuffer::new(container_fd, 4096, PD2_IOVA)?;
    let mut pd1 = DmaBuffer::new(container_fd, 4096, PD1_IOVA)?;
    let mut pd0 = DmaBuffer::new(container_fd, 4096, PD0_IOVA)?;
    let mut pt0 = DmaBuffer::new(container_fd, 4096, PT0_IOVA)?;

    let w = |reg: usize, val: u32| -> DriverResult<()> {
        bar0.write_u32(reg, val)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("diag {reg:#x}: {e}"))))
    };
    let r = |reg: usize| bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD);

    // ── One-shot probes ─────────────────────────────────────────────────

    eprintln!("╔══ DIAGNOSTIC MATRIX — ONE-SHOT PROBES ═══════════════════╗");
    eprintln!("║ BOOT0:         {:#010x}", r(0));
    eprintln!("║ PMC_ENABLE:    {:#010x}", r(pmc::ENABLE));
    eprintln!("║ PFIFO_ENABLE:  {:#010x}", r(pfifo::ENABLE));
    eprintln!("║ SCHED_DISABLE: {:#010x}", r(0x2630));
    eprintln!("║ PFIFO_INTR:    {:#010x}", r(pfifo::INTR));
    eprintln!("║ PBDMA_MAP:     {:#010x}", r(pfifo::PBDMA_MAP));
    eprintln!("║ ENGN0_STATUS:  {:#010x}", r(0x2640));
    eprintln!("║ BIND_ERROR:    {:#010x}", r(0x252C));
    eprintln!("║ FB_TIMEOUT:    {:#010x}", r(0x2254));
    eprintln!("║ PRIV_RING:     {:#010x}", r(0x012070));
    eprintln!("║ ── MMU Fault Buffers ──");
    eprintln!("║ BUF0_LO:  {:#010x}  BUF0_HI:  {:#010x}  SIZE: {:#010x}",
        r(0x100E24), r(0x100E28), r(0x100E2C));
    eprintln!("║ BUF0_GET: {:#010x}  BUF0_PUT: {:#010x}",
        r(0x100E30), r(0x100E34));
    eprintln!("║ BUF1_LO:  {:#010x}  BUF1_HI:  {:#010x}  SIZE: {:#010x}",
        r(0x100E44), r(0x100E48), r(0x100E4C));
    eprintln!("║ BUF1_GET: {:#010x}  BUF1_PUT: {:#010x}",
        r(0x100E50), r(0x100E54));
    eprintln!("║ ── PCCSR Channel Scan ──");
    for ch in 0..8_u32 {
        let inst_val = r(pccsr::inst(ch));
        let chan_val = r(pccsr::channel(ch));
        if inst_val != 0 || chan_val != 0 {
            eprintln!("║ CH{ch}: INST={inst_val:#010x} CHAN={chan_val:#010x}");
        }
    }
    eprintln!("║ MMU_FAULT_STATUS: {:#010x}", r(0x100A2C));
    eprintln!("║ MMU_FAULT_ADDR:   {:#010x}_{:#010x}", r(0x100A34), r(0x100A30));
    eprintln!("║ MMU_FAULT_INST:   {:#010x}_{:#010x}", r(0x100A3C), r(0x100A38));

    // ── Warm state verification ──
    let pmc_en = r(pmc::ENABLE);
    let pfifo_en = r(pfifo::ENABLE);
    let gpu_warm = pmc_en != 0x4000_0020 && pfifo_en != 0xBAD0_DA00;
    eprintln!("║ WARM STATE:       {}", if gpu_warm { "WARM ✓" } else { "COLD ✗ — PFIFO de-clocked, results unreliable" });
    eprintln!("╚═══════════════════════════════════════════════════════════╝");

    if !gpu_warm {
        eprintln!("╔══ WARNING: GPU IS COLD ═════════════════════════════════════╗");
        eprintln!("║ PMC_ENABLE={pmc_en:#010x} (expect 0x5fecdff1 when warm)");
        eprintln!("║ PFIFO_ENABLE={pfifo_en:#010x} (expect 0x00000000, not 0xbad0da00)");
        eprintln!("║ Nouveau warm-boot may have failed. Re-run rebind sequence.");
        eprintln!("║ Continuing anyway — some experiments still yield useful data.");
        eprintln!("╚═════════════════════════════════════════════════════════════╝");
    }

    // ── Shared init ─────────────────────────────────────────────────────
    // DO NOT write PMC_ENABLE — on a warm GPU, nouveau left the correct
    // value (0x5fecdff1). Writing 0xFFFFFFFF can break engine clocking.

    let pbdma_map = r(pfifo::PBDMA_MAP);
    if pbdma_map == 0 || pbdma_map == 0xBAD0_DA00 {
        return Err(DriverError::SubmitFailed(Cow::Borrowed(
            "no PBDMAs (PFIFO de-clocked — GPU cold, need nouveau warm)"
        )));
    }

    let mut gr_runlist: Option<u32> = None;
    let mut cur_type: u32 = 0xFFFF;
    let mut cur_runlist: u32 = 0xFFFF;
    for i in 0..64_u32 {
        let data = r(0x0002_2700 + (i as usize) * 4);
        if data == 0 { break; }
        let kind = data & 3;
        match kind {
            1 => cur_type = (data >> 2) & 0x3F,
            3 => cur_runlist = (data >> 11) & 0x1F,
            _ => {}
        }
        if data & (1 << 31) != 0 {
            if cur_type == 0 && gr_runlist.is_none() && cur_runlist != 0xFFFF {
                gr_runlist = Some(cur_runlist);
            }
            cur_type = 0xFFFF;
            cur_runlist = 0xFFFF;
        }
    }
    if gr_runlist.is_none() {
        let engn0 = r(0x2640);
        let rl = (engn0 >> 12) & 0xF;
        if rl <= 31 { gr_runlist = Some(rl); }
    }
    let target_runlist = gr_runlist.unwrap_or(0);
    eprintln!("║ Target runlist: {target_runlist}");

    // Dump ALL PBDMA → runlist mappings and engine info
    {
        let mut seq = 0_usize;
        for pid in 0..32_usize {
            if pbdma_map & (1 << pid) == 0 { continue; }
            let rl = r(0x2390 + seq * 4);
            eprintln!("║ PBDMA_RUNL_MAP[{seq}]: PBDMA {pid} → runlist {rl}");
            seq += 1;
        }
        // Also dump engine table at 0x22700
        eprintln!("║ ── Engine Table (0x22700) ──");
        let mut cur_type: u32 = 0xFFFF;
        let mut cur_rl: u32 = 0xFFFF;
        for i in 0..32_u32 {
            let data = r(0x2_2700 + (i as usize) * 4);
            if data == 0 { break; }
            let kind = data & 3;
            match kind {
                1 => cur_type = (data >> 2) & 0x3F,
                3 => cur_rl = (data >> 11) & 0x1F,
                _ => {}
            }
            if data & (1 << 31) != 0 {
                eprintln!("║   ENGN_TABLE[{i}]: {data:#010x} — type={cur_type} runlist={cur_rl} (FINAL)");
            } else {
                eprintln!("║   ENGN_TABLE[{i}]: {data:#010x} — kind={kind}");
            }
        }
        // Dump all engine statuses
        for eidx in 0..8_u32 {
            let status = r(0x2640 + (eidx as usize) * 4);
            if status != 0 {
                let rl_from_status = (status >> 12) & 0xF;
                eprintln!("║   ENGN{eidx}_STATUS: {status:#010x} runlist_from_bits={rl_from_status}");
            }
        }
    }

    // Find the PBDMA serving our GR runlist (used for all experiments)
    let mut target_pbdma: usize = 0;
    {
        let mut seq = 0_usize;
        for pid in 0..32_usize {
            if pbdma_map & (1 << pid) == 0 { continue; }
            let rl = r(0x2390 + seq * 4);
            if rl == target_runlist {
                target_pbdma = pid;
                break;
            }
            seq += 1;
        }
    }
    let pb = 0x040000 + target_pbdma * 0x2000;
    eprintln!("║ Target PBDMA: {target_pbdma} (base={pb:#x})");

    for id in 0..32_usize {
        if pbdma_map & (1 << id) == 0 { continue; }
        w(pbdma::intr(id), 0xFFFF_FFFF)?;
        w(pbdma::intr_en(id), 0xFFFF_FEFF)?;
        let b = 0x0004_0000 + id * 0x2000;
        w(b + 0x13C, 0)?;
        w(pbdma::hce_intr(id), 0)?;
        w(pbdma::hce_intr_en(id), 0)?;
        w(b + 0x164, 0xFFFF_FFFF)?;
    }

    w(pfifo::INTR, 0xFFFF_FFFF)?;
    w(pfifo::INTR_EN, 0x7FFF_FFFF)?;

    populate_page_tables_static(
        pd3.as_mut_slice(), pd2.as_mut_slice(),
        pd1.as_mut_slice(), pd0.as_mut_slice(),
        pt0.as_mut_slice(),
    );

    // Snapshot PBDMA residual state before any experiments (for comparison)
    let residual_userd_lo = r(pb + 0xD0);
    let residual_gp_base_lo = r(pb + 0x40);
    eprintln!("║ PBDMA residual: USERD_LO={residual_userd_lo:#010x} GP_BASE_LO={residual_gp_base_lo:#010x}");

    // Comprehensive PBDMA register dump for all active PBDMAs
    eprintln!("║ ── Full PBDMA Register Dump ──");
    for pid in [0_usize, 1, 2, 3] {
        if pbdma_map & (1 << pid) == 0 && pid != 0 { continue; }
        let base = 0x40000 + pid * 0x2000;
        let active = pbdma_map & (1 << pid) != 0;
        eprint!("║ PBDMA{pid}{}:", if active { "" } else { "(off)" });
        for off in (0x00..=0x1FC_usize).step_by(4) {
            let val = r(base + off);
            if val != 0 {
                eprint!(" [{off:#05x}]={val:#010x}");
            }
        }
        eprintln!();
    }

    // ── Run experiment matrix ───────────────────────────────────────────

    let header = format!(
        "{:<42} | {:>8} | {:<5} | {:<5} | {:>17} | {:>3} | {:>9} | {:>8}",
        "Config", "PCCSR", "Fault", "Sched", "PBDMA USERD", "Own", "GP pt/ft", "ENGN0"
    );
    eprintln!("\n╔══ EXPERIMENT MATRIX ({} configs) ════════════════════════╗", configs.len());
    eprintln!("║ {header}");
    eprintln!("║ {}", "─".repeat(header.len()));

    let limit2 = gpfifo_entries.ilog2();
    let mut results = Vec::with_capacity(configs.len());

    for cfg in configs {
        instance.as_mut_slice().fill(0);
        runlist.as_mut_slice().fill(0);

        populate_instance_block_static(
            instance.as_mut_slice(),
            gpfifo_iova, gpfifo_entries, userd_iova, channel_id,
        );

        populate_runlist_static(
            runlist.as_mut_slice(),
            userd_iova, channel_id,
            cfg.runlist_userd_target, cfg.runlist_inst_target, 0,
        );

        // Clear stale PCCSR state
        let stale = r(pccsr::channel(channel_id));
        if stale & 1 != 0 {
            let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_CLR);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        if stale & (pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET) != 0 {
            let _ = w(pccsr::channel(channel_id),
                pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let _ = w(pccsr::inst(channel_id), 0);
        std::thread::sleep(std::time::Duration::from_millis(5));
        let _ = w(pfifo::INTR, 0xFFFF_FFFF);

        // Build PCCSR inst value
        let pccsr_inst_val = {
            let base = (INSTANCE_IOVA >> 12) as u32 | (cfg.pccsr_target << 28);
            match cfg.ordering {
                ExperimentOrdering::BindWithInstBindEnableRunlist
                | ExperimentOrdering::DirectPbdmaWithInstBind
                | ExperimentOrdering::FullDispatchWithInstBind
                | ExperimentOrdering::FullDispatchWithPreempt
                | ExperimentOrdering::ScheduledPlusDirectPbdma
                | ExperimentOrdering::VramFullDispatch => base | pccsr::INST_BIND_TRUE,
                _ => base,
            }
        };
        let rl_base = (cfg.runlist_base_target << 28) | (RUNLIST_IOVA >> 12) as u32;
        let rl_submit = (target_runlist << 20) | 2;

        match cfg.ordering {
            ExperimentOrdering::BindEnableRunlist => {
                let _ = w(pccsr::inst(channel_id), pccsr_inst_val);
                std::thread::sleep(std::time::Duration::from_millis(2));
                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
                std::thread::sleep(std::time::Duration::from_millis(2));
                let _ = w(pfifo::RUNLIST_BASE, rl_base);
                let _ = w(pfifo::RUNLIST, rl_submit);
            }
            ExperimentOrdering::BindRunlistEnable => {
                let _ = w(pccsr::inst(channel_id), pccsr_inst_val);
                std::thread::sleep(std::time::Duration::from_millis(2));
                let _ = w(pfifo::RUNLIST_BASE, rl_base);
                let _ = w(pfifo::RUNLIST, rl_submit);
                std::thread::sleep(std::time::Duration::from_millis(2));
                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
            }
            ExperimentOrdering::RunlistBindEnable => {
                let _ = w(pfifo::RUNLIST_BASE, rl_base);
                let _ = w(pfifo::RUNLIST, rl_submit);
                std::thread::sleep(std::time::Duration::from_millis(2));
                let _ = w(pccsr::inst(channel_id), pccsr_inst_val);
                std::thread::sleep(std::time::Duration::from_millis(2));
                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
            }
            ExperimentOrdering::BindWithInstBindEnableRunlist => {
                let _ = w(pccsr::inst(channel_id), pccsr_inst_val);
                std::thread::sleep(std::time::Duration::from_millis(5));
                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
                std::thread::sleep(std::time::Duration::from_millis(2));
                let _ = w(pfifo::RUNLIST_BASE, rl_base);
                let _ = w(pfifo::RUNLIST, rl_submit);
            }
            ExperimentOrdering::DirectPbdmaProgramming
            | ExperimentOrdering::DirectPbdmaWithInstBind => {
                let _ = w(pccsr::inst(channel_id), pccsr_inst_val);
                std::thread::sleep(std::time::Duration::from_millis(5));
                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
                std::thread::sleep(std::time::Duration::from_millis(5));

                let _ = w(pb + 0x40, gpfifo_iova as u32);
                let _ = w(pb + 0x44,
                    (gpfifo_iova >> 32) as u32
                    | (limit2 << 16)
                    | (PBDMA_TARGET_SYS_MEM_COHERENT << 28));
                let _ = w(pb + 0xD0,
                    (userd_iova as u32 & 0xFFFF_FE00)
                    | PBDMA_TARGET_SYS_MEM_COHERENT);
                let _ = w(pb + 0xD4, (userd_iova >> 32) as u32);
                let _ = w(pb + 0xC0, 0x0000_FACE);
                let _ = w(pb + 0xAC, 0x1000_3080);
                let _ = w(pb + 0xA8, 0x0000_1100);
                let _ = w(pb + 0x54, 0);
            }
            ExperimentOrdering::DirectPbdmaActivate
            | ExperimentOrdering::DirectPbdmaActivateDoorbell
            | ExperimentOrdering::DirectPbdmaActivateScheduled => {
                // Step 1: Bind instance to PCCSR + enable channel
                let _ = w(pccsr::inst(channel_id), pccsr_inst_val);
                std::thread::sleep(std::time::Duration::from_millis(5));
                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
                std::thread::sleep(std::time::Duration::from_millis(5));

                // Step 2: Write PBDMA registers (same as E)
                let _ = w(pb + 0x40, gpfifo_iova as u32);
                let _ = w(pb + 0x44,
                    (gpfifo_iova >> 32) as u32
                    | (limit2 << 16)
                    | (PBDMA_TARGET_SYS_MEM_COHERENT << 28));
                let _ = w(pb + 0xD0,
                    (userd_iova as u32 & 0xFFFF_FE00)
                    | PBDMA_TARGET_SYS_MEM_COHERENT);
                let _ = w(pb + 0xD4, (userd_iova >> 32) as u32);
                let _ = w(pb + 0xC0, 0x0000_FACE);
                let _ = w(pb + 0xAC, 0x1000_3080);
                let _ = w(pb + 0xA8, 0x0000_1100);

                // Step 3: Reset GP_FETCH and GP_STATE to 0
                let _ = w(pb + 0x48, 0);
                let _ = w(pb + 0x4C, 0);

                // Step 4: Write a GPFIFO entry into the ring buffer (slot 0).
                // Points to RUNLIST_IOVA (filled with zeros = NOP pushbuffer).
                // GPFIFO entry: DW0 = VA[31:2]|TYPE=0, DW1 = VA_HI|LEN_DWORDS<<10
                let gp_entry: u64 = (RUNLIST_IOVA & 0xFFFF_FFFC)
                    | ((1_u64) << (32 + 10)); // 1 dword of NOP
                gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());

                // Step 5: Write USERD GP_PUT = 1 (host DMA memory)
                write_u32_le(userd_page, ramuserd::GP_PUT, 1);
                // Also zero USERD GP_GET
                write_u32_le(userd_page, ramuserd::GP_GET, 0);

                // Memory fence: ensure DMA writes are visible before MMIO
                std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

                // Step 6: Write PBDMA GP_PUT = 1 (via BAR0 MMIO)
                let _ = w(pb + 0x54, 1);

                // Step 7: Variant-specific activation
                if matches!(cfg.ordering, ExperimentOrdering::DirectPbdmaActivateDoorbell) {
                    let _ = w(usermode::NOTIFY_CHANNEL_PENDING, channel_id);
                }
                if matches!(cfg.ordering, ExperimentOrdering::DirectPbdmaActivateScheduled) {
                    let _ = w(pccsr::channel(channel_id),
                        pccsr::CHANNEL_ENABLE_SET | 0x2);
                }
            }
            ExperimentOrdering::VramInstanceBind => {
                const PRAMIN_BASE: usize = 0x0070_0000;
                const BAR0_WINDOW: usize = 0x0000_1700;
                const VRAM_INST_OFF: usize = 0x3000;

                let _ = w(BAR0_WINDOW, 0);
                std::thread::sleep(std::time::Duration::from_millis(1));

                // Copy instance block to VRAM
                let inst_bytes = instance.as_slice();
                for off in (0..inst_bytes.len()).step_by(4) {
                    let val = u32::from_le_bytes([
                        inst_bytes[off], inst_bytes[off+1],
                        inst_bytes[off+2], inst_bytes[off+3],
                    ]);
                    let _ = w(PRAMIN_BASE + VRAM_INST_OFF + off, val);
                }

                let vram_sig = r(PRAMIN_BASE + VRAM_INST_OFF + ramfc::SIGNATURE);
                let vram_gpb = r(PRAMIN_BASE + VRAM_INST_OFF + ramfc::GP_BASE_LO);
                eprintln!("║   VRAM verify: SIG={vram_sig:#010x} GP_BASE={vram_gpb:#010x}");

                // Reset PBDMA registers to sentinel (0xBEEF) so we can
                // detect if the scheduler overwrites them with RAMFC values.
                let _ = w(pb + 0x40, 0xBEEF_0040); // GP_BASE_LO
                let _ = w(pb + 0x44, 0xBEEF_0044); // GP_BASE_HI
                let _ = w(pb + 0x48, 0);            // GP_FETCH
                let _ = w(pb + 0x4C, 0);            // GP_STATE
                let _ = w(pb + 0x54, 0);            // GP_PUT
                let _ = w(pb + 0xD0, 0xBEEF_00D0); // USERD_LO
                let _ = w(pb + 0xD4, 0xBEEF_00D4); // USERD_HI
                let _ = w(pb + 0xC0, 0xBEEF_00C0); // SIGNATURE
                let _ = w(pb + 0xAC, 0xBEEF_00AC); // CHANNEL_INFO

                // Runlist: INST_TARGET=0 (VID_MEM)
                runlist.as_mut_slice().fill(0);
                populate_runlist_static(
                    runlist.as_mut_slice(),
                    userd_iova, channel_id,
                    cfg.runlist_userd_target,
                    0, // INST_TARGET = VID_MEM
                    0,
                );

                // Flush GPU L2 cache so engines see our PRAMIN writes
                let _ = w(0x70010, 0x0000_0001);
                for _ in 0..2000_u32 {
                    if r(0x70010) & 3 == 0 { break; }
                    std::thread::sleep(std::time::Duration::from_micros(100));
                }

                // PCCSR: TARGET=0 (VID_MEM), NO BIND — let scheduler load context
                let vram_pccsr = VRAM_INST_OFF as u32 >> 12;
                let _ = w(pccsr::inst(channel_id), vram_pccsr);
                std::thread::sleep(std::time::Duration::from_millis(5));

                let post_inst = r(pccsr::channel(channel_id));
                eprintln!("║   post-INST(noBind): {post_inst:#010x}");

                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
                std::thread::sleep(std::time::Duration::from_millis(5));

                // Submit runlist
                let _ = w(pfifo::RUNLIST_BASE, rl_base);
                let _ = w(pfifo::RUNLIST, rl_submit);
                std::thread::sleep(std::time::Duration::from_millis(50));

                // Check PBDMA after scheduler should have loaded context
                let sched_gpb = r(pb + 0x40);
                let sched_userd = r(pb + 0xD0);
                let sched_sig = r(pb + 0xC0);
                let sched_state = r(pb + 0xB0);
                eprintln!("║   post-sched PBDMA: GP_BASE={sched_gpb:#010x} USERD={sched_userd:#010x} SIG={sched_sig:#010x} STATE={sched_state:#010x}");

                // Set up GPFIFO entry + USERD GP_PUT + doorbell
                let gp_entry: u64 = (RUNLIST_IOVA & 0xFFFF_FFFC)
                    | ((1_u64) << (32 + 10));
                gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
                write_u32_le(userd_page, ramuserd::GP_PUT, 1);
                write_u32_le(userd_page, ramuserd::GP_GET, 0);
                std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

                let _ = w(usermode::NOTIFY_CHANNEL_PENDING, channel_id);
            }
            ExperimentOrdering::AllVram => {
                // Put EVERY structure in VRAM via PRAMIN.
                // VRAM layout (all within 64KB PRAMIN window at BAR0_WINDOW=0):
                //   0x0000 PD3, 0x1000 PD2, 0x2000 PD1, 0x3000 PD0,
                //   0x4000 PT0, 0x8000 Instance, 0x9000 GPFIFO,
                //   0xA000 USERD, 0xB000 NOP pushbuf, 0xC000 Runlist
                const PM: usize = 0x0070_0000;
                const BW: usize = 0x0000_1700;
                let _ = w(BW, 0);
                std::thread::sleep(std::time::Duration::from_millis(1));

                // Helper: write u32 to VRAM offset via PRAMIN
                let wv = |off: usize, val: u32| -> DriverResult<()> {
                    w(PM + off, val)
                };

                // Zero critical regions first (64KB is too big; just zero what we use)
                for off in (0..0xD000_usize).step_by(4) {
                    let _ = wv(off, 0);
                }

                // ── Page tables (all VRAM, VID_MEM aperture) ──
                // PDE format: bits[1:0]=aperture(1=VID_MEM), bit[2]=VOL
                // PTE format: bit[0]=VALID, bits[2:1]=aperture(0=VID_MEM), bit[3]=VOL
                let vram_pde = |addr: u64| -> u64 {
                    (addr >> 4) | 1 // aperture=VID_MEM(1)
                };
                let vram_pte = |addr: u64| -> u64 {
                    (addr >> 4) | 1 // VALID=1, aperture=VID_MEM(0)
                };

                // PD3[0] → PD2 at VRAM 0x1000
                let e = vram_pde(0x1000);
                let _ = wv(0x0000, e as u32);
                let _ = wv(0x0004, (e >> 32) as u32);

                // PD2[0] → PD1 at VRAM 0x2000
                let e = vram_pde(0x2000);
                let _ = wv(0x1000, e as u32);
                let _ = wv(0x1004, (e >> 32) as u32);

                // PD1[0] → PD0 at VRAM 0x3000
                let e = vram_pde(0x3000);
                let _ = wv(0x2000, e as u32);
                let _ = wv(0x2004, (e >> 32) as u32);

                // PD0 dual entry: [0]=big page PDE(invalid), [8]=small page PDE → PT0
                let e = vram_pde(0x4000);
                let _ = wv(0x3008, e as u32);
                let _ = wv(0x300C, (e >> 32) as u32);

                // PT0: identity map VRAM pages 0..0xC (covering 0x0000-0xCFFF)
                for page in 0..13_usize {
                    let phys = (page as u64) * 4096;
                    let e = vram_pte(phys);
                    let off = 0x4000 + page * 8;
                    let _ = wv(off, e as u32);
                    let _ = wv(off + 4, (e >> 32) as u32);
                }

                // ── Instance block at VRAM 0x8000 ──
                // RAMFC: all addresses are VRAM physical, PBDMA target=0 (VID_MEM)
                let inst_base = 0x8000_usize;

                // RAMIN: PDB → PD3 at VRAM 0x0000
                // Matches populate_instance_block_static format but with VID_MEM target
                let pdb_lo: u32 = ((0_u64 >> 12) as u32) << 12
                    | (1 << 11)             // fault replay
                    | (1 << 10)             // fault replay
                    | (1 << 2)              // VOL
                    | 1;                    // aperture=VID_MEM(1)
                let _ = wv(inst_base + ramin::PAGE_DIR_BASE_LO, pdb_lo);
                let _ = wv(inst_base + ramin::PAGE_DIR_BASE_HI, 0);
                let _ = wv(inst_base + ramin::ENGINE_WFI_VEID, 0);
                let _ = wv(inst_base + ramin::SC_PDB_VALID, 1);
                let _ = wv(inst_base + ramin::SC0_PAGE_DIR_BASE_LO, pdb_lo);
                let _ = wv(inst_base + ramin::SC0_PAGE_DIR_BASE_HI, 0);

                // RAMFC fields (PBDMA target 0 = VID_MEM)
                let gpfifo_vram: u64 = 0x9000;
                let userd_vram: u64 = 0xA000;
                let _ = wv(inst_base + ramfc::GP_BASE_LO, gpfifo_vram as u32);
                let _ = wv(inst_base + ramfc::GP_BASE_HI,
                    (gpfifo_vram >> 32) as u32
                    | (limit2 << 16)
                    | (0_u32 << 28)); // PBDMA_TARGET_VID_MEM = 0
                let _ = wv(inst_base + ramfc::USERD_LO,
                    (userd_vram as u32 & 0xFFFF_FE00) | 0); // target=VID_MEM
                let _ = wv(inst_base + ramfc::USERD_HI, 0);
                let _ = wv(inst_base + ramfc::SIGNATURE, 0x0000_FACE);
                let _ = wv(inst_base + ramfc::ACQUIRE, 0x7FFF_F902);
                let _ = wv(inst_base + ramfc::PB_HEADER, 0x2040_0000);
                let _ = wv(inst_base + ramfc::SUBDEVICE, 0x3000_0000 | 0xFFF);
                let _ = wv(inst_base + ramfc::HCE_CTRL, 0x0000_0020);
                let _ = wv(inst_base + ramfc::CHID, channel_id);
                let _ = wv(inst_base + ramfc::CONFIG, 0x0000_1100);
                let _ = wv(inst_base + ramfc::CHANNEL_INFO, 0x1000_3080);

                // Verify instance block
                let v_sig = r(PM + inst_base + ramfc::SIGNATURE);
                let v_gpb = r(PM + inst_base + ramfc::GP_BASE_LO);
                let v_pdb = r(PM + inst_base + ramin::PAGE_DIR_BASE_LO);
                let v_usr = r(PM + inst_base + ramfc::USERD_LO);
                eprintln!("║   VRAM inst: SIG={v_sig:#010x} GP={v_gpb:#010x} PDB={v_pdb:#010x} USR={v_usr:#010x}");

                // ── GPFIFO entry at VRAM 0x9000 ──
                // Points to NOP push buffer at GPU VA 0xB000 (identity-mapped to VRAM 0xB000)
                let gp_entry: u64 = (0xB000_u64 & 0xFFFF_FFFC)
                    | ((1_u64) << (32 + 10)); // 1 dword
                let _ = wv(0x9000, gp_entry as u32);
                let _ = wv(0x9004, (gp_entry >> 32) as u32);

                // ── USERD page at VRAM 0xA000 ──
                let _ = wv(0xA000 + ramuserd::GP_PUT, 1); // GP_PUT = 1
                let _ = wv(0xA000 + ramuserd::GP_GET, 0); // GP_GET = 0

                // ── NOP push buffer at VRAM 0xB000 ──
                // Already zeroed (NOP method: count=0, method=0)

                // ── Runlist at VRAM 0xC000 ──
                // TSG group entry (16 bytes): TYPE=1, TIMESLICE_SCALE=3, TIMEOUT=128, LEN=1
                let _ = wv(0xC000, (128 << 24) | (3 << 16) | 1);
                let _ = wv(0xC004, 1); // TSG_LENGTH = 1 channel
                let _ = wv(0xC008, 0); // TSG_ID = 0
                let _ = wv(0xC00C, 0);
                // Channel entry: USERD_PTR[31:8]|USERD_TGT[7:6]|INST_TGT[5:4]|RQ[1]|TYPE=0
                let chan_dw0 = userd_vram as u32 & 0xFFFF_FF00;
                let _ = wv(0xC010, chan_dw0);
                let _ = wv(0xC014, 0); // USERD_PTR_HI
                let chan_dw2 = (0x8000_u32 & 0xFFFF_F000) | channel_id; // INST@0x8000, CH=0
                let _ = wv(0xC018, chan_dw2);
                let _ = wv(0xC01C, 0); // INST_PTR_HI

                // Reset PBDMA to sentinel
                let _ = w(pb + 0x40, 0xBEEF_0040);
                let _ = w(pb + 0x44, 0xBEEF_0044);
                let _ = w(pb + 0x48, 0);
                let _ = w(pb + 0x54, 0);
                let _ = w(pb + 0xD0, 0xBEEF_00D0);
                let _ = w(pb + 0xD4, 0xBEEF_00D4);
                let _ = w(pb + 0xC0, 0xBEEF_00C0);

                // Flush GPU L2 cache so engines see our PRAMIN writes
                let _ = w(0x70010, 0x0000_0001);
                for _ in 0..2000_u32 {
                    if r(0x70010) & 3 == 0 { break; }
                    std::thread::sleep(std::time::Duration::from_micros(100));
                }

                // PCCSR: INST_PTR = 0x8000>>12 = 8, TARGET=0 (VID_MEM), BIND=TRUE
                let vram_inst = (0x8000_u32 >> 12) | pccsr::INST_BIND_TRUE;
                let _ = w(pccsr::inst(channel_id), vram_inst);
                std::thread::sleep(std::time::Duration::from_millis(10));

                let post_bind = r(pccsr::channel(channel_id));
                eprintln!("║   post-BIND(allVram): {post_bind:#010x}");

                // Clear any faults from INST_BIND
                if post_bind & 0x11000000 != 0 {
                    let _ = w(pccsr::channel(channel_id),
                        pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET);
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }

                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
                std::thread::sleep(std::time::Duration::from_millis(5));

                // Submit runlist: base=VRAM 0xC000, target=VID_MEM(0), 2 entries
                let _ = w(pfifo::RUNLIST_BASE, (0_u32 << 28) | (0xC000_u32 >> 12));
                let _ = w(pfifo::RUNLIST, (target_runlist << 20) | 2);
                std::thread::sleep(std::time::Duration::from_millis(50));

                // Check scheduler status
                let post_rl = r(pccsr::channel(channel_id));
                let eng_rl_base = r(0x2288); // ENG_RUNLIST_BASE(1)
                let eng_rl = r(0x228C);      // ENG_RUNLIST(1)
                let sched_gpb = r(pb + 0x40);
                let sched_usr = r(pb + 0xD0);
                let sched_sig = r(pb + 0xC0);
                let sched_state = r(pb + 0xB0);
                eprintln!("║   post-submit: PCCSR={post_rl:#010x} ENG_RL_BASE={eng_rl_base:#010x} ENG_RL={eng_rl:#010x}");
                eprintln!("║   PBDMA: GP_BASE={sched_gpb:#010x} USERD={sched_usr:#010x} SIG={sched_sig:#010x} STATE={sched_state:#010x}");

                // Ring doorbell
                let _ = w(usermode::NOTIFY_CHANNEL_PENDING, channel_id);
                std::thread::sleep(std::time::Duration::from_millis(50));

                let post_db = r(pccsr::channel(channel_id));
                let gpb_post = r(pb + 0x40);
                let usr_post = r(pb + 0xD0);
                let sig_post = r(pb + 0xC0);
                let gp_put_post = r(pb + 0x54);
                let gp_fetch_post = r(pb + 0x48);
                eprintln!("║   post-doorbell: PCCSR={post_db:#010x} GP_BASE={gpb_post:#010x} USERD={usr_post:#010x} SIG={sig_post:#010x} GP_PUT={gp_put_post} GP_FETCH={gp_fetch_post}");
            }
            ExperimentOrdering::AllVramDirectPbdma => {
                // Same VRAM setup as K but with direct PBDMA programming
                const PM: usize = 0x0070_0000;
                const BW: usize = 0x0000_1700;
                let _ = w(BW, 0);
                std::thread::sleep(std::time::Duration::from_millis(1));

                let wv = |off: usize, val: u32| -> DriverResult<()> { w(PM + off, val) };

                // Zero VRAM region
                for off in (0..0xD000_usize).step_by(4) { let _ = wv(off, 0); }

                // Page tables (VRAM, VID_MEM aperture)
                let vram_pde = |addr: u64| -> u64 { (addr >> 4) | 1 };
                let vram_pte = |addr: u64| -> u64 { (addr >> 4) | 1 };

                let e = vram_pde(0x1000); let _ = wv(0x0000, e as u32); let _ = wv(0x0004, (e >> 32) as u32);
                let e = vram_pde(0x2000); let _ = wv(0x1000, e as u32); let _ = wv(0x1004, (e >> 32) as u32);
                let e = vram_pde(0x3000); let _ = wv(0x2000, e as u32); let _ = wv(0x2004, (e >> 32) as u32);
                let e = vram_pde(0x4000); let _ = wv(0x3008, e as u32); let _ = wv(0x300C, (e >> 32) as u32);
                for page in 0..13_usize {
                    let e = vram_pte((page as u64) * 4096);
                    let off = 0x4000 + page * 8;
                    let _ = wv(off, e as u32); let _ = wv(off + 4, (e >> 32) as u32);
                }

                // Instance block at VRAM 0x8000 (for PCCSR only — PBDMA gets direct writes)
                let ib = 0x8000_usize;
                let pdb_lo: u32 = (1 << 11) | (1 << 10) | (1 << 2) | 1;
                let _ = wv(ib + ramin::PAGE_DIR_BASE_LO, pdb_lo);
                let _ = wv(ib + ramin::PAGE_DIR_BASE_HI, 0);
                let _ = wv(ib + ramin::ENGINE_WFI_VEID, 0);
                let _ = wv(ib + ramin::SC_PDB_VALID, 1);
                let _ = wv(ib + ramin::SC0_PAGE_DIR_BASE_LO, pdb_lo);
                let _ = wv(ib + ramin::SC0_PAGE_DIR_BASE_HI, 0);
                let gpfifo_vram: u64 = 0x9000;
                let userd_vram: u64 = 0xA000;
                let _ = wv(ib + ramfc::GP_BASE_LO, gpfifo_vram as u32);
                let _ = wv(ib + ramfc::GP_BASE_HI, limit2 << 16);
                let _ = wv(ib + ramfc::USERD_LO, userd_vram as u32 & 0xFFFF_FE00);
                let _ = wv(ib + ramfc::USERD_HI, 0);
                let _ = wv(ib + ramfc::SIGNATURE, 0x0000_FACE);
                let _ = wv(ib + ramfc::ACQUIRE, 0x7FFF_F902);
                let _ = wv(ib + ramfc::PB_HEADER, 0x2040_0000);
                let _ = wv(ib + ramfc::SUBDEVICE, 0x3000_0FFF);
                let _ = wv(ib + ramfc::HCE_CTRL, 0x0000_0020);
                let _ = wv(ib + ramfc::CHID, channel_id);
                let _ = wv(ib + ramfc::CONFIG, 0x0000_1100);
                let _ = wv(ib + ramfc::CHANNEL_INFO, 0x1000_3080);

                // GPFIFO entry → NOP at GPU VA 0xB000
                let gp_entry: u64 = (0xB000_u64 & 0xFFFF_FFFC) | ((1_u64) << (32 + 10));
                let _ = wv(0x9000, gp_entry as u32);
                let _ = wv(0x9004, (gp_entry >> 32) as u32);

                // USERD: GP_PUT=1
                let _ = wv(0xA000 + ramuserd::GP_PUT, 1);
                let _ = wv(0xA000 + ramuserd::GP_GET, 0);

                // Runlist at VRAM 0xC000
                let _ = wv(0xC000, (128 << 24) | (3 << 16) | 1);
                let _ = wv(0xC004, 1);
                let _ = wv(0xC008, 0);
                let _ = wv(0xC00C, 0);
                let _ = wv(0xC010, (userd_vram as u32) & 0xFFFF_FF00);
                let _ = wv(0xC014, 0);
                let _ = wv(0xC018, (0x8000_u32 & 0xFFFF_F000) | channel_id);
                let _ = wv(0xC01C, 0);

                // L2 flush
                let _ = w(0x70010, 1);
                for _ in 0..2000_u32 {
                    if r(0x70010) & 3 == 0 { break; }
                    std::thread::sleep(std::time::Duration::from_micros(100));
                }

                // PCCSR: set INST to VRAM 0x8000, NO BIND
                let _ = w(pccsr::inst(channel_id), 0x8000_u32 >> 12);
                std::thread::sleep(std::time::Duration::from_millis(5));

                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
                std::thread::sleep(std::time::Duration::from_millis(5));

                // Directly program PBDMA with VRAM pointers
                let _ = w(pb + 0x40, gpfifo_vram as u32); // GP_BASE_LO
                let _ = w(pb + 0x44, limit2 << 16);
                let _ = w(pb + 0x48, 0); // GP_FETCH = 0
                let _ = w(pb + 0x4C, 0); // GP_STATE = 0
                let _ = w(pb + 0xD0, userd_vram as u32 & 0xFFFF_FE00); // USERD_LO (VID_MEM=0)
                let _ = w(pb + 0xD4, 0); // USERD_HI
                let _ = w(pb + 0xC0, 0x0000_FACE); // SIGNATURE
                let _ = w(pb + 0xAC, 0x1000_3080); // CHANNEL_INFO
                let _ = w(pb + 0xA8, 0x0000_1100); // CONFIG

                // Submit runlist
                let _ = w(pfifo::RUNLIST_BASE, 0xC000_u32 >> 12);
                let _ = w(pfifo::RUNLIST, (target_runlist << 20) | 2);
                std::thread::sleep(std::time::Duration::from_millis(20));

                // GP_PUT = 1 (both PBDMA register and doorbell)
                let _ = w(pb + 0x54, 1);
                std::thread::sleep(std::time::Duration::from_millis(5));
                let _ = w(usermode::NOTIFY_CHANNEL_PENDING, channel_id);
                std::thread::sleep(std::time::Duration::from_millis(50));

                let post = r(pccsr::channel(channel_id));
                let gp_put = r(pb + 0x54);
                let gp_fetch = r(pb + 0x48);
                let userd_lo = r(pb + 0xD0);
                let sig = r(pb + 0xC0);
                let state = r(pb + 0xB0);
                eprintln!("║   L result: PCCSR={post:#010x} GP_PUT={gp_put} GP_FETCH={gp_fetch} USERD={userd_lo:#010x} SIG={sig:#010x} STATE={state:#010x}");
            }
            ExperimentOrdering::PfifoResetInit => {
                // PFIFO engine reset via PMC bit 8 toggle — DESTRUCTIVE!
                // Skip on warm GPU to avoid killing PFIFO for other experiments.
                let pmc_cur = r(pmc::ENABLE);
                if !gpu_warm {
                    eprintln!("║   M: GPU cold, performing PMC reset...");
                    let _ = w(pmc::ENABLE, pmc_cur & !0x100);
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    let _ = w(pmc::ENABLE, pmc_cur | 0x100);
                    std::thread::sleep(std::time::Duration::from_millis(10));
                } else {
                    eprintln!("║   M: GPU warm, skipping PMC toggle to preserve state");
                }

                let pmc_post = r(pmc::ENABLE);
                let pfifo_post = r(pfifo::ENABLE);
                let pbdma_post = r(pfifo::PBDMA_MAP);
                let sched_post = r(0x2630);
                eprintln!("║   M state: PMC={pmc_post:#010x} PFIFO={pfifo_post:#010x} PBDMA_MAP={pbdma_post:#010x} SCHED_DIS={sched_post:#010x}");

                // Follow the D path: INST_BIND + enable + runlist
                let _ = w(pccsr::inst(channel_id), pccsr_inst_val);
                std::thread::sleep(std::time::Duration::from_millis(5));
                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
                std::thread::sleep(std::time::Duration::from_millis(2));
                let _ = w(pfifo::RUNLIST_BASE, rl_base);
                let _ = w(pfifo::RUNLIST, rl_submit);
                std::thread::sleep(std::time::Duration::from_millis(50));

                let post = r(pccsr::channel(channel_id));
                let gp_fetch = r(pb + 0x48);
                let userd_rd = r(pb + 0xD0);
                eprintln!("║   M result: PCCSR={post:#010x} GP_FETCH={gp_fetch} USERD={userd_rd:#010x}");
            }
            ExperimentOrdering::FullDispatchWithInstBind => {
                // Step 1: Prepare GPFIFO entry BEFORE bind so DMA is ready
                let gp_entry: u64 = (RUNLIST_IOVA & 0xFFFF_FFFC)
                    | ((1_u64) << (32 + 10));
                gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
                write_u32_le(userd_page, ramuserd::GP_PUT, 1);
                write_u32_le(userd_page, ramuserd::GP_GET, 0);
                std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

                // Step 2: INST_BIND (D path — proven to achieve SCHEDULED on warm GPU)
                let _ = w(pccsr::inst(channel_id), pccsr_inst_val);
                std::thread::sleep(std::time::Duration::from_millis(5));
                let post_bind = r(pccsr::channel(channel_id));
                eprintln!("║   N post-INST_BIND: {post_bind:#010x} (inst_val={pccsr_inst_val:#010x})");

                // Detailed fault diagnostics BEFORE clearing anything
                if post_bind & 0x11000000 != 0 {
                    let bind_err = r(0x252C);
                    let pfifo_intr = r(pfifo::INTR);
                    let mmu_fault_status = r(0x100E34);
                    let mmu_fault_addr_lo = r(0x100E38);
                    let mmu_fault_addr_hi = r(0x100E3C);
                    let mmu_fault_inst_lo = r(0x100E40);
                    let mmu_fault_inst_hi = r(0x100E44);
                    let mmu_buf0_put = r(0x100E34 - 4);
                    eprintln!("║   N FAULT DIAG: BIND_ERR={bind_err:#010x} PFIFO_INTR={pfifo_intr:#010x}");
                    eprintln!("║   N FAULT DIAG: MMU_STATUS={mmu_fault_status:#010x} ADDR={mmu_fault_addr_hi:#010x}_{mmu_fault_addr_lo:#010x}");
                    eprintln!("║   N FAULT DIAG: MMU_INST={mmu_fault_inst_hi:#010x}_{mmu_fault_inst_lo:#010x}");
                    // Check all PBDMA interrupt registers
                    for pid in [1_usize, 2] {
                        let intr = r(pbdma::intr(pid));
                        let status = r(0x40000 + pid * 0x2000 + 0xB0);
                        let method = r(0x40000 + pid * 0x2000 + 0x1C0);
                        eprintln!("║   N PBDMA{pid} INTR={intr:#010x} STATE={status:#010x} METHOD={method:#010x}");
                    }
                    // Non-replayable fault buffer (may have fault entry)
                    let nrfb_get = r(0x100E4C);
                    let nrfb_put = r(0x100E50);
                    eprintln!("║   N NR_FAULT_BUF: GET={nrfb_get:#010x} PUT={nrfb_put:#010x}");
                }

                // Step 3: Enable channel
                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
                std::thread::sleep(std::time::Duration::from_millis(2));

                // Step 4: Submit runlist
                let _ = w(pfifo::RUNLIST_BASE, rl_base);
                let _ = w(pfifo::RUNLIST, rl_submit);
                std::thread::sleep(std::time::Duration::from_millis(50));

                let post_rl = r(pccsr::channel(channel_id));
                let scheduled = (post_rl & 2) != 0;
                eprintln!("║   N post-runlist: {post_rl:#010x} scheduled={scheduled}");

                // Step 5: Check PBDMA state — did the scheduler load our context?
                let pbdma_userd = r(pb + 0xD0);
                let pbdma_gpbase = r(pb + 0x40);
                let pbdma_sig = r(pb + 0xC0);
                let pbdma_gp_put = r(pb + 0x54);
                let pbdma_gp_fetch = r(pb + 0x48);
                let pbdma_state = r(pb + 0xB0);
                eprintln!("║   N pre-doorbell PBDMA: USERD={pbdma_userd:#010x} GP_BASE={pbdma_gpbase:#010x} SIG={pbdma_sig:#010x} GP_PUT={pbdma_gp_put} GP_FETCH={pbdma_gp_fetch} STATE={pbdma_state:#010x}");

                // Step 6: Ring doorbell
                let _ = w(usermode::NOTIFY_CHANNEL_PENDING, channel_id);
                std::thread::sleep(std::time::Duration::from_millis(100));

                // Step 7: Check PBDMA again — did doorbell trigger context load + fetch?
                let post_db = r(pccsr::channel(channel_id));
                let db_userd = r(pb + 0xD0);
                let db_gpbase = r(pb + 0x40);
                let db_sig = r(pb + 0xC0);
                let db_gp_put = r(pb + 0x54);
                let db_gp_fetch = r(pb + 0x48);
                let db_state = r(pb + 0xB0);
                let db_gp_state = r(pb + 0x4C);
                eprintln!("║   N post-doorbell: PCCSR={post_db:#010x} USERD={db_userd:#010x} GP_BASE={db_gpbase:#010x} SIG={db_sig:#010x}");
                eprintln!("║   N post-doorbell: GP_PUT={db_gp_put} GP_FETCH={db_gp_fetch} STATE={db_state:#010x} GP_STATE={db_gp_state:#010x}");

                // Check ALL PBDMAs on this runlist (scheduler may assign to PBDMA 2!)
                {
                    let mut seq = 0_usize;
                    for pid in 0..32_usize {
                        if pbdma_map & (1 << pid) == 0 { continue; }
                        let rl = r(0x2390 + seq * 4);
                        seq += 1;
                        if rl != target_runlist { continue; }
                        let pbx = 0x40000 + pid * 0x2000;
                        let userd = r(pbx + 0xD0);
                        let gpbase = r(pbx + 0x40);
                        let sig = r(pbx + 0xC0);
                        let gp_put = r(pbx + 0x54);
                        let gp_fetch = r(pbx + 0x48);
                        let state = r(pbx + 0xB0);
                        eprintln!("║   N PBDMA{pid}: USERD={userd:#010x} GP_BASE={gpbase:#010x} SIG={sig:#010x} GP_PUT={gp_put} GP_FETCH={gp_fetch} STATE={state:#010x}");
                    }
                }

                // Step 8: If still not fetching, try a longer wait + second doorbell
                if db_gp_fetch == 0 {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    let _ = w(usermode::NOTIFY_CHANNEL_PENDING, channel_id);
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    let final_pccsr = r(pccsr::channel(channel_id));
                    let final_intr = r(pfifo::INTR);
                    // Check all PBDMAs again after retry
                    let mut any_fetch = false;
                    let mut seqr = 0_usize;
                    for pid in 0..32_usize {
                        if pbdma_map & (1 << pid) == 0 { continue; }
                        let rl = r(0x2390 + seqr * 4);
                        seqr += 1;
                        if rl != target_runlist { continue; }
                        let pbx = 0x40000 + pid * 0x2000;
                        let gp_fetch = r(pbx + 0x48);
                        let userd = r(pbx + 0xD0);
                        let state = r(pbx + 0xB0);
                        if gp_fetch != 0 { any_fetch = true; }
                        eprintln!("║   N retry PBDMA{pid}: GP_FETCH={gp_fetch} USERD={userd:#010x} STATE={state:#010x}");
                    }
                    eprintln!("║   N retry: PCCSR={final_pccsr:#010x} PFIFO_INTR={final_intr:#010x} any_fetch={any_fetch}");
                }
            }
            ExperimentOrdering::FullDispatchWithPreempt => {
                // Same as N but with PREEMPT to force context load

                // Prepare GPFIFO + USERD
                let gp_entry: u64 = (RUNLIST_IOVA & 0xFFFF_FFFC)
                    | ((1_u64) << (32 + 10));
                gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
                write_u32_le(userd_page, ramuserd::GP_PUT, 1);
                write_u32_le(userd_page, ramuserd::GP_GET, 0);
                std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

                // INST_BIND + enable + runlist (D path)
                let _ = w(pccsr::inst(channel_id), pccsr_inst_val);
                std::thread::sleep(std::time::Duration::from_millis(5));
                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
                std::thread::sleep(std::time::Duration::from_millis(2));
                let _ = w(pfifo::RUNLIST_BASE, rl_base);
                let _ = w(pfifo::RUNLIST, rl_submit);
                std::thread::sleep(std::time::Duration::from_millis(50));

                let post_rl = r(pccsr::channel(channel_id));
                eprintln!("║   O post-runlist: {post_rl:#010x} sched={}", post_rl & 2 != 0);

                // PREEMPT: force context switch to our channel
                // Nouveau reference shows PREEMPT=0x01000001 at 0x2634
                // Format: bit[24]=PENDING, bits[11:0]=channel_id (type 1 = channel preempt)
                // Also try runlist preempt: bit[20]=1, bits[15:12]=runlist_id
                let preempt_ch = (1_u32 << 24) | channel_id;
                let _ = w(0x2634, preempt_ch);
                std::thread::sleep(std::time::Duration::from_millis(50));

                let post_preempt = r(pccsr::channel(channel_id));
                let preempt_rb = r(0x2634);
                eprintln!("║   O post-preempt(ch): PCCSR={post_preempt:#010x} PREEMPT={preempt_rb:#010x}");

                // Also try runlist preempt
                let preempt_rl = (1_u32 << 20) | (target_runlist << 16);
                let _ = w(0x2634, preempt_rl);
                std::thread::sleep(std::time::Duration::from_millis(50));

                let post_rl_preempt = r(pccsr::channel(channel_id));
                eprintln!("║   O post-preempt(rl): PCCSR={post_rl_preempt:#010x} PREEMPT={:#010x}", r(0x2634));

                // Ring doorbell
                let _ = w(usermode::NOTIFY_CHANNEL_PENDING, channel_id);
                std::thread::sleep(std::time::Duration::from_millis(100));

                let post_db = r(pccsr::channel(channel_id));
                let gp_fetch = r(pb + 0x48);
                let gp_put = r(pb + 0x54);
                let userd_lo = r(pb + 0xD0);
                let sig = r(pb + 0xC0);
                let state = r(pb + 0xB0);
                let gpbase = r(pb + 0x40);
                eprintln!("║   O final: PCCSR={post_db:#010x} GP_PUT={gp_put} GP_FETCH={gp_fetch} USERD={userd_lo:#010x} GP_BASE={gpbase:#010x} SIG={sig:#010x} STATE={state:#010x}");
            }
            ExperimentOrdering::ScheduledPlusDirectPbdma => {
                // Phase 0: Enable PFIFO + PBDMA interrupts (nouveau values)
                let _ = w(pfifo::INTR_EN, 0x6181_0101);
                let _ = w(pbdma::intr_en(1), 0xFFFF_FFFF);
                let _ = w(pbdma::intr_en(2), 0xFFFF_FFFF);

                // Phase 1: Prepare GPFIFO + USERD in host memory BEFORE bind
                let gp_entry: u64 = (RUNLIST_IOVA & 0xFFFF_FFFC)
                    | ((1_u64) << (32 + 10));
                gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
                write_u32_le(userd_page, ramuserd::GP_PUT, 1);
                write_u32_le(userd_page, ramuserd::GP_GET, 0);
                std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

                // Phase 2: INST_BIND + enable + runlist (D path, NO direct PBDMA writes)
                let _ = w(pccsr::inst(channel_id), pccsr_inst_val);
                std::thread::sleep(std::time::Duration::from_millis(5));
                let post_bind = r(pccsr::channel(channel_id));
                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
                std::thread::sleep(std::time::Duration::from_millis(2));
                let _ = w(pfifo::RUNLIST_BASE, rl_base);
                let _ = w(pfifo::RUNLIST, rl_submit);
                std::thread::sleep(std::time::Duration::from_millis(100));

                let post_rl = r(pccsr::channel(channel_id));
                eprintln!("║   P phase2: post_bind={post_bind:#010x} post_rl={post_rl:#010x} sched={}", post_rl & 2 != 0);

                // Check PBDMA state — did the scheduler load context from RAMFC?
                for pid in [1_usize, 2] {
                    let pbx = 0x40000 + pid * 0x2000;
                    let userd = r(pbx + 0xD0);
                    let gpbase = r(pbx + 0x40);
                    let sig = r(pbx + 0xC0);
                    let gp_put = r(pbx + 0x50);
                    let gp_fetch = r(pbx + 0x48);
                    let state = r(pbx + 0xB0);
                    let intr = r(pbdma::intr(pid));
                    eprintln!("║   P pre-db PBDMA{pid}: USERD={userd:#010x} GP_BASE={gpbase:#010x} SIG={sig:#010x} GP_PUT={gp_put} GP_FETCH={gp_fetch} STATE={state:#010x} INTR={intr:#010x}");
                }

                // Phase 4: Ring doorbell
                let _ = w(usermode::NOTIFY_CHANNEL_PENDING, channel_id);
                std::thread::sleep(std::time::Duration::from_millis(100));

                let post_db = r(pccsr::channel(channel_id));
                let pfifo_intr = r(pfifo::INTR);
                eprintln!("║   P post-doorbell: PCCSR={post_db:#010x} PFIFO_INTR={pfifo_intr:#010x}");

                // Read detailed fault info
                let mmu_fault_status = r(0x100E34);
                let mmu_fault_lo = r(0x100E38);
                let mmu_fault_hi = r(0x100E3C);
                let mmu_fault_inst_lo = r(0x100E40);
                let bind_err = r(0x252C);
                eprintln!("║   P FAULT: MMU_STATUS={mmu_fault_status:#010x} ADDR={mmu_fault_hi:#010x}_{mmu_fault_lo:#010x} INST={mmu_fault_inst_lo:#010x} BIND_ERR={bind_err:#010x}");

                // Read PBDMA interrupt and fault details using correct register offsets
                for pid in [1_usize, 2] {
                    let pbx = 0x40000 + pid * 0x2000;
                    let intr = r(pbdma::intr(pid));
                    let hce_intr = r(pbdma::hce_intr(pid));
                    let userd = r(pbx + 0xD0);
                    let gpbase = r(pbx + 0x40);
                    let gp_put = r(pbx + 0x50);
                    let gp_fetch = r(pbx + 0x48);
                    let state = r(pbx + 0xB0);
                    let gp_state = r(pbx + 0x4C);
                    eprintln!("║   P PBDMA{pid}: INTR={intr:#010x} HCE_INTR={hce_intr:#010x}");
                    eprintln!("║   P PBDMA{pid}: USERD={userd:#010x} GP_BASE={gpbase:#010x} GP_PUT={gp_put} GP_FETCH={gp_fetch} STATE={state:#010x} GP_STATE={gp_state:#010x}");
                }

                // Check non-replayable fault buffer
                let nrfb_get = r(0x100E4C);
                let nrfb_put = r(0x100E50);
                let rfb_get = r(0x100E30);
                let rfb_put = r(0x100E34);
                eprintln!("║   P FAULTBUF: NR_GET={nrfb_get:#010x} NR_PUT={nrfb_put:#010x} R_GET={rfb_get:#010x} R_PUT={rfb_put:#010x}");

                eprintln!("║   P final: PCCSR={post_db:#010x}");

                // Phase 5: Clear faults and retry doorbell
                let _ = w(pccsr::channel(channel_id),
                    pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET);
                let _ = w(pfifo::INTR, 0xFFFF_FFFF);
                let _ = w(pbdma::intr(1), 0xFFFF_FFFF);
                let _ = w(pbdma::intr(2), 0xFFFF_FFFF);
                std::thread::sleep(std::time::Duration::from_millis(50));
                let _ = w(usermode::NOTIFY_CHANNEL_PENDING, channel_id);
                std::thread::sleep(std::time::Duration::from_millis(300));
                let final_pccsr = r(pccsr::channel(channel_id));
                let final_fetch1 = r(pb + 0x48);
                let final_fetch2 = r(0x44000 + 0x48);
                let final_pfifo_intr = r(pfifo::INTR);
                eprintln!("║   P retry: PCCSR={final_pccsr:#010x} PBDMA1_FETCH={final_fetch1} PBDMA2_FETCH={final_fetch2} PFIFO_INTR={final_pfifo_intr:#010x}");
            }
            ExperimentOrdering::VramFullDispatch => {
                const PM: usize = 0x0070_0000; // PRAMIN base in BAR0
                const BW: usize = 0x0000_1700; // BAR0_WINDOW register
                const VI: usize = 0x3000;       // VRAM offset for instance block

                // Set BAR0 window → page 0 so PRAMIN maps VRAM[0..64K]
                let _ = w(BW, 0);
                std::thread::sleep(std::time::Duration::from_millis(1));

                // Copy instance block (already populated) to VRAM via PRAMIN
                let inst_bytes = instance.as_slice();
                for off in (0..inst_bytes.len()).step_by(4) {
                    let val = u32::from_le_bytes([
                        inst_bytes[off], inst_bytes[off+1],
                        inst_bytes[off+2], inst_bytes[off+3],
                    ]);
                    if val != 0 {
                        let _ = w(PM + VI + off, val);
                    }
                }

                // Keep SIGNATURE = 0xFACE (PBDMA validates this; 0xDEAD causes
                // SIGNATURE error at PBDMA INTR bit 31).
                // The ectx binding below is the real fix for CTXNOTVALID.

                // GR engine context binding (gv100_ectx_bind):
                // The PBDMA fires CTXNOTVALID (INTR=0x80000000) without this.
                // Set inst[0x0AC] bit 16 = engine context valid.
                // Set inst[0x210/0x214] = engine context GPU VA (bit 2 = valid).
                // Use PT0_IOVA as a placeholder — the PBDMA should at least
                // start fetching GPFIFO before the GR engine tries to load context.
                let _ = w(PM + VI + 0x0AC, 0x0001_0000);
                let _ = w(PM + VI + 0x210, (PT0_IOVA as u32) | 4);
                let _ = w(PM + VI + 0x214, (PT0_IOVA >> 32) as u32);

                // Verify key RAMFC fields in VRAM
                let v_sig = r(PM + VI + ramfc::SIGNATURE);
                let v_gpb = r(PM + VI + ramfc::GP_BASE_LO);
                let v_usr = r(PM + VI + ramfc::USERD_LO);
                let v_pdb = r(PM + VI + ramin::PAGE_DIR_BASE_LO);
                eprintln!("║   Q VRAM inst: SIG={v_sig:#010x} GP_BASE={v_gpb:#010x} USERD={v_usr:#010x} PDB={v_pdb:#010x}");

                // USERD in VRAM: write GP_PUT=1, GP_GET=0 at VRAM offset 0x0000
                // The PBDMA may not be able to read USERD from system memory
                // (IOMMU access issue), so put it in VRAM alongside the instance block.
                const VRAM_USERD: usize = 0x0000;
                let _ = w(PM + VRAM_USERD + ramuserd::GP_PUT, 1);
                let _ = w(PM + VRAM_USERD + ramuserd::GP_GET, 0);
                // Verify USERD in VRAM
                let vram_gp_put = r(PM + VRAM_USERD + ramuserd::GP_PUT);
                eprintln!("║   Q VRAM USERD: GP_PUT={vram_gp_put} (at 0x{:04x})", VRAM_USERD + ramuserd::GP_PUT);

                // Override RAMFC USERD to point to VRAM (target=0 for VID_MEM)
                // PBDMA_TARGET encoding: 0=VID_MEM, 1=SYS_MEM_COH, 2=SYS_MEM_NCOH
                let _ = w(PM + VI + ramfc::USERD_LO,
                    (VRAM_USERD as u32 & 0xFFFF_FE00) | 0); // target=0 VID_MEM
                let _ = w(PM + VI + ramfc::USERD_HI, 0);

                // Also prepare GPFIFO entry in host memory (still at system IOVA)
                let gp_entry: u64 = (RUNLIST_IOVA & 0xFFFF_FFFC)
                    | ((1_u64) << (32 + 10));
                gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
                // Keep host USERD updated too (for completeness)
                write_u32_le(userd_page, ramuserd::GP_PUT, 1);
                write_u32_le(userd_page, ramuserd::GP_GET, 0);
                std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

                // GV100 runlist preempt BEFORE we submit — force stale context off PBDMA
                let _ = w(0x002638, 1 << target_runlist);
                std::thread::sleep(std::time::Duration::from_millis(50));
                let preempt_pending = r(0x002284 + (target_runlist as usize) * 8);
                eprintln!("║   Q pre-preempt: pending={preempt_pending:#010x}");

                // INST_BIND with VRAM target + enable + runlist (D path)
                eprintln!("║   Q pccsr_inst_val={pccsr_inst_val:#010x} rl_base={rl_base:#010x} rl_submit={rl_submit:#010x}");
                let _ = w(pccsr::inst(channel_id), pccsr_inst_val);
                std::thread::sleep(std::time::Duration::from_millis(5));
                let post_bind = r(pccsr::channel(channel_id));
                let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
                std::thread::sleep(std::time::Duration::from_millis(2));
                let _ = w(pfifo::RUNLIST_BASE, rl_base);
                let _ = w(pfifo::RUNLIST, rl_submit);
                std::thread::sleep(std::time::Duration::from_millis(50));

                // Check if runlist update was processed (pending bit should clear)
                let rl_pending = r(0x002284 + (target_runlist as usize) * 8);
                eprintln!("║   Q runlist_pending={rl_pending:#010x} (bit20={})",
                    rl_pending & 0x00100000 != 0);

                std::thread::sleep(std::time::Duration::from_millis(100));
                let post_rl = r(pccsr::channel(channel_id));
                eprintln!("║   Q bind={post_bind:#010x} sched={post_rl:#010x} ok={}",
                    post_rl & 2 != 0);

                // Interrupt-driven runlist acknowledgment + PBDMA dispatch
                let pb2: usize = 0x44000; // PBDMA2 base

                // Phase A: Acknowledge runlist interrupt
                let pfifo_intr = r(pfifo::INTR);
                let rl_mask = r(0x002A00);
                eprintln!("║   Q ack: PFIFO_INTR={pfifo_intr:#010x} RL_MASK={rl_mask:#010x}");
                if rl_mask != 0 {
                    let _ = w(0x002A00, rl_mask);
                }
                // Clear all PFIFO + PBDMA interrupts
                let _ = w(pfifo::INTR, 0xFFFF_FFFF);
                let _ = w(pbdma::intr(1), 0xFFFF_FFFF);
                let _ = w(pbdma::intr(2), 0xFFFF_FFFF);
                let _ = w(pccsr::channel(channel_id),
                    pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET);
                std::thread::sleep(std::time::Duration::from_millis(20));

                // Phase B: Check PBDMA2 context load (RAMFC offsets)
                let p2_sig = r(pb2 + 0x010);
                let p2_userd = r(pb2 + 0x008);
                let p2_gpbase = r(pb2 + 0x048);
                let p2_chid = r(pb2 + 0x0E8);
                eprintln!("║   Q ctx: SIG={p2_sig:#010x} USERD={p2_userd:#010x} GP_BASE={p2_gpbase:#010x} CHID={p2_chid:#010x}");

                // Phase C: Ring doorbell and check for PBDMA activity
                let _ = w(usermode::NOTIFY_CHANNEL_PENDING, channel_id);
                std::thread::sleep(std::time::Duration::from_millis(100));

                // Scan PBDMA2 for ALL changes
                eprint!("║   Q pb2-post-db:");
                for off in (0x000..=0x0FF_usize).step_by(4) {
                    let val = r(pb2 + off);
                    if val != 0 {
                        eprint!(" [{off:#05x}]={val:#010x}");
                    }
                }
                eprintln!();

                let pccsr_post = r(pccsr::channel(channel_id));
                let intr_post = r(pfifo::INTR);
                let p2_intr = r(pbdma::intr(2));
                let p2_hce = r(pbdma::hce_intr(2));
                eprintln!("║   Q post-db: PCCSR={pccsr_post:#010x} PFIFO_INTR={intr_post:#010x} PBDMA2_INTR={p2_intr:#010x} HCE={p2_hce:#010x}");

                // Phase D: If PBDMA2 has CTXNOTVALID or SIGNATURE error, handle it
                if p2_intr != 0 || p2_hce != 0 {
                    let _ = w(pbdma::intr(2), 0xFFFF_FFFF);
                    let _ = w(pbdma::hce_intr(2), 0xFFFF_FFFF);
                    let _ = w(pfifo::INTR, 0xFFFF_FFFF);
                    let _ = w(pccsr::channel(channel_id),
                        pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET);
                    std::thread::sleep(std::time::Duration::from_millis(20));

                    // Second doorbell after clearing interrupts
                    let _ = w(usermode::NOTIFY_CHANNEL_PENDING, channel_id);
                    std::thread::sleep(std::time::Duration::from_millis(200));

                    eprint!("║   Q pb2-retry:");
                    for off in (0x000..=0x0FF_usize).step_by(4) {
                        let val = r(pb2 + off);
                        if val != 0 {
                            eprint!(" [{off:#05x}]={val:#010x}");
                        }
                    }
                    eprintln!();

                    let retry_intr = r(pbdma::intr(2));
                    let retry_hce = r(pbdma::hce_intr(2));
                    let retry_pccsr = r(pccsr::channel(channel_id));
                    eprintln!("║   Q retry: PCCSR={retry_pccsr:#010x} PBDMA2_INTR={retry_intr:#010x} HCE={retry_hce:#010x}");
                }

                let final_pccsr = r(pccsr::channel(channel_id));
                let final_intr = r(pfifo::INTR);
                eprintln!("║   Q final: PCCSR={final_pccsr:#010x} PFIFO_INTR={final_intr:#010x}");
            }
        }

        // Wait for hardware to process
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Capture snapshot
        let pccsr_chan = r(pccsr::channel(channel_id));
        let pccsr_inst_rb = r(pccsr::inst(channel_id));
        let cur_userd_lo = r(pb + 0xD0);
        let cur_userd_hi = r(pb + 0xD4);
        let cur_gp_base_lo = r(pb + 0x40);

        let result = ExperimentResult {
            name: cfg.name.to_string(),
            pccsr_chan,
            pccsr_inst_readback: pccsr_inst_rb,
            pbdma_userd_lo: cur_userd_lo,
            pbdma_userd_hi: cur_userd_hi,
            pbdma_gp_base_lo: cur_gp_base_lo,
            pbdma_gp_base_hi: r(pb + 0x44),
            pbdma_gp_put: r(pb + 0x54),
            pbdma_gp_fetch: r(pb + 0x48),
            pbdma_channel_state: r(pb + 0xB0),
            pbdma_signature: r(pb + 0xC0),
            pfifo_intr: r(pfifo::INTR),
            mmu_fault_status: r(0x100A2C),
            engn0_status: r(0x2640),
            faulted: (pccsr_chan >> 24) & 1 != 0 || (pccsr_chan >> 28) & 1 != 0,
            scheduled: (pccsr_chan & 2) != 0,
            pbdma_ours: cur_userd_lo != residual_userd_lo
                || cur_gp_base_lo != residual_gp_base_lo,
        };

        eprintln!("║ {}", result.summary_line());

        // Tear down
        let _ = w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_CLR);
        std::thread::sleep(std::time::Duration::from_millis(5));
        let _ = w(pccsr::channel(channel_id),
            pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET);
        std::thread::sleep(std::time::Duration::from_millis(2));
        let _ = w(pccsr::inst(channel_id), 0);
        std::thread::sleep(std::time::Duration::from_millis(5));

        // Reset GPFIFO/USERD DMA buffers for next experiment
        gpfifo_ring.iter_mut().take(16).for_each(|b| *b = 0);
        write_u32_le(userd_page, ramuserd::GP_PUT, 0);
        write_u32_le(userd_page, ramuserd::GP_GET, 0);

        runlist.as_mut_slice().fill(0);
        let empty_rl_base = (cfg.runlist_base_target << 28) | (RUNLIST_IOVA >> 12) as u32;
        let _ = w(pfifo::RUNLIST_BASE, empty_rl_base);
        let _ = w(pfifo::RUNLIST, (target_runlist << 20) | 0);
        std::thread::sleep(std::time::Duration::from_millis(10));

        results.push(result);
    }

    eprintln!("╚═══════════════════════════════════════════════════════════╝");
    Ok(results)
}

// ── V2 MMU page table encoding ─────────────────────────────────────────

/// Write a PDE entry at `index` in a page directory buffer.
///
/// V2 PDE layout: `(phys_addr >> 4) | flags` — the GPU decodes the
/// physical address as `(PDE & ~0x7) << 4`.  Matches nouveau's
/// `gp100_vmm_pd0_pde()`: `(spt->addr >> 4) | spt->type`.
fn write_pde(pd_slice: &mut [u8], index: usize, target_iova: u64) {
    let pde = encode_pde(target_iova);
    let off = index * 8;
    pd_slice[off..off + 8].copy_from_slice(&pde.to_le_bytes());
}

/// Encode a V2 PDE pointing to a page table at `iova` in system memory.
///
/// Bit layout: `[1:0]=aperture, [2]=volatile, addr in upper bits`.
/// Encoding: `(iova >> 4) | flags`.  For 4K-aligned IOVAs the low 8 bits
/// of `(iova >> 4)` are zero, so the OR with flags (bits [2:0]) is clean.
fn encode_pde(iova: u64) -> u64 {
    const FLAGS: u64 = 2 | (1 << 2); // aperture=SYS_MEM_COH(2) + VOL(bit2)
    (iova >> 4) | FLAGS
}

/// Encode a V2 small-page PTE for an identity-mapped physical address.
///
/// Bit layout: `[0]=valid, [1]=aperture(SYS_MEM_COH), [2]=volatile`.
/// Encoding matches nouveau `gp100_vmm_pgt_mem()`: `(addr >> 4) | type`.
fn encode_pte(phys_addr: u64) -> u64 {
    const FLAGS: u64 = 1 | 2 | (1 << 2); // VALID(bit0) + SYS_MEM_COH(bit1) + VOL(bit2)
    (phys_addr >> 4) | FLAGS
}

/// Write a little-endian `u32` into a byte slice at the given byte offset.
fn write_u32_le(buf: &mut [u8], offset: usize, value: u32) {
    buf[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pde_encoding_sys_mem_coherent() {
        // PDE = (0x6000 >> 4) | 6 = 0x600 | 6 = 0x606
        let pde = encode_pde(0x6000);
        assert_eq!(pde, 0x606);
        assert_eq!(pde & 0x7, 6, "flags: aperture(2) + VOL(4)");
        let addr = (pde & !0x7) << 4;
        assert_eq!(addr, 0x6000, "GPU decode: (PDE & ~0x7) << 4");
    }

    #[test]
    fn pte_encoding_identity_map() {
        // PTE = (0x1000 >> 4) | 7 = 0x100 | 7 = 0x107
        let pte = encode_pte(0x1000);
        assert_eq!(pte, 0x107);
        assert_eq!(pte & 1, 1, "valid bit");
        assert_eq!(pte & 0x7, 7, "flags: VALID(1) + SYS_MEM_COH(2) + VOL(4)");
        let addr = (pte & !0x7) << 4;
        assert_eq!(addr, 0x1000, "GPU decode: (PTE & ~0x7) << 4");
    }

    #[test]
    fn pte_encoding_higher_address() {
        // PTE = (0x10_0000 >> 4) | 7 = 0x10000 | 7 = 0x10007
        let pte = encode_pte(0x10_0000);
        assert_eq!(pte, 0x1_0007);
        let addr = (pte & !0x7) << 4;
        assert_eq!(addr, 0x10_0000);
    }

    #[test]
    fn ramuserd_offsets_match_nvidia_spec() {
        assert_eq!(ramuserd::GP_GET, 0x88);
        assert_eq!(ramuserd::GP_PUT, 0x8C);
    }

    #[test]
    fn pccsr_register_offsets() {
        assert_eq!(pccsr::inst(0), 0x80_0000);
        assert_eq!(pccsr::channel(0), 0x80_0004);
        assert_eq!(pccsr::inst(1), 0x80_0008);
        assert_eq!(pccsr::channel(1), 0x80_000C);
    }

    #[test]
    fn iova_layout_non_overlapping() {
        let iovas = [
            ("INSTANCE", INSTANCE_IOVA),
            ("RUNLIST", RUNLIST_IOVA),
            ("PD3", PD3_IOVA),
            ("PD2", PD2_IOVA),
            ("PD1", PD1_IOVA),
            ("PD0", PD0_IOVA),
            ("PT0", PT0_IOVA),
        ];
        for i in 0..iovas.len() {
            for j in (i + 1)..iovas.len() {
                assert_ne!(
                    iovas[i].1, iovas[j].1,
                    "{} and {} overlap at {:#x}",
                    iovas[i].0, iovas[j].0, iovas[i].1
                );
            }
        }
    }

    #[test]
    fn iova_layout_after_userd() {
        assert!(INSTANCE_IOVA > 0x2000, "instance after USERD");
        assert!(
            PT0_IOVA + 4096 <= 0x10_0000,
            "page tables before USER_IOVA_BASE"
        );
    }

    #[test]
    fn channel_info_constants() {
        assert_eq!(VfioChannel::doorbell_offset(), 0x81_0090);
    }

    #[test]
    fn pccsr_inst_value_channel_zero() {
        let value = (INSTANCE_IOVA >> 12) as u32
            | pccsr::INST_TARGET_SYS_MEM_NCOH;
        assert_eq!(value & 0x0FFF_FFFF, 3, "INST_PTR = 3 (0x3000 >> 12)");
        assert_eq!((value >> 28) & 3, 3, "target = SYS_MEM_NCOH");
        assert_eq!((value >> 31) & 1, 0, "BIND not set — implicit via runlist");
    }

    #[test]
    fn runlist_base_value() {
        let rl_base = (RUNLIST_IOVA >> 12) as u32 | (3_u32 << 28);
        assert_eq!(rl_base & 0x0FFF_FFFF, 4, "PTR = 4 (0x4000 >> 12)");
        assert_eq!((rl_base >> 28) & 3, 3, "target = SYS_MEM_NCOH");
    }

    #[test]
    fn runlist_chan_entry_encoding() {
        let userd: u64 = 0x2000;

        // DW0: USERD_ADDR | USERD_TARGET(COH=2) | RUNQ | TYPE=0
        let dw0 = userd as u32 | (TARGET_SYS_MEM_COHERENT << 2) | (0 << 1);
        assert_eq!(dw0, 0x2008, "USERD=0x2000, target=COH(2), runq=0");
        assert_eq!((dw0 >> 2) & 3, 2, "USERD_TARGET = SYS_MEM_COH");
        assert_eq!(dw0 & 1, 0, "TYPE = 0 (channel)");

        let dw0_runq1 = userd as u32 | (TARGET_SYS_MEM_COHERENT << 2) | (1 << 1);
        assert_eq!(dw0_runq1, 0x200A, "USERD=0x2000, target=COH(2), runq=1");
        assert_eq!((dw0_runq1 >> 1) & 1, 1, "RUNQUEUE = 1");

        // DW2: INST_ADDR | INST_TARGET(NCOH=3) | CHID
        let inst: u64 = 0x3000;
        let chid: u32 = 0;
        let dw2 = inst as u32 | (TARGET_SYS_MEM_NONCOHERENT << 4) | chid;
        assert_eq!(dw2, 0x3030, "INST=0x3000, target=NCOH(3), chid=0");
        assert_eq!((dw2 >> 4) & 3, 3, "INST_TARGET = SYS_MEM_NCOH");
    }

    #[test]
    fn write_u32_le_roundtrip() {
        let mut buf = [0u8; 8];
        write_u32_le(&mut buf, 4, 0xDEAD_BEEF);
        let val = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
        assert_eq!(val, 0xDEAD_BEEF);
    }
}
