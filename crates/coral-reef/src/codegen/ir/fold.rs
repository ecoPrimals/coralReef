// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
//! Constant folding data types and the `Foldable` trait.

use super::{Dst, DstsAsSlice, ShaderModel, Src, SrcRef, SrcsAsSlice};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FoldData {
    Pred(bool),
    Carry(bool),
    U32(u32),
    Vec2([u32; 2]),
}

pub struct OpFoldData<'a> {
    pub dsts: &'a mut [FoldData],
    pub srcs: &'a [FoldData],
}

impl OpFoldData<'_> {
    pub fn get_pred_src(&self, op: &impl SrcsAsSlice, src: &Src) -> bool {
        let i = op.src_idx(src);
        let b = match src.reference {
            SrcRef::Zero | SrcRef::Imm32(_) => super::super::ice!("Expected a predicate"),
            SrcRef::True => true,
            SrcRef::False => false,
            _ => {
                if let FoldData::Pred(b) = self.srcs[i] {
                    b
                } else {
                    super::super::ice!("FoldData is not a predicate");
                }
            }
        };
        b ^ src.modifier.is_bnot()
    }

    pub fn get_u32_src(&self, op: &impl SrcsAsSlice, src: &Src) -> u32 {
        let i = op.src_idx(src);
        match src.reference {
            SrcRef::Zero => 0,
            SrcRef::Imm32(imm) => imm,
            SrcRef::True | SrcRef::False => super::super::ice!("Unexpected predicate"),
            _ => {
                if let FoldData::U32(u) = self.srcs[i] {
                    u
                } else {
                    super::super::ice!("FoldData is not a U32");
                }
            }
        }
    }

    pub fn get_u32_bnot_src(&self, op: &impl SrcsAsSlice, src: &Src) -> u32 {
        let x = self.get_u32_src(op, src);
        if src.modifier.is_bnot() { !x } else { x }
    }

    pub fn get_carry_src(&self, op: &impl SrcsAsSlice, src: &Src) -> bool {
        assert!(src.reference.as_ssa().is_some());
        let i = op.src_idx(src);
        if let FoldData::Carry(b) = self.srcs[i] {
            b
        } else {
            super::super::ice!("FoldData is not a predicate");
        }
    }

    pub fn get_f32_src(&self, op: &impl SrcsAsSlice, src: &Src) -> f32 {
        f32::from_bits(self.get_u32_src(op, src))
    }

    pub fn get_f64_src(&self, op: &impl SrcsAsSlice, src: &Src) -> f64 {
        let i = op.src_idx(src);
        match src.reference {
            SrcRef::Zero => 0.0,
            SrcRef::Imm32(imm) => f64::from_bits(u64::from(imm) << 32),
            SrcRef::True | SrcRef::False => super::super::ice!("Unexpected predicate"),
            _ => {
                if let FoldData::Vec2(v) = self.srcs[i] {
                    let u = u64::from(v[0]) | (u64::from(v[1]) << 32);
                    f64::from_bits(u)
                } else {
                    super::super::ice!("FoldData is not a U32");
                }
            }
        }
    }

    pub fn set_pred_dst(&mut self, op: &impl DstsAsSlice, dst: &Dst, b: bool) {
        self.dsts[op.dst_idx(dst)] = FoldData::Pred(b);
    }

    pub fn set_carry_dst(&mut self, op: &impl DstsAsSlice, dst: &Dst, b: bool) {
        self.dsts[op.dst_idx(dst)] = FoldData::Carry(b);
    }

    pub fn set_u32_dst(&mut self, op: &impl DstsAsSlice, dst: &Dst, u: u32) {
        self.dsts[op.dst_idx(dst)] = FoldData::U32(u);
    }

    pub fn set_f32_dst(&mut self, op: &impl DstsAsSlice, dst: &Dst, f: f32) {
        self.set_u32_dst(op, dst, f.to_bits());
    }

    pub fn set_f64_dst(&mut self, op: &impl DstsAsSlice, dst: &Dst, f: f64) {
        let u = f.to_bits();
        let v = [u as u32, (u >> 32) as u32];
        self.dsts[op.dst_idx(dst)] = FoldData::Vec2(v);
    }
}

pub trait Foldable: SrcsAsSlice + DstsAsSlice {
    fn fold(&self, sm: &dyn ShaderModel, f: &mut OpFoldData<'_>);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::ir::{
        IntCmpOp, IntCmpType, IntType, LogicOp2, LogicOp3, OpFlo, OpIAbs, OpIAdd2, OpIAdd3,
        OpIMnMx, OpISetP, OpLea, OpLop2, OpLop3, OpPopC, OpPrmt, OpShf, OpShl, OpShr, PredSetOp,
        PrmtMode, ShaderModelInfo, SrcMod,
    };

