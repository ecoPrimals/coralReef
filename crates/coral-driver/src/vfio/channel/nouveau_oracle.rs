// SPDX-License-Identifier: AGPL-3.0-or-later
//! Nouveau MMU oracle — reads page table state from a nouveau-bound card.
//!
//! When nouveau initializes a GPU, it sets up V2 MMU page tables in VRAM.
//! This module reads those page tables through the BAR0 PRAMIN window to
//! produce ground truth for PDE/PTE encoding, which can then be compared
//! against our sovereign pipeline's `encode_pde()`/`encode_pte()`.
//!
//! The PRAMIN window is a 1 MiB aperture at BAR0 + 0x700000 that gives
//! host access to VRAM. The BAR0_WINDOW register (0x1700) controls which
//! 1 MiB page of VRAM is visible. By sliding this window, we can read
//! arbitrary VRAM locations.

use std::ptr::NonNull;

use crate::error::ChannelError;

use super::registers::{misc, pccsr, ramin};

const PRAMIN_OFFSET: usize = 0x0070_0000;

/// Result of reading a nouveau channel's MMU page table chain.
#[derive(Debug)]
#[expect(
    missing_docs,
    reason = "diagnostic struct — field names are self-documenting"
)]
pub struct NouveauPageTableDump {
    /// Channel ID that was read.
    pub channel_id: u32,
    /// Raw PCCSR instance pointer register value.
    pub pccsr_inst_raw: u32,
    /// Instance block VRAM address (decoded from PCCSR).
    pub inst_vram_addr: u64,
    /// PAGE_DIR_BASE_LO raw value from instance block offset 0x200.
    pub pdb_lo: u32,
    /// PAGE_DIR_BASE_HI raw value from instance block offset 0x204.
    pub pdb_hi: u32,
    /// Decoded PD3 VRAM address from PAGE_DIR_BASE.
    pub pd3_vram_addr: u64,
    /// PD3 entry 0 raw bytes (the PDE pointing to PD2).
    pub pd3_entry0: u64,
    /// PD2 VRAM address decoded from PD3 entry 0.
    pub pd2_vram_addr: u64,
    /// PD2 entry 0 raw bytes.
    pub pd2_entry0: u64,
    /// PD1 VRAM address decoded from PD2 entry 0.
    pub pd1_vram_addr: u64,
    /// PD1 entry 0 raw bytes.
    pub pd1_entry0: u64,
    /// PD0 VRAM address decoded from PD1 entry 0.
    pub pd0_vram_addr: u64,
    /// PD0 raw entry (16 bytes — dual PDE: `[0:7]`=small, `[8:15]`=large).
    pub pd0_entry0_small: u64,
    pub pd0_entry0_large: u64,
    /// PT VRAM address decoded from PD0 small PDE.
    pub pt_vram_addr: u64,
    /// First 16 PTEs from PT (entries 0-15).
    pub pt_entries: Vec<u64>,
    /// Instance block RAMFC fields.
    pub ramfc_userd_lo: u32,
    pub ramfc_userd_hi: u32,
    pub ramfc_gp_base_lo: u32,
    pub ramfc_gp_base_hi: u32,
    /// Subcontext 0 PDB.
    pub sc0_pdb_lo: u32,
    pub sc0_pdb_hi: u32,
    /// ADDR_LIMIT from instance block.
    pub addr_limit_lo: u32,
    pub addr_limit_hi: u32,
    /// Diagnostic messages.
    pub log: Vec<String>,
}

/// Read-write mmap of BAR0 for oracle page table walking.
struct Bar0Rw {
    ptr: NonNull<u8>,
    size: usize,
    _file: std::fs::File,
}

impl Bar0Rw {
    fn open(bdf: &str) -> Result<Self, ChannelError> {
        let path = crate::linux_paths::sysfs_pci_device_file(bdf, "resource0");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| ChannelError::resource_io("open", path.clone(), e))?;

        let size = 16 * 1024 * 1024; // 16 MiB BAR0
        // SAFETY: mmap of PCI BAR0 sysfs resource with R/W for PRAMIN window sliding.
        let raw = unsafe {
            rustix::mm::mmap(
                std::ptr::null_mut(),
                size,
                rustix::mm::ProtFlags::READ | rustix::mm::ProtFlags::WRITE,
                rustix::mm::MapFlags::SHARED,
                &file,
                0,
            )
        }
        .map_err(|e| ChannelError::Bar0Mmap {
            path: path.clone(),
            source: e,
        })?;

