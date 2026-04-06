// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)

use super::ir::*;

mod emit;

pub use emit::SSABuilder;

pub trait Builder {
    fn push_instr(&mut self, instr: Instr) -> &mut Instr;

    fn sm(&self) -> u8;

    fn is_amd(&self) -> bool {
        false
    }

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
            Dst::None => {
                debug_assert!(false, "No LOP destination");
                return;
            }
            Dst::SSA(ssa) => ssa.is_predicate(),
            Dst::Reg(reg) => reg.is_predicate(),
        };
        assert!(x.is_predicate() == is_predicate);
        assert!(y.is_predicate() == is_predicate);

        if self.sm() >= 70 {
            let mut op = op.to_lut();
            if x.modifier.is_bnot() {
                op = LogicOp3::new_lut(&|x, y, _| op.eval(!x, y, 0));
                x.modifier = SrcMod::None;
            }
            if y.modifier.is_bnot() {
                op = LogicOp3::new_lut(&|x, y, _| op.eval(x, !y, 0));
                y.modifier = SrcMod::None;
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
                srcs: [x, y, sel_u32.into()],
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
    sm: &'a dyn ShaderModel,
}

impl<'a> InstrBuilder<'a> {
    pub fn new(sm: &'a dyn ShaderModel) -> Self {
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
        // Infallible: push() guarantees the vec is non-empty, so last_mut() always returns Some.
        self.instrs
            .last_mut()
            .expect("just pushed instr; vec is non-empty")
    }

    fn sm(&self) -> u8 {
        self.sm.sm()
    }

    fn is_amd(&self) -> bool {
        self.sm.is_amd()
    }
}

pub struct SSAInstrBuilder<'a> {
    b: InstrBuilder<'a>,
    alloc: &'a mut SSAValueAllocator,
}

impl<'a> SSAInstrBuilder<'a> {
    pub fn new(sm: &'a dyn ShaderModel, alloc: &'a mut SSAValueAllocator) -> Self {
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

    fn is_amd(&self) -> bool {
        self.b.is_amd()
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

    fn is_amd(&self) -> bool {
        self.b.is_amd()
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
    pub const fn new(b: &'a mut T, uniform: bool) -> Self {
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

    fn is_amd(&self) -> bool {
        self.b.is_amd()
    }
}

impl<T: SSABuilder> SSABuilder for UniformBuilder<'_, T> {
    fn alloc_ssa(&mut self, file: RegFile) -> SSAValue {
        let file = if self.uniform {
            file.to_uniform().unwrap_or(file)
        } else {
            file
        };
        self.b.alloc_ssa(file)
    }

    fn alloc_ssa_vec(&mut self, file: RegFile, comps: u8) -> SSARef {
        let file = if self.uniform {
            file.to_uniform().unwrap_or(file)
        } else {
            file
        };
        self.b.alloc_ssa_vec(file, comps)
    }
}

#[cfg(test)]
mod tests_emit;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::ir::{
        FloatCmpOp, IntCmpOp, IntCmpType, LogicOp2, Op, OpNop, RegFile, RroOp, ShaderModelInfo,
        TranscendentalOp,
    };

    pub(super) fn make_sm70() -> ShaderModelInfo {
        ShaderModelInfo::new(70, 64)
    }

    pub(super) fn make_sm50() -> ShaderModelInfo {
        ShaderModelInfo::new(50, 64)
    }

    #[test]
    fn test_instr_builder_new_creates_empty() {
        let sm = make_sm70();
        let builder = InstrBuilder::new(&sm);
        let instrs = builder.into_vec();
        assert!(instrs.is_empty());
    }

    #[test]
    fn test_push_instr_and_build_block() {
        let sm = make_sm70();
        let mut builder = InstrBuilder::new(&sm);
        builder.push_op(OpNop { label: None });
        builder.push_op(OpNop { label: None });
        let instrs = builder.into_vec();
        assert_eq!(instrs.len(), 2);
        assert!(matches!(instrs[0].op, Op::Nop(_)));
        assert!(matches!(instrs[1].op, Op::Nop(_)));
    }

    #[test]
    fn test_alloc_ssa_unique_values() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let mut builder = SSAInstrBuilder::new(&sm, &mut alloc);
        let a = builder.alloc_ssa(RegFile::GPR);
        let b = builder.alloc_ssa(RegFile::GPR);
        let c = builder.alloc_ssa(RegFile::Pred);
        assert_ne!(a.idx(), b.idx());
        assert_ne!(b.idx(), c.idx());
        assert_ne!(a.idx(), c.idx());
        assert!(a.file().is_gpr());
        assert!(c.file().is_predicate());
    }

    #[test]
    fn test_alloc_ssa_vec_component_count() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let mut builder = SSAInstrBuilder::new(&sm, &mut alloc);
        let vec2 = builder.alloc_ssa_vec(RegFile::GPR, 2);
        let vec4 = builder.alloc_ssa_vec(RegFile::GPR, 4);
        assert_eq!(vec2.comps(), 2);
        assert_eq!(vec4.comps(), 4);
        assert_eq!(vec2.len(), 2);
        assert_eq!(vec4.len(), 4);
    }

