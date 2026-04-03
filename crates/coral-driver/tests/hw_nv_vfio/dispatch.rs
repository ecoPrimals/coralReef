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
            35..=37 => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm35),
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

/// Exp 079/125: Warm handoff dispatch test.
///
/// Assumes `coralctl warm-fecs <bdf>` has been run first (with the
/// `livepatch_nvkm_mc_reset` module loaded) to cycle the GPU through
/// nouveau (loading FECS firmware) and back to VFIO with falcons preserved.
///
/// `open_vfio_warm()` internally:
///  - Creates a channel with warm PFIFO init (no PMC reset / PBDMA clear)
///  - Issues STARTCPU to restart GPCCS then FECS from HRESET
///  - Sets up GR context (discover sizes, bind, golden save)
///
/// If that succeeds, this test compiles a NOP shader and dispatches it.
/// A successful sync = Layer 7 breakthrough (sovereign compute via warm handoff).
#[test]
#[ignore = "requires VFIO-bound GPU hardware + warm-fecs + livepatch"]
fn vfio_dispatch_warm_handoff() {
    let mut dev = crate::helpers::open_vfio_warm();
    eprintln!("\n=== Warm Handoff Dispatch Test (Exp 079/125) ===\n");

    let diag_pre = dev.layer7_diagnostics("WARM-HANDOFF-PRE");
    eprintln!("{diag_pre}");

    let fecs = &diag_pre.fecs;
    let fecs_dead = fecs.cpuctl == 0xDEAD_DEAD || fecs.cpuctl & 0xBADF_0000 == 0xBADF_0000;

    eprintln!("\n── FECS State (post-restart) ──");
    eprintln!(
        "  cpuctl={:#010x} mailbox0={:#010x} in_reset={} halted={}",
        fecs.cpuctl,
        fecs.mailbox0,
        fecs.is_in_reset(),
        fecs.is_halted()
    );

    if fecs_dead {
        eprintln!("\nFECS engine inaccessible (PRI timeout) — GPU is cold.");
        eprintln!("Ensure `coralctl warm-fecs` was run with livepatch loaded.");
        let gr = dev.gr_engine_status();
        eprintln!("{gr}");
        eprintln!("\n=== End Warm Handoff (engine dead) ===");
        return;
    }

    eprintln!("\nFECS engine accessible. Attempting compute dispatch...");

    let sm = dev.sm_version();
    let wgsl = "@compute @workgroup_size(64) fn main() {}";
    let opts = coral_reef::CompileOptions {
        target: match sm {
            35..=37 => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm35),
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
            eprintln!("\nDispatch failed despite FECS restart. Possible causes:");
            eprintln!("  1. GR context binding incompatible with nouveau's GR state");
            eprintln!("  2. FECS firmware rejected STARTCPU re-entry");
            eprintln!("  3. MMU page tables differ between nouveau and VFIO contexts");
            let diag_timeout = dev.layer7_diagnostics("WARM-HANDOFF-POST-TIMEOUT");
            eprintln!("\n{diag_timeout}");
        }
    }

    eprintln!("\n=== End Warm Handoff Dispatch ===");
}

/// Exp 132: Ember-frozen warm dispatch via `open_warm_with_context`.
///
/// This test uses the diesel engine pattern:
/// 1. Glowplug orchestrates nouveau boot → livepatch → STOP_CTXSW
/// 2. PFIFO snapshot captured via ember.mmio.read
/// 3. Swap back to vfio-pci (FECS alive + frozen, PFIFO destroyed)
/// 4. `open_warm_with_context` selects `warm_fecs` PFIFO config
/// 5. Rebuild PFIFO infrastructure, START_CTXSW, GR context, dispatch
///
/// Compared to `vfio_dispatch_warm_handoff` (Exp 079/125), this path:
/// - Freezes FECS scheduling BEFORE nouveau teardown (STOP_CTXSW)
/// - Captures PFIFO state for informed rebuild
/// - Uses hybrid PFIFO config that rebuilds infrastructure while
///   preserving falcon IMEM
#[test]
#[ignore = "requires VFIO-bound GPU hardware + warm-fecs + livepatch"]
fn vfio_dispatch_warm_fecs_frozen() {
    let mut dev = crate::helpers::open_vfio_warm_with_context();
    eprintln!("\n=== Exp 132: Ember-Frozen Warm Dispatch ===\n");

    let diag_pre = dev.layer7_diagnostics("WARM-FECS-FROZEN-PRE");
    eprintln!("{diag_pre}");

    let fecs = &diag_pre.fecs;
    let fecs_dead = fecs.cpuctl == 0xDEAD_DEAD || fecs.cpuctl & 0xBADF_0000 == 0xBADF_0000;

    if fecs_dead {
        eprintln!("FECS registers return dead/PRI-fault — FECS did not survive handoff");
        eprintln!("STOP_CTXSW may not have been effective, or PCI reset still killed FECS.");
        return;
    }

    let wgsl = "
        @group(0) @binding(0) var<storage, read_write> out: array<u32>;
        @compute @workgroup_size(1)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            out[gid.x] = gid.x + 132u;
        }
    ";

    let opts = coral_reef::CompileOptions {
        target: match dev.sm_version() {
            35..=37 => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm35),
            70..=79 => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm70),
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
            eprintln!("\nTimed post-doorbell captures (frozen FECS dispatch):");
            for cap in &captures {
                eprintln!("{cap}");
            }
        }
        Err(e) => {
            eprintln!("dispatch_traced failed: {e}");
        }
    }

    let diag_post = dev.layer7_diagnostics("WARM-FECS-FROZEN-POST-DISPATCH");
    eprintln!("{diag_post}");

    match dev.sync_fence(std::time::Duration::from_secs(5)) {
        Ok(()) => {
            eprintln!("************************************************************");
            eprintln!("*  SYNC SUCCEEDED — EXP 132 FROZEN FECS DISPATCH WORKING! *");
            eprintln!("*  Diesel engine pattern: STOP_CTXSW → PFIFO rebuild →    *");
            eprintln!("*  START_CTXSW → sovereign compute. Full pipeline.        *");
            eprintln!("************************************************************");
        }
        Err(e) => {
            eprintln!("sync failed: {e}");
            eprintln!("\nDispatch failed. Diagnostic context:");
            eprintln!("  - FECS was frozen via STOP_CTXSW before nouveau teardown");
            eprintln!("  - PFIFO rebuilt with warm_fecs config");
            eprintln!("  - START_CTXSW sent to resume scheduling");
            eprintln!("  Possible causes:");
            eprintln!("  1. PCI bus reset during vfio-pci rebind killed FECS despite freeze");
            eprintln!("  2. PFIFO rebuild from snapshot missed critical register state");
            eprintln!("  3. FECS rejected START_CTXSW after runlist replacement");
            let diag_timeout = dev.layer7_diagnostics("WARM-FECS-FROZEN-POST-TIMEOUT");
            eprintln!("\n{diag_timeout}");
        }
    }

    eprintln!("\n=== End Exp 132 Frozen FECS Dispatch ===");
}
