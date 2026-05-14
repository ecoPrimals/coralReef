// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt::Write as _;

use crate::error::CompileError;

use super::PtxEmitter;
use super::types::PtxVal;

impl PtxEmitter<'_> {
    /// Emits PTX for a built-in mathematical function (`min`, `max`, `fma`, etc.).
    pub(super) fn eval_math(
        &mut self,
        fun: naga::MathFunction,
        arg: &PtxVal,
        arg1: Option<&PtxVal>,
        arg2: Option<&PtxVal>,
        arg_handle: naga::Handle<naga::Expression>,
    ) -> Result<PtxVal, CompileError> {
        use naga::MathFunction as MF;
        let scalar = self.scalar_of(arg_handle);
        let ts = Self::ptx_type_suffix(scalar);

        match fun {
            MF::Abs => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    abs.{ts} {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Min => {
                let rhs =
                    arg1.ok_or_else(|| CompileError::NotImplemented("min without arg1".into()))?;
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    min.{ts} {}, {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                    rhs.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Max => {
                let rhs =
                    arg1.ok_or_else(|| CompileError::NotImplemented("max without arg1".into()))?;
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    max.{ts} {}, {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                    rhs.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Clamp => {
                let lo =
                    arg1.ok_or_else(|| CompileError::NotImplemented("clamp without arg1".into()))?;
                let hi =
                    arg2.ok_or_else(|| CompileError::NotImplemented("clamp without arg2".into()))?;
                let tmp = self.alloc_for_scalar(scalar);
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    max.{ts} {}, {}, {};",
                    tmp.fmt_operand(),
                    arg.fmt_operand(),
                    lo.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    min.{ts} {}, {}, {};",
                    dst.fmt_operand(),
                    tmp.fmt_operand(),
                    hi.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Floor | MF::Ceil | MF::Round | MF::Trunc => {
                let mode = match fun {
                    MF::Floor => "rmi",
                    MF::Ceil => "rpi",
                    MF::Round => "rni",
                    MF::Trunc => "rzi",
                    _ => crate::codegen::ice!("rounding mode matched Floor|Ceil|Round|Trunc above"),
                };
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    cvt.{mode}.{ts}.{ts} {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Sqrt => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    sqrt.rn.{ts} {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::InverseSqrt => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    rsqrt.approx.{ts} {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Sin | MF::Cos => {
                let op_name = if matches!(fun, MF::Sin) { "sin" } else { "cos" };
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    {op_name}.approx.{ts} {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Exp2 => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    ex2.approx.{ts} {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Log2 => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    lg2.approx.{ts} {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Fma => {
                let mul_term =
                    arg1.ok_or_else(|| CompileError::NotImplemented("fma without arg1".into()))?;
                let add_term =
                    arg2.ok_or_else(|| CompileError::NotImplemented("fma without arg2".into()))?;
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    fma.rn.{ts} {}, {}, {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                    mul_term.fmt_operand(),
                    add_term.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Pow => {
                let exp =
                    arg1.ok_or_else(|| CompileError::NotImplemented("pow without arg1".into()))?;
                let lg = self.alloc_for_scalar(scalar);
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    lg2.approx.{ts} {}, {};",
                    lg.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    mul.{ts} {}, {}, {};",
                    lg.fmt_operand(),
                    lg.fmt_operand(),
                    exp.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    ex2.approx.{ts} {}, {};",
                    dst.fmt_operand(),
                    lg.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Exp => {
                // exp(x) = ex2(x * log2(e))
                let tmp = self.alloc_for_scalar(scalar);
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    mul.{ts} {}, {}, 0f3FB8AA3B;",
                    tmp.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    ex2.approx.{ts} {}, {};",
                    dst.fmt_operand(),
                    tmp.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Log => {
                // log(x) = lg2(x) * ln(2)
                let tmp = self.alloc_for_scalar(scalar);
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    lg2.approx.{ts} {}, {};",
                    tmp.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    mul.{ts} {}, {}, 0f3F317218;",
                    dst.fmt_operand(),
                    tmp.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Sign => {
                let dst = self.alloc_for_scalar(scalar);
                let p_pos = self.alloc_pred();
                let p_neg = self.alloc_pred();
                writeln!(
                    self.body,
                    "    setp.gt.{ts} {}, {}, 0f00000000;",
                    p_pos.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    setp.lt.{ts} {}, {}, 0f00000000;",
                    p_neg.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    selp.{ts} {}, 0f3F800000, 0f00000000, {};",
                    dst.fmt_operand(),
                    p_pos.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    selp.{ts} {}, 0fBF800000, {}, {};",
                    dst.fmt_operand(),
                    dst.fmt_operand(),
                    p_neg.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Fract => {
                // fract(x) = x - floor(x)
                let floored = self.alloc_for_scalar(scalar);
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    cvt.rmi.{ts}.{ts} {}, {};",
                    floored.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    sub.{ts} {}, {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                    floored.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Mix => {
                // mix(a, b, t) = a*(1-t) + b*t = a + t*(b - a)
                let b =
                    arg1.ok_or_else(|| CompileError::NotImplemented("mix without arg1".into()))?;
                let t =
                    arg2.ok_or_else(|| CompileError::NotImplemented("mix without arg2".into()))?;
                let diff = self.alloc_for_scalar(scalar);
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    sub.{ts} {}, {}, {};",
                    diff.fmt_operand(),
                    b.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    fma.rn.{ts} {}, {}, {}, {};",
                    dst.fmt_operand(),
                    t.fmt_operand(),
                    diff.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Step => {
                // step(edge, x) = x >= edge ? 1.0 : 0.0
                let x =
                    arg1.ok_or_else(|| CompileError::NotImplemented("step without arg1".into()))?;
                let pred = self.alloc_pred();
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    setp.ge.{ts} {}, {}, {};",
                    pred.fmt_operand(),
                    x.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    selp.{ts} {}, 0f3F800000, 0f00000000, {};",
                    dst.fmt_operand(),
                    pred.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Dot => {
                let rhs =
                    arg1.ok_or_else(|| CompileError::NotImplemented("dot without arg1".into()))?;
                match (arg, rhs) {
                    (PtxVal::Vec(lhs_comps), PtxVal::Vec(rhs_comps)) => {
                        let dst = self.alloc_for_scalar(scalar);
                        writeln!(
                            self.body,
                            "    mul.{ts} {}, {}, {};",
                            dst.fmt_operand(),
                            lhs_comps[0].fmt_operand(),
                            rhs_comps[0].fmt_operand(),
                        )
                        .expect("write to String");
                        for (l, r) in lhs_comps.iter().zip(rhs_comps.iter()).skip(1) {
                            writeln!(
                                self.body,
                                "    fma.rn.{ts} {}, {}, {}, {};",
                                dst.fmt_operand(),
                                l.fmt_operand(),
                                r.fmt_operand(),
                                dst.fmt_operand(),
                            )
                            .expect("write to String");
                        }
                        Ok(dst)
                    }
                    _ => Err(CompileError::NotImplemented(
                        "PTX dot: non-vector operands".into(),
                    )),
                }
            }
            MF::Saturate => {
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    max.{ts} {}, {}, 0f00000000;",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    min.{ts} {}, {}, 0f3F800000;",
                    dst.fmt_operand(),
                    dst.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Radians => {
                let dst = self.alloc_for_scalar(scalar);
                // π/180 ≈ 0.01745329 = 0x3C8EFA35 in f32
                writeln!(
                    self.body,
                    "    mul.{ts} {}, {}, 0f3C8EFA35;",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Degrees => {
                let dst = self.alloc_for_scalar(scalar);
                // 180/π ≈ 57.2957795 = 0x42652EE1 in f32
                writeln!(
                    self.body,
                    "    mul.{ts} {}, {}, 0f42652EE1;",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::CountOneBits => {
                let dst = self.alloc_r32();
                writeln!(
                    self.body,
                    "    popc.b32 {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::CountLeadingZeros => {
                let dst = self.alloc_r32();
                writeln!(
                    self.body,
                    "    clz.b32 {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::CountTrailingZeros => {
                // PTX has no ctz — use brev + clz
                let rev = self.alloc_r32();
                let dst = self.alloc_r32();
                writeln!(
                    self.body,
                    "    brev.b32 {}, {};",
                    rev.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    clz.b32 {}, {};",
                    dst.fmt_operand(),
                    rev.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::ReverseBits => {
                let dst = self.alloc_r32();
                writeln!(
                    self.body,
                    "    brev.b32 {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::SmoothStep => {
                // smoothstep(low, high, x) = t*t*(3-2t), where t = clamp((x-low)/(high-low), 0, 1)
                let low = arg1.ok_or_else(|| {
                    CompileError::NotImplemented("smoothstep without arg1".into())
                })?;
                let x = arg2.ok_or_else(|| {
                    CompileError::NotImplemented("smoothstep without arg2".into())
                })?;
                let range = self.alloc_for_scalar(scalar);
                let t = self.alloc_for_scalar(scalar);
                let two_t = self.alloc_for_scalar(scalar);
                let three_minus_2t = self.alloc_for_scalar(scalar);
                let dst = self.alloc_for_scalar(scalar);
                // range = high - low (arg is low, arg1 is high, arg2 is x in naga convention)
                // Actually naga::MathFunction::SmoothStep has args: (low, high, x)
                // naga passes them as arg=low, arg1=high, arg2=x
                writeln!(
                    self.body,
                    "    sub.{ts} {}, {}, {};",
                    range.fmt_operand(),
                    low.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                // t = (x - low) / range
                writeln!(
                    self.body,
                    "    sub.{ts} {}, {}, {};",
                    t.fmt_operand(),
                    x.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    rcp.approx.{ts} {}, {};",
                    range.fmt_operand(),
                    range.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    mul.{ts} {}, {}, {};",
                    t.fmt_operand(),
                    t.fmt_operand(),
                    range.fmt_operand(),
                )
                .expect("write to String");
                // clamp t to [0, 1]
                writeln!(
                    self.body,
                    "    max.{ts} {}, {}, 0f00000000;",
                    t.fmt_operand(),
                    t.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    min.{ts} {}, {}, 0f3F800000;",
                    t.fmt_operand(),
                    t.fmt_operand(),
                )
                .expect("write to String");
                // 3 - 2*t
                writeln!(
                    self.body,
                    "    mul.{ts} {}, {}, 0f40000000;",
                    two_t.fmt_operand(),
                    t.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    sub.{ts} {}, 0f40400000, {};",
                    three_minus_2t.fmt_operand(),
                    two_t.fmt_operand(),
                )
                .expect("write to String");
                // t * t * (3 - 2t)
                writeln!(
                    self.body,
                    "    mul.{ts} {}, {}, {};",
                    dst.fmt_operand(),
                    t.fmt_operand(),
                    t.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    mul.{ts} {}, {}, {};",
                    dst.fmt_operand(),
                    dst.fmt_operand(),
                    three_minus_2t.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Tanh => {
                // tanh(x) ≈ 1 - 2/(exp(2x)+1): use ex2 approximation
                let two_x = self.alloc_for_scalar(scalar);
                let exp_val = self.alloc_for_scalar(scalar);
                let denom = self.alloc_for_scalar(scalar);
                let frac = self.alloc_for_scalar(scalar);
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    mul.{ts} {}, {}, 0f40000000;",
                    two_x.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    mul.{ts} {}, {}, 0f3FB8AA3B;",
                    two_x.fmt_operand(),
                    two_x.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    ex2.approx.{ts} {}, {};",
                    exp_val.fmt_operand(),
                    two_x.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    add.{ts} {}, {}, 0f3F800000;",
                    denom.fmt_operand(),
                    exp_val.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    rcp.approx.{ts} {}, {};",
                    denom.fmt_operand(),
                    denom.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    mul.{ts} {}, 0f40000000, {};",
                    frac.fmt_operand(),
                    denom.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    sub.{ts} {}, 0f3F800000, {};",
                    dst.fmt_operand(),
                    frac.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            _ => Err(CompileError::NotImplemented(
                format!("PTX math function: {fun:?}").into(),
            )),
        }
    }
}
