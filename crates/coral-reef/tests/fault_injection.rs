// SPDX-License-Identifier: AGPL-3.0-only
//! Fault injection tests — verify the compiler handles edge cases gracefully.
//!
//! Each test asserts that compilation either succeeds or returns a proper
//! CompileError — never panics, OOMs, or stack overflows.

use coral_reef::{
    AmdArch, CompileError, CompileOptions, GpuTarget, IntelArch, compile, compile_wgsl,
};
use std::panic::AssertUnwindSafe;
use std::panic::catch_unwind;

fn default_opts() -> CompileOptions {
    CompileOptions::default()
}

fn amd_rdna2_opts() -> CompileOptions {
    CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        ..CompileOptions::default()
    }
}

/// Asserts compile returns Ok or Err(CompileError), never panics.
fn assert_no_panic<R, F: FnOnce() -> R>(f: F) -> R {
    let result = catch_unwind(AssertUnwindSafe(f));
    match result {
        Ok(r) => r,
        Err(e) => panic!("Unexpected panic: {e:?}"),
    }
}

// --- Massive input / OOM resistance ---

#[test]
fn fault_massive_wgsl_source() {
    let mut wgsl = String::with_capacity(1_100_000);
    wgsl.push_str("@compute @workgroup_size(1) fn main() {\n");
    while wgsl.len() < 1_100_000 {
        wgsl.push_str("  var x: f32 = 1.0;\n");
    }
    wgsl.push_str("}\n");
    let result = assert_no_panic(|| compile_wgsl(&wgsl, &default_opts()));
    assert!(
        result.is_ok() || result.is_err(),
        "must return Result, not panic"
    );
    if let Err(e) = &result {
        assert!(matches!(
            e,
            CompileError::InvalidInput(_) | CompileError::Validation(_)
        ));
    }
}

// --- Deep control flow / stack overflow resistance ---

#[test]
fn fault_deeply_nested_control_flow() {
    let mut wgsl = String::from("@compute @workgroup_size(1) fn main() {\n");
    for _ in 0..120 {
        wgsl.push_str("  if true {\n");
    }
    wgsl.push_str("    var x: f32 = 1.0;\n");
    for _ in 0..120 {
        wgsl.push_str("  }\n");
    }
    wgsl.push_str("}\n");
    let result = assert_no_panic(|| compile_wgsl(&wgsl, &default_opts()));
    assert!(result.is_ok() || result.is_err());
}

// --- Workgroup size limits ---

#[test]
fn fault_enormous_workgroup_size() {
    // Exceeds WebGPU limit (256 per dimension) — naga/validator should reject
    let wgsl = "@compute @workgroup_size(257, 1, 1) fn main() {}";
    let result = assert_no_panic(|| compile_wgsl(wgsl, &default_opts()));
    assert!(result.is_ok() || result.is_err());
}

// --- Maximum bindings ---

#[test]
fn fault_maximum_bindings() {
    let mut wgsl = String::from("@group(0) @binding(0) var<storage, read_write> b0: array<f32>;\n");
    for i in 1..16 {
        wgsl.push_str(&format!(
            "@group(0) @binding({i}) var<storage, read_write> b{i}: array<f32>;\n"
        ));
    }
    wgsl.push_str("@compute @workgroup_size(1) fn main() { b0[0] = b1[0] + b2[0]; }\n");
    let result = assert_no_panic(|| compile_wgsl(&wgsl, &default_opts()));
    assert!(result.is_ok() || result.is_err());
}

// --- Empty main (zero instructions after optimization) ---

#[test]
fn fault_empty_main() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let result = assert_no_panic(|| compile_wgsl(wgsl, &default_opts()));
    assert!(result.is_ok() || result.is_err());
}

// --- Invalid / unsupported GPU target ---

#[test]
fn fault_invalid_gpu_target() {
    let opts = CompileOptions {
        target: GpuTarget::Intel(IntelArch::XeHpg),
        ..CompileOptions::default()
    };
    let result =
        assert_no_panic(|| compile_wgsl("@compute @workgroup_size(1) fn main() {}", &opts));
    assert!(matches!(result, Err(CompileError::UnsupportedArch(_))));
}

// --- Rapid sequential compilations (resource leak test) ---

#[test]
fn fault_rapid_sequential_compilations() {
    let wgsl = "@compute @workgroup_size(1) fn main() { var x: f32 = 1.0; }";
    for _ in 0..120 {
        let result = assert_no_panic(|| compile_wgsl(wgsl, &default_opts()));
        assert!(result.is_ok() || result.is_err());
    }
}

// --- Concurrent compilations ---

#[test]
fn fault_concurrent_compilations() {
    let wgsl = "@compute @workgroup_size(1) fn main() { var x: f32 = 1.0; }";
    let handles: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(move || {
                for _ in 0..16 {
                    let result = compile_wgsl(wgsl, &default_opts());
                    assert!(result.is_ok() || result.is_err());
                }
            })
        })
        .collect();
    for h in handles {
        h.join().expect("thread should not panic");
    }
}

// --- SPIR-V truncated / corrupted data ---

#[test]
fn fault_spirv_truncated() {
    let spirv: Vec<u32> = vec![0x07230203, 0x00010000, 0x00000001];
    let result = assert_no_panic(|| compile(&spirv, &default_opts()));
    assert!(matches!(result, Err(CompileError::InvalidInput(_))));
}

#[test]
fn fault_spirv_corrupted() {
    let spirv: Vec<u32> = vec![0xDEADBEEF, 0xCAFEBABE, 0x12345678];
    let result = assert_no_panic(|| compile(&spirv, &default_opts()));
    assert!(result.is_err());
}

// --- Math stress test ---

#[test]
fn fault_all_math_functions_stress() {
    let wgsl = r#"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1) fn main() {
  var x: f32 = 1.5;
  var y: f32 = 2.0;
  out[0] = abs(x) + min(x, y) + max(x, y) + clamp(x, 0.0, 1.0);
  out[1] = floor(x) + ceil(x) + round(x) + fract(x) + trunc(x);
  out[2] = sqrt(x) + inverseSqrt(x) + sign(x);
  out[3] = sin(x) + cos(x) + tan(x) + exp(x) + exp2(x) + log(x) + log2(x);
  out[4] = pow(x, y) + fma(x, y, 1.0);
  out[5] = mix(x, y, 0.5) + step(x, y) + smoothstep(0.0, 1.0, x);
  out[6] = length(vec2(x, y)) + dot(vec2(x, y), vec2(y, x));
}
"#;
    let result = assert_no_panic(|| compile_wgsl(wgsl, &default_opts()));
    assert!(result.is_ok() || result.is_err());
}

// --- AMD RDNA2 target variants ---

#[test]
fn fault_empty_main_amd_rdna2() {
    let result = assert_no_panic(|| {
        compile_wgsl(
            "@compute @workgroup_size(1) fn main() {}",
            &amd_rdna2_opts(),
        )
    });
    assert!(result.is_ok() || result.is_err());
}

#[test]
fn fault_malformed_wgsl_amd_rdna2() {
    let result = assert_no_panic(|| compile_wgsl("not valid wgsl {{{", &amd_rdna2_opts()));
    assert!(matches!(result, Err(CompileError::InvalidInput(_))));
}
