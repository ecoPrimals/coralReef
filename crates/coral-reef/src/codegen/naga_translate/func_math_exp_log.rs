// SPDX-License-Identifier: AGPL-3.0-or-later
//! Exponential and logarithmic math operations: exp, exp2, log, log2, pow, sinh, cosh, tanh,
//! asinh, acosh, atanh.

use super::super::ir::*;
use super::func::FuncTranslator;
use crate::error::CompileError;
use naga::Handle;

pub(super) fn translate(
    ft: &mut FuncTranslator<'_, '_>,
    fun: naga::MathFunction,
    a: &SSARef,
    b: Option<&SSARef>,
    _c: Option<&SSARef>,
    arg_handle: Handle<naga::Expression>,
) -> Result<Option<SSARef>, CompileError> {
    let result = match fun {
        naga::MathFunction::Exp2 => Some(translate_exp2(ft, a.clone(), arg_handle)?),
        naga::MathFunction::Log2 => Some(translate_log2(ft, a.clone(), arg_handle)?),
        naga::MathFunction::Exp => Some(translate_exp(ft, a.clone(), arg_handle)?),
        naga::MathFunction::Log => Some(translate_log(ft, a.clone(), arg_handle)?),
        naga::MathFunction::Pow => {
            let b =
                b.ok_or_else(|| CompileError::InvalidInput("Pow requires two arguments".into()))?;
            Some(translate_pow(ft, a.clone(), b.clone(), arg_handle)?)
        }
        naga::MathFunction::Tanh => Some(translate_tanh(ft, a.clone(), arg_handle)?),
        naga::MathFunction::Asinh => Some(translate_asinh(ft, a.clone())?),
        naga::MathFunction::Acosh => Some(translate_acosh(ft, a.clone())?),
        naga::MathFunction::Atanh => Some(translate_atanh(ft, a.clone())?),
        naga::MathFunction::Sinh => Some(translate_sinh(ft, a.clone())?),
        naga::MathFunction::Cosh => Some(translate_cosh(ft, a.clone())?),
        _ => None,
    };
    Ok(result)
}

fn translate_exp2(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
    arg_handle: Handle<naga::Expression>,
) -> Result<SSARef, CompileError> {
    let is_f64 = ft.is_f64_expr(arg_handle);
    if is_f64 {
        let dst = ft.alloc_ssa_vec(RegFile::GPR, 2);
        ft.push_instr(Instr::new(OpF64Exp2 {
            dst: dst.clone().into(),
            src: Src::from(a),
        }));
        Ok(dst)
    } else {
        let dst = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpTranscendental {
            dst: dst.into(),
            op: TranscendentalOp::Exp2,
            src: a[0].into(),
        }));
        Ok(dst.into())
    }
}

fn translate_log2(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
    arg_handle: Handle<naga::Expression>,
) -> Result<SSARef, CompileError> {
    let is_f64 = ft.is_f64_expr(arg_handle);
    if is_f64 {
        let dst = ft.alloc_ssa_vec(RegFile::GPR, 2);
        ft.push_instr(Instr::new(OpF64Log2 {
            dst: dst.clone().into(),
            src: Src::from(a),
        }));
        Ok(dst)
    } else {
        let dst = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpTranscendental {
            dst: dst.into(),
            op: TranscendentalOp::Log2,
            src: a[0].into(),
        }));
        Ok(dst.into())
    }
}

