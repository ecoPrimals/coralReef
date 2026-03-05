// Copyright © 2022 Collabora, Ltd.
// SPDX-License-Identifier: MIT

#![allow(clippy::wildcard_imports)]

use super::ir::*;

mod emit;

pub use emit::SSABuilder;

pub trait Builder {
    fn push_instr(&mut self, instr: Instr) -> &mut Instr;

    fn sm(&self) -> u8;

    fn push_op(&mut self, op: impl Into<Op>) -> &mut Instr {
        self.push_instr(Instr::new(op))
    }

    fn predicate(&mut self, pred: Pred) -> PredicatedBuilder<'_, Self>
    where
        Self: Sized,
    {
        PredicatedBuilder { b: self, pred }
    }

    fn lop2_to(&mut self, dst: Dst, op: LogicOp2, mut x: Src, mut y: Src) {
        let is_predicate = match &dst {
            Dst::None => panic!("No LOP destination"),
            Dst::SSA(ssa) => ssa.is_predicate(),
            Dst::Reg(reg) => reg.is_predicate(),
        };
        assert!(x.is_predicate() == is_predicate);
        assert!(y.is_predicate() == is_predicate);

        if self.sm() >= 70 {
            let mut op = op.to_lut();
            if x.src_mod.is_bnot() {
                op = LogicOp3::new_lut(&|x, y, _| op.eval(!x, y, 0));
                x.src_mod = SrcMod::None;
            }
            if y.src_mod.is_bnot() {
                op = LogicOp3::new_lut(&|x, y, _| op.eval(x, !y, 0));
                y.src_mod = SrcMod::None;
            }
            if is_predicate {
                self.push_op(OpPLop3 {
                    dsts: [dst, Dst::None],
                    srcs: [x, y, true.into()],
                    ops: [op, LogicOp3::new_const(false)],
                });
            } else {
                self.push_op(OpLop3 {
                    dst,
                    srcs: [x, y, 0.into()],
                    op,
                });
            }
        } else {
            if is_predicate {
                let mut x = x;
                let cmp_op = match op {
                    LogicOp2::And => PredSetOp::And,
                    LogicOp2::Or => PredSetOp::Or,
                    LogicOp2::Xor => PredSetOp::Xor,
                    LogicOp2::PassB => {
                        // Pass through B by AND with PT
                        x = true.into();
                        PredSetOp::And
                    }
                };
                self.push_op(OpPSetP {
                    dsts: [dst, Dst::None],
                    ops: [cmp_op, PredSetOp::And],
                    srcs: [x, y, true.into()],
                });
            } else {
                self.push_op(OpLop2 {
                    dst,
                    srcs: [x, y],
                    op,
                });
            }
        }
    }

    fn prmt_to(&mut self, dst: Dst, x: Src, y: Src, sel: [u8; 4]) {
        if sel == [0, 1, 2, 3] {
            self.copy_to(dst, x);
        } else if sel == [4, 5, 6, 7] {
            self.copy_to(dst, y);
        } else {
            let mut sel_u32 = 0;
            for i in 0..4 {
                assert!(sel[i] < 16);
                sel_u32 |= u32::from(sel[i]) << (i * 4);
            }

            self.push_op(OpPrmt {
                dst,
                srcs: [x, y],
                sel: sel_u32.into(),
                mode: PrmtMode::Index,
            });
        }
    }

    fn copy_to(&mut self, dst: Dst, src: Src) {
        self.push_op(OpCopy { dst, src });
    }

    fn swap(&mut self, x: RegRef, y: RegRef) {
        assert!(x.file() == y.file());
        self.push_op(OpSwap {
            dsts: [x.into(), y.into()],
            srcs: [y.into(), x.into()],
        });
    }
}

pub struct InstrBuilder<'a> {
    instrs: MappedInstrs,
    sm: &'a ShaderModelInfo,
}

impl<'a> InstrBuilder<'a> {
    pub fn new(sm: &'a ShaderModelInfo) -> Self {
        Self {
            instrs: MappedInstrs::None,
            sm,
        }
    }
}

