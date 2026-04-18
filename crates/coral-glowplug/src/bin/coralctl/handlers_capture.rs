// SPDX-License-Identifier: AGPL-3.0-or-later
//! Training recipe capture handlers.
//!
//! `capture training` routes through glowplug for full lifecycle orchestration
//! (cold snapshot → warm driver bind → oracle capture → diff → save recipe).
//! `capture compare` loads two recipe files and analyzes pattern overlap.

use crate::rpc::{check_rpc_error, rpc_call};
use coral_glowplug::capture::TrainingRecipe;

/// `capture.training` — capture a training recipe via glowplug.
pub(crate) fn rpc_capture_training(
    glowplug_socket: &str,
    bdf: &str,
    warm_driver: Option<&str>,
) {
    println!("================================================================");
    println!("  CAPTURE TRAINING — Trace-Capture-Replay Pipeline");
    println!("================================================================");
    println!("  BDF:      {bdf}");
    println!("  Driver:   {}", warm_driver.unwrap_or("auto-detect"));
    println!("  Route:    coralctl -> glowplug -> ember");
    println!("  Flow:     cold snap -> warm driver -> oracle -> diff -> recipe");
    println!("================================================================\n");

    let mut params = serde_json::json!({"bdf": bdf});
    if let Some(driver) = warm_driver {
        params["warm_driver"] = serde_json::Value::String(driver.to_string());
    }

    let response = rpc_call(glowplug_socket, "capture.training", params);
    check_rpc_error(&response);

    if let Some(result) = response.get("result") {
        let success = result["success"].as_bool().unwrap_or(false);
        let summary = result["summary"].as_str().unwrap_or("unknown");
        let total_writes = result["total_writes"].as_u64().unwrap_or(0);
        let recipe_path = result["recipe_path"].as_str();

        println!("Result: {}", if success { "SUCCESS" } else { "FAILED" });
        println!("Summary: {summary}");

        if let Some(path) = recipe_path {
            println!("Recipe: {path}");
        }
        println!("Training writes: {total_writes}");

        if let Some(steps) = result["steps"].as_array() {
            println!("\nSteps:");
            for step in steps {
                let name = step["name"].as_str().unwrap_or("?");
                let status = step["status"].as_str().unwrap_or("?");
                let detail = step["detail"].as_str().unwrap_or("");
                let ms = step["duration_ms"].as_u64().unwrap_or(0);
                let icon = match status {
                    "ok" => "+",
                    "skipped" => "-",
                    _ => "!",
                };
                println!("  [{icon}] {name} ({ms}ms) {detail}");
            }
        }

        if !success {
            std::process::exit(1);
        }
    }
}

