// SPDX-License-Identifier: AGPL-3.0-only
//! Compiler service — shared logic for both JSON-RPC and tarpc transports.
//!
//! Follows wateringHole semantic method naming: `shader.compile.{operation}`.

mod compile;
mod types;

pub use compile::{
    handle_compile, handle_compile_spirv, handle_compile_wgsl, handle_compile_wgsl_multi,
};
pub use types::{
    CompileRequest, CompileResponse, CompileSpirvRequestTarpc, CompileWgslRequest, HealthResponse,
    MultiDeviceCompileRequest, MultiDeviceCompileResponse,
};

use coral_reef::{AmdArch, NvArch};

/// Generate a health response listing all supported architectures.
#[must_use]
pub fn handle_health() -> HealthResponse {
    let mut archs: Vec<String> = NvArch::ALL.iter().map(ToString::to_string).collect();
    archs.extend(AmdArch::ALL.iter().map(ToString::to_string));
    HealthResponse {
        name: env!("CARGO_PKG_NAME").into(),
        version: env!("CARGO_PKG_VERSION").into(),
        status: "operational".into(),
        supported_archs: archs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use compile::parse_target;
    use coral_reef::{FmaPolicy, GpuArch, GpuTarget};
    use types::{DeviceTarget, MultiDeviceCompileRequest};

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
            wgsl_source: String::new(),
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
            wgsl_source: "@compute @workgroup_size(1) fn main() {}".to_owned(),
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
            wgsl_source: "@compute @workgroup_size(1) fn main() {}".to_owned(),
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
            wgsl_source: "@compute @workgroup_size(1) fn main() {}".to_owned(),
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
        let resp = handle_compile_wgsl_multi(&req).expect("multi-device should succeed");
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
            wgsl_source: "@compute @workgroup_size(1) fn main() {}".to_owned(),
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
            handle_compile_wgsl_multi(&req).expect("partial failure is not a top-level error");
        assert_eq!(resp.total_count, 2);
        assert_eq!(resp.success_count, 1);
        assert!(resp.results[0].binary.is_some());
        assert!(resp.results[1].binary.is_none());
        assert!(resp.results[1].error.is_some());
    }

    #[test]
    fn test_multi_device_compile_empty_source() {
        let req = MultiDeviceCompileRequest {
            wgsl_source: String::new(),
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
        assert!(handle_compile_wgsl_multi(&req).is_err());
    }

    #[test]
    fn test_multi_device_compile_no_targets() {
        let req = MultiDeviceCompileRequest {
            wgsl_source: "@compute @workgroup_size(1) fn main() {}".to_owned(),
            targets: vec![],
            opt_level: 2,
            fp64_software: false,
            fp64_strategy: None,
            fma_policy: None,
        };
        assert!(handle_compile_wgsl_multi(&req).is_err());
    }

    #[test]
    fn test_multi_device_compile_cross_vendor() {
        let req = MultiDeviceCompileRequest {
            wgsl_source: "@compute @workgroup_size(1) fn main() {}".to_owned(),
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
        let resp = handle_compile_wgsl_multi(&req).expect("cross-vendor should succeed");
        assert_eq!(resp.success_count, 2);
        assert_eq!(resp.results[0].arch, "sm_80");
        assert_eq!(resp.results[1].arch, "rdna2");
    }

    #[test]
    fn test_multi_device_request_serde_roundtrip() {
        let req = MultiDeviceCompileRequest {
            wgsl_source: "fn main() {}".to_owned(),
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
        assert_eq!(roundtrip.wgsl_source, req.wgsl_source);
        assert_eq!(roundtrip.targets.len(), 1);
        assert_eq!(roundtrip.targets[0].arch, "sm_70");
        assert_eq!(roundtrip.targets[0].pcie_group, Some(1));
        assert_eq!(roundtrip.fma_policy.as_deref(), Some("separate"));
    }
}
