// SPDX-License-Identifier: AGPL-3.0-only
//! Naga IR → codegen IR translator.
//!
//! Translates a `naga::Module` (parsed from SPIR-V or WGSL) into the
//! internal SSA-based IR (`Shader`), which then flows through the
//! optimization / legalization / RA / encoding pipeline.

#![allow(clippy::wildcard_imports)]
use super::ir::*;
use crate::FmaPolicy;
use crate::error::CompileError;

pub(super) mod expr;
mod expr_binary;
pub(super) mod func;
mod func_builtins;
mod func_control;
mod func_math;
mod func_math_bitops;
mod func_math_exp_log;
mod func_math_extrema;
mod func_math_helpers;
mod func_math_interp;
mod func_math_rounding;
mod func_math_sqrt;
mod func_math_trig;
mod func_math_vector;
mod func_mem;
mod func_ops;

fn mem_access_global_b32() -> MemAccess {
    MemAccess {
        mem_type: MemType::B32,
        space: MemSpace::Global(MemAddrType::A64),
        order: MemOrder::Weak,
        eviction_priority: MemEvictionPriority::Normal,
    }
}

fn lit_scalar(lit: &naga::Literal) -> naga::Scalar {
    match lit {
        naga::Literal::F32(_) => naga::Scalar {
            kind: naga::ScalarKind::Float,
            width: 4,
        },
        naga::Literal::F64(_) => naga::Scalar {
            kind: naga::ScalarKind::Float,
            width: 8,
        },
        naga::Literal::U32(_) => naga::Scalar {
            kind: naga::ScalarKind::Uint,
            width: 4,
        },
        naga::Literal::I32(_) => naga::Scalar {
            kind: naga::ScalarKind::Sint,
            width: 4,
        },
        naga::Literal::U64(_) => naga::Scalar {
            kind: naga::ScalarKind::Uint,
            width: 8,
        },
        naga::Literal::I64(_) => naga::Scalar {
            kind: naga::ScalarKind::Sint,
            width: 8,
        },
        naga::Literal::Bool(_) => naga::Scalar {
            kind: naga::ScalarKind::Bool,
            width: 1,
        },
        _ => naga::Scalar {
            kind: naga::ScalarKind::Uint,
            width: 4,
        },
    }
}

mod sys_regs {
    pub const SR_TID_X: u8 = 0x21;
    pub const SR_TID_Y: u8 = 0x22;
    pub const SR_TID_Z: u8 = 0x23;
    pub const SR_CTAID_X: u8 = 0x25;
    pub const SR_CTAID_Y: u8 = 0x26;
    pub const SR_CTAID_Z: u8 = 0x27;
    pub const SR_NTID_X: u8 = 0x29;
    pub const SR_NTID_Y: u8 = 0x2a;
    pub const SR_NTID_Z: u8 = 0x2b;
    pub const SR_NCTAID_X: u8 = 0x2d;
    pub const SR_NCTAID_Y: u8 = 0x2e;
    pub const SR_NCTAID_Z: u8 = 0x2f;
    pub const SR_LANEID: u8 = 0x00;
}

/// Top-level translator state.
pub struct NagaTranslator<'sm, 'mod_lt> {
    sm: &'sm dyn ShaderModel,
    module: &'mod_lt naga::Module,
}

impl<'sm, 'mod_lt> NagaTranslator<'sm, 'mod_lt> {
    pub fn new(sm: &'sm dyn ShaderModel, module: &'mod_lt naga::Module) -> Self {
        Self { sm, module }
    }

    /// Translate a compute shader entry point into a `Shader`.
    pub fn translate_compute(
        &self,
        entry_point: &naga::EntryPoint,
    ) -> Result<Shader<'sm>, CompileError> {
        if entry_point.stage != naga::ShaderStage::Compute {
            return Err(CompileError::InvalidInput(
                format!("expected compute stage, got {:?}", entry_point.stage,).into(),
            ));
        }

        let function = self.translate_function(&entry_point.function, Some(entry_point))?;

