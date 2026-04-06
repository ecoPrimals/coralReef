// SPDX-License-Identifier: AGPL-3.0-or-later
#![forbid(unsafe_code)]
use coral_gpu::GpuContext;

const STORE_42: &str = r#"
@group(0) @binding(0) var<storage, read_write> output: array<u32>;

@compute @workgroup_size(1)
fn main() {
    output[0] = 42u;
}
"#;

fn main() {
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║       coralReef — Cross-Vendor Parity               ║");
    println!("║  Same shader, different hardware, identical results  ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!();

    #[cfg(not(target_os = "linux"))]
    {
        println!("  Cross-vendor parity requires Linux with DRM render nodes.");
        println!("  Use Level 00 demos for compile-only testing.");
        return;
    }

    #[cfg(target_os = "linux")]
    run_parity();
}

#[cfg(target_os = "linux")]
fn run_parity() {
    let nodes = coral_driver::drm::enumerate_render_nodes();
    if nodes.is_empty() {
        println!("  No DRM render nodes found.");
        println!("  This demo requires GPU hardware.");
        return;
    }

    println!("  Found {} render node(s):", nodes.len());
    for node in &nodes {
        println!("    {} — {}", node.path, node.driver);
    }
    println!();

    let contexts = GpuContext::enumerate_all();
    if contexts.is_empty() {
        println!("  No supported GPU contexts available.");
        return;
    }

    println!("  Testing store_42 shader on each GPU:");
    println!();

    let mut results: Vec<(String, Result<u32, String>)> = Vec::new();

    for ctx_result in contexts {
        let mut ctx = match ctx_result {
            Ok(ctx) => ctx,
            Err(e) => {
                results.push(("(open failed)".to_string(), Err(format!("{e}"))));
                continue;
            }
        };

        let target = format!("{}", ctx.target());

        let result = (|| -> Result<u32, String> {
            let kernel = ctx.compile_wgsl(STORE_42).map_err(|e| format!("compile: {e}"))?;
            let buf = ctx.alloc(256).map_err(|e| format!("alloc: {e}"))?;
            ctx.dispatch(&kernel, &[buf], [1, 1, 1])
                .map_err(|e| format!("dispatch: {e}"))?;
            ctx.sync().map_err(|e| format!("sync: {e}"))?;
            let data = ctx.readback(buf, 4).map_err(|e| format!("readback: {e}"))?;
            ctx.free(buf).map_err(|e| format!("free: {e}"))?;
            Ok(u32::from_le_bytes([data[0], data[1], data[2], data[3]]))
        })();

        results.push((target, result));
    }

    println!("  {:<25} {:>10}  {}", "Target", "Result", "Status");
    println!("  {}", "─".repeat(60));

    let mut all_match = true;
    for (target, result) in &results {
        match result {
            Ok(42) => println!("  {target:<25} {:>10}  PASS", "42"),
            Ok(v) => {
                println!("  {target:<25} {v:>10}  MISMATCH (expected 42)");
                all_match = false;
            }
            Err(e) => {
                println!("  {target:<25} {:>10}  {e}", "—");
                all_match = false;
            }
        }
    }

    println!();
    if all_match && !results.is_empty() {
        println!("  PARITY VERIFIED — all GPUs produced identical results.");
    } else if results.iter().any(|(_, r)| r.is_ok()) {
        println!("  PARTIAL PARITY — some GPUs succeeded, others have known limitations.");
        println!("  Each limitation is a deep debt evolution opportunity.");
    } else {
        println!("  NO RESULTS — check hardware access and driver status.");
    }
}
