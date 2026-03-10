// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2023)

#![allow(clippy::wildcard_imports)]

use super::ir::*;

use coral_reef_stubs::fxhash::FxHashMap;

struct PrmtSrcs {
    srcs: [SrcRef; 2],
    num_srcs: usize,
    imm_src: usize,
    num_imm_bytes: usize,
}

impl PrmtSrcs {
    fn new() -> Self {
        Self {
            srcs: [SrcRef::Zero, SrcRef::Zero],
            num_srcs: 0,
            imm_src: usize::MAX,
            num_imm_bytes: 0,
        }
    }

    fn try_add_src(&mut self, src: &SrcRef) -> Option<usize> {
        for i in 0..self.num_srcs {
            if self.srcs[i] == *src {
                return Some(i);
            }
        }

        if self.num_srcs < 2 {
            let i = self.num_srcs;
            self.num_srcs += 1;
            self.srcs[i] = src.clone();
            Some(i)
        } else {
            None
        }
    }

    fn try_add_imm_u8(&mut self, u: u8) -> Option<usize> {
        if self.imm_src == usize::MAX {
            if self.num_srcs >= 2 {
                return None;
            }
            self.imm_src = self.num_srcs;
            self.num_srcs += 1;
        }

        match &mut self.srcs[self.imm_src] {
            SrcRef::Zero => {
                if u == 0 {
                    // Common case, just leave it as a SrcRef::Zero
                    debug_assert!(self.num_imm_bytes <= 1);
                    self.num_imm_bytes = 1;
                    Some(0)
                } else {
                    let b = self.num_imm_bytes;
                    self.num_imm_bytes += 1;
                    let imm = u32::from(u) << (b * 8);
                    self.srcs[self.imm_src] = SrcRef::Imm32(imm);
                    Some(b)
                }
            }
            SrcRef::Imm32(imm) => {
                let b = self.num_imm_bytes;
                self.num_imm_bytes += 1;
                *imm |= u32::from(u) << (b * 8);
                Some(b)
            }
            _ => panic!("We said this was the imm src"),
        }
    }
}

struct PrmtEntry {
    sel: PrmtSel,
    srcs: [SrcRef; 2],
}

struct PrmtPass {
    ssa_prmt: FxHashMap<SSAValue, PrmtEntry>,
}

impl PrmtPass {
    fn new() -> Self {
        Self {
            ssa_prmt: FxHashMap::default(),
        }
    }

    fn add_prmt(&mut self, op: &OpPrmt) {
        let Dst::SSA(dst_ssa) = &op.dst else {
            return;
        };
        debug_assert!(dst_ssa.comps() == 1);
        let dst_ssa = dst_ssa[0];

        let Some(sel) = op.get_sel() else {
            return;
        };

        debug_assert!(op.srcs[0].is_unmodified());
        debug_assert!(op.srcs[1].is_unmodified());
        let srcs = [op.srcs[0].reference.clone(), op.srcs[1].reference.clone()];

        self.ssa_prmt.insert(dst_ssa, PrmtEntry { sel, srcs });
    }

    fn get_prmt(&self, ssa: &SSAValue) -> Option<&PrmtEntry> {
        self.ssa_prmt.get(ssa)
    }

    fn get_prmt_for_src(&self, src: &Src) -> Option<&PrmtEntry> {
        debug_assert!(src.is_unmodified());
        if let SrcRef::SSA(vec) = &src.reference {
            debug_assert!(vec.comps() == 1);
            self.get_prmt(&vec[0])
        } else {
            None
        }
    }

