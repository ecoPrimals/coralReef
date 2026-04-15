// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2025-2026 ecoPrimals
// Derived from Collabora, Ltd. (2022)
//! SM70 ALU instruction encoders: misc ops and encoder helpers.

use super::int::{fold_lop_src, src_as_lop_imm};
use super::*;

impl SM70Encoder<'_> {
    pub(super) fn set_float_cmp_op(&mut self, range: Range<usize>, op: FloatCmpOp) {
        assert!(range.len() == 4);
        self.set_field(
            range,
            match op {
                FloatCmpOp::OrdLt => 0x01_u8,
                FloatCmpOp::OrdEq => 0x02_u8,
                FloatCmpOp::OrdLe => 0x03_u8,
                FloatCmpOp::OrdGt => 0x04_u8,
                FloatCmpOp::OrdNe => 0x05_u8,
                FloatCmpOp::OrdGe => 0x06_u8,
                FloatCmpOp::UnordLt => 0x09_u8,
                FloatCmpOp::UnordEq => 0x0a_u8,
                FloatCmpOp::UnordLe => 0x0b_u8,
                FloatCmpOp::UnordGt => 0x0c_u8,
                FloatCmpOp::UnordNe => 0x0d_u8,
                FloatCmpOp::UnordGe => 0x0e_u8,
                FloatCmpOp::IsNum => 0x07_u8,
                FloatCmpOp::IsNan => 0x08_u8,
            },
        );
    }

    pub(super) fn set_pred_set_op(&mut self, range: Range<usize>, op: PredSetOp) {
        assert!(range.len() == 2);
        self.set_field(
            range,
            match op {
                PredSetOp::And => 0_u8,
                PredSetOp::Or => 1_u8,
                PredSetOp::Xor => 2_u8,
            },
        );
    }

    pub(super) fn set_int_cmp_op(&mut self, range: Range<usize>, op: IntCmpOp) {
        assert!(range.len() == 3);
        self.set_field(
            range,
            match op {
                IntCmpOp::False => 0_u8,
                IntCmpOp::True => 7_u8,
                IntCmpOp::Eq => 2_u8,
                IntCmpOp::Ne => 5_u8,
                IntCmpOp::Lt => 1_u8,
                IntCmpOp::Le => 3_u8,
                IntCmpOp::Gt => 4_u8,
                IntCmpOp::Ge => 6_u8,
            },
        );
    }
}

impl SM70Op for OpTranscendental {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.encode_alu(0x108, Some(&self.dst), None, Some(&self.src), None);
        e.set_field(
            74..80,
            match self.op {
                TranscendentalOp::Cos => 0_u8,
                TranscendentalOp::Sin => 1_u8,
                TranscendentalOp::Exp2 => 2_u8,
                TranscendentalOp::Log2 => 3_u8,
                TranscendentalOp::Rcp => 4_u8,
                TranscendentalOp::Rsq => 5_u8,
                TranscendentalOp::Rcp64H => 6_u8,
                TranscendentalOp::Rsq64H => 7_u8,
                TranscendentalOp::Sqrt => 8_u8,
                TranscendentalOp::Tanh => 9_u8,
            },
        );
    }
}

impl SM70Op for OpMov {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.set_opcode(0xc82);
            e.set_udst(&self.dst);

            // umov is encoded like a non-uniform ALU op
            let src = ALUSrc::from_src(e, Some(&self.src), true);
            let form: u8 = match &src {
                ALUSrc::Reg(reg) => {
                    e.encode_alu_ureg(reg, false);
                    0x6 // form
                }
                ALUSrc::Imm32(imm) => {
                    e.encode_alu_imm(imm);
                    0x4 // form
                }
                _ => crate::codegen::ice!("Invalid umov src"),
            };
            e.set_field(9..12, form);
        } else {
            e.encode_alu(0x002, Some(&self.dst), None, Some(&self.src), None);
            e.set_field(72..76, self.quad_lanes);
        }
    }
}

