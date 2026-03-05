// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! Integer ALU instruction op structs.

#![allow(clippy::wildcard_imports)]

use super::*;
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpBMsk {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_type(ALU)]
    pub pos: Src,

    #[src_type(ALU)]
    pub width: Src,

    pub wrap: bool,
}

impl DisplayOp for OpBMsk {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let wrap = if self.wrap { ".wrap" } else { ".clamp" };
        write!(f, "bmsk{} {} {}", wrap, self.pos, self.width)
    }
}
impl_display_for_op!(OpBMsk);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpBRev {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_type(ALU)]
    pub src: Src,
}

impl DisplayOp for OpBRev {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "brev {}", self.src)
    }
}
impl_display_for_op!(OpBRev);

/// Bitfield extract. Extracts all bits from `base` starting at `offset` into
/// `dst`.
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpBfe {
    /// Where to insert the bits.
    #[dst_type(GPR)]
    pub dst: Dst,

    /// The source of bits to extract.
    #[src_type(ALU)]
    pub base: Src,

    /// The range of bits to extract. This source is interpreted as four
    /// separate bytes, [b0, b1, b2, b3].
    ///
    /// b0 and b1: unused
    /// b2: the number of bits to extract.
    /// b3: the offset of the first bit to extract.
    ///
    /// This matches the way the hardware works.
    #[src_type(ALU)]
    pub range: Src,

    /// Whether the output is signed
    pub signed: bool,

    /// Whether to reverse the bits before inserting them into `dst`.
    pub reverse: bool,
}

impl DisplayOp for OpBfe {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bfe")?;
        if self.signed {
            write!(f, ".s")?;
        }
        if self.reverse {
            write!(f, ".rev")?;
        }
        write!(f, " {} {}", self.base, self.range,)
    }
}
impl_display_for_op!(OpBfe);

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpFlo {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_type(ALU)]
    pub src: Src,

    pub signed: bool,
    pub return_shift_amount: bool,
}

impl Foldable for OpFlo {
    fn fold(&self, _sm: &ShaderModelInfo, f: &mut OpFoldData<'_>) {
        let src = f.get_u32_src(self, &self.src);
        let leading = if self.signed && (src & 0x8000_0000) != 0 {
            (!src).leading_zeros()
        } else {
            src.leading_zeros()
        };
        let dst = if self.return_shift_amount {
            leading
        } else {
            31 - leading
        };
        f.set_u32_dst(self, &self.dst, dst);
    }
}

impl DisplayOp for OpFlo {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "flo")?;
        if self.return_shift_amount {
            write!(f, ".samt")?;
        }
        write!(f, " {}", self.src)
    }
}
impl_display_for_op!(OpFlo);

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpIAbs {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_type(ALU)]
    pub src: Src,
}

impl Foldable for OpIAbs {
    fn fold(&self, _sm: &ShaderModelInfo, f: &mut OpFoldData<'_>) {
        let src = f.get_u32_src(self, &self.src);
        let dst = (src as i32).unsigned_abs();
        f.set_u32_dst(self, &self.dst, dst);
    }
}

impl DisplayOp for OpIAbs {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "iabs {}", self.src)
    }
}
impl_display_for_op!(OpIAbs);

/// Only used on SM50
#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpIAdd2 {
    #[dst_type(GPR)]
    pub dst: Dst,
    #[dst_type(Carry)]
    pub carry_out: Dst,

    #[src_type(I32)]
    pub srcs: [Src; 2],
}

impl Foldable for OpIAdd2 {
    fn fold(&self, _sm: &ShaderModelInfo, f: &mut OpFoldData<'_>) {
        let srcs = [
            f.get_u32_src(self, &self.srcs[0]),
            f.get_u32_src(self, &self.srcs[1]),
        ];

        let mut sum = 0_u64;
        for i in 0..2 {
            if self.srcs[i].src_mod.is_ineg() {
                // This is a very literal interpretation of 2's compliment.
                // This is not -u64::from(src) or u64::from(-src).
                sum += u64::from(!srcs[i]) + 1;
            } else {
                sum += u64::from(srcs[i]);
            }
        }

        f.set_u32_dst(self, &self.dst, sum as u32);
        f.set_carry_dst(self, &self.carry_out, sum >= (1 << 32));
    }
}

