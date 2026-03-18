// SPDX-License-Identifier: AGPL-3.0-only
//! Interpolation and misc math: mix, step, smoothstep, sign, fma.

use super::super::ir::*;
use super::func::FuncTranslator;
use crate::error::CompileError;
use naga::Handle;

pub fn translate(
    ft: &mut FuncTranslator<'_, '_>,
    fun: naga::MathFunction,
    a: SSARef,
    b: Option<SSARef>,
    c: Option<SSARef>,
    _arg_handle: Handle<naga::Expression>,
) -> Result<Option<SSARef>, CompileError> {
    let result = match fun {
        naga::MathFunction::Fma => {
            let b = b.ok_or_else(|| CompileError::InvalidInput("fma requires 3 args".into()))?;
            let c = c.ok_or_else(|| CompileError::InvalidInput("fma requires 3 args".into()))?;
            Some(translate_fma(ft, a, b, c)?)
        }
        naga::MathFunction::Sign => Some(translate_sign(ft, a)?),
        naga::MathFunction::Mix => {
            let b = b.ok_or_else(|| CompileError::InvalidInput("mix requires 3 args".into()))?;
            let t = c.ok_or_else(|| CompileError::InvalidInput("mix requires 3 args".into()))?;
            Some(translate_mix(ft, a, b, t)?)
        }
        naga::MathFunction::Step => {
            let b = b.ok_or_else(|| CompileError::InvalidInput("step requires 2 args".into()))?;
            Some(translate_step(ft, a, b)?)
        }
        naga::MathFunction::SmoothStep => {
            let b =
                b.ok_or_else(|| CompileError::InvalidInput("smoothstep requires 3 args".into()))?;
            let x =
                c.ok_or_else(|| CompileError::InvalidInput("smoothstep requires 3 args".into()))?;
            Some(translate_smoothstep(ft, a, b, x)?)
        }
        _ => None,
    };
    Ok(result)
}

fn translate_fma(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
    b: SSARef,
    c: SSARef,
) -> Result<SSARef, CompileError> {
    let dst = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFFma {
        dst: dst.into(),
        srcs: [a[0].into(), b[0].into(), c[0].into()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    Ok(dst.into())
}

fn translate_sign(ft: &mut FuncTranslator<'_, '_>, a: SSARef) -> Result<SSARef, CompileError> {
    let dst = ft.alloc_ssa(RegFile::GPR);
    let pos = ft.alloc_ssa(RegFile::Pred);
    ft.push_instr(Instr::new(OpFSetP {
        dst: pos.into(),
        set_op: PredSetOp::And,
        cmp_op: FloatCmpOp::OrdGt,
        srcs: [a[0].into(), Src::ZERO, SrcRef::True.into()],
        ftz: false,
    }));
    let neg = ft.alloc_ssa(RegFile::Pred);
    ft.push_instr(Instr::new(OpFSetP {
        dst: neg.into(),
        set_op: PredSetOp::And,
        cmp_op: FloatCmpOp::OrdLt,
        srcs: [a[0].into(), Src::ZERO, SrcRef::True.into()],
        ftz: false,
    }));
    ft.push_instr(Instr::new(OpCopy {
        dst: dst.into(),
        src: Src::ZERO,
    }));
    let neg_one: f32 = -1.0;
    let tmp = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpSel {
        dst: tmp.into(),
        srcs: [neg.into(), Src::new_imm_u32(neg_one.to_bits()), dst.into()],
    }));
    let result = ft.alloc_ssa(RegFile::GPR);
    let one: f32 = 1.0;
    ft.push_instr(Instr::new(OpSel {
        dst: result.into(),
        srcs: [pos.into(), Src::new_imm_u32(one.to_bits()), tmp.into()],
    }));
    Ok(result.into())
}

fn translate_mix(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
    b: SSARef,
    t: SSARef,
) -> Result<SSARef, CompileError> {
    // mix(a, b, t) = a + t*(b - a) = (b-a)*t + a
    let diff = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFAdd {
        dst: diff.into(),
        srcs: [b[0].into(), Src::from(a[0]).fneg()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
    }));
    let dst = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFFma {
        dst: dst.into(),
        srcs: [diff.into(), t[0].into(), a[0].into()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    Ok(dst.into())
}

fn translate_step(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
    b: SSARef,
) -> Result<SSARef, CompileError> {
    // step(edge, x) = x >= edge ? 1.0 : 0.0
    let pred = ft.alloc_ssa(RegFile::Pred);
    ft.push_instr(Instr::new(OpFSetP {
        dst: pred.into(),
        set_op: PredSetOp::And,
        cmp_op: FloatCmpOp::OrdGe,
        srcs: [b[0].into(), a[0].into(), SrcRef::True.into()],
        ftz: false,
    }));
    let one: f32 = 1.0;
    let dst = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpSel {
        dst: dst.into(),
        srcs: [pred.into(), Src::new_imm_u32(one.to_bits()), Src::ZERO],
    }));
    Ok(dst.into())
}

fn translate_smoothstep(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
    b: SSARef,
    x: SSARef,
) -> Result<SSARef, CompileError> {
    // smoothstep(lo, hi, x): t = clamp((x-lo)/(hi-lo), 0, 1); return t*t*(3-2*t)
    let diff_x = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFAdd {
        dst: diff_x.into(),
        srcs: [x[0].into(), Src::from(a[0]).fneg()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
    }));
    let range = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFAdd {
        dst: range.into(),
        srcs: [b[0].into(), Src::from(a[0]).fneg()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
    }));
    let rcp_range = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpTranscendental {
        dst: rcp_range.into(),
        op: TranscendentalOp::Rcp,
        src: range.into(),
    }));
    let t_raw = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFMul {
        dst: t_raw.into(),
        srcs: [diff_x.into(), rcp_range.into()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    let t = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFAdd {
        dst: t.into(),
        srcs: [t_raw.into(), Src::ZERO],
        saturate: true,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
    }));
    let t2 = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFMul {
        dst: t2.into(),
        srcs: [t.into(), t.into()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    let two: f32 = 2.0;
    let three: f32 = 3.0;
    let two_t = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFMul {
        dst: two_t.into(),
        srcs: [Src::new_imm_u32(two.to_bits()), t.into()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    let three_minus_2t = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFAdd {
        dst: three_minus_2t.into(),
        srcs: [Src::new_imm_u32(three.to_bits()), Src::from(two_t).fneg()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
    }));
    let dst = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFMul {
        dst: dst.into(),
        srcs: [t2.into(), three_minus_2t.into()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    Ok(dst.into())
}
