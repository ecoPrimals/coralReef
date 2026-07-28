// SPDX-License-Identifier: AGPL-3.0-or-later
//! SPIR-V emission, version targeting, and provenance pipeline tests.

use super::*;
use std::sync::Arc;
use types::CompileWgslRequest;

#[test]
fn test_compile_wgsl_emits_sovereign_spirv() {
    let req = CompileWgslRequest {
        wgsl_source: Arc::from("@compute @workgroup_size(1) fn main() {}"),
        arch: "sm_70".to_owned(),
        opt_level: 2,
        fp64_software: false,
        fp64_strategy: None,
        fma_policy: None,
        precision_advice: None,
        adapter: None,
        emit_spirv: true,
        spirv_version: None,
    };
    let resp = handle_compile_wgsl(&req).expect("should compile");
    let spirv = resp
        .spirv_binary
        .expect("WGSL compile must emit sovereign SPIR-V");
    assert!(spirv.len() >= 20, "SPIR-V must be non-trivial");
    assert_eq!(spirv.len() % 4, 0, "SPIR-V must be word-aligned");
    let magic = u32::from_le_bytes([spirv[0], spirv[1], spirv[2], spirv[3]]);
    assert_eq!(magic, 0x0723_0203, "SPIR-V must have correct magic number");
}

#[test]
fn test_compile_wgsl_no_spirv_when_emit_false() {
    let req = CompileWgslRequest {
        wgsl_source: Arc::from("@compute @workgroup_size(1) fn main() {}"),
        arch: "sm_70".to_owned(),
        opt_level: 2,
        fp64_software: false,
        fp64_strategy: None,
        fma_policy: None,
        precision_advice: None,
        adapter: None,
        emit_spirv: false,
        spirv_version: None,
    };
    let resp = handle_compile_wgsl(&req).expect("should compile");
    assert!(
        resp.spirv_binary.is_none(),
        "spirv_binary should be None when emit_spirv=false"
    );
}

#[test]
fn test_compile_wgsl_spirv_version_targeting() {
    let req = CompileWgslRequest {
        wgsl_source: Arc::from("@compute @workgroup_size(1) fn main() {}"),
        arch: "sm_70".to_owned(),
        opt_level: 2,
        fp64_software: false,
        fp64_strategy: None,
        fma_policy: None,
        precision_advice: None,
        adapter: None,
        emit_spirv: true,
        spirv_version: Some([1, 5]),
    };
    let resp = handle_compile_wgsl(&req).expect("should compile");
    let spirv = resp.spirv_binary.expect("SPIR-V should be emitted");
    let ver_word = u32::from_le_bytes([spirv[4], spirv[5], spirv[6], spirv[7]]);
    let expected = (1u32 << 16) | (5u32 << 8);
    assert_eq!(
        ver_word, expected,
        "should emit SPIR-V 1.5: {ver_word:#010x} vs {expected:#010x}"
    );
}

#[test]
fn test_spirv_end_to_end_compile_provenance_output() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> data: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    data[idx] = data[idx] * 2.0 + 1.0;
}
";
    let req = CompileWgslRequest {
        wgsl_source: Arc::from(wgsl),
        arch: "sm_80".to_owned(),
        opt_level: 2,
        fp64_software: false,
        fp64_strategy: None,
        fma_policy: None,
        precision_advice: None,
        adapter: None,
        emit_spirv: true,
        spirv_version: Some([1, 5]),
    };

    let resp = handle_compile_wgsl(&req).expect("compile should succeed");

    assert!(!resp.binary.is_empty(), "native binary must be non-empty");
    assert_eq!(
        resp.status.as_deref(),
        Some("success"),
        "status must be success"
    );

    let spirv = resp
        .spirv_binary
        .clone()
        .expect("SPIR-V output must be present");
    assert!(spirv.len() >= 20, "SPIR-V must be non-trivial");
    assert_eq!(spirv.len() % 4, 0, "SPIR-V must be word-aligned");
    let magic = u32::from_le_bytes([spirv[0], spirv[1], spirv[2], spirv[3]]);
    assert_eq!(magic, 0x0723_0203, "SPIR-V magic number");
    let version = u32::from_le_bytes([spirv[4], spirv[5], spirv[6], spirv[7]]);
    let expected_ver = (1u32 << 16) | (5u32 << 8);
    assert_eq!(version, expected_ver, "SPIR-V version should be 1.5");

    let resp_with_prov = resp.with_provenance();
    let prov = resp_with_prov
        .provenance
        .as_ref()
        .expect("provenance must be attached");
    assert_eq!(prov.hash_algorithm, "sha256");
    assert_eq!(prov.content_hash.len(), 64, "SHA-256 hex is 64 chars");
    assert!(!prov.compiler_version.is_empty());
    assert!(!prov.gate_of_compilation.is_empty());

    let spirv_word_count = spirv.chunks_exact(4).count();
    let module = naga::front::spv::parse_u8_slice(&spirv, &naga::front::spv::Options::default())
        .expect("emitted SPIR-V must be parseable");
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator
        .validate(&module)
        .expect("emitted SPIR-V must pass naga validation");

    assert!(
        !module.entry_points.is_empty(),
        "SPIR-V must have at least one entry point"
    );
    assert_eq!(
        module.entry_points[0].stage,
        naga::ShaderStage::Compute,
        "entry point must be compute"
    );
    assert!(spirv_word_count > 20, "non-trivial SPIR-V module");
}

#[test]
fn test_compile_spirv_has_no_sovereign_spirv() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let module = naga::front::wgsl::parse_str(wgsl).expect("WGSL should parse");
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::default(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .expect("should validate");
    let spirv_words =
        naga::back::spv::write_vec(&module, &info, &naga::back::spv::Options::default(), None)
            .expect("should produce SPIR-V");
    let spirv_bytes: Vec<u8> = spirv_words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let resp = handle_compile_spirv(&spirv_bytes, "sm_70", 2, true).expect("should compile");
    assert!(
        resp.spirv_binary.is_none(),
        "SPIR-V input path should not re-emit SPIR-V"
    );
}