fn translate_exp(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
    arg_handle: Handle<naga::Expression>,
) -> Result<SSARef, CompileError> {
    let is_f64 = ft.is_f64_expr(arg_handle);
    if is_f64 {
        // exp(x) = exp2(x * log2(e))
        let log2_e = std::f64::consts::LOG2_E;
        let log2_e_bits = log2_e.to_bits();
        let lo = (log2_e_bits & 0xFFFF_FFFF) as u32;
        let hi = ((log2_e_bits >> 32) & 0xFFFF_FFFF) as u32;
        let scale = ft.alloc_ssa_vec(RegFile::GPR, 2);
        ft.push_instr(Instr::new(OpCopy {
            dst: scale[0].into(),
            src: Src::new_imm_u32(lo),
        }));
        ft.push_instr(Instr::new(OpCopy {
            dst: scale[1].into(),
            src: Src::new_imm_u32(hi),
        }));
        let scaled = ft.alloc_ssa_vec(RegFile::GPR, 2);
        ft.push_instr(Instr::new(OpDMul {
            dst: scaled.clone().into(),
            srcs: [Src::from(a), Src::from(scale)],
            rnd_mode: FRndMode::NearestEven,
        }));
        let dst = ft.alloc_ssa_vec(RegFile::GPR, 2);
        ft.push_instr(Instr::new(OpF64Exp2 {
            dst: dst.clone().into(),
            src: Src::from(scaled),
        }));
        Ok(dst)
    } else {
        let log2_e: f32 = std::f32::consts::LOG2_E;
        let scaled = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpFMul {
            dst: scaled.into(),
            srcs: [a[0].into(), Src::new_imm_u32(log2_e.to_bits())],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            dnz: false,
        }));
        let dst = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpTranscendental {
            dst: dst.into(),
            op: TranscendentalOp::Exp2,
            src: scaled.into(),
        }));
        Ok(dst.into())
    }
}

fn translate_log(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
    arg_handle: Handle<naga::Expression>,
) -> Result<SSARef, CompileError> {
    let is_f64 = ft.is_f64_expr(arg_handle);
    if is_f64 {
        // ln(x) = log2(x) * ln(2)
        let ln2 = std::f64::consts::LN_2;
        let ln2_bits = ln2.to_bits();
        let lo = (ln2_bits & 0xFFFF_FFFF) as u32;
        let hi = ((ln2_bits >> 32) & 0xFFFF_FFFF) as u32;
        let scale = ft.alloc_ssa_vec(RegFile::GPR, 2);
        ft.push_instr(Instr::new(OpCopy {
            dst: scale[0].into(),
            src: Src::new_imm_u32(lo),
        }));
        ft.push_instr(Instr::new(OpCopy {
            dst: scale[1].into(),
            src: Src::new_imm_u32(hi),
        }));
        let log2_val = ft.alloc_ssa_vec(RegFile::GPR, 2);
        ft.push_instr(Instr::new(OpF64Log2 {
            dst: log2_val.clone().into(),
            src: Src::from(a),
        }));
        let dst = ft.alloc_ssa_vec(RegFile::GPR, 2);
        ft.push_instr(Instr::new(OpDMul {
            dst: dst.clone().into(),
            srcs: [Src::from(log2_val), Src::from(scale)],
            rnd_mode: FRndMode::NearestEven,
        }));
        Ok(dst)
    } else {
        let log2_val = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpTranscendental {
            dst: log2_val.into(),
            op: TranscendentalOp::Log2,
            src: a[0].into(),
        }));
        let ln2: f32 = std::f32::consts::LN_2;
        let dst = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpFMul {
            dst: dst.into(),
            srcs: [log2_val.into(), Src::new_imm_u32(ln2.to_bits())],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            dnz: false,
        }));
        Ok(dst.into())
    }
}

fn translate_pow(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
    b: SSARef,
    arg_handle: Handle<naga::Expression>,
) -> Result<SSARef, CompileError> {
    let is_f64 = ft.is_f64_expr(arg_handle);
    if is_f64 {
        let log_x = ft.alloc_ssa_vec(RegFile::GPR, 2);
        ft.push_instr(Instr::new(OpF64Log2 {
            dst: log_x.clone().into(),
            src: Src::from(a),
        }));
        let y_log_x = ft.alloc_ssa_vec(RegFile::GPR, 2);
        ft.push_instr(Instr::new(OpDMul {
            dst: y_log_x.clone().into(),
            srcs: [Src::from(b), Src::from(log_x)],
            rnd_mode: FRndMode::NearestEven,
        }));
        let dst = ft.alloc_ssa_vec(RegFile::GPR, 2);
        ft.push_instr(Instr::new(OpF64Exp2 {
            dst: dst.clone().into(),
            src: Src::from(y_log_x),
        }));
        Ok(dst)
    } else {
        let log_x = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpTranscendental {
            dst: log_x.into(),
            op: TranscendentalOp::Log2,
            src: a[0].into(),
        }));
        let y_log_x = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpFMul {
            dst: y_log_x.into(),
            srcs: [b[0].into(), log_x.into()],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            dnz: false,
        }));
        let dst = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpTranscendental {
            dst: dst.into(),
            op: TranscendentalOp::Exp2,
            src: y_log_x.into(),
        }));
        Ok(dst.into())
    }
}

