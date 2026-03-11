// SPDX-License-Identifier: AGPL-3.0-only
//! Compilation handlers — SPIR-V, WGSL, and multi-device.

use bytes::Bytes;
use coral_reef::{AmdArch, CompileError, CompileOptions, FmaPolicy, GpuTarget, NvArch};

use super::types::{
    CompileRequest, CompileResponse, CompileWgslRequest, DeviceCompileResult,
    MultiDeviceCompileRequest, MultiDeviceCompileResponse,
};

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

/// Parse an optional FMA policy string into an [`FmaPolicy`].
pub(crate) fn parse_fma_policy(s: Option<&str>) -> FmaPolicy {
    match s {
        Some("fused") => FmaPolicy::Fused,
        Some("separate") => FmaPolicy::Separate,
        _ => FmaPolicy::Auto,
    }
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
