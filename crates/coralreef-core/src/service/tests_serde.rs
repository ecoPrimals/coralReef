// SPDX-License-Identifier: AGPL-3.0-or-later
//! Wire-contract serde roundtrip tests for compile/health/capability types.

use super::*;
use bytes::Bytes;
use std::sync::Arc;
use types::{
    CompilationInfoResponse, CompileRequest, CompileResponse, CompileSpirvRequestTarpc,
    DeviceCompileResult, DeviceTarget, HealthResponse, MultiDeviceCompileRequest,
    MultiDeviceCompileResponse,
};

#[test]
fn test_compile_response_serde_roundtrip() {
    let resp = CompileResponse {
        binary: Bytes::from(vec![0x01, 0x02, 0x03]),
        size: 3,
        arch: Some("sm_70".to_owned()),
        status: Some("success".to_owned()),
        info: Some(CompilationInfoResponse {
            gpr_count: 24,
            instr_count: 100,
            shared_mem_bytes: 256,
            barrier_count: 1,
            workgroup_size: [64, 1, 1],
            wave_size: 32,
            local_memory: 0,
        }),
        compile_time_ms: Some(42.0),
        dispatch_hints: None,
        spirv_binary: None,
        provenance: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(
        json.contains("\"binary_b64\""),
        "wire field must be binary_b64"
    );
    assert!(json.contains("\"target\""), "wire field must be target");
    assert!(
        json.contains("\"shader_info\""),
        "wire field must be shader_info"
    );
    assert!(json.contains("\"gprs\""), "wire field must be gprs");
    assert!(
        json.contains("\"shared_memory\""),
        "wire field must be shared_memory"
    );
    assert!(json.contains("\"barriers\""), "wire field must be barriers");
    assert!(
        json.contains("\"workgroup\""),
        "wire field must be workgroup"
    );
    assert!(
        json.contains("\"wave_size\""),
        "wire field must include wave_size"
    );
    assert!(
        json.contains("\"local_memory\""),
        "wire field must include local_memory"
    );
    assert!(
        json.contains("\"compile_time_ms\""),
        "wire field must include compile_time_ms"
    );
    let roundtrip: CompileResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtrip.binary.as_ref(), resp.binary.as_ref());
    assert_eq!(roundtrip.size, resp.size);
    assert_eq!(roundtrip.arch, resp.arch);
    assert_eq!(roundtrip.status, resp.status);
    let info = roundtrip.info.expect("info should be present");
    assert_eq!(info.gpr_count, 24);
    assert_eq!(info.shared_mem_bytes, 256);
    assert_eq!(info.barrier_count, 1);
    assert_eq!(info.workgroup_size, [64, 1, 1]);
    assert_eq!(info.wave_size, 32);
    assert_eq!(info.local_memory, 0);
    assert_eq!(roundtrip.compile_time_ms, Some(42.0));
}

#[test]
fn test_compile_response_defaults_from_json() {
    let resp = CompileResponse {
        binary: Bytes::from(vec![1, 2, 3]),
        size: 3,
        arch: None,
        status: None,
        info: None,
        compile_time_ms: None,
        dispatch_hints: None,
        spirv_binary: None,
        provenance: None,
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(!json.contains("\"compile_time_ms\""), "None should skip");
    let roundtrip: CompileResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtrip.binary.as_ref(), &[1, 2, 3]);
    assert_eq!(roundtrip.size, 3);
    assert!(roundtrip.arch.is_none());
    assert!(roundtrip.status.is_none());
    assert!(roundtrip.info.is_none());
    assert!(roundtrip.compile_time_ms.is_none());
}

#[test]
fn test_device_target_serde_roundtrip() {
    let target = DeviceTarget {
        card_index: 1,
        arch: "sm_89".to_owned(),
        pcie_group: Some(2),
    };
    let json = serde_json::to_string(&target).unwrap();
    let roundtrip: DeviceTarget = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtrip.card_index, target.card_index);
    assert_eq!(roundtrip.arch, target.arch);
    assert_eq!(roundtrip.pcie_group, target.pcie_group);
}