fn translate_tanh(
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
        let tan_val = ft.alloc_ssa_vec(RegFile::GPR, 2);
        ft.push_instr(Instr::new(OpDMul {
            dst: tan_val.clone().into(),
            srcs: [Src::from(sin_val), Src::from(rcp_cos)],
            rnd_mode: FRndMode::NearestEven,
        }));
        Ok(tan_val)
    } else {
        // tanh(x) = (exp(2x) - 1) / (exp(2x) + 1)
        let two_x = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpFAdd {
            dst: two_x.into(),
            srcs: [a[0].into(), a[0].into()],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
        }));
        let log2_e: f32 = std::f32::consts::LOG2_E;
        let scaled = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpFMul {
            dst: scaled.into(),
            srcs: [two_x.into(), Src::new_imm_u32(log2_e.to_bits())],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            dnz: false,
        }));
        let exp2x = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpTranscendental {
            dst: exp2x.into(),
            op: TranscendentalOp::Exp2,
            src: scaled.into(),
        }));
        let num = ft.alloc_ssa(RegFile::GPR);
        let neg_one: f32 = -1.0;
        ft.push_instr(Instr::new(OpFAdd {
            dst: num.into(),
            srcs: [exp2x.into(), Src::new_imm_u32(neg_one.to_bits())],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
        }));
        let den = ft.alloc_ssa(RegFile::GPR);
        let one: f32 = 1.0;
        ft.push_instr(Instr::new(OpFAdd {
            dst: den.into(),
            srcs: [exp2x.into(), Src::new_imm_u32(one.to_bits())],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
        }));
        let rcp_den = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpTranscendental {
            dst: rcp_den.into(),
            op: TranscendentalOp::Rcp,
            src: den.into(),
        }));
        let dst = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpFMul {
            dst: dst.into(),
            srcs: [num.into(), rcp_den.into()],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            dnz: false,
        }));
        Ok(dst.into())
    }
}

fn translate_asinh(ft: &mut FuncTranslator<'_, '_>, a: SSARef) -> Result<SSARef, CompileError> {
    // asinh(x) = ln(x + sqrt(x*x + 1)) = log2(x + sqrt(x*x + 1)) * ln(2)
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
    let x2p1 = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFAdd {
        dst: x2p1.into(),
        srcs: [x2.into(), Src::new_imm_u32(one.to_bits())],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
    }));
    let rsq = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpTranscendental {
        dst: rsq.into(),
        op: TranscendentalOp::Rsq,
        src: x2p1.into(),
    }));
    let sq = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpTranscendental {
        dst: sq.into(),
        op: TranscendentalOp::Rcp,
        src: rsq.into(),
    }));
    let sum = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFAdd {
        dst: sum.into(),
        srcs: [a[0].into(), sq.into()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
    }));
    let log2_val = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpTranscendental {
        dst: log2_val.into(),
        op: TranscendentalOp::Log2,
        src: sum.into(),
    }));
    let ln2: f32 = std::f32::consts::LN_2;
    let dst = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFMul {
        dst: dst.into(),
        srcs: [log2_val.into(), Src::new_imm_u32(ln2.to_bits())],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    Ok(dst.into())
}

fn translate_acosh(ft: &mut FuncTranslator<'_, '_>, a: SSARef) -> Result<SSARef, CompileError> {
    // acosh(x) = ln(x + sqrt(x*x - 1))
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
    let x2m1 = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFAdd {
        dst: x2m1.into(),
        srcs: [x2.into(), Src::new_imm_u32((-one).to_bits())],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
    }));
    let rsq = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpTranscendental {
        dst: rsq.into(),
        op: TranscendentalOp::Rsq,
        src: x2m1.into(),
    }));
    let sq = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpTranscendental {
        dst: sq.into(),
        op: TranscendentalOp::Rcp,
        src: rsq.into(),
    }));
    let sum = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFAdd {
        dst: sum.into(),
        srcs: [a[0].into(), sq.into()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
    }));
    let log2_val = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpTranscendental {
        dst: log2_val.into(),
        op: TranscendentalOp::Log2,
        src: sum.into(),
    }));
    let ln2: f32 = std::f32::consts::LN_2;
    let dst = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFMul {
        dst: dst.into(),
        srcs: [log2_val.into(), Src::new_imm_u32(ln2.to_bits())],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    Ok(dst.into())
}

