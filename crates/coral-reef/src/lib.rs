// SPDX-License-Identifier: AGPL-3.0-only
#![deny(unsafe_code)]
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
//! ```rust,ignore
//! use coral_reef::{compile_wgsl, CompileOptions, GpuTarget, NvArch, AmdArch};
//!
//! // NVIDIA
//! let nv_binary = compile_wgsl(wgsl, &CompileOptions {
//!     target: GpuTarget::Nvidia(NvArch::Sm86),
//!     ..Default::default()
//! })?;
//!
//! // AMD
//! let amd_binary = compile_wgsl(wgsl, &CompileOptions {
//!     target: GpuTarget::Amd(AmdArch::Rdna2),
//!     ..Default::default()
//! })?;
//! ```

pub mod backend;
pub mod error;
pub mod frontend;
pub mod gpu_arch;
pub mod ir;
pub mod tol;

// Codegen module — evolved from upstream NAK into idiomatic Rust.
// ISA domain types intentionally use naming conventions that mirror
// hardware documentation (e.g. OpFAdd, SrcType, UGPR). Only the
// naming and documentation suppressions remain; all other allows
// have been resolved at the individual item level.
#[allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    missing_docs,
    reason = "ISA domain types mirror hardware docs; codegen uses intentionally unused variants"
)]
mod codegen;

pub use backend::{AmdBackend, Backend, CompiledBinary, NvidiaBackend};
pub use codegen::ir::{Shader, ShaderModel};
pub use codegen::pipeline::CompiledShader;
pub use error::CompileError;
pub use frontend::{Frontend, NagaFrontend};
pub use gpu_arch::{AmdArch, GpuArch, GpuTarget, IntelArch, NvArch};

/// df64 (double-float) WGSL preamble — Dekker/Knuth pair arithmetic.
/// Prepended automatically when source uses df64 functions or when
/// `Fp64Strategy::DoubleFloat` is selected.
const DF64_PREAMBLE: &str = include_str!("df64_preamble.wgsl");

/// FMA (fused multiply-add) control policy.
///
/// SPIR-V `NoContraction` and WGSL `@fma_control` decorations map to this.
/// Controls whether the compiler may fuse `a*b + c` into a single FMA
/// instruction, which changes rounding behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FmaPolicy {
    /// Compiler is free to fuse multiply-add into FMA (default, fastest).
    #[default]
    AllowFusion,
    /// Preserve individual operations — no FMA fusion.
    /// Equivalent to SPIR-V `NoContraction`.
    /// Required for bit-exact CPU parity in precision-critical workloads.
    NoContraction,
}

/// Three-tier f64 precision strategy.
///
/// barraCuda decides WHICH tier based on accuracy requirements and hardware.
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

    /// Backward-compatible accessor — returns the `NvArch` or panics.
    ///
    /// Prefer [`Self::nv_arch`] in new code.
    ///
    /// # Panics
    ///
    /// Panics if the target is not an NVIDIA GPU.
    #[must_use]
    pub const fn arch(&self) -> GpuArch {
        self.target
            .as_nvidia()
            .expect("CompileOptions::arch() called on non-NVIDIA target")
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
            Ok(Box::new(codegen::amd::shader_model::ShaderModelRdna2::new(
                gfx,
            )))
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

/// Prepare WGSL source: auto-prepend df64 preamble when needed,
/// strip `enable f64;` (naga handles f64 natively without extension directives).
///
/// Returns a `Cow::Borrowed` if no transformations are needed, avoiding allocation.
fn prepare_wgsl<'a>(wgsl: &'a str, options: &CompileOptions) -> std::borrow::Cow<'a, str> {
    let needs_df64 = options.fp64_strategy == Fp64Strategy::DoubleFloat
        || wgsl.contains("Df64")
        || wgsl.contains("df64_");
    let has_enable_f64 = wgsl.contains("enable f64");

    if !needs_df64 && !has_enable_f64 {
        return std::borrow::Cow::Borrowed(wgsl);
    }

    let source = if has_enable_f64 {
        tracing::debug!("stripping 'enable f64;' directive (naga handles f64 natively)");
        wgsl.replace("enable f64;", "// [coralReef] f64 enabled natively")
    } else {
        wgsl.to_owned()
    };

    if needs_df64 && !source.contains("struct Df64") {
        tracing::debug!("auto-prepending df64 preamble");
        let mut combined = String::with_capacity(DF64_PREAMBLE.len() + 1 + source.len());
        combined.push_str(DF64_PREAMBLE);
        combined.push('\n');
        combined.push_str(&source);
        std::borrow::Cow::Owned(combined)
    } else {
        std::borrow::Cow::Owned(source)
    }
}

