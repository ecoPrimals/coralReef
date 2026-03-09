// SPDX-License-Identifier: AGPL-3.0-only
use coral_gpu::{GpuContext, GpuTarget, NvArch};

const SHADER: &str = r#"
@group(0) @binding(0) var<storage, read_write> output: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    output[gid.x] = gid.x * gid.x;
}
"#;

fn main() {
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║         coralReef — Hello Compiler                  ║");
    println!("║  Sovereign GPU compiler: WGSL → native binary       ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!();

    println!("Input WGSL shader ({} bytes):", SHADER.len());
    println!("  Workgroup size: 64");
    println!("  Operation: output[i] = i * i");
    println!();

    let target = GpuTarget::Nvidia(NvArch::Sm86);
    let ctx = GpuContext::new(target).expect("compiler init");

    println!("Compiling for {target}...");
    let kernel = ctx.compile_wgsl(SHADER).expect("compilation");

    println!();
    println!("Compilation result:");
    println!("  Target:         {}", kernel.target);
    println!("  Binary size:    {} bytes", kernel.binary.len());
    println!("  GPR count:      {}", kernel.gpr_count);
    println!("  Instructions:   {}", kernel.instr_count);
    println!("  Shared memory:  {} bytes", kernel.shared_mem_bytes);
    println!("  Barriers:       {}", kernel.barrier_count);
    println!(
        "  Workgroup:      {}x{}x{}",
        kernel.workgroup[0], kernel.workgroup[1], kernel.workgroup[2]
    );
    println!();

    print!("  First 32 bytes: ");
    for b in kernel.binary.iter().take(32) {
        print!("{b:02x} ");
    }
    println!();
    println!();
    println!("No GPU hardware needed. No vendor SDK. Pure Rust compilation.");
}
