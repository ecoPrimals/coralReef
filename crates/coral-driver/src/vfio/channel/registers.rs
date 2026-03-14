// SPDX-License-Identifier: AGPL-3.0-only
//! BAR0 register offsets and IOVA constants for Volta+ PFIFO channels.
//!
//! Register sources:
//! - NVIDIA open-gpu-doc `dev_fifo.ref.txt` (PCCSR, PFIFO runlist)
//! - NVIDIA open-gpu-doc `dev_ram.ref.txt` (RAMFC, RAMUSERD, RAMRL, RAMIN)
//! - NVIDIA open-gpu-doc `dev_usermode.ref.txt` (doorbell)
//! - nouveau `nvkm/engine/fifo/gv100.c`

// ── BAR0 register offsets ──────────────────────────────────────────────

/// `NV_PFIFO` registers (BAR0 + 0x2000..0x3FFF).
pub(crate) mod pfifo {
    /// PFIFO engine enable — toggle 0→1 to reset scheduler + PBDMAs.
    pub const ENABLE: usize = 0x0000_2200;
    /// FIFO scheduler enable (1 = enabled).
    pub const SCHED_EN: usize = 0x0000_2504;
    /// Scheduler disable (0 = scheduler runs). Read to verify scheduler state.
    pub const SCHED_DISABLE: usize = 0x0000_2630;
    /// PFIFO interrupt status — write `0xFFFF_FFFF` to clear pending.
    pub const INTR: usize = 0x0000_2100;
    /// PFIFO interrupt enable mask.
    pub const INTR_EN: usize = 0x0000_2140;
    /// PBDMA active map — bit N = 1 means PBDMA N exists.
    pub const PBDMA_MAP: usize = 0x0000_2004;
    /// PBDMA-to-runlist mapping table. Entry at `+seq*4` for each active PBDMA.
    pub const PBDMA_RUNL_MAP: usize = 0x0000_2390;
    /// GV100 runlist base address LO (addr >> 12, no target bits).
    /// Per-runlist registers at stride 16: 0x2270 + runlist_id * 16.
    pub const RUNLIST_BASE_LO: usize = 0x0000_2270;
    /// GV100 runlist base address HI (upper addr | aperture flag).
    /// Nouveau uses `| 0x2` for VRAM aperture.
    pub const RUNLIST_BASE_HI: usize = 0x0000_2274;
    /// GV100 runlist submit: (count << 16) | start_offset.
    pub const RUNLIST_SUBMIT: usize = 0x0000_2278;
    /// Runlist pending status. Per-runlist at stride 8.
    pub const RUNLIST_PENDING: usize = 0x0000_2284;
    /// Preempt trigger (channel or runlist).
    pub const PREEMPT: usize = 0x0000_2634;
    /// GV100 runlist-level preempt — write bitmask of runlist IDs.
    pub const GV100_PREEMPT: usize = 0x0000_2638;
    /// Runlist interrupt acknowledge mask.
    pub const RUNLIST_ACK: usize = 0x0000_2A00;
    /// BIND_ERROR status.
    pub const BIND_ERROR: usize = 0x0000_252C;
    /// FB timeout counter.
    pub const FB_TIMEOUT: usize = 0x0000_2254;
    /// Engine status, per-engine at stride 4.
    pub const ENGN_STATUS: usize = 0x0000_2640;
    /// Engine topology table at stride 4 (GV100).
    pub const ENGN_TABLE: usize = 0x0002_2700;
}

/// `NV_PMC` — Power Management Controller / engine enables.
pub(crate) mod pmc {
    /// Master engine enable — write `0xFFFF_FFFF` to un-gate all clock domains.
    /// After readback, bits that remained 1 correspond to present engines.
    pub const ENABLE: usize = 0x0000_0200;
    /// PBDMA master enable — set bit N to enable PBDMA N.
    #[allow(dead_code, reason = "kept for hardware documentation completeness")]
    pub const PBDMA_ENABLE: usize = 0x0000_0204;
    /// PBDMA interrupt routing.
    #[allow(dead_code, reason = "kept for hardware documentation completeness")]
    pub const PBDMA_INTR_EN: usize = 0x0000_2A04;
}

/// Per-PBDMA registers (stride 0x2000, base 0x04_0000).
pub(crate) mod pbdma {
    pub const BASE: usize = 0x0004_0000;
    pub const STRIDE: usize = 0x2000;

    /// Base address for a specific PBDMA in BAR0.
    pub const fn base(id: usize) -> usize {
        BASE + id * STRIDE
    }

    const fn reg(id: usize, off: usize) -> usize {
        BASE + id * STRIDE + off
    }

    /// PBDMA interrupt status — write `0xFFFF_FFFF` to clear.
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

