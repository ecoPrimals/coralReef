// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! Float, double, and half-precision ALU instruction op structs.

#![allow(clippy::wildcard_imports)]

use super::*;
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

    #[src_type(F32)]
    pub srcs: [Src; 2],

    #[src_type(Pred)]
    pub min: Src,

    pub ftz: bool,
}

impl DisplayOp for OpFMnMx {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ftz = if self.ftz { ".ftz" } else { "" };
        write!(
            f,
            "fmnmx{ftz} {} {} {}",
            self.srcs[0], self.srcs[1], self.min
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

    #[src_type(F32)]
    pub srcs: [Src; 2],

    #[src_type(Pred)]
    pub accum: Src,

    pub ftz: bool,
}

impl DisplayOp for OpFSetP {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ftz = if self.ftz { ".ftz" } else { "" };
        write!(f, "fsetp{}{ftz}", self.cmp_op)?;
        if !self.set_op.is_trivial(&self.accum) {
            write!(f, "{}", self.set_op)?;
        }
        write!(f, " {} {}", self.srcs[0], self.srcs[1])?;
        if !self.set_op.is_trivial(&self.accum) {
            write!(f, " {}", self.accum)?;
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

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpDAdd {
    #[dst_type(F64)]
    pub dst: Dst,

    #[src_type(F64)]
    pub srcs: [Src; 2],

    pub rnd_mode: FRndMode,
}

impl DisplayOp for OpDAdd {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "dadd")?;
        if self.rnd_mode != FRndMode::NearestEven {
            write!(f, "{}", self.rnd_mode)?;
        }
        write!(f, " {} {}", self.srcs[0], self.srcs[1],)
    }
}
impl_display_for_op!(OpDAdd);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpDMul {
    #[dst_type(F64)]
    pub dst: Dst,

    #[src_type(F64)]
    pub srcs: [Src; 2],

    pub rnd_mode: FRndMode,
}

impl DisplayOp for OpDMul {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "dmul")?;
        if self.rnd_mode != FRndMode::NearestEven {
            write!(f, "{}", self.rnd_mode)?;
        }
        write!(f, " {} {}", self.srcs[0], self.srcs[1],)
    }
}
impl_display_for_op!(OpDMul);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpDFma {
    #[dst_type(F64)]
    pub dst: Dst,

    #[src_type(F64)]
    pub srcs: [Src; 3],

    pub rnd_mode: FRndMode,
}

impl DisplayOp for OpDFma {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "dfma")?;
        if self.rnd_mode != FRndMode::NearestEven {
            write!(f, "{}", self.rnd_mode)?;
        }
        write!(f, " {} {} {}", self.srcs[0], self.srcs[1], self.srcs[2])
    }
}
impl_display_for_op!(OpDFma);

/// Placeholder op for f64 sqrt. Lowered to Newton-Raphson via MUFU.Rsq64H + DFMA.
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpF64Sqrt {
    #[dst_type(F64)]
    pub dst: Dst,

    #[src_type(F64)]
    pub src: Src,
}

impl DisplayOp for OpF64Sqrt {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "f64sqrt {}", self.src)
    }
}
impl_display_for_op!(OpF64Sqrt);

/// Placeholder op for f64 reciprocal. Lowered to Newton-Raphson via MUFU.RCP64H + DFMA.
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpF64Rcp {
    #[dst_type(F64)]
    pub dst: Dst,

    #[src_type(F64)]
    pub src: Src,
}

impl DisplayOp for OpF64Rcp {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "f64rcp {}", self.src)
    }
}
impl_display_for_op!(OpF64Rcp);

/// Placeholder op for f64 exp2. Lowered to Horner polynomial via DFMA.
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpF64Exp2 {
    #[dst_type(F64)]
    pub dst: Dst,

    #[src_type(F64)]
    pub src: Src,
}

impl DisplayOp for OpF64Exp2 {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "f64exp2 {}", self.src)
    }
}
impl_display_for_op!(OpF64Exp2);

/// Placeholder op for f64 log2. Lowered to MUFU.LOG2 f32 seed extended to f64.
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpF64Log2 {
    #[dst_type(F64)]
    pub dst: Dst,

    #[src_type(F64)]
    pub src: Src,
}

