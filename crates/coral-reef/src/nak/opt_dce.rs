// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT

#![allow(clippy::wildcard_imports)]

use super::{
    api::{DEBUG, GetDebugFlags},
    ir::*,
};
use coral_reef_stubs::bitset::BitSet;

struct DeadCodePass {
    any_dead: bool,
    new_live: bool,
    live_ssa: BitSet<SSAValue>,
    live_phi: BitSet<Phi>,
}

impl DeadCodePass {
    pub fn new() -> DeadCodePass {
        DeadCodePass {
            any_dead: false,
            new_live: false,
            live_ssa: BitSet::default(),
            live_phi: BitSet::default(),
        }
    }

    fn mark_ssa_live(&mut self, ssa: &SSAValue) {
        self.new_live |= self.live_ssa.insert(*ssa);
    }

    fn mark_src_live(&mut self, src: &Src) {
        for ssa in src.iter_ssa() {
            self.mark_ssa_live(ssa);
        }
    }

    fn mark_phi_live(&mut self, phi: Phi) {
        self.new_live |= self.live_phi.insert(phi);
    }

    fn is_dst_live(&self, dst: &Dst) -> bool {
        match dst {
            Dst::SSA(ssa) => {
                for val in ssa.iter() {
                    if self.live_ssa.contains(*val) {
                        return true;
                    }
                }
                false
            }
            Dst::None => false,
            Dst::Reg(_) => panic!("Invalid SSA destination"),
        }
    }

    fn is_phi_live(&self, phi: Phi) -> bool {
        self.live_phi.contains(phi)
    }

    fn is_instr_live(&self, instr: &Instr) -> bool {
        if instr.pred.is_false() {
            return false;
        }

        if !instr.can_eliminate() {
            return true;
        }

        for dst in instr.dsts() {
            if self.is_dst_live(dst) {
                return true;
            }
        }

        false
    }

    fn mark_instr(&mut self, instr: &Instr) {
        match &instr.op {
            Op::PhiSrcs(phi) => {
                assert!(instr.pred.is_true());
                for (phi, src) in phi.srcs.iter() {
                    if self.is_phi_live(*phi) {
                        self.mark_src_live(src);
                    } else {
                        self.any_dead = true;
                    }
                }
            }
            Op::PhiDsts(phi) => {
                assert!(instr.pred.is_true());
                for (phi, dst) in phi.dsts.iter() {
                    if self.is_dst_live(dst) {
                        self.mark_phi_live(*phi);
                    } else {
                        self.any_dead = true;
                    }
                }
            }
            Op::ParCopy(pcopy) => {
                assert!(instr.pred.is_true());
                for (dst, src) in pcopy.dsts_srcs.iter() {
                    if self.is_dst_live(dst) {
                        self.mark_src_live(src);
                    } else {
                        self.any_dead = true;
                    }
                }
            }
            _ => {
                if self.is_instr_live(instr) {
                    if let PredRef::SSA(ssa) = &instr.pred.pred_ref {
                        self.mark_ssa_live(ssa);
                    }

                    for src in instr.srcs() {
                        self.mark_src_live(src);
                    }
                } else {
                    self.any_dead = true;
                }
            }
        }
    }

    fn map_instr(&self, mut instr: Instr) -> MappedInstrs {
        let is_live = match &mut instr.op {
            Op::PhiSrcs(phi) => {
                phi.srcs.retain(|phi, _| self.is_phi_live(*phi));
                !phi.srcs.is_empty()
            }
            Op::PhiDsts(phi) => {
                phi.dsts.retain(|_, dst| self.is_dst_live(dst));
                !phi.dsts.is_empty()
            }
            Op::ParCopy(pcopy) => {
                pcopy.dsts_srcs.retain(|dst, _| self.is_dst_live(dst));
                !pcopy.dsts_srcs.is_empty()
            }
            _ => self.is_instr_live(&instr),
        };

        if is_live {
            MappedInstrs::One(instr)
        } else {
            if DEBUG.annotate() {
                MappedInstrs::One(Instr::new(OpAnnotate {
                    annotation: "killed by dce".into(),
                }))
            } else {
                MappedInstrs::None
            }
        }
    }

    pub fn run(&mut self, f: &mut Function) {
        loop {
            self.new_live = false;
            self.any_dead = false;

            for b in f.blocks.iter().rev() {
                for instr in b.instrs.iter().rev() {
                    self.mark_instr(instr);
                }
            }

            if !self.new_live {
                break;
            }
        }

        if self.any_dead {
            f.map_instrs(|instr, _| self.map_instr(instr));
        }
    }
}

impl Function {
    pub fn opt_dce(&mut self) {
        DeadCodePass::new().run(self);
    }
}

impl Shader<'_> {
    pub fn opt_dce(&mut self) {
        for f in &mut self.functions {
            f.opt_dce();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nak::ir::{
        BasicBlock, Function, Instr, LabelAllocator, Op, OpCopy, OpExit, OpRegOut, PhiAllocator,
        RegFile, Src, SSAValueAllocator,
    };
    use coral_reef_stubs::cfg::CFGBuilder;

    fn make_function_with_instrs(instrs: Vec<Instr>) -> Function {
        let mut label_alloc = LabelAllocator::new();
        let mut cfg_builder = CFGBuilder::new();
        let block = BasicBlock {
            label: label_alloc.alloc(),
            uniform: false,
            instrs,
        };
        cfg_builder.add_block(block);
        Function {
            ssa_alloc: SSAValueAllocator::new(),
            phi_alloc: PhiAllocator::new(),
            blocks: cfg_builder.build(),
        }
    }

    #[test]
    fn test_dce_eliminates_unreachable_instructions() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let dst_a = ssa_alloc.alloc(RegFile::GPR);
        let dst_b = ssa_alloc.alloc(RegFile::GPR);
        let mut f = make_function_with_instrs(vec![
            Instr::new(OpCopy {
                dst: dst_a.into(),
                src: Src::ZERO,
            }),
            Instr::new(OpCopy {
                dst: dst_b.into(),
                src: Src::ZERO,
            }),
            Instr::new(OpExit {}),
        ]);
        f.ssa_alloc = ssa_alloc;

        let before_count = f.blocks[0].instrs.len();
        assert_eq!(before_count, 3);

        f.opt_dce();

        let after_count = f.blocks[0].instrs.len();
        assert_eq!(after_count, 1, "only exit should remain");
        assert!(matches!(f.blocks[0].instrs[0].op, Op::Exit(_)));
    }

    #[test]
    fn test_dce_preserves_used_instructions() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let dst_a = ssa_alloc.alloc(RegFile::GPR);
        let dst_b = ssa_alloc.alloc(RegFile::GPR);
        let mut f = make_function_with_instrs(vec![
            Instr::new(OpCopy {
                dst: dst_a.into(),
                src: Src::ZERO,
            }),
            Instr::new(OpCopy {
                dst: dst_b.into(),
                src: dst_a.into(),
            }),
            Instr::new(OpRegOut {
                srcs: vec![dst_b.into()],
            }),
            Instr::new(OpExit {}),
        ]);
        f.ssa_alloc = ssa_alloc;

        f.opt_dce();

        let after_count = f.blocks[0].instrs.len();
        assert_eq!(after_count, 4, "all instructions should be preserved");
    }
}
