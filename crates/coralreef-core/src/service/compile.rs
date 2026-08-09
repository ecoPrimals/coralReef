// SPDX-License-Identifier: AGPL-3.0-or-later
//! Single-target compilation handlers — SPIR-V, WGSL, and GEMM.

use std::borrow::Cow;
use std::time::Instant;

use super::types::{
    CompilationInfoResponse, CompileRequest, CompileResponse, CompileWgslRequest,
    GemmCompileRequest,
};
use bytes::Bytes;
use coral_reef::gemm::{GemmPrecision, GemmShape};
use coral_reef::{AmdArch, CompileError, CompileOptions, FmaPolicy, GpuTarget, NvArch};

pub const STATUS_SUCCESS: &str = "success";

/// PCI vendor ID: NVIDIA Corporation.
const PCI_VENDOR_NVIDIA: u32 = 0x10DE;
/// PCI vendor ID: Advanced Micro Devices (AMD).
const PCI_VENDOR_AMD: u32 = 0x1002;

/// Default wave/warp size for Intel GPUs (EU SIMD width).
const INTEL_DEFAULT_WAVE_SIZE: u32 = 16;
/// Default warp size for NVIDIA GPUs (32 threads per warp since G80).
const NVIDIA_DEFAULT_WARP_SIZE: u32 = 32;

/// Derive the wave/warp size from a compilation target.
pub fn wave_size_for(target: GpuTarget) -> u32 {
    match target {
        GpuTarget::Amd(amd) => u32::from(amd.default_wave_size()),
        GpuTarget::Intel(_) => INTEL_DEFAULT_WAVE_SIZE,
        _ => NVIDIA_DEFAULT_WARP_SIZE,
    }
}

/// Derive hardware dispatch hint from `precision_advice` carried in the request.
///
/// Tensor-core tiers (F16, BF16, TF32, FP8 variants) route to `"tensor_core"`;
/// everything else routes to `"compute"` (standard ALU path).
fn dispatch_hint_from_precision_advice(
    advice: Option<&super::types::PrecisionAdvice>,
) -> Cow<'static, str> {
    let Some(adv) = advice else {
        return "compute".into();
    };
    match adv.tier.to_ascii_lowercase().as_str() {
        "f16" | "bf16" | "tf32" | "fp8e4m3" | "fp8e5m2" | "fp8_e4m3" | "fp8_e5m2" => {
            "tensor_core".into()
        }
        _ => "compute".into(),
    }
}

/// Determine binary format string from the target architecture.
pub fn binary_format_for(target: GpuTarget) -> Cow<'static, str> {
    match target {
        GpuTarget::Nvidia(_) => "ptx".into(),
        GpuTarget::Amd(_) => "isa".into(),
        GpuTarget::Intel(_) => "spirv".into(),
        _ => "binary".into(),
    }
}

/// Resolve the effective architecture from the request.
///
/// When the caller specifies an explicit (non-default) arch string, that wins.
/// When the arch is the serde default (`"sm70"`) and an
/// [`AdapterDescriptor`](super::types::AdapterDescriptor) is present, the
/// adapter's `vendor_id` / `device_name` is used to infer the best target.
/// This enables callers to pass hardware
/// identity without knowing the exact SM version.
fn resolve_arch(arch: &str, adapter: Option<&super::types::AdapterDescriptor>) -> String {
    let default = super::types::default_arch();
    if arch != default {
        return arch.to_owned();
    }
    let Some(ad) = adapter else {
        return arch.to_owned();
    };
    infer_arch_from_adapter(ad).unwrap_or(arch).to_owned()
}

