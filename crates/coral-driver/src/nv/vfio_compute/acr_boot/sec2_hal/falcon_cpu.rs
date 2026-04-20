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

/// Zero all of IMEM via PIO when the ROM fails to scrub it.
///
/// The falcon ROM normally scrubs IMEM/DMEM after PMC enable, but if the ROM
/// doesn't execute (e.g. wrong reset sequence), stale code from a previous
/// driver (nouveau) persists and all strategies execute that old firmware.
pub fn falcon_pio_scrub_imem(bar0: &MappedBar, base: usize) {
    let hwcfg = bar0.read_u32(base + falcon::HWCFG).unwrap_or(0);
    let imem_bytes = falcon::imem_size_bytes(hwcfg) as usize;
    let imem_words = if imem_bytes > 0 {
        imem_bytes / 4
    } else {
        0x10000 / 4
    };

    // IMEMC: bits[15:2] = address>>2, bit 24 = auto-increment, bit 25 = write mode
    let imemc_val = (1u32 << 24) | (1u32 << 25); // auto-inc + write mode, address 0
    let _ = bar0.write_u32(base + falcon::IMEMC, imemc_val);
    for _ in 0..imem_words {
        let _ = bar0.write_u32(base + falcon::IMEMD, 0);
    }

    // Also scrub DMEM
    let dmem_bytes = falcon::dmem_size_bytes(hwcfg) as usize;
    let dmem_words = if dmem_bytes > 0 {
        dmem_bytes / 4
    } else {
        0x4000 / 4
    };
    let dmemc_val = (1u32 << 24) | (1u32 << 25); // auto-inc + write mode, address 0
    let _ = bar0.write_u32(base + falcon::DMEMC, dmemc_val);
    for _ in 0..dmem_words {
        let _ = bar0.write_u32(base + falcon::DMEMD, 0);
    }

    tracing::info!(
        base = format!("{base:#x}"),
        imem_words,
        dmem_words,
        "Manual PIO scrub complete"
    );
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

/// Configure ALL FBIF_TRANSCFG indices for system-memory DMA via IOMMU.
///
/// The falcon has 5 DMA index slots (UCODE, VIRT, PHYS_VID, PHYS_SYS_COH,
/// PHYS_SYS_NCOH). The ACR firmware selects an index via ctx_dma fields in
/// descriptors. If system-memory indices (3, 4) are left at 0x0 (default),
/// DMA through those channels silently fails or faults.
pub fn falcon_configure_fbif_all_sysmem(bar0: &MappedBar, base: usize, notes: &mut Vec<String>) {
    let phys_override = falcon::FBIF_PHYSICAL_OVERRIDE; // 0x80
    let phys_vid_val = phys_override | 0x10; // 0x90 — matches observed post-reset default

    for idx in 0..5usize {
        let off = base + falcon::FBIF_TRANSCFG_IDX_BASE + idx * falcon::FBIF_TRANSCFG_IDX_STRIDE;
        let before = bar0.read_u32(off).unwrap_or(0xDEAD);
        let new_val = phys_vid_val;
        let _ = bar0.write_u32(off, new_val);
        let after = bar0.read_u32(off).unwrap_or(0xDEAD);
        let name = match idx {
            0 => "UCODE",
            1 => "VIRT",
            2 => "PHYS_VID",
            3 => "PHYS_SYS_COH",
            4 => "PHYS_SYS_NCOH",
            _ => "?",
        };
        notes.push(format!(
            "FBIF[{idx}]({name})@{off:#x}: {before:#x}->{after:#x}"
        ));
    }
    let _ = bar0.write_u32(base + falcon::DMACTL, 0);
}
