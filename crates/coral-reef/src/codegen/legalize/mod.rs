// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)

//! Legalization pass — rewrites IR so every instruction satisfies
//! hardware register-file constraints for the target shader model.

#![allow(clippy::wildcard_imports)]

mod helpers;

pub use helpers::{
    LegalizeBuildHelpers, PadValue, src_is_reg, src_is_upred_reg, swap_srcs_if_not_reg,
};

use super::const_tracker::ConstTracker;
use super::ir::*;
use super::liveness::{BlockLiveness, Liveness, SimpleLiveness};

use coral_reef_stubs::fxhash::{FxHashMap, FxHashSet};

pub struct LegalizeBuilder<'a> {
    b: SSAInstrBuilder<'a>,
    const_tracker: &'a mut ConstTracker,
}

impl<'a> LegalizeBuilder<'a> {
    fn new(
        sm: &'a dyn ShaderModel,
        alloc: &'a mut SSAValueAllocator,
        const_tracker: &'a mut ConstTracker,
    ) -> Self {
        LegalizeBuilder {
            b: SSAInstrBuilder::new(sm, alloc),
            const_tracker,
        }
    }

    pub fn into_vec(self) -> Vec<Instr> {
        self.b.into_vec()
    }

    pub fn into_mapped_instrs(self) -> MappedInstrs {
        self.b.into_mapped_instrs()
    }
}

impl<'a> Builder for LegalizeBuilder<'a> {
    fn push_instr(&mut self, instr: Instr) -> &mut Instr {
        self.b.push_instr(instr)
    }

    fn sm(&self) -> u8 {
        self.b.sm()
    }

    fn copy_to(&mut self, dst: Dst, mut src: Src) {
        if let Some(ssa_ref) = src.as_ssa() {
            if let &[ssa_value] = &ssa_ref[..] {
                if let Some(new_src) = self.const_tracker.get(&ssa_value) {
                    src = new_src.clone();
                }
            }
        }
        self.b.copy_to(dst, src);
    }
}

impl<'a> SSABuilder for LegalizeBuilder<'a> {
    fn alloc_ssa(&mut self, file: RegFile) -> SSAValue {
        self.b.alloc_ssa(file)
    }

    fn alloc_ssa_vec(&mut self, file: RegFile, comps: u8) -> SSARef {
        self.b.alloc_ssa_vec(file, comps)
    }
}

impl LegalizeBuildHelpers for LegalizeBuilder<'_> {}

fn legalize_instr(
    sm: &dyn ShaderModel,
    b: &mut LegalizeBuilder,
    bl: &impl BlockLiveness,
    block_uniform: bool,
    pinned: &FxHashSet<SSARef>,
    ip: usize,
    instr: &mut Instr,
) -> Result<(), crate::CompileError> {
    match &instr.op {
        Op::Annotate(_) => {
            return Ok(());
        }
        Op::Undef(_)
        | Op::PhiSrcs(_)
        | Op::PhiDsts(_)
        | Op::Pin(_)
        | Op::Unpin(_)
        | Op::RegOut(_) => {
            debug_assert!(instr.pred.is_true());
            return Ok(());
        }
        Op::Copy(_) => {
            return Ok(());
        }
        Op::SrcBar(_) => {
            return Ok(());
        }
        Op::Swap(_) | Op::ParCopy(_) => {
            super::ice!("ICE: Unsupported instruction");
        }
        _ => (),
    }

    if !instr.is_uniform() {
        b.copy_pred_if_upred(&mut instr.pred);
    }

    let src_types = instr.src_types();
    for (i, src) in instr.srcs_mut().iter_mut().enumerate() {
        if matches!(src.reference, SrcRef::Imm32(_)) {
            if let Some(u) = src.as_u32(src_types[i]) {
                *src = u.into();
            }
        }
        b.copy_src_if_not_same_file(src);

        if !block_uniform {
            match &mut src.reference {
                SrcRef::SSA(vec) => {
                    if vec.is_uniform() && vec.comps() > 1 && !pinned.contains(vec) {
                        b.copy_ssa_ref(vec, vec.file().to_warp());
                    }
                }
                SrcRef::CBuf(CBufRef {
                    buf: CBuf::BindlessSSA(handle),
                    ..
                }) => assert!(pinned.contains(&SSARef::new(handle))),
                _ => (),
            }
        }
    }

    match &mut instr.op {
        Op::Break(op) => {
            let bar_in_ssa = op
                .bar_in()
                .reference
                .as_ssa()
                .expect("bar_in source must be SSA value");
            if !op.bar_out.is_none() && bl.is_live_after_ip(&bar_in_ssa[0], ip) {
                let gpr = b.bmov_to_gpr(op.bar_in().clone());
                let tmp = b.bmov_to_bar(gpr.into());
                op.srcs[0] = tmp.into();
            }
        }
        Op::BSSy(op) => {
            let bar_in_ssa = op
                .bar_in()
                .reference
                .as_ssa()
                .expect("bar_in source must be SSA value");
            if !op.bar_out.is_none() && bl.is_live_after_ip(&bar_in_ssa[0], ip) {
                let gpr = b.bmov_to_gpr(op.bar_in().clone());
                let tmp = b.bmov_to_bar(gpr.into());
                op.srcs[0] = tmp.into();
            }
        }
        _ => (),
    }

    sm.legalize_op(b, &mut instr.op)?;

    let mut vec_src_map: FxHashMap<SSARef, SSARef> = FxHashMap::default();
    let mut vec_comps: FxHashSet<_> = FxHashSet::default();
    for src in instr.srcs_mut() {
        if let SrcRef::SSA(vec) = &src.reference {
            if vec.comps() == 1 {
                continue;
            }

            if let Some(new_vec) = vec_src_map.get(vec) {
                src.reference = new_vec.clone().into();
                continue;
            }

            let mut new_vec = vec.clone();
            for c in 0..vec.comps() {
                let ssa = vec[usize::from(c)];
                if vec_comps.contains(&ssa) {
                    let copy = b.alloc_ssa(ssa.file());
                    b.copy_to(copy.into(), ssa.into());
                    new_vec[usize::from(c)] = copy;
                } else {
                    vec_comps.insert(ssa);
                }
            }

            vec_src_map.insert(vec.clone(), new_vec.clone());
            src.reference = new_vec.into();
        }
    }
    Ok(())
}