/// Infer SM/ISA architecture from adapter hardware identity.
///
/// Returns a `&'static str` — all arch names are compile-time constants.
fn infer_arch_from_adapter(ad: &super::types::AdapterDescriptor) -> Option<&'static str> {
    let name = ad.device_name.to_lowercase();
    if ad.vendor_id == PCI_VENDOR_NVIDIA {
        if name.contains("5060")
            || name.contains("5070")
            || name.contains("5080")
            || name.contains("5090")
            || name.contains("blackwell")
            || name.contains("gb2")
        {
            return Some("sm_120");
        }
        if name.contains("4060")
            || name.contains("4070")
            || name.contains("4080")
            || name.contains("4090")
            || name.contains("ada")
            || name.contains("l40")
        {
            return Some("sm_89");
        }
        if name.contains("3060")
            || name.contains("3070")
            || name.contains("3080")
            || name.contains("3090")
        {
            return Some("sm_86");
        }
        if name.contains("a100") || name.contains("a30") || name.contains("a10") {
            return Some("sm_80");
        }
        if name.contains("titan v") || name.contains("v100") || name.contains("gv100") {
            return Some("sm_70");
        }
    }
    if ad.vendor_id == PCI_VENDOR_AMD {
        if name.contains("7900") || name.contains("gfx1100") || name.contains("rdna3") {
            return Some("rdna3");
        }
        if name.contains("6900") || name.contains("6800") || name.contains("rdna2") {
            return Some("rdna2");
        }
    }
    None
}

/// Parse an architecture string into a [`GpuTarget`].
///
/// Tries NVIDIA first, then AMD. No hardcoded arch list.
///
/// # Errors
///
/// Returns an error if the architecture string is not recognized by any vendor.
#[must_use = "returns the parsed target or an error — check the result"]
pub fn parse_target(s: &str) -> Result<GpuTarget, CompileError> {
    if let Some(nv) = NvArch::parse(s) {
        return Ok(GpuTarget::Nvidia(nv));
    }
    if let Some(amd) = AmdArch::parse(s) {
        return Ok(GpuTarget::Amd(amd));
    }
    Err(CompileError::UnsupportedArch(s.to_owned().into()))
}

pub fn build_options(
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
    Ok(bytes
        .chunks_exact(4)
        .map(|chunk| {
            u32::from_le_bytes(
                chunk
                    .try_into()
                    .expect("chunks_exact(4) always yields 4 bytes"),
            )
        })
        .collect())
}

/// Execute a SPIR-V compile from raw bytes (zero-copy friendly).
///
/// Accepts `Bytes` or `&[u8]` so IPC transports can pass SPIR-V without
/// copying. The compiler expects `&[u32]`, so we convert once at this boundary.
///
/// # Errors
///
/// Returns [`CompileError`] on invalid input or compilation failure.
#[must_use = "contains the compiled binary or an error"]
pub fn handle_compile_spirv(
    spirv: impl AsRef<[u8]>,
    arch: impl Into<String>,
    opt_level: u32,
    fp64_software: bool,
) -> Result<CompileResponse, CompileError> {
    let arch = arch.into();
    let options = build_options(&arch, opt_level, fp64_software, FmaPolicy::Auto)?;
    let words = bytes_to_spirv_words(spirv.as_ref())?;
    if words.is_empty() {
        return Err(CompileError::InvalidInput("empty SPIR-V module".into()));
    }
    let t0 = Instant::now();
    let binary = coral_reef::compile(&words, &options)?;
    let elapsed = t0.elapsed();
    let size = binary.len();
    Ok(CompileResponse {
        binary: Bytes::from(binary),
        size,
        arch: Some(arch),
        status: Some(Cow::Borrowed(STATUS_SUCCESS)),
        info: None,
        compile_time_ms: Some(elapsed.as_secs_f64() * 1000.0),
        dispatch_hints: Some(super::types::DispatchHints {
            hardware_hint: "compute".into(),
            binary_format: Some(binary_format_for(options.target)),
            execution_model: Some("simt".into()),
        }),
        spirv_binary: None,
        provenance: None,
    })
}

/// Execute a compile request (SPIR-V input).
///
/// Kept for backward compatibility with [`CompileRequest`] (JSON-RPC wire format).
///
/// # Errors
///
/// Returns [`CompileError`] on invalid input or compilation failure.
#[must_use = "contains the compiled binary or an error"]
pub fn handle_compile(req: &CompileRequest) -> Result<CompileResponse, CompileError> {
    let bytes: Vec<u8> = req
        .spirv_words
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect();
    handle_compile_spirv(bytes, &req.arch, req.opt_level, req.fp64_software)
}

