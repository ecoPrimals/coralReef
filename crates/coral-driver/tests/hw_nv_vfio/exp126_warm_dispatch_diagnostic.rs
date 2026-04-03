// SPDX-License-Identifier: AGPL-3.0-only
//! Exp 126: Warm handoff dispatch diagnostic.
//!
//! Structured diagnostic test that captures BAR2, PCCSR, fault buffer,
//! and PBDMA state before and after warm handoff to pinpoint the fence
//! timeout root cause from Exp 125.
//!
//! The test orchestrates the full warm handoff flow through glowplug/ember:
//! - glowplug swaps device to nouveau → FECS boots → swaps back to vfio-pci
//! - ember provides VFIO fds via SCM_RIGHTS (no sudo needed)
//! - test opens device in warm mode and runs diagnostics
//!
//! ```sh
//! RUST_LOG=debug CORALREEF_VFIO_BDF=0000:xx:xx.x \
//!   cargo test --test hw_nv_vfio --features vfio \
//!   -- --ignored exp126 --test-threads=1 --nocapture
//! ```

use coral_driver::nv::NvVfioComputeDevice;
use coral_driver::vfio::channel::registers::{falcon, misc, mmu, pccsr};
use coral_driver::vfio::device::MappedBar;

/// Snapshot of warm-handoff-relevant registers at a point in time.
struct WarmDiagnosticSnapshot {
    label: &'static str,
    bar2_block: u32,
    pmc_enable: u32,
    pfifo_sched_en: u32,
    fault_buf0_lo: u32,
    fault_buf0_get: u32,
    fault_buf0_put: u32,
    fault_buf1_lo: u32,
    fault_buf1_get: u32,
    fault_buf1_put: u32,
    fault_status: u32,
    fault_addr_lo: u32,
    fault_addr_hi: u32,
    fecs_cpuctl: u32,
    fecs_pc: u32,
    fecs_mailbox0: u32,
    fecs_mailbox1: u32,
    fecs_mthd_status: u32,
    fecs_mthd_status2: u32,
    fecs_exci: u32,
    fecs_irqstat: u32,
    gpccs_cpuctl: u32,
    gpccs_pc: u32,
    gpccs_mailbox0: u32,
    pccsr_channels: [(u32, u32); 8],
}

impl WarmDiagnosticSnapshot {
    fn capture(bar0: &MappedBar, label: &'static str) -> Self {
        let r = |a: usize| bar0.read_u32(a).unwrap_or(0xDEAD_DEAD);
        let mut pccsr_channels = [(0u32, 0u32); 8];
        for ch in 0..8u32 {
            pccsr_channels[ch as usize] = (r(pccsr::inst(ch)), r(pccsr::channel(ch)));
        }
        Self {
            label,
            bar2_block: r(misc::PBUS_BAR2_BLOCK),
            pmc_enable: r(misc::PMC_ENABLE),
            pfifo_sched_en: r(misc::PFIFO_SCHED_EN),
            fault_buf0_lo: r(mmu::FAULT_BUF0_LO),
            fault_buf0_get: r(mmu::FAULT_BUF0_GET),
            fault_buf0_put: r(mmu::FAULT_BUF0_PUT),
            fault_buf1_lo: r(mmu::FAULT_BUF1_LO),
            fault_buf1_get: r(mmu::FAULT_BUF1_GET),
            fault_buf1_put: r(mmu::FAULT_BUF1_PUT),
            fault_status: r(mmu::FAULT_STATUS),
            fault_addr_lo: r(mmu::FAULT_ADDR_LO),
            fault_addr_hi: r(mmu::FAULT_ADDR_HI),
            fecs_cpuctl: r(falcon::FECS_BASE + falcon::CPUCTL),
            fecs_pc: r(falcon::FECS_BASE + falcon::PC),
            fecs_mailbox0: r(falcon::FECS_BASE + falcon::MAILBOX0),
            fecs_mailbox1: r(falcon::FECS_BASE + falcon::MAILBOX1),
            fecs_mthd_status: r(falcon::FECS_BASE + falcon::MTHD_STATUS),
            fecs_mthd_status2: r(falcon::FECS_BASE + falcon::MTHD_STATUS2),
            fecs_exci: r(falcon::FECS_BASE + falcon::EXCI),
            fecs_irqstat: r(falcon::FECS_BASE + falcon::IRQSTAT),
            gpccs_cpuctl: r(falcon::GPCCS_BASE + falcon::CPUCTL),
            gpccs_pc: r(falcon::GPCCS_BASE + falcon::PC),
            gpccs_mailbox0: r(falcon::GPCCS_BASE + falcon::MAILBOX0),
            pccsr_channels,
        }
    }

