// SPDX-License-Identifier: AGPL-3.0-or-later
use super::*;

#[test]
fn test_compile_module_empty_entry_points_rejected() {
    let module = naga::Module::default();
    let result = compile_module(&module, &CompileOptions::default());
    assert!(
        matches!(result, Err(CompileError::InvalidInput(_))),
        "module with no entry points should fail: {result:?}"
    );
}

#[test]
fn test_compile_module_minimal_compute() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let module = naga::front::wgsl::parse_str(wgsl).expect("parse minimal WGSL");
    let opts = CompileOptions::default();
    let result = compile_module(&module, &opts);
    assert!(
        result.is_ok(),
        "direct naga::Module compile should succeed: {result:?}"
    );
}

#[test]
fn test_compile_module_full_returns_metadata() {
    let wgsl = "@compute @workgroup_size(64) fn main() {}";
    let module = naga::front::wgsl::parse_str(wgsl).expect("parse WGSL");
    let opts = CompileOptions::default();
    let compiled = compile_module_full(&module, &opts).expect("module_full compile");
    assert!(!compiled.binary.is_empty(), "binary should be non-empty");
    assert_eq!(compiled.info.local_size[0], 64, "workgroup_size x");
}

#[test]
fn test_compile_module_matches_wgsl_output() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let module = naga::front::wgsl::parse_str(wgsl).expect("parse WGSL");
    let opts = CompileOptions::default();
    let from_wgsl = compile_wgsl(wgsl, &opts).expect("wgsl compile");
    let from_module = compile_module(&module, &opts).expect("module compile");
    assert_eq!(
        from_wgsl, from_module,
        "direct module path should produce identical binary to WGSL path"
    );
}

#[test]
fn test_compile_module_amd_target() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let module = naga::front::wgsl::parse_str(wgsl).expect("parse WGSL");
    let opts = CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        ..Default::default()
    };
    let result = compile_module(&module, &opts);
    assert!(
        result.is_ok(),
        "AMD module compile should succeed: {result:?}"
    );
}

#[test]
fn test_compile_module_intel_unsupported() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let module = naga::front::wgsl::parse_str(wgsl).expect("parse WGSL");
    let opts = CompileOptions {
        target: GpuTarget::Intel(IntelArch::XeHpg),
        ..Default::default()
    };
    let result = compile_module(&module, &opts);
    assert!(
        matches!(result, Err(CompileError::UnsupportedArch(_))),
        "Intel should return UnsupportedArch: {result:?}"
    );
}

#[test]
fn test_compile_module_full_f64_software_lowering() {
    let wgsl = r"
@compute @workgroup_size(1)
fn main() {
    let a: f64 = 1.0;
    let b: f64 = 2.0;
    let c = a * b + a;
    _ = c;
}";
    let module = naga::front::wgsl::parse_str(wgsl).expect("parse f64 WGSL");
    let opts = CompileOptions {
        fp64_software: true,
        ..Default::default()
    };
    let compiled =
        compile_module_full(&module, &opts).expect("f64 software lowering through module API");
    assert!(!compiled.binary.is_empty());
    assert!(compiled.info.gpr_count > 0);
}

#[test]
fn test_compile_module_full_fma_fused() {
    let wgsl = r"
@compute @workgroup_size(32)
fn main() {
    let a: f32 = 1.0;
    let b: f32 = 2.0;
    let c = a * b + a;
    _ = c;
}";
    let module = naga::front::wgsl::parse_str(wgsl).expect("parse fma WGSL");
    let opts = CompileOptions {
        fma_policy: FmaPolicy::Fused,
        ..Default::default()
    };
    let compiled = compile_module_full(&module, &opts).expect("fused FMA through module API");
    assert!(!compiled.binary.is_empty());
    assert_eq!(compiled.info.local_size[0], 32);
}

#[test]
fn test_compile_module_sm120_ptx_path() {
    let wgsl = "@compute @workgroup_size(128) fn main() {}";
    let module = naga::front::wgsl::parse_str(wgsl).expect("parse WGSL");
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm120),
        ..Default::default()
    };
    let compiled = compile_module_full(&module, &opts).expect("SM120 PTX emit through module API");
    assert!(!compiled.binary.is_empty());
    let ptx = String::from_utf8_lossy(&compiled.binary);
    assert!(
        ptx.contains(".version") || ptx.contains(".target"),
        "SM120 should produce PTX text: {ptx:.200}"
    );
}

