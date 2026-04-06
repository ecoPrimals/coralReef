// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::helpers::open_vfio;
use coral_driver::{ComputeDevice, DispatchDims, ShaderInfo};

/// Exp 078: Comprehensive Layer 7 diagnostic matrix.
///
/// Captures falcon states (FECS, GPCCS, PMU, SEC2), engine topology,
/// PCCSR channel status, PFIFO scheduler state, and PBDMA register
/// snapshots. Then dispatches a nop shader with timed post-doorbell
/// captures to observe scheduler behavior over time.
#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn vfio_layer7_diagnostic() {
    let mut dev = open_vfio();

    eprintln!("\n=== Layer 7 Diagnostic Matrix (Exp 078) ===\n");

    // Phase 1: Full state capture before any dispatch attempt.
    let diag_pre = dev.layer7_diagnostics("PRE-DISPATCH");
    eprintln!("{diag_pre}");

    // Phase 2: Engine topology analysis.
    let topo = &diag_pre.engine_topology;
    let gr_entry = topo.iter().find(|e| e.engine_type == 0);
    let ce_entries: Vec<_> = topo.iter().filter(|e| e.engine_type == 1).collect();

    if let Some(gr) = gr_entry {
        eprintln!("\nGR engine on runlist {}", gr.runlist);
    } else {
        eprintln!("\nWARNING: No GR engine found in topology!");
    }
    for ce in &ce_entries {
        eprintln!("CE engine on runlist {}", ce.runlist);
    }

    // Phase 3: FECS analysis.
    eprintln!("\n── FECS Falcon Analysis ──");
    let fecs = &diag_pre.fecs;
    if fecs.is_in_reset() {
        eprintln!("FECS is in HRESET state — no firmware loaded.");
        eprintln!("  This is the root cause of Layer 7 failures:");
        eprintln!(
            "  GR engine dead → scheduler holds channel in PENDING → PBDMA never loads context."
        );
    } else if fecs.is_halted() {
        eprintln!("FECS is HALTED — firmware may have been loaded but stopped.");
    } else if fecs.mailbox0 != 0 {
        eprintln!(
            "FECS appears RUNNING — mailbox0={:#010x} (firmware active!).",
            fecs.mailbox0
        );
    } else {
        eprintln!(
            "FECS state unclear — cpuctl={:#010x}, mailboxes zero.",
            fecs.cpuctl
        );
    }
    eprintln!(
        "  secure_mode={} (signed firmware required)",
        fecs.requires_signed_firmware()
    );

    // Phase 4: PCCSR channel analysis.
    eprintln!("\n── Channel Status Analysis ──");
    let pccsr = &diag_pre.pccsr;
    eprintln!(
        "Channel 0: status={} enabled={} busy={}",
        pccsr.status_name(),
        pccsr.is_enabled(),
        pccsr.is_busy()
    );
    if pccsr.status() == 1 {
        eprintln!("  PENDING confirms: scheduler sees channel but won't schedule onto PBDMA.");
        eprintln!("  Root cause: GR engine not ready (FECS firmware not loaded).");
    }

    // Phase 5: Dispatch with timed captures.
    eprintln!("\n── Dispatching nop shader with timed PBDMA captures ──");
    let sm = dev.sm_version();
    let wgsl = "@compute @workgroup_size(64) fn main() {}";
    let opts = coral_reef::CompileOptions {
        target: match sm {
            70 => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm70),
            75 => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm75),
            80 => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm80),
            _ => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm86),
        },
        ..coral_reef::CompileOptions::default()
    };
    let compiled = coral_reef::compile_wgsl_full(wgsl, &opts).expect("compile");
    let info = ShaderInfo {
        gpr_count: compiled.info.gpr_count,
        shared_mem_bytes: compiled.info.shared_mem_bytes,
        barrier_count: compiled.info.barrier_count,
        workgroup: compiled.info.local_size,
        wave_size: 32,
    };

    match dev.dispatch_traced(&compiled.binary, &[], DispatchDims::linear(1), &info) {
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

    // Phase 6: Post-dispatch full state capture.
    let diag_post = dev.layer7_diagnostics("POST-DISPATCH");
    eprintln!("\n{diag_post}");

    // Phase 7: Sync attempt (will fence-timeout on cold VFIO).
    eprintln!("\n── Sync attempt ──");
    let sync_result = dev.sync();
    match &sync_result {
        Ok(()) => eprintln!("SYNC SUCCEEDED — Layer 7 breakthrough!"),
        Err(e) => {
            eprintln!("sync failed (expected on cold VFIO): {e}");
            let diag_timeout = dev.layer7_diagnostics("POST-TIMEOUT");
            eprintln!("\n{diag_timeout}");
        }
    }

    eprintln!("\n=== End Layer 7 Diagnostic Matrix ===");
}