impl DisplayOp for OpIAdd2 {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "iadd2 {} {}", self.srcs[0], self.srcs[1])
    }
}

/// Only used on SM50
#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpIAdd2X {
    #[dst_type(GPR)]
    pub dst: Dst,
    #[dst_type(Carry)]
    pub carry_out: Dst,

    #[src_type(B32)]
    pub srcs: [Src; 2],
    #[src_type(Carry)]
    pub carry_in: Src,
}

impl Foldable for OpIAdd2X {
    fn fold(&self, _sm: &ShaderModelInfo, f: &mut OpFoldData<'_>) {
        let srcs = [
            f.get_u32_bnot_src(self, &self.srcs[0]),
            f.get_u32_bnot_src(self, &self.srcs[1]),
        ];
        let carry_in = f.get_carry_src(self, &self.carry_in);

        let sum = u64::from(srcs[0]) + u64::from(srcs[1]) + u64::from(carry_in);

        f.set_u32_dst(self, &self.dst, sum as u32);
        f.set_carry_dst(self, &self.carry_out, sum >= (1 << 32));
    }
}

impl DisplayOp for OpIAdd2X {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "iadd2.x {} {}", self.srcs[0], self.srcs[1])?;
        if !self.carry_in.is_zero() {
            write!(f, " {}", self.carry_in)?;
        }
        Ok(())
    }
}

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpIAdd3 {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[dst_type(Pred)]
    pub overflow: [Dst; 2],

    #[src_type(I32)]
    pub srcs: [Src; 3],
}

impl Foldable for OpIAdd3 {
    fn fold(&self, _sm: &ShaderModelInfo, f: &mut OpFoldData<'_>) {
        let srcs = [
            f.get_u32_src(self, &self.srcs[0]),
            f.get_u32_src(self, &self.srcs[1]),
            f.get_u32_src(self, &self.srcs[2]),
        ];

        let mut sum = 0_u64;
        for i in 0..3 {
            if self.srcs[i].src_mod.is_ineg() {
                // This is a very literal interpretation of 2's compliment.
                // This is not -u64::from(src) or u64::from(-src).
                sum += u64::from(!srcs[i]) + 1;
            } else {
                sum += u64::from(srcs[i]);
            }
        }

        f.set_u32_dst(self, &self.dst, sum as u32);
        f.set_pred_dst(self, &self.overflow[0], sum >= 1_u64 << 32);
        f.set_pred_dst(self, &self.overflow[1], sum >= 2_u64 << 32);
    }
}

impl DisplayOp for OpIAdd3 {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "iadd3 {} {} {}",
            self.srcs[0], self.srcs[1], self.srcs[2],
        )
    }
}
impl_display_for_op!(OpIAdd3);

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpIAdd3X {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[dst_type(Pred)]
    pub overflow: [Dst; 2],

    #[src_type(B32)]
    pub srcs: [Src; 3],

    #[src_type(Pred)]
    pub carry: [Src; 2],
}

impl Foldable for OpIAdd3X {
    fn fold(&self, _sm: &ShaderModelInfo, f: &mut OpFoldData<'_>) {
        let srcs = [
            f.get_u32_bnot_src(self, &self.srcs[0]),
            f.get_u32_bnot_src(self, &self.srcs[1]),
            f.get_u32_bnot_src(self, &self.srcs[2]),
        ];
        let carry = [
            f.get_pred_src(self, &self.carry[0]),
            f.get_pred_src(self, &self.carry[1]),
        ];

        let mut sum = 0_u64;
        for i in 0..3 {
            sum += u64::from(srcs[i]);
        }

        for i in 0..2 {
            sum += u64::from(carry[i]);
        }

        f.set_u32_dst(self, &self.dst, sum as u32);
        f.set_pred_dst(self, &self.overflow[0], sum >= 1_u64 << 32);
        f.set_pred_dst(self, &self.overflow[1], sum >= 2_u64 << 32);
    }
}

impl DisplayOp for OpIAdd3X {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "iadd3.x {} {} {} {} {}",
            self.srcs[0], self.srcs[1], self.srcs[2], self.carry[0], self.carry[1]
        )
    }
}
impl_display_for_op!(OpIAdd3X);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpIDp4 {
    #[dst_type(GPR)]
    pub dst: Dst,

    pub src_types: [IntType; 2],

    #[src_type(I32)]
    pub srcs: [Src; 3],
}

