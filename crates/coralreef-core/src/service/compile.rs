// SPDX-License-Identifier: AGPL-3.0-or-later
//! Compilation handlers — SPIR-V, WGSL, and multi-device.

use std::time::Instant;

use super::types::{
    CompilationInfoResponse, CompileRequest, CompileResponse, CompileWgslRequest,
    DeviceCompileResult, GemmCompileRequest, MultiDeviceCompileRequest, MultiDeviceCompileResponse,
};
use bytes::Bytes;
use coral_reef::gemm::{GemmPrecision, GemmShape};
use coral_reef::{AmdArch, CompileError, CompileOptions, FmaPolicy, GpuTarget, NvArch};

const STATUS_SUCCESS: &str = "success";

/// PCI vendor ID: NVIDIA Corporation.
const PCI_VENDOR_NVIDIA: u32 = 0x10DE;
/// PCI vendor ID: Advanced Micro Devices (AMD).
const PCI_VENDOR_AMD: u32 = 0x1002;

/// Default wave/warp size for Intel GPUs (EU SIMD width).
const INTEL_DEFAULT_WAVE_SIZE: u32 = 16;

/// Derive the wave/warp size from a compilation target.
fn wave_size_for(target: GpuTarget) -> u32 {
    match target {
        GpuTarget::Amd(amd) => u32::from(amd.default_wave_size()),
        GpuTarget::Intel(_) => INTEL_DEFAULT_WAVE_SIZE,
        _ => 32,
    }
}

/// Derive hardware dispatch hint from `precision_advice` carried in the request.
///
/// Tensor-core tiers (F16, BF16, TF32, FP8 variants) route to `"tensor_core"`;
/// everything else routes to `"compute"` (standard ALU path).
fn dispatch_hint_from_precision_advice(advice: Option<&super::types::PrecisionAdvice>) -> String {
    let Some(adv) = advice else {
        return "compute".to_owned();
    };
    match adv.tier.to_ascii_lowercase().as_str() {
        "f16" | "bf16" | "tf32" | "fp8e4m3" | "fp8e5m2" | "fp8_e4m3" | "fp8_e5m2" => {
            "tensor_core".to_owned()
        }
        _ => "compute".to_owned(),
    }
}

/// Determine binary format string from the target architecture.
fn binary_format_for(target: GpuTarget) -> String {
    match target {
        GpuTarget::Nvidia(_) => "ptx".to_owned(),
        GpuTarget::Amd(_) => "isa".to_owned(),
        GpuTarget::Intel(_) => "spirv".to_owned(),
        _ => "binary".to_owned(),
    }
}

/// Resolve the effective architecture from the request.
///
/// When the caller specifies an explicit (non-default) arch string, that wins.
/// When the arch is the serde default (`"sm70"`) and an [`AdapterDescriptor`] is
/// present, the adapter's `vendor_id` / `device_name` is used to infer the best
/// target. This enables callers (e.g. `barraCuda`, `hotSpring`) to pass hardware
/// identity without knowing the exact SM version.
fn resolve_arch(arch: &str, adapter: Option<&super::types::AdapterDescriptor>) -> String {
    let default = super::types::default_arch();
    if arch != default {
        return arch.to_owned();
    }
    let Some(ad) = adapter else {
        return arch.to_owned();
    };
    infer_arch_from_adapter(ad).unwrap_or_else(|| arch.to_owned())
}

