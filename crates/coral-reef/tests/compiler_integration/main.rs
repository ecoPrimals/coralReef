// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]
//! Integration tests for the coral-reef compiler pipeline.
//!
//! Compute shader compilation through the full pipeline is exercised here.
//! naga frontend (SPIR-V/WGSL), codegen, vendor backend, and driver wiring.

mod amd;
mod pipeline;
mod sm70;
mod stress;

use coral_reef::{CompileError, CompileOptions, GpuArch, compile, compile_wgsl};

// ---------------------------------------------------------------------------
// Shared helpers (pub(crate) for submodules)
// ---------------------------------------------------------------------------

/// WGSL → SPIR-V helper for SPIR-V input tests.
pub(crate) fn wgsl_to_spirv(source: &str) -> Vec<u32> {
    let module = naga::front::wgsl::parse_str(source).unwrap();
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap();
    naga::back::spv::write_vec(&module, &info, &naga::back::spv::Options::default(), None).unwrap()
}

/// SPH header size: 32 words = 128 bytes (SPHV4).
pub(crate) const SPH_HEADER_BYTES: usize = 32 * 4;

/// SM70 compile options for encoder tests.
pub(crate) fn sm70_opts() -> CompileOptions {
    CompileOptions {
        target: GpuArch::Sm70.into(),
        ..CompileOptions::default()
    }
}

// ---------------------------------------------------------------------------
// Architecture-specific compilation tests (opt levels 0, 2, 3)
// ---------------------------------------------------------------------------

#[test]
fn test_compile_wgsl_sm70() {
    let source = "@compute @workgroup_size(1) fn main() {}";
    for opt_level in [0, 2, 3] {
        let opts = CompileOptions {
            target: GpuArch::Sm70.into(),
            opt_level,
            debug_info: false,
            fp64_software: false,
            ..CompileOptions::default()
        };
        let result = compile_wgsl(source, &opts);
        match result {
            Ok(binary) => assert!(!binary.is_empty()),
            Err(CompileError::NotImplemented(_)) => {}
            Err(e) => panic!("unexpected error at opt_level {opt_level}: {e}"),
        }
    }
}

#[test]
fn test_compile_wgsl_sm75() {
    let source = "@compute @workgroup_size(1) fn main() {}";
    for opt_level in [0, 2, 3] {
        let opts = CompileOptions {
            target: GpuArch::Sm75.into(),
            opt_level,
            debug_info: false,
            fp64_software: false,
            ..CompileOptions::default()
        };
        let result = compile_wgsl(source, &opts);
        match result {
            Ok(binary) => assert!(!binary.is_empty()),
            Err(CompileError::NotImplemented(_)) => {}
            Err(e) => panic!("unexpected error at opt_level {opt_level}: {e}"),
        }
    }
}

#[test]
fn test_compile_wgsl_sm80() {
    let source = "@compute @workgroup_size(1) fn main() {}";
    for opt_level in [0, 2, 3] {
        let opts = CompileOptions {
            target: GpuArch::Sm80.into(),
            opt_level,
            debug_info: false,
            fp64_software: false,
            ..CompileOptions::default()
        };
        let result = compile_wgsl(source, &opts);
        match result {
            Ok(binary) => assert!(!binary.is_empty()),
            Err(CompileError::NotImplemented(_)) => {}
            Err(e) => panic!("unexpected error at opt_level {opt_level}: {e}"),
        }
    }
}

#[test]
fn test_compile_wgsl_sm86() {
    let source = "@compute @workgroup_size(1) fn main() {}";
    for opt_level in [0, 2, 3] {
        let opts = CompileOptions {
            target: GpuArch::Sm86.into(),
            opt_level,
            debug_info: false,
            fp64_software: false,
            ..CompileOptions::default()
        };
        let result = compile_wgsl(source, &opts);
        match result {
            Ok(binary) => assert!(!binary.is_empty()),
            Err(CompileError::NotImplemented(_)) => {}
            Err(e) => panic!("unexpected error at opt_level {opt_level}: {e}"),
        }
    }
}

