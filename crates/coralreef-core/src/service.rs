// SPDX-License-Identifier: AGPL-3.0-only
//! Compiler service — shared logic for both JSON-RPC and tarpc transports.
//!
//! Follows wateringHole semantic method naming: `shader.compile.{operation}`.

use bytes::Bytes;
use coral_reef::{AmdArch, CompileError, CompileOptions, FmaPolicy, GpuTarget, NvArch};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// tarpc-only SPIR-V compile request (zero-copy via `Bytes`).
///
/// Uses `bytes::Bytes` so SPIR-V can be shared without copying over the wire.
/// Serializes as base64 when using JSON transport.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileSpirvRequestTarpc {
    /// Raw SPIR-V bytes (zero-copy).
    pub spirv: Bytes,
    /// Target GPU architecture name (e.g. `sm_70`, `rdna2`).
    pub arch: String,
    /// Optimization level (0-3).
    pub opt_level: u32,
    /// Enable f64 software transcendentals.
    pub fp64_software: bool,
}

/// Request to compile a shader (JSON-RPC wire format).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileRequest {
    /// SPIR-V words (JSON array of u32; base64 in tarpc uses [`CompileSpirvRequestTarpc`]).
    pub spirv_words: Vec<u32>,
    /// Target GPU architecture name (e.g. `sm70`, `sm86`, `rdna2`). Optional; defaults to sm70.
    #[serde(default = "default_arch")]
    pub arch: String,
    /// Optimization level (0-3).
    #[serde(default = "default_opt_level")]
    pub opt_level: u32,
    /// Enable f64 software transcendentals.
    #[serde(default)]
    pub fp64_software: bool,
}

/// Request to compile WGSL source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileWgslRequest {
    /// WGSL source code.
    pub wgsl_source: String,
    /// Target GPU architecture name (e.g. `sm70`, `sm86`, `rdna2`). Optional; defaults to sm70.
    #[serde(default = "default_arch")]
    pub arch: String,
    /// Optimization level (0-3).
    #[serde(default = "default_opt_level")]
    pub opt_level: u32,
    /// Enable f64 software transcendentals.
    #[serde(default)]
    pub fp64_software: bool,
    /// f64 strategy hint from the caller (e.g. `"software"`, `"native"`).
    /// Optional — defaults to using `fp64_software` if absent.
    #[serde(default)]
    pub fp64_strategy: Option<String>,
    /// FMA fusion policy hint (e.g. `"fused"`, `"separate"`, `"auto"`).
    /// Optional — defaults to `"auto"` (compiler decides).
    #[serde(default)]
    pub fma_policy: Option<String>,
}

/// Response from shader compilation.
///
/// Uses `bytes::Bytes` for zero-copy IPC payloads — `Bytes::from(Vec<u8>)`
/// takes ownership of the allocation without copying.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompileResponse {
    /// Compiled GPU binary (zero-copy via `bytes::Bytes`).
    pub binary: Bytes,
    /// Size in bytes.
    pub size: usize,
    /// Target architecture the binary was compiled for.
    #[serde(default)]
    pub arch: Option<String>,
    /// Compilation status (e.g. `"success"`, `"partial"`).
    #[serde(default)]
    pub status: Option<String>,
}

/// Health check response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    /// Primal name.
    pub name: Cow<'static, str>,
    /// Version.
    pub version: Cow<'static, str>,
    /// Current status.
    pub status: Cow<'static, str>,
    /// Supported architectures.
    pub supported_archs: Vec<String>,
}

fn default_arch() -> String {
    coral_reef::GpuArch::default().to_string()
}

const fn default_opt_level() -> u32 {
    2
}

/// Parse an architecture string into a [`GpuTarget`].
///
/// Tries NVIDIA first, then AMD. No hardcoded arch list.
///
/// # Errors
///
/// Returns an error if the architecture string is not recognized by any vendor.
pub fn parse_target(s: &str) -> Result<GpuTarget, CompileError> {
    if let Some(nv) = NvArch::parse(s) {
        return Ok(GpuTarget::Nvidia(nv));
    }
    if let Some(amd) = AmdArch::parse(s) {
        return Ok(GpuTarget::Amd(amd));
    }
    Err(CompileError::UnsupportedArch(s.to_owned().into()))
}

fn build_options(
    arch: &str,
    opt_level: u32,
    fp64_software: bool,
    fma: FmaPolicy,
) -> Result<CompileOptions, CompileError> {
    let target = parse_target(arch)?;
    Ok(CompileOptions {
        target,
        opt_level,
        debug_info: false,
        fp64_software,
        fma_policy: fma,
        ..CompileOptions::default()
    })
}

