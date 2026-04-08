// SPDX-License-Identifier: AGPL-3.0-only
//! MMU fault diagnostic — opens VFIO device, creates channel + page tables,
//! submits NOP GPFIFO, then captures structured MMU fault state.
//!
//! Usage:
//!   Direct: `cargo run --example bench_mmu_fault_diagnostic --features vfio -- <BDF>`
//!   Ember:  `cargo run --example bench_mmu_fault_diagnostic --features vfio -- --ember <BDF>`
//!
//! In `--ember` mode, BAR0 diagnostics run via FD sharing from coral-ember.
//! Channel creation + NOP dispatch are skipped (no DMA buffers in ember mode).
//!
//! Requires: GPU bound to `vfio-pci`, IOMMU enabled.

use coral_driver::nv::vfio_compute::RawVfioDevice;
use coral_driver::nv::vfio_compute::falcon_capability::FalconProbe;
use coral_driver::vfio::channel::VfioChannel;
use coral_driver::vfio::channel::mmu_fault;
use coral_driver::vfio::device::MappedBar;
use coral_driver::vfio::ember_client::EmberSession;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter("coral_driver=debug")
        .with_writer(std::io::stderr)
        .init();

    let args: Vec<String> = std::env::args().collect();
    let use_ember = args.iter().any(|a| a == "--ember");
    let ce_pos = args.iter().position(|a| a == "--ce");
    let bdf = args
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(i, a)| !a.starts_with('-') && ce_pos.map_or(true, |cp| *i != cp + 1))
        .map(|(_, a)| a.clone())
        .next()
        .unwrap_or_else(|| {
            eprintln!("Usage: bench_mmu_fault_diagnostic [--ember] [--vram] [--ce <RL>] <BDF>");
            eprintln!("Example: bench_mmu_fault_diagnostic --vram --ce 2 0000:06:00.0");
            std::process::exit(1);
        });

    let mode = if use_ember { "ember" } else { "direct" };
    eprintln!("═══════════════════════════════════════════════════════════════");
    eprintln!("  MMU Fault Diagnostic — {bdf} (mode: {mode})");
    eprintln!("═══════════════════════════════════════════════════════════════");

    if use_ember {
        eprintln!("\n▶ Phase 1: Connect to ember for BAR0 access");
        let session = match EmberSession::connect(&bdf) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("  ✗ Failed to connect to ember: {e}");
                std::process::exit(1);
            }
        };
        eprintln!("  ✓ Ember BAR0 access established");
        run_bar0_diagnostics(&session.bar0, &bdf);
        eprintln!("\n  (channel creation / NOP dispatch skipped in ember mode)");
        return;
    }

    eprintln!("\n▶ Phase 1: Open VFIO device");
    let raw = match RawVfioDevice::open(&bdf) {
        Ok(dev) => dev,
        Err(e) => {
            eprintln!("  ✗ Failed to open VFIO device: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("  ✓ VFIO device opened");

    if let Err(e) = raw.enable_bus_master() {
        eprintln!("  ⚠ Bus master enable failed: {e}");
    }

    let boot0 = raw.bar0.read_u32(0x0000_0000).unwrap_or(0xDEAD);
    eprintln!("  BOOT0 = {boot0:#010x}");

    // ── Phase 1.5: Firmware Boundary Probe ──────────────────────────────
    let clear_pbdma = args.iter().any(|a| a == "--clear-pbdma");
    {
        eprintln!("\n▶ Phase 1.5: Firmware Boundary Probe");
        let probe = FalconProbe::discover(&raw.bar0);
        eprintln!("{probe}");

        if !probe.dispatch_viable() {
            eprintln!("\n  ⚠ Dispatch NOT viable. Blockers:");
            for b in probe.dispatch_blockers() {
                eprintln!("    - {b}");
            }
            eprintln!("  (Continuing diagnostic for data collection)");
        }

        let r = |reg: usize| raw.bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD);
        let pfifo_en_reg = r(0x2200);
        let pfifo_intr = r(0x2100);
        eprintln!("  PFIFO_ENABLE= {pfifo_en_reg:#010x}");
        eprintln!("  PFIFO_INTR  = {pfifo_intr:#010x}");

        let pbdma_map = r(0x2004);
        eprintln!("  PBDMA_MAP   = {pbdma_map:#010x}");
        for pid in [1_usize, 2, 3, 21] {
            let b = 0x40000 + pid * 0x2000;
            let userd = r(b + 0xD0);
            let gpbase_hi = r(b + 0x44);
            let sig = r(b + 0xC0);
            let idle = r(b + 0x04);
            eprintln!("  PBDMA{pid:2}: USERD={userd:#010x} GP_HI={gpbase_hi:#010x} SIG={sig:#010x} IDLE={idle:#010x}");
        }

        if clear_pbdma {
            eprintln!("\n  ── --clear-pbdma: Clearing stale PBDMA registers ──");
            for pid in [1_usize, 2, 3] {
                let b = 0x40000 + pid * 0x2000;
                let _ = raw.bar0.write_u32(b + 0x0D0, 0);
                let _ = raw.bar0.write_u32(b + 0x0D4, 0);
                let _ = raw.bar0.write_u32(b + 0x058, 0);
                let _ = raw.bar0.write_u32(b + 0x054, 0);
                let _ = raw.bar0.write_u32(b + 0x048, 0);
                let userd_rb = r(b + 0xD0);
                eprintln!("  PBDMA{pid}: USERD after clear = {userd_rb:#010x}");
            }
        }
    }

    eprintln!("\n▶ Phase 2: Pre-channel MMU state");
    let pre_fault = mmu_fault::read_mmu_faults(&raw.bar0);
    print_fault("pre-channel", &pre_fault);

    // Detect VRAM state to choose channel creation strategy.
    let use_vram = args.iter().any(|a| a == "--vram");
    let ce_runlist: Option<u32> = args.iter()
        .position(|a| a == "--ce")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok());
    let pmc = raw.bar0.read_u32(0x200).unwrap_or(0);
    let vram_alive = pmc != 0x40000020 && pmc != 0;
    if use_vram {
        eprintln!("  --vram mode: using VRAM-based scheduler structures");
    }
    if let Some(rl) = ce_runlist {
        eprintln!("  --ce mode: targeting runlist {rl} (copy engine, bypasses FECS)");
    }
    eprintln!("  PMC_ENABLE = {pmc:#010x} (VRAM {})", if vram_alive { "alive" } else { "dead" });

    eprintln!("\n▶ Phase 3: Create PFIFO channel");
    let channel = match if use_vram && vram_alive {
        VfioChannel::create_vram_sched_on(
            raw.container.clone(),
            &raw.bar0,
            RawVfioDevice::gpfifo_iova(),
            RawVfioDevice::gpfifo_entries(),
            RawVfioDevice::userd_iova(),
            0,
            ce_runlist,
        )
    } else {
        VfioChannel::create(
            raw.container.clone(),
            &raw.bar0,
            RawVfioDevice::gpfifo_iova(),
            RawVfioDevice::gpfifo_entries(),
            RawVfioDevice::userd_iova(),
            0,
        )
    } {
        Ok(ch) => {
            eprintln!("  ✓ Channel created (id={})", ch.id());
            ch
        }
        Err(e) => {
            eprintln!("  ✗ Channel creation failed: {e}");
            eprintln!("\n▶ Post-failure MMU state:");
            let fail_fault = mmu_fault::read_mmu_faults(&raw.bar0);
            print_fault("post-channel-fail", &fail_fault);
            raw.leak();
            std::process::exit(1);
        }
    };

    // Extra diagnostics: PFIFO/PBDMA/BAR2 state right after channel creation
    eprintln!("\n▶ Phase 3b: PFIFO state after channel creation");
    let pfifo_en = raw.bar0.read_u32(0x2200).unwrap_or(0xDEAD);
    let pfifo_sched = raw.bar0.read_u32(0x2204).unwrap_or(0xDEAD);
    let bar2_block = raw.bar0.read_u32(0x1714).unwrap_or(0xDEAD);
    let pccsr_inst = raw.bar0.read_u32(0x800000).unwrap_or(0xDEAD);
    let pccsr_chan = raw.bar0.read_u32(0x800004).unwrap_or(0xDEAD);
    let pmc_enable = raw.bar0.read_u32(0x200).unwrap_or(0xDEAD);
    let priv_ring = raw.bar0.read_u32(0x12070).unwrap_or(0xDEAD);
    eprintln!("  PMC_ENABLE  = {pmc_enable:#010x}");
    eprintln!("  PFIFO_EN    = {pfifo_en:#010x}");
    eprintln!("  PFIFO_SCHED = {pfifo_sched:#010x}");
    eprintln!("  BAR2_BLOCK  = {bar2_block:#010x}");
    eprintln!("  PCCSR_INST  = {pccsr_inst:#010x}");
    eprintln!("  PCCSR_CHAN  = {pccsr_chan:#010x}");
    eprintln!("  PRIV_RING   = {priv_ring:#010x}");

    // Check runlist state
    for rl in 0..4u32 {
        let base_r = raw
            .bar0
            .read_u32(0x2270 + (rl as usize) * 0x10)
            .unwrap_or(0);
        let sub_r = raw
            .bar0
            .read_u32(0x2274 + (rl as usize) * 0x10)
            .unwrap_or(0);
        if base_r != 0 || sub_r != 0 {
            eprintln!("  RUNLIST{rl}: BASE={base_r:#010x} SUBMIT={sub_r:#010x}");
        }
    }

    eprintln!("\n▶ Phase 4: Post-channel MMU state");
    let post_ch_fault = mmu_fault::read_mmu_faults(&raw.bar0);
    print_fault("post-channel", &post_ch_fault);

    eprintln!("\n▶ Phase 5: Submit NOP GPFIFO entry");
    // Write a NOP GPFIFO entry (zero = NOP) to slot 0.
    let ring_slice = raw.gpfifo_ring.as_slice();
    let ring_ptr = ring_slice.as_ptr() as *mut u64;
    // SAFETY: gpfifo_ring DMA buffer is valid; writing slot 0.
    unsafe { std::ptr::write_volatile(ring_ptr, 0u64) };

    // Write GP_PUT=1 to USERD at Volta RAMUSERD offset 0x8C.
    let userd_slice = raw.userd.as_slice();
    let userd_ptr = userd_slice.as_ptr();
    // SAFETY: userd DMA buffer is valid 4096-byte page; 0x8C within bounds.
    unsafe {
        let gp_put_ptr = userd_ptr.add(0x8C) as *mut u32;
        std::ptr::write_volatile(gp_put_ptr, 1);
    }

    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

    if let Err(e) = raw
        .bar0
        .write_u32(VfioChannel::doorbell_offset(), channel.id())
    {
        eprintln!("  ✗ Doorbell write failed: {e}");
    } else {
        eprintln!("  ✓ Doorbell written (channel_id={})", channel.id());
    }

    std::thread::sleep(std::time::Duration::from_millis(500));

    eprintln!("\n▶ Phase 6: Post-NOP MMU state");
    let post_nop_fault = mmu_fault::read_mmu_faults(&raw.bar0);
    print_fault("post-nop", &post_nop_fault);

    // Read GP_GET from USERD at Volta RAMUSERD offset 0x88.
    // SAFETY: `userd_ptr` points at the channel USERD DMA buffer (4096 bytes); offset 0x88
    // plus 4-byte read fits in that page. The mapping remains valid for this read.
    let mut gp_get = unsafe {
        let gp_get_ptr = userd_ptr.add(0x88) as *const u32;
        std::ptr::read_volatile(gp_get_ptr)
    };
    eprintln!("  USERD GP_GET = {gp_get} (expected: 1 if consumed)");

    // Map runlist to PBDMA (GV100 topology from PBDMA_RL_SEQ[0-3]):
    // PBDMA1,2→RL1(GR), PBDMA3→RL2(CE), PBDMA21→RL4(CE)
    let pbdma_for_rl = |rl: u32| -> usize {
        match rl { 1 => 1, 2 => 3, 4 => 21, _ => 1 }
    };
    let target_rl = ce_runlist.unwrap_or(1);
    let target_pbdma = pbdma_for_rl(target_rl);
    let pb = 0x40000 + target_pbdma * 0x2000;
    let r = |reg: usize| raw.bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD);

    eprintln!("\n▶ Phase 7: PBDMA{target_pbdma} state (post-doorbell, RL{target_rl})");

    let gpu_pci_cmd = r(0x88004);
    eprintln!("  GPU PCI CMD  = {gpu_pci_cmd:#010x} (bus_master={})", gpu_pci_cmd & 4 != 0);

    eprintln!("  PBDMA{target_pbdma} GP_BASE_LO = {:#010x}", r(pb + 0x040));
    eprintln!("  PBDMA{target_pbdma} GP_BASE_HI = {:#010x}", r(pb + 0x044));
    eprintln!("  PBDMA{target_pbdma} GP_FETCH   = {:#010x}", r(pb + 0x048));
    eprintln!("  PBDMA{target_pbdma} GP_STATE   = {:#010x}", r(pb + 0x04C));
    eprintln!("  PBDMA{target_pbdma} GP_PUT     = {:#010x}", r(pb + 0x054));
    eprintln!("  PBDMA{target_pbdma} GP_GET     = {:#010x}", r(pb + 0x058));
    eprintln!("  PBDMA{target_pbdma} USERD_LO   = {:#010x}", r(pb + 0x0D0));
    eprintln!("  PBDMA{target_pbdma} USERD_HI   = {:#010x}", r(pb + 0x0D4));
    eprintln!("  PBDMA{target_pbdma} SIG        = {:#010x}", r(pb + 0x0C0));
    eprintln!("  PBDMA{target_pbdma} CONFIG     = {:#010x}", r(pb + 0x0A8));
    eprintln!("  PBDMA{target_pbdma} CH_INFO    = {:#010x}", r(pb + 0x0AC));
    eprintln!("  PBDMA{target_pbdma} CH_STATE   = {:#010x}", r(pb + 0x0B0));
    eprintln!("  PBDMA{target_pbdma} INTR       = {:#010x}", r(pb + 0x108));
    eprintln!("  PBDMA{target_pbdma} IDLE       = {:#010x}", r(pb + 0x004));
    eprintln!("  PBDMA{target_pbdma} CTX USERD  = {:#010x}", r(pb + 0x008));
    eprintln!("  PBDMA{target_pbdma} CTX 0x0F4  = {:#010x}", r(pb + 0x0F4));
    eprintln!("  PBDMA{target_pbdma} CTX 0x0F8  = {:#010x}", r(pb + 0x0F8));

    let pfifo_intr = r(0x2100);
    let priv_ring = r(0x0001_2070);
    eprintln!("  PFIFO_INTR = {pfifo_intr:#010x}");
    eprintln!("  PRIV_RING  = {priv_ring:#010x}");

    // Phase 8: Direct PBDMA programming — bypass the HOST scheduler.
    if gp_get == 0 {
        eprintln!("\n▶ Phase 8: Direct PBDMA programming (bypass scheduler)");
        let pbdma_id = target_pbdma;
        let base = 0x40000 + pbdma_id * 0x2000;

        let gpfifo_iova: u64 = 0x1000;
        let userd_iova: u64 = 0x2000;
        let limit2 = 9_u32;
        const TARGET_SYS_MEM_COH: u32 = 2;

        // GP_BASE with TARGET=SYS_MEM_COH + VALID. Without TARGET bits,
        // PBDMA fetches from VRAM 0x1000 instead of system memory IOVA 0x1000.
        let _ = raw.bar0.write_u32(base + 0x040, gpfifo_iova as u32);
        let gp_base_hi = (gpfifo_iova >> 32) as u32
            | (limit2 << 16)
            | (TARGET_SYS_MEM_COH << 8) // aperture = SYS_MEM_COH
            | (1 << 2);                 // VALID
        let _ = raw.bar0.write_u32(base + 0x044, gp_base_hi);
        // USERD with TARGET=SYS_MEM_COH
        let _ = raw.bar0.write_u32(
            base + 0x0D0,
            (userd_iova as u32 & 0xFFFF_FE00) | TARGET_SYS_MEM_COH,
        );
        let _ = raw.bar0.write_u32(base + 0x0D4, (userd_iova >> 32) as u32);
        let _ = raw.bar0.write_u32(base + 0x0C0, 0x0000_FACE); // SIG
        let _ = raw.bar0.write_u32(base + 0x0AC, 0x1000_3080); // CH_INFO
        let _ = raw.bar0.write_u32(base + 0x0A8, 0x0000_1100); // CONFIG
        let _ = raw.bar0.write_u32(base + 0x048, 0); // GP_FETCH
        let _ = raw.bar0.write_u32(base + 0x04C, 0); // GP_STATE
        let _ = raw.bar0.write_u32(base + 0x054, 1); // GP_PUT

        // Bind instance block + enable channel + doorbell.
        let _ = raw.bar0.write_u32(0x800000 + channel.id() as usize * 8,
            0x80000030); // PCCSR_INST: VRAM 0x30000, BIND=TRUE
        let _ = raw.bar0.write_u32(0x800000 + channel.id() as usize * 8 + 4,
            (1 << 10) | 0x2); // CHANNEL_ENABLE_SET
        std::thread::sleep(std::time::Duration::from_millis(20));
        let _ = raw.bar0.write_u32(0x81_0090, channel.id()); // doorbell

        std::thread::sleep(std::time::Duration::from_millis(100));

        let direct_gp_get = raw.bar0.read_u32(base + 0x058).unwrap_or(0xDEAD);
        let direct_gp_put = raw.bar0.read_u32(base + 0x054).unwrap_or(0xDEAD);
        let direct_gp_fetch = raw.bar0.read_u32(base + 0x048).unwrap_or(0xDEAD);
        let direct_intr = raw.bar0.read_u32(base + 0x108).unwrap_or(0xDEAD);
        let direct_state = raw.bar0.read_u32(base + 0x0B0).unwrap_or(0xDEAD);
        let direct_sig = raw.bar0.read_u32(base + 0x0C0).unwrap_or(0xDEAD);
        // SAFETY: Same USERD DMA page as above; 0x88+4 is within the 4096-byte buffer.
        let userd_gp_get_direct = unsafe {
            let gp_get_ptr = userd_ptr.add(0x88) as *const u32;
            std::ptr::read_volatile(gp_get_ptr)
        };

        eprintln!(
            "  PBDMA{pbdma_id} (direct): GP_GET={direct_gp_get} GP_PUT={direct_gp_put} GP_FETCH={direct_gp_fetch:#010x}"
        );
        eprintln!(
            "  PBDMA{pbdma_id} (direct): INTR={direct_intr:#010x} STATE={direct_state:#010x} SIG={direct_sig:#010x}"
        );
        eprintln!("  USERD GP_GET (from DMA buf) = {userd_gp_get_direct}");

        let post_direct_fault = mmu_fault::read_mmu_faults(&raw.bar0);
        if post_direct_fault.has_fault {
            eprintln!("  ⚠ MMU fault after direct PBDMA:");
            print_fault("direct-pbdma", &post_direct_fault);
        } else {
            eprintln!("  (no MMU fault)");
        }

        gp_get = userd_gp_get_direct;
    }

    eprintln!("\n═══════════════════════════════════════════════════════════════");
    if gp_get >= 1 {
        eprintln!("  RESULT: NOP consumed! GPFIFO dispatch succeeded.");
    } else {
        let post_final = mmu_fault::read_mmu_faults(&raw.bar0);
        if post_final.has_fault {
            eprintln!("  RESULT: MMU fault detected — see decoded fault above");
            eprintln!("  Fault type: {}", post_final.fault_type);
            eprintln!("  Faulting VA: {:#018x}", post_final.fault_va);
            eprintln!("  Engine: {}", post_final.engine);
        } else {
            eprintln!("  RESULT: No fault but GP_GET=0 — PBDMA did not fetch.");
        }
    }
    eprintln!("═══════════════════════════════════════════════════════════════");

    std::mem::forget(channel);
    raw.leak();
}