impl DisplayOp for OpIDp4 {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "idp4{}{} {} {} {}",
            self.src_types[0], self.src_types[1], self.srcs[0], self.srcs[1], self.srcs[2],
        )
    }
}
impl_display_for_op!(OpIDp4);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpIMad {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_type(ALU)]
    pub srcs: [Src; 3],

    pub signed: bool,
}

impl DisplayOp for OpIMad {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "imad {} {} {}", self.srcs[0], self.srcs[1], self.srcs[2],)
    }
}
impl_display_for_op!(OpIMad);

/// Only used on SM50
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpIMul {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_type(ALU)]
    pub srcs: [Src; 2],

    pub signed: [bool; 2],
    pub high: bool,
}

impl DisplayOp for OpIMul {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "imul")?;
        if self.high {
            write!(f, ".hi")?;
        }
        let src_type = |signed| if signed { ".s32" } else { ".u32" };
        write!(
            f,
            "{}{}",
            src_type(self.signed[0]),
            src_type(self.signed[1])
        )?;
        write!(f, " {} {}", self.srcs[0], self.srcs[1])
    }
}

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpIMad64 {
    #[dst_type(Vec)]
    pub dst: Dst,

    #[src_type(ALU)]
    pub srcs: [Src; 3],

    pub signed: bool,
}

impl DisplayOp for OpIMad64 {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "imad64 {} {} {}",
            self.srcs[0], self.srcs[1], self.srcs[2],
        )
    }
}
impl_display_for_op!(OpIMad64);

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpIMnMx {
    #[dst_type(GPR)]
    pub dst: Dst,

    pub cmp_type: IntCmpType,

    #[src_type(ALU)]
    pub srcs: [Src; 2],

    #[src_type(Pred)]
    pub min: Src,
}

impl Foldable for OpIMnMx {
    fn fold(&self, _sm: &ShaderModelInfo, f: &mut OpFoldData<'_>) {
        let (a, b) = (
            f.get_u32_bnot_src(self, &self.srcs[0]),
            f.get_u32_bnot_src(self, &self.srcs[1]),
        );
        let min = f.get_pred_src(self, &self.min);

        let res = match (min, self.cmp_type) {
            (true, IntCmpType::U32) => a.min(b),
            (true, IntCmpType::I32) => (a as i32).min(b as i32) as u32,
            (false, IntCmpType::U32) => a.max(b),
            (false, IntCmpType::I32) => (a as i32).max(b as i32) as u32,
        };

        f.set_u32_dst(self, &self.dst, res);
    }
}

impl DisplayOp for OpIMnMx {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "imnmx{} {} {} {}",
            self.cmp_type, self.srcs[0], self.srcs[1], self.min
        )
    }
}
impl_display_for_op!(OpIMnMx);

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpISetP {
    #[dst_type(Pred)]
    pub dst: Dst,

    pub set_op: PredSetOp,
    pub cmp_op: IntCmpOp,
    pub cmp_type: IntCmpType,
    pub ex: bool,

    #[src_type(ALU)]
    pub srcs: [Src; 2],

    #[src_type(Pred)]
    pub accum: Src,

    #[src_type(Pred)]
    pub low_cmp: Src,
}

impl Foldable for OpISetP {
    fn fold(&self, sm: &ShaderModelInfo, f: &mut OpFoldData<'_>) {
        let x = f.get_u32_src(self, &self.srcs[0]);
        let y = f.get_u32_src(self, &self.srcs[1]);
        let accum = f.get_pred_src(self, &self.accum);
        let low_cmp = f.get_pred_src(self, &self.low_cmp);

        let cmp = if self.cmp_type.is_signed() {
            let x = x as i32;
            let y = y as i32;
            match &self.cmp_op {
                IntCmpOp::False => false,
                IntCmpOp::True => true,
                IntCmpOp::Eq => x == y,
                IntCmpOp::Ne => x != y,
                IntCmpOp::Lt => x < y,
                IntCmpOp::Le => x <= y,
                IntCmpOp::Gt => x > y,
                IntCmpOp::Ge => x >= y,
            }
        } else {
            match &self.cmp_op {
                IntCmpOp::False => false,
                IntCmpOp::True => true,
                IntCmpOp::Eq => x == y,
                IntCmpOp::Ne => x != y,
                IntCmpOp::Lt => x < y,
                IntCmpOp::Le => x <= y,
                IntCmpOp::Gt => x > y,
                IntCmpOp::Ge => x >= y,
            }
        };

        let cmp_op_is_const = matches!(self.cmp_op, IntCmpOp::False | IntCmpOp::True);
        let cmp = if self.ex && x == y && !cmp_op_is_const {
            // Pre-Volta, isetp.x takes the accumulator into account.  If we
            // want to support this, we need to take an an accumulator into
            // account.  Disallow it for now.
            assert!(sm.sm() >= 70);
            low_cmp
        } else {
            cmp
        };

        let dst = self.set_op.eval(cmp, accum);

        f.set_pred_dst(self, &self.dst, dst);
    }
}

