// SPDX-License-Identifier: AGPL-3.0-only
use coral_gpu::{AmdArch, GpuContext, GpuTarget, NvArch};

const SIMPLE_SHADER: &str = r#"
@group(0) @binding(0) var<storage, read_write> output: array<u32>;

@compute @workgroup_size(64)
fn main() {
    output[0] = 42u;
}
"#;

const MATH_SHADER: &str = r#"
@group(0) @binding(0) var<storage, read_write> output: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    output[gid.x] = gid.x * gid.x + 1u;
}
"#;

struct TargetResult {
    target: GpuTarget,
    simple_ok: bool,
    simple_bytes: usize,
    math_ok: bool,
    math_bytes: usize,
    note: &'static str,
}

fn try_compile(target: GpuTarget, wgsl: &str) -> (bool, usize) {
    let ctx = match GpuContext::new(target) {
        Ok(c) => c,
        Err(_) => return (false, 0),
    };
    match ctx.compile_wgsl(wgsl) {
        Ok(k) => (true, k.binary.len()),
        Err(_) => (false, 0),
    }
}

fn main() {
    println!("╔══════════════════════════════════════════════════════╗");
    println!("║       coralReef — Multi-Target Compilation          ║");
    println!("║  One WGSL source → every supported GPU ISA          ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!();

    let targets: Vec<(GpuTarget, &str)> = vec![
        (GpuTarget::Nvidia(NvArch::Sm70), ""),
        (GpuTarget::Nvidia(NvArch::Sm75), ""),
        (GpuTarget::Nvidia(NvArch::Sm80), ""),
        (GpuTarget::Nvidia(NvArch::Sm86), ""),
        (GpuTarget::Nvidia(NvArch::Sm89), ""),
        (
            GpuTarget::Amd(AmdArch::Rdna2),
            "global_invocation_id not yet lowered",
        ),
        (GpuTarget::Amd(AmdArch::Rdna3), ""),
        (GpuTarget::Amd(AmdArch::Rdna4), ""),
    ];

    let mut results = Vec::new();

    for (target, note) in &targets {
        let (s_ok, s_bytes) = try_compile(*target, SIMPLE_SHADER);
        let (m_ok, m_bytes) = try_compile(*target, MATH_SHADER);
        results.push(TargetResult {
            target: *target,
            simple_ok: s_ok,
            simple_bytes: s_bytes,
            math_ok: m_ok,
            math_bytes: m_bytes,
            note,
        });
    }

    println!(
        "  {:<20} {:>10} {:>10}  {:>10} {:>10}  {}",
        "Target", "Simple", "Bytes", "Math", "Bytes", "Notes"
    );
    println!("  {}", "─".repeat(78));

    for r in &results {
        let simple_status = if r.simple_ok { "PASS" } else { "FAIL" };
        let math_status = if r.math_ok { "PASS" } else { "FAIL" };

        let simple_bytes = if r.simple_ok {
            format!("{}", r.simple_bytes)
        } else {
            "—".to_string()
        };
        let math_bytes = if r.math_ok {
            format!("{}", r.math_bytes)
        } else {
            "—".to_string()
        };

        println!(
            "  {:<20} {:>10} {:>10}  {:>10} {:>10}  {}",
            format!("{}", r.target),
            simple_status,
            simple_bytes,
            math_status,
            math_bytes,
            r.note
        );
    }

    let total = results.len();
    let simple_pass = results.iter().filter(|r| r.simple_ok).count();
    let math_pass = results.iter().filter(|r| r.math_ok).count();

    println!();
    println!("  Simple shader: {simple_pass}/{total} targets compile");
    println!("  Math shader:   {math_pass}/{total} targets compile");
    println!();

    if simple_pass < total || math_pass < total {
        println!("  Known limitations are documented — each represents a");
        println!("  deep debt evolution opportunity for the compiler.");
    }

    println!();
    println!("  All compilation is pure Rust. No CUDA toolkit. No ROCm. No Vulkan.");
}
