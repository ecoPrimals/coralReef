// SPDX-License-Identifier: AGPL-3.0-or-later

//! Falcon STARTCPU and physical-DMA preparation.

use crate::vfio::channel::registers::falcon;
use crate::vfio::device::MappedBar;

/// Issue STARTCPU to a falcon, using CPUCTL_ALIAS if ALIAS_EN (bit 6) is set.
///
/// Matches Nouveau's `nvkm_falcon_v1_start`:
/// ```c
/// u32 reg = nvkm_falcon_rd32(falcon, 0x100);
/// if (reg & BIT(6))
///     nvkm_falcon_wr32(falcon, 0x130, 0x2);
/// else
///     nvkm_falcon_wr32(falcon, 0x100, 0x2);
/// ```
pub fn falcon_start_cpu(bar0: &MappedBar, base: usize) {
    let cpuctl = bar0.read_u32(base + falcon::CPUCTL).unwrap_or(0);
    let bootvec = bar0.read_u32(base + falcon::BOOTVEC).unwrap_or(0xDEAD);
    let alias_en = cpuctl & (1 << 6) != 0;
    tracing::info!(
        "falcon_start_cpu: base={:#x} cpuctl={:#010x} bootvec={:#010x} alias_en={}",
        base,
        cpuctl,
        bootvec,
        alias_en
    );
    if alias_en {
        let _ = bar0.write_u32(base + falcon::CPUCTL_ALIAS, falcon::CPUCTL_STARTCPU);
    } else {
        let _ = bar0.write_u32(base + falcon::CPUCTL, falcon::CPUCTL_STARTCPU);
    }

    std::thread::sleep(std::time::Duration::from_millis(20));

    let pc_after = bar0.read_u32(base + falcon::PC).unwrap_or(0xDEAD);
    let exci_after = bar0.read_u32(base + falcon::EXCI).unwrap_or(0xDEAD);
    let cpuctl_after = bar0.read_u32(base + falcon::CPUCTL).unwrap_or(0xDEAD);
    if exci_after != 0 || pc_after == 0 {
        tracing::warn!(
            "falcon_start_cpu: POST-START FAULT base={:#x} pc={:#06x} exci={:#010x} cpuctl={:#010x}",
            base,
            pc_after,
            exci_after,
            cpuctl_after
        );
    } else {
        tracing::info!(
            "falcon_start_cpu: OK base={:#x} pc={:#06x} exci={:#010x} cpuctl={:#010x}",
            base,
            pc_after,
            exci_after,
            cpuctl_after
        );
    }
}

/// Prepare a falcon for no-instance-block DMA (physical mode).
///
/// Matches Nouveau's `gm200_flcn_fw_load` for the non-instance path:
/// ```c
/// nvkm_falcon_mask(falcon, 0x624, 0x00000080, 0x00000080);
/// nvkm_falcon_wr32(falcon, 0x10c, 0x00000000);
/// ```
pub(crate) fn falcon_prepare_physical_dma(bar0: &MappedBar, base: usize) {
    let cur = bar0.read_u32(base + falcon::FBIF_TRANSCFG).unwrap_or(0);
    let _ = bar0.write_u32(
        base + falcon::FBIF_TRANSCFG,
        cur | falcon::FBIF_PHYSICAL_OVERRIDE,
    );
    let _ = bar0.write_u32(base + falcon::DMACTL, 0);
}
