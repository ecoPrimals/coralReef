// SPDX-License-Identifier: AGPL-3.0-or-later
#![forbid(unsafe_code)]
#![warn(missing_docs)]
//! # coral-reef — Sovereign Rust GPU Compiler
//!
//! Multi-vendor GPU compiler: WGSL/SPIR-V/GLSL → vendor-specific binary.
//! Targets NVIDIA (SM70+) and AMD (RDNA2+), with Intel planned.
//!
//! ## Architecture
//!
//! The compiler pipeline is vendor-agnostic: each GPU architecture
//! implements the [`ShaderModel`] trait
//! directly. No manual vtables — Rust's trait dispatch drives
//! vendor-specific legalization, register allocation, and encoding.
//!
//! ## f64 Transcendental Lowering
//!
//! Hardware transcendental units are f32-only.
//! coralReef adds software lowering for f64 transcendentals:
//!
//! | Function | Strategy |
//! |----------|----------|
//! | sin/cos  | Range reduction + DFMA polynomial |
//! | exp2     | Integer/fraction split + DFMA reconstruction |
//! | log2     | Exponent extraction + MUFU.LOG2 + Newton refinement |
//! | sqrt     | MUFU.RSQ64H + two Newton iterations |
//! | rcp      | MUFU.RCP64H + two Newton iterations |
//!
//! ## Public API
//!
//! ```rust
//! use coral_reef::{compile_wgsl, CompileOptions, GpuTarget, NvArch, AmdArch};
//!
//! let wgsl = "@compute @workgroup_size(1) fn main() {}";
//!
//! // NVIDIA
//! let nv_binary = compile_wgsl(wgsl, &CompileOptions {
//!     target: GpuTarget::Nvidia(NvArch::Sm86),
//!     ..Default::default()
//! }).unwrap();
//!
//! // AMD
//! let amd_binary = compile_wgsl(wgsl, &CompileOptions {
//!     target: GpuTarget::Amd(AmdArch::Rdna2),
//!     ..Default::default()
//! }).unwrap();
//! ```

pub mod backend;
pub mod env_keys;
pub mod error;
pub mod frontend;
pub mod gpu_arch;
pub mod ir;
pub mod tol;
pub mod tolerances;

// Codegen module — evolved from upstream NAK into idiomatic Rust.
// ISA domain types intentionally use naming conventions that mirror
// hardware documentation (e.g. OpFAdd, SrcType, UGPR). dead_code covers
// builder traits and ISA variants reserved for future use.
mod codegen;
pub mod gemm;
mod preamble;

pub use backend::{AmdBackend, Backend, BinaryFormat, CompiledBinary, NvidiaBackend};
pub use codegen::ir::{Shader, ShaderModel};
pub use codegen::pipeline::CompiledShader;
pub use error::CompileError;
pub use frontend::{Frontend, NagaFrontend};
pub use gemm::{GemmPrecision, GemmShape, compile_gemm};
pub use gpu_arch::{
    AmdArch, CompileTarget, CpuArch, GpuArch, GpuTarget, IntelArch, NpuTarget, NvArch,
};

/// Re-export naga for callers that build `naga::Module` directly.
pub use naga;

/// df64 (double-float) WGSL preamble — Dekker/Knuth pair arithmetic.
/// Prepended automatically when source uses df64 functions or when
/// `Fp64Strategy::DoubleFloat` is selected.
const DF64_PREAMBLE: &str = include_str!("df64_preamble.wgsl");

/// Complex64 WGSL preamble — complex arithmetic on native f64 pairs.
/// Prepended automatically when source uses Complex64 or c64_ functions.
const COMPLEX64_PREAMBLE: &str = include_str!("complex_f64_preamble.wgsl");

/// f32 transcendental workaround preamble — ecosystem health monitoring polyfills.
/// Prepended automatically when source uses `power_f32`, `log_f32_safe`, or `exp_f32_safe`.
const F32_TRANSCENDENTAL_PREAMBLE: &str = include_str!("f32_transcendental_preamble.wgsl");

/// PRNG preamble — `xorshift32` and `wang_hash`.
/// Prepended automatically when source uses `xorshift32` or `wang_hash`.
const PRNG_PREAMBLE: &str = include_str!("prng_preamble.wgsl");

