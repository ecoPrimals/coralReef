// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! Miscellaneous virtual ops: copy, pin, phi, parallel copy, register output.

use super::*;

mod system;
pub use system::*;

pub struct VecPair<A, B> {
    a: Vec<A>,
    b: Vec<B>,
}

impl<A, B> VecPair<A, B> {
    pub fn append(&mut self, other: &mut Self) {
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

    use coral_reef_stubs::bitset::IntoBitIndex;
    use std::fmt;

    /// A phi node
    ///
    /// Phis are implemented differently from other IRs (e.g. NIR).
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
    /// In other SSA IRs, this has caused no end of headaches.  Most passes which need to
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
    /// coralReef places the instruction which consumes phi sources at the end of the
    /// predecessor block and the instruction which defines phi destinations at
    /// the start of the successor block.  This structurally eliminates the
    /// problem that has plagued other IRs.  The cost to this solution is
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
        pub fn new() -> Self {
            Self { count: 0 }
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
    pub fn new() -> Self {
        Self {
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
    pub fn new() -> Self {
        Self {
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
    pub fn new() -> Self {
        Self {
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
            write!(f, " {dst} = {src}")?;
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
            write!(f, " {src}")?;
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
            Self::Emit => write!(f, "emit"),
            Self::Cut => write!(f, "cut"),
            Self::EmitThenCut => write!(f, "emit_then_cut"),
        }
    }
}

#[repr(C)]
#[derive(SrcsAsSlice, DstsAsSlice)]
pub struct OpOut {
    pub dst: Dst,

    #[src_types(SSA, ALU)]
    #[src_names(handle, stream)]
    pub srcs: [Src; 2],

    pub out_type: OutType,
}

impl DisplayOp for OpOut {
    fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "out.{} {} {}",
            self.out_type,
            self.handle(),
            self.stream()
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    fn zero_src() -> Src {
        Src::ZERO
    }

    #[test]
    fn test_op_cs2r_display() {
        let op = OpCS2R {
            dst: Dst::None,
            idx: 0x10,
        };
        let s = format!("{op}");
        assert!(s.contains("cs2r"));
        assert!(s.contains("0x10"));
    }

    #[test]
    fn test_op_isberd_display() {
        let op = OpIsberd {
            dst: Dst::None,
            idx: zero_src(),
        };
        let s = format!("{op}");
        assert!(s.contains("isberd"));
    }

    #[test]
    fn test_op_vild_display() {
        let op = OpViLd {
            dst: Dst::None,
            idx: zero_src(),
            off: 0,
        };
        let s = format!("{op}");
        assert!(s.contains("vild"));
    }

    #[test]
    fn test_op_kill_display() {
        let op = OpKill {};
        let s = format!("{op}");
        assert!(s.contains("kill"));
    }

    #[test]
    fn test_op_nop_display() {
        let op = OpNop { label: None };
        let s = format!("{op}");
        assert!(s.contains("nop"));
    }

    #[test]
    fn test_op_nop_with_label() {
        let mut alloc = LabelAllocator::new();
        let label = alloc.alloc();
        let op = OpNop { label: Some(label) };
        let s = format!("{op}");
        assert!(s.contains("nop"));
    }

    #[test]
    fn test_pix_val_display() {
        assert_eq!(format!("{}", PixVal::MsCount), ".mscount");
        assert_eq!(format!("{}", PixVal::CovMask), ".covmask");
        assert_eq!(format!("{}", PixVal::InnerCoverage), ".inner_coverage");
    }

    #[test]
    fn test_op_pixld_display() {
        let op = OpPixLd {
            dst: Dst::None,
            val: PixVal::MsCount,
        };
        let s = format!("{op}");
        assert!(s.contains("pixld"));
        assert!(s.contains("mscount"));
    }

    #[test]
    fn test_op_s2r_display() {
        let op = OpS2R {
            dst: Dst::None,
            idx: 0x20,
        };
        let s = format!("{op}");
        assert!(s.contains("s2r"));
        assert!(s.contains("0x20"));
    }

    #[test]
    fn test_vote_op_display() {
        assert_eq!(format!("{}", VoteOp::Any), "any");
        assert_eq!(format!("{}", VoteOp::All), "all");
        assert_eq!(format!("{}", VoteOp::Eq), "eq");
    }

    #[test]
    fn test_op_vote_display() {
        let op = OpVote {
            op: VoteOp::Any,
            dsts: [Dst::None, Dst::None],
            pred: Src::new_imm_bool(true),
        };
        let s = format!("{op}");
        assert!(s.contains("vote"));
        assert!(s.contains("any"));
    }

    #[test]
    fn test_match_op_display() {
        assert_eq!(format!("{}", MatchOp::All), ".all");
        assert_eq!(format!("{}", MatchOp::Any), ".any");
    }

    #[test]
    fn test_op_match_display() {
        let op = OpMatch {
            dsts: [Dst::None, Dst::None],
            src: zero_src(),
            op: MatchOp::All,
            u64: false,
        };
        let s = format!("{op}");
        assert!(s.contains("match"));
        assert!(s.contains(".all"));
    }

    #[test]
    fn test_op_undef_display() {
        let op = OpUndef { dst: Dst::None };
        let s = format!("{op}");
        assert!(s.contains("undef"));
    }

    #[test]
    fn test_op_srcbar_display() {
        let op = OpSrcBar { src: zero_src() };
        let s = format!("{op}");
        assert!(s.contains("src_bar"));
    }

    #[test]
    fn test_op_copy_display() {
        let op = OpCopy {
            dst: Dst::None,
            src: zero_src(),
        };
        let s = format!("{op}");
        assert!(s.contains("copy"));
    }

    #[test]
    fn test_op_pin_display() {
        let op = OpPin {
            dst: Dst::None,
            src: zero_src(),
        };
        let s = format!("{op}");
        assert!(s.contains("pin"));
    }

    #[test]
    fn test_op_unpin_display() {
        let op = OpUnpin {
            dst: Dst::None,
            src: zero_src(),
        };
        let s = format!("{op}");
        assert!(s.contains("unpin"));
    }

    #[test]
    fn test_op_swap_display() {
        let op = OpSwap {
            dsts: [Dst::None, Dst::None],
            srcs: [zero_src(), Src::new_imm_u32(1)],
        };
        let s = format!("{op}");
        assert!(s.contains("swap"));
    }

    #[test]
    fn test_op_parcopy_display() {
        let mut op = OpParCopy::new();
        op.push(Dst::None, zero_src());
        let s = format!("{op}");
        assert!(s.contains("par_copy"));
    }

    #[test]
    fn test_op_regout_display() {
        let op = OpRegOut {
            srcs: vec![zero_src()],
        };
        let s = format!("{op}");
        assert!(s.contains("reg_out"));
    }

    #[test]
    fn test_out_type_display() {
        assert_eq!(format!("{}", OutType::Emit), "emit");
        assert_eq!(format!("{}", OutType::Cut), "cut");
        assert_eq!(format!("{}", OutType::EmitThenCut), "emit_then_cut");
    }

    #[test]
    fn test_op_out_display() {
        let op = OpOut {
            dst: Dst::None,
            srcs: [zero_src(), zero_src()],
            out_type: OutType::Emit,
        };
        let s = format!("{op}");
        assert!(s.contains("out"));
        assert!(s.contains("emit"));
    }

    #[test]
    fn test_op_outfinal_display() {
        let op = OpOutFinal { handle: zero_src() };
        let s = format!("{op}");
        assert!(s.contains("out.final"));
    }

    #[test]
    fn test_op_annotate_display() {
        let op = OpAnnotate {
            annotation: "test comment".into(),
        };
        let s = format!("{op}");
        assert!(s.contains("//"));
        assert!(s.contains("test comment"));
    }
}
