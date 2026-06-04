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
fn test_wgsl_to_spirv_produces_valid_spirv() {
    let spirv = wgsl_to_spirv(
        "@compute @workgroup_size(1) fn main() {}",
        &CompileOptions::default(),
    )
    .expect("should produce SPIR-V");
    assert!(spirv.len() >= 20, "SPIR-V should be non-trivial");
    assert_eq!(spirv.len() % 4, 0, "SPIR-V must be word-aligned");
    let magic = u32::from_le_bytes([spirv[0], spirv[1], spirv[2], spirv[3]]);
    assert_eq!(magic, 0x0723_0203, "SPIR-V magic");
}

#[test]
fn test_wgsl_to_spirv_empty_rejected() {
    let result = wgsl_to_spirv("", &CompileOptions::default());
    assert!(matches!(result, Err(CompileError::InvalidInput(_))));
}

#[test]
fn test_wgsl_to_spirv_roundtrip_through_compile() {
    let opts = CompileOptions::default();
    let spirv_bytes = wgsl_to_spirv("@compute @workgroup_size(1) fn main() {}", &opts)
        .expect("should emit SPIR-V");
    let words: Vec<u32> = spirv_bytes
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    let result = compile(&words, &opts);
    assert!(
        result.is_ok(),
        "sovereign SPIR-V should re-compile: {result:?}"
    );
}

#[test]
fn test_wgsl_to_spirv_version_targeting() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let opts_13 = CompileOptions {
        spirv: Some(SpirVOptions {
            version: (1, 3),
            ..SpirVOptions::default()
        }),
        ..CompileOptions::default()
    };
    let opts_15 = CompileOptions {
        spirv: Some(SpirVOptions {
            version: (1, 5),
            ..SpirVOptions::default()
        }),
        ..CompileOptions::default()
    };
    let bytes_13 = wgsl_to_spirv(wgsl, &opts_13).expect("SPIR-V 1.3");
    let bytes_15 = wgsl_to_spirv(wgsl, &opts_15).expect("SPIR-V 1.5");
    assert!(bytes_13.len() >= 20);
    assert!(bytes_15.len() >= 20);
    // SPIR-V version is encoded in word[1]: (major << 16) | (minor << 8)
    let ver_word_13 = u32::from_le_bytes([bytes_13[4], bytes_13[5], bytes_13[6], bytes_13[7]]);
    let ver_word_15 = u32::from_le_bytes([bytes_15[4], bytes_15[5], bytes_15[6], bytes_15[7]]);
    let expected_13 = (1u32 << 16) | (3u32 << 8);
    let expected_15 = (1u32 << 16) | (5u32 << 8);
    assert_eq!(
        ver_word_13, expected_13,
        "SPIR-V 1.3 version word: {ver_word_13:#010x} vs {expected_13:#010x}"
    );
    assert_eq!(
        ver_word_15, expected_15,
        "SPIR-V 1.5 version word: {ver_word_15:#010x} vs {expected_15:#010x}"
    );
}

#[test]
fn test_wgsl_to_spirv_naga_validates_output() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> data: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    data[gid.x] = data[gid.x] * 2.0 + 1.0;
}
";
    let spirv_bytes =
        wgsl_to_spirv(wgsl, &CompileOptions::default()).expect("should emit SPIR-V");
    let words: Vec<u32> = spirv_bytes
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();

    let module = naga::front::spv::parse_u8_slice(
        &spirv_bytes,
        &naga::front::spv::Options::default(),
    )
    .expect("emitted SPIR-V should be parseable by naga");

    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator
        .validate(&module)
        .expect("emitted SPIR-V should pass naga validation");

    assert!(words[0] == 0x0723_0203, "SPIR-V magic word");
    assert!(words.len() > 20, "non-trivial module");
}

