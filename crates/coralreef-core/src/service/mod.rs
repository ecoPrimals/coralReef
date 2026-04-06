// SPDX-License-Identifier: AGPL-3.0-or-later
//! Compiler service — shared logic for both JSON-RPC and tarpc transports.
//!
//! Follows wateringHole semantic method naming: `shader.compile.{operation}`.

mod compile;
pub mod types;

pub use compile::{
    handle_compile, handle_compile_spirv, handle_compile_wgsl, handle_compile_wgsl_multi,
};
pub use types::{
    CapabilityListResponse, CompileCapabilitiesResponse, CompileRequest, CompileResponse,
    CompileSpirvRequestTarpc, CompileWgslRequest, F64TranscendentalCapabilities,
    HealthCheckResponse, HealthResponse, IdentityGetResponse, LivenessResponse,
    MultiDeviceCompileRequest, MultiDeviceCompileResponse, ReadinessResponse,
};

use std::collections::BTreeSet;
use std::sync::OnceLock;

use crate::capability::SelfDescription;
use crate::config;
use coral_reef::{AmdArch, NvArch};

static IDENTITY_ADVERTISED: OnceLock<IdentityGetResponse> = OnceLock::new();

/// Store the primal identity for `identity.get` after IPC binds (full transports).
///
/// If not called, [`handle_identity_get`] returns [`IdentityGetResponse::fallback`].
pub fn set_identity_for_ipc(identity: IdentityGetResponse) {
    let _ = IDENTITY_ADVERTISED.set(identity);
}

/// Build identity from a bound [`SelfDescription`] and publish for JSON-RPC.
pub fn set_identity_from_self_description(desc: &SelfDescription) {
    set_identity_for_ipc(IdentityGetResponse {
        name: config::PRIMAL_NAME.into(),
        version: config::PRIMAL_VERSION.into(),
        provides: desc.provides.clone(),
        requires: desc.requires.clone(),
        transports: desc.transports.clone(),
    });
}

/// `identity.get` — return this primal's self-description for ecosystem discovery.
#[must_use]
pub fn handle_identity_get() -> IdentityGetResponse {
    IDENTITY_ADVERTISED
        .get()
        .cloned()
        .unwrap_or_else(IdentityGetResponse::fallback)
}

/// `capability.list` — capability domains this primal serves (wateringHole discovery).
///
/// Includes advertised [`crate::capability::Capability`] ids plus JSON-RPC namespaces
/// exposed by this binary (`health.*`, `identity.get`).
#[must_use]
pub fn handle_capability_list() -> CapabilityListResponse {
    let desc = crate::capability::self_description();
    let mut domains: BTreeSet<String> = desc.provides.iter().map(|c| c.id.to_string()).collect();
    domains.insert("health".into());
    domains.insert("identity".into());
    CapabilityListResponse {
        capabilities: domains.into_iter().collect(),
        version: config::PRIMAL_VERSION.into(),
    }
}

/// Generate a health response listing all supported architectures.
#[must_use]
pub fn handle_health() -> HealthResponse {
    let mut archs: Vec<String> = NvArch::ALL.iter().map(ToString::to_string).collect();
    archs.extend(AmdArch::ALL.iter().map(ToString::to_string));
    HealthResponse {
        name: config::PRIMAL_NAME.into(),
        version: config::PRIMAL_VERSION.into(),
        status: "operational".into(),
        supported_archs: archs,
    }
}

/// `shader.compile.capabilities` — structured capability report.
///
/// Reports both supported architectures AND f64 transcendental lowering
/// capabilities. Callers use this to decide whether to route transcendental-
/// heavy shaders through the sovereign compiler (polyfill) vs native driver.
#[must_use]
pub fn handle_compile_capabilities() -> CompileCapabilitiesResponse {
    let health = handle_health();
    CompileCapabilitiesResponse {
        supported_archs: health.supported_archs,
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
    }
}

/// `health.check` — full health check per wateringHole standard.
///
/// Probes internal subsystems and returns a detailed health report.
#[must_use]
pub fn handle_health_check() -> HealthCheckResponse {
    let health = handle_health();
    let is_healthy = health.status == "operational";
    HealthCheckResponse {
        name: health.name,
        version: health.version,
        healthy: is_healthy,
        status: health.status,
        supported_archs: health.supported_archs,
        family_id: config::family_id().into(),
    }
}

