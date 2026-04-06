// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! SM70 instruction encoding and legalization.

mod encoder;
use self::encoder::*;

mod alu;
mod control;
mod mem;
mod tex;

use super::sm70::ShaderModel70;
use crate::codegen::legalize::LegalizeBuilder;
use coral_reef_stubs::fxhash::FxHashMap;

pub fn legalize_sm70_op(_sm: &ShaderModel70, b: &mut LegalizeBuilder, op: &mut Op) {
    op.legalize(b);
}

pub fn encode_sm70_shader(sm: &ShaderModel70, s: &Shader<'_>) -> Vec<u32> {
    assert!(s.functions.len() == 1);
    let func = &s.functions[0];

    let mut ip = 0_usize;
    let mut labels = FxHashMap::default();
    for b in &func.blocks {
        labels.insert(b.label, ip);
        for instr in &b.instrs {
            if let Op::Nop(op) = &instr.op {
                if let Some(label) = op.label {
                    labels.insert(label, ip);
                }
            }
            ip += 4;
        }
    }

    let mut encoded = Vec::new();
    for b in &func.blocks {
        for instr in &b.instrs {
            let mut e = SM70Encoder {
                sm: sm.sm(),
                ip: encoded.len(),
                labels: &labels,
                inst: [0_u32; 4],
            };
            instr.op.encode(&mut e);
            e.set_pred(&instr.pred);
            e.set_instr_deps(&instr.deps);
            encoded.extend_from_slice(&e.inst[..]);
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::ir::{Dst, OffsetStride, RegFile, RegRef, Src, SrcMod, SrcSwizzle};
    use crate::codegen::ssa_value::SSAValueAllocator;

    #[test]
    fn test_src_mod_has_abs() {
        assert!(!src_mod_has_abs(SrcMod::None));
        assert!(!src_mod_has_abs(SrcMod::FNeg));
        assert!(!src_mod_has_abs(SrcMod::INeg));
        assert!(!src_mod_has_abs(SrcMod::BNot));
        assert!(src_mod_has_abs(SrcMod::FAbs));
        assert!(src_mod_has_abs(SrcMod::FNegAbs));
    }

    #[test]
    fn test_src_mod_has_neg() {
        assert!(!src_mod_has_neg(SrcMod::None));
        assert!(!src_mod_has_neg(SrcMod::FAbs));
        assert!(src_mod_has_neg(SrcMod::FNeg));
        assert!(src_mod_has_neg(SrcMod::FNegAbs));
        assert!(src_mod_has_neg(SrcMod::INeg));
        assert!(src_mod_has_neg(SrcMod::BNot));
    }

    #[test]
    fn test_src_mod_is_bnot() {
        assert!(!src_mod_is_bnot(SrcMod::None));
        assert!(src_mod_is_bnot(SrcMod::BNot));
    }

    #[test]
    fn test_src_is_zero_or_gpr() {
        assert!(src_is_zero_or_gpr(&Src::ZERO));
        let reg = RegRef::new(RegFile::GPR, 5, 1);
        assert!(src_is_zero_or_gpr(&Src {
            reference: reg.into(),
            modifier: SrcMod::None,
            swizzle: SrcSwizzle::None,
        }));
        let ugpr = RegRef::new(RegFile::UGPR, 0, 1);
        assert!(!src_is_zero_or_gpr(&Src {
            reference: ugpr.into(),
            modifier: SrcMod::None,
            swizzle: SrcSwizzle::None,
        }));
        assert!(!src_is_zero_or_gpr(&Src::new_imm_u32(42)));
    }

    #[test]
    fn test_dst_is_bar() {
        assert!(!dst_is_bar(&Dst::None));
        let bar_reg = RegRef::new(RegFile::Bar, 0, 1);
        assert!(dst_is_bar(&Dst::Reg(bar_reg)));
        let gpr_reg = RegRef::new(RegFile::GPR, 0, 1);
        assert!(!dst_is_bar(&Dst::Reg(gpr_reg)));
        let mut alloc = SSAValueAllocator::new();
        let bar_ssa = alloc.alloc(RegFile::Bar);
        assert!(dst_is_bar(&Dst::SSA(bar_ssa.into())));
    }

    #[test]
    fn test_offset_stride_encode_sm75() {
        assert_eq!(OffsetStride::X1.encode_sm75(), 0);
        assert_eq!(OffsetStride::X4.encode_sm75(), 1);
        assert_eq!(OffsetStride::X8.encode_sm75(), 2);
        assert_eq!(OffsetStride::X16.encode_sm75(), 3);
    }

    #[test]
    fn test_encode_sm70_shader_minimal() {
        use crate::codegen::ir::{
            BasicBlock, ComputeShaderInfo, Function, Instr, LabelAllocator, OpExit, OpNop,
            PhiAllocator, Shader, ShaderInfo, ShaderIoInfo, ShaderStageInfo,
        };
        use coral_reef_stubs::cfg::CFGBuilder;

        let sm = ShaderModel70::new(70);
        let sm_info = Box::leak(Box::new(crate::codegen::ir::ShaderModelInfo::new(70, 64)));
        let mut label_alloc = LabelAllocator::new();
        let mut cfg_builder = CFGBuilder::new();
        let block = BasicBlock {
            label: label_alloc.alloc(),
            uniform: false,
            instrs: vec![Instr::new(OpNop { label: None }), Instr::new(OpExit {})],
        };
        cfg_builder.add_block(block);
        let function = Function {
            ssa_alloc: SSAValueAllocator::new(),
            phi_alloc: PhiAllocator::new(),
            blocks: cfg_builder.build(),
        };
        let shader = Shader {
            sm: sm_info,
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
        };
        let encoded = encode_sm70_shader(&sm, &shader);
        assert!(!encoded.is_empty(), "minimal shader should produce binary");
    }

    #[test]
    fn test_encode_sm70_shader_with_bra() {
        use crate::codegen::ir::{
            BasicBlock, ComputeShaderInfo, Function, Instr, LabelAllocator, OpBra, OpExit, OpNop,
            PhiAllocator, Shader, ShaderInfo, ShaderIoInfo, ShaderStageInfo, Src,
        };
        use coral_reef_stubs::cfg::CFGBuilder;

        let sm = ShaderModel70::new(70);
        let sm_info = Box::leak(Box::new(crate::codegen::ir::ShaderModelInfo::new(70, 64)));
        let mut label_alloc = LabelAllocator::new();
        let mut cfg_builder = CFGBuilder::new();

        let exit_label = label_alloc.alloc();
        let bra_label = label_alloc.alloc();

        let bra_block = BasicBlock {
            label: bra_label,
            uniform: false,
            instrs: vec![
                Instr::new(OpNop { label: None }),
                Instr::new(OpBra {
                    target: exit_label,
                    cond: Src::new_imm_bool(true),
                }),
            ],
        };
        cfg_builder.add_block(bra_block);

        let exit_block = BasicBlock {
            label: exit_label,
            uniform: false,
            instrs: vec![Instr::new(OpExit {})],
        };
        cfg_builder.add_block(exit_block);

        let function = Function {
            ssa_alloc: SSAValueAllocator::new(),
            phi_alloc: PhiAllocator::new(),
            blocks: cfg_builder.build(),
        };
        let shader = Shader {
            sm: sm_info,
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
        };
        let encoded = encode_sm70_shader(&sm, &shader);
        assert!(
            encoded.len() >= 8,
            "shader with Bra and Exit should produce at least 2 instructions (8 bytes)"
        );
    }
}
