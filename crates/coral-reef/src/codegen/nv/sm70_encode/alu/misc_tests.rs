// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2025-2026 ecoPrimals
//! Unit tests for SM70 miscellaneous ALU encoders (`alu/misc.rs`).

use super::super::encoder::{SM70Encoder, SM70Op};
use bitview::BitViewable;
use coral_reef_stubs::fxhash::FxHashMap;

use crate::codegen::ir::{
    Dst, FloatCmpOp, IntCmpOp, IntCmpType, LogicOp3, OpMov, OpPLop3, OpPrmt, OpR2UR, OpRedux,
    OpSel, OpSgxt, OpShfl, OpTranscendental, PredSetOp, PrmtMode, ReduxOp, RegFile, RegRef, ShflOp,
    Src, SrcMod, SrcSwizzle, TranscendentalOp,
};

fn gpr_src(idx: u32) -> Src {
    Src {
        reference: RegRef::new(RegFile::GPR, idx, 1).into(),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn ugpr_src(idx: u32) -> Src {
    Src {
        reference: RegRef::new(RegFile::UGPR, idx, 1).into(),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn pred_src(idx: u32) -> Src {
    Src {
        reference: RegRef::new(RegFile::Pred, idx, 1).into(),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn upred_src(idx: u32) -> Src {
    Src {
        reference: RegRef::new(RegFile::UPred, idx, 1).into(),
        modifier: SrcMod::None,
        swizzle: SrcSwizzle::None,
    }
}

fn encoder(sm: u8) -> SM70Encoder<'static> {
    let labels: &'static FxHashMap<crate::codegen::ir::Label, usize> =
        Box::leak(Box::new(FxHashMap::default()));
    SM70Encoder {
        sm,
        ip: 0,
        labels,
        inst: [0_u32; 4],
    }
}

fn opcode9(e: &SM70Encoder<'_>) -> u64 {
    e.get_field(0..9)
}

#[test]
fn helper_cmp_and_pred_setters_roundtrip() {
    let labels: &'static FxHashMap<crate::codegen::ir::Label, usize> =
        Box::leak(Box::new(FxHashMap::default()));
    let mut e = SM70Encoder {
        sm: 70,
        ip: 0,
        labels,
        inst: [0_u32; 4],
    };
    e.set_float_cmp_op(10..14, FloatCmpOp::OrdLt);
    assert_eq!(e.get_field(10..14), 1);

    e.set_pred_set_op(20..22, PredSetOp::Xor);
    assert_eq!(e.get_field(20..22), 2);

    e.set_int_cmp_op(30..33, IntCmpOp::Ge);
    assert_eq!(e.get_field(30..33), 6);
}

#[test]
fn op_transcendental_exp2_and_sqrt_subops() {
    let mut e = encoder(70);
    OpTranscendental {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        src: gpr_src(2),
        op: TranscendentalOp::Exp2,
    }
    .encode(&mut e);
    assert_eq!(opcode9(&e), 0x108);
    assert_eq!(e.get_field(74..80), 2);

    let mut e = encoder(70);
    OpTranscendental {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        src: gpr_src(2),
        op: TranscendentalOp::Sqrt,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(74..80), 8);
}

#[test]
fn op_mov_gpr_and_uniform_umov() {
    let mut e = encoder(70);
    OpMov {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 3, 1)),
        src: gpr_src(4),
        quad_lanes: 0xf,
    }
    .encode(&mut e);
    assert_eq!(opcode9(&e), 0x002);
    assert_eq!(e.get_field(72..76), 0xf);

    let mut e = encoder(73);
    OpMov {
        dst: Dst::Reg(RegRef::new(RegFile::UGPR, 2, 1)),
        src: Src::new_imm_u32(0xfeed_beef),
        quad_lanes: 0xf,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(9..12), 4, "imm form");
    assert_eq!(
        e.get_field(0..12),
        0x882,
        "umov opcode after form overwrite"
    );
    assert_eq!(e.get_field(32..64), 0xfeed_beef);
}

#[test]
fn op_prmt_warp_and_uniform_modes() {
    let mut e = encoder(70);
    OpPrmt {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), gpr_src(3), Src::new_imm_u32(0x3210)],
        mode: PrmtMode::Index,
    }
    .encode(&mut e);
    assert_eq!(opcode9(&e), 0x016);
    assert_eq!(e.get_field(72..75), 0);

    let mut e = encoder(73);
    OpPrmt {
        dst: Dst::Reg(RegRef::new(RegFile::UGPR, 5, 1)),
        srcs: [ugpr_src(1), ugpr_src(2), Src::new_imm_u32(0)],
        mode: PrmtMode::Replicate16,
    }
    .encode(&mut e);
    assert_eq!(opcode9(&e), 0x096);
    assert_eq!(e.get_field(72..75), 6);
}

#[test]
fn op_sel_pred_and_upred_paths() {
    let mut e = encoder(70);
    OpSel {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 8, 1)),
        srcs: [pred_src(2), gpr_src(3), gpr_src(4)],
    }
    .encode(&mut e);
    assert_eq!(opcode9(&e), 0x007);
    assert_eq!(e.get_field(87..90), 2);

    let mut e = encoder(73);
    OpSel {
        dst: Dst::Reg(RegRef::new(RegFile::UGPR, 1, 1)),
        srcs: [upred_src(3), ugpr_src(2), ugpr_src(4)],
    }
    .encode(&mut e);
    assert_eq!(opcode9(&e), 0x087);
}

