// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! Miscellaneous virtual ops: copy, pin, phi, parallel copy, register output.

#![allow(clippy::wildcard_imports)]

use super::*;
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
            write!(f, " {}", label)?;
        }
        Ok(())
    }
}
impl_display_for_op!(OpNop);

#[allow(dead_code)]
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
            PixVal::MsCount => write!(f, ".mscount"),
            PixVal::CovMask => write!(f, ".covmask"),
            PixVal::Covered => write!(f, ".covered"),
            PixVal::Offset => write!(f, ".offset"),
            PixVal::CentroidOffset => write!(f, ".centroid_offset"),
            PixVal::MyIndex => write!(f, ".my_index"),
            PixVal::InnerCoverage => write!(f, ".inner_coverage"),
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
            VoteOp::Any => write!(f, "any"),
            VoteOp::All => write!(f, "all"),
            VoteOp::Eq => write!(f, "eq"),
        }
    }
}

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpVote {
    pub op: VoteOp,

    #[dst_type(GPR)]
    pub ballot: Dst,

    #[dst_type(Pred)]
    pub vote: Dst,

    #[src_type(Pred)]
    pub pred: Src,
}

impl DisplayOp for OpVote {
    fn fmt_dsts(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ballot.is_none() && self.vote.is_none() {
            write!(f, "none")
        } else {
            if !self.ballot.is_none() {
                write!(f, "{}", self.ballot)?;
            }
            if !self.vote.is_none() {
                write!(f, "{}", self.vote)?;
            }
            Ok(())
        }
    }

    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "vote.{} {}", self.op, self.pred)
    }
}
impl_display_for_op!(OpVote);

#[allow(dead_code)]
#[derive(Copy, Clone)]
pub enum MatchOp {
    All,
    Any,
}

impl fmt::Display for MatchOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MatchOp::All => write!(f, ".all"),
            MatchOp::Any => write!(f, ".any"),
        }
    }
}

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpMatch {
    #[dst_type(Pred)]
    pub pred: Dst,

    #[dst_type(GPR)]
    pub mask: Dst,

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

pub struct VecPair<A, B> {
    a: Vec<A>,
    b: Vec<B>,
}

impl<A, B> VecPair<A, B> {
    pub fn append(&mut self, other: &mut VecPair<A, B>) {
        self.a.append(&mut other.a);
        self.b.append(&mut other.b);
    }

    pub fn is_empty(&self) -> bool {
        debug_assert!(self.a.len() == self.b.len());
        self.a.is_empty()
    }

    pub fn iter(&self) -> Zip<slice::Iter<'_, A>, slice::Iter<'_, B>> {
        debug_assert!(self.a.len() == self.b.len());
        self.a.iter().zip(self.b.iter())
    }

    pub fn iter_mut(&mut self) -> Zip<slice::IterMut<'_, A>, slice::IterMut<'_, B>> {
        debug_assert!(self.a.len() == self.b.len());
        self.a.iter_mut().zip(self.b.iter_mut())
    }

    pub fn len(&self) -> usize {
        debug_assert!(self.a.len() == self.b.len());
        self.a.len()
    }

    pub fn new() -> Self {
        Self {
            a: Vec::new(),
            b: Vec::new(),
        }
    }

    pub fn push(&mut self, a: A, b: B) {
        debug_assert!(self.a.len() == self.b.len());
        self.a.push(a);
        self.b.push(b);
    }
}

impl<A: Clone, B: Clone> VecPair<A, B> {
    pub fn retain(&mut self, mut f: impl FnMut(&A, &B) -> bool) {
        debug_assert!(self.a.len() == self.b.len());
        let len = self.a.len();
        let mut i = 0_usize;
        while i < len {
            if !f(&self.a[i], &self.b[i]) {
                break;
            }
            i += 1;
        }

        let mut new_len = i;

        // Don't check this one twice.
        i += 1;

        while i < len {
            // This could be more efficient but it's good enough for our
            // purposes since everything we're storing is small and has a
            // trivial Drop.
            if f(&self.a[i], &self.b[i]) {
                self.a[new_len] = self.a[i].clone();
                self.b[new_len] = self.b[i].clone();
                new_len += 1;
            }
            i += 1;
        }

        if new_len < len {
            self.a.truncate(new_len);
            self.b.truncate(new_len);
        }
    }
}

mod phi {
    #[allow(unused_imports)]
    use super::{OpPhiDsts, OpPhiSrcs};
    use coral_nak_stubs::bitset::IntoBitIndex;
    use std::fmt;

    /// A phi node
    ///
    /// Phis in NAK are implemented differently from NIR and similar IRs.
    /// Instead of having a single phi instruction which lives in the successor
    /// block, each `Phi` represents a single merged 32-bit (or 1-bit for
    /// predicates) value and we have separate [`OpPhiSrcs`] and [`OpPhiDsts`]
    /// instructions which map phis to sources and destinations.
    ///
    /// One of the problems fundamental to phis is that they really live on the
    /// edges between blocks.  Regardless of where the phi instruction lives in
    /// the IR data structures, its sources are consumed at the end of the
    /// predecessor block and its destinations are defined at the start of the
    /// successor block and all phi sources and destinations get consumed and go
    /// live simultaneously for any given CFG edge.  For a phi that participates
    /// in a back-edge, this means that the source of the phi may be consumed
    /// after (in block order) the destination goes live.
    ///
    /// In NIR, this has caused no end of headaches.  Most passes which need to
    /// process phis ignore phis when first processing a block and then have a
    /// special case at the end of each block which walks the successors and
    /// processes the successor's phis, looking only at the phi sources whose
    /// predecessor matches the block.  This is clunky and often forgotten by
    /// optimization and lowering pass authors.  It's also easy to get missed by
    /// testing since it only really breaks if you have a phi which participates
    /// in a back-edge so it often gets found later when something breaks in the
    /// wild.
    ///
    /// To work around this (and also make things a little more Rust-friendly),
    /// NAK places the instruction which consumes phi sources at the end of the
    /// predecessor block and the instruction which defines phi destinations at
    /// the start of the successor block.  This structurally eliminates the
    /// problem that has plagued NIR for years.  The cost to this solution is
    /// that we have to create maps from phis to/from SSA values whenever we
    /// want to optimize the phis themselves.  However, this affects few enough
    /// passes that the benefits to the rest of the IR are worth the trade-off,
    /// at least for a back-end compiler.
    #[derive(Clone, Copy, Eq, Hash, PartialEq)]
    pub struct Phi {
        idx: u32,
    }

    impl IntoBitIndex for Phi {
        fn bit_index(&self) -> usize {
            self.idx as usize
        }
    }

    impl fmt::Display for Phi {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "φ{}", self.idx)
        }
    }

    pub struct PhiAllocator {
        count: u32,
    }

    impl PhiAllocator {
        pub fn new() -> PhiAllocator {
            PhiAllocator { count: 0 }
        }

        pub fn alloc(&mut self) -> Phi {
            let idx = self.count;
            self.count = idx + 1;
            Phi { idx }
        }
    }
}
pub use phi::{Phi, PhiAllocator};

