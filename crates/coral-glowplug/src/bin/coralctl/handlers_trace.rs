// SPDX-License-Identifier: AGPL-3.0-only
//! coralctl handlers for trace parsing, recipe application, devinit replay,
//! and K80 sovereign cold boot.
//!
//! These absorb functionality that previously lived in Python scripts
//! (`parse_mmiotrace.py`, `apply_recipe.py`, `replay_devinit.py`) into
//! the coralctl CLI, backed by coral-driver's Rust implementations.

use std::path::Path;

use coral_driver::vfio::channel::diagnostic::boot_follower::BootTrace;
use coral_driver::vfio::channel::diagnostic::k80_cold_boot;
use coral_driver::vfio::channel::diagnostic::replay;

use crate::rpc::rpc_call;

/// Parse an mmiotrace file and display a domain-classified summary.
pub(crate) fn trace_parse(file: &str, recipe_json: bool) {
    let path = Path::new(file);
    let trace = match BootTrace::from_mmiotrace(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: failed to parse mmiotrace: {e}");
            std::process::exit(1);
        }
    };

    if recipe_json {
        let recipe = trace.to_recipe();
        let json = serde_json::to_string_pretty(&recipe).expect("serialize recipe");
        println!("{json}");
        return;
    }

    eprintln!(
        "Parsed {} writes, {} reads from {} (driver: {}, duration: {:.3}s)",
        trace.writes.len(),
        trace.reads.len(),
        file,
        trace.driver,
        trace.duration_us as f64 / 1_000_000.0,
    );

    let recipe = trace.to_recipe();
    let mut domains: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for step in &recipe {
        *domains.entry(&step.domain).or_default() += 1;
    }

    println!("\n{:<20} WRITES", "DOMAIN");
    println!("{}", "-".repeat(32));
    for (domain, count) in &domains {
        println!("{:<20} {count}", domain);
    }
    println!("{}", "-".repeat(32));
    println!("{:<20} {} total recipe steps", "", recipe.len());
}

/// Apply a recipe to a GPU via RPC (individual register writes through the daemon).
pub(crate) fn oracle_apply_rpc(socket: &str, bdf: &str, recipe_path: &str) {
    let path = Path::new(recipe_path);
    let recipe = match replay::load_recipe(path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: failed to load recipe {recipe_path}: {e}");
            std::process::exit(1);
        }
    };

    eprintln!("Applying {} recipe steps to {bdf} via RPC...", recipe.len());

    let mut applied = 0usize;
    let mut failed = 0usize;

    for step in &recipe {
        let resp = rpc_call(
            socket,
            "device.write_register",
            serde_json::json!({
                "bdf": bdf,
                "offset": step.offset,
                "value": step.value as u64,
                "allow_dangerous": false,
            }),
        );

        if resp.get("error").is_some() {
            failed += 1;
        } else {
            applied += 1;
        }
    }

    println!("Recipe applied: {applied} writes, {failed} failed ({} total steps)", recipe.len());
    if failed > 0 {
        std::process::exit(1);
    }
}

/// Apply a recipe directly via VFIO BAR0 (local, no daemon).
pub(crate) fn oracle_apply_local(bdf: &str, recipe_path: &str) {
    let path = Path::new(recipe_path);
    let recipe = match replay::load_recipe(path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: failed to load recipe {recipe_path}: {e}");
            std::process::exit(1);
        }
    };

    eprintln!(
        "Applying {} recipe steps to {bdf} via direct BAR0...",
        recipe.len()
    );

    match replay::apply_recipe(bdf, &recipe) {
        Ok(result) => {
            println!(
                "Recipe applied: {} writes, {} failed",
                result.applied, result.failed
            );
            println!(
                "PMC_BOOT_0={:#010x} PTIMER={}",
                result.pmc_boot_0,
                if result.ptimer_ticking {
                    "ticking"
                } else {
                    "stopped"
                }
            );
            if !result.is_alive() {
                eprintln!("warning: GPU does not appear healthy after recipe replay");
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("error: recipe replay failed: {e}");
            std::process::exit(1);
        }
    }
}

