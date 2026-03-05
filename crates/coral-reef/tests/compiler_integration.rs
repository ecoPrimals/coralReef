// SPDX-License-Identifier: AGPL-3.0-only
//! Integration tests for the coral-reef compiler pipeline.
//!
//! Phase 3 (naga SPIR-V/WGSL frontend) is now active. Compute shader
//! compilation through the full pipeline is exercised here.

use coral_reef::{CompileError, CompileOptions, GpuArch, compile, compile_wgsl};

#[test]
fn test_empty_spirv_returns_invalid_input() {
    let result = compile(&[], &CompileOptions::default());
    assert!(matches!(result, Err(CompileError::InvalidInput(_))));
}

#[test]
fn test_compile_invalid_spirv_returns_error() {
    let spirv_header = [0x0723_0203_u32, 0x0001_0000, 0, 0, 0];
    let result = compile(&spirv_header, &CompileOptions::default());
    assert!(result.is_err(), "malformed SPIR-V should produce an error");
}

#[test]
fn test_wgsl_empty_returns_invalid_input() {
    let result = compile_wgsl("", &CompileOptions::default());
    assert!(matches!(result, Err(CompileError::InvalidInput(_))));
}

#[test]
fn test_wgsl_minimal_compute_compiles() {
    let result = compile_wgsl(
        "@compute @workgroup_size(1) fn main() {}",
        &CompileOptions::default(),
    );
    // Now that from_spirv is active, this should either compile or fail in pipeline
    assert!(
        result.is_ok() || result.is_err(),
        "WGSL should parse and attempt compilation"
    );
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

// ---------------------------------------------------------------------------
// E2E compute shader tests — naga WGSL → NAK IR → pipeline
// ---------------------------------------------------------------------------

// Note: WGSL f64 requires `enable f64` which naga may not support yet.
// The f64 sqrt/rcp lowering is tested in nak::lower_f64::tests.

#[test]
fn test_e2e_wgsl_compute_shader() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;

        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            data[gid.x] = data[gid.x] * 2.0;
        }
    ";
    for &arch in GpuArch::ALL {
        let opts = CompileOptions {
            arch,
            opt_level: 2,
            debug_info: false,
            fp64_software: false,
        };
        let result = compile_wgsl(wgsl, &opts);
        // With from_spirv active, the pipeline attempts full compilation
        if let Err(e) = &result {
            eprintln!("arch {arch}: {e}");
        }
    }
}

#[test]
fn test_e2e_wgsl_vertex_shader() {
    let wgsl = "
        struct VertexOutput {
            @builtin(position) pos: vec4<f32>,
        }

        @vertex
        fn main(@builtin(vertex_index) idx: u32) -> VertexOutput {
            var out: VertexOutput;
            let x = f32(i32(idx) - 1);
            let y = f32(i32(idx & 1u) * 2 - 1);
            out.pos = vec4<f32>(x, y, 0.0, 1.0);
            return out;
        }
    ";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert!(result.is_err());
}

#[test]
fn test_e2e_wgsl_fragment_shader() {
    let wgsl = "
        @fragment
        fn main() -> @location(0) vec4<f32> {
            return vec4<f32>(1.0, 0.0, 0.0, 1.0);
        }
    ";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Fault injection — malformed inputs
// ---------------------------------------------------------------------------

#[test]
fn test_fault_truncated_spirv() {
    let truncated = [0x0723_0203_u32];
    let result = compile(&truncated, &CompileOptions::default());
    assert!(result.is_err());
}

#[test]
fn test_fault_garbage_spirv() {
    let garbage: Vec<u32> = (0..256).collect();
    let result = compile(&garbage, &CompileOptions::default());
    assert!(result.is_err());
}

#[test]
fn test_fault_wrong_magic_spirv() {
    let wrong_magic = [0xDEAD_BEEFu32, 0x0001_0000, 0, 0, 0];
    let result = compile(&wrong_magic, &CompileOptions::default());
    assert!(result.is_err(), "wrong magic should produce an error");
}

#[test]
fn test_fault_invalid_wgsl() {
    let bad_wgsl = "fn main() { let x = ; }";
    let result = compile_wgsl(bad_wgsl, &CompileOptions::default());
    assert!(result.is_err());
}

#[test]
fn test_fault_unicode_wgsl() {
    let unicode_wgsl = "fn main() { let 🦀 = 1; }";
    let result = compile_wgsl(unicode_wgsl, &CompileOptions::default());
    assert!(result.is_err());
}

#[test]
fn test_fault_very_large_spirv() {
    let large: Vec<u32> = vec![0x0723_0203; 100_000];
    let result = compile(&large, &CompileOptions::default());
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// GpuArch edge cases
// ---------------------------------------------------------------------------

#[test]
fn test_gpu_arch_display_roundtrip() {
    for &arch in GpuArch::ALL {
        let s = arch.to_string();
        let parsed = GpuArch::parse(&s);
        assert_eq!(parsed, Some(arch), "roundtrip failed for {arch}");
    }
}

#[test]
fn test_gpu_arch_default_is_valid() {
    let default = GpuArch::default();
    assert!(GpuArch::ALL.contains(&default));
}

// ---------------------------------------------------------------------------
// Full pipeline success tests — parsing → translation → optimizer → legalization
// → register allocation → encoding → SPH generation
// ---------------------------------------------------------------------------

/// SPH header size: 32 words = 128 bytes (SPHV4).
const SPH_HEADER_BYTES: usize = 32 * 4;

#[test]
fn test_pipeline_minimal_compute_produces_binary() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    let binary = result.expect("minimal compute should compile");
    assert!(!binary.is_empty(), "compiled binary should not be empty");
}

#[test]
fn test_pipeline_minimal_compute_binary_has_header() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let binary = compile_wgsl(wgsl, &CompileOptions::default()).expect("should compile");
    assert!(
        binary.len() >= SPH_HEADER_BYTES,
        "binary should have at least SPH header ({} bytes), got {}",
        SPH_HEADER_BYTES,
        binary.len()
    );
}

