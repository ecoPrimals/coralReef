// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT
//! Instruction representation: Instr, InstrDeps, MappedInstrs.

use std::fmt;

use coral_nak_stubs::smallvec::SmallVec;

use super::op::Op;
use super::{DstsAsSlice, Fmt, IsUniform, MemSpace, OpBra, Pred, SrcTypeList, SrcsAsSlice};

pub const MIN_INSTR_DELAY: u8 = 1;

pub struct InstrDeps {
    pub delay: u8,
    pub yld: bool,
    wr_bar: i8,
    rd_bar: i8,
    pub wt_bar_mask: u8,
    pub reuse_mask: u8,
}

impl InstrDeps {
    pub fn new() -> InstrDeps {
        InstrDeps {
            delay: 0,
            yld: false,
            wr_bar: -1,
            rd_bar: -1,
            wt_bar_mask: 0,
            reuse_mask: 0,
        }
    }

    pub fn rd_bar(&self) -> Option<u8> {
        if self.rd_bar < 0 {
            None
        } else {
            Some(self.rd_bar.try_into().unwrap())
        }
    }

    pub fn wr_bar(&self) -> Option<u8> {
        if self.wr_bar < 0 {
            None
        } else {
            Some(self.wr_bar.try_into().unwrap())
        }
    }

    pub fn set_delay(&mut self, delay: u8) {
        self.delay = delay;
    }

    pub fn set_yield(&mut self, yld: bool) {
        self.yld = yld;
    }

    pub fn set_rd_bar(&mut self, idx: u8) {
        assert!(idx < 6);
        self.rd_bar = idx.try_into().unwrap();
    }

    pub fn set_wr_bar(&mut self, idx: u8) {
        assert!(idx < 6);
        self.wr_bar = idx.try_into().unwrap();
    }

    pub fn add_wt_bar(&mut self, idx: u8) {
        self.add_wt_bar_mask(1 << idx);
    }

    pub fn add_wt_bar_mask(&mut self, bar_mask: u8) {
        assert!(bar_mask < 1 << 6);
        self.wt_bar_mask |= bar_mask;
    }

    #[allow(dead_code)]
    pub fn add_reuse(&mut self, idx: u8) {
        assert!(idx < 6);
        self.reuse_mask |= 1_u8 << idx;
    }
}

impl fmt::Display for InstrDeps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.delay > 0 {
            write!(f, " delay={}", self.delay)?;
        }
        if self.wt_bar_mask != 0 {
            write!(f, " wt={:06b}", self.wt_bar_mask)?;
        }
        if self.rd_bar >= 0 {
            write!(f, " rd:{}", self.rd_bar)?;
        }
        if self.wr_bar >= 0 {
            write!(f, " wr:{}", self.wr_bar)?;
        }
        if self.reuse_mask != 0 {
            write!(f, " reuse={:06b}", self.reuse_mask)?;
        }
        if self.yld {
            write!(f, " yld")?;
        }
        Ok(())
    }
}

pub struct Instr {
    pub pred: Pred,
    pub op: Op,
    pub deps: InstrDeps,
}

impl Instr {
    pub fn new(op: impl Into<Op>) -> Self {
        Self {
            op: op.into(),
            pred: true.into(),
            deps: InstrDeps::new(),
        }
    }

    pub fn dsts(&self) -> &[super::Dst] {
        self.op.dsts_as_slice()
    }

    pub fn dsts_mut(&mut self) -> &mut [super::Dst] {
        self.op.dsts_as_mut_slice()
    }

    pub fn srcs(&self) -> &[super::Src] {
        self.op.srcs_as_slice()
    }

    pub fn srcs_mut(&mut self) -> &mut [super::Src] {
        self.op.srcs_as_mut_slice()
    }

    pub fn src_types(&self) -> SrcTypeList {
        self.op.src_types()
    }

    pub fn ssa_uses(&self) -> impl Iterator<Item = &super::SSAValue> {
        self.srcs()
            .iter()
            .flat_map(|src| src.iter_ssa())
            .chain(self.pred.pred_ref.iter_ssa())
    }