    fn print(&self) {
        eprintln!("\n╔══ {label} ══", label = self.label);
        eprintln!("║ BAR2_BLOCK  = {:#010x}", self.bar2_block);
        let bar2_target = (self.bar2_block >> 28) & 0x3;
        let bar2_mode = (self.bar2_block >> 31) & 1;
        eprintln!(
            "║   target={} mode={} (0=PHYS, 1=VIRTUAL)",
            bar2_target,
            if bar2_mode == 0 { "PHYS" } else { "VIRTUAL" }
        );
        eprintln!(
            "║ PMC_ENABLE  = {:#010x} (GR={}, PFIFO={})",
            self.pmc_enable,
            if self.pmc_enable & (1 << 12) != 0 {
                "ON"
            } else {
                "OFF"
            },
            if self.pmc_enable & (1 << 8) != 0 {
                "ON"
            } else {
                "OFF"
            },
        );
        eprintln!("║ SCHED_EN    = {:#010x}", self.pfifo_sched_en);

        eprintln!("║");
        eprintln!("║ ── MMU Fault Buffers ──");
        eprintln!(
            "║ FB0: lo={:#010x} get={:#010x} put={:#010x}",
            self.fault_buf0_lo, self.fault_buf0_get, self.fault_buf0_put
        );
        eprintln!(
            "║ FB1: lo={:#010x} get={:#010x} put={:#010x}",
            self.fault_buf1_lo, self.fault_buf1_get, self.fault_buf1_put
        );
        let fb0_overflow = self.fault_buf0_get != (self.fault_buf0_put & 0x7FFF_FFFF);
        let fault_pending = self.fault_status != 0;
        eprintln!(
            "║ FAULT_STATUS={:#010x} FAULT_ADDR={:#010x}:{:#010x}",
            self.fault_status, self.fault_addr_hi, self.fault_addr_lo
        );
        if fb0_overflow {
            eprintln!("║ ⚠ FB0 GET≠PUT — faults have been logged");
        }
        if fault_pending {
            eprintln!("║ ⚠ MMU fault pending!");
        }

        eprintln!("║");
        eprintln!("║ ── Falcon State ──");
        let fecs_halted = self.fecs_cpuctl & falcon::CPUCTL_HALTED != 0;
        let fecs_stopped = self.fecs_cpuctl & falcon::CPUCTL_STOPPED != 0;
        eprintln!(
            "║ FECS: cpuctl={:#010x} pc={:#06x} mb0={:#010x} mb1={:#010x}",
            self.fecs_cpuctl, self.fecs_pc, self.fecs_mailbox0, self.fecs_mailbox1
        );
        eprintln!(
            "║   halted={fecs_halted} stopped={fecs_stopped} exci={:#010x} irq={:#010x}",
            self.fecs_exci, self.fecs_irqstat
        );
        eprintln!(
            "║   mthd_status={:#010x} mthd_status2={:#010x}",
            self.fecs_mthd_status, self.fecs_mthd_status2
        );
        let gpccs_halted = self.gpccs_cpuctl & falcon::CPUCTL_HALTED != 0;
        eprintln!(
            "║ GPCCS: cpuctl={:#010x} pc={:#06x} mb0={:#010x} halted={gpccs_halted}",
            self.gpccs_cpuctl, self.gpccs_pc, self.gpccs_mailbox0
        );

        eprintln!("║");
        eprintln!("║ ── PCCSR Channels 0..7 ──");
        for (ch, (inst, chan)) in self.pccsr_channels.iter().enumerate() {
            if *inst == 0 && *chan == 0 {
                continue;
            }
            let en = chan & 1 != 0;
            let busy = chan & (1 << 28) != 0;
            let status = pccsr::status_name(*chan);
            let pbdma_f = chan & (1 << 22) != 0;
            let eng_f = chan & (1 << 23) != 0;
            eprintln!(
                "║ CH[{ch}]: inst={inst:#010x} chan={chan:#010x} en={en} busy={busy} {status} pbdma_f={pbdma_f} eng_f={eng_f}"
            );
        }
        eprintln!("╚══════════════════════════════════════════════════");
    }
}

