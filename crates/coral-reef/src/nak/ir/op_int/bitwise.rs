// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! Bitwise and logic integer operations.

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