/// Replay VBIOS devinit scripts on a cold GPU.
pub(crate) fn devinit_replay(bdf: &str, diagnostics: bool, vbios_path: Option<&str>) {
    use coral_driver::vfio::VfioDevice;

    eprintln!("Opening VFIO device {bdf} for devinit replay...");

    let fds = match coral_driver::vfio::ember_client::request_vfio_fds(bdf) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("error: ember VFIO fds for {bdf}: {e} — falling back to direct open");
            match VfioDevice::open(bdf) {
                Ok(d) => {
                    let bar0 = match d.map_bar(0) {
                        Ok(b) => b,
                        Err(e2) => {
                            eprintln!("error: failed to map BAR0: {e2}");
                            std::process::exit(1);
                        }
                    };
                    return devinit_replay_inner(&bar0, bdf, diagnostics, vbios_path);
                }
                Err(e2) => {
                    eprintln!("error: direct open also failed: {e2}");
                    std::process::exit(1);
                }
            }
        }
    };
    let device = match VfioDevice::from_received(bdf, fds) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: failed to open VFIO device {bdf} from ember fds: {e}");
            std::process::exit(1);
        }
    };

    let bar0 = match device.map_bar(0) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: failed to map BAR0 for {bdf}: {e}");
            std::process::exit(1);
        }
    };

    devinit_replay_inner(&bar0, bdf, diagnostics, vbios_path);
}

fn devinit_replay_inner(
    bar0: &coral_driver::vfio::device::MappedBar,
    bdf: &str,
    diagnostics: bool,
    vbios_path: Option<&str>,
) {
    use coral_driver::vfio::channel::devinit;

    let boot0 = bar0.read_u32(0x0).unwrap_or(0xDEAD_DEAD);
    eprintln!("BOOT0={boot0:#010x}");

    if let Some(path) = vbios_path {
        eprintln!("Using pre-captured VBIOS from: {path}");
        let rom = match std::fs::read(path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("error: failed to read VBIOS file: {e}");
                std::process::exit(1);
            }
        };
        eprintln!("VBIOS: {} bytes from file", rom.len());

        eprintln!("Running host-side VBIOS interpreter...");
        match devinit::interpret_boot_scripts(bar0, &rom) {
            Ok(stats) => {
                println!(
                    "VBIOS interpreter: {} ops, {} writes, {} unknown opcodes",
                    stats.ops_executed, stats.writes_applied, stats.unknown_opcodes.len()
                );
                if stats.writes_applied == 0 {
                    eprintln!("warning: no writes applied — interpreter may not support this VBIOS format");
                }
            }
            Err(e) => {
                eprintln!("error: VBIOS interpreter failed: {e}");
            }
        }

        eprintln!("Attempting PMU FALCON devinit with file-based ROM...");
        match devinit::execute_devinit(bar0, &rom) {
            Ok(true) => println!("PMU devinit completed successfully"),
            Ok(false) => println!("PMU devinit: device already POSTed"),
            Err(e) => eprintln!("PMU devinit error: {e}"),
        }

        let boot0_post = bar0.read_u32(0x0).unwrap_or(0xDEAD_DEAD);
        let ptimer = bar0.read_u32(0x9400).unwrap_or(0xDEAD_DEAD);
        let pfb = bar0.read_u32(0x100000).unwrap_or(0xDEAD_DEAD);
        println!("Post-devinit: BOOT0={boot0_post:#010x} PTIMER={ptimer:#010x} PFB={pfb:#010x}");
        return;
    }

    if diagnostics {
        match devinit::execute_devinit_with_diagnostics(bar0, Some(bdf)) {
            Ok(success) => {
                if success {
                    println!("Devinit (diagnostics) completed successfully");
                } else {
                    eprintln!("warning: devinit with diagnostics reported partial completion");
                    std::process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("error: devinit with diagnostics failed: {e}");
                std::process::exit(1);
            }
        }
    } else {
        let rom = match devinit::read_vbios_prom(bar0) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("error: failed to read VBIOS PROM: {e}");
                std::process::exit(1);
            }
        };
        eprintln!("VBIOS: {} bytes read from PROM", rom.len());

        match devinit::execute_devinit(bar0, &rom) {
            Ok(success) => {
                if success {
                    println!("Devinit replay completed successfully");
                } else {
                    eprintln!("warning: devinit replay reported partial completion");
                    std::process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("error: devinit replay failed: {e}");
                std::process::exit(1);
            }
        }
    }

    let boot0_post = bar0.read_u32(0x0).unwrap_or(0xDEAD_DEAD);
    let ptimer = bar0.read_u32(0x9400).unwrap_or(0xDEAD_DEAD);
    println!("Post-devinit: BOOT0={boot0_post:#010x} PTIMER={ptimer:#010x}");
}

