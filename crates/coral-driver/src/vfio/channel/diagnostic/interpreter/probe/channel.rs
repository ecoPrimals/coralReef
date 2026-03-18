// SPDX-License-Identifier: AGPL-3.0-only
#![allow(missing_docs)]
//! Channel config — Layer 5 probing.

use std::os::fd::RawFd;

use crate::vfio::channel::registers::{
    PBDMA_TARGET_SYS_MEM_COHERENT, TARGET_SYS_MEM_COHERENT, TARGET_SYS_MEM_NONCOHERENT, mmu, pbdma,
    pccsr, pfifo, ramuserd, usermode,
};
use crate::vfio::device::MappedBar;
use crate::vfio::dma::DmaBuffer;

use super::super::layers::*;

fn r(bar0: &MappedBar, reg: usize) -> u32 {
    bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD)
}

fn w(bar0: &MappedBar, reg: usize, val: u32) {
    let _ = bar0.write_u32(reg, val);
}

/// Layer 5: Channel config — scheduler + doorbell + GP_GET.
pub fn probe_channel(
    bar0: &MappedBar,
    container_fd: RawFd,
    dma: &DmaCapability,
) -> Result<ChannelConfig, ProbeFailure> {
    eprintln!("╠══ L5: Channel — scheduler + doorbell + GP_GET ═════════╣");

    let engines = &dma.engines;
    let channel_id: u32 = 1;
    let pbdma_id = engines.gr_pbdma.unwrap_or(1);
    let pb = pbdma::BASE + pbdma_id * pbdma::STRIDE;

    let l5_gpfifo_iova: u64 = 0x10_1000;
    let l5_userd_iova: u64 = 0x10_2000;
    let l5_inst_iova: u64 = 0x10_3000;
    let l5_pd3_iova: u64 = 0x10_5000;
    let l5_pd2_iova: u64 = 0x10_6000;
    let l5_pd1_iova: u64 = 0x10_7000;
    let l5_pd0_iova: u64 = 0x10_8000;
    let l5_pt0_iova: u64 = 0x10_9000;
    let l5_runlist_iova: u64 = 0x10_4000;

    let mut instance =
        DmaBuffer::new(container_fd, 4096, l5_inst_iova).map_err(|e| ProbeFailure {
            layer: "L5_CHANNEL",
            step: "alloc_instance",
            evidence: vec![],
            message: format!("DMA alloc instance: {e}"),
        })?;
    let mut gpfifo =
        DmaBuffer::new(container_fd, 4096, l5_gpfifo_iova).map_err(|e| ProbeFailure {
            layer: "L5_CHANNEL",
            step: "alloc_gpfifo",
            evidence: vec![],
            message: format!("DMA alloc gpfifo: {e}"),
        })?;
    let mut userd =
        DmaBuffer::new(container_fd, 4096, l5_userd_iova).map_err(|e| ProbeFailure {
            layer: "L5_CHANNEL",
            step: "alloc_userd",
            evidence: vec![],
            message: format!("DMA alloc userd: {e}"),
        })?;

    let mut pd3 = DmaBuffer::new(container_fd, 4096, l5_pd3_iova).ok();
    let mut pd2 = DmaBuffer::new(container_fd, 4096, l5_pd2_iova).ok();
    let mut pd1 = DmaBuffer::new(container_fd, 4096, l5_pd1_iova).ok();
    let mut pd0 = DmaBuffer::new(container_fd, 4096, l5_pd0_iova).ok();
    let mut pt0 = DmaBuffer::new(container_fd, 4096, l5_pt0_iova).ok();

    let pt_ok = pd3.is_some() && pd2.is_some() && pd1.is_some() && pd0.is_some() && pt0.is_some();

    if pt_ok {
        crate::vfio::channel::page_tables::populate_page_tables_custom(
            pd3.as_mut().unwrap().as_mut_slice(),
            pd2.as_mut().unwrap().as_mut_slice(),
            pd1.as_mut().unwrap().as_mut_slice(),
            pd0.as_mut().unwrap().as_mut_slice(),
            pt0.as_mut().unwrap().as_mut_slice(),
            l5_pd2_iova,
            l5_pd1_iova,
            l5_pd0_iova,
            l5_pt0_iova,
        );
    }

    crate::vfio::channel::page_tables::populate_instance_block_custom(
        instance.as_mut_slice(),
        l5_gpfifo_iova,
        8,
        l5_userd_iova,
        channel_id,
        l5_pd3_iova,
    );

    let gp = gpfifo.as_mut_slice();
    gp[0..4].copy_from_slice(&0u32.to_le_bytes());
    gp[4..8].copy_from_slice(&0u32.to_le_bytes());

    let us = userd.as_mut_slice();
    us[ramuserd::GP_PUT..ramuserd::GP_PUT + 4].copy_from_slice(&1u32.to_le_bytes());
    us[ramuserd::GP_GET..ramuserd::GP_GET + 4].copy_from_slice(&0u32.to_le_bytes());

    #[cfg(target_arch = "x86_64")]
    {
        for buf in [instance.as_slice(), gpfifo.as_slice(), userd.as_slice()] {
            crate::vfio::cache_ops::clflush_range(buf);
        }
        if let Some(ref p) = pd3 {
            crate::vfio::cache_ops::clflush_range(p.as_slice());
        }
        crate::vfio::cache_ops::memory_fence();
    }

    w(bar0, 0x100CBC, 1);
    std::thread::sleep(std::time::Duration::from_millis(5));

    w(bar0, pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_CLR);
    std::thread::sleep(std::time::Duration::from_millis(5));
    w(
        bar0,
        pccsr::channel(channel_id),
        pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET,
    );
    w(bar0, pccsr::inst(channel_id), 0);
    w(bar0, pfifo::INTR, 0xFFFF_FFFF);
    w(bar0, pbdma::intr(pbdma_id), 0xFFFF_FFFF);
    std::thread::sleep(std::time::Duration::from_millis(5));

    w(bar0, pfifo::INTR_EN, 0x7FFF_FFFF);

    let mut working_target = 0u32;
    let mut scheduling_method = SchedulingMethod::None;
    let inst_bind_needed = true;
    let mut runlist_ack = false;

    struct L5Attempt {
        label: &'static str,
        inst_target: u32,
        inst_addr_shr12: u32,
        submit_runlist: bool,
    }

    let inst_sysmem = (l5_inst_iova >> 12) as u32;

    let attempts = [
        L5Attempt {
            label: "sched_sysmem_coh",
            inst_target: TARGET_SYS_MEM_COHERENT,
            inst_addr_shr12: inst_sysmem,
            submit_runlist: true,
        },
        L5Attempt {
            label: "sched_sysmem_ncoh",
            inst_target: TARGET_SYS_MEM_NONCOHERENT,
            inst_addr_shr12: inst_sysmem,
            submit_runlist: true,
        },
    ];

    for attempt in &attempts {
        w(bar0, pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_CLR);
        std::thread::sleep(std::time::Duration::from_millis(5));
        w(
            bar0,
            pccsr::channel(channel_id),
            pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET,
        );
        w(bar0, pccsr::inst(channel_id), 0);
        w(bar0, pfifo::INTR, 0xFFFF_FFFF);
        w(bar0, pbdma::intr(pbdma_id), 0xFFFF_FFFF);
        std::thread::sleep(std::time::Duration::from_millis(5));

        w(bar0, pb + pbdma::CTX_SIGNATURE, 0xBEEF_0010);
        w(bar0, pb + pbdma::CTX_GP_BASE_LO, 0xBEEF_0048);

        let us = userd.as_mut_slice();
        us[ramuserd::GP_GET..ramuserd::GP_GET + 4].copy_from_slice(&0xDEADu32.to_le_bytes());
        #[cfg(target_arch = "x86_64")]
        {
            crate::vfio::cache_ops::clflush_range(&us[ramuserd::GP_GET..]);
            crate::vfio::cache_ops::memory_fence();
        }

        let inst_val =
            attempt.inst_addr_shr12 | (attempt.inst_target << 28) | pccsr::INST_BIND_TRUE;
        w(bar0, pccsr::inst(channel_id), inst_val);
        std::thread::sleep(std::time::Duration::from_millis(5));

        w(bar0, pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
        std::thread::sleep(std::time::Duration::from_millis(5));

        if attempt.submit_runlist {
            let mut runlist = DmaBuffer::new(container_fd, 4096, l5_runlist_iova).ok();
            if let Some(ref mut rl) = runlist {
                let rl_data = rl.as_mut_slice();
                let tsg_id: u32 = 0;
                rl_data[0..4].copy_from_slice(&((tsg_id << 3) | 4).to_le_bytes());
                rl_data[4..8].copy_from_slice(&0x8000_0000u32.to_le_bytes());
                rl_data[8..12].copy_from_slice(&1u32.to_le_bytes());
                rl_data[12..16].copy_from_slice(&0u32.to_le_bytes());
                rl_data[16..20].copy_from_slice(&channel_id.to_le_bytes());
                rl_data[20..24].copy_from_slice(&0u32.to_le_bytes());

                #[cfg(target_arch = "x86_64")]
                {
                    crate::vfio::cache_ops::clflush_range(&rl_data[..64]);
                    crate::vfio::cache_ops::memory_fence();
                }

                let gr_rl = engines.gr_runlist.unwrap_or(1);
                w(
                    bar0,
                    pfifo::runlist_base(gr_rl),
                    pfifo::gv100_runlist_base_value(l5_runlist_iova),
                );
                w(
                    bar0,
                    pfifo::runlist_submit(gr_rl),
                    pfifo::gv100_runlist_submit_value(l5_runlist_iova, 2),
                );
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }

        w(bar0, usermode::NOTIFY_CHANNEL_PENDING, channel_id);
        std::thread::sleep(std::time::Duration::from_millis(100));

        let ctrl = r(bar0, pccsr::channel(channel_id));
        let status = pccsr::status(ctrl);
        let status_name = pccsr::status_name(ctrl);
        let sig = r(bar0, pb + pbdma::CTX_SIGNATURE);
        let gpbase = r(bar0, pb + pbdma::CTX_GP_BASE_LO);
        let gp_put = r(bar0, pb + pbdma::CTX_GP_PUT);
        let gp_get = r(bar0, pb + pbdma::CTX_GP_FETCH);
        let chsw = r(bar0, pfifo::CHSW_ERROR);
        let intr = r(bar0, pfifo::INTR);
        let pbdma_intr_val = r(bar0, pbdma::intr(pbdma_id));

        #[cfg(target_arch = "x86_64")]
        {
            crate::vfio::cache_ops::clflush_range(&userd.as_slice()[ramuserd::GP_GET..]);
            crate::vfio::cache_ops::memory_fence();
        }
        let userd_gp_get = u32::from_le_bytes(
            userd.as_slice()[ramuserd::GP_GET..ramuserd::GP_GET + 4]
                .try_into()
                .unwrap(),
        );

        let context_loaded = sig != 0xBEEF_0010 && gpbase != 0xBEEF_0048;
        let sig_correct = sig == 0x0000_FACE;

        eprintln!(
            "║ L5 {}: st={status}({status_name}) SIG={sig:#x} GP_BASE={gpbase:#x}",
            attempt.label
        );
        eprintln!(
            "║   GP_GET={gp_get:#x} GP_PUT={gp_put:#x} CHSW={chsw:#x} INTR={intr:#x} \
             PBDMA_INTR={pbdma_intr_val:#x} USERD_GP_GET={userd_gp_get:#x}"
        );

        if context_loaded && sig_correct {
            eprintln!("║   ✓ Context loaded via scheduler!");
            working_target = attempt.inst_target;
            scheduling_method = SchedulingMethod::HardwareScheduler;
            if gp_get != 0 {
                eprintln!("║   ✓ GP_GET advanced to {gp_get}!");
            }
            if userd_gp_get != 0xDEAD && userd_gp_get != 0 {
                eprintln!("║   ✓ USERD GP_GET written back: {userd_gp_get:#x}!");
            }
            runlist_ack = (intr & pfifo::INTR_RL_COMPLETE) != 0;
            break;
        } else if context_loaded {
            eprintln!("║   ~ Context loaded but SIG={sig:#x} (expected 0xFACE)");
            working_target = attempt.inst_target;
            scheduling_method = SchedulingMethod::HardwareScheduler;
            break;
        } else {
            eprintln!(
                "║   ✗ Context not loaded (sentinels={} dead={})",
                (sig >> 16) == 0xBEEF,
                (sig >> 16) == 0xDEAD
            );
        }
    }

    let fault_status = r(bar0, mmu::FAULT_STATUS);
    if fault_status != 0 {
        let fault_addr_lo = r(bar0, mmu::FAULT_ADDR_LO);
        let fault_addr_hi = r(bar0, mmu::FAULT_ADDR_HI);
        let fault_inst_lo = r(bar0, mmu::FAULT_INST_LO);
        eprintln!(
            "║ L5 MMU_FAULT: status={fault_status:#x} addr={fault_addr_hi:#x}_{fault_addr_lo:#x} \
             inst={fault_inst_lo:#x}"
        );
    }

    w(bar0, pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_CLR);
    std::thread::sleep(std::time::Duration::from_millis(5));
    w(
        bar0,
        pccsr::channel(channel_id),
        pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET,
    );
    w(bar0, pccsr::inst(channel_id), 0);
    w(bar0, pfifo::INTR, 0xFFFF_FFFF);

    Ok(ChannelConfig {
        dma: dma.clone(),
        working_inst_target: working_target,
        working_userd_target: PBDMA_TARGET_SYS_MEM_COHERENT,
        instance_requires_vram: working_target == 0,
        userd_requires_vram: false,
        inst_bind_needed,
        runlist_ack_works: runlist_ack,
        scheduling_method,
    })
}
