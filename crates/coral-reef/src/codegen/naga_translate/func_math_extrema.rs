// SPDX-License-Identifier: AGPL-3.0-only
//! Min/max/clamp and absolute value operations.

#![allow(clippy::wildcard_imports)]
use super::super::ir::*;
use super::func::FuncTranslator;
use crate::error::CompileError;
use naga::Handle;

pub(super) fn translate(
    ft: &mut FuncTranslator<'_, '_>,
    fun: naga::MathFunction,
    a: SSARef,
    b: Option<SSARef>,
    c: Option<SSARef>,
    arg_handle: Handle<naga::Expression>,
) -> Result<Option<SSARef>, CompileError> {
    let result = match fun {
        naga::MathFunction::Abs => Some(translate_abs(ft, a, arg_handle)?),
        naga::MathFunction::Min => {
            let b = b.ok_or_else(|| CompileError::InvalidInput("min requires 2 args".into()))?;
            Some(translate_min(ft, a, b, arg_handle)?)
        }
        naga::MathFunction::Max => {
            let b = b.ok_or_else(|| CompileError::InvalidInput("max requires 2 args".into()))?;
            Some(translate_max(ft, a, b, arg_handle)?)
        }
        naga::MathFunction::Clamp => {
            let b = b.ok_or_else(|| CompileError::InvalidInput("clamp requires 3 args".into()))?;
            let c = c.ok_or_else(|| CompileError::InvalidInput("clamp requires 3 args".into()))?;
            Some(translate_clamp(ft, a, b, c, arg_handle)?)
        }
        _ => None,
    };
    Ok(result)
}

fn translate_abs(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
    arg_handle: Handle<naga::Expression>,
) -> Result<SSARef, CompileError> {
    let is_f64 = ft.is_f64_expr(arg_handle);
    if is_f64 {
        let a = ft.ensure_f64_ssa_ref(a);
        let n_pairs = a.comps() / 2;
        let dst = ft.alloc_ssa_vec(RegFile::GPR, n_pairs * 2);
        for i in 0..(n_pairs as usize) {
            let idx = i;
            ft.push_instr(Instr::new(OpCopy {
                dst: dst[idx * 2].into(),
                src: a[idx * 2].into(),
            }));
            ft.emit_logic_and(
                dst[idx * 2 + 1],
                a[idx * 2 + 1].into(),
                Src::new_imm_u32(0x7FFF_FFFF),
            );
        }
        Ok(dst)
    } else {
        let dst = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpFAdd {
            dst: dst.into(),
            srcs: [Src::ZERO, Src::from(a[0]).fabs()],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
        }));
        Ok(dst.into())
    }
}

fn translate_min(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
    b: SSARef,
    arg_handle: Handle<naga::Expression>,
) -> Result<SSARef, CompileError> {
    let is_f64 = ft.is_f64_expr(arg_handle);
    if is_f64 {
        ft.emit_f64_min_max(a, b, FloatCmpOp::OrdLt)
    } else {
        let dst = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpFMnMx {
            dst: dst.into(),
            srcs: [a[0].into(), b[0].into()],
            min: SrcRef::True.into(),
            ftz: false,
        }));
        Ok(dst.into())
    }
}

fn translate_max(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
    b: SSARef,
    arg_handle: Handle<naga::Expression>,
) -> Result<SSARef, CompileError> {
    let is_f64 = ft.is_f64_expr(arg_handle);
    if is_f64 {
        ft.emit_f64_min_max(a, b, FloatCmpOp::OrdGt)
    } else {
        let dst = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpFMnMx {
            dst: dst.into(),
            srcs: [a[0].into(), b[0].into()],
            min: SrcRef::False.into(),
            ftz: false,
        }));
        Ok(dst.into())
    }
}

fn translate_clamp(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
    b: SSARef,
    c: SSARef,
    arg_handle: Handle<naga::Expression>,
) -> Result<SSARef, CompileError> {
    let is_f64 = ft.is_f64_expr(arg_handle);
    if is_f64 {
        let tmp = ft.emit_f64_min_max(a, c, FloatCmpOp::OrdLt)?;
        ft.emit_f64_min_max(tmp, b, FloatCmpOp::OrdGt)
    } else {
        let tmp = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpFMnMx {
            dst: tmp.into(),
            srcs: [a[0].into(), c[0].into()],
            min: SrcRef::True.into(),
            ftz: false,
        }));
        let dst = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpFMnMx {
            dst: dst.into(),
            srcs: [tmp.into(), b[0].into()],
            min: SrcRef::False.into(),
            ftz: false,
        }));
        Ok(dst.into())
    }
}
