// SPDX-License-Identifier: AGPL-3.0-or-later
//! Structured MMU fault decoding for Volta+ VFIO sovereign pipeline.
//!
//! Reads the GPU's hub MMU fault registers (BAR0 `0x100xxx`) and decodes
//! them into a [`MmuFaultInfo`] struct. Used by the GPFIFO submission
//! path to produce actionable diagnostics instead of raw hex dumps.
//!
//! Register source: NVIDIA open-gpu-doc `dev_fb.ref.txt`, nouveau
//! `nvkm/subdev/fault/gv100.c`.

use crate::vfio::device::MappedBar;

use super::registers::{mmu, pfb};

/// Decoded MMU fault information from BAR0 registers.
#[derive(Debug, Clone)]
pub struct MmuFaultInfo {
    /// Raw `NV_PFB_PRI_MMU_FAULT_STATUS` value.
    pub fault_status: u32,
    /// Faulting GPU virtual address (low 32 bits).
    pub fault_addr_lo: u32,
    /// Faulting GPU virtual address (high 32 bits).
    pub fault_addr_hi: u32,
    /// Faulting GPU virtual address (combined 64-bit).
    pub fault_va: u64,
    /// Instance block pointer of the faulting channel (low 32 bits).
    pub fault_inst_lo: u32,
    /// Instance block pointer of the faulting channel (high 32 bits).
    pub fault_inst_hi: u32,
    /// Fault buffer 0 GET pointer.
    pub fault_buf0_get: u32,
    /// Fault buffer 0 PUT pointer.
    pub fault_buf0_put: u32,
    /// MMU control register.
    pub mmu_ctrl: u32,
    /// Hub TLB error register.
    pub hubtlb_err: u32,
    /// Whether any fault is pending (fault_status != 0 or buf0 GET != PUT).
    pub has_fault: bool,
    /// Decoded fault type string.
    pub fault_type: &'static str,
    /// Decoded fault access type string.
    pub access_type: &'static str,
    /// Decoded fault engine string.
    pub engine: &'static str,
    /// Decoded aperture.
    pub aperture: &'static str,
}

/// Read and decode the full MMU fault state from BAR0 registers.
///
/// This reads the hub-level fault registers (not the replayable fault
/// buffer entries). For sovereign VFIO dispatch, these registers tell
/// us exactly which GPU VA translation failed and why.
pub fn read_mmu_faults(bar0: &MappedBar) -> MmuFaultInfo {
    let r = |reg: usize| bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD);

    let fault_status = r(mmu::FAULT_STATUS);
    let fault_addr_lo = r(mmu::FAULT_ADDR_LO);
    let fault_addr_hi = r(mmu::FAULT_ADDR_HI);
    let fault_inst_lo = r(mmu::FAULT_INST_LO);
    let fault_inst_hi = r(mmu::FAULT_INST_HI);
    let fault_buf0_get = r(mmu::FAULT_BUF0_GET);
    let fault_buf0_put = r(mmu::FAULT_BUF0_PUT);
    let mmu_ctrl = r(pfb::MMU_CTRL);

    // Hub TLB error register (undocumented, observed in diagnostic runs).
    let hubtlb_err = r(0x0010_4A20);

    let fault_va = (u64::from(fault_addr_hi) << 32) | u64::from(fault_addr_lo);

    // fault_buf0_put bit 31 = ENABLE (our init value); only compare index bits.
    let buf0_put_idx = fault_buf0_put & 0x7FFF_FFFF;
    let buf0_get_idx = fault_buf0_get & 0x7FFF_FFFF;
    let has_fault = fault_status != 0 || buf0_get_idx != buf0_put_idx;

    let fault_type = decode_fault_type(fault_status);
    let access_type = decode_access_type(fault_status);
    let engine = decode_engine(fault_status);
    let aperture = decode_aperture(fault_status);

    MmuFaultInfo {
        fault_status,
        fault_addr_lo,
        fault_addr_hi,
        fault_va,
        fault_inst_lo,
        fault_inst_hi,
        fault_buf0_get,
        fault_buf0_put,
        mmu_ctrl,
        hubtlb_err,
        has_fault,
        fault_type,
        access_type,
        engine,
        aperture,
    }
}

/// Log a decoded MMU fault via tracing at error level.
pub fn log_mmu_faults(info: &MmuFaultInfo) {
    if info.has_fault {
        tracing::error!(
            fault_status = format_args!("{:#010x}", info.fault_status),
            fault_va = format_args!("{:#018x}", info.fault_va),
            fault_inst = format_args!("{:#010x}_{:#010x}", info.fault_inst_hi, info.fault_inst_lo),
            fault_type = info.fault_type,
            access_type = info.access_type,
            engine = info.engine,
            aperture = info.aperture,
            mmu_ctrl = format_args!("{:#010x}", info.mmu_ctrl),
            hubtlb_err = format_args!("{:#010x}", info.hubtlb_err),
            fault_buf0 = format_args!("GET={} PUT={}", info.fault_buf0_get, info.fault_buf0_put),
            "MMU fault detected"
        );
    } else {
        tracing::debug!(
            fault_status = format_args!("{:#010x}", info.fault_status),
            mmu_ctrl = format_args!("{:#010x}", info.mmu_ctrl),
            hubtlb_err = format_args!("{:#010x}", info.hubtlb_err),
            "No MMU fault pending"
        );
    }
}

