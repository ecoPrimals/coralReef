// SPDX-License-Identifier: AGPL-3.0-only

//! Post-boot diagnostic capture for SEC2/ACR falcon.
//!
//! Extracts falcon register dumps, TRACEPC, DMEM/IMEM snapshots,
//! and FBHUB/MMU fault state into a reusable function. Called by
//! [`strategy_sysmem`](super::strategy_sysmem) after the boot polling loop.

use crate::vfio::channel::registers::falcon;
use crate::vfio::device::MappedBar;

use super::instance_block::SEC2_FLCN_BIND_INST;
use super::sec2_hal::sec2_dmem_read;

/// Capture post-boot diagnostics from the SEC2 falcon and GPU MMU.
///
/// Appends diagnostic lines to `notes`. Call after the boot polling loop
/// but before the final `PostBootCapture`.
pub fn capture_post_boot_diagnostics(bar0: &MappedBar, base: usize, notes: &mut Vec<String>) {
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };

    let exci = r(falcon::EXCI);
    let sctl_post = r(falcon::SCTL);
    let hs_mode = sctl_post & 0x02 != 0;
    notes.push(format!(
        "Diag: EXCI={exci:#010x} SCTL={sctl_post:#010x} HS={hs_mode}"
    ));

    // Falcon interrupt + exception register dump
    {
        let irqstat = r(0x008);
        let irqmode = r(0x00C);
        let irqmask = r(0x010);
        let debug1 = r(0x090);
        let exci_raw = r(0x148);
        notes.push(format!(
            "Falcon IRQ: IRQSTAT={irqstat:#010x} MODE={irqmode:#010x} MASK={irqmask:#010x} DEBUG1={debug1:#010x}"
        ));
        notes.push(format!(
            "EXCI raw={exci_raw:#010x}: cause[31:24]={:#04x} tracepc_cnt[23:16]={} pc_lo[15:0]={:#06x}",
            (exci_raw >> 24) & 0xFF,
            (exci_raw >> 16) & 0xFF,
            exci_raw & 0xFFFF
        ));
    }

    // TRACEPC dump
    let tidx = r(0x148);
    let nr_traces = ((tidx & 0x00FF_0000) >> 16).min(32);
    if nr_traces > 0 {
        let mut traces = Vec::new();
        for i in 0..nr_traces {
            w(0x148, i);
            let tpc = r(0x14C);
            traces.push(format!("{tpc:#06x}"));
        }
        notes.push(format!("TRACEPC[0..{nr_traces}]: {}", traces.join(" ")));
    }

    // DMA state
    let fbif_post = r(falcon::FBIF_TRANSCFG);
    let itfen_post = r(falcon::ITFEN);
    let dma_10c = r(falcon::DMACTL);
    let dma_bind = r(SEC2_FLCN_BIND_INST);
    let dmatrfbase = r(0x110);
    let dmatrfmoffs = r(0x114);
    let dmatrfcmd = r(0x118);
    let dmatrffboffs = r(0x11C);
    let dmaidx_604 = r(0x604);
    notes.push(format!(
        "DMA state: FBIF={fbif_post:#x} ITFEN={itfen_post:#x} DMACTL={dma_10c:#x} DMAIDX_604={dmaidx_604:#x}"
    ));
    notes.push(format!("DMA bind: bind_inst={dma_bind:#010x}"));
    notes.push(format!(
        "DMA xfer: base={dmatrfbase:#010x} moffs={dmatrfmoffs:#010x} cmd={dmatrfcmd:#010x} fboffs={dmatrffboffs:#010x}"
    ));

    // FBHUB + GPU MMU fault diagnostics
    {
        let mmu_ctrl = bar0.read_u32(0x100C80).unwrap_or(0xDEAD);
        let mmu_fault_status = bar0.read_u32(0x100E10).unwrap_or(0xDEAD);
        let mmu_fault_lo = bar0.read_u32(0x100E14).unwrap_or(0xDEAD);
        let mmu_fault_hi = bar0.read_u32(0x100E18).unwrap_or(0xDEAD);
        let mmu_fault_alt = bar0.read_u32(0x100A2C).unwrap_or(0xDEAD);
        let mem_ctrl = bar0.read_u32(0x100804).unwrap_or(0xDEAD);
        let mem_ack = bar0.read_u32(0x100808).unwrap_or(0xDEAD);
        notes.push(format!(
            "FBHUB: MMU_CTRL={mmu_ctrl:#010x} FAULT_STATUS={mmu_fault_status:#010x} FAULT_ADDR={mmu_fault_hi:#010x}_{mmu_fault_lo:#010x}"
        ));
        notes.push(format!(
            "FBHUB: ALT_FAULT={mmu_fault_alt:#010x} MEM_CTRL={mem_ctrl:#010x} MEM_ACK={mem_ack:#010x}"
        ));
        if mmu_fault_status != 0 && mmu_fault_status != 0xDEAD {
            let fault_type = mmu_fault_status & 0xF;
            let client = (mmu_fault_status >> 8) & 0x7F;
            let engine = (mmu_fault_status >> 16) & 0x3F;
            notes.push(format!(
                "FBHUB fault decoded: type={fault_type} client={client:#x} engine={engine:#x} valid={}",
                mmu_fault_status & 0x80000000 != 0
            ));
        }
    }

    // IMEM dump around crash PC (0x0500)
    {
        let _ = bar0.write_u32(base + 0x180, (1u32 << 25) | 0x04C0);
        let mut imem_words = Vec::new();
        for _ in 0..32 {
            imem_words.push(bar0.read_u32(base + 0x184).unwrap_or(0xDEAD));
        }
        let hex: Vec<String> = imem_words
            .iter()
            .enumerate()
            .map(|(i, &w)| format!("[{:#05x}]={w:#010x}", 0x4C0 + i * 4))
            .collect();
        notes.push(format!("IMEM[0x4C0..0x540]: {}", hex.join(" ")));
    }

    // DMEM diagnostic: BL descriptor region (0x00..0x54)
    let dmem_bl = sec2_dmem_read(bar0, 0x00, 0x54);
    let bl_vals: Vec<String> = dmem_bl
        .iter()
        .enumerate()
        .filter(|&(_, &w)| w != 0 && w != 0xDEAD_DEAD)
        .take(8)
        .map(|(i, &w)| format!("[{:#05x}]={w:#010x}", i * 4))
        .collect();
    notes.push(format!(
        "DMEM[0x00..0x54] (BL desc): {}",
        if bl_vals.is_empty() {
            "ALL ZERO/DEAD".to_string()
        } else {
            bl_vals.join(" ")
        }
    ));

    // DMEM diagnostic: ACR descriptor region (0x200..0x270)
    let dmem_acr = sec2_dmem_read(bar0, 0x200, 0x70);
    let acr_vals: Vec<String> = dmem_acr
        .iter()
        .enumerate()
        .filter(|&(_, &w)| w != 0 && w != 0xDEAD_DEAD && w != 0xDEAD_5EC2)
        .map(|(i, &w)| format!("[{:#05x}]={w:#010x}", 0x200 + i * 4))
        .collect();
    notes.push(format!(
        "DMEM[0x200..0x270] (ACR desc): {}",
        if acr_vals.is_empty() {
            "ALL ZERO/DEAD/5EC2".to_string()
        } else {
            acr_vals.join(" ")
        }
    ));
    let raw8: Vec<String> = dmem_acr
        .iter()
        .take(8)
        .map(|w| format!("{w:#010x}"))
        .collect();
    notes.push(format!("DMEM[0x200] raw: {}", raw8.join(" ")));

    // MMU TLB state after boot
    {
        let tlb_hi = bar0.read_u32(0x100CEC).unwrap_or(0);
        let reg_cf0 = bar0.read_u32(0x100CF0).unwrap_or(0);
        notes.push(format!(
            "MMU post-boot: TLB_hi={tlb_hi:#010x} 0xCF0={reg_cf0:#010x}"
        ));
    }
}