#[test]
fn test_device_target_defaults_from_json() {
    let json = r#"{"arch":"sm_70"}"#;
    let target: DeviceTarget = serde_json::from_str(json).unwrap();
    assert_eq!(target.card_index, 0);
    assert!(target.pcie_group.is_none());
}

#[test]
fn test_device_compile_result_serde_roundtrip() {
    let result = DeviceCompileResult {
        card_index: 0,
        arch: "sm_70".to_owned(),
        binary: Some(Bytes::from(vec![0xCA, 0xFE])),
        size: 2,
        error: None,
        info: Some(CompilationInfoResponse {
            gpr_count: 16,
            instr_count: 50,
            shared_mem_bytes: 0,
            barrier_count: 0,
            workgroup_size: [256, 1, 1],
            wave_size: 32,
            local_memory: 0,
        }),
    };
    let json = serde_json::to_string(&result).unwrap();
    let roundtrip: DeviceCompileResult = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtrip.card_index, result.card_index);
    assert_eq!(roundtrip.binary.as_ref(), result.binary.as_ref());
    assert_eq!(roundtrip.error, result.error);
}

#[test]
fn test_device_compile_result_error_skips_binary_in_json() {
    let result = DeviceCompileResult {
        card_index: 1,
        arch: "sm_99".to_owned(),
        binary: None,
        size: 0,
        error: Some("unsupported arch".to_owned()),
        info: None,
    };
    let json = serde_json::to_string(&result).unwrap();
    assert!(
        !json.contains("\"binary_b64\""),
        "None binary should be skipped"
    );
    assert!(json.contains("unsupported arch"));
    let roundtrip: DeviceCompileResult = serde_json::from_str(&json).unwrap();
    assert!(roundtrip.binary.is_none());
    assert_eq!(roundtrip.error.as_deref(), Some("unsupported arch"));
}

#[test]
fn test_multi_device_compile_response_serde_roundtrip() {
    let resp = MultiDeviceCompileResponse {
        results: vec![DeviceCompileResult {
            card_index: 0,
            arch: "sm_70".to_owned(),
            binary: Some(Bytes::from(vec![1, 2, 3])),
            size: 3,
            error: None,
            info: Some(CompilationInfoResponse::default()),
        }],
        success_count: 1,
        total_count: 1,
    };
    let json = serde_json::to_string(&resp).unwrap();
    let roundtrip: MultiDeviceCompileResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtrip.results.len(), 1);
    assert_eq!(roundtrip.success_count, 1);
    assert_eq!(roundtrip.total_count, 1);
}

#[test]
fn test_compile_spirv_request_tarpc_serde_roundtrip() {
    let req = CompileSpirvRequestTarpc {
        spirv: Bytes::from(vec![0x07, 0x23, 0x02, 0x03]),
        arch: "sm_70".to_owned(),
        opt_level: 2,
        fp64_software: true,
    };
    let json = serde_json::to_string(&req).unwrap();
    let roundtrip: CompileSpirvRequestTarpc = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtrip.spirv.as_ref(), req.spirv.as_ref());
    assert_eq!(roundtrip.arch, req.arch);
}

#[test]
fn test_health_response_serde_roundtrip() {
    let health = handle_health();
    let json = serde_json::to_string(&health).unwrap();
    let roundtrip: HealthResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtrip.name.as_ref(), health.name.as_ref());
    assert_eq!(roundtrip.version.as_ref(), health.version.as_ref());
    assert_eq!(roundtrip.supported_archs, health.supported_archs);
}

#[test]
fn test_compile_wgsl_fp64_strategy_software_overrides_bool() {
    use super::compile::handle_compile_wgsl;
    use types::CompileWgslRequest;

    let req = CompileWgslRequest {
        wgsl_source: Arc::from("@compute @workgroup_size(1) fn main() {}"),
        arch: "sm_70".to_owned(),
        opt_level: 2,
        fp64_software: false,
        fp64_strategy: Some("software".to_owned()),
        fma_policy: None,
        precision_advice: None,
        adapter: None,
        emit_spirv: false,
        spirv_version: None,
    };
    let result = handle_compile_wgsl(&req);
    assert!(
        result.is_ok(),
        "fp64_strategy=software should force software path: {result:?}"
    );
}

