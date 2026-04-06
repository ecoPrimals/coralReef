// SPDX-License-Identifier: AGPL-3.0-or-later

use super::super::*;

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

    #[src_types(F16v2, F16v2, Pred)]
    #[src_names(src_a, src_b, accum)]
    pub srcs: [Src; 3],

    pub ftz: bool,
}

impl DisplayOp for OpHSet2 {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ftz = if self.ftz { ".ftz" } else { "" };
        write!(f, "hset2{}{ftz}", self.cmp_op)?;
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
impl_display_for_op!(OpHSet2);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpHSetP2 {
    #[dst_type(Pred)]
    pub dsts: [Dst; 2],

    pub set_op: PredSetOp,
    pub cmp_op: FloatCmpOp,

    #[src_types(F16v2, F16v2, Pred)]
    #[src_names(src_a, src_b, accum)]
    pub srcs: [Src; 3],

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

    #[src_types(F16v2, F16v2, Pred)]
    #[src_names(src_a, src_b, min)]
    pub srcs: [Src; 3],

    pub ftz: bool,
}

impl DisplayOp for OpHMnMx2 {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ftz = if self.ftz { ".ftz" } else { "" };
        write!(
            f,
            "hmnmx2{ftz} {} {} {}",
            self.srcs[0],
            self.srcs[1],
            self.min()
        )
    }
}
impl_display_for_op!(OpHMnMx2);
