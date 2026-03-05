// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT
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

#[allow(dead_code)]
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
            FSwzAddOp::Add => write!(f, "add"),
            FSwzAddOp::SubRight => write!(f, "subr"),
            FSwzAddOp::SubLeft => write!(f, "sub"),
            FSwzAddOp::MoveLeft => write!(f, "mov2"),
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
#[allow(dead_code)]
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
            FSwzShuffle::Quad0 => write!(f, ".0000"),
            FSwzShuffle::Quad1 => write!(f, ".1111"),
            FSwzShuffle::Quad2 => write!(f, ".2222"),
            FSwzShuffle::Quad3 => write!(f, ".3333"),
            FSwzShuffle::SwapHorizontal => write!(f, ".1032"),
            FSwzShuffle::SwapVertical => write!(f, ".2301"),
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

pub enum RroOp {
    SinCos,
    Exp2,
}

impl fmt::Display for RroOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RroOp::SinCos => write!(f, ".sincos"),
            RroOp::Exp2 => write!(f, ".exp2"),
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

#[allow(dead_code)]
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum MuFuOp {
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

impl fmt::Display for MuFuOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MuFuOp::Cos => write!(f, "cos"),
            MuFuOp::Sin => write!(f, "sin"),
            MuFuOp::Exp2 => write!(f, "exp2"),
            MuFuOp::Log2 => write!(f, "log2"),
            MuFuOp::Rcp => write!(f, "rcp"),
            MuFuOp::Rsq => write!(f, "rsq"),
            MuFuOp::Rcp64H => write!(f, "rcp64h"),
            MuFuOp::Rsq64H => write!(f, "rsq64h"),
            MuFuOp::Sqrt => write!(f, "sqrt"),
            MuFuOp::Tanh => write!(f, "tanh"),
        }
    }
}

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpMuFu {
    #[dst_type(F32)]
    pub dst: Dst,

    pub op: MuFuOp,

    #[src_type(F32)]
    pub src: Src,
}

impl DisplayOp for OpMuFu {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "mufu.{} {}", self.op, self.src)
    }
}
impl_display_for_op!(OpMuFu);

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
#[allow(dead_code)]
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
            ImmaSize::M8N8K16 => write!(f, ".m8n8k16"),
            ImmaSize::M8N8K32 => write!(f, ".m8n8k32"),
            ImmaSize::M16N8K16 => write!(f, ".m16n8k16"),
            ImmaSize::M16N8K32 => write!(f, ".m16n8k32"),
            ImmaSize::M16N8K64 => write!(f, ".m16n8k64"),
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
            HmmaSize::M16N8K16 => write!(f, ".m16n8k16"),
            HmmaSize::M16N8K8 => write!(f, ".m16n8k8"),
            HmmaSize::M16N8K4 => write!(f, ".m16n8k4"),
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