impl SM70Op for OpPrmt {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        let [src0, src1, _] = &mut self.srcs;
        b.copy_alu_src_if_not_reg(src0, gpr, SrcType::ALU);
        b.copy_alu_src_if_not_reg(src1, gpr, SrcType::ALU);
        self.reduce_sel_imm();
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.encode_ualu(
                0x96,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(self.sel()),
                Some(&self.srcs[1]),
            );
        } else {
            e.encode_alu(
                0x16,
                Some(&self.dst),
                Some(&self.srcs[0]),
                Some(self.sel()),
                Some(&self.srcs[1]),
            );
        }

        e.set_field(
            72..75,
            match self.mode {
                PrmtMode::Index => 0_u8,
                PrmtMode::Forward4Extract => 1_u8,
                PrmtMode::Backward4Extract => 2_u8,
                PrmtMode::Replicate8 => 3_u8,
                PrmtMode::EdgeClampLeft => 4_u8,
                PrmtMode::EdgeClampRight => 5_u8,
                PrmtMode::Replicate16 => 6_u8,
            },
        );
    }
}

impl SM70Op for OpSel {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        if !self.is_uniform() {
            b.copy_src_if_upred(self.cond_mut());
        }
        let cond_val = self.cond().clone().bnot();
        let swapped = {
            let [_, src0, src1] = &mut self.srcs;
            swap_srcs_if_not_reg(src0, src1, gpr)
        };
        if swapped {
            *self.cond_mut() = cond_val;
        }
        b.copy_alu_src_if_not_reg(&mut self.srcs[1], gpr, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.encode_ualu(
                0x087,
                Some(&self.dst),
                Some(&self.srcs[1]),
                Some(&self.srcs[2]),
                None,
            );

            e.set_upred_src(87..90, 90, self.cond());
        } else {
            e.encode_alu(
                0x007,
                Some(&self.dst),
                Some(&self.srcs[1]),
                Some(&self.srcs[2]),
                None,
            );

            e.set_pred_src(87..90, 90, self.cond());
        }
    }
}

impl SM70Op for OpSgxt {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        b.copy_alu_src_if_not_reg(self.a_mut(), gpr, SrcType::ALU);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.encode_ualu(
                0x09a,
                Some(&self.dst),
                Some(self.a()),
                Some(self.bits()),
                None,
            );
        } else {
            e.encode_alu(
                0x01a,
                Some(&self.dst),
                Some(self.a()),
                Some(self.bits()),
                None,
            );
        }
        e.set_bit(73, self.signed);
        e.set_bit(75, false); // .W (wrap vs clamp)
    }
}

impl SM70Op for OpShfl {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        let gpr = op_gpr(self);
        b.copy_alu_src_if_not_reg(self.src_mut(), gpr, SrcType::GPR);
        b.copy_alu_src_if_not_reg_or_imm(self.lane_mut(), gpr, SrcType::ALU);
        b.copy_alu_src_if_not_reg_or_imm(self.c_mut(), gpr, SrcType::ALU);
        self.reduce_lane_c_imm();
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        assert!(self.lane().is_unmodified());
        assert!(self.c().is_unmodified());

        match &self.lane().reference {
            SrcRef::Zero | SrcRef::Reg(_) => match &self.c().reference {
                SrcRef::Zero | SrcRef::Reg(_) => {
                    e.set_opcode(0x389);
                    e.set_reg_src(32..40, self.lane());
                    e.set_reg_src(64..72, self.c());
                }
                SrcRef::Imm32(imm_c) => {
                    e.set_opcode(0x589);
                    e.set_reg_src(32..40, self.lane());
                    e.set_field(40..53, *imm_c);
                }
                _ => crate::codegen::ice!("Invalid instruction form"),
            },
            SrcRef::Imm32(imm_lane) => match &self.c().reference {
                SrcRef::Zero | SrcRef::Reg(_) => {
                    e.set_opcode(0x989);
                    e.set_field(53..58, *imm_lane);
                    e.set_reg_src(64..72, self.c());
                }
                SrcRef::Imm32(imm_c) => {
                    e.set_opcode(0xf89);
                    e.set_field(40..53, *imm_c);
                    e.set_field(53..58, *imm_lane);
                }
                _ => crate::codegen::ice!("Invalid instruction form"),
            },
            _ => crate::codegen::ice!("Invalid instruction form"),
        }

        e.set_dst(self.dst());
        e.set_pred_dst(81..84, self.in_bounds());
        e.set_reg_src(24..32, self.src());
        e.set_field(
            58..60,
            match self.op {
                ShflOp::Idx => 0_u8,
                ShflOp::Up => 1_u8,
                ShflOp::Down => 2_u8,
                ShflOp::Bfly => 3_u8,
            },
        );
    }
}