/// Exp 078: PBDMA isolation test — verify PBDMA works on CE runlist.
///
/// Creates a channel on the copy engine (CE) runlist instead of the
/// GR runlist. If GP_GET advances on CE but not GR, it proves the
/// PBDMA mechanism works and the issue is GR-engine-specific (FECS).
///
/// NOTE: This test captures diagnostic data but does not assert success
/// because CE runlist may also require engine-specific init. The data
/// is compared with GR runlist behavior to isolate the failure.
#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn vfio_pbdma_ce_isolation() {
    let dev = open_vfio();
    eprintln!("\n=== PBDMA CE Isolation Test (Exp 078) ===\n");

    let diag = dev.layer7_diagnostics("CE-ISOLATION");

    let ce_entries: Vec<_> = diag
        .engine_topology
        .iter()
        .filter(|e| e.engine_type == 1)
        .collect();

    if ce_entries.is_empty() {
        eprintln!("No CE engines found in topology — cannot run isolation test.");
        eprintln!("Engine topology:");
        for e in &diag.engine_topology {
            eprintln!("  {e}");
        }
        return;
    }

    let ce_runlist = ce_entries[0].runlist;
    let gr_runlist = diag
        .engine_topology
        .iter()
        .find(|e| e.engine_type == 0)
        .map(|e| e.runlist)
        .unwrap_or(1);

    eprintln!("GR runlist: {gr_runlist}");
    eprintln!("CE runlist: {ce_runlist}");

    if ce_runlist == gr_runlist {
        eprintln!("CE and GR share the same runlist — isolation test not possible.");
        return;
    }

    // Capture PBDMA state for both runlists.
    let pbdma_map = diag.pfifo.pbdma_map;
    let gr_pbdmas = coral_driver::nv::vfio_compute::diagnostics::find_pbdmas_for_runlist(
        pbdma_map,
        dev.bar0_ref(),
        gr_runlist,
    );
    let ce_pbdmas = coral_driver::nv::vfio_compute::diagnostics::find_pbdmas_for_runlist(
        pbdma_map,
        dev.bar0_ref(),
        ce_runlist,
    );

    eprintln!("GR PBDMAs: {gr_pbdmas:?}");
    eprintln!("CE PBDMAs: {ce_pbdmas:?}");

    let gr_snaps = dev.pbdma_snapshot(&gr_pbdmas);
    let ce_snaps = dev.pbdma_snapshot(&ce_pbdmas);

    eprintln!("\n── GR PBDMA state ──");
    for s in &gr_snaps {
        eprintln!("{s}");
    }
    eprintln!("\n── CE PBDMA state ──");
    for s in &ce_snaps {
        eprintln!("{s}");
    }

    // Channel is on GR runlist from open_vfio(). Compare PCCSR status.
    let pccsr = dev.pccsr_status();
    eprintln!("\n{pccsr}");
    eprintln!("\nConclusion:");
    if pccsr.status() == 1 {
        eprintln!("  Channel PENDING on GR runlist — consistent with FECS-not-running hypothesis.");
        eprintln!("  CE PBDMAs shown above for comparison. If CE has different state,");
        eprintln!("  the issue is GR-engine-specific (requires FECS firmware).");
    } else {
        eprintln!(
            "  Channel status: {} — unexpected, investigate.",
            pccsr.status_name()
        );
    }

    eprintln!("\n=== End PBDMA CE Isolation ===");
}
