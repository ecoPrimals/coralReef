// SPDX-License-Identifier: AGPL-3.0-only
//! NVIDIA VFIO hardware validation — core device opening, BAR0, basic ops.
//!
//! These tests exercise the VFIO compute pipeline:
//! open → alloc → upload → dispatch → sync → readback.
//!
//! # Prerequisites
//!
//! - GPU bound to `vfio-pci` (not nouveau/nvidia)
//! - IOMMU enabled in BIOS and kernel
//! - User has `/dev/vfio/*` permissions
//! - Set `CORALREEF_VFIO_BDF` env var to the GPU's PCIe address
//!
//! # GlowPlug integration
//!
//! If `coral-glowplug` is running and holds the VFIO fd, the test harness
//! automatically borrows the device via `device.lend` and returns it via
//! `device.reclaim` on drop. No manual VFIO management needed.
//!
//! Run: `CORALREEF_VFIO_BDF=0000:01:00.0 cargo test --test hw_nv_vfio --features vfio -- --ignored --test-threads=1`
//!
//! `--test-threads=1` is required because all tests share ember's single IOMMU
//! IOAS. Parallel device creation maps the same fixed IOVAs and gets `EEXIST`
//! from `IOMMU_IOAS_MAP`.

#[cfg(feature = "vfio")]
#[path = "glowplug_client.rs"]
mod glowplug_client;

#[cfg(feature = "vfio")]
#[path = "ember_client.rs"]
mod ember_client;