/// SU(3) lattice preamble — 3×3 unitary matrix operations for lattice QCD.
/// Prepended automatically when source uses `su3_` functions.
/// Depends on Complex64 preamble (auto-chained).
const SU3_PREAMBLE: &str = include_str!("su3_f64_preamble.wgsl");

/// FMA (fused multiply-add) control policy.
///
/// SPIR-V `NoContraction` and WGSL `@fma_control` decorations map to this.
/// Controls whether the compiler may fuse `a*b + c` into a single FMA
/// instruction, which changes rounding behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FmaPolicy {
    /// Allow FMA contractions (fastest).
    Fused,
    /// Prevent FMA — separate multiply + add for IEEE compliance.
    Separate,
    /// Let the compiler decide based on architecture.
    #[default]
    Auto,
}

/// Three-tier f64 precision strategy.
///
/// The compute dispatcher decides WHICH tier based on accuracy requirements and hardware.
/// coralReef decides HOW to implement the tier on the target GPU.
///
/// | Strategy     | Mantissa | Throughput vs f32 | Use case |
/// |--------------|----------|-------------------|----------|
/// | Native       | 52 bits  | 1/2 (HPC) to 1/32 (consumer) | Gold standard |
/// | DoubleFloat  | ~48 bits | ~1/4 of f32 | Science on consumer GPUs |
/// | F32Only      | 24 bits  | 1:1 | Visualization, inference |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fp64Strategy {
    /// Use native f64 hardware with software transcendental lowering.
    /// Best on HPC GPUs (Titan V, A100, MI250) with fast f64 (1:2 rate).
    #[default]
    Native,
    /// Lower f64 ops to double-float (df64) pair arithmetic using f32 cores.
    /// ~48-bit mantissa, 12-18x faster than native f64 on consumer GPUs.
    /// Requires df64 preamble (Dekker multiplication, Knuth two-sum).
    DoubleFloat,
    /// Truncate f64 to f32. Lossy — only for visualization or tolerance-insensitive work.
    F32Only,
}

/// Compile options for the shader compiler.
#[derive(Debug, Clone)]
pub struct CompileOptions {
    /// Target GPU — vendor-discriminated.
    pub target: GpuTarget,
    /// Optimization level (0-3).
    pub opt_level: u32,
    /// Include debug info in output.
    pub debug_info: bool,
    /// f64 precision strategy — controls how f64 operations are compiled.
    pub fp64_strategy: Fp64Strategy,
    /// Legacy: enable software lowering for f64 transcendentals.
    /// Equivalent to `fp64_strategy != F32Only`.
    pub fp64_software: bool,
    /// FMA fusion policy — controls whether `a*b + c` may be fused.
    pub fma_policy: FmaPolicy,
    /// Select a specific entry point by name for `compile_module*` APIs.
    /// When `None`, uses the first compute entry point in the module.
    pub entry_point: Option<String>,
    /// Run `naga::valid::Validator` on modules before compilation.
    /// Catches malformed IR early with clear diagnostics. Disable for
    /// trusted modules where validation overhead is unwanted.
    pub validate: bool,
}

impl CompileOptions {
    /// Convenience: the NVIDIA architecture, if the target is NVIDIA.
    #[must_use]
    pub const fn nv_arch(&self) -> Option<NvArch> {
        self.target.as_nvidia()
    }

    /// Convenience: the AMD architecture, if the target is AMD.
    #[must_use]
    pub const fn amd_arch(&self) -> Option<AmdArch> {
        self.target.as_amd()
    }

    /// Backward-compatible accessor — returns the `NvArch` or an error.
    ///
    /// Prefer [`Self::nv_arch`] in new code.
    ///
    /// # Errors
    ///
    /// Returns [`CompileError::UnsupportedArch`] if the target is not an NVIDIA GPU.
    pub fn arch(&self) -> Result<GpuArch, CompileError> {
        self.target.as_nvidia().ok_or_else(|| {
            CompileError::UnsupportedArch(
                format!("expected NVIDIA target, got {}", self.target).into(),
            )
        })
    }
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            target: GpuTarget::default(),
            opt_level: 2,
            debug_info: false,
            fp64_strategy: Fp64Strategy::default(),
            fp64_software: true,
            fma_policy: FmaPolicy::default(),
            entry_point: None,
            validate: true,
        }
    }
}

