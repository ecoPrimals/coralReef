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

/// SYS_MEM_COHERENT aperture target for PCCSR/PFIFO/RAMIN registers.
/// PCCSR_INST_TARGET[29:28]: 0=VRAM, 2=COHERENT_SYSMEM, 3=NONCOHERENT_SYSMEM
/// RUNLIST_BASE_TARGET[31:28]: same encoding as PCCSR.
const TARGET_SYS_MEM_COHERENT: u32 = 2;

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

        // ── Step 1: Toggle PFIFO enable (gv100_fifo_init) ───────────────
        // This resets the PFIFO scheduler + all PBDMAs.
        w(pfifo::ENABLE, 0)?;
        std::thread::sleep(std::time::Duration::from_millis(2));
        w(pfifo::ENABLE, 1)?;
        std::thread::sleep(std::time::Duration::from_millis(2));

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

        // ── Step 3: Per-PBDMA interrupt init (gk104_fifo_init_pbdmas) ───
        for id in 0..32_usize {
            if pbdma_map & (1 << id) == 0 {
                continue;
            }
            w(pbdma::intr(id), 0xFFFF_FFFF)?;
            w(pbdma::intr_en(id), 0xFFFF_FEFF)?;
            w(pbdma::hce_intr(id), 0xFFFF_FFFF)?;
            w(pbdma::hce_intr_en(id), 0xFFFF_FFFF)?;
        }

        // ── Step 4: Clear + enable PFIFO interrupts ──────────────────────
        w(pfifo::INTR, 0xFFFF_FFFF)?;
        w(pfifo::INTR_EN, 0x7FFF_FFFF)?;

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
        write_u32_le(inst, ramfc::GP_BASE_LO, gpfifo_iova as u32);
        // GP_BASE_HI: addr_hi[7:0] | limit2[20:16] | aperture[29:28]
        // aperture=1 (SYS_MEM_COH) so the PBDMA reads GPFIFO from system memory.
        write_u32_le(
            inst,
            ramfc::GP_BASE_HI,
            (gpfifo_iova >> 32) as u32 | (limit2 << 16) | (PBDMA_TARGET_SYS_MEM_COHERENT << 28),
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
        // GV100 RAMRL channel entry format:
        //   DW0: USERD_PTR[31:9] | USERD_TARGET[3:2] | RUNQUEUE[1] | TYPE[0]=0
        //   DW1: USERD_PTR[63:32]
        //   DW2: INST_PTR[31:12] | INST_TARGET[5:4] | CHID[11:0]
        //   DW3: INST_PTR[63:32]
        //
        // TARGET encoding: 0=VID_MEM, 2=SYS_MEM_COH, 3=SYS_MEM_NCOH
        write_u32_le(rl, 0x10, userd_iova as u32 | (runq << 1));
        write_u32_le(rl, 0x14, (userd_iova >> 32) as u32);
        // DW2: INST_ADDR | CHID (no target bits — PCCSR_INST carries the aperture).
        write_u32_le(rl, 0x18, INSTANCE_IOVA as u32 | self.channel_id);
        write_u32_le(rl, 0x1C, (INSTANCE_IOVA >> 32) as u32);
    }

    // ── BAR0 register programming ─────────────────────────────────────

    /// Bind the channel's instance block to PCCSR.
    fn bind_channel(&self, bar0: &MappedBar) -> DriverResult<()> {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "INSTANCE_IOVA >> 12 fits u32 for our allocation range"
        )]
        let value = (INSTANCE_IOVA >> 12) as u32
            | pccsr::INST_TARGET_SYS_MEM_NCOH
            | pccsr::INST_BIND_TRUE;

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
            | pccsr::INST_TARGET_SYS_MEM_NCOH
            | pccsr::INST_BIND_TRUE;
        assert_eq!(value & 0x0FFF_FFFF, 3, "INST_PTR = 3 (0x3000 >> 12)");
        assert_eq!((value >> 28) & 3, 3, "target = SYS_MEM_NCOH");
        assert_eq!((value >> 31) & 1, 1, "BIND = TRUE");
    }

    #[test]
    fn runlist_base_value() {
        let rl_base = (RUNLIST_IOVA >> 12) as u32 | (3_u32 << 28);
        assert_eq!(rl_base & 0x0FFF_FFFF, 4, "PTR = 4 (0x4000 >> 12)");
        assert_eq!((rl_base >> 28) & 3, 3, "target = SYS_MEM_NCOH");
    }

    #[test]
    fn runlist_chan_entry_matches_nouveau() {
        let userd: u64 = 0x2000;
        // nouveau: lower_32_bits(user) | (runq << 1)
        let dw0_runq0 = userd as u32 | (0 << 1);
        assert_eq!(dw0_runq0, 0x2000, "runq=0");
        let dw0_runq1 = userd as u32 | (1 << 1);
        assert_eq!(dw0_runq1, 0x2002, "runq=1: bit 1 set");
        assert_eq!(dw0_runq1 & 1, 0, "TYPE = 0 (channel)");
    }

    #[test]
    fn write_u32_le_roundtrip() {
        let mut buf = [0u8; 8];
        write_u32_le(&mut buf, 4, 0xDEAD_BEEF);
        let val = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
        assert_eq!(val, 0xDEAD_BEEF);
    }
}
