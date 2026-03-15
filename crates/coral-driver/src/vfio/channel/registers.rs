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
    /// Channel switch error detail (REQ_TIMEOUT, ACK_TIMEOUT, etc.).
    /// Read after PFIFO_INTR bit 16 (CHSW_ERROR) fires.
    pub const CHSW_ERROR: usize = 0x0000_256C;
    /// PFIFO interrupt status — write `0xFFFF_FFFF` to clear pending.
    pub const INTR: usize = 0x0000_2100;
    /// PFIFO interrupt enable mask.
    pub const INTR_EN: usize = 0x0000_2140;
    /// PFIFO_INTR bit 30 — runlist update completion event.
    pub const INTR_RL_COMPLETE: u32 = 0x4000_0000;
    /// PFIFO_INTR bit 16 — channel switch error.
    pub const INTR_CHSW_ERROR: u32 = 0x0001_0000;
    /// PFIFO_INTR bit 29 — aggregate "any PBDMA has an interrupt pending".
    pub const INTR_PBDMA: u32 = 0x2000_0000;
    /// PBDMA active map — bit N = 1 means PBDMA N exists.
    pub const PBDMA_MAP: usize = 0x0000_2004;
    #[allow(dead_code, reason = "diagnostic matrix migration in progress")]
    /// PBDMA-to-runlist mapping table. Entry at `+seq*4` for each active PBDMA.
    pub const PBDMA_RUNL_MAP: usize = 0x0000_2390;
    /// GK104/GV100 runlist base address (global, NOT per-runlist strided).
    /// Format: `(target << 28) | (addr >> 12)`.
    /// TARGET[31:28] = aperture (0=VID_MEM, 3=SYS_MEM_NCOH).
    /// PTR[27:0] = physical/IOVA page number.
    /// Source: nouveau `gk104_runl_commit()`.
    pub const RUNLIST_BASE: usize = 0x0000_2270;
    /// GK104/GV100 runlist submit trigger (global, NOT per-runlist strided).
    /// Format: `(runlist_id << 20) | count`.
    /// Writing this register triggers the scheduler to process the runlist.
    /// Source: nouveau `gk104_runl_commit()`.
    pub const RUNLIST_SUBMIT: usize = 0x0000_2274;
    #[allow(dead_code, reason = "diagnostic matrix migration in progress")]
    /// Runlist pending status. Per-runlist at stride 8.
    pub const RUNLIST_PENDING: usize = 0x0000_2284;
    #[allow(dead_code, reason = "diagnostic matrix migration in progress")]
    /// Preempt trigger (channel or runlist).
    pub const PREEMPT: usize = 0x0000_2634;
    #[allow(dead_code, reason = "diagnostic matrix migration in progress")]
    /// GV100 runlist-level preempt — write bitmask of runlist IDs.
    pub const GV100_PREEMPT: usize = 0x0000_2638;
    #[allow(dead_code, reason = "diagnostic matrix migration in progress")]
    /// Runlist interrupt acknowledge mask.
    pub const RUNLIST_ACK: usize = 0x0000_2A00;
    #[allow(dead_code, reason = "diagnostic matrix migration in progress")]
    /// BIND_ERROR status.
    pub const BIND_ERROR: usize = 0x0000_252C;
    #[allow(dead_code, reason = "diagnostic matrix migration in progress")]
    /// FB timeout counter.
    pub const FB_TIMEOUT: usize = 0x0000_2254;
    #[allow(dead_code, reason = "diagnostic matrix migration in progress")]
    /// Engine status, per-engine at stride 4.
    pub const ENGN_STATUS: usize = 0x0000_2640;
    #[allow(dead_code, reason = "diagnostic matrix migration in progress")]
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
    #[allow(
        dead_code,
        reason = "will be used when diagnostic migrates from inline computation"
    )]
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

    /// PBDMA METHOD0 — method address that caused a fault (offset within PBDMA).
    pub const METHOD0: usize = 0x1C0;
    /// PBDMA DATA0 — method data payload for a faulted method.
    pub const DATA0: usize = 0x1C4;

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
#[allow(dead_code, reason = "diagnostic matrix migration in progress")]
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
#[allow(
    dead_code,
    reason = "used by diagnostic matrix; will migrate inline magic numbers"
)]
pub(crate) mod misc {
    pub const BOOT0: usize = 0x0000_0000;
    pub const PRIV_RING: usize = 0x0001_2070;
    pub const PRAMIN_BASE: usize = 0x0070_0000;
    pub const BAR0_WINDOW: usize = 0x0000_1700;
    pub const L2_FLUSH: usize = 0x0007_0010;
    /// NV_PBUS_BAR1_BLOCK — BAR1 aperture instance block pointer.
    /// Format: PTR[27:0] | TARGET[29:28] | MODE[31] (0=PHYS, 1=VIRTUAL).
    /// Per `dev_bus.ref.txt`: 0x1704.
    pub const PBUS_BAR1_BLOCK: usize = 0x0000_1704;
    /// NV_PBUS_BIND_STATUS — pending/outstanding bits for BAR1/BAR2 bind ops.
    /// [0] BAR1_PENDING, [1] BAR1_OUTSTANDING, [2] BAR2_PENDING, [3] BAR2_OUTSTANDING.
    pub const PBUS_BIND_STATUS: usize = 0x0000_1710;
    /// NV_PBUS_BAR2_BLOCK — BAR2 aperture instance block pointer.
    /// Format: PTR[27:0] | TARGET[29:28] | MODE[31] (0=PHYS, 1=VIRTUAL).
    /// Per `dev_bus.ref.txt`: 0x1714. PFIFO scheduler requires this configured.
    pub const PBUS_BAR2_BLOCK: usize = 0x0000_1714;
}

