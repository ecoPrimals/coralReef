// SPDX-License-Identifier: AGPL-3.0-or-later

use super::compile;
use super::*;
use compile::parse_target;
use coral_reef::{AmdArch, FmaPolicy, GpuArch, GpuTarget, NvArch};
use std::sync::Arc;
use types::{
    CompileRequest, CompileResponse, CompileWgslRequest, DeviceTarget, MultiDeviceCompileRequest,
};

#[test]
fn test_handle_compile_spirv_valid_minimal() {
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
    let result = handle_compile_spirv(&bytes, "sm_70", 2, true);
    assert!(
        result.is_ok(),
        "valid minimal SPIR-V should compile: {result:?}"
    );
    let resp = result.unwrap();
    assert!(resp.size > 0);
    assert_eq!(resp.arch.as_deref(), Some("sm_70"));
    assert_eq!(resp.status.as_deref(), Some("success"));
}

#[test]
fn test_bytes_to_spirv_words_exactly_four_bytes() {
    let four_bytes = 0x0723_0203u32.to_le_bytes();
    let result = handle_compile_spirv(four_bytes, "sm_70", 2, true);
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string().to_lowercase();
    assert!(
        !err_msg.contains("multiple of 4"),
        "4 bytes should pass bytes_to_spirv_words; error was: {err_msg}"
    );
}

#[test]
fn test_handle_compile_capabilities() {
    let caps = handle_compile_capabilities();
    assert!(!caps.supported_archs.is_empty());
    assert!(caps.f64_transcendentals.sin);
    assert!(caps.f64_transcendentals.cos);
    assert!(caps.f64_transcendentals.sqrt);
    assert!(caps.f64_transcendentals.exp2);
    assert!(caps.f64_transcendentals.log2);
    assert!(caps.f64_transcendentals.rcp);
    assert!(caps.f64_transcendentals.exp);
    assert!(caps.f64_transcendentals.log);
    assert!(caps.f64_transcendentals.composite_lowering);
}

#[test]
fn test_handle_health_check() {
    let resp = handle_health_check();
    assert!(resp.healthy);
    assert_eq!(resp.name, env!("CARGO_PKG_NAME"));
    assert!(!resp.version.is_empty());
    assert!(!resp.supported_archs.is_empty());
    assert!(!resp.family_id.is_empty());
}

#[test]
fn test_handle_capability_list() {
    let resp = handle_capability_list();
    assert_eq!(resp.primal.as_ref(), env!("CARGO_PKG_NAME"));
    assert_eq!(resp.version.as_ref(), env!("CARGO_PKG_VERSION"));
    assert!(resp.methods.iter().any(|m| m == "shader.compile.wgsl"));
    assert!(resp.methods.iter().any(|m| m == "capability.list"));
    assert!(resp.capabilities.iter().any(|d| d == "shader.compile"));
    assert!(resp.capabilities.iter().any(|d| d == "shader.health"));
    assert!(resp.capabilities.iter().any(|d| d == "health"));
    assert!(resp.capabilities.iter().any(|d| d == "identity"));
    let sorted = {
        let mut v = resp.capabilities.clone();
        v.sort();
        v
    };
    assert_eq!(
        resp.capabilities, sorted,
        "capability domains must be sorted for stable discovery"
    );
}

#[test]
fn capability_list_wire_standard_l3() {
    let resp = handle_capability_list();
    assert_eq!(resp.primal.as_ref(), env!("CARGO_PKG_NAME"));
    assert!(!resp.version.is_empty());
    assert!(!resp.methods.is_empty());
    // Wire Standard L2: every method is dotted notation
    for method in &resp.methods {
        assert!(
            method.contains('.'),
            "method must use dotted notation: {method}"
        );
    }
    // Required methods per wire standard
    assert!(resp.methods.contains(&"health.check".to_string()));
    assert!(resp.methods.contains(&"health.liveness".to_string()));
    assert!(resp.methods.contains(&"identity.get".to_string()));
    assert!(resp.methods.contains(&"capability.list".to_string()));
    // Wire Standard L3: protocol and transport fields
    assert_eq!(resp.protocol.as_ref(), "jsonrpc-2.0");
    assert!(!resp.transport.is_empty());
    assert!(resp.transport.iter().any(|t| t.as_ref() == "tcp"));
}

