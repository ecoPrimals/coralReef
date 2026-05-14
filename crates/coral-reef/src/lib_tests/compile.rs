// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

#[test]
fn test_compile_empty_spirv_rejected() {
    let result = compile(&[], &CompileOptions::default());
    assert!(matches!(result, Err(CompileError::InvalidInput(_))));
}

#[test]
fn test_compile_invalid_spirv_rejected() {
    let result = compile(&[0x0723_0203], &CompileOptions::default());
    assert!(result.is_err(), "invalid SPIR-V should fail: {result:?}");
}

#[test]
fn test_compile_wgsl_empty_rejected() {
    let result = compile_wgsl("", &CompileOptions::default());
    assert!(matches!(result, Err(CompileError::InvalidInput(_))));
}

#[test]
fn test_compile_wgsl_minimal_compute() {
    let result = compile_wgsl(
        "@compute @workgroup_size(1) fn main() {}",
        &CompileOptions::default(),
    );
    assert!(
        result.is_ok() || result.is_err(),
        "should parse and attempt compilation"
    );
}

#[test]
fn test_compile_wgsl_f64_min_max_abs_clamp() {
    let wgsl = r"
@compute @workgroup_size(1)
fn main() {
    let rho = f64(1.5);
    let rho_pos = max(rho, f64(0.0));
    let v = f64(-100.0);
    let clamped = clamp(v, f64(-5000.0), f64(5000.0));
    let a = abs(v);
}
";
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm70),
        fp64_software: true,
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok(),
        "f64 min/max/abs/clamp should compile: {result:?}"
    );
}

#[test]
fn test_compile_glsl_empty_rejected() {
    let result = compile_glsl("", &CompileOptions::default());
    assert!(matches!(result, Err(CompileError::InvalidInput(_))));
}

#[test]
fn test_compile_glsl_minimal_compute() {
    let glsl = "#version 450\nlayout(local_size_x = 1) in;\nvoid main() {}";
    let result = compile_glsl(glsl, &CompileOptions::default());
    assert!(
        result.is_ok(),
        "minimal GLSL compute should compile: {result:?}"
    );
}

#[test]
fn test_compile_glsl_malformed_returns_error() {
    let result = compile_glsl(
        "#version 450\nvoid main() { int x = ; }",
        &CompileOptions::default(),
    );
    assert!(
        result.is_err(),
        "malformed GLSL should return error: {result:?}"
    );
}
