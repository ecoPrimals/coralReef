// SPDX-License-Identifier: AGPL-3.0-or-later
//! GR MMIO / PMC / traps precursor before FECS PIO upload (cold Kepler sovereign boot path).

use super::super::hardware_guard::GuardedBar;

pub(super) fn run_gr_boot_precursor(
    guard: &GuardedBar<'_>,
    cached_gpc_count: u32,
    cached_tpc_total: u32,
    cached_tpc_counts: &[(u32, u32); 8],
) {
    use crate::nv::kepler_falcon;

    const FECS: u32 = kepler_falcon::FECS_BASE;
    const GPCCS: u32 = kepler_falcon::GPCCS_BASE;

    let r = guard.read_fn();
    let w = |reg: u32, val: u32| {
        if let Err(refusal) = guard.write_u32(reg, val) {
            tracing::error!(%refusal, "kepler_load_and_boot_fecs: guard refused write");
        }
    };
    super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);

    // Step 0: PMC GR engine reset + PGOB ungate in a single tight sequence.
    //
    // On GK210B, the GR HUB auto-clock-gates within nanoseconds of the last
    // PRI access. The PGOB disable sequence accesses GR registers for ~200ms,
    // keeping it alive. We chain the PMC reset, PGOB ungate, and CG disable
    // into a single uninterrupted burst so the domain stays accessible.
    {
        let pmc_pre = r(0x200);
        const GR_BIT: u32 = 1 << 12;
        w(0x200, pmc_pre & !GR_BIT);
        let _ = r(0x200);
        w(0x200, pmc_pre | GR_BIT);
        // PGOB ungate runs ~200ms of PRI writes, keeping GR HUB alive.
        // After it returns, immediately slam CG-disable writes — no logging.
        super::super::pgob::gk110_pgob_disable(guard);
        // IMMEDIATE: disable BLCG/SLCG before auto-gating kicks in
        w(0x40_41f0, 0x0000_0000); // BLCG off
        w(0x40_41f4, 0x0000_0000); // SLCG off
        w(0x40_9890, 0x0000_0000); // FECS BLCG off
        w(0x40_98b0, 0x0000_0000); // FECS BLCG2 off
        w(0x40_0500, 0x0000_0000); // TRAP_EN off (keep GR HUB warm)
        super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        let gr_hub = r(0x40_0000);
        let trap_rb = r(0x40_0500);
        tracing::info!(
            pmc_pre = format_args!("{pmc_pre:#010x}"),
            gr_hub = format_args!("{gr_hub:#010x}"),
            trap_rb = format_args!("{trap_rb:#010x}"),
            ok = gr_hub != 0xDEAD_DEAD && gr_hub & 0xBAD0_0000 != 0xBAD0_0000,
            "Step 0: PMC reset + PGOB + CG-disable burst"
        );
    }

    // Step 2: GPC MMU init (gf100_gr_init_gpc_mmu) — per-GPC.
    //
    // Broadcast 0x418xxx writes are silently dropped on GK210B.
    {
        let fb_mmu = r(0x10_0C80) & 0x0000_0001;
        for gpc in 0..8u32 {
            let base = 0x50_0000 + gpc * 0x8000;
            let probe = r(base + 0x2100);
            if probe == 0xDEAD_DEAD || probe & 0xBAD0_0000 == 0xBAD0_0000 {
                continue;
            }
            w(base + 0x0880, fb_mmu);
            w(base + 0x0890, 0x0000_0000);
            w(base + 0x0894, 0x0000_0000);
        }
    }

    // Step 3a: MMIO init table (gk110_gr_pack_mmio — hardcoded baseline).
    {
        let (gr_applied, gr_faulted) = super::super::kepler_gr_init::apply_gk110_gr_init(guard);
        super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        tracing::info!(
            gr_applied,
            gr_faulted,
            "Step 3a: GR MMIO init (hardcoded gk110 pack)"
        );
    }

    // Step 3b: Apply sw_nonctx.bin — GK210B-specific register overrides.
    {
        let (nonctx_applied, nonctx_skipped) = super::super::pri::apply_sw_nonctx(guard, "gk210");
        super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        tracing::info!(
            nonctx_applied,
            nonctx_skipped,
            "Step 3b: sw_nonctx.bin GK210B overrides"
        );
    }

    // gf100_gr_wait_idle — Nouveau waits for GR idle after MMIO init.
    {
        let mut gr_idle = false;
        for _ in 0..2000 {
            let status = r(0x40_0700);
            if status != 0xDEAD_DEAD && status & 0x1 == 0 {
                gr_idle = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        tracing::info!(
            gr_idle,
            "gf100_gr_wait_idle after MMIO init (0x400700 bit 0)"
        );
    }

    // Step 3c: Clock gating init (gk110_clkgate_pack — BLCG + SLCG).
    {
        let (cg_applied, cg_faulted) = super::super::kepler_gr_init::apply_gk110_clkgate(guard);
        super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        tracing::info!(cg_applied, cg_faulted, "Step 3c: GK110 clock gating init");
    }

    // Step 3d: PGOB disable — ensures GR power domain is ungated.
    // Deferred to after MMIO + CG init because the PMC GR reset in Step 0
    // only briefly makes the GR HUB accessible; auto-clock-gating would
    // re-gate it if we ran PGOB first (which takes 200ms+).
    {
        let gr_hub_pre = r(0x40_0000);
        let gr_hub_ok = gr_hub_pre != 0xDEAD_DEAD && gr_hub_pre & 0xBAD0_0000 != 0xBAD0_0000;
        if !gr_hub_ok {
            tracing::info!(
                gr_hub = format_args!("{gr_hub_pre:#010x}"),
                "GR HUB still gated after MMIO init — running PGOB disable"
            );
            super::super::pgob::gk110_pgob_disable(guard);
            super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        }
        let gr_hub_post = r(0x40_0000);
        tracing::info!(
            gr_hub = format_args!("{gr_hub_post:#010x}"),
            ok = gr_hub_post != 0xDEAD_DEAD && gr_hub_post & 0xBAD0_0000 != 0xBAD0_0000,
            "Step 3d: GR HUB state after CG init + PGOB"
        );
    }

    // Step 4: Trap re-enable + interrupt clearing (matches nouveau ordering).
    w(0x40_0500, 0x0001_0001); // re-enable traps
    w(0x40_0100, 0xFFFF_FFFF); // GR_INTR: write-1-to-clear all
    w(0x40_013c, 0xFFFF_FFFF); // GR_INTR_NONSTALL: clear
    w(0x40_0124, 0x0000_0002); // INTR_NOTIFY_EN

    // Step 5: Exception handling + HWW ESR (matches gf100_gr_init).
    w(0x40_4000, 0xc000_0000); // PD
    w(0x40_4600, 0xc000_0000); // PD
    w(0x40_8030, 0xc000_0000); // BE
    w(0x40_6018, 0xc000_0000); // DS
    w(0x40_4490, 0xc000_0000); // PRI
    w(0x40_5840, 0xc000_0000); // DS_DEBUG
    w(0x40_5844, 0x00ff_ffff); // DS_DEBUG

    // Interrupt clear + exception2 (nouveau: gf100_gr_init_exception2).
    w(0x40_0108, 0xFFFF_FFFF); // GR_TRAP_NONSTALL
    w(0x40_0138, 0xFFFF_FFFF); // GR_EXCEPTION
    w(0x40_0118, 0xFFFF_FFFF);
    w(0x40_0130, 0xFFFF_FFFF);
    w(0x40_011c, 0xFFFF_FFFF); // EXCEPTION2
    w(0x40_0134, 0xFFFF_FFFF); // EXCEPTION2

    // Step 6: GR_UNITS (0x400054) — written LATE like nouveau's
    // gf100_gr_init_400054.  Nouveau writes a fixed 0x34ce3464;
    // 0x400054 may be read-only fuse mirror — write it but don't
    // depend on the readback.  Use cached topology (read before
    // the PMC reset cleared fuse mirrors at 0x502608).
    {
        let mut gr_units: u32 = 0;
        for &(gpc, tpc_nr) in cached_tpc_counts.iter().take(cached_gpc_count as usize) {
            gr_units |= (tpc_nr & 0xF) << (gpc * 4);
        }
        w(0x40_0054, gr_units);
        let readback = r(0x40_0054);
        tracing::info!(
            gr_units = format_args!("{gr_units:#010x}"),
            readback = format_args!("{readback:#010x}"),
            cached_gpcs = cached_gpc_count,
            cached_tpcs = cached_tpc_total,
            "Step 6: GR_UNITS (0x400054 — may be RO fuse)"
        );
    }

    // Log state after GR init.
    {
        super::super::pri::clear_pri_ring_faults(guard.inner(), &r, &w);
        let fecs_cpuctl = r(FECS + 0x100);
        let gpccs_cpuctl = r(GPCCS + 0x100);
        let gr_hub = r(0x40_0700);
        let gr_units = r(0x40_0054);
        let is_ok = |v: u32| v != 0xDEAD_DEAD && v & 0xBAD0_0000 != 0xBAD0_0000;
        tracing::info!(
            fecs_cpuctl = format_args!("{fecs_cpuctl:#010x}"),
            gpccs_cpuctl = format_args!("{gpccs_cpuctl:#010x}"),
            gr_hub = format_args!(
                "{gr_hub:#010x}[{}]",
                if is_ok(gr_hub) { "OK" } else { "FAULT" }
            ),
            gr_units = format_args!("{gr_units:#010x}"),
            "Pre-upload state"
        );
    }

    // Immediate GR HUB recheck — is it still accessible right here?
    {
        let gh = r(0x40_0000);
        let fecs_r = r(FECS + 0x100);
        tracing::info!(
            gr_hub = format_args!("{gh:#010x}"),
            fecs_cpuctl = format_args!("{fecs_r:#010x}"),
            ok = gh != 0xDEAD_DEAD && gh & 0xBAD0_0000 != 0xBAD0_0000,
            "GR HUB IMMEDIATE recheck (no writes between this and Pre-upload state)"
        );
    }

    // MC_UNK260 (0x260) — GR method dispatch control.
    // Nouveau brackets firmware upload with unk260=0 (disable) / unk260=1 (enable).
    // Earlier we skipped this because 0x400000 returned 0xbadf1002 with unk260=0,
    // but that turned out to be "GR not initialized" status, not "GR gated."
    // FECS registers at 0x409xxx remain accessible with unk260=0, and PIO
    // uploads work correctly.  Restore the Nouveau bracket.
    w(0x260, 0);
    tracing::info!("MC_UNK260=0 (GR method dispatch disabled for firmware upload)");

    // Deep-dive diagnostics: read ENGCTL (0x058), SCTL (0x240), and
    // other undocumented control registers on GPCCS to understand
    // why STARTCPU is refused.
    {
        let bar0 = guard.inner();
        let gpc0 = 0x50_2000u32;
        let rd = |off: u32| -> u32 { bar0.read_u32(off as usize).unwrap_or(0xDEAD_DEAD) };

        let engctl_058 = rd(gpc0 + 0x058);
        let sctl_240 = rd(gpc0 + 0x240);
        let cpuctl = rd(gpc0 + 0x100);
        let dmactl = rd(gpc0 + 0x10C);
        let itfen = rd(gpc0 + 0x048);
        let hwcfg = rd(gpc0 + 0x108);
        let hwcfg2 = rd(gpc0 + 0x00C);
        let irqstat = rd(gpc0 + 0x008);
        let falcon_ver = rd(gpc0 + 0x004);
        let exci = rd(gpc0 + 0x04C);
        let unk_3c0 = rd(gpc0 + 0x3C0);
        let bootvec = rd(gpc0 + 0x104);
        let debugi = rd(gpc0 + 0x0A8);

        // Also read FECS for comparison
        let f_engctl = rd(FECS + 0x058);
        let f_sctl = rd(FECS + 0x240);
        let f_cpuctl = rd(FECS + 0x100);

        tracing::info!(
            engctl_058 = format_args!("{engctl_058:#010x}"),
            sctl_240 = format_args!("{sctl_240:#010x}"),
            cpuctl = format_args!("{cpuctl:#010x}"),
            dmactl = format_args!("{dmactl:#010x}"),
            itfen = format_args!("{itfen:#010x}"),
            hwcfg = format_args!("{hwcfg:#010x}"),
            hwcfg2 = format_args!("{hwcfg2:#010x}"),
            irqstat = format_args!("{irqstat:#010x}"),
            falcon_ver = format_args!("{falcon_ver:#010x}"),
            "GPC0 GPCCS deep-dive (pre-upload)"
        );
        tracing::info!(
            exci = format_args!("{exci:#010x}"),
            unk_3c0 = format_args!("{unk_3c0:#010x}"),
            bootvec = format_args!("{bootvec:#010x}"),
            debugi = format_args!("{debugi:#010x}"),
            f_engctl = format_args!("{f_engctl:#010x}"),
            f_sctl = format_args!("{f_sctl:#010x}"),
            f_cpuctl = format_args!("{f_cpuctl:#010x}"),
            "GPCCS vs FECS control state (pre-upload)"
        );
    }

    // Re-scan TPC topology in case GPCs became accessible after PRI re-enum.
    let (live_gpc_count, live_tpc_total, live_tpc_counts) =
        super::super::pri::scan_gpc_topology(guard);
    let (use_gpc_count, use_tpc_total, _use_tpc_counts) = if live_tpc_total > 0 {
        (live_gpc_count, live_tpc_total, live_tpc_counts)
    } else {
        (cached_gpc_count, cached_tpc_total, *cached_tpc_counts)
    };
    tracing::info!(
        live_gpcs = live_gpc_count,
        live_tpcs = live_tpc_total,
        use_gpcs = use_gpc_count,
        use_tpcs = use_tpc_total,
        "Topology scan (live vs cached fallback)"
    );

    // Diagnose GPCCS Falcon state before upload.
    {
        let gpc0_gpccs_dmactl = r(0x50_2000 + 0x10C);
        let gpc0_gpccs_cpuctl = r(0x50_2000 + 0x100);
        tracing::info!(
            gpc0_dmactl = format_args!("{gpc0_gpccs_dmactl:#010x}"),
            gpc0_cpuctl = format_args!("{gpc0_gpccs_cpuctl:#010x}"),
            "Pre-upload GPCCS state (post ENGCTL HRESET)"
        );
    }

    // GR HUB check before firmware upload
    {
        let gh = r(0x40_0000);
        tracing::info!(
            gr_hub = format_args!("{gh:#010x}"),
            ok = gh != 0xDEAD_DEAD && gh & 0xBAD0_0000 != 0xBAD0_0000,
            "GR HUB check BEFORE firmware upload"
        );
    }
}