#[test]
fn test_handle_health_liveness() {
    let resp = handle_health_liveness();
    assert_eq!(resp.status, "alive");
}

#[test]
fn test_handle_health_readiness() {
    mark_startup();
    let resp = handle_health_readiness();
    assert!(resp.ready);
    assert_eq!(resp.name, env!("CARGO_PKG_NAME"));
}

#[test]
fn test_handle_identity_get_without_advertised_transports() {
    let resp = handle_identity_get();
    assert_eq!(resp.name.as_ref(), env!("CARGO_PKG_NAME"));
    assert!(!resp.provides.is_empty());
}

#[test]
fn test_handle_health_returns_all_architectures() {
    let health = handle_health();
    let expected_nv: Vec<String> = NvArch::ALL.iter().map(ToString::to_string).collect();
    let expected_amd: Vec<String> = AmdArch::ALL.iter().map(ToString::to_string).collect();
    for arch in &expected_nv {
        assert!(
            health.supported_archs.contains(arch),
            "handle_health should include NvArch {arch}"
        );
    }
    for arch in &expected_amd {
        assert!(
            health.supported_archs.contains(arch),
            "handle_health should include AmdArch {arch}"
        );
    }
    assert_eq!(
        health.supported_archs.len(),
        expected_nv.len() + expected_amd.len(),
        "handle_health should return exactly NvArch::ALL + AmdArch::ALL"
    );
}

#[test]
fn test_parse_target_nvidia_variants() {
    assert_eq!(
        parse_target("sm_70").unwrap(),
        GpuTarget::Nvidia(NvArch::Sm70)
    );
    assert_eq!(
        parse_target("sm70").unwrap(),
        GpuTarget::Nvidia(NvArch::Sm70)
    );
    assert_eq!(
        parse_target("sm_89").unwrap(),
        GpuTarget::Nvidia(NvArch::Sm89)
    );
}

#[test]
fn test_parse_target_invalid() {
    assert!(parse_target("sm_99").is_err());
    assert!(parse_target("").is_err());
    assert!(parse_target("unknown_gpu").is_err());
}

#[test]
fn test_parse_target_amd() {
    let t = parse_target("rdna2").unwrap();
    assert_eq!(t, GpuTarget::Amd(AmdArch::Rdna2));
    let t2 = parse_target("gfx1100").unwrap();
    assert_eq!(t2, GpuTarget::Amd(AmdArch::Rdna3));
}

#[test]
fn test_health_response() {
    let health = handle_health();
    assert_eq!(health.name, env!("CARGO_PKG_NAME"));
    assert!(!health.supported_archs.is_empty());
    assert!(health.supported_archs.iter().any(|a| a.contains("sm_")));
    assert!(health.supported_archs.iter().any(|a| a.contains("rdna")));
}

#[test]
fn test_compile_request_empty_spirv() {
    let req = CompileRequest {
        spirv_words: vec![],
        arch: GpuArch::default().to_string(),
        opt_level: 2,
        fp64_software: true,
    };
    assert!(handle_compile(&req).is_err());
}

#[test]
fn test_compile_wgsl_empty() {
    let req = CompileWgslRequest {
        wgsl_source: Arc::from(""),
        arch: "sm_70".to_owned(),
        opt_level: 2,
        fp64_software: true,
        fp64_strategy: None,
        fma_policy: None,
        precision_advice: None,
        adapter: None,
        emit_spirv: false,
        spirv_version: None,
    };
    assert!(handle_compile_wgsl(&req).is_err());
}

#[test]
fn test_handle_compile_spirv_invalid_length() {
    let bytes = vec![0u8; 5];
    let result = handle_compile_spirv(&bytes, "sm_70", 2, true);
    assert!(result.is_err());
    let e = result.unwrap_err();
    assert!(e.to_string().to_lowercase().contains("multiple of 4"));
}

