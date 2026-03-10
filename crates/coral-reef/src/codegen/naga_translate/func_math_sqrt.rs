// SPDX-License-Identifier: AGPL-3.0-only
//! Square root and inverse square root operations.

#![allow(clippy::wildcard_imports)]
use super::super::ir::*;
use super::func::FuncTranslator;
use crate::error::CompileError;
use naga::Handle;

pub(super) fn translate(
    ft: &mut FuncTranslator<'_, '_>,
    fun: naga::MathFunction,
    a: SSARef,
    _b: Option<SSARef>,
    _c: Option<SSARef>,
    arg_handle: Handle<naga::Expression>,
) -> Result<Option<SSARef>, CompileError> {
    let result = match fun {
        naga::MathFunction::Sqrt => Some(translate_sqrt(ft, a, arg_handle)?),
        naga::MathFunction::InverseSqrt => Some(translate_inverse_sqrt(ft, a, arg_handle)?),
        _ => None,
    };
    Ok(result)
}

fn translate_sqrt(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
    arg_handle: Handle<naga::Expression>,
) -> Result<SSARef, CompileError> {
    let is_f64 = ft.is_f64_expr(arg_handle);
    if is_f64 {
        let dst = ft.alloc_ssa_vec(RegFile::GPR, 2);
        ft.push_instr(Instr::new(OpF64Sqrt {
            dst: dst.clone().into(),
            src: Src::from(a),
        }));
        Ok(dst)
    } else {
        let rsq = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpTranscendental {
            dst: rsq.into(),
            op: TranscendentalOp::Rsq,
            src: a[0].into(),
        }));
        let rcp_rsq = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpTranscendental {
            dst: rcp_rsq.into(),
            op: TranscendentalOp::Rcp,
            src: rsq.into(),
        }));
        Ok(rcp_rsq.into())
    }
}

fn translate_inverse_sqrt(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
    arg_handle: Handle<naga::Expression>,
) -> Result<SSARef, CompileError> {
    let is_f64 = ft.is_f64_expr(arg_handle);
    if is_f64 {
        let dst = ft.alloc_ssa_vec(RegFile::GPR, 2);
        ft.push_instr(Instr::new(OpF64Rcp {
            dst: dst.clone().into(),
            src: Src::from(a),
        }));
        Ok(dst)
    } else {
        let dst = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpTranscendental {
            dst: dst.into(),
            op: TranscendentalOp::Rsq,
            src: a[0].into(),
        }));
        Ok(dst.into())
    }
}
