// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! Bitwise and logic integer operations.

use super::*;
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpBMsk {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_types(ALU, ALU)]
    #[src_names(pos, width)]
    pub srcs: [Src; 2],

    pub wrap: bool,
}

impl DisplayOp for OpBMsk {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let wrap = if self.wrap { ".wrap" } else { ".clamp" };
        write!(f, "bmsk{} {} {}", wrap, self.pos(), self.width())
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

    /// [base, range]: The source of bits to extract, and the range.
    /// The range source is interpreted as four
    /// separate bytes, [b0, b1, b2, b3].
    ///
    /// b0 and b1: unused
    /// b2: the number of bits to extract.
    /// b3: the offset of the first bit to extract.
    ///
    /// This matches the way the hardware works.
    #[src_types(ALU, ALU)]
    #[src_names(base, range)]
    pub srcs: [Src; 2],

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
        write!(f, " {} {}", self.base(), self.range())
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
    fn fold(&self, _sm: &dyn ShaderModel, f: &mut OpFoldData<'_>) {
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
    fn fold(&self, _sm: &dyn ShaderModel, f: &mut OpFoldData<'_>) {
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
    fn fold(&self, _sm: &dyn ShaderModel, f: &mut OpFoldData<'_>) {
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
            Self::Idx => write!(f, "idx"),
            Self::Up => write!(f, "up"),
            Self::Down => write!(f, "down"),
            Self::Bfly => write!(f, "bfly"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zero_src() -> Src {
        Src::ZERO
    }

    fn imm_src(u: u32) -> Src {
        Src::new_imm_u32(u)
    }

    #[test]
    fn test_op_bmsk_display() {
        let op = OpBMsk {
            dst: Dst::None,
            srcs: [imm_src(0), imm_src(8)],
            wrap: true,
        };
        let s = format!("{op}");
        assert!(s.contains("bmsk"));
        assert!(s.contains(".wrap"));
    }

    #[test]
    fn test_op_bmsk_clamp() {
        let op = OpBMsk {
            dst: Dst::None,
            srcs: [zero_src(), imm_src(16)],
            wrap: false,
        };
        let s = format!("{op}");
        assert!(s.contains(".clamp"));
    }

    #[test]
    fn test_op_brev_display() {
        let op = OpBRev {
            dst: Dst::None,
            src: imm_src(0x1234_5678),
        };
        let s = format!("{op}");
        assert!(s.contains("brev"));
        assert!(s.contains("0x12345678"));
    }

    #[test]
    fn test_op_bfe_display() {
        let op = OpBfe {
            dst: Dst::None,
            srcs: [imm_src(0xff), imm_src(0)],
            signed: true,
            reverse: true,
        };
        let s = format!("{op}");
        assert!(s.contains("bfe"));
        assert!(s.contains(".s"));
        assert!(s.contains(".rev"));
    }

    #[test]
    fn test_op_flo_display() {
        let op = OpFlo {
            dst: Dst::None,
            src: zero_src(),
            signed: false,
            return_shift_amount: false,
        };
        let s = format!("{op}");
        assert!(s.contains("flo"));
    }

    #[test]
    fn test_op_flo_samt() {
        let op = OpFlo {
            dst: Dst::None,
            src: imm_src(1),
            signed: true,
            return_shift_amount: true,
        };
        let s = format!("{op}");
        assert!(s.contains("flo"));
        assert!(s.contains(".samt"));
    }

    #[test]
    fn test_op_lop3_display() {
        let op = OpLop3 {
            dst: Dst::None,
            srcs: [zero_src(), imm_src(1), imm_src(2)],
            op: LogicOp2::And.to_lut(),
        };
        let s = format!("{op}");
        assert!(s.contains("lop3"));
        assert!(s.contains("LUT"));
    }

    #[test]
    fn test_shfl_op_display() {
        assert_eq!(format!("{}", ShflOp::Idx), "idx");
        assert_eq!(format!("{}", ShflOp::Up), "up");
        assert_eq!(format!("{}", ShflOp::Down), "down");
        assert_eq!(format!("{}", ShflOp::Bfly), "bfly");
    }
}