#[test]
fn test_handle_compile_spirv_empty() {
    let bytes: Vec<u8> = vec![];
    let result = handle_compile_spirv(&bytes, "sm_70", 2, true);
    assert!(result.is_err());
    let e = result.unwrap_err();
    assert!(e.to_string().to_lowercase().contains("empty"));
}

#[test]
fn test_handle_compile_spirv_unsupported_arch() {
    let bytes = vec![0u8; 8];
    let result = handle_compile_spirv(&bytes, "sm_99", 2, true);
    assert!(result.is_err());
    let e = result.unwrap_err();
    assert!(e.to_string().to_lowercase().contains("unsupported"));
}

#[test]
fn test_handle_compile_wgsl_unsupported_arch() {
    let req = CompileWgslRequest {
        wgsl_source: Arc::from("@compute @workgroup_size(1) fn main() {}"),
        arch: "unknown_gpu".to_owned(),
        opt_level: 2,
        fp64_software: true,
        fp64_strategy: None,
        fma_policy: None,
        precision_advice: None,
        adapter: None,
        emit_spirv: false,
        spirv_version: None,
    };
    let result = handle_compile_wgsl(&req);
    assert!(result.is_err());
    let e = result.unwrap_err();
    assert!(e.to_string().to_lowercase().contains("unsupported"));
}

#[test]
fn test_parse_target_intel_not_supported() {
    assert!(parse_target("xe_hpg").is_err());
}

#[test]
fn test_parse_fma_policy_variants() {
    assert_eq!(compile::parse_fma_policy(Some("fused")), FmaPolicy::Fused);
    assert_eq!(
        compile::parse_fma_policy(Some("separate")),
        FmaPolicy::Separate
    );
    assert_eq!(compile::parse_fma_policy(Some("auto")), FmaPolicy::Auto);
    assert_eq!(compile::parse_fma_policy(None), FmaPolicy::Auto);
    assert_eq!(compile::parse_fma_policy(Some("unknown")), FmaPolicy::Auto);
}

#[test]
fn test_compile_wgsl_with_fma_separate() {
    let req = CompileWgslRequest {
        wgsl_source: Arc::from("@compute @workgroup_size(1) fn main() {}"),
        arch: "sm_70".to_owned(),
        opt_level: 2,
        fp64_software: false,
        fp64_strategy: None,
        fma_policy: Some("separate".to_owned()),
        precision_advice: None,
        adapter: None,
        emit_spirv: false,
        spirv_version: None,
    };
    let result = handle_compile_wgsl(&req);
    assert!(result.is_ok(), "FMA separate should compile: {result:?}");
}

// --- SPIR-V pipeline and multi-device tests live in dedicated submodules ---

// --- types.rs serde and default value tests ---

#[test]
fn test_compile_request_serde_roundtrip() {
    let req = CompileRequest {
        spirv_words: vec![0x0723_0203, 0x0001_0000],
        arch: "sm_70".to_owned(),
        opt_level: 2,
        fp64_software: true,
    };
    let json = serde_json::to_string(&req).unwrap();
    let roundtrip: CompileRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtrip.spirv_words, req.spirv_words);
    assert_eq!(roundtrip.arch, req.arch);
    assert_eq!(roundtrip.opt_level, req.opt_level);
    assert_eq!(roundtrip.fp64_software, req.fp64_software);
}

#[test]
fn test_compile_request_defaults_from_json() {
    let json = r#"{"spirv_words":[1,2,3,4]}"#;
    let req: CompileRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.arch, coral_reef::GpuArch::default().to_string());
    assert_eq!(req.opt_level, 2);
    assert!(!req.fp64_software);
}