/// Convert SPIR-V bytes to words for the compiler.
fn bytes_to_spirv_words(bytes: &[u8]) -> Result<Vec<u32>, CompileError> {
    if bytes.len() % 4 != 0 {
        return Err(CompileError::InvalidInput(
            "SPIR-V must be multiple of 4 bytes".into(),
        ));
    }
    let mut words = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        debug_assert_eq!(chunk.len(), 4, "chunks_exact(4) yields 4 bytes");
        let arr: [u8; 4] = chunk
            .try_into()
            .map_err(|_| CompileError::InvalidInput("SPIR-V chunk must be 4 bytes".into()))?;
        words.push(u32::from_le_bytes(arr));
    }
    Ok(words)
}

/// Execute a SPIR-V compile from raw bytes (zero-copy friendly).
///
/// Accepts `Bytes` or `&[u8]` so IPC transports can pass SPIR-V without
/// copying. The compiler expects `&[u32]`, so we convert once at this boundary.
///
/// # Errors
///
/// Returns [`CompileError`] on invalid input or compilation failure.
pub fn handle_compile_spirv(
    spirv: impl AsRef<[u8]>,
    arch: &str,
    opt_level: u32,
    fp64_software: bool,
) -> Result<CompileResponse, CompileError> {
    let options = build_options(arch, opt_level, fp64_software, FmaPolicy::Auto)?;
    let words = bytes_to_spirv_words(spirv.as_ref())?;
    if words.is_empty() {
        return Err(CompileError::InvalidInput("empty SPIR-V module".into()));
    }
    let binary = coral_reef::compile(&words, &options)?;
    let size = binary.len();
    Ok(CompileResponse {
        binary: Bytes::from(binary),
        size,
        arch: Some(arch.to_owned()),
        status: Some("success".to_owned()),
    })
}

/// Execute a compile request (SPIR-V input).
///
/// Kept for backward compatibility with [`CompileRequest`] (JSON-RPC wire format).
///
/// # Errors
///
/// Returns [`CompileError`] on invalid input or compilation failure.
pub fn handle_compile(req: &CompileRequest) -> Result<CompileResponse, CompileError> {
    let bytes: Vec<u8> = req
        .spirv_words
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    handle_compile_spirv(bytes, &req.arch, req.opt_level, req.fp64_software)
}

/// Execute a WGSL compile request.
///
/// # Errors
///
/// Returns [`CompileError`] on invalid input or compilation failure.
pub fn handle_compile_wgsl(req: &CompileWgslRequest) -> Result<CompileResponse, CompileError> {
    let fp64_sw = req
        .fp64_strategy
        .as_deref()
        .map_or(req.fp64_software, |s| s == "software");
    let fma = parse_fma_policy(req.fma_policy.as_deref());
    let options = build_options(&req.arch, req.opt_level, fp64_sw, fma)?;
    let binary = coral_reef::compile_wgsl(&req.wgsl_source, &options)?;
    let size = binary.len();
    Ok(CompileResponse {
        binary: Bytes::from(binary),
        size,
        arch: Some(req.arch.clone()),
        status: Some("success".to_owned()),
    })
}

/// A single device target for multi-device compilation.
///
/// Carries an architecture hint and optional `PCIe` group ID so the caller
/// can request compilation for specific GPU slots in a multi-GPU system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceTarget {
    /// Card index (0-based, maps to `/dev/dri/renderD128+N`).
    #[serde(default)]
    pub card_index: u32,
    /// GPU architecture hint (e.g. `"sm_89"`, `"rdna2"`).
    pub arch: String,
    /// Optional `PCIe` group / switch affinity hint.
    #[serde(default)]
    pub pcie_group: Option<u32>,
}

/// Request to compile a single WGSL shader for multiple GPU targets at once.
///
/// Implements the `shader.compile.wgsl.multi` endpoint from the toadStool S144
/// handoff. Compiles the same shader source to native binaries for each
/// target device in a single request, enabling multi-GPU dispatch preparation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiDeviceCompileRequest {
    /// WGSL source code (shared across all targets).
    pub wgsl_source: String,
    /// Target devices to compile for.
    pub targets: Vec<DeviceTarget>,
    /// Optimization level (0-3).
    #[serde(default = "default_opt_level")]
    pub opt_level: u32,
    /// Enable f64 software transcendentals.
    #[serde(default)]
    pub fp64_software: bool,
    /// f64 strategy hint (e.g. `"software"`, `"native"`).
    #[serde(default)]
    pub fp64_strategy: Option<String>,
    /// FMA fusion policy hint (e.g. `"fused"`, `"separate"`, `"auto"`).
    #[serde(default)]
    pub fma_policy: Option<String>,
}