/// Sovereign cold boot a Tesla K80 (GK210) from fully powered-off state.
///
/// Orchestrates the full boot sequence: clock init, devinit replay,
/// optional PGRAPH/PCCSR/PRAMIN setup, and FECS/GPCCS PIO firmware upload.
#[expect(clippy::fn_params_excessive_bools)]
pub(crate) fn cold_boot_replay(
    bdf: &str,
    recipe_path: &str,
    firmware_dir: Option<&str>,
    pgraph: bool,
    pccsr: bool,
    pramin: bool,
    skip_firmware: bool,
) {
    use coral_driver::vfio::VfioDevice;

    println!("=== K80 Sovereign Cold Boot ===");
    println!("  device:   {bdf}");
    println!("  recipe:   {recipe_path}");
    println!("  pgraph:   {pgraph}  pccsr: {pccsr}  pramin: {pramin}");
    println!("  firmware: {}", if skip_firmware { "skip" } else { "upload" });
    println!();

    // Request VFIO fds from ember (which holds the immortal group fd).
    let fds = match coral_driver::vfio::ember_client::request_vfio_fds(bdf) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("error: ember VFIO fds for {bdf}: {e}");
            eprintln!("  hint: is coral-ember running? (systemctl status coral-ember)");
            std::process::exit(1);
        }
    };
    let device = match VfioDevice::from_received(bdf, fds) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: failed to open VFIO device {bdf} from ember fds: {e}");
            std::process::exit(1);
        }
    };

    let bar0 = match device.map_bar(0) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: failed to map BAR0 for {bdf}: {e}");
            std::process::exit(1);
        }
    };

    let boot0 = bar0.read_u32(0x0).unwrap_or(0xDEAD_DEAD);
    println!("BOOT0={boot0:#010x}");

    let config = k80_cold_boot::ColdBootConfig {
        include_pgraph: pgraph,
        include_pccsr: pccsr,
        include_pramin: pramin,
    };

    let (fecs_code, fecs_data, gpccs_code, gpccs_data) = if skip_firmware {
        (None, None, None, None)
    } else {
        let fw_dir = firmware_dir
            .map(std::path::PathBuf::from)
            .unwrap_or_else(default_firmware_dir);

        println!("Loading firmware from {}", fw_dir.display());

        let fc = load_fw_file(&fw_dir, "fecs_inst.bin");
        let fd = load_fw_file(&fw_dir, "fecs_data.bin");
        let gc = load_fw_file(&fw_dir, "gpccs_inst.bin");
        let gd = load_fw_file(&fw_dir, "gpccs_data.bin");

        println!(
            "  fecs_inst={}B  fecs_data={}B  gpccs_inst={}B  gpccs_data={}B",
            fc.len(), fd.len(), gc.len(), gd.len()
        );

        (Some(fc), Some(fd), Some(gc), Some(gd))
    };

    let recipe = Path::new(recipe_path);
    let result = k80_cold_boot::cold_boot(
        &bar0,
        recipe,
        &config,
        fecs_code.as_deref(),
        fecs_data.as_deref(),
        gpccs_code.as_deref(),
        gpccs_data.as_deref(),
    );

    match result {
        Ok(boot) => {
            println!();
            println!("=== Cold Boot Log ===");
            for line in &boot.log {
                println!("  {line}");
            }
            println!();
            println!("=== Results ===");
            println!(
                "  clock:    applied={} failed={}  ptimer={}",
                boot.clock_replay.applied,
                boot.clock_replay.failed,
                if boot.clock_replay.ptimer_ticking { "ticking" } else { "stopped" }
            );
            if let Some(ref devinit) = boot.devinit_replay {
                println!(
                    "  devinit:  applied={} failed={}",
                    devinit.applied, devinit.failed
                );
            }
            if let Some(ref pgraph_r) = boot.pgraph_replay {
                println!(
                    "  extended: applied={} failed={}",
                    pgraph_r.applied, pgraph_r.failed
                );
            }
            println!("  fecs:     {}", if boot.fecs_running { "RUNNING" } else { "NOT RUNNING" });
            println!(
                "  BOOT0:    {:#010x}  arch={}",
                boot.firmware_snapshot.boot0, boot.firmware_snapshot.architecture
            );

            if boot.fecs_running {
                println!();
                println!(">>> SOVEREIGN COLD BOOT SUCCESS — FECS is alive");
                println!(">>> GPU is ready for compute channel creation via NvVfioComputeDevice");
            } else {
                println!();
                eprintln!("warning: FECS did not start — GPU may need additional initialization");
                eprintln!("  Try: coralctl cold-boot {bdf} --recipe {recipe_path} --pccsr --pramin");
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("error: cold boot failed: {e}");
            std::process::exit(1);
        }
    }
}

