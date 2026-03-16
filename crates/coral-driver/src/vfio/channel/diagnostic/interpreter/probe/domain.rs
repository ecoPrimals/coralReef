// SPDX-License-Identifier: AGPL-3.0-only
#![allow(missing_docs)]
//! Domain probing — BAR, identity, power, engine topology.

use std::os::fd::RawFd;

use crate::vfio::channel::registers::{misc, mmu, pbdma, pfifo, pmc};
use crate::vfio::device::MappedBar;
use crate::vfio::dma::DmaBuffer;

use super::super::layers::*;

fn r(bar0: &MappedBar, reg: usize) -> u32 {
    bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD)
}

fn w(bar0: &MappedBar, reg: usize, val: u32) {
    let _ = bar0.write_u32(reg, val);
}

/// Layer 0: BAR topology — MMIO accessibility.
pub fn probe_bar(bar0: &MappedBar) -> BarTopology {
    let boot0 = r(bar0, 0);
    let all_ff = boot0 == 0xFFFF_FFFF;
    let all_zero = boot0 == 0;
    let in_d3hot = all_ff;

    // Test BAR0 write capability. NV_PMC_SCRATCH_0 (0x1400) doesn't exist on GV100.
    // PRAMIN window (0x1700) is writable on all arches: steer it and restore.
    let bar0_writable = if !in_d3hot {
        let saved_window = r(bar0, 0x1700);
        w(bar0, 0x1700, 0x0001_0000);
        let readback = r(bar0, 0x1700);
        w(bar0, 0x1700, saved_window);
        readback == 0x0001_0000
    } else {
        false
    };

    BarTopology {
        bar0_readable: !all_ff && !all_zero,
        bar0_writable,
        boot0_raw: boot0,
        in_d3hot,
    }
}

/// Layer 1: GPU identity from BOOT0.
pub fn probe_identity(bar0: &MappedBar, bar: BarTopology) -> GpuIdentity {
    let boot0 = bar.boot0_raw;
    let arch = GpuArch::from_boot0(boot0);
    let implementation = ((boot0 >> 20) & 0xFF) as u8;
    let revision = (boot0 & 0xFF) as u8;
    let boot42 = {
        let v = r(bar0, 0x0000_00A8);
        if v != 0 && v != 0xDEAD_DEAD && v != 0xBAD0_0200 {
            Some(v)
        } else {
            None
        }
    };

    GpuIdentity {
        bar,
        boot0,
        architecture: arch,
        implementation,
        revision,
        boot42,
    }
}

fn check_ptimer(bar0: &MappedBar) -> bool {
    let t0 = r(bar0, 0x009400);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let t1 = r(bar0, 0x009400);
    t0 != t1 && t0 != 0xDEAD_DEAD && t0 != 0xBAD0_DA00
}