#[test]
fn op_sgxt_signed_and_uniform() {
    let mut e = encoder(70);
    OpSgxt {
        dst: Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
        srcs: [gpr_src(2), Src::new_imm_u32(8)],
        signed: true,
    }
    .encode(&mut e);
    assert_eq!(opcode9(&e), 0x01a);
    assert!(e.get_bit(73));

    let mut e = encoder(73);
    OpSgxt {
        dst: Dst::Reg(RegRef::new(RegFile::UGPR, 2, 1)),
        srcs: [ugpr_src(1), Src::new_imm_u32(16)],
        signed: false,
    }
    .encode(&mut e);
    assert_eq!(opcode9(&e), 0x09a);
}

#[test]
fn op_shfl_register_and_immediate_forms() {
    let base = |lane: Src, c: Src, expect_opcode: u16| {
        let mut e = encoder(70);
        OpShfl {
            dsts: [
                Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)),
                Dst::Reg(RegRef::new(RegFile::Pred, 2, 1)),
            ],
            srcs: [gpr_src(9), lane, c],
            op: ShflOp::Idx,
        }
        .encode(&mut e);
        assert_eq!(e.get_field(0..12), u64::from(expect_opcode));
    };

    base(gpr_src(3), gpr_src(4), 0x389);
    base(gpr_src(3), Src::new_imm_u32(5), 0x589);
    base(Src::new_imm_u32(7), gpr_src(4), 0x989);
    base(Src::new_imm_u32(7), Src::new_imm_u32(3), 0xf89);
}

#[test]
fn op_shfl_shuffle_mode_field() {
    let mut e = encoder(70);
    OpShfl {
        dsts: [Dst::Reg(RegRef::new(RegFile::GPR, 1, 1)), Dst::None],
        srcs: [gpr_src(1), gpr_src(2), gpr_src(3)],
        op: ShflOp::Bfly,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(58..60), 3);
}

#[test]
fn op_plop3_uniform_and_warp_with_uniform_src2() {
    let lut = LogicOp3::new_lut(&|a, b, c| a ^ b ^ c);
    let mut e = encoder(73);
    OpPLop3 {
        dsts: [
            Dst::Reg(RegRef::new(RegFile::UPred, 1, 1)),
            Dst::Reg(RegRef::new(RegFile::UPred, 2, 1)),
        ],
        srcs: [upred_src(1), upred_src(2), upred_src(3)],
        ops: [lut, lut],
    }
    .encode(&mut e);
    assert_eq!(e.get_field(0..12), 0x89c);

    let mut e = encoder(73);
    OpPLop3 {
        dsts: [
            Dst::Reg(RegRef::new(RegFile::Pred, 0, 1)),
            Dst::Reg(RegRef::new(RegFile::Pred, 1, 1)),
        ],
        srcs: [
            pred_src(2),
            pred_src(3),
            Src {
                reference: RegRef::new(RegFile::UPred, 4, 1).into(),
                modifier: SrcMod::None,
                swizzle: SrcSwizzle::None,
            },
        ],
        ops: [lut, lut],
    }
    .encode(&mut e);
    assert_eq!(e.get_field(0..12), 0x81c);
    assert!(e.get_bit(67), "uniform predicate on src2");
}

#[test]
fn op_r2ur_opcode_sm70_vs_sm100() {
    let mut e = encoder(89);
    OpR2UR {
        dst: Dst::Reg(RegRef::new(RegFile::UGPR, 3, 1)),
        src: gpr_src(5),
    }
    .encode(&mut e);
    assert_eq!(e.get_field(0..12), 0x3c2);

    let mut e = encoder(100);
    OpR2UR {
        dst: Dst::Reg(RegRef::new(RegFile::UGPR, 3, 1)),
        src: gpr_src(5),
    }
    .encode(&mut e);
    assert_eq!(e.get_field(0..12), 0x2ca);
}

#[test]
fn op_redux_ops_and_min_i32_bit() {
    let mut e = encoder(73);
    OpRedux {
        dst: Dst::Reg(RegRef::new(RegFile::UGPR, 1, 1)),
        src: gpr_src(2),
        op: ReduxOp::Xor,
    }
    .encode(&mut e);
    assert_eq!(e.get_field(0..12), 0x3c4);
    assert_eq!(e.get_field(78..81), 2);
    assert!(!e.get_bit(73));

    let mut e = encoder(73);
    OpRedux {
        dst: Dst::Reg(RegRef::new(RegFile::UGPR, 1, 1)),
        src: gpr_src(2),
        op: ReduxOp::Min(IntCmpType::I32),
    }
    .encode(&mut e);
    assert_eq!(e.get_field(78..81), 4);
    assert!(e.get_bit(73));
}
