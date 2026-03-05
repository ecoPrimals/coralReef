// SPDX-License-Identifier: AGPL-3.0-only
//! Integration tests for the coral-nak compiler pipeline.

use coral_nak::{CompileError, CompileOptions, GpuArch, compile, compile_wgsl};

#[test]
fn test_empty_spirv_returns_invalid_input() {
    let result = compile(&[], &CompileOptions::default());
    assert!(matches!(result, Err(CompileError::InvalidInput(_))));
}

#[test]
fn test_compile_reports_not_implemented() {
    let spirv_header = [0x0723_0203_u32, 0x0001_0000, 0, 0, 0];
    let result = compile(&spirv_header, &CompileOptions::default());
    assert!(
        matches!(result, Err(CompileError::NotImplemented(_))),
        "pipeline should report not-implemented until Phase 2 completes"
    );
}

#[test]
fn test_wgsl_empty_returns_invalid_input() {
    let result = compile_wgsl("", &CompileOptions::default());
    assert!(matches!(result, Err(CompileError::InvalidInput(_))));
}

#[test]
fn test_wgsl_compile_reports_not_implemented() {
    let result = compile_wgsl(
        "@compute @workgroup_size(1) fn main() {}",
        &CompileOptions::default(),
    );
    assert!(matches!(result, Err(CompileError::NotImplemented(_))));
}

#[test]
fn test_all_archs_accept_input() {
    let spirv = [0x0723_0203_u32];
    for &arch in GpuArch::ALL {
        let opts = CompileOptions {
            arch,
            opt_level: 2,
            debug_info: false,
            fp64_software: true,
        };
        let result = compile(&spirv, &opts);
        assert!(
            result.is_err(),
            "arch {arch} should return error (not-implemented)"
        );
    }
}

#[test]
fn test_compile_options_opt_levels() {
    let spirv = [0x0723_0203_u32];
    for level in 0..=3 {
        let opts = CompileOptions {
            opt_level: level,
            ..CompileOptions::default()
        };
        let result = compile(&spirv, &opts);
        assert!(result.is_err());
    }
}