/// Infer SM/ISA architecture from adapter hardware identity.
fn infer_arch_from_adapter(ad: &super::types::AdapterDescriptor) -> Option<String> {
    let name = ad.device_name.to_lowercase();
    if ad.vendor_id == PCI_VENDOR_NVIDIA {
        if name.contains("5060")
            || name.contains("5070")
            || name.contains("5080")
            || name.contains("5090")
            || name.contains("blackwell")
            || name.contains("gb2")
        {
            return Some("sm_120".to_owned());
        }
        if name.contains("4060")
            || name.contains("4070")
            || name.contains("4080")
            || name.contains("4090")
            || name.contains("ada")
            || name.contains("l40")
        {
            return Some("sm_89".to_owned());
        }
        if name.contains("3060")
            || name.contains("3070")
            || name.contains("3080")
            || name.contains("3090")
        {
            return Some("sm_86".to_owned());
        }
        if name.contains("a100") || name.contains("a30") || name.contains("a10") {
            return Some("sm_80".to_owned());
        }
        if name.contains("titan v") || name.contains("v100") || name.contains("gv100") {
            return Some("sm_70".to_owned());
        }
    }
    if ad.vendor_id == PCI_VENDOR_AMD {
        if name.contains("7900") || name.contains("gfx1100") || name.contains("rdna3") {
            return Some("rdna3".to_owned());
        }
        if name.contains("6900") || name.contains("6800") || name.contains("rdna2") {
            return Some("rdna2".to_owned());
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
        status: Some(STATUS_SUCCESS.to_owned()),
        info: None,
        compile_time_ms: Some(elapsed.as_secs_f64() * 1000.0),
        dispatch_hints: Some(super::types::DispatchHints {
            hardware_hint: "compute".to_owned(),
            binary_format: Some(binary_format_for(options.target)),
            execution_model: Some("simt".to_owned()),
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
    let compiled = coral_reef::compile_wgsl_full(req.wgsl_source.as_ref(), &options)?;
    let spirv = if req.emit_spirv {
        coral_reef::wgsl_to_spirv(req.wgsl_source.as_ref(), &options)
            .map(Bytes::from)
            .ok()
    } else {
        None
    };
    let elapsed = t0.elapsed();
    let size = compiled.binary.len();
    Ok(CompileResponse {
        binary: Bytes::from(compiled.binary),
        size,
        arch: Some(effective_arch),
        status: Some(STATUS_SUCCESS.to_owned()),
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
            execution_model: Some("simt".to_owned()),
        }),
        spirv_binary: spirv,
        provenance: None,
    })
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
/// in the `error` field of each [`DeviceCompileResult`].
#[must_use = "contains per-device results or an error"]
pub fn handle_compile_wgsl_multi(
    req: MultiDeviceCompileRequest,
) -> Result<MultiDeviceCompileResponse, CompileError> {
    if req.wgsl_source.as_ref().is_empty() {
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

    let total_count = req.targets.len();
    let mut results = Vec::with_capacity(total_count);
    let mut success_count = 0usize;

    for target in req.targets {
        let result = (|| -> Result<(coral_reef::CompiledBinary, GpuTarget), CompileError> {
            let gpu_target = parse_target(&target.arch)?;
            let options = CompileOptions {
                target: gpu_target,
                opt_level: req.opt_level,
                debug_info: false,
                fp64_software: fp64_sw,
                fma_policy: fma,
                ..CompileOptions::default()
            };
            let compiled = coral_reef::compile_wgsl_full(req.wgsl_source.as_ref(), &options)?;
            Ok((compiled, gpu_target))
        })();

        match result {
            Ok((compiled, gpu_target)) => {
                success_count += 1;
                let size = compiled.binary.len();
                results.push(DeviceCompileResult {
                    card_index: target.card_index,
                    arch: target.arch,
                    binary: Some(Bytes::from(compiled.binary)),
                    size,
                    error: None,
                    info: Some(CompilationInfoResponse {
                        gpr_count: compiled.info.gpr_count,
                        instr_count: compiled.info.instr_count,
                        shared_mem_bytes: compiled.info.shared_mem_bytes,
                        barrier_count: compiled.info.barrier_count,
                        workgroup_size: compiled.info.local_size,
                        wave_size: wave_size_for(gpu_target),
                        local_memory: compiled.info.local_mem_bytes,
                    }),
                });
            }
            Err(e) => {
                results.push(DeviceCompileResult {
                    card_index: target.card_index,
                    arch: target.arch,
                    binary: None,
                    size: 0,
                    error: Some(e.to_string()),
                    info: None,
                });
            }
        }
    }
    Ok(MultiDeviceCompileResponse {
        results,
        success_count,
        total_count,
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
        status: Some(STATUS_SUCCESS.to_owned()),
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
            hardware_hint: "tensor_core".to_owned(),
            binary_format: Some("ptx".to_owned()),
            execution_model: Some("simt".to_owned()),
        }),
        spirv_binary: None,
        provenance: None,
    })
}