impl DisplayOp for OpISetP {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "isetp{}{}", self.cmp_op, self.cmp_type)?;
        if !self.set_op.is_trivial(&self.accum) {
            write!(f, "{}", self.set_op)?;
        }
        if self.ex {
            write!(f, ".ex")?;
        }
        write!(f, " {} {}", self.srcs[0], self.srcs[1])?;
        if !self.set_op.is_trivial(&self.accum) {
            write!(f, " {}", self.accum)?;
        }
        if self.ex {
            write!(f, " {}", self.low_cmp)?;
        }
        Ok(())
    }
}
impl_display_for_op!(OpISetP);

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpLea {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[dst_type(Pred)]
    pub overflow: Dst,

    #[src_type(ALU)]
    pub a: Src,

    #[src_type(I32)]
    pub b: Src,

    #[src_type(ALU)]
    pub a_high: Src, // High 32-bits of a if .dst_high is set

    pub shift: u8,
    pub dst_high: bool,
    pub intermediate_mod: SrcMod, // Modifier for shifted temporary (a << shift)
}

impl Foldable for OpLea {
    fn fold(&self, _sm: &ShaderModelInfo, f: &mut OpFoldData<'_>) {
        let a = f.get_u32_src(self, &self.a);
        let mut b = f.get_u32_src(self, &self.b);
        let a_high = f.get_u32_src(self, &self.a_high);

        let mut overflow = false;

        let mut shift_result = if self.dst_high {
            let a = a as u64;
            let a_high = a_high as u64;
            let a = (a_high << 32) | a;

            (a >> (32 - self.shift)) as u32
        } else {
            a << self.shift
        };

        if self.intermediate_mod.is_ineg() {
            let o;
            (shift_result, o) = u32::overflowing_add(!shift_result, 1);
            overflow |= o;
        }

        if self.b.src_mod.is_ineg() {
            let o;
            (b, o) = u32::overflowing_add(!b, 1);
            overflow |= o;
        }

        let (dst, o) = u32::overflowing_add(shift_result, b);
        overflow |= o;

        f.set_u32_dst(self, &self.dst, dst);
        f.set_pred_dst(self, &self.overflow, overflow);
    }
}

impl DisplayOp for OpLea {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "lea")?;
        if self.dst_high {
            write!(f, ".hi")?;
        }
        write!(f, " {} {} {}", self.a, self.shift, self.b)?;
        if self.dst_high {
            write!(f, " {}", self.a_high)?;
        }
        Ok(())
    }
}
impl_display_for_op!(OpLea);

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpLeaX {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[dst_type(Pred)]
    pub overflow: Dst,

    #[src_type(ALU)]
    pub a: Src,

    #[src_type(B32)]
    pub b: Src,

    #[src_type(ALU)]
    pub a_high: Src, // High 32-bits of a if .dst_high is set

    #[src_type(Pred)]
    pub carry: Src,

    pub shift: u8,
    pub dst_high: bool,
    pub intermediate_mod: SrcMod, // Modifier for shifted temporary (a << shift)
}