/// Capture PBDMA state for the GR runlist's PBDMAs.
fn print_pbdma_state(dev: &NvVfioComputeDevice, label: &str) {
    let pbdma_ids = dev.gr_runlist_pbdma_ids();
    if pbdma_ids.is_empty() {
        eprintln!("  [{label}] No PBDMAs found for GR runlist");
        return;
    }
    let snaps = dev.pbdma_snapshot(&pbdma_ids);
    eprintln!("  [{label}] PBDMA state for GR runlist:");
    for s in &snaps {
        eprintln!("{s}");
    }
}

/// Clear stale PCCSR for ALL channels (not just channel 0).
///
/// Nouveau leaves its channels in various states. After warm handoff, these
/// stale PCCSR entries may confuse the scheduler. Disable + fault-clear each.
fn clear_all_stale_pccsr(bar0: &MappedBar) {
    let r = |a: usize| bar0.read_u32(a).unwrap_or(0);
    for ch in 0..512u32 {
        let chan_val = r(pccsr::channel(ch));
        let inst_val = r(pccsr::inst(ch));
        if chan_val == 0 && inst_val == 0 {
            continue;
        }
        // Disable
        if chan_val & 1 != 0 {
            let _ = bar0.write_u32(pccsr::channel(ch), pccsr::CHANNEL_ENABLE_CLR);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        // Clear faults
        let _ = bar0.write_u32(
            pccsr::channel(ch),
            pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET,
        );
        // Clear instance block
        let _ = bar0.write_u32(pccsr::inst(ch), 0);
    }
    eprintln!("  Cleared stale PCCSR for all 512 channels");
}

#[test]
#[ignore = "requires GPU on vfio-pci + ember + glowplug running"]
fn exp126_warm_dispatch_diagnostic() {
    crate::helpers::init_tracing();
    let bdf = crate::helpers::vfio_bdf();
    let sm = crate::helpers::vfio_sm();

    eprintln!("\n╔══════════════════════════════════════════════════════╗");
    eprintln!("║  Exp 126: Warm Handoff Dispatch Diagnostic          ║");
    eprintln!("║  BDF: {bdf:<46} ║");
    eprintln!("╚══════════════════════════════════════════════════════╝");

    // Phase 0: Orchestrate warm handoff via glowplug → ember
    // (nouveau boots FECS, livepatch freezes teardown, swap back to vfio-pci)
    eprintln!("\n── Phase 0: Warm handoff via glowplug ──");
    match crate::glowplug_client::GlowPlugClient::connect() {
        Ok(mut gp) => {
            eprintln!("  glowplug: orchestrating warm handoff for {bdf}...");
            match gp.warm_handoff(&bdf, "nouveau", 2000, true, 15000) {
                Ok(result) => {
                    let fecs_running = result
                        .get("fecs_ever_running")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let total_ms = result.get("total_ms").and_then(|v| v.as_u64()).unwrap_or(0);
                    eprintln!(
                        "  glowplug: warm handoff complete (fecs_running={fecs_running}, {total_ms}ms)"
                    );
                    if !fecs_running {
                        eprintln!(
                            "  WARNING: FECS never seen running — diagnostic may show stale state"
                        );
                    }
                }
                Err(e) => {
                    eprintln!("  glowplug: warm_handoff failed: {e}");
                    eprintln!("  Continuing — assuming warm handoff was done externally");
                }
            }
        }
        Err(e) => {
            eprintln!("  glowplug not available ({e})");
            eprintln!(
                "  Continuing — assuming warm handoff was done externally (coralctl warm-fecs)"
            );
        }
    }

    // Phase 1: Get raw VFIO fds and capture pre-channel state
    let fds = match crate::ember_client::request_fds(&bdf) {
        Ok(fds) => {
            eprintln!("ember: received VFIO fds for {bdf} (WARM DIAGNOSTIC MODE)");
            fds
        }
        Err(e) => {
            panic!("warm diagnostic requires ember for VFIO fds (ember unavailable: {e})");
        }
    };

    // Phase 2: Open warm device (includes channel creation + falcon restart)
    eprintln!("\n── Phase 2: Opening warm device ──");
    let mut dev = NvVfioComputeDevice::open_warm(&bdf, fds, sm, 0)
        .expect("open_warm should succeed (channel + falcon restart)");

    // Phase 3: Post-open diagnostic capture
    let snap_post_open = WarmDiagnosticSnapshot::capture(dev.bar0_ref(), "POST-OPEN-WARM");
    snap_post_open.print();
    print_pbdma_state(&dev, "POST-OPEN");

    // Phase 4: Layer 7 diagnostics
    let diag = dev.layer7_diagnostics("EXP126-POST-OPEN");
    eprintln!("\n{diag}");

    // Phase 5: FECS method interface probe
    eprintln!("\n── Phase 5: FECS Method Interface Probe ──");
    let fecs_probe = dev.fecs_method_probe();
    eprintln!("{fecs_probe}");

    let fecs_responsive = fecs_probe.ctx_size.is_ok();
    eprintln!(
        "FECS method interface: {}",
        if fecs_responsive {
            "RESPONSIVE"
        } else {
            "NOT RESPONDING"
        }
    );

    // Phase 6: If FECS methods failed, try recovery strategies
    if !fecs_responsive {
        eprintln!("\n── Phase 6: Recovery Strategies ──");

        // Strategy A: Clear ALL stale PCCSR entries (nouveau residue)
        eprintln!("\n[Strategy A] Clear all stale PCCSR entries...");
        clear_all_stale_pccsr(dev.bar0_ref());
        let snap_a = WarmDiagnosticSnapshot::capture(dev.bar0_ref(), "POST-PCCSR-CLEAR");
        snap_a.print();

        // Strategy B: Preempt all runlists to force FECS to process
        eprintln!("\n[Strategy B] Preempt all runlists...");
        let bar0 = dev.bar0_ref();
        let _ = bar0.write_u32(0x2100, 0xFFFF_FFFF); // clear PFIFO INTR
        let _ = bar0.write_u32(0x2638, 0xFFFF_FFFF); // GV100_PREEMPT all
        std::thread::sleep(std::time::Duration::from_millis(50));
        let intr_post = bar0.read_u32(0x2100).unwrap_or(0);
        eprintln!("  PFIFO INTR after preempt-all: {intr_post:#010x}");
        let snap_b = WarmDiagnosticSnapshot::capture(dev.bar0_ref(), "POST-PREEMPT-ALL");
        snap_b.print();

        // Strategy C: Re-probe FECS method interface after cleanup
        eprintln!("\n[Strategy C] Re-probe FECS methods after cleanup...");
        let fecs_probe_2 = dev.fecs_method_probe();
        eprintln!("{fecs_probe_2}");

        if fecs_probe_2.ctx_size.is_ok() {
            eprintln!("*** FECS responsive after PCCSR cleanup + preempt! ***");
        } else {
            // Strategy D: Full GR context setup attempt
            eprintln!("\n[Strategy D] Attempting full GR context setup...");
            match dev.setup_gr_context() {
                Ok(ctx) => {
                    eprintln!(
                        "  GR context ready: image={}B iova={:#x}",
                        ctx.image_size, ctx.iova
                    );
                }
                Err(e) => {
                    eprintln!("  GR context setup failed: {e}");
                    let snap_d =
                        WarmDiagnosticSnapshot::capture(dev.bar0_ref(), "POST-GR-CONTEXT-FAIL");
                    snap_d.print();
                }
            }
        }
    }

    // Phase 7: Attempt NOP dispatch regardless of FECS state
    eprintln!("\n── Phase 7: NOP Dispatch Attempt ──");
    let sm_ver = dev.sm_version();
    let wgsl = "@compute @workgroup_size(64) fn main() {}";
    let opts = coral_reef::CompileOptions {
        target: match sm_ver {
            70 => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm70),
            75 => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm75),
            80 => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm80),
            _ => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm86),
        },
        ..coral_reef::CompileOptions::default()
    };
    let compiled = coral_reef::compile_wgsl_full(wgsl, &opts).expect("NOP shader compile");
    let info = coral_driver::ShaderInfo {
        gpr_count: compiled.info.gpr_count,
        shared_mem_bytes: compiled.info.shared_mem_bytes,
        barrier_count: compiled.info.barrier_count,
        workgroup: compiled.info.local_size,
        wave_size: 32,
    };

    let snap_pre_dispatch = WarmDiagnosticSnapshot::capture(dev.bar0_ref(), "PRE-DISPATCH");
    snap_pre_dispatch.print();
    print_pbdma_state(&dev, "PRE-DISPATCH");

    match dev.dispatch_traced(
        &compiled.binary,
        &[],
        coral_driver::DispatchDims::linear(1),
        &info,
    ) {
        Ok(captures) => {
            eprintln!("\nTimed post-doorbell captures:");
            for cap in &captures {
                eprintln!("{cap}");
            }
        }
        Err(e) => {
            eprintln!("dispatch_traced failed: {e}");
        }
    }

    let snap_post_dispatch = WarmDiagnosticSnapshot::capture(dev.bar0_ref(), "POST-DISPATCH");
    snap_post_dispatch.print();
    print_pbdma_state(&dev, "POST-DISPATCH");

    // Phase 8: Sync (fence wait)
    eprintln!("\n── Phase 8: Sync ──");
    use coral_driver::ComputeDevice;
    match dev.sync() {
        Ok(()) => {
            eprintln!("╔══════════════════════════════════════════════════╗");
            eprintln!("║  SYNC SUCCEEDED — LAYER 7 BREAKTHROUGH!         ║");
            eprintln!("║  Warm handoff sovereign compute is WORKING.     ║");
            eprintln!("╚══════════════════════════════════════════════════╝");
        }
        Err(e) => {
            eprintln!("sync failed: {e}");
            let snap_timeout =
                WarmDiagnosticSnapshot::capture(dev.bar0_ref(), "POST-FENCE-TIMEOUT");
            snap_timeout.print();

            let diag_post = dev.layer7_diagnostics("EXP126-POST-TIMEOUT");
            eprintln!("\n{diag_post}");

            eprintln!("\n── Root Cause Analysis ──");
            analyze_timeout_root_cause(&snap_post_dispatch, &snap_timeout);
        }
    }

    eprintln!("\n=== End Exp 126 ===");
}