    // RAMFC-mapped PBDMA register offsets (context save/restore area).
    // These mirror the RAMFC layout — the scheduler loads RAMFC fields HERE.
    pub const CTX_USERD_LO: usize = 0x008;
    pub const CTX_USERD_HI: usize = 0x00C;
    pub const CTX_SIGNATURE: usize = 0x010;
    pub const CTX_ACQUIRE: usize = 0x030;
    pub const CTX_GP_BASE_LO: usize = 0x048;
    pub const CTX_GP_BASE_HI: usize = 0x04C;
    pub const CTX_GP_PUT: usize = 0x054;
    pub const CTX_GP_FETCH: usize = 0x058;

    // Direct PBDMA programming offsets (operational registers).
    // These may be separate from the context area.
    pub const GP_BASE_LO: usize = 0x040;
    pub const GP_BASE_HI: usize = 0x044;
    pub const GP_FETCH: usize = 0x048;
    pub const GP_STATE: usize = 0x04C;
    pub const GP_PUT: usize = 0x054;
    pub const USERD_LO: usize = 0x0D0;
    pub const USERD_HI: usize = 0x0D4;
    pub const CONFIG: usize = 0x0A8;
    pub const CHANNEL_INFO: usize = 0x0AC;
    pub const CHANNEL_STATE: usize = 0x0B0;
    pub const SIGNATURE: usize = 0x0C0;
}

/// MMU fault buffer registers (BAR0 + 0x10_0E00).
pub(crate) mod mmu {
    pub const FAULT_BUF0_LO: usize = 0x0010_0E24;
    pub const FAULT_BUF0_HI: usize = 0x0010_0E28;
    pub const FAULT_BUF0_SIZE: usize = 0x0010_0E2C;
    pub const FAULT_BUF0_GET: usize = 0x0010_0E30;
    pub const FAULT_BUF0_PUT: usize = 0x0010_0E34;
    pub const FAULT_BUF1_LO: usize = 0x0010_0E44;
    pub const FAULT_BUF1_HI: usize = 0x0010_0E48;
    pub const FAULT_BUF1_SIZE: usize = 0x0010_0E4C;
    pub const FAULT_BUF1_GET: usize = 0x0010_0E50;
    pub const FAULT_BUF1_PUT: usize = 0x0010_0E54;
    pub const FAULT_STATUS: usize = 0x0010_0A2C;
    pub const FAULT_ADDR_LO: usize = 0x0010_0A30;
    pub const FAULT_ADDR_HI: usize = 0x0010_0A34;
    pub const FAULT_INST_LO: usize = 0x0010_0A38;
    pub const FAULT_INST_HI: usize = 0x0010_0A3C;
}

/// Miscellaneous BAR0 registers.
pub(crate) mod misc {
    pub const BOOT0: usize = 0x0000_0000;
    pub const PRIV_RING: usize = 0x0001_2070;
    pub const PRAMIN_BASE: usize = 0x0070_0000;
    pub const BAR0_WINDOW: usize = 0x0000_1700;
    pub const L2_FLUSH: usize = 0x0007_0010;
}

/// `NV_PCCSR` — per-channel control/status registers (BAR0 + 0x80_0000).
pub(crate) mod pccsr {
    /// Instance block pointer register for channel `id`.
    /// Contains `INST_PTR[27:0]`, `INST_TARGET[29:28]`, `INST_BIND[31]`.
    pub const fn inst(id: u32) -> usize {
        0x0080_0000 + (id as usize) * 8
    }

    /// Channel control register for channel `id`.
    /// Contains `ENABLE[0]`, `ENABLE_SET[10]`, `ENABLE_CLR[11]`, `STATUS[27:24]`.
    pub const fn channel(id: u32) -> usize {
        0x0080_0004 + (id as usize) * 8
    }

    /// `INST_TARGET` = `SYS_MEM_NONCOHERENT` (bits [29:28] = 3).
    pub const INST_TARGET_SYS_MEM_NCOH: u32 = 3 << 28;
    /// `INST_BIND` = TRUE (bit 31).
    pub const INST_BIND_TRUE: u32 = 1 << 31;
    /// `CHANNEL_ENABLE_SET` trigger (bit 10).
    pub const CHANNEL_ENABLE_SET: u32 = 1 << 10;
    /// `CHANNEL_ENABLE_CLR` trigger (bit 11).
    pub const CHANNEL_ENABLE_CLR: u32 = 1 << 11;
    /// `PBDMA_FAULTED` — write 1 to clear (bit 24).
    pub const PBDMA_FAULTED_RESET: u32 = 1 << 24;
    /// `ENG_FAULTED` — write 1 to clear (bit 28).
    pub const ENG_FAULTED_RESET: u32 = 1 << 28;
}