        let ptr = NonNull::new(raw.cast::<u8>())
            .ok_or(ChannelError::Bar0MmapNull { path: path.clone() })?;

        Ok(Self {
            ptr,
            size,
            _file: file,
        })
    }

    fn read_u32(&self, offset: usize) -> u32 {
        assert!(offset + 4 <= self.size);
        // SAFETY: bounds checked, volatile for MMIO.
        unsafe { std::ptr::read_volatile(self.ptr.as_ptr().add(offset).cast::<u32>()) }
    }

    fn write_u32(&self, offset: usize, val: u32) {
        assert!(offset + 4 <= self.size);
        // SAFETY: bounds checked, volatile for MMIO.
        unsafe {
            std::ptr::write_volatile(self.ptr.as_ptr().add(offset).cast::<u32>(), val);
        }
    }

    /// Read a 64-bit value from PRAMIN at the given VRAM-relative offset
    /// within the current 1 MiB window.
    fn read_pramin_u64(&self, offset_in_window: usize) -> u64 {
        let lo = self.read_u32(PRAMIN_OFFSET + offset_in_window) as u64;
        let hi = self.read_u32(PRAMIN_OFFSET + offset_in_window + 4) as u64;
        lo | (hi << 32)
    }

    fn read_pramin_u32(&self, offset_in_window: usize) -> u32 {
        self.read_u32(PRAMIN_OFFSET + offset_in_window)
    }

    /// Set the BAR0 window to expose `vram_page` (1 MiB aligned) at PRAMIN.
    fn set_window(&self, vram_page: u64) {
        let window_val = (vram_page >> 16) as u32;
        self.write_u32(misc::BAR0_WINDOW, window_val);
        // Read back to ensure the write landed.
        let _ = self.read_u32(misc::BAR0_WINDOW);
    }

    /// Read a u32 from arbitrary VRAM address by sliding the PRAMIN window.
    fn read_vram_u32(&self, vram_addr: u64) -> u32 {
        let page = vram_addr & !0xF_FFFF; // 1 MiB aligned
        let offset = (vram_addr & 0xF_FFFF) as usize;
        self.set_window(page);
        self.read_pramin_u32(offset)
    }

    /// Read a u64 from arbitrary VRAM address.
    fn read_vram_u64(&self, vram_addr: u64) -> u64 {
        let page = vram_addr & !0xF_FFFF;
        let offset = (vram_addr & 0xF_FFFF) as usize;
        self.set_window(page);
        self.read_pramin_u64(offset)
    }
}

impl Drop for Bar0Rw {
    fn drop(&mut self) {
        // SAFETY: unmapping the region mapped in open().
        unsafe {
            let _ = rustix::mm::munmap(self.ptr.as_ptr().cast(), self.size);
        }
    }
}

/// Decode a VRAM physical address from a V2 PDE.
/// PDE format: `addr = (PDE & ~0xFFF) << 4` (bits [63:12] shifted left 4).
/// Actually for Volta V2: addr bits are [63:4], flags are [3:0].
/// So addr = (PDE >> 4) << 8 ... no, let's read what nouveau does:
/// `gp100_vmm_pde`: data[0] = `(addr >> 8) | target`
/// So: addr = `(PDE & ~0xFF) << 8`? No...
///
/// From nouveau gp100_vmm.c `gp100_vmm_pd0_pde`:
///   `VMM_WO128(pd, ..., data[0] = small_pte_addr >> 4 | flags, data[1] = ...)`
/// And `gp100_vmm_pde`:
///   `data[0] = ((pgt_addr >> 8) << 8) | flags` — wait, that's trivial.
///
/// Actually the encoding is: `(phys >> 4) | flags` where flags are bits [3:0].
/// So to decode: `(PDE & ~0xF) << 4` gives the physical address.
fn decode_pde_addr(pde: u64) -> u64 {
    (pde & !0xF) << 4
}

/// Decode a VRAM physical address from a V2 PTE.
/// Same encoding: `(PTE & ~0xF) << 4`.
fn decode_pte_addr(pte: u64) -> u64 {
    (pte & !0xF) << 4
}

/// Decode PDE/PTE flag bits.
fn decode_flags(entry: u64) -> String {
    let valid = entry & 1;
    let aperture = (entry >> 1) & 3;
    let vol = (entry >> 3) & 1;
    let aper_name = match aperture {
        0 => "INVALID",
        1 => "VRAM",
        2 => "SYS_COH",
        3 => "SYS_NCOH",
        _ => "?",
    };
    format!("valid={valid} aper={aper_name}({aperture}) vol={vol}")
}

