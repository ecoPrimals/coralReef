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
    /// PFIFO_INTR bit 8 — fires after runlist submit on GV100 (undocumented).
    /// Must be cleared before retrying dispatch or the scheduler stalls.
    pub const INTR_BIT8: u32 = 0x0000_0100;
    /// PFIFO_INTR bit 16 — channel switch error.
    pub const INTR_CHSW_ERROR: u32 = 0x0001_0000;
    /// PFIFO_INTR bit 29 — aggregate "any PBDMA has an interrupt pending".
    pub const INTR_PBDMA: u32 = 0x2000_0000;
    /// PFIFO_INTR bit 30 — runlist update completion event.
    pub const INTR_RL_COMPLETE: u32 = 0x4000_0000;
    /// PBDMA active map — bit N = 1 means PBDMA N exists.
    pub const PBDMA_MAP: usize = 0x0000_2004;
    #[expect(dead_code, reason = "diagnostic matrix migration in progress")]
    /// PBDMA-to-runlist mapping table. Entry at `+seq*4` for each active PBDMA.
    pub const PBDMA_RUNL_MAP: usize = 0x0000_2390;
    /// GK104 runlist base address (global pair — gk104 only, NOT Volta).
    ///
    /// **GV100+ uses per-runlist registers** at stride 0x10; prefer
    /// [`runlist_base`] and [`runlist_submit`] for Volta.
    #[expect(
        dead_code,
        reason = "hardware register map — used as reference during bring-up"
    )]
    pub const RUNLIST_BASE: usize = 0x0000_2270;
    /// GK104 runlist submit trigger (global — gk104 only).
    #[expect(
        dead_code,
        reason = "hardware register map — used as reference during bring-up"
    )]
    pub const RUNLIST_SUBMIT: usize = 0x0000_2274;

    /// GV100 per-runlist base register (stride 0x10).
    ///
    /// Value: `lower_32(phys_or_iova >> 12)`.
    /// Source: nouveau `gv100_runl_commit()`.
    pub const fn runlist_base(id: u32) -> usize {
        0x0000_2270 + (id as usize) * 0x10
    }

    /// GV100 per-runlist submit register (stride 0x10).
    ///
    /// Value: `upper_32(phys_or_iova >> 12) | (entry_count << 16)`.
    /// Writing triggers the scheduler to process the runlist.
    /// Source: nouveau `gv100_runl_commit()`.
    pub const fn runlist_submit(id: u32) -> usize {
        0x0000_2274 + (id as usize) * 0x10
    }

    /// Encode GV100 runlist BASE register value.
    #[must_use]
    pub const fn gv100_runlist_base_value(iova: u64) -> u32 {
        (iova >> 12) as u32
    }

    /// Encode GV100 runlist SUBMIT register value.
    #[must_use]
    pub const fn gv100_runlist_submit_value(iova: u64, entry_count: u32) -> u32 {
        ((iova >> 44) as u32) | (entry_count << 16)
    }
    #[expect(dead_code, reason = "diagnostic matrix migration in progress")]
    /// Runlist pending status. Per-runlist at stride 8.
    pub const RUNLIST_PENDING: usize = 0x0000_2284;
    #[expect(dead_code, reason = "diagnostic matrix migration in progress")]
    /// Preempt trigger (channel or runlist).
    pub const PREEMPT: usize = 0x0000_2634;
    /// GV100 runlist-level preempt — write bitmask of runlist IDs.
    pub const GV100_PREEMPT: usize = 0x0000_2638;
    /// Runlist interrupt acknowledge mask.
    pub const RUNLIST_ACK: usize = 0x0000_2A00;
    #[expect(dead_code, reason = "diagnostic matrix migration in progress")]
    /// BIND_ERROR status.
    pub const BIND_ERROR: usize = 0x0000_252C;
    #[expect(dead_code, reason = "diagnostic matrix migration in progress")]
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
    /// GP100+ device-level engine enable. NOT present on GV100 (Titan V)
    /// — returns 0xBAD00200 (PBUS timeout). Do not use for Volta targets.
    #[expect(dead_code, reason = "present on GP102+/TU10x, not GV100")]
    pub const DEVICE_ENABLE: usize = 0x0000_0600;
    /// PBDMA master enable — set bit N to enable PBDMA N.
    #[expect(dead_code, reason = "kept for hardware documentation completeness")]
    pub const PBDMA_ENABLE: usize = 0x0000_0204;
    /// PBDMA interrupt routing.
    #[expect(dead_code, reason = "kept for hardware documentation completeness")]
    pub const PBDMA_INTR_EN: usize = 0x0000_2A04;
}