/// `NV_USERMODE` doorbell (BAR0 + 0x81_0000..0x81_FFFF).
pub(crate) mod usermode {
    /// Write channel ID here to notify Host that a channel has new work.
    pub const NOTIFY_CHANNEL_PENDING: usize = 0x0081_0090;
}

/// Volta RAMUSERD (User-Driver State Descriptor) offsets within a 512-byte
/// channel USERD page. From NVIDIA `dev_ram.ref.txt`.
pub mod ramuserd {
    /// `GP_GET`: next GP entry the GPU will process (GPU writes, host reads).
    pub const GP_GET: usize = 34 * 4;
    /// `GP_PUT`: next GP entry available for GPU (host writes, GPU reads).
    pub const GP_PUT: usize = 35 * 4;
}

/// Offsets within the RAMFC region of the instance block.
/// Derived from `gv100_chan_ramfc_write()` and `dev_ram.ref.txt`.
pub(super) mod ramfc {
    /// `NV_RAMFC_USERD` (dword 2) — USERD base address low + aperture target.
    pub const USERD_LO: usize = 0x008;
    /// `NV_RAMFC_USERD_HI` (dword 3) — USERD base address high.
    pub const USERD_HI: usize = 0x00C;
    /// `NV_RAMFC_SIGNATURE` (dword 4) — channel signature (0xFACE).
    pub const SIGNATURE: usize = 0x010;
    /// `NV_RAMFC_ACQUIRE` (dword 12) — semaphore acquire timeout.
    pub const ACQUIRE: usize = 0x030;
    /// `NV_RAMFC_GP_BASE` (dword 18) — GPFIFO ring GPU VA low.
    pub const GP_BASE_LO: usize = 0x048;
    /// `NV_RAMFC_GP_BASE_HI` (dword 19) — GPFIFO ring GPU VA high + limit.
    pub const GP_BASE_HI: usize = 0x04C;
    /// `NV_RAMFC_PB_HEADER` (dword 33).
    pub const PB_HEADER: usize = 0x084;
    /// `NV_RAMFC_SUBDEVICE` (dword 37) — subdevice mask.
    pub const SUBDEVICE: usize = 0x094;
    /// `NV_RAMFC_HCE_CTRL` (dword 57) — host compute engine control.
    pub const HCE_CTRL: usize = 0x0E4;
    /// Channel ID (Volta-specific, dword 58).
    pub const CHID: usize = 0x0E8;
    /// `NV_RAMFC_CONFIG` (dword 61) — PBDMA configuration.
    pub const CONFIG: usize = 0x0F4;
    /// Volta-specific channel info (dword 62).
    pub const CHANNEL_INFO: usize = 0x0F8;
}

/// `NV_RAMIN` offsets beyond RAMFC for MMU page directory configuration.
pub(super) mod ramin {
    /// Page directory base — low word (DW128, offset 0x200).
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

/// Instance block DMA buffer IOVA.
pub(super) const INSTANCE_IOVA: u64 = 0x3000;
/// Runlist DMA buffer IOVA.
pub(super) const RUNLIST_IOVA: u64 = 0x4000;
/// PD3 (level-4 page directory) IOVA.
pub(super) const PD3_IOVA: u64 = 0x5000;
/// PD2 (level-3 page directory) IOVA.
pub(super) const PD2_IOVA: u64 = 0x6000;
/// PD1 (level-2 page directory) IOVA.
pub(super) const PD1_IOVA: u64 = 0x7000;
/// PD0 (level-1 page directory) IOVA.
pub(super) const PD0_IOVA: u64 = 0x8000;
/// PT0 (page table) IOVA.
pub(super) const PT0_IOVA: u64 = 0x9000;

/// `SYS_MEM_COHERENT` aperture target for PCCSR/PFIFO/RAMIN/runlist registers.
pub(super) const TARGET_SYS_MEM_COHERENT: u32 = 2;

/// `SYS_MEM_NONCOHERENT` aperture target (PCCSR/RAMIN/runlist encoding).
pub(super) const TARGET_SYS_MEM_NONCOHERENT: u32 = 3;

/// `SYS_MEM_COHERENT` aperture target for PBDMA registers (RAMFC fields).
/// NV_PPBDMA_USERD_TARGET[1:0]: 0=VID_MEM, 1=SYS_MEM_COHERENT, 2=SYS_MEM_NONCOHERENT.
/// These differ from the PCCSR/RAMIN encoding.
pub(super) const PBDMA_TARGET_SYS_MEM_COHERENT: u32 = 1;

/// Number of 4 KiB pages identity-mapped in PT0.
pub(super) const PT_ENTRIES: usize = 512;
