// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2024)

use super::ir::*;

use coral_reef_stubs::fxhash::FxHashMap;

fn should_lower_to_warp(
    sm: &dyn ShaderModel,
    instr: &Instr,
    r2ur: &FxHashMap<SSAValue, SSAValue>,
) -> bool {
    if !sm.op_can_be_uniform(&instr.op) {
        return true;
    }

    let mut num_non_uniform_srcs = 0;
    instr.for_each_ssa_use(|ssa| {
        if !ssa.is_uniform() || r2ur.contains_key(ssa) {
            num_non_uniform_srcs += 1;
        }
    });

    num_non_uniform_srcs >= 2
}

fn propagate_r2ur(instr: &mut Instr, r2ur: &FxHashMap<SSAValue, SSAValue>) -> bool {
    let mut progress = false;

    // We don't want Instr::for_each_ssa_use_mut() because it would treat
    // bindless cbuf sources as SSA sources.
    for src in instr.srcs_mut() {
        if let SrcRef::SSA(vec) = &mut src.reference {
            for ssa in &mut vec[..] {
                if let Some(r) = r2ur.get(ssa) {
                    progress = true;
                    *ssa = *r;
                }
            }
        }
    }

    progress
}

impl Shader<'_> {
    pub fn opt_uniform_instrs(&mut self) {
        let sm = self.sm;
        let mut r2ur = FxHashMap::default();
        let mut propagated_r2ur = false;
        self.map_instrs(|mut instr, alloc| {
            match &instr.op {
                Op::Redux(_)
                | Op::PhiDsts(_)
                | Op::PhiSrcs(_)
                | Op::Pin(_)
                | Op::Unpin(_)
                | Op::Vote(_) => MappedInstrs::One(instr),
                Op::Bra(bra) if sm.sm() >= 80 => match &instr.pred.predicate {
                    PredRef::SSA(ssa) if ssa.file() == RegFile::UPred => {
                        let bra_u = OpBra {
                            target: bra.target,
                            cond: instr.pred.into(),
                        };
                        MappedInstrs::One(Instr::new(bra_u))
                    }
                    _ => MappedInstrs::One(instr),
                },
                _ if instr.is_uniform() => {
                    let mut b = InstrBuilder::new(sm);
                    if should_lower_to_warp(sm, &instr, &r2ur) {
                        propagated_r2ur |= propagate_r2ur(&mut instr, &r2ur);
                        instr.for_each_ssa_def_mut(|ssa| {
                            let w = alloc.alloc(ssa.file().to_warp());
                            r2ur.insert(*ssa, w);
                            b.push_op(OpR2UR {
                                dst: (*ssa).into(),
                                src: w.into(),
                            });
                            *ssa = w;
                        });
                        let mut v = b.into_vec();
                        v.insert(0, instr);
                        MappedInstrs::Many(v)
                    } else {
                        // We may have non-uniform sources
                        instr.for_each_ssa_use_mut(|ssa| {
                            let file = ssa.file();
                            if !file.is_uniform() {
                                let u = alloc.alloc(
                                    file.to_uniform()
                                        .expect("non-uniform file must have uniform variant"),
                                );
                                b.push_op(OpR2UR {
                                    dst: u.into(),
                                    src: (*ssa).into(),
                                });
                                *ssa = u;
                            }
                        });
                        b.push_instr(instr);
                        b.into_mapped_instrs()
                    }
                }
                _ => {
                    propagated_r2ur |= propagate_r2ur(&mut instr, &r2ur);
                    MappedInstrs::One(instr)
                }
            }
        });

        if propagated_r2ur {
            self.opt_dce();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::ir::{
        Instr, OpCopy, OpIMad, OpNop, RegFile, SSAValueAllocator, ShaderModelInfo, Src,
    };
    use coral_reef_stubs::fxhash::FxHashMap;

    fn make_sm70() -> ShaderModelInfo {
        ShaderModelInfo::new(70, 64)
    }

    fn make_sm75() -> ShaderModelInfo {
        ShaderModelInfo::new(75, 64)
    }

    #[test]
    fn test_should_lower_to_warp_op_cannot_be_uniform() {
        let sm = make_sm70();
        let instr = Instr::new(OpNop { label: None });
        let r2ur = FxHashMap::default();
        assert!(
            should_lower_to_warp(&sm, &instr, &r2ur),
            "OpNop cannot be uniform, should lower to warp"
        );
    }

    #[test]
    fn test_should_lower_to_warp_two_non_uniform_srcs() {
        let sm = make_sm75();
        let mut alloc = SSAValueAllocator::new();
        let ugpr = alloc.alloc(RegFile::UGPR);
        let gpr1 = alloc.alloc(RegFile::GPR);
        let gpr2 = alloc.alloc(RegFile::GPR);
        let instr = Instr::new(OpIMad {
            dst: ugpr.into(),
            srcs: [Src::from(gpr1), Src::from(gpr2), Src::ZERO],
            signed: false,
        });
        let r2ur = FxHashMap::default();
        assert!(
            should_lower_to_warp(&sm, &instr, &r2ur),
            "2+ non-uniform srcs should lower to warp"
        );
    }

    #[test]
    fn test_should_lower_to_warp_one_non_uniform_src() {
        let sm = make_sm75();
        let mut alloc = SSAValueAllocator::new();
        let ugpr_dst = alloc.alloc(RegFile::UGPR);
        let gpr_src = alloc.alloc(RegFile::GPR);
        let instr = Instr::new(OpCopy {
            dst: ugpr_dst.into(),
            src: Src::from(gpr_src),
        });
        let r2ur = FxHashMap::default();
        assert!(
            !should_lower_to_warp(&sm, &instr, &r2ur),
            "1 non-uniform src should not lower to warp"
        );
    }

    #[test]
    fn test_should_lower_to_warp_r2ur_counts_as_non_uniform() {
        let sm = make_sm75();
        let mut alloc = SSAValueAllocator::new();
        let ugpr_dst = alloc.alloc(RegFile::UGPR);
        let ugpr_mapped = alloc.alloc(RegFile::UGPR);
        let gpr1 = alloc.alloc(RegFile::GPR);
        let gpr2 = alloc.alloc(RegFile::GPR);
        let mut r2ur = FxHashMap::default();
        r2ur.insert(gpr1, ugpr_mapped);
        let instr = Instr::new(OpIMad {
            dst: ugpr_dst.into(),
            srcs: [Src::from(gpr1), Src::from(gpr2), Src::ZERO],
            signed: false,
        });
        assert!(
            should_lower_to_warp(&sm, &instr, &r2ur),
            "gpr1 in r2ur + gpr2 non-uniform = 2, should lower to warp"
        );
    }

    #[test]
    fn test_propagate_r2ur_replaces_mapped_src() {
        let mut alloc = SSAValueAllocator::new();
        let gpr_src = alloc.alloc(RegFile::GPR);
        let ugpr_dst = alloc.alloc(RegFile::UGPR);
        let mut r2ur = FxHashMap::default();
        r2ur.insert(gpr_src, ugpr_dst);

        let mut instr = Instr::new(OpCopy {
            dst: ugpr_dst.into(),
            src: Src::from(gpr_src),
        });
        let progress = propagate_r2ur(&mut instr, &r2ur);
        assert!(progress);
        let mut uses = Vec::new();
        instr.for_each_ssa_use(|ssa| uses.push(*ssa));
        assert_eq!(uses, [ugpr_dst]);
    }

    #[test]
    fn test_propagate_r2ur_no_progress_when_not_mapped() {
        let mut alloc = SSAValueAllocator::new();
        let gpr_src = alloc.alloc(RegFile::GPR);
        let ugpr_dst = alloc.alloc(RegFile::UGPR);
        let r2ur = FxHashMap::default();

        let mut instr = Instr::new(OpCopy {
            dst: ugpr_dst.into(),
            src: Src::from(gpr_src),
        });
        let progress = propagate_r2ur(&mut instr, &r2ur);
        assert!(!progress);
    }
}