/// Layer 2: Power state — PMC, PFIFO bring-up.
pub fn probe_power(bar0: &MappedBar, identity: GpuIdentity) -> Result<PowerState, ProbeFailure> {
    let pmc_initial = r(bar0, pmc::ENABLE);
    let pfifo_reg = r(bar0, pfifo::ENABLE);

    // On GV100 (Volta), NV_PFIFO_ENGINE (0x2200) does NOT exist — the nouveau oracle
    // also reads 0 here. PFIFO is controlled purely via PMC_ENABLE bit 8.
    let is_volta_plus = matches!(
        identity.architecture,
        GpuArch::Volta | GpuArch::Turing | GpuArch::Ampere | GpuArch::Ada | GpuArch::Blackwell
    );

    let pbdma_map = r(bar0, pfifo::PBDMA_MAP);
    let pfifo_functional = if is_volta_plus {
        let pmc_pfifo_bit = pmc_initial & (1 << 8) != 0;
        let pbdma_alive = pbdma_map != 0 && pbdma_map != 0xBAD0_DA00;
        pmc_pfifo_bit && pbdma_alive
    } else {
        pfifo_reg == 1
    };

    let already_warm = pmc_initial != 0x4000_0020 && pfifo_reg != 0xBAD0_DA00 && pfifo_functional;

    if already_warm {
        let ptimer = check_ptimer(bar0);
        return Ok(PowerState {
            identity,
            pmc_enable_initial: pmc_initial,
            pmc_enable_final: pmc_initial,
            engines_present: pmc_initial,
            pfifo_enabled: true,
            pfifo_enable_raw: pfifo_reg,
            method: PowerMethod::AlreadyWarm,
            ptimer_ticking: ptimer,
        });
    }

    // Step 1: PMC_ENABLE — clock all engines
    eprintln!("║ L2: PMC_ENABLE={pmc_initial:#010x} PBDMA_MAP={pbdma_map:#010x} — warming");
    w(bar0, pmc::ENABLE, 0xFFFF_FFFF);
    std::thread::sleep(std::time::Duration::from_millis(50));
    let _pmc_after = r(bar0, pmc::ENABLE);

    // Step 2: On pre-Volta, try direct PFIFO enable
    if !is_volta_plus {
        w(bar0, pfifo::ENABLE, 1);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    // Step 3: PMC reset cycle for PFIFO engine (bit 8)
    let pfifo_bit: u32 = 1 << 8;
    let pmc_cur = r(bar0, pmc::ENABLE);
    w(bar0, pmc::ENABLE, pmc_cur & !pfifo_bit);
    std::thread::sleep(std::time::Duration::from_millis(20));
    w(bar0, pmc::ENABLE, pmc_cur | pfifo_bit);
    std::thread::sleep(std::time::Duration::from_millis(50));

    if !is_volta_plus {
        w(bar0, pfifo::ENABLE, 1);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let pmc_final = r(bar0, pmc::ENABLE);
    let pfifo_final_reg = r(bar0, pfifo::ENABLE);
    let pbdma_after = r(bar0, pfifo::PBDMA_MAP);
    let ptimer = check_ptimer(bar0);

    let pfifo_ok = if is_volta_plus {
        let has_bit8 = pmc_final & pfifo_bit != 0;
        let has_pbdma = pbdma_after != 0 && pbdma_after != 0xBAD0_DA00;
        eprintln!(
            "║ L2: PMC={pmc_final:#010x} bit8={has_bit8} PBDMA_MAP={pbdma_after:#010x} ptimer={ptimer}"
        );
        has_bit8 && has_pbdma
    } else {
        pfifo_final_reg == 1
    };

    let method = if pfifo_ok {
        PowerMethod::PmcResetCycle
    } else if pmc_final != 0x4000_0020 {
        PowerMethod::GlowPlug
    } else {
        PowerMethod::Failed
    };

    Ok(PowerState {
        identity,
        pmc_enable_initial: pmc_initial,
        pmc_enable_final: pmc_final,
        engines_present: pmc_final,
        pfifo_enabled: pfifo_ok,
        pfifo_enable_raw: pfifo_final_reg,
        method,
        ptimer_ticking: ptimer,
    })
}

/// Layer 3: Engine topology — PBDMAs, runlists, BAR2.
#[expect(clippy::cast_possible_truncation)]
pub fn probe_engines(
    bar0: &MappedBar,
    container_fd: RawFd,
    power: PowerState,
) -> Result<EngineTopology, ProbeFailure> {
    use crate::vfio::channel::registers::FAULT_BUF_IOVA;

    let pbdma_map = r(bar0, pfifo::PBDMA_MAP);
    if pbdma_map == 0 || pbdma_map == 0xBAD0_DA00 {
        return Err(ProbeFailure {
            layer: "L3_ENGINES",
            step: "pbdma_map",
            evidence: vec![
                ("PBDMA_MAP".into(), pbdma_map),
                ("PFIFO_ENABLE".into(), power.pfifo_enable_raw),
            ],
            message: "No PBDMAs detected — PFIFO not functional".into(),
        });
    }

    let mut pbdma_to_runlist = Vec::new();
    let mut gr_runlist: Option<u32> = None;
    let mut gr_pbdma: Option<usize> = None;
    let mut alt_pbdma: Option<usize> = None;

    // Enumerate PBDMA → runlist mapping
    let mut seq = 0_usize;
    for pid in 0..32_usize {
        if pbdma_map & (1 << pid) == 0 {
            continue;
        }
        let rl = r(bar0, 0x2390 + seq * 4);
        pbdma_to_runlist.push((pid, rl));
        seq += 1;
    }

    // Find GR engine's runlist via PTOP engine table
    let mut cur_type: u32 = 0xFFFF;
    let mut cur_rl: u32 = 0xFFFF;
    for i in 0..64_u32 {
        let data = r(bar0, 0x0002_2700 + (i as usize) * 4);
        if data == 0 {
            break;
        }
        match data & 3 {
            1 => cur_type = (data >> 2) & 0x3F,
            3 => cur_rl = (data >> 11) & 0x1F,
            _ => {}
        }
        if data & (1 << 31) != 0 {
            if cur_type == 0 && gr_runlist.is_none() && cur_rl != 0xFFFF {
                gr_runlist = Some(cur_rl);
            }
            cur_type = 0xFFFF;
            cur_rl = 0xFFFF;
        }
    }

    // Fallback: use ENGN0_STATUS
    if gr_runlist.is_none() {
        let engn0 = r(bar0, 0x2640);
        let rl = (engn0 >> 12) & 0xF;
        if rl <= 31 {
            gr_runlist = Some(rl);
        }
    }

    // Find PBDMAs serving the GR runlist.
    if let Some(target_rl) = gr_runlist {
        let rl_bit = 1u32 << target_rl;
        let mut found_first = false;
        for &(pid, rl_mask) in &pbdma_to_runlist {
            if rl_mask & rl_bit != 0 {
                if !found_first {
                    gr_pbdma = Some(pid);
                    found_first = true;
                } else if alt_pbdma.is_none() {
                    alt_pbdma = Some(pid);
                }
            }
        }
    }

    // BAR block registers
    let _bar1_block = r(bar0, misc::PBUS_BAR1_BLOCK);
    let bar2_block = r(bar0, misc::PBUS_BAR2_BLOCK);
    let bar2_invalid = bar2_block == 0x4000_0000 || bar2_block == 0 || bar2_block == 0xBAD0_DA00;

    let bar2_setup_needed = bar2_invalid;
    if bar2_setup_needed {
        eprintln!("║ L3: BAR2_BLOCK={bar2_block:#010x} (invalid) — will need page table setup");
        if let Err(e) = crate::vfio::channel::pfifo::setup_bar2_page_table(bar0) {
            eprintln!("║ L3: BAR2 setup failed: {e}");
        }
    }

    // Enable PBDMA and HCE interrupts for all active PBDMAs
    for pid in 0..32_usize {
        if pbdma_map & (1 << pid) == 0 {
            continue;
        }
        w(bar0, pbdma::intr(pid), 0xFFFF_FFFF);
        w(bar0, pbdma::intr_en(pid), 0xEFFF_FEFF);
        w(bar0, pbdma::hce_intr(pid), 0xFFFF_FFFF);
        w(bar0, pbdma::hce_intr_en(pid), 0x8000_001F);
    }

    // Enable PFIFO interrupts
    w(bar0, pfifo::INTR, 0xFFFF_FFFF);
    w(bar0, pfifo::INTR_EN, 0x6181_0101);

    // MMU fault buffers — allocate in system memory DMA (not VRAM).
    {
        let fb = DmaBuffer::new(container_fd, 4096, FAULT_BUF_IOVA);
        if let Ok(fault_buf) = &fb {
            fault_buf.as_slice(); // ensure mlock'd
        }
        let fb_lo = (FAULT_BUF_IOVA >> 12) as u32;
        let fb_entries: u32 = 64;
        w(bar0, mmu::FAULT_BUF0_LO, fb_lo);
        w(bar0, mmu::FAULT_BUF0_HI, 0);
        w(bar0, mmu::FAULT_BUF0_SIZE, fb_entries);
        w(bar0, mmu::FAULT_BUF0_PUT, 0x8000_0000); // enable bit
        w(bar0, mmu::FAULT_BUF1_LO, fb_lo);
        w(bar0, mmu::FAULT_BUF1_HI, 0);
        w(bar0, mmu::FAULT_BUF1_SIZE, fb_entries);
        w(bar0, mmu::FAULT_BUF1_PUT, 0x8000_0000);
        eprintln!(
            "║ L3: MMU fault buffers (DMA): BUF0_LO={:#x} BUF1_LO={:#x} (IOVA={FAULT_BUF_IOVA:#x})",
            r(bar0, mmu::FAULT_BUF0_LO),
            r(bar0, mmu::FAULT_BUF1_LO),
        );
        std::mem::drop(fb);
    }

    Ok(EngineTopology {
        power,
        pbdma_map,
        pbdma_to_runlist,
        gr_runlist,
        gr_pbdma,
        alt_pbdma,
        bar1_block: r(bar0, misc::PBUS_BAR1_BLOCK),
        bar2_block: r(bar0, misc::PBUS_BAR2_BLOCK),
        bar2_setup_needed,
    })
}