    pub fn for_each_ssa_use(&self, mut f: impl FnMut(&super::SSAValue)) {
        for ssa in self.pred.iter_ssa() {
            f(ssa);
        }
        for src in self.srcs() {
            for ssa in src.iter_ssa() {
                f(ssa);
            }
        }
    }

    pub fn for_each_ssa_use_mut(&mut self, mut f: impl FnMut(&mut super::SSAValue)) {
        for ssa in self.pred.iter_ssa_mut() {
            f(ssa);
        }
        for src in self.srcs_mut() {
            for ssa in src.iter_ssa_mut() {
                f(ssa);
            }
        }
    }

    pub fn for_each_ssa_def(&self, mut f: impl FnMut(&super::SSAValue)) {
        for dst in self.dsts() {
            for ssa in dst.iter_ssa() {
                f(ssa);
            }
        }
    }

    pub fn for_each_ssa_def_mut(&mut self, mut f: impl FnMut(&mut super::SSAValue)) {
        for dst in self.dsts_mut() {
            for ssa in dst.iter_ssa_mut() {
                f(ssa);
            }
        }
    }

    pub fn is_branch(&self) -> bool {
        self.op.is_branch()
    }

    /// Returns true if `self`` is a branch instruction that is always taken.
    /// It returns false for non branch instructions.
    pub fn is_branch_always_taken(&self) -> bool {
        if self.pred.is_true() {
            match &self.op {
                Op::Bra(bra) => bra.cond.is_true(),
                _ => self.is_branch(),
            }
        } else {
            false
        }
    }

    pub fn uses_global_mem(&self) -> bool {
        match &self.op {
            Op::Atom(op) => op.mem_space != MemSpace::Local,
            Op::Ld(op) => op.access.space != MemSpace::Local,
            Op::St(op) => op.access.space != MemSpace::Local,
            Op::SuAtom(_) | Op::SuLd(_) | Op::SuSt(_) | Op::SuLdGa(_) | Op::SuStGa(_) => true,
            _ => false,
        }
    }

    pub fn writes_global_mem(&self) -> bool {
        match &self.op {
            Op::Atom(op) => matches!(op.mem_space, MemSpace::Global(_)),
            Op::St(op) => matches!(op.access.space, MemSpace::Global(_)),
            Op::SuAtom(_) | Op::SuSt(_) | Op::SuStGa(_) => true,
            _ => false,
        }
    }

    pub fn can_eliminate(&self) -> bool {
        match &self.op {
            Op::ASt(_)
            | Op::SuSt(_)
            | Op::SuStGa(_)
            | Op::SuAtom(_)
            | Op::LdSharedLock(_)
            | Op::St(_)
            | Op::StSCheckUnlock(_)
            | Op::Atom(_)
            | Op::CCtl(_)
            | Op::MemBar(_)
            | Op::Kill(_)
            | Op::Nop(_)
            | Op::BSync(_)
            | Op::Bra(_)
            | Op::SSy(_)
            | Op::Sync(_)
            | Op::Brk(_)
            | Op::PBk(_)
            | Op::Cont(_)
            | Op::PCnt(_)
            | Op::Exit(_)
            | Op::WarpSync(_)
            | Op::Bar(_)
            | Op::TexDepBar(_)
            | Op::RegOut(_)
            | Op::Out(_)
            | Op::OutFinal(_)
            | Op::Annotate(_) => false,
            Op::BMov(op) => !op.clear,
            _ => true,
        }
    }

    pub fn is_uniform(&self) -> bool {
        match &self.op {
            Op::PhiDsts(_) => false,
            op => op.is_uniform(),
        }
    }

    pub fn needs_yield(&self) -> bool {
        matches!(&self.op, Op::Bar(_) | Op::BSync(_))
    }

    pub fn fmt_pred(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.pred.is_true() {
            write!(f, "@{} ", self.pred)?;
        }
        Ok(())
    }
}

impl fmt::Display for Instr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}{}", Fmt(|f| self.fmt_pred(f)), self.op, self.deps)
    }
}

impl<T: Into<Op>> From<T> for Instr {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

pub type MappedInstrs = SmallVec<Instr, 4>;