/// Per-PBDMA registers (stride 0x2000, base 0x04_0000).
pub(crate) mod pbdma {
    pub const BASE: usize = 0x0004_0000;
    pub const STRIDE: usize = 0x2000;

    /// Base address for a specific PBDMA in BAR0.
    #[expect(dead_code, reason = "hardware register map — used by diagnostic tests")]
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

    // RAMFC-mapped PBDMA context register offsets (context save/restore area).
    // These mirror the RAMFC layout — the scheduler loads RAMFC fields HERE.
    pub const CTX_USERD_LO: usize = 0x008;
    pub const CTX_USERD_HI: usize = 0x00C;
    pub const CTX_SIGNATURE: usize = 0x010;
    pub const CTX_ACQUIRE: usize = 0x030;
    pub const CTX_GP_BASE_LO: usize = 0x048;
    pub const CTX_GP_BASE_HI: usize = 0x04C;
    #[expect(
        dead_code,
        reason = "hardware register definition — used by future targets"
    )]
    /// RAMFC GP_FETCH (byte-granular fetch pointer) mapped to PBDMA[0x050].
    pub const CTX_GP_FETCH_BYTE: usize = 0x050;
    /// RAMFC GP_PUT (entry index) mapped to PBDMA[0x054].
    pub const CTX_GP_PUT: usize = 0x054;
    /// RAMFC GP_GET (entry index consumed) mapped to PBDMA[0x058].
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

/// NV_PFB — Framebuffer controller registers (BAR0 + 0x100000).
/// These control HBM2/GDDR memory initialization and are the key to
/// sovereign VRAM access after D3cold. Register values vary by GPU model;
/// the differential probe discovers which writes unlock VRAM.
pub(crate) mod pfb {
    /// NV_PFB_PRI_MMU_CTRL — MMU control (enable, invalidate config).
    pub const MMU_CTRL: usize = 0x0010_0C80;
    /// NV_PFB_PRI_MMU_INVALIDATE_PDB — PDB address for TLB invalidation.
    pub const MMU_INVALIDATE_PDB: usize = 0x0010_0CB8;
    /// NV_PFB_PRI_MMU_INVALIDATE_PDB_HI — high bits.
    pub const MMU_INVALIDATE_PDB_HI: usize = 0x0010_0CEC;
    /// NV_PFB_PRI_MMU_INVALIDATE — trigger TLB invalidation.
    pub const MMU_INVALIDATE: usize = 0x0010_0CBC;

    /// NV_PFB_NISO_FLUSH_SYSMEM_ADDR_LO — NISO flush target (lo).
    pub const NISO_FLUSH_ADDR_LO: usize = 0x0010_0B20;
    /// NV_PFB_NISO_FLUSH_SYSMEM_ADDR_HI — NISO flush target (hi).
    pub const NISO_FLUSH_ADDR_HI: usize = 0x0010_0B24;

    /// Start of the FB register region to scan during differential probe.
    pub const REGION_START: usize = 0x0010_0000;
    /// End of the scannable FB region.
    pub const REGION_END: usize = 0x0010_1000;

    /// FBPA (Framebuffer Partition) base — per-partition config.
    /// GV100 has 4 FBPAs (0x900000, 0x900800, 0x901000, 0x901800).
    pub const FBPA_BASE: usize = 0x0090_0000;
    pub const FBPA_STRIDE: usize = 0x800;
    pub const FBPA_COUNT_MAX: usize = 16;
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

/// PRI (Primary Register Interface) bus monitoring and recovery.
///
/// When a BAR0 read returns `0xBADxxxxx`, the PRI bus has faulted — the
/// target domain didn't respond within the timeout window. This happens
/// when writing to clock-gated or power-gated domains. Without detection,
/// subsequent writes pile up and lock the entire bus.
pub mod pri {
    /// PMC master interrupt status. Bit 26 = PRIV_RING fault pending.
    pub const PMC_INTR: usize = 0x0000_0100;
    /// Bitmask for the PRIV_RING fault bit in `PMC_INTR`.
    pub const PMC_INTR_PRIV_RING_BIT: u32 = 1 << 26;

