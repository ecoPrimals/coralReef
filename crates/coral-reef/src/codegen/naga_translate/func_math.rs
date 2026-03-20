// SPDX-License-Identifier: AGPL-3.0-only
//! Math operation translation (naga → IR).
//!
//! Coordinates delegation to domain-specific submodules:
//! - [`super::func_math_trig`]: sin, cos, tan, atan, atan2, asin, acos
//! - [`super::func_math_exp_log`]: exp, exp2, log, log2, pow, sinh, cosh, tanh, asinh, acosh, atanh
//! - [`super::func_math_rounding`]: floor, ceil, round, trunc, fract
//! - [`super::func_math_extrema`]: abs, min, max, clamp
//! - [`super::func_math_sqrt`]: sqrt, inverseSqrt
//! - [`super::func_math_vector`]: dot, cross, length, normalize
//! - [`super::func_math_bitops`]: countOneBits, reverseBits, firstLeadingBit, countLeadingZeros
//! - [`super::func_math_interp`]: mix, step, smoothstep, sign, fma

use super::super::ir::*;
use super::func::FuncTranslator;
use super::{
    func_math_bitops, func_math_exp_log, func_math_extrema, func_math_interp, func_math_rounding,
    func_math_sqrt, func_math_trig, func_math_vector,
};
use crate::error::CompileError;
use naga::Handle;

impl<'a, 'b> FuncTranslator<'a, 'b> {
    pub(super) fn translate_math(
        &mut self,
        fun: naga::MathFunction,
        a: SSARef,
        b: Option<SSARef>,
        c: Option<SSARef>,
        arg_handle: Handle<naga::Expression>,
    ) -> Result<SSARef, CompileError> {
        if let Some(dst) =
            func_math_trig::translate(self, fun, &a, b.as_ref(), c.as_ref(), arg_handle)?
        {
            return Ok(dst);
        }
        if let Some(dst) =
            func_math_exp_log::translate(self, fun, &a, b.as_ref(), c.as_ref(), arg_handle)?
        {
            return Ok(dst);
        }
        if let Some(dst) =
            func_math_rounding::translate(self, fun, &a, b.as_ref(), c.as_ref(), arg_handle)?
        {
            return Ok(dst);
        }
        if let Some(dst) =
            func_math_extrema::translate(self, fun, &a, b.as_ref(), c.as_ref(), arg_handle)?
        {
            return Ok(dst);
        }
        if let Some(dst) =
            func_math_sqrt::translate(self, fun, &a, b.as_ref(), c.as_ref(), arg_handle)?
        {
            return Ok(dst);
        }
        if let Some(dst) =
            func_math_vector::translate(self, fun, &a, b.as_ref(), c.as_ref(), arg_handle)?
        {
            return Ok(dst);
        }
        if let Some(dst) =
            func_math_bitops::translate(self, fun, &a, b.as_ref(), c.as_ref(), arg_handle)?
        {
            return Ok(dst);
        }
        if let Some(dst) = func_math_interp::translate(self, fun, a, b, c, arg_handle)? {
            return Ok(dst);
        }
        Err(CompileError::NotImplemented(
            format!("math function {fun:?} not yet supported").into(),
        ))
    }
}