/// Decode the fault type from bits [3:0] of FAULT_STATUS.
///
/// Source: nouveau `nvkm/subdev/fault/gv100.c` `gv100_fault_type[]`.
fn decode_fault_type(status: u32) -> &'static str {
    match status & 0xF {
        0x0 => "PDE (page directory entry invalid)",
        0x1 => "PDE_SIZE (page directory size mismatch)",
        0x2 => "PTE (page table entry invalid)",
        0x3 => "VA_LIMIT_VIOLATION",
        0x4 => "UNBOUND_INST_BLOCK",
        0x5 => "PRIV_VIOLATION",
        0x6 => "RO_VIOLATION (write to read-only)",
        0x7 => "WO_VIOLATION (read from write-only)",
        0x8 => "PITCH_MASK_VIOLATION",
        0x9 => "WORK_CREATION",
        0xA => "UNSUPPORTED_APERTURE",
        0xB => "COMPRESSION_FAILURE",
        0xC => "UNSUPPORTED_KIND",
        0xD => "REGION_VIOLATION",
        0xE => "POISONED",
        _ => "UNKNOWN",
    }
}

/// Decode the access type from bits [7:4] of FAULT_STATUS.
fn decode_access_type(status: u32) -> &'static str {
    match (status >> 4) & 0xF {
        0x0 => "VIRT_READ",
        0x1 => "VIRT_WRITE",
        0x2 => "VIRT_ATOMIC_STRONG",
        0x3 => "VIRT_PREFETCH",
        0x4 => "VIRT_ATOMIC_WEAK",
        0x8 => "PHYS_READ",
        0x9 => "PHYS_WRITE",
        0xA => "PHYS_ATOMIC",
        0xB => "PHYS_PREFETCH",
        _ => "UNKNOWN_ACCESS",
    }
}

/// Decode the faulting engine from bits [15:8] of FAULT_STATUS.
///
/// Partial decode — the full table has ~60 entries per GPU.
fn decode_engine(status: u32) -> &'static str {
    match (status >> 8) & 0xFF {
        0x00 => "GR (graphics/compute)",
        0x01 => "DISPLAY",
        0x02 => "IFB (internal framebuffer)",
        0x03 => "BAR1",
        0x04 => "BAR2",
        0x05 => "HOST (PFIFO/PBDMA)",
        0x06 => "HOST_CPU",
        0x07 => "HOST_CPU_NB",
        0x08..=0x0F => "CE (copy engine)",
        0x1F => "PHYSICAL",
        _ => "OTHER_ENGINE",
    }
}

/// Decode the aperture from bits [17:16] of FAULT_STATUS.
fn decode_aperture(status: u32) -> &'static str {
    match (status >> 16) & 0x3 {
        0 => "VRAM",
        1 => "SYS_MEM_COH",
        2 => "SYS_MEM_NCOH",
        3 => "PEER",
        _ => "UNKNOWN_APERTURE",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fault_type_decode_covers_known_types() {
        assert_eq!(decode_fault_type(0x0), "PDE (page directory entry invalid)");
        assert_eq!(decode_fault_type(0x2), "PTE (page table entry invalid)");
        assert_eq!(decode_fault_type(0x3), "VA_LIMIT_VIOLATION");
        assert_eq!(decode_fault_type(0x4), "UNBOUND_INST_BLOCK");
        assert_eq!(decode_fault_type(0xA), "UNSUPPORTED_APERTURE");
        assert_eq!(decode_fault_type(0xF), "UNKNOWN");
    }

    #[test]
    fn access_type_decode() {
        assert_eq!(decode_access_type(0x00), "VIRT_READ");
        assert_eq!(decode_access_type(0x10), "VIRT_WRITE");
        assert_eq!(decode_access_type(0x30), "VIRT_PREFETCH");
    }

    #[test]
    fn engine_decode() {
        assert_eq!(decode_engine(0x0000), "GR (graphics/compute)");
        assert_eq!(decode_engine(0x0500), "HOST (PFIFO/PBDMA)");
    }

    #[test]
    fn aperture_decode() {
        assert_eq!(decode_aperture(0x0_0000), "VRAM");
        assert_eq!(decode_aperture(0x1_0000), "SYS_MEM_COH");
        assert_eq!(decode_aperture(0x2_0000), "SYS_MEM_NCOH");
    }
}