impl DisplayOp for OpF64Log2 {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "f64log2 {}", self.src)
    }
}
impl_display_for_op!(OpF64Log2);

/// Placeholder op for f64 sin. Lowered to minimax polynomial.
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpF64Sin {
    #[dst_type(F64)]
    pub dst: Dst,

    #[src_type(F64)]
    pub src: Src,
}

impl DisplayOp for OpF64Sin {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "f64sin {}", self.src)
    }
}
impl_display_for_op!(OpF64Sin);

/// Placeholder op for f64 cos. Lowered to minimax polynomial.
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpF64Cos {
    #[dst_type(F64)]
    pub dst: Dst,

    #[src_type(F64)]
    pub src: Src,
}

impl DisplayOp for OpF64Cos {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "f64cos {}", self.src)
    }
}
impl_display_for_op!(OpF64Cos);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpDMnMx {
    #[dst_type(F64)]
    pub dst: Dst,

    #[src_type(F64)]
    pub srcs: [Src; 2],

    #[src_type(Pred)]
    pub min: Src,
}

impl DisplayOp for OpDMnMx {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "dmnmx {} {} {}", self.srcs[0], self.srcs[1], self.min)
    }
}
impl_display_for_op!(OpDMnMx);

#[repr(C)]
#[derive(Clone, SrcsAsSlice, DstsAsSlice)]
pub struct OpDSetP {
    #[dst_type(Pred)]
    pub dst: Dst,

    pub set_op: PredSetOp,
    pub cmp_op: FloatCmpOp,

    #[src_type(F64)]
    pub srcs: [Src; 2],

    #[src_type(Pred)]
    pub accum: Src,
}

impl Foldable for OpDSetP {
    fn fold(&self, _sm: &ShaderModelInfo, f: &mut OpFoldData<'_>) {
        let a = f.get_f64_src(self, &self.srcs[0]);
        let b = f.get_f64_src(self, &self.srcs[1]);
        let accum = f.get_pred_src(self, &self.accum);

        let ordered = !a.is_nan() && !b.is_nan();
        let cmp_res = match self.cmp_op {
            FloatCmpOp::OrdEq => ordered && a == b,
            FloatCmpOp::OrdNe => ordered && a != b,
            FloatCmpOp::OrdLt => ordered && a < b,
            FloatCmpOp::OrdLe => ordered && a <= b,
            FloatCmpOp::OrdGt => ordered && a > b,
            FloatCmpOp::OrdGe => ordered && a >= b,
            FloatCmpOp::UnordEq => !ordered || a == b,
            FloatCmpOp::UnordNe => !ordered || a != b,
            FloatCmpOp::UnordLt => !ordered || a < b,
            FloatCmpOp::UnordLe => !ordered || a <= b,
            FloatCmpOp::UnordGt => !ordered || a > b,
            FloatCmpOp::UnordGe => !ordered || a >= b,
            FloatCmpOp::IsNum => ordered,
            FloatCmpOp::IsNan => !ordered,
        };
        let res = self.set_op.eval(cmp_res, accum);

        f.set_pred_dst(self, &self.dst, res);
    }
}

impl DisplayOp for OpDSetP {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "dsetp{}", self.cmp_op)?;
        if !self.set_op.is_trivial(&self.accum) {
            write!(f, "{}", self.set_op)?;
        }
        write!(f, " {} {}", self.srcs[0], self.srcs[1])?;
        if !self.set_op.is_trivial(&self.accum) {
            write!(f, " {}", self.accum)?;
        }
        Ok(())
    }
}
impl_display_for_op!(OpDSetP);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpHAdd2 {
    #[dst_type(F16v2)]
    pub dst: Dst,

    #[src_type(F16v2)]
    pub srcs: [Src; 2],

    pub saturate: bool,
    pub ftz: bool,
    pub f32: bool,
}

impl DisplayOp for OpHAdd2 {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sat = if self.saturate { ".sat" } else { "" };
        let f32 = if self.f32 { ".f32" } else { "" };
        write!(f, "hadd2{sat}{f32}")?;
        if self.ftz {
            write!(f, ".ftz")?;
        }
        write!(f, " {} {}", self.srcs[0], self.srcs[1])
    }
}
impl_display_for_op!(OpHAdd2);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpHSet2 {
    #[dst_type(F16v2)]
    pub dst: Dst,

    pub set_op: PredSetOp,
    pub cmp_op: FloatCmpOp,

    #[src_type(F16v2)]
    pub srcs: [Src; 2],

    #[src_type(Pred)]
    pub accum: Src,

    pub ftz: bool,
}

