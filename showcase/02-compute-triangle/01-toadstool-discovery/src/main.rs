// SPDX-License-Identifier: AGPL-3.0-or-later
#![forbid(unsafe_code)]

fn main() {
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║       coralReef — Ecosystem Discovery               ║");
    println!("║  Capability-based. No hardcoded primal names.       ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!();

    println!("Step 1: Self-Description");
    println!();
    let desc = coralreef_core::capability::self_description();

    println!("  What coralReef provides:");
    for cap in &desc.provides {
        println!("    {} v{}", cap.id, cap.version);
        if !cap.metadata.is_null() {
            if let Some(formats) = cap.metadata.get("input_formats") {
                println!("      Input formats: {formats}");
            }
            if let Some(archs) = cap.metadata.get("architectures") {
                println!("      Architectures: {archs}");
            }
        }
    }
    println!();

    println!("  What coralReef requires:");
    for cap in &desc.requires {
        println!("    {} {}", cap.id, cap.version);
        if let Some(reason) = cap.metadata.get("reason") {
            println!("      Reason: {reason}");
        }
    }
    println!();

    println!("  Transports: (bound at runtime, empty until server starts)");
    if desc.transports.is_empty() {
        println!("    (none — call start_jsonrpc_server / start_tarpc_server)");
    }
    println!();

    println!("Step 2: Ecosystem GPU Discovery");
    println!();

    let discovery_dir = coralreef_core::config::discovery_dir();
    match &discovery_dir {
        Ok(dir) => println!("  Discovery directory: {}", dir.display()),
        Err(e) => println!("  Discovery directory: unavailable ({e})"),
    }
    println!();

    let devices = coralreef_core::discovery::discover_gpu_devices();
    if devices.is_empty() {
        println!("  No GPU devices discovered via ecosystem capabilities.");
        println!("  This is expected when no gpu.dispatch provider is running.");
        println!("  coralReef falls back to direct DRM render node scan.");
    } else {
        println!("  Discovered {} GPU device(s):", devices.len());
        for dev in &devices {
            println!();
            println!("    Vendor:      {}", dev.vendor);
            println!(
                "    Arch:        {}",
                dev.arch.as_deref().unwrap_or("(unknown)")
            );
            println!(
                "    Render node: {}",
                dev.render_node.as_deref().unwrap_or("(unknown)")
            );
            println!(
                "    Driver:      {}",
                dev.driver.as_deref().unwrap_or("(unknown)")
            );
            println!("    Source:       {}", dev.source);
        }
    }
    println!();

    println!("Step 3: Context Creation from Descriptors");
    println!();

    #[cfg(target_os = "linux")]
    {
        if !devices.is_empty() {
            for dev in &devices {
                match coral_gpu::GpuContext::from_descriptor(
                    &dev.vendor,
                    dev.arch.as_deref(),
                    dev.driver.as_deref(),
                ) {
                    Ok(ctx) => {
                        println!("  {} — context created via ecosystem discovery", ctx.target());
                    }
                    Err(e) => {
                        println!(
                            "  {} {} — {e}",
                            dev.vendor,
                            dev.arch.as_deref().unwrap_or("?")
                        );
                    }
                }
            }
        } else {
            println!("  No ecosystem devices — demonstrating DRM fallback...");
            match coral_gpu::GpuContext::auto() {
                Ok(ctx) => println!("  {} — context created via DRM scan", ctx.target()),
                Err(e) => println!("  DRM fallback: {e}"),
            }
        }
    }

    #[cfg(not(target_os = "linux"))]
    println!("  Context creation requires Linux. Compile-only mode on this platform.");

    println!();
    println!("The Compute Triangle (capability-based, no hardcoded primal names):");
    println!();
    println!("  shader.compile  -->  gpu.orchestrate  -->  gpu.dispatch");
    println!("  (this primal)        (discovered)          (discovered)");
    println!();
    println!("  coralReef discovers providers by capability at runtime.");
    println!("  It never knows WHO provides them — only WHAT it needs.");
}