impl Foldable for OpLeaX {
    fn fold(&self, _sm: &ShaderModelInfo, f: &mut OpFoldData<'_>) {
        let a = f.get_u32_src(self, &self.a);
        let mut b = f.get_u32_src(self, &self.b);
        let a_high = f.get_u32_src(self, &self.a_high);
        let carry = f.get_pred_src(self, &self.carry);

        let mut overflow = false;

        let mut shift_result = if self.dst_high {
            let a = a as u64;
            let a_high = a_high as u64;
            let a = (a_high << 32) | a;

            (a >> (32 - self.shift)) as u32
        } else {
            a << self.shift
        };

        if self.intermediate_mod.is_bnot() {
            shift_result = !shift_result;
        }

        if self.b.src_mod.is_bnot() {
            b = !b;
        }

        let (dst, o) = u32::overflowing_add(shift_result, b);
        overflow |= o;

        let (dst, o) = u32::overflowing_add(dst, if carry { 1 } else { 0 });
        overflow |= o;

        f.set_u32_dst(self, &self.dst, dst);
        f.set_pred_dst(self, &self.overflow, overflow);
    }
}

impl DisplayOp for OpLeaX {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "lea.x")?;
        if self.dst_high {
            write!(f, ".hi")?;
        }
        write!(f, " {} {} {}", self.a, self.shift, self.b)?;
        if self.dst_high {
            write!(f, " {}", self.a_high)?;
        }
        write!(f, " {}", self.carry)?;
        Ok(())
    }
}
impl_display_for_op!(OpLeaX);

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpLop2 {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_type(B32)]
    pub srcs: [Src; 2],

    pub op: LogicOp2,
}

impl DisplayOp for OpLop2 {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "lop2.{} {} {}", self.op, self.srcs[0], self.srcs[1],)
    }
}

impl Foldable for OpLop2 {
    fn fold(&self, _sm: &ShaderModelInfo, f: &mut OpFoldData<'_>) {
        let srcs = [
            f.get_u32_bnot_src(self, &self.srcs[0]),
            f.get_u32_bnot_src(self, &self.srcs[1]),
        ];
        let dst = match self.op {
            LogicOp2::And => srcs[0] & srcs[1],
            LogicOp2::Or => srcs[0] | srcs[1],
            LogicOp2::Xor => srcs[0] ^ srcs[1],
            LogicOp2::PassB => srcs[1],
        };
        f.set_u32_dst(self, &self.dst, dst);
    }
}

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpLop3 {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_type(ALU)]
    pub srcs: [Src; 3],

    pub op: LogicOp3,
}

impl Foldable for OpLop3 {
    fn fold(&self, _sm: &ShaderModelInfo, f: &mut OpFoldData<'_>) {
        let srcs = [
            f.get_u32_bnot_src(self, &self.srcs[0]),
            f.get_u32_bnot_src(self, &self.srcs[1]),
            f.get_u32_bnot_src(self, &self.srcs[2]),
        ];
        let dst = self.op.eval(srcs[0], srcs[1], srcs[2]);
        f.set_u32_dst(self, &self.dst, dst);
    }
}

impl DisplayOp for OpLop3 {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "lop3.{} {} {} {}",
            self.op, self.srcs[0], self.srcs[1], self.srcs[2],
        )
    }
}
impl_display_for_op!(OpLop3);

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum ShflOp {
    Idx,
    Up,
    Down,
    Bfly,
}

impl fmt::Display for ShflOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShflOp::Idx => write!(f, "idx"),
            ShflOp::Up => write!(f, "up"),
            ShflOp::Down => write!(f, "down"),
            ShflOp::Bfly => write!(f, "bfly"),
        }
    }
}

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpShf {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_type(GPR)]
    pub low: Src,

    #[src_type(ALU)]
    pub high: Src,

    #[src_type(ALU)]
    pub shift: Src,

    pub right: bool,
    pub wrap: bool,
    pub data_type: IntType,
    pub dst_high: bool,
}

fn reduce_shift_imm(shift: &mut Src, wrap: bool, bits: u32) {
    debug_assert!(shift.src_mod.is_none());
    if let SrcRef::Imm32(shift) = &mut shift.src_ref {
        if wrap {
            *shift &= bits - 1;
        } else {
            *shift = std::cmp::min(*shift, bits);
        }
    }
}

impl OpShf {
    /// Reduces the shift immediate, if any.  Out-of-range shifts are either
    /// clamped to the maximum or wrapped as needed.
    pub fn reduce_shift_imm(&mut self) {
        let bits = self.data_type.bits().try_into().unwrap();
        reduce_shift_imm(&mut self.shift, self.wrap, bits);
    }
}

