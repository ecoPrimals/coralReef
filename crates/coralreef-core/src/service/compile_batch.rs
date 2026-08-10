// SPDX-License-Identifier: AGPL-3.0-or-later
//! Batch and multi-device compilation handlers.
//!
//! Handles `shader.compile.wgsl.multi` (same source → many targets) and
//! `shader.compile.multi` (heterogeneous jobs in a single request).

use std::borrow::Cow;
use std::time::Instant;

use bytes::Bytes;
use coral_reef::{CompileError, CompileOptions, FmaPolicy, GpuTarget};

use super::compile::{
    STATUS_SUCCESS, binary_format_for, build_options, handle_compile_spirv, parse_fma_policy,
    parse_target, wave_size_for,
};
use super::types::{
    BatchCompileJobResult, BatchCompileRequest, BatchCompileResponse, CompilationInfoResponse,
    CompileResponse, DeviceCompileResult, MultiDeviceCompileRequest, MultiDeviceCompileResponse,
};

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

/// Execute a batch of mixed-input compilation jobs (`shader.compile.multi`).
///
/// Each job carries its own input type (WGSL, SPIR-V, or GLSL), source, and
/// target architecture. Failures for one job do not prevent others from succeeding.
///
/// # Errors
///
/// Returns [`CompileError`] only if the request is structurally invalid
/// (e.g. empty jobs array). Per-job failures are reported inline.
#[must_use = "contains per-job results or an error"]
pub fn handle_compile_multi(
    req: BatchCompileRequest,
) -> Result<BatchCompileResponse, CompileError> {
    if req.jobs.is_empty() {
        return Err(CompileError::InvalidInput(
            "at least one compilation job required".into(),
        ));
    }

    let total_count = req.jobs.len();
    let mut results = Vec::with_capacity(total_count);
    let mut success_count = 0usize;

    for (index, job) in req.jobs.into_iter().enumerate() {
        let fma = parse_fma_policy(job.fma_policy.as_deref());
        let result = match job.input_type.to_ascii_lowercase().as_str() {
            "wgsl" => compile_wgsl_job(
                &job.source,
                &job.arch,
                job.opt_level,
                job.fp64_software,
                fma,
            ),
            "spirv" => compile_spirv_job(&job.source, &job.arch, job.opt_level, job.fp64_software),
            "glsl" => compile_glsl_job(
                &job.source,
                &job.arch,
                job.opt_level,
                job.fp64_software,
                fma,
            ),
            other => Err(CompileError::InvalidInput(
                format!("unsupported input_type: {other:?} (expected wgsl, spirv, or glsl)").into(),
            )),
        };

        match result {
            Ok(resp) => {
                success_count += 1;
                results.push(BatchCompileJobResult {
                    index,
                    label: job.label,
                    binary: Some(resp.binary),
                    size: resp.size,
                    arch: resp.arch.unwrap_or_default(),
                    input_type: job.input_type,
                    error: None,
                    info: resp.info,
                    compile_time_ms: resp.compile_time_ms,
                });
            }
            Err(e) => {
                results.push(BatchCompileJobResult {
                    index,
                    label: job.label,
                    binary: None,
                    size: 0,
                    arch: job.arch,
                    input_type: job.input_type,
                    error: Some(e.to_string()),
                    info: None,
                    compile_time_ms: None,
                });
            }
        }
    }

    Ok(BatchCompileResponse {
        results,
        success_count,
        total_count,
    })
}

/// Compile a single WGSL job within a batch.
fn compile_wgsl_job(
    source: &str,
    arch: &str,
    opt_level: u32,
    fp64_software: bool,
    fma: FmaPolicy,
) -> Result<CompileResponse, CompileError> {
    let options = build_options(arch, opt_level, fp64_software, fma)?;
    let wave_size = wave_size_for(options.target);
    let t0 = Instant::now();
    let compiled = coral_reef::compile_wgsl_full(source, &options)?;
    let elapsed = t0.elapsed();
    let size = compiled.binary.len();
    Ok(CompileResponse {
        binary: Bytes::from(compiled.binary),
        size,
        arch: Some(arch.to_owned()),
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
            hardware_hint: "compute".into(),
            binary_format: Some(binary_format_for(options.target)),
            execution_model: Some("simt".into()),
        }),
        spirv_binary: None,
        provenance: None,
    })
}

/// Compile a single SPIR-V job within a batch (base64-decoded source).
fn compile_spirv_job(
    source: &str,
    arch: &str,
    opt_level: u32,
    fp64_software: bool,
) -> Result<CompileResponse, CompileError> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(source.as_bytes())
        .map_err(|e| CompileError::InvalidInput(format!("invalid base64 SPIR-V: {e}").into()))?;
    handle_compile_spirv(bytes, arch, opt_level, fp64_software)
}

/// Compile a single GLSL job within a batch.
fn compile_glsl_job(
    source: &str,
    arch: &str,
    opt_level: u32,
    fp64_software: bool,
    fma: FmaPolicy,
) -> Result<CompileResponse, CompileError> {
    let options = build_options(arch, opt_level, fp64_software, fma)?;
    let wave_size = wave_size_for(options.target);
    let t0 = Instant::now();
    let compiled = coral_reef::compile_glsl_full(source, &options)?;
    let elapsed = t0.elapsed();
    let size = compiled.binary.len();
    Ok(CompileResponse {
        binary: Bytes::from(compiled.binary),
        size,
        arch: Some(arch.to_owned()),
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
            hardware_hint: "compute".into(),
            binary_format: Some(binary_format_for(options.target)),
            execution_model: Some("simt".into()),
        }),
        spirv_binary: None,
        provenance: None,
    })
}