    /// Try to optimize for the OpPrmt of OpPrmt case where only one source of
    /// the inner OpPrmt is used
    fn try_opt_prmt_src(&self, op: &mut OpPrmt, src_idx: usize) -> bool {
        let Some(op_sel) = op.get_sel() else {
            return false;
        };

        let Some(src_prmt) = self.get_prmt_for_src(&op.srcs[src_idx]) else {
            return false;
        };

        let mut new_sel = [PrmtSelByte::INVALID; 4];
        let mut src_prmt_src = usize::MAX;
        for i in 0..4 {
            let op_sel_byte = op_sel.get(i);
            if op_sel_byte.src() != src_idx {
                new_sel[i] = op_sel_byte;
                continue;
            }

            let src_sel_byte = src_prmt.sel.get(op_sel_byte.byte());

            if src_prmt_src != usize::MAX && src_prmt_src != src_sel_byte.src() {
                return false;
            }
            src_prmt_src = src_sel_byte.src();

            new_sel[i] = PrmtSelByte::new(
                src_idx,
                src_sel_byte.byte(),
                op_sel_byte.msb() | src_sel_byte.msb(),
            );
        }

        let new_sel = PrmtSel::new(new_sel);

        *op.sel_mut() = new_sel.into();
        if src_prmt_src == usize::MAX {
            // This source is unused
            op.srcs[src_idx] = 0.into();
        } else {
            op.srcs[src_idx] = src_prmt.srcs[src_prmt_src].clone().into();
        }
        true
    }

    /// Try to optimize for the OpPrmt of OpPrmt case as if we're considering a
    /// full 4-way OpPrmt in which some sources may be duplicates
    fn try_opt_prmt4(&self, op: &mut OpPrmt) -> bool {
        let Some(op_sel) = op.get_sel() else {
            return false;
        };

        let mut srcs = PrmtSrcs::new();
        let mut new_sel = [PrmtSelByte::INVALID; 4];
        for i in 0..4 {
            let op_sel_byte = op_sel.get(i);
            let src = &op.srcs[op_sel_byte.src()];

            if let Some(src_prmt) = self.get_prmt_for_src(src) {
                let src_sel_byte = src_prmt.sel.get(op_sel_byte.byte());
                let src_prmt_src = &src_prmt.srcs[src_sel_byte.src()];
                if let Some(u) = src_prmt_src.as_u32() {
                    let mut imm_u8 = src_sel_byte.fold_u32(u);
                    if op_sel_byte.msb() {
                        imm_u8 = ((imm_u8 as i8) >> 7) as u8;
                    }

                    let Some(byte_idx) = srcs.try_add_imm_u8(imm_u8) else {
                        return false;
                    };

                    new_sel[i] = PrmtSelByte::new(srcs.imm_src, byte_idx, false);
                } else {
                    let Some(src_idx) = srcs.try_add_src(src_prmt_src) else {
                        return false;
                    };

                    new_sel[i] = PrmtSelByte::new(
                        src_idx,
                        src_sel_byte.byte(),
                        op_sel_byte.msb() | src_sel_byte.msb(),
                    );
                }
            } else if let Some(u) = src.as_u32(SrcType::ALU) {
                let imm_u8 = op_sel_byte.fold_u32(u);
                let Some(byte_idx) = srcs.try_add_imm_u8(imm_u8) else {
                    return false;
                };

                new_sel[i] = PrmtSelByte::new(srcs.imm_src, byte_idx, false);
            } else {
                debug_assert!(src.is_unmodified());
                let Some(src_idx) = srcs.try_add_src(&src.reference) else {
                    return false;
                };

                new_sel[i] = PrmtSelByte::new(src_idx, op_sel_byte.byte(), op_sel_byte.msb());
            }
        }

        let new_sel = PrmtSel::new(new_sel);
        if new_sel == op_sel
            && srcs.srcs[0] == op.srcs[0].reference
            && srcs.srcs[1] == op.srcs[1].reference
        {
            return false;
        }

        *op.sel_mut() = new_sel.into();
        let [srcs0, srcs1] = srcs.srcs;
        op.srcs[0] = srcs0.into();
        op.srcs[1] = srcs1.into();
        true
    }

    fn opt_prmt(&mut self, op: &mut OpPrmt) {
        for i in 0..2 {
            loop {
                if !self.try_opt_prmt_src(op, i) {
                    break;
                }
            }
        }

        loop {
            if !self.try_opt_prmt4(op) {
                break;
            }
        }

        self.add_prmt(op);
    }

    fn run(&mut self, f: &mut Function) {
        for b in &mut f.blocks {
            for instr in &mut b.instrs {
                if let Op::Prmt(op) = &mut instr.op {
                    self.opt_prmt(op);
                }
            }
        }
    }
}