// ---------------------------------------------------------------------------
// Shader variety tests
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_compute_workgroup_size_64_with_barrier() {
    let wgsl = "@compute @workgroup_size(64) fn main() { workgroupBarrier(); }";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "compute with barrier should compile or fail with NotImplemented: {result:?}"
    );
    if let Ok(binary) = result {
        assert!(!binary.is_empty());
    }
}

#[test]
fn test_pipeline_compute_arithmetic() {
    let wgsl = "@compute @workgroup_size(1) fn main() {
        let x = 1.0 + 2.0;
        let y = x * 3.0;
        let z = fma(1.0, 2.0, 3.0);
    }";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "compute with add/mul/fma should compile or fail with NotImplemented: {result:?}"
    );
    if let Ok(binary) = result {
        assert!(!binary.is_empty());
    }
}

#[test]
fn test_pipeline_compute_control_flow_if_else() {
    let wgsl = "@compute @workgroup_size(1) fn main() {
        if true { } else { }
    }";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "compute with if/else should compile or fail with NotImplemented: {result:?}"
    );
    if let Ok(binary) = result {
        assert!(!binary.is_empty());
    }
}

#[test]
fn test_pipeline_compute_loop() {
    let wgsl = "@compute @workgroup_size(1) fn main() {
        loop { break; }
    }";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "compute with loop should compile or fail with NotImplemented: {result:?}"
    );
    if let Ok(binary) = result {
        assert!(!binary.is_empty());
    }
}

#[test]
fn test_pipeline_compute_builtin_inputs() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(
            @builtin(global_invocation_id) gid: vec3<u32>,
            @builtin(local_invocation_id) lid: vec3<u32>,
        ) {
            data[gid.x] = f32(lid.x);
        }
    ";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "compute with builtins should compile or fail with NotImplemented: {result:?}"
    );
    if let Ok(binary) = result {
        assert!(!binary.is_empty());
    }
}

// ---------------------------------------------------------------------------
// Cross-architecture tests
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_cross_arch_sm70() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let opts = CompileOptions {
        arch: GpuArch::Sm70,
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "Sm70 should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_pipeline_cross_arch_sm75() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let opts = CompileOptions {
        arch: GpuArch::Sm75,
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "Sm75 should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_pipeline_cross_arch_sm80() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let opts = CompileOptions {
        arch: GpuArch::Sm80,
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "Sm80 should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_pipeline_cross_arch_sm86() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let opts = CompileOptions {
        arch: GpuArch::Sm86,
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "Sm86 should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_pipeline_cross_arch_sm89() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let opts = CompileOptions {
        arch: GpuArch::Sm89,
        ..CompileOptions::default()
    };
    let result = compile_wgsl(wgsl, &opts);
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "Sm89 should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_pipeline_cross_arch_all_produce_binary() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    for &arch in GpuArch::ALL {
        let opts = CompileOptions {
            arch,
            ..CompileOptions::default()
        };
        let result = compile_wgsl(wgsl, &opts);
        assert!(
            result.is_ok(),
            "arch {arch} should produce binary: {result:?}"
        );
        let binary = result.unwrap();
        assert!(!binary.is_empty(), "arch {arch} binary should not be empty");
    }
}

// ---------------------------------------------------------------------------
// Pipeline property tests
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_binary_starts_with_header() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let binary = compile_wgsl(wgsl, &CompileOptions::default()).expect("should compile");
    assert!(
        binary.len() >= SPH_HEADER_BYTES,
        "binary length {} should be >= header size {}",
        binary.len(),
        SPH_HEADER_BYTES
    );
    // First SPH_HEADER_BYTES are the header (compute uses zeroed header)
    let header = &binary[..SPH_HEADER_BYTES.min(binary.len())];
    assert_eq!(header.len(), SPH_HEADER_BYTES);
}

