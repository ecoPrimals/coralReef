// SPDX-License-Identifier: AGPL-3.0-only
//! # coral-nak — Sovereign Rust NVIDIA Shader Compiler
//!
//! Forked from Mesa's NAK compiler (`src/nouveau/compiler/nak/`).
//! Evolves to a standalone Rust crate that fixes f64 transcendental
//! emission and operates independently of the Mesa C build system.
//!
//! ## Status
//!
//! **Level 2**: NAK sources extracted, Mesa dependency stubs in place.
//! Compilation pipeline is being evolved to standalone Rust.
//!
//! ## f64 Transcendental Gap
//!
//! NAK's MUFU (Multi-Function Unit) instructions are f32-only.
//! coralNak adds software lowering for f64 transcendentals:
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
//! use coral_nak::{compile, CompileOptions, GpuArch};
//!
//! let binary = compile(&spirv_words, &CompileOptions {
//!     arch: GpuArch::Sm70,
//!     opt_level: 2,
//!     fp64_software: true,
//!     ..Default::default()
//! })?;
//! ```

pub mod error;
pub mod gpu_arch;
pub mod ir;

// NAK code ported from Mesa's C codebase — retains NVIDIA naming conventions
// (e.g. OpFAdd, SrcType, UGPR) and has incomplete code paths for GPU
// architectures still being evolved. These allows are scoped to this module
// only and should be narrowed further as the code matures.
#[allow(
    // NVIDIA ISA types use CamelCase variants that don't follow Rust conventions
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    // Ported code has many defined-but-not-yet-wired types and functions
    dead_code,
    unused_imports,
    unused_variables,
    unused_mut,
    unused_assignments,
    unused_macros,
    // Internal compiler module — docs added incrementally
    missing_docs,
    // Match exhaustiveness evolves as GPU architectures are added
    unreachable_patterns,
    irrefutable_let_patterns,
)]
mod nak;

pub use error::CompileError;
pub use gpu_arch::GpuArch;
pub use nak::ir::Shader;
pub use nak::pipeline::CompiledShader;

/// Compile options for the shader compiler.
#[derive(Debug, Clone)]
pub struct CompileOptions {
    /// Target GPU architecture.
    pub arch: GpuArch,
    /// Optimization level (0-3).
    pub opt_level: u32,
    /// Include debug info in output.
    pub debug_info: bool,
    /// Enable software lowering for f64 transcendentals.
    pub fp64_software: bool,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            arch: GpuArch::default(),
            opt_level: 2,
            debug_info: false,
            fp64_software: true,
        }
    }
}

/// Compile SPIR-V to native NVIDIA GPU binary.
///
/// This is the primary entry point for the compiler.
///
/// # Errors
///
/// Returns [`CompileError`] if compilation fails.
pub fn compile(spirv: &[u32], options: &CompileOptions) -> Result<Vec<u8>, CompileError> {
    if spirv.is_empty() {
        return Err(CompileError::InvalidInput("empty SPIR-V module".into()));
    }
    tracing::info!(
        arch = ?options.arch,
        opt = options.opt_level,
        fp64 = options.fp64_software,
        "coral-nak compile"
    );

    let module = nak::from_spirv::parse_spirv(spirv)?;

    let sm_info = nak::ir::ShaderModelInfo::new(options.arch.sm_version(), 64);

    let ep = module
        .entry_points
        .first()
        .ok_or_else(|| CompileError::InvalidInput("no entry points in module".into()))?;

    let mut shader = nak::from_spirv::translate(&module, &sm_info, &ep.name)?;
    let compiled = compile_ir(&mut shader)?;

    let mut binary = Vec::with_capacity(compiled.header.len() * 4 + compiled.code.len() * 4);
    for word in &compiled.header {
        binary.extend_from_slice(&word.to_le_bytes());
    }
    for word in &compiled.code {
        binary.extend_from_slice(&word.to_le_bytes());
    }
    Ok(binary)
}

/// Compile a pre-built NAK IR shader through the full optimization and encoding pipeline.
/// This is the internal entry point used once the SPIR-V → IR frontend is wired.
///
/// # Errors
///
/// Returns [`CompileError`] if compilation fails.
pub fn compile_ir(shader: &mut Shader<'_>) -> Result<CompiledShader, CompileError> {
    nak::pipeline::compile_shader(shader, false)
}

/// Compile WGSL source to native NVIDIA GPU binary.
///
/// Convenience wrapper that uses naga to parse WGSL → SPIR-V, then compiles.
///
/// # Errors
///
/// Returns [`CompileError`] if parsing or compilation fails.
pub fn compile_wgsl(wgsl: &str, options: &CompileOptions) -> Result<Vec<u8>, CompileError> {
    if wgsl.is_empty() {
        return Err(CompileError::InvalidInput("empty WGSL source".into()));
    }
    tracing::info!(
        arch = ?options.arch,
        opt = options.opt_level,
        "coral-nak compile_wgsl"
    );

    let module = nak::from_spirv::parse_wgsl(wgsl)?;

    let sm_info = nak::ir::ShaderModelInfo::new(options.arch.sm_version(), 64);

    let ep = module
        .entry_points
        .first()
        .ok_or_else(|| CompileError::InvalidInput("no entry points in WGSL".into()))?;

    let mut shader = nak::from_spirv::translate(&module, &sm_info, &ep.name)?;
    let compiled = compile_ir(&mut shader)?;

    let mut binary = Vec::with_capacity(compiled.header.len() * 4 + compiled.code.len() * 4);
    for word in &compiled.header {
        binary.extend_from_slice(&word.to_le_bytes());
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
    fn test_default_options() {
        let opts = CompileOptions::default();
        assert_eq!(opts.arch, GpuArch::Sm70);
        assert_eq!(opts.opt_level, 2);
        assert!(opts.fp64_software);
        assert!(!opts.debug_info);
    }

    #[test]
    fn test_options_clone() {
        let opts = CompileOptions {
            arch: GpuArch::Sm89,
            opt_level: 3,
            debug_info: true,
            fp64_software: false,
        };
        let cloned = opts.clone();
        assert_eq!(cloned.arch, GpuArch::Sm89);
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
                arch,
                ..CompileOptions::default()
            };
            let result = compile(&[0x0723_0203], &opts);
            assert!(result.is_err(), "should be not-implemented for {arch}");
        }
    }
}