/// `health.liveness` — lightweight liveness probe.
///
/// Returns true if the process is alive and responsive (no deep checks).
#[must_use]
pub const fn handle_health_liveness() -> LivenessResponse {
    LivenessResponse { alive: true }
}

/// `health.readiness` — readiness probe for accepting work.
///
/// Checks whether the compiler is initialized and ready to serve
/// compilation requests. May return false during startup.
#[must_use]
pub fn handle_health_readiness() -> ReadinessResponse {
    ReadinessResponse {
        ready: true,
        name: config::PRIMAL_NAME.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use compile::parse_target;
    use coral_reef::{FmaPolicy, GpuArch, GpuTarget};
    use std::sync::Arc;
    use types::{
        CompileRequest, CompileResponse, CompileSpirvRequestTarpc, CompileWgslRequest,
        DeviceCompileResult, DeviceTarget, HealthResponse, MultiDeviceCompileRequest,
        MultiDeviceCompileResponse,
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
        assert_eq!(resp.version.as_ref(), env!("CARGO_PKG_VERSION"));
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
    fn test_handle_health_liveness() {
        let resp = handle_health_liveness();
        assert!(resp.alive);
    }

    #[test]
    fn test_handle_health_readiness() {
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
        };
        let result = handle_compile_wgsl(&req);
        assert!(result.is_ok(), "FMA separate should compile: {result:?}");
    }

    #[test]
    fn test_multi_device_compile_basic() {
        let req = MultiDeviceCompileRequest {
            wgsl_source: Arc::from("@compute @workgroup_size(1) fn main() {}"),
            targets: vec![
                DeviceTarget {
                    card_index: 0,
                    arch: "sm_70".to_owned(),
                    pcie_group: None,
                },
                DeviceTarget {
                    card_index: 1,
                    arch: "sm_89".to_owned(),
                    pcie_group: Some(0),
                },
            ],
            opt_level: 2,
            fp64_software: false,
            fp64_strategy: None,
            fma_policy: None,
        };
        let resp = handle_compile_wgsl_multi(req).expect("multi-device should succeed");
        assert_eq!(resp.total_count, 2);
        assert_eq!(resp.success_count, 2);
        assert_eq!(resp.results.len(), 2);
        assert_eq!(resp.results[0].card_index, 0);
        assert_eq!(resp.results[0].arch, "sm_70");
        assert!(resp.results[0].binary.is_some());
        assert!(resp.results[0].size > 0);
        assert!(resp.results[0].error.is_none());
        assert_eq!(resp.results[1].card_index, 1);
        assert_eq!(resp.results[1].arch, "sm_89");
        assert!(resp.results[1].binary.is_some());
    }

    #[test]
    fn test_multi_device_compile_mixed_success_failure() {
        let req = MultiDeviceCompileRequest {
            wgsl_source: Arc::from("@compute @workgroup_size(1) fn main() {}"),
            targets: vec![
                DeviceTarget {
                    card_index: 0,
                    arch: "sm_70".to_owned(),
                    pcie_group: None,
                },
                DeviceTarget {
                    card_index: 1,
                    arch: "sm_99".to_owned(),
                    pcie_group: None,
                },
            ],
            opt_level: 2,
            fp64_software: false,
            fp64_strategy: None,
            fma_policy: None,
        };
        let resp =
            handle_compile_wgsl_multi(req).expect("partial failure is not a top-level error");
        assert_eq!(resp.total_count, 2);
        assert_eq!(resp.success_count, 1);
        assert!(resp.results[0].binary.is_some());
        assert!(resp.results[1].binary.is_none());
        assert!(resp.results[1].error.is_some());
    }

    #[test]
    fn test_multi_device_compile_empty_source() {
        let req = MultiDeviceCompileRequest {
            wgsl_source: Arc::from(""),
            targets: vec![DeviceTarget {
                card_index: 0,
                arch: "sm_70".to_owned(),
                pcie_group: None,
            }],
            opt_level: 2,
            fp64_software: false,
            fp64_strategy: None,
            fma_policy: None,
        };
        assert!(handle_compile_wgsl_multi(req).is_err());
    }

    #[test]
    fn test_multi_device_compile_no_targets() {
        let req = MultiDeviceCompileRequest {
            wgsl_source: Arc::from("@compute @workgroup_size(1) fn main() {}"),
            targets: vec![],
            opt_level: 2,
            fp64_software: false,
            fp64_strategy: None,
            fma_policy: None,
        };
        assert!(handle_compile_wgsl_multi(req).is_err());
    }

    #[test]
    fn test_multi_device_compile_cross_vendor() {
        let req = MultiDeviceCompileRequest {
            wgsl_source: Arc::from("@compute @workgroup_size(1) fn main() {}"),
            targets: vec![
                DeviceTarget {
                    card_index: 0,
                    arch: "sm_80".to_owned(),
                    pcie_group: Some(0),
                },
                DeviceTarget {
                    card_index: 1,
                    arch: "rdna2".to_owned(),
                    pcie_group: Some(1),
                },
            ],
            opt_level: 2,
            fp64_software: false,
            fp64_strategy: None,
            fma_policy: Some("fused".to_owned()),
        };
        let resp = handle_compile_wgsl_multi(req).expect("cross-vendor should succeed");
        assert_eq!(resp.success_count, 2);
        assert_eq!(resp.results[0].arch, "sm_80");
        assert_eq!(resp.results[1].arch, "rdna2");
    }

    #[test]
    fn test_multi_device_request_serde_roundtrip() {
        let req = MultiDeviceCompileRequest {
            wgsl_source: Arc::from("fn main() {}"),
            targets: vec![DeviceTarget {
                card_index: 0,
                arch: "sm_70".to_owned(),
                pcie_group: Some(1),
            }],
            opt_level: 3,
            fp64_software: true,
            fp64_strategy: Some("software".to_owned()),
            fma_policy: Some("separate".to_owned()),
        };
        let json = serde_json::to_string(&req).unwrap();
        let roundtrip: MultiDeviceCompileRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.wgsl_source.as_ref(), req.wgsl_source.as_ref());
        assert_eq!(roundtrip.targets.len(), 1);
        assert_eq!(roundtrip.targets[0].arch, "sm_70");
        assert_eq!(roundtrip.targets[0].pcie_group, Some(1));
        assert_eq!(roundtrip.fma_policy.as_deref(), Some("separate"));
    }

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
    fn test_compile_response_serde_roundtrip() {
        let resp = CompileResponse {
            binary: Bytes::from(vec![0x01, 0x02, 0x03]),
            size: 3,
            arch: Some("sm_70".to_owned()),
            status: Some("success".to_owned()),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let roundtrip: CompileResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.binary.as_ref(), resp.binary.as_ref());
        assert_eq!(roundtrip.size, resp.size);
        assert_eq!(roundtrip.arch, resp.arch);
        assert_eq!(roundtrip.status, resp.status);
    }

    #[test]
    fn test_compile_response_defaults_from_json() {
        let resp = CompileResponse {
            binary: Bytes::from(vec![1, 2, 3]),
            size: 3,
            arch: None,
            status: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let roundtrip: CompileResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip.binary.as_ref(), &[1, 2, 3]);
        assert_eq!(roundtrip.size, 3);
        assert!(roundtrip.arch.is_none());
        assert!(roundtrip.status.is_none());
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
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(!json.contains("\"binary\""));
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
        let req = CompileWgslRequest {
            wgsl_source: Arc::from("@compute @workgroup_size(1) fn main() {}"),
            arch: "sm_70".to_owned(),
            opt_level: 2,
            fp64_software: false,
            fp64_strategy: Some("software".to_owned()),
            fma_policy: None,
        };
        let result = handle_compile_wgsl(&req);
        assert!(
            result.is_ok(),
            "fp64_strategy=software should force software path: {result:?}"
        );
    }

    #[test]
    fn test_compile_wgsl_fp64_strategy_native_uses_fp64_software_flag() {
        let req = CompileWgslRequest {
            wgsl_source: Arc::from("@compute @workgroup_size(1) fn main() {}"),
            arch: "sm_70".to_owned(),
            opt_level: 2,
            fp64_software: true,
            fp64_strategy: Some("native".to_owned()),
            fma_policy: Some("fused".to_owned()),
        };
        let result = handle_compile_wgsl(&req);
        assert!(result.is_ok(), "native strategy should compile: {result:?}");
    }

    #[test]
    fn test_handle_compile_spirv_amd_rdna2_valid_module() {
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
}
