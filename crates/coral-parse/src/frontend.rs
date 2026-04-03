// SPDX-License-Identifier: AGPL-3.0-only
//! `CoralFrontend` — sovereign implementation of `coral_reef::Frontend`.

use coral_reef::codegen::ir::{Shader, ShaderModel};
use coral_reef::error::CompileError;
use coral_reef::Frontend;

/// Sovereign shader frontend — parses WGSL/SPIR-V/GLSL without naga.
pub struct CoralFrontend;

impl Frontend for CoralFrontend {
    fn compile_wgsl<'a>(
        &self,
        source: &str,
        sm: &'a dyn ShaderModel,
    ) -> Result<Shader<'a>, CompileError> {
        let module = crate::wgsl::parse(source).map_err(|e| {
            CompileError::InvalidInput(format!("WGSL parse error: {e}").into())
        })?;

        let ep_name = module
            .entry_points
            .first()
            .map(|ep| ep.name.clone())
            .ok_or_else(|| CompileError::InvalidInput("no entry points in module".into()))?;

        crate::lower::lower(&module, sm, &ep_name)
    }

    fn compile_spirv<'a>(
        &self,
        spirv: &[u32],
        sm: &'a dyn ShaderModel,
    ) -> Result<Shader<'a>, CompileError> {
        let module = crate::spirv::parse(spirv).map_err(|e| {
            CompileError::InvalidInput(format!("SPIR-V parse error: {e}").into())
        })?;
        let ep_name = module
            .entry_points
            .first()
            .map(|ep| ep.name.clone())
            .ok_or_else(|| CompileError::InvalidInput("no entry points in SPIR-V module".into()))?;
        crate::lower::lower(&module, sm, &ep_name)
    }

    fn compile_glsl<'a>(
        &self,
        source: &str,
        sm: &'a dyn ShaderModel,
    ) -> Result<Shader<'a>, CompileError> {
        let module = crate::glsl::parse(source).map_err(|e| {
            CompileError::InvalidInput(format!("GLSL parse error: {e}").into())
        })?;
        let ep_name = module
            .entry_points
            .first()
            .map(|ep| ep.name.clone())
            .ok_or_else(|| CompileError::InvalidInput("no entry points in GLSL source".into()))?;
        crate::lower::lower(&module, sm, &ep_name)
    }
}