    fn imm(u: u32) -> Src {
        Src::new_imm_u32(u)
    }

    fn run_fold<O: Foldable>(op: &O, srcs: &[FoldData], dst_count: usize) -> Vec<FoldData> {
        let sm = ShaderModelInfo::new(70, 64);
        let mut dsts = vec![FoldData::U32(0); dst_count];
        let mut f = OpFoldData {
            dsts: &mut dsts,
            srcs,
        };
        op.fold(&sm, &mut f);
        dsts
    }

    // -------------------------------------------------------------------------
    // Integer constant folding
    // -------------------------------------------------------------------------

    #[test]
    fn fold_iadd2_constants() {
        let op = OpIAdd2 {
            dsts: [Dst::None, Dst::None],
            srcs: [imm(10), imm(20)],
        };
        let dsts = run_fold(&op, &[], 2);
        assert_eq!(dsts[0], FoldData::U32(30));
        assert_eq!(dsts[1], FoldData::Carry(false));
    }

    #[test]
    fn fold_iadd2_identity_zero_left() {
        let op = OpIAdd2 {
            dsts: [Dst::None, Dst::None],
            srcs: [Src::ZERO, imm(42)],
        };
        let dsts = run_fold(&op, &[], 2);
        assert_eq!(dsts[0], FoldData::U32(42));
    }

    #[test]
    fn fold_iadd2_identity_zero_right() {
        let op = OpIAdd2 {
            dsts: [Dst::None, Dst::None],
            srcs: [imm(42), Src::ZERO],
        };
        let dsts = run_fold(&op, &[], 2);
        assert_eq!(dsts[0], FoldData::U32(42));
    }

    #[test]
    fn fold_iadd2_overflow_carry() {
        let op = OpIAdd2 {
            dsts: [Dst::None, Dst::None],
            srcs: [imm(0xFFFF_FFFF), imm(1)],
        };
        let dsts = run_fold(&op, &[], 2);
        assert_eq!(dsts[0], FoldData::U32(0));
        assert_eq!(dsts[1], FoldData::Carry(true));
    }

    #[test]
    fn fold_iadd3_constants() {
        let op = OpIAdd3 {
            dsts: [Dst::None, Dst::None, Dst::None],
            srcs: [imm(10), imm(20), imm(5)],
        };
        let dsts = run_fold(&op, &[], 3);
        assert_eq!(dsts[0], FoldData::U32(35));
        assert_eq!(dsts[1], FoldData::Pred(false));
        assert_eq!(dsts[2], FoldData::Pred(false));
    }

    #[test]
    fn fold_iadd3_identity_two_zeros() {
        let op = OpIAdd3 {
            dsts: [Dst::None, Dst::None, Dst::None],
            srcs: [Src::ZERO, Src::ZERO, imm(100)],
        };
        let dsts = run_fold(&op, &[], 3);
        assert_eq!(dsts[0], FoldData::U32(100));
    }

    #[test]
    fn fold_iadd3_overflow() {
        let op = OpIAdd3 {
            dsts: [Dst::None, Dst::None, Dst::None],
            srcs: [imm(0xFFFF_FFFF), imm(0xFFFF_FFFF), imm(2)],
        };
        let dsts = run_fold(&op, &[], 3);
        assert_eq!(dsts[0], FoldData::U32(0));
        assert_eq!(dsts[1], FoldData::Pred(true));
    }

    #[test]
    fn fold_iabs_positive() {
        let op = OpIAbs {
            dst: Dst::None,
            src: imm(42),
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::U32(42));
    }

    #[test]
    fn fold_iabs_negative() {
        let op = OpIAbs {
            dst: Dst::None,
            src: imm(0xFFFF_FFD6), // -42 as i32
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::U32(42));
    }

    #[test]
    fn fold_iabs_min_int() {
        let op = OpIAbs {
            dst: Dst::None,
            src: imm(0x8000_0000),
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::U32(0x8000_0000));
    }

    // -------------------------------------------------------------------------
    // Bitwise / logic folding
    // -------------------------------------------------------------------------

    #[test]
    fn fold_lop2_and() {
        let op = OpLop2 {
            dst: Dst::None,
            srcs: [imm(0xFF00), imm(0x0F0F)],
            op: LogicOp2::And,
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::U32(0x0F00));
    }

    #[test]
    fn fold_lop2_or() {
        let op = OpLop2 {
            dst: Dst::None,
            srcs: [imm(0xFF00), imm(0x00FF)],
            op: LogicOp2::Or,
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::U32(0xFFFF));
    }