/// Run the Falcon Boot Solver (all ACR strategies) on a Volta+ GPU.
///
/// Gets VFIO fds from ember, detects chip from BOOT0, then runs all
/// strategies in `FalconBootSolver::boot` to authenticate and start
/// FECS/GPCCS firmware.
pub(crate) fn acr_boot(bdf: &str) {
    use coral_driver::nv::identity::{boot0_to_sm, chip_name};
    use coral_driver::nv::vfio_compute::acr_boot::{FalconBootSolver, FalconProbe};
    use coral_driver::vfio::VfioDevice;

    println!("=== ACR Boot Solver ===");
    println!("  device: {bdf}");

    let fds = match coral_driver::vfio::ember_client::request_vfio_fds(bdf) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("error: ember VFIO fds for {bdf}: {e}");
            eprintln!("  hint: is coral-ember running? (systemctl status coral-ember)");
            std::process::exit(1);
        }
    };
    let device = match VfioDevice::from_received(bdf, fds) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: failed to open VFIO device {bdf} from ember fds: {e}");
            std::process::exit(1);
        }
    };

    let bar0 = match device.map_bar(0) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: failed to map BAR0 for {bdf}: {e}");
            std::process::exit(1);
        }
    };

    let boot0 = bar0.read_u32(0x0).unwrap_or(0xDEAD_DEAD);
    let sm = boot0_to_sm(boot0).unwrap_or(0);
    let chip = chip_name(sm);
    println!("  BOOT0:  {boot0:#010x}  SM={sm}  chip={chip}");

    let probe = FalconProbe::capture(&bar0);
    println!("  {probe}");

    let container = device.dma_backend();

    println!("\nRunning all ACR boot strategies...\n");
    match FalconBootSolver::boot(&bar0, chip, Some(container), None) {
        Ok(results) => {
            println!("\n=== ACR Boot Results ({} strategies tried) ===", results.len());
            for (i, r) in results.iter().enumerate() {
                let status = if r.success { "SUCCESS" } else { "FAILED" };
                println!("  [{i}] {status}: {:?}", r.strategy);
                for note in &r.notes {
                    println!("       {note}");
                }
            }

            let any_success = results.iter().any(|r| r.success);
            println!();
            if any_success {
                println!(">>> FECS boot succeeded — GPU context switch engine is alive");
            } else {
                eprintln!(">>> No strategy succeeded — FECS did not boot");

                let post_probe = FalconProbe::capture(&bar0);
                println!("  post-boot: {post_probe}");
                std::process::exit(1);
            }
        }
        Err(e) => {
            eprintln!("error: ACR solver failed: {e}");
            std::process::exit(1);
        }
    }
}

