// SPDX-License-Identifier: AGPL-3.0-only
//! Vector math operations: dot, cross, length, normalize.

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
    _arg_handle: Handle<naga::Expression>,
) -> Result<Option<SSARef>, CompileError> {
    let result = match fun {
        naga::MathFunction::Dot => {
            let b = b.ok_or_else(|| CompileError::InvalidInput("dot requires 2 args".into()))?;
            Some(translate_dot(ft, a, b)?)
        }
        naga::MathFunction::Cross => {
            let b = b.ok_or_else(|| CompileError::InvalidInput("cross requires 2 args".into()))?;
            Some(translate_cross(ft, a, b)?)
        }
        naga::MathFunction::Length => Some(translate_length(ft, a)?),
        naga::MathFunction::Normalize => Some(translate_normalize(ft, a)?),
        _ => None,
    };
    Ok(result)
}

fn translate_dot(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
    b: SSARef,
) -> Result<SSARef, CompileError> {
    let comps = a.comps().min(b.comps());
    let mut acc = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFMul {
        dst: acc.into(),
        srcs: [a[0].into(), b[0].into()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    for c in 1..comps as usize {
        let next = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpFFma {
            dst: next.into(),
            srcs: [a[c].into(), b[c].into(), acc.into()],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            dnz: false,
        }));
        acc = next;
    }
    Ok(acc.into())
}

fn translate_cross(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
    b: SSARef,
) -> Result<SSARef, CompileError> {
    // cross(a, b) = (a.y*b.z - a.z*b.y, a.z*b.x - a.x*b.z, a.x*b.y - a.y*b.x)
    let dst = ft.alloc_ssa_vec(RegFile::GPR, 3);
    let tmp0 = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFMul {
        dst: tmp0.into(),
        srcs: [a[1].into(), b[2].into()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    ft.push_instr(Instr::new(OpFFma {
        dst: dst[0].into(),
        srcs: [Src::from(a[2]).fneg(), b[1].into(), tmp0.into()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    let tmp1 = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFMul {
        dst: tmp1.into(),
        srcs: [a[2].into(), b[0].into()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    ft.push_instr(Instr::new(OpFFma {
        dst: dst[1].into(),
        srcs: [Src::from(a[0]).fneg(), b[2].into(), tmp1.into()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    let tmp2 = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpFMul {
        dst: tmp2.into(),
        srcs: [a[0].into(), b[1].into()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    ft.push_instr(Instr::new(OpFFma {
        dst: dst[2].into(),
        srcs: [Src::from(a[1]).fneg(), b[0].into(), tmp2.into()],
        saturate: false,
        rnd_mode: FRndMode::NearestEven,
        ftz: false,
        dnz: false,
    }));
    Ok(dst)
}

fn translate_length(ft: &mut FuncTranslator<'_, '_>, a: SSARef) -> Result<SSARef, CompileError> {
    let comps = a.comps();
    if comps == 1 {
        let dst = ft.alloc_ssa(RegFile::GPR);
        ft.push_instr(Instr::new(OpFAdd {
            dst: dst.into(),
            srcs: [Src::ZERO, Src::from(a[0]).fabs()],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
        }));
        return Ok(dst.into());
    }
    let dot = ft.emit_f32_dot_self(&a);
    let rsq = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpTranscendental {
        dst: rsq.into(),
        op: TranscendentalOp::Rsq,
        src: dot.into(),
    }));
    let dst = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpTranscendental {
        dst: dst.into(),
        op: TranscendentalOp::Rcp,
        src: rsq.into(),
    }));
    Ok(dst.into())
}

fn translate_normalize(ft: &mut FuncTranslator<'_, '_>, a: SSARef) -> Result<SSARef, CompileError> {
    let comps = a.comps();
    let dot = ft.emit_f32_dot_self(&a);
    let inv_len = ft.alloc_ssa(RegFile::GPR);
    ft.push_instr(Instr::new(OpTranscendental {
        dst: inv_len.into(),
        op: TranscendentalOp::Rsq,
        src: dot.into(),
    }));
    let dst = ft.alloc_ssa_vec(RegFile::GPR, comps);
    for c in 0..comps as usize {
        ft.push_instr(Instr::new(OpFMul {
            dst: dst[c].into(),
            srcs: [a[c].into(), inv_len.into()],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            dnz: false,
        }));
    }
    Ok(dst)
}