#[cfg(feature = "vfio")]
mod tests {
    use super::ember_client;
    use coral_driver::nv::NvVfioComputeDevice;
    use coral_driver::{ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

    fn init_tracing() {
        use std::sync::Once;
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::from_default_env(),
                )
                .with_test_writer()
                .try_init()
                .ok();
        });
    }

    fn vfio_bdf() -> String {
        std::env::var("CORALREEF_VFIO_BDF")
            .expect("set CORALREEF_VFIO_BDF=0000:XX:XX.X to run VFIO tests")
    }

    /// SM hint: 0 = auto-detect from BOOT0 (preferred), nonzero = validate.
    fn vfio_sm() -> u32 {
        std::env::var("CORALREEF_VFIO_SM")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    }

    /// Open VFIO device — primary path: get fds from ember via SCM_RIGHTS.
    /// Fallback: open /dev/vfio/* directly (only works without ember).
    ///
    /// SM and compute class are auto-detected from BOOT0 by default.
    /// Set `CORALREEF_VFIO_SM` to a nonzero value to validate instead.
    fn open_vfio() -> NvVfioComputeDevice {
        init_tracing();
        let bdf = vfio_bdf();
        let sm = vfio_sm();

        match ember_client::request_fds(&bdf) {
            Ok(fds) => {
                eprintln!("ember: received VFIO fds for {bdf}");
                NvVfioComputeDevice::open_from_fds(&bdf, fds, sm, 0)
                    .expect("NvVfioComputeDevice::open_from_fds()")
            }
            Err(e) => {
                eprintln!("ember unavailable ({e}), opening VFIO directly");
                NvVfioComputeDevice::open(&bdf, sm, 0)
                    .expect("NvVfioComputeDevice::open() — is GPU bound to vfio-pci?")
            }
        }
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_open_and_bar0_read() {
        let _dev = open_vfio();
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_alloc_and_free() {
        let mut dev = open_vfio();
        let handle = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc");
        dev.free(handle).expect("free");
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_upload_and_readback() {
        let mut dev = open_vfio();
        let handle = dev.alloc(256, MemoryDomain::Gtt).expect("alloc");
        let data: Vec<u8> = (0..256).map(|i| (i & 0xFF) as u8).collect();
        dev.upload(handle, 0, &data).expect("upload");
        let result = dev.readback(handle, 0, 256).expect("readback");
        assert_eq!(result, data);
        dev.free(handle).expect("free");
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_multiple_buffers() {
        let mut dev = open_vfio();
        let handles: Vec<_> = (0..4)
            .map(|_| dev.alloc(4096, MemoryDomain::Gtt).expect("alloc"))
            .collect();
        for h in handles {
            dev.free(h).expect("free");
        }
    }

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
            eprintln!("  GR engine dead → scheduler holds channel in PENDING → PBDMA never loads context.");
        } else if fecs.is_halted() {
            eprintln!("FECS is HALTED — firmware may have been loaded but stopped.");
        } else if fecs.mailbox0 != 0 {
            eprintln!("FECS appears RUNNING — mailbox0={:#010x} (firmware active!).", fecs.mailbox0);
        } else {
            eprintln!("FECS state unclear — cpuctl={:#010x}, mailboxes zero.", fecs.cpuctl);
        }
        eprintln!("  secure_mode={} (signed firmware required)", fecs.requires_signed_firmware());

        // Phase 4: PCCSR channel analysis.
        eprintln!("\n── Channel Status Analysis ──");
        let pccsr = &diag_pre.pccsr;
        eprintln!("Channel 0: status={} enabled={} busy={}", pccsr.status_name(), pccsr.is_enabled(), pccsr.is_busy());
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

        let ce_entries: Vec<_> = diag.engine_topology.iter()
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
        let gr_runlist = diag.engine_topology.iter()
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
            pbdma_map, &dev.bar0_ref(), gr_runlist,
        );
        let ce_pbdmas = coral_driver::nv::vfio_compute::diagnostics::find_pbdmas_for_runlist(
            pbdma_map, &dev.bar0_ref(), ce_runlist,
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
            eprintln!("  Channel status: {} — unexpected, investigate.", pccsr.status_name());
        }

        eprintln!("\n=== End PBDMA CE Isolation ===");
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
        eprintln!("  cpuctl={:#010x} mailbox0={:#010x}", fecs.cpuctl, fecs.mailbox0);
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

    /// Exp 080: Sovereign FECS boot + compute dispatch.
    ///
    /// Loads FECS firmware directly into the falcon IMEM/DMEM ports,
    /// bypassing ACR secure boot. If FECS reports `secure=false` (as
    /// discovered in Exp 078), the firmware should load and execute.
    ///
    /// After FECS boot, attempts a full compute dispatch. If sync
    /// succeeds, we have achieved full sovereign GPU compute.
    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_sovereign_fecs_boot() {
        let mut dev = open_vfio();
        eprintln!("\n=== Sovereign FECS Boot (Exp 080) ===\n");

        let diag_pre = dev.layer7_diagnostics("SOVEREIGN-PRE");
        eprintln!("FECS state before boot:");
        eprintln!("  cpuctl={:#010x} mailbox0={:#010x} secure={}",
            diag_pre.fecs.cpuctl, diag_pre.fecs.mailbox0,
            diag_pre.fecs.requires_signed_firmware());

        if !diag_pre.fecs.is_in_reset() {
            eprintln!("FECS not in HRESET — already running? Checking mailbox...");
            if diag_pre.fecs.mailbox0 != 0 {
                eprintln!("  FECS firmware active (mailbox0={:#010x}), skipping boot.",
                    diag_pre.fecs.mailbox0);
            }
        }

        eprintln!("\n── Sovereign FECS Boot Attempt ──");
        match dev.sovereign_fecs_boot() {
            Ok(result) => {
                eprintln!("{result}");
                if result.running {
                    eprintln!("\n*** FECS firmware is RUNNING! ***\n");
                } else if result.mailbox0 != 0 {
                    eprintln!("\nFECS responded (mb0={:#010x}) but may not be fully running.",
                        result.mailbox0);
                } else {
                    eprintln!("\nFECS did not respond — boot may have failed.");
                    eprintln!("  cpuctl={:#010x} — check if halted or still in reset.",
                        result.cpuctl_after);
                }
            }
            Err(e) => {
                eprintln!("FECS boot error: {e}");
                let diag_fail = dev.layer7_diagnostics("SOVEREIGN-BOOT-FAIL");
                eprintln!("\n{diag_fail}");
                eprintln!("\n=== End Sovereign FECS Boot (FAILED) ===");
                return;
            }
        }

        // Also try GPCCS.
        eprintln!("\n── Sovereign GPCCS Boot Attempt ──");
        match coral_driver::nv::vfio_compute::fecs_boot::boot_gpccs(dev.bar0_ref(), "gv100") {
            Ok(result) => eprintln!("{result}"),
            Err(e) => eprintln!("GPCCS boot: {e}"),
        }

        let diag_post = dev.layer7_diagnostics("SOVEREIGN-POST-BOOT");
        eprintln!("\n{diag_post}");

        // Attempt dispatch if FECS appears running.
        let fecs_post = &diag_post.fecs;
        let fecs_alive = !fecs_post.is_in_reset() && !fecs_post.is_halted()
            || fecs_post.mailbox0 != 0;

        if !fecs_alive {
            eprintln!("\nFECS not running after boot — skipping dispatch attempt.");
            eprintln!("=== End Sovereign FECS Boot (no dispatch) ===");
            return;
        }

        eprintln!("\n── Dispatch attempt with sovereign-booted FECS ──");
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
            Err(e) => eprintln!("dispatch_traced: {e}"),
        }

        eprintln!("\n── Sync attempt ──");
        match dev.sync() {
            Ok(()) => {
                eprintln!("****************************************************");
                eprintln!("*  SYNC SUCCEEDED — SOVEREIGN COMPUTE ACHIEVED!    *");
                eprintln!("*  FECS firmware loaded directly via DMA upload.    *");
                eprintln!("*  No external driver dependency.                   *");
                eprintln!("****************************************************");
            }
            Err(e) => {
                eprintln!("sync failed: {e}");
                let diag_timeout = dev.layer7_diagnostics("SOVEREIGN-POST-TIMEOUT");
                eprintln!("\n{diag_timeout}");
            }
        }

        eprintln!("\n=== End Sovereign FECS Boot ===");
    }

    // ── Experiment 081: Falcon Boot Solver ──────────────────────────

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_sec2_probe() {
        init_tracing();
        let dev = open_vfio();

        eprintln!("\n=== Exp 081: SEC2 Probe (corrected base 0x087000) ===\n");

        let probe = dev.falcon_probe();
        eprintln!("{probe}");

        let sec2 = dev.sec2_probe();
        eprintln!("\nDetailed SEC2:\n{sec2}");

        eprintln!("\nSEC2 state classification: {:?}", sec2.state);
        eprintln!("  HS-locked: {}", sec2.state == coral_driver::nv::vfio_compute::acr_boot::Sec2State::HsLocked);
        eprintln!("  Clean reset: {}", sec2.state == coral_driver::nv::vfio_compute::acr_boot::Sec2State::CleanReset);

        // EMEM accessibility test
        let bar0 = dev.bar0_ref();
        let test_data = [0x42u8, 0x43, 0x44, 0x45];
        coral_driver::nv::vfio_compute::acr_boot::sec2_emem_write(bar0, 0, &test_data);
        let readback = coral_driver::nv::vfio_compute::acr_boot::sec2_emem_read(bar0, 0, 4);
        eprintln!("\nEMEM write/read test:");
        eprintln!("  wrote: {:02x?}", test_data);
        eprintln!("  read:  {:#010x}", readback.first().copied().unwrap_or(0));
        let expected = u32::from_le_bytes(test_data);
        let emem_ok = readback.first().copied() == Some(expected);
        eprintln!("  match: {emem_ok}");

        eprintln!("\n=== End SEC2 Probe ===");
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_falcon_boot_solver() {
        init_tracing();
        let dev = open_vfio();

        eprintln!("\n=== Exp 081: Falcon Boot Solver (all strategies) ===\n");

        let results = dev.falcon_boot_solver().expect("solver should not panic");

        eprintln!("Solver returned {} result(s):", results.len());
        for (i, result) in results.iter().enumerate() {
            eprintln!("\n── Strategy {} ──\n{result}", i + 1);
        }

        // Post-solver diagnostic
        let probe = dev.falcon_probe();
        eprintln!("\n── Post-solver falcon state ──\n{probe}");

        // Check for success
        let any_success = results.iter().any(|r| r.success);
        if any_success {
            eprintln!("\n****************************************************");
            eprintln!("*  FALCON BOOT SOLVER SUCCEEDED!                   *");
            eprintln!("*  FECS is running — GR engine should be ready.    *");
            eprintln!("****************************************************");
        } else {
            eprintln!("\nNo strategy achieved FECS boot.");
            eprintln!("Full ACR WPR chain (080b-d) needed for sovereign boot.");
        }

        let diag = dev.layer7_diagnostics("POST-SOLVER");
        eprintln!("\n{diag}");

        eprintln!("\n=== End Falcon Boot Solver ===");
    }

    /// Exp 083: System-memory ACR boot — WPR/inst/page tables in IOMMU DMA.
    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_sysmem_acr_boot() {
        init_tracing();
        let dev = open_vfio();

        eprintln!("\n=== Exp 083: System-Memory ACR Boot ===\n");

        let pre = dev.falcon_probe();
        eprintln!("Pre-boot falcon state:\n{pre}");

        // 083a: Pure system memory (all DMA buffers)
        let result_a = dev.sysmem_acr_boot();
        eprintln!("\n── 083a: Pure SysMem ──\n{result_a}");

        // 083b: Hybrid (VRAM page tables + sysmem data)
        let result_b = dev.hybrid_acr_boot();
        eprintln!("\n── 083b: Hybrid (VRAM PT + SysMem data) ──\n{result_b}");

        let post = dev.falcon_probe();
        eprintln!("\nPost-boot falcon state:\n{post}");

        if result_a.success || result_b.success {
            eprintln!("\n** ACR BOOT SUCCEEDED — FECS running! **");
        }

        eprintln!("\n=== End Exp 083 ===");
    }

    /// Test: VFIO FLR + PCI D3→D0 power cycle → check SEC2 state.
    /// If either puts SEC2 into HRESET, we can boot it with STARTCPU.
    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_flr_then_falcon_probe() {
        init_tracing();
        let dev = open_vfio();

        eprintln!("\n=== GPU Reset + Falcon Probe ===\n");

        let pre = dev.falcon_probe();
        eprintln!("Pre-reset:\n{pre}");

        // Try 1: VFIO device reset (FLR)
        eprintln!("\n--- Try 1: VFIO DEVICE_RESET (FLR) ---");
        match dev.device_reset() {
            Ok(()) => eprintln!("FLR succeeded"),
            Err(e) => eprintln!("FLR failed: {e}"),
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        let after_flr = dev.falcon_probe();
        eprintln!("After FLR:\n{after_flr}");
        eprintln!("SEC2 cpuctl: {:#010x} HRESET={}", after_flr.sec2.cpuctl, after_flr.sec2.cpuctl & 0x10 != 0);

        // Try 2: PCI D3→D0 power cycle
        eprintln!("\n--- Try 2: PCI D3→D0 power cycle ---");
        match dev.pci_power_cycle() {
            Ok((before, after)) => {
                eprintln!("Power cycle: D{before} → D3 → D{after}");
            }
            Err(e) => eprintln!("Power cycle failed: {e}"),
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
        let after_pm = dev.falcon_probe();
        eprintln!("After D3→D0:\n{after_pm}");
        eprintln!("SEC2 cpuctl: {:#010x} HRESET={}", after_pm.sec2.cpuctl, after_pm.sec2.cpuctl & 0x10 != 0);

        eprintln!("\n=== End GPU Reset Probe ===");
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_acr_firmware_inventory() {
        init_tracing();

        eprintln!("\n=== ACR Firmware Inventory ===\n");

        match coral_driver::nv::vfio_compute::acr_boot::AcrFirmwareSet::load("gv100") {
            Ok(fw) => {
                eprintln!("{}", fw.summary());
                eprintln!("\nACR BL:\n{}", fw.acr_bl_parsed);
                eprintln!("\nACR ucode:\n{}", fw.acr_ucode_parsed);

                // Dump sec2 desc.bin header for analysis
                eprintln!("\nSEC2 desc.bin ({} bytes):", fw.sec2_desc.len());
                let hex: String = fw.sec2_desc.iter().take(48).enumerate()
                    .map(|(i, b)| {
                        if i > 0 && i % 16 == 0 { format!("\n  {b:02x}") }
                        else if i > 0 && i % 4 == 0 { format!("  {b:02x}") }
                        else { format!("{b:02x}") }
                    }).collect();
                eprintln!("  {hex}");
            }
            Err(e) => eprintln!("Failed to load firmware: {e}"),
        }

        eprintln!("\n=== End Firmware Inventory ===");
    }

    /// Read SEC2 falcon registers via SysfsBar0 — works regardless of driver.
    /// Use this to capture Nouveau-warm state after a driver swap.
    #[test]
    #[ignore = "reads BAR0 via sysfs — run with appropriate BDF"]
    fn sysfs_sec2_register_dump() {
        let bdf = std::env::var("CORALREEF_VFIO_BDF")
            .unwrap_or_else(|_| "0000:03:00.0".to_string());

        eprintln!("\n=== SEC2 Register Dump via SysfsBar0 ({bdf}) ===\n");

        let bar0 = coral_driver::vfio::sysfs_bar0::SysfsBar0::open(&bdf, 0x100_0000)
            .expect("SysfsBar0::open");

        let driver_path = format!("/sys/bus/pci/devices/{bdf}/driver");
        let driver = std::fs::read_link(&driver_path)
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_else(|| "none".to_string());
        eprintln!("Current driver: {driver}");

        let sec2: usize = 0x87000;
        let fecs: usize = 0x409800;
        let gpccs: usize = 0x41A800;

        for (name, base) in [("SEC2", sec2), ("FECS", fecs), ("GPCCS", gpccs)] {
            let cpuctl = bar0.read_u32(base + 0x100);
            let sctl = bar0.read_u32(base + 0x240);
            let hwcfg = bar0.read_u32(base + 0x108);
            let bootvec = bar0.read_u32(base + 0x104);
            let mb0 = bar0.read_u32(base + 0x040);
            let mb1 = bar0.read_u32(base + 0x044);
            let dmactl = bar0.read_u32(base + 0x10C);
            let tracepc = bar0.read_u32(base + 0x030);
            let exci = bar0.read_u32(base + 0x148);

            eprintln!("{name} @ {base:#08x}:");
            eprintln!("  cpuctl={cpuctl:#010x} sctl={sctl:#010x} hwcfg={hwcfg:#010x}");
            eprintln!("  bootvec={bootvec:#010x} tracepc={tracepc:#010x} exci={exci:#010x}");
            eprintln!("  mb0={mb0:#010x} mb1={mb1:#010x} dmactl={dmactl:#010x}");
            eprintln!("  halted={} hreset={} hs_mode={}",
                cpuctl & 0x20 != 0, cpuctl & 0x10 != 0, sctl & 0x3000 != 0);

            if name == "SEC2" {
                let bind_inst = bar0.read_u32(base + 0x668);
                let fbif_624 = bar0.read_u32(base + 0x624);
                let dma_base = bar0.read_u32(base + 0x110);
                let dma_moffs = bar0.read_u32(base + 0x114);
                let dma_cmd = bar0.read_u32(base + 0x118);
                let dma_fboffs = bar0.read_u32(base + 0x11C);
                eprintln!("  0x668={bind_inst:#010x} 0x624={fbif_624:#010x}");
                eprintln!("  dma_base={dma_base:#010x} dma_moffs={dma_moffs:#010x} dma_cmd={dma_cmd:#010x} dma_fboffs={dma_fboffs:#010x}");

                // Also read some additional SEC2-specific registers
                for off in [0x480, 0x484, 0x488, 0x48C, 0x490, 0x494] {
                    let v = bar0.read_u32(base + off);
                    if v != 0 { eprintln!("  +{off:#05x}={v:#010x}"); }
                }
            }
            eprintln!();
        }

        // Check PMC
        let pmc_enable = bar0.read_u32(0x200);
        let sec2_bit = 22;
        eprintln!("PMC_ENABLE={pmc_enable:#010x} SEC2_enabled={}", pmc_enable & (1 << sec2_bit) != 0);
        eprintln!("\n=== End SEC2 Register Dump ===");
    }

    #[cfg(feature = "test-utils")]
    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_free_invalid_handle() {
        let mut dev = open_vfio();
        let result = dev.free(coral_driver::BufferHandle::from_id(9999));
        assert!(result.is_err());
    }

    #[cfg(feature = "test-utils")]
    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_readback_invalid_handle() {
        let dev = open_vfio();
        let result = dev.readback(coral_driver::BufferHandle::from_id(9999), 0, 16);
        assert!(result.is_err());
    }
}
