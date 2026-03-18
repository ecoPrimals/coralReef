// SPDX-License-Identifier: AGPL-3.0-only

use super::super::*;

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpFAdd {
    #[dst_type(F32)]
    pub dst: Dst,

    #[src_type(F32)]
    pub srcs: [Src; 2],

    pub saturate: bool,
    pub rnd_mode: FRndMode,
    pub ftz: bool,
}

impl DisplayOp for OpFAdd {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sat = if self.saturate { ".sat" } else { "" };
        write!(f, "fadd{sat}")?;
        if self.rnd_mode != FRndMode::NearestEven {
            write!(f, "{}", self.rnd_mode)?;
        }
        if self.ftz {
            write!(f, ".ftz")?;
        }
        write!(f, " {} {}", self.srcs[0], self.srcs[1],)
    }
}
impl_display_for_op!(OpFAdd);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpFFma {
    #[dst_type(F32)]
    pub dst: Dst,

    #[src_type(F32)]
    pub srcs: [Src; 3],

    pub saturate: bool,
    pub rnd_mode: FRndMode,
    pub ftz: bool,
    pub dnz: bool,
}

impl DisplayOp for OpFFma {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sat = if self.saturate { ".sat" } else { "" };
        write!(f, "ffma{sat}")?;
        if self.rnd_mode != FRndMode::NearestEven {
            write!(f, "{}", self.rnd_mode)?;
        }
        if self.dnz {
            write!(f, ".dnz")?;
        } else if self.ftz {
            write!(f, ".ftz")?;
        }
        write!(f, " {} {} {}", self.srcs[0], self.srcs[1], self.srcs[2])
    }
}
impl_display_for_op!(OpFFma);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpFMnMx {
    #[dst_type(F32)]
    pub dst: Dst,

    #[src_types(F32, F32, Pred)]
    #[src_names(src_a, src_b, min)]
    pub srcs: [Src; 3],

    pub ftz: bool,
}

impl DisplayOp for OpFMnMx {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ftz = if self.ftz { ".ftz" } else { "" };
        write!(
            f,
            "fmnmx{ftz} {} {} {}",
            self.srcs[0],
            self.srcs[1],
            self.min()
        )
    }
}
impl_display_for_op!(OpFMnMx);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpFMul {
    #[dst_type(F32)]
    pub dst: Dst,

    #[src_type(F32)]
    pub srcs: [Src; 2],

    pub saturate: bool,
    pub rnd_mode: FRndMode,
    pub ftz: bool,
    pub dnz: bool,
}

impl DisplayOp for OpFMul {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sat = if self.saturate { ".sat" } else { "" };
        write!(f, "fmul{sat}")?;
        if self.rnd_mode != FRndMode::NearestEven {
            write!(f, "{}", self.rnd_mode)?;
        }
        if self.dnz {
            write!(f, ".dnz")?;
        } else if self.ftz {
            write!(f, ".ftz")?;
        }
        write!(f, " {} {}", self.srcs[0], self.srcs[1],)
    }
}
impl_display_for_op!(OpFMul);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpFSet {
    #[dst_type(F32)]
    pub dst: Dst,

    pub cmp_op: FloatCmpOp,

    #[src_type(F32)]
    pub srcs: [Src; 2],

    pub ftz: bool,
}

impl DisplayOp for OpFSet {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ftz = if self.ftz { ".ftz" } else { "" };
        write!(
            f,
            "fset{}{ftz} {} {}",
            self.cmp_op, self.srcs[0], self.srcs[1]
        )
    }
}
impl_display_for_op!(OpFSet);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpFSetP {
    #[dst_type(Pred)]
    pub dst: Dst,

    pub set_op: PredSetOp,
    pub cmp_op: FloatCmpOp,

    #[src_types(F32, F32, Pred)]
    #[src_names(src_a, src_b, accum)]
    pub srcs: [Src; 3],

    pub ftz: bool,
}

impl DisplayOp for OpFSetP {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ftz = if self.ftz { ".ftz" } else { "" };
        write!(f, "fsetp{}{ftz}", self.cmp_op)?;
        if !self.set_op.is_trivial(self.accum()) {
            write!(f, "{}", self.set_op)?;
        }
        write!(f, " {} {}", self.srcs[0], self.srcs[1])?;
        if !self.set_op.is_trivial(self.accum()) {
            write!(f, " {}", self.accum())?;
        }
        Ok(())
    }
}
impl_display_for_op!(OpFSetP);

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum FSwzAddOp {
    Add,
    SubRight,
    SubLeft,
    MoveLeft,
}