/// Parse an optional FMA policy string into an [`FmaPolicy`].
#[must_use]
pub fn parse_fma_policy(s: Option<&str>) -> FmaPolicy {
    match s {
        Some("fused") => FmaPolicy::Fused,
        Some("separate") => FmaPolicy::Separate,
        _ => FmaPolicy::Auto,
    }
}

/// Execute a WGSL compile request.
///
/// Uses `compile_wgsl_full` to return both the binary and compilation
/// metadata (`CompilationInfo`) so callers can construct dispatch
/// descriptors without re-parsing.
///
/// # Errors
///
/// Returns [`CompileError`] on invalid input or compilation failure.
#[must_use = "contains the compiled binary or an error"]
pub fn handle_compile_wgsl(req: &CompileWgslRequest) -> Result<CompileResponse, CompileError> {
    let fp64_sw = req
        .fp64_strategy
        .as_deref()
        .map_or(req.fp64_software, |s| s == "software");
    let fma = parse_fma_policy(req.fma_policy.as_deref());
    let effective_arch = resolve_arch(&req.arch, req.adapter.as_ref());
    let mut options = build_options(&effective_arch, req.opt_level, fp64_sw, fma)?;
    if let Some(ver) = req.spirv_version {
        options.spirv = Some(coral_reef::SpirVOptions {
            version: ver.into(),
            ..coral_reef::SpirVOptions::default()
        });
    }
    let wave_size = wave_size_for(options.target);
    let hardware_hint = dispatch_hint_from_precision_advice(req.precision_advice.as_ref());
    let t0 = Instant::now();
    let is_ptx_emitter = options.target.as_nvidia().is_some_and(|nv| nv.sm() >= 100);
    let (compiled, spirv) = if !is_ptx_emitter && req.emit_spirv {
        let module = coral_reef::parse_wgsl_to_naga(req.wgsl_source.as_ref(), &options)?;
        let compiled = coral_reef::compile_naga_module_full(&module, &options)?;
        let spirv = match coral_reef::module_to_spirv(&module, &options) {
            Ok(bytes) => Some(Bytes::from(bytes)),
            Err(e) => {
                tracing::warn!(
                    arch = effective_arch,
                    "SPIR-V emission failed (native binary succeeded): {e}"
                );
                None
            }
        };
        (compiled, spirv)
    } else {
        let compiled = coral_reef::compile_wgsl_full(req.wgsl_source.as_ref(), &options)?;
        let spirv = if req.emit_spirv {
            match coral_reef::wgsl_to_spirv(req.wgsl_source.as_ref(), &options) {
                Ok(bytes) => Some(Bytes::from(bytes)),
                Err(e) => {
                    tracing::warn!(
                        arch = effective_arch,
                        "SPIR-V emission failed (native binary succeeded): {e}"
                    );
                    None
                }
            }
        } else {
            None
        };
        (compiled, spirv)
    };
    let elapsed = t0.elapsed();
    let size = compiled.binary.len();
    Ok(CompileResponse {
        binary: Bytes::from(compiled.binary),
        size,
        arch: Some(effective_arch),
        status: Some(Cow::Borrowed(STATUS_SUCCESS)),
        info: Some(CompilationInfoResponse {
            gpr_count: compiled.info.gpr_count,
            instr_count: compiled.info.instr_count,
            shared_mem_bytes: compiled.info.shared_mem_bytes,
            barrier_count: compiled.info.barrier_count,
            workgroup_size: compiled.info.local_size,
            wave_size,
            local_memory: compiled.info.local_mem_bytes,
        }),
        compile_time_ms: Some(elapsed.as_secs_f64() * 1000.0),
        dispatch_hints: Some(super::types::DispatchHints {
            hardware_hint,
            binary_format: Some(binary_format_for(options.target)),
            execution_model: Some("simt".into()),
        }),
        spirv_binary: spirv,
        provenance: None,
    })
}