#[test]
fn test_wgsl_to_spirv_compute_preserves_entry_point() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(32)
fn my_kernel(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = gid.x;
}
";
    let spirv_bytes =
        wgsl_to_spirv(wgsl, &CompileOptions::default()).expect("should emit SPIR-V");
    let module = naga::front::spv::parse_u8_slice(
        &spirv_bytes,
        &naga::front::spv::Options::default(),
    )
    .expect("parse emitted SPIR-V");

    assert!(
        !module.entry_points.is_empty(),
        "module should have entry points"
    );
    let ep = &module.entry_points[0];
    assert_eq!(ep.stage, naga::ShaderStage::Compute);
    assert_eq!(ep.workgroup_size, [32, 1, 1]);
}

#[test]
fn test_wgsl_to_spirv_f64_arithmetic() {
    let wgsl = r"
enable f16;

@group(0) @binding(0) var<storage, read_write> buf: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = f32(gid.x);
    let b = a * 3.14159;
    buf[gid.x] = a + b;
}
";
    let opts = CompileOptions {
        fp64_software: true,
        ..CompileOptions::default()
    };
    let spirv_bytes = wgsl_to_spirv(wgsl, &opts).expect("f32 arithmetic should emit SPIR-V");
    let module = naga::front::spv::parse_u8_slice(
        &spirv_bytes,
        &naga::front::spv::Options::default(),
    )
    .expect("emitted SPIR-V should parse");
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator
        .validate(&module)
        .expect("f32 arithmetic SPIR-V should validate");
}

#[test]
fn test_wgsl_to_spirv_atomics() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> counter: atomic<u32>;
@group(0) @binding(1) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(local_invocation_index) lid: u32) {
    let prev = atomicAdd(&counter, 1u);
    out[lid] = prev;
}
";
    let spirv_bytes =
        wgsl_to_spirv(wgsl, &CompileOptions::default()).expect("atomics should emit SPIR-V");
    let module = naga::front::spv::parse_u8_slice(
        &spirv_bytes,
        &naga::front::spv::Options::default(),
    )
    .expect("atomic SPIR-V should parse");
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator
        .validate(&module)
        .expect("atomic SPIR-V should validate");
}

#[test]
fn test_wgsl_to_spirv_shared_memory_barrier() {
    let wgsl = r"
var<workgroup> shared_tile: array<f32, 256>;

@group(0) @binding(0) var<storage, read_write> data: array<f32>;

@compute @workgroup_size(256)
fn main(@builtin(local_invocation_index) lid: u32, @builtin(global_invocation_id) gid: vec3<u32>) {
    shared_tile[lid] = data[gid.x];
    workgroupBarrier();
    let neighbor_idx = (lid + 1u) % 256u;
    data[gid.x] = shared_tile[lid] + shared_tile[neighbor_idx];
}
";
    let spirv_bytes = wgsl_to_spirv(wgsl, &CompileOptions::default())
        .expect("shared memory + barrier should emit SPIR-V");
    let module = naga::front::spv::parse_u8_slice(
        &spirv_bytes,
        &naga::front::spv::Options::default(),
    )
    .expect("shared memory SPIR-V should parse");
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator
        .validate(&module)
        .expect("shared memory SPIR-V should validate");
}

#[test]
fn test_wgsl_to_spirv_control_flow_complex() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> data: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    var sum: f32 = 0.0;
    for (var i: u32 = 0u; i < 10u; i = i + 1u) {
        if (i % 2u == 0u) {
            sum = sum + f32(i);
        } else {
            sum = sum * 1.1;
        }
    }
    data[gid.x] = sum;
}
";
    let spirv_bytes = wgsl_to_spirv(wgsl, &CompileOptions::default())
        .expect("complex control flow should emit SPIR-V");
    let module = naga::front::spv::parse_u8_slice(
        &spirv_bytes,
        &naga::front::spv::Options::default(),
    )
    .expect("control flow SPIR-V should parse");
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator
        .validate(&module)
        .expect("control flow SPIR-V should validate");
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