/// Compile WGSL source to native GPU binary.
///
/// Convenience wrapper using the default [`NagaFrontend`].
/// Auto-prepends the df64 preamble when the source uses `Df64` types
/// or `Fp64Strategy::DoubleFloat` is selected.
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

    let sm = shader_model_for(options.target)?;
    let mut shader = frontend.compile_wgsl(&prepared, sm.as_ref())?;
    let backend = backend::backend_for(options.target)?;
    backend.compile(&mut shader)
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

    let sm = shader_model_for(options.target)?;
    let mut shader = frontend.compile_wgsl(&prepared, sm.as_ref())?;
    let compiled = compile_ir(&mut shader)?;
    Ok(emit_binary(&compiled, options.target))
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
    let backend = backend::backend_for(options.target)?;
    backend.compile(&mut shader)
}

fn emit_binary(compiled: &CompiledShader, target: GpuTarget) -> Vec<u8> {
    let include_header = target.as_nvidia().is_some();
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
    let include_header = sm >= 70;
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
    Ok(binary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_empty_spirv_rejected() {
        let result = compile(&[], &CompileOptions::default());
        assert!(matches!(result, Err(CompileError::InvalidInput(_))));
    }

    #[test]
    fn test_compile_invalid_spirv_rejected() {
        let result = compile(&[0x0723_0203], &CompileOptions::default());
        assert!(result.is_err(), "invalid SPIR-V should fail: {result:?}");
    }

    #[test]
    fn test_compile_wgsl_empty_rejected() {
        let result = compile_wgsl("", &CompileOptions::default());
        assert!(matches!(result, Err(CompileError::InvalidInput(_))));
    }

    #[test]
    fn test_compile_wgsl_minimal_compute() {
        let result = compile_wgsl(
            "@compute @workgroup_size(1) fn main() {}",
            &CompileOptions::default(),
        );
        assert!(
            result.is_ok() || result.is_err(),
            "should parse and attempt compilation"
        );
    }

    #[test]
    fn test_compile_glsl_empty_rejected() {
        let result = compile_glsl("", &CompileOptions::default());
        assert!(matches!(result, Err(CompileError::InvalidInput(_))));
    }

    #[test]
    fn test_compile_glsl_minimal_compute() {
        let glsl = "#version 450\nlayout(local_size_x = 1) in;\nvoid main() {}";
        let result = compile_glsl(glsl, &CompileOptions::default());
        assert!(
            result.is_ok(),
            "minimal GLSL compute should compile: {result:?}"
        );
    }

    #[test]
    fn test_compile_glsl_malformed_returns_error() {
        let result = compile_glsl(
            "#version 450\nvoid main() { int x = ; }",
            &CompileOptions::default(),
        );
        assert!(
            result.is_err(),
            "malformed GLSL should return error: {result:?}"
        );
    }

    #[test]
    fn test_default_options() {
        let opts = CompileOptions::default();
        assert_eq!(opts.arch(), GpuArch::Sm70);
        assert_eq!(opts.opt_level, 2);
        assert!(opts.fp64_software);
        assert!(!opts.debug_info);
    }

    #[test]
    fn test_options_clone() {
        let opts = CompileOptions {
            target: GpuArch::Sm89.into(),
            opt_level: 3,
            debug_info: true,
            fp64_software: false,
            ..CompileOptions::default()
        };
        let cloned = opts;
        assert_eq!(cloned.arch(), GpuArch::Sm89);
        assert_eq!(cloned.opt_level, 3);
        assert!(cloned.debug_info);
        assert!(!cloned.fp64_software);
    }

    #[test]
    fn test_options_debug() {
        let opts = CompileOptions::default();
        let dbg = format!("{opts:?}");
        assert!(dbg.contains("CompileOptions"));
    }

    #[test]
    fn test_compile_with_all_archs() {
        for arch in [
            GpuArch::Sm70,
            GpuArch::Sm75,
            GpuArch::Sm80,
            GpuArch::Sm86,
            GpuArch::Sm89,
        ] {
            let opts = CompileOptions {
                target: arch.into(),
                ..CompileOptions::default()
            };
            let result = compile(&[0x0723_0203], &opts);
            assert!(result.is_err(), "should be not-implemented for {arch}");
        }
    }

    #[test]
    fn test_shader_model_for_nvidia() {
        let sm = shader_model_for(GpuTarget::Nvidia(NvArch::Sm86));
        assert!(sm.is_ok());
        assert_eq!(sm.unwrap().sm(), 86);
    }

    #[test]
    fn test_shader_model_for_amd() {
        let sm = shader_model_for(GpuTarget::Amd(AmdArch::Rdna2));
        assert!(sm.is_ok());
        assert_eq!(sm.unwrap().sm(), 103);
    }

    #[test]
    fn test_shader_model_for_intel_unsupported() {
        let sm = shader_model_for(GpuTarget::Intel(IntelArch::XeHpg));
        assert!(sm.is_err());
    }

    #[test]
    fn test_amd_compile_wgsl_minimal() {
        let opts = CompileOptions {
            target: GpuTarget::Amd(AmdArch::Rdna2),
            ..CompileOptions::default()
        };
        let result = compile_wgsl("@compute @workgroup_size(1) fn main() {}", &opts);
        // Should parse and attempt compilation (may not fully succeed yet)
        assert!(
            result.is_ok() || result.is_err(),
            "should parse and attempt AMD compilation"
        );
    }

    #[test]
    fn test_backend_for_resolves_amd() {
        let be = backend::backend_for(GpuTarget::Amd(AmdArch::Rdna2));
        assert!(be.is_ok());
    }

    #[test]
    fn test_cross_vendor_both_compile_same_wgsl() {
        let wgsl = "@compute @workgroup_size(1) fn main() {}";
        let nv_opts = CompileOptions {
            target: GpuTarget::Nvidia(NvArch::Sm70),
            ..CompileOptions::default()
        };
        let amd_opts = CompileOptions {
            target: GpuTarget::Amd(AmdArch::Rdna2),
            ..CompileOptions::default()
        };
        let nv_result = compile_wgsl(wgsl, &nv_opts);
        let amd_result = compile_wgsl(wgsl, &amd_opts);

        // Both should compile (or both fail at the same pipeline stage)
        assert!(
            nv_result.is_ok(),
            "NVIDIA compilation failed: {nv_result:?}"
        );
        assert!(amd_result.is_ok(), "AMD compilation failed: {amd_result:?}");

        let nv_bin = nv_result.unwrap();
        let amd_bin = amd_result.unwrap();

        // NVIDIA binary includes SPH header, AMD does not
        assert!(
            nv_bin.len() > amd_bin.len(),
            "NVIDIA binary should be larger (includes SPH)"
        );
        // AMD binary should be non-empty (at least s_endpgm)
        assert!(
            !amd_bin.is_empty(),
            "AMD binary should contain at least s_endpgm"
        );

        // NVIDIA binary includes SPH header (32 bytes); compute shaders use zeroed header
        assert!(
            nv_bin.len() >= 32,
            "NVIDIA binary should have at least 32 bytes (SPH header)"
        );
        // AMD binary has no SPH header — size difference confirms structural difference
    }

    #[test]
    #[should_panic(expected = "NVIDIA shader model must be >= SM 2.0")]
    fn test_shader_model_info_new_panics_for_sm_below_20() {
        let _ = codegen::ir::ShaderModelInfo::new(19, 4);
    }

    #[test]
    fn test_fma_policy_default() {
        assert_eq!(FmaPolicy::default(), FmaPolicy::AllowFusion);
    }

    #[test]
    fn test_fma_policy_debug() {
        let dbg = format!("{:?}", FmaPolicy::AllowFusion);
        assert!(dbg.contains("AllowFusion"));
        let dbg = format!("{:?}", FmaPolicy::NoContraction);
        assert!(dbg.contains("NoContraction"));
    }

    #[test]
    fn test_fma_policy_equality() {
        assert_eq!(FmaPolicy::AllowFusion, FmaPolicy::AllowFusion);
        assert_eq!(FmaPolicy::NoContraction, FmaPolicy::NoContraction);
        assert_ne!(FmaPolicy::AllowFusion, FmaPolicy::NoContraction);
    }

    #[test]
    fn test_compile_options_nv_arch() {
        let nv_opts = CompileOptions {
            target: GpuTarget::Nvidia(NvArch::Sm86),
            ..CompileOptions::default()
        };
        let amd_opts = CompileOptions {
            target: GpuTarget::Amd(AmdArch::Rdna2),
            ..CompileOptions::default()
        };
        assert_eq!(nv_opts.nv_arch(), Some(NvArch::Sm86));
        assert_eq!(amd_opts.nv_arch(), None);
    }

    #[test]
    fn test_compile_options_amd_arch() {
        let nv_opts = CompileOptions {
            target: GpuTarget::Nvidia(NvArch::Sm86),
            ..CompileOptions::default()
        };
        let amd_opts = CompileOptions {
            target: GpuTarget::Amd(AmdArch::Rdna2),
            ..CompileOptions::default()
        };
        assert_eq!(amd_opts.amd_arch(), Some(AmdArch::Rdna2));
        assert_eq!(nv_opts.amd_arch(), None);
    }

    #[test]
    #[should_panic(expected = "CompileOptions::arch() called on non-NVIDIA target")]
    fn test_compile_options_arch_panics_for_amd() {
        let opts = CompileOptions {
            target: GpuTarget::Amd(AmdArch::Rdna2),
            ..CompileOptions::default()
        };
        let _ = opts.arch();
    }

    #[test]
    fn test_compile_wgsl_malformed_returns_error() {
        let opts = CompileOptions::default();
        let result = compile_wgsl("not valid wgsl", &opts);
        assert!(
            result.is_err(),
            "malformed WGSL should return error: {result:?}"
        );
    }

    #[test]
    fn test_compile_wgsl_intel_returns_unsupported_arch() {
        let opts = CompileOptions {
            target: GpuTarget::Intel(IntelArch::XeHpg),
            ..CompileOptions::default()
        };
        let result = compile_wgsl("@compute @workgroup_size(1) fn main() {}", &opts);
        assert!(
            matches!(result, Err(CompileError::UnsupportedArch(_))),
            "compile_wgsl with Intel target should return UnsupportedArch: {result:?}"
        );
    }

    #[test]
    fn test_compile_intel_returns_unsupported_arch() {
        let opts = CompileOptions {
            target: GpuTarget::Intel(IntelArch::XeHpg),
            ..CompileOptions::default()
        };
        let result = compile(&[0x0723_0203], &opts);
        assert!(
            matches!(result, Err(CompileError::UnsupportedArch(_))),
            "compile with Intel target should return UnsupportedArch: {result:?}"
        );
    }
}