impl Shader<'_> {
    /// Legalize IR for the target shader model.
    ///
    /// # Errors
    ///
    /// Returns `CompileError::UnsupportedArch` if the shader model is not supported.
    pub fn legalize(&mut self) -> Result<(), crate::CompileError> {
        let sm = self.sm;
        for f in &mut self.functions {
            let live = SimpleLiveness::for_function(f);
            let mut pinned: FxHashSet<_> = FxHashSet::default();
            let mut const_tracker = ConstTracker::new();

            for (bi, b) in f.blocks.iter_mut().enumerate() {
                let bl = live.block_live(bi);
                let bu = b.uniform;

                let mut instrs = Vec::new();
                for (ip, mut instr) in b.instrs.drain(..).enumerate() {
                    match &instr.op {
                        Op::Pin(pin) => {
                            if let Dst::SSA(ssa) = &pin.dst {
                                pinned.insert(ssa.clone());
                            }
                        }
                        Op::Copy(copy) => {
                            const_tracker.add_copy(copy);
                        }
                        _ => (),
                    }

                    let mut b = LegalizeBuilder::new(sm, &mut f.ssa_alloc, &mut const_tracker);
                    legalize_instr(sm, &mut b, bl, bu, &pinned, ip, &mut instr)?;
                    b.push_instr(instr);
                    instrs.append(&mut b.into_vec());
                }
                b.instrs = instrs;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::ir::{
        BasicBlock, ComputeShaderInfo, FRndMode, Function, Instr, LabelAllocator, OpCopy, OpExit,
        OpFAdd, OpRegOut, PhiAllocator, RegFile, SSAValueAllocator, Shader, ShaderInfo,
        ShaderIoInfo, ShaderModelInfo, ShaderStageInfo, Src, SrcMod, SrcRef, SrcSwizzle,
    };
    use coral_reef_stubs::cfg::CFGBuilder;

    fn make_shader_with_function(
        instrs: Vec<Instr>,
        ssa_alloc: SSAValueAllocator,
    ) -> Shader<'static> {
        let sm = Box::leak(Box::new(ShaderModelInfo::new(70, 64)));
        let mut label_alloc = LabelAllocator::new();
        let mut cfg_builder = CFGBuilder::new();
        let block = BasicBlock {
            label: label_alloc.alloc(),
            uniform: false,
            instrs,
        };
        cfg_builder.add_block(block);
        let function = Function {
            ssa_alloc,
            phi_alloc: PhiAllocator::new(),
            blocks: cfg_builder.build(),
        };
        Shader {
            sm,
            info: ShaderInfo {
                max_warps_per_sm: 0,
                gpr_count: 0,
                control_barrier_count: 0,
                instr_count: 0,
                static_cycle_count: 0,
                spills_to_mem: 0,
                fills_from_mem: 0,
                spills_to_reg: 0,
                fills_from_reg: 0,
                shared_local_mem_size: 0,
                max_crs_depth: 0,
                uses_global_mem: false,
                writes_global_mem: false,
                uses_fp64: false,
                stage: ShaderStageInfo::Compute(ComputeShaderInfo {
                    local_size: [1, 1, 1],
                    shared_mem_size: 0,
                }),
                io: ShaderIoInfo::None,
            },
            functions: vec![function],
            fma_policy: crate::FmaPolicy::default(),
        }
    }

    #[test]
    fn test_src_is_reg_zero_true_false() {
        assert!(src_is_reg(&Src::ZERO, RegFile::GPR));
        assert!(src_is_reg(&true.into(), RegFile::Pred));
        assert!(src_is_reg(&false.into(), RegFile::Pred));
        assert!(!src_is_reg(&true.into(), RegFile::GPR));
        assert!(!src_is_reg(&false.into(), RegFile::GPR));
    }

    #[test]
    fn test_src_is_reg_imm32_cbuf() {
        assert!(!src_is_reg(&Src::new_imm_u32(42), RegFile::GPR));
    }

    #[test]
    fn test_src_is_reg_ssa_matching_file() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let gpr = ssa_alloc.alloc(RegFile::GPR);
        let pred = ssa_alloc.alloc(RegFile::Pred);
        assert!(src_is_reg(&gpr.into(), RegFile::GPR));
        assert!(src_is_reg(&pred.into(), RegFile::Pred));
        assert!(!src_is_reg(&gpr.into(), RegFile::Pred));
        assert!(!src_is_reg(&pred.into(), RegFile::GPR));
    }

    #[test]
    fn test_src_is_upred_reg() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let upred = ssa_alloc.alloc(RegFile::UPred);
        let src = Src {
            reference: SrcRef::SSA(upred.into()),
            modifier: SrcMod::None,
            swizzle: SrcSwizzle::None,
        };
        assert!(src_is_upred_reg(&src));
    }

    #[test]
    fn test_src_is_upred_reg_false_for_pred() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let pred = ssa_alloc.alloc(RegFile::Pred);
        let src = Src {
            reference: SrcRef::SSA(pred.into()),
            modifier: SrcMod::None,
            swizzle: SrcSwizzle::None,
        };
        assert!(!src_is_upred_reg(&src));
    }

    #[test]
    fn test_swap_srcs_if_not_reg_swaps_when_x_imm_y_reg() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let gpr = ssa_alloc.alloc(RegFile::GPR);
        let mut x = Src::new_imm_u32(1);
        let mut y = gpr.into();
        assert!(swap_srcs_if_not_reg(&mut x, &mut y, RegFile::GPR));
        assert!(matches!(x.reference, SrcRef::SSA(_)));
        assert!(matches!(y.reference, SrcRef::Imm32(1)));
    }

    #[test]
    fn test_swap_srcs_if_not_reg_no_swap_when_both_reg() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let gpr_a = ssa_alloc.alloc(RegFile::GPR);
        let gpr_b = ssa_alloc.alloc(RegFile::GPR);
        let mut x = gpr_a.into();
        let mut y = gpr_b.into();
        assert!(!swap_srcs_if_not_reg(&mut x, &mut y, RegFile::GPR));
    }

    #[test]
    fn test_legalize_preserves_simple_copy() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let dst = ssa_alloc.alloc(RegFile::GPR);
        let mut shader = make_shader_with_function(
            vec![
                Instr::new(OpCopy {
                    dst: dst.into(),
                    src: Src::ZERO,
                }),
                Instr::new(OpRegOut {
                    srcs: vec![dst.into()],
                }),
                Instr::new(OpExit {}),
            ],
            ssa_alloc,
        );
        let result = shader.legalize();
        assert!(result.is_ok());
        let block = &shader.functions[0].blocks[0];
        assert!(!block.instrs.is_empty());
    }

    #[test]
    fn test_legalize_preserves_fadd() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let dst_a = ssa_alloc.alloc(RegFile::GPR);
        let dst_b = ssa_alloc.alloc(RegFile::GPR);
        let mut shader = make_shader_with_function(
            vec![
                Instr::new(OpCopy {
                    dst: dst_a.into(),
                    src: Src::new_imm_u32(1),
                }),
                Instr::new(OpFAdd {
                    dst: dst_b.into(),
                    srcs: [dst_a.into(), dst_a.into()],
                    saturate: false,
                    rnd_mode: FRndMode::NearestEven,
                    ftz: false,
                }),
                Instr::new(OpRegOut {
                    srcs: vec![dst_b.into()],
                }),
                Instr::new(OpExit {}),
            ],
            ssa_alloc,
        );
        let result = shader.legalize();
        assert!(result.is_ok());
    }
}
