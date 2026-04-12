// SPDX-License-Identifier: AGPL-3.0-only
//! End-to-end sovereign compute: pure Rust from cold GPU to GPU execution.
//!
//! This is the culmination of the nouveau-replacement strategy:
//!
//! 1. GPU starts on vfio-pci (cold or warm — no nouveau needed)
//! 2. SovereignInit runs the full pure Rust init pipeline:
//!    - Stage 0: HBM2 Training (cold only — auto-detected)
//!    - Stage 1: PMC + Engine Gating
//!    - Stage 2: Topology Discovery (GPC/TPC/SM/FBP)
//!    - Stage 3: PFB + Memory Controller
//!    - Stage 4: Falcon Boot (SEC2 → ACR → FECS/GPCCS solver)
//!    - Stage 5: GR Engine Init (firmware BAR0 writes + FECS method probe)
//!    - Stage 6: PFIFO Discovery
//! 3. VfioChannel creates a PFIFO channel (no DRM ioctls)
//! 4. NOP command dispatched and verified via GPFIFO
//! 5. Subsystem validation confirms register state matches nouveau
//!
//! Zero external GPU drivers. Zero DRM. Zero proprietary code.
//! Just Rust + VFIO + vendor firmware blobs as ingredients.
//!
//! # Safety Warning
//!
//! This example opens VFIO **directly**, bypassing ember's fork-isolated
//! safety layer, PCIe armor, and sacrificial child pattern. A BAR0 write
//! to an uninitialized GPU can cause a PCIe bus hang that freezes the
//! **entire system** (not just this process). In production, always route
//! through `coralctl sovereign init` which uses ember's staged pipeline.
//!
//! Usage:
//!   sudo cargo run --example sovereign_compute_e2e --features vfio --release -- \
//!       --bdf 0000:03:00.0 \
//!       [--reference /path/to/warm_bar0.json] \
//!       [--sm 70] \
//!       [--cold]

use coral_driver::nv::vfio_compute::NvVfioComputeDevice;
use coral_driver::vfio::channel::diagnostic::subsystem_validator;
use coral_driver::{ComputeDevice, DispatchDims, ShaderInfo};
use std::path::PathBuf;

