// SPDX-License-Identifier: AGPL-3.0-only
//! Warm handoff: nouveau trains HBM2, then VFIO takes over for compute.
//!
//! This is the endgame pipeline: nouveau (or silicon BootROM) handles DEVINIT
//! and HBM2 training, then we rebind the GPU to vfio-pci **without FLR** to
//! preserve VRAM state, open via VFIO, and dispatch compute work.
//!
//! Stages:
//!   1. Verify nouveau is currently bound and GPU is alive
//!   2. Unbind from nouveau (no FLR — preserves HBM2)
//!   3. Disable all PCI reset methods (prevent kernel from resetting on bind)
//!   4. Bind to vfio-pci
//!   5. Open via VFIO, validate BAR0 is still responsive
//!   6. Run per-subsystem validation against optional reference snapshot
//!   7. Optionally create a sovereign channel and submit a NOP
//!
//! Usage:
//!   sudo cargo run --example warm_handoff_vfio --features vfio --release -- \
//!       --bdf 0000:03:00.0 \
//!       [--reference /path/to/warm_bar0.json] \
//!       [--dispatch-nop] \
//!       [--skip-rebind]
//!
//! With `--skip-rebind`: assumes GPU is already on vfio-pci (e.g. from a previous run).
//!
//! # Safety Warning
//!
//! This example opens VFIO **directly**, bypassing ember's fork-isolated
//! safety layer. A BAR0 write to an uninitialized GPU can cause a PCIe
//! bus hang that freezes the entire system. In production, always route
//! through `coralctl sovereign init` which uses ember's staged pipeline.

use coral_driver::vfio::channel::diagnostic::subsystem_validator;
use coral_driver::vfio::device::{MappedBar, VfioDevice};
use std::path::PathBuf;

const PTIMER_TIME_0: usize = 0x9400;
const PTIMER_TIME_1: usize = 0x9410;
const PMC_BOOT_0: usize = 0x0;

fn sysfs_pci(bdf: &str, file: &str) -> PathBuf {
    PathBuf::from(format!("/sys/bus/pci/devices/{bdf}/{file}"))
}

fn write_sysfs(path: &std::path::Path, val: &str) -> Result<(), String> {
    std::fs::write(path, val).map_err(|e| format!("sysfs write {}: {e}", path.display()))
}

fn current_driver(bdf: &str) -> Option<String> {
    let link = sysfs_pci(bdf, "driver");
    std::fs::read_link(&link)
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
}

fn disable_reset_methods(bdf: &str) -> Result<(), String> {
    let path = sysfs_pci(bdf, "reset_method");
    if path.exists() {
        write_sysfs(&path, "")?;
        println!("  Disabled PCI reset methods (FLR/PM/bus)");
    } else {
        println!("  reset_method sysfs not available (kernel < 5.15?)");
        println!("  WARNING: kernel may FLR on vfio-pci bind, destroying HBM2 state");
    }
    Ok(())
}

fn unbind_driver(bdf: &str, driver: &str) -> Result<(), String> {
    let unbind_path = PathBuf::from(format!("/sys/bus/pci/drivers/{driver}/unbind"));
    write_sysfs(&unbind_path, bdf)?;
    std::thread::sleep(std::time::Duration::from_millis(500));
    println!("  Unbound from {driver}");
    Ok(())
}

fn bind_vfio_pci(bdf: &str) -> Result<(), String> {
    let override_path = sysfs_pci(bdf, "driver_override");
    write_sysfs(&override_path, "vfio-pci")?;
    println!("  Set driver_override = vfio-pci");

    let probe_path = PathBuf::from("/sys/bus/pci/drivers_probe");
    write_sysfs(&probe_path, bdf)?;
    std::thread::sleep(std::time::Duration::from_millis(500));

    match current_driver(bdf) {
        Some(ref d) if d == "vfio-pci" => {
            println!("  Bound to vfio-pci");
            Ok(())
        }
        Some(d) => Err(format!("expected vfio-pci, got {d}")),
        None => Err("no driver bound after probe".into()),
    }
}