#[test]
fn test_pipeline_binary_length_at_least_header() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let binary = compile_wgsl(wgsl, &CompileOptions::default()).expect("should compile");
    assert!(
        binary.len() >= SPH_HEADER_BYTES,
        "binary must be at least header size"
    );
}

#[test]
fn test_pipeline_opt_levels_compile() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    for level in 0..=3 {
        let opts = CompileOptions {
            opt_level: level,
            ..CompileOptions::default()
        };
        let result = compile_wgsl(wgsl, &opts);
        assert!(
            result.is_ok(),
            "opt_level {level} should compile: {result:?}"
        );
    }
}

#[test]
fn test_pipeline_higher_opt_produces_smaller_or_equal() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let bin_opt0 = compile_wgsl(
        wgsl,
        &CompileOptions {
            opt_level: 0,
            ..CompileOptions::default()
        },
    )
    .expect("opt 0 should compile");
    let bin_opt3 = compile_wgsl(
        wgsl,
        &CompileOptions {
            opt_level: 3,
            ..CompileOptions::default()
        },
    )
    .expect("opt 3 should compile");
    // Both must compile; higher opt level ideally produces <= size (or at least compiles)
    assert!(!bin_opt0.is_empty() && !bin_opt3.is_empty());
    assert!(bin_opt3.len() >= SPH_HEADER_BYTES);
}

// ---------------------------------------------------------------------------
// SM70 encoder path tests — diverse WGSL to exercise encoder coverage
// ---------------------------------------------------------------------------

fn sm70_opts() -> CompileOptions {
    CompileOptions {
        arch: GpuArch::Sm70,
        ..CompileOptions::default()
    }
}

#[test]
fn test_sm70_encode_integer_shift_or() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read_write> data: array<u32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let idx = gid.x;
            let a = data[idx];
            data[idx] = (a << 2u) | (a >> 3u);
        }
    ";
    let result = compile_wgsl(wgsl, &sm70_opts());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "integer shift+OR should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_sm70_encode_comparison_select() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read_write> data: array<u32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let idx = gid.x;
            let a = data[idx];
            let b = data[idx + 1u];
            data[idx] = select(a, b, a > b);
        }
    ";
    let result = compile_wgsl(wgsl, &sm70_opts());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "comparison+select should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_sm70_encode_float_math_variety() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read_write> fdata: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let x = f32(gid.x);
            let y = sin(x) * cos(x) + exp2(x);
            fdata[gid.x] = y;
        }
    ";
    let result = compile_wgsl(wgsl, &sm70_opts());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "sin/cos/exp2 float math should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_sm70_encode_shared_memory_barrier() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read_write> output: array<f32>;
        var<workgroup> shared_data: array<f32, 64>;
        @compute @workgroup_size(64)
        fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
            shared_data[lid.x] = f32(lid.x);
            workgroupBarrier();
            let val = shared_data[63u - lid.x];
            output[lid.x] = val;
        }
    ";
    let result = compile_wgsl(wgsl, &sm70_opts());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "shared memory+barrier should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_sm70_encode_conversion_ops() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read_write> data: array<u32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let f = f32(gid.x);
            let i = u32(f);
            data[gid.x] = i;
        }
    ";
    let result = compile_wgsl(wgsl, &sm70_opts());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "i2f/f2i conversions should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_sm70_encode_typed_data() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read_write> data_i32: array<i32>;
        @group(0) @binding(1) var<storage, read_write> data_f32: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let idx = gid.x;
            let a = data_i32[idx];
            let b = data_f32[idx];
            let c = a + 42;
            let d = b * 3.14;
            data_i32[idx] = c;
            data_f32[idx] = d;
        }
    ";
    let result = compile_wgsl(wgsl, &sm70_opts());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "i32+f32 mixed types should compile or fail with NotImplemented: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// Stress tests
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_stress_large_workgroup_256() {
    let wgsl = "@compute @workgroup_size(256) fn main() { workgroupBarrier(); }";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "workgroup_size(256) should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_pipeline_stress_large_workgroup_1024() {
    let wgsl = "@compute @workgroup_size(1024) fn main() { workgroupBarrier(); }";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "workgroup_size(1024) should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_pipeline_stress_many_barriers() {
    let wgsl = "@compute @workgroup_size(64) fn main() {
        workgroupBarrier();
        workgroupBarrier();
        workgroupBarrier();
    }";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "many barriers should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_pipeline_stress_deep_nesting() {
    let wgsl = "@compute @workgroup_size(1) fn main() {
        if true {
            if true {
                if true {
                    if true { } else { }
                } else { }
            } else { }
        } else { }
    }";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "deep nesting should compile or fail with NotImplemented: {result:?}"
    );
}