fn main() {
    eprintln!("WARNING: This example opens VFIO directly, bypassing ember's safety layer.");
    eprintln!("WARNING: BAR0 writes on a cold/partial GPU can freeze the entire system.");
    eprintln!("WARNING: Use `coralctl sovereign init` for safe, staged initialization.\n");

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args: Vec<String> = std::env::args().collect();

    let mut bdf = String::new();
    let mut reference_path: Option<PathBuf> = None;
    let mut sm_version = 0u32; // Auto-detect from BOOT0
    let mut cold_boot = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--bdf" => {
                bdf = args.get(i + 1).cloned().unwrap_or_default();
                i += 2;
            }
            "--reference" => {
                reference_path =
                    Some(PathBuf::from(args.get(i + 1).cloned().unwrap_or_default()));
                i += 2;
            }
            "--sm" => {
                sm_version = args
                    .get(i + 1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                i += 2;
            }
            "--cold" => {
                cold_boot = true;
                i += 1;
            }
            other => {
                eprintln!("Unknown argument: {other}");
                std::process::exit(1);
            }
        }
    }

    if bdf.is_empty() {
        eprintln!(
            "Usage: sovereign_compute_e2e --bdf <BDF> [--reference <path>] [--sm <version>] [--cold]"
        );
        std::process::exit(1);
    }

    println!("================================================================");
    println!("  SOVEREIGN COMPUTE — Pure Rust GPU Pipeline (nouveau replaced)");
    println!("================================================================");
    println!("  BDF:        {bdf}");
    println!("  SM version: {}", if sm_version == 0 { "auto-detect".to_string() } else { sm_version.to_string() });
    println!("  Cold boot:  {cold_boot}");
    println!("  Driver:     NONE (pure Rust + VFIO + firmware blobs)");
    println!("================================================================\n");

    // ── Phase 1: Sovereign Open (full SovereignInit pipeline) ────────
    println!("--- Phase 1: Sovereign GPU Open (SovereignInit pipeline) ---");
    let (mut dev, init_result) = NvVfioComputeDevice::open_sovereign(&bdf, sm_version)
        .unwrap_or_else(|e| {
            eprintln!("FATAL: Sovereign open failed: {e}");
            if cold_boot {
                eprintln!("  Cold boot requested — HBM2 training may have failed.");
                eprintln!("  Check VBIOS availability (PROM, sysfs, or file).");
            } else {
                eprintln!("  Ensure GPU is bound to vfio-pci: lspci -ks {bdf}");
            }
            std::process::exit(1);
        });

    println!("\n  Init Pipeline Results:");
    for stage in &init_result.stages {
        let status = if stage.ok() { "OK" } else { "FAIL" };
        println!(
            "  [{status:4}] {:20} {} writes, {} failed, {} us",
            stage.stage, stage.writes_applied, stage.writes_failed, stage.duration_us,
        );
    }

    if let Some(ref topo) = init_result.topology {
        println!("\n  Topology:");
        println!(
            "    GPC: {}  TPC/GPC: {:?}  SM: {}",
            topo.gpc_count, topo.tpc_per_gpc, topo.sm_count
        );
        println!(
            "    FBP: {}  LTC: {}  PBDMA: {} (mask {:#010x})",
            topo.fbp_count, topo.ltc_count, topo.pbdma_count, topo.pbdma_mask
        );
        println!("    GR runlist: {}", topo.gr_runlist_id);
    }

    println!("\n  HBM2 trained: {}", init_result.hbm2_trained);
    println!("  Falcons alive: {}", init_result.falcons_alive);
    println!("  GR ready:      {}", init_result.gr_ready);
    println!("  PFIFO ready:   {}", init_result.pfifo_ready);

    if !init_result.all_ok() {
        println!("\n  WARNING: Some init stages reported failures.");
        println!("  Proceeding anyway — partial init may still allow compute.");
    }
    println!();

    // ── Phase 2: Subsystem Validation ────────────────────────────────
    if let Some(ref ref_path) = reference_path {
        println!("--- Phase 2: Subsystem Validation vs Nouveau Reference ---");
        match subsystem_validator::load_reference_snapshot(ref_path) {
            Ok(reference) => {
                let validations =
                    subsystem_validator::validate_all(dev.bar0_ref(), &reference);
                let mut pass = 0;
                let mut fail = 0;
                for v in &validations {
                    if v.passed() {
                        pass += 1;
                    } else {
                        fail += 1;
                    }
                    println!("  {}", v.summary());
                }
                println!("  Total: {pass} passed, {fail} failed\n");
            }
            Err(e) => {
                eprintln!("  Cannot load reference: {e}\n");
            }
        }
    } else {
        println!("--- Phase 2: Skipped (no --reference provided) ---\n");
    }

    // ── Phase 3: NOP Dispatch ────────────────────────────────────────
    println!("--- Phase 3: Sovereign NOP Dispatch ---");

    let gr_status = dev.gr_engine_status();
    println!("  GR Engine Status:");
    println!("    PGRAPH:    {:#010x}", gr_status.pgraph_status);
    println!("    FECS:      cpuctl={:#010x} mb0={:#010x} mb1={:#010x}",
        gr_status.fecs_cpuctl, gr_status.fecs_mailbox0, gr_status.fecs_mailbox1);
    println!("    GPCCS:     cpuctl={:#010x}", gr_status.gpccs_cpuctl);
    println!("    PMC:       {:#010x}", gr_status.pmc_enable);
    println!("    PFIFO:     {:#010x}", gr_status.pfifo_enable);

    let fecs_alive = dev.fecs_is_alive();
    println!("    FECS alive: {fecs_alive}");

    let mut nop_success = false;
    if fecs_alive {
        match dev.discover_gr_context_sizes() {
            Ok((img, zcull, pm)) => {
                println!("    Context sizes: image={img}B zcull={zcull}B pm={pm}B");
            }
            Err(e) => {
                println!("    Context size query failed: {e}");
            }
        }

        println!("\n  Compiling minimal compute shader...");
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

        match coral_reef::compile_wgsl_full(wgsl, &opts) {
            Ok(compiled) => {
                let info = ShaderInfo {
                    gpr_count: compiled.info.gpr_count,
                    shared_mem_bytes: compiled.info.shared_mem_bytes,
                    barrier_count: compiled.info.barrier_count,
                    workgroup: compiled.info.local_size,
                    wave_size: 32,
                };

                println!("  Shader: {} bytes, {} GPRs, workgroup {:?}",
                    compiled.binary.len(), info.gpr_count, info.workgroup);

                println!("  Dispatching NOP shader via GPFIFO...");
                match dev.dispatch(&compiled.binary, &[], DispatchDims::linear(1), &info) {
                    Ok(()) => {
                        println!("  Dispatch submitted — waiting for sync...");
                        match dev.sync() {
                            Ok(()) => {
                                nop_success = true;
                                println!("  ****************************************************");
                                println!("  *  NOP DISPATCH + SYNC SUCCEEDED!                  *");
                                println!("  *  Pure Rust sovereign compute is WORKING.          *");
                                println!("  *  No nouveau. No nvidia. No DRM. Just Rust.        *");
                                println!("  ****************************************************");
                            }
                            Err(e) => {
                                println!("  Sync failed: {e}");
                                println!("  Dispatch was submitted but GPU did not complete.");
                                let diag = dev.layer7_diagnostics("NOP-DISPATCH-TIMEOUT");
                                println!("  {diag}");
                            }
                        }
                    }
                    Err(e) => {
                        println!("  Dispatch failed: {e}");
                    }
                }
            }
            Err(e) => {
                println!("  Shader compilation failed: {e}");
                println!("  (coral-reef compiler may not support SM {sm})");
            }
        }
    } else {
        println!("\n  FECS not alive — skipping NOP dispatch.");
        println!("  Falcon boot may have failed. Check firmware availability.");
    }

    // ── Summary ──────────────────────────────────────────────────────
    println!("\n================================================================");
    println!("  SOVEREIGN COMPUTE SUMMARY");
    println!("================================================================");
    println!(
        "  Init:     {} stages, {} OK",
        init_result.stages.len(),
        init_result.stages.iter().filter(|s| s.ok()).count()
    );

    if let Some(ref topo) = init_result.topology {
        println!(
            "  GPU:      {} SMs across {} GPCs ({} TPC/GPC)",
            topo.sm_count,
            topo.gpc_count,
            topo.tpc_per_gpc.first().unwrap_or(&0)
        );
    }

    if let Some(ref ctx) = init_result.gr_context {
        println!(
            "  Context:  image={}B zcull={}B iova={:#x} golden={}",
            ctx.image_size, ctx.zcull_size, ctx.iova, ctx.golden_saved,
        );
    }

    println!(
        "  HBM2:     {}",
        if init_result.hbm2_trained { "TRAINED" } else { "SKIPPED (warm)" }
    );
    println!(
        "  Falcons:  {}",
        if init_result.falcons_alive { "ALIVE" } else { "DEAD" }
    );
    println!(
        "  FECS:     {}",
        if fecs_alive { "RESPONSIVE" } else { "NOT RESPONDING" }
    );
    println!(
        "  NOP:      {}",
        if nop_success { "DISPATCHED + SYNCED" } else { "NOT ATTEMPTED / FAILED" }
    );
    println!("  DRM:      NONE");
    println!("  nouveau:  NOT LOADED");
    println!("  Pipeline: Rust → VFIO → SovereignInit → BAR0 → GPFIFO → GPU");
    println!("================================================================");

    if nop_success {
        std::process::exit(0);
    } else if !fecs_alive {
        std::process::exit(2);
    } else {
        std::process::exit(1);
    }
}
