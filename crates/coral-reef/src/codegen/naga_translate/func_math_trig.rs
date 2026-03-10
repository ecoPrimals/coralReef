// SPDX-License-Identifier: AGPL-3.0-only
//! Trigonometric math operations: sin, cos, tan, atan, atan2, asin, acos.

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
    _c: Option<SSARef>,
    arg_handle: Handle<naga::Expression>,
) -> Result<Option<SSARef>, CompileError> {
    let result = match fun {
        naga::MathFunction::Sin => Some(ft.emit_f32_trig_scaled(a[0], TranscendentalOp::Sin)?),
        naga::MathFunction::Cos => Some(ft.emit_f32_trig_scaled(a[0], TranscendentalOp::Cos)?),
        naga::MathFunction::Tan => Some(translate_tan(ft, a, arg_handle)?),
        naga::MathFunction::Atan => Some(ft.emit_f32_atan(a[0])?),
        naga::MathFunction::Atan2 => {
            let b = b.ok_or_else(|| CompileError::InvalidInput("atan2 requires 2 args".into()))?;
            Some(ft.emit_f32_atan2(a[0], b[0])?)
        }
        naga::MathFunction::Asin => Some(translate_asin(ft, a)?),
        naga::MathFunction::Acos => Some(translate_acos(ft, a)?),
        _ => None,
    };
    Ok(result)
}

fn translate_tan(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
    arg_handle: Handle<naga::Expression>,
) -> Result<SSARef, CompileError> {
    let is_f64 = ft.is_f64_expr(arg_handle);
    if is_f64 {
        let sin_val = ft.alloc_ssa_vec(RegFile::GPR, 2);
        ft.push_instr(Instr::new(OpF64Sin {
            dst: sin_val.clone().into(),
            src: Src::from(a.clone()),
        }));
        let cos_val = ft.alloc_ssa_vec(RegFile::GPR, 2);
        ft.push_instr(Instr::new(OpF64Cos {
            dst: cos_val.clone().into(),
            src: Src::from(a),
        }));
        let rcp_cos = ft.alloc_ssa_vec(RegFile::GPR, 2);
        ft.push_instr(Instr::new(OpF64Rcp {
            dst: rcp_cos.clone().into(),
            src: Src::from(cos_val),
        }));
        let dst = ft.alloc_ssa_vec(RegFile::GPR, 2);
        ft.push_instr(Instr::new(OpDMul {
            dst: dst.clone().into(),
            srcs: [Src::from(sin_val), Src::from(rcp_cos)],
            rnd_mode: FRndMode::NearestEven,
        }));
        Ok(dst)
    } else {
        let frac_1_2pi = 1.0 / (2.0 * std::f32::consts::PI);
        let scaled = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpFMul {
            dst: scaled.into(),
            srcs: [a[0].into(), Src::new_imm_u32(frac_1_2pi.to_bits())],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            dnz: false,
        }));
        let sin_val = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpTranscendental {
            dst: sin_val.into(),
            op: TranscendentalOp::Sin,
            src: scaled.into(),
        }));
        let cos_val = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpTranscendental {
            dst: cos_val.into(),
            op: TranscendentalOp::Cos,
            src: scaled.into(),
        }));
        let rcp_cos = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpTranscendental {
            dst: rcp_cos.into(),
            op: TranscendentalOp::Rcp,
            src: cos_val.into(),
        }));
        let dst = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpFMul {
            dst: dst.into(),
            srcs: [sin_val.into(), rcp_cos.into()],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            dnz: false,
        }));
        Ok(dst.into())
    }
}

/// asin(x) = atan2(x, sqrt(1 - x*x))
fn translate_asin(ft: &mut FuncTranslator<'_, '_>, a: SSARef) -> Result<SSARef, CompileError> {
    let x2 = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFMul {
        dst: x2.into(),
        srcs: [a[0].into(), a[0].into()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    let one: f32 = 1.0;
    let one_minus_x2 = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFAdd {
        dst: one_minus_x2.into(),
        srcs: [Src::new_imm_u32(one.to_bits()), Src::from(x2).fneg()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
    }));
    let rsq = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpTranscendental {
        dst: rsq.into(),
        op: TranscendentalOp::Rsq,
        src: one_minus_x2.into(),
    }));
    let sqrt_val = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpTranscendental {
        dst: sqrt_val.into(),
        op: TranscendentalOp::Rcp,
        src: rsq.into(),
    }));
    ft.emit_f32_atan2(a[0], sqrt_val)
}

/// acos(x) = atan2(sqrt(1 - x*x), x)
fn translate_acos(ft: &mut FuncTranslator<'_, '_>, a: SSARef) -> Result<SSARef, CompileError> {
    let x2 = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFMul {
        dst: x2.into(),
        srcs: [a[0].into(), a[0].into()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    let one: f32 = 1.0;
    let one_minus_x2 = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFAdd {
        dst: one_minus_x2.into(),
        srcs: [Src::new_imm_u32(one.to_bits()), Src::from(x2).fneg()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
    }));
    let rsq = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpTranscendental {
        dst: rsq.into(),
        op: TranscendentalOp::Rsq,
        src: one_minus_x2.into(),
    }));
    let sqrt_val = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpTranscendental {
        dst: sqrt_val.into(),
        op: TranscendentalOp::Rcp,
        src: rsq.into(),
    }));
    ft.emit_f32_atan2(sqrt_val, a[0])
}