    #[test]
    fn fold_lop2_xor() {
        let op = OpLop2 {
            dst: Dst::None,
            srcs: [imm(0xAAAA), imm(0x5555)],
            op: LogicOp2::Xor,
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::U32(0xFFFF));
    }

    #[test]
    fn fold_lop2_pass_b_identity() {
        let op = OpLop2 {
            dst: Dst::None,
            srcs: [imm(0xDEAD), imm(0xBEEF)],
            op: LogicOp2::PassB,
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::U32(0xBEEF));
    }

    #[test]
    fn fold_lop2_and_zero_identity() {
        let op = OpLop2 {
            dst: Dst::None,
            srcs: [Src::ZERO, imm(0x1234)],
            op: LogicOp2::And,
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::U32(0));
    }

    #[test]
    fn fold_lop3_and_all() {
        let op = OpLop3 {
            dst: Dst::None,
            srcs: [imm(0xFF), imm(0xF0), imm(0x0F)],
            op: LogicOp3::new_lut(&|x, y, z| x & y & z),
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::U32(0));
    }

    #[test]
    fn fold_lop3_const_true() {
        let op = OpLop3 {
            dst: Dst::None,
            srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
            op: LogicOp3::new_const(true),
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::U32(0xFFFF_FFFF));
    }

    #[test]
    fn fold_lop3_const_false() {
        let op = OpLop3 {
            dst: Dst::None,
            srcs: [Src::ZERO, Src::ZERO, Src::ZERO],
            op: LogicOp3::new_const(false),
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::U32(0));
    }

    #[test]
    fn fold_popc() {
        let op = OpPopC {
            dst: Dst::None,
            src: imm(0b1011),
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::U32(3));
    }

    #[test]
    fn fold_popc_bnot() {
        let op = OpPopC {
            dst: Dst::None,
            src: imm(0b1011).modify(SrcMod::BNot),
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::U32(29));
    }

    // -------------------------------------------------------------------------
    // Shift folding
    // -------------------------------------------------------------------------

    #[test]
    fn fold_shl_basic() {
        let op = OpShl {
            dst: Dst::None,
            srcs: [imm(1), imm(4)],
            wrap: false,
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::U32(16));
    }

    #[test]
    fn fold_shl_overflow_clamped() {
        let op = OpShl {
            dst: Dst::None,
            srcs: [imm(1), imm(40)],
            wrap: false,
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::U32(0));
    }

    #[test]
    fn fold_shl_wrap() {
        let op = OpShl {
            dst: Dst::None,
            srcs: [imm(1), imm(32)],
            wrap: true,
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::U32(1));
    }

    #[test]
    fn fold_shr_u32_basic() {
        let op = OpShr {
            dst: Dst::None,
            srcs: [imm(0x1000), imm(4)],
            wrap: false,
            signed: false,
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::U32(0x100));
    }

    #[test]
    fn fold_shr_signed_negative() {
        let op = OpShr {
            dst: Dst::None,
            srcs: [imm(0x8000_0000), imm(31)],
            wrap: false,
            signed: true,
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::U32(0xFFFF_FFFF));
    }

    // -------------------------------------------------------------------------
    // Comparison / predicate folding
    // -------------------------------------------------------------------------

    #[test]
    fn fold_isetp_eq_true() {
        let op = OpISetP {
            dst: Dst::None,
            set_op: PredSetOp::Or,
            cmp_op: IntCmpOp::Eq,
            cmp_type: IntCmpType::U32,
            ex: false,
            srcs: [
                imm(42),
                imm(42),
                Src::new_imm_bool(false),
                Src::new_imm_bool(false),
            ],
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::Pred(true));
    }

    #[test]
    fn fold_isetp_eq_false() {
        let op = OpISetP {
            dst: Dst::None,
            set_op: PredSetOp::Or,
            cmp_op: IntCmpOp::Eq,
            cmp_type: IntCmpType::U32,
            ex: false,
            srcs: [
                imm(42),
                imm(43),
                Src::new_imm_bool(false),
                Src::new_imm_bool(false),
            ],
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::Pred(false));
    }

    #[test]
    fn fold_isetp_lt() {
        let op = OpISetP {
            dst: Dst::None,
            set_op: PredSetOp::Or,
            cmp_op: IntCmpOp::Lt,
            cmp_type: IntCmpType::U32,
            ex: false,
            srcs: [
                imm(10),
                imm(20),
                Src::new_imm_bool(false),
                Src::new_imm_bool(false),
            ],
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::Pred(true));
    }