    /// PRIV ring interrupt status — reports which hub/GPC/FBP faulted.
    pub const PRIV_RING_INTR_STATUS: usize = 0x0012_0058;
    /// PRIV ring command — write 0x2 to ack/clear faults.
    pub const PRIV_RING_COMMAND: usize = 0x0012_004C;
    /// Acknowledge/clear command value for `PRIV_RING_COMMAND`.
    pub const PRIV_RING_CMD_ACK: u32 = 0x0000_0002;

    /// PRI master IOCTL — controls timeout duration and enable.
    pub const PRI_IOCTL: usize = 0x0012_2120;

    /// Sentinel values returned when a PRI target doesn't respond.
    /// The upper 16 bits encode the error type.
    pub const fn is_pri_error(val: u32) -> bool {
        let hi = val >> 16;
        hi == 0xBADF || hi == 0xBAD0 || hi == 0xBAD1
    }

    /// Check if a specific PRI error indicates a timeout (domain unresponsive).
    pub const fn is_pri_timeout(val: u32) -> bool {
        (val & 0xFFFF_0000) == 0xBAD0_0000
    }

    /// Check if a specific PRI error indicates an access violation.
    pub const fn is_pri_access_error(val: u32) -> bool {
        (val & 0xFFFF_0000) == 0xBADF_0000
    }

    /// Decode a PRI error value into a human-readable description.
    ///
    /// NVIDIA PRI errors encode the source and reason:
    /// - `0xBADF_xxxx`: PRIV fault — target domain rejected the access
    ///   - `0xBADF1100`: FBPA partition powered down (BLCG/SLCG gated)
    ///   - `0xBADF3000`: Domain clock-gated at hub level (PRIV ring)
    ///   - `0xBADF5040`: Clock domain not configured (PLL not locked)
    /// - `0xBAD0_xxxx`: PRI timeout — no response within timeout window
    ///   - `0xBAD0_0200`: PBUS timeout (bus controller not responding)
    ///   - `0xBAD0_AC0x`: PRAMIN/VRAM timeout (memory not trained)
    ///   - `0xBAD0_DA00`: PFIFO timeout (scheduler not initialized)
    pub fn decode_pri_error(val: u32) -> &'static str {
        match val & 0xFFFF_FF00 {
            0xBADF_1100 => "FBPA power-gated (BLCG/SLCG)",
            0xBADF_3000 => "Hub-level clock gate (PRIV ring)",
            0xBADF_5000 => "Clock domain unconfigured (PLL unlocked)",
            0xBAD0_0200 => "PBUS timeout",
            0xBAD0_AC00 => "PRAMIN/VRAM timeout (memory untrained)",
            0xBAD0_DA00 => "PFIFO scheduler timeout",
            _ => match val & 0xFFFF_0000 {
                0xBADF_0000 => "PRIV fault (domain rejected access)",
                0xBAD0_0000 => "PRI timeout (no response)",
                0xBAD1_0000 => "PRI target error",
                _ => "Unknown PRI error pattern",
            },
        }
    }

    /// Classify a BAR0 domain by address range.
    pub fn domain_name(offset: usize) -> &'static str {
        match offset {
            0x000000..=0x000FFF => "PMC",
            0x001000..=0x001FFF => "PBUS",
            0x002000..=0x003FFF => "PFIFO",
            0x009000..=0x009FFF => "PTIMER",
            0x00D000..=0x00DFFF => "PGRAPH_GLOBAL",
            0x020000..=0x022FFF => "PTOP/FUSE",
            0x040000..=0x09FFFF => "PBDMA",
            // Narrower PFB sub-regions first
            0x100800..=0x100AFF => "FBHUB",
            0x100C00..=0x100FFF => "PFB_NISO/MMU",
            0x100000..=0x101FFF => "PFB",
            0x10A000..=0x10BFFF => "PMU_FALCON",
            0x122000..=0x122FFF => "PRI_MASTER",
            0x130000..=0x139FFF => "PCLOCK/CLK",
            0x140000..=0x17DFFF => "GPC",
            0x17E000..=0x18FFFF => "LTC",
            0x1FA000..=0x1FAFFF => "PMEM",
            0x700000..=0x7FFFFF => "PRAMIN",
            0x800000..=0x8FFFFF => "PCCSR",
            0x900000..=0x9BFFFF => "FBPA",
            _ => "UNKNOWN",
        }
    }
}