/// Result of compiling for a single device in a multi-device request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCompileResult {
    /// Card index this result corresponds to.
    pub card_index: u32,
    /// Architecture compiled for.
    pub arch: String,
    /// Compiled binary (zero-copy via `bytes::Bytes`), or `None` on failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binary: Option<Bytes>,
    /// Binary size in bytes (0 on failure).
    pub size: usize,
    /// Error message if compilation failed for this target.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Response from multi-device compilation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiDeviceCompileResponse {
    /// Per-device compilation results (same order as request `targets`).
    pub results: Vec<DeviceCompileResult>,
    /// Number of targets that compiled successfully.
    pub success_count: usize,
    /// Total number of targets requested.
    pub total_count: usize,
}

/// Parse an optional FMA policy string into an [`FmaPolicy`].
fn parse_fma_policy(s: Option<&str>) -> coral_reef::FmaPolicy {
    match s {
        Some("fused") => coral_reef::FmaPolicy::Fused,
        Some("separate") => coral_reef::FmaPolicy::Separate,
        _ => coral_reef::FmaPolicy::Auto,
    }
}

/// Execute a multi-device WGSL compile request.
///
/// Compiles the same WGSL source for every target device. Each target is
/// compiled independently; failures for one target do not prevent others
/// from succeeding.
///
/// # Errors
///
/// Returns [`CompileError`] only if the request itself is malformed
/// (e.g. empty WGSL source). Per-target failures are reported inline
/// in [`DeviceCompileResult::error`].
pub fn handle_compile_wgsl_multi(
    req: &MultiDeviceCompileRequest,
) -> Result<MultiDeviceCompileResponse, CompileError> {
    if req.wgsl_source.is_empty() {
        return Err(CompileError::InvalidInput("empty WGSL source".into()));
    }
    if req.targets.is_empty() {
        return Err(CompileError::InvalidInput(
            "at least one target device required".into(),
        ));
    }

    let fp64_sw = req
        .fp64_strategy
        .as_deref()
        .map_or(req.fp64_software, |s| s == "software");
    let fma = parse_fma_policy(req.fma_policy.as_deref());

    let mut results = Vec::with_capacity(req.targets.len());
    let mut success_count = 0usize;

    for target in &req.targets {
        let result = (|| -> Result<(Bytes, usize), CompileError> {
            let gpu_target = parse_target(&target.arch)?;
            let options = CompileOptions {
                target: gpu_target,
                opt_level: req.opt_level,
                debug_info: false,
                fp64_software: fp64_sw,
                fma_policy: fma,
                ..CompileOptions::default()
            };
            let binary = coral_reef::compile_wgsl(&req.wgsl_source, &options)?;
            let size = binary.len();
            Ok((Bytes::from(binary), size))
        })();

        match result {
            Ok((binary, size)) => {
                success_count += 1;
                results.push(DeviceCompileResult {
                    card_index: target.card_index,
                    arch: target.arch.clone(),
                    binary: Some(binary),
                    size,
                    error: None,
                });
            }
            Err(e) => {
                results.push(DeviceCompileResult {
                    card_index: target.card_index,
                    arch: target.arch.clone(),
                    binary: None,
                    size: 0,
                    error: Some(e.to_string()),
                });
            }
        }
    }

    let total_count = req.targets.len();
    Ok(MultiDeviceCompileResponse {
        results,
        success_count,
        total_count,
    })
}

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
    use coral_reef::GpuArch;

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
        // 4 bytes = 1 word; bytes_to_spirv_words accepts it (no "multiple of 4" error)
        // Compile fails because 1 word is not valid SPIR-V
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
        assert_eq!(parse_fma_policy(Some("fused")), FmaPolicy::Fused);
        assert_eq!(parse_fma_policy(Some("separate")), FmaPolicy::Separate);
        assert_eq!(parse_fma_policy(Some("auto")), FmaPolicy::Auto);
        assert_eq!(parse_fma_policy(None), FmaPolicy::Auto);
        assert_eq!(parse_fma_policy(Some("unknown")), FmaPolicy::Auto);
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