/// An instruction which maps [Phi]s to sources in the predecessor block
#[repr(C)]
#[derive(DstsAsSlice)]
pub struct OpPhiSrcs {
    pub srcs: VecPair<Phi, Src>,
}

impl OpPhiSrcs {
    pub fn new() -> OpPhiSrcs {
        OpPhiSrcs {
            srcs: VecPair::new(),
        }
    }
}

impl AsSlice<Src> for OpPhiSrcs {
    type Attr = SrcType;

    fn as_slice(&self) -> &[Src] {
        &self.srcs.b
    }

    fn as_mut_slice(&mut self) -> &mut [Src] {
        &mut self.srcs.b
    }

    fn attrs(&self) -> SrcTypeList {
        SrcTypeList::Uniform(SrcType::GPR)
    }
}

impl DisplayOp for OpPhiSrcs {
    fn fmt_dsts(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(())
    }

    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "phi_src ")?;
        for (i, (phi, src)) in self.srcs.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{phi} = {src}")?;
        }
        Ok(())
    }
}
impl_display_for_op!(OpPhiSrcs);

/// An instruction which maps [Phi]s to destinations in the succeessor block
#[repr(C)]
#[derive(SrcsAsSlice)]
pub struct OpPhiDsts {
    pub dsts: VecPair<Phi, Dst>,
}

impl OpPhiDsts {
    pub fn new() -> OpPhiDsts {
        OpPhiDsts {
            dsts: VecPair::new(),
        }
    }
}

impl AsSlice<Dst> for OpPhiDsts {
    type Attr = DstType;

    fn as_slice(&self) -> &[Dst] {
        &self.dsts.b
    }

    fn as_mut_slice(&mut self) -> &mut [Dst] {
        &mut self.dsts.b
    }

    fn attrs(&self) -> DstTypeList {
        DstTypeList::Uniform(DstType::Vec)
    }
}

impl DisplayOp for OpPhiDsts {
    fn fmt_dsts(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(())
    }

    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "phi_dst ")?;
        for (i, (phi, dst)) in self.dsts.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{dst} = {phi}")?;
        }
        Ok(())
    }
}
impl_display_for_op!(OpPhiDsts);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpCopy {
    pub dst: Dst,
    pub src: Src,
}