fn translate_atanh(ft: &mut FuncTranslator<'_, '_>, a: SSARef) -> Result<SSARef, CompileError> {
    // atanh(x) = 0.5 * ln((1+x)/(1-x))
    let one: f32 = 1.0;
    let half: f32 = 0.5;
    let one_plus_x = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFAdd {
        dst: one_plus_x.into(),
        srcs: [Src::new_imm_u32(one.to_bits()), a[0].into()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
    }));
    let one_minus_x = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFAdd {
        dst: one_minus_x.into(),
        srcs: [Src::new_imm_u32(one.to_bits()), Src::from(a[0]).fneg()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
    }));
    let rcp_denom = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpTranscendental {
        dst: rcp_denom.into(),
        op: TranscendentalOp::Rcp,
        src: one_minus_x.into(),
    }));
    let ratio = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFMul {
        dst: ratio.into(),
        srcs: [one_plus_x.into(), rcp_denom.into()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    let log2_val = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpTranscendental {
        dst: log2_val.into(),
        op: TranscendentalOp::Log2,
        src: ratio.into(),
    }));
    let ln2: f32 = std::f32::consts::LN_2;
    let ln_val = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFMul {
        dst: ln_val.into(),
        srcs: [log2_val.into(), Src::new_imm_u32(ln2.to_bits())],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    let dst = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFMul {
        dst: dst.into(),
        srcs: [ln_val.into(), Src::new_imm_u32(half.to_bits())],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    Ok(dst.into())
}

fn translate_sinh(ft: &mut FuncTranslator<'_, '_>, a: SSARef) -> Result<SSARef, CompileError> {
    // sinh(x) = (exp(x) - exp(-x)) / 2
    let log2_e: f32 = std::f32::consts::LOG2_E;
    let half: f32 = 0.5;
    let s1 = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFMul {
        dst: s1.into(),
        srcs: [a[0].into(), Src::new_imm_u32(log2_e.to_bits())],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    let exp_pos = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpTranscendental {
        dst: exp_pos.into(),
        op: TranscendentalOp::Exp2,
        src: s1.into(),
    }));
    let s2 = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFMul {
        dst: s2.into(),
        srcs: [Src::from(a[0]).fneg(), Src::new_imm_u32(log2_e.to_bits())],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    let exp_neg = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpTranscendental {
        dst: exp_neg.into(),
        op: TranscendentalOp::Exp2,
        src: s2.into(),
    }));
    let diff = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFAdd {
        dst: diff.into(),
        srcs: [exp_pos.into(), Src::from(exp_neg).fneg()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
    }));
    let dst = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFMul {
        dst: dst.into(),
        srcs: [diff.into(), Src::new_imm_u32(half.to_bits())],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    Ok(dst.into())
}

fn translate_cosh(ft: &mut FuncTranslator<'_, '_>, a: SSARef) -> Result<SSARef, CompileError> {
    // cosh(x) = (exp(x) + exp(-x)) / 2
    let log2_e: f32 = std::f32::consts::LOG2_E;
    let half: f32 = 0.5;
    let s1 = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFMul {
        dst: s1.into(),
        srcs: [a[0].into(), Src::new_imm_u32(log2_e.to_bits())],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    let exp_pos = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpTranscendental {
        dst: exp_pos.into(),
        op: TranscendentalOp::Exp2,
        src: s1.into(),
    }));
    let s2 = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFMul {
        dst: s2.into(),
        srcs: [Src::from(a[0]).fneg(), Src::new_imm_u32(log2_e.to_bits())],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    let exp_neg = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpTranscendental {
        dst: exp_neg.into(),
        op: TranscendentalOp::Exp2,
        src: s2.into(),
    }));
    let sum = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFAdd {
        dst: sum.into(),
        srcs: [exp_pos.into(), exp_neg.into()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
    }));
    let dst = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFMul {
        dst: dst.into(),
        srcs: [sum.into(), Src::new_imm_u32(half.to_bits())],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    Ok(dst.into())
}
