// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::helpers::{init_tracing, open_vfio, vfio_bdf};

/// Exp 095: Sovereign ACR boot with VRAM recovery via nouveau DEVINIT cycle.
///
/// GV100 DEVINIT (clock + HBM2 + VRAM init) requires signed PMU firmware —
/// only nouveau/nvidia drivers can execute it. If VRAM is dead (from a prior
/// SBR), we cycle the GPU through nouveau via GlowPlug to run DEVINIT, then
/// re-acquire the VFIO device with live VRAM.
///
/// Phase 0: Quick VRAM probe — if alive, skip to Phase 2.
/// Phase 1: Nouveau DEVINIT cycle via GlowPlug device.swap.
/// Phase 2: SEC2 soft reset (ENGCTL/PMC, preserves VRAM).
/// Phase 3: VRAM ACR boot → FECS/GPCCS bootstrap.
#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn vfio_clean_vram_acr_boot() {
    use coral_driver::vfio::memory::{MemoryRegion, PraminRegion};

    const SEC2_BASE: usize = 0x087000;
    const FECS_BASE: usize = 0x409000;
    const GPCCS_BASE: usize = 0x41a000;

    init_tracing();

    eprintln!("\n=== Exp 095: Sovereign ACR Boot ===\n");

    let bdf = vfio_bdf();

    // ── Phase 0: Quick VRAM probe ──
    eprintln!("── Phase 0: VRAM probe ──");
    let mut dev = open_vfio();
    let _vram_alive = {
        let bar0 = dev.bar0_ref();
        let win = bar0.read_u32(0x1700).unwrap_or(0xDEAD);
        eprintln!("BAR0_WINDOW = {win:#010x}");

        match PraminRegion::new(bar0, 0x2_6000, 8) {
            Ok(mut region) => {
                let sentinel = 0xCAFE_DEAD_u32;
                let _ = region.write_u32(0, sentinel);
                let rb = region.read_u32(0).unwrap_or(0);
                let ok = rb == sentinel;
                eprintln!("PRAMIN sentinel: wrote={sentinel:#010x} read={rb:#010x} ok={ok}");
                ok
            }
            Err(e) => {
                eprintln!("PRAMIN failed: {e}");
                false
            }
        }
    };

    // ── Phase 1: Always cycle through nouveau for fresh DEVINIT + signed firmware ──
    // Nouveau's ACR boot loads properly signed firmware into SEC2's IMEM.
    // After swap back to vfio-pci, SEC2 goes to HRESET but IMEM is preserved.
    // We can then restart SEC2 with its existing authenticated code.
    {
        eprintln!("\n── Phase 1: Nouveau cycle (signed firmware load) ──");
        drop(dev);
        eprintln!("Dropped VFIO device handle");

        let mut gp =
            crate::glowplug_client::GlowPlugClient::connect().expect("GlowPlug connection");

        eprintln!("Swapping to nouveau for DEVINIT + ACR boot...");
        match gp.swap(&bdf, "nouveau") {
            Ok(r) => eprintln!("swap→nouveau: {r}"),
            Err(e) => {
                eprintln!("swap→nouveau FAILED: {e}");
                eprintln!("=== End Exp 095 (no GlowPlug) ===");
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(3));

        eprintln!("Swapping back to vfio-pci...");
        match gp.swap(&bdf, "vfio-pci") {
            Ok(r) => eprintln!("swap→vfio-pci: {r}"),
            Err(e) => eprintln!("swap→vfio-pci FAILED: {e}"),
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    dev = open_vfio();
    let bar0 = dev.bar0_ref();
    let base = SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD_DEAD);

    // VRAM sanity check after nouveau cycle
    let vram_ok = match PraminRegion::new(bar0, 0x2_6000, 8) {
        Ok(mut region) => {
            let sentinel = 0xCAFE_DEAD_u32;
            let _ = region.write_u32(0, sentinel);
            let rb = region.read_u32(0).unwrap_or(0);
            let ok = rb == sentinel;
            eprintln!("VRAM after nouveau cycle: {ok}");
            ok
        }
        Err(_) => false,
    };
    if !vram_ok {
        eprintln!("VRAM dead after nouveau cycle — aborting");
        eprintln!("\n=== End Exp 095 ===");
        return;
    }

    // ── Phase 2: Probe SEC2/FECS/GPCCS + hardware WPR boundaries ──
    eprintln!("\n── Phase 2: Post-nouveau state ──");
    let cpuctl = r(0x100);
    let sctl = r(0x240);
    let pc = r(0x030);
    let tidx = r(0x148); // TRACE INDEX, NOT exci (see gm200_flcn_tracepc)
    let hwcfg = r(0x108);
    eprintln!(
        "SEC2: cpuctl={cpuctl:#010x} sctl={sctl:#010x} pc={pc:#06x} tidx={tidx:#010x} hwcfg={hwcfg:#010x}"
    );

    let fecs_cpu = bar0.read_u32(FECS_BASE + 0x100).unwrap_or(0xDEAD);
    let gpccs_cpu = bar0.read_u32(GPCCS_BASE + 0x100).unwrap_or(0xDEAD);
    eprintln!("FECS: cpuctl={fecs_cpu:#010x} | GPCCS: cpuctl={gpccs_cpu:#010x}");

    // PMU state — ACR may depend on PMU for power/clock management
    let pmu_cpu2 = bar0.read_u32(0x10A100).unwrap_or(0xDEAD);
    let pmu_sctl2 = bar0.read_u32(0x10A240).unwrap_or(0xDEAD);
    let pmu_pc = bar0.read_u32(0x10A030).unwrap_or(0xDEAD);
    eprintln!("PMU: cpuctl={pmu_cpu2:#010x} sctl={pmu_sctl2:#010x} pc={pmu_pc:#06x}");

    // IMEM probe: check if nouveau's firmware survived the driver swap
    // On falcon, IMEMC bit25=read mode, bits[15:2]=word address
    let read_imem = |base: usize, word_off: u32| -> u32 {
        let _ = bar0.write_u32(base + 0x180, (1u32 << 25) | (word_off & 0xFFFC));
        bar0.read_u32(base + 0x184).unwrap_or(0xDEAD)
    };
    let sec2_i0 = read_imem(SEC2_BASE, 0);
    let sec2_i4 = read_imem(SEC2_BASE, 4);
    let sec2_i8 = read_imem(SEC2_BASE, 8);
    let fecs_i0 = read_imem(FECS_BASE, 0);
    let fecs_i4 = read_imem(FECS_BASE, 4);
    let gpccs_i0 = read_imem(GPCCS_BASE, 0);
    let gpccs_i4 = read_imem(GPCCS_BASE, 4);
    let sec2_populated = sec2_i0 != 0 || sec2_i4 != 0 || sec2_i8 != 0;
    let fecs_populated = fecs_i0 != 0 || fecs_i4 != 0;
    let gpccs_populated = gpccs_i0 != 0 || gpccs_i4 != 0;
    eprintln!(
        "IMEM after swap: SEC2[0..8]=[{sec2_i0:#010x} {sec2_i4:#010x} {sec2_i8:#010x}] populated={sec2_populated}"
    );
    eprintln!("  FECS[0..4]=[{fecs_i0:#010x} {fecs_i4:#010x}] populated={fecs_populated}");
    eprintln!("  GPCCS[0..4]=[{gpccs_i0:#010x} {gpccs_i4:#010x}] populated={gpccs_populated}");

    // DMA state left by nouveau (preserved across vfio-pci bind?)
    let fbif_post = r(0x624);
    let dmactl_post = r(0x10C);
    let itfen_post = r(0x048);
    let bind_inst_post = bar0.read_u32(SEC2_BASE + 0x054).unwrap_or(0xDEAD);
    eprintln!(
        "DMA state: FBIF={fbif_post:#010x} DMACTL={dmactl_post:#010x} ITFEN={itfen_post:#010x} BIND_INST={bind_inst_post:#010x}"
    );

    // ── Clear priv ring faults (nouveau gk104_privring_intr pattern) ──
    // Nouveau: nvkm_mask(device, 0x12004c, 0x3f, 0x02) to ACK fault at 0x120058 bit 8.
    let pri_status = bar0.read_u32(0x120058).unwrap_or(0);
    if pri_status & 0x100 != 0 {
        let cur = bar0.read_u32(0x12004C).unwrap_or(0);
        let _ = bar0.write_u32(0x12004C, (cur & !0x3F) | 0x02);
        for _ in 0..200 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            let s = bar0.read_u32(0x120058).unwrap_or(0xDEAD);
            if s & 0x100 == 0 {
                break;
            }
        }
    }
    let pri_after = bar0.read_u32(0x120058).unwrap_or(0xDEAD);
    eprintln!("Priv ring: status_before={pri_status:#010x} status_after={pri_after:#010x}");

    // PFB WPR registers (direct, from experiment 086)
    // Exp 086 finding: "WPR is NEVER configured as persistent hardware state —
    // WPR must be set up dynamically by SEC2 firmware during ACR execution."
    let wpr1_beg = bar0.read_u32(0x100CE4).unwrap_or(0xDEAD);
    let wpr1_end = bar0.read_u32(0x100CE8).unwrap_or(0xDEAD);
    let wpr2_beg = bar0.read_u32(0x100CEC).unwrap_or(0xDEAD);
    let wpr2_end = bar0.read_u32(0x100CF0).unwrap_or(0xDEAD);
    eprintln!("PFB WPR1: beg={wpr1_beg:#010x} end={wpr1_end:#010x}");
    eprintln!("PFB WPR2: beg={wpr2_beg:#010x} end={wpr2_end:#010x}");

    // GM200 indexed register (0x100cd4) — used by gm200_acr_wpr_check
    let _ = bar0.write_u32(0x100cd4, 2);
    std::thread::sleep(std::time::Duration::from_micros(10));
    let wpr_lo_raw = bar0.read_u32(0x100cd4).unwrap_or(0xDEAD);
    let _ = bar0.write_u32(0x100cd4, 3);
    std::thread::sleep(std::time::Duration::from_micros(10));
    let wpr_hi_raw = bar0.read_u32(0x100cd4).unwrap_or(0xDEAD);
    let gm200_start = ((wpr_lo_raw as u64) & 0xFFFF_FF00) << 8;
    let gm200_end = (((wpr_hi_raw as u64) & 0xFFFF_FF00) << 8) + 0x20000;
    eprintln!(
        "GM200 indexed: lo_raw={wpr_lo_raw:#010x} hi_raw={wpr_hi_raw:#010x} → {gm200_start:#x}..{gm200_end:#x}"
    );

    // FBPA WPR2 (often returns 0xBADF1100 = priv ring dead)
    let fbpa_lo = bar0.read_u32(0x1fa824).unwrap_or(0xBADF_1100);
    let fbpa_hi = bar0.read_u32(0x1fa828).unwrap_or(0xBADF_1100);
    eprintln!("FBPA WPR2: lo={fbpa_lo:#010x} hi={fbpa_hi:#010x}");

    // FBHUB MMU fault buffer state (Exp 076: FBHUB stalls without a valid drain target)
    {
        let fb0_lo = bar0.read_u32(0x100E24).unwrap_or(0xDEAD);
        let fb0_hi = bar0.read_u32(0x100E28).unwrap_or(0xDEAD);
        let fb0_sz = bar0.read_u32(0x100E2C).unwrap_or(0xDEAD);
        let fb0_get = bar0.read_u32(0x100E30).unwrap_or(0xDEAD);
        let fb0_put = bar0.read_u32(0x100E34).unwrap_or(0xDEAD);
        let fb0_addr = ((fb0_hi as u64) << 44) | ((fb0_lo as u64) << 12);
        let fb0_enabled = fb0_put & 0x8000_0000 != 0;
        eprintln!(
            "FAULT_BUF0 (non-replay): addr={fb0_addr:#x} sz={fb0_sz} get={fb0_get:#x} put={fb0_put:#010x} enabled={fb0_enabled}"
        );

        let fb1_lo = bar0.read_u32(0x100E44).unwrap_or(0xDEAD);
        let fb1_hi = bar0.read_u32(0x100E48).unwrap_or(0xDEAD);
        let fb1_sz = bar0.read_u32(0x100E4C).unwrap_or(0xDEAD);
        let fb1_get = bar0.read_u32(0x100E50).unwrap_or(0xDEAD);
        let fb1_put = bar0.read_u32(0x100E54).unwrap_or(0xDEAD);
        let fb1_enabled = fb1_put & 0x8000_0000 != 0;
        eprintln!(
            "FAULT_BUF1 (replay):     addr={:#x} sz={fb1_sz} get={fb1_get:#x} put={fb1_put:#010x} enabled={fb1_enabled}",
            ((fb1_hi as u64) << 44) | ((fb1_lo as u64) << 12)
        );

        // FBHUB status: PRI accessibility check
        let fbhub_c2c = bar0.read_u32(0x100C2C).unwrap_or(0xDEAD);
        let mmu_ctrl = bar0.read_u32(0x100C80).unwrap_or(0xDEAD);
        let mmu_phys = bar0.read_u32(0x100C94).unwrap_or(0xDEAD);
        eprintln!(
            "FBHUB: 0x100C2C={fbhub_c2c:#010x} MMU_CTRL={mmu_ctrl:#010x} MMU_PHYS={mmu_phys:#010x}"
        );
        if fbhub_c2c == 0xBADF_5040 {
            eprintln!("  *** FBHUB PRI ERROR — hub not accessible! ***");
        }
    }

    // ── Probe SEC2 DMEM for nouveau's ACR descriptor (before our reset!) ──
    // The ACR data section in DMEM contains flcn_acr_desc_v1 with WPR addresses.
    // Layout: 0x200=reserved, 0x210=wpr_region_id, 0x260=ucode_blob_base
    let read_dmem_u32 = |off: u32| -> u32 {
        let _ = bar0.write_u32(SEC2_BASE + 0x1C0, (1u32 << 25) | off);
        bar0.read_u32(SEC2_BASE + 0x1C4).unwrap_or(0xDEAD)
    };
    let read_dmem_u64 = |off: u32| -> u64 {
        let lo = read_dmem_u32(off) as u64;
        let hi = read_dmem_u32(off + 4) as u64;
        lo | (hi << 32)
    };
    let dmem_wpr_id = read_dmem_u32(0x210);
    let dmem_wpr_offset = read_dmem_u32(0x214);
    let dmem_mmu_range = read_dmem_u32(0x218);
    let dmem_no_regions = read_dmem_u32(0x21C);
    let dmem_r0_start = read_dmem_u32(0x220);
    let dmem_r0_end = read_dmem_u32(0x224);
    let dmem_r0_shadow = read_dmem_u32(0x238);
    let dmem_blob_size = read_dmem_u32(0x258);
    let dmem_blob_base = read_dmem_u64(0x260);
    eprintln!("Nouveau ACR desc (DMEM):");
    eprintln!(
        "  wpr_id={dmem_wpr_id} wpr_off={dmem_wpr_offset:#x} mmu_range={dmem_mmu_range:#x} regions={dmem_no_regions}"
    );
    eprintln!("  r0: start={dmem_r0_start:#x} end={dmem_r0_end:#x} shadow={dmem_r0_shadow:#x}");
    eprintln!("  blob: base={dmem_blob_base:#x} size={dmem_blob_size:#x}");

    // Nouveau's actual WPR location: blob_base points to WPR in VRAM
    // r0.start/end are addr>>8, so real addresses are <<8
    let nouveau_wpr_start = (dmem_r0_start as u64) << 8;
    let nouveau_wpr_end = (dmem_r0_end as u64) << 8;
    eprintln!(
        "  Nouveau WPR region: {nouveau_wpr_start:#x}..{nouveau_wpr_end:#x} ({} KB)",
        (nouveau_wpr_end.saturating_sub(nouveau_wpr_start)) / 1024
    );

    // ── Phase 2.5: Probe EMEM + try restarting nouveau's SEC2 ──
    //
    // Before we overwrite SEC2, probe its EMEM for queue descriptors that
    // nouveau's ACR firmware left behind, and attempt to restart the halted
    // firmware to see if it enters its command loop.
    eprintln!("\n── Phase 2.5: EMEM probe + SEC2 restart ──");
    {
        use coral_driver::nv::vfio_compute::acr_boot::sec2_emem_read;
        let emem = sec2_emem_read(bar0, 0, 128);
        let nonzero_count = emem.iter().filter(|&&w| w != 0).count();
        eprintln!("EMEM[0..128]: {nonzero_count} non-zero words");
        if nonzero_count > 0 {
            for (i, chunk) in emem.chunks(8).enumerate() {
                let any_nz = chunk.iter().any(|&w| w != 0);
                if any_nz {
                    let vals: Vec<String> = chunk.iter().map(|w| format!("{w:#010x}")).collect();
                    eprintln!("  EMEM[{:3}..{:3}]: {}", i * 8, i * 8 + 8, vals.join(" "));
                }
            }
        }

        // Also read queue registers before any reset
        let qh0_pre = bar0.read_u32(SEC2_BASE + 0xC00).unwrap_or(0xDEAD);
        let qt0_pre = bar0.read_u32(SEC2_BASE + 0xC04).unwrap_or(0xDEAD);
        let qh1_pre = bar0.read_u32(SEC2_BASE + 0xC08).unwrap_or(0xDEAD);
        let qt1_pre = bar0.read_u32(SEC2_BASE + 0xC0C).unwrap_or(0xDEAD);
        eprintln!(
            "Pre-restart queues: Q0 h={qh0_pre:#x} t={qt0_pre:#x} | Q1 h={qh1_pre:#x} t={qt1_pre:#x}"
        );

        // Try restarting from HALT via CPUCTL_ALIAS (accessible in LS mode)
        let cpuctl_pre = r(0x100);
        let pc_pre = bar0.read_u32(SEC2_BASE + 0x104).unwrap_or(0xDEAD);
        eprintln!("SEC2 before restart: cpuctl={cpuctl_pre:#010x} pc={pc_pre:#06x}");

        if cpuctl_pre & 0x10 != 0 {
            // HALTED — try resuming via CPUCTL_ALIAS (0x130)
            eprintln!("  SEC2 HALTED — writing CPUCTL_ALIAS START...");
            let _ = bar0.write_u32(SEC2_BASE + 0x130, 0x02); // START bit

            // Wait up to 500ms for it to do something
            for tick in 0..50 {
                std::thread::sleep(std::time::Duration::from_millis(10));
                let c = r(0x100);
                let p = bar0.read_u32(SEC2_BASE + 0x104).unwrap_or(0xDEAD);
                if tick == 0 || tick == 4 || tick == 49 || c != cpuctl_pre || p != pc_pre {
                    eprintln!("  t={:3}ms cpuctl={c:#010x} pc={p:#06x}", (tick + 1) * 10);
                }
                if c & 0x10 != 0 && tick > 5 {
                    break;
                } // re-halted
            }

            let cpuctl_post = r(0x100);
            let pc_post = bar0.read_u32(SEC2_BASE + 0x104).unwrap_or(0xDEAD);
            let mb0_post = bar0.read_u32(SEC2_BASE + 0x040).unwrap_or(0xDEAD);
            let mb1_post = bar0.read_u32(SEC2_BASE + 0x044).unwrap_or(0xDEAD);
            eprintln!(
                "SEC2 after restart: cpuctl={cpuctl_post:#010x} pc={pc_post:#06x} mb0={mb0_post:#010x} mb1={mb1_post:#010x}"
            );

            // Check queues after restart
            let qh0_r = bar0.read_u32(SEC2_BASE + 0xA00).unwrap_or(0xDEAD);
            let qt0_r = bar0.read_u32(SEC2_BASE + 0xA04).unwrap_or(0xDEAD);
            let qh1_r = bar0.read_u32(SEC2_BASE + 0xA30).unwrap_or(0xDEAD);
            let qt1_r = bar0.read_u32(SEC2_BASE + 0xA34).unwrap_or(0xDEAD);
            let q_alive = qh0_r != 0 || qt0_r != 0 || qh1_r != 0 || qt1_r != 0;
            eprintln!(
                "Post-restart queues: Q0 h={qh0_r:#x} t={qt0_r:#x} | Q1 h={qh1_r:#x} t={qt1_r:#x} alive={q_alive}"
            );

            // Read EMEM after restart
            let emem_post = sec2_emem_read(bar0, 0, 32);
            let nz_post = emem_post.iter().filter(|&&w| w != 0).count();
            eprintln!("EMEM after restart: {nz_post}/32 non-zero");

            // Check priv ring for new faults
            let pri_post = bar0.read_u32(0x120058).unwrap_or(0xDEAD);
            eprintln!("Priv ring after restart: {pri_post:#010x}");
        } else {
            eprintln!("  SEC2 not halted (maybe running?) — skipping restart");
        }
    }

    // ── Phase 2.75: DEFERRED — runs after Phase 3 to avoid HS mode corruption ──
    // HS mode (SCTL bit 1) entered by sysmem BL authentication is NOT clearable
    // via PMC reset, corrupting all subsequent IMEM/DMEM operations.
    // Phase 3 needs fresh SEC2 (SCTL=0x3000) for VRAM DMA diagnostics.

    // ── Phase 3 first: ACR boot (VRAM DMA, v2 desc) — see below ──

    // ── Phase 2.75: Sysmem ACR boot (FBHUB bypass, blob_size=0) ──
    // Re-enabled: sysmem approach enters HS mode (SCTL=0x3002).
    // With blob_size=0 patch in `strategy_sysmem`, the ACR should skip
    // the blob DMA that previously caused a trap.
    {
        eprintln!("\n── Phase 2.75: Sysmem ACR boot (FBHUB bypass attempt) ──");
        {
            let dump_sec2_post = |label: &str| {
                let sctl = bar0.read_u32(SEC2_BASE + 0x240).unwrap_or(0xDEAD);
                let cpuctl = bar0.read_u32(SEC2_BASE + 0x100).unwrap_or(0xDEAD);
                let pc = bar0.read_u32(SEC2_BASE + 0x030).unwrap_or(0xDEAD);
                let mb0 = bar0.read_u32(SEC2_BASE + 0x040).unwrap_or(0xDEAD);
                let hs_mode = sctl & 0x02 != 0;
                eprintln!(
                    "  {label}: sctl={sctl:#010x} cpuctl={cpuctl:#010x} pc={pc:#06x} mb0={mb0:#010x} HS={hs_mode}"
                );

                // TRACEPC dump (may be readable even in HS mode)
                let tidx = bar0.read_u32(SEC2_BASE + 0x148).unwrap_or(0);
                let nr_traces = ((tidx & 0x00FF_0000) >> 16).min(32);
                if nr_traces > 0 {
                    let mut traces = Vec::new();
                    for i in 0..nr_traces {
                        let _ = bar0.write_u32(SEC2_BASE + 0x148, i);
                        let tpc = bar0.read_u32(SEC2_BASE + 0x14C).unwrap_or(0xDEAD);
                        traces.push(format!("{tpc:#06x}"));
                    }
                    eprintln!("  TRACEPC[0..{nr_traces}]: {}", traces.join(" "));
                } else {
                    eprintln!("  TRACEPC: {nr_traces} entries (tidx={tidx:#010x})");
                }

                // DMA engine state
                let trfbase = bar0.read_u32(SEC2_BASE + 0x110).unwrap_or(0xDEAD);
                let trfcmd = bar0.read_u32(SEC2_BASE + 0x11C).unwrap_or(0xDEAD);
                let dmactl = bar0.read_u32(SEC2_BASE + 0x10C).unwrap_or(0xDEAD);
                let fbif0 = bar0.read_u32(SEC2_BASE + 0x620).unwrap_or(0xDEAD);
                let fbif1 = bar0.read_u32(SEC2_BASE + 0x624).unwrap_or(0xDEAD);
                eprintln!(
                    "  DMA: trfbase={trfbase:#010x} trfcmd={trfcmd:#010x} dmactl={dmactl:#010x} fbif[0]={fbif0:#06x} fbif[1]={fbif1:#06x}"
                );

                // Queue state
                let cmdq_h = bar0.read_u32(SEC2_BASE + 0xA00).unwrap_or(0);
                let cmdq_t = bar0.read_u32(SEC2_BASE + 0xA04).unwrap_or(0);
                let msgq_h = bar0.read_u32(SEC2_BASE + 0xA30).unwrap_or(0);
                let msgq_t = bar0.read_u32(SEC2_BASE + 0xA34).unwrap_or(0);
                if cmdq_h != 0 || cmdq_t != 0 || msgq_h != 0 || msgq_t != 0 {
                    eprintln!(
                        "  Queues ALIVE: CMDQ h={cmdq_h:#x} t={cmdq_t:#x} MSGQ h={msgq_h:#x} t={msgq_t:#x}"
                    );
                }

                // FECS/GPCCS state
                let fecs = bar0.read_u32(FECS_BASE + 0x100).unwrap_or(0xDEAD);
                let gpccs = bar0.read_u32(GPCCS_BASE + 0x100).unwrap_or(0xDEAD);
                eprintln!("  FECS={fecs:#010x} GPCCS={gpccs:#010x}");
                if fecs & 0x10 == 0 && fecs & 0x20 == 0 {
                    eprintln!("  *** FECS LEFT HRESET! ***");
                }
                if gpccs & 0x10 == 0 && gpccs & 0x20 == 0 {
                    eprintln!("  *** GPCCS LEFT HRESET! ***");
                }
            };

            eprintln!(
                "Strategy 1: sysmem_acr_boot (instance block + page tables in system memory)"
            );
            let sysmem_result = dev.sysmem_acr_boot();
            eprintln!(
                "  result: success={} strategy={}",
                sysmem_result.success, sysmem_result.strategy
            );
            for note in &sysmem_result.notes {
                eprintln!("  | {note}");
            }
            dump_sec2_post("post-sysmem");

            // PMC reset SEC2 to clear HS mode before next attempt
            {
                let pmc = bar0.read_u32(0x200).unwrap_or(0);
                let sec2_bit = 1u32 << 22;
                let _ = bar0.write_u32(0x200, pmc & !sec2_bit);
                std::thread::sleep(std::time::Duration::from_micros(50));
                let _ = bar0.write_u32(0x200, pmc | sec2_bit);
                for _ in 0..1000 {
                    let d = bar0.read_u32(SEC2_BASE + 0x10C).unwrap_or(0xFF);
                    if d & 0x06 == 0 {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_micros(100));
                }
                let sctl_post = bar0.read_u32(SEC2_BASE + 0x240).unwrap_or(0xDEAD);
                eprintln!(
                    "  PMC reset: sctl={sctl_post:#010x} (HS cleared={})",
                    sctl_post & 0x02 == 0
                );
            }

            eprintln!("Strategy 2: sysmem_physical_boot (PHYS_SYS, no instance block)");
            let phys_result = dev.sysmem_physical_boot();
            eprintln!(
                "  result: success={} strategy={}",
                phys_result.success, phys_result.strategy
            );
            for note in &phys_result.notes {
                eprintln!("  | {note}");
            }
            dump_sec2_post("post-phys");

            // PMC reset SEC2 to clear HS mode before Phase 3
            {
                let pmc = bar0.read_u32(0x200).unwrap_or(0);
                let sec2_bit = 1u32 << 22;
                let _ = bar0.write_u32(0x200, pmc & !sec2_bit);
                std::thread::sleep(std::time::Duration::from_micros(50));
                let _ = bar0.write_u32(0x200, pmc | sec2_bit);
                for _ in 0..1000 {
                    let d = bar0.read_u32(SEC2_BASE + 0x10C).unwrap_or(0xFF);
                    if d & 0x06 == 0 {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_micros(100));
                }
                let sctl_final = bar0.read_u32(SEC2_BASE + 0x240).unwrap_or(0xDEAD);
                eprintln!("  PMC reset for Phase 3: sctl={sctl_final:#010x}");
            }
        }
    } // end Phase 2.75

    // ── Phase 3: ACR boot (VRAM DMA, v2 desc) — DEFERRED ──
    // Phase 2.75 leaves SEC2 in HS mode, which PMC reset cannot clear.
    // Running Phase 3 on a post-HS SEC2 gives misleading results.
    #[allow(
        unreachable_code,
        reason = "early return guards above may make cleanup unreachable"
    )]
    if false {
        crate::falcon_exp095_phase3::exp095_phase3_deferred(&dev);
    }

    // ── Phase 3.5: Firmware interaction experiments ──
    // ACR is running at PC≈0x1934 with ALL interrupts disabled. Try to
    // communicate with it: write mailboxes, enable + trigger interrupts.
    eprintln!("\n── Phase 3.5: Firmware interaction ──");
    {
        let pc_before = r(0x030);
        let cpu_before = r(0x100);
        eprintln!("Before interaction: PC={pc_before:#06x} CPUCTL={cpu_before:#010x}");

        // Experiment 1: Write MB0 = 0x00000001 (host acknowledgment)
        let _ = bar0.write_u32(SEC2_BASE + 0x040, 0x0000_0001);
        std::thread::sleep(std::time::Duration::from_millis(100));
        let pc_a = r(0x030);
        let mb0_a = bar0.read_u32(SEC2_BASE + 0x040).unwrap_or(0xDEAD);
        eprintln!(
            "After MB0=1: PC={pc_a:#06x} MB0={mb0_a:#010x} (PC moved={})",
            pc_a != pc_before
        );

        // Experiment 2: Enable SWGEN0 interrupt and trigger it
        // Falcon IRQ layout: bit 6 = SWGEN0, bit 7 = SWGEN1
        let _ = bar0.write_u32(SEC2_BASE + 0x010, 1u32 << 6); // IRQMSET: enable SWGEN0
        let irqmask = r(0x018); // read back IRQMASK
        eprintln!("IRQMASK after IRQMSET(SWGEN0): {irqmask:#010x}");

        let _ = bar0.write_u32(SEC2_BASE, 1u32 << 6); // IRQSSET: trigger SWGEN0
        std::thread::sleep(std::time::Duration::from_millis(200));
        let pc_b = r(0x030);
        let irqstat_b = r(0x008);
        let mb0_b = bar0.read_u32(SEC2_BASE + 0x040).unwrap_or(0xDEAD);
        eprintln!(
            "After SWGEN0: PC={pc_b:#06x} IRQSTAT={irqstat_b:#010x} MB0={mb0_b:#010x} (PC moved={})",
            pc_b != pc_a
        );

        // Experiment 3: Also enable + trigger SWGEN1 (bit 7)
        let _ = bar0.write_u32(SEC2_BASE + 0x010, 1u32 << 7); // IRQMSET: enable SWGEN1
        let _ = bar0.write_u32(SEC2_BASE, 1u32 << 7); // IRQSSET: trigger SWGEN1
        std::thread::sleep(std::time::Duration::from_millis(200));
        let pc_c = r(0x030);
        let irqstat_c = r(0x008);
        let mb0_c = bar0.read_u32(SEC2_BASE + 0x040).unwrap_or(0xDEAD);
        eprintln!(
            "After SWGEN1: PC={pc_c:#06x} IRQSTAT={irqstat_c:#010x} MB0={mb0_c:#010x} (PC moved={})",
            pc_c != pc_b
        );

        // Experiment 4: Enable EXT interrupt (bit 0, external/engine interrupt)
        let _ = bar0.write_u32(SEC2_BASE + 0x010, 1u32 << 0); // IRQMSET: enable EXT(0)
        let _ = bar0.write_u32(SEC2_BASE, 1u32 << 0); // IRQSSET: trigger EXT(0)
        std::thread::sleep(std::time::Duration::from_millis(200));
        let pc_d = r(0x030);
        let irqstat_d = r(0x008);
        let mb0_d = bar0.read_u32(SEC2_BASE + 0x040).unwrap_or(0xDEAD);
        eprintln!(
            "After EXT(0): PC={pc_d:#06x} IRQSTAT={irqstat_d:#010x} MB0={mb0_d:#010x} (PC moved={})",
            pc_d != pc_c
        );

        // Final state
        let irqmask_fin = r(0x018);
        let irqstat_fin = r(0x008);
        let cpu_fin = r(0x100);
        let pc_fin = r(0x030);
        eprintln!(
            "Final: PC={pc_fin:#06x} CPUCTL={cpu_fin:#010x} IRQMASK={irqmask_fin:#010x} IRQSTAT={irqstat_fin:#010x}"
        );

        // Check if queues came alive after interaction (Exp 089b: CMDQ=0xA00, MSGQ=0xA30)
        let cmdq_h2 = bar0.read_u32(SEC2_BASE + 0xA00).unwrap_or(0);
        let cmdq_t2 = bar0.read_u32(SEC2_BASE + 0xA04).unwrap_or(0);
        let msgq_h2 = bar0.read_u32(SEC2_BASE + 0xA30).unwrap_or(0);
        let msgq_t2 = bar0.read_u32(SEC2_BASE + 0xA34).unwrap_or(0);
        let q_alive2 = cmdq_h2 != 0 || cmdq_t2 != 0 || msgq_h2 != 0 || msgq_t2 != 0;
        eprintln!(
            "Queues after interaction: CMDQ h={cmdq_h2:#x} t={cmdq_t2:#x} | MSGQ h={msgq_h2:#x} t={msgq_t2:#x} alive={q_alive2}"
        );

        // Re-read EMEM to see if firmware wrote anything new
        {
            use coral_driver::nv::vfio_compute::acr_boot::sec2_emem_read;
            let emem_int = sec2_emem_read(bar0, 0, 64);
            let nz_int = emem_int.iter().filter(|&&w| w != 0).count();
            if nz_int > 0 {
                eprintln!("EMEM after interaction: {nz_int}/64 non-zero");
                for (i, chunk) in emem_int.chunks(8).enumerate() {
                    let any_nz = chunk.iter().any(|&w| w != 0);
                    if any_nz {
                        let vals: Vec<String> =
                            chunk.iter().map(|w| format!("{w:#010x}")).collect();
                        eprintln!("  EMEM[{:3}..{:3}]: {}", i * 8, i * 8 + 8, vals.join(" "));
                    }
                }
            }
        }
    }

    // ── Phase 4: BOOTSTRAP_FALCON (mailbox + CMDQ, try both) ──
    let sec2_c = r(0x100);
    let sec2_running = sec2_c & 0x30 == 0;
    if sec2_running {
        eprintln!("\n── Phase 4: BOOTSTRAP_FALCON commands ──");

        // IMEM probe: Falcon IMEM uses block addressing (off>>6 per 64-byte block)
        // Read mode: write IMEMC = (block_addr >> 6) | (1<<25), then read 16 words
        let imem_read_block = |pc: u32| -> Vec<u32> {
            let block = pc & !0x3F;
            let _ = bar0.write_u32(SEC2_BASE + 0x180, (block >> 6) | (1 << 25));
            let mut words = Vec::new();
            for _ in 0..16 {
                words.push(bar0.read_u32(SEC2_BASE + 0x184).unwrap_or(0xDEAD));
            }
            words
        };

        // Read the current idle loop PC and key trace PCs
        let idle_pc = r(0x030);
        eprintln!("IMEM probe (block-addressed):");
        for &(label, pc) in &[
            ("idle", idle_pc),
            ("0x2d07", 0x2d07),
            ("0x1c32", 0x1c32),
            ("0x4e5a", 0x4e5a),
        ] {
            let block = imem_read_block(pc);
            let word_idx = ((pc & 0x3F) >> 2) as usize;
            let w0 = block.get(word_idx).copied().unwrap_or(0);
            let w1 = block.get(word_idx + 1).copied().unwrap_or(0);
            let all_zero = block.iter().all(|&w| w == 0);
            eprintln!(
                "  IMEM[{label}@{pc:#06x}]: [{word_idx}]={w0:#010x} [{next}]={w1:#010x} block_zero={all_zero}",
                next = word_idx + 1
            );
        }

        // Scan IMEM to find ANY non-zero region (is IMEM populated at all?)
        let mut nz_blocks = 0u32;
        let mut first_nz = 0u32;
        for blk in 0..1024u32 {
            let _ = bar0.write_u32(SEC2_BASE + 0x180, blk | (1 << 25));
            let w = bar0.read_u32(SEC2_BASE + 0x184).unwrap_or(0);
            if w != 0 {
                nz_blocks += 1;
                if first_nz == 0 {
                    first_nz = blk * 64;
                }
            }
        }
        eprintln!("  IMEM scan: {nz_blocks}/1024 non-zero blocks, first_nz_addr={first_nz:#x}");

        // Strategy A: Mailbox BOOTSTRAP_FALCON (from `strategy_mailbox`)
        eprintln!("Strategy A: Mailbox BOOTSTRAP_FALCON");
        let falcon_mask = (1u32 << 2) | (1u32 << 3); // FECS=2, GPCCS=3
        let _ = bar0.write_u32(SEC2_BASE + 0x044, falcon_mask); // MB1 = falcon mask
        let _ = bar0.write_u32(SEC2_BASE + 0x040, 1); // MB0 = BOOTSTRAP_FALCON cmd
        let _ = bar0.write_u32(SEC2_BASE, 1u32 << 6); // IRQSSET SWGEN0
        std::thread::sleep(std::time::Duration::from_millis(500));
        let mb0_a = bar0.read_u32(SEC2_BASE + 0x040).unwrap_or(0);
        let mb1_a = bar0.read_u32(SEC2_BASE + 0x044).unwrap_or(0);
        let pc_a = r(0x030);
        let fecs_a = bar0.read_u32(FECS_BASE + 0x100).unwrap_or(0xDEAD);
        let gpccs_a = bar0.read_u32(GPCCS_BASE + 0x100).unwrap_or(0xDEAD);
        eprintln!("  After 500ms: MB0={mb0_a:#x} MB1={mb1_a:#x} PC={pc_a:#06x}");
        eprintln!("  FECS={fecs_a:#010x} GPCCS={gpccs_a:#010x}");
        if fecs_a & 0x10 == 0 {
            eprintln!("  *** FECS LEFT HRESET! ***");
        }

        // Strategy B: CMDQ push (force even if h==t==0)
        eprintln!("Strategy B: CMDQ BOOTSTRAP_FALCON (forced)");
        const NV_SEC2_UNIT_ACR: u8 = 0x07;
        let send_bootstrap = |falcon_id: u32, seq: u8, label: &str| {
            let tail = bar0.read_u32(SEC2_BASE + 0xA04).unwrap_or(0);
            let cmd: [u32; 4] = [
                u32::from_le_bytes([NV_SEC2_UNIT_ACR, 16, 0x00, seq]),
                0x01, // BOOTSTRAP_FALCON
                0x01, // RESET_YES
                falcon_id,
            ];
            // Write cmd to EMEM at tail offset
            let _ = bar0.write_u32(SEC2_BASE + 0xAC0, tail | (1 << 24));
            for &w in &cmd {
                let _ = bar0.write_u32(SEC2_BASE + 0xAC4, w);
            }
            let new_tail = tail + 16;
            let _ = bar0.write_u32(SEC2_BASE + 0xA04, new_tail);
            let _ = bar0.write_u32(SEC2_BASE, 1u32 << 6); // IRQSSET SWGEN0
            eprintln!("  {label}: falcon_id={falcon_id} @tail={tail:#x}→{new_tail:#x}");
        };
        send_bootstrap(3, 1, "GPCCS");
        std::thread::sleep(std::time::Duration::from_secs(1));
        let fecs_b = bar0.read_u32(FECS_BASE + 0x100).unwrap_or(0xDEAD);
        let gpccs_b = bar0.read_u32(GPCCS_BASE + 0x100).unwrap_or(0xDEAD);
        eprintln!("  After GPCCS: FECS={fecs_b:#010x} GPCCS={gpccs_b:#010x}");

        send_bootstrap(2, 2, "FECS");
        std::thread::sleep(std::time::Duration::from_secs(1));

        // Strategy C: Also try unit_id=0x08 (seen in sec2_cmdq.rs)
        eprintln!("Strategy C: CMDQ with unit_id=0x08");
        {
            let tail = bar0.read_u32(SEC2_BASE + 0xA04).unwrap_or(0);
            let cmd: [u32; 4] = [
                0x0003_1008, // seq=0, ctrl=0x03, size=0x10, unit=0x08
                0x0000_0000, // cmd_type=0x00(BOOTSTRAP)
                0x0000_0000, // flags=RESET_YES
                0x0000_0003, // falcon_id=GPCCS
            ];
            let _ = bar0.write_u32(SEC2_BASE + 0xAC0, tail | (1 << 24));
            for &w in &cmd {
                let _ = bar0.write_u32(SEC2_BASE + 0xAC4, w);
            }
            let _ = bar0.write_u32(SEC2_BASE + 0xA04, tail + 16);
            let _ = bar0.write_u32(SEC2_BASE, 1u32 << 6);
        }
        std::thread::sleep(std::time::Duration::from_secs(1));

        // Final check
        let fecs_fin = bar0.read_u32(FECS_BASE + 0x100).unwrap_or(0xDEAD);
        let gpccs_fin = bar0.read_u32(GPCCS_BASE + 0x100).unwrap_or(0xDEAD);
        let pc_fin = r(0x030);
        let mb0_fin = bar0.read_u32(SEC2_BASE + 0x040).unwrap_or(0);
        let cpu_fin = r(0x100);
        eprintln!(
            "Phase 4 final: FECS={fecs_fin:#010x} GPCCS={gpccs_fin:#010x} PC={pc_fin:#06x} MB0={mb0_fin:#x} cpuctl={cpu_fin:#010x}"
        );
        if fecs_fin & 0x10 == 0 {
            eprintln!("*** FECS LEFT HRESET! ***");
        }
        if gpccs_fin & 0x10 == 0 {
            eprintln!("*** GPCCS LEFT HRESET! ***");
        }

        // TRACEPC after Phase 4
        let tidx_p4 = r(0x148);
        let nr_p4 = ((tidx_p4 & 0x00FF_0000) >> 16).min(32);
        if nr_p4 > 0 {
            let mut traces = Vec::new();
            for i in 0..nr_p4 {
                let _ = bar0.write_u32(SEC2_BASE + 0x148, i);
                let tpc = bar0.read_u32(SEC2_BASE + 0x14C).unwrap_or(0xDEAD);
                traces.push(format!("{tpc:#06x}"));
            }
            eprintln!("TRACEPC after Phase 4: {}", traces.join(" "));
        }
    } else {
        eprintln!("\nSEC2 not running (cpuctl={sec2_c:#010x})");
    }

    // ── Final state ──
    eprintln!("\n── Final state ──");
    let probe = dev.falcon_probe();
    eprintln!("{probe}");

    eprintln!("\n=== End Exp 095 ===");
}
