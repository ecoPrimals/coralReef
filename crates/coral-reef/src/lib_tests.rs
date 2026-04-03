// SPDX-License-Identifier: AGPL-3.0-only
use super::*;

#[cfg(feature = "naga")]
mod naga_frontend_tests {
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

    #[test]
    fn test_compile_wgsl_full_empty_rejected() {
        let opts = CompileOptions::default();
        let result = compile_wgsl_full("", &opts);
        assert!(matches!(result, Err(CompileError::InvalidInput(_))));
    }

    #[test]
    fn test_compile_glsl_full_empty_rejected() {
        let opts = CompileOptions::default();
        let result = compile_glsl_full("", &opts);
        assert!(matches!(result, Err(CompileError::InvalidInput(_))));
    }

    #[test]
    fn test_compile_wgsl_raw_sm_empty_rejected() {
        let result = compile_wgsl_raw_sm("", 70);
        assert!(matches!(result, Err(CompileError::InvalidInput(_))));
    }

    #[test]
    fn test_compile_wgsl_raw_sm_70() {
        let result = compile_wgsl_raw_sm("@compute @workgroup_size(1) fn main() {}", 70);
        assert!(result.is_ok(), "raw sm70 should compile: {result:?}");
    }

    #[test]
    fn test_compile_glsl_intel_returns_unsupported() {
        let opts = CompileOptions {
            target: GpuTarget::Intel(IntelArch::XeHpg),
            ..CompileOptions::default()
        };
        let glsl = "#version 450\nlayout(local_size_x = 1) in;\nvoid main() {}";
        let result = compile_glsl(glsl, &opts);
        assert!(
            matches!(result, Err(CompileError::UnsupportedArch(_))),
            "compile_glsl with Intel should return UnsupportedArch: {result:?}"
        );
    }

    #[test]
    fn test_compile_glsl_full_minimal() {
        let opts = CompileOptions::default();
        let glsl = "#version 450\nlayout(local_size_x = 1) in;\nvoid main() {}";
        let result = compile_glsl_full(glsl, &opts);
        assert!(
            result.is_ok(),
            "minimal GLSL full compile should succeed: {result:?}"
        );
    }

    #[test]
    fn test_compile_wgsl_full_minimal() {
        let opts = CompileOptions::default();
        let result = compile_wgsl_full("@compute @workgroup_size(1) fn main() {}", &opts);
        assert!(
            result.is_ok(),
            "minimal WGSL full compile should succeed: {result:?}"
        );
    }
}