impl InstrBuilder<'_> {
    pub fn into_vec(self) -> Vec<Instr> {
        match self.instrs {
            MappedInstrs::None => Vec::new(),
            MappedInstrs::One(i) => vec![i],
            MappedInstrs::Many(v) => v,
        }
    }

    pub fn into_mapped_instrs(self) -> MappedInstrs {
        self.instrs
    }
}

impl Builder for InstrBuilder<'_> {
    fn push_instr(&mut self, instr: Instr) -> &mut Instr {
        self.instrs.push(instr);
        self.instrs.last_mut().unwrap()
    }

    fn sm(&self) -> u8 {
        self.sm.sm()
    }
}

pub struct SSAInstrBuilder<'a> {
    b: InstrBuilder<'a>,
    alloc: &'a mut SSAValueAllocator,
}

impl<'a> SSAInstrBuilder<'a> {
    pub fn new(sm: &'a ShaderModelInfo, alloc: &'a mut SSAValueAllocator) -> Self {
        Self {
            b: InstrBuilder::new(sm),
            alloc,
        }
    }
}

impl SSAInstrBuilder<'_> {
    pub fn into_vec(self) -> Vec<Instr> {
        self.b.into_vec()
    }

    pub fn into_mapped_instrs(self) -> MappedInstrs {
        self.b.into_mapped_instrs()
    }
}

impl Builder for SSAInstrBuilder<'_> {
    fn push_instr(&mut self, instr: Instr) -> &mut Instr {
        self.b.push_instr(instr)
    }

    fn sm(&self) -> u8 {
        self.b.sm()
    }
}

impl SSABuilder for SSAInstrBuilder<'_> {
    fn alloc_ssa(&mut self, file: RegFile) -> SSAValue {
        self.alloc.alloc(file)
    }

    fn alloc_ssa_vec(&mut self, file: RegFile, comps: u8) -> SSARef {
        self.alloc.alloc_vec(file, comps)
    }
}

pub struct PredicatedBuilder<'a, T: Builder> {
    b: &'a mut T,
    pred: Pred,
}

impl<T: Builder> Builder for PredicatedBuilder<'_, T> {
    fn push_instr(&mut self, instr: Instr) -> &mut Instr {
        let mut instr = instr;
        assert!(instr.pred.is_true());
        instr.pred = self.pred;
        self.b.push_instr(instr)
    }

    fn sm(&self) -> u8 {
        self.b.sm()
    }
}

impl<T: SSABuilder> SSABuilder for PredicatedBuilder<'_, T> {
    fn alloc_ssa(&mut self, file: RegFile) -> SSAValue {
        self.b.alloc_ssa(file)
    }

    fn alloc_ssa_vec(&mut self, file: RegFile, comps: u8) -> SSARef {
        self.b.alloc_ssa_vec(file, comps)
    }
}

pub struct UniformBuilder<'a, T: Builder> {
    b: &'a mut T,
    uniform: bool,
}

impl<'a, T: Builder> UniformBuilder<'a, T> {
    pub fn new(b: &'a mut T, uniform: bool) -> Self {
        Self { b, uniform }
    }
}

impl<T: Builder> Builder for UniformBuilder<'_, T> {
    fn push_instr(&mut self, instr: Instr) -> &mut Instr {
        self.b.push_instr(instr)
    }

    fn sm(&self) -> u8 {
        self.b.sm()
    }
}

impl<T: SSABuilder> SSABuilder for UniformBuilder<'_, T> {
    fn alloc_ssa(&mut self, file: RegFile) -> SSAValue {
        let file = if self.uniform {
            file.to_uniform().unwrap()
        } else {
            file
        };
        self.b.alloc_ssa(file)
    }

    fn alloc_ssa_vec(&mut self, file: RegFile, comps: u8) -> SSARef {
        let file = if self.uniform {
            file.to_uniform().unwrap()
        } else {
            file
        };
        self.b.alloc_ssa_vec(file, comps)
    }
}