/// Build the appropriate `ShaderModel` for the target and return it boxed.
///
/// This replaces the old `ShaderModelInfo::new(sm, warps_per_sm)` approach
/// with direct construction of the vendor-specific model.
fn shader_model_for(target: GpuTarget) -> Result<Box<dyn codegen::ir::ShaderModel>, CompileError> {
    match target {
        GpuTarget::Nvidia(nv) => {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "warps_per_sm is always <= 64"
            )]
            let warps = nv.max_warps_per_sm() as u8;
            Ok(Box::new(codegen::ir::ShaderModelInfo::new(
                nv.sm_version(),
                warps,
            )))
        }
        GpuTarget::Amd(amd) => {
            let gfx = amd.gfx_major() * 10 + 3;
            let wave = amd.default_wave_size();
            Ok(Box::new(
                codegen::amd::shader_model::ShaderModelRdna2::new(gfx).with_wave_size(wave),
            ))
        }
        GpuTarget::Intel(_) => Err(CompileError::UnsupportedArch(target.to_string().into())),
    }
}

/// Compile SPIR-V to native GPU binary.
///
/// This is the primary entry point for the compiler. Supports both
/// NVIDIA and AMD targets via [`CompileOptions::target`].
///
/// # Errors
///
/// Returns [`CompileError`] if compilation fails.
pub fn compile(spirv: &[u32], options: &CompileOptions) -> Result<Vec<u8>, CompileError> {
    compile_with(&NagaFrontend, spirv, options)
}

/// Compile SPIR-V to native GPU binary using a custom [`Frontend`].
///
/// # Errors
///
/// Returns [`CompileError`] if compilation fails.
pub fn compile_with(
    frontend: &dyn Frontend,
    spirv: &[u32],
    options: &CompileOptions,
) -> Result<Vec<u8>, CompileError> {
    if spirv.is_empty() {
        return Err(CompileError::InvalidInput("empty SPIR-V module".into()));
    }
    tracing::info!(
        target = %options.target,
        opt = options.opt_level,
        fp64 = options.fp64_software,
        "coral-reef compile"
    );

    let sm = shader_model_for(options.target)?;
    let mut shader = frontend.compile_spirv(spirv, sm.as_ref())?;
    shader.fma_policy = options.fma_policy;
    let compiled = compile_ir(&mut shader)?;
    Ok(emit_binary(&compiled, options.target))
}

/// Compile a pre-built IR shader through the full optimization and encoding pipeline.
///
/// # Errors
///
/// Returns [`CompileError`] if compilation fails.
pub fn compile_ir(shader: &mut Shader<'_>) -> Result<CompiledShader, CompileError> {
    codegen::pipeline::compile_shader(shader, false)
}

use preamble::prepare_wgsl;
#[cfg(test)]
use preamble::strip_enable_directives;

/// Compile WGSL source to native GPU binary.
///
/// Convenience wrapper using the default [`NagaFrontend`].
/// Auto-prepends preambles (Complex64, PRNG, SU3, df64, f32 transcendental)
/// when the source uses their respective types or functions.
///
/// # Errors
///
/// Returns [`CompileError`] if parsing or compilation fails.
pub fn compile_wgsl(wgsl: &str, options: &CompileOptions) -> Result<Vec<u8>, CompileError> {
    compile_wgsl_with(&NagaFrontend, wgsl, options)
}

/// Compile WGSL source to [`CompiledBinary`] with full metadata.
///
/// Returns the native GPU binary plus compilation info (GPR count,
/// instruction count) needed by the driver for QMD construction.
///
/// # Errors
///
/// Returns [`CompileError`] if parsing or compilation fails.
pub fn compile_wgsl_full(
    wgsl: &str,
    options: &CompileOptions,
) -> Result<CompiledBinary, CompileError> {
    compile_wgsl_full_with(&NagaFrontend, wgsl, options)
}