impl Foldable for OpShf {
    fn fold(&self, sm: &ShaderModelInfo, f: &mut OpFoldData<'_>) {
        let low = f.get_u32_src(self, &self.low);
        let high = f.get_u32_src(self, &self.high);
        let shift = f.get_u32_src(self, &self.shift);

        let bits: u32 = self.data_type.bits().try_into().unwrap();
        let shift = if self.wrap {
            shift & (bits - 1)
        } else {
            min(shift, bits)
        };

        let x = u64::from(low) | (u64::from(high) << 32);
        let shifted = if sm.sm() < 70 && self.dst_high && self.data_type != IntType::I64 {
            if self.right {
                x.checked_shr(shift).unwrap_or(0)
            } else {
                x.checked_shl(shift).unwrap_or(0)
            }
        } else if self.data_type.is_signed() {
            if self.right {
                let x = x as i64;
                x.checked_shr(shift).unwrap_or(x >> 63) as u64
            } else {
                x.checked_shl(shift).unwrap_or(0)
            }
        } else {
            if self.right {
                x.checked_shr(shift).unwrap_or(0)
            } else {
                x.checked_shl(shift).unwrap_or(0)
            }
        };

        let dst = if (sm.sm() < 70 && !self.right) || self.dst_high {
            (shifted >> 32) as u32
        } else {
            shifted as u32
        };

        f.set_u32_dst(self, &self.dst, dst);
    }
}

impl DisplayOp for OpShf {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "shf")?;
        if self.right {
            write!(f, ".r")?;
        } else {
            write!(f, ".l")?;
        }
        if self.wrap {
            write!(f, ".w")?;
        }
        write!(f, "{}", self.data_type)?;
        if self.dst_high {
            write!(f, ".hi")?;
        }
        write!(f, " {} {} {}", self.low, self.high, self.shift)
    }
}
impl_display_for_op!(OpShf);

/// Only used on SM50
#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpShl {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_type(GPR)]
    pub src: Src,

    #[src_type(ALU)]
    pub shift: Src,

    pub wrap: bool,
}

impl OpShl {
    /// Reduces the shift immediate, if any.  Out-of-range shifts are either
    /// clamped to the maximum or wrapped as needed.
    pub fn reduce_shift_imm(&mut self) {
        reduce_shift_imm(&mut self.shift, self.wrap, 32);
    }
}

impl DisplayOp for OpShl {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "shl")?;
        if self.wrap {
            write!(f, ".w")?;
        }
        write!(f, " {} {}", self.src, self.shift)
    }
}

impl Foldable for OpShl {
    fn fold(&self, _sm: &ShaderModelInfo, f: &mut OpFoldData<'_>) {
        let x = f.get_u32_src(self, &self.src);
        let shift = f.get_u32_src(self, &self.shift);

        let shift = if self.wrap {
            shift & 31
        } else {
            min(shift, 32)
        };
        let dst = x.checked_shl(shift).unwrap_or(0);
        f.set_u32_dst(self, &self.dst, dst);
    }
}

/// Only used on SM50
#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpShr {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_type(GPR)]
    pub src: Src,

    #[src_type(ALU)]
    pub shift: Src,

    pub wrap: bool,
    pub signed: bool,
}

impl DisplayOp for OpShr {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "shr")?;
        if self.wrap {
            write!(f, ".w")?;
        }
        if !self.signed {
            write!(f, ".u32")?;
        }
        write!(f, " {} {}", self.src, self.shift)
    }
}

impl OpShr {
    /// Reduces the shift immediate, if any.  Out-of-range shifts are either
    /// clamped to the maximum or wrapped as needed.
    pub fn reduce_shift_imm(&mut self) {
        reduce_shift_imm(&mut self.shift, self.wrap, 32);
    }
}

impl Foldable for OpShr {
    fn fold(&self, _sm: &ShaderModelInfo, f: &mut OpFoldData<'_>) {
        let x = f.get_u32_src(self, &self.src);
        let shift = f.get_u32_src(self, &self.shift);

        let shift = if self.wrap {
            shift & 31
        } else {
            min(shift, 32)
        };
        let dst = if self.signed {
            let x = x as i32;
            x.checked_shr(shift).unwrap_or(x >> 31) as u32
        } else {
            x.checked_shr(shift).unwrap_or(0)
        };
        f.set_u32_dst(self, &self.dst, dst);
    }
}