/// Analyze fence timeout to narrow the root cause.
fn analyze_timeout_root_cause(pre: &WarmDiagnosticSnapshot, post: &WarmDiagnosticSnapshot) {
    // Check FECS state
    let fecs_halted = post.fecs_cpuctl & falcon::CPUCTL_HALTED != 0;
    let fecs_dead = post.fecs_cpuctl == 0xDEAD_DEAD;

    if fecs_dead {
        eprintln!("  → FECS unreachable (PRI timeout). GPU may have been reset.");
    } else if fecs_halted {
        eprintln!("  → FECS still HALTED. Firmware did not wake from idle loop.");
        eprintln!("    FECS needs SWGEN0 or STARTCPU to transition to command mode.");
        if post.fecs_exci != 0 {
            eprintln!("  → FECS has exception: exci={:#010x}", post.fecs_exci);
        }
    } else {
        eprintln!(
            "  → FECS appears running (cpuctl={:#010x})",
            post.fecs_cpuctl
        );
        eprintln!(
            "    Method interface status: {:#010x}/{:#010x}",
            post.fecs_mthd_status, post.fecs_mthd_status2
        );
    }

    // Check if MMU fault occurred
    if post.fault_status != 0 {
        eprintln!(
            "  → MMU fault detected! status={:#010x} addr={:#010x}:{:#010x}",
            post.fault_status, post.fault_addr_hi, post.fault_addr_lo
        );
    }

    // Check PCCSR state change
    let (_, ch0_post) = post.pccsr_channels[0];
    let ch0_status = pccsr::status_name(ch0_post);
    let ch0_enabled = ch0_post & 1 != 0;
    let ch0_busy = ch0_post & (1 << 28) != 0;
    let ch0_pbdma_f = ch0_post & (1 << 22) != 0;
    let ch0_eng_f = ch0_post & (1 << 23) != 0;

    eprintln!("  → CH[0] status={ch0_status} en={ch0_enabled} busy={ch0_busy}");
    if ch0_pbdma_f {
        eprintln!("  → PBDMA faulted on channel 0!");
    }
    if ch0_eng_f {
        eprintln!("  → Engine faulted on channel 0!");
    }

    // Check if GP_PUT moved (dispatch was written)
    let (_, ch0_pre) = pre.pccsr_channels[0];
    if ch0_pre == ch0_post {
        eprintln!("  → PCCSR CH[0] unchanged between dispatch and timeout");
        eprintln!("    Channel may not have been scheduled by PFIFO");
    }

    // Summary
    eprintln!("\n  Hypothesis ranking:");
    if fecs_halted {
        eprintln!("  1. FECS firmware not woken — needs different wake strategy");
    }
    if post.fault_status != 0 {
        eprintln!("  2. MMU fault blocking PBDMA — check page table / fault buffer");
    }
    if ch0_pbdma_f || ch0_eng_f {
        eprintln!("  3. Channel fault — PBDMA or engine rejected our work");
    }
    if !fecs_halted && post.fecs_mthd_status == 0 && post.fecs_mthd_status2 == 0 {
        eprintln!("  4. FECS running but not processing runlist — needs ctxsw start");
    }
}