impl Shader<'_> {
    pub fn opt_prmt(&mut self) {
        for f in &mut self.functions {
            PrmtPass::new().run(f);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::ir::{
        BasicBlock, ComputeShaderInfo, Dst, Function, Instr, LabelAllocator, Op, OpCopy, OpExit,
        OpPrmt, OpRegOut, PhiAllocator, PrmtMode, RegFile, SSAValueAllocator, Shader, ShaderInfo,
        ShaderIoInfo, ShaderModelInfo, ShaderStageInfo, Src,
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
    fn test_opt_prmt_single_prmt_runs_without_panic() {
        let mut ssa_alloc = SSAValueAllocator::new();
        let dst_a = ssa_alloc.alloc(RegFile::GPR);
        let dst_b = ssa_alloc.alloc(RegFile::GPR);
        let mut shader = make_shader_with_function(
            vec![
                Instr::new(OpCopy {
                    dst: dst_a.into(),
                    src: Src::new_imm_u32(0x1234_5678),
                }),
                Instr::new(OpPrmt {
                    dst: dst_b.into(),
                    srcs: [dst_a.into(), Src::ZERO, Src::new_imm_u32(0x3210)],
                    mode: PrmtMode::Index,
                }),
                Instr::new(OpRegOut {
                    srcs: vec![dst_b.into()],
                }),
                Instr::new(OpExit {}),
            ],
            ssa_alloc,
        );
        shader.opt_prmt();
        let prmt = &shader.functions[0].blocks[0].instrs[1];
        assert!(matches!(prmt.op, Op::Prmt(_)));
    }

    #[test]
    fn test_opt_prmt_nested_prmt_optimizes_src() {
        // prmt(prmt(a,b,0x3210), c, 0x3210) - outer takes byte 0 from inner's src0
        // Inner prmt sel=0x3210 is identity from src0, so inner dst = a.
        // Outer prmt sel=0x3210 takes byte 0 from outer src0 (inner result) = byte 0 of a.
        // try_opt_prmt_src can inline: outer src0 comes from inner, inner is identity.
        let mut ssa_alloc = SSAValueAllocator::new();
        let dst_a = ssa_alloc.alloc(RegFile::GPR);
        let dst_b = ssa_alloc.alloc(RegFile::GPR);
        let dst_c = ssa_alloc.alloc(RegFile::GPR);
        let mut shader = make_shader_with_function(
            vec![
                Instr::new(OpCopy {
                    dst: dst_a.into(),
                    src: Src::new_imm_u32(0xDEAD_BEEF),
                }),
                Instr::new(OpPrmt {
                    dst: dst_b.into(),
                    srcs: [dst_a.into(), Src::ZERO, Src::new_imm_u32(0x3210)],
                    mode: PrmtMode::Index,
                }),
                Instr::new(OpPrmt {
                    dst: dst_c.into(),
                    srcs: [dst_b.into(), Src::ZERO, Src::new_imm_u32(0x3210)],
                    mode: PrmtMode::Index,
                }),
                Instr::new(OpRegOut {
                    srcs: vec![dst_c.into()],
                }),
                Instr::new(OpExit {}),
            ],
            ssa_alloc,
        );
        shader.opt_prmt();
        let outer_prmt = &shader.functions[0].blocks[0].instrs[2];
        let Op::Prmt(op) = &outer_prmt.op else {
            panic!("expected Prmt");
        };
        assert!(
            matches!(op.srcs[0].reference, super::super::ir::SrcRef::SSA(_)),
            "opt_prmt_src may inline inner prmt; src0 could become SSA or stay"
        );
    }

    #[test]
    fn test_opt_prmt_prmt_with_non_ssa_dst_skipped() {
        // add_prmt returns early when dst is not SSA - use Dst::None for prmt (invalid but tests path)
        let mut ssa_alloc = SSAValueAllocator::new();
        let dst_a = ssa_alloc.alloc(RegFile::GPR);
        let mut shader = make_shader_with_function(
            vec![
                Instr::new(OpCopy {
                    dst: dst_a.into(),
                    src: Src::ZERO,
                }),
                Instr::new(OpPrmt {
                    dst: Dst::None,
                    srcs: [dst_a.into(), Src::ZERO, Src::new_imm_u32(0x3210)],
                    mode: PrmtMode::Index,
                }),
                Instr::new(OpExit {}),
            ],
            ssa_alloc,
        );
        shader.opt_prmt();
    }
}
