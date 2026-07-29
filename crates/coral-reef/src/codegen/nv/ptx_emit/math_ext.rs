// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::error::CompileError;

use super::PtxEmitter;
use super::types::PtxVal;

impl PtxEmitter<'_> {
    /// Extended math operations (geometry, inverse trig, bit manipulation).
    pub(super) fn eval_math_extended(
        &mut self,
        fun: naga::MathFunction,
        arg: &PtxVal,
        arg1: Option<&PtxVal>,
        arg2: Option<&PtxVal>,
        scalar: naga::Scalar,
        ts: &str,
    ) -> Result<PtxVal, CompileError> {
        use naga::MathFunction as MF;
        match fun {
            MF::Length => {
                let PtxVal::Vec(comps) = arg else {
                    return Err(CompileError::NotImplemented(
                        "PTX length: non-vector operand".into(),
                    ));
                };
                let dot_acc = self.alloc_r32();
                writeln_ptx!(
                    self.body,
                    "    mul.rn.f32 {}, {}, {};",
                    dot_acc.fmt_operand(),
                    comps[0].fmt_operand(),
                    comps[0].fmt_operand(),
                );
                for c in comps.iter().skip(1) {
                    writeln_ptx!(
                        self.body,
                        "    fma.rn.f32 {}, {}, {}, {};",
                        dot_acc.fmt_operand(),
                        c.fmt_operand(),
                        c.fmt_operand(),
                        dot_acc.fmt_operand(),
                    );
                }
                let dst = self.alloc_r32();
                writeln_ptx!(
                    self.body,
                    "    sqrt.rn.f32 {}, {};",
                    dst.fmt_operand(),
                    dot_acc.fmt_operand(),
                );
                Ok(dst)
            }
            MF::Normalize => {
                let PtxVal::Vec(comps) = arg else {
                    let dst = self.alloc_r32();
                    writeln_ptx!(self.body, "    mov.f32 {}, 0f3F800000;", dst.fmt_operand());
                    return Ok(dst);
                };
                let dot_acc = self.alloc_r32();
                writeln_ptx!(
                    self.body,
                    "    mul.rn.f32 {}, {}, {};",
                    dot_acc.fmt_operand(),
                    comps[0].fmt_operand(),
                    comps[0].fmt_operand(),
                );
                for c in comps.iter().skip(1) {
                    writeln_ptx!(
                        self.body,
                        "    fma.rn.f32 {}, {}, {}, {};",
                        dot_acc.fmt_operand(),
                        c.fmt_operand(),
                        c.fmt_operand(),
                        dot_acc.fmt_operand(),
                    );
                }
                let inv_len = self.alloc_r32();
                writeln_ptx!(
                    self.body,
                    "    rsqrt.approx.f32 {}, {};",
                    inv_len.fmt_operand(),
                    dot_acc.fmt_operand(),
                );
                let result: Vec<PtxVal> = comps
                    .iter()
                    .map(|c| {
                        let d = self.alloc_r32();
                        writeln_ptx!(
                            self.body,
                            "    mul.rn.f32 {}, {}, {};",
                            d.fmt_operand(),
                            c.fmt_operand(),
                            inv_len.fmt_operand(),
                        );
                        d
                    })
                    .collect();
                Ok(PtxVal::Vec(result))
            }
            MF::Distance => {
                let rhs = super::require_math_arg(arg1, "distance", 1)?;
                let (PtxVal::Vec(lhs_comps), PtxVal::Vec(rhs_comps)) = (arg, rhs) else {
                    return Err(CompileError::NotImplemented(
                        "PTX distance: non-vector operands".into(),
                    ));
                };
                let dot_acc = self.alloc_r32();
                let diff0 = self.alloc_r32();
                writeln_ptx!(
                    self.body,
                    "    sub.rn.f32 {}, {}, {};",
                    diff0.fmt_operand(),
                    lhs_comps[0].fmt_operand(),
                    rhs_comps[0].fmt_operand(),
                );
                writeln_ptx!(
                    self.body,
                    "    mul.rn.f32 {}, {}, {};",
                    dot_acc.fmt_operand(),
                    diff0.fmt_operand(),
                    diff0.fmt_operand(),
                );
                for (l, r) in lhs_comps.iter().zip(rhs_comps.iter()).skip(1) {
                    let d = self.alloc_r32();
                    writeln_ptx!(
                        self.body,
                        "    sub.rn.f32 {}, {}, {};",
                        d.fmt_operand(),
                        l.fmt_operand(),
                        r.fmt_operand(),
                    );
                    writeln_ptx!(
                        self.body,
                        "    fma.rn.f32 {}, {}, {}, {};",
                        dot_acc.fmt_operand(),
                        d.fmt_operand(),
                        d.fmt_operand(),
                        dot_acc.fmt_operand(),
                    );
                }
                let dst = self.alloc_r32();
                writeln_ptx!(
                    self.body,
                    "    sqrt.rn.f32 {}, {};",
                    dst.fmt_operand(),
                    dot_acc.fmt_operand(),
                );
                Ok(dst)
            }
            MF::Cross => {
                let rhs = super::require_math_arg(arg1, "cross", 1)?;
                let (PtxVal::Vec(a), PtxVal::Vec(b)) = (arg, rhs) else {
                    return Err(CompileError::NotImplemented(
                        "PTX cross: non-vector operands".into(),
                    ));
                };
                if a.len() < 3 || b.len() < 3 {
                    return Err(CompileError::NotImplemented(
                        "PTX cross: requires 3-component vectors".into(),
                    ));
                }
                let rx = self.alloc_r32();
                let ry = self.alloc_r32();
                let rz = self.alloc_r32();
                writeln_ptx!(
                    self.body,
                    "    mul.rn.f32 {rx}, {a1}, {b2};",
                    rx = rx.fmt_operand(),
                    a1 = a[1].fmt_operand(),
                    b2 = b[2].fmt_operand(),
                );
                writeln_ptx!(
                    self.body,
                    "    fma.rn.f32 {rx}, -{a2}, {b1}, {rx};",
                    rx = rx.fmt_operand(),
                    a2 = a[2].fmt_operand(),
                    b1 = b[1].fmt_operand(),
                );
                writeln_ptx!(
                    self.body,
                    "    mul.rn.f32 {ry}, {a2}, {b0};",
                    ry = ry.fmt_operand(),
                    a2 = a[2].fmt_operand(),
                    b0 = b[0].fmt_operand(),
                );
                writeln_ptx!(
                    self.body,
                    "    fma.rn.f32 {ry}, -{a0}, {b2}, {ry};",
                    ry = ry.fmt_operand(),
                    a0 = a[0].fmt_operand(),
                    b2 = b[2].fmt_operand(),
                );
                writeln_ptx!(
                    self.body,
                    "    mul.rn.f32 {rz}, {a0}, {b1};",
                    rz = rz.fmt_operand(),
                    a0 = a[0].fmt_operand(),
                    b1 = b[1].fmt_operand(),
                );
                writeln_ptx!(
                    self.body,
                    "    fma.rn.f32 {rz}, -{a1}, {b0}, {rz};",
                    rz = rz.fmt_operand(),
                    a1 = a[1].fmt_operand(),
                    b0 = b[0].fmt_operand(),
                );
                Ok(PtxVal::Vec(vec![rx, ry, rz]))
            }
            MF::Tan | MF::Atan | MF::Atan2 | MF::Asin | MF::Acos => {
                self.eval_math_trig(fun, arg, arg1, scalar, ts)
            }
            MF::Reflect => {
                let rhs = super::require_math_arg(arg1, "reflect", 1)?;
                let (PtxVal::Vec(i_comps), PtxVal::Vec(n_comps)) = (arg, rhs) else {
                    return Err(CompileError::NotImplemented(
                        "PTX reflect: non-vector operands".into(),
                    ));
                };
                // reflect(I, N) = I - 2.0 * dot(N, I) * N
                let dot_acc = self.alloc_r32();
                writeln_ptx!(
                    self.body,
                    "    mul.rn.f32 {}, {}, {};",
                    dot_acc.fmt_operand(),
                    n_comps[0].fmt_operand(),
                    i_comps[0].fmt_operand(),
                );
                for (n, i) in n_comps.iter().zip(i_comps.iter()).skip(1) {
                    writeln_ptx!(
                        self.body,
                        "    fma.rn.f32 {}, {}, {}, {};",
                        dot_acc.fmt_operand(),
                        n.fmt_operand(),
                        i.fmt_operand(),
                        dot_acc.fmt_operand(),
                    );
                }
                let two_dot = self.alloc_r32();
                writeln_ptx!(
                    self.body,
                    "    mul.rn.f32 {}, 0f40000000, {};",
                    two_dot.fmt_operand(),
                    dot_acc.fmt_operand(),
                );
                let result: Vec<PtxVal> = i_comps
                    .iter()
                    .zip(n_comps.iter())
                    .map(|(i, n)| {
                        let d = self.alloc_r32();
                        writeln_ptx!(
                            self.body,
                            "    mul.rn.f32 {tmp}, {two_dot}, {n};",
                            tmp = d.fmt_operand(),
                            two_dot = two_dot.fmt_operand(),
                            n = n.fmt_operand(),
                        );
                        writeln_ptx!(
                            self.body,
                            "    sub.f32 {d}, {i}, {d};",
                            d = d.fmt_operand(),
                            i = i.fmt_operand(),
                        );
                        d
                    })
                    .collect();
                Ok(PtxVal::Vec(result))
            }
            MF::FaceForward => {
                let n = arg1.ok_or_else(|| {
                    CompileError::NotImplemented("faceForward without arg1".into())
                })?;
                let i = arg2.ok_or_else(|| {
                    CompileError::NotImplemented("faceForward without arg2".into())
                })?;
                let (PtxVal::Vec(n_comps), PtxVal::Vec(nref_comps), PtxVal::Vec(i_comps)) =
                    (arg, n, i)
                else {
                    return Err(CompileError::NotImplemented(
                        "PTX faceForward: non-vector operands".into(),
                    ));
                };
                // faceForward(N, I, Nref) = N if dot(Nref, I) < 0, else -N
                let dot_acc = self.alloc_r32();
                writeln_ptx!(
                    self.body,
                    "    mul.rn.f32 {}, {}, {};",
                    dot_acc.fmt_operand(),
                    nref_comps[0].fmt_operand(),
                    i_comps[0].fmt_operand(),
                );
                for (nr, ic) in nref_comps.iter().zip(i_comps.iter()).skip(1) {
                    writeln_ptx!(
                        self.body,
                        "    fma.rn.f32 {}, {}, {}, {};",
                        dot_acc.fmt_operand(),
                        nr.fmt_operand(),
                        ic.fmt_operand(),
                        dot_acc.fmt_operand(),
                    );
                }
                let pred = self.alloc_pred();
                writeln_ptx!(
                    self.body,
                    "    setp.lt.f32 {}, {}, 0f00000000;",
                    pred.fmt_operand(),
                    dot_acc.fmt_operand(),
                );
                let result: Vec<PtxVal> = n_comps
                    .iter()
                    .map(|nc| {
                        let neg = self.alloc_r32();
                        let out = self.alloc_r32();
                        writeln_ptx!(
                            self.body,
                            "    neg.f32 {}, {};",
                            neg.fmt_operand(),
                            nc.fmt_operand(),
                        );
                        writeln_ptx!(
                            self.body,
                            "    selp.f32 {out}, {pos}, {neg}, {pred};",
                            out = out.fmt_operand(),
                            pos = nc.fmt_operand(),
                            neg = neg.fmt_operand(),
                            pred = pred.fmt_operand(),
                        );
                        out
                    })
                    .collect();
                Ok(PtxVal::Vec(result))
            }
            MF::ExtractBits => {
                let offset = arg1.ok_or_else(|| {
                    CompileError::NotImplemented("extractBits without arg1".into())
                })?;
                let count = arg2.ok_or_else(|| {
                    CompileError::NotImplemented("extractBits without arg2".into())
                })?;
                let dst = self.alloc_for_scalar(scalar);
                writeln_ptx!(
                    self.body,
                    "    bfe.u32 {}, {}, {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                    offset.fmt_operand(),
                    count.fmt_operand(),
                );
                Ok(dst)
            }
            MF::InsertBits => {
                let insert = arg1.ok_or_else(|| {
                    CompileError::NotImplemented("insertBits without arg1".into())
                })?;
                let offset = arg2.ok_or_else(|| {
                    CompileError::NotImplemented("insertBits without arg2".into())
                })?;
                let dst = self.alloc_for_scalar(scalar);
                writeln_ptx!(
                    self.body,
                    "    bfi.b32 {}, {}, {}, {}, 32;",
                    dst.fmt_operand(),
                    insert.fmt_operand(),
                    arg.fmt_operand(),
                    offset.fmt_operand(),
                );
                Ok(dst)
            }
            MF::Asinh | MF::Acosh | MF::Atanh => self.eval_math_trig(fun, arg, arg1, scalar, ts),
            MF::Modf => {
                // modf returns the fractional part; the integer part goes to the result pointer.
                // In WGSL/naga context, modf(x) → fract(x), trunc is handled separately.
                let dst = self.alloc_for_scalar(scalar);
                let trunc_val = self.alloc_for_scalar(scalar);
                writeln_ptx!(
                    self.body,
                    "    cvt.rzi.{ts}.{ts} {}, {};",
                    trunc_val.fmt_operand(),
                    arg.fmt_operand(),
                );
                writeln_ptx!(
                    self.body,
                    "    sub.{ts} {}, {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                    trunc_val.fmt_operand(),
                );
                Ok(dst)
            }
            MF::Frexp => {
                // frexp(x) → mantissa in [0.5, 1.0), handled as: mantissa = x * 2^(-exp)
                // For PTX: extract exponent bits, normalize. Approximation using lg2.
                let lg2 = self.alloc_for_scalar(scalar);
                let exp_floor = self.alloc_for_scalar(scalar);
                let pow2 = self.alloc_for_scalar(scalar);
                let dst = self.alloc_for_scalar(scalar);
                writeln_ptx!(
                    self.body,
                    "    lg2.approx.{ts} {}, {};",
                    lg2.fmt_operand(),
                    arg.fmt_operand(),
                );
                writeln_ptx!(
                    self.body,
                    "    cvt.rmi.{ts}.{ts} {}, {};",
                    exp_floor.fmt_operand(),
                    lg2.fmt_operand(),
                );
                writeln_ptx!(
                    self.body,
                    "    neg.{ts} {}, {};",
                    pow2.fmt_operand(),
                    exp_floor.fmt_operand(),
                );
                writeln_ptx!(
                    self.body,
                    "    ex2.approx.{ts} {}, {};",
                    pow2.fmt_operand(),
                    pow2.fmt_operand(),
                );
                writeln_ptx!(
                    self.body,
                    "    mul.{ts} {}, {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                    pow2.fmt_operand(),
                );
                Ok(dst)
            }
            MF::Ldexp => {
                // ldexp(x, exp) = x * 2^exp
                let exp_val = super::require_math_arg(arg1, "ldexp", 1)?;
                let pow2 = self.alloc_for_scalar(scalar);
                let dst = self.alloc_for_scalar(scalar);
                writeln_ptx!(
                    self.body,
                    "    ex2.approx.{ts} {}, {};",
                    pow2.fmt_operand(),
                    exp_val.fmt_operand(),
                );
                writeln_ptx!(
                    self.body,
                    "    mul.{ts} {}, {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                    pow2.fmt_operand(),
                );
                Ok(dst)
            }
            _ => Err(CompileError::NotImplemented(
                format!("PTX math function: {fun:?}").into(),
            )),
        }
    }
}