/// Clock gating and power gating control registers.
///
/// Volta GPUs have three levels of clock gating:
/// - ELCG (Engine-Level): Controlled per-engine, coarsest
/// - BLCG (Block-Level): Per-block within engines
/// - SLCG (Second-Level): Finest-grained, within blocks
///
/// Disabling these gates is required to access domains that report
/// `0xBADF1100` (BLCG/SLCG gated) or `0xBADF3000` (hub clock gated).
///
/// Register map derived from nouveau `nvkm/subdev/therm/` and open-gpu-doc.
#[expect(
    dead_code,
    reason = "clock gating constants reserved for sovereign power management"
)]
pub(crate) mod cg {
    /// PTHERM CG control — master clock gating override.
    /// Writing 0x0 disables CG at the top level.
    pub const PTHERM_GATE_CTRL: usize = 0x0002_0200;

    /// PMC clock gating control registers.
    /// Each engine domain has a CG register at PMC + 0x800 + engine*4.
    pub const PMC_CG_BASE: usize = 0x0000_0800;

    /// PRIV_RING CG control (hub level).
    pub const PRIV_RING_CG: usize = 0x0012_0100;

    /// FBPA per-partition CG controls.
    /// Within each FBPA (base + stride*N), the CG register is at +0x0028.
    pub const FBPA_CG_OFFSET: usize = 0x0028;

    /// LTC cache-level CG controls.
    /// Within each LTC (base + stride*N), the CG register is at +0x01C8.
    pub const LTC_CG_OFFSET: usize = 0x01C8;

    /// PFB memory controller CG.
    pub const PFB_CG: usize = 0x0010_0C00;

    /// PCLOCK/CLK engine CG.
    pub const PCLOCK_CG: usize = 0x0013_7018;

    /// Standard CG disable value: all gating disabled.
    pub const CG_DISABLE: u32 = 0x0000_0000;

    /// Standard CG enable value (nouveau default): auto gating.
    pub const CG_AUTO: u32 = 0x0000_0500;

    /// Volta FBPA base and stride.
    pub const FBPA0_BASE: usize = 0x009A_0000;
    pub const FBPA_STRIDE: usize = 0x0000_4000;
    pub const FBPA_COUNT: usize = 4;

    /// Volta LTC base and stride.
    pub const LTC0_BASE: usize = 0x0017_E000;
    pub const LTC_STRIDE: usize = 0x0000_2000;
    pub const LTC_COUNT: usize = 6;

    /// Known CG control registers to sweep for disabling clock gating.
    /// Format: (register_offset, description).
    pub const CG_SWEEP_TARGETS: &[(usize, &str)] = &[
        (PTHERM_GATE_CTRL, "PTHERM master gate"),
        (0x0002_0204, "PTHERM CG1"),
        (0x0002_0208, "PTHERM CG2"),
        (PRIV_RING_CG, "PRIV_RING CG"),
        (0x0012_0104, "PRIV_RING CG1"),
        (PFB_CG, "PFB CG"),
        (PCLOCK_CG, "PCLOCK CG"),
        // Engine PMC CG slots (0x800+engine*4)
        (0x0000_0800, "PMC CG slot 0"),
        (0x0000_0804, "PMC CG slot 1"),
        (0x0000_0808, "PMC CG slot 2"),
        (0x0000_080C, "PMC CG slot 3"),
    ];
}