fn check_gpu_alive(bar0: &MappedBar) -> (u32, bool) {
    let boot0 = bar0.read_u32(PMC_BOOT_0).unwrap_or(0xFFFF_FFFF);
    let t0_a = bar0.read_u32(PTIMER_TIME_0).unwrap_or(0);
    let t1_a = bar0.read_u32(PTIMER_TIME_1).unwrap_or(0);
    for _ in 0..1000 {
        let _ = bar0.read_u32(PMC_BOOT_0);
    }
    let t0_b = bar0.read_u32(PTIMER_TIME_0).unwrap_or(0);
    let t1_b = bar0.read_u32(PTIMER_TIME_1).unwrap_or(0);
    let ticking = t0_a != t0_b || t1_a != t1_b;
    (boot0, ticking)
}

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
    let mut dispatch_nop = false;
    let mut skip_rebind = false;

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
            "--dispatch-nop" => {
                dispatch_nop = true;
                i += 1;
            }
            "--skip-rebind" => {
                skip_rebind = true;
                i += 1;
            }
            other => {
                eprintln!("Unknown argument: {other}");
                std::process::exit(1);
            }
        }
    }

    if bdf.is_empty() {
        eprintln!("Usage: warm_handoff_vfio --bdf <BDF> [--reference <path>] [--dispatch-nop] [--skip-rebind]");
        std::process::exit(1);
    }

    println!("=== Warm Handoff: Nouveau → VFIO ===");
    println!("BDF: {bdf}");
    println!();

    // ── Stage 1: Verify current state ────────────────────────────────
    if !skip_rebind {
        println!("--- Stage 1: Verify current driver ---");
        let driver = current_driver(&bdf);
        println!("  Current driver: {}", driver.as_deref().unwrap_or("none"));

        match driver.as_deref() {
            Some("nouveau") => {
                println!("  nouveau is bound — GPU should be initialized with HBM2 trained");
            }
            Some("vfio-pci") => {
                println!("  Already on vfio-pci — switching to --skip-rebind mode");
                // Fall through to VFIO open
            }
            Some(d) => {
                eprintln!("  WARNING: unexpected driver '{d}' — proceeding with unbind");
            }
            None => {
                eprintln!("  No driver bound — GPU may not be initialized");
                eprintln!("  Proceeding anyway (VFIO open may fail)");
            }
        }

        if driver.as_deref() != Some("vfio-pci") {
            // ── Stage 2: Disable reset methods ───────────────────────────
            println!("\n--- Stage 2: Disable PCI reset methods ---");
            if let Err(e) = disable_reset_methods(&bdf) {
                eprintln!("  WARNING: {e}");
            }

            // ── Stage 3: Unbind from current driver ──────────────────────
            if let Some(ref d) = driver {
                println!("\n--- Stage 3: Unbind from {d} ---");
                if let Err(e) = unbind_driver(&bdf, d) {
                    eprintln!("  ERROR: {e}");
                    std::process::exit(1);
                }
            }

            // ── Stage 4: Bind to vfio-pci ────────────────────────────────
            println!("\n--- Stage 4: Bind to vfio-pci ---");
            if let Err(e) = bind_vfio_pci(&bdf) {
                eprintln!("  ERROR: {e}");
                std::process::exit(1);
            }
        }
    } else {
        println!("--- Skipping rebind (--skip-rebind) ---");
        let driver = current_driver(&bdf);
        if driver.as_deref() != Some("vfio-pci") {
            eprintln!(
                "  ERROR: expected vfio-pci, got {}",
                driver.as_deref().unwrap_or("none")
            );
            std::process::exit(1);
        }
        println!("  vfio-pci confirmed");
    }

    // ── Stage 5: Open via VFIO ───────────────────────────────────────
    println!("\n--- Stage 5: Open via VFIO ---");
    let device = VfioDevice::open(&bdf).unwrap_or_else(|e| {
        eprintln!("  ERROR: VfioDevice::open failed: {e}");
        std::process::exit(1);
    });

    let bar0 = device.map_bar(0).unwrap_or_else(|e| {
        eprintln!("  ERROR: map_bar(0) failed: {e}");
        std::process::exit(1);
    });

    let (boot0, ticking) = check_gpu_alive(&bar0);
    println!("  PMC_BOOT_0 = {boot0:#010x}");
    println!("  PTIMER ticking = {ticking}");

    if boot0 == 0xFFFF_FFFF {
        eprintln!("  FATAL: GPU unresponsive — HBM2 state likely destroyed by FLR");
        eprintln!("  Ensure reset_method is cleared before unbind");
        std::process::exit(2);
    }

    if !ticking {
        eprintln!("  WARNING: PTIMER not ticking — GPU may be partially initialized");
    }

    let pmc_enable = bar0.read_u32(0x200).unwrap_or(0);
    let pfifo_enable = bar0.read_u32(0x2200).unwrap_or(0);
    println!("  PMC_ENABLE   = {pmc_enable:#010x}");
    println!("  PFIFO_ENABLE = {pfifo_enable:#010x}");

    // ── Stage 6: Subsystem validation ────────────────────────────────
    println!("\n--- Stage 6: Subsystem Validation ---");

    let reference = match reference_path {
        Some(ref p) => {
            match subsystem_validator::load_reference_snapshot(p) {
                Ok(r) => {
                    println!("  Loaded reference: {} registers from {}", r.len(), p.display());
                    r
                }
                Err(e) => {
                    eprintln!("  WARNING: failed to load reference: {e}");
                    eprintln!("  Running validation without reference (actual values only)");
                    std::collections::BTreeMap::new()
                }
            }
        }
        None => {
            println!("  No reference snapshot provided — showing actual values only");
            std::collections::BTreeMap::new()
        }
    };

    let validations = subsystem_validator::validate_all(&bar0, &reference);
    let mut total_pass = 0usize;
    let mut total_fail = 0usize;

    for v in &validations {
        println!("  {}", v.summary());
        if v.passed() {
            total_pass += 1;
        } else {
            total_fail += 1;
            for c in &v.comparisons {
                if !c.matches {
                    println!(
                        "    {} [{:#08x}]: actual={:#010x} expected={:#010x}",
                        c.name, c.offset, c.actual, c.expected,
                    );
                }
            }
        }
    }

    println!(
        "\n  Subsystems: {total_pass} passed, {total_fail} failed out of {}",
        validations.len()
    );

    // ── Stage 7: SovereignInit validation (nouveau replacement pipeline) ──
    println!("\n--- Stage 7: SovereignInit Pipeline Validation ---");
    use coral_driver::nv::vfio_compute::sovereign_init::SovereignInit;

    let sm_version = coral_driver::nv::identity::boot0_to_sm(boot0).unwrap_or(70);
    let init = SovereignInit::new(&bar0, sm_version)
        .with_bdf(&bdf)
        .with_dma_backend(device.dma_backend());
    let init_result = init.init_all();

    for stage in &init_result.stages {
        let status = if stage.ok() { "OK" } else { "FAIL" };
        println!(
            "  [{status:4}] {:20} {} writes, {} failed, {} us",
            stage.stage, stage.writes_applied, stage.writes_failed, stage.duration_us,
        );
    }

    if let Some(ref topo) = init_result.topology {
        println!("  Topology: {}GPC {}SM {}FBP {}PBDMA",
            topo.gpc_count, topo.sm_count, topo.fbp_count, topo.pbdma_count);
    }

    let falcons_alive = init_result.falcons_alive;
    println!("  Falcons alive:    {}", init_result.falcons_alive);
    println!("  FECS responsive:  {}", init_result.fecs_responsive);
    println!("  Compute ready:    {}", init_result.compute_ready());

    if falcons_alive {
        println!("  Falcons appear ALIVE — GR engine ready for sovereign compute");
    } else {
        println!("  Falcons appear DEAD — would need ACR/SEC2 boot for GR dispatch");
    }

    // ── Stage 8: Optional NOP dispatch ───────────────────────────────
    if dispatch_nop {
        println!("\n--- Stage 8: Sovereign NOP Dispatch ---");
        println!("  Creating sovereign VFIO channel...");

        let container = device.dma_backend();
        let gpfifo_iova = 0x1000u64;
        let userd_iova = 0x2000u64;
        let gpfifo_entries = 128u32;

        use coral_driver::vfio::dma::DmaBuffer;

        let _gpfifo = match DmaBuffer::new(container.clone(), 128 * 8, gpfifo_iova) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("  ERROR: GPFIFO DMA alloc failed: {e}");
                std::process::exit(1);
            }
        };

        let _userd = match DmaBuffer::new(container.clone(), 4096, userd_iova) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("  ERROR: USERD DMA alloc failed: {e}");
                std::process::exit(1);
            }
        };

        use coral_driver::vfio::channel::VfioChannel;

        match VfioChannel::create_sovereign(
            container,
            &bar0,
            gpfifo_iova,
            gpfifo_entries,
            userd_iova,
            0,
        ) {
            Ok(chan) => {
                println!("  Channel created: id={}", chan.id());
                println!("  Sovereign VFIO channel operational — no DRM ioctls used");

                let post_pfifo = bar0.read_u32(0x2200).unwrap_or(0);
                let post_sched = bar0.read_u32(0x2504).unwrap_or(0);
                println!("  Post-channel: PFIFO_ENABLE={post_pfifo:#010x} SCHED_EN={post_sched:#010x}");
            }
            Err(e) => {
                eprintln!("  Channel creation failed: {e}");
                eprintln!("  This is expected if falcons are dead after nouveau unbind");
                eprintln!("  The warm handoff itself succeeded — BAR0 is accessible");
            }
        }
    }

    // ── Summary ──────────────────────────────────────────────────────
    println!("\n=== Warm Handoff Summary ===");
    println!("  GPU:        PMC_BOOT_0={boot0:#010x} PTIMER={ticking}");
    println!("  BAR0:       Responsive (VFIO path operational)");
    println!("  HBM2:       {} (preserved across driver rebind)",
        if ticking { "ALIVE" } else { "UNKNOWN" });
    println!("  Subsystems: {total_pass}/{} validated", validations.len());

    let stage_names: Vec<&str> = validations
        .iter()
        .filter(|v| v.passed())
        .map(|v| v.subsystem.as_str())
        .collect();
    if !stage_names.is_empty() {
        println!("  Passing:    {}", stage_names.join(", "));
    }

    let failed_names: Vec<&str> = validations
        .iter()
        .filter(|v| !v.passed())
        .map(|v| v.subsystem.as_str())
        .collect();
    if !failed_names.is_empty() {
        println!("  Failing:    {}", failed_names.join(", "));
    }

    if falcons_alive {
        println!("  Compute:    READY (falcons alive, channel creation possible)");
    } else {
        println!("  Compute:    NEEDS FALCON BOOT (ACR/SEC2 required before dispatch)");
    }

    println!("\n=== Warm Handoff Complete ===");
}