    #[test]
    fn fold_isetp_ge() {
        let op = OpISetP {
            dst: Dst::None,
            set_op: PredSetOp::Or,
            cmp_op: IntCmpOp::Ge,
            cmp_type: IntCmpType::I32,
            ex: false,
            srcs: [
                imm(100),
                imm(50),
                Src::new_imm_bool(false),
                Src::new_imm_bool(false),
            ],
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::Pred(true));
    }

    #[test]
    fn fold_isetp_false_const() {
        let op = OpISetP {
            dst: Dst::None,
            set_op: PredSetOp::Or,
            cmp_op: IntCmpOp::False,
            cmp_type: IntCmpType::U32,
            ex: false,
            srcs: [
                imm(1),
                imm(2),
                Src::new_imm_bool(false),
                Src::new_imm_bool(false),
            ],
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::Pred(false));
    }

    #[test]
    fn fold_isetp_true_const() {
        let op = OpISetP {
            dst: Dst::None,
            set_op: PredSetOp::And,
            cmp_op: IntCmpOp::True,
            cmp_type: IntCmpType::U32,
            ex: false,
            srcs: [
                imm(1),
                imm(2),
                Src::new_imm_bool(true),
                Src::new_imm_bool(false),
            ],
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::Pred(true));
    }

    #[test]
    fn fold_imnmx_min_u32() {
        let op = OpIMnMx {
            dst: Dst::None,
            cmp_type: IntCmpType::U32,
            srcs: [imm(100), imm(50), Src::new_imm_bool(true)],
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::U32(50));
    }

    #[test]
    fn fold_imnmx_max_u32() {
        let op = OpIMnMx {
            dst: Dst::None,
            cmp_type: IntCmpType::U32,
            srcs: [imm(100), imm(50), Src::new_imm_bool(false)],
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::U32(100));
    }

    #[test]
    fn fold_imnmx_min_signed() {
        let op = OpIMnMx {
            dst: Dst::None,
            cmp_type: IntCmpType::I32,
            srcs: [imm(0xFFFF_FFFE), imm(1), Src::new_imm_bool(true)],
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::U32(0xFFFF_FFFE));
    }

    // -------------------------------------------------------------------------
    // Lea, Prmt, Flo
    // -------------------------------------------------------------------------

    #[test]
    fn fold_lea_basic() {
        let op = OpLea {
            dsts: [Dst::None, Dst::None],
            srcs: [imm(4), imm(8), imm(0)],
            shift: 2,
            dst_high: false,
            intermediate_mod: SrcMod::None,
        };
        let dsts = run_fold(&op, &[], 2);
        assert_eq!(dsts[0], FoldData::U32(24));
        assert_eq!(dsts[1], FoldData::Pred(false));
    }

    #[test]
    fn fold_prmt_identity() {
        let op = OpPrmt {
            dst: Dst::None,
            srcs: [imm(0xDEAD_BEEF), imm(0x1234_5678), imm(0x3210)],
            mode: PrmtMode::Index,
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::U32(0xDEAD_BEEF));
    }

    #[test]
    fn fold_flo_leading_zeros() {
        // return_shift_amount=false: returns 31 - leading_zeros (bit position of MSB)
        let op = OpFlo {
            dst: Dst::None,
            src: imm(0x0080_0000),
            signed: false,
            return_shift_amount: false,
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::U32(23));
    }

    #[test]
    fn fold_flo_return_shift_amount() {
        // return_shift_amount=true: returns leading_zeros (shift amount)
        let op = OpFlo {
            dst: Dst::None,
            src: imm(0x0080_0000),
            signed: false,
            return_shift_amount: true,
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::U32(8));
    }

    #[test]
    fn fold_shf_left_u32() {
        let op = OpShf {
            dst: Dst::None,
            srcs: [imm(1), imm(0), imm(4)],
            right: false,
            wrap: false,
            data_type: IntType::U32,
            dst_high: false,
        };
        let dsts = run_fold(&op, &[], 1);
        assert_eq!(dsts[0], FoldData::U32(16));
    }

    // -------------------------------------------------------------------------
    // FoldData and OpFoldData infrastructure
    // -------------------------------------------------------------------------

    #[test]
    fn fold_data_equality() {
        assert_eq!(FoldData::Pred(true), FoldData::Pred(true));
        assert_ne!(FoldData::Pred(true), FoldData::Pred(false));
        assert_eq!(FoldData::U32(42), FoldData::U32(42));
        assert_eq!(FoldData::Carry(true), FoldData::Carry(true));
        assert_eq!(FoldData::Vec2([1, 2]), FoldData::Vec2([1, 2]));
    }
}