/// Compile WGSL source to [`CompiledBinary`] using a custom [`Frontend`].
///
/// # Errors
///
/// Returns [`CompileError`] if parsing or compilation fails.
pub fn compile_wgsl_full_with(
    frontend: &dyn Frontend,
    wgsl: &str,
    options: &CompileOptions,
) -> Result<CompiledBinary, CompileError> {
    if wgsl.is_empty() {
        return Err(CompileError::InvalidInput("empty WGSL source".into()));
    }
    let prepared = prepare_wgsl(wgsl, options);
    tracing::info!(
        target = %options.target,
        opt = options.opt_level,
        "coral-reef compile_wgsl_full"
    );

    if let Some(nv) = options.target.as_nvidia() {
        if nv.sm() >= 100 {
            return codegen::nv::ptx_emit::emit_compute_ptx(&prepared, nv.sm_version());
        }
    }

    let sm = shader_model_for(options.target)?;
    let mut shader = frontend.compile_wgsl(&prepared, sm.as_ref())?;
    shader.fma_policy = options.fma_policy;
    let backend = backend::backend_for(options.target)?;
    backend.compile(&mut shader)
}

/// Emit validated SPIR-V binary from WGSL source.
///
/// Parses the WGSL, validates with `naga::valid::Validator`, and emits
/// standard SPIR-V binary (magic `0x07230203`). This is the sovereign
/// SPIR-V output path — GAP-HS-124.
///
/// Returns SPIR-V words as a `Vec<u8>` (little-endian u32 words packed
/// to bytes, as expected by Vulkan `VkShaderModuleCreateInfo`).
///
/// # Errors
///
/// Returns [`CompileError`] if WGSL parsing or SPIR-V emission fails.
pub fn wgsl_to_spirv(wgsl: &str, options: &CompileOptions) -> Result<Vec<u8>, CompileError> {
    if wgsl.is_empty() {
        return Err(CompileError::InvalidInput("empty WGSL source".into()));
    }
    let prepared = prepare_wgsl(wgsl, options);
    let module = naga::front::wgsl::parse_str(&prepared)
        .map_err(|e| CompileError::InvalidInput(format!("WGSL parse: {e}").into()))?;

    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    let info = validator
        .validate(&module)
        .map_err(|e| CompileError::Validation(format!("{e}").into()))?;

    let spv_words =
        naga::back::spv::write_vec(&module, &info, &naga::back::spv::Options::default(), None)
            .map_err(|e| CompileError::Encoding(format!("SPIR-V emit: {e}").into()))?;

    let mut bytes = Vec::with_capacity(spv_words.len() * 4);
    for word in &spv_words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    Ok(bytes)
}

/// Compile WGSL source to native GPU binary using a custom [`Frontend`].
///
/// # Errors
///
/// Returns [`CompileError`] if parsing or compilation fails.
pub fn compile_wgsl_with(
    frontend: &dyn Frontend,
    wgsl: &str,
    options: &CompileOptions,
) -> Result<Vec<u8>, CompileError> {
    if wgsl.is_empty() {
        return Err(CompileError::InvalidInput("empty WGSL source".into()));
    }
    let prepared = prepare_wgsl(wgsl, options);
    tracing::info!(
        target = %options.target,
        opt = options.opt_level,
        "coral-reef compile_wgsl"
    );

    if let Some(nv) = options.target.as_nvidia() {
        if nv.sm() >= 100 {
            let compiled = codegen::nv::ptx_emit::emit_compute_ptx(&prepared, nv.sm_version())?;
            return Ok(compiled.binary);
        }
    }

    let sm = shader_model_for(options.target)?;
    let mut shader = frontend.compile_wgsl(&prepared, sm.as_ref())?;
    shader.fma_policy = options.fma_policy;
    let compiled = compile_ir(&mut shader)?;
    Ok(emit_binary(&compiled, options.target))
}

/// Compile a pre-parsed `naga::Module` to native GPU binary.
///
/// Accepts a naga IR module directly, skipping the WGSL/SPIR-V/GLSL
/// parse step. This avoids the overhead of text round-tripping when
/// the caller already holds a `naga::Module` (e.g. from programmatic
/// IR construction or a custom frontend).
///
/// Entry point selection: set `options.entry_point` to target a specific
/// entry point by name, or leave it `None` to use the first compute entry
/// point in the module.
///
/// # Errors
///
/// Returns [`CompileError`] if the module has no entry points, validation
/// fails, or compilation fails.
pub fn compile_module(
    module: &naga::Module,
    options: &CompileOptions,
) -> Result<Vec<u8>, CompileError> {
    let compiled = compile_module_full(module, options)?;
    Ok(compiled.binary)
}