impl DisplayOp for OpCopy {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "copy {}", self.src)
    }
}
impl_display_for_op!(OpCopy);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
/// Copies a value and pins its destination in the register file
pub struct OpPin {
    pub dst: Dst,
    #[src_type(SSA)]
    pub src: Src,
}

impl DisplayOp for OpPin {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pin {}", self.src)
    }
}
impl_display_for_op!(OpPin);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
/// Copies a pinned value to an unpinned value
pub struct OpUnpin {
    pub dst: Dst,
    #[src_type(SSA)]
    pub src: Src,
}

impl DisplayOp for OpUnpin {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unpin {}", self.src)
    }
}
impl_display_for_op!(OpUnpin);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpSwap {
    pub dsts: [Dst; 2],
    pub srcs: [Src; 2],
}

impl DisplayOp for OpSwap {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "swap {} {}", self.srcs[0], self.srcs[1])
    }
}
impl_display_for_op!(OpSwap);

#[repr(C)]
pub struct OpParCopy {
    pub dsts_srcs: VecPair<Dst, Src>,
    pub tmp: Option<RegRef>,
}

impl OpParCopy {
    pub fn new() -> OpParCopy {
        OpParCopy {
            dsts_srcs: VecPair::new(),
            tmp: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.dsts_srcs.is_empty()
    }

    pub fn push(&mut self, dst: Dst, src: Src) {
        self.dsts_srcs.push(dst, src);
    }
}

impl AsSlice<Src> for OpParCopy {
    type Attr = SrcType;

    fn as_slice(&self) -> &[Src] {
        &self.dsts_srcs.b
    }

    fn as_mut_slice(&mut self) -> &mut [Src] {
        &mut self.dsts_srcs.b
    }

    fn attrs(&self) -> SrcTypeList {
        SrcTypeList::Uniform(SrcType::GPR)
    }
}

impl AsSlice<Dst> for OpParCopy {
    type Attr = DstType;

    fn as_slice(&self) -> &[Dst] {
        &self.dsts_srcs.a
    }

    fn as_mut_slice(&mut self) -> &mut [Dst] {
        &mut self.dsts_srcs.a
    }

    fn attrs(&self) -> DstTypeList {
        DstTypeList::Uniform(DstType::Vec)
    }
}

impl DisplayOp for OpParCopy {
    fn fmt_dsts(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(())
    }

    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "par_copy")?;
        for (i, (dst, src)) in self.dsts_srcs.iter().enumerate() {
            if i > 0 {
                write!(f, ",")?;
            }
            write!(f, " {} = {}", dst, src)?;
        }
        Ok(())
    }
}
impl_display_for_op!(OpParCopy);

#[repr(C)]
#[derive(DstsAsSlice)]
pub struct OpRegOut {
    pub srcs: Vec<Src>,
}

impl AsSlice<Src> for OpRegOut {
    type Attr = SrcType;

    fn as_slice(&self) -> &[Src] {
        &self.srcs
    }

    fn as_mut_slice(&mut self) -> &mut [Src] {
        &mut self.srcs
    }

    fn attrs(&self) -> SrcTypeList {
        SrcTypeList::Uniform(SrcType::GPR)
    }
}

impl DisplayOp for OpRegOut {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "reg_out {{")?;
        for (i, src) in self.srcs.iter().enumerate() {
            if i > 0 {
                write!(f, ",")?;
            }
            write!(f, " {}", src)?;
        }
        write!(f, " }}")
    }
}
impl_display_for_op!(OpRegOut);

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum OutType {
    Emit,
    Cut,
    EmitThenCut,
}

impl fmt::Display for OutType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OutType::Emit => write!(f, "emit"),
            OutType::Cut => write!(f, "cut"),
            OutType::EmitThenCut => write!(f, "emit_then_cut"),
        }
    }
}

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpOut {
    pub dst: Dst,

    #[src_type(SSA)]
    pub handle: Src,

    #[src_type(ALU)]
    pub stream: Src,

    pub out_type: OutType,
}

impl DisplayOp for OpOut {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "out.{} {} {}", self.out_type, self.handle, self.stream)
    }
}
impl_display_for_op!(OpOut);

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpOutFinal {
    #[src_type(SSA)]
    pub handle: Src,
}

impl DisplayOp for OpOutFinal {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "out.final {{ {} }}", self.handle)
    }
}
impl_display_for_op!(OpOutFinal);

/// Describes an annotation on an instruction.
#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpAnnotate {
    /// The annotation
    pub annotation: String,
}

impl DisplayOp for OpAnnotate {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "// {}", self.annotation)
    }
}

impl fmt::Display for OpAnnotate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_op(f)
    }
}