#[test]
fn test_compile_module_full_shared_memory_reporting() {
    let wgsl = r"
var<workgroup> shared_data: array<f32, 256>;
@compute @workgroup_size(64)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    shared_data[lid.x] = f32(lid.x);
    workgroupBarrier();
    let v = shared_data[63u - lid.x];
    _ = v;
}";
    let module = naga::front::wgsl::parse_str(wgsl).expect("parse shared mem WGSL");
    let opts = CompileOptions::default();
    let compiled =
        compile_module_full(&module, &opts).expect("shared memory shader via module API");
    assert!(
        compiled.info.shared_mem_bytes > 0,
        "should report shared memory usage: {} bytes",
        compiled.info.shared_mem_bytes
    );
    assert!(compiled.info.gpr_count > 0, "should report GPR usage");
    assert_eq!(compiled.info.local_size[0], 64);
}

#[test]
fn test_compile_module_entry_point_selection_by_name() {
    let wgsl = r"
@compute @workgroup_size(32)
fn kernel_a() {}

@compute @workgroup_size(128)
fn kernel_b() {}
";
    let module = naga::front::wgsl::parse_str(wgsl).expect("parse multi-EP WGSL");
    let opts_a = CompileOptions {
        entry_point: Some("kernel_a".into()),
        ..Default::default()
    };
    let compiled_a = compile_module_full(&module, &opts_a).expect("compile kernel_a by name");
    assert_eq!(compiled_a.info.local_size[0], 32);

    let opts_b = CompileOptions {
        entry_point: Some("kernel_b".into()),
        ..Default::default()
    };
    let compiled_b = compile_module_full(&module, &opts_b).expect("compile kernel_b by name");
    assert_eq!(compiled_b.info.local_size[0], 128);
}

#[test]
fn test_compile_module_entry_point_not_found() {
    let wgsl = "@compute @workgroup_size(64) fn main() {}";
    let module = naga::front::wgsl::parse_str(wgsl).expect("parse WGSL");
    let opts = CompileOptions {
        entry_point: Some("nonexistent_kernel".into()),
        ..Default::default()
    };
    let err = compile_module_full(&module, &opts).expect_err("should fail for missing entry point");
    assert!(matches!(err, CompileError::InvalidInput(_)));
    let msg = err.to_string();
    assert!(
        msg.contains("nonexistent_kernel"),
        "error should name the missing EP: {msg}"
    );
    assert!(
        msg.contains("main"),
        "error should list available EPs: {msg}"
    );
}

#[test]
fn test_compile_module_default_selects_compute() {
    let wgsl = r"
@vertex
fn vert_main() -> @builtin(position) vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, 1.0);
}

@compute @workgroup_size(256)
fn compute_main() {}
";
    let module = naga::front::wgsl::parse_str(wgsl).expect("parse mixed-stage WGSL");
    let opts = CompileOptions::default();
    let compiled =
        compile_module_full(&module, &opts).expect("should select compute EP by default");
    assert_eq!(
        compiled.info.local_size[0], 256,
        "should have selected compute_main (wg=256), not vert_main"
    );
}

#[test]
fn test_compile_module_validation_rejects_malformed() {
    let wgsl = r"
var<workgroup> data: array<f32, 64>;
@compute @workgroup_size(64)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    data[lid.x] = f32(lid.x);
}";
    let mut module = naga::front::wgsl::parse_str(wgsl).expect("parse WGSL");
    module.global_variables.clear();
    let opts = CompileOptions {
        validate: true,
        ..Default::default()
    };
    let err = compile_module_full(&module, &opts)
        .expect_err("malformed module should be rejected by validation");
    assert!(
        matches!(err, CompileError::Validation(_)),
        "should be a validation error, got: {err}"
    );
}

#[test]
fn test_compile_module_validation_disabled_skips_check() {
    let wgsl = "@compute @workgroup_size(64) fn main() {}";
    let module = naga::front::wgsl::parse_str(wgsl).expect("parse WGSL");
    let opts = CompileOptions {
        validate: false,
        ..Default::default()
    };
    let result = compile_module_full(&module, &opts);
    assert!(
        result.is_ok(),
        "valid module should compile with validation disabled"
    );
}