    #[test]
    fn test_lop2_helper() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let mut builder = SSAInstrBuilder::new(&sm, &mut alloc);
        let x = builder.alloc_ssa(RegFile::GPR);
        let y = builder.alloc_ssa(RegFile::GPR);
        let result = builder.lop2(LogicOp2::And, x.into(), y.into());
        let instrs = builder.into_vec();
        assert_eq!(instrs.len(), 1);
        assert!(result.file().is_gpr());
    }

    #[test]
    fn test_mufu_emits_op() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let mut builder = SSAInstrBuilder::new(&sm, &mut alloc);
        let src = builder.alloc_ssa(RegFile::GPR);
        let result = builder.transcendental(TranscendentalOp::Sin, src.into());
        let instrs = builder.into_vec();
        assert_eq!(instrs.len(), 1);
        assert!(result.file().is_gpr());
        let Op::Transcendental(op) = &instrs[0].op else {
            panic!("expected MuFu");
        };
        assert!(matches!(op.op, TranscendentalOp::Sin));
    }

    #[test]
    fn test_fsin_sm70_uses_fmul_and_mufu() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let mut builder = SSAInstrBuilder::new(&sm, &mut alloc);
        let src = builder.alloc_ssa(RegFile::GPR);
        let _ = builder.fsin(src.into());
        let instrs = builder.into_vec();
        assert_eq!(instrs.len(), 2, "fsin on SM70: fmul + transcendental");
        assert!(matches!(instrs[0].op, Op::FMul(_)));
        assert!(matches!(instrs[1].op, Op::Transcendental(_)));
    }

    #[test]
    fn test_fcos_sm70_uses_fmul_and_mufu() {
        let sm = make_sm50();
        let mut alloc = SSAValueAllocator::new();
        let mut builder = SSAInstrBuilder::new(&sm, &mut alloc);
        let src = builder.alloc_ssa(RegFile::GPR);
        let _ = builder.fcos(src.into());
        let instrs = builder.into_vec();
        assert_eq!(instrs.len(), 2, "fcos on SM50: rro + transcendental");
        assert!(matches!(instrs[0].op, Op::Rro(_)));
        let Op::Rro(rro) = &instrs[0].op else {
            unreachable!()
        };
        assert!(matches!(rro.op, RroOp::SinCos));
        assert!(matches!(instrs[1].op, Op::Transcendental(_)));
    }

    #[test]
    fn test_fexp2_sm70_passes_through_to_mufu() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let mut builder = SSAInstrBuilder::new(&sm, &mut alloc);
        let src = builder.alloc_ssa(RegFile::GPR);
        let _ = builder.fexp2(src.into());
        let instrs = builder.into_vec();
        assert_eq!(instrs.len(), 1, "fexp2 on SM70: direct transcendental");
        let Op::Transcendental(op) = &instrs[0].op else {
            panic!("expected MuFu");
        };
        assert!(matches!(op.op, TranscendentalOp::Exp2));
    }

    #[test]
    fn test_fexp2_sm50_uses_rro_and_mufu() {
        let sm = make_sm50();
        let mut alloc = SSAValueAllocator::new();
        let mut builder = SSAInstrBuilder::new(&sm, &mut alloc);
        let src = builder.alloc_ssa(RegFile::GPR);
        let _ = builder.fexp2(src.into());
        let instrs = builder.into_vec();
        assert_eq!(instrs.len(), 2, "fexp2 on SM50: rro + transcendental");
        assert!(matches!(instrs[0].op, Op::Rro(_)));
        let Op::Rro(rro) = &instrs[0].op else {
            unreachable!()
        };
        assert!(matches!(rro.op, RroOp::Exp2));
    }

    #[test]
    fn test_fadd_fmul_fset() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let mut builder = SSAInstrBuilder::new(&sm, &mut alloc);
        let x = builder.alloc_ssa(RegFile::GPR);
        let y = builder.alloc_ssa(RegFile::GPR);
        let _ = builder.fadd(x.into(), y.into());
        let _ = builder.fmul(x.into(), y.into());
        let _ = builder.fset(FloatCmpOp::OrdEq, x.into(), y.into());
        let instrs = builder.into_vec();
        assert_eq!(instrs.len(), 3);
        assert!(matches!(instrs[0].op, Op::FAdd(_)));
        assert!(matches!(instrs[1].op, Op::FMul(_)));
        assert!(matches!(instrs[2].op, Op::FSet(_)));
    }

    #[test]
    fn test_sel_gpr() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let mut builder = SSAInstrBuilder::new(&sm, &mut alloc);
        let cond = builder.alloc_ssa(RegFile::Pred);
        let x = builder.alloc_ssa(RegFile::GPR);
        let y = builder.alloc_ssa(RegFile::GPR);
        let result = builder.sel(cond.into(), x.into(), y.into());
        let instrs = builder.into_vec();
        assert_eq!(instrs.len(), 1);
        assert!(result.file().is_gpr());
        assert!(matches!(instrs[0].op, Op::Sel(_)));
    }

    #[test]
    fn test_copy_allocates_correct_file() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let mut builder = SSAInstrBuilder::new(&sm, &mut alloc);
        let gpr = builder.alloc_ssa(RegFile::GPR);
        let pred = builder.alloc_ssa(RegFile::Pred);
        let gpr_result = builder.copy(gpr.into());
        let pred_result = builder.copy(pred.into());
        assert!(gpr_result.file().is_gpr());
        assert!(pred_result.file().is_predicate());
    }

    #[test]
    fn test_shl_shr_urol_uror() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let mut builder = SSAInstrBuilder::new(&sm, &mut alloc);
        let x = builder.alloc_ssa(RegFile::GPR);
        let shift = builder.alloc_ssa(RegFile::GPR);
        let _ = builder.shl(x.into(), shift.into());
        let _ = builder.shr(x.into(), shift.into(), false);
        let _ = builder.urol(x.into(), shift.into());
        let _ = builder.uror(x.into(), shift.into());
        let instrs = builder.into_vec();
        assert_eq!(instrs.len(), 4);
        assert!(matches!(instrs[0].op, Op::Shf(_)));
        assert!(matches!(instrs[1].op, Op::Shf(_)));
        assert!(matches!(instrs[2].op, Op::Shf(_)));
        assert!(matches!(instrs[3].op, Op::Shf(_)));
    }

    #[test]
    fn test_iadd_ineg_imul() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let mut builder = SSAInstrBuilder::new(&sm, &mut alloc);
        let x = builder.alloc_ssa(RegFile::GPR);
        let y = builder.alloc_ssa(RegFile::GPR);
        let _ = builder.iadd(x.into(), y.into(), 0.into());
        let _ = builder.ineg(x.into());
        let _ = builder.imul(x.into(), y.into());
        let instrs = builder.into_vec();
        assert_eq!(instrs.len(), 3);
        assert!(matches!(instrs[0].op, Op::IAdd3(_)));
        assert!(matches!(instrs[1].op, Op::IAdd3(_)));
        assert!(matches!(instrs[2].op, Op::IMad(_)));
    }

    #[test]
    fn test_isetp_brev() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let mut builder = SSAInstrBuilder::new(&sm, &mut alloc);
        let x = builder.alloc_ssa(RegFile::GPR);
        let y = builder.alloc_ssa(RegFile::GPR);
        let pred = builder.isetp(IntCmpType::U32, IntCmpOp::Eq, x.into(), y.into());
        assert!(pred.file().is_predicate());
        let _ = builder.brev(x.into());
        let instrs = builder.into_vec();
        assert_eq!(instrs.len(), 2);
        assert!(matches!(instrs[0].op, Op::ISetP(_)));
        assert!(matches!(instrs[1].op, Op::BRev(_)));
    }

    #[test]
    fn test_prmt_identity_optimizes_to_copy() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let mut builder = SSAInstrBuilder::new(&sm, &mut alloc);
        let x = builder.alloc_ssa(RegFile::GPR);
        let y = builder.alloc_ssa(RegFile::GPR);
        let _ = builder.prmt(x.into(), y.into(), [0, 1, 2, 3]);
        let instrs = builder.into_vec();
        assert_eq!(instrs.len(), 1);
        assert!(matches!(instrs[0].op, Op::Copy(_)));
    }

    #[test]
    fn test_prmt_src1_identity_optimizes_to_copy() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let mut builder = SSAInstrBuilder::new(&sm, &mut alloc);
        let x = builder.alloc_ssa(RegFile::GPR);
        let y = builder.alloc_ssa(RegFile::GPR);
        let _ = builder.prmt(x.into(), y.into(), [4, 5, 6, 7]);
        let instrs = builder.into_vec();
        assert_eq!(instrs.len(), 1);
        assert!(matches!(instrs[0].op, Op::Copy(_)));
    }

    #[test]
    fn test_lop2_predicate_and() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let mut builder = SSAInstrBuilder::new(&sm, &mut alloc);
        let p1 = builder.alloc_ssa(RegFile::Pred);
        let p2 = builder.alloc_ssa(RegFile::Pred);
        let _ = builder.lop2(LogicOp2::And, p1.into(), p2.into());
        let instrs = builder.into_vec();
        assert_eq!(instrs.len(), 1);
        assert!(matches!(instrs[0].op, Op::PLop3(_) | Op::PSetP(_)));
    }

    #[test]
    fn test_predicated_builder_wraps_instrs() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let mut builder = SSAInstrBuilder::new(&sm, &mut alloc);
        let pred = builder.alloc_ssa(RegFile::Pred);
        let x = builder.alloc_ssa(RegFile::GPR);
        let y = builder.alloc_ssa(RegFile::GPR);
        {
            let mut pred_b = builder.predicate(pred.into());
            let _ = pred_b.fadd(x.into(), y.into());
        }
        let instrs = builder.into_vec();
        assert_eq!(instrs.len(), 1);
        assert!(!instrs[0].pred.is_true());
    }

    #[test]
    fn test_uniform_builder_allocates_uniform() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let mut builder = SSAInstrBuilder::new(&sm, &mut alloc);
        let mut uniform_b = crate::codegen::builder::UniformBuilder::new(&mut builder, true);
        let u = uniform_b.alloc_ssa(RegFile::GPR);
        assert!(u.file().is_uniform());
    }

    #[test]
    fn test_undef() {
        let sm = make_sm70();
        let mut alloc = SSAValueAllocator::new();
        let mut builder = SSAInstrBuilder::new(&sm, &mut alloc);
        let result = builder.undef();
        assert!(result.file().is_gpr());
        let instrs = builder.into_vec();
        assert_eq!(instrs.len(), 1);
        assert!(matches!(instrs[0].op, Op::Undef(_)));
    }
}