#[test]
fn test_compile_wgsl_fp64_strategy_native_uses_fp64_software_flag() {
    use super::compile::handle_compile_wgsl;
    use types::CompileWgslRequest;

    let req = CompileWgslRequest {
        wgsl_source: Arc::from("@compute @workgroup_size(1) fn main() {}"),
        arch: "sm_70".to_owned(),
        opt_level: 2,
        fp64_software: true,
        fp64_strategy: Some("native".to_owned()),
        fma_policy: Some("fused".to_owned()),
        precision_advice: None,
        adapter: None,
        emit_spirv: false,
        spirv_version: None,
    };
    let result = handle_compile_wgsl(&req);
    assert!(result.is_ok(), "native strategy should compile: {result:?}");
}

#[test]
fn test_handle_compile_spirv_amd_rdna2_valid_module() {
    use super::compile::handle_compile_spirv;

    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let module = naga::front::wgsl::parse_str(wgsl).expect("WGSL should parse");
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::default(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .expect("module should validate");
    let words =
        naga::back::spv::write_vec(&module, &info, &naga::back::spv::Options::default(), None)
            .expect("SPIR-V write should succeed");
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    let result = handle_compile_spirv(bytes.as_slice(), "rdna2", 2, false);
    assert!(result.is_ok(), "SPIR-V to RDNA2 should succeed: {result:?}");
    let resp = result.expect("amd compile");
    assert_eq!(resp.arch.as_deref(), Some("rdna2"));
}

#[test]
fn test_handle_compile_request_spirv_words_amd() {
    use super::compile::handle_compile;
    use super::types::CompileRequest;

    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let module = naga::front::wgsl::parse_str(wgsl).expect("WGSL should parse");
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::default(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .expect("module should validate");
    let words =
        naga::back::spv::write_vec(&module, &info, &naga::back::spv::Options::default(), None)
            .expect("SPIR-V write should succeed");
    let req = CompileRequest {
        spirv_words: words,
        arch: "gfx1100".to_owned(),
        opt_level: 1,
        fp64_software: false,
    };
    let result = handle_compile(&req);
    assert!(
        result.is_ok(),
        "CompileRequest path for AMD arch should work: {result:?}"
    );
}

// ---- Wire contract serde roundtrip tests for health / capability types ----

#[test]
fn test_health_check_response_serde_roundtrip() {
    use types::HealthCheckResponse;

    let resp = HealthCheckResponse {
        name: "coralReef".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        healthy: true,
        status: "operational".into(),
        supported_archs: vec!["sm_70".to_owned(), "rdna2".to_owned()],
        family_id: "default".into(),
    };
    let json = serde_json::to_string(&resp).expect("serialize");
    let rt: HealthCheckResponse = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(rt.name, resp.name);
    assert_eq!(rt.version, resp.version);
    assert_eq!(rt.healthy, resp.healthy);
    assert_eq!(rt.status, resp.status);
    assert_eq!(rt.supported_archs, resp.supported_archs);
    assert_eq!(rt.family_id, resp.family_id);
}

#[test]
fn test_liveness_response_serde_roundtrip() {
    use types::LivenessResponse;

    let resp = LivenessResponse {
        status: "alive".into(),
    };
    let json = serde_json::to_string(&resp).expect("serialize");
    let rt: LivenessResponse = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(rt.status, "alive");
}

#[test]
fn test_readiness_response_serde_roundtrip() {
    use types::ReadinessResponse;

    let resp = ReadinessResponse {
        ready: true,
        name: "coralReef".into(),
    };
    let json = serde_json::to_string(&resp).expect("serialize");
    let rt: ReadinessResponse = serde_json::from_str(&json).expect("deserialize");
    assert!(rt.ready);
    assert_eq!(rt.name, resp.name);
}

#[test]
fn test_compile_capabilities_response_serde_roundtrip() {
    use types::{CompileCapabilitiesResponse, F64TranscendentalCapabilities};

    let resp = CompileCapabilitiesResponse {
        supported_archs: vec!["sm_70".to_owned(), "sm_86".to_owned(), "rdna2".to_owned()],
        f64_transcendentals: F64TranscendentalCapabilities {
            sin: true,
            cos: true,
            sqrt: true,
            exp2: true,
            log2: true,
            rcp: true,
            exp: true,
            log: true,
            composite_lowering: true,
        },
        math_ops: Some(25),
        sm_target: Some("sm_120".to_owned()),
        atomics: Some(true),
        subgroup_ops: Some(true),
    };
    let json = serde_json::to_string(&resp).expect("serialize");
    assert!(
        json.contains("\"targets\""),
        "Gate 1: wire field must be targets"
    );
    assert!(
        !json.contains("\"supported_archs\""),
        "supported_archs must not appear on wire"
    );
    let rt: CompileCapabilitiesResponse = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(rt.supported_archs, resp.supported_archs);
    assert!(rt.f64_transcendentals.sin);
    assert!(rt.f64_transcendentals.cos);
    assert!(rt.f64_transcendentals.sqrt);
    assert!(rt.f64_transcendentals.exp2);
    assert!(rt.f64_transcendentals.log2);
    assert!(rt.f64_transcendentals.rcp);
    assert!(rt.f64_transcendentals.composite_lowering);
}

#[test]
fn test_tarpc_compile_error_serde_roundtrip() {
    use types::TarpcCompileError;

    let err = TarpcCompileError::from_error("unsupported architecture: foo_bar");
    let json = serde_json::to_string(&err).expect("serialize");
    let rt: TarpcCompileError = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(rt.message, "unsupported architecture: foo_bar");
    assert_eq!(rt.to_string(), err.to_string());
}

#[test]
fn test_provenance_attached_on_with_provenance() {
    let resp = CompileResponse {
        binary: Bytes::from(vec![0xDE, 0xAD, 0xBE, 0xEF]),
        size: 4,
        arch: Some("sm_70".to_owned()),
        status: Some("success".to_owned()),
        info: None,
        compile_time_ms: None,
        dispatch_hints: None,
        spirv_binary: None,
        provenance: None,
    };
    assert!(resp.provenance.is_none());

    let resp = resp.with_provenance();
    let prov = resp.provenance.as_ref().expect("provenance should be set");
    assert_eq!(prov.hash_algorithm, "sha256");
    assert_eq!(prov.content_hash.len(), 64, "SHA-256 hex is 64 chars");
    assert!(!prov.compiler_version.is_empty());
    assert!(!prov.gate_of_compilation.is_empty());
    assert!(prov.signature.is_none(), "no bearDog signing yet");
}

#[test]
fn test_provenance_serde_roundtrip() {
    let resp = CompileResponse {
        binary: Bytes::from(vec![1, 2, 3, 4, 5]),
        size: 5,
        arch: Some("rdna2".to_owned()),
        status: Some("success".to_owned()),
        info: None,
        compile_time_ms: Some(1.5),
        dispatch_hints: None,
        spirv_binary: None,
        provenance: None,
    }
    .with_provenance();

    let json = serde_json::to_string(&resp).expect("serialize");
    assert!(json.contains("\"provenance\""), "provenance in JSON");
    assert!(json.contains("\"content_hash\""), "content_hash in JSON");
    assert!(json.contains("\"sha256\""), "hash_algorithm in JSON");

    let rt: CompileResponse = serde_json::from_str(&json).expect("deserialize");
    let prov = rt.provenance.expect("provenance roundtrip");
    assert_eq!(prov.content_hash, resp.provenance.unwrap().content_hash);
}

#[test]
fn test_provenance_hash_deterministic() {
    let binary = vec![42u8; 128];
    let resp1 = CompileResponse {
        binary: Bytes::from(binary.clone()),
        size: 128,
        arch: None,
        status: None,
        info: None,
        compile_time_ms: None,
        dispatch_hints: None,
        spirv_binary: None,
        provenance: None,
    }
    .with_provenance();
    let resp2 = CompileResponse {
        binary: Bytes::from(binary),
        size: 128,
        arch: None,
        status: None,
        info: None,
        compile_time_ms: None,
        dispatch_hints: None,
        spirv_binary: None,
        provenance: None,
    }
    .with_provenance();
    assert_eq!(
        resp1.provenance.unwrap().content_hash,
        resp2.provenance.unwrap().content_hash,
        "same binary should produce same hash"
    );
}
