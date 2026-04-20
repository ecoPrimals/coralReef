// SPDX-License-Identifier: AGPL-3.0-or-later

//! SEC2 reset, falcon bind, IMEM/DMEM load, STARTCPU, polling, and post-boot capture.

use crate::vfio::channel::registers::{falcon, misc};
use crate::vfio::device::MappedBar;
use crate::vfio::dma::DmaBuffer;

use super::super::boot_result::{AcrBootResult, PostBootCapture};
use super::super::firmware::ParsedAcrFirmware;
use super::super::instance_block;
use super::super::sec2_hal::{
    Sec2Probe, falcon_dmem_upload, falcon_imem_upload_nouveau, falcon_pio_scrub_imem,
    falcon_start_cpu, find_sec2_pmc_bit, pmc_enable_sec2, sec2_dmem_read,
};
use super::super::sysmem_iova;
use super::super::wpr::build_bl_dmem_desc;
use super::boot_config::BootConfig;
use super::sysmem_state::SysmemDmaState;

/// From SEC2 reset through [`PostBootCapture::into_result`].
pub(super) fn sec2_reset_bind_load_and_poll(
    bar0: &MappedBar,
    dma: &mut SysmemDmaState,
    parsed: &ParsedAcrFirmware,
    payload_patched: &[u8],
    config: &BootConfig,
    notes: &mut Vec<String>,
    sec2_before: Sec2Probe,
) -> AcrBootResult {
    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };

    let data_off = parsed.load_header.data_dma_base as usize;

    w(falcon::ITFEN, r(falcon::ITFEN) & !0x03);
    w(falcon::IRQMCLR, 0xFFFF_FFFF);

    // 1. PMC disable (stop engine clock)
    {
        let sec2_bit = find_sec2_pmc_bit(bar0).unwrap_or(22);
        let sec2_mask = 1u32 << sec2_bit;
        let val = bar0.read_u32(misc::PMC_ENABLE).unwrap_or(0);
        if val & sec2_mask != 0 {
            let _ = bar0.write_u32(misc::PMC_ENABLE, val & !sec2_mask);
            let _ = bar0.read_u32(misc::PMC_ENABLE);
            std::thread::sleep(std::time::Duration::from_micros(20));
        }
    }

    // 2. ENGCTL local reset (while engine clock is off)
    w(falcon::ENGCTL, 0x01);
    std::thread::sleep(std::time::Duration::from_micros(10));
    w(falcon::ENGCTL, 0x00);

    // 3. PMC enable → ROM starts fresh, scrubs IMEM/DMEM, then HALTs
    if let Err(e) = pmc_enable_sec2(bar0) {
        notes.push(format!("PMC enable failed: {e}"));
    }
    let _ = bar0.read_u32(base + falcon::MAILBOX0);
    std::thread::sleep(std::time::Duration::from_micros(50));

    // 4. Wait for memory scrub (DMACTL bits [2:1] = 0)
    let scrub_start = std::time::Instant::now();
    loop {
        let scrub = r(falcon::DMACTL);
        if scrub & 0x06 == 0 {
            break;
        }
        if scrub_start.elapsed() > std::time::Duration::from_millis(100) {
            notes.push(format!("Scrub timeout: DMACTL={scrub:#010x}"));
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    // 5. Write BOOT_0 chip ID (per nouveau gm200_flcn_enable)
    let boot0 = bar0.read_u32(misc::BOOT0).unwrap_or(0);
    w(0x084, boot0);

    // 6. Wait for ROM halt — nouveau checks CPUCTL bit 4 (0x10) for halt.
    let halt_start = std::time::Instant::now();
    let mut rom_halted = false;
    loop {
        let cpuctl = r(falcon::CPUCTL);
        if cpuctl & falcon::CPUCTL_HRESET != 0 {
            rom_halted = true;
            notes.push(format!(
                "ROM halted in {:?} cpuctl={cpuctl:#010x}",
                halt_start.elapsed()
            ));
            break;
        }
        if halt_start.elapsed() > std::time::Duration::from_millis(500) {
            notes.push(format!("HALT timeout (500ms) cpuctl={cpuctl:#010x}"));
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    if !rom_halted {
        falcon_pio_scrub_imem(bar0, base);
        notes.push("Manual IMEM/DMEM scrub (ROM did not halt)".to_string());
    }

    let cpuctl_post = r(falcon::CPUCTL);
    let sctl_post = r(falcon::SCTL);
    notes.push(format!(
        "Post-reset: cpuctl={cpuctl_post:#010x} sctl={sctl_post:#010x}"
    ));

    let itfen = r(falcon::ITFEN);
    w(falcon::ITFEN, (itfen & !0x01) | 0x01);
    notes.push(format!("ITFEN: {itfen:#010x} → {:#010x}", r(falcon::ITFEN)));

    let bind_target = if config.bind_vram { 0u32 } else { 2u32 };
    let inst_bind_val = instance_block::encode_bind_inst(sysmem_iova::INST, bind_target);
    notes.push(format!(
        "Binding: target={} addr={:#x}",
        if bind_target == 0 { "VRAM" } else { "SYS_MEM" },
        sysmem_iova::INST
    ));
    let (bind_ok_ctx, bind_notes) =
        instance_block::falcon_bind_context(&|off| r(off), &|off, val| w(off, val), inst_bind_val);
    for n in &bind_notes {
        notes.push(n.clone());
    }

    let bind_ok = bind_ok_ctx;
    notes.push(format!(
        "bind_stat→0: {} (0x0dc={:#010x})",
        if bind_ok {
            "OK (via falcon_bind_context)"
        } else {
            "TIMEOUT"
        },
        r(0x0dc)
    ));

    if config.tlb_invalidate {
        let mmu_ctrl = bar0.read_u32(0x100C80).unwrap_or(0);
        let flush_slot_avail = mmu_ctrl & 0x00FF_0000 != 0;

        let pdb_addr = sysmem_iova::INST;
        let pdb_inv = ((pdb_addr >> 12) << 4) as u32;
        let _ = bar0.write_u32(0x100CB8, pdb_inv);
        let _ = bar0.write_u32(0x100CEC, 0);
        let _ = bar0.write_u32(0x100CBC, 0x8000_0005);

        let mut flush_ack = false;
        for _ in 0..200 {
            if bar0.read_u32(0x100C80).unwrap_or(0) & 0x0000_8000 != 0 {
                flush_ack = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_micros(100));
        }
        notes.push(format!(
            "TLB invalidate: PDB={pdb_inv:#010x} slot_avail={flush_slot_avail} ack={flush_ack}"
        ));
    } else {
        notes.push("TLB invalidate: SKIPPED".to_string());
    }

    // Configure ALL FBIF indices for system-memory DMA via IOMMU.
    // Previously only index 2 (PHYS_VID) was set, leaving indices 3/4
    // (SYS_COH/NCOH) at 0x0 — the ACR firmware uses those for sysmem DMA.
    super::super::sec2_hal::falcon_configure_fbif_all_sysmem(bar0, base, notes);

    let hwcfg = r(falcon::HWCFG);
    let code_limit = falcon::imem_size_bytes(hwcfg);
    let boot_size = (parsed.bl_desc.bl_code_off + parsed.bl_desc.bl_code_size + 0xFF) & !0xFF;
    let imem_addr = code_limit.saturating_sub(boot_size);
    let start_tag = parsed.bl_desc.bl_start_tag;
    let boot_addr = start_tag << 8;

    falcon_imem_upload_nouveau(bar0, base, imem_addr, &parsed.bl_code, start_tag);
    notes.push(format!(
        "BL code: {}B → IMEM@{imem_addr:#x} tag={start_tag:#x} boot={boot_addr:#x}",
        parsed.bl_code.len()
    ));

    if config.imem_preload {
        let total_code_size = (parsed.load_header.non_sec_code_size
            + parsed.load_header.apps.first().map(|a| a.1).unwrap_or(0))
            as usize;
        let code_end = total_code_size.min(payload_patched.len());
        if code_end > 0 {
            let code = &payload_patched[..code_end];
            falcon_imem_upload_nouveau(bar0, base, 0, code, 0);
            notes.push(format!(
                "IMEM pre-load: {}B → IMEM@0x0 tag=0 (non-secure)",
                code.len()
            ));
        }
    }

    let code_dma_base = sysmem_iova::ACR;
    let data_dma_base = sysmem_iova::ACR + data_off as u64;

    let data_section = &payload_patched[data_off..];
    falcon_dmem_upload(bar0, base, 0, data_section);
    notes.push(format!(
        "ACR data pre-loaded: {}B → DMEM@0",
        data_section.len()
    ));

    let bl_desc = build_bl_dmem_desc(code_dma_base, data_dma_base, parsed);
    let ctx_dma_val = u32::from_le_bytes(bl_desc[32..36].try_into().unwrap_or([1, 0, 0, 0]));
    falcon_dmem_upload(bar0, base, 0, &bl_desc);
    notes.push(format!(
        "BL desc: code={code_dma_base:#x} data={data_dma_base:#x} data_size={:#x} ctx_dma={ctx_dma_val}",
        parsed.load_header.data_size
    ));

    {
        let wpr1_beg = bar0.read_u32(0x100CE4).unwrap_or(0xDEAD);
        let wpr1_end = bar0.read_u32(0x100CE8).unwrap_or(0xDEAD);
        let wpr2_beg = bar0.read_u32(0x100CEC).unwrap_or(0xDEAD);
        let wpr2_end = bar0.read_u32(0x100CF0).unwrap_or(0xDEAD);
        let fbpa_lo = bar0.read_u32(0x1FA824).unwrap_or(0xDEAD);
        let fbpa_hi = bar0.read_u32(0x1FA828).unwrap_or(0xDEAD);

        let _ = bar0.write_u32(0x100CD4, 2);
        let indexed_start = bar0.read_u32(0x100CD4).unwrap_or(0xDEAD);
        let _ = bar0.write_u32(0x100CD4, 3);
        let indexed_end = bar0.read_u32(0x100CD4).unwrap_or(0xDEAD);

        notes.push(format!(
            "WPR hw: WPR1=[{wpr1_beg:#010x}..{wpr1_end:#010x}] WPR2=[{wpr2_beg:#010x}..{wpr2_end:#010x}]"
        ));
        notes.push(format!(
            "WPR FBPA: lo={fbpa_lo:#010x} hi={fbpa_hi:#010x} indexed=[{indexed_start:#010x}..{indexed_end:#010x}]"
        ));

        for idx in 0u32..5 {
            let fbif_off = 0x604 + (idx as usize) * 0x10;
            let val = r(fbif_off);
            let name = match idx {
                0 => "UCODE",
                1 => "VIRT",
                2 => "PHYS_VID",
                3 => "PHYS_SYS_COH",
                4 => "PHYS_SYS_NCOH",
                _ => "?",
            };
            notes.push(format!(
                "FBIF_TRANSCFG[{idx}]({name})@{fbif_off:#05x}={val:#010x}"
            ));
        }

        // Map a large DMA catch-all covering the top of the 32-bit IOVA space.
        // The ACR firmware reads VRAM physical WPR2 addresses and truncates them
        // to 32 bits for DMA. For 12GB Titan V (VRAM at 0x0-0x300000000), the
        // truncated WPR addresses fall in 0xFF000000-0xFFFFFFFF. Observed faults:
        // 0xFFC4B000, 0xFFDFD000, 0xFFE3B000, 0xFFE4B000.
        let catch_base: u64 = 0xFF00_0000;
        let catch_size: usize = 0x100_0000; // 16 MiB (covers 0xFF000000-0xFFFFFFFF)
        match DmaBuffer::new(dma.container.clone(), catch_size, catch_base) {
            Ok(mut buf) => {
                let wpr = &dma.wpr_data;
                // Tile the WPR data across the entire buffer so firmware finds
                // valid data at any truncated VRAM offset.
                let sl = buf.as_mut_slice();
                let mut off = 0;
                while off < catch_size {
                    let chunk = wpr.len().min(catch_size - off);
                    sl[off..off + chunk].copy_from_slice(&wpr[..chunk]);
                    off += chunk;
                }
                notes.push(format!(
                    "4GiB catch: {catch_size:#x}B at IOVA {catch_base:#x} (FBPA lo={fbpa_lo:#x})"
                ));
                dma._4gib_catch = Some(buf);
            }
            Err(e) => {
                notes.push(format!("4GiB catch alloc FAILED at {catch_base:#x}: {e}"));
            }
        }
    }

    {
        let pre_dmem = sec2_dmem_read(bar0, 0x200, 0x70);
        let pre_nz: Vec<String> = pre_dmem
            .iter()
            .enumerate()
            .filter(|&(_, &w)| w != 0 && w != 0xDEAD_DEAD && w != 0xDEAD_5EC2)
            .take(6)
            .map(|(i, &w)| format!("[{:#05x}]={w:#010x}", 0x200 + i * 4))
            .collect();
        let pre_bl = sec2_dmem_read(bar0, 0, 0x54);
        let bl_ctx_dma = pre_bl.get(8).copied().unwrap_or(0xDEAD);
        let bl_data_sz = pre_bl.get(18).copied().unwrap_or(0xDEAD);
        notes.push(format!(
            "Pre-boot DMEM verify: BL[0x20]ctx_dma={bl_ctx_dma:#x} BL[0x48]data_size={bl_data_sz:#x}"
        ));
        notes.push(format!(
            "Pre-boot DMEM[0x200..0x270]: {}",
            if pre_nz.is_empty() {
                "ALL ZERO/DEAD".to_string()
            } else {
                pre_nz.join(" ")
            }
        ));
    }

    w(falcon::MAILBOX0, 0xdead_a5a5_u32);
    w(falcon::MAILBOX1, 0);
    w(falcon::BOOTVEC, boot_addr);
    notes.push(format!(
        "BOOTVEC={boot_addr:#x} mb0=0xdeada5a5, issuing STARTCPU"
    ));

    {
        let _ = bar0.write_u32(base + 0x180, (1u32 << 25) | 0x0500);
        let mut w5 = Vec::new();
        for _ in 0..4 {
            w5.push(bar0.read_u32(base + 0x184).unwrap_or(0xDEAD));
        }
        let hex5: Vec<String> = w5
            .iter()
            .enumerate()
            .map(|(i, &v)| format!("[{:#05x}]={v:#010x}", 0x500 + i * 4))
            .collect();
        notes.push(format!("Pre-boot IMEM[0x500..0x510]: {}", hex5.join(" ")));

        let bl_start = imem_addr;
        let _ = bar0.write_u32(base + 0x180, (1u32 << 25) | bl_start);
        let mut wbl = Vec::new();
        for _ in 0..4 {
            wbl.push(bar0.read_u32(base + 0x184).unwrap_or(0xDEAD));
        }
        let hexbl: Vec<String> = wbl
            .iter()
            .enumerate()
            .map(|(i, &v)| format!("[{:#05x}]={v:#010x}", bl_start as usize + i * 4))
            .collect();
        notes.push(format!(
            "Pre-boot IMEM BL[{bl_start:#x}..]: {}",
            hexbl.join(" ")
        ));
    }

    {
        let fbif_pre = r(falcon::FBIF_TRANSCFG);
        let itfen_pre = r(falcon::ITFEN);
        let dmactl_pre = r(falcon::DMACTL);
        let irqstat_pre = r(0x008);
        notes.push(format!(
            "Pre-boot DMA: FBIF={fbif_pre:#x} ITFEN={itfen_pre:#x} DMACTL={dmactl_pre:#x} IRQSTAT={irqstat_pre:#x}"
        ));
    }
    falcon_start_cpu(bar0, base);

    let timeout = std::time::Duration::from_secs(5);
    let start_time = std::time::Instant::now();
    let mut pc_samples = Vec::new();
    let mut last_pc = 0u32;

    let mut all_pcs: Vec<u32> = Vec::new();
    for _ in 0..500 {
        let pc = r(falcon::PC);
        if all_pcs.last() != Some(&pc) {
            all_pcs.push(pc);
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
        if start_time.elapsed().as_millis() > 50 {
            break;
        }
    }

    let mut settled_count = 0u32;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let cpuctl = r(falcon::CPUCTL);
        let mb0 = r(falcon::MAILBOX0);
        let mb1 = r(falcon::MAILBOX1);
        let pc = r(falcon::PC);

        if pc != last_pc {
            pc_samples.push(format!(
                "{:#06x}@{}ms",
                pc,
                start_time.elapsed().as_millis()
            ));
            last_pc = pc;
            settled_count = 0;
        } else {
            settled_count += 1;
        }

        let halted = cpuctl & falcon::CPUCTL_HALTED != 0;
        let hreset_back = cpuctl & falcon::CPUCTL_HRESET != 0;

        if mb0 != 0 || halted || hreset_back {
            notes.push(format!(
                "SEC2 response: cpuctl={cpuctl:#010x} mb0={mb0:#010x} mb1={mb1:#010x} pc={pc:#010x} ({}ms)",
                start_time.elapsed().as_millis()
            ));
            break;
        }
        if settled_count > 200 {
            notes.push(format!(
                "SEC2 settled: cpuctl={cpuctl:#010x} mb0={mb0:#010x} pc={pc:#010x} ({}ms)",
                start_time.elapsed().as_millis()
            ));
            break;
        }
        if start_time.elapsed() > timeout {
            notes.push(format!(
                "SEC2 timeout: cpuctl={cpuctl:#010x} mb0={mb0:#010x} pc={pc:#010x}"
            ));
            break;
        }
    }

    if !all_pcs.is_empty() {
        let pc_trace: Vec<String> = all_pcs.iter().map(|p| format!("{p:#06x}")).collect();
        notes.push(format!("Fast PC trace: [{}]", pc_trace.join(" → ")));
    }
    if !pc_samples.is_empty() {
        notes.push(format!("PC progression: [{}]", pc_samples.join(", ")));
    }

    super::super::boot_diagnostics::capture_post_boot_diagnostics(bar0, base, notes);

    {
        let wpr_slice = dma.wpr_dma.as_mut_slice();
        let fecs_status =
            u32::from_le_bytes([wpr_slice[20], wpr_slice[21], wpr_slice[22], wpr_slice[23]]);
        let gpccs_status =
            u32::from_le_bytes([wpr_slice[44], wpr_slice[45], wpr_slice[46], wpr_slice[47]]);
        let shadow_slice = dma.shadow_dma.as_mut_slice();
        let sf = u32::from_le_bytes([
            shadow_slice[20],
            shadow_slice[21],
            shadow_slice[22],
            shadow_slice[23],
        ]);
        let sg = u32::from_le_bytes([
            shadow_slice[44],
            shadow_slice[45],
            shadow_slice[46],
            shadow_slice[47],
        ]);
        notes.push(format!(
            "WPR FECS={fecs_status} GPCCS={gpccs_status} | Shadow FECS={sf} GPCCS={sg} (1=copy, 0xFF=done)"
        ));
    }

    let mb0_final = r(falcon::MAILBOX0);
    let mb1_final = r(falcon::MAILBOX1);
    let acr_success = mb0_final == 0;
    notes.push(format!(
        "ACR result: mb0={mb0_final:#010x} mb1={mb1_final:#010x} success={acr_success}"
    ));

    super::super::sec2_queue::probe_and_bootstrap(bar0, notes);

    let sec2_after = Sec2Probe::capture(bar0);
    let post = PostBootCapture::capture(bar0);
    notes.push(format!(
        "Final: FECS cpuctl={:#010x} pc={:#06x} exci={:#010x} GPCCS cpuctl={:#010x} pc={:#06x} exci={:#010x}",
        post.fecs_cpuctl, post.fecs_pc, post.fecs_exci,
        post.gpccs_cpuctl, post.gpccs_pc, post.gpccs_exci
    ));

    post.into_result(
        "System-memory ACR boot (IOMMU DMA)",
        sec2_before,
        sec2_after,
        std::mem::take(notes),
    )
}
