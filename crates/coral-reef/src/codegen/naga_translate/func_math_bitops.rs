// SPDX-License-Identifier: AGPL-3.0-only
//! Integer bit operations: countOneBits, reverseBits, firstLeadingBit, countLeadingZeros.

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
        naga::MathFunction::CountOneBits => Some(translate_count_one_bits(ft, a)?),
        naga::MathFunction::ReverseBits => Some(translate_reverse_bits(ft, a)?),
        naga::MathFunction::FirstLeadingBit => {
            Some(translate_first_leading_bit(ft, a, arg_handle)?)
        }
        naga::MathFunction::CountLeadingZeros => Some(translate_count_leading_zeros(ft, a)?),
        _ => None,
    };
    Ok(result)
}

fn translate_count_one_bits(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
) -> Result<SSARef, CompileError> {
    let comps = a.comps();
    let dst = ft.alloc_ssa_vec(RegFile::GPR, comps);
    for c in 0..comps as usize {
        ft.push_instr(Instr::new(OpPopC {
            dst: dst[c].into(),
            src: a[c].into(),
        }));
    }
    Ok(dst)
}

fn translate_reverse_bits(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
) -> Result<SSARef, CompileError> {
    let comps = a.comps();
    let dst = ft.alloc_ssa_vec(RegFile::GPR, comps);
    for c in 0..comps as usize {
        ft.push_instr(Instr::new(OpBRev {
            dst: dst[c].into(),
            src: a[c].into(),
        }));
    }
    Ok(dst)
}

fn translate_first_leading_bit(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
    arg_handle: Handle<naga::Expression>,
) -> Result<SSARef, CompileError> {
    let signed = ft.is_signed_int_expr(arg_handle);
    let comps = a.comps();
    let dst = ft.alloc_ssa_vec(RegFile::GPR, comps);
    for c in 0..comps as usize {
        ft.push_instr(Instr::new(OpFlo {
            dst: dst[c].into(),
            src: a[c].into(),
            signed,
            return_shift_amount: false,
        }));
    }
    Ok(dst)
}

fn translate_count_leading_zeros(
    ft: &mut FuncTranslator<'_, '_>,
    a: SSARef,
) -> Result<SSARef, CompileError> {
    let comps = a.comps();
    let dst = ft.alloc_ssa_vec(RegFile::GPR, comps);
    for c in 0..comps as usize {
        ft.push_instr(Instr::new(OpFlo {
            dst: dst[c].into(),
            src: a[c].into(),
            signed: false,
            return_shift_amount: true,
        }));
    }
    Ok(dst)
}