/// Compare two training recipes and identify cross-generational patterns.
pub(crate) fn compare_recipes(left_path: &str, right_path: &str) {
    let left = match TrainingRecipe::load(std::path::Path::new(left_path)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: cannot load left recipe: {e}");
            std::process::exit(1);
        }
    };
    let right = match TrainingRecipe::load(std::path::Path::new(right_path)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: cannot load right recipe: {e}");
            std::process::exit(1);
        }
    };

    println!("================================================================");
    println!("  TRAINING RECIPE COMPARISON");
    println!("================================================================");
    println!("  Left:  {} ({}, {} writes)", left.chip, left.warm_driver, left.total_writes);
    println!("  Right: {} ({}, {} writes)", right.chip, right.warm_driver, right.total_writes);
    println!("================================================================\n");

    // Per-domain comparison
    println!("Domain write counts:");
    println!("  {:<12} {:>8} {:>8}", "Domain", &left.chip, &right.chip);
    println!("  {:-<12} {:->8} {:->8}", "", "", "");

    let all_domains: std::collections::BTreeSet<&str> = left
        .training_writes
        .iter()
        .chain(right.training_writes.iter())
        .map(|d| d.name.as_str())
        .collect();

    for domain in &all_domains {
        let left_count = left
            .training_writes
            .iter()
            .find(|d| d.name == *domain)
            .map(|d| d.registers.len())
            .unwrap_or(0);
        let right_count = right
            .training_writes
            .iter()
            .find(|d| d.name == *domain)
            .map(|d| d.registers.len())
            .unwrap_or(0);
        let indicator = if left_count > 0 && right_count > 0 {
            "COMMON"
        } else {
            ""
        };
        println!("  {:<12} {:>8} {:>8}  {indicator}", domain, left_count, right_count);
    }

    // Overlapping register offsets
    let left_offsets: std::collections::HashSet<usize> = left
        .flat_writes()
        .iter()
        .map(|(off, _)| *off)
        .collect();
    let right_offsets: std::collections::HashSet<usize> = right
        .flat_writes()
        .iter()
        .map(|(off, _)| *off)
        .collect();
    let common_offsets: Vec<usize> = left_offsets
        .intersection(&right_offsets)
        .copied()
        .collect();

    println!("\nOverlap analysis:");
    println!("  Left-only offsets:  {}", left_offsets.len() - common_offsets.len());
    println!("  Right-only offsets: {}", right_offsets.len() - common_offsets.len());
    println!("  Common offsets:     {}", common_offsets.len());

    // PLL/timing pattern detection
    let pll_ranges: &[(&str, usize, usize)] = &[
        ("PCLOCK_PLL", 0x137000, 0x138000),
        ("ROOT_PLL", 0x136000, 0x137000),
        ("MEMPLL", 0x137100, 0x137200),
        ("NVPLL", 0x137050, 0x1370A0),
        ("FBPA_TIMING", 0x9A0080, 0x9A00A0),
    ];

    println!("\nPLL/Timing pattern detection:");
    for &(name, start, end) in pll_ranges {
        let left_hits: Vec<_> = left.flat_writes().into_iter().filter(|(o, _)| *o >= start && *o < end).collect();
        let right_hits: Vec<_> = right.flat_writes().into_iter().filter(|(o, _)| *o >= start && *o < end).collect();
        if !left_hits.is_empty() || !right_hits.is_empty() {
            println!("  {name}:");
            println!("    {}: {} writes", left.chip, left_hits.len());
            println!("    {}: {} writes", right.chip, right_hits.len());
            let common_in_range: usize = left_hits
                .iter()
                .filter(|(o, _)| right_hits.iter().any(|(ro, _)| ro == o))
                .count();
            if common_in_range > 0 {
                println!("    Common offsets: {common_in_range}");
            }
        }
    }

    println!("\nConclusion:");
    if common_offsets.len() > 10 {
        println!("  Significant register overlap ({} common offsets) — shared training", common_offsets.len());
        println!("  patterns likely exist. A native Rust training engine can target these.");
    } else if !common_offsets.is_empty() {
        println!("  Some overlap ({} common offsets) — architectures share a few patterns.", common_offsets.len());
    } else {
        println!("  No overlap — these GPU families use entirely different register layouts.");
    }
}

/// Decode PLL and timing patterns from a training recipe.
pub(crate) fn decode_recipe(file_path: &str) {
    let recipe = match TrainingRecipe::load(std::path::Path::new(file_path)) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: cannot load recipe: {e}");
            std::process::exit(1);
        }
    };

    println!("================================================================");
    println!("  TRAINING RECIPE DECODE — {}", recipe.chip);
    println!("================================================================");
    println!("  Source: {file_path}");
    println!("  Chip:   {}", recipe.chip);
    println!("  Driver: {}", recipe.warm_driver);
    println!("  Total writes: {}", recipe.total_writes);
    println!("================================================================\n");

    println!("Domains:");
    for domain in &recipe.training_writes {
        println!("  {:<12} {} writes", domain.name, domain.registers.len());
        for &(off, val) in domain.registers.iter().take(5) {
            println!("    {off:#010x} = {val:#010x}");
        }
        if domain.registers.len() > 5 {
            println!("    ... and {} more", domain.registers.len() - 5);
        }
    }
}

/// List all training recipes in the training directory.
pub(crate) fn list_recipes() {
    let training_dir = coral_glowplug::capture::training_dir();

    println!("Training recipes in {}:", training_dir.display());
    println!();

    let entries = match std::fs::read_dir(&training_dir) {
        Ok(e) => e,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::NotFound {
                println!("  (no recipes captured yet)");
                println!("  Run: coralctl capture training <bdf>");
            } else {
                eprintln!("  error reading {}: {e}", training_dir.display());
            }
            return;
        }
    };

    let mut found = false;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            match TrainingRecipe::load(&path) {
                Ok(recipe) => {
                    found = true;
                    println!("  {} — {} ({} writes via {}, captured {})",
                        path.file_name().unwrap_or_default().to_string_lossy(),
                        recipe.chip,
                        recipe.total_writes,
                        recipe.warm_driver,
                        recipe.timestamp,
                    );
                }
                Err(e) => {
                    println!("  {} — error: {e}",
                        path.file_name().unwrap_or_default().to_string_lossy(),
                    );
                }
            }
        }
    }

    if !found {
        println!("  (no recipes captured yet)");
        println!("  Run: coralctl capture training <bdf>");
    }
}