/// Root PLL and PCLOCK register offsets.
///
/// The root PLLs at 0x136xxx are in an always-on power domain — they remain
/// accessible even when the main PCLOCK (0x137xxx) is gated behind PLL lock.
/// These are the "crack in the wall" for sovereign GPU initialization.
///
/// From PCLOCK deep probe (March 2026): 283 readable registers found in
/// the 0x136xxx range on a cold GV100 card.
#[expect(
    missing_docs,
    reason = "register offsets are self-documenting via constant names"
)]
pub mod pclock {
    /// Root PLL domain — always-on, writable from host even when PCLOCK is gated.
    pub const ROOT_PLL_BASE: usize = 0x0013_6000;
    pub const ROOT_PLL_END: usize = 0x0013_7000;

    /// PCLOCK domain — gated behind PLL lock, partially accessible.
    pub const PCLOCK_BASE: usize = 0x0013_7000;
    pub const PCLOCK_END: usize = 0x0013_8000;

    /// Full CLK domain — includes all clock management registers.
    pub const CLK_BASE: usize = 0x0013_0000;
    pub const CLK_END: usize = 0x0013_8000;

    /// PCLOCK control register. PRI error = `0xBADF5040` (PLL unlocked).
    pub const PCLOCK_CTL: usize = 0x0013_7000;
    pub const PCLOCK_STATUS: usize = 0x0013_7004;
    pub const PCLOCK_COEFF: usize = 0x0013_7008;
    pub const PCLOCK_PLL0: usize = 0x0013_7010;
    pub const PCLOCK_PLL1: usize = 0x0013_7014;

    /// PCLOCK bypass — writable even on cold card.
    pub const PCLOCK_BYPASS: usize = 0x0013_7020;
    /// NVPLL control — writable even on cold card.
    pub const NVPLL_CTL: usize = 0x0013_7050;
    pub const NVPLL_COEFF: usize = 0x0013_7054;
    /// Memory PLL control — writable even on cold card.
    pub const MEMPLL_CTL: usize = 0x0013_7100;
    pub const MEMPLL_COEFF: usize = 0x0013_7104;

    /// Known root PLL sub-ranges (from the deep probe).
    /// These are clusters of readable registers within the 0x136xxx range.
    pub const ROOT_PLL_CLUSTERS: &[(usize, &str)] = &[
        (0x136400, "Root PLL cluster 0"),
        (0x136600, "Root PLL cluster 1"),
        (0x136800, "Root PLL cluster 2"),
        (0x136A00, "Root PLL cluster 3"),
        (0x136C00, "Root PLL cluster 4"),
        (0x136E00, "Root PLL cluster 5"),
    ];
}

pub(crate) mod falcon;