impl fmt::Display for FSwzAddOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Add => write!(f, "add"),
            Self::SubRight => write!(f, "subr"),
            Self::SubLeft => write!(f, "sub"),
            Self::MoveLeft => write!(f, "mov2"),
        }
    }
}

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpFSwzAdd {
    #[dst_type(F32)]
    pub dst: Dst,

    #[src_type(GPR)]
    pub srcs: [Src; 2],

    pub rnd_mode: FRndMode,
    pub ftz: bool,
    pub deriv_mode: TexDerivMode,

    pub ops: [FSwzAddOp; 4],
}

impl DisplayOp for OpFSwzAdd {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fswzadd",)?;
        if self.rnd_mode != FRndMode::NearestEven {
            write!(f, "{}", self.rnd_mode)?;
        }
        if self.ftz {
            write!(f, ".ftz")?;
        }
        write!(f, "{}", self.deriv_mode)?;
        write!(
            f,
            " {} {} [{}, {}, {}, {}]",
            self.srcs[0], self.srcs[1], self.ops[0], self.ops[1], self.ops[2], self.ops[3],
        )
    }
}
impl_display_for_op!(OpFSwzAdd);

/// Describes where the second src is taken before doing the ops
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum FSwzShuffle {
    Quad0,
    Quad1,
    Quad2,
    Quad3,
    // swap [0, 1] and [2, 3]
    SwapHorizontal,
    // swap [0, 2] and [1, 3]
    SwapVertical,
}

impl fmt::Display for FSwzShuffle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Quad0 => write!(f, ".0000"),
            Self::Quad1 => write!(f, ".1111"),
            Self::Quad2 => write!(f, ".2222"),
            Self::Quad3 => write!(f, ".3333"),
            Self::SwapHorizontal => write!(f, ".1032"),
            Self::SwapVertical => write!(f, ".2301"),
        }
    }
}

/// Op only present in Kepler and older
/// It first does a shuffle on the second src and then applies
/// src0 op src1, each thread on a quad might do a different operation.
///
/// This is used to encode ddx/ddy
/// ex: ddx
///   src1 = shuffle swap horizontal src1
///   ops = [sub, subr, sub, subr]
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpFSwz {
    #[dst_type(F32)]
    pub dst: Dst,

    #[src_type(GPR)]
    pub srcs: [Src; 2],

    pub rnd_mode: FRndMode,
    pub ftz: bool,
    pub deriv_mode: TexDerivMode,
    pub shuffle: FSwzShuffle,

    pub ops: [FSwzAddOp; 4],
}

impl DisplayOp for OpFSwz {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fswz{}", self.shuffle)?;
        if self.rnd_mode != FRndMode::NearestEven {
            write!(f, "{}", self.rnd_mode)?;
        }
        write!(f, "{}", self.deriv_mode)?;
        if self.ftz {
            write!(f, ".ftz")?;
        }
        write!(
            f,
            " {} {} [{}, {}, {}, {}]",
            self.srcs[0], self.srcs[1], self.ops[0], self.ops[1], self.ops[2], self.ops[3],
        )
    }
}
impl_display_for_op!(OpFSwz);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RroOp {
    SinCos,
    Exp2,
}

impl fmt::Display for RroOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SinCos => write!(f, ".sincos"),
            Self::Exp2 => write!(f, ".exp2"),
        }
    }
}

/// MuFu range reduction operator
///
/// Not available on SM70+
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpRro {
    #[dst_type(F32)]
    pub dst: Dst,

    pub op: RroOp,

    #[src_type(F32)]
    pub src: Src,
}

impl DisplayOp for OpRro {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "rro{} {}", self.op, self.src)
    }
}
impl_display_for_op!(OpRro);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TranscendentalOp {
    Cos,
    Sin,
    Exp2,
    Log2,
    Rcp,
    Rsq,
    Rcp64H,
    Rsq64H,
    Sqrt,
    Tanh,
}

impl fmt::Display for TranscendentalOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cos => write!(f, "cos"),
            Self::Sin => write!(f, "sin"),
            Self::Exp2 => write!(f, "exp2"),
            Self::Log2 => write!(f, "log2"),
            Self::Rcp => write!(f, "rcp"),
            Self::Rsq => write!(f, "rsq"),
            Self::Rcp64H => write!(f, "rcp64h"),
            Self::Rsq64H => write!(f, "rsq64h"),
            Self::Sqrt => write!(f, "sqrt"),
            Self::Tanh => write!(f, "tanh"),
        }
    }
}

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpTranscendental {
    #[dst_type(F32)]
    pub dst: Dst,

    pub op: TranscendentalOp,

    #[src_type(F32)]
    pub src: Src,
}

impl DisplayOp for OpTranscendental {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "transcendental.{} {}", self.op, self.src)
    }
}
impl_display_for_op!(OpTranscendental);