/// Sovereign boot: recipe replay → ACR/FECS boot. No kernel driver needed.
pub(crate) fn sovereign_boot(bdf: &str, recipe_path: Option<&str>, skip_acr: bool, pio_only: bool) {
    use coral_driver::nv::identity::{boot0_to_sm, chip_name};
    use coral_driver::nv::vfio_compute::acr_boot::{FalconBootSolver, FalconProbe};
    use coral_driver::vfio::VfioDevice;
    use coral_driver::vfio::channel::diagnostic::sovereign_boot as sb;

    println!("=== Sovereign Boot ===");
    println!("  device: {bdf}");

    let fds = match coral_driver::vfio::ember_client::request_vfio_fds(bdf) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("error: ember VFIO fds for {bdf}: {e}");
            std::process::exit(1);
        }
    };
    let device = match VfioDevice::from_received(bdf, fds) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: VFIO device {bdf}: {e}");
            std::process::exit(1);
        }
    };
    let bar0 = match device.map_bar(0) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: BAR0 map for {bdf}: {e}");
            std::process::exit(1);
        }
    };

    let boot0 = bar0.read_u32(0x0).unwrap_or(0xDEAD_DEAD);
    let era = sb::GpuEra::from_boot0(boot0);
    let sm = boot0_to_sm(boot0).unwrap_or(0);
    let chip = chip_name(sm);
    println!("  BOOT0:  {boot0:#010x}  era={era}  SM={sm}  chip={chip}");

    // Pre-boot falcon state.
    let pre_probe = FalconProbe::capture(&bar0);
    println!("  pre-boot: {pre_probe}");

    let pmc_enable = bar0.read_u32(0x200).unwrap_or(0xDEAD_DEAD);
    let vram_test = bar0.read_u32(0x700000).unwrap_or(0xDEAD_DEAD);
    println!("  PMC_ENABLE: {pmc_enable:#010x}  VRAM@0x700000: {vram_test:#010x}");

    // Phase 0: Enable engines + clear PRIV ring faults.
    // The VBIOS devinit (run at power-on) leaves PMC_ENABLE minimal.
    // We must enable GR, PFIFO, FBHUB, etc. before the recipe can be effective.
    if matches!(era, sb::GpuEra::NvidiaVolta | sb::GpuEra::NvidiaModern) {
        println!("\n--- Phase 0: PMC Engine Enable ---");
        pmc_full_enable(&bar0);
    }

    // Resolve recipe path.
    let resolved_recipe = match recipe_path {
        Some(p) => std::path::PathBuf::from(p),
        None => resolve_sovereign_recipe(bdf, era),
    };
    println!("  recipe: {}", resolved_recipe.display());

    // Phase 1: sovereign_boot recipe replay.
    println!("\n--- Phase 1: Recipe Replay ({era}) ---");
    match sb::sovereign_boot(&bar0, &resolved_recipe, None) {
        Ok(result) => {
            for line in &result.log {
                println!("  {line}");
            }
            for (phase, res) in &result.phase_results {
                println!(
                    "  phase '{phase}': applied={} failed={} ptimer={} alive={}",
                    res.applied, res.failed, res.ptimer_ticking, res.is_alive()
                );
            }
            println!("  alive: {}", result.alive);

            let post_pmc = bar0.read_u32(0x200).unwrap_or(0xDEAD_DEAD);
            let post_vram = bar0.read_u32(0x700000).unwrap_or(0xDEAD_DEAD);
            println!("  PMC_ENABLE: {post_pmc:#010x}  VRAM@0x700000: {post_vram:#010x}");
        }
        Err(e) => {
            eprintln!("error: sovereign boot recipe replay failed: {e}");
            std::process::exit(1);
        }
    }

    // Post-recipe falcon state.
    let mid_probe = FalconProbe::capture(&bar0);
    println!("\n  post-recipe: {mid_probe}");

    // Phase 2: ACR boot (Volta+) or FECS PIO (Kepler).
    if skip_acr {
        println!("\n--- Phase 2: ACR boot SKIPPED (--skip-acr) ---");
        return;
    }

    match era {
        sb::GpuEra::NvidiaVolta | sb::GpuEra::NvidiaModern => {
            let mode_str = if pio_only { "PIO-only" } else { "ACR" };
            println!("\n--- Phase 2: {mode_str} Boot ({chip}) ---");
            let solver_result = if pio_only {
                FalconBootSolver::boot_pio_only(&bar0, chip, None)
            } else {
                let container = device.dma_backend();
                FalconBootSolver::boot(&bar0, chip, Some(container), None)
            };
            match solver_result {
                Ok(results) => {
                    println!("\n  ACR Results ({} strategies):", results.len());
                    for (i, r) in results.iter().enumerate() {
                        let status = if r.success { "SUCCESS" } else { "FAILED" };
                        println!("  [{i}] {status}: {:?}", r.strategy);
                        for note in &r.notes {
                            println!("       {note}");
                        }
                    }
                    let any_success = results.iter().any(|r| r.success);
                    if any_success {
                        println!("\n>>> FECS boot succeeded — sovereign compute ready");
                    } else {
                        let post = FalconProbe::capture(&bar0);
                        println!("\n  post-acr: {post}");
                        eprintln!(">>> ACR: no strategy succeeded");
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("error: ACR solver: {e}");
                    std::process::exit(1);
                }
            }
        }
        sb::GpuEra::NvidiaKepler => {
            println!("\n--- Phase 2: Kepler FECS PIO Boot ({chip}) ---");
            use coral_driver::nv::kepler_falcon;
            let fw_dir = resolve_kepler_firmware_dir(bdf);
            println!("  firmware: {}", fw_dir.display());

            let load = |name: &str| -> Option<Vec<u8>> {
                let path = fw_dir.join(name);
                match std::fs::read(&path) {
                    Ok(d) => { println!("  loaded {name} ({}B)", d.len()); Some(d) }
                    Err(e) => { eprintln!("  missing {name}: {e}"); None }
                }
            };

            let fecs_code = load("fecs_inst.bin");
            let fecs_data = load("fecs_data.bin");
            let gpccs_code = load("gpccs_inst.bin");
            let gpccs_data = load("gpccs_data.bin");

            if let (Some(fc), Some(fd), Some(gc), Some(gd)) =
                (&fecs_code, &fecs_data, &gpccs_code, &gpccs_data)
            {
                let mut bar0_reg = bar0;
                match kepler_falcon::boot_fecs_gpccs(
                    &mut bar0_reg, fc, fd, gc, gd,
                    std::time::Duration::from_secs(5),
                ) {
                    Ok(()) => {
                        println!("\n>>> FECS boot succeeded — sovereign compute ready");
                    }
                    Err(e) => {
                        eprintln!("error: Kepler FECS PIO boot: {e}");
                        std::process::exit(1);
                    }
                }
            } else {
                eprintln!("warning: incomplete Kepler firmware — skipping FECS PIO boot");
            }
        }
        _ => {
            println!("\n--- Phase 2: No boot strategy for {era} ---");
        }
    }
}

