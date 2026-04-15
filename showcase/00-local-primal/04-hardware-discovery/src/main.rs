// SPDX-License-Identifier: AGPL-3.0-or-later
#![forbid(unsafe_code)]

fn main() {
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║       coralReef — Hardware Discovery                ║");
    println!("║  DRM scan + ecosystem discovery, no vendor SDK      ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!();

    println!("Layer 1: DRM Render Node Scan");
    println!();

    #[cfg(target_os = "linux")]
    {
        let nodes = coral_driver::drm::enumerate_render_nodes();
        if nodes.is_empty() {
            println!("  No render nodes found. This system may not have a GPU,");
            println!("  or the current user lacks /dev/dri/ access.");
        } else {
            println!("  Found {} render node(s):", nodes.len());
            println!();
            for node in &nodes {
                println!("  {} ", node.path);
                println!("    Driver:  {}", node.driver);
                println!(
                    "    Version: {}.{}",
                    node.version_major, node.version_minor
                );
                let vendor = match node.driver.as_str() {
                    "amdgpu" => "AMD (open-source)",
                    "nouveau" => "NVIDIA (open-source, sovereign)",
                    "nvidia-drm" => "NVIDIA (proprietary, compatible)",
                    "i915" | "xe" => "Intel",
                    other => other,
                };
                println!("    Vendor:  {vendor}");
                println!();
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        println!("  DRM render nodes are Linux-only.");
        println!("  On this platform, coralReef operates in compile-only mode.");
        println!();
    }

    println!("Layer 2: Ecosystem Discovery (capability: gpu.dispatch)");
    println!();

    let devices = coralreef_core::discovery::discover_gpu_devices();
    if devices.is_empty() {
        println!("  No ecosystem capability files found.");
        println!("  No gpu.dispatch provider is running or has published capabilities.");
        println!("  coralReef falls back to direct DRM scan (Layer 1).");
    } else {
        println!("  Discovered {} device(s) via ecosystem:", devices.len());
        println!();
        for dev in &devices {
            println!(
                "  {} / {}",
                dev.vendor,
                dev.arch.as_deref().unwrap_or("(unknown)")
            );
            if let Some(ref node) = dev.render_node {
                println!("    Render node: {node}");
            }
            if let Some(ref driver) = dev.driver {
                println!("    Driver:      {driver}");
            }
            if let Some(mem) = dev.memory_bytes {
                println!("    Memory:      {} MiB", mem / (1024 * 1024));
            }
            println!("    Source:       {}", dev.source);
            println!();
        }
    }

    println!();
    println!("Layer 3: Unified GPU Context");
    println!();

    #[cfg(target_os = "linux")]
    {
        let all = coral_gpu::GpuContext::enumerate_all();
        if all.is_empty() {
            println!("  No supported GPUs available for compute.");
            println!("  coralReef can still compile shaders (compile-only mode).");
        } else {
            println!("  {} GPU context(s) available:", all.len());
            for result in &all {
                match result {
                    Ok(ctx) => println!("    {} — ready", ctx.target()),
                    Err(e) => println!("    error: {e}"),
                }
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    println!("  Compile-only mode (non-Linux).");

    println!();
    println!("No nvidia-smi. No rocm-smi. No lspci parsing.");
    println!("Pure DRM ioctls and ecosystem capability discovery.");
}