impl DisplayOp for OpHSet2 {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ftz = if self.ftz { ".ftz" } else { "" };
        write!(f, "hset2{}{ftz}", self.cmp_op)?;
        if !self.set_op.is_trivial(&self.accum) {
            write!(f, "{}", self.set_op)?;
        }
        write!(f, " {} {}", self.srcs[0], self.srcs[1])?;
        if !self.set_op.is_trivial(&self.accum) {
            write!(f, " {}", self.accum)?;
        }
        Ok(())
    }
}
impl_display_for_op!(OpHSet2);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpHSetP2 {
    #[dst_type(Pred)]
    pub dsts: [Dst; 2],

    pub set_op: PredSetOp,
    pub cmp_op: FloatCmpOp,

    #[src_type(F16v2)]
    pub srcs: [Src; 2],

    #[src_type(Pred)]
    pub accum: Src,

    pub ftz: bool,

    // When not set, each dsts get the result of each lanes.
    // When set, the first dst gets the result of both lanes (res0 && res1)
    // and the second dst gets the negation !(res0 && res1)
    // before applying the accumulator.
    pub horizontal: bool,
}

impl DisplayOp for OpHSetP2 {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ftz = if self.ftz { ".ftz" } else { "" };
        write!(f, "hsetp2{}{ftz}", self.cmp_op)?;
        if !self.set_op.is_trivial(&self.accum) {
            write!(f, "{}", self.set_op)?;
        }
        write!(f, " {} {}", self.srcs[0], self.srcs[1])?;
        if !self.set_op.is_trivial(&self.accum) {
            write!(f, " {}", self.accum)?;
        }
        Ok(())
    }
}
impl_display_for_op!(OpHSetP2);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpHMul2 {
    #[dst_type(F16v2)]
    pub dst: Dst,

    #[src_type(F16v2)]
    pub srcs: [Src; 2],

    pub saturate: bool,
    pub ftz: bool,
    pub dnz: bool,
}

impl DisplayOp for OpHMul2 {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sat = if self.saturate { ".sat" } else { "" };
        write!(f, "hmul2{sat}")?;
        if self.dnz {
            write!(f, ".dnz")?;
        } else if self.ftz {
            write!(f, ".ftz")?;
        }
        write!(f, " {} {}", self.srcs[0], self.srcs[1])
    }
}
impl_display_for_op!(OpHMul2);

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum ImmaSize {
    M8N8K16,
    M8N8K32,
    M16N8K16,
    M16N8K32,
    M16N8K64,
}

impl fmt::Display for ImmaSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::M8N8K16 => write!(f, ".m8n8k16"),
            Self::M8N8K32 => write!(f, ".m8n8k32"),
            Self::M16N8K16 => write!(f, ".m16n8k16"),
            Self::M16N8K32 => write!(f, ".m16n8k32"),
            Self::M16N8K64 => write!(f, ".m16n8k64"),
        }
    }
}

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpImma {
    #[dst_type(Vec)]
    pub dst: Dst,

    pub mat_size: ImmaSize,
    pub src_types: [IntType; 2],
    pub saturate: bool,

    #[src_type(SSA)]
    pub srcs: [Src; 3],
}

impl DisplayOp for OpImma {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sat = if self.saturate { ".sat" } else { "" };
        write!(
            f,
            "imma{}{}{}{sat} {} {} {}",
            self.mat_size,
            self.src_types[0],
            self.src_types[1],
            self.srcs[0],
            self.srcs[1],
            self.srcs[2],
        )
    }
}

impl_display_for_op!(OpImma);

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum HmmaSize {
    M16N8K16,
    M16N8K8,
    M16N8K4,
}

impl fmt::Display for HmmaSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::M16N8K16 => write!(f, ".m16n8k16"),
            Self::M16N8K8 => write!(f, ".m16n8k8"),
            Self::M16N8K4 => write!(f, ".m16n8k4"),
        }
    }
}

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpHmma {
    #[dst_type(Vec)]
    pub dst: Dst,

    pub mat_size: HmmaSize,
    pub src_type: FloatType,
    pub dst_type: FloatType,

    #[src_type(SSA)]
    pub srcs: [Src; 3],
}