/// Compile a pre-parsed `naga::Module` to [`CompiledBinary`] with full metadata.
///
/// Like [`compile_module`] but returns compilation info (GPR count,
/// shared memory, barriers) needed by the driver for dispatch setup.
///
/// Entry point selection: set `options.entry_point` to target a specific
/// entry point by name, or leave it `None` to use the first compute entry
/// point in the module.
///
/// # Errors
///
/// Returns [`CompileError`] if the module has no entry points, validation
/// fails, or compilation fails.
pub fn compile_module_full(
    module: &naga::Module,
    options: &CompileOptions,
) -> Result<CompiledBinary, CompileError> {
    if module.entry_points.is_empty() {
        return Err(CompileError::InvalidInput(
            "naga::Module has no entry points".into(),
        ));
    }

    if options.validate {
        validate_module(module)?;
    }

    let ep = resolve_entry_point(module, options.entry_point.as_deref())?;

    tracing::info!(
        target = %options.target,
        opt = options.opt_level,
        entry_point = %ep.name,
        "coral-reef compile_module"
    );

    if let Some(nv) = options.target.as_nvidia() {
        if nv.sm() >= 100 {
            return codegen::nv::ptx_emit::emit_compute_ptx_module(
                module,
                nv.sm_version(),
                Some(&ep.name),
            );
        }
    }

    let sm = shader_model_for(options.target)?;
    let mut shader = codegen::naga_translate::translate(module, sm.as_ref(), &ep.name)?;
    shader.fma_policy = options.fma_policy;
    let backend = backend::backend_for(options.target)?;
    backend.compile(&mut shader)
}

/// Validate a `naga::Module` using naga's built-in validator.
///
/// Returns `Ok(())` on success, or a [`CompileError::Validation`] with
/// diagnostic details on failure.
fn validate_module(module: &naga::Module) -> Result<(), CompileError> {
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator
        .validate(module)
        .map_err(|e| CompileError::Validation(format!("{e}").into()))?;
    Ok(())
}

/// Resolve which entry point to compile from the module.
///
/// If `name` is `Some`, looks up that specific entry point by name and
/// verifies it is a compute shader.
/// If `None`, returns the first compute-stage entry point, or an error
/// if no compute entry points exist.
fn resolve_entry_point<'m>(
    module: &'m naga::Module,
    name: Option<&str>,
) -> Result<&'m naga::EntryPoint, CompileError> {
    match name {
        Some(requested) => {
            let ep = module
                .entry_points
                .iter()
                .find(|ep| ep.name == requested)
                .ok_or_else(|| {
                    let available: Vec<&str> = module
                        .entry_points
                        .iter()
                        .map(|ep| ep.name.as_str())
                        .collect();
                    CompileError::InvalidInput(
                        format!("entry point '{requested}' not found; available: {available:?}")
                            .into(),
                    )
                })?;
            if ep.stage != naga::ShaderStage::Compute {
                return Err(CompileError::InvalidInput(
                    format!(
                        "entry point '{}' is {:?}, not Compute — \
                         coralReef only compiles compute shaders",
                        ep.name, ep.stage,
                    )
                    .into(),
                ));
            }
            Ok(ep)
        }
        None => module
            .entry_points
            .iter()
            .find(|ep| ep.stage == naga::ShaderStage::Compute)
            .ok_or_else(|| {
                let stages: Vec<String> = module
                    .entry_points
                    .iter()
                    .map(|ep| format!("{}({:?})", ep.name, ep.stage))
                    .collect();
                CompileError::InvalidInput(
                    format!("no compute entry point found; module has: {stages:?}").into(),
                )
            }),
    }
}

/// Compile GLSL compute shader source to native GPU binary.
///
/// The source must be a `#version 450` (or 460) compute shader.
/// GLSL is parsed via naga's GLSL frontend — no df64 preamble injection.
///
/// # Errors
///
/// Returns [`CompileError`] if parsing or compilation fails.
pub fn compile_glsl(glsl: &str, options: &CompileOptions) -> Result<Vec<u8>, CompileError> {
    compile_glsl_with(&NagaFrontend, glsl, options)
}

