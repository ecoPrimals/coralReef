// SPDX-License-Identifier: AGPL-3.0-or-later

//! Multi-stage ML pipeline composition tests.
//!
//! Validates the sequential compile-and-dispatch pattern documented in
//! `SHADER_COMPILE_WIRE_CONTRACT.md` § Multi-Stage ML Pipelines. Each ML stage
//! (tokenizer, attention, FFN) is compiled independently, and `CompilationInfo`
//! is available for dispatch decisions.

use std::sync::Arc;

use super::compile;
use super::types::{CompileResponse, CompileWgslRequest};

const TOKENIZER_WGSL: &str = "\
@group(0) @binding(0) var<storage, read> tokens: array<u32>;
@group(0) @binding(1) var<storage, read_write> embeddings: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if idx < arrayLength(&tokens) {
        embeddings[idx] = f32(tokens[idx]) * 0.01;
    }
}";

const ATTENTION_WGSL: &str = "\
@group(0) @binding(0) var<storage, read> q: array<f32>;
@group(0) @binding(1) var<storage, read> k: array<f32>;
@group(0) @binding(2) var<storage, read_write> attn_out: array<f32>;
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if idx < arrayLength(&q) {
        attn_out[idx] = q[idx] * k[idx];
    }
}";

const FFN_WGSL: &str = "\
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;
@compute @workgroup_size(128)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if idx < arrayLength(&input) {
        let x = input[idx];
        output[idx] = max(x, 0.0);
    }
}";

fn compile_stage(source: &str, arch: &str) -> CompileResponse {
    let req = CompileWgslRequest {
        wgsl_source: Arc::from(source),
        arch: arch.to_owned(),
        opt_level: 2,
        fp64_software: false,
        fp64_strategy: None,
        fma_policy: None,
    };
    compile::handle_compile_wgsl(&req).expect("stage should compile")
}

#[test]
fn test_ml_pipeline_three_stage_sequential_compile() {
    let stages = [
        ("tokenizer", TOKENIZER_WGSL),
        ("attention", ATTENTION_WGSL),
        ("ffn", FFN_WGSL),
    ];
    let mut binaries = Vec::new();
    for (name, source) in &stages {
        let resp = compile_stage(source, "sm_70");
        assert!(resp.size > 0, "{name} should produce a non-empty binary");
        assert!(resp.info.is_some(), "{name} should include CompilationInfo");
        binaries.push((name, resp));
    }

    for (name, resp) in &binaries {
        let info = resp.info.as_ref().unwrap();
        assert!(info.gpr_count > 0, "{name} should use at least 1 GPR");
        assert!(info.instr_count > 0, "{name} should have instructions");
    }

    assert_ne!(
        binaries[0].1.binary, binaries[1].1.binary,
        "tokenizer and attention should produce different binaries"
    );
    assert_ne!(
        binaries[1].1.binary, binaries[2].1.binary,
        "attention and FFN should produce different binaries"
    );
}

#[test]
fn test_ml_pipeline_stages_have_distinct_workgroup_sizes() {
    let tok = compile_stage(TOKENIZER_WGSL, "sm_70");
    let attn = compile_stage(ATTENTION_WGSL, "sm_70");
    let ffn = compile_stage(FFN_WGSL, "sm_70");

    let tok_ws = tok.info.as_ref().unwrap().workgroup_size;
    let attn_ws = attn.info.as_ref().unwrap().workgroup_size;
    let ffn_ws = ffn.info.as_ref().unwrap().workgroup_size;

    assert_eq!(tok_ws, [64, 1, 1]);
    assert_eq!(attn_ws, [256, 1, 1]);
    assert_eq!(ffn_ws, [128, 1, 1]);
}

#[test]
fn test_ml_pipeline_cross_vendor_same_stages() {
    for (arch, label) in [("sm_70", "nvidia"), ("rdna2", "amd")] {
        let tok = compile_stage(TOKENIZER_WGSL, arch);
        let attn = compile_stage(ATTENTION_WGSL, arch);
        let ffn = compile_stage(FFN_WGSL, arch);

        assert!(tok.size > 0, "{label} tokenizer binary");
        assert!(attn.size > 0, "{label} attention binary");
        assert!(ffn.size > 0, "{label} FFN binary");

        assert!(tok.info.is_some(), "{label} tokenizer info");
        assert!(attn.info.is_some(), "{label} attention info");
        assert!(ffn.info.is_some(), "{label} FFN info");
    }
}

#[test]
fn test_ml_pipeline_compilation_info_for_occupancy_planning() {
    let stages = [
        ("tokenizer", TOKENIZER_WGSL),
        ("attention", ATTENTION_WGSL),
        ("ffn", FFN_WGSL),
    ];

    let mut max_gprs: u32 = 0;
    let mut max_shared_mem: u32 = 0;
    for (name, source) in &stages {
        let resp = compile_stage(source, "sm_70");
        let info = resp
            .info
            .as_ref()
            .unwrap_or_else(|| panic!("{name} must have CompilationInfo"));

        max_gprs = max_gprs.max(info.gpr_count);
        max_shared_mem = max_shared_mem.max(info.shared_mem_bytes);

        assert!(
            info.barrier_count == 0,
            "{name}: single-workgroup stages should have 0 barriers"
        );
    }

    assert!(
        max_gprs <= 255,
        "max GPR usage across stages should fit in SM70 register file ({max_gprs})"
    );
    assert!(
        max_shared_mem <= 48 * 1024,
        "max shared memory per stage should fit in SM70 48KB limit ({max_shared_mem})"
    );
}

#[test]
fn test_ml_pipeline_stage_independence() {
    let tok_1 = compile_stage(TOKENIZER_WGSL, "sm_70");
    let _attn = compile_stage(ATTENTION_WGSL, "sm_70");
    let tok_2 = compile_stage(TOKENIZER_WGSL, "sm_70");

    assert_eq!(
        tok_1.binary, tok_2.binary,
        "same source + same arch should produce identical binaries regardless of interleaved compiles"
    );
    assert_eq!(
        tok_1.info.as_ref().unwrap().gpr_count,
        tok_2.info.as_ref().unwrap().gpr_count
    );
    assert_eq!(
        tok_1.info.as_ref().unwrap().instr_count,
        tok_2.info.as_ref().unwrap().instr_count
    );
}

#[test]
fn test_ml_pipeline_response_serde_roundtrip_with_info() {
    let resp = compile_stage(ATTENTION_WGSL, "sm_70");
    let json = serde_json::to_string(&resp).expect("serialize");
    let roundtrip: CompileResponse = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(roundtrip.size, resp.size);
    assert!(roundtrip.info.is_some());
    let orig = resp.info.unwrap();
    let rt = roundtrip.info.unwrap();
    assert_eq!(orig.gpr_count, rt.gpr_count);
    assert_eq!(orig.instr_count, rt.instr_count);
    assert_eq!(orig.shared_mem_bytes, rt.shared_mem_bytes);
    assert_eq!(orig.barrier_count, rt.barrier_count);
    assert_eq!(orig.workgroup_size, rt.workgroup_size);
}