/// BAR0-only diagnostics usable in both direct and ember modes.
fn run_bar0_diagnostics(bar0: &MappedBar, bdf: &str) {
    let boot0 = bar0.read_u32(0x0000_0000).unwrap_or(0xDEAD);
    eprintln!("  BOOT0 = {boot0:#010x}");

    eprintln!("\n▶ MMU fault state");
    let fault = mmu_fault::read_mmu_faults(bar0);
    print_fault("current", &fault);

    eprintln!("\n▶ PFIFO / PBDMA / PMC state");
    let pfifo_en = bar0.read_u32(0x2200).unwrap_or(0xDEAD);
    let pfifo_sched = bar0.read_u32(0x2204).unwrap_or(0xDEAD);
    let bar2_block = bar0.read_u32(0x1714).unwrap_or(0xDEAD);
    let pccsr_inst = bar0.read_u32(0x800000).unwrap_or(0xDEAD);
    let pccsr_chan = bar0.read_u32(0x800004).unwrap_or(0xDEAD);
    let pmc_enable = bar0.read_u32(0x200).unwrap_or(0xDEAD);
    let priv_ring = bar0.read_u32(0x12070).unwrap_or(0xDEAD);
    let pfifo_intr = bar0.read_u32(0x2100).unwrap_or(0xDEAD);
    eprintln!("  PMC_ENABLE  = {pmc_enable:#010x}");
    eprintln!("  PFIFO_EN    = {pfifo_en:#010x}");
    eprintln!("  PFIFO_SCHED = {pfifo_sched:#010x}");
    eprintln!("  PFIFO_INTR  = {pfifo_intr:#010x}");
    eprintln!("  BAR2_BLOCK  = {bar2_block:#010x}");
    eprintln!("  PCCSR_INST  = {pccsr_inst:#010x}");
    eprintln!("  PCCSR_CHAN  = {pccsr_chan:#010x}");
    eprintln!("  PRIV_RING   = {priv_ring:#010x}");

    for rl in 0..4u32 {
        let base_r = bar0.read_u32(0x2270 + (rl as usize) * 0x10).unwrap_or(0);
        let sub_r = bar0.read_u32(0x2274 + (rl as usize) * 0x10).unwrap_or(0);
        if base_r != 0 || sub_r != 0 {
            eprintln!("  RUNLIST{rl}: BASE={base_r:#010x} SUBMIT={sub_r:#010x}");
        }
    }

    for pbdma_id in 0..4_usize {
        let base = 0x40000 + pbdma_id * 0x2000;
        let intr = bar0.read_u32(base + 0x108).unwrap_or(0xDEAD);
        let state = bar0.read_u32(base + 0xB0).unwrap_or(0xDEAD);
        let gp_fetch = bar0.read_u32(base + 0x48).unwrap_or(0xDEAD);
        if intr != 0 || state != 0 || gp_fetch != 0 {
            let gp_put = bar0.read_u32(base + 0x54).unwrap_or(0xDEAD);
            let userd_lo = bar0.read_u32(base + 0xD0).unwrap_or(0xDEAD);
            let gpbase = bar0.read_u32(base + 0x40).unwrap_or(0xDEAD);
            let sig = bar0.read_u32(base + 0xC0).unwrap_or(0xDEAD);
            eprintln!(
                "  PBDMA{pbdma_id}: INTR={intr:#010x} STATE={state:#010x} GP_FETCH={gp_fetch} GP_PUT={gp_put} USERD={userd_lo:#010x} GP_BASE={gpbase:#010x} SIG={sig:#010x}"
            );
        }
    }

    eprintln!("\n═══════════════════════════════════════════════════════════════");
    if fault.has_fault {
        eprintln!(
            "  ⚠ Active MMU fault: type={} engine={}",
            fault.fault_type, fault.engine
        );
    } else {
        eprintln!("  ✓ No active MMU faults on {bdf}");
    }
    eprintln!("═══════════════════════════════════════════════════════════════");
}

fn print_fault(label: &str, info: &mmu_fault::MmuFaultInfo) {
    eprintln!("  [{label}] fault_status  = {:#010x}", info.fault_status);
    eprintln!("  [{label}] fault_va      = {:#018x}", info.fault_va);
    eprintln!(
        "  [{label}] fault_inst    = {:#010x}_{:#010x}",
        info.fault_inst_hi, info.fault_inst_lo
    );
    eprintln!("  [{label}] mmu_ctrl      = {:#010x}", info.mmu_ctrl);
    eprintln!("  [{label}] hubtlb_err    = {:#010x}", info.hubtlb_err);
    eprintln!(
        "  [{label}] fault_buf0    = GET={} PUT={}",
        info.fault_buf0_get, info.fault_buf0_put
    );
    if info.has_fault {
        eprintln!(
            "  [{label}] ⚠ FAULT: type={} access={} engine={} aperture={}",
            info.fault_type, info.access_type, info.engine, info.aperture
        );
    } else {
        eprintln!("  [{label}] (no fault)");
    }
}