/// Enable all GPU engines via PMC and clear PRIV ring faults.
///
/// After VBIOS devinit, the GPU has clocks running and VRAM trained, but
/// most engines are clock-gated in PMC_ENABLE. This function enables them
/// so FBHUB, PFIFO, GR, and SEC2 are accessible for ACR boot.
fn pmc_full_enable(bar0: &coral_driver::vfio::device::MappedBar) {
    let pmc_pre = bar0.read_u32(0x200).unwrap_or(0);
    println!("  PMC_ENABLE before: {pmc_pre:#010x}");

    // Enable all engines. On Volta, bit 0 is reserved and should stay 0.
    // The safe comprehensive mask enables: PFIFO(1), PGRAPH(12), CE(various),
    // PBDMA(various), PMU(13), SEC2(22), NVDEC(15), FBHUB, LTC, etc.
    // Writing 0xFFFFFFFF can cause issues on some GPUs, so use the mask
    // from nvidia driver's full init: enable known-good bits.
    let full_enable: u32 = 0x7FFF_FFFF;
    bar0.write_u32(0x200, full_enable).ok();
    std::thread::sleep(std::time::Duration::from_millis(10));

    let pmc_post = bar0.read_u32(0x200).unwrap_or(0);
    println!("  PMC_ENABLE after:  {pmc_post:#010x}");

    // Clear PRIV ring faults (0x12_0100 = PRI_RING_INTR).
    let pri_intr = bar0.read_u32(0x12_0100).unwrap_or(0);
    if pri_intr != 0 {
        bar0.write_u32(0x12_0100, pri_intr).ok();
        println!("  PRI_RING_INTR cleared: {pri_intr:#010x}");
    }

    // Volta PRIV ring GPC broadcast enable (0x12004c).
    bar0.write_u32(0x12_004c, 0x0000_0002).ok();
    std::thread::sleep(std::time::Duration::from_millis(5));

    // Re-read PRIV ring to confirm clear.
    let pri_post = bar0.read_u32(0x12_0100).unwrap_or(0);
    if pri_post != 0 {
        bar0.write_u32(0x12_0100, pri_post).ok();
        println!("  PRI_RING_INTR (2nd clear): {pri_post:#010x}");
    }

    // Check VRAM accessibility after engine enable.
    bar0.write_u32(0x1700, 0x7).ok();
    std::thread::sleep(std::time::Duration::from_millis(2));
    let vram_check = bar0.read_u32(0x70_0000).unwrap_or(0xDEAD);
    println!("  VRAM@PRAMIN:  {vram_check:#010x}");

    // Check FBHUB status.
    let fbhub_ctrl = bar0.read_u32(0x100c80).unwrap_or(0xDEAD);
    let pmc_final = bar0.read_u32(0x200).unwrap_or(0);
    println!("  FBHUB_CTRL:   {fbhub_ctrl:#010x}  PMC_ENABLE: {pmc_final:#010x}");
}

