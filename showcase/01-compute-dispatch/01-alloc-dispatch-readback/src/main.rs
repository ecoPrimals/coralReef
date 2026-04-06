// SPDX-License-Identifier: AGPL-3.0-or-later
#![forbid(unsafe_code)]
use coral_gpu::GpuContext;

const STORE_42_SHADER: &str = r#"
@group(0) @binding(0) var<storage, read_write> output: array<u32>;

@compute @workgroup_size(1)
fn main() {
    output[0] = 42u;
}
"#;

fn main() {
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║       coralReef — Alloc → Dispatch → Readback      ║");
    println!("║  Full sovereign GPU compute cycle                   ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!();

    println!("Step 1: Auto-detect GPU...");
    let mut ctx = match GpuContext::auto() {
        Ok(ctx) => {
            println!("  Found: {}", ctx.target());
            ctx
        }
        Err(e) => {
            println!("  No GPU available: {e}");
            println!();
            println!("  This demo requires GPU hardware.");
            println!("  Level 00 demos work without hardware (compile-only).");
            return;
        }
    };
    println!();

    println!("Step 2: Compile WGSL → native binary...");
    let kernel = ctx.compile_wgsl(STORE_42_SHADER).expect("compilation");
    println!("  Compiled: {} bytes, {} GPRs", kernel.binary.len(), kernel.gpr_count);
    println!();

    println!("Step 3: Allocate GPU buffer (256 bytes)...");
    let buf = ctx.alloc(256).expect("alloc");
    println!("  Buffer allocated: {buf:?}");
    println!();

    println!("Step 4: Dispatch compute shader...");
    ctx.dispatch(&kernel, &[buf], [1, 1, 1]).expect("dispatch");
    println!("  Dispatched with grid [1, 1, 1]");
    println!();

    println!("Step 5: Sync (wait for GPU)...");
    ctx.sync().expect("sync");
    println!("  GPU work complete");
    println!();

    println!("Step 6: Readback results...");
    let data = ctx.readback(buf, 4).expect("readback");
    let value = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    println!("  output[0] = {value}");
    println!();

    if value == 42 {
        println!("  PASS — GPU computed the correct result.");
    } else {
        println!("  UNEXPECTED — expected 42, got {value}.");
        println!("  This may indicate a driver or compiler issue.");
    }

    println!();
    ctx.free(buf).expect("free");
    println!("  Buffer freed. Full cycle complete.");
    println!();
    println!("No Vulkan. No wgpu. No vendor SDK. Pure Rust → DRM → GPU.");
}
