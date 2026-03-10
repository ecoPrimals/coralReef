// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! Program structure: BasicBlock, Function, InstrIdx.

use std::fmt;
use std::fmt::Write;
use std::ops::{Deref, DerefMut, Index};

use coral_reef_stubs::cfg::CFG;
use std::cmp::max;

use super::instr::{Instr, MappedInstrs};
use super::op::Op;
use super::{DisplayOp, Fmt, Label};

/// Stores the index of an instruction in a given Function
///
/// The block and instruction indices are stored in a memory-efficient way.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InstrIdx {
    pub block_idx: u32,
    pub instr_idx: u32,
}

impl InstrIdx {
    pub fn new(bi: usize, ii: usize) -> Self {
        let block_idx = bi.try_into().unwrap_or_else(|_| {
            debug_assert!(false, "Block index overflow");
            0
        });
        let instr_idx = ii.try_into().unwrap_or_else(|_| {
            debug_assert!(false, "Instruction index overflow");
            0
        });
        Self {
            block_idx,
            instr_idx,
        }
    }
}

pub struct BasicBlock {
    pub label: Label,

    /// Whether or not this block is uniform
    ///
    /// If true, then all non-exited lanes in a warp which execute this block
    /// are guaranteed to execute it together
    pub uniform: bool,

    pub instrs: Vec<Instr>,
}

impl BasicBlock {
    pub fn map_instrs(&mut self, mut map: impl FnMut(Instr) -> MappedInstrs) {
        let mut instrs = Vec::new();
        for i in self.instrs.drain(..) {
            match map(i) {
                MappedInstrs::None => (),
                MappedInstrs::One(i) => {
                    instrs.push(i);
                }
                MappedInstrs::Many(mut v) => {
                    instrs.append(&mut v);
                }
            }
        }
        self.instrs = instrs;
    }

    pub fn phi_dsts_ip(&self) -> Option<usize> {
        for (ip, instr) in self.instrs.iter().enumerate() {
            match &instr.op {
                Op::Annotate(_) => (),
                Op::PhiDsts(_) => return Some(ip),
                _ => break,
            }
        }
        None
    }

    pub fn phi_dsts(&self) -> Option<&super::op_misc::OpPhiDsts> {
        self.phi_dsts_ip().and_then(|ip| match &self.instrs[ip].op {
            Op::PhiDsts(phi) => Some(phi.deref()),
            _ => None,
        })
    }

    pub fn phi_dsts_mut(&mut self) -> Option<&mut super::op_misc::OpPhiDsts> {
        self.phi_dsts_ip()
            .and_then(|ip| match &mut self.instrs[ip].op {
                Op::PhiDsts(phi) => Some(phi.deref_mut()),
                _ => None,
            })
    }

    pub fn phi_srcs_ip(&self) -> Option<usize> {
        for (ip, instr) in self.instrs.iter().enumerate().rev() {
            match &instr.op {
                Op::Annotate(_) => (),
                Op::PhiSrcs(_) => return Some(ip),
                _ if instr.is_branch() => (),
                _ => break,
            }
        }
        None
    }
    pub fn phi_srcs(&self) -> Option<&super::op_misc::OpPhiSrcs> {
        self.phi_srcs_ip().and_then(|ip| match &self.instrs[ip].op {
            Op::PhiSrcs(phi) => Some(phi.deref()),
            _ => None,
        })
    }

    pub fn phi_srcs_mut(&mut self) -> Option<&mut super::op_misc::OpPhiSrcs> {
        self.phi_srcs_ip()
            .and_then(|ip| match &mut self.instrs[ip].op {
                Op::PhiSrcs(phi) => Some(phi.deref_mut()),
                _ => None,
            })
    }

    pub fn branch(&self) -> Option<&Instr> {
        self.instrs.last().filter(|&i| i.is_branch())
    }

    pub fn branch_ip(&self) -> Option<usize> {
        if let Some(i) = self.instrs.last() {
            if i.is_branch() {
                Some(self.instrs.len() - 1)
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn branch_mut(&mut self) -> Option<&mut Instr> {
        self.instrs.last_mut().filter(|i| i.is_branch())
    }

    pub fn falls_through(&self) -> bool {
        if let Some(i) = self.branch() {
            !i.is_branch_always_taken()
        } else {
            true
        }
    }
}

pub struct Function {
    pub ssa_alloc: super::SSAValueAllocator,
    pub phi_alloc: super::PhiAllocator,
    pub blocks: CFG<BasicBlock>,
}

impl Function {
    pub fn map_instrs(
        &mut self,
        mut map: impl FnMut(Instr, &mut super::SSAValueAllocator) -> MappedInstrs,
    ) {
        let alloc = &mut self.ssa_alloc;
        for b in &mut self.blocks {
            b.map_instrs(|i| map(i, alloc));
        }
    }
}

impl fmt::Display for Function {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut pred_width = 0;
        let mut dsts_width = 0;
        let mut op_width = 0;

        let mut blocks = Vec::new();
        for b in &self.blocks {
            let mut instrs = Vec::new();
            for i in &b.instrs {
                let mut pred = String::new();
                write!(pred, "{}", Fmt(|f| i.fmt_pred(f)))?;
                let mut dsts = String::new();
                write!(dsts, "{}", Fmt(|f| i.op.fmt_dsts(f)))?;
                let mut op = String::new();
                write!(op, "{}", Fmt(|f| i.op.fmt_op(f)))?;
                let mut deps = String::new();
                write!(deps, "{}", i.deps)?;

                pred_width = max(pred_width, pred.len());
                dsts_width = max(dsts_width, dsts.len());
                op_width = max(op_width, op.len());
                let is_annotation = matches!(i.op, Op::Annotate(_));

                instrs.push((pred, dsts, op, deps, is_annotation));
            }
            blocks.push(instrs);
        }

        for (i, b) in blocks.into_iter().enumerate() {
            let u = if self.blocks[i].uniform { ".u" } else { "" };
            write!(f, "block{u} {} {} [", i, self.blocks[i].label)?;
            for (pi, p) in self.blocks.pred_indices(i).iter().enumerate() {
                if pi > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{p}")?;
            }
            write!(f, "] -> {{\n")?;

            for (pred, dsts, op, deps, is_annotation) in b.into_iter() {
                let eq_sym = if dsts.is_empty() { " " } else { "=" };
                if is_annotation {
                    write!(f, "\n{op}\n")?;
                } else if deps.is_empty() {
                    write!(f, "{pred:<pred_width$} {dsts:<dsts_width$} {eq_sym} {op}\n",)?;
                } else {
                    write!(
                        f,
                        "{pred:<pred_width$} {dsts:<dsts_width$} {eq_sym} \
                         {op:<op_width$} //{deps}\n",
                    )?;
                }
            }

            write!(f, "}} -> [")?;
            for (si, s) in self.blocks.succ_indices(i).iter().enumerate() {
                if si > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{s}")?;
            }
            write!(f, "]\n")?;
        }
        Ok(())
    }
}

impl Index<InstrIdx> for Function {
    type Output = Instr;

    fn index(&self, index: InstrIdx) -> &Self::Output {
        let block_idx: usize = index.block_idx as usize;
        let instr_idx: usize = index.instr_idx as usize;
        &self.blocks[block_idx].instrs[instr_idx]
    }
}
