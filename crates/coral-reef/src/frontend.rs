// SPDX-License-Identifier: AGPL-3.0-or-later
//! Shader source frontend abstraction.
//!
//! The [`Frontend`] trait decouples the compiler core from any specific
//! shader language parser.  The default implementation, [`NagaFrontend`],
//! uses the `naga` crate for WGSL, SPIR-V, and GLSL, but alternative
//! frontends can be plugged in without changing the compilation pipeline.

use crate::codegen::ir::{Shader, ShaderModel};
use crate::error::CompileError;

/// A shader-source frontend that parses input into the compiler's IR.
///
/// Implementors encapsulate both parsing **and** lowering to the
/// internal `Shader` representation.
pub trait Frontend {
    /// Parse WGSL source and lower to the compiler IR.
    ///
    /// # Errors
    ///
    /// Returns [`CompileError::InvalidInput`] if the source is malformed or
    /// contains unsupported constructs.
    fn compile_wgsl<'a>(
        &self,
        source: &str,
        sm: &'a dyn ShaderModel,
    ) -> Result<Shader<'a>, CompileError>;

    /// Parse SPIR-V words and lower to the compiler IR.
    ///
    /// # Errors
    ///
    /// Returns [`CompileError::InvalidInput`] if the SPIR-V module is
    /// invalid or contains unsupported constructs.
    fn compile_spirv<'a>(
        &self,
        spirv: &[u32],
        sm: &'a dyn ShaderModel,
    ) -> Result<Shader<'a>, CompileError>;

    /// Parse GLSL compute shader source and lower to the compiler IR.
    ///
    /// # Errors
    ///
    /// Returns [`CompileError::InvalidInput`] if the source is malformed or
    /// contains unsupported constructs.
    fn compile_glsl<'a>(
        &self,
        source: &str,
        sm: &'a dyn ShaderModel,
    ) -> Result<Shader<'a>, CompileError>;
}

/// Default frontend backed by the `naga` crate (WGSL + SPIR-V + GLSL).
pub struct NagaFrontend;

impl Frontend for NagaFrontend {
    fn compile_wgsl<'a>(
        &self,
        source: &str,
        sm: &'a dyn ShaderModel,
    ) -> Result<Shader<'a>, CompileError> {
        let module = crate::codegen::naga_translate::parse_wgsl(source)?;
        translate_first_entry(&module, sm)
    }

    fn compile_spirv<'a>(
        &self,
        spirv: &[u32],
        sm: &'a dyn ShaderModel,
    ) -> Result<Shader<'a>, CompileError> {
        let module = crate::codegen::naga_translate::parse_spirv(spirv)?;
        translate_first_entry(&module, sm)
    }

    fn compile_glsl<'a>(
        &self,
        source: &str,
        sm: &'a dyn ShaderModel,
    ) -> Result<Shader<'a>, CompileError> {
        let module = crate::codegen::naga_translate::parse_glsl(source)?;
        translate_first_entry(&module, sm)
    }
}

fn translate_first_entry<'sm>(
    module: &naga::Module,
    sm: &'sm dyn ShaderModel,
) -> Result<Shader<'sm>, CompileError> {
    let ep = module
        .entry_points
        .first()
        .ok_or_else(|| CompileError::InvalidInput("no entry points in module".into()))?;
    crate::codegen::naga_translate::translate(module, sm, &ep.name)
}