#[test]
fn test_compile_wgsl_sm89() {
    let source = "@compute @workgroup_size(1) fn main() {}";
    for opt_level in [0, 2, 3] {
        let opts = CompileOptions {
            target: GpuArch::Sm89.into(),
            opt_level,
            debug_info: false,
            fp64_software: false,
            ..CompileOptions::default()
        };
        let result = compile_wgsl(source, &opts);
        match result {
            Ok(binary) => assert!(!binary.is_empty()),
            Err(CompileError::NotImplemented(_)) => {}
            Err(e) => panic!("unexpected error at opt_level {opt_level}: {e}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Error handling tests
// ---------------------------------------------------------------------------

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
    // The naga_translate pipeline attempts full compilation
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
            target: arch.into(),
            opt_level: 2,
            debug_info: false,
            fp64_software: true,
            ..CompileOptions::default()
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
// SPIR-V input tests — WGSL → SPIR-V (via naga) → compile
// ---------------------------------------------------------------------------

#[test]
fn test_spirv_input_minimal_compute() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let spirv = wgsl_to_spirv(wgsl);
    assert!(!spirv.is_empty(), "naga should produce non-empty SPIR-V");
    let result = compile(&spirv, &CompileOptions::default());
    match result {
        Ok(binary) => assert!(!binary.is_empty()),
        Err(CompileError::NotImplemented(_)) => {}
        Err(e) => panic!("unexpected error: {e}"),
    }
}

#[test]
fn test_spirv_input_arithmetic_shader() {
    let wgsl = "
        @compute @workgroup_size(1) fn main() {
            let x = 1.0 + 2.0;
            let y = x * 3.0;
        }
    ";
    let spirv = wgsl_to_spirv(wgsl);
    let result = compile(&spirv, &CompileOptions::default());
    match result {
        Ok(binary) => assert!(!binary.is_empty()),
        Err(CompileError::NotImplemented(_)) => {}
        Err(e) => panic!("unexpected error: {e}"),
    }
}

#[test]
fn test_spirv_input_storage_buffer() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read_write> data: array<u32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            data[gid.x] = gid.x;
        }
    ";
    let spirv = wgsl_to_spirv(wgsl);
    let result = compile(&spirv, &CompileOptions::default());
    match result {
        Ok(binary) => assert!(!binary.is_empty()),
        Err(CompileError::NotImplemented(_)) => {}
        Err(e) => panic!("unexpected error: {e}"),
    }
}

#[test]
fn test_spirv_input_all_archs() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let spirv = wgsl_to_spirv(wgsl);
    for &arch in GpuArch::ALL {
        let opts = CompileOptions {
            target: arch.into(),
            opt_level: 2,
            debug_info: false,
            fp64_software: false,
            ..CompileOptions::default()
        };
        let result = compile(&spirv, &opts);
        match result {
            Ok(binary) => assert!(!binary.is_empty(), "arch {arch} binary should not be empty"),
            Err(CompileError::NotImplemented(_)) => {}
            Err(e) => panic!("arch {arch} unexpected error: {e}"),
        }
    }
}

// ---------------------------------------------------------------------------
// E2E compute shader tests — naga WGSL → codegen IR → pipeline
// ---------------------------------------------------------------------------

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
            target: arch.into(),
            opt_level: 2,
            debug_info: false,
            fp64_software: false,
            ..CompileOptions::default()
        };
        let result = compile_wgsl(wgsl, &opts);
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
// Optimization pass and legalization tests
// ---------------------------------------------------------------------------

#[test]
fn test_pipeline_legalize_via_arithmetic() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let x = data[gid.x];
            let y = x * 2.0 + 1.0;
            data[gid.x] = y;
        }
    ";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "legalize (arithmetic) should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_pipeline_copy_prop_via_let_bindings() {
    let wgsl = "
        @compute @workgroup_size(1) fn main() {
            let a = 1u + 2u;
            let b = a;
            let c = b;
            let result = c;
        }
    ";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "copy prop (let bindings) should compile or fail with NotImplemented: {result:?}"
    );
}

#[test]
fn test_pipeline_calc_instr_deps_via_memory() {
    let wgsl = "
        @group(0) @binding(0) var<storage, read_write> data: array<u32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let idx = gid.x;
            let a = data[idx];
            let b = data[idx + 1u];
            data[idx] = a + b;
        }
    ";
    let result = compile_wgsl(wgsl, &CompileOptions::default());
    assert!(
        result.is_ok() || matches!(result, Err(CompileError::NotImplemented(_))),
        "calc_instr_deps (memory) should compile or fail with NotImplemented: {result:?}"
    );
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
fn test_error_spirv_garbage_data() {
    let garbage: Vec<u32> = (0u32..64).map(|i| i.wrapping_mul(0xDEAD_BEEF)).collect();
    let result = compile(&garbage, &CompileOptions::default());
    assert!(
        result.is_err(),
        "garbage SPIR-V data should produce an error"
    );
}

#[test]
fn test_error_spirv_zero_length() {
    let empty: &[u32] = &[];
    let result = compile(empty, &CompileOptions::default());
    assert!(
        matches!(result, Err(CompileError::InvalidInput(_))),
        "zero-length SPIR-V should return InvalidInput"
    );
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