impl SM70Op for OpPLop3 {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        // Fold constants and modifiers if we can
        for lop in &mut self.ops {
            *lop = LogicOp3::new_lut(&|mut x, mut y, mut z| {
                fold_lop_src(&self.srcs[0], &mut x);
                fold_lop_src(&self.srcs[1], &mut y);
                fold_lop_src(&self.srcs[2], &mut z);
                lop.eval(x, y, z)
            });
        }
        for src in &mut self.srcs {
            src.modifier = SrcMod::None;
            if src_as_lop_imm(src).is_some() {
                src.reference = SrcRef::True;
            }
        }

        if !self.is_uniform() {
            // The warp form of plop3 allows a single uniform predicate in
            // src2. If we have a uniform predicate anywhere, try to move it
            // there.
            let [src0, src1, src2] = &mut self.srcs;
            if src_is_upred_reg(src0) && !src_is_upred_reg(src2) {
                std::mem::swap(src0, src2);
                for lop in &mut self.ops {
                    *lop = LogicOp3::new_lut(&|x, y, z| lop.eval(z, y, x));
                }
            }
            if src_is_upred_reg(src1) && !src_is_upred_reg(src2) {
                std::mem::swap(src1, src2);
                for lop in &mut self.ops {
                    *lop = LogicOp3::new_lut(&|x, y, z| lop.eval(x, z, y));
                }
            }
            b.copy_src_if_upred(src0);
            b.copy_src_if_upred(src1);
        }
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if self.is_uniform() {
            e.set_opcode(0x89c);

            e.set_upred_src(68..71, 71, &self.srcs[2]);
            e.set_upred_src(77..80, 80, &self.srcs[1]);
            e.set_upred_src(87..90, 90, &self.srcs[0]);
        } else {
            e.set_opcode(0x81c);

            if self.srcs[2]
                .reference
                .as_reg()
                .is_some_and(RegRef::is_uniform)
            {
                e.set_upred_src(68..71, 71, &self.srcs[2]);
                e.set_bit(67, true);
            } else {
                e.set_pred_src(68..71, 71, &self.srcs[2]);
            }
            e.set_pred_src(77..80, 80, &self.srcs[1]);
            e.set_pred_src(87..90, 90, &self.srcs[0]);
        }
        e.set_field(16..24, self.ops[1].lut);
        e.set_field(64..67, self.ops[0].lut & 0x7);
        e.set_field(72..77, self.ops[0].lut >> 3);

        e.set_pred_dst(81..84, &self.dsts[0]);
        e.set_pred_dst(84..87, &self.dsts[1]);
    }
}

impl SM70Op for OpR2UR {
    fn legalize(&mut self, _b: &mut LegalizeBuilder) {
        // Nothing to do
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        if e.sm >= 100 {
            e.set_opcode(0x2ca);
        } else {
            e.set_opcode(0x3c2);
        }
        e.set_udst(&self.dst);
        e.set_reg_src(24..32, &self.src);
        e.set_pred_dst(81..84, &Dst::None);
    }
}

impl SM70Op for OpRedux {
    fn legalize(&mut self, b: &mut LegalizeBuilder) {
        b.copy_alu_src_if_not_reg(&mut self.src, RegFile::GPR, SrcType::GPR);
    }

    fn encode(&self, e: &mut SM70Encoder<'_>) {
        e.set_opcode(0x3c4);
        e.set_udst(&self.dst);
        e.set_reg_src(24..32, &self.src);

        e.set_bit(
            73,
            match self.op {
                ReduxOp::Min(cmp_type) | ReduxOp::Max(cmp_type) => cmp_type == IntCmpType::I32,
                _ => false,
            },
        );
        e.set_field(
            78..81,
            match self.op {
                ReduxOp::And => 0_u8,
                ReduxOp::Or => 1,
                ReduxOp::Xor => 2,
                ReduxOp::Sum => 3,
                ReduxOp::Min(_) => 4,
                ReduxOp::Max(_) => 5,
            },
        );
    }
}

#[cfg(test)]
#[path = "misc_tests.rs"]
mod tests;
