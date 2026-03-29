// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]
use coral_gpu::DriverPreference;

fn main() {
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║       coralReef — Driver Sovereignty                ║");
    println!("║  Prefer open-source, fall back to what exists       ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!();

    println!("Driver Preference Modes:");
    println!();

    let sovereign = DriverPreference::sovereign();
    println!("  Sovereign (default):");
    println!("    Order: {}", sovereign.order().join(" → "));
    println!("    Philosophy: Force deep understanding. Own every ioctl.");
    println!();

    let pragmatic = DriverPreference::pragmatic();
    println!("  Pragmatic:");
    println!("    Order: {}", pragmatic.order().join(" → "));
    println!("    Philosophy: Use what's already installed. Maximize compatibility.");
    println!();

    let active = DriverPreference::from_env();
    let source = match std::env::var("CORALREEF_DRIVER_PREFERENCE") {
        Ok(val) if !val.is_empty() => format!("CORALREEF_DRIVER_PREFERENCE={val}"),
        _ => "default (sovereign)".to_string(),
    };
    println!("  Active preference:");
    println!("    Source: {source}");
    println!("    Order:  {}", active.order().join(" → "));
    println!();

    println!("Selection Simulation:");
    println!();

    let scenarios: Vec<(&str, Vec<&str>)> = vec![
        ("Typical AMD system", vec!["amdgpu"]),
        ("NVIDIA proprietary only", vec!["nvidia-drm"]),
        ("NVIDIA with nouveau loaded", vec!["nouveau", "nvidia-drm"]),
        (
            "Multi-GPU (AMD + NVIDIA)",
            vec!["amdgpu", "nvidia-drm"],
        ),
        (
            "Full multi-GPU",
            vec!["amdgpu", "nouveau", "nvidia-drm"],
        ),
        ("Intel-only system", vec!["i915"]),
    ];

    for (name, available) in &scenarios {
        let selected_sov = sovereign.select(available);
        let selected_prag = pragmatic.select(available);

        println!("  {name}");
        println!("    Available: [{}]", available.join(", "));
        println!(
            "    Sovereign selects:  {}",
            selected_sov.unwrap_or("(none — no supported driver)")
        );
        println!(
            "    Pragmatic selects:  {}",
            selected_prag.unwrap_or("(none — no supported driver)")
        );
        println!();
    }

    #[cfg(target_os = "linux")]
    {
        println!("Live System Scan:");
        println!();

        let nodes = coral_driver::drm::enumerate_render_nodes();
        if nodes.is_empty() {
            println!("  No DRM render nodes found (headless or non-GPU system).");
        } else {
            for node in &nodes {
                println!(
                    "  {} — driver: {}, version: {}.{}",
                    node.path, node.driver, node.version_major, node.version_minor
                );
            }
            let drivers: Vec<&str> = nodes.iter().map(|n| n.driver.as_str()).collect();
            let selected = active.select(&drivers);
            println!();
            println!(
                "  Active selection: {}",
                selected.unwrap_or("(none matched preference)")
            );
        }
        println!();
    }

    println!("The compiled shader binary is identical regardless of driver.");
    println!("Only the dispatch path differs. Sovereignty is a runtime choice.");
}