/// Miscellaneous BAR0 registers.
pub(crate) mod misc {
    /// BOOT0 — chip identification register.
    pub const BOOT0: usize = 0x0000_0000;
    /// PRI ring interrupt status.
    pub const PRIV_RING: usize = 0x0001_2070;
    /// PRAMIN base address for the BAR0 VRAM window.
    pub const PRAMIN_BASE: usize = 0x0070_0000;
    /// BAR0 window control register.
    pub const BAR0_WINDOW: usize = 0x0000_1700;
    /// L2 cache flush trigger.
    #[expect(dead_code, reason = "hardware register map — used in diagnostics")]
    pub const L2_FLUSH: usize = 0x0007_0010;
    /// NV_PMC_UNK260 — clock-gating restore. Nouveau: `nvkm_mc_unk260(device, 1)`.
    pub const PMC_UNK260: usize = 0x0000_0260;
    /// NV_PMC_ENABLE — per-engine enable register.
    /// Bit 12: GR, Bit 22: SEC2, etc. Some bits are hardware-locked (e.g. SEC2 on GV100).
    pub const PMC_ENABLE: usize = 0x0000_0200;
    /// NV_PMC_DEVICE_ENABLE — extended engine enable.
    #[expect(
        dead_code,
        reason = "hardware register map — used as reference during bring-up"
    )]
    pub const PMC_DEVICE_ENABLE: usize = 0x0000_0204;
    /// NV_PGRAPH_PRI — PGRAPH status register (read-only).
    pub const PGRAPH_STATUS: usize = 0x0040_0700;
    /// PFIFO scheduler enable (1 = running).
    pub const PFIFO_SCHED_EN: usize = 0x0000_2504;
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
    #[expect(
        dead_code,
        reason = "hardware register map — used as reference during bring-up"
    )]
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
    /// `NV_RAMFC_GP_FETCH` (dword 20) — PBDMA fetch pointer (byte address in ring).
    pub const GP_FETCH: usize = 0x050;
    /// `NV_RAMFC_GP_PUT` (dword 21) — next GP entry index for GPU.
    pub const GP_PUT: usize = 0x054;
    /// `NV_RAMFC_GP_GET` (dword 22) — next GP entry index consumed by GPU.
    pub const GP_GET: usize = 0x058;
    /// `NV_RAMFC_PB_HEADER` (dword 33).
    pub const PB_HEADER: usize = 0x084;
    /// `NV_RAMFC_SUBDEVICE` (dword 37) — subdevice mask.
    pub const SUBDEVICE: usize = 0x094;
    /// `NV_RAMFC_HCE_CTRL` (dword 57) — host compute engine control.
    pub const HCE_CTRL: usize = 0x0E4;
    /// Channel ID (Volta-specific, dword 58).
    pub const CHID: usize = 0x0E8;
    /// `NV_RAMFC_CONFIG` — PBDMA configuration. Maps to NV_PPBDMA_CONFIG.
    /// 0xbad00200 on GV100 PBDMA (register doesn't exist on Volta).
    pub const CONFIG: usize = 0x0A8;
    /// `NV_RAMFC_CHANNEL_INFO` — channel ID + group info.
    /// Maps to NV_PPBDMA_CHANNEL_INFO (PBDMA offset 0x0AC).
    /// Nouveau writes (chid | 0x03000000) for GV100.
    pub const CHANNEL_INFO: usize = 0x0AC;
}

/// `NV_RAMIN` offsets beyond RAMFC for MMU page directory configuration.
pub(super) mod ramin {
    /// Page directory base — low word (DW128, offset 0x200).
    pub const PAGE_DIR_BASE_LO: usize = 0x200;
    /// Page directory base — high word (DW129, offset 0x204).
    pub const PAGE_DIR_BASE_HI: usize = 0x204;
    /// VA space address limit — low word (DW130, offset 0x208).
    /// Nouveau sets to `vmm->limit - 1` = 0xFFFFFFFF for 128TB (low 32 bits).
    pub const ADDR_LIMIT_LO: usize = 0x208;
    /// VA space address limit — high word (DW131, offset 0x20C).
    /// Nouveau sets to 0x0001FFFF for 128TB (bits[47:32]).
    pub const ADDR_LIMIT_HI: usize = 0x20C;
    /// Engine WFI VEID (offset 0x218).
    pub const ENGINE_WFI_VEID: usize = 0x218;
    /// Subcontext PDB valid bitmap (DW166, offset 0x298).
    pub const SC_PDB_VALID: usize = 0x298;
    /// Subcontext 0 page directory base — low word (offset 0x2A0).
    pub const SC0_PAGE_DIR_BASE_LO: usize = 0x2A0;
    /// Subcontext 0 page directory base — high word (offset 0x2A4).
    pub const SC0_PAGE_DIR_BASE_HI: usize = 0x2A4;
    /// Subcontext 1 page directory base — low word (offset 0x2B0).
    pub const SC1_PAGE_DIR_BASE_LO: usize = 0x2B0;
    /// Subcontext 1 page directory base — high word (offset 0x2B4).
    pub const SC1_PAGE_DIR_BASE_HI: usize = 0x2B4;
}

// ── IOVA assignments for channel infrastructure DMA buffers ────────────

/// Instance block DMA buffer IOVA.
pub(super) const INSTANCE_IOVA: u64 = 0x3000;
/// Runlist DMA buffer IOVA.
pub(super) const RUNLIST_IOVA: u64 = 0x4000;
/// PD3 (level-4 page directory) IOVA.
/// pub(crate) so SEC2 ACR boot can bind the falcon's instance block to this.
pub(crate) const PD3_IOVA: u64 = 0x5000;
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

