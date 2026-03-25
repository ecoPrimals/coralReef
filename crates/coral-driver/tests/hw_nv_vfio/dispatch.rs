// SPDX-License-Identifier: AGPL-3.0-only

use crate::helpers::open_vfio;
use coral_driver::{ComputeDevice, DispatchDims, ShaderInfo};

#[test]
#[ignore = "requires VFIO-bound GPU hardware + compute shader binary"]
fn vfio_dispatch_nop_shader() {
    let mut dev = open_vfio();
    let sm = dev.sm_version();

    let gr = dev.gr_engine_status();
    eprintln!("Pre-dispatch {gr}");

    if gr.fecs_halted() {
        eprintln!("FECS falcon halted — dispatch will fence-timeout on cold VFIO");
        eprintln!("  (FECS requires signed firmware loaded by nouveau/ACR)");
        eprintln!("  Use GlowPlug oracle warm-up to initialize GR before VFIO dispatch.");
    }

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

    dev.dispatch(&compiled.binary, &[], DispatchDims::linear(1), &info)
        .expect("dispatch");

    let sync_result = dev.sync();
    let gr_post = dev.gr_engine_status();
    eprintln!("Post-dispatch {gr_post}");

    if let Err(e) = sync_result {
        if gr.fecs_halted() {
            eprintln!(
                "Dispatch fence-timeout expected: FECS not running — need GlowPlug oracle warm-up"
            );
        }
        panic!("sync: {e}");
    }
}

/// Exp 079: Warm handoff dispatch test.
///
/// Assumes `coralctl warm-fecs <bdf>` has been run first to cycle the
/// GPU through nouveau (loading FECS firmware) and back to VFIO.
/// Ember's NvidiaLifecycle disables `reset_method`, so FECS IMEM
/// should persist across the swap.
///
/// If FECS is running, this test attempts a full compute dispatch.
/// If it succeeds, we have a Layer 7 breakthrough.
#[test]
#[ignore = "requires VFIO-bound GPU hardware + warm-fecs"]
fn vfio_dispatch_warm_handoff() {
    let mut dev = open_vfio();
    eprintln!("\n=== Warm Handoff Dispatch Test (Exp 079) ===\n");

    let diag_pre = dev.layer7_diagnostics("WARM-HANDOFF-PRE");
    eprintln!("{diag_pre}");

    let fecs = &diag_pre.fecs;
    let fecs_running = !fecs.is_halted() && !fecs.is_in_reset() && fecs.cpuctl != 0xDEAD_DEAD;
    let fecs_has_mailbox = fecs.mailbox0 != 0;

    eprintln!("\n── FECS State After Warm Handoff ──");
    eprintln!(
        "  cpuctl={:#010x} mailbox0={:#010x}",
        fecs.cpuctl, fecs.mailbox0
    );
    eprintln!("  running={fecs_running} has_mailbox={fecs_has_mailbox}");

    if !fecs_running && !fecs_has_mailbox {
        eprintln!("\nFECS firmware NOT running after warm handoff.");
        eprintln!("Possible causes:");
        eprintln!("  1. `coralctl warm-fecs` was not run before this test");
        eprintln!("  2. Ember did not disable reset_method (FLR cleared IMEM)");
        eprintln!("  3. Nouveau failed to load FECS firmware");
        eprintln!("\nCapturing post-state for analysis...");
        let gr = dev.gr_engine_status();
        eprintln!("{gr}");
        eprintln!("\n=== End Warm Handoff (FECS not running) ===");
        return;
    }

    eprintln!("\nFECS appears active! Attempting compute dispatch...");

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
            eprintln!("\nTimed post-doorbell captures (warm handoff):");
            for cap in &captures {
                eprintln!("{cap}");
            }
        }
        Err(e) => {
            eprintln!("dispatch_traced failed: {e}");
        }
    }

    let diag_post = dev.layer7_diagnostics("WARM-HANDOFF-POST-DISPATCH");
    eprintln!("\n{diag_post}");

    eprintln!("\n── Sync attempt ──");
    let sync_result = dev.sync();
    match &sync_result {
        Ok(()) => {
            eprintln!("****************************************************");
            eprintln!("*  SYNC SUCCEEDED — LAYER 7 BREAKTHROUGH!          *");
            eprintln!("*  Warm handoff from nouveau preserved FECS IMEM.  *");
            eprintln!("*  Full sovereign compute dispatch is WORKING.     *");
            eprintln!("****************************************************");
        }
        Err(e) => {
            eprintln!("sync failed: {e}");
            eprintln!("\nEven with FECS running, dispatch failed. Possible causes:");
            eprintln!("  1. Channel context not compatible with nouveau's GR state");
            eprintln!("  2. GR engine needs additional init after handoff");
            eprintln!("  3. MMU page tables differ between nouveau and VFIO contexts");
            let diag_timeout = dev.layer7_diagnostics("WARM-HANDOFF-POST-TIMEOUT");
            eprintln!("\n{diag_timeout}");
        }
    }

    eprintln!("\n=== End Warm Handoff Dispatch ===");
}