/// Compile GLSL compute shader source to native GPU binary using a custom [`Frontend`].
///
/// # Errors
///
/// Returns [`CompileError`] if parsing or compilation fails.
pub fn compile_glsl_with(
    frontend: &dyn Frontend,
    glsl: &str,
    options: &CompileOptions,
) -> Result<Vec<u8>, CompileError> {
    if glsl.is_empty() {
        return Err(CompileError::InvalidInput("empty GLSL source".into()));
    }
    tracing::info!(
        target = %options.target,
        opt = options.opt_level,
        "coral-reef compile_glsl"
    );

    let sm = shader_model_for(options.target)?;
    let mut shader = frontend.compile_glsl(glsl, sm.as_ref())?;
    shader.fma_policy = options.fma_policy;
    let compiled = compile_ir(&mut shader)?;
    Ok(emit_binary(&compiled, options.target))
}

/// Compile GLSL compute shader source to [`CompiledBinary`] with full metadata.
///
/// # Errors
///
/// Returns [`CompileError`] if parsing or compilation fails.
pub fn compile_glsl_full(
    glsl: &str,
    options: &CompileOptions,
) -> Result<CompiledBinary, CompileError> {
    compile_glsl_full_with(&NagaFrontend, glsl, options)
}

/// Compile GLSL compute shader to [`CompiledBinary`] using a custom [`Frontend`].
///
/// # Errors
///
/// Returns [`CompileError`] if parsing or compilation fails.
pub fn compile_glsl_full_with(
    frontend: &dyn Frontend,
    glsl: &str,
    options: &CompileOptions,
) -> Result<CompiledBinary, CompileError> {
    if glsl.is_empty() {
        return Err(CompileError::InvalidInput("empty GLSL source".into()));
    }
    tracing::info!(
        target = %options.target,
        opt = options.opt_level,
        "coral-reef compile_glsl_full"
    );

    let sm = shader_model_for(options.target)?;
    let mut shader = frontend.compile_glsl(glsl, sm.as_ref())?;
    shader.fma_policy = options.fma_policy;
    let backend = backend::backend_for(options.target)?;
    backend.compile(&mut shader)
}

fn emit_binary(compiled: &CompiledShader, target: GpuTarget) -> Vec<u8> {
    // Compute shaders have no SPH — all dispatch metadata lives in the QMD.
    // The all-zeros header produced by encode_header for Compute is a sentinel
    // that must be stripped; graphics SPH always has non-zero SPH_TYPE/VERSION.
    let include_header = target.as_nvidia().is_some() && compiled.header.iter().any(|&w| w != 0);
    let header_size = if include_header {
        compiled.header.len() * 4
    } else {
        0
    };
    let mut binary = Vec::with_capacity(header_size + compiled.code.len() * 4);
    if include_header {
        for word in &compiled.header {
            binary.extend_from_slice(&word.to_le_bytes());
        }
    }
    for word in &compiled.code {
        binary.extend_from_slice(&word.to_le_bytes());
    }
    binary
}

/// Compile WGSL using a raw NVIDIA SM version number (test infrastructure).
///
/// Allows integration tests to exercise legacy SM20/SM32/SM50 encoder paths
/// that are not reachable through the public `NvArch` enum.
///
/// # Errors
///
/// Returns [`CompileError`] if compilation fails.
#[doc(hidden)]
pub fn compile_wgsl_raw_sm(wgsl: &str, sm: u8) -> Result<Vec<u8>, CompileError> {
    if wgsl.is_empty() {
        return Err(CompileError::InvalidInput("empty WGSL source".into()));
    }
    let warps: u8 = if sm >= 70 { 64 } else { 32 };
    let sm_info: Box<dyn codegen::ir::ShaderModel> =
        Box::new(codegen::ir::ShaderModelInfo::new(sm, warps));
    let frontend = NagaFrontend;
    let mut shader = frontend.compile_wgsl(wgsl, sm_info.as_ref())?;
    let compiled = compile_ir(&mut shader)?;
    Ok(emit_binary(&compiled, GpuTarget::Nvidia(NvArch::Sm70)))
}

#[cfg(test)]
#[path = "lib_tests/mod.rs"]
mod tests;