#[test]
fn test_compile_wgsl_request_serde_roundtrip() {
    let req = CompileWgslRequest {
        wgsl_source: Arc::from("fn main() {}"),
        arch: "sm_80".to_owned(),
        opt_level: 3,
        fp64_software: false,
        fp64_strategy: Some("native".to_owned()),
        fma_policy: Some("fused".to_owned()),
        precision_advice: None,
        adapter: None,
        emit_spirv: false,
        spirv_version: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    let roundtrip: CompileWgslRequest = serde_json::from_str(&json).unwrap();
    assert_eq!(roundtrip.wgsl_source.as_ref(), req.wgsl_source.as_ref());
    assert_eq!(roundtrip.arch, req.arch);
    assert_eq!(roundtrip.fp64_strategy.as_deref(), Some("native"));
    assert_eq!(roundtrip.fma_policy.as_deref(), Some("fused"));
}

#[test]
fn test_compile_wgsl_request_defaults_from_json() {
    let json = r#"{"wgsl_source":"fn main() {}"}"#;
    let req: CompileWgslRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.arch, coral_reef::GpuArch::default().to_string());
    assert_eq!(req.opt_level, 2);
    assert!(req.fp64_strategy.is_none());
    assert!(req.fma_policy.is_none());
}

#[test]
fn test_compile_wgsl_request_source_alias() {
    let json = r#"{"source":"@compute @workgroup_size(1) fn main() {}"}"#;
    let req: CompileWgslRequest = serde_json::from_str(json).unwrap();
    assert_eq!(
        req.wgsl_source.as_ref(),
        "@compute @workgroup_size(1) fn main() {}"
    );
}

#[test]
fn test_multi_device_request_source_alias() {
    let json = r#"{"source":"fn main() {}","targets":[{"card_index":0,"arch":"sm_70"}]}"#;
    let req: MultiDeviceCompileRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.wgsl_source.as_ref(), "fn main() {}");
}

#[test]
fn test_compile_response_legacy_field_aliases() {
    let json = r#"{
        "binary": [1, 2, 3],
        "size": 3,
        "info": {"gprs": 16, "instr_count": 50, "shared_memory": 0, "barriers": 0, "workgroup": [1,1,1], "wave_size": 32, "local_memory": 0}
    }"#;
    let resp: CompileResponse = serde_json::from_str(json).unwrap();
    assert_eq!(resp.binary.as_ref(), &[1, 2, 3]);
    assert!(resp.info.is_some());
    assert_eq!(resp.info.unwrap().gpr_count, 16);
}

#[test]
fn health_standard_returns_alive_with_uptime() {
    mark_startup();
    let resp = handle_health_standard();
    assert_eq!(resp["status"], "alive");
    assert_eq!(resp["primal"], "coralreef-core");
    assert!(resp["version"].as_str().is_some());
    assert!(resp["uptime_s"].as_u64().is_some());
}

#[test]
fn resolve_arch_uses_explicit_arch_when_not_default() {
    let arch = super::compile::parse_target("sm_86").expect("sm_86 should parse");
    let _ = arch;
    let desc = types::AdapterDescriptor {
        vendor_id: 0x10DE,
        device_name: "RTX 4090".into(),
        device_type: "DiscreteGpu".into(),
    };
    let req = types::CompileWgslRequest {
        wgsl_source: std::sync::Arc::from("@compute @workgroup_size(1) fn main() {}"),
        arch: "sm_86".into(),
        opt_level: 2,
        fp64_software: false,
        fp64_strategy: None,
        fma_policy: None,
        precision_advice: None,
        adapter: Some(desc),
        emit_spirv: false,
        spirv_version: None,
    };
    let result = super::compile::handle_compile_wgsl(&req);
    assert!(result.is_ok());
    let resp = result.unwrap();
    assert_eq!(resp.arch.as_deref(), Some("sm_86"));
}

#[test]
fn resolve_arch_infers_from_adapter_when_default() {
    let desc = types::AdapterDescriptor {
        vendor_id: 0x10DE,
        device_name: "NVIDIA GeForce RTX 4090".into(),
        device_type: "DiscreteGpu".into(),
    };
    let req = types::CompileWgslRequest {
        wgsl_source: std::sync::Arc::from("@compute @workgroup_size(1) fn main() {}"),
        arch: types::default_arch(),
        opt_level: 2,
        fp64_software: false,
        fp64_strategy: None,
        fma_policy: None,
        precision_advice: None,
        adapter: Some(desc),
        emit_spirv: false,
        spirv_version: None,
    };
    let result = super::compile::handle_compile_wgsl(&req);
    assert!(result.is_ok());
    let resp = result.unwrap();
    assert_eq!(
        resp.arch.as_deref(),
        Some("sm_89"),
        "RTX 4090 should infer sm_89"
    );
}

