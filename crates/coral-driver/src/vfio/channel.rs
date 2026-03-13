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
    /// FIFO engine enable (1 = enabled).
    pub const ENABLE: usize = 0x0000_2504;
    /// Runlist base address and aperture target.
    pub const RUNLIST_BASE: usize = 0x0000_2270;
    /// Runlist submit: length + runlist ID.
    pub const RUNLIST: usize = 0x0000_2274;
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

    /// INST_TARGET = SYS_MEM_COHERENT (bits [29:28] = 2).
    pub const INST_TARGET_SYS_MEM_COHERENT: u32 = 2 << 28;
    /// INST_BIND = TRUE (bit 31).
    pub const INST_BIND_TRUE: u32 = 1 << 31;
    /// CHANNEL_ENABLE_SET trigger (bit 10).
    pub const CHANNEL_ENABLE_SET: u32 = 1 << 10;
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

/// SYS_MEM_COHERENT aperture target value for PCCSR/PFIFO/RAMIN registers.
const TARGET_SYS_MEM_COHERENT: u32 = 2;

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
        };

        chan.populate_page_tables();
        chan.populate_instance_block(gpfifo_iova, gpfifo_entries, userd_iova);
        chan.populate_runlist(userd_iova);

        bar0.write_u32(pfifo::ENABLE, 1)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("PFIFO enable: {e}"))))?;

        chan.bind_channel(bar0)?;
        chan.enable_channel(bar0)?;
        chan.submit_runlist(bar0)?;

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
        write_u32_le(
            inst,
            ramfc::USERD_LO,
            (userd_iova as u32 & 0xFFFF_FE00) | TARGET_SYS_MEM_COHERENT,
        );
        write_u32_le(inst, ramfc::USERD_HI, (userd_iova >> 32) as u32);

        write_u32_le(inst, ramfc::SIGNATURE, 0x0000_FACE);
        write_u32_le(inst, ramfc::ACQUIRE, 0x7FFF_F902);

        // GPFIFO base (GPU virtual address — our identity map makes VA = IOVA).
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
    fn populate_runlist(&mut self, userd_iova: u64) {
        let rl = self.runlist.as_mut_slice();

        // ── TSG (channel group) header — 16 bytes ──────────────────────
        // DW0: (timeslice=128 << 24) | (tsg_length=3 << 16) | type=1
        write_u32_le(rl, 0x00, (128 << 24) | (3 << 16) | 1);
        write_u32_le(rl, 0x04, 1); // 1 channel in group
        write_u32_le(rl, 0x08, 0); // group ID = 0
        write_u32_le(rl, 0x0C, 0);

        // ── Channel entry — 16 bytes ───────────────────────────────────
        // DW0: USERD addr low | target (SYS_MEM_COHERENT = 2)
        write_u32_le(
            rl,
            0x10,
            (userd_iova as u32 & 0xFFFF_FE00) | TARGET_SYS_MEM_COHERENT,
        );
        write_u32_le(rl, 0x14, (userd_iova >> 32) as u32);
        // DW2: INST addr low | channel_id (inst is 4K-aligned, low 12 bits free)
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
            | pccsr::INST_TARGET_SYS_MEM_COHERENT
            | pccsr::INST_BIND_TRUE;

        bar0.write_u32(pccsr::inst(self.channel_id), value)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("PCCSR bind: {e}"))))
    }

    /// Enable the channel via PCCSR ENABLE_SET trigger.
    fn enable_channel(&self, bar0: &MappedBar) -> DriverResult<()> {
        bar0.write_u32(pccsr::channel(self.channel_id), pccsr::CHANNEL_ENABLE_SET)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("channel enable: {e}"))))
    }

    /// Submit runlist to PFIFO (runlist 0, 2 entries: TSG header + channel).
    fn submit_runlist(&self, bar0: &MappedBar) -> DriverResult<()> {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "RUNLIST_IOVA >> 12 fits u32 for our allocation range"
        )]
        let rl_base = (RUNLIST_IOVA >> 12) as u32 | (TARGET_SYS_MEM_COHERENT << 28);

        bar0.write_u32(pfifo::RUNLIST_BASE, rl_base)
            .map_err(|e| DriverError::SubmitFailed(Cow::Owned(format!("runlist base: {e}"))))?;

        // Length = 2 entries (TSG + channel), runlist ID = 0.
        bar0.write_u32(pfifo::RUNLIST, 2)
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
/// PDE format (V2): `[1:0]=aperture, [2]=volatile, [35:4]=address>>12`.
fn write_pde(pd_slice: &mut [u8], index: usize, target_iova: u64) {
    let pde = encode_pde(target_iova);
    let off = index * 8;
    pd_slice[off..off + 8].copy_from_slice(&pde.to_le_bytes());
}

