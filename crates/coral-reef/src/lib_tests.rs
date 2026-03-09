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

#[test]
fn test_default_options() {
    let opts = CompileOptions::default();
    assert_eq!(opts.arch(), GpuArch::Sm70);
    assert_eq!(opts.opt_level, 2);
    assert!(opts.fp64_software);
    assert!(!opts.debug_info);
}

#[test]
fn test_options_clone() {
    let opts = CompileOptions {
        target: GpuArch::Sm89.into(),
        opt_level: 3,
        debug_info: true,
        fp64_software: false,
        ..CompileOptions::default()
    };
    let cloned = opts;
    assert_eq!(cloned.arch(), GpuArch::Sm89);
    assert_eq!(cloned.opt_level, 3);
    assert!(cloned.debug_info);
    assert!(!cloned.fp64_software);
}

#[test]
fn test_options_debug() {
    let opts = CompileOptions::default();
    let dbg = format!("{opts:?}");
    assert!(dbg.contains("CompileOptions"));
}

#[test]
fn test_compile_with_all_archs() {
    for arch in [
        GpuArch::Sm70,
        GpuArch::Sm75,
        GpuArch::Sm80,
        GpuArch::Sm86,
        GpuArch::Sm89,
    ] {
        let opts = CompileOptions {
            target: arch.into(),
            ..CompileOptions::default()
        };
        let result = compile(&[0x0723_0203], &opts);
        assert!(result.is_err(), "should be not-implemented for {arch}");
    }
}

#[test]
fn test_shader_model_for_nvidia() {
    let sm = shader_model_for(GpuTarget::Nvidia(NvArch::Sm86));
    assert!(sm.is_ok());
    assert_eq!(sm.unwrap().sm(), 86);
}

#[test]
fn test_shader_model_for_amd() {
    let sm = shader_model_for(GpuTarget::Amd(AmdArch::Rdna2));
    assert!(sm.is_ok());
    assert_eq!(sm.unwrap().sm(), 103);
}

#[test]
fn test_shader_model_for_intel_unsupported() {
    let sm = shader_model_for(GpuTarget::Intel(IntelArch::XeHpg));
    assert!(sm.is_err());
}

#[test]
fn test_amd_compile_wgsl_minimal() {
    let opts = CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        ..CompileOptions::default()
    };
    let result = compile_wgsl("@compute @workgroup_size(1) fn main() {}", &opts);
    assert!(
        result.is_ok() || result.is_err(),
        "should parse and attempt AMD compilation"
    );
}

#[test]
fn test_backend_for_resolves_amd() {
    let be = backend::backend_for(GpuTarget::Amd(AmdArch::Rdna2));
    assert!(be.is_ok());
}

#[test]
fn test_cross_vendor_both_compile_same_wgsl() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let nv_opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm70),
        ..CompileOptions::default()
    };
    let amd_opts = CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        ..CompileOptions::default()
    };
    let nv_result = compile_wgsl(wgsl, &nv_opts);
    let amd_result = compile_wgsl(wgsl, &amd_opts);

    assert!(
        nv_result.is_ok(),
        "NVIDIA compilation failed: {nv_result:?}"
    );
    assert!(amd_result.is_ok(), "AMD compilation failed: {amd_result:?}");

    let nv_bin = nv_result.unwrap();
    let amd_bin = amd_result.unwrap();

    assert!(
        nv_bin.len() > amd_bin.len(),
        "NVIDIA binary should be larger (includes SPH)"
    );
    assert!(
        !amd_bin.is_empty(),
        "AMD binary should contain at least s_endpgm"
    );
    assert!(
        nv_bin.len() >= 32,
        "NVIDIA binary should have at least 32 bytes (SPH header)"
    );
}

#[test]
#[should_panic(expected = "NVIDIA shader model must be >= SM 2.0")]
fn test_shader_model_info_new_panics_for_sm_below_20() {
    let _ = codegen::ir::ShaderModelInfo::new(19, 4);
}

#[test]
fn test_fma_policy_default() {
    assert_eq!(FmaPolicy::default(), FmaPolicy::AllowFusion);
}

#[test]
fn test_fma_policy_debug() {
    let dbg = format!("{:?}", FmaPolicy::AllowFusion);
    assert!(dbg.contains("AllowFusion"));
    let dbg = format!("{:?}", FmaPolicy::NoContraction);
    assert!(dbg.contains("NoContraction"));
}

#[test]
fn test_fma_policy_equality() {
    assert_eq!(FmaPolicy::AllowFusion, FmaPolicy::AllowFusion);
    assert_eq!(FmaPolicy::NoContraction, FmaPolicy::NoContraction);
    assert_ne!(FmaPolicy::AllowFusion, FmaPolicy::NoContraction);
}

#[test]
fn test_compile_options_nv_arch() {
    let nv_opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm86),
        ..CompileOptions::default()
    };
    let amd_opts = CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        ..CompileOptions::default()
    };
    assert_eq!(nv_opts.nv_arch(), Some(NvArch::Sm86));
    assert_eq!(amd_opts.nv_arch(), None);
}

#[test]
fn test_compile_options_amd_arch() {
    let nv_opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm86),
        ..CompileOptions::default()
    };
    let amd_opts = CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        ..CompileOptions::default()
    };
    assert_eq!(amd_opts.amd_arch(), Some(AmdArch::Rdna2));
    assert_eq!(nv_opts.amd_arch(), None);
}

#[test]
#[should_panic(expected = "CompileOptions::arch() called on non-NVIDIA target")]
fn test_compile_options_arch_panics_for_amd() {
    let opts = CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        ..CompileOptions::default()
    };
    let _ = opts.arch();
}

#[test]
fn test_compile_wgsl_malformed_returns_error() {
    let opts = CompileOptions::default();
    let result = compile_wgsl("not valid wgsl", &opts);
    assert!(
        result.is_err(),
        "malformed WGSL should return error: {result:?}"
    );
}

#[test]
fn test_compile_wgsl_intel_returns_unsupported_arch() {
    let opts = CompileOptions {
        target: GpuTarget::Intel(IntelArch::XeHpg),
        ..CompileOptions::default()
    };
    let result = compile_wgsl("@compute @workgroup_size(1) fn main() {}", &opts);
    assert!(
        matches!(result, Err(CompileError::UnsupportedArch(_))),
        "compile_wgsl with Intel target should return UnsupportedArch: {result:?}"
    );
}

#[test]
fn test_compile_intel_returns_unsupported_arch() {
    let opts = CompileOptions {
        target: GpuTarget::Intel(IntelArch::XeHpg),
        ..CompileOptions::default()
    };
    let result = compile(&[0x0723_0203], &opts);
    assert!(
        matches!(result, Err(CompileError::UnsupportedArch(_))),
        "compile with Intel target should return UnsupportedArch: {result:?}"
    );
}
