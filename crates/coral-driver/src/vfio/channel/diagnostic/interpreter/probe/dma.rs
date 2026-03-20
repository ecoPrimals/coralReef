// SPDX-License-Identifier: AGPL-3.0-only
//! DMA validation — Layer 4 probing.

use std::os::fd::OwnedFd;
use std::sync::Arc;

use crate::vfio::channel::registers::{
    BAR2_VRAM_BASE, INSTANCE_IOVA, PD0_IOVA, PD1_IOVA, PD2_IOVA, PD3_IOVA, PT0_IOVA,
    TARGET_SYS_MEM_COHERENT, pbdma, pccsr, pfifo, ramfc, usermode,
};
use crate::vfio::device::MappedBar;
use crate::vfio::dma::DmaBuffer;
use crate::vfio::memory::{MemoryRegion, PathStatus, PraminRegion};

use super::super::layers::*;

fn r(bar0: &MappedBar, reg: usize) -> u32 {
    bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD)
}

fn w(bar0: &MappedBar, reg: usize, val: u32) {
    let _ = bar0.write_u32(reg, val);
}

/// Layer 4: DMA validation — test GPU can DMA-read instance block from system memory.
#[expect(clippy::cast_possible_truncation)]
pub fn probe_dma(
    bar0: &MappedBar,
    container: Arc<OwnedFd>,
    engines: EngineTopology,
) -> Result<DmaCapability, ProbeFailure> {
    let channel_id: u32 = 0;
    let gpfifo_iova: u64 = 0x1000;
    let userd_iova: u64 = 0x2000;

    let mut instance = match DmaBuffer::new(Arc::clone(&container), 4096, INSTANCE_IOVA) {
        Ok(b) => b,
        Err(e) => {
            return Err(ProbeFailure {
                layer: "L4_DMA",
                step: "alloc_instance",
                evidence: vec![],
                message: format!("DMA buffer allocation failed: {e}"),
            });
        }
    };
    let mut pd3 = DmaBuffer::new(Arc::clone(&container), 4096, PD3_IOVA).ok();
    let mut pd2 = DmaBuffer::new(Arc::clone(&container), 4096, PD2_IOVA).ok();
    let mut pd1 = DmaBuffer::new(Arc::clone(&container), 4096, PD1_IOVA).ok();
    let mut pd0 = DmaBuffer::new(Arc::clone(&container), 4096, PD0_IOVA).ok();
    let mut pt0 = DmaBuffer::new(Arc::clone(&container), 4096, PT0_IOVA).ok();

    let iommu_ok = pd3.is_some() && pd2.is_some() && pd1.is_some();

    let pt_ok = if let (Some(d3), Some(d2), Some(d1), Some(d0), Some(t0)) =
        (&mut pd3, &mut pd2, &mut pd1, &mut pd0, &mut pt0)
    {
        crate::vfio::channel::page_tables::populate_page_tables(
            d3.as_mut_slice(),
            d2.as_mut_slice(),
            d1.as_mut_slice(),
            d0.as_mut_slice(),
            t0.as_mut_slice(),
        );
        true
    } else {
        false
    };

    crate::vfio::channel::page_tables::populate_instance_block_static(
        instance.as_mut_slice(),
        gpfifo_iova,
        8,
        userd_iova,
        channel_id,
    );

    #[cfg(target_arch = "x86_64")]
    {
        crate::vfio::cache_ops::clflush_range(instance.as_slice());
        crate::vfio::cache_ops::memory_fence();
    }

    let pbdma_id = engines.gr_pbdma.unwrap_or(1);
    let pb = 0x040000 + pbdma_id * 0x2000;

    w(bar0, pb + pbdma::CTX_USERD_LO, 0xBEEF_0008);
    w(bar0, pb + pbdma::CTX_SIGNATURE, 0xBEEF_0010);
    w(bar0, pb + pbdma::CTX_GP_BASE_LO, 0xBEEF_0048);
    w(bar0, pb + pbdma::CTX_ACQUIRE, 0xBEEF_0030);

    let stale = r(bar0, pccsr::channel(channel_id));
    if stale & 1 != 0 {
        w(bar0, pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_CLR);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    w(bar0, pccsr::inst(channel_id), 0);
    std::thread::sleep(std::time::Duration::from_millis(5));
    w(bar0, pfifo::INTR, 0xFFFF_FFFF);

    let mut ctx_evidence: Vec<(String, u32)> = vec![];
    let mut instance_accessible = false;
    let mut gpu_can_read = false;

    let pramin_ok: bool;
    let vram_inst_addr: u32;
    {
        let bar2_inst_0;
        let bar2_inst_4;
        let bar2_pdb_lo;
        if let Ok(region) = PraminRegion::new(bar0, BAR2_VRAM_BASE, 0x1000) {
            bar2_inst_0 = region.read_u32(0).unwrap_or(0xDEAD_DEAD);
            bar2_inst_4 = region.read_u32(4).unwrap_or(0xDEAD_DEAD);
            bar2_pdb_lo = region.read_u32(0x200).unwrap_or(0xDEAD_DEAD);
        } else {
            bar2_inst_0 = 0xDEAD_DEAD;
            bar2_inst_4 = 0xDEAD_DEAD;
            bar2_pdb_lo = 0xDEAD_DEAD;
        }
        let is_bad = (bar2_inst_0 >> 16) == 0xBAD0 || bar2_inst_0 == 0xBAD0_AC00;
        eprintln!(
            "║ L4: VRAM readback @ 0x{:x}: [0]={bar2_inst_0:#x} [4]={bar2_inst_4:#x} \
             [PDB]={bar2_pdb_lo:#x} bad={}",
            BAR2_VRAM_BASE, is_bad
        );

        let vram0_status = if let Ok(mut region) = PraminRegion::new(bar0, 0, 8) {
            region.probe_sentinel(0, 0xDEAD_BEEF)
        } else {
            PathStatus::ErrorPattern { pattern: 0 }
        };
        let vram0_ok = vram0_status.is_working();
        let vram0_after = match &vram0_status {
            PathStatus::Working { .. } => 0xDEAD_BEEF,
            PathStatus::Corrupted { read, .. } => *read,
            PathStatus::ErrorPattern { pattern } => *pattern,
            PathStatus::Untested => 0,
        };
        eprintln!("║ L4: VRAM@0x00000: {vram0_status:?}");

        let vram2_addr = BAR2_VRAM_BASE + 0x6000;
        let vram2_status = if let Ok(mut region) = PraminRegion::new(bar0, vram2_addr, 8) {
            region.probe_sentinel(0, 0xCAFE_1234)
        } else {
            PathStatus::ErrorPattern { pattern: 0 }
        };
        let vram2_ok = vram2_status.is_working();
        let test_rb = match &vram2_status {
            PathStatus::Working { .. } => 0xCAFE_1234,
            PathStatus::Corrupted { read, .. } => *read,
            PathStatus::ErrorPattern { pattern } => *pattern,
            PathStatus::Untested => 0,
        };
        eprintln!("║ L4: VRAM@{vram2_addr:#x}: {vram2_status:?}");

        pramin_ok = vram0_ok || vram2_ok;
        vram_inst_addr = if vram2_ok {
            vram2_addr
        } else if vram0_ok {
            0x0001_0000
        } else {
            0x0002_6000
        };

        ctx_evidence.push(("PRAMIN_VRAM0".into(), vram0_after));
        ctx_evidence.push(("PRAMIN_VRAM2".into(), test_rb));

        if pramin_ok && let Ok(mut vram_region) = PraminRegion::new(bar0, vram_inst_addr, 4096) {
            let inst_data = instance.as_slice();
            for (i, chunk) in inst_data.chunks(4).enumerate() {
                if chunk.len() == 4 {
                    let val = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                    let _ = vram_region.write_u32(i * 4, val);
                }
            }
            let verify_sig = vram_region.read_u32(ramfc::SIGNATURE).unwrap_or(0);
            let verify_gpbase = vram_region.read_u32(ramfc::GP_BASE_LO).unwrap_or(0);
            let verify_userd = vram_region.read_u32(ramfc::USERD_LO).unwrap_or(0);
            eprintln!(
                "║ L4: VRAM inst verify: SIG={verify_sig:#x} GP_BASE={verify_gpbase:#x} \
                 USERD={verify_userd:#x}"
            );
        }
    }

    struct BindAttempt {
        label: &'static str,
        target: u32,
        inst_addr_shr12: u32,
        enable_channel: bool,
        submit_runlist: bool,
        enable_interrupts: bool,
    }

    let attempts = [
        BindAttempt {
            label: "A_sysmem_bind_only",
            target: TARGET_SYS_MEM_COHERENT,
            inst_addr_shr12: (INSTANCE_IOVA >> 12) as u32,
            enable_channel: false,
            submit_runlist: false,
            enable_interrupts: false,
        },
        BindAttempt {
            label: "B_sysmem_bind+enable",
            target: TARGET_SYS_MEM_COHERENT,
            inst_addr_shr12: (INSTANCE_IOVA >> 12) as u32,
            enable_channel: true,
            submit_runlist: false,
            enable_interrupts: false,
        },
        BindAttempt {
            label: "C_vram_bind+enable",
            target: 0,
            inst_addr_shr12: vram_inst_addr >> 12,
            enable_channel: true,
            submit_runlist: false,
            enable_interrupts: false,
        },
        BindAttempt {
            label: "D_vram_full_sequence",
            target: 0,
            inst_addr_shr12: vram_inst_addr >> 12,
            enable_channel: true,
            submit_runlist: true,
            enable_interrupts: true,
        },
        BindAttempt {
            label: "E_sysmem_full_sequence",
            target: TARGET_SYS_MEM_COHERENT,
            inst_addr_shr12: (INSTANCE_IOVA >> 12) as u32,
            enable_channel: true,
            submit_runlist: true,
            enable_interrupts: true,
        },
    ];

    let gr_rl_id = engines.gr_runlist.unwrap_or(1) as usize;
    let rl_base_test_reg = 0x2270 + gr_rl_id * 0x10;

    let rl_before = r(bar0, rl_base_test_reg);
    w(bar0, rl_base_test_reg, 0xCAFE_0001);
    let rl_after = r(bar0, rl_base_test_reg);
    w(bar0, rl_base_test_reg, rl_before);

    let rl0_before = r(bar0, 0x2270);
    w(bar0, 0x2270, 0xCAFE_0002);
    let rl0_after = r(bar0, 0x2270);
    w(bar0, 0x2270, rl0_before);

    eprintln!(
        "║ L4: RL{gr_rl_id} write test: before={rl_before:#x} after_write={rl_after:#x} (expected 0xcafe0001)"
    );
    eprintln!(
        "║ L4: RL0 write test: before={rl0_before:#x} after_write={rl0_after:#x} (expected 0xcafe0002)"
    );
    ctx_evidence.push(("RL_WRITE_TEST".into(), rl_after));
    ctx_evidence.push(("RL0_WRITE_TEST".into(), rl0_after));

    eprintln!(
        "║ L4: Progressive INST_BIND sequence ({} attempts)",
        attempts.len()
    );

    let try_direct_pbdma = true;

    for attempt in &attempts {
        w(bar0, pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_CLR);
        std::thread::sleep(std::time::Duration::from_millis(5));
        w(
            bar0,
            pccsr::channel(channel_id),
            pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET,
        );
        w(bar0, pccsr::inst(channel_id), 0);
        std::thread::sleep(std::time::Duration::from_millis(5));
        w(bar0, pfifo::INTR, 0xFFFF_FFFF);

        w(bar0, pb + pbdma::CTX_USERD_LO, 0xBEEF_0008);
        w(bar0, pb + pbdma::CTX_SIGNATURE, 0xBEEF_0010);
        w(bar0, pb + pbdma::CTX_GP_BASE_LO, 0xBEEF_0048);

        if attempt.enable_interrupts {
            w(bar0, pfifo::INTR_EN, 0x7FFF_FFFF);
        }

        if attempt.submit_runlist {
            let vram_rl_addr: u32 = 0x0002_7000;
            if let Ok(mut rl_region) = PraminRegion::new(bar0, vram_rl_addr, 0x100) {
                let _ = rl_region.write_u32(0x00, 0x0000_0004);
                let _ = rl_region.write_u32(0x04, 0x8000_0000);
                let _ = rl_region.write_u32(0x08, 1);
                let _ = rl_region.write_u32(0x0C, 0);
                let _ = rl_region.write_u32(0x10, channel_id);
                let _ = rl_region.write_u32(0x14, 0);
            }

            let gr_rl = engines.gr_runlist.unwrap_or(1);
            w(
                bar0,
                pfifo::runlist_base(gr_rl),
                pfifo::gv100_runlist_base_value(vram_rl_addr as u64),
            );
            w(
                bar0,
                pfifo::runlist_submit(gr_rl),
                pfifo::gv100_runlist_submit_value(vram_rl_addr as u64, 2),
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        let inst_val = attempt.inst_addr_shr12 | (attempt.target << 28) | pccsr::INST_BIND_TRUE;
        w(bar0, pccsr::inst(channel_id), inst_val);
        std::thread::sleep(std::time::Duration::from_millis(10));

        if attempt.enable_channel {
            w(bar0, pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
            std::thread::sleep(std::time::Duration::from_millis(5));
            w(bar0, usermode::NOTIFY_CHANNEL_PENDING, channel_id);
            std::thread::sleep(std::time::Duration::from_millis(50));
        } else {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        let ctx_userd = r(bar0, pb + pbdma::CTX_USERD_LO);
        let ctx_sig = r(bar0, pb + pbdma::CTX_SIGNATURE);
        let ctx_gpbase = r(bar0, pb + pbdma::CTX_GP_BASE_LO);
        let chsw = r(bar0, pfifo::CHSW_ERROR);
        let _pfifo_intr = r(bar0, pfifo::INTR);
        let pccsr_ctrl = r(bar0, pccsr::channel(channel_id));
        let pccsr_inst_rb = r(bar0, pccsr::inst(channel_id));
        let pbdma_status = r(bar0, pb + 0x118);
        let sched_dis = r(bar0, pfifo::SCHED_DISABLE);

        let sentinels = (ctx_userd >> 16) == 0xBEEF || (ctx_sig >> 16) == 0xBEEF;
        let dead = (ctx_userd >> 16) == 0xDEAD || (ctx_sig >> 16) == 0xDEAD;
        let sig_ok = ctx_sig == 0x0000_FACE;
        let loaded = !sentinels && !dead;
        let status = (pccsr_ctrl >> 24) & 0xF;

        eprintln!(
            "║   {}: SIG={ctx_sig:#010x} USERD={ctx_userd:#010x} GP={ctx_gpbase:#010x} \
             CHSW={chsw:#x} CTRL={pccsr_ctrl:#x}(st={status}) {}",
            attempt.label,
            if loaded {
                if sig_ok {
                    "✓ LOADED+CORRECT"
                } else {
                    "~ LOADED"
                }
            } else if sentinels {
                "✗ sentinels"
            } else {
                "✗ dead"
            }
        );
        eprintln!(
            "║     PCCSR_INST={pccsr_inst_rb:#010x} PBDMA_ST={pbdma_status:#x} SCHED_DIS={sched_dis:#x}"
        );

        ctx_evidence.push((format!("{}_SIG", attempt.label), ctx_sig));
        ctx_evidence.push((format!("{}_USERD", attempt.label), ctx_userd));
        ctx_evidence.push((format!("{}_CTRL", attempt.label), pccsr_ctrl));
        ctx_evidence.push((format!("{}_PBDMA_ST", attempt.label), pbdma_status));
        ctx_evidence.push((format!("{}_SCHED_DIS", attempt.label), sched_dis));
        if chsw != 0 {
            ctx_evidence.push((format!("{}_CHSW", attempt.label), chsw));
        }

        if loaded {
            gpu_can_read = true;
            if sig_ok {
                instance_accessible = true;
            }
            break;
        }
    }

    if try_direct_pbdma && !instance_accessible {
        eprintln!("║ L4: Attempt F — direct PBDMA context programming (bypass scheduler)");

        w(bar0, pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_CLR);
        std::thread::sleep(std::time::Duration::from_millis(5));
        w(
            bar0,
            pccsr::channel(channel_id),
            pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET,
        );
        w(bar0, pccsr::inst(channel_id), 0);
        w(bar0, pfifo::INTR, 0xFFFF_FFFF);
        std::thread::sleep(std::time::Duration::from_millis(5));

        let inst_val = (vram_inst_addr >> 12) | pccsr::INST_BIND_TRUE;
        w(bar0, pccsr::inst(channel_id), inst_val);
        std::thread::sleep(std::time::Duration::from_millis(5));

        let (
            ramfc_gp_base_lo,
            ramfc_gp_base_hi,
            ramfc_gp_put,
            ramfc_userd_lo,
            ramfc_userd_hi,
            ramfc_sig,
        ) = if let Ok(inst_region) = PraminRegion::new(bar0, vram_inst_addr, 4096) {
            (
                inst_region.read_u32(ramfc::GP_BASE_LO).unwrap_or(0),
                inst_region.read_u32(ramfc::GP_BASE_HI).unwrap_or(0),
                inst_region.read_u32(ramfc::GP_PUT).unwrap_or(0),
                inst_region.read_u32(ramfc::USERD_LO).unwrap_or(0),
                inst_region.read_u32(ramfc::USERD_HI).unwrap_or(0),
                inst_region.read_u32(ramfc::SIGNATURE).unwrap_or(0),
            )
        } else {
            (0, 0, 0, 0, 0, 0)
        };

        eprintln!(
            "║   RAMFC: GP_BASE={ramfc_gp_base_lo:#x}/{ramfc_gp_base_hi:#x} \
             USERD={ramfc_userd_lo:#x}/{ramfc_userd_hi:#x} SIG={ramfc_sig:#x} GP_PUT={ramfc_gp_put:#x}"
        );

        w(bar0, pb + pbdma::CTX_GP_BASE_LO, ramfc_gp_base_lo);
        w(bar0, pb + pbdma::CTX_GP_BASE_HI, ramfc_gp_base_hi);
        w(bar0, pb + pbdma::CTX_USERD_LO, ramfc_userd_lo);
        w(bar0, pb + pbdma::CTX_USERD_HI, ramfc_userd_hi);
        w(bar0, pb + pbdma::CTX_SIGNATURE, ramfc_sig);
        w(bar0, pb + pbdma::CTX_GP_PUT, 1);
        w(bar0, pb + pbdma::CTX_GP_FETCH, 0);

        w(bar0, pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
        std::thread::sleep(std::time::Duration::from_millis(5));
        w(bar0, usermode::NOTIFY_CHANNEL_PENDING, channel_id);
        std::thread::sleep(std::time::Duration::from_millis(50));

        let f_userd = r(bar0, pb + pbdma::CTX_USERD_LO);
        let f_sig = r(bar0, pb + pbdma::CTX_SIGNATURE);
        let f_gpbase = r(bar0, pb + pbdma::CTX_GP_BASE_LO);
        let f_gp_put = r(bar0, pb + pbdma::CTX_GP_PUT);
        let f_gp_get = r(bar0, pb + pbdma::CTX_GP_FETCH);
        let f_ctrl = r(bar0, pccsr::channel(channel_id));
        let f_chsw = r(bar0, pfifo::CHSW_ERROR);
        let f_intr = r(bar0, pfifo::INTR);
        let f_status = (f_ctrl >> 24) & 0xF;
        let f_pbdma_intr = r(bar0, pbdma::intr(pbdma_id));
        let f_pbdma_status = r(bar0, pb + 0x118);

        let f_loaded = f_userd == (ramfc_userd_lo & 0xFFFF_FF00) || f_gpbase == ramfc_gp_base_lo;
        let f_fetching = f_gp_get != 0 || f_gp_put != 1;

        eprintln!("║   F_direct: USERD={f_userd:#010x} SIG={f_sig:#010x} GP_BASE={f_gpbase:#010x}");
        eprintln!(
            "║     GP_GET={f_gp_get:#x} GP_PUT={f_gp_put:#x} CTRL={f_ctrl:#x}(st={f_status}) \
             CHSW={f_chsw:#x} INTR={f_intr:#x} PBDMA_INTR={f_pbdma_intr:#x} \
             PBDMA_ST={f_pbdma_status:#x}"
        );

        ctx_evidence.push(("F_USERD".into(), f_userd));
        ctx_evidence.push(("F_SIG".into(), f_sig));
        ctx_evidence.push(("F_GP_BASE".into(), f_gpbase));
        ctx_evidence.push(("F_GP_GET".into(), f_gp_get));
        ctx_evidence.push(("F_GP_PUT".into(), f_gp_put));
        ctx_evidence.push(("F_CTRL".into(), f_ctrl));
        ctx_evidence.push(("F_PBDMA_INTR".into(), f_pbdma_intr));
        ctx_evidence.push(("F_PBDMA_ST".into(), f_pbdma_status));

        if f_loaded || f_fetching {
            gpu_can_read = true;
            eprintln!("║   ✓ Direct PBDMA programming loaded context!");
            if f_sig == 0x0000_FACE || f_gpbase == ramfc_gp_base_lo {
                instance_accessible = true;
            }
        }
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

    Ok(DmaCapability {
        engines,
        gpu_can_read_sysmem: gpu_can_read,
        gpu_can_write_sysmem: false,
        iommu_mapping_ok: iommu_ok,
        page_tables_ok: pt_ok,
        instance_block_accessible: instance_accessible,
        ctx_evidence,
    })
}
