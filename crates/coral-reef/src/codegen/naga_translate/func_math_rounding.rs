// SPDX-License-Identifier: AGPL-3.0-only
//! Rounding and fractional math operations: floor, ceil, round, trunc, fract.

use super::super::ir::*;
use super::func::FuncTranslator;
use crate::error::CompileError;
use naga::Handle;

pub(super) fn translate(
    ft: &mut FuncTranslator<'_, '_>,
    fun: naga::MathFunction,
    a: &SSARef,
    _b: Option<&SSARef>,
    _c: Option<&SSARef>,
    arg_handle: Handle<naga::Expression>,
) -> Result<Option<SSARef>, CompileError> {
    let result = match fun {
        naga::MathFunction::Floor => Some(translate_floor(ft, a.clone(), arg_handle)?),
        naga::MathFunction::Ceil => Some(translate_ceil(ft, a.clone(), arg_handle)?),
        naga::MathFunction::Round => Some(translate_round(ft, a.clone(), arg_handle)?),
        naga::MathFunction::Trunc => Some(translate_trunc(ft, a.clone(), arg_handle)?),
        naga::MathFunction::Fract => Some(translate_fract(ft, a.clone(), arg_handle)?),
        _ => None,
    };
    Ok(result)
}

fn translate_floor(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
    arg_handle: Handle<naga::Expression>,
) -> Result<SSARef, CompileError> {
    let is_f64 = ft.is_f64_expr(arg_handle);
    if is_f64 {
        ft.emit_f64_floor(a)
    } else {
        let dst = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpFRnd {
            dst: dst.into(),
            src: a[0].into(),
            dst_type: FloatType::F32,
            src_type: FloatType::F32,
            rnd_mode: FRndMode::NegInf,
            ftz: false,
        }));
        Ok(dst.into())
    }
}

fn translate_ceil(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
    arg_handle: Handle<naga::Expression>,
) -> Result<SSARef, CompileError> {
    let is_f64 = ft.is_f64_expr(arg_handle);
    if is_f64 {
        ft.emit_f64_ceil(a)
    } else {
        let dst = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpFRnd {
            dst: dst.into(),
            src: a[0].into(),
            dst_type: FloatType::F32,
            src_type: FloatType::F32,
            rnd_mode: FRndMode::PosInf,
            ftz: false,
        }));
        Ok(dst.into())
    }
}

fn translate_round(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
    arg_handle: Handle<naga::Expression>,
) -> Result<SSARef, CompileError> {
    let is_f64 = ft.is_f64_expr(arg_handle);
    if is_f64 {
        ft.emit_f64_round(a)
    } else {
        let dst = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpFRnd {
            dst: dst.into(),
            src: a[0].into(),
            dst_type: FloatType::F32,
            src_type: FloatType::F32,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
        }));
        Ok(dst.into())
    }
}

fn translate_trunc(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
    arg_handle: Handle<naga::Expression>,
) -> Result<SSARef, CompileError> {
    let is_f64 = ft.is_f64_expr(arg_handle);
    if is_f64 {
        // trunc = copysign(floor(abs(x)), x)
        let abs_x = ft.alloc_ssa_vec(RegFile::GPR, 2);
        ft.push_instr(Instr::new(OpDAdd {
            dst: abs_x.clone().into(),
            srcs: [Src::ZERO, Src::from(a.clone()).fabs()],
            rnd_mode: FRndMode::NearestEven,
        }));
        let floored = ft.emit_f64_floor(abs_x)?;

        let sign_bit = ft.alloc_ssa(RegFile::GPR);
        ft.emit_logic_and(sign_bit, a[1].into(), Src::new_imm_u32(0x8000_0000));

        let cleared = ft.alloc_ssa(RegFile::GPR);
        ft.emit_logic_and(cleared, floored[1].into(), Src::new_imm_u32(0x7FFF_FFFF));

        let dst = ft.alloc_ssa_vec(RegFile::GPR, 2);
        ft.push_instr(Instr::new(OpCopy {
            dst: dst[0].into(),
            src: floored[0].into(),
        }));
        ft.emit_logic_or(dst[1], cleared.into(), sign_bit.into());
        Ok(dst)
    } else {
        let dst = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpFRnd {
            dst: dst.into(),
            src: a[0].into(),
            dst_type: FloatType::F32,
            src_type: FloatType::F32,
            rnd_mode: FRndMode::Zero,
            ftz: false,
        }));
        Ok(dst.into())
    }
}

fn translate_fract(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
    arg_handle: Handle<naga::Expression>,
) -> Result<SSARef, CompileError> {
    let is_f64 = ft.is_f64_expr(arg_handle);
    if is_f64 {
        let floored = ft.emit_f64_floor(a.clone())?;
        let dst = ft.alloc_ssa_vec(RegFile::GPR, 2);
        ft.push_instr(Instr::new(OpDAdd {
            dst: dst.clone().into(),
            srcs: [Src::from(a), Src::from(floored).fneg()],
            rnd_mode: FRndMode::NearestEven,
        }));
        Ok(dst)
    } else {
        let floored = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpFRnd {
            dst: floored.into(),
            src: a[0].into(),
            dst_type: FloatType::F32,
            src_type: FloatType::F32,
            rnd_mode: FRndMode::NegInf,
            ftz: false,
        }));
        let dst = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpFAdd {
            dst: dst.into(),
            srcs: [a[0].into(), Src::from(floored).fneg()],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
        }));
        Ok(dst.into())
    }
}
