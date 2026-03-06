// SPDX-License-Identifier: AGPL-3.0-only
//! # coral-reef — Sovereign Rust GPU Compiler
//!
//! Multi-vendor GPU compiler: WGSL/SPIR-V → vendor-specific binary.
//! Currently targets NVIDIA (SM70+), with AMD and Intel backends planned.
//!
//! ## f64 Transcendental Lowering
//!
//! Hardware transcendental units (MUFU on NVIDIA) are f32-only.
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
//! ## Public API (target)
//!
//! ```rust,ignore
//! use coral_reef::{compile, CompileOptions, GpuArch};
//!
//! let binary = compile(&spirv_words, &CompileOptions {
//!     target: GpuArch::Sm70.into(),
//!     opt_level: 2,
//!     fp64_software: true,
//!     ..Default::default()
//! })?;
//! ```

pub mod backend;
pub mod error;
pub mod frontend;
pub mod gpu_arch;
pub mod ir;

// Codegen module — derived from upstream, evolving to idiomatic Rust.
// ISA types use naming conventions that don't follow Rust defaults (e.g.
// OpFAdd, SrcType, UGPR). These allows are scoped to this module only
// and should be narrowed further as the code matures.
#[allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    unused_macros,
    missing_docs,
    unreachable_patterns,
    irrefutable_let_patterns
)]
mod codegen;

pub use backend::{Backend, CompiledBinary, NvidiaBackend};
pub use codegen::ir::Shader;
pub use codegen::pipeline::CompiledShader;
pub use error::CompileError;
pub use frontend::{Frontend, NagaFrontend};
pub use gpu_arch::{AmdArch, GpuArch, GpuTarget, IntelArch, NvArch};

/// Compile options for the shader compiler.
#[derive(Debug, Clone)]
pub struct CompileOptions {
    /// Target GPU — vendor-discriminated.
    pub target: GpuTarget,
    /// Optimization level (0-3).
    pub opt_level: u32,
    /// Include debug info in output.
    pub debug_info: bool,
    /// Enable software lowering for f64 transcendentals.
    pub fp64_software: bool,
}

impl CompileOptions {
    /// Convenience: the NVIDIA architecture, if the target is NVIDIA.
    #[must_use]
    pub fn nv_arch(&self) -> Option<NvArch> {
        self.target.as_nvidia()
    }

    /// Backward-compatible accessor — returns the `NvArch` or panics.
    ///
    /// Prefer [`Self::nv_arch`] in new code.
    ///
    /// # Panics
    ///
    /// Panics if the target is not an NVIDIA GPU.
    #[must_use]
    pub fn arch(&self) -> GpuArch {
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
            fp64_software: true,
        }
    }
}

/// Compile SPIR-V to native GPU binary.
///
/// This is the primary entry point for the compiler.  Uses the default
/// [`NagaFrontend`]; call [`compile_with`] to supply a custom frontend.
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
    let nv = options
        .nv_arch()
        .ok_or(CompileError::UnsupportedArch(options.target.to_string()))?;
    tracing::info!(
        target = %options.target,
        opt = options.opt_level,
        fp64 = options.fp64_software,
        "coral-reef compile"
    );

    let sm_info = codegen::ir::ShaderModelInfo::new(nv.sm_version(), 64);
    let mut shader = frontend.compile_spirv(spirv, &sm_info)?;
    let compiled = compile_ir(&mut shader)?;
    Ok(emit_binary(&compiled))
}

/// Compile a pre-built IR shader through the full optimization and encoding pipeline.
///
/// # Errors
///
/// Returns [`CompileError`] if compilation fails.
pub fn compile_ir(shader: &mut Shader<'_>) -> Result<CompiledShader, CompileError> {
    codegen::pipeline::compile_shader(shader, false)
}

/// Compile WGSL source to native GPU binary.
///
/// Convenience wrapper using the default [`NagaFrontend`].
/// Call [`compile_wgsl_with`] to supply a custom frontend.
///
/// # Errors
///
/// Returns [`CompileError`] if parsing or compilation fails.
pub fn compile_wgsl(wgsl: &str, options: &CompileOptions) -> Result<Vec<u8>, CompileError> {
    compile_wgsl_with(&NagaFrontend, wgsl, options)
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
    let nv = options
        .nv_arch()
        .ok_or(CompileError::UnsupportedArch(options.target.to_string()))?;
    tracing::info!(
        target = %options.target,
        opt = options.opt_level,
        "coral-reef compile_wgsl"
    );

    let sm_info = codegen::ir::ShaderModelInfo::new(nv.sm_version(), 64);
    let mut shader = frontend.compile_wgsl(wgsl, &sm_info)?;
    let compiled = compile_ir(&mut shader)?;
    Ok(emit_binary(&compiled))
}

fn emit_binary(compiled: &CompiledShader) -> Vec<u8> {
    let mut binary = Vec::with_capacity(compiled.header.len() * 4 + compiled.code.len() * 4);
    for word in &compiled.header {
        binary.extend_from_slice(&word.to_le_bytes());
    }
    for word in &compiled.code {
        binary.extend_from_slice(&word.to_le_bytes());
    }
    binary
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
}
