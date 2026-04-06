// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals

use crate::codegen::ir::{
    BasicBlock, CBuf, CBufRef, ComputeShaderInfo, Dst, Function, Instr, LabelAllocator, LogicOp2,
    Op, OpCopy, OpR2UR, OpSwap, PhiAllocator, RegFile, RegRef, Shader, ShaderInfo, ShaderIoInfo,
    ShaderModelInfo, ShaderStageInfo, Src, SrcMod, SrcSwizzle,
};

use coral_reef_stubs::cfg::CFGBuilder;

fn make_shader(sm: u8, instrs: Vec<Instr>) -> Shader<'static> {
    let sm_ref: &'static dyn crate::codegen::ir::ShaderModel =
        Box::leak(Box::new(ShaderModelInfo::new(sm, 64)));
    let mut label_alloc = LabelAllocator::new();
    let mut cfg_builder = CFGBuilder::new();
    let block = BasicBlock {
        label: label_alloc.alloc(),
        uniform: false,
        instrs,
    };
    cfg_builder.add_block(block);
    let function = Function {
        ssa_alloc: crate::codegen::ir::SSAValueAllocator::new(),
        phi_alloc: PhiAllocator::new(),
        blocks: cfg_builder.build(),
    };
    Shader {
        sm: sm_ref,
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
fn copy_gpr_to_gpr_emits_mov() {
    let r1 = RegRef::new(RegFile::GPR, 3, 1);
    let r2 = RegRef::new(RegFile::GPR, 7, 1);
    let mut shader = make_shader(
        75,
        vec![Instr::new(OpCopy {
            dst: r2.into(),
            src: r1.into(),
        })],
    );
    shader.lower_copy_swap();
    let instrs = &shader.functions[0].blocks[0].instrs;
    assert_eq!(instrs.len(), 1);
    let Op::Mov(m) = &instrs[0].op else {
        panic!("expected OpMov");
    };
    let Dst::Reg(dst) = m.dst else {
        panic!("expected Reg dst");
    };
    assert!(dst == r2);
    assert!(m.src.reference == r1.into());
}

#[test]
fn copy_true_to_pred_emits_plop3_on_sm70() {
    let p = RegRef::new(RegFile::Pred, 2, 1);
    let mut shader = make_shader(
        70,
        vec![Instr::new(OpCopy {
            dst: p.into(),
            src: Src::new_imm_bool(true),
        })],
    );
    shader.lower_copy_swap();
    assert!(matches!(
        shader.functions[0].blocks[0].instrs[0].op,
        Op::PLop3(_)
    ));
}

#[test]
fn copy_true_to_pred_emits_psetp_sm60() {
    let p = RegRef::new(RegFile::Pred, 2, 1);
    let mut shader = make_shader(
        60,
        vec![Instr::new(OpCopy {
            dst: p.into(),
            src: Src::new_imm_bool(true),
        })],
    );
    shader.lower_copy_swap();
    assert!(matches!(
        shader.functions[0].blocks[0].instrs[0].op,
        Op::PSetP(_)
    ));
}

#[test]
fn copy_pred_to_gpr_emits_sel() {
    let p = RegRef::new(RegFile::Pred, 1, 1);
    let g = RegRef::new(RegFile::GPR, 4, 1);
    let mut shader = make_shader(
        70,
        vec![Instr::new(OpCopy {
            dst: g.into(),
            src: p.into(),
        })],
    );
    shader.lower_copy_swap();
    assert!(matches!(
        shader.functions[0].blocks[0].instrs[0].op,
        Op::Sel(_)
    ));
}

#[test]
fn copy_ugpr_from_cbuf_emits_ldc() {
    let u = RegRef::new(RegFile::UGPR, 2, 1);
    let cb = Src {
        reference: crate::codegen::ir::SrcRef::CBuf(CBufRef {
            buf: CBuf::Binding(0),
            offset: 0x10,
        }),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    };
    let mut shader = make_shader(
        75,
        vec![Instr::new(OpCopy {
            dst: u.into(),
            src: cb,
        })],
    );
    shader.lower_copy_swap();
    assert!(matches!(
        shader.functions[0].blocks[0].instrs[0].op,
        Op::Ldc(_)
    ));
}

#[test]
fn copy_gpr_from_cbuf_sm75_emits_mov_not_ldc() {
    let g = RegRef::new(RegFile::GPR, 5, 1);
    let cb = Src {
        reference: crate::codegen::ir::SrcRef::CBuf(CBufRef {
            buf: CBuf::Binding(1),
            offset: 0,
        }),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    };
    let mut shader = make_shader(
        75,
        vec![Instr::new(OpCopy {
            dst: g.into(),
            src: cb,
        })],
    );
    shader.lower_copy_swap();
    assert!(matches!(
        shader.functions[0].blocks[0].instrs[0].op,
        Op::Mov(_)
    ));
}

#[test]
fn copy_gpr_from_mem_slot_emits_ld_with_offset() {
    let m = RegRef::new(RegFile::Mem, 4, 1);
    let g = RegRef::new(RegFile::GPR, 1, 1);
    let mut shader = make_shader(
        70,
        vec![Instr::new(OpCopy {
            dst: g.into(),
            src: m.into(),
        })],
    );
    shader.lower_copy_swap();
    let instrs = &shader.functions[0].blocks[0].instrs;
    let Op::Ld(ld) = &instrs[0].op else {
        panic!("expected OpLd");
    };
    let Dst::Reg(dst) = ld.dst else {
        panic!("expected Reg dst");
    };
    assert!(dst == g);
    assert_eq!(ld.offset, 16);
    assert_eq!(shader.info.shared_local_mem_size, 20);
}

#[test]
fn copy_mem_from_gpr_emits_st() {
    let m = RegRef::new(RegFile::Mem, 2, 1);
    let g = RegRef::new(RegFile::GPR, 9, 1);
    let mut shader = make_shader(
        70,
        vec![Instr::new(OpCopy {
            dst: m.into(),
            src: g.into(),
        })],
    );
    shader.lower_copy_swap();
    let instrs = &shader.functions[0].blocks[0].instrs;
    let Op::St(st) = &instrs[0].op else {
        panic!("expected OpSt");
    };
    assert_eq!(st.offset, 8);
    assert_eq!(shader.info.shared_local_mem_size, 12);
}

#[test]
fn copy_bar_from_gpr_emits_bmov() {
    let b = RegRef::new(RegFile::Bar, 0, 1);
    let g = RegRef::new(RegFile::GPR, 3, 1);
    let mut shader = make_shader(
        70,
        vec![Instr::new(OpCopy {
            dst: b.into(),
            src: g.into(),
        })],
    );
    shader.lower_copy_swap();
    assert!(matches!(
        shader.functions[0].blocks[0].instrs[0].op,
        Op::BMov(_)
    ));
}

#[test]
fn copy_upred_to_pred_emits_plop3() {
    let u = RegRef::new(RegFile::UPred, 2, 1);
    let p = RegRef::new(RegFile::Pred, 4, 1);
    let mut shader = make_shader(
        80,
        vec![Instr::new(OpCopy {
            dst: p.into(),
            src: u.into(),
        })],
    );
    shader.lower_copy_swap();
    assert!(matches!(
        shader.functions[0].blocks[0].instrs[0].op,
        Op::PLop3(_)
    ));
}

#[test]
fn r2ur_gpr_to_ugpr_passthrough() {
    let g = RegRef::new(RegFile::GPR, 6, 1);
    let u = RegRef::new(RegFile::UGPR, 1, 1);
    let mut shader = make_shader(
        75,
        vec![Instr::new(OpR2UR {
            dst: u.into(),
            src: g.into(),
        })],
    );
    shader.lower_copy_swap();
    assert!(matches!(
        shader.functions[0].blocks[0].instrs[0].op,
        Op::R2UR(_)
    ));
}

#[test]
fn r2ur_pred_to_upred_vote() {
    let p = RegRef::new(RegFile::Pred, 2, 1);
    let u = RegRef::new(RegFile::UPred, 3, 1);
    let mut shader = make_shader(
        75,
        vec![Instr::new(OpR2UR {
            dst: u.into(),
            src: p.into(),
        })],
    );
    shader.lower_copy_swap();
    assert!(matches!(
        shader.functions[0].blocks[0].instrs[0].op,
        Op::Vote(_)
    ));
}

#[test]
fn swap_two_gprs_xor_sequence_sm60() {
    let x = RegRef::new(RegFile::GPR, 1, 1);
    let y = RegRef::new(RegFile::GPR, 2, 1);
    let mut shader = make_shader(
        60,
        vec![Instr::new(OpSwap {
            dsts: [x.into(), y.into()],
            srcs: [y.into(), x.into()],
        })],
    );
    shader.lower_copy_swap();
    let instrs = &shader.functions[0].blocks[0].instrs;
    assert_eq!(instrs.len(), 3);
    for i in instrs {
        let Op::Lop2(l) = &i.op else {
            panic!("expected XOR lop2 chain");
        };
        assert!(l.op == LogicOp2::Xor);
    }
}

#[test]
fn swap_pred_sm70_single_plop3() {
    let a = RegRef::new(RegFile::Pred, 0, 1);
    let b = RegRef::new(RegFile::Pred, 1, 1);
    let mut shader = make_shader(
        70,
        vec![Instr::new(OpSwap {
            dsts: [a.into(), b.into()],
            srcs: [b.into(), a.into()],
        })],
    );
    shader.lower_copy_swap();
    let instrs = &shader.functions[0].blocks[0].instrs;
    assert_eq!(instrs.len(), 1);
    assert!(matches!(instrs[0].op, Op::PLop3(_)));
}

#[test]
fn swap_same_reg_is_noop() {
    let p = RegRef::new(RegFile::GPR, 5, 1);
    let mut shader = make_shader(
        75,
        vec![Instr::new(OpSwap {
            dsts: [p.into(), p.into()],
            srcs: [p.into(), p.into()],
        })],
    );
    shader.lower_copy_swap();
    assert!(shader.functions[0].blocks[0].instrs.is_empty());
}