/// Scan PCCSR for the first active channel and return its ID and instance pointer.
fn find_active_channel(bar0: &Bar0Rw) -> Option<(u32, u32)> {
    for id in 0..512u32 {
        let inst_reg = bar0.read_u32(pccsr::inst(id));
        if inst_reg == 0 || inst_reg == 0xFFFF_FFFF || inst_reg == 0xBADF_1000 {
            continue;
        }
        let chan_reg = bar0.read_u32(pccsr::channel(id));
        let enabled = chan_reg & 1;
        if enabled != 0 || inst_reg != 0 {
            return Some((id, inst_reg));
        }
    }
    None
}

/// Read page table state from a nouveau-bound card.
///
/// The card must be bound to the `nouveau` kernel driver. This function:
/// 1. Opens BAR0 R/W via sysfs resource0
/// 2. Scans PCCSR for an active channel
/// 3. Reads the instance block from VRAM via PRAMIN
/// 4. Walks the V2 page table chain: PD3 → PD2 → PD1 → PD0 → PT
/// 5. Returns raw PDE/PTE values for comparison with sovereign encoding
pub fn read_nouveau_page_tables(bdf: &str) -> Result<NouveauPageTableDump, ChannelError> {
    let bar0 = Bar0Rw::open(bdf)?;
    let mut log = Vec::new();

    let boot0 = bar0.read_u32(misc::BOOT0);
    log.push(format!("BOOT0: {boot0:#010x}"));

    if boot0 == 0xFFFF_FFFF {
        return Err(ChannelError::Bar0ReadsAllOnes);
    }

    // Save current BAR0 window so we can restore it.
    let saved_window = bar0.read_u32(misc::BAR0_WINDOW);
    log.push(format!("BAR0_WINDOW (saved): {saved_window:#010x}"));

    let (channel_id, pccsr_inst_raw) =
        find_active_channel(&bar0).ok_or(ChannelError::NoActivePccsrChannel)?;

    log.push(format!(
        "channel {channel_id}: PCCSR inst={pccsr_inst_raw:#010x}"
    ));

    // PCCSR INST_PTR: bits [27:0] are the instance block address >> 12.
    // Target: bits [29:28] (0=VRAM, 2=SYS_COH, 3=SYS_NCOH).
    let inst_ptr_shifted = pccsr_inst_raw & 0x0FFF_FFFF;
    let inst_target = (pccsr_inst_raw >> 28) & 3;
    let inst_vram_addr = (inst_ptr_shifted as u64) << 12;

    log.push(format!(
        "instance block: vram_addr={inst_vram_addr:#010x} target={inst_target} (0=VRAM)"
    ));

    // Read instance block fields via PRAMIN.
    let pdb_lo = bar0.read_vram_u32(inst_vram_addr + ramin::PAGE_DIR_BASE_LO as u64);
    let pdb_hi = bar0.read_vram_u32(inst_vram_addr + ramin::PAGE_DIR_BASE_HI as u64);

    log.push(format!(
        "PAGE_DIR_BASE: lo={pdb_lo:#010x} hi={pdb_hi:#010x}"
    ));

    // Decode PD3 address from PAGE_DIR_BASE_LO.
    // Format: PTR[31:12] | flags[11:0]
    let pd3_vram_addr = (pdb_lo as u64 & 0xFFFF_F000) | ((pdb_hi as u64) << 32);

    log.push(format!("PD3 VRAM addr: {pd3_vram_addr:#010x}"));
    log.push(format!(
        "PDB flags: BIG_PAGE_SIZE={} VER2={} VOL={} TARGET={}",
        (pdb_lo >> 11) & 1,
        (pdb_lo >> 10) & 1,
        (pdb_lo >> 2) & 1,
        pdb_lo & 3,
    ));

    // Read RAMFC fields.
    let ramfc_userd_lo = bar0.read_vram_u32(inst_vram_addr + 0x008);
    let ramfc_userd_hi = bar0.read_vram_u32(inst_vram_addr + 0x00C);
    let ramfc_gp_base_lo = bar0.read_vram_u32(inst_vram_addr + 0x010);
    let ramfc_gp_base_hi = bar0.read_vram_u32(inst_vram_addr + 0x014);
    log.push(format!(
        "RAMFC: USERD={ramfc_userd_hi:#010x}_{ramfc_userd_lo:#010x} GP_BASE={ramfc_gp_base_hi:#010x}_{ramfc_gp_base_lo:#010x}"
    ));

    let sc0_pdb_lo = bar0.read_vram_u32(inst_vram_addr + ramin::SC0_PAGE_DIR_BASE_LO as u64);
    let sc0_pdb_hi = bar0.read_vram_u32(inst_vram_addr + ramin::SC0_PAGE_DIR_BASE_HI as u64);
    let addr_limit_lo = bar0.read_vram_u32(inst_vram_addr + ramin::ADDR_LIMIT_LO as u64);
    let addr_limit_hi = bar0.read_vram_u32(inst_vram_addr + ramin::ADDR_LIMIT_HI as u64);

    log.push(format!(
        "SC0_PDB: lo={sc0_pdb_lo:#010x} hi={sc0_pdb_hi:#010x}"
    ));
    log.push(format!(
        "ADDR_LIMIT: lo={addr_limit_lo:#010x} hi={addr_limit_hi:#010x}"
    ));

    // Walk PD3 → PD2: read PD3 entry 0.
    let pd3_entry0 = bar0.read_vram_u64(pd3_vram_addr);
    let pd2_vram_addr = decode_pde_addr(pd3_entry0);
    log.push(format!(
        "PD3[0]: raw={pd3_entry0:#018x} → PD2 addr={pd2_vram_addr:#010x} flags=[{}]",
        decode_flags(pd3_entry0)
    ));

    // Walk PD2 → PD1: read PD2 entry 0.
    let pd2_entry0 = bar0.read_vram_u64(pd2_vram_addr);
    let pd1_vram_addr = decode_pde_addr(pd2_entry0);
    log.push(format!(
        "PD2[0]: raw={pd2_entry0:#018x} → PD1 addr={pd1_vram_addr:#010x} flags=[{}]",
        decode_flags(pd2_entry0)
    ));

    // Walk PD1 → PD0: read PD1 entry 0.
    let pd1_entry0 = bar0.read_vram_u64(pd1_vram_addr);
    let pd0_vram_addr = decode_pde_addr(pd1_entry0);
    log.push(format!(
        "PD1[0]: raw={pd1_entry0:#018x} → PD0 addr={pd0_vram_addr:#010x} flags=[{}]",
        decode_flags(pd1_entry0)
    ));

    // PD0 is dual-format: 16 bytes per entry.
    // [0:7] = small page PDE, [8:15] = large page PDE.
    let pd0_entry0_small = bar0.read_vram_u64(pd0_vram_addr);
    let pd0_entry0_large = bar0.read_vram_u64(pd0_vram_addr + 8);
    let pt_vram_addr = decode_pde_addr(pd0_entry0_small);
    log.push(format!(
        "PD0[0] small: raw={pd0_entry0_small:#018x} → PT addr={pt_vram_addr:#010x} flags=[{}]",
        decode_flags(pd0_entry0_small)
    ));
    log.push(format!(
        "PD0[0] large: raw={pd0_entry0_large:#018x} flags=[{}]",
        decode_flags(pd0_entry0_large)
    ));

    // Read first 16 PTEs from PT.
    let mut pt_entries = Vec::with_capacity(16);
    for i in 0..16u64 {
        let pte = bar0.read_vram_u64(pt_vram_addr + i * 8);
        let phys = decode_pte_addr(pte);
        log.push(format!(
            "PT[{i:2}]: raw={pte:#018x} → phys={phys:#012x} flags=[{}]",
            decode_flags(pte)
        ));
        pt_entries.push(pte);
    }

    // Restore BAR0 window.
    bar0.set_window((saved_window as u64) << 16);

    Ok(NouveauPageTableDump {
        channel_id,
        pccsr_inst_raw,
        inst_vram_addr,
        pdb_lo,
        pdb_hi,
        pd3_vram_addr,
        pd3_entry0,
        pd2_vram_addr,
        pd2_entry0,
        pd1_vram_addr,
        pd1_entry0,
        pd0_vram_addr,
        pd0_entry0_small,
        pd0_entry0_large,
        pt_vram_addr,
        pt_entries,
        ramfc_userd_lo,
        ramfc_userd_hi,
        ramfc_gp_base_lo,
        ramfc_gp_base_hi,
        sc0_pdb_lo,
        sc0_pdb_hi,
        addr_limit_lo,
        addr_limit_hi,
        log,
    })
}

