// SPDX-License-Identifier: AGPL-3.0-only
//! coralctl handlers for trace parsing, recipe application, and devinit replay.
//!
//! These absorb functionality that previously lived in Python scripts
//! (`parse_mmiotrace.py`, `apply_recipe.py`, `replay_devinit.py`) into
//! the coralctl CLI, backed by coral-driver's Rust implementations.

use std::path::Path;

use coral_driver::vfio::channel::diagnostic::boot_follower::BootTrace;
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
