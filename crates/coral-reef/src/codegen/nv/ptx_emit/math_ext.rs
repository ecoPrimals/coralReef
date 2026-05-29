// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt::Write as _;

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
                writeln!(
                    self.body,
                    "    mul.rn.f32 {}, {}, {};",
                    dot_acc.fmt_operand(),
                    comps[0].fmt_operand(),
                    comps[0].fmt_operand(),
                )
                .expect("write to String");
                for c in comps.iter().skip(1) {
                    writeln!(
                        self.body,
                        "    fma.rn.f32 {}, {}, {}, {};",
                        dot_acc.fmt_operand(),
                        c.fmt_operand(),
                        c.fmt_operand(),
                        dot_acc.fmt_operand(),
                    )
                    .expect("write to String");
                }
                let dst = self.alloc_r32();
                writeln!(
                    self.body,
                    "    sqrt.rn.f32 {}, {};",
                    dst.fmt_operand(),
                    dot_acc.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Normalize => {
                let PtxVal::Vec(comps) = arg else {
                    let dst = self.alloc_r32();
                    writeln!(
                        self.body,
                        "    mov.f32 {}, 0f3F800000;",
                        dst.fmt_operand(),
                    )
                    .expect("write to String");
                    return Ok(dst);
                };
                let dot_acc = self.alloc_r32();
                writeln!(
                    self.body,
                    "    mul.rn.f32 {}, {}, {};",
                    dot_acc.fmt_operand(),
                    comps[0].fmt_operand(),
                    comps[0].fmt_operand(),
                )
                .expect("write to String");
                for c in comps.iter().skip(1) {
                    writeln!(
                        self.body,
                        "    fma.rn.f32 {}, {}, {}, {};",
                        dot_acc.fmt_operand(),
                        c.fmt_operand(),
                        c.fmt_operand(),
                        dot_acc.fmt_operand(),
                    )
                    .expect("write to String");
                }
                let inv_len = self.alloc_r32();
                writeln!(
                    self.body,
                    "    rsqrt.approx.f32 {}, {};",
                    inv_len.fmt_operand(),
                    dot_acc.fmt_operand(),
                )
                .expect("write to String");
                let result: Vec<PtxVal> = comps
                    .iter()
                    .map(|c| {
                        let d = self.alloc_r32();
                        writeln!(
                            self.body,
                            "    mul.rn.f32 {}, {}, {};",
                            d.fmt_operand(),
                            c.fmt_operand(),
                            inv_len.fmt_operand(),
                        )
                        .expect("write to String");
                        d
                    })
                    .collect();
                Ok(PtxVal::Vec(result))
            }
            MF::Distance => {
                let rhs = arg1.ok_or_else(|| {
                    CompileError::NotImplemented("distance without arg1".into())
                })?;
                let (PtxVal::Vec(lhs_comps), PtxVal::Vec(rhs_comps)) = (arg, rhs) else {
                    return Err(CompileError::NotImplemented(
                        "PTX distance: non-vector operands".into(),
                    ));
                };
                let dot_acc = self.alloc_r32();
                let diff0 = self.alloc_r32();
                writeln!(
                    self.body,
                    "    sub.rn.f32 {}, {}, {};",
                    diff0.fmt_operand(),
                    lhs_comps[0].fmt_operand(),
                    rhs_comps[0].fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    mul.rn.f32 {}, {}, {};",
                    dot_acc.fmt_operand(),
                    diff0.fmt_operand(),
                    diff0.fmt_operand(),
                )
                .expect("write to String");
                for (l, r) in lhs_comps.iter().zip(rhs_comps.iter()).skip(1) {
                    let d = self.alloc_r32();
                    writeln!(
                        self.body,
                        "    sub.rn.f32 {}, {}, {};",
                        d.fmt_operand(),
                        l.fmt_operand(),
                        r.fmt_operand(),
                    )
                    .expect("write to String");
                    writeln!(
                        self.body,
                        "    fma.rn.f32 {}, {}, {}, {};",
                        dot_acc.fmt_operand(),
                        d.fmt_operand(),
                        d.fmt_operand(),
                        dot_acc.fmt_operand(),
                    )
                    .expect("write to String");
                }
                let dst = self.alloc_r32();
                writeln!(
                    self.body,
                    "    sqrt.rn.f32 {}, {};",
                    dst.fmt_operand(),
                    dot_acc.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Cross => {
                let rhs = arg1
                    .ok_or_else(|| CompileError::NotImplemented("cross without arg1".into()))?;
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
                writeln!(
                    self.body,
                    "    mul.rn.f32 {rx}, {a1}, {b2};",
                    rx = rx.fmt_operand(),
                    a1 = a[1].fmt_operand(),
                    b2 = b[2].fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    fma.rn.f32 {rx}, -{a2}, {b1}, {rx};",
                    rx = rx.fmt_operand(),
                    a2 = a[2].fmt_operand(),
                    b1 = b[1].fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    mul.rn.f32 {ry}, {a2}, {b0};",
                    ry = ry.fmt_operand(),
                    a2 = a[2].fmt_operand(),
                    b0 = b[0].fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    fma.rn.f32 {ry}, -{a0}, {b2}, {ry};",
                    ry = ry.fmt_operand(),
                    a0 = a[0].fmt_operand(),
                    b2 = b[2].fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    mul.rn.f32 {rz}, {a0}, {b1};",
                    rz = rz.fmt_operand(),
                    a0 = a[0].fmt_operand(),
                    b1 = b[1].fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    fma.rn.f32 {rz}, -{a1}, {b0}, {rz};",
                    rz = rz.fmt_operand(),
                    a1 = a[1].fmt_operand(),
                    b0 = b[0].fmt_operand(),
                )
                .expect("write to String");
                Ok(PtxVal::Vec(vec![rx, ry, rz]))
            }
            MF::Tan => {
                let sin_dst = self.alloc_for_scalar(scalar);
                let cos_dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    sin.approx.{ts} {}, {};",
                    sin_dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    cos.approx.{ts} {}, {};",
                    cos_dst.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                let dst = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    div.rn.{ts} {}, {}, {};",
                    dst.fmt_operand(),
                    sin_dst.fmt_operand(),
                    cos_dst.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Atan => {
                let dst = self.alloc_for_scalar(scalar);
                let abs_x = self.alloc_for_scalar(scalar);
                let x2 = self.alloc_for_scalar(scalar);
                writeln!(self.body, "    abs.{ts} {}, {};", abs_x.fmt_operand(), arg.fmt_operand())
                    .expect("write to String");
                writeln!(
                    self.body,
                    "    mul.rn.{ts} {}, {}, {};",
                    x2.fmt_operand(),
                    arg.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                // Polynomial approximation: atan(x) ≈ x * (1 - x²/3 + x⁴/5 ...)
                // Use hardware-friendly: atan(x) ≈ x / (1 + 0.28125*x²) for |x|<1
                let denom = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    fma.rn.{ts} {denom}, 0f3E900000, {x2}, 0f3F800000;",
                    denom = denom.fmt_operand(),
                    x2 = x2.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    div.rn.{ts} {}, {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                    denom.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Atan2 => {
                let rhs =
                    arg1.ok_or_else(|| CompileError::NotImplemented("atan2 without arg1".into()))?;
                let dst = self.alloc_for_scalar(scalar);
                let ratio = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    div.rn.{ts} {}, {}, {};",
                    ratio.fmt_operand(),
                    arg.fmt_operand(),
                    rhs.fmt_operand(),
                )
                .expect("write to String");
                let r2 = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    mul.rn.{ts} {}, {}, {};",
                    r2.fmt_operand(),
                    ratio.fmt_operand(),
                    ratio.fmt_operand(),
                )
                .expect("write to String");
                let denom = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    fma.rn.{ts} {denom}, 0f3E900000, {r2}, 0f3F800000;",
                    denom = denom.fmt_operand(),
                    r2 = r2.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    div.rn.{ts} {}, {}, {};",
                    dst.fmt_operand(),
                    ratio.fmt_operand(),
                    denom.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Asin => {
                let dst = self.alloc_for_scalar(scalar);
                let x2 = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    mul.rn.{ts} {}, {}, {};",
                    x2.fmt_operand(),
                    arg.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                let one_minus = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    sub.{ts} {}, 0f3F800000, {};",
                    one_minus.fmt_operand(),
                    x2.fmt_operand(),
                )
                .expect("write to String");
                let inv_sqrt = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    rsqrt.approx.{ts} {}, {};",
                    inv_sqrt.fmt_operand(),
                    one_minus.fmt_operand(),
                )
                .expect("write to String");
                let scaled = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    mul.rn.{ts} {}, {}, {};",
                    scaled.fmt_operand(),
                    arg.fmt_operand(),
                    inv_sqrt.fmt_operand(),
                )
                .expect("write to String");
                // atan approximation on scaled value
                let s2 = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    mul.rn.{ts} {}, {}, {};",
                    s2.fmt_operand(),
                    scaled.fmt_operand(),
                    scaled.fmt_operand(),
                )
                .expect("write to String");
                let denom = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    fma.rn.{ts} {denom}, 0f3E900000, {s2}, 0f3F800000;",
                    denom = denom.fmt_operand(),
                    s2 = s2.fmt_operand(),
                )
                .expect("write to String");
                writeln!(
                    self.body,
                    "    div.rn.{ts} {}, {}, {};",
                    dst.fmt_operand(),
                    scaled.fmt_operand(),
                    denom.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Acos => {
                // acos(x) = π/2 - asin(x)
                let dst = self.alloc_for_scalar(scalar);
                let x2 = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    mul.rn.{ts} {}, {}, {};",
                    x2.fmt_operand(),
                    arg.fmt_operand(),
                    arg.fmt_operand(),
                )
                .expect("write to String");
                let one_minus = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    sub.{ts} {}, 0f3F800000, {};",
                    one_minus.fmt_operand(),
                    x2.fmt_operand(),
                )
                .expect("write to String");
                let inv_sqrt = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    rsqrt.approx.{ts} {}, {};",
                    inv_sqrt.fmt_operand(),
                    one_minus.fmt_operand(),
                )
                .expect("write to String");
                let scaled = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    mul.rn.{ts} {}, {}, {};",
                    scaled.fmt_operand(),
                    arg.fmt_operand(),
                    inv_sqrt.fmt_operand(),
                )
                .expect("write to String");
                let s2 = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    mul.rn.{ts} {}, {}, {};",
                    s2.fmt_operand(),
                    scaled.fmt_operand(),
                    scaled.fmt_operand(),
                )
                .expect("write to String");
                let denom = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    fma.rn.{ts} {denom}, 0f3E900000, {s2}, 0f3F800000;",
                    denom = denom.fmt_operand(),
                    s2 = s2.fmt_operand(),
                )
                .expect("write to String");
                let asin_val = self.alloc_for_scalar(scalar);
                writeln!(
                    self.body,
                    "    div.rn.{ts} {}, {}, {};",
                    asin_val.fmt_operand(),
                    scaled.fmt_operand(),
                    denom.fmt_operand(),
                )
                .expect("write to String");
                // pi/2 = 0x3FC90FDB
                writeln!(
                    self.body,
                    "    sub.{ts} {}, 0f3FC90FDB, {};",
                    dst.fmt_operand(),
                    asin_val.fmt_operand(),
                )
                .expect("write to String");
                Ok(dst)
            }
            MF::Reflect => {
                let rhs = arg1
                    .ok_or_else(|| CompileError::NotImplemented("reflect without arg1".into()))?;
                let (PtxVal::Vec(i_comps), PtxVal::Vec(n_comps)) = (arg, rhs) else {
                    return Err(CompileError::NotImplemented(
                        "PTX reflect: non-vector operands".into(),
                    ));
                };
                // reflect(I, N) = I - 2.0 * dot(N, I) * N
                let dot_acc = self.alloc_r32();
                writeln!(
                    self.body,
                    "    mul.rn.f32 {}, {}, {};",
                    dot_acc.fmt_operand(),
                    n_comps[0].fmt_operand(),
                    i_comps[0].fmt_operand(),
                )
                .expect("write to String");
                for (n, i) in n_comps.iter().zip(i_comps.iter()).skip(1) {
                    writeln!(
                        self.body,
                        "    fma.rn.f32 {}, {}, {}, {};",
                        dot_acc.fmt_operand(),
                        n.fmt_operand(),
                        i.fmt_operand(),
                        dot_acc.fmt_operand(),
                    )
                    .expect("write to String");
                }
                let two_dot = self.alloc_r32();
                writeln!(
                    self.body,
                    "    mul.rn.f32 {}, 0f40000000, {};",
                    two_dot.fmt_operand(),
                    dot_acc.fmt_operand(),
                )
                .expect("write to String");
                let result: Vec<PtxVal> = i_comps
                    .iter()
                    .zip(n_comps.iter())
                    .map(|(i, n)| {
                        let d = self.alloc_r32();
                        writeln!(
                            self.body,
                            "    mul.rn.f32 {tmp}, {two_dot}, {n};",
                            tmp = d.fmt_operand(),
                            two_dot = two_dot.fmt_operand(),
                            n = n.fmt_operand(),
                        )
                        .expect("write to String");
                        writeln!(
                            self.body,
                            "    sub.f32 {d}, {i}, {d};",
                            d = d.fmt_operand(),
                            i = i.fmt_operand(),
                        )
                        .expect("write to String");
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
                writeln!(
                    self.body,
                    "    mul.rn.f32 {}, {}, {};",
                    dot_acc.fmt_operand(),
                    nref_comps[0].fmt_operand(),
                    i_comps[0].fmt_operand(),
                )
                .expect("write to String");
                for (nr, ic) in nref_comps.iter().zip(i_comps.iter()).skip(1) {
                    writeln!(
                        self.body,
                        "    fma.rn.f32 {}, {}, {}, {};",
                        dot_acc.fmt_operand(),
                        nr.fmt_operand(),
                        ic.fmt_operand(),
                        dot_acc.fmt_operand(),
                    )
                    .expect("write to String");
                }
                let pred = self.alloc_pred();
                writeln!(
                    self.body,
                    "    setp.lt.f32 {}, {}, 0f00000000;",
                    pred.fmt_operand(),
                    dot_acc.fmt_operand(),
                )
                .expect("write to String");
                let result: Vec<PtxVal> = n_comps
                    .iter()
                    .map(|nc| {
                        let neg = self.alloc_r32();
                        let out = self.alloc_r32();
                        writeln!(
                            self.body,
                            "    neg.f32 {}, {};",
                            neg.fmt_operand(),
                            nc.fmt_operand(),
                        )
                        .expect("write to String");
                        writeln!(
                            self.body,
                            "    selp.f32 {out}, {pos}, {neg}, {pred};",
                            out = out.fmt_operand(),
                            pos = nc.fmt_operand(),
                            neg = neg.fmt_operand(),
                            pred = pred.fmt_operand(),
                        )
                        .expect("write to String");
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
                writeln!(
                    self.body,
                    "    bfe.u32 {}, {}, {}, {};",
                    dst.fmt_operand(),
                    arg.fmt_operand(),
                    offset.fmt_operand(),
                    count.fmt_operand(),
                )
                .expect("write to String");
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
                writeln!(
                    self.body,
                    "    bfi.b32 {}, {}, {}, {}, 32;",
                    dst.fmt_operand(),
                    insert.fmt_operand(),
                    arg.fmt_operand(),
                    offset.fmt_operand(),
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
