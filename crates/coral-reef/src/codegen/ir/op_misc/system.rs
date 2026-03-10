// SPDX-License-Identifier: AGPL-3.0-only

#![allow(clippy::wildcard_imports)]

use super::super::*;

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpCS2R {
    pub dst: Dst,
    pub idx: u8,
}

impl DisplayOp for OpCS2R {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cs2r sr[{:#x}]", self.idx)
    }
}
impl_display_for_op!(OpCS2R);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpIsberd {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_type(SSA)]
    pub idx: Src,
}

impl DisplayOp for OpIsberd {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "isberd [{}]", self.idx)
    }
}
impl_display_for_op!(OpIsberd);

/// Vertex Index Load
/// (Only available in Kepler)
///
/// Takes as input the vertex index and loads the vertex address in
/// attribute space.
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpViLd {
    #[dst_type(GPR)]
    pub dst: Dst,

    #[src_type(SSA)]
    pub idx: Src,

    pub off: i8,
}

impl DisplayOp for OpViLd {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "vild v[")?;

        if !self.idx.is_zero() {
            write!(f, "{}", self.idx)?;
            if self.off != 0 {
                write!(f, "{:+}", self.off)?;
            }
        } else {
            write!(f, "{}", self.off)?;
        }

        write!(f, "]")
    }
}
impl_display_for_op!(OpViLd);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpKill {}

impl DisplayOp for OpKill {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "kill")
    }
}
impl_display_for_op!(OpKill);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpNop {
    pub label: Option<Label>,
}

impl DisplayOp for OpNop {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "nop")?;
        if let Some(label) = &self.label {
            write!(f, " {label}")?;
        }
        Ok(())
    }
}
impl_display_for_op!(OpNop);

pub enum PixVal {
    MsCount,
    CovMask,
    Covered,
    Offset,
    CentroidOffset,
    MyIndex,
    InnerCoverage,
}

impl fmt::Display for PixVal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MsCount => write!(f, ".mscount"),
            Self::CovMask => write!(f, ".covmask"),
            Self::Covered => write!(f, ".covered"),
            Self::Offset => write!(f, ".offset"),
            Self::CentroidOffset => write!(f, ".centroid_offset"),
            Self::MyIndex => write!(f, ".my_index"),
            Self::InnerCoverage => write!(f, ".inner_coverage"),
        }
    }
}

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpPixLd {
    pub dst: Dst,
    pub val: PixVal,
}

impl DisplayOp for OpPixLd {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pixld{}", self.val)
    }
}
impl_display_for_op!(OpPixLd);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpS2R {
    pub dst: Dst,
    pub idx: u8,
}

impl DisplayOp for OpS2R {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "s2r sr[{:#x}]", self.idx)
    }
}
impl_display_for_op!(OpS2R);

pub enum VoteOp {
    Any,
    All,
    Eq,
}

impl fmt::Display for VoteOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Any => write!(f, "any"),
            Self::All => write!(f, "all"),
            Self::Eq => write!(f, "eq"),
        }
    }
}

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpVote {
    pub op: VoteOp,

    #[dst_types(GPR, Pred)]
    #[dst_names(ballot, vote)]
    pub dsts: [Dst; 2],

    #[src_type(Pred)]
    pub pred: Src,
}

impl DisplayOp for OpVote {
    fn fmt_dsts(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ballot().is_none() && self.vote().is_none() {
            write!(f, "none")
        } else {
            if !self.ballot().is_none() {
                write!(f, "{}", self.ballot())?;
            }
            if !self.vote().is_none() {
                write!(f, "{}", self.vote())?;
            }
            Ok(())
        }
    }

    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "vote.{} {}", self.op, self.pred)
    }
}
impl_display_for_op!(OpVote);

#[derive(Copy, Clone)]
pub enum MatchOp {
    All,
    Any,
}

impl fmt::Display for MatchOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => write!(f, ".all"),
            Self::Any => write!(f, ".any"),
        }
    }
}

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpMatch {
    #[dst_types(Pred, GPR)]
    #[dst_names(pred, mask)]
    pub dsts: [Dst; 2],

    #[src_type(GPR)]
    pub src: Src,

    pub op: MatchOp,
    pub u64: bool,
}

impl DisplayOp for OpMatch {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let u64_str = if self.u64 { ".u64" } else { "" };
        write!(f, "match{}{} {}", self.op, u64_str, self.src)
    }
}
impl_display_for_op!(OpMatch);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpUndef {
    pub dst: Dst,
}

impl DisplayOp for OpUndef {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "undef {}", self.dst)
    }
}
impl_display_for_op!(OpUndef);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpSrcBar {
    pub src: Src,
}

impl DisplayOp for OpSrcBar {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "src_bar {}", self.src)
    }
}
impl_display_for_op!(OpSrcBar);