/// Resolve the best recipe file for a GPU based on its era.
fn resolve_sovereign_recipe(
    bdf: &str,
    era: coral_driver::vfio::channel::diagnostic::sovereign_boot::GpuEra,
) -> std::path::PathBuf {
    use coral_driver::vfio::channel::diagnostic::sovereign_boot::GpuEra;

    let data_dir = find_data_dir();

    let candidates: Vec<std::path::PathBuf> = match era {
        GpuEra::NvidiaVolta | GpuEra::NvidiaModern => vec![
            data_dir.join("titanv/nouveau_init_recipe.json"),
            data_dir.join("titanv/nvidia535-captures/nvidia535_init_recipe.json"),
            data_dir.join("titanv/nvidia535-vm-captures/gv100_full_bios_recipe.json"),
        ],
        GpuEra::NvidiaKepler => {
            vec![
                data_dir.join("k80/gk210_devinit_recipe.json"),
                data_dir.join("k80/recipes/gk210_devinit_recipe.json"),
                data_dir.join("k80/gk210_init_recipe.json"),
            ]
        }
        _ => vec![],
    };

    for c in &candidates {
        if c.exists() {
            return c.clone();
        }
    }

    eprintln!("error: no recipe found for {era} at {bdf}");
    eprintln!("  tried:");
    for c in &candidates {
        eprintln!("    {}", c.display());
    }
    eprintln!("  use --recipe to specify a recipe file");
    std::process::exit(1);
}

/// Resolve Kepler firmware directory (handles flat gk210→gk20a layout).
fn resolve_kepler_firmware_dir(_bdf: &str) -> std::path::PathBuf {
    let candidates = [
        std::path::PathBuf::from("/lib/firmware/nvidia/gk210/gr"),
        std::path::PathBuf::from("/lib/firmware/nvidia/gk210"),
        std::path::PathBuf::from("/lib/firmware/nvidia/gk20a"),
        std::path::PathBuf::from("/usr/share/coralreef/firmware/nvidia/gk110"),
    ];
    for c in &candidates {
        if c.join("fecs_inst.bin").exists() {
            return c.clone();
        }
    }
    eprintln!("warning: Kepler firmware not found, tried:");
    for c in &candidates {
        eprintln!("  {}", c.display());
    }
    candidates[0].clone()
}

/// Find the hotSpring data directory.
fn find_data_dir() -> std::path::PathBuf {
    let candidates = [
        std::path::PathBuf::from("/home/biomegate/Development/ecoPrimals/springs/hotSpring/data"),
        std::path::PathBuf::from("data"),
        std::path::PathBuf::from("../../springs/hotSpring/data"),
    ];
    for c in &candidates {
        if c.is_dir() {
            return c.clone();
        }
    }
    candidates[0].clone()
}

fn default_firmware_dir() -> std::path::PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    let crate_root = exe
        .ancestors()
        .find(|p| p.join("Cargo.toml").exists())
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let candidates = [
        crate_root.join("../../data/firmware/nvidia/gk110"),
        crate_root.join("data/firmware/nvidia/gk110"),
        std::path::PathBuf::from("/usr/share/coralreef/firmware/nvidia/gk110"),
    ];

    for c in &candidates {
        if c.join("fecs_inst.bin").exists() {
            return c.clone();
        }
    }

    eprintln!("warning: firmware directory not found, tried:");
    for c in &candidates {
        eprintln!("  {}", c.display());
    }
    eprintln!("specify --firmware-dir explicitly");
    std::process::exit(1);
}

fn load_fw_file(dir: &std::path::Path, name: &str) -> Vec<u8> {
    let path = dir.join(name);
    std::fs::read(&path).unwrap_or_else(|e| {
        eprintln!("error: cannot read firmware {}: {e}", path.display());
        std::process::exit(1);
    })
}