#[test]
fn dispatch_hint_tensor_core_for_f16_tier() {
    let req = types::CompileWgslRequest {
        wgsl_source: std::sync::Arc::from("@compute @workgroup_size(1) fn main() {}"),
        arch: "sm_80".into(),
        opt_level: 2,
        fp64_software: false,
        fp64_strategy: None,
        fma_policy: None,
        precision_advice: Some(types::PrecisionAdvice {
            tier: "F16".into(),
            needs_transcendental_lowering: false,
            df64_naga_poisoned: false,
            domain: None,
        }),
        adapter: None,
        emit_spirv: false,
        spirv_version: None,
    };
    let result = super::compile::handle_compile_wgsl(&req).unwrap();
    let hints = result.dispatch_hints.expect("should have hints");
    assert_eq!(hints.hardware_hint.as_ref(), "tensor_core");
    assert_eq!(hints.execution_model.as_deref(), Some("simt"));
}

#[test]
fn dispatch_hint_compute_for_f64_tier() {
    let req = types::CompileWgslRequest {
        wgsl_source: std::sync::Arc::from("@compute @workgroup_size(1) fn main() {}"),
        arch: "sm_80".into(),
        opt_level: 2,
        fp64_software: false,
        fp64_strategy: None,
        fma_policy: None,
        precision_advice: Some(types::PrecisionAdvice {
            tier: "F64".into(),
            needs_transcendental_lowering: false,
            df64_naga_poisoned: false,
            domain: Some("physics.simulation".into()),
        }),
        adapter: None,
        emit_spirv: false,
        spirv_version: None,
    };
    let result = super::compile::handle_compile_wgsl(&req).unwrap();
    let hints = result.dispatch_hints.expect("should have hints");
    assert_eq!(hints.hardware_hint.as_ref(), "compute");
}

#[test]
fn compile_wgsl_provenance_attached() {
    let req = types::CompileWgslRequest {
        wgsl_source: std::sync::Arc::from("@compute @workgroup_size(1) fn main() {}"),
        arch: "sm_70".into(),
        opt_level: 2,
        fp64_software: false,
        fp64_strategy: None,
        fma_policy: None,
        precision_advice: None,
        adapter: None,
        emit_spirv: false,
        spirv_version: None,
    };
    let resp = super::compile::handle_compile_wgsl(&req)
        .unwrap()
        .with_provenance();
    let prov = resp.provenance.expect("provenance should be attached");
    assert_eq!(prov.hash_algorithm, "sha256");
    assert_eq!(
        prov.content_hash.len(),
        64,
        "SHA-256 hex should be 64 chars"
    );
    assert!(!prov.gate_of_compilation.is_empty());
    assert!(!prov.compiler_version.is_empty());
    assert!(
        prov.signature.is_none(),
        "no crypto-domain provider available, signature should be None"
    );
}

#[test]
fn compile_gemm_sm80_succeeds() {
    let req = types::GemmCompileRequest {
        m: 128,
        n: 128,
        k: 32,
        precision: "f16f32".into(),
        arch: "sm_80".into(),
    };
    let result = super::compile::handle_compile_gemm(&req);
    assert!(result.is_ok(), "GEMM compile should succeed: {result:?}");
    let resp = result.unwrap();
    assert!(resp.size > 0);
    let hints = resp
        .dispatch_hints
        .expect("GEMM should have dispatch hints");
    assert_eq!(hints.hardware_hint.as_ref(), "tensor_core");
    assert_eq!(hints.binary_format.as_deref(), Some("ptx"));
    assert!(resp.info.is_some());
}

#[test]
fn compile_gemm_invalid_precision() {
    let req = types::GemmCompileRequest {
        m: 64,
        n: 64,
        k: 16,
        precision: "int4".into(),
        arch: "sm_80".into(),
    };
    let result = super::compile::handle_compile_gemm(&req);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("unknown GEMM precision"));
}