/// `NV_PCCSR` — per-channel control/status registers (BAR0 + 0x80_0000).
pub(crate) mod pccsr {
    /// Instance block pointer register for channel `id`.
    /// Contains `INST_PTR[27:0]`, `INST_TARGET[29:28]`, `INST_BIND[31]`.
    pub const fn inst(id: u32) -> usize {
        0x0080_0000 + (id as usize) * 8
    }

    /// Channel control/status register for channel `id`.
    /// Layout (from open-gpu-doc dev_fifo.ref.txt):
    ///   [0]     ENABLE (R)
    ///   [1]     NEXT (RW) — scheduled on runlist
    ///   [10]    ENABLE_SET (W)
    ///   [11]    ENABLE_CLR (W)
    ///   [22]    PBDMA_FAULTED (RW1C)
    ///   [23]    ENG_FAULTED (RW1C)
    ///   [27:24] STATUS — 0=IDLE, 5=ON_PBDMA, 6=ON_PBDMA_AND_ENG, 7=ON_ENG
    ///   [28]    BUSY
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
    /// `PBDMA_FAULTED` — read to check, write 1 to clear (bit 22).
    pub const PBDMA_FAULTED_RESET: u32 = 1 << 22;
    /// `ENG_FAULTED` — read to check, write 1 to clear (bit 23).
    pub const ENG_FAULTED_RESET: u32 = 1 << 23;

    /// Decode STATUS[27:24] from a PCCSR channel register value.
    pub const fn status(val: u32) -> u32 {
        (val >> 24) & 0xF
    }

    /// Status name for display.
    pub fn status_name(val: u32) -> &'static str {
        match status(val) {
            0x0 => "IDLE",
            0x1 => "PENDING",
            0x2 => "PEND_CTX_RELOAD",
            0x3 => "PEND_ACQUIRE",
            0x5 => "ON_PBDMA",
            0x6 => "ON_PBDMA+ENG",
            0x7 => "ON_ENG",
            0x8 => "ENG_PEND_ACQ",
            0x9 => "ENG_PENDING",
            _ => "UNKNOWN",
        }
    }
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
/// Non-replayable MMU fault buffer IOVA.
pub(super) const FAULT_BUF_IOVA: u64 = 0xA000;
/// NOP push buffer IOVA — dedicated buffer with valid NOP GPU methods.
pub(super) const NOP_PB_IOVA: u64 = 0xB000;

/// `SYS_MEM_COHERENT` aperture target for PCCSR/PFIFO/RAMIN/runlist registers.
pub(super) const TARGET_SYS_MEM_COHERENT: u32 = 2;

/// `SYS_MEM_NONCOHERENT` aperture target (PCCSR/RAMIN/runlist encoding).
pub(super) const TARGET_SYS_MEM_NONCOHERENT: u32 = 3;

/// `SYS_MEM_COHERENT` aperture target for PBDMA registers (RAMFC fields).
/// RAMFC + PBDMA USERD target encoding (bits [1:0]), same as PCCSR/RAMIN:
///   0 = VID_MEM, 2 = SYS_MEM_COH, 3 = SYS_MEM_NCOH
pub(super) const PBDMA_TARGET_SYS_MEM_COHERENT: u32 = 2;

/// Number of 4 KiB pages identity-mapped in PT0.
pub(super) const PT_ENTRIES: usize = 512;

// ── BAR2 VRAM page table layout (for glow plug self-warm) ──────────────
// All structures live in VRAM, written via PRAMIN.  BAR0_WINDOW is set to
// (BAR2_VRAM_BASE >> 16) so PRAMIN offsets start at 0.
//
// VRAM layout (6 × 4KB pages, 24KB total):
//   0x20000  Instance block   (PDB + GV100 subcontexts)
//   0x21000  PD3 root         (4 entries × 8 bytes)
//   0x22000  PD2              (512 entries × 8 bytes)
//   0x23000  PD1              (512 entries × 8 bytes)
//   0x24000  PD0              (256 dual entries × 16 bytes)
//   0x25000  SPT              (512 PTEs × 8 bytes → maps first 2MB)

/// Base VRAM offset for BAR2 page tables.
pub(super) const BAR2_VRAM_BASE: u32 = 0x0002_0000;
/// VRAM offset of the BAR2 instance block.
pub(super) const BAR2_INST_OFF: u32 = 0x0000;
/// VRAM offset of PD3 root (relative to BAR2_VRAM_BASE).
pub(super) const BAR2_PD3_OFF: u32 = 0x1000;
/// VRAM offset of PD2 (relative to BAR2_VRAM_BASE).
pub(super) const BAR2_PD2_OFF: u32 = 0x2000;
/// VRAM offset of PD1 (relative to BAR2_VRAM_BASE).
pub(super) const BAR2_PD1_OFF: u32 = 0x3000;
/// VRAM offset of PD0 (relative to BAR2_VRAM_BASE).
pub(super) const BAR2_PD0_OFF: u32 = 0x4000;
/// VRAM offset of SPT (small page table, relative to BAR2_VRAM_BASE).
pub(super) const BAR2_SPT_OFF: u32 = 0x5000;