        let local_size = [
            entry_point.workgroup_size[0] as u16,
            entry_point.workgroup_size[1] as u16,
            entry_point.workgroup_size[2] as u16,
        ];
        let shared_mem_size = self.compute_shared_mem_size();

        let info = ShaderInfo {
            max_warps_per_sm: 0,
            gpr_count: 0,
            control_barrier_count: 0,
            instr_count: 0,
            static_cycle_count: 0,
            spills_to_mem: 0,
            fills_from_mem: 0,
            spills_to_reg: 0,
            fills_from_reg: 0,
            shared_local_mem_size: 0,
            max_crs_depth: 0,
            uses_global_mem: true,
            writes_global_mem: true,
            uses_fp64: false,
            stage: ShaderStageInfo::Compute(ComputeShaderInfo {
                local_size,
                shared_mem_size,
            }),
            io: ShaderIoInfo::None,
        };

        Ok(Shader {
            sm: self.sm,
            info,
            functions: vec![function],
            fma_policy: FmaPolicy::default(),
        })
    }

    fn compute_shared_mem_size(&self) -> u16 {
        let mut total = 0u32;
        for (_, gv) in self.module.global_variables.iter() {
            if gv.space == naga::AddressSpace::WorkGroup {
                let ty = &self.module.types[gv.ty];
                total += ty.inner.size(self.module.to_ctx());
            }
        }
        total.min(u32::from(u16::MAX)) as u16
    }

    fn translate_function(
        &self,
        func: &naga::Function,
        entry_point: Option<&naga::EntryPoint>,
    ) -> Result<Function, CompileError> {
        let mut ft = func::FuncTranslator::new(self.sm, self.module, func);

        ft.start_block();
        ft.pre_allocate_local_vars();

        if let Some(ep) = entry_point {
            if ep.stage == naga::ShaderStage::Compute {
                ft.emit_compute_prologue(ep)?;
            }
        }

        ft.translate_block(&func.body)?;

        ft.push_instr(Instr::new(OpExit {}));
        ft.finish_block()?;

        Ok(ft.build_function())
    }
}

/// Parse SPIR-V bytes into a naga Module.
pub fn parse_spirv(data: &[u32]) -> Result<naga::Module, CompileError> {
    let bytes: Vec<u8> = data.iter().flat_map(|w| w.to_le_bytes()).collect();
    let opts = naga::front::spv::Options::default();
    naga::front::spv::parse_u8_slice(&bytes, &opts)
        .map_err(|e| CompileError::InvalidInput(format!("SPIR-V parse error: {e}").into()))
}

/// Parse WGSL source into a naga Module.
pub fn parse_wgsl(source: &str) -> Result<naga::Module, CompileError> {
    naga::front::wgsl::parse_str(source)
        .map_err(|e| CompileError::InvalidInput(format!("WGSL parse error: {e}").into()))
}

/// Parse GLSL compute shader source into a naga Module.
pub fn parse_glsl(source: &str) -> Result<naga::Module, CompileError> {
    let opts = naga::front::glsl::Options::from(naga::ShaderStage::Compute);
    let mut frontend = naga::front::glsl::Frontend::default();
    frontend
        .parse(&opts, source)
        .map_err(|e| CompileError::InvalidInput(format!("GLSL parse error: {e:?}").into()))
}

/// Translate a naga Module into a Shader for a compute entry point.
pub fn translate<'sm>(
    module: &naga::Module,
    sm: &'sm dyn ShaderModel,
    entry_point_name: &str,
) -> Result<Shader<'sm>, CompileError> {
    let ep = module
        .entry_points
        .iter()
        .find(|ep| ep.name == entry_point_name)
        .ok_or_else(|| {
            CompileError::InvalidInput(
                format!("entry point '{entry_point_name}' not found",).into(),
            )
        })?;

    let translator = NagaTranslator::new(sm, module);
    translator.translate_compute(ep)
}

#[cfg(test)]
mod tests_interpolation_builtins;
#[cfg(test)]
mod tests_math_coverage;
#[cfg(test)]
mod tests_parse_translate;