#[cfg(test)]
mod tests {
    use super::falcon;
    use super::pbdma;
    use super::pccsr;
    use super::pfifo;
    use super::pri;
    use super::{INSTANCE_IOVA, PD0_IOVA, PT_ENTRIES, TARGET_SYS_MEM_COHERENT};

    #[test]
    fn pfifo_runlist_stride() {
        assert_eq!(pfifo::runlist_base(0), 0x2270);
        assert_eq!(pfifo::runlist_base(1), 0x2280);
        assert_eq!(pfifo::runlist_submit(2), 0x2294);
    }

    #[test]
    fn gv100_runlist_base_and_submit_encode() {
        let iova = 0x1_2345_6789_ABCD_u64;
        assert_eq!(pfifo::gv100_runlist_base_value(iova), (iova >> 12) as u32);
        let entry_count = 7_u32;
        let expected_submit = ((iova >> 44) as u32) | (entry_count << 16);
        assert_eq!(
            pfifo::gv100_runlist_submit_value(iova, entry_count),
            expected_submit
        );
    }

    #[test]
    fn pfifo_intr_bits() {
        assert_eq!(pfifo::INTR_BIT8 | pfifo::INTR_CHSW_ERROR, 0x0001_0100);
        assert_ne!(pfifo::INTR_RL_COMPLETE, 0);
        assert_ne!(pfifo::INTR_PBDMA, 0);
        assert_eq!(pfifo::INTR_RL_COMPLETE & pfifo::INTR_PBDMA, 0);
    }

    #[test]
    fn pbdma_stride_and_intr() {
        assert_eq!(pbdma::base(0), 0x4_0000);
        assert_eq!(pbdma::base(1), 0x4_2000);
        assert_eq!(pbdma::intr(0), 0x4_0108);
        assert_eq!(pbdma::intr(1), 0x4_2108);
    }

    #[test]
    fn pccsr_inst_channel_and_status() {
        assert_eq!(pccsr::inst(0), 0x80_0000);
        assert_eq!(pccsr::channel(0), 0x80_0004);
        let ch = (0x5 << 24) | 1;
        assert_eq!(pccsr::status(ch), 0x5);
        assert_eq!(pccsr::status_name(ch), "ON_PBDMA");
    }

    #[test]
    fn pccsr_channel_flags() {
        assert_eq!(pccsr::inst(3), 0x80_0018);
        assert_eq!(
            pccsr::CHANNEL_ENABLE_SET & pccsr::CHANNEL_ENABLE_CLR,
            0,
            "enable set/clear are distinct bits"
        );
    }

    #[test]
    fn pri_error_classifiers() {
        assert!(pri::is_pri_error(0xBADF_1234));
        assert!(pri::is_pri_timeout(0xBAD0_0200));
        assert!(pri::is_pri_access_error(0xBADF_0000));
        assert!(!pri::is_pri_error(0x1234_5678));
    }

    #[test]
    fn pri_decode_and_domain_names() {
        assert_eq!(
            pri::decode_pri_error(0xBADF_1100),
            "FBPA power-gated (BLCG/SLCG)"
        );
        assert_eq!(pri::decode_pri_error(0xBAD0_0200), "PBUS timeout");
        assert_eq!(pri::domain_name(0x200), "PMC");
        assert_eq!(pri::domain_name(0x2000), "PFIFO");
        assert_eq!(pri::domain_name(0x100_C80), "PFB_NISO/MMU");
        assert_eq!(pri::domain_name(0xFF_FFFF), "UNKNOWN");
    }

    #[test]
    fn falcon_hwcfg_sizes() {
        let hwcfg = (3 << 9) | 5;
        assert_eq!(falcon::imem_size_bytes(hwcfg), 5 * 256);
        assert_eq!(falcon::dmem_size_bytes(hwcfg), 3 * 256);
        assert_eq!(falcon::CPUCTL_STARTCPU | falcon::CPUCTL_IINVAL, 0x3);
    }

    #[test]
    fn channel_iova_and_target_constants() {
        assert_eq!(INSTANCE_IOVA, 0x3000);
        assert_eq!(PD0_IOVA, 0x8000);
        assert_eq!(TARGET_SYS_MEM_COHERENT, 2);
        assert_eq!(PT_ENTRIES, 512);
    }
}
