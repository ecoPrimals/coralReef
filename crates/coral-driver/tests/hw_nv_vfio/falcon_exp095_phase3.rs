// SPDX-License-Identifier: AGPL-3.0-only

use coral_driver::nv::vfio_compute::NvVfioComputeDevice;
use coral_driver::vfio::memory::{MemoryRegion, PraminRegion};

/// Deferred Phase 3 (disabled): VRAM ACR path — extracted from Exp 095 for size limits.
pub fn exp095_phase3_deferred(dev: &NvVfioComputeDevice) {
    let bar0 = dev.bar0_ref();
    const SEC2_BASE: usize = 0x087000;
    const FECS_BASE: usize = 0x409000;
    const GPCCS_BASE: usize = 0x41a000;
    let r = |off: usize| bar0.read_u32(SEC2_BASE + off).unwrap_or(0xDEAD_DEAD);

    eprintln!("\n── Phase 3: ACR boot (VRAM DMA, v2 desc) ──");

    let chip = "gv100";
    let fw = coral_driver::nv::vfio_compute::acr_boot::AcrFirmwareSet::load(chip)
        .expect("firmware load");
    let parsed = coral_driver::nv::vfio_compute::acr_boot::ParsedAcrFirmware::parse(&fw)
        .expect("firmware parse");
    eprintln!(
        "FW: bl={}B acr={}B data_off={:#x} data_size={:#x} non_sec=[{:#x}+{:#x}] apps={:?}",
        parsed.bl_code.len(),
        parsed.acr_payload.len(),
        parsed.load_header.data_dma_base,
        parsed.load_header.data_size,
        parsed.load_header.non_sec_code_off,
        parsed.load_header.non_sec_code_size,
        parsed.load_header.apps
    );
    eprintln!(
        "Sig: prod_off={:#x} prod_size={:#x} patch_loc={:#x} patch_sig={:#x}",
        parsed.hs_header.sig_prod_offset,
        parsed.hs_header.sig_prod_size,
        parsed.hs_header.patch_loc,
        parsed.hs_header.patch_sig
    );

    // WPR + ACR always at low VRAM (within 2MB identity-mapped page tables).
    // Nouveau allocates DOUBLE the WPR size: [shadow][WPR]. The ACR reads the
    // shadow for verification, then copies to the WPR. shadow != wpr is required.
    let acr_payload = &parsed.acr_payload;
    let acr_vram_base = 0x50000u64;
    let shadow_vram_base = 0x60000u64;
    let wpr_vram_base = 0x70000u64;

    let wpr_data = coral_driver::nv::vfio_compute::acr_boot::build_wpr(&fw, wpr_vram_base);
    let wpr_vram_end = wpr_vram_base + wpr_data.len() as u64;
    let mut payload_patched = acr_payload.to_vec();
    let data_off = parsed.load_header.data_dma_base as usize;
    coral_driver::nv::vfio_compute::acr_boot::patch_acr_desc(
        &mut payload_patched,
        data_off,
        wpr_vram_base,
        wpr_vram_end,
        shadow_vram_base,
    );
    // Skip WPR blob DMA: ACR won't DMA the blob from VRAM (we pre-populated WPR via PRAMIN).
    // blob_size=0 tells ACR the WPR is already in place.
    payload_patched[data_off + 0x258..data_off + 0x25C].copy_from_slice(&0u32.to_le_bytes());
    payload_patched[data_off + 0x260..data_off + 0x268].copy_from_slice(&0u64.to_le_bytes());
    eprintln!(
        "ACR: {acr_vram_base:#x} Shadow: {shadow_vram_base:#x} WPR: {wpr_vram_base:#x}..{wpr_vram_end:#x} blob_size=0(skip DMA)"
    );

    // Write ACR payload + WPR to VRAM via PRAMIN
    let write_to_vram = |vaddr: u64, data: &[u8], label: &str| -> bool {
        let mut off = 0usize;
        while off < data.len() {
            let chunk_vram = (vaddr + off as u64) as u32;
            let chunk_size = (data.len() - off).min(0xC000);
            match PraminRegion::new(bar0, chunk_vram, chunk_size) {
                Ok(mut region) => {
                    for w_off in (0..chunk_size).step_by(4) {
                        let src = off + w_off;
                        if src >= data.len() {
                            break;
                        }
                        let end = (src + 4).min(data.len());
                        let mut bytes = [0u8; 4];
                        bytes[..end - src].copy_from_slice(&data[src..end]);
                        if region.write_u32(w_off, u32::from_le_bytes(bytes)).is_err() {
                            eprintln!("  VRAM write failed: {label}@{chunk_vram:#x}+{w_off:#x}");
                            return false;
                        }
                    }
                    off += chunk_size;
                }
                Err(e) => {
                    eprintln!("  PRAMIN failed for {label}@{chunk_vram:#x}: {e}");
                    return false;
                }
            }
        }
        true
    };

    if !write_to_vram(acr_vram_base, &payload_patched, "ACR payload") {
        eprintln!("ACR payload write failed — aborting");
        eprintln!("\n=== End Exp 095 ===");
        return;
    }
    eprintln!(
        "ACR payload: {}B → VRAM@{acr_vram_base:#x}",
        payload_patched.len()
    );

    // Shadow WPR: identical copy at separate address (nouveau: first half of double allocation)
    if !write_to_vram(shadow_vram_base, &wpr_data, "Shadow WPR") {
        eprintln!("Shadow WPR write failed — aborting");
        eprintln!("\n=== End Exp 095 ===");
        return;
    }
    eprintln!(
        "Shadow WPR: {}B → VRAM@{shadow_vram_base:#x}",
        wpr_data.len()
    );

    if !write_to_vram(wpr_vram_base, &wpr_data, "WPR") {
        eprintln!("WPR write failed — aborting");
        eprintln!("\n=== End Exp 095 ===");
        return;
    }
    eprintln!("WPR: {}B → VRAM@{wpr_vram_base:#x}", wpr_data.len());

    use coral_driver::nv::vfio_compute::acr_boot::{
        falcon_dmem_upload, falcon_imem_upload_nouveau, falcon_start_cpu,
    };

    // ── Pre-configure WPR hardware registers ──
    // The ACR firmware may poll WPR registers to verify its WPR setup took effect.
    // With WPR disabled (all zeros after nouveau unbind), the ACR's poll would fail.
    // Try writing our WPR range to PFB WPR2 registers (0x100CEC/0x100CF0).
    //
    // Format: address >> 8 for GM200 indexed (0x100CD4), raw address for PFB direct.
    // We try BOTH PFB direct and GM200 indexed approaches.
    {
        let wpr_beg_val = wpr_vram_base as u32; // 0x70000
        let wpr_end_val = (wpr_vram_base + wpr_data.len() as u64) as u32; // 0x7CD00

        // PFB direct: try raw address, address>>8, address>>12 with enable bits
        let formats: &[(&str, u32, u32)] = &[
            ("raw", wpr_beg_val, wpr_end_val),
            (">>8|1", (wpr_beg_val >> 8) | 1, wpr_end_val >> 8),
            (">>12|1", (wpr_beg_val >> 12) | 1, wpr_end_val >> 12),
        ];

        for (name, beg, end) in formats {
            let _ = bar0.write_u32(0x100CEC, *beg); // WPR2_BEG
            let _ = bar0.write_u32(0x100CF0, *end); // WPR2_END
            let rb_beg = bar0.read_u32(0x100CEC).unwrap_or(0xDEAD);
            let rb_end = bar0.read_u32(0x100CF0).unwrap_or(0xDEAD);
            let stuck = rb_beg == *beg && rb_end == *end;
            eprintln!(
                "WPR2 write ({name}): beg={beg:#010x}→{rb_beg:#010x} end={end:#010x}→{rb_end:#010x} wrote={stuck}"
            );
            if stuck {
                break;
            }
        }

        // Also try GM200 indexed write approach
        let gm200_lo = (wpr_beg_val >> 8) | 0x01; // enable bit
        let gm200_hi = wpr_end_val >> 8;
        let _ = bar0.write_u32(0x100CD4, gm200_lo);
        std::thread::sleep(std::time::Duration::from_micros(10));
        let rb_lo = bar0.read_u32(0x100CD4).unwrap_or(0xDEAD);
        eprintln!("GM200 indexed write: lo={gm200_lo:#010x}→{rb_lo:#010x}");

        // Final read of all WPR-related registers
        let wpr2_beg = bar0.read_u32(0x100CEC).unwrap_or(0xDEAD);
        let wpr2_end = bar0.read_u32(0x100CF0).unwrap_or(0xDEAD);
        eprintln!("WPR2 final: beg={wpr2_beg:#010x} end={wpr2_end:#010x}");
    }

    // ── Reconfigure FBHUB MMU fault buffers ──
    // Exp 076: FBHUB stalls ALL DMA (including SEC2 FBIF) without a valid fault
    // buffer drain target. After nouveau unbind, the old fault buffer DMA mapping
    // is invalid. VfioChannel::create configures FAULT_BUF0/1 at IOVA 0xA000,
    // but the write may not stick if PFB is in a degraded state. Force it here.
    {
        let fb_iova: u64 = 0xA000; // Same IOVA as VfioChannel's fault buffer
        let fb_lo = (fb_iova >> 12) as u32;
        let fb_entries: u32 = 64;

        // Reset GET pointers and disable before reconfiguring
        let _ = bar0.write_u32(0x100E34, 0); // FAULT_BUF0_PUT: disable
        let _ = bar0.write_u32(0x100E54, 0); // FAULT_BUF1_PUT: disable
        std::thread::sleep(std::time::Duration::from_micros(100));

        // Non-replayable fault buffer (BUF0)
        let _ = bar0.write_u32(0x100E24, fb_lo); // LO
        let _ = bar0.write_u32(0x100E28, 0); // HI
        let _ = bar0.write_u32(0x100E2C, fb_entries); // SIZE
        let _ = bar0.write_u32(0x100E30, 0); // GET
        let _ = bar0.write_u32(0x100E34, 0x8000_0000); // PUT: enable

        // Replayable fault buffer (BUF1) — same backing buffer
        let _ = bar0.write_u32(0x100E44, fb_lo);
        let _ = bar0.write_u32(0x100E48, 0);
        let _ = bar0.write_u32(0x100E4C, fb_entries);
        let _ = bar0.write_u32(0x100E50, 0);
        let _ = bar0.write_u32(0x100E54, 0x8000_0000);

        // Verify readback
        let rb_lo = bar0.read_u32(0x100E24).unwrap_or(0xDEAD);
        let rb_put = bar0.read_u32(0x100E34).unwrap_or(0xDEAD);
        let rb_enabled = rb_put & 0x8000_0000 != 0;
        eprintln!(
            "Fault buffer reconfig: lo={rb_lo:#x} (expect {fb_lo:#x}) put={rb_put:#010x} enabled={rb_enabled}"
        );
        if rb_lo != fb_lo {
            eprintln!("  *** FAULT_BUF0_LO write FAILED — FBHUB may be PRI-dead ***");
        }

        // Flush GPU MMU TLB (Exp 060: PAGE_ALL + HUB_ONLY via 0x100CBC)
        let _ = bar0.write_u32(0x100CBC, 0x0000_0001); // PRI_PFB_PRI_MMU_CTRL
        std::thread::sleep(std::time::Duration::from_micros(100));
        let mmu_ctrl_post = bar0.read_u32(0x100CBC).unwrap_or(0xDEAD);
        eprintln!("MMU TLB flush: ctrl={mmu_ctrl_post:#010x}");
    }

    // ── SEC2 engine reset (matching nouveau gm200_flcn_disable + gm200_flcn_enable) ──
    {
        // DISABLE: clear ITFEN, clear interrupts, PMC disable
        let _ = bar0.write_u32(SEC2_BASE + 0x048, r(0x048) & !0x03); // ITFEN clear bits 0:1
        let _ = bar0.write_u32(SEC2_BASE + 0x014, 0xFFFF_FFFF); // clear all interrupts

        let pmc = bar0.read_u32(0x200).unwrap_or(0);
        let sec2_bit = 1u32 << 22;
        let _ = bar0.write_u32(0x200, pmc & !sec2_bit); // PMC disable
        std::thread::sleep(std::time::Duration::from_micros(50));

        // ENABLE: PMC enable, wait scrub, write BOOT0
        let _ = bar0.write_u32(0x200, pmc | sec2_bit); // PMC enable
        for _ in 0..5000 {
            if r(0x10C) & 0x06 == 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_micros(100));
        }
        let boot0 = bar0.read_u32(0x000).unwrap_or(0);
        let _ = bar0.write_u32(SEC2_BASE + 0x084, boot0);
        for _ in 0..5000 {
            if r(0x100) & 0x10 != 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_micros(100));
        }
        eprintln!(
            "Post-reset: cpuctl={:#010x} sctl={:#010x} FBIF={:#010x} DMACTL={:#010x}",
            r(0x100),
            r(0x240),
            r(0x624),
            r(0x10C)
        );
    }

    // ── DMA config: HYBRID page tables (sysmem PTEs for ACR code, VRAM for WPR) ──
    // FBHUB is degraded after VFIO takeover: VRAM DMA reads corrupt data, breaking
    // the BL's signature verification and preventing HS mode entry. System memory
    // DMA bypasses FBHUB entirely. Allocate a sysmem DMA buffer for the ACR payload
    // and patch PT0 entries for those pages to SYS_MEM_COH aperture. WPR/shadow
    // pages stay as VRAM PTEs (WPR hardware only protects VRAM).
    let _acr_dma_guard; // DMA buffer must outlive the boot
    {
        use coral_driver::nv::vfio_compute::acr_boot::{
            FALCON_INST_VRAM, FALCON_PT0_VRAM, build_vram_falcon_inst_block, encode_bind_inst,
            encode_sysmem_pte, falcon_bind_context,
        };
        use coral_driver::vfio::dma::DmaBuffer;
        use coral_driver::vfio::memory::PraminRegion as PR2;

        // Build VRAM page tables (identity-map first 2MB — all VRAM aperture)
        let pt_ok = build_vram_falcon_inst_block(bar0);
        eprintln!("Instance block built: {pt_ok}");

        // Allocate sysmem DMA buffer for ACR payload at IOVA matching acr_vram_base
        let acr_buf_size = (payload_patched.len().div_ceil(4096)) * 4096;
        let container = dev.dma_backend();
        let mut acr_dma = DmaBuffer::new(container, acr_buf_size.max(4096), acr_vram_base)
            .expect("DMA alloc for ACR sysmem");
        acr_dma.as_mut_slice()[..payload_patched.len()].copy_from_slice(&payload_patched);
        eprintln!(
            "ACR sysmem DMA: {}B at IOVA {acr_vram_base:#x}",
            payload_patched.len()
        );

        // Overwrite PT0 entries for ACR payload pages: VRAM → SYS_MEM_COH
        let acr_page_start = (acr_vram_base / 4096) as usize;
        let acr_page_end = ((acr_vram_base + acr_buf_size as u64).div_ceil(4096)) as usize;
        let mut pt_patched = 0usize;
        for page in acr_page_start..acr_page_end {
            let iova = (page as u64) * 4096;
            let pte = encode_sysmem_pte(iova);
            let pte_lo = (pte & 0xFFFF_FFFF) as u32;
            let pte_hi = (pte >> 32) as u32;
            let off = page * 8;
            if let Ok(mut r) = PR2::new(bar0, FALCON_PT0_VRAM, off + 8) {
                let _ = r.write_u32(off, pte_lo);
                let _ = r.write_u32(off + 4, pte_hi);
                pt_patched += 1;
            }
        }
        eprintln!(
            "PT0 hybrid: pages {acr_page_start}..{acr_page_end} → SYS_MEM_COH ({pt_patched} patched)"
        );
        _acr_dma_guard = acr_dma;

        // Enable ITFEN for FBIF + ENGINE interfaces
        let _ = bar0.write_u32(SEC2_BASE + 0x048, r(0x048) | 0x03);

        // Bind instance block to SEC2 (full nouveau sequence)
        let bind_val = encode_bind_inst(FALCON_INST_VRAM as u64, 0); // target=0=VRAM
        let (bind_ok, bind_notes) = falcon_bind_context(
            &|off| bar0.read_u32(SEC2_BASE + off).unwrap_or(0xDEAD),
            &|off, val| {
                let _ = bar0.write_u32(SEC2_BASE + off, val);
            },
            bind_val,
        );
        for note in &bind_notes {
            eprintln!("  bind: {note}");
        }
        eprintln!("Instance block bind: ok={bind_ok}");

        // Set DMACTL for virtual DMA context (matching nouveau)
        let _ = bar0.write_u32(SEC2_BASE + 0x10C, 0x02); // DMACTL=2 (use bound ctx)

        eprintln!("TRANSCFG (no physical override):");
        for port in 0..8usize {
            let reg = 0x620 + port * 4;
            let val = r(reg);
            eprint!("  [{port}]={val:#06x}");
        }
        eprintln!();
        eprintln!(
            "Virtual DMA: ITFEN={:#010x} DMACTL={:#010x} BIND={:#010x}",
            r(0x048),
            r(0x10C),
            r(0x054)
        );
    }

    // ── Upload BL to IMEM ──
    let hwcfg = r(0x108);
    let code_limit = (hwcfg & 0x1FF) * 256;
    let boot_size =
        ((parsed.bl_desc.bl_code_off + parsed.bl_desc.bl_code_size + 0xFF) & !0xFF) as u32;
    let imem_addr = code_limit.saturating_sub(boot_size);
    let start_tag = parsed.bl_desc.bl_start_tag;
    let boot_addr = start_tag << 8;
    falcon_imem_upload_nouveau(bar0, SEC2_BASE, imem_addr, &parsed.bl_code, start_tag);
    eprintln!(
        "BL: {}B → IMEM@{imem_addr:#x} tag={start_tag:#x} boot_addr={boot_addr:#x}",
        parsed.bl_code.len()
    );

    // ── Pre-load data section → DMEM, then BL descriptor on top ──
    // The BL only loads code to IMEM; neither BL nor ACR successfully DMA-loads the
    // data section to DMEM (DMA transfer engine fails with our physical config).
    // Solution: pre-load the data section to DMEM[0..data_size] ourselves, then
    // write the BLD on top at DMEM[0..84]. The data section's first 512 bytes are
    // reserved_dmem (zeros), so the BLD overlaps only with don't-care bytes.
    // The ACR descriptor starts at DMEM[0x200], safely beyond the 84-byte BLD.
    let code_dma_base = acr_vram_base;
    let data_dma_base = acr_vram_base + parsed.load_header.data_dma_base as u64;
    let data_off = parsed.load_header.data_dma_base as usize;
    let data_size = parsed.load_header.data_size as usize;
    let data_section = &payload_patched[data_off..data_off + data_size];
    eprintln!(
        "Pre-loading data section: {}B → DMEM[0..{data_size:#x}]",
        data_section.len()
    );
    falcon_dmem_upload(bar0, SEC2_BASE, 0, data_section);

    let mut bl_desc = coral_driver::nv::vfio_compute::acr_boot::build_bl_dmem_desc(
        code_dma_base,
        data_dma_base,
        &parsed,
    );
    // ctx_dma=1 (FALCON_DMAIDX_VIRT): DMA through the bound instance block's page tables.
    bl_desc[32..36].copy_from_slice(&1u32.to_le_bytes());
    let ctx_dma_val = u32::from_le_bytes(bl_desc[32..36].try_into().unwrap());
    eprintln!(
        "BL desc: {}B ctx_dma={ctx_dma_val} code={code_dma_base:#x} data={data_dma_base:#x}",
        bl_desc.len()
    );
    let dmem_off = parsed.bl_desc.bl_desc_dmem_load_off;
    eprintln!("BL expects desc at DMEM offset {dmem_off:#x} (we write at 0x0)");
    falcon_dmem_upload(bar0, SEC2_BASE, 0, &bl_desc);

    // Verify IMEM upload: read back first 4 words of BL at imem_addr
    {
        let _ = bar0.write_u32(SEC2_BASE + 0x180, imem_addr | (start_tag << 24));
        let imem_w0 = bar0.read_u32(SEC2_BASE + 0x184).unwrap_or(0xDEAD);
        let imem_w1 = bar0.read_u32(SEC2_BASE + 0x184).unwrap_or(0xDEAD);
        let expect_w0 = u32::from_le_bytes(parsed.bl_code[..4].try_into().unwrap());
        let expect_w1 = u32::from_le_bytes(parsed.bl_code[4..8].try_into().unwrap());
        eprintln!(
            "IMEM verify @{imem_addr:#x}: [{imem_w0:#010x} {imem_w1:#010x}] expect=[{expect_w0:#010x} {expect_w1:#010x}] match={}",
            imem_w0 == expect_w0 && imem_w1 == expect_w1
        );
    }

    // Verify DMEM upload: read back first 4 words of BL desc at offset 0
    {
        use coral_driver::nv::vfio_compute::acr_boot::sec2_emem_read;
        let _ = bar0.write_u32(SEC2_BASE + 0x1C0, 0); // DMEM index port 0
        let dm0 = bar0.read_u32(SEC2_BASE + 0x1C4).unwrap_or(0xDEAD); // word 0 (reserved)
        let _ = bar0.write_u32(SEC2_BASE + 0x1C0, 32); // offset 32 = ctx_dma
        let dm_ctx = bar0.read_u32(SEC2_BASE + 0x1C4).unwrap_or(0xDEAD);
        let _ = bar0.write_u32(SEC2_BASE + 0x1C0, 36); // offset 36 = code_dma_base lo
        let dm_code_lo = bar0.read_u32(SEC2_BASE + 0x1C4).unwrap_or(0xDEAD);
        eprintln!(
            "DMEM verify @0: reserved={dm0:#010x} ctx_dma={dm_ctx:#010x} code_lo={dm_code_lo:#010x}"
        );
    }

    // ── Boot SEC2 ──

    // NVIDIA RM sets TIMPRE before starting the falcon (kflcnableSetup_HAL).
    // TIMPRE = timer prescaler: falcon timer frequency = ref_clock / (TIMPRE + 1).
    // 0xE2 = 226, giving ~6.6μs ticks at 1.5GHz. Without this, firmware timeout
    // logic may behave incorrectly.
    let _ = bar0.write_u32(SEC2_BASE + 0x024, 0x0000_00E2); // TIMPRE
    let timpre_rb = r(0x024);
    eprintln!("TIMPRE={timpre_rb:#010x}");

    // IRQDEST routes each interrupt source to falcon CPU vs HOST.
    // Bit N set → source N goes to falcon CPU. Without this, the firmware
    // can't receive timer or software-generated interrupts.
    // Enable: bit 0 (EXT/GPTMR), bit 1 (WDTMR), bit 6 (SWGEN0), bit 7 (SWGEN1)
    let irqdest = (1u32 << 0) | (1u32 << 1) | (1u32 << 6) | (1u32 << 7);
    let _ = bar0.write_u32(SEC2_BASE + 0x01C, irqdest);
    let irqdest_rb = r(0x01C);
    eprintln!("IRQDEST={irqdest_rb:#010x} (expect {irqdest:#010x})");

    let _ = bar0.write_u32(SEC2_BASE + 0x040, 0xdead_a5a5); // sentinel
    let _ = bar0.write_u32(SEC2_BASE + 0x044, 0);
    let _ = bar0.write_u32(SEC2_BASE + 0x104, boot_addr); // BOOTVEC
    let bv_readback = r(0x104);
    eprintln!(
        "Pre-start: BOOTVEC={bv_readback:#010x} cpuctl={:#010x}",
        r(0x100)
    );
    eprintln!("STARTCPU: bootvec={boot_addr:#x} mb0=0xdeada5a5");
    // Full SEC2 register diff: snapshot before/after CPU start to see what firmware changed
    let reg_range: Vec<usize> = (0..0xD00usize).step_by(4).collect();
    let mut pre_regs: Vec<(usize, u32)> = Vec::new();
    for &off in &reg_range {
        let v = bar0.read_u32(SEC2_BASE + off).unwrap_or(0xBADF_1100);
        pre_regs.push((off, v));
    }

    falcon_start_cpu(bar0, SEC2_BASE);
    std::thread::sleep(std::time::Duration::from_millis(2));

    let mut post_regs: Vec<(usize, u32)> = Vec::new();
    for &off in &reg_range {
        let v = bar0.read_u32(SEC2_BASE + off).unwrap_or(0xBADF_1100);
        post_regs.push((off, v));
    }

    let mut diffs: Vec<(usize, u32, u32)> = Vec::new();
    for (i, &(off, pre)) in pre_regs.iter().enumerate() {
        let post = post_regs[i].1;
        if pre != post {
            diffs.push((off, pre, post));
        }
    }
    eprintln!(
        "SEC2 register diff (pre vs post-boot, {} changed):",
        diffs.len()
    );
    for &(off, pre, post) in &diffs {
        eprintln!("  [{off:#05x}] {pre:#010x} → {post:#010x}");
    }

    // Also check: PRIV ring, PFB WPR, PGRAPH engine after firmware ran
    let pri_mid = bar0.read_u32(0x120058).unwrap_or(0xDEAD);
    eprintln!("Priv ring 2ms after boot: {pri_mid:#010x}");

    // Continue with slower polling for remaining timeout
    let poll_start = std::time::Instant::now();
    let mut pc_trace: Vec<u32> = Vec::new();
    let timeout = std::time::Duration::from_secs(5);
    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let c = r(0x100);
        let p = r(0x030);
        let m0 = r(0x040);
        let m1 = r(0x044);
        if pc_trace.last() != Some(&p) {
            pc_trace.push(p);
        }

        let halted = c & 0x20 != 0;
        let mb_changed = m0 != 0xdead_a5a5 && m0 != 0;
        let hreset = c & 0x10 != 0;

        if halted || mb_changed || hreset {
            eprintln!(
                "SEC2 event: cpuctl={c:#010x} pc={p:#06x} mb0={m0:#010x} mb1={m1:#010x} ({}ms)",
                poll_start.elapsed().as_millis()
            );
            if mb_changed {
                eprintln!("  *** BL/ACR responded via MAILBOX! ***");
            }
            break;
        }
        if poll_start.elapsed() > timeout {
            eprintln!("SEC2 timeout: cpuctl={c:#010x} pc={p:#06x} mb0={m0:#010x} mb1={m1:#010x}");
            break;
        }
    }
    let pcs: Vec<String> = pc_trace.iter().map(|p| format!("{p:#06x}")).collect();
    eprintln!("PC trace: [{}]", pcs.join(" → "));

    // Post-boot diagnostics
    // Register 0x148 is TRACE INDEX (not EXCI!). Nouveau gm200_flcn_tracepc():
    //   bits[23:16] = number of trace entries. Write index → 0x148, read PC → 0x14C.
    let tidx = r(0x148);
    let nr_traces = ((tidx & 0x00FF_0000) >> 16).min(32);
    let fbif_final = r(0x624);
    let dmactl_final = r(0x10C);
    eprintln!(
        "Post-boot: TIDX={tidx:#010x} ({nr_traces} traces) FBIF={fbif_final:#010x} DMACTL={dmactl_final:#010x}"
    );

    // Dump TRACEPC buffer — shows actual execution history
    if nr_traces > 0 {
        let mut traces = Vec::new();
        for i in 0..nr_traces {
            let _ = bar0.write_u32(SEC2_BASE + 0x148, i);
            let tpc = bar0.read_u32(SEC2_BASE + 0x14C).unwrap_or(0xDEAD);
            traces.push(format!("{tpc:#06x}"));
        }
        eprintln!("TRACEPC[0..{nr_traces}]: {}", traces.join(" "));
    }

    // DMA engine registers: if ACR is stuck polling a DMA completion, this reveals it
    eprintln!(
        "DMA engine: TRFBASE={:#010x} TRFMOFFS={:#010x} TRFFBOFFS={:#010x} TRFCMD={:#010x}",
        r(0x110),
        r(0x114),
        r(0x118),
        r(0x11C)
    );
    // Interrupt state: pending IRQs the ACR firmware might be waiting for
    eprintln!(
        "IRQ: STAT={:#010x} MASK={:#010x} DEST={:#010x} SSET={:#010x} MSET={:#010x}",
        r(0x008),
        r(0x014),
        r(0x01C),
        r(0x010),
        r(0x018)
    );
    eprintln!("SCTL={:#010x} CPUCTL={:#010x}", r(0x240), r(0x100));
    // Falcon exception/halt info: 0x18C is EXE_CTRL on some revisions
    eprintln!(
        "Falcon debug: 0x18C={:#010x} 0x030(PC)={:#06x} 0x034(SP)={:#010x}",
        r(0x18C),
        r(0x030),
        r(0x034)
    );
    // PMU state: ACR may require PMU to be alive for power/clock management
    let pmu_cpuctl = bar0.read_u32(0x10A100).unwrap_or(0xDEAD);
    let pmu_sctl = bar0.read_u32(0x10A240).unwrap_or(0xDEAD);
    eprintln!("PMU: cpuctl={pmu_cpuctl:#010x} sctl={pmu_sctl:#010x}");
    // Check PGRAPH engine state (FECS/GPCCS) — if ACR tried to bootstrap them
    let fecs_mid = bar0.read_u32(FECS_BASE + 0x100).unwrap_or(0xDEAD);
    let gpccs_mid = bar0.read_u32(GPCCS_BASE + 0x100).unwrap_or(0xDEAD);
    eprintln!("Mid-boot: FECS cpuctl={fecs_mid:#010x} GPCCS cpuctl={gpccs_mid:#010x}");

    // DMEM dump — see what the BL loaded and what the ACR wrote
    {
        let read_dmem = |off: u32| -> u32 {
            let _ = bar0.write_u32(SEC2_BASE + 0x1C0, (1u32 << 25) | off);
            bar0.read_u32(SEC2_BASE + 0x1C4).unwrap_or(0xDEAD)
        };

        // Dump ACR descriptor region (data_section[0x200..0x270] = DMEM[0x200..0x270])
        eprintln!("DMEM ACR descriptor after boot:");
        eprintln!(
            "  [0x200] signatures: {:08x} {:08x} {:08x} {:08x}",
            read_dmem(0x200),
            read_dmem(0x204),
            read_dmem(0x208),
            read_dmem(0x20C)
        );
        eprintln!(
            "  [0x210] wpr_region_id={:#x} wpr_off={:#x} mmu_range={:#x} no_regions={:#x}",
            read_dmem(0x210),
            read_dmem(0x214),
            read_dmem(0x218),
            read_dmem(0x21C)
        );
        eprintln!(
            "  [0x220] r0: start={:#x} end={:#x} id={:#x} read={:#x}",
            read_dmem(0x220),
            read_dmem(0x224),
            read_dmem(0x228),
            read_dmem(0x22C)
        );
        eprintln!(
            "  [0x230] r0: write={:#x} client={:#x} shadow={:#x}",
            read_dmem(0x230),
            read_dmem(0x234),
            read_dmem(0x238)
        );
        eprintln!(
            "  [0x258] blob_size={:#x} blob_base={:#x}_{:08x}",
            read_dmem(0x258),
            read_dmem(0x264),
            read_dmem(0x260)
        );

        // Scan DMEM for non-zero regions outside the descriptor
        // First 32 words (BL descriptor area at offset 0)
        eprintln!("DMEM BL desc area [0x00..0x54]:");
        for off in (0..0x54).step_by(16) {
            let w: Vec<String> = (0..4)
                .map(|i| format!("{:08x}", read_dmem(off + i * 4)))
                .collect();
            eprintln!("  [{off:#05x}] {}", w.join(" "));
        }

        // The BL may load data to DMEM at its ORIGINAL offset (data_dma_base=0x2F00),
        // not at offset 0. Check the ACR descriptor at DMEM[0x2F00 + 0x210] = DMEM[0x3110].
        eprintln!("DMEM ACR descriptor at data_off-relative (DMEM[0x2F00+]):");
        let d2 = 0x2F00u32;
        eprintln!(
            "  [+0x210] wpr_region_id={:#x} wpr_off={:#x} no_regions={:#x}",
            read_dmem(d2 + 0x210),
            read_dmem(d2 + 0x214),
            read_dmem(d2 + 0x21C)
        );
        eprintln!(
            "  [+0x220] r0: start={:#x} end={:#x} shadow={:#x}",
            read_dmem(d2 + 0x220),
            read_dmem(d2 + 0x224),
            read_dmem(d2 + 0x238)
        );
        eprintln!(
            "  [+0x258] blob_size={:#x} blob_base={:#x}_{:08x}",
            read_dmem(d2 + 0x258),
            read_dmem(d2 + 0x264),
            read_dmem(d2 + 0x260)
        );

        // Wide scan: every 256 bytes in 0..0x8000 (32KB of 64KB DMEM)
        let mut nz_regions = Vec::new();
        for off in (0u32..0x8000).step_by(256) {
            let v = read_dmem(off);
            if v != 0 {
                nz_regions.push((off, v));
            }
        }
        eprintln!(
            "DMEM non-zero samples (every 256B in 0..32K): {} hits",
            nz_regions.len()
        );
        for (off, v) in &nz_regions {
            eprintln!("  [{off:#06x}] = {v:#010x}");
        }

        // Dense scan of data section area (0x2E00..0x4000) every 16B
        let mut nz_data = Vec::new();
        for off in (0x2E00u32..0x4000).step_by(16) {
            let v = read_dmem(off);
            if v != 0 {
                nz_data.push((off, v));
            }
        }
        eprintln!(
            "DMEM data section area (0x2E00..0x4000): {} non-zero",
            nz_data.len()
        );
        for (off, v) in nz_data.iter().take(16) {
            eprintln!("  [{off:#06x}] = {v:#010x}");
        }
    }

    // EMEM dump after ACR boot — wide scan to find any firmware-written data
    {
        use coral_driver::nv::vfio_compute::acr_boot::sec2_emem_read;
        let emem_post3 = sec2_emem_read(bar0, 0, 256);
        let nz = emem_post3.iter().filter(|&&w| w != 0).count();
        eprintln!("EMEM after ACR boot: {nz}/256 non-zero");
        for (i, chunk) in emem_post3.chunks(8).enumerate() {
            let any_nz = chunk.iter().any(|&w| w != 0);
            if any_nz {
                let vals: Vec<String> = chunk.iter().map(|w| format!("{w:#010x}")).collect();
                eprintln!("  EMEM[{:3}..{:3}]: {}", i * 8, i * 8 + 8, vals.join(" "));
            }
        }
        let pri_p3 = bar0.read_u32(0x120058).unwrap_or(0xDEAD);
        eprintln!("Priv ring after ACR boot: {pri_p3:#010x}");
    }

    // Queue registers: CMDQ at 0xA00/0xA04, MSGQ at 0xA30/0xA34 (Exp 089b)
    let cmdq_h = bar0.read_u32(SEC2_BASE + 0xA00).unwrap_or(0xDEAD);
    let cmdq_t = bar0.read_u32(SEC2_BASE + 0xA04).unwrap_or(0xDEAD);
    let msgq_h = bar0.read_u32(SEC2_BASE + 0xA30).unwrap_or(0xDEAD);
    let msgq_t = bar0.read_u32(SEC2_BASE + 0xA34).unwrap_or(0xDEAD);
    let queues_alive = cmdq_h != 0 || cmdq_t != 0 || msgq_h != 0 || msgq_t != 0;
    eprintln!(
        "Queues: CMDQ h={cmdq_h:#x} t={cmdq_t:#x} | MSGQ h={msgq_h:#x} t={msgq_t:#x} alive={queues_alive}"
    );
    // Scan both queue register ranges for non-zero values
    {
        let mut q_nz = Vec::new();
        for off in (0xA00u32..=0xAFF).step_by(4) {
            let v = r(off as usize);
            if v != 0 {
                q_nz.push((off, v));
            }
        }
        for off in (0xC00u32..=0xCFF).step_by(4) {
            let v = r(off as usize);
            if v != 0 {
                q_nz.push((off, v));
            }
        }
        if !q_nz.is_empty() {
            eprintln!(
                "Non-zero queue regs in 0xA00..0xAFF,0xC00..0xCFF: {:?}",
                q_nz.iter()
                    .map(|(o, v)| format!("{o:#05x}={v:#x}"))
                    .collect::<Vec<_>>()
            );
        }
    }

    // Timer registers: if the firmware's polling loop waits on a timer
    eprintln!(
        "Timers: GPTMR={:#010x} TIMPRE={:#010x} FTIMER={:#010x}",
        r(0x020),
        r(0x024),
        r(0x028)
    );

    // FECS/GPCCS state after ACR
    let fecs_post = bar0.read_u32(FECS_BASE + 0x100).unwrap_or(0xDEAD);
    let gpccs_post = bar0.read_u32(GPCCS_BASE + 0x100).unwrap_or(0xDEAD);
    eprintln!("After ACR: FECS cpuctl={fecs_post:#010x} GPCCS cpuctl={gpccs_post:#010x}");

    if fecs_post & 0x10 == 0 && fecs_post & 0x20 == 0 {
        eprintln!("*** FECS LEFT HRESET! ***");
    }
    if gpccs_post & 0x10 == 0 && gpccs_post & 0x20 == 0 {
        eprintln!("*** GPCCS LEFT HRESET! ***");
    }
}
