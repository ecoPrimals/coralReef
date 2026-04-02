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
pub(crate) fn devinit_replay(bdf: &str, diagnostics: bool) {
    use coral_driver::vfio::channel::devinit;
    use coral_driver::vfio::VfioDevice;

    eprintln!("Opening VFIO device {bdf} for devinit replay...");

    let device = match VfioDevice::open(bdf) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: failed to open VFIO device {bdf}: {e}");
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
    eprintln!("BOOT0={boot0:#010x}");

    if diagnostics {
        // Enhanced diagnostics mode: probes falcon state, selects VBIOS source automatically.
        match devinit::execute_devinit_with_diagnostics(&bar0, Some(bdf)) {
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
        let rom = match devinit::read_vbios_prom(&bar0) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("error: failed to read VBIOS PROM: {e}");
                std::process::exit(1);
            }
        };
        eprintln!("VBIOS: {} bytes read from PROM", rom.len());

        match devinit::execute_devinit(&bar0, &rom) {
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

    let device = match VfioDevice::open(bdf) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: failed to open VFIO device {bdf}: {e}");
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