/// Targeted test: attempt warm handoff with pre-emptive stale PCCSR cleanup.
///
/// If Exp 126 shows stale PCCSR is the issue, this test validates the fix.
#[test]
#[ignore = "requires GPU on vfio-pci + ember + glowplug running"]
fn exp126_warm_with_pccsr_cleanup() {
    crate::helpers::init_tracing();
    let bdf = crate::helpers::vfio_bdf();
    let sm = crate::helpers::vfio_sm();

    eprintln!("\n=== Exp 126b: Warm Handoff with Pre-emptive PCCSR Cleanup ===");

    // Orchestrate warm handoff via glowplug (same as open_vfio_warm)
    if let Ok(mut gp) = crate::glowplug_client::GlowPlugClient::connect() {
        match gp.warm_handoff(&bdf, "nouveau", 2000, true, 15000) {
            Ok(_) => eprintln!("  glowplug: warm handoff complete"),
            Err(e) => eprintln!("  glowplug: warm_handoff failed ({e}), continuing"),
        }
    }

    let fds = crate::ember_client::request_fds(&bdf).expect("warm diagnostic requires ember");

    let mut dev = NvVfioComputeDevice::open_warm(&bdf, fds, sm, 0).expect("open_warm");

    // Proactive cleanup: clear ALL stale PCCSR
    clear_all_stale_pccsr(dev.bar0_ref());

    // Re-bind our channel (channel 0) after clearing stale state
    let pccsr_snap = dev.pccsr_status();
    eprintln!("Our channel after cleanup: {pccsr_snap}");

    // Try GR context setup
    let ctx_result = dev.setup_gr_context();
    match &ctx_result {
        Ok(ctx) => eprintln!("GR context setup: OK (image={}B)", ctx.image_size),
        Err(e) => eprintln!("GR context setup: FAILED — {e}"),
    }

    // Attempt dispatch
    let sm_ver = dev.sm_version();
    let wgsl = "@compute @workgroup_size(64) fn main() {}";
    let opts = coral_reef::CompileOptions {
        target: match sm_ver {
            70 => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm70),
            _ => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm86),
        },
        ..coral_reef::CompileOptions::default()
    };
    let compiled = coral_reef::compile_wgsl_full(wgsl, &opts).expect("compile");
    let info = coral_driver::ShaderInfo {
        gpr_count: compiled.info.gpr_count,
        shared_mem_bytes: compiled.info.shared_mem_bytes,
        barrier_count: compiled.info.barrier_count,
        workgroup: compiled.info.local_size,
        wave_size: 32,
    };

    dev.dispatch(
        &compiled.binary,
        &[],
        coral_driver::DispatchDims::linear(1),
        &info,
    )
    .expect("dispatch");

    use coral_driver::ComputeDevice;
    match dev.sync() {
        Ok(()) => {
            eprintln!("*** SYNC SUCCEEDED WITH PCCSR CLEANUP ***");
        }
        Err(e) => {
            eprintln!("sync failed: {e}");
            let diag = dev.layer7_diagnostics("EXP126B-TIMEOUT");
            eprintln!("{diag}");
        }
    }
}