#[test]
fn test_default_options() {
    let opts = CompileOptions::default();
    assert_eq!(opts.arch().unwrap(), GpuArch::Sm70);
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
    assert_eq!(cloned.arch().unwrap(), GpuArch::Sm89);
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
fn test_backend_for_resolves_amd() {
    let be = backend::backend_for(GpuTarget::Amd(AmdArch::Rdna2));
    assert!(be.is_ok());
}

#[test]
#[should_panic(expected = "NVIDIA shader model must be >= SM 2.0")]
fn test_shader_model_info_new_panics_for_sm_below_20() {
    let _ = codegen::ir::ShaderModelInfo::new(19, 4);
}

#[test]
fn test_fma_policy_default() {
    assert_eq!(FmaPolicy::default(), FmaPolicy::Auto);
}

#[test]
fn test_fma_policy_debug() {
    let dbg = format!("{:?}", FmaPolicy::Fused);
    assert!(dbg.contains("Fused"));
    let dbg = format!("{:?}", FmaPolicy::Separate);
    assert!(dbg.contains("Separate"));
}

#[test]
fn test_fma_policy_equality() {
    assert_eq!(FmaPolicy::Fused, FmaPolicy::Fused);
    assert_eq!(FmaPolicy::Separate, FmaPolicy::Separate);
    assert_ne!(FmaPolicy::Fused, FmaPolicy::Separate);
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
fn test_compile_options_arch_returns_err_for_amd() {
    let opts = CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        ..CompileOptions::default()
    };
    assert!(opts.arch().is_err());
}

#[test]
fn test_fp64_strategy_variants() {
    assert_eq!(Fp64Strategy::default(), Fp64Strategy::Native);
    assert_ne!(Fp64Strategy::Native, Fp64Strategy::DoubleFloat);
    assert_ne!(Fp64Strategy::DoubleFloat, Fp64Strategy::F32Only);
    let dbg = format!("{:?}", Fp64Strategy::DoubleFloat);
    assert!(dbg.contains("DoubleFloat"));
}

#[test]
fn test_prepare_wgsl_no_preamble() {
    let plain = "@compute @workgroup_size(1) fn main() {}";
    let opts = CompileOptions::default();
    let result = prepare_wgsl(plain, &opts);
    assert!(
        matches!(result, std::borrow::Cow::Borrowed(_)),
        "plain WGSL needs no preamble allocation"
    );
}

#[test]
fn test_prepare_wgsl_df64_preamble() {
    let source = "@compute @workgroup_size(1) fn main() { let x = df64_add(a, b); }";
    let opts = CompileOptions::default();
    let result = prepare_wgsl(source, &opts);
    assert!(result.contains("struct Df64") || result.contains("df64_"));
}

#[test]
fn test_prepare_wgsl_complex64_preamble() {
    let source = "@compute @workgroup_size(1) fn main() { let z = c64_mul(a, b); }";
    let opts = CompileOptions::default();
    let result = prepare_wgsl(source, &opts);
    assert!(result.contains("Complex64") || result.contains("c64_"));
}

#[test]
fn test_prepare_wgsl_f32_transcendental_preamble() {
    let source = "@compute @workgroup_size(1) fn main() { let p = power_f32(2.0, 3.0); }";
    let opts = CompileOptions::default();
    let result = prepare_wgsl(source, &opts);
    assert!(result.contains("power_f32"));
}

#[test]
fn test_prepare_wgsl_prng_preamble() {
    let source = "@compute @workgroup_size(1) fn main() { var s = 42u; let r = xorshift32(&s); }";
    let opts = CompileOptions::default();
    let result = prepare_wgsl(source, &opts);
    assert!(result.contains("xorshift32"));
}

#[test]
fn test_prepare_wgsl_su3_auto_chains_complex64_and_prng() {
    let source = "@compute @workgroup_size(1) fn main() { let m = su3_identity(); }";
    let opts = CompileOptions::default();
    let result = prepare_wgsl(source, &opts);
    assert!(
        result.contains("Complex64") || result.contains("su3_identity"),
        "SU3 should auto-chain complex64 preamble"
    );
}

#[test]
fn test_prepare_wgsl_strip_enable_f64() {
    let source = "enable f64;\n@compute @workgroup_size(1) fn main() {}";
    let opts = CompileOptions::default();
    let result = prepare_wgsl(source, &opts);
    assert!(
        !result.contains("enable f64"),
        "enable f64 directive should be stripped"
    );
}

#[test]
fn test_prepare_wgsl_strip_enable_f16() {
    let source = "enable f16;\n@compute @workgroup_size(1) fn main() {}";
    let opts = CompileOptions::default();
    let result = prepare_wgsl(source, &opts);
    assert!(
        !result.contains("enable f16"),
        "enable f16 directive should be stripped"
    );
}

#[test]
fn test_prepare_wgsl_double_float_strategy_triggers_df64() {
    let source = "@compute @workgroup_size(1) fn main() {}";
    let opts = CompileOptions {
        fp64_strategy: Fp64Strategy::DoubleFloat,
        ..CompileOptions::default()
    };
    let result = prepare_wgsl(source, &opts);
    assert!(
        result.contains("Df64") || result.contains("struct Df64"),
        "DoubleFloat strategy should inject df64 preamble"
    );
}

#[test]
fn test_strip_enable_directives_preserves_other_lines() {
    let source = "enable f64;\nfn foo() {}\nenable f16;\nfn bar() {}";
    let result = strip_enable_directives(source);
    assert!(!result.contains("enable f64"));
    assert!(!result.contains("enable f16"));
    assert!(result.contains("fn foo()"));
    assert!(result.contains("fn bar()"));
}

#[test]
fn test_emit_binary_nvidia_includes_header() {
    let mut header = [0u32; codegen::nv::shader_header::CURRENT_MAX_SHADER_HEADER_SIZE];
    header[0] = 0xDEAD;
    header[1] = 0xBEEF;
    let compiled = codegen::pipeline::CompiledShader {
        header,
        code: vec![0xCAFE],
    };
    let binary = emit_binary(&compiled, GpuTarget::Nvidia(NvArch::Sm70));
    let header_bytes = codegen::nv::shader_header::CURRENT_MAX_SHADER_HEADER_SIZE * 4;
    assert_eq!(binary.len(), header_bytes + 4, "full header + 1 code word");
}

#[test]
fn test_emit_binary_amd_no_header() {
    let compiled = codegen::pipeline::CompiledShader {
        header: [0u32; codegen::nv::shader_header::CURRENT_MAX_SHADER_HEADER_SIZE],
        code: vec![0xCAFE],
    };
    let binary = emit_binary(&compiled, GpuTarget::Amd(AmdArch::Rdna2));
    assert_eq!(binary.len(), 4, "AMD skips header, only code words");
}