/// Encode a V2 PDE pointing to a page table at `iova` in system memory.
fn encode_pde(iova: u64) -> u64 {
    let aperture = 2_u64; // SYS_MEM_COHERENT
    let vol = 1_u64 << 2;
    let addr = (iova >> 12) << 4;
    aperture | vol | addr
}

/// Encode a V2 small-page PTE for an identity-mapped physical address.
///
/// PTE format: `[0]=valid, [2:1]=aperture, [35:4]=addr>>12, [36]=volatile`.
fn encode_pte(phys_addr: u64) -> u64 {
    let valid = 1_u64;
    let aperture = 2_u64 << 1; // SYS_MEM_COHERENT
    let addr = (phys_addr >> 12) << 4;
    let vol = 1_u64 << 36;
    valid | aperture | addr | vol
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
        let pde = encode_pde(0x6000);
        assert_eq!(pde & 0x3, 2, "aperture = SYS_MEM_COHERENT");
        assert_eq!((pde >> 2) & 1, 1, "volatile bit set");
        let addr = ((pde >> 4) & 0xFFFF_FFFF) << 12;
        assert_eq!(addr, 0x6000, "decoded address matches");
    }

    #[test]
    fn pte_encoding_identity_map() {
        let pte = encode_pte(0x1000);
        assert_eq!(pte & 1, 1, "valid bit");
        assert_eq!((pte >> 1) & 3, 2, "aperture = SYS_MEM_COHERENT");
        assert_eq!((pte >> 36) & 1, 1, "volatile bit");
        let addr = ((pte >> 4) & 0xFFFF_FFFF) << 12;
        assert_eq!(addr, 0x1000, "decoded address matches");
    }

    #[test]
    fn pte_encoding_higher_address() {
        let pte = encode_pte(0x10_0000);
        let addr = ((pte >> 4) & 0xFFFF_FFFF) << 12;
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
            | pccsr::INST_TARGET_SYS_MEM_COHERENT
            | pccsr::INST_BIND_TRUE;
        assert_eq!(value & 0x0FFF_FFFF, 3, "INST_PTR = 3 (0x3000 >> 12)");
        assert_eq!((value >> 28) & 3, 2, "target = SYS_MEM_COHERENT");
        assert_eq!((value >> 31) & 1, 1, "BIND = TRUE");
    }

    #[test]
    fn runlist_base_value() {
        let rl_base = (RUNLIST_IOVA >> 12) as u32 | (TARGET_SYS_MEM_COHERENT << 28);
        assert_eq!(rl_base & 0x0FFF_FFFF, 4, "PTR = 4 (0x4000 >> 12)");
        assert_eq!((rl_base >> 28) & 3, 2, "target = SYS_MEM_COHERENT");
    }

    #[test]
    fn write_u32_le_roundtrip() {
        let mut buf = [0u8; 8];
        write_u32_le(&mut buf, 4, 0xDEAD_BEEF);
        let val = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
        assert_eq!(val, 0xDEAD_BEEF);
    }
}
