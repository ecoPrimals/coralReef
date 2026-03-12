// SPDX-License-Identifier: AGPL-3.0-only
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