/// `shader.compile.gemm` — compile a tensor-core GEMM kernel.
///
/// # Errors
///
/// Returns [`CompileError`] if the target is not NVIDIA SM80+, or if
/// dimensions are not aligned to tile boundaries.
pub fn handle_compile_gemm(req: &GemmCompileRequest) -> Result<CompileResponse, CompileError> {
    let target = parse_target(&req.arch)?;
    let precision = match req.precision.to_ascii_lowercase().as_str() {
        "f16" => GemmPrecision::F16,
        "f16f32" | "f16_f32" => GemmPrecision::F16F32,
        "tf32" => GemmPrecision::Tf32,
        other => {
            return Err(CompileError::InvalidInput(
                format!("unknown GEMM precision: {other:?} (expected f16, f16f32, tf32)").into(),
            ));
        }
    };
    let shape = GemmShape {
        m: req.m,
        n: req.n,
        k: req.k,
    };

    let t0 = Instant::now();
    let compiled = coral_reef::gemm::compile_gemm(shape, precision, target)?;
    let compile_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let size = compiled.binary.len();

    Ok(CompileResponse {
        binary: Bytes::from(compiled.binary),
        size,
        arch: Some(req.arch.clone()),
        status: Some(Cow::Borrowed(STATUS_SUCCESS)),
        compile_time_ms: Some(compile_ms),
        info: Some(CompilationInfoResponse {
            gpr_count: compiled.info.gpr_count,
            instr_count: compiled.info.instr_count,
            shared_mem_bytes: compiled.info.shared_mem_bytes,
            barrier_count: compiled.info.barrier_count,
            workgroup_size: compiled.info.local_size,
            wave_size: 32,
            local_memory: compiled.info.local_mem_bytes,
        }),
        dispatch_hints: Some(super::types::DispatchHints {
            hardware_hint: "tensor_core".into(),
            binary_format: Some("ptx".into()),
            execution_model: Some("simt".into()),
        }),
        spirv_binary: None,
        provenance: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adapter(vendor_id: u32, name: &str) -> super::super::types::AdapterDescriptor {
        super::super::types::AdapterDescriptor {
            vendor_id,
            device_name: name.to_owned(),
            device_type: String::new(),
        }
    }

    #[test]
    fn nvidia_rtx_5090_infers_sm120() {
        let ad = adapter(PCI_VENDOR_NVIDIA, "NVIDIA GeForce RTX 5090");
        assert_eq!(infer_arch_from_adapter(&ad), Some("sm_120"));
    }

    #[test]
    fn nvidia_rtx_4090_infers_sm89() {
        let ad = adapter(PCI_VENDOR_NVIDIA, "NVIDIA GeForce RTX 4090");
        assert_eq!(infer_arch_from_adapter(&ad), Some("sm_89"));
    }

    #[test]
    fn nvidia_rtx_3080_infers_sm86() {
        let ad = adapter(PCI_VENDOR_NVIDIA, "NVIDIA GeForce RTX 3080");
        assert_eq!(infer_arch_from_adapter(&ad), Some("sm_86"));
    }

    #[test]
    fn nvidia_a100_infers_sm80() {
        let ad = adapter(PCI_VENDOR_NVIDIA, "NVIDIA A100-SXM4-40GB");
        assert_eq!(infer_arch_from_adapter(&ad), Some("sm_80"));
    }

    #[test]
    fn nvidia_v100_infers_sm70() {
        let ad = adapter(PCI_VENDOR_NVIDIA, "Tesla V100-SXM2-16GB");
        assert_eq!(infer_arch_from_adapter(&ad), Some("sm_70"));
    }

    #[test]
    fn amd_7900_infers_rdna3() {
        let ad = adapter(PCI_VENDOR_AMD, "AMD Radeon RX 7900 XTX");
        assert_eq!(infer_arch_from_adapter(&ad), Some("rdna3"));
    }

    #[test]
    fn amd_6800_infers_rdna2() {
        let ad = adapter(PCI_VENDOR_AMD, "AMD Radeon RX 6800 XT");
        assert_eq!(infer_arch_from_adapter(&ad), Some("rdna2"));
    }

    #[test]
    fn unknown_vendor_returns_none() {
        let ad = adapter(0x8086, "Intel Arc A770");
        assert_eq!(infer_arch_from_adapter(&ad), None);
    }

    #[test]
    fn unknown_nvidia_model_returns_none() {
        let ad = adapter(PCI_VENDOR_NVIDIA, "NVIDIA GeForce GTX 960");
        assert_eq!(infer_arch_from_adapter(&ad), None);
    }

    #[test]
    fn resolve_arch_uses_explicit_when_not_default() {
        let ad = adapter(PCI_VENDOR_NVIDIA, "NVIDIA GeForce RTX 4090");
        assert_eq!(resolve_arch("sm_50", Some(&ad)), "sm_50");
    }

    #[test]
    fn resolve_arch_infers_from_adapter_on_default() {
        let ad = adapter(PCI_VENDOR_NVIDIA, "NVIDIA GeForce RTX 4090");
        let default = super::super::types::default_arch();
        assert_eq!(resolve_arch(&default, Some(&ad)), "sm_89");
    }

    #[test]
    fn resolve_arch_falls_back_without_adapter() {
        let default = super::super::types::default_arch();
        assert_eq!(resolve_arch(&default, None), default);
    }

    #[test]
    fn wave_size_nvidia_is_32() {
        assert_eq!(wave_size_for(GpuTarget::Nvidia(NvArch::Sm70)), 32);
    }

    #[test]
    fn wave_size_amd_matches_arch() {
        let rdna3 = GpuTarget::Amd(AmdArch::parse("rdna3").expect("valid arch"));
        assert!(wave_size_for(rdna3) > 0);
    }

    #[test]
    fn dispatch_hint_tensor_core_for_f16() {
        let advice = super::super::types::PrecisionAdvice {
            tier: "f16".to_owned(),
            needs_transcendental_lowering: false,
            df64_naga_poisoned: false,
            domain: None,
        };
        assert_eq!(
            dispatch_hint_from_precision_advice(Some(&advice)),
            "tensor_core"
        );
    }

    #[test]
    fn dispatch_hint_compute_for_none() {
        assert_eq!(dispatch_hint_from_precision_advice(None), "compute");
    }

    #[test]
    fn dispatch_hint_compute_for_unknown_tier() {
        let advice = super::super::types::PrecisionAdvice {
            tier: "fp32".to_owned(),
            needs_transcendental_lowering: false,
            df64_naga_poisoned: false,
            domain: None,
        };
        assert_eq!(
            dispatch_hint_from_precision_advice(Some(&advice)),
            "compute"
        );
    }

    #[test]
    fn binary_format_nvidia_is_ptx() {
        assert_eq!(
            binary_format_for(GpuTarget::Nvidia(NvArch::Sm70)).as_ref(),
            "ptx"
        );
    }

    #[test]
    fn binary_format_amd_is_isa() {
        let rdna3 = GpuTarget::Amd(AmdArch::parse("rdna3").expect("valid arch"));
        assert_eq!(binary_format_for(rdna3).as_ref(), "isa");
    }

    #[test]
    fn bytes_to_spirv_words_rejects_odd_length() {
        let result = bytes_to_spirv_words(&[0, 1, 2]);
        assert!(result.is_err());
    }

    #[test]
    fn bytes_to_spirv_words_converts_le() {
        let bytes = [0x03, 0x02, 0x23, 0x07];
        let words = bytes_to_spirv_words(&bytes).expect("valid input");
        assert_eq!(words, vec![0x0723_0203]);
    }

    #[test]
    fn parse_fma_policy_fused() {
        assert!(matches!(parse_fma_policy(Some("fused")), FmaPolicy::Fused));
    }

    #[test]
    fn parse_fma_policy_separate() {
        assert!(matches!(
            parse_fma_policy(Some("separate")),
            FmaPolicy::Separate
        ));
    }

    #[test]
    fn parse_fma_policy_default() {
        assert!(matches!(parse_fma_policy(None), FmaPolicy::Auto));
    }
}