/// Emit a comparison report between nouveau's page tables and our encoding via tracing.
pub fn print_comparison_report(dump: &NouveauPageTableDump) {
    use super::page_tables::{encode_pde, encode_pte};

    tracing::info!("=== Nouveau MMU Oracle — Page Table Comparison ===");

    for line in &dump.log {
        tracing::debug!(message = %line, "nouveau walk log line");
    }

    tracing::info!("--- Encoding Comparison ---");

    // Compare PDE encoding.
    // Nouveau stores page tables in VRAM (aperture=1), ours are in SYS_MEM_COH (aperture=2).
    // So the flag bits WILL differ — the address encoding is what matters.
    let our_flags: u64 = (2 << 1) | (1 << 3); // COH + VOL
    let nouveau_flags = dump.pd3_entry0 & 0xF;
    tracing::info!(
        nouveau_flags = format_args!("{nouveau_flags:#x}"),
        our_flags = format_args!("{our_flags:#x}"),
        note = "expected to differ: nouveau VRAM aperture vs SYS_MEM_COH",
        "PDE flags"
    );

    let nouveau_addr_shift = dump.pd3_entry0 >> 4;
    tracing::debug!(
        nouveau_addr_shift = format_args!("{nouveau_addr_shift:#x}"),
        decoded_addr = format_args!("{:#x}", decode_pde_addr(dump.pd3_entry0)),
        "PDE addr encoding (upper bits store addr >> 4)"
    );

    // Compare our encode_pde with a hypothetical VRAM-target PDE.
    let test_iova: u64 = 0x1_0000; // 64 KiB
    let our_pde = encode_pde(test_iova);
    let expected_addr_bits = test_iova >> 4;
    tracing::debug!(
        test_iova = format_args!("{test_iova:#x}"),
        our_pde = format_args!("{our_pde:#018x}"),
        addr_bits = format_args!("{expected_addr_bits:#x}"),
        flags = format_args!("{:#x}", our_pde & 0xF),
        "encode_pde test"
    );

    // Compare PTE encoding.
    if let Some(&pte1) = dump.pt_entries.get(1)
        && pte1 != 0
    {
        let nouveau_pte_flags = pte1 & 0xF;
        let nouveau_pte_addr = decode_pte_addr(pte1);
        tracing::debug!(
            index = 1,
            raw = format_args!("{pte1:#018x}"),
            flags = format_args!("{nouveau_pte_flags:#x}"),
            addr = format_args!("{nouveau_pte_addr:#x}"),
            "PT[1] nouveau"
        );

        let our_pte = encode_pte(nouveau_pte_addr);
        let flag_match = (our_pte & 0xF) == nouveau_pte_flags;
        let addr_match = decode_pte_addr(our_pte) == nouveau_pte_addr;
        tracing::debug!(
            our_pte = format_args!("{our_pte:#018x}"),
            flag_match,
            addr_match,
            "encode_pte comparison"
        );
        if !flag_match {
            tracing::warn!(
                nouveau = format_args!("{nouveau_pte_flags:#x}"),
                ours = format_args!("{:#x}", our_pte & 0xF),
                "PTE flag mismatch"
            );
        }
    }

    // Compare instance block PAGE_DIR_BASE encoding.
    tracing::info!("--- Instance Block PAGE_DIR_BASE ---");
    tracing::debug!(
        pdb_lo = format_args!("{:#010x}", dump.pdb_lo),
        ptr_31_12 = format_args!("{:#010x}", dump.pdb_lo & 0xFFFF_F000),
        big_page = (dump.pdb_lo >> 11) & 1,
        ver2_pt = (dump.pdb_lo >> 10) & 1,
        vol = (dump.pdb_lo >> 2) & 1,
        target = dump.pdb_lo & 3,
        "Nouveau PDB_LO fields (TARGET: 0=VRAM, 2=COH, 3=NCOH)"
    );
    tracing::debug!(
        pdb_hi = format_args!("{:#010x}", dump.pdb_hi),
        "Nouveau PDB_HI"
    );
    tracing::debug!(
        sc0_lo = format_args!("{:#010x}", dump.sc0_pdb_lo),
        sc0_hi = format_args!("{:#010x}", dump.sc0_pdb_hi),
        "Nouveau SC0_PDB"
    );
    tracing::info!(
        limit_lo = format_args!("{:#010x}", dump.addr_limit_lo),
        limit_hi = format_args!("{:#010x}", dump.addr_limit_hi),
        "Nouveau ADDR_LIMIT"
    );

    tracing::info!("=== End Oracle Report ===");
}