#[test]
fn compile_gemm_unsupported_arch() {
    let req = types::GemmCompileRequest {
        m: 64,
        n: 64,
        k: 16,
        precision: "f16".into(),
        arch: "sm_70".into(),
    };
    let result = super::compile::handle_compile_gemm(&req);
    assert!(result.is_err(), "GEMM on sm_70 should fail (needs SM80+)");
}

#[test]
fn binary_format_for_amd_is_isa() {
    let req = types::CompileWgslRequest {
        wgsl_source: std::sync::Arc::from("@compute @workgroup_size(1) fn main() {}"),
        arch: "rdna2".into(),
        opt_level: 2,
        fp64_software: false,
        fp64_strategy: None,
        fma_policy: None,
        precision_advice: None,
        adapter: None,
        emit_spirv: false,
        spirv_version: None,
    };
    let result = super::compile::handle_compile_wgsl(&req).unwrap();
    let hints = result.dispatch_hints.expect("should have hints");
    assert_eq!(hints.binary_format.as_deref(), Some("isa"));
}

#[test]
fn adapter_descriptor_amd_inference() {
    let desc = types::AdapterDescriptor {
        vendor_id: 0x1002,
        device_name: "AMD Radeon RX 7900 XTX".into(),
        device_type: "DiscreteGpu".into(),
    };
    let req = types::CompileWgslRequest {
        wgsl_source: std::sync::Arc::from("@compute @workgroup_size(1) fn main() {}"),
        arch: types::default_arch(),
        opt_level: 2,
        fp64_software: false,
        fp64_strategy: None,
        fma_policy: None,
        precision_advice: None,
        adapter: Some(desc),
        emit_spirv: false,
        spirv_version: None,
    };
    let result = super::compile::handle_compile_wgsl(&req);
    assert!(result.is_ok());
    let resp = result.unwrap();
    assert_eq!(
        resp.arch.as_deref(),
        Some("rdna3"),
        "RX 7900 XTX should infer rdna3"
    );
}

#[test]
fn handle_health_version_returns_build_info() {
    let resp = handle_health_version();
    assert_eq!(resp.name.as_ref(), env!("CARGO_PKG_NAME"));
    assert_eq!(resp.version.as_ref(), env!("CARGO_PKG_VERSION"));
    assert!(!resp.build_hash.is_empty());
    assert!(!resp.session.is_empty());
}

#[test]
fn handle_health_readiness_returns_ready() {
    mark_startup();
    let resp = handle_health_readiness();
    assert!(resp.ready);
    assert!(!resp.name.is_empty());
}

#[test]
fn set_identity_from_self_description_populates_response() {
    let desc = crate::capability::self_description();
    let transports = vec![crate::capability::Transport {
        protocol: "jsonrpc".into(),
        address: "127.0.0.1:0".into(),
    }];
    let desc = crate::capability::with_transports(desc, transports);
    set_identity_from_self_description(&desc);
    let identity = handle_identity_get();
    assert_eq!(identity.name.as_ref(), config::PRIMAL_NAME);
    assert!(!identity.provides.is_empty());
    assert!(!identity.transports.is_empty());
}

#[test]
fn handle_compile_capabilities_has_extended_fields() {
    let caps = handle_compile_capabilities();
    assert_eq!(caps.math_ops, Some(34));
    assert_eq!(caps.sm_target.as_deref(), Some("sm_120"));
    assert_eq!(caps.atomics, Some(true));
    assert_eq!(caps.subgroup_ops, Some(true));
}

#[test]
fn mark_startup_is_idempotent() {
    mark_startup();
    mark_startup();
    let val = handle_health_standard();
    assert!(val["uptime_s"].as_u64().is_some());
}

#[path = "tests_serde.rs"]
mod tests_serde;

#[path = "tests_ml_pipeline.rs"]
mod tests_ml_pipeline;

#[path = "tests_spirv.rs"]
mod tests_spirv;

#[path = "tests_multi_device.rs"]
mod tests_multi_device;