impl DisplayOp for OpHmma {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "hmma{}{} {} {} {}",
            self.mat_size, self.dst_type, self.srcs[0], self.srcs[1], self.srcs[2],
        )
    }
}

impl_display_for_op!(OpHmma);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpHFma2 {
    #[dst_type(F16v2)]
    pub dst: Dst,

    #[src_type(F16v2)]
    pub srcs: [Src; 3],

    pub saturate: bool,
    pub ftz: bool,
    pub dnz: bool,
    pub f32: bool,
}

impl DisplayOp for OpHFma2 {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sat = if self.saturate { ".sat" } else { "" };
        let f32 = if self.f32 { ".f32" } else { "" };
        write!(f, "hfma2{sat}{f32}")?;
        if self.dnz {
            write!(f, ".dnz")?;
        } else if self.ftz {
            write!(f, ".ftz")?;
        }
        write!(f, " {} {} {}", self.srcs[0], self.srcs[1], self.srcs[2])
    }
}
impl_display_for_op!(OpHFma2);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpHMnMx2 {
    #[dst_type(F16v2)]
    pub dst: Dst,

    #[src_type(F16v2)]
    pub srcs: [Src; 2],

    #[src_type(Pred)]
    pub min: Src,

    pub ftz: bool,
}

impl DisplayOp for OpHMnMx2 {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ftz = if self.ftz { ".ftz" } else { "" };
        write!(
            f,
            "hmnmx2{ftz} {} {} {}",
            self.srcs[0], self.srcs[1], self.min
        )
    }
}
impl_display_for_op!(OpHMnMx2);

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
    fn test_op_fadd_display() {
        let op = OpFAdd {
            dst: Dst::None,
            srcs: [zero_src(), imm_src(0x42)],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
        };
        assert!(format!("{}", op).contains("fadd"));
        assert!(format!("{}", op).contains("rZ"));
        assert!(format!("{}", op).contains("0x42"));
    }

    #[test]
    fn test_op_fadd_saturate_ftz() {
        let op = OpFAdd {
            dst: Dst::None,
            srcs: [zero_src(), zero_src()],
            saturate: true,
            rnd_mode: FRndMode::NearestEven,
            ftz: true,
        };
        let s = format!("{}", op);
        assert!(s.contains(".sat"));
        assert!(s.contains(".ftz"));
    }

    #[test]
    fn test_op_ffma_display() {
        let op = OpFFma {
            dst: Dst::None,
            srcs: [zero_src(), zero_src(), imm_src(1)],
            saturate: false,
            rnd_mode: FRndMode::NegInf,
            ftz: false,
            dnz: false,
        };
        let s = format!("{}", op);
        assert!(s.contains("ffma"));
        assert!(s.contains(".rm"));
    }

    #[test]
    fn test_op_fmnmx_display() {
        let op = OpFMnMx {
            dst: Dst::None,
            srcs: [zero_src(), imm_src(2)],
            min: Src::new_imm_bool(true),
            ftz: true,
        };
        let s = format!("{}", op);
        assert!(s.contains("fmnmx"));
        assert!(s.contains(".ftz"));
    }

    #[test]
    fn test_op_fmul_display() {
        let op = OpFMul {
            dst: Dst::None,
            srcs: [zero_src(), zero_src()],
            saturate: false,
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            dnz: true,
        };
        let s = format!("{}", op);
        assert!(s.contains("fmul"));
        assert!(s.contains(".dnz"));
    }

    #[test]
    fn test_op_fset_display() {
        let op = OpFSet {
            dst: Dst::None,
            cmp_op: FloatCmpOp::OrdEq,
            srcs: [zero_src(), zero_src()],
            ftz: false,
        };
        let s = format!("{}", op);
        assert!(s.contains("fset"));
        assert!(s.contains(".eq"));
    }

    #[test]
    fn test_op_fsetp_display() {
        let op = OpFSetP {
            dst: Dst::None,
            set_op: PredSetOp::And,
            cmp_op: FloatCmpOp::OrdLt,
            srcs: [zero_src(), imm_src(1)],
            accum: Src::new_imm_bool(true),
            ftz: false,
        };
        let s = format!("{}", op);
        assert!(s.contains("fsetp"));
        assert!(s.contains(".lt"));
    }

    #[test]
    fn test_fswz_add_op_display() {
        assert_eq!(format!("{}", FSwzAddOp::Add), "add");
        assert_eq!(format!("{}", FSwzAddOp::SubRight), "subr");
        assert_eq!(format!("{}", FSwzAddOp::SubLeft), "sub");
        assert_eq!(format!("{}", FSwzAddOp::MoveLeft), "mov2");
    }

    #[test]
    fn test_op_fswzadd_display() {
        let op = OpFSwzAdd {
            dst: Dst::None,
            srcs: [zero_src(), zero_src()],
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            deriv_mode: TexDerivMode::Auto,
            ops: [
                FSwzAddOp::Add,
                FSwzAddOp::SubRight,
                FSwzAddOp::SubLeft,
                FSwzAddOp::MoveLeft,
            ],
        };
        let s = format!("{}", op);
        assert!(s.contains("fswzadd"));
        assert!(s.contains("add"));
        assert!(s.contains("subr"));
    }

    #[test]
    fn test_fswz_shuffle_display() {
        assert_eq!(format!("{}", FSwzShuffle::Quad0), ".0000");
        assert_eq!(format!("{}", FSwzShuffle::SwapHorizontal), ".1032");
        assert_eq!(format!("{}", FSwzShuffle::SwapVertical), ".2301");
    }

    #[test]
    fn test_op_fswz_display() {
        let op = OpFSwz {
            dst: Dst::None,
            srcs: [zero_src(), zero_src()],
            rnd_mode: FRndMode::NearestEven,
            ftz: false,
            deriv_mode: TexDerivMode::NonDivergent,
            shuffle: FSwzShuffle::Quad1,
            ops: [FSwzAddOp::Add; 4],
        };
        let s = format!("{}", op);
        assert!(s.contains("fswz"));
        assert!(s.contains(".1111"));
    }

    #[test]
    fn test_rro_op_display() {
        assert_eq!(format!("{}", RroOp::SinCos), ".sincos");
        assert_eq!(format!("{}", RroOp::Exp2), ".exp2");
    }

    #[test]
    fn test_op_rro_display() {
        let op = OpRro {
            dst: Dst::None,
            op: RroOp::SinCos,
            src: zero_src(),
        };
        let s = format!("{}", op);
        assert!(s.contains("rro"));
        assert!(s.contains(".sincos"));
    }

    #[test]
    fn test_mufu_op_display() {
        assert_eq!(format!("{}", TranscendentalOp::Cos), "cos");
        assert_eq!(format!("{}", TranscendentalOp::Sin), "sin");
        assert_eq!(format!("{}", TranscendentalOp::Sqrt), "sqrt");
        assert_eq!(format!("{}", TranscendentalOp::Rcp), "rcp");
    }

    #[test]
    fn test_op_mufu_display() {
        let op = OpTranscendental {
            dst: Dst::None,
            op: TranscendentalOp::Sqrt,
            src: zero_src(),
        };
        let s = format!("{}", op);
        assert!(s.contains("transcendental"));
        assert!(s.contains("sqrt"));
    }

    #[test]
    fn test_op_dadd_display() {
        let op = OpDAdd {
            dst: Dst::None,
            srcs: [zero_src(), zero_src()],
            rnd_mode: FRndMode::Zero,
        };
        let s = format!("{}", op);
        assert!(s.contains("dadd"));
        assert!(s.contains(".rz"));
    }

    #[test]
    fn test_op_dmul_display() {
        let op = OpDMul {
            dst: Dst::None,
            srcs: [zero_src(), imm_src(0xdead)],
            rnd_mode: FRndMode::NearestEven,
        };
        let s = format!("{}", op);
        assert!(s.contains("dmul"));
    }

    #[test]
    fn test_op_dfma_display() {
        let op = OpDFma {
            dst: Dst::None,
            srcs: [zero_src(), zero_src(), zero_src()],
            rnd_mode: FRndMode::PosInf,
        };
        let s = format!("{}", op);
        assert!(s.contains("dfma"));
        assert!(s.contains(".rp"));
    }

    #[test]
    fn test_op_f64_sqrt_rcp_exp2_log2_sin_cos_display() {
        let sqrt = OpF64Sqrt {
            dst: Dst::None,
            src: zero_src(),
        };
        assert!(format!("{}", sqrt).contains("f64sqrt"));

        let rcp = OpF64Rcp {
            dst: Dst::None,
            src: zero_src(),
        };
        assert!(format!("{}", rcp).contains("f64rcp"));

        let exp2 = OpF64Exp2 {
            dst: Dst::None,
            src: zero_src(),
        };
        assert!(format!("{}", exp2).contains("f64exp2"));

        let log2 = OpF64Log2 {
            dst: Dst::None,
            src: zero_src(),
        };
        assert!(format!("{}", log2).contains("f64log2"));

        let sin = OpF64Sin {
            dst: Dst::None,
            src: zero_src(),
        };
        assert!(format!("{}", sin).contains("f64sin"));

        let cos = OpF64Cos {
            dst: Dst::None,
            src: zero_src(),
        };
        assert!(format!("{}", cos).contains("f64cos"));
    }

    #[test]
    fn test_op_dmnmx_display() {
        let op = OpDMnMx {
            dst: Dst::None,
            srcs: [zero_src(), zero_src()],
            min: Src::new_imm_bool(false),
        };
        let s = format!("{}", op);
        assert!(s.contains("dmnmx"));
    }

    #[test]
    fn test_op_hadd2_display() {
        let op = OpHAdd2 {
            dst: Dst::None,
            srcs: [zero_src(), zero_src()],
            saturate: true,
            ftz: false,
            f32: true,
        };
        let s = format!("{}", op);
        assert!(s.contains("hadd2"));
        assert!(s.contains(".sat"));
        assert!(s.contains(".f32"));
    }

    #[test]
    fn test_op_hmul2_display() {
        let op = OpHMul2 {
            dst: Dst::None,
            srcs: [zero_src(), zero_src()],
            saturate: false,
            ftz: true,
            dnz: false,
        };
        let s = format!("{}", op);
        assert!(s.contains("hmul2"));
        assert!(s.contains(".ftz"));
    }

    #[test]
    fn test_imma_size_display() {
        assert_eq!(format!("{}", ImmaSize::M8N8K16), ".m8n8k16");
        assert_eq!(format!("{}", ImmaSize::M16N8K64), ".m16n8k64");
    }

    #[test]
    fn test_op_imma_display() {
        let op = OpImma {
            dst: Dst::None,
            mat_size: ImmaSize::M8N8K32,
            src_types: [IntType::U8, IntType::I8],
            saturate: false,
            srcs: [zero_src(), zero_src(), zero_src()],
        };
        let s = format!("{}", op);
        assert!(s.contains("imma"));
        assert!(s.contains(".m8n8k32"));
    }

    #[test]
    fn test_hmma_size_display() {
        assert_eq!(format!("{}", HmmaSize::M16N8K16), ".m16n8k16");
        assert_eq!(format!("{}", HmmaSize::M16N8K4), ".m16n8k4");
    }

    #[test]
    fn test_op_hmma_display() {
        let op = OpHmma {
            dst: Dst::None,
            mat_size: HmmaSize::M16N8K8,
            src_type: FloatType::F16,
            dst_type: FloatType::F32,
            srcs: [zero_src(), zero_src(), zero_src()],
        };
        let s = format!("{}", op);
        assert!(s.contains("hmma"));
        assert!(s.contains(".m16n8k8"));
    }

    #[test]
    fn test_op_hfma2_display() {
        let op = OpHFma2 {
            dst: Dst::None,
            srcs: [zero_src(), zero_src(), zero_src()],
            saturate: false,
            ftz: false,
            dnz: true,
            f32: false,
        };
        let s = format!("{}", op);
        assert!(s.contains("hfma2"));
        assert!(s.contains(".dnz"));
    }

    #[test]
    fn test_op_hmnmx2_display() {
        let op = OpHMnMx2 {
            dst: Dst::None,
            srcs: [zero_src(), zero_src()],
            min: Src::new_imm_bool(true),
            ftz: false,
        };
        let s = format!("{}", op);
        assert!(s.contains("hmnmx2"));
    }
}
